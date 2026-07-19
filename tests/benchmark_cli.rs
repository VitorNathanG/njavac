use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn test_directory(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "njavac-benchmark-cli-{name}-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn prepare(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let directory = test_directory(name);
    let source = directory.join("Empty.java");
    std::fs::write(
        &source,
        "public class Empty { public static void main(String[] args) {} }\n",
    )
    .unwrap();
    let golden = directory.join("golden");
    std::fs::create_dir(&golden).unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_njavac"))
        .arg("-d")
        .arg(&golden)
        .arg(&source)
        .status()
        .unwrap();
    assert!(status.success());
    (directory, source, golden)
}

fn run_offline(directory: &Path, source: &Path, golden: &Path, compiler: &Path) -> Output {
    run_offline_with_javap(directory, source, golden, compiler, Path::new("/bin/true"))
}

fn run_offline_with_javap(
    directory: &Path,
    source: &Path,
    golden: &Path,
    compiler: &Path,
    javap: &Path,
) -> Output {
    Command::new(env!("CARGO_BIN_EXE_benchmark"))
        .args(["--offline", "--golden-dir"])
        .arg(golden)
        .args(["--njavac"])
        .arg(compiler)
        .arg("--javap")
        .arg(javap)
        .arg("--out-dir")
        .arg(directory.join("out"))
        .arg(source)
        .output()
        .unwrap()
}

fn script(directory: &Path, name: &str, body: &str) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
}

#[test]
fn focused_correctness_produces_no_report() {
    let (directory, source, golden) = prepare("focused");
    let output = run_offline(
        &directory,
        &source,
        &golden,
        Path::new(env!("CARGO_BIN_EXE_njavac")),
    );
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("correctness only; no report generated")
    );
    assert!(!directory.read_dir().unwrap().any(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|extension| extension == "json")
    }));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn usage_errors_exit_two_without_panicking() {
    let output = Command::new(env!("CARGO_BIN_EXE_benchmark"))
        .args(["--samples", "bad"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));

    let output = Command::new(env!("CARGO_BIN_EXE_benchmark"))
        .arg(OsString::from_vec(vec![0xff]))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
}

#[test]
fn compiler_failure_missing_output_and_wrong_output_are_distinct_failures() {
    let (directory, source, golden) = prepare("failures");

    let failing = script(&directory, "failing", "echo sentinel-error >&2; exit 7");
    let output = run_offline(&directory, &source, &golden, &failing);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("status") && stderr.contains("sentinel-error") && stderr.contains("command:"));

    let stale_dir = directory.join("out/njavac");
    std::fs::create_dir_all(&stale_dir).unwrap();
    std::fs::copy(golden.join("Empty.class"), stale_dir.join("Empty.class")).unwrap();
    let output = run_offline(&directory, &source, &golden, Path::new("/bin/true"));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("output missing"));

    let wrong = script(
        &directory,
        "wrong",
        "out=$2; mkdir -p \"$out\"; printf wrong > \"$out/Empty.class\"; exit 0",
    );
    let output = run_offline(&directory, &source, &golden, &wrong);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("fixtures differed"));

    let output = run_offline_with_javap(
        &directory,
        &source,
        &golden,
        &wrong,
        Path::new("/bin/false"),
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("javap failed"));

    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn resource_child_and_allocation_helper_protocols_work_end_to_end() {
    for (command, outcome) in [
        ("/bin/true", "success"),
        ("/bin/false", "child_failure"),
        ("/definitely/missing", "spawn_failure"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_benchmark"))
            .args(["--resource-child", command])
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["outcome"], outcome);
    }

    let output = Command::new(env!("CARGO_BIN_EXE_benchmark_alloc"))
        .arg("--selftest-accounting")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    let (directory, source, _golden) = prepare("allocation-preflight");
    let output = Command::new(env!("CARGO_BIN_EXE_benchmark_alloc"))
        .arg("--verify-only")
        .arg(&source)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("allocation instrumentation verified for 1 fixtures")
    );
    std::fs::remove_dir_all(directory).unwrap();
}
