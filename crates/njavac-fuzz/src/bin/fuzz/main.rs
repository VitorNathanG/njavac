//! njavac's differential fuzzer; see `docs/src/tooling/fuzzing.md`.
//!
//! Generates random **in-scope** Java (`main` bodies over the supported numeric +
//! branch + short-circuit subset), compiles each program with BOTH the pinned
//! `javac` and njavac (in-process), and byte-compares. Byte divergences pass through
//! a second layer that executes both classes and compares their observable output
//! and termination. Seed-reproducible (`fuzz <seed>`); generation and its PRNG use
//! only `std`.
//!
//! ## Two-layer oracle
//!
//! Exact bytes remain the first and cheapest comparison. When they differ, a
//! persistent JVM observer loads each class in a fresh class loader, invokes
//! `main`, and compares stdout, stderr, and normalized termination. For accepted
//! classes, only an observation difference is a hard finding; byte-only differences
//! are telemetry. Invalid njavac rejections and internal panics are also findings.
//!
//! | outcome                          | meaning                       | action              |
//! | -------------------------------- | ----------------------------- | ------------------- |
//! | both accept, bytes and observation differ | behavioral bug         | FINDING             |
//! | both accept, bytes differ, observation equal | compatibility drift   | telemetry           |
//! | both accept, bytes equal         | exact                         | pass                |
//! | javac rejects (no `.class`)      | generator emitted bad Java    | `generator-invalid` |
//! | javac accepts, njavac returns Unsupported | valid but out of subset | `njavac-unsupported` telemetry |
//! | javac accepts, njavac returns SyntaxError | invalid njavac rejection | compiler finding    |
//! | javac accepts, njavac panics     | njavac invariant failure      | compiler finding    |
//!
//! The observation layer deliberately provides empirical semantic confidence, not
//! proof: unobserved state can hide a wrong compilation. The generator therefore
//! prints every mutation and branch choice to maximize the visible execution trace.
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
mod observe;
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
    pub(crate) dump_sources: bool,
    pub(crate) selftest: bool,
    pub(crate) verify_worker: bool,
    pub(crate) verify_observer: bool,
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
            dump_sources: false,
            selftest: false,
            verify_worker: false,
            verify_observer: false,
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
                "--dump-sources" => cfg.dump_sources = true,
                "--selftest" => cfg.selftest = true,
                "--verify-worker" => cfg.verify_worker = true,
                "--verify-observer" => cfg.verify_observer = true,
                "-h" | "--help" => {
                    println!(
                        "usage: fuzz [<seed>] [<count>] [--seed N] [--count N] [--batch N] \
                         [--keep-going] [--out-dir DIR] [--jobs 1] [--dump-sources] \
                         [--selftest] [--verify-worker] [--verify-observer] [--javac PATH]\n\
                         \n  <seed> / --seed  pin the seed; OMIT for a fresh random seed each run\
                         \n                   (printed so a finding reproduces with `make fuzz SEED=<n>`)\
                         \n  --keep-going     don't stop at the first finding; enumerate distinct ones\
                          \n  --verify-worker  prove the in-memory javac worker == the javac CLI byte-for-byte\
                          \n                   over <count> generated programs (run after any JDK bump)\
                          \n  --verify-observer exercise return, throw, invalid-class, difference, and timeout paths"
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
    // Candidate panics are captured and reported with their payload as compiler
    // findings; suppress the hook's duplicate panic report.
    std::panic::set_hook(Box::new(|_| {}));
    let mut cfg = Config::from_args();

    if cfg.dump_sources {
        dump_sources(&cfg);
        return;
    }
    if cfg.selftest {
        std::process::exit(minimize::selftest(&cfg));
    }
    if cfg.verify_observer {
        std::process::exit(verify::verify_observer(&cfg));
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
