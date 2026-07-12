//! njavac's single test + benchmark harness.
//!
//! It does two things over the `fixtures/` corpus:
//!
//!   1. CORRECTNESS (always): compile every `fixtures/*.java` with both javac and
//!      njavac and assert the `.class` bytes are identical. Byte-identity is
//!      deterministic, so this runs anywhere; it fails loudly, with a javap-level
//!      diff of the first mismatch, so it doubles as the acceptance test.
//!
//!   2. TIMING (deterministic harness only): time compiling the whole suite with
//!      each compiler. Host timings are noise (JVM startup jitter, scheduler,
//!      thermal), so timings are only produced inside the Docker harness — run
//!      `make bench`. Correctness still runs on the host.
//!
//! Dependency-free on purpose. Configure via flags (see `--help`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

struct Config {
    javac_runs: usize,
    njavac_runs: usize,
    warmup: usize,
    javac: String,
    javap: String,
    njavac: String,
    fixtures_dir: String,
    out_dir: String,
    /// A single `.java` file to verify instead of the whole corpus (ROADMAP §0.2).
    /// `Some` ⇒ correctness over just this file, no timing.
    single: Option<PathBuf>,
    /// Write current javac outputs to the golden cache and exit (ROADMAP §0.5).
    record: bool,
    /// Byte-compare njavac against the golden cache instead of live javac —
    /// a javac-free inner loop (ROADMAP §0.5).
    offline: bool,
    /// The git-ignored golden cache dir (`--record` writes it, `--offline` reads
    /// it). Under `target/`, so it is never committed and `cargo clean` drops it.
    golden_dir: String,
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
                "--help" | "-h" => {
                    println!(
                        "usage: bench [<File.java> | --fixtures DIR] [--record] [--offline] \
                         [--golden-dir DIR] [--javac-runs N] [--njavac-runs N] [--warmup N] \
                         [--javac PATH] [--javap PATH] [--njavac PATH] [--out-dir DIR]\n\
                         \n  <File.java>   verify just this one fixture (no timing)\
                         \n  --record      compile all fixtures with javac into the golden cache, then exit\
                         \n  --offline     byte-compare njavac against the golden cache (no javac needed)"
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

// -------------------- correctness --------------------

fn base_name(fix: &Path) -> String {
    fix.file_stem().unwrap().to_string_lossy().into_owned()
}

/// The reference `.class` for `base`: the golden cache in `--offline` mode, else
/// the freshly-compiled javac output.
fn want_path(cfg: &Config, javac_dir: &Path, base: &str) -> PathBuf {
    let dir = if cfg.offline { Path::new(&cfg.golden_dir) } else { javac_dir };
    dir.join(format!("{base}.class"))
}

/// Compile every fixture with njavac and byte-compare against the reference
/// (live javac, or the golden cache in `--offline` mode). Exits the process
/// (non-zero) on the first mismatch after showing a localized diff.
fn correctness(cfg: &Config, fixtures: &[PathBuf], javac_dir: &Path, njavac_dir: &Path) {
    let source = if cfg.offline { "golden cache" } else { "live javac" };
    println!("correctness ({} fixtures, vs {source}):", fixtures.len());
    let mut failures: Vec<String> = Vec::new();

    for fix in fixtures {
        let base = base_name(fix);
        let fix_s = fix.to_string_lossy().into_owned();
        let njavac_out = njavac_dir.join(format!("{base}.class"));

        // Delete njavac's output first, so a compiler that panics/errors and
        // writes nothing yields a *missing* file (a FAIL) rather than a false
        // pass off a stale artifact left by an earlier run. In online mode do the
        // same for javac and recompile; offline reads the pre-recorded cache.
        let _ = std::fs::remove_file(&njavac_out);
        if !cfg.offline {
            let javac_out = javac_dir.join(format!("{base}.class"));
            let _ = std::fs::remove_file(&javac_out);
            run_quiet(&[cfg.javac.clone(), "-d".into(), javac_dir.display().to_string(), fix_s.clone()]);
        }
        run_quiet(&[cfg.njavac.clone(), "-d".into(), njavac_dir.display().to_string(), fix_s]);

        let want = std::fs::read(want_path(cfg, javac_dir, &base));
        let got = std::fs::read(&njavac_out);
        match (want, got) {
            (Ok(a), Ok(b)) if a == b => println!("  PASS  {base}  ({} bytes)", a.len()),
            _ => {
                println!("  FAIL  {base}");
                failures.push(base);
            }
        }
    }

    if failures.is_empty() {
        println!("  -> all {} byte-identical\n", fixtures.len());
        return;
    }

    // Localize the first failure: the structural classdiff (byte-offset precise,
    // works even when javap agrees) followed by the noise-stripped javap diff.
    let base = &failures[0];
    println!("\n{}/{} failed. First mismatch: {base}", failures.len(), fixtures.len());
    if cfg.offline && fixtures.len() > 1 && failures.len() == fixtures.len() {
        println!("(every fixture failed in --offline mode — is the golden cache stale or empty? \
                  run `bench --record` to (re)build {})", cfg.golden_dir);
    }

    let want_file = want_path(cfg, javac_dir, base);
    let njavac_file = njavac_dir.join(format!("{base}.class"));
    match (std::fs::read(&want_file), std::fs::read(&njavac_file)) {
        (Ok(a), Ok(b)) => {
            match njavac::classdump::diff_report(&a, &b) {
                Some(report) => {
                    println!("\nstructural divergence (classdiff):");
                    for line in report.lines() {
                        println!("  {line}");
                    }
                }
                None => println!("(files are byte-identical — nothing to localize)"),
            }
            println!("\njavap divergence:");
            print_first_divergence(&javap_lines(cfg, &want_file), &javap_lines(cfg, &njavac_file));
        }
        _ => println!(
            "(one output is missing: {} / {} — cannot diff)",
            want_file.display(),
            njavac_file.display()
        ),
    }
    std::process::exit(1);
}

/// Compile every fixture with javac into the golden cache and exit (ROADMAP §0.5).
/// The cache is a convenience mirror, always re-recorded from javac — never
/// committed, never hand-edited. Intended to be run *inside the Docker image* so
/// the goldens come from the pinned `javac`, then persisted to a volume.
fn record_goldens(cfg: &Config, fixtures: &[PathBuf], golden_dir: &Path) {
    std::fs::create_dir_all(golden_dir).expect("create golden dir");
    // Clear the goldens we're about to regenerate, so a renamed/removed fixture
    // can't leave a stale orphan in the cache.
    for fix in fixtures {
        let _ = std::fs::remove_file(golden_dir.join(format!("{}.class", base_name(fix))));
    }
    // One javac invocation over the whole suite. The fixtures are independent
    // single-class files, so batch output is byte-identical to per-file javac —
    // but it pays ONE JVM startup instead of one per fixture, which is the point
    // of a persisted cache: recording stays cheap.
    println!(
        "recording {} goldens into {} (pinned javac, one invocation):",
        fixtures.len(),
        golden_dir.display()
    );
    let mut argv = vec![cfg.javac.clone(), "-d".into(), golden_dir.display().to_string()];
    argv.extend(fixtures.iter().map(|p| p.to_string_lossy().into_owned()));
    if !run_quiet(&argv) {
        eprintln!("  WARN  javac exited non-zero while recording");
    }
    let ok = fixtures
        .iter()
        .filter(|f| golden_dir.join(format!("{}.class", base_name(f))).exists())
        .count();
    println!("  -> recorded {ok}/{} goldens", fixtures.len());
}

/// `javap -v -p` output as lines, with the header lines that legitimately differ
/// (file path, mtime, checksum) stripped.
fn javap_lines(cfg: &Config, class: &Path) -> Vec<String> {
    let out = Command::new(&cfg.javap)
        .args(["-v", "-p"])
        .arg(class)
        .output();
    let text = match out {
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
        Err(_) => return vec![format!("<could not run javap on {}>", class.display())],
    };
    text.lines()
        .filter(|l| {
            !l.starts_with("Classfile ")
                && !l.starts_with("  Last modified")
                && !l.starts_with("  SHA-256")
        })
        .map(str::to_string)
        .collect()
}

/// Print the first line where the two javap dumps diverge, with context.
fn print_first_divergence(a: &[String], b: &[String]) {
    let n = a.len().max(b.len());
    let first = (0..n).find(|&i| a.get(i) != b.get(i));
    let Some(i) = first else {
        println!("(bytes differ but javap output matches — likely a trailing/attribute byte)");
        return;
    };
    let lo = i.saturating_sub(4);
    println!("first divergence at javap line {} ('<' javac, '>' njavac):", i + 1);
    for j in lo..=i {
        if let Some(l) = a.get(j) {
            println!("  {} < {l}", if j == i { "*" } else { " " });
        }
    }
    for j in lo..=i {
        if let Some(l) = b.get(j) {
            println!("  {} > {l}", if j == i { "*" } else { " " });
        }
    }
}

// -------------------- timing --------------------

struct Stats {
    min: f64,
    median: f64,
    mean: f64,
    stddev: f64,
}

fn stats(mut xs: Vec<f64>) -> Stats {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = xs.len();
    let mean = xs.iter().sum::<f64>() / n as f64;
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
    let median = if n % 2 == 1 {
        xs[n / 2]
    } else {
        (xs[n / 2 - 1] + xs[n / 2]) / 2.0
    };
    Stats { min: xs[0], median, mean, stddev: var.sqrt() }
}

/// Time a closure (which may spawn several processes) in milliseconds.
fn time_block<F: Fn()>(f: F) -> f64 {
    let t0 = Instant::now();
    f();
    t0.elapsed().as_secs_f64() * 1000.0
}

fn bench_block<F: Fn()>(name: &str, cfg: &Config, runs: usize, work: F) -> Stats {
    for _ in 0..cfg.warmup {
        work();
    }
    let times = (0..runs).map(|_| time_block(&work)).collect();
    let s = stats(times);
    println!(
        "{name:12}  min {:8.3}  median {:8.3}  mean {:8.3}  stddev {:7.3}  (ms, n={runs})",
        s.min, s.median, s.mean, s.stddev
    );
    s
}

/// Time compiling the entire suite with each compiler. Both get a *single*
/// invocation over all fixtures — the way each is actually used, and now that
/// njavac accepts many sources like javac does, an apples-to-apples wall-clock
/// of one javac process against one njavac process (each: process startup +
/// compiling every file).
fn timing(cfg: &Config, fixtures: &[PathBuf], javac_dir: &Path, njavac_dir: &Path) {
    let in_harness = std::env::var_os("NJAVAC_IN_CONTAINER").is_some()
        || Path::new("/.dockerenv").exists();
    if !in_harness && std::env::var_os("NJAVAC_BENCH_ALLOW_HOST").is_none() {
        println!("timing skipped: run `make bench` for deterministic numbers");
        println!("(host timings are noise; set NJAVAC_BENCH_ALLOW_HOST=1 to force anyway)");
        return;
    }

    let fix_paths: Vec<String> = fixtures.iter().map(|p| p.to_string_lossy().into_owned()).collect();

    let javac = cfg.javac.clone();
    let javac_out = javac_dir.display().to_string();
    let javac_all = || {
        let mut argv = vec![javac.clone(), "-d".into(), javac_out.clone()];
        argv.extend(fix_paths.iter().cloned());
        run_quiet(&argv);
    };

    let njavac = cfg.njavac.clone();
    let njavac_out = njavac_dir.display().to_string();
    let njavac_all = || {
        let mut argv = vec![njavac.clone(), "-d".into(), njavac_out.clone()];
        argv.extend(fix_paths.iter().cloned());
        run_quiet(&argv);
    };

    println!("timing (compile the whole {}-fixture suite in one invocation):", fixtures.len());
    let j = bench_block("javac", cfg, cfg.javac_runs, javac_all);
    let n = bench_block("njavac", cfg, cfg.njavac_runs, njavac_all);
    println!(
        "\nnjavac median is {:.1}x faster (single invocation, wall-clock incl. process startup)",
        j.median / n.median
    );
    println!("note: javac's time is dominated by JVM startup; njavac is one native process over all files.");
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
    // fixture (meaningless) and in --offline mode (no javac to time against).
    if cfg.single.is_none() && !cfg.offline {
        timing(&cfg, &fixtures, &javac_dir, &njavac_dir);
    }
}
