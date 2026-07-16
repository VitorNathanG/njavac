//! njavac's differential fuzzer (ROADMAP §0.1).
//!
//! Generates random **in-scope** Java (`main` bodies over the supported numeric +
//! branch + short-circuit subset), compiles each program with BOTH the pinned
//! `javac` and njavac (in-process), and byte-compares. On a mismatch it
//! auto-minimizes to a `fixtures/`-ready `.java` and localizes the divergence with
//! the same `classdump::diff_report` the bench uses. Seed-reproducible
//! (`fuzz <seed>`). Dependency-free (`std` only).
//!
//! ## Why this is sound (no false positives)
//!
//! The ONLY hard-fail signal is *both compilers accept a program (each emits a
//! `.class`) and the bytes differ* — which is, by definition, an njavac bug, since
//! byte-identity-to-javac IS the spec. Everything else is skip/telemetry:
//!
//! | outcome                          | meaning                       | action              |
//! | -------------------------------- | ----------------------------- | ------------------- |
//! | both accept, **bytes differ**    | njavac bug                    | FINDING → minimize  |
//! | both accept, bytes equal         | correct                       | pass                |
//! | javac rejects (no `.class`)      | generator emitted bad Java    | `generator-invalid` |
//! | njavac panics, javac accepted    | valid Java njavac can't do    | `njavac-reject`     |
//!
//! Generator over-reach can never cause a false finding: if njavac *accepts*
//! out-of-scope code and bytes differ, that's a real bug; if it *rejects*, it's
//! telemetry. So the generator's in-subset discipline is a **yield** lever, not a
//! soundness lever. Three harness invariants make the equivalence airtight — the
//! `ident()` naming chokepoint, the class-set guards, and generating all IR before
//! compiler interaction.
//!
//! ## Performance
//!
//! njavac runs in-process. The reference compiler runs in a persistent in-memory
//! worker (`tools/FuzzJavac.java`, driven by `JavacWorker`): one hot JVM for the
//! entire run, with sources and class bytes transferred over a framed pipe. The
//! worker's bytes are checked against the CLI by `--verify-worker`; the CLI remains
//! the ground truth.

mod finding;
mod generate;
mod javac;
mod minimize;
mod model;
mod oracle;
mod render;
mod run;
mod verify;

use std::path::PathBuf;

use generate::{Gen, Rng};
use render::render;

pub(crate) struct Config {
    pub(crate) seed: u64,
    /// Whether the seed was pinned on the command line (positional or `--seed`).
    pub(crate) seed_set: bool,
    pub(crate) count: u64,
    pub(crate) batch: u64,
    pub(crate) javac: String,
    pub(crate) out_dir: PathBuf,
    pub(crate) keep_going: bool,
    pub(crate) no_min: bool,
    pub(crate) dump_sources: bool,
    pub(crate) selftest: bool,
    pub(crate) verify_worker: bool,
}

impl Config {
    fn from_args() -> Config {
        let home = std::env::var("HOME").unwrap_or_default();
        let default_javac = format!("{home}/.sdkman/candidates/java/25.0.2-graalce/bin/javac");
        let mut cfg = Config {
            seed: 0,
            seed_set: false,
            count: 5000,
            batch: 1000,
            javac: std::env::var("JAVAC").unwrap_or(default_javac),
            out_dir: PathBuf::from("fuzz-out"),
            keep_going: false,
            no_min: false,
            dump_sources: false,
            selftest: false,
            verify_worker: false,
        };
        let mut positional = 0;
        let mut args = std::env::args().skip(1);
        while let Some(a) = args.next() {
            match a.as_str() {
                "--seed" => {
                    cfg.seed = args.next().and_then(|v| v.parse().ok()).expect("--seed needs a u64");
                    cfg.seed_set = true;
                }
                "--count" => cfg.count = args.next().and_then(|v| v.parse().ok()).unwrap_or(cfg.count),
                "--batch" => cfg.batch = args.next().and_then(|v| v.parse().ok()).unwrap_or(cfg.batch),
                "--out-dir" => cfg.out_dir = PathBuf::from(args.next().expect("--out-dir needs a path")),
                "--javac" => cfg.javac = args.next().expect("--javac needs a path"),
                "--jobs" => {
                    let j: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                    assert_eq!(j, 1, "--jobs > 1 is not implemented in v1 (single-threaded batched)");
                }
                "--keep-going" => cfg.keep_going = true,
                "--no-min" => cfg.no_min = true,
                "--dump-sources" => cfg.dump_sources = true,
                "--selftest" => cfg.selftest = true,
                "--verify-worker" => cfg.verify_worker = true,
                "-h" | "--help" => {
                    println!(
                        "usage: fuzz [<seed>] [<count>] [--seed N] [--count N] [--batch N] \
                         [--keep-going] [--no-min] [--out-dir DIR] [--jobs 1] [--dump-sources] \
                         [--selftest] [--verify-worker] [--javac PATH]\n\
                         \n  <seed> / --seed  pin the seed; OMIT for a fresh random seed each run\
                         \n                   (printed so a finding reproduces with `make fuzz SEED=<n>`)\
                         \n  --keep-going     don't stop at the first finding; enumerate distinct ones\
                         \n  --no-min         skip minimization (fast enumeration; emits raw repros)\
                         \n  --verify-worker  prove the in-memory javac worker == the javac CLI byte-for-byte\
                         \n                   over <count> generated programs (run after any JDK bump)"
                    );
                    std::process::exit(0);
                }
                s if s.starts_with('-') => {
                    eprintln!("fuzz: unknown flag {s}");
                    std::process::exit(2);
                }
                s => {
                    let v: u64 = s.parse().unwrap_or_else(|_| {
                        eprintln!("fuzz: bad positional {s}");
                        std::process::exit(2)
                    });
                    if positional == 0 {
                        cfg.seed = v;
                        cfg.seed_set = true;
                    } else if positional == 1 {
                        cfg.count = v;
                    }
                    positional += 1;
                }
            }
        }
        cfg
    }
}

fn main() {
    // Speak in one voice: out-of-scope inputs panic by design and are caught.
    std::panic::set_hook(Box::new(|_| {}));
    let mut cfg = Config::from_args();

    if cfg.dump_sources {
        dump_sources(&cfg);
        return;
    }
    if cfg.selftest {
        std::process::exit(minimize::selftest(&cfg));
    }

    // Randomize only after deterministic dump/selftest modes have taken the fixed
    // default, and before worker verification exactly as in the monolith.
    if !cfg.seed_set {
        cfg.seed = run::random_seed();
    }

    if cfg.verify_worker {
        std::process::exit(verify::verify_worker(&cfg));
    }

    run::run(&cfg);
}

/// Print each generated source with no compiler interaction.
fn dump_sources(cfg: &Config) {
    let mut g = Gen { rng: Rng::new(cfg.seed) };
    for k in 0..cfg.count {
        let prog = g.gen_prog(k);
        println!("// ===== {} =====", prog.name.class);
        print!("{}", render(&prog));
    }
}
