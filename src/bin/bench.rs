//! njavac benchmark harness.
//!
//! Spawns the reference compiler (`javac`) and `njavac` as subprocesses, times
//! end-to-end wall-clock (process spawn included, since that's what a user
//! waits for), and reports min/median/mean/stddev over N runs after warmup.
//!
//! Dependency-free on purpose: reproducible builds, nothing to pin but the
//! toolchain itself. Configure via flags (see `--help`).

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

struct Config {
    runs: usize,
    warmup: usize,
    src: String,
    javac: String,
    njavac: String,
    out_dir: String,
}

impl Config {
    fn from_args() -> Config {
        let home = std::env::var("HOME").unwrap_or_default();
        let mut c = Config {
            runs: 5,
            warmup: 5,
            src: "reference/HelloWorld.java".into(),
            javac: format!("{home}/.sdkman/candidates/java/25.0.2-graalce/bin/javac"),
            njavac: "target/release/njavac".into(),
            out_dir: "target/bench-out".into(),
        };
        // Env override is convenient in containers where paths differ.
        if let Ok(v) = std::env::var("JAVAC") {
            c.javac = v;
        }
        let mut args = std::env::args().skip(1);
        while let Some(a) = args.next() {
            let mut val = || args.next().expect("flag needs a value");
            match a.as_str() {
                "--runs" => c.runs = val().parse().expect("runs must be a number"),
                "--warmup" => c.warmup = val().parse().expect("warmup must be a number"),
                "--src" => c.src = val(),
                "--javac" => c.javac = val(),
                "--njavac" => c.njavac = val(),
                "--out-dir" => c.out_dir = val(),
                "--help" | "-h" => {
                    println!(
                        "usage: bench [--runs N] [--warmup N] [--src F] \
                         [--javac PATH] [--njavac PATH] [--out-dir D]"
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

/// Run `argv` once, asserting success; return elapsed milliseconds.
fn time_once(argv: &[String]) -> f64 {
    let t0 = Instant::now();
    let status = Command::new(&argv[0])
        .args(&argv[1..])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap_or_else(|e| panic!("failed to spawn {}: {e}", argv[0]));
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    assert!(status.success(), "{} exited with {status}", argv[0]);
    ms
}

fn bench(name: &str, argv: &[String], cfg: &Config) -> Stats {
    for _ in 0..cfg.warmup {
        time_once(argv);
    }
    let times = (0..cfg.runs).map(|_| time_once(argv)).collect();
    let s = stats(times);
    println!(
        "{name:12}  min {:8.3}  median {:8.3}  mean {:8.3}  stddev {:7.3}  (ms, n={})",
        s.min, s.median, s.mean, s.stddev, cfg.runs
    );
    s
}

fn main() {
    let cfg = Config::from_args();
    // Each compiler writes into its own subdir so the file name stays
    // HelloWorld.class (matching the class name) and the two never clobber.
    let javac_dir = PathBuf::from(&cfg.out_dir).join("javac");
    let njavac_dir = PathBuf::from(&cfg.out_dir).join("njavac");
    std::fs::create_dir_all(&javac_dir).expect("create javac out dir");
    std::fs::create_dir_all(&njavac_dir).expect("create njavac out dir");
    let javac_out = javac_dir.join("HelloWorld.class");
    let njavac_out = njavac_dir.join("HelloWorld.class");

    let javac_argv = vec![
        cfg.javac.clone(),
        "-d".into(),
        javac_dir.to_string_lossy().into_owned(),
        cfg.src.clone(),
    ];
    let njavac_argv = vec![cfg.njavac.clone(), njavac_out.to_string_lossy().into_owned()];

    // Integrity check first: compile with both, then require our bytes to be
    // byte-identical to *this environment's* javac. There is no committed
    // golden — the reference is whatever javac produces here, which also makes
    // the Docker run self-validating. A fast compiler that emits wrong bytes is
    // not a compiler.
    time_once(&javac_argv);
    time_once(&njavac_argv);
    let reference = std::fs::read(&javac_out).expect("read javac output");
    let produced = std::fs::read(&njavac_out).expect("read njavac output");
    if produced != reference {
        eprintln!(
            "FAIL: njavac output ({} bytes) is not byte-identical to javac ({} bytes)",
            produced.len(),
            reference.len()
        );
        std::process::exit(1);
    }
    println!(
        "integrity: njavac output is byte-identical to javac ({} bytes)\n",
        reference.len()
    );

    let j = bench("javac", &javac_argv, &cfg);
    let n = bench("njavac", &njavac_argv, &cfg);

    println!(
        "\nnjavac median is {:.1}x faster (wall-clock incl. process spawn)",
        j.median / n.median
    );
    println!("note: javac's time is dominated by JVM startup; njavac does not yet parse.");
}
