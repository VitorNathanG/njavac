use crate::generate::{Gen, Rng};
use crate::javac::{
    assert_batch_classes, assert_no_unexpected_classes, derive_java, reset_dir, run_javac_batch,
    worker_src_path, JavacWorker,
};
use crate::model::Prog;
use crate::render::render;
use crate::Config;

/// Prove the in-memory worker is byte-identical to the `javac` CLI over `count`
/// generated programs. The CLI remains the ground truth.
pub(crate) fn verify_worker(cfg: &Config) -> i32 {
    let scratch = std::env::temp_dir().join(format!("njavac-fuzz-verify-{}", cfg.seed));
    let src_dir = scratch.join("src");
    let cli_out = scratch.join("out");
    reset_dir(&scratch);
    std::fs::create_dir_all(&src_dir).expect("create src dir");

    let worker_src = worker_src_path();
    let mut worker = JavacWorker::spawn(&derive_java(&cfg.javac), &worker_src);
    let mut g = Gen { rng: Rng::new(cfg.seed) };

    let mut agree = 0u64;
    let mut agree_reject = 0u64;
    let mut diverge = 0u64;
    let mut dumped = 0u32;

    println!(
        "verify-worker: seed={} count={} — worker={} vs CLI={}",
        cfg.seed, cfg.count, worker_src.display(), cfg.javac
    );

    let mut n = 0u64;
    while n < cfg.count {
        let this = cfg.batch.min(cfg.count - n);
        let progs: Vec<Prog> = (0..this).map(|k| g.gen_prog(n + k)).collect();
        let sources: Vec<String> = progs.iter().map(render).collect();

        reset_dir(&cli_out);
        let mut argfile_body = String::new();
        for (p, s) in progs.iter().zip(&sources) {
            let path = src_dir.join(&p.name.java_file);
            std::fs::write(&path, s).expect("write source");
            argfile_body.push_str(&path.display().to_string());
            argfile_body.push('\n');
        }
        let argfile = scratch.join("files.txt");
        std::fs::write(&argfile, &argfile_body).expect("write argfile");
        run_javac_batch(&cfg.javac, &cli_out, &argfile);
        assert_no_unexpected_classes(&cli_out, &progs);

        let units: Vec<(&str, &str)> = progs
            .iter()
            .zip(&sources)
            .map(|(p, s)| (p.name.class.as_str(), s.as_str()))
            .collect();
        let wclasses = worker.compile_batch(&units);
        assert_batch_classes(&wclasses, &progs);

        for (p, s) in progs.iter().zip(&sources) {
            let cli = std::fs::read(cli_out.join(format!("{}.class", p.name.class))).ok();
            let wrk = wclasses.get(&p.name.class).cloned();
            let same = match (&cli, &wrk) {
                (None, None) => {
                    agree_reject += 1;
                    true
                }
                (Some(a), Some(b)) if a == b => {
                    agree += 1;
                    true
                }
                _ => false,
            };
            if !same {
                diverge += 1;
                if dumped < 20 {
                    let kind = match (&cli, &wrk) {
                        (Some(_), None) => "CLI accepted, worker rejected",
                        (None, Some(_)) => "CLI rejected, worker accepted",
                        _ => "bytes differ",
                    };
                    println!("  DIVERGENCE {}: {kind}", p.name.class);
                    let dir = cfg.out_dir.join("worker-mismatch");
                    let _ = std::fs::create_dir_all(&dir);
                    let _ = std::fs::write(dir.join(&p.name.java_file), s);
                    if let Some(a) = &cli {
                        let _ = std::fs::write(dir.join(format!("{}.cli.class", p.name.class)), a);
                    }
                    if let Some(b) = &wrk {
                        let _ = std::fs::write(dir.join(format!("{}.worker.class", p.name.class)), b);
                    }
                    dumped += 1;
                }
            }
        }
        n += this;
        println!(
            "  verify {n}/{}  agree={agree} agree-reject={agree_reject} diverge={diverge}",
            cfg.count
        );
    }

    println!(
        "\nverify-worker done: {} programs  agree={agree} agree-reject={agree_reject} diverge={diverge}",
        cfg.count
    );
    if diverge == 0 {
        println!("  \u{2713} worker is byte-identical to the javac CLI");
        0
    } else {
        println!(
            "  \u{2717} {diverge} divergence(s) — see {}/worker-mismatch/",
            cfg.out_dir.display()
        );
        1
    }
}
