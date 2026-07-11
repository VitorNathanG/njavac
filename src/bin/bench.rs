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
//!      thermal), so timings are only produced inside the Docker harness — see
//!      docker-bench.sh. Correctness still runs on the host.
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
                "--help" | "-h" => {
                    println!(
                        "usage: bench [--javac-runs N] [--njavac-runs N] [--warmup N] \
                         [--fixtures DIR] [--javac PATH] [--javap PATH] [--njavac PATH] \
                         [--out-dir DIR]"
                    );
                    std::process::exit(0);
                }
                other => {
                    eprintln!("unknown flag: {other}");
                    std::process::exit(2);
                }
            }
        }
        c
    }
}

/// All `*.java` fixtures, sorted by name for stable output.
fn discover_fixtures(dir: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read fixtures dir {dir}: {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "java"))
        .collect();
    v.sort();
    assert!(!v.is_empty(), "no .java fixtures in {dir}");
    v
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

/// Compile every fixture with both compilers and byte-compare. Exits the process
/// (non-zero) on the first mismatch after showing a localized javap diff.
fn correctness(cfg: &Config, fixtures: &[PathBuf], javac_dir: &Path, njavac_dir: &Path) {
    println!("correctness ({} fixtures):", fixtures.len());
    let mut failures: Vec<String> = Vec::new();

    for fix in fixtures {
        let base = base_name(fix);
        let fix_s = fix.to_string_lossy().into_owned();
        let njavac_out = njavac_dir.join(format!("{base}.class"));

        run_quiet(&[cfg.javac.clone(), "-d".into(), javac_dir.display().to_string(), fix_s.clone()]);
        run_quiet(&[cfg.njavac.clone(), fix_s, njavac_out.display().to_string()]);

        let want = std::fs::read(javac_dir.join(format!("{base}.class")));
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

    // Localize the first failure with a noise-stripped javap diff.
    let base = &failures[0];
    println!("\n{}/{} failed. First mismatch: {base}", failures.len(), fixtures.len());
    let a = javap_lines(cfg, &javac_dir.join(format!("{base}.class")));
    let b = javap_lines(cfg, &njavac_dir.join(format!("{base}.class")));
    print_first_divergence(&a, &b);
    std::process::exit(1);
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

/// Time compiling the entire suite with each compiler. javac gets a single
/// invocation (it amortizes JVM startup across all files, which is how it is
/// actually used); njavac spawns once per file, reflecting its process model.
fn timing(cfg: &Config, fixtures: &[PathBuf], javac_dir: &Path, njavac_dir: &Path) {
    let in_harness = std::env::var_os("NJAVAC_IN_CONTAINER").is_some()
        || Path::new("/.dockerenv").exists();
    if !in_harness && std::env::var_os("NJAVAC_BENCH_ALLOW_HOST").is_none() {
        println!("timing skipped: run ./docker-bench.sh for deterministic numbers");
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
    let njavac_all = || {
        for fix in fixtures {
            let out = njavac_dir.join(format!("{}.class", base_name(fix)));
            run_quiet(&[njavac.clone(), fix.to_string_lossy().into_owned(), out.display().to_string()]);
        }
    };

    println!("timing (compile the whole {}-fixture suite):", fixtures.len());
    let j = bench_block("javac", cfg, cfg.javac_runs, javac_all);
    let n = bench_block("njavac", cfg, cfg.njavac_runs, njavac_all);
    println!(
        "\nnjavac median is {:.1}x faster (wall-clock incl. process spawn)",
        j.median / n.median
    );
    println!("note: javac's time is dominated by JVM startup; njavac spawns once per file.");
}

fn main() {
    let cfg = Config::from_args();
    let fixtures = discover_fixtures(&cfg.fixtures_dir);
    let javac_dir = PathBuf::from(&cfg.out_dir).join("javac");
    let njavac_dir = PathBuf::from(&cfg.out_dir).join("njavac");
    std::fs::create_dir_all(&javac_dir).expect("create javac out dir");
    std::fs::create_dir_all(&njavac_dir).expect("create njavac out dir");

    correctness(&cfg, &fixtures, &javac_dir, &njavac_dir);
    timing(&cfg, &fixtures, &javac_dir, &njavac_dir);
}
