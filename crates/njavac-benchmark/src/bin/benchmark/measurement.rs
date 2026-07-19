use std::ffi::OsString;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use njavac_compiler::{CompileObserver, CompilePhase};

use super::Config;
use super::correctness::remove_if_exists;
use super::model::{
    AllocationMetric, AllocationProfile, Measurements, PerformanceMeasurements, PhaseProfile,
    PhaseSample, ProcessScenario, ResourceSeries, ScalarSeries, WorkloadIdentity,
};
use super::phase::{PhaseName, PhaseValues, SequenceValidator};
use super::resource::{self, InvocationContext};

#[derive(Clone)]
pub(super) struct Fixture {
    pub path: PathBuf,
    pub source: String,
    pub source_file: String,
    pub class_name: String,
    pub baseline_output: Vec<u8>,
}

pub(super) struct Workload {
    pub fixtures: Vec<Fixture>,
    pub identity: WorkloadIdentity,
    startup_index: usize,
}

pub(super) fn load_workload(paths: &[PathBuf]) -> Result<Workload, String> {
    let mut fixtures = Vec::with_capacity(paths.len());
    let mut source_bytes = 0_u64;
    let mut physical_lines = 0_u64;
    let mut nonblank_lines = 0_u64;
    let mut hash = 0xcbf29ce484222325_u64;

    for path in paths {
        let source = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let source_file = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{} has no UTF-8 filename", path.display()))?
            .to_string();
        let class_name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("{} has no UTF-8 class basename", path.display()))?
            .to_string();
        source_bytes = source_bytes
            .checked_add(source.len() as u64)
            .ok_or("source byte count overflow")?;
        physical_lines = physical_lines
            .checked_add(line_count(&source) as u64)
            .ok_or("physical line count overflow")?;
        nonblank_lines = nonblank_lines
            .checked_add(
                source
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .count() as u64,
            )
            .ok_or("nonblank line count overflow")?;
        hash_bytes(&mut hash, path.to_string_lossy().as_bytes());
        hash_bytes(&mut hash, &[0]);
        hash_bytes(&mut hash, source.as_bytes());
        hash_bytes(&mut hash, &[0xff]);
        let baseline_output = njavac::compile(&source, &source_file)
            .map_err(|diagnostic| diagnostic.render(&path.display().to_string(), &source))?;
        fixtures.push(Fixture {
            path: path.clone(),
            source,
            source_file,
            class_name,
            baseline_output,
        });
    }

    let output_class_bytes = fixtures.iter().try_fold(0_u64, |total, fixture| {
        total
            .checked_add(fixture.baseline_output.len() as u64)
            .ok_or("class output byte count overflow")
    })?;
    let startup_index = fixtures
        .iter()
        .position(|fixture| fixture.path.ends_with("basics/Empty.java"))
        .unwrap_or(0);
    let startup = &fixtures[startup_index];
    Ok(Workload {
        identity: WorkloadIdentity {
            fingerprint: format!("fnv1a64:{hash:016x}"),
            files: fixtures.len() as u64,
            source_bytes,
            physical_lines,
            nonblank_lines,
            output_class_bytes,
            minimal_input_fixture: startup.path.display().to_string(),
            minimal_input_source_bytes: startup.source.len() as u64,
            minimal_input_physical_lines: line_count(&startup.source) as u64,
            minimal_input_output_bytes: startup.baseline_output.len() as u64,
        },
        fixtures,
        startup_index,
    })
}

pub(super) fn run(
    cfg: &Config,
    workload: &Workload,
    javac_dir: &Path,
    njavac_dir: &Path,
) -> Result<Measurements, String> {
    println!("performance measurement (uninstrumented)");
    let performance = performance_measurements(cfg, workload, javac_dir, njavac_dir)?;

    println!("\nphase attribution (instrumented production pipeline)");
    let phase_profile = phase_profile(cfg, workload)?;

    println!("\nallocation attribution (separate instrumented helper)");
    let allocations = allocation_profile(cfg, workload)?;

    Ok(Measurements {
        performance,
        phase_profile,
        allocations,
    })
}

pub(super) fn instrumented_preflight(workload: &Workload) -> Result<(), String> {
    for fixture in &workload.fixtures {
        let ordinary =
            njavac::compile(&fixture.source, &fixture.source_file).map_err(|diagnostic| {
                diagnostic.render(&fixture.path.display().to_string(), &fixture.source)
            })?;
        if ordinary != fixture.baseline_output {
            return Err(format!(
                "ordinary compile preflight differs from the loaded baseline for {}",
                fixture.path.display(),
            ));
        }
        let mut observer = TimingObserver::new();
        let observed =
            njavac_compiler::compile_observed(&fixture.source, &fixture.source_file, &mut observer)
                .map_err(|diagnostic| {
                    diagnostic.render(&fixture.path.display().to_string(), &fixture.source)
                })?;
        observer.complete_compile()?;
        if observed != ordinary {
            return Err(format!(
                "timing-observed compile differs from ordinary compile for {}",
                fixture.path.display(),
            ));
        }
        observer.drop_result(observed);
    }
    println!(
        "  ordinary and timing-observed output match {} fixtures",
        workload.fixtures.len()
    );
    Ok(())
}

fn performance_measurements(
    cfg: &Config,
    workload: &Workload,
    javac_dir: &Path,
    njavac_dir: &Path,
) -> Result<PerformanceMeasurements, String> {
    let minimal_input_fresh_process_compile = process_scenario(
        cfg,
        "minimal-input fresh-process compile",
        std::slice::from_ref(&workload.fixtures[workload.startup_index]),
        javac_dir,
        njavac_dir,
    )?;
    let whole_corpus_cli_compile = process_scenario(
        cfg,
        "whole-corpus CLI compile",
        &workload.fixtures,
        javac_dir,
        njavac_dir,
    )?;
    let hot_in_process_corpus_compile = hot_series(cfg, workload)?;
    Ok(PerformanceMeasurements {
        minimal_input_fresh_process_compile,
        whole_corpus_cli_compile,
        hot_in_process_corpus_compile,
    })
}

fn process_scenario(
    cfg: &Config,
    scenario: &str,
    fixtures: &[Fixture],
    javac_dir: &Path,
    njavac_dir: &Path,
) -> Result<ProcessScenario, String> {
    println!("  {scenario} ({} files)", fixtures.len());
    let javac_command = compiler_command(&cfg.javac, javac_dir, fixtures);
    let njavac_command = compiler_command(&cfg.njavac, njavac_dir, fixtures);

    for warmup in 0..cfg.warmup {
        let iteration = format!("warm-up {}/{}", warmup + 1, cfg.warmup);
        if warmup % 2 == 0 {
            measure_compiler(
                &javac_command,
                javac_dir,
                fixtures,
                &InvocationContext {
                    compiler: "javac".into(),
                    scenario: scenario.into(),
                    iteration: iteration.clone(),
                },
            )?;
            measure_compiler(
                &njavac_command,
                njavac_dir,
                fixtures,
                &InvocationContext {
                    compiler: "njavac".into(),
                    scenario: scenario.into(),
                    iteration,
                },
            )?;
        } else {
            measure_compiler(
                &njavac_command,
                njavac_dir,
                fixtures,
                &InvocationContext {
                    compiler: "njavac".into(),
                    scenario: scenario.into(),
                    iteration: iteration.clone(),
                },
            )?;
            measure_compiler(
                &javac_command,
                javac_dir,
                fixtures,
                &InvocationContext {
                    compiler: "javac".into(),
                    scenario: scenario.into(),
                    iteration,
                },
            )?;
        }
    }

    let mut javac_samples = Vec::with_capacity(cfg.samples);
    let mut njavac_samples = Vec::with_capacity(cfg.samples);
    for sample in 0..cfg.samples {
        let iteration = format!("sample {}/{}", sample + 1, cfg.samples);
        if sample % 2 == 0 {
            javac_samples.push(measure_compiler(
                &javac_command,
                javac_dir,
                fixtures,
                &InvocationContext {
                    compiler: "javac".into(),
                    scenario: scenario.into(),
                    iteration: iteration.clone(),
                },
            )?);
            njavac_samples.push(measure_compiler(
                &njavac_command,
                njavac_dir,
                fixtures,
                &InvocationContext {
                    compiler: "njavac".into(),
                    scenario: scenario.into(),
                    iteration,
                },
            )?);
        } else {
            njavac_samples.push(measure_compiler(
                &njavac_command,
                njavac_dir,
                fixtures,
                &InvocationContext {
                    compiler: "njavac".into(),
                    scenario: scenario.into(),
                    iteration: iteration.clone(),
                },
            )?);
            javac_samples.push(measure_compiler(
                &javac_command,
                javac_dir,
                fixtures,
                &InvocationContext {
                    compiler: "javac".into(),
                    scenario: scenario.into(),
                    iteration,
                },
            )?);
        }
    }

    Ok(ProcessScenario {
        files: fixtures.len() as u64,
        source_bytes: fixtures
            .iter()
            .map(|fixture| fixture.source.len() as u64)
            .sum(),
        physical_lines: fixtures
            .iter()
            .map(|fixture| line_count(&fixture.source) as u64)
            .sum(),
        output_bytes: fixtures
            .iter()
            .map(|fixture| fixture.baseline_output.len() as u64)
            .sum(),
        javac: ResourceSeries {
            samples: javac_samples,
        },
        njavac: ResourceSeries {
            samples: njavac_samples,
        },
    })
}

fn compiler_command(compiler: &str, out_dir: &Path, fixtures: &[Fixture]) -> Vec<OsString> {
    let mut command = vec![
        OsString::from(compiler),
        OsString::from("-d"),
        out_dir.as_os_str().into(),
    ];
    command.extend(
        fixtures
            .iter()
            .map(|fixture| fixture.path.as_os_str().into()),
    );
    command
}

fn measure_compiler(
    command: &[OsString],
    out_dir: &Path,
    fixtures: &[Fixture],
    context: &InvocationContext,
) -> Result<super::model::ResourceSample, String> {
    for fixture in fixtures {
        remove_if_exists(&out_dir.join(format!("{}.class", fixture.class_name)))?;
    }
    let sample = resource::measure(command, context)?;
    for fixture in fixtures {
        let path = out_dir.join(format!("{}.class", fixture.class_name));
        std::fs::read(&path).map_err(|error| {
            format!(
                "{} {} {} omitted {}: {error}; command: {}",
                context.compiler,
                context.scenario,
                context.iteration,
                path.display(),
                resource::format_command(command),
            )
        })?;
    }
    Ok(sample)
}

fn hot_series(cfg: &Config, workload: &Workload) -> Result<ScalarSeries, String> {
    for _ in 0..cfg.warmup {
        compile_round(workload)?;
    }
    let mut samples_ns = Vec::with_capacity(cfg.samples);
    for sample in 0..cfg.samples {
        let start = Instant::now();
        for _ in 0..cfg.rounds {
            compile_round(workload)?;
        }
        samples_ns.push(duration_ns(start.elapsed()));
        println!("  hot sample {}/{} complete", sample + 1, cfg.samples);
    }
    Ok(ScalarSeries { samples_ns })
}

fn compile_round(workload: &Workload) -> Result<(), String> {
    for fixture in &workload.fixtures {
        let bytes =
            njavac::compile(&fixture.source, &fixture.source_file).map_err(|diagnostic| {
                diagnostic.render(&fixture.path.display().to_string(), &fixture.source)
            })?;
        black_box(bytes);
    }
    Ok(())
}

struct TimingObserver {
    starts: PhaseValues<Option<Instant>>,
    elapsed: PhaseValues<Duration>,
    sequence: SequenceValidator,
}

impl TimingObserver {
    fn new() -> Self {
        Self {
            starts: PhaseValues::default(),
            elapsed: PhaseValues::default(),
            sequence: SequenceValidator::default(),
        }
    }

    fn complete_compile(&mut self) -> Result<(), String> {
        self.sequence
            .complete_success()
            .map_err(|error| error.to_string())
    }

    fn drop_result(&mut self, bytes: Vec<u8>) {
        black_box(&bytes);
        let start = Instant::now();
        drop(bytes);
        self.elapsed.result_bytes_drop += start.elapsed();
    }

    fn durations_ns(&self) -> PhaseValues<u64> {
        PhaseValues {
            lex: duration_ns(self.elapsed.lex),
            parse: duration_ns(self.elapsed.parse),
            semantic_analysis: duration_ns(self.elapsed.semantic_analysis),
            codegen_planning: duration_ns(self.elapsed.codegen_planning),
            classfile_serialization_and_plan_drop: duration_ns(
                self.elapsed.classfile_serialization_and_plan_drop,
            ),
            analysis_and_syntax_drop: duration_ns(self.elapsed.analysis_and_syntax_drop),
            result_bytes_drop: duration_ns(self.elapsed.result_bytes_drop),
        }
    }
}

impl CompileObserver for TimingObserver {
    fn phase_started(&mut self, phase: CompilePhase) {
        self.sequence.started(phase);
        let start = self.starts.get_mut(PhaseName::from(phase));
        if start.is_none() {
            *start = Some(Instant::now());
        }
    }

    fn phase_finished(&mut self, phase: CompilePhase) {
        self.sequence.finished(phase);
        let phase = PhaseName::from(phase);
        if let Some(start) = self.starts.get_mut(phase).take() {
            *self.elapsed.get_mut(phase) += start.elapsed();
        }
    }
}

fn phase_profile(cfg: &Config, workload: &Workload) -> Result<PhaseProfile, String> {
    let mut samples = Vec::with_capacity(cfg.samples);
    for sample in 0..cfg.samples {
        let mut observer = TimingObserver::new();
        let start = Instant::now();
        for _ in 0..cfg.rounds {
            for fixture in &workload.fixtures {
                let bytes = njavac_compiler::compile_observed(
                    &fixture.source,
                    &fixture.source_file,
                    &mut observer,
                )
                .map_err(|diagnostic| {
                    diagnostic.render(&fixture.path.display().to_string(), &fixture.source)
                })?;
                observer.complete_compile()?;
                observer.drop_result(bytes);
            }
        }
        let phases_ns = observer.durations_ns();
        let wall_ns = duration_ns(start.elapsed());
        let attributed = PhaseName::ALL
            .into_iter()
            .try_fold(0_u64, |total, phase| {
                total.checked_add(*phases_ns.get(phase))
            })
            .ok_or("phase duration sum overflow")?;
        let unattributed_wall_ns = wall_ns.checked_sub(attributed).ok_or_else(|| {
            format!("attributed phase time {attributed}ns exceeded profile wall time {wall_ns}ns")
        })?;
        println!("  phase sample {}/{} complete", sample + 1, cfg.samples);
        samples.push(PhaseSample {
            wall_ns,
            phases_ns,
            unattributed_wall_ns,
        });
    }
    Ok(PhaseProfile { samples })
}

fn allocation_profile(cfg: &Config, workload: &Workload) -> Result<AllocationProfile, String> {
    let mut command = Command::new(&cfg.alloc_helper);
    command.arg(cfg.allocation_rounds.to_string());
    command.args(workload.fixtures.iter().map(|fixture| &fixture.path));
    let output = command.output().map_err(|error| {
        format!(
            "cannot run allocation helper {}: {error}; command: {:?}",
            cfg.alloc_helper, command,
        )
    })?;
    if !output.status.success() {
        return Err(format!(
            "allocation helper failed with status {}; command: {:?}; stderr: {}",
            output.status,
            command,
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    parse_allocation_profile(
        &String::from_utf8_lossy(&output.stdout),
        workload.fixtures.len(),
    )
}

fn parse_allocation_profile(
    output: &str,
    expected_fixtures: usize,
) -> Result<AllocationProfile, String> {
    let mut phases: PhaseValues<Option<AllocationMetric>> = PhaseValues::default();
    let mut baseline_live_bytes = None;
    let mut peak_live_growth_bytes = None;
    let mut final_live_bytes = None;
    let mut total = None;
    for line in output.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["phase", name, calls, requested, released] => {
                let phase = PhaseName::from_protocol(name)
                    .ok_or_else(|| format!("unknown allocation phase: {name}"))?;
                let slot = phases.get_mut(phase);
                if slot.is_some() {
                    return Err(format!("duplicate allocation phase: {name}"));
                }
                *slot = Some(AllocationMetric {
                    allocation_calls: parse_u64(calls, line)?,
                    requested_bytes: parse_u64(requested, line)?,
                    released_bytes: parse_u64(released, line)?,
                });
            }
            ["baseline_live", bytes] => set_once(
                &mut baseline_live_bytes,
                parse_u64(bytes, line)?,
                "baseline_live",
            )?,
            ["peak_live_growth", bytes] => set_once(
                &mut peak_live_growth_bytes,
                parse_u64(bytes, line)?,
                "peak_live_growth",
            )?,
            ["final_live", bytes] => {
                set_once(&mut final_live_bytes, parse_u64(bytes, line)?, "final_live")?
            }
            ["total", requested, released] => set_once(
                &mut total,
                (parse_u64(requested, line)?, parse_u64(released, line)?),
                "total",
            )?,
            _ => return Err(format!("invalid allocation-helper output: {line}")),
        }
    }
    let _ = expected_fixtures;
    let phases =
        phases.try_map(|metric| metric.ok_or("allocation helper omitted a named phase"))?;
    let baseline_live_bytes =
        baseline_live_bytes.ok_or("allocation helper omitted baseline_live")?;
    let final_live_bytes = final_live_bytes.ok_or("allocation helper omitted final_live")?;
    if final_live_bytes != baseline_live_bytes {
        return Err(format!(
            "allocation helper final live bytes {final_live_bytes} differ from baseline {baseline_live_bytes}"
        ));
    }
    let (total_requested_bytes, total_released_bytes) =
        total.ok_or("allocation helper omitted total")?;
    if total_requested_bytes != total_released_bytes {
        return Err(format!(
            "allocation requested total {total_requested_bytes} differs from released total {total_released_bytes}"
        ));
    }
    let phase_requested = PhaseName::ALL.into_iter().try_fold(0_u64, |sum, phase| {
        sum.checked_add(phases.get(phase).requested_bytes)
    });
    let phase_released = PhaseName::ALL.into_iter().try_fold(0_u64, |sum, phase| {
        sum.checked_add(phases.get(phase).released_bytes)
    });
    if phase_requested != Some(total_requested_bytes)
        || phase_released != Some(total_released_bytes)
    {
        return Err(
            "allocation named-phase totals differ from complete workload totals".to_string(),
        );
    }
    Ok(AllocationProfile {
        phases,
        baseline_live_bytes,
        peak_live_growth_bytes: peak_live_growth_bytes
            .ok_or("allocation helper omitted peak_live_growth")?,
        final_live_bytes,
        total_requested_bytes,
        total_released_bytes,
    })
}

fn parse_u64(value: &str, line: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|_| format!("invalid numeric allocation field: {line}"))
}

fn set_once<T>(slot: &mut Option<T>, value: T, name: &str) -> Result<(), String> {
    if slot.replace(value).is_some() {
        Err(format!("allocation helper emitted duplicate {name}"))
    } else {
        Ok(())
    }
}

fn line_count(source: &str) -> usize {
    if source.is_empty() {
        0
    } else {
        source.bytes().filter(|&byte| byte == b'\n').count() + usize::from(!source.ends_with('\n'))
    }
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for &byte in bytes {
        *hash ^= byte as u64;
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::{Fixture, Workload, instrumented_preflight, parse_allocation_profile};
    use crate::model::WorkloadIdentity;

    const PHASES: &str = "phase\tlex\t1\t2\t2\nphase\tparse\t1\t2\t2\nphase\tsemantic_analysis\t1\t2\t2\nphase\tcodegen_planning\t1\t2\t2\nphase\tclassfile_serialization_and_plan_drop\t1\t2\t2\nphase\tanalysis_and_syntax_drop\t1\t2\t2\nphase\tresult_bytes_drop\t1\t2\t2\n";

    #[test]
    fn allocation_protocol_requires_named_complete_balanced_output() {
        let valid = format!(
            "{PHASES}baseline_live\t10\npeak_live_growth\t20\nfinal_live\t10\ntotal\t14\t14\n"
        );
        assert!(parse_allocation_profile(&valid, 2).is_ok());
        assert!(
            parse_allocation_profile(&valid.replace("final_live\t10", "final_live\t11"), 2)
                .is_err()
        );
        assert!(
            parse_allocation_profile(&valid.replace("phase\tlex", "phase\tunknown"), 2).is_err()
        );
        assert!(
            parse_allocation_profile(&valid.replace("phase\tparse\t1\t2\t2\n", ""), 2).is_err()
        );
        assert!(parse_allocation_profile(&(valid.clone() + "baseline_live\t10\n"), 2).is_err());
        assert!(
            parse_allocation_profile(&valid.replace("total\t14\t14", "total\t14\t13"), 2).is_err()
        );
    }

    #[test]
    fn timing_observer_preflight_matches_ordinary_output() {
        let source = "public class X { public static void main(String[] args) {} }\n";
        let baseline_output = njavac::compile(source, "X.java").unwrap();
        let workload = Workload {
            fixtures: vec![Fixture {
                path: "X.java".into(),
                source: source.to_string(),
                source_file: "X.java".to_string(),
                class_name: "X".to_string(),
                baseline_output,
            }],
            identity: WorkloadIdentity {
                fingerprint: "test".into(),
                files: 1,
                source_bytes: source.len() as u64,
                physical_lines: 1,
                nonblank_lines: 1,
                output_class_bytes: 0,
                minimal_input_fixture: "X.java".into(),
                minimal_input_source_bytes: source.len() as u64,
                minimal_input_physical_lines: 1,
                minimal_input_output_bytes: 0,
            },
            startup_index: 0,
        };
        assert!(instrumented_preflight(&workload).is_ok());
    }
}
