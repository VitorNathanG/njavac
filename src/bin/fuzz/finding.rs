use std::collections::HashMap;

use crate::minimize::{minimize, MinHarness};
use crate::model::Prog;
use crate::render::render;
use crate::Config;

/// One distinct finding class: how many programs hit it, and one example.
pub(super) struct SigInfo {
    pub(super) count: u64,
    pub(super) example: String,
}

/// A stable signature for a finding: the normalized structural divergence path
/// from `diff_report` (bracketed indices collapsed to `N`).
pub(super) fn finding_sig(report: Option<&str>) -> String {
    let Some(rep) = report else { return "bytes-differ".to_string() };
    for line in rep.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("path") {
            if let Some((_, val)) = rest.split_once(':') {
                return normalize_indices(val.trim());
            }
        }
    }
    "bytes-differ".to_string()
}

/// Collapse every run of digits to a single `N` (`cp[17].bytes` -> `cp[N].bytes`).
fn normalize_indices(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_digits = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('N');
                in_digits = true;
            }
        } else {
            out.push(c);
            in_digits = false;
        }
    }
    out
}

pub(super) fn print_sig_breakdown(sigs: &HashMap<String, SigInfo>) {
    if sigs.is_empty() {
        return;
    }
    println!("\ndistinct finding signatures ({}):", sigs.len());
    let mut v: Vec<(&String, &SigInfo)> = sigs.iter().collect();
    v.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(a.0.cmp(b.0)));
    for (sig, info) in v {
        println!("  {:>6} x  {}   (e.g. {})", info.count, sig, info.example);
    }
}

/// Write a finding to the out-dir, preserving the raw/minimized artifact paths and
/// real-CLI re-confirmation behavior.
pub(super) fn report_finding(cfg: &Config, prog: &Prog, src: &str, orig_rep: Option<&str>) {
    let _ = std::fs::create_dir_all(&cfg.out_dir);

    if cfg.no_min {
        let out_java = cfg.out_dir.join(&prog.name.java_file);
        std::fs::write(&out_java, src).expect("write finding source");
        if let Some(rep) = orig_rep {
            let _ = std::fs::write(cfg.out_dir.join(format!("{}.diff", prog.name.class)), rep);
        }
        println!("  wrote raw finding to {}", out_java.display());
        return;
    }

    let mut harness = MinHarness::new(&cfg.javac, cfg.seed);
    let minimized = minimize(prog, &mut harness);
    let msrc = render(&minimized);
    let out_java = cfg.out_dir.join(&minimized.name.java_file);
    std::fs::write(&out_java, &msrc).expect("write finding source");

    let (want, got) = harness.compile_both(&minimized);
    if let (Some(a), Some(b)) = (want, got) {
        if a != b {
            if let Some(rep) = njavac::classdump::diff_report(&a, &b) {
                let _ = std::fs::write(cfg.out_dir.join(format!("{}.diff", minimized.name.class)), rep);
            }
        } else {
            eprintln!("fuzz: WARNING minimizer produced a non-reproducing case — see the .java");
        }
    }
    println!("  wrote minimized finding to {}", out_java.display());
}
