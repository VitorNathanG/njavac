//! Docker-backed correctness, performance, phase, and allocation benchmark.

mod correctness;
mod measurement;
mod model;
mod phase;
mod report;
mod resource;

use std::collections::HashMap;
use std::ffi::{OsString, OsString as NativeString};
use std::path::{Path, PathBuf};
use std::process::Command;

use correctness::{correctness, record_goldens};
use model::ReportDocument;

#[derive(Clone)]
struct Config {
    samples: usize,
    warmup: usize,
    rounds: usize,
    allocation_rounds: usize,
    javac: String,
    javap: String,
    njavac: String,
    alloc_helper: String,
    fixtures_dir: String,
    out_dir: String,
    json_path: Option<PathBuf>,
    single: Option<PathBuf>,
    record: bool,
    offline: bool,
    golden_dir: String,
    no_performance: bool,
    verify_instrumentation: bool,
}

enum ParseOutcome {
    Run(Config),
    Help,
}

#[derive(Debug)]
enum AppError {
    Usage(String),
    Runtime(String),
}

impl Config {
    fn defaults() -> Self {
        let home = std::env::var("HOME").unwrap_or_default();
        let javac = std::env::var("JAVAC").unwrap_or_else(|_| {
            format!("{home}/.sdkman/candidates/java/25.0.2-graalce/bin/javac")
        });
        let javap = std::env::var("JAVAP").unwrap_or_else(|_| {
            javac
                .strip_suffix("javac")
                .map_or_else(|| "javap".into(), |base| format!("{base}javap"))
        });
        Self {
            samples: 5,
            warmup: 2,
            rounds: 100,
            allocation_rounds: 1,
            javac,
            javap,
            njavac: "target/release/njavac".into(),
            alloc_helper: "target/release/benchmark_alloc".into(),
            fixtures_dir: "fixtures".into(),
            out_dir: "/tmp/njavac-benchmark".into(),
            json_path: None,
            single: None,
            record: false,
            offline: false,
            golden_dir: "target/benchmark-golden".into(),
            no_performance: false,
            verify_instrumentation: false,
        }
    }

    fn parse(args: impl IntoIterator<Item = String>) -> Result<ParseOutcome, AppError> {
        let mut config = Self::defaults();
        let mut args = args.into_iter();
        let mut explicit_performance_control = false;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--samples" => {
                    config.samples = positive(&next_value(&mut args, &argument)?, "samples")?;
                    explicit_performance_control = true;
                }
                "--warmup" => {
                    config.warmup = nonnegative(&next_value(&mut args, &argument)?, "warmup")?;
                    explicit_performance_control = true;
                }
                "--rounds" => {
                    config.rounds = positive(&next_value(&mut args, &argument)?, "rounds")?;
                    explicit_performance_control = true;
                }
                "--allocation-rounds" => {
                    config.allocation_rounds =
                        positive(&next_value(&mut args, &argument)?, "allocation-rounds")?;
                    explicit_performance_control = true;
                }
                "--javac" => config.javac = next_value(&mut args, &argument)?,
                "--javap" => config.javap = next_value(&mut args, &argument)?,
                "--njavac" => config.njavac = next_value(&mut args, &argument)?,
                "--alloc-helper" => config.alloc_helper = next_value(&mut args, &argument)?,
                "--fixtures" => config.fixtures_dir = next_value(&mut args, &argument)?,
                "--out-dir" => config.out_dir = next_value(&mut args, &argument)?,
                "--json" => {
                    config.json_path = Some(PathBuf::from(next_value(&mut args, &argument)?));
                }
                "--golden-dir" => config.golden_dir = next_value(&mut args, &argument)?,
                "--record" => config.record = true,
                "--offline" => config.offline = true,
                "--no-performance" => config.no_performance = true,
                "--verify-instrumentation" => config.verify_instrumentation = true,
                "--help" | "-h" => return Ok(ParseOutcome::Help),
                path if !path.starts_with('-') && path.ends_with(".java") => {
                    if config.single.replace(PathBuf::from(path)).is_some() {
                        return Err(AppError::Usage(
                            "only one positional .java fixture is allowed".to_string(),
                        ));
                    }
                }
                other => return Err(AppError::Usage(format!("unknown argument: {other}"))),
            }
        }

        let modes = usize::from(config.record)
            + usize::from(config.offline)
            + usize::from(config.no_performance)
            + usize::from(config.verify_instrumentation);
        if modes > 1 {
            return Err(AppError::Usage(
                "--record, --offline, --no-performance, and --verify-instrumentation are mutually exclusive".to_string(),
            ));
        }
        if config.record && config.single.is_some() {
            return Err(AppError::Usage(
                "--record always records the complete corpus and does not accept a positional fixture"
                    .to_string(),
            ));
        }
        let produces_report = !config.record
            && !config.offline
            && !config.no_performance
            && !config.verify_instrumentation
            && config.single.is_none();
        if config.json_path.is_some() && !produces_report {
            return Err(AppError::Usage(
                "--json is valid only for a complete performance benchmark".to_string(),
            ));
        }
        if explicit_performance_control && !produces_report {
            return Err(AppError::Usage(
                "performance controls are invalid in correctness-only and record modes".to_string(),
            ));
        }
        Ok(ParseOutcome::Run(config))
    }

    fn performance_enabled(&self) -> bool {
        self.single.is_none()
            && !self.record
            && !self.offline
            && !self.no_performance
            && !self.verify_instrumentation
    }

    fn resolve_executables(&mut self) -> Result<(), String> {
        if !self.record {
            self.njavac = resource::resolve_executable(&self.njavac)?;
        }
        if !self.offline && !self.verify_instrumentation {
            self.javac = resource::resolve_executable(&self.javac)?;
        }
        if !self.record && !self.performance_enabled() && !self.verify_instrumentation {
            self.javap = resource::resolve_executable(&self.javap)?;
        }
        if self.performance_enabled() || self.verify_instrumentation {
            self.alloc_helper = resource::resolve_executable(&self.alloc_helper)?;
        }
        Ok(())
    }
}

fn next_value(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<String, AppError> {
    args.next()
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| AppError::Usage(format!("{flag} requires a value")))
}

fn positive(value: &str, name: &str) -> Result<usize, AppError> {
    value
        .parse()
        .ok()
        .filter(|&value| value > 0)
        .ok_or_else(|| AppError::Usage(format!("{name} must be a positive integer")))
}

fn nonnegative(value: &str, name: &str) -> Result<usize, AppError> {
    value
        .parse()
        .map_err(|_| AppError::Usage(format!("{name} must be a nonnegative integer")))
}

fn discover_fixtures(directory: &str) -> Result<Vec<PathBuf>, String> {
    let mut fixtures = Vec::new();
    collect_java(Path::new(directory), &mut fixtures)?;
    fixtures.sort();
    if fixtures.is_empty() {
        return Err(format!("no .java fixtures under {directory}"));
    }
    reject_duplicate_class_names(&fixtures)?;
    Ok(fixtures)
}

fn collect_java(directory: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("cannot read fixture directory {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("cannot read an entry in {}: {error}", directory.display()))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", entry.path().display()))?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_java(&path, output)?;
        } else if file_type.is_file() && path.extension().is_some_and(|extension| extension == "java") {
            output.push(path);
        }
    }
    Ok(())
}

fn reject_duplicate_class_names(fixtures: &[PathBuf]) -> Result<(), String> {
    let mut names: HashMap<NativeString, &Path> = HashMap::new();
    for fixture in fixtures {
        let stem = fixture
            .file_stem()
            .ok_or_else(|| format!("{} has no class basename", fixture.display()))?
            .to_os_string();
        if let Some(previous) = names.insert(stem, fixture) {
            return Err(format!(
                "duplicate class basename in {} and {}",
                previous.display(),
                fixture.display(),
            ));
        }
    }
    Ok(())
}

fn main() {
    let raw_args: Vec<OsString> = std::env::args_os().collect();
    if raw_args.get(1).is_some_and(|argument| argument == "--resource-child") {
        resource::child(raw_args.into_iter().skip(2).collect());
    }
    let args: Result<Vec<String>, AppError> = raw_args
        .iter()
        .skip(1)
        .cloned()
        .map(|argument| {
            argument.into_string().map_err(|_| {
                AppError::Usage("arguments must be valid UTF-8".to_string())
            })
        })
        .collect();
    let result = args.and_then(run);
    match result {
        Ok(()) => {}
        Err(AppError::Usage(error)) => {
            eprintln!("benchmark: {error}\ntry `benchmark --help`");
            std::process::exit(2);
        }
        Err(AppError::Runtime(error)) => {
            eprintln!("benchmark: {error}");
            std::process::exit(1);
        }
    }
}

fn run(args: impl IntoIterator<Item = String>) -> Result<(), AppError> {
    let mut config = match Config::parse(args)? {
        ParseOutcome::Help => {
            print_help();
            return Ok(());
        }
        ParseOutcome::Run(config) => config,
    };
    config.resolve_executables().map_err(AppError::Runtime)?;

    let fixtures = match &config.single {
        Some(fixture) => {
            if !fixture.is_file() {
                return Err(AppError::Usage(format!(
                    "no such fixture file: {}",
                    fixture.display(),
                )));
            }
            let fixtures = vec![fixture.clone()];
            reject_duplicate_class_names(&fixtures).map_err(AppError::Runtime)?;
            fixtures
        }
        None => discover_fixtures(&config.fixtures_dir).map_err(AppError::Runtime)?,
    };

    if config.record {
        return record_goldens(&config, &fixtures, Path::new(&config.golden_dir))
            .map_err(AppError::Runtime);
    }

    if config.verify_instrumentation {
        let workload = measurement::load_workload(&fixtures).map_err(AppError::Runtime)?;
        measurement::instrumented_preflight(&workload).map_err(AppError::Runtime)?;
        let output = Command::new(&config.alloc_helper)
            .arg("--verify-only")
            .args(&fixtures)
            .output()
            .map_err(|error| {
                AppError::Runtime(format!(
                    "cannot run allocation instrumentation verification: {error}"
                ))
            })?;
        if !output.status.success() {
            return Err(AppError::Runtime(format!(
                "allocation instrumentation verification failed with status {}; stderr: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim(),
            )));
        }
        println!("instrumentation verification passed for {} fixtures", fixtures.len());
        return Ok(());
    }

    let javac_dir = PathBuf::from(&config.out_dir).join("javac");
    let njavac_dir = PathBuf::from(&config.out_dir).join("njavac");
    std::fs::create_dir_all(&javac_dir)
        .map_err(|error| AppError::Runtime(format!("cannot create {}: {error}", javac_dir.display())))?;
    std::fs::create_dir_all(&njavac_dir)
        .map_err(|error| AppError::Runtime(format!("cannot create {}: {error}", njavac_dir.display())))?;
    if !config.performance_enabled() {
        correctness(&config, &fixtures, &javac_dir, &njavac_dir).map_err(AppError::Runtime)?;
        println!("correctness only; no report generated");
        return Ok(());
    }
    if std::env::var_os("NJAVAC_IN_CONTAINER").is_none()
        && std::env::var_os("NJAVAC_BENCHMARK_ALLOW_HOST").is_none()
    {
        println!("performance skipped outside the controlled Docker harness; no report generated");
        return Ok(());
    }
    if let Some(path) = config.json_path.as_deref() {
        report::preflight_destination(path).map_err(AppError::Runtime)?;
    }
    let context = report::collect_context(&config).map_err(AppError::Runtime)?;
    let workload = measurement::load_workload(&fixtures).map_err(AppError::Runtime)?;
    let measurements = measurement::run(&config, &workload, &javac_dir, &njavac_dir)
        .map_err(AppError::Runtime)?;
    let document = ReportDocument::new(
        context.metadata,
        context.provenance,
        workload.identity,
        context.configuration,
        measurements,
    )
    .map_err(AppError::Runtime)?;
    report::print_and_publish(&document, config.json_path.as_deref()).map_err(AppError::Runtime)
}

fn print_help() {
    println!(
        "benchmark - controlled performance report and internal fixture harness\n\n\
         USAGE:\n  benchmark [OPTIONS] [FILE.java]\n\n\
         MODES:\n  (default)         controlled performance/resource report; no correctness gate\n  FILE.java         focused correctness only; no report\n  --no-performance  complete live correctness only; no report\n  --offline         compare against --golden-dir without javac; no report\n  --record          record the complete fixture corpus; no positional FILE\n  --verify-instrumentation  deterministic observer/helper equivalence check\n\n\
         PERFORMANCE CONTROLS (complete report mode only):\n  --samples N             measured samples (default: 5; positive)\n  --warmup N              untimed warm-ups (default: 2; may be zero)\n  --rounds N              hot and phase corpus rounds/sample (default: 100; positive)\n  --allocation-rounds N   allocation corpus rounds (default: 1; positive)\n  --json PATH             atomically publish a no-clobber JSON report\n\n\
         PATHS:\n  --fixtures DIR          fixture root (default: fixtures)\n  --out-dir DIR           temporary class output root (default: /tmp/njavac-benchmark)\n  --golden-dir DIR        cache used by --record/--offline\n  --javac PATH            reference compiler\n  --javap PATH            mismatch disassembler\n  --njavac PATH           candidate compiler\n  --alloc-helper PATH     allocation helper\n\n\
         INTERACTIONS AND STATUS:\n  Non-performance modes are mutually exclusive.\n  --json and explicit performance controls require performance-report mode.\n  Usage errors exit 2; correctness, measurement, and publication failures exit 1."
    );
}

#[cfg(test)]
mod tests {
    use super::{AppError, Config, ParseOutcome, reject_duplicate_class_names};
    use std::path::PathBuf;

    fn parse(arguments: &[&str]) -> Result<ParseOutcome, AppError> {
        Config::parse(arguments.iter().map(|argument| argument.to_string()))
    }

    #[test]
    fn parser_rejects_bad_numbers_duplicate_positionals_and_incompatible_modes() {
        for arguments in [
            vec!["--samples", "0"],
            vec!["--rounds", "bad"],
            vec!["--allocation-rounds", "184467440737095516160"],
            vec!["--samples"],
            vec!["A.java", "B.java"],
            vec!["--record", "--offline"],
            vec!["--verify-instrumentation", "--offline"],
            vec!["--record", "A.java"],
            vec!["--offline", "--json", "x.json"],
            vec!["A.java", "--samples", "1"],
        ] {
            assert!(matches!(parse(&arguments), Err(AppError::Usage(_))), "{arguments:?}");
        }
        assert!(matches!(parse(&["--warmup", "0"]), Ok(ParseOutcome::Run(_))));
        assert!(matches!(parse(&["--help"]), Ok(ParseOutcome::Help)));
    }

    #[test]
    fn duplicate_class_basenames_are_rejected() {
        assert!(reject_duplicate_class_names(&[
            PathBuf::from("a/Same.java"),
            PathBuf::from("b/Same.java"),
        ])
        .is_err());
    }
}
