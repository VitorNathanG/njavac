use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::Config;
use super::model::{
    BenchmarkConfiguration, Metadata, PhaseMetricSummary, ProcessScenarioSummary,
    ProcessSeriesSummary, Provenance, ReportDocument,
};
use super::phase::PhaseName;

pub(super) struct ReportContext {
    pub metadata: Metadata,
    pub provenance: Provenance,
    pub configuration: BenchmarkConfiguration,
}

pub(super) fn collect_context(cfg: &Config) -> Result<ReportContext, String> {
    let benchmark = std::env::current_exe()
        .map_err(|error| format!("cannot locate benchmark binary: {error}"))?;
    let version = Command::new(&cfg.javac)
        .arg("-version")
        .output()
        .map_err(|error| format!("cannot query javac version at {}: {error}", cfg.javac))?;
    if !version.status.success() {
        return Err(format!(
            "javac provenance preflight failed with status {}; stderr: {}",
            version.status,
            String::from_utf8_lossy(&version.stderr).trim(),
        ));
    }
    let javac_version = if version.stderr.is_empty() {
        String::from_utf8_lossy(&version.stdout).trim().to_string()
    } else {
        String::from_utf8_lossy(&version.stderr).trim().to_string()
    };
    Ok(ReportContext {
        metadata: Metadata {
            generated_at_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            revision: metadata("NJAVAC_BENCH_REVISION"),
            operating_system: std::env::consts::OS.to_string(),
            architecture: std::env::consts::ARCH.to_string(),
            cpu: cpu_model(),
            power_mode: metadata("NJAVAC_BENCH_POWER_MODE"),
            image_id: metadata("NJAVAC_BENCH_IMAGE_ID"),
            cpu_control: metadata("NJAVAC_BENCH_CPU"),
            memory_control: metadata("NJAVAC_BENCH_MEM"),
        },
        provenance: Provenance {
            benchmark_binary_sha256: file_fingerprint(&benchmark)?,
            njavac_binary_sha256: file_fingerprint(Path::new(&cfg.njavac))?,
            allocation_helper_sha256: file_fingerprint(Path::new(&cfg.alloc_helper))?,
            javac_binary_sha256: file_fingerprint(Path::new(&cfg.javac))?,
            javac_version,
        },
        configuration: BenchmarkConfiguration {
            samples: cfg.samples as u64,
            warmup: cfg.warmup as u64,
            rounds: cfg.rounds as u64,
            allocation_rounds: cfg.allocation_rounds as u64,
        },
    })
}

pub(super) fn preflight_destination(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!("report destination already exists: {}", path.display()));
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create report directory {}: {error}", parent.display()))?;
    let temporary = temporary_path(path);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("cannot create report preflight file {}: {error}", temporary.display()))?;
    drop(file);
    std::fs::remove_file(&temporary)
        .map_err(|error| format!("cannot remove report preflight file {}: {error}", temporary.display()))
}

pub(super) fn print_and_publish(document: &ReportDocument, path: Option<&Path>) -> Result<(), String> {
    let stdout = std::io::stdout();
    render(document, &mut stdout.lock()).map_err(|error| format!("cannot render benchmark report: {error}"))?;
    if let Some(path) = path {
        publish(document, path)?;
        println!("\nraw report      {}", path.display());
    }
    Ok(())
}

fn render(document: &ReportDocument, output: &mut impl Write) -> io::Result<()> {
    writeln!(output, "\nbenchmark report")?;
    writeln!(output, "  evidence       exploratory (not a numerical baseline)")?;
    writeln!(output, "  revision       {}", document.metadata.revision)?;
    writeln!(
        output,
        "  architecture   {}-{}",
        document.metadata.operating_system,
        document.metadata.architecture,
    )?;
    writeln!(output, "  CPU            {}", document.metadata.cpu)?;
    writeln!(output, "  power mode     {}", document.metadata.power_mode)?;
    writeln!(output, "  image ID       {}", document.metadata.image_id)?;
    writeln!(
        output,
        "  controls       CPU={} memory={}",
        document.metadata.cpu_control,
        document.metadata.memory_control,
    )?;
    writeln!(
        output,
        "  workload       {} files, {} bytes, {} physical lines, {} nonblank lines",
        document.workload.files,
        document.workload.source_bytes,
        document.workload.physical_lines,
        document.workload.nonblank_lines,
    )?;
    writeln!(output, "  corpus         {}", document.workload.fingerprint)?;
    writeln!(output, "  class output   {} bytes", document.workload.output_class_bytes)?;
    writeln!(output, "  njavac binary  {}", document.provenance.njavac_binary_sha256)?;
    writeln!(output, "  runner binary  {}", document.provenance.benchmark_binary_sha256)?;

    writeln!(output, "\nend-to-end performance (uninstrumented)")?;
    print_process_summary(
        output,
        "minimal-input fresh-process compile",
        &document.summaries.minimal_input_fresh_process_compile,
    )?;
    print_process_summary(
        output,
        "whole-corpus CLI compile",
        &document.summaries.whole_corpus_cli_compile,
    )?;
    let hot = &document.summaries.hot_in_process_corpus_compile;
    writeln!(output, "  hot in-process corpus compile")?;
    writeln!(
        output,
        "    njavac  median {:8.3} ms/sample  MAD {:7.3} ms  {:8.3} ms/corpus pass  {:9.1} effective files/s",
        ns_to_ms(hot.sample_wall_ns.median),
        ns_to_ms(hot.sample_wall_ns.median_absolute_deviation),
        ns_to_ms(hot.median_corpus_pass_wall_ns),
        hot.effective_files_per_second,
    )?;
    writeln!(
        output,
        "            {:7.2} normalized source MB/s  {:7.2} normalized output MB/s  {:10.0} physical lines/s",
        hot.normalized_source_mb_per_second,
        hot.normalized_output_mb_per_second,
        hot.physical_lines_per_second,
    )?;

    let phases = &document.summaries.phase_profile;
    writeln!(output, "\nphase attribution (instrumented diagnostic)")?;
    writeln!(
        output,
        "  profiler wall {:8.3} ms/sample  hot wall {:8.3} ms/sample  observed delta {:+6.2}%",
        ns_to_ms(phases.wall_ns.median),
        ns_to_ms(hot.sample_wall_ns.median),
        document.analysis.profile_wall_delta_percent,
    )?;
    writeln!(
        output,
        "  unattributed  {:8.3} ms/sample  {:6.2}% of profile wall",
        ns_to_ms(phases.median_unattributed_wall_ns),
        phases.unattributed_wall_percent,
    )?;
    writeln!(
        output,
        "  {:<40} {:>10} {:>8} {:>11} {:>11} {:>12} {:>12}",
        "phase", "ns/file", "share", "files/s", "alloc/file", "bytes/file", "freed/file",
    )?;
    for phase in PhaseName::ALL {
        print_phase(output, phase, phases.phases.get(phase))?;
    }
    let allocations = &document.measurements.allocations;
    writeln!(
        output,
        "  peak tracked live growth             {} bytes",
        allocations.peak_live_growth_bytes,
    )?;
    writeln!(
        output,
        "  allocation balance invariant         final {} == baseline {} bytes",
        allocations.final_live_bytes,
        allocations.baseline_live_bytes,
    )?;
    Ok(())
}

fn print_process_summary(
    output: &mut impl Write,
    name: &str,
    scenario: &ProcessScenarioSummary,
) -> io::Result<()> {
    writeln!(output, "  {name}")?;
    print_process_series(output, "javac", &scenario.javac)?;
    print_process_series(output, "njavac", &scenario.njavac)
}

fn print_process_series(
    output: &mut impl Write,
    name: &str,
    summary: &ProcessSeriesSummary,
) -> io::Result<()> {
    writeln!(
        output,
        "    {name:7} median {:8.3} ms  MAD {:7.3} ms  CPU {:8.3} ms  RSS {:8.0} KiB",
        ns_to_ms(summary.wall_ns.median),
        ns_to_ms(summary.wall_ns.median_absolute_deviation),
        summary.median_cpu_total_us / 1000.0,
        summary.median_max_rss_kib,
    )?;
    writeln!(
        output,
        "            {:9.1} effective files/s  {:7.2} normalized source MB/s  {:7.2} normalized output MB/s  {:10.0} physical lines/s",
        summary.effective_files_per_second,
        summary.normalized_source_mb_per_second,
        summary.normalized_output_mb_per_second,
        summary.physical_lines_per_second,
    )
}

fn print_phase(
    output: &mut impl Write,
    phase: PhaseName,
    summary: &PhaseMetricSummary,
) -> io::Result<()> {
    writeln!(
        output,
        "  {:<40} {:>10.0} {:>7.1}% {:>11.0} {:>11.1} {:>12.1} {:>12.1}",
        phase.as_str(),
        summary.median_ns_per_file,
        summary.share_percent,
        summary.effective_files_per_second,
        summary.allocation_calls_per_file,
        summary.requested_bytes_per_file,
        summary.released_bytes_per_file,
    )
}

fn publish(document: &ReportDocument, path: &Path) -> Result<(), String> {
    document.validate()?;
    let mut bytes = serde_json::to_vec_pretty(document)
        .map_err(|error| format!("cannot serialize benchmark report: {error}"))?;
    bytes.push(b'\n');
    publish_with(path, |file| file.write_all(&bytes))
}

fn publish_with(
    path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> Result<(), String> {
    if path.exists() {
        return Err(format!("report destination already exists: {}", path.display()));
    }
    let temporary = temporary_path(path);
    let mut guard = TemporaryFile::create(&temporary)?;
    write(guard.file_mut()).map_err(|error| {
        format!("cannot write temporary report {}: {error}", temporary.display())
    })?;
    guard
        .file_mut()
        .flush()
        .and_then(|()| guard.file_mut().sync_all())
        .map_err(|error| format!("cannot flush temporary report {}: {error}", temporary.display()))?;
    std::fs::hard_link(&temporary, path).map_err(|error| {
        format!(
            "cannot atomically publish {} without clobbering: {error}",
            path.display(),
        )
    })?;
    guard.remove_after_publication();
    Ok(())
}

struct TemporaryFile {
    path: PathBuf,
    file: Option<File>,
}

impl TemporaryFile {
    fn create(path: &Path) -> Result<Self, String> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| format!("cannot create temporary report {}: {error}", path.display()))?;
        Ok(Self { path: path.to_path_buf(), file: Some(file) })
    }

    fn file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("temporary report file is open")
    }

    fn remove_after_publication(&mut self) {
        self.file.take();
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        self.file.take();
        let _ = std::fs::remove_file(&self.path);
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(
        ".{name}.tmp-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    ))
}

fn metadata(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| "unknown".to_string())
}

fn file_fingerprint(path: &Path) -> Result<String, String> {
    let output = Command::new("sha256sum")
        .arg(path)
        .output()
        .map_err(|error| format!("cannot fingerprint {}: {error}", path.display()))?;
    if !output.status.success() {
        return Err(format!(
            "sha256sum failed for {} with status {}; stderr: {}",
            path.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim(),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let hash = text
        .split_whitespace()
        .next()
        .filter(|value| value.len() == 64)
        .ok_or_else(|| format!("invalid sha256sum output for {}", path.display()))?;
    Ok(format!("sha256:{hash}"))
}

fn cpu_model() -> String {
    if let Ok(model) = std::env::var("NJAVAC_BENCH_HOST_CPU") {
        return model;
    }
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                matches!(name.trim(), "model name" | "Hardware")
                    .then(|| value.trim().to_string())
            })
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn ns_to_ms(value: f64) -> f64 {
    value / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use super::{collect_context, preflight_destination, publish_with};
    use crate::Config;

    fn test_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "njavac-benchmark-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn publication_is_no_clobber_and_atomic_on_write_failure() {
        let directory = test_dir("publication");
        let final_path = directory.join("report.json");
        preflight_destination(&final_path).unwrap();
        let error = publish_with(&final_path, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("injected failure"))
        })
        .unwrap_err();
        assert!(error.contains("injected failure"));
        assert!(!final_path.exists());

        publish_with(&final_path, |file| file.write_all(b"complete\n")).unwrap();
        assert_eq!(std::fs::read(&final_path).unwrap(), b"complete\n");
        assert!(publish_with(&final_path, |_| Ok(())).is_err());
        assert_eq!(std::fs::read(&final_path).unwrap(), b"complete\n");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn provenance_failure_happens_during_preflight() {
        let mut config = Config::defaults();
        config.javac = "/definitely/missing/javac".to_string();
        assert!(collect_context(&config).is_err());
    }
}
