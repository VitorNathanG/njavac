use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::finding::{finding_sig, print_sig_breakdown, report_finding, SigInfo};
use crate::generate::{Gen, Rng};
use crate::javac::{assert_batch_classes, derive_java, worker_src_path, JavacWorker};
use crate::model::Prog;
use crate::oracle::{classify, njavac_compile, ByteOutcome};
use crate::render::render;
use crate::Config;

#[derive(Default)]
struct Tally {
    pass: u64,
    generator_invalid: u64,
    njavac_reject: u64,
    findings: u64,
    lines: u64,
    javac_time: Duration,
    njavac_time: Duration,
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
    let worker_src = worker_src_path();
    let mut worker = JavacWorker::spawn(&derive_java(&cfg.javac), &worker_src);
    let mut g = Gen { rng: Rng::new(cfg.seed) };
    let mut tally = Tally::default();
    let mut sigs: HashMap<String, SigInfo> = HashMap::new();
    let mut reject_dumped = 0u32;

    println!(
        "fuzz: seed={} count={} batch={} javac-worker={}\n  reproduce this exact run with: make fuzz SEED={}",
        cfg.seed, cfg.count, cfg.batch, worker_src.display(), cfg.seed
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
                ByteOutcome::NjavacReject => {
                    tally.njavac_reject += 1;
                    if reject_dumped < 20 {
                        let rd = cfg.out_dir.join("rejects");
                        let _ = std::fs::create_dir_all(&rd);
                        let _ = std::fs::write(rd.join(&p.name.java_file), s);
                        reject_dumped += 1;
                    }
                }
                ByteOutcome::Identical => tally.pass += 1,
                ByteOutcome::Divergent { javac: a, njavac: b } => {
                    tally.findings += 1;
                    let rep = njavac::classdump::diff_report(a, &b);
                    let sig = finding_sig(rep.as_deref());
                    let first = !sigs.contains_key(&sig);
                    let info = sigs.entry(sig.clone()).or_insert_with(|| SigInfo {
                        count: 0,
                        example: p.name.class.clone(),
                    });
                    info.count += 1;
                    if first {
                        println!("\nNEW FINDING [{sig}]: {} ({} vs {} bytes)", p.name.class, a.len(), b.len());
                        report_finding(cfg, p, s, rep.as_deref());
                    }
                    if !cfg.keep_going {
                        summary(&tally, cfg.count);
                        print_sig_breakdown(&sigs);
                        std::process::exit(1);
                    }
                }
            }
        }

        n += this;
        println!(
            "  progress {n}/{}  pass={} gen-invalid={} njavac-reject={} findings={} lines={}",
            cfg.count, tally.pass, tally.generator_invalid, tally.njavac_reject, tally.findings, tally.lines
        );
    }

    summary(&tally, cfg.count);
    print_sig_breakdown(&sigs);
    std::process::exit(if tally.findings > 0 { 1 } else { 0 });
}

fn summary(t: &Tally, count: u64) {
    println!(
        "\nfuzz done: {count} cases  pass={} gen-invalid={} njavac-reject={} findings={}  ({} lines compiled)",
        t.pass, t.generator_invalid, t.njavac_reject, t.findings, t.lines
    );
    let (jt, nt, n) = (t.javac_time.as_secs_f64(), t.njavac_time.as_secs_f64(), count.max(1) as f64);
    let ratio = if nt > 0.0 { jt / nt } else { 0.0 };
    println!(
        "  compile time: javac(worker) {jt:.3}s ({:.3} ms/prog, incl. one-time JVM warmup + IPC)  |  \
         njavac {nt:.3}s ({:.1} µs/prog)  |  njavac ~{ratio:.0}x faster end-to-end",
        jt * 1e3 / n,
        nt * 1e6 / n,
    );
    if t.findings > 0 {
        println!("  -> {} byte-mismatch finding(s); see the fuzz-out/ dir", t.findings);
    }
}
