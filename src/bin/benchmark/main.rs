//! njavac's correctness and performance benchmark harness.
//!
//! It does two things over the `fixtures/` corpus:
//!
//!   1. CORRECTNESS (always): compile every `fixtures/*.java` with both javac and
//!      njavac and assert the `.class` bytes are identical. Byte-identity is
//!      reproducible only against the pinned javac in the Docker harness. It fails
//!      loudly, with a localized structural and javap diff of the first mismatch,
//!      so it doubles as the exact-byte fixture gate.
//!
//!   2. PERFORMANCE (controlled Docker harness only): collect uninstrumented
//!      process and hot-pipeline samples followed by isolated phase and allocation
//!      passes.
//!
//! Dependency-free on purpose. Configure via flags (see `--help`).

mod correctness;
mod measurement;
mod report;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use correctness::{correctness, record_goldens};
struct Config {
    samples: usize,
    warmup: usize,
    rounds: usize,
    allocation_rounds: usize,
    javac: String,
    javap: String,
    njavac: String,
    alloc_helper: String,
    fixtures_dir: String,
    out_dir: String,
    json_path: Option<PathBuf>,
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
    /// Skip performance measurement and run only the correctness check.
    no_performance: bool,
}

impl Config {
    fn from_args() -> Config {
        let home = std::env::var("HOME").unwrap_or_default();
        let default_javac =
            format!("{home}/.sdkman/candidates/java/25.0.2-graalce/bin/javac");
        let mut c = Config {
            samples: 5,
            warmup: 2,
            rounds: 100,
            allocation_rounds: 1,
            javac: default_javac,
            javap: String::new(), // filled in below, derived from javac
            njavac: "target/release/njavac".into(),
            alloc_helper: "target/release/benchmark_alloc".into(),
            fixtures_dir: "fixtures".into(),
            out_dir: "/tmp/njavac-benchmark".into(),
            json_path: None,
            single: None,
            record: false,
            offline: false,
            golden_dir: "target/benchmark-golden".into(),
            no_performance: false,
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
                "--samples" => c.samples = positive(&val(), "samples"),
                "--warmup" => c.warmup = val().parse().expect("warmup must be a nonnegative number"),
                "--rounds" => c.rounds = positive(&val(), "rounds"),
                "--allocation-rounds" => {
                    c.allocation_rounds = positive(&val(), "allocation-rounds")
                }
                "--javac" => c.javac = val(),
                "--javap" => c.javap = val(),
                "--njavac" => c.njavac = val(),
                "--alloc-helper" => c.alloc_helper = val(),
                "--fixtures" => c.fixtures_dir = val(),
                "--out-dir" => c.out_dir = val(),
                "--json" => c.json_path = Some(PathBuf::from(val())),
                "--golden-dir" => c.golden_dir = val(),
                "--record" => c.record = true,
                "--offline" => c.offline = true,
                "--no-performance" => c.no_performance = true,
                "--help" | "-h" => {
                    println!(
                        "usage: benchmark [<File.java> | --fixtures DIR] [--record] [--offline] \
                         [--no-performance] [--golden-dir DIR] [--samples N] [--warmup N] \
                         [--rounds N] [--allocation-rounds N] [--javac PATH] [--javap PATH] \
                         [--njavac PATH] [--alloc-helper PATH] [--out-dir DIR] [--json PATH]\n\
                         \n  <File.java>   verify just this one fixture (no timing)\
                         \n  --record      compile all fixtures with javac into the golden cache, then exit\
                         \n  --offline     byte-compare njavac against the golden cache (no javac needed)\
                         \n  --no-performance  run the correctness check only"
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

fn positive(value: &str, name: &str) -> usize {
    value
        .parse()
        .ok()
        .filter(|&value| value > 0)
        .unwrap_or_else(|| panic!("{name} must be a positive integer"))
}

fn main() {
    let raw_args: Vec<OsString> = std::env::args_os().collect();
    if raw_args.get(1).is_some_and(|arg| arg == "--resource-child") {
        measurement::resource_child(raw_args.into_iter().skip(2).collect());
    }

    let cfg = Config::from_args();

    // A single `.java` file verifies just that fixture; otherwise the whole
    // corpus (discovered recursively under --fixtures).
    let fixtures = match &cfg.single {
        Some(f) => {
            if !f.is_file() {
                eprintln!("benchmark: no such fixture file: {}", f.display());
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

    if cfg.single.is_none() && !cfg.offline && !cfg.no_performance {
        let in_harness = std::env::var_os("NJAVAC_IN_CONTAINER").is_some();
        if !in_harness && std::env::var_os("NJAVAC_BENCHMARK_ALLOW_HOST").is_none() {
            println!("performance skipped: run `make benchmark` for controlled same-host evidence");
            return;
        }
        let workload = measurement::load_workload(&fixtures, &njavac_dir).unwrap_or_else(|error| {
            eprintln!("benchmark: {error}");
            std::process::exit(1);
        });
        let measurements = measurement::run(&cfg, &workload, &javac_dir, &njavac_dir)
            .unwrap_or_else(|error| {
                eprintln!("benchmark: {error}");
                std::process::exit(1);
            });
        report::print_and_write(&cfg, &workload, &measurements).unwrap_or_else(|error| {
            eprintln!("benchmark: {error}");
            std::process::exit(1);
        });
    }
}
