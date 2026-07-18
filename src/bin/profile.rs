//! In-process micro-profiler for the njavac compile pipeline.
//!
//! The `bench` bin measures the wall-clock of *process spawns*; for these tiny
//! inputs that is almost entirely OS process creation + dynamic linking, not
//! compilation. This bin exercises the compiler IN-PROCESS in a hot loop and
//! reports ns/compile plus a per-phase breakdown
//! (lex / parse / sema / codegen / classfile serialization),
//! so we can see where the compiler's own time actually goes.
//!
//!   make profile [ROUNDS=n] [TRIALS=n] [PHASE=all|lex|parse|sema|codegen|full]
//!
//! Phase times are measured cumulatively (each phase re-runs the prior ones),
//! then differenced, so every phase figure is non-negative by construction.

use std::hint::black_box;
use std::time::{Duration, Instant};

use njavac::{codegen, lexer, parser, sema};

const DEFAULT_ROUNDS: usize = 1_000;
const DEFAULT_TRIALS: usize = 5;

fn main() {
    // rounds per trial; trials taken and min-reduced (min rejects OS/thermal
    // noise, which can only ever add time — critical on a noisy host).
    let mut a = std::env::args().skip(1);
    let first = a.next();
    if first
        .as_deref()
        .is_some_and(|arg| matches!(arg, "-h" | "--help"))
    {
        println!(
            "usage: profile [rounds] [trials] [all|lex|parse|sema|codegen|full]\n\n\
             Hot-loop the full fixture corpus through cumulative compiler phases.\n\
             Defaults: {DEFAULT_ROUNDS} rounds, {DEFAULT_TRIALS} trials, all phases."
        );
        return;
    }
    let rounds = positive_arg(first.as_deref(), DEFAULT_ROUNDS, "rounds");
    let trials = positive_arg(a.next().as_deref(), DEFAULT_TRIALS, "trials");
    let phase = a.next().unwrap_or_else(|| "all".to_string());
    if a.next().is_some() {
        usage_error("too many arguments");
    }
    if !matches!(phase.as_str(), "all" | "lex" | "parse" | "sema" | "codegen" | "full") {
        usage_error("phase must be one of: all, lex, parse, sema, codegen, full");
    }

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
    let total_lines: usize = fixtures.iter().map(|(s, _)| s.lines().count()).sum();
    let compiles = (fixtures.len() * rounds) as f64;
    println!(
        "profiling {} fixtures ({total_bytes} bytes, {total_lines} lines) x {rounds} rounds = {:.0} compiles\n",
        fixtures.len(),
        compiles
    );

    // Warm up caches / branch predictors.
    for (src, name) in &fixtures {
        black_box(njavac::compile(src, name).expect("valid fixture"));
    }

    if phase != "all" {
        let elapsed = match phase.as_str() {
            "lex" => time("lex", rounds, trials, &fixtures, |src, _| {
                black_box(lexer::lex(src).expect("valid fixture"));
            }),
            "parse" => time("parse", rounds, trials, &fixtures, |src, _| {
                let tokens = lexer::lex(src).expect("valid fixture");
                black_box(parser::parse(tokens).expect("valid fixture"));
            }),
            "sema" => time("sema", rounds, trials, &fixtures, |src, _| {
                let tokens = lexer::lex(src).expect("valid fixture");
                let unit = parser::parse(tokens).expect("valid fixture");
                let analysis = sema::analyze(&unit).expect("valid fixture");
                black_box((unit, analysis));
            }),
            "codegen" => time("codegen", rounds, trials, &fixtures, |src, name| {
                let tokens = lexer::lex(src).expect("valid fixture");
                let unit = parser::parse(tokens).expect("valid fixture");
                let analysis = sema::analyze(&unit).expect("valid fixture");
                black_box(codegen::plan(&unit, &analysis, name).expect("valid fixture"));
            }),
            "full" => time("full", rounds, trials, &fixtures, |src, name| {
                let tokens = lexer::lex(src).expect("valid fixture");
                let unit = parser::parse(tokens).expect("valid fixture");
                let analysis = sema::analyze(&unit).expect("valid fixture");
                black_box(codegen::generate(&unit, &analysis, name).expect("valid fixture"));
            }),
            _ => unreachable!(),
        };
        report_throughput(
            elapsed / compiles,
            elapsed,
            rounds,
            total_bytes,
            total_lines,
        );
        return;
    }

    // Cumulative timings over growing prefixes of the pipeline (min of trials).
    let t_lex = time("lex", rounds, trials, &fixtures, |src, _| {
        black_box(lexer::lex(src).expect("valid fixture"));
    });
    let t_parse = time("parse", rounds, trials, &fixtures, |src, _| {
        let tokens = lexer::lex(src).expect("valid fixture");
        black_box(parser::parse(tokens).expect("valid fixture"));
    });
    let t_sema = time("sema", rounds, trials, &fixtures, |src, _| {
        let tokens = lexer::lex(src).expect("valid fixture");
        let unit = parser::parse(tokens).expect("valid fixture");
        let analysis = sema::analyze(&unit).expect("valid fixture");
        black_box((unit, analysis));
    });
    let t_codegen = time("codegen", rounds, trials, &fixtures, |src, name| {
        let tokens = lexer::lex(src).expect("valid fixture");
        let unit = parser::parse(tokens).expect("valid fixture");
        let analysis = sema::analyze(&unit).expect("valid fixture");
        black_box(codegen::plan(&unit, &analysis, name).expect("valid fixture"));
    });
    let t_full = time("full", rounds, trials, &fixtures, |src, name| {
        // Full pipeline including codegen + class-file serialization.
        let tokens = lexer::lex(src).expect("valid fixture");
        let unit = parser::parse(tokens).expect("valid fixture");
        let analysis = sema::analyze(&unit).expect("valid fixture");
        black_box(codegen::generate(&unit, &analysis, name).expect("valid fixture"));
    });

    let per = |ns: f64| ns / compiles;
    let lex = per(t_lex);
    let parse = per(t_parse) - per(t_lex);
    let sema = per(t_sema) - per(t_parse);
    let cgen = per(t_codegen) - per(t_sema);
    let emit = per(t_full) - per(t_codegen);
    let full = per(t_full);

    println!("phase breakdown (ns per file-compile):");
    row("lex", lex, full);
    row("parse", parse, full);
    row("sema", sema, full);
    row("codegen", cgen, full);
    row("classfile emit", emit, full);
    println!("  {:-<38}", "");
    row("total", full, full);

    report_throughput(full, t_full, rounds, total_bytes, total_lines);
}

fn report_throughput(
    ns_per_compile: f64,
    elapsed: f64,
    rounds: usize,
    total_bytes: usize,
    total_lines: usize,
) {
    let mb_s = (total_bytes as f64 * rounds as f64) / elapsed * 1000.0;
    let loc_s = (total_lines as f64 * rounds as f64) / elapsed * 1e9;
    println!(
        "\nthroughput: {:.2}M compiles/s   {:.0} ns/compile   {:.0} MB/s source   {:.2}M loc/s",
        1000.0 / ns_per_compile,
        ns_per_compile,
        mb_s,
        loc_s / 1e6,
    );
}

fn positive_arg(arg: Option<&str>, default: usize, name: &str) -> usize {
    match arg {
        None => default,
        Some(value) => match value.parse() {
            Ok(value) if value > 0 => value,
            _ => usage_error(&format!("{name} must be a positive integer")),
        },
    }
}

fn usage_error(message: &str) -> ! {
    eprintln!(
        "profile: {message}\nusage: profile [rounds] [trials] [all|lex|parse|sema|codegen|full]"
    );
    std::process::exit(2);
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
    phase: &str,
    rounds: usize,
    trials: usize,
    fixtures: &[(String, String)],
    f: F,
) -> f64 {
    let mut best = f64::INFINITY;
    let chunk_rounds = rounds.div_ceil(10).max(1);
    for trial in 1..=trials {
        let mut elapsed = Duration::ZERO;
        let mut completed = 0;
        while completed < rounds {
            let end = (completed + chunk_rounds).min(rounds);
            let t0 = Instant::now();
            for _ in completed..end {
                for (src, name) in fixtures {
                    f(src, name);
                }
            }
            elapsed += t0.elapsed();
            completed = end;
            println!(
                "  {phase:<5} trial {trial}/{trials}: {completed}/{rounds} rounds ({:>3}%)  measured {:.3}s",
                completed * 100 / rounds,
                elapsed.as_secs_f64(),
            );
        }
        best = best.min(elapsed.as_nanos() as f64);
    }
    best
}

fn row(name: &str, ns: f64, total: f64) {
    let pct = if total > 0.0 { ns / total * 100.0 } else { 0.0 };
    println!("  {name:<14} {ns:8.0} ns  ({pct:4.1}%)");
}
