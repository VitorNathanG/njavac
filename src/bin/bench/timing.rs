use std::path::{Path, PathBuf};
use std::time::Instant;

use super::{Config, run_quiet};

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
pub(super) fn timing(
    cfg: &Config,
    fixtures: &[PathBuf],
    javac_dir: &Path,
    njavac_dir: &Path,
) {
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
