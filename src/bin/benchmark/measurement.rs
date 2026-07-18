use std::ffi::OsString;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use njavac::{CompileObserver, CompilePhase};

use super::Config;

pub(super) const PHASE_NAMES: [&str; 7] = [
    "lex",
    "parse",
    "sema",
    "codegen plan",
    "classfile emit",
    "cleanup",
    "result drop",
];

#[derive(Clone)]
pub(super) struct Fixture {
    pub path: PathBuf,
    pub source: String,
    pub source_file: String,
    pub expected_output: Vec<u8>,
}

pub(super) struct Workload {
    pub fixtures: Vec<Fixture>,
    pub fingerprint: String,
    pub source_bytes: usize,
    pub physical_lines: usize,
    pub nonblank_lines: usize,
    pub output_class_bytes: usize,
}

#[derive(Clone)]
pub(super) struct Summary {
    pub min: f64,
    pub median: f64,
    pub mean: f64,
    pub stddev: f64,
    pub mad: f64,
}

#[derive(Clone)]
pub(super) struct ResourceSample {
    pub wall_ns: u64,
    pub user_us: u64,
    pub system_us: u64,
    pub max_rss_kb: u64,
    pub minor_faults: u64,
    pub major_faults: u64,
    pub voluntary_switches: u64,
    pub involuntary_switches: u64,
}

pub(super) struct ResourceSeries {
    pub samples: Vec<ResourceSample>,
}

pub(super) struct ProcessScenario {
    pub name: &'static str,
    pub files: usize,
    pub source_bytes: usize,
    pub physical_lines: usize,
    pub output_bytes: usize,
    pub javac: ResourceSeries,
    pub njavac: ResourceSeries,
}

pub(super) struct ScalarSeries {
    pub samples_ns: Vec<u64>,
}

pub(super) struct PerformancePass {
    pub startup: ProcessScenario,
    pub batch: ProcessScenario,
    pub hot: ScalarSeries,
}

pub(super) struct PhaseSample {
    pub wall_ns: u64,
    pub phases_ns: [u64; 7],
}

pub(super) struct PhaseProfile {
    pub samples: Vec<PhaseSample>,
}

#[derive(Clone, Copy, Default)]
pub(super) struct AllocationMetric {
    pub calls: u64,
    pub bytes: u64,
    pub deallocated_bytes: u64,
}

pub(super) struct AllocationProfile {
    pub phases: [AllocationMetric; 7],
    pub peak_live_bytes: u64,
    pub final_live_bytes: u64,
}

pub(super) struct BenchmarkReport {
    pub performance: PerformancePass,
    pub phases: PhaseProfile,
    pub allocations: AllocationProfile,
}

impl ScalarSeries {
    pub fn summary_ms(&self) -> Summary {
        summary(self.samples_ns.iter().map(|&ns| ns as f64 / 1_000_000.0).collect())
    }
}

impl ResourceSeries {
    pub fn wall_summary_ms(&self) -> Summary {
        summary(
            self.samples
                .iter()
                .map(|sample| sample.wall_ns as f64 / 1_000_000.0)
                .collect(),
        )
    }

    pub fn median_cpu_ms(&self) -> f64 {
        median(
            self.samples
                .iter()
                .map(|sample| (sample.user_us + sample.system_us) as f64 / 1000.0)
                .collect(),
        )
    }

    pub fn median_max_rss_kb(&self) -> f64 {
        median(self.samples.iter().map(|sample| sample.max_rss_kb as f64).collect())
    }
}

impl PhaseProfile {
    pub fn wall_summary_ms(&self) -> Summary {
        summary(
            self.samples
                .iter()
                .map(|sample| sample.wall_ns as f64 / 1_000_000.0)
                .collect(),
        )
    }

    pub fn median_phase_ns(&self, phase: usize) -> f64 {
        median(
            self.samples
                .iter()
                .map(|sample| sample.phases_ns[phase] as f64)
                .collect(),
        )
    }
}

pub(super) fn load_workload(paths: &[PathBuf], output_dir: &Path) -> Result<Workload, String> {
    let mut fixtures = Vec::with_capacity(paths.len());
    let mut source_bytes = 0;
    let mut physical_lines = 0;
    let mut nonblank_lines = 0;
    let mut hash = 0xcbf29ce484222325_u64;

    for path in paths {
        let source = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let source_file = path
            .file_name()
            .ok_or_else(|| format!("{} has no filename", path.display()))?
            .to_string_lossy()
            .into_owned();

        source_bytes += source.len();
        physical_lines += line_count(&source);
        nonblank_lines += source.lines().filter(|line| !line.trim().is_empty()).count();
        hash_bytes(&mut hash, path.to_string_lossy().as_bytes());
        hash_bytes(&mut hash, &[0]);
        hash_bytes(&mut hash, source.as_bytes());
        hash_bytes(&mut hash, &[0xff]);
        let class_name = path.file_stem().unwrap().to_string_lossy();
        let class_path = output_dir.join(format!("{class_name}.class"));
        let expected_output = std::fs::read(&class_path)
            .map_err(|error| format!("cannot inspect {}: {error}", class_path.display()))?;
        fixtures.push(Fixture { path: path.clone(), source, source_file, expected_output });
    }

    let output_class_bytes = fixtures.iter().map(|fixture| fixture.expected_output.len()).sum();

    Ok(Workload {
        fixtures,
        fingerprint: format!("fnv1a64:{hash:016x}"),
        source_bytes,
        physical_lines,
        nonblank_lines,
        output_class_bytes,
    })
}

pub(super) fn run(
    cfg: &Config,
    workload: &Workload,
    javac_dir: &Path,
    njavac_dir: &Path,
) -> Result<BenchmarkReport, String> {
    println!("performance pass (uninstrumented)");
    let performance = performance_pass(cfg, workload, javac_dir, njavac_dir)?;

    println!("\nphase pass (instrumented production pipeline)");
    let phases = phase_profile(cfg, workload)?;

    println!("\nallocation pass (separate instrumented helper)");
    let allocations = allocation_profile(cfg, workload)?;

    Ok(BenchmarkReport { performance, phases, allocations })
}

fn performance_pass(
    cfg: &Config,
    workload: &Workload,
    javac_dir: &Path,
    njavac_dir: &Path,
) -> Result<PerformancePass, String> {
    let startup_index = workload
        .fixtures
        .iter()
        .position(|fixture| fixture.path.ends_with("basics/Empty.java"))
        .unwrap_or(0);
    let startup_fixture = &workload.fixtures[startup_index];
    let startup = process_scenario(
        cfg,
        "fresh-process startup",
        std::slice::from_ref(startup_fixture),
        javac_dir,
        njavac_dir,
    )?;
    print_process_scenario(&startup);

    let batch = process_scenario(
        cfg,
        "end-to-end batch",
        &workload.fixtures,
        javac_dir,
        njavac_dir,
    )?;
    print_process_scenario(&batch);

    let hot = hot_series(cfg, workload)?;
    let hot_summary = hot.summary_ms();
    println!(
        "  hot compiler core  min {:8.3}  median {:8.3}  mean {:8.3}  stddev {:7.3}  MAD {:7.3} ms/sample  n={}",
        hot_summary.min,
        hot_summary.median,
        hot_summary.mean,
        hot_summary.stddev,
        hot_summary.mad,
        hot.samples_ns.len(),
    );
    Ok(PerformancePass { startup, batch, hot })
}

fn process_scenario(
    cfg: &Config,
    name: &'static str,
    fixtures: &[Fixture],
    javac_dir: &Path,
    njavac_dir: &Path,
) -> Result<ProcessScenario, String> {
    let javac = compiler_command(&cfg.javac, javac_dir, fixtures);
    let njavac = compiler_command(&cfg.njavac, njavac_dir, fixtures);

    for warmup in 0..cfg.warmup {
        if warmup % 2 == 0 {
            measure_compiler(&javac, javac_dir, fixtures)?;
            measure_compiler(&njavac, njavac_dir, fixtures)?;
        } else {
            measure_compiler(&njavac, njavac_dir, fixtures)?;
            measure_compiler(&javac, javac_dir, fixtures)?;
        }
    }

    let mut javac_samples = Vec::with_capacity(cfg.samples);
    let mut njavac_samples = Vec::with_capacity(cfg.samples);
    for sample in 0..cfg.samples {
        if sample % 2 == 0 {
            javac_samples.push(measure_compiler(&javac, javac_dir, fixtures)?);
            njavac_samples.push(measure_compiler(&njavac, njavac_dir, fixtures)?);
        } else {
            njavac_samples.push(measure_compiler(&njavac, njavac_dir, fixtures)?);
            javac_samples.push(measure_compiler(&javac, javac_dir, fixtures)?);
        }
    }

    Ok(ProcessScenario {
        name,
        files: fixtures.len(),
        source_bytes: fixtures.iter().map(|fixture| fixture.source.len()).sum(),
        physical_lines: fixtures.iter().map(|fixture| line_count(&fixture.source)).sum(),
        output_bytes: fixtures.iter().map(|fixture| fixture.expected_output.len()).sum(),
        javac: ResourceSeries { samples: javac_samples },
        njavac: ResourceSeries { samples: njavac_samples },
    })
}

fn compiler_command(compiler: &str, out_dir: &Path, fixtures: &[Fixture]) -> Vec<OsString> {
    let mut command = vec![OsString::from(compiler), OsString::from("-d"), out_dir.as_os_str().into()];
    command.extend(fixtures.iter().map(|fixture| fixture.path.as_os_str().into()));
    command
}

fn measure_compiler(
    command: &[OsString],
    out_dir: &Path,
    fixtures: &[Fixture],
) -> Result<ResourceSample, String> {
    for fixture in fixtures {
        let class_name = fixture.path.file_stem().unwrap().to_string_lossy();
        let path = out_dir.join(format!("{class_name}.class"));
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot clear {}: {error}", path.display())),
        }
    }

    let sample = measure_resource(command)?;
    for fixture in fixtures {
        let class_name = fixture.path.file_stem().unwrap().to_string_lossy();
        let path = out_dir.join(format!("{class_name}.class"));
        let actual = std::fs::read(&path)
            .map_err(|error| format!("timed compiler omitted {}: {error}", path.display()))?;
        if actual != fixture.expected_output {
            return Err(format!(
                "timed compiler wrote unexpected bytes to {}",
                path.display(),
            ));
        }
    }
    Ok(sample)
}

fn measure_resource(command: &[OsString]) -> Result<ResourceSample, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("cannot locate benchmark executable: {error}"))?;
    let output = Command::new(executable)
        .arg("--resource-child")
        .args(command)
        .output()
        .map_err(|error| format!("cannot start resource helper: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "resource helper failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_resource_sample(&String::from_utf8_lossy(&output.stdout))
}

fn parse_resource_sample(line: &str) -> Result<ResourceSample, String> {
    let fields: Vec<&str> = line.trim().split('\t').collect();
    if fields.len() != 9 {
        return Err(format!("invalid resource-helper output: {line:?}"));
    }
    if fields[0] != "ok" {
        return Err(format!("timed compiler invocation failed with status {}", fields[0]));
    }
    let number = |index: usize| {
        fields[index]
            .parse::<u64>()
            .map_err(|error| format!("invalid resource field {}: {error}", fields[index]))
    };
    Ok(ResourceSample {
        wall_ns: number(1)?,
        user_us: number(2)?,
        system_us: number(3)?,
        max_rss_kb: number(4)?,
        minor_faults: number(5)?,
        major_faults: number(6)?,
        voluntary_switches: number(7)?,
        involuntary_switches: number(8)?,
    })
}

fn hot_series(cfg: &Config, workload: &Workload) -> Result<ScalarSeries, String> {
    for _ in 0..cfg.warmup {
        compile_round(workload)?;
    }
    let mut samples_ns = Vec::with_capacity(cfg.samples);
    for _ in 0..cfg.samples {
        let start = Instant::now();
        for _ in 0..cfg.rounds {
            compile_round(workload)?;
        }
        samples_ns.push(duration_ns(start.elapsed()));
    }
    Ok(ScalarSeries { samples_ns })
}

fn compile_round(workload: &Workload) -> Result<(), String> {
    for fixture in &workload.fixtures {
        let bytes = njavac::compile(&fixture.source, &fixture.source_file)
            .map_err(|diagnostic| diagnostic.render(&fixture.path.display().to_string(), &fixture.source))?;
        black_box(bytes);
    }
    Ok(())
}

struct TimingObserver {
    starts: [Option<Instant>; 7],
    elapsed: [Duration; 7],
}

impl TimingObserver {
    fn new() -> Self {
        Self { starts: [None; 7], elapsed: [Duration::ZERO; 7] }
    }
}

impl CompileObserver for TimingObserver {
    fn phase_started(&mut self, phase: CompilePhase) {
        self.starts[phase_index(phase)] = Some(Instant::now());
    }

    fn phase_finished(&mut self, phase: CompilePhase) {
        let index = phase_index(phase);
        self.elapsed[index] += self.starts[index].take().expect("phase start").elapsed();
    }
}

fn phase_profile(cfg: &Config, workload: &Workload) -> Result<PhaseProfile, String> {
    let mut samples = Vec::with_capacity(cfg.samples);
    for sample in 0..cfg.samples {
        let mut observer = TimingObserver::new();
        let start = Instant::now();
        for _ in 0..cfg.rounds {
            for fixture in &workload.fixtures {
                let bytes = njavac::compile_observed(
                    &fixture.source,
                    &fixture.source_file,
                    &mut observer,
                )
                .map_err(|diagnostic| {
                    diagnostic.render(&fixture.path.display().to_string(), &fixture.source)
                })?;
                observer.phase_started(CompilePhase::ResultDrop);
                black_box(bytes);
                observer.phase_finished(CompilePhase::ResultDrop);
            }
        }
        let phases_ns = observer.elapsed.map(duration_ns);
        let wall_ns = duration_ns(start.elapsed());
        println!(
            "  sample {}/{}  measured {:.3} ms",
            sample + 1,
            cfg.samples,
            wall_ns as f64 / 1_000_000.0,
        );
        samples.push(PhaseSample { wall_ns, phases_ns });
    }
    Ok(PhaseProfile { samples })
}

fn allocation_profile(cfg: &Config, workload: &Workload) -> Result<AllocationProfile, String> {
    let mut command = Command::new(&cfg.alloc_helper);
    command.arg(cfg.allocation_rounds.to_string());
    command.args(workload.fixtures.iter().map(|fixture| &fixture.path));
    let output = command
        .output()
        .map_err(|error| format!("cannot run allocation helper {}: {error}", cfg.alloc_helper))?;
    if !output.status.success() {
        return Err(format!(
            "allocation helper failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut phases = [AllocationMetric::default(); 7];
    let mut seen_phases = [false; 7];
    let mut peak_live_bytes = None;
    let mut final_live_bytes = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["phase", index, calls, bytes, deallocated_bytes] => {
                let index: usize = index.parse().map_err(|_| format!("invalid phase index: {line}"))?;
                if index >= phases.len() {
                    return Err(format!("invalid allocation phase: {line}"));
                }
                if seen_phases[index] {
                    return Err(format!("duplicate allocation phase: {line}"));
                }
                seen_phases[index] = true;
                phases[index] = AllocationMetric {
                    calls: calls.parse().map_err(|_| format!("invalid allocation calls: {line}"))?,
                    bytes: bytes.parse().map_err(|_| format!("invalid allocation bytes: {line}"))?,
                    deallocated_bytes: deallocated_bytes
                        .parse()
                        .map_err(|_| format!("invalid deallocation bytes: {line}"))?,
                };
            }
            ["peak", bytes] => {
                if peak_live_bytes.is_some() {
                    return Err("allocation helper emitted duplicate peak".to_string());
                }
                peak_live_bytes = Some(
                    bytes.parse().map_err(|_| format!("invalid peak allocation: {line}"))?,
                );
            }
            ["live", bytes] => {
                if final_live_bytes.is_some() {
                    return Err("allocation helper emitted duplicate final live value".to_string());
                }
                final_live_bytes = Some(
                    bytes.parse().map_err(|_| format!("invalid live allocation: {line}"))?,
                );
            }
            _ => return Err(format!("invalid allocation-helper output: {line}")),
        }
    }
    if let Some(index) = seen_phases.iter().position(|seen| !seen) {
        return Err(format!("allocation helper omitted phase {index}"));
    }
    let final_live_bytes = final_live_bytes.ok_or("allocation helper omitted final live bytes")?;
    if final_live_bytes != 0 {
        return Err(format!(
            "allocation helper retained {final_live_bytes} tracked bytes after the workload"
        ));
    }
    Ok(AllocationProfile {
        phases,
        peak_live_bytes: peak_live_bytes.ok_or("allocation helper omitted peak live bytes")?,
        final_live_bytes,
    })
}

fn print_process_scenario(scenario: &ProcessScenario) {
    println!("  {} ({} files)", scenario.name, scenario.files);
    for (name, series) in [("javac", &scenario.javac), ("njavac", &scenario.njavac)] {
        let wall = series.wall_summary_ms();
        println!(
            "    {name:7} min {:8.3}  median {:8.3}  mean {:8.3}  stddev {:7.3}  MAD {:7.3} ms",
            wall.min,
            wall.median,
            wall.mean,
            wall.stddev,
            wall.mad,
        );
        println!(
            "            CPU total {:7.3} ms  RSS {:8.0} KiB  n={}",
            series.median_cpu_ms(),
            series.median_max_rss_kb(),
            series.samples.len(),
        );
    }
}

pub(super) fn resource_child(args: Vec<OsString>) -> ! {
    if args.is_empty() {
        eprintln!("resource child requires a command");
        std::process::exit(2);
    }
    let before = child_usage().unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });
    let start = Instant::now();
    let status = Command::new(&args[0])
        .args(&args[1..])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let wall_ns = duration_ns(start.elapsed());
    let after = child_usage().unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });
    let success = status.is_ok_and(|status| status.success());
    println!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        if success { "ok" } else { "failed" },
        wall_ns,
        after.user_us.saturating_sub(before.user_us),
        after.system_us.saturating_sub(before.system_us),
        after.max_rss_kb,
        after.minor_faults.saturating_sub(before.minor_faults),
        after.major_faults.saturating_sub(before.major_faults),
        after.voluntary_switches.saturating_sub(before.voluntary_switches),
        after.involuntary_switches.saturating_sub(before.involuntary_switches),
    );
    std::process::exit(0);
}

#[derive(Default)]
struct ChildUsage {
    user_us: u64,
    system_us: u64,
    max_rss_kb: u64,
    minor_faults: u64,
    major_faults: u64,
    voluntary_switches: u64,
    involuntary_switches: u64,
}

#[cfg(target_os = "linux")]
fn child_usage() -> Result<ChildUsage, String> {
    use std::os::raw::{c_int, c_long};

    #[repr(C)]
    #[derive(Default)]
    struct TimeVal {
        seconds: c_long,
        microseconds: c_long,
    }

    #[repr(C)]
    #[derive(Default)]
    struct ResourceUsage {
        user: TimeVal,
        system: TimeVal,
        max_rss: c_long,
        shared_memory: c_long,
        unshared_data: c_long,
        unshared_stack: c_long,
        minor_faults: c_long,
        major_faults: c_long,
        swaps: c_long,
        block_inputs: c_long,
        block_outputs: c_long,
        messages_sent: c_long,
        messages_received: c_long,
        signals: c_long,
        voluntary_switches: c_long,
        involuntary_switches: c_long,
    }

    unsafe extern "C" {
        fn getrusage(who: c_int, usage: *mut ResourceUsage) -> c_int;
    }

    const RUSAGE_CHILDREN: c_int = -1;
    let mut usage = ResourceUsage::default();
    // Linux amd64 and arm64 use the glibc rusage layout modeled above.
    let result = unsafe { getrusage(RUSAGE_CHILDREN, &mut usage) };
    if result != 0 {
        return Err(format!("getrusage failed: {}", std::io::Error::last_os_error()));
    }
    let micros = |value: &TimeVal| {
        (value.seconds.max(0) as u64) * 1_000_000 + value.microseconds.max(0) as u64
    };
    Ok(ChildUsage {
        user_us: micros(&usage.user),
        system_us: micros(&usage.system),
        max_rss_kb: usage.max_rss.max(0) as u64,
        minor_faults: usage.minor_faults.max(0) as u64,
        major_faults: usage.major_faults.max(0) as u64,
        voluntary_switches: usage.voluntary_switches.max(0) as u64,
        involuntary_switches: usage.involuntary_switches.max(0) as u64,
    })
}

#[cfg(not(target_os = "linux"))]
fn child_usage() -> Result<ChildUsage, String> {
    Err("resource accounting requires Linux".to_string())
}

pub(super) fn summary(values: Vec<f64>) -> Summary {
    assert!(!values.is_empty(), "statistics require at least one sample");
    let mut sorted = values;
    sorted.sort_by(f64::total_cmp);
    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let variance = sorted.iter().map(|value| (value - mean).powi(2)).sum::<f64>()
        / sorted.len() as f64;
    let center = median_sorted(&sorted);
    let mut deviations: Vec<f64> = sorted.iter().map(|value| (value - center).abs()).collect();
    deviations.sort_by(f64::total_cmp);
    Summary {
        min: sorted[0],
        median: center,
        mean,
        stddev: variance.sqrt(),
        mad: median_sorted(&deviations),
    }
}

fn median(mut values: Vec<f64>) -> f64 {
    assert!(!values.is_empty(), "median requires at least one sample");
    values.sort_by(f64::total_cmp);
    median_sorted(&values)
}

fn median_sorted(values: &[f64]) -> f64 {
    if values.len() % 2 == 1 {
        values[values.len() / 2]
    } else {
        (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
    }
}

pub(super) fn phase_index(phase: CompilePhase) -> usize {
    match phase {
        CompilePhase::Lex => 0,
        CompilePhase::Parse => 1,
        CompilePhase::Sema => 2,
        CompilePhase::CodegenPlan => 3,
        CompilePhase::ClassfileEmit => 4,
        CompilePhase::Cleanup => 5,
        CompilePhase::ResultDrop => 6,
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
    use super::summary;

    #[test]
    fn statistics_include_median_and_mad() {
        let stats = summary(vec![1.0, 2.0, 100.0]);
        assert_eq!(stats.min, 1.0);
        assert_eq!(stats.median, 2.0);
        assert_eq!(stats.mad, 1.0);
    }
}
