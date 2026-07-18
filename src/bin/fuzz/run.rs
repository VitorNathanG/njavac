use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::finding::{
    behavior_sig, finding_sig, print_sig_breakdown, report_compiler_finding, report_finding, SigInfo,
};
use crate::generate::{Gen, Rng};
use crate::javac::{assert_batch_classes, derive_java, worker_src_path, JavacWorker};
use crate::model::Prog;
use crate::observe::{observer_src_path, ObserveWorker};
use crate::oracle::{classify, njavac_compile, ByteOutcome};
use crate::render::render;
use crate::Config;

#[derive(Default)]
struct Tally {
    exact: u64,
    byte_divergent: u64,
    behavior_match: u64,
    generator_invalid: u64,
    njavac_unsupported: u64,
    njavac_syntax_error: u64,
    njavac_internal_panic: u64,
    findings: u64,
    lines: u64,
    javac_time: Duration,
    njavac_time: Duration,
    observer_time: Duration,
}

/// A fresh seed for a bare `make fuzz`: wall-clock nanoseconds and pid mixed
/// through the SplitMix64 finalizer.
pub(super) fn random_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = nanos ^ (std::process::id() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

pub(super) fn run(cfg: &Config) -> ! {
    let java = derive_java(&cfg.javac);
    let worker_src = worker_src_path();
    let observer_src = observer_src_path();
    let mut worker = JavacWorker::spawn(&java, &worker_src);
    let mut observer: Option<ObserveWorker> = None;
    let mut g = Gen { rng: Rng::new(cfg.seed) };
    let mut tally = Tally::default();
    let mut byte_sigs: HashMap<String, SigInfo> = HashMap::new();
    let mut behavior_sigs: HashMap<String, SigInfo> = HashMap::new();
    let mut unsupported_dumped = 0u32;

    println!(
        "fuzz: seed={} count={} batch={} javac-worker={} observer={} (lazy)\n  reproduce this exact run with: make fuzz SEED={}",
        cfg.seed, cfg.count, cfg.batch, worker_src.display(), observer_src.display(), cfg.seed
    );

    // Generate every batch completely before either compiler sees it. The single
    // shared RNG stream therefore remains independent of compiler outcomes.
    let mut n: u64 = 0;
    while n < cfg.count {
        let this = cfg.batch.min(cfg.count - n);
        let progs: Vec<Prog> = (0..this).map(|k| g.gen_prog(n + k)).collect();
        let sources: Vec<String> = progs.iter().map(render).collect();

        let units: Vec<(&str, &str)> = progs
            .iter()
            .zip(&sources)
            .map(|(p, s)| (p.name.class.as_str(), s.as_str()))
            .collect();
        let t_javac = Instant::now();
        let classes = worker.compile_batch(&units);
        tally.javac_time += t_javac.elapsed();
        assert_batch_classes(&classes, &progs);

        for (p, s) in progs.iter().zip(&sources) {
            tally.lines += s.lines().count() as u64;
            let want = classes.get(&p.name.class);
            let t_njavac = Instant::now();
            let got = njavac_compile(s, &p.name.source_arg);
            tally.njavac_time += t_njavac.elapsed();
            match classify(want.map(Vec::as_slice), got) {
                ByteOutcome::GeneratorInvalid => tally.generator_invalid += 1,
                ByteOutcome::NjavacUnsupported(diagnostic) => {
                    tally.njavac_unsupported += 1;
                    if unsupported_dumped < 20 {
                        let dir = cfg.out_dir.join("unsupported");
                        let _ = std::fs::create_dir_all(&dir);
                        let _ = std::fs::write(dir.join(&p.name.java_file), s);
                        let _ = std::fs::write(
                            dir.join(format!("{}.diagnostic", p.name.class)),
                            diagnostic.render(&p.name.source_arg, s),
                        );
                        unsupported_dumped += 1;
                    }
                }
                ByteOutcome::NjavacSyntaxError(diagnostic) => {
                    tally.njavac_syntax_error += 1;
                    report_compiler_finding(
                        cfg,
                        p,
                        s,
                        "syntax-error",
                        &diagnostic.render(&p.name.source_arg, s),
                    );
                    if !cfg.keep_going {
                        finish_and_exit(&tally, cfg.count, &byte_sigs, &behavior_sigs);
                    }
                }
                ByteOutcome::NjavacInternalPanic(detail) => {
                    tally.njavac_internal_panic += 1;
                    report_compiler_finding(cfg, p, s, "internal-panic", &detail);
                    if !cfg.keep_going {
                        finish_and_exit(&tally, cfg.count, &byte_sigs, &behavior_sigs);
                    }
                }
                ByteOutcome::Identical => tally.exact += 1,
                ByteOutcome::Divergent { javac: a, njavac: b } => {
                    tally.byte_divergent += 1;
                    let rep = njavac::classdump::diff_report(a, &b);
                    let sig = finding_sig(rep.as_deref());
                    let first_byte = !byte_sigs.contains_key(&sig);
                    let info = byte_sigs.entry(sig.clone()).or_insert_with(|| SigInfo {
                        count: 0,
                        example: p.name.class.clone(),
                    });
                    info.count += 1;
                    let observer = observer.get_or_insert_with(|| {
                        ObserveWorker::spawn(&java, &observer_src)
                    });
                    let t_observer = Instant::now();
                    let observations = observer.observe_pair(&p.name.class, a, &b);
                    tally.observer_time += t_observer.elapsed();
                    if observations.reference == observations.candidate {
                        tally.behavior_match += 1;
                        if first_byte {
                            println!(
                                "\nBYTE DIVERGENCE [{sig}] behavior matches: {} ({} vs {} bytes)",
                                p.name.class,
                                a.len(),
                                b.len(),
                            );
                        }
                        continue;
                    }

                    tally.findings += 1;
                    let behavior_sig = behavior_sig(&observations);
                    let first_behavior = !behavior_sigs.contains_key(&behavior_sig);
                    let info = behavior_sigs.entry(behavior_sig.clone()).or_insert_with(|| SigInfo {
                        count: 0,
                        example: p.name.class.clone(),
                    });
                    info.count += 1;
                    if first_behavior {
                        println!(
                            "\nNEW BEHAVIOR FINDING [{behavior_sig}; bytes={sig}]: {} ({} vs {} bytes)",
                            p.name.class,
                            a.len(),
                            b.len(),
                        );
                        report_finding(cfg, p, s, rep.as_deref(), &observations);
                    }
                    if !cfg.keep_going {
                        finish_and_exit(&tally, cfg.count, &byte_sigs, &behavior_sigs);
                    }
                }
            }
        }

        n += this;
        println!(
            "  progress {n}/{}  exact={} behavior-match={} byte-divergent={} gen-invalid={} njavac-unsupported={} njavac-syntax-error={} njavac-internal-panic={} behavioral-findings={} lines={}",
            cfg.count, tally.exact, tally.behavior_match, tally.byte_divergent,
            tally.generator_invalid, tally.njavac_unsupported, tally.njavac_syntax_error,
            tally.njavac_internal_panic, tally.findings, tally.lines
        );
    }

    summary(&tally, cfg.count);
    print_sig_breakdown("byte-divergence", &byte_sigs);
    print_sig_breakdown("behavior-finding", &behavior_sigs);
    std::process::exit(if has_hard_findings(&tally) { 1 } else { 0 });
}

fn summary(t: &Tally, count: u64) {
    debug_assert_eq!(t.byte_divergent, t.behavior_match + t.findings);
    let processed = t.exact
        + t.byte_divergent
        + t.generator_invalid
        + t.njavac_unsupported
        + t.njavac_syntax_error
        + t.njavac_internal_panic;
    println!(
        "\nfuzz done: {processed}/{count} cases  exact={} behavior-match={} byte-divergent={} gen-invalid={} njavac-unsupported={} njavac-syntax-error={} njavac-internal-panic={} behavioral-findings={}  ({} lines compiled)",
        t.exact, t.behavior_match, t.byte_divergent, t.generator_invalid,
        t.njavac_unsupported, t.njavac_syntax_error, t.njavac_internal_panic,
        t.findings, t.lines
    );
    let (jt, nt, n) = (
        t.javac_time.as_secs_f64(),
        t.njavac_time.as_secs_f64(),
        processed.max(1) as f64,
    );
    let ratio = if nt > 0.0 { jt / nt } else { 0.0 };
    println!(
        "  compile time: javac(worker) {jt:.3}s ({:.3} ms/prog, incl. one-time JVM warmup + IPC)  |  \
         njavac {nt:.3}s ({:.1} µs/prog)  |  njavac ~{ratio:.0}x faster end-to-end",
        jt * 1e3 / n,
        nt * 1e6 / n,
    );
    if t.findings > 0 {
        println!("  -> {} behavioral finding(s); see the fuzz-out/ dir", t.findings);
    }
    let compiler_findings = t.njavac_syntax_error + t.njavac_internal_panic;
    if compiler_findings > 0 {
        println!("  -> {compiler_findings} compiler finding(s); see the fuzz-out/ dir");
    }
    if t.byte_divergent > 0 {
        println!(
            "  observer time: {:.3}s across {} byte-divergent program(s)",
            t.observer_time.as_secs_f64(),
            t.byte_divergent,
        );
    }
}

fn has_hard_findings(t: &Tally) -> bool {
    t.findings > 0 || t.njavac_syntax_error > 0 || t.njavac_internal_panic > 0
}

fn finish_and_exit(
    tally: &Tally,
    count: u64,
    byte_sigs: &HashMap<String, SigInfo>,
    behavior_sigs: &HashMap<String, SigInfo>,
) -> ! {
    summary(tally, count);
    print_sig_breakdown("byte-divergence", byte_sigs);
    print_sig_breakdown("behavior-finding", behavior_sigs);
    std::process::exit(1);
}
