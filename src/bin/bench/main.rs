//! njavac's single test + benchmark harness.
//!
//! It does two things over the `fixtures/` corpus:
//!
//!   1. CORRECTNESS (always): compile every `fixtures/*.java` with both javac and
//!      njavac and assert the `.class` bytes are identical. Byte-identity is
//!      reproducible only against the pinned javac in the Docker harness. It fails
//!      loudly, with a localized structural and javap diff of the first mismatch,
//!      so it doubles as the exact-byte fixture gate.
//!
//!   2. TIMING (controlled Docker harness only): time compiling the whole suite with
//!      each compiler. Host timings are noise (JVM startup jitter, scheduler,
//!      thermal), so timings are only produced inside the Docker harness.
//!
//! Dependency-free on purpose. Configure via flags (see `--help`).

mod correctness;
mod timing;

use std::path::{Path, PathBuf};
use std::process::Command;

use correctness::{correctness, record_goldens};
use timing::timing;

struct Config {
    javac_runs: usize,
    njavac_runs: usize,
    warmup: usize,
    javac: String,
    javap: String,
    njavac: String,
    fixtures_dir: String,
    out_dir: String,
    /// A single `.java` file to verify instead of the whole corpus.
    /// `Some` ⇒ correctness over just this file, no timing.
    single: Option<PathBuf>,
    /// Write current javac outputs to the golden cache and exit.
    record: bool,
    /// Byte-compare njavac against the golden cache instead of live javac —
    /// a javac-free inner loop.
    offline: bool,
    /// The git-ignored golden cache dir (`--record` writes it, `--offline` reads
    /// it). Under `target/`, so it is never committed and `cargo clean` drops it.
    golden_dir: String,
    /// Skip the timing pass — run the correctness check only. A fresh, authoritative
    /// online exact-byte fixture gate without the ~12s timing measurement.
    no_timing: bool,
}

impl Config {
    fn from_args() -> Config {
        let home = std::env::var("HOME").unwrap_or_default();
        let default_javac =
            format!("{home}/.sdkman/candidates/java/25.0.2-graalce/bin/javac");
        let mut c = Config {
            // njavac is fast enough that many runs are cheap and tighten the
            // estimate; javac pays ~700 ms of JVM startup per run, so keep it low.
            javac_runs: 5,
            njavac_runs: 1000,
            warmup: 5,
            javac: default_javac,
            javap: String::new(), // filled in below, derived from javac
            njavac: "target/release/njavac".into(),
            fixtures_dir: "fixtures".into(),
            out_dir: "target/bench-out".into(),
            single: None,
            record: false,
            offline: false,
            golden_dir: "target/bench-golden".into(),
            no_timing: false,
        };
        if let Ok(v) = std::env::var("JAVAC") {
            c.javac = v;
        }
        c.javap = std::env::var("JAVAP")
            .unwrap_or_else(|_| c.javac.strip_suffix("javac").map_or_else(
                || "javap".into(),
                |base| format!("{base}javap"),
            ));

        let mut args = std::env::args().skip(1);
        while let Some(a) = args.next() {
            let mut val = || args.next().expect("flag needs a value");
            match a.as_str() {
                "--javac-runs" => c.javac_runs = val().parse().expect("javac-runs must be a number"),
                "--njavac-runs" => c.njavac_runs = val().parse().expect("njavac-runs must be a number"),
                "--warmup" => c.warmup = val().parse().expect("warmup must be a number"),
                "--javac" => c.javac = val(),
                "--javap" => c.javap = val(),
                "--njavac" => c.njavac = val(),
                "--fixtures" => c.fixtures_dir = val(),
                "--out-dir" => c.out_dir = val(),
                "--golden-dir" => c.golden_dir = val(),
                "--record" => c.record = true,
                "--offline" => c.offline = true,
                "--no-timing" => c.no_timing = true,
                "--help" | "-h" => {
                    println!(
                        "usage: bench [<File.java> | --fixtures DIR] [--record] [--offline] \
                         [--no-timing] [--golden-dir DIR] [--javac-runs N] [--njavac-runs N] \
                         [--warmup N] [--javac PATH] [--javap PATH] [--njavac PATH] [--out-dir DIR]\n\
                         \n  <File.java>   verify just this one fixture (no timing)\
                         \n  --record      compile all fixtures with javac into the golden cache, then exit\
                         \n  --offline     byte-compare njavac against the golden cache (no javac needed)\
                         \n  --no-timing   run the correctness check only, skip the timing pass"
                    );
                    std::process::exit(0);
                }
                // A bare positional is a single `.java` file to verify; anything
                // else beginning with '-' is an unknown flag.
                path if !path.starts_with('-') && path.ends_with(".java") => {
                    c.single = Some(PathBuf::from(path));
                }
                other => {
                    eprintln!("unknown argument: {other}");
                    std::process::exit(2);
                }
            }
        }
        c
    }
}

/// Every `*.java` fixture under `dir`, **recursively** (fixtures are grouped into
/// topical subfolders), sorted by path for stable output.
fn discover_fixtures(dir: &str) -> Vec<PathBuf> {
    let mut v = Vec::new();
    collect_java(Path::new(dir), &mut v);
    v.sort();
    assert!(!v.is_empty(), "no .java fixtures under {dir}");
    v
}

/// Recurse into `dir`, appending every `*.java` file to `out`.
fn collect_java(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read fixtures dir {}: {e}", dir.display()));
    for p in entries.filter_map(|e| e.ok().map(|e| e.path())) {
        if p.is_dir() {
            collect_java(&p, out);
        } else if p.extension().is_some_and(|x| x == "java") {
            out.push(p);
        }
    }
}

/// Run a command to completion, discarding output. Returns whether it succeeded
/// (a compiler panic/error becomes `false` rather than aborting the harness).
fn run_quiet(argv: &[String]) -> bool {
    Command::new(&argv[0])
        .args(&argv[1..])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn main() {
    let cfg = Config::from_args();

    // A single `.java` file verifies just that fixture; otherwise the whole
    // corpus (discovered recursively under --fixtures).
    let fixtures = match &cfg.single {
        Some(f) => {
            if !f.is_file() {
                eprintln!("bench: no such fixture file: {}", f.display());
                std::process::exit(2);
            }
            vec![f.clone()]
        }
        None => discover_fixtures(&cfg.fixtures_dir),
    };

    if cfg.record {
        record_goldens(&cfg, &fixtures, Path::new(&cfg.golden_dir));
        return;
    }

    let javac_dir = PathBuf::from(&cfg.out_dir).join("javac");
    let njavac_dir = PathBuf::from(&cfg.out_dir).join("njavac");
    std::fs::create_dir_all(&javac_dir).expect("create javac out dir");
    std::fs::create_dir_all(&njavac_dir).expect("create njavac out dir");

    correctness(&cfg, &fixtures, &javac_dir, &njavac_dir);

    // Timing is a whole-suite, live-javac measurement: skip it for a single
    // fixture (meaningless), in --offline mode (no javac to time against), and
    // when --no-timing asks for a correctness-only run.
    if cfg.single.is_none() && !cfg.offline && !cfg.no_timing {
        timing(&cfg, &fixtures, &javac_dir, &njavac_dir);
    }
}
