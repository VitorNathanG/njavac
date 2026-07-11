//! In-process micro-profiler for the njavac compile pipeline.
//!
//! The `bench` bin measures the wall-clock of *process spawns*; for these tiny
//! inputs that is almost entirely OS process creation + dynamic linking, not
//! compilation. This bin exercises the compiler IN-PROCESS in a hot loop and
//! reports ns/compile plus a per-phase breakdown (lex / parse / sema / codegen),
//! so we can see where the compiler's own time actually goes.
//!
//!   cargo run --release --bin profile [rounds]
//!
//! Phase times are measured cumulatively (each phase re-runs the prior ones),
//! then differenced, so every phase figure is non-negative by construction.

use std::hint::black_box;
use std::time::Instant;

use njavac::{codegen, lexer, parser, sema};

fn main() {
    // rounds per trial; trials taken and min-reduced (min rejects OS/thermal
    // noise, which can only ever add time — critical on a noisy host).
    let mut a = std::env::args().skip(1);
    let rounds: usize = a.next().and_then(|s| s.parse().ok()).unwrap_or(30_000);
    let trials: usize = a.next().and_then(|s| s.parse().ok()).unwrap_or(5);

    let mut paths = Vec::new();
    collect_java(std::path::Path::new("fixtures"), &mut paths);
    let mut fixtures: Vec<(String, String)> = paths
        .iter()
        .map(|p| {
            // SourceFile is the bare basename, so that is the compile name.
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (std::fs::read_to_string(p).unwrap(), name)
        })
        .collect();
    fixtures.sort_by(|a, b| a.1.cmp(&b.1));

    let total_bytes: usize = fixtures.iter().map(|(s, _)| s.len()).sum();
    let compiles = (fixtures.len() * rounds) as f64;
    println!(
        "profiling {} fixtures ({total_bytes} source bytes) x {rounds} rounds = {:.0} compiles\n",
        fixtures.len(),
        compiles
    );

    // Warm up caches / branch predictors.
    for (src, name) in &fixtures {
        black_box(njavac::compile(src, name));
    }

    // Cumulative timings over growing prefixes of the pipeline (min of trials).
    let t_lex = time(rounds, trials, &fixtures, |src, _| {
        black_box(lexer::lex(src));
    });
    let t_parse = time(rounds, trials, &fixtures, |src, _| {
        black_box(parser::parse(lexer::lex(src)));
    });
    let t_sema = time(rounds, trials, &fixtures, |src, _| {
        let unit = parser::parse(lexer::lex(src));
        let analysis = sema::analyze(&unit);
        black_box((unit, analysis));
    });
    let t_full = time(rounds, trials, &fixtures, |src, name| {
        // Full pipeline including codegen + class-file serialization.
        let unit = parser::parse(lexer::lex(src));
        let analysis = sema::analyze(&unit);
        black_box(codegen::generate(&unit.class, &analysis, name));
    });

    let per = |ns: f64| ns / compiles;
    let lex = per(t_lex);
    let parse = per(t_parse) - per(t_lex);
    let sema = per(t_sema) - per(t_parse);
    let cgen = per(t_full) - per(t_sema);
    let full = per(t_full);

    println!("phase breakdown (ns per file-compile):");
    row("lex", lex, full);
    row("parse", parse, full);
    row("sema", sema, full);
    row("codegen+emit", cgen, full);
    println!("  {:-<38}", "");
    row("total", full, full);

    let mb_s = (total_bytes as f64 * rounds as f64) / t_full * 1000.0;
    println!(
        "\nthroughput: {:.2}M compiles/s   {:.0} ns/compile   {:.0} MB/s source",
        1000.0 / full,
        full,
        mb_s
    );
}

/// Recurse into `dir`, appending every `*.java` file (fixtures are grouped into
/// topical subfolders).
fn collect_java(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("read fixtures dir");
    for p in entries.filter_map(|e| e.ok().map(|e| e.path())) {
        if p.is_dir() {
            collect_java(&p, out);
        } else if p.extension().is_some_and(|x| x == "java") {
            out.push(p);
        }
    }
}

/// Minimum (over `trials`) nanoseconds to run `f` over every fixture `rounds`
/// times. Min is the robust estimator: noise only ever adds time.
fn time<F: Fn(&str, &str)>(
    rounds: usize,
    trials: usize,
    fixtures: &[(String, String)],
    f: F,
) -> f64 {
    let mut best = f64::INFINITY;
    for _ in 0..trials {
        let t0 = Instant::now();
        for _ in 0..rounds {
            for (src, name) in fixtures {
                f(src, name);
            }
        }
        best = best.min(t0.elapsed().as_nanos() as f64);
    }
    best
}

fn row(name: &str, ns: f64, total: f64) {
    let pct = if total > 0.0 { ns / total * 100.0 } else { 0.0 };
    println!("  {name:<14} {ns:8.0} ns  ({pct:4.1}%)");
}
