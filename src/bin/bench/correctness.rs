use std::path::{Path, PathBuf};
use std::process::Command;

use super::{Config, run_quiet};

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
pub(super) fn correctness(
    cfg: &Config,
    fixtures: &[PathBuf],
    javac_dir: &Path,
    njavac_dir: &Path,
) {
    let source = if cfg.offline { "golden cache" } else { "live javac" };
    println!("correctness ({} fixtures, vs {source}):", fixtures.len());

    // Clear the outputs we're about to regenerate, so a compiler that emits
    // nothing for a fixture yields a *missing* file (a FAIL) rather than a false
    // pass off a stale artifact left by an earlier run.
    for fix in fixtures {
        let base = base_name(fix);
        let _ = std::fs::remove_file(njavac_dir.join(format!("{base}.class")));
        if !cfg.offline {
            let _ = std::fs::remove_file(javac_dir.join(format!("{base}.class")));
        }
    }

    // Compile the whole suite in a *single* invocation of each compiler — one JVM
    // startup for javac instead of one per fixture, which is the whole cost of the
    // online run (javac's ~0.8s is almost all JVM startup). The fixtures are
    // independent single-class files, so batch output is byte-identical to
    // per-file. Offline skips javac entirely and compares against the golden cache.
    let fix_paths: Vec<String> = fixtures.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    if !cfg.offline {
        let mut argv = vec![cfg.javac.clone(), "-d".into(), javac_dir.display().to_string()];
        argv.extend(fix_paths.iter().cloned());
        run_quiet(&argv);
    }
    let mut argv = vec![cfg.njavac.clone(), "-d".into(), njavac_dir.display().to_string()];
    argv.extend(fix_paths);
    run_quiet(&argv);

    // Byte-compare each fixture's output against the reference.
    let mut failures: Vec<String> = Vec::new();
    for fix in fixtures {
        let base = base_name(fix);
        let want = std::fs::read(want_path(cfg, javac_dir, &base));
        let got = std::fs::read(njavac_dir.join(format!("{base}.class")));
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

/// Compile every fixture with javac into the golden cache and exit.
/// The cache is a convenience mirror, always re-recorded from javac — never
/// committed, never hand-edited. Intended to be run *inside the Docker image* so
/// the goldens come from the pinned `javac`, then persisted to a volume.
pub(super) fn record_goldens(cfg: &Config, fixtures: &[PathBuf], golden_dir: &Path) {
    std::fs::create_dir_all(golden_dir).expect("create golden dir");
    // Clear outputs for current fixtures before regenerating them. Goldens for
    // renamed or removed fixtures remain as harmless orphans in this flat cache.
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
        eprintln!("  FAIL  javac exited non-zero while recording");
        std::process::exit(1);
    }
    let ok = fixtures
        .iter()
        .filter(|f| golden_dir.join(format!("{}.class", base_name(f))).exists())
        .count();
    println!("  -> recorded {ok}/{} goldens", fixtures.len());
    if ok != fixtures.len() {
        eprintln!("  FAIL  golden recording was incomplete");
        std::process::exit(1);
    }
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
