use std::collections::HashMap;

use crate::Config;
use crate::model::Prog;
use crate::observe::{Observation, ObservationPair};

/// One distinct finding class: how many programs hit it, and one example.
pub(super) struct SigInfo {
    pub(super) count: u64,
    pub(super) example: String,
}

/// A stable signature for a finding: the normalized structural divergence path
/// from `diff_report` (bracketed indices collapsed to `N`).
pub(super) fn finding_sig(report: Option<&str>) -> String {
    let Some(rep) = report else {
        return "bytes-differ".to_string();
    };
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

/// Stable behavioral signature based on which observable fields differ, not on
/// the unrelated structural path that caused the byte divergence.
pub(super) fn behavior_sig(pair: &ObservationPair) -> String {
    let mut fields = Vec::new();
    if pair.reference.termination != pair.candidate.termination {
        fields.push("termination");
    }
    if pair.reference.detail != pair.candidate.detail {
        fields.push("detail");
    }
    if pair.reference.stdout != pair.candidate.stdout {
        fields.push("stdout");
    }
    if pair.reference.stderr != pair.candidate.stderr {
        fields.push("stderr");
    }
    if fields.is_empty() {
        "observations-match".to_string()
    } else {
        fields.join("+")
    }
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

pub(super) fn print_sig_breakdown(kind: &str, sigs: &HashMap<String, SigInfo>) {
    if sigs.is_empty() {
        return;
    }
    println!("\ndistinct {kind} signatures ({}):", sigs.len());
    let mut v: Vec<(&String, &SigInfo)> = sigs.iter().collect();
    v.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(a.0.cmp(b.0)));
    for (sig, info) in v {
        println!("  {:>6} x  {}   (e.g. {})", info.count, sig, info.example);
    }
}

/// Behavioral findings stay raw until the minimizer gains an observation-aware
/// predicate. Byte-only minimization could drift to an observationally-equivalent
/// divergence and erase the bug this artifact is meant to preserve.
pub(super) fn report_finding(
    cfg: &Config,
    prog: &Prog,
    src: &str,
    class_report: Option<&str>,
    observations: &ObservationPair,
) {
    let _ = std::fs::create_dir_all(&cfg.out_dir);
    let out_java = cfg.out_dir.join(&prog.name.java_file);
    std::fs::write(&out_java, src).expect("write finding source");
    if let Some(report) = class_report {
        let _ = std::fs::write(
            cfg.out_dir.join(format!("{}.diff", prog.name.class)),
            report,
        );
    }
    let observation_report = format!(
        "reference:\n{}\n\ncandidate:\n{}\n",
        describe_observation(&observations.reference),
        describe_observation(&observations.candidate),
    );
    let _ = std::fs::write(
        cfg.out_dir.join(format!("{}.observe", prog.name.class)),
        observation_report,
    );
    println!("  wrote raw behavioral finding to {}", out_java.display());
}

pub(super) fn report_compiler_finding(
    cfg: &Config,
    prog: &Prog,
    src: &str,
    kind: &str,
    detail: &str,
) {
    let heading = if kind.starts_with("scheduled-") {
        "SCHEDULED COVERAGE FAILURE"
    } else {
        "NJAVAC COMPILER FINDING"
    };
    println!(
        "\n{heading} [{kind}]: {} ({})\n{detail}",
        prog.name.class, prog.name.source_arg,
    );
    let dir = cfg.out_dir.join("compiler-findings").join(kind);
    let _ = std::fs::create_dir_all(&dir);
    let out_java = dir.join(&prog.name.java_file);
    let _ = std::fs::write(&out_java, src);
    let _ = std::fs::write(dir.join(format!("{}.txt", prog.name.class)), detail);
    println!("  wrote compiler finding to {}", out_java.display());
}

fn describe_observation(observation: &Observation) -> String {
    format!(
        "  termination: {:?}\n  detail: {:?}\n  stdout: {:?}\n  stderr: {:?}",
        observation.termination,
        observation.detail,
        String::from_utf8_lossy(&observation.stdout),
        String::from_utf8_lossy(&observation.stderr),
    )
}
