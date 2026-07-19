use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::Config;
use super::resource::format_command;

pub(super) fn class_name(fixture: &Path) -> Result<&str, String> {
    fixture
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("{} has no UTF-8 class basename", fixture.display()))
}

fn expected_path(cfg: &Config, javac_dir: &Path, base: &str) -> PathBuf {
    let directory = if cfg.offline { Path::new(&cfg.golden_dir) } else { javac_dir };
    directory.join(format!("{base}.class"))
}

pub(super) fn remove_if_exists(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot remove {}: {error}", path.display())),
    }
}

pub(super) fn correctness(
    cfg: &Config,
    fixtures: &[PathBuf],
    javac_dir: &Path,
    njavac_dir: &Path,
) -> Result<(), String> {
    let source = if cfg.offline { "golden cache" } else { "live javac" };
    println!("correctness ({} fixtures, vs {source}):", fixtures.len());

    for fixture in fixtures {
        let base = class_name(fixture)?;
        remove_if_exists(&njavac_dir.join(format!("{base}.class")))?;
        if !cfg.offline {
            remove_if_exists(&javac_dir.join(format!("{base}.class")))?;
        }
    }

    if !cfg.offline {
        let mut command = vec![
            OsString::from(&cfg.javac),
            OsString::from("-d"),
            javac_dir.as_os_str().into(),
        ];
        command.extend(fixtures.iter().map(|fixture| fixture.as_os_str().into()));
        run_checked(&command, "javac correctness preflight")?;
    }
    let mut command = vec![
        OsString::from(&cfg.njavac),
        OsString::from("-d"),
        njavac_dir.as_os_str().into(),
    ];
    command.extend(fixtures.iter().map(|fixture| fixture.as_os_str().into()));
    run_checked(&command, "njavac correctness preflight")?;

    let mut failures = Vec::new();
    for fixture in fixtures {
        let base = class_name(fixture)?;
        let expected_file = expected_path(cfg, javac_dir, base);
        let actual_file = njavac_dir.join(format!("{base}.class"));
        let expected = std::fs::read(&expected_file)
            .map_err(|error| format!("reference output missing at {}: {error}", expected_file.display()))?;
        let actual = std::fs::read(&actual_file)
            .map_err(|error| format!("njavac output missing at {}: {error}", actual_file.display()))?;
        if expected == actual {
            println!("  PASS  {base}  ({} bytes)", expected.len());
        } else {
            println!("  FAIL  {base}");
            failures.push((base.to_string(), expected_file, actual_file, expected, actual));
        }
    }
    if failures.is_empty() {
        println!("  -> all {} byte-identical\n", fixtures.len());
        return Ok(());
    }

    let (base, expected_file, actual_file, expected, actual) = &failures[0];
    println!(
        "\n{}/{} failed. First mismatch: {base}",
        failures.len(),
        fixtures.len(),
    );
    if cfg.offline && fixtures.len() > 1 && failures.len() == fixtures.len() {
        println!(
            "(every fixture failed in --offline mode; rebuild the golden cache at {})",
            cfg.golden_dir,
        );
    }
    if let Some(report) = njavac::classdump::diff_report(expected, actual) {
        println!("\nstructural divergence (classdiff):");
        for line in report.lines() {
            println!("  {line}");
        }
    }
    println!("\njavap divergence:");
    let expected_lines = javap_lines(cfg, expected_file)?;
    let actual_lines = javap_lines(cfg, actual_file)?;
    print_first_divergence(&expected_lines, &actual_lines);
    Err(format!("{}/{} fixtures differed", failures.len(), fixtures.len()))
}

pub(super) fn record_goldens(
    cfg: &Config,
    fixtures: &[PathBuf],
    golden_dir: &Path,
) -> Result<(), String> {
    std::fs::create_dir_all(golden_dir)
        .map_err(|error| format!("cannot create golden directory {}: {error}", golden_dir.display()))?;
    for fixture in fixtures {
        remove_if_exists(&golden_dir.join(format!("{}.class", class_name(fixture)?)))?;
    }
    println!(
        "recording {} goldens into {} (pinned javac, one invocation):",
        fixtures.len(),
        golden_dir.display(),
    );
    let mut command = vec![
        OsString::from(&cfg.javac),
        OsString::from("-d"),
        golden_dir.as_os_str().into(),
    ];
    command.extend(fixtures.iter().map(|fixture| fixture.as_os_str().into()));
    run_checked(&command, "javac golden recording")?;
    for fixture in fixtures {
        let path = golden_dir.join(format!("{}.class", class_name(fixture)?));
        if !path.is_file() {
            return Err(format!("golden recording omitted {}", path.display()));
        }
    }
    println!("  -> recorded {}/{} goldens", fixtures.len(), fixtures.len());
    Ok(())
}

fn run_checked(command: &[OsString], context: &str) -> Result<(), String> {
    let output = Command::new(&command[0])
        .args(&command[1..])
        .output()
        .map_err(|error| {
            format!(
                "{context} could not start: {error}; command: {}",
                format_command(command),
            )
        })?;
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{context} failed with status {}; command: {}; stderr: {}",
        output.status,
        format_command(command),
        String::from_utf8_lossy(&output.stderr).trim(),
    ))
}

fn javap_lines(cfg: &Config, class: &Path) -> Result<Vec<String>, String> {
    let command = [
        OsString::from(&cfg.javap),
        OsString::from("-v"),
        OsString::from("-p"),
        class.as_os_str().into(),
    ];
    let output = Command::new(&command[0])
        .args(&command[1..])
        .output()
        .map_err(|error| {
            format!(
                "cannot run javap on {}: {error}; command: {}",
                class.display(),
                format_command(&command),
            )
        })?;
    if !output.status.success() {
        return Err(format!(
            "javap failed for {} with status {}; command: {}; stderr: {}",
            class.display(),
            output.status,
            format_command(&command),
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| {
            !line.starts_with("Classfile ")
                && !line.starts_with("  Last modified")
                && !line.starts_with("  SHA-256")
        })
        .map(str::to_string)
        .collect())
}

fn print_first_divergence(expected: &[String], actual: &[String]) {
    let length = expected.len().max(actual.len());
    let Some(index) = (0..length).find(|&index| expected.get(index) != actual.get(index)) else {
        println!("(bytes differ but javap output matches)");
        return;
    };
    let start = index.saturating_sub(4);
    println!("first divergence at javap line {} ('<' javac, '>' njavac):", index + 1);
    for line_index in start..=index {
        if let Some(line) = expected.get(line_index) {
            println!("  {} < {line}", if line_index == index { "*" } else { " " });
        }
    }
    for line_index in start..=index {
        if let Some(line) = actual.get(line_index) {
            println!("  {} > {line}", if line_index == index { "*" } else { " " });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{javap_lines, remove_if_exists};
    use crate::Config;

    #[test]
    fn removal_ignores_only_not_found() {
        let path = std::env::temp_dir().join(format!("njavac-remove-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        assert!(remove_if_exists(&path).is_ok());
        std::fs::create_dir(&path).unwrap();
        assert!(remove_if_exists(&path).is_err());
        std::fs::remove_dir(&path).unwrap();
    }

    #[test]
    fn javap_failure_is_not_treated_as_a_diff() {
        let mut config = Config::defaults();
        config.javap = "/bin/false".to_string();
        assert!(javap_lines(&config, std::path::Path::new("missing.class")).is_err());
    }
}
