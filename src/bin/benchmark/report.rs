use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::Config;
use super::measurement::{
    AllocationProfile, BenchmarkReport, PHASE_NAMES, PerformancePass, PhaseProfile,
    ProcessScenario, ResourceSeries, ScalarSeries, Workload,
};

struct Provenance {
    benchmark_binary: String,
    njavac_binary: String,
    allocation_helper: String,
    javac_binary: String,
    javac_version: String,
}

pub(super) fn print_and_write(
    cfg: &Config,
    workload: &Workload,
    report: &BenchmarkReport,
) -> Result<(), String> {
    let provenance = collect_provenance(cfg)?;
    println!("\nbenchmark report");
    println!("  revision       {}", metadata("NJAVAC_BENCH_REVISION"));
    println!("  architecture   {}-{}", std::env::consts::OS, std::env::consts::ARCH);
    println!("  CPU            {}", cpu_model());
    println!("  power mode     {}", metadata("NJAVAC_BENCH_POWER_MODE"));
    println!("  image ID       {}", metadata("NJAVAC_BENCH_IMAGE_ID"));
    println!(
        "  controls       CPU={} memory={}",
        metadata("NJAVAC_BENCH_CPU"),
        metadata("NJAVAC_BENCH_MEM"),
    );
    println!(
        "  workload       {} files, {} bytes, {} physical lines, {} nonblank lines",
        workload.fixtures.len(),
        workload.source_bytes,
        workload.physical_lines,
        workload.nonblank_lines,
    );
    println!("  corpus         {}", workload.fingerprint);
    println!("  class output   {} bytes", workload.output_class_bytes);
    println!("  njavac binary  {}", provenance.njavac_binary);
    println!("  runner binary  {}", provenance.benchmark_binary);

    println!("\nend-to-end performance (uninstrumented)");
    print_process_scenario(&report.performance.startup);
    print_process_scenario(&report.performance.batch);
    print_hot(cfg, workload, &report.performance.hot);
    print_phase_report(cfg, workload, &report.phases, &report.allocations, report);

    if let Some(path) = &cfg.json_path {
        write_json(path, cfg, workload, report, &provenance)?;
        println!("\nraw report      {}", path.display());
    }
    Ok(())
}

fn print_process_scenario(scenario: &ProcessScenario) {
    println!("  {}", scenario.name);
    for (name, series) in [("javac", &scenario.javac), ("njavac", &scenario.njavac)] {
        let wall = series.wall_summary_ms();
        let files_per_second = scenario.files as f64 * 1000.0 / wall.median;
        let source_mb_per_second =
            scenario.source_bytes as f64 / 1_000_000.0 * 1000.0 / wall.median;
        let output_mb_per_second =
            scenario.output_bytes as f64 / 1_000_000.0 * 1000.0 / wall.median;
        let lines_per_second = scenario.physical_lines as f64 * 1000.0 / wall.median;
        println!(
            "    {name:7} median {:8.3} ms  MAD {:7.3}  {:9.1} files/s  {:7.2} MB/s in  {:7.2} MB/s out  CPU {:8.3} ms  RSS {:8.0} KiB",
            wall.median,
            wall.mad,
            files_per_second,
            source_mb_per_second,
            output_mb_per_second,
            series.median_cpu_ms(),
            series.median_max_rss_kb(),
        );
        println!("             {:10.0} physical lines/s", lines_per_second);
    }
}

fn print_hot(cfg: &Config, workload: &Workload, series: &ScalarSeries) {
    let summary = series.summary_ms();
    let pass_ms = summary.median / cfg.rounds as f64;
    let physical_lines_per_second = workload.physical_lines as f64 * 1000.0 / pass_ms;
    println!("  hot compiler core");
    println!(
        "    njavac  median {:8.3} ms/sample  MAD {:7.3}  {:8.3} ms/pass  {:9.1} files/s  {:7.2} MB/s in  {:7.2} MB/s out",
        summary.median,
        summary.mad,
        pass_ms,
        workload.fixtures.len() as f64 * 1000.0 / pass_ms,
        workload.source_bytes as f64 / 1_000_000.0 * 1000.0 / pass_ms,
        workload.output_class_bytes as f64 / 1_000_000.0 * 1000.0 / pass_ms,
    );
    println!(
        "             {:10.0} physical lines/s ({:.2}M)",
        physical_lines_per_second,
        physical_lines_per_second / 1_000_000.0,
    );
}

fn print_phase_report(
    cfg: &Config,
    workload: &Workload,
    phases: &PhaseProfile,
    allocations: &AllocationProfile,
    report: &BenchmarkReport,
) {
    let operations = (cfg.rounds * workload.fixtures.len()) as f64;
    let phase_medians: Vec<f64> = (0..PHASE_NAMES.len())
        .map(|phase| phases.median_phase_ns(phase))
        .collect();
    let phase_total: f64 = phase_medians.iter().sum();
    let profile_wall = phases.wall_summary_ms().median;
    let hot_wall = report.performance.hot.summary_ms().median;
    let profile_delta = percent_change(hot_wall, profile_wall);

    println!("\nphase attribution (instrumented; not authoritative throughput)");
    if profile_delta > 0.0 {
        println!(
            "  profiler wall {:8.3} ms/sample  uninstrumented {:8.3} ms/sample  observed overhead {:+6.2}%",
            profile_wall, hot_wall, profile_delta,
        );
    } else {
        println!(
            "  profiler wall {:8.3} ms/sample  uninstrumented {:8.3} ms/sample  observed delta {:+6.2}% (overhead unresolved in noise)",
            profile_wall, hot_wall, profile_delta,
        );
    }
    println!(
        "  {:<16} {:>10} {:>8} {:>11} {:>9} {:>10} {:>11} {:>12} {:>12}",
        "phase", "ns/file", "share", "files/s", "MB/s", "M lines/s", "alloc/file", "bytes/file", "freed/file",
    );
    for phase in 0..PHASE_NAMES.len() {
        let ns_per_file = phase_medians[phase] / operations;
        let allocation_ops = (cfg.allocation_rounds * workload.fixtures.len()) as f64;
        println!(
            "  {:<16} {:>10.0} {:>7.1}% {:>11.0} {:>9.1} {:>10.2} {:>11.1} {:>12.1} {:>12.1}",
            PHASE_NAMES[phase],
            ns_per_file,
            if phase_total > 0.0 { phase_medians[phase] / phase_total * 100.0 } else { 0.0 },
            1_000_000_000.0 / ns_per_file,
            (workload.source_bytes as f64 / workload.fixtures.len() as f64) / ns_per_file * 1000.0,
            (workload.physical_lines as f64 / workload.fixtures.len() as f64) / ns_per_file
                * 1000.0,
            allocations.phases[phase].calls as f64 / allocation_ops,
            allocations.phases[phase].bytes as f64 / allocation_ops,
            allocations.phases[phase].deallocated_bytes as f64 / allocation_ops,
        );
    }
    let emit_ns_per_file = phase_medians[4] / operations;
    println!(
        "  classfile emit output throughput        {:.1} MB/s",
        (workload.output_class_bytes as f64 / workload.fixtures.len() as f64)
            / emit_ns_per_file
            * 1000.0,
    );
    println!("  peak compiler-managed live allocation  {} bytes", allocations.peak_live_bytes);
    println!("  final compiler-managed live allocation {} bytes", allocations.final_live_bytes);
}

fn write_json(
    path: &Path,
    cfg: &Config,
    workload: &Workload,
    report: &BenchmarkReport,
    provenance: &Provenance,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    let mut json = String::new();
    writeln!(&mut json, "{{").unwrap();
    writeln!(&mut json, "  \"schema_version\": 1,").unwrap();
    writeln!(&mut json, "  \"methodology_version\": 1,").unwrap();
    writeln!(&mut json, "  \"metadata\": {{").unwrap();
    writeln!(
        &mut json,
        "    \"generated_at_unix_seconds\": {},",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    )
    .unwrap();
    json_string_field(&mut json, 4, "revision", &metadata("NJAVAC_BENCH_REVISION"), true);
    json_string_field(&mut json, 4, "os", std::env::consts::OS, true);
    json_string_field(&mut json, 4, "architecture", std::env::consts::ARCH, true);
    json_string_field(&mut json, 4, "cpu", &cpu_model(), true);
    json_string_field(
        &mut json,
        4,
        "power_mode",
        &metadata("NJAVAC_BENCH_POWER_MODE"),
        true,
    );
    json_string_field(&mut json, 4, "image_id", &metadata("NJAVAC_BENCH_IMAGE_ID"), true);
    json_string_field(&mut json, 4, "cpu_control", &metadata("NJAVAC_BENCH_CPU"), true);
    json_string_field(&mut json, 4, "memory_control", &metadata("NJAVAC_BENCH_MEM"), false);
    writeln!(&mut json, "  }},").unwrap();
    writeln!(&mut json, "  \"provenance\": {{").unwrap();
    json_string_field(&mut json, 4, "benchmark_binary", &provenance.benchmark_binary, true);
    json_string_field(&mut json, 4, "njavac_binary", &provenance.njavac_binary, true);
    json_string_field(&mut json, 4, "allocation_helper", &provenance.allocation_helper, true);
    json_string_field(&mut json, 4, "javac_binary", &provenance.javac_binary, true);
    json_string_field(&mut json, 4, "javac_version", &provenance.javac_version, false);
    writeln!(&mut json, "  }},").unwrap();
    writeln!(&mut json, "  \"workload\": {{").unwrap();
    json_string_field(&mut json, 4, "fingerprint", &workload.fingerprint, true);
    writeln!(&mut json, "    \"files\": {},", workload.fixtures.len()).unwrap();
    writeln!(&mut json, "    \"source_bytes\": {},", workload.source_bytes).unwrap();
    writeln!(&mut json, "    \"physical_lines\": {},", workload.physical_lines).unwrap();
    writeln!(&mut json, "    \"nonblank_lines\": {},", workload.nonblank_lines).unwrap();
    writeln!(&mut json, "    \"output_class_bytes\": {}", workload.output_class_bytes).unwrap();
    writeln!(&mut json, "  }},").unwrap();
    writeln!(&mut json, "  \"configuration\": {{").unwrap();
    writeln!(&mut json, "    \"samples\": {},", cfg.samples).unwrap();
    writeln!(&mut json, "    \"warmup\": {},", cfg.warmup).unwrap();
    writeln!(&mut json, "    \"rounds\": {},", cfg.rounds).unwrap();
    writeln!(&mut json, "    \"allocation_rounds\": {}", cfg.allocation_rounds).unwrap();
    writeln!(&mut json, "  }},").unwrap();
    writeln!(&mut json, "  \"performance\": {{").unwrap();
    write_performance_pass(&mut json, &report.performance, 4);
    writeln!(&mut json, "  }},").unwrap();
    writeln!(&mut json, "  \"phase_profile\": {{").unwrap();
    write_phase_profile(&mut json, &report.phases, 4);
    writeln!(&mut json, "  }},").unwrap();
    writeln!(&mut json, "  \"allocations\": {{").unwrap();
    writeln!(&mut json, "    \"peak_live_bytes\": {},", report.allocations.peak_live_bytes).unwrap();
    writeln!(&mut json, "    \"final_live_bytes\": {},", report.allocations.final_live_bytes).unwrap();
    writeln!(&mut json, "    \"phases\": [").unwrap();
    for phase in 0..PHASE_NAMES.len() {
        writeln!(
            &mut json,
            "      {{\"name\": \"{}\", \"calls\": {}, \"bytes\": {}, \"deallocated_bytes\": {}}}{}",
            PHASE_NAMES[phase],
            report.allocations.phases[phase].calls,
            report.allocations.phases[phase].bytes,
            report.allocations.phases[phase].deallocated_bytes,
            if phase + 1 == PHASE_NAMES.len() { "" } else { "," },
        )
        .unwrap();
    }
    writeln!(&mut json, "    ]").unwrap();
    writeln!(&mut json, "  }},").unwrap();
    let profile_wall = report.phases.wall_summary_ms().median;
    let hot_wall = report.performance.hot.summary_ms().median;
    writeln!(&mut json, "  \"analysis\": {{").unwrap();
    writeln!(
        &mut json,
        "    \"profile_wall_delta_percent\": {:.6}",
        percent_change(hot_wall, profile_wall),
    )
    .unwrap();
    writeln!(&mut json, "  }}").unwrap();
    writeln!(&mut json, "}}").unwrap();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("cannot create {} exclusively: {error}", path.display()))?;
    file.write_all(json.as_bytes())
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn write_performance_pass(
    json: &mut String,
    pass: &PerformancePass,
    indent: usize,
) {
    let pad = " ".repeat(indent);
    writeln!(json, "{pad}\"startup\": {{").unwrap();
    write_process_scenario(json, &pass.startup, indent + 2);
    writeln!(json, "{pad}}},").unwrap();
    writeln!(json, "{pad}\"batch\": {{").unwrap();
    write_process_scenario(json, &pass.batch, indent + 2);
    writeln!(json, "{pad}}},").unwrap();
    writeln!(json, "{pad}\"hot_samples_ns\": {:?}", pass.hot.samples_ns).unwrap();
}

fn write_process_scenario(json: &mut String, scenario: &ProcessScenario, indent: usize) {
    let pad = " ".repeat(indent);
    writeln!(json, "{pad}\"files\": {},", scenario.files).unwrap();
    writeln!(json, "{pad}\"source_bytes\": {},", scenario.source_bytes).unwrap();
    writeln!(json, "{pad}\"physical_lines\": {},", scenario.physical_lines).unwrap();
    writeln!(json, "{pad}\"output_bytes\": {},", scenario.output_bytes).unwrap();
    writeln!(json, "{pad}\"javac\": [").unwrap();
    write_resource_samples(json, &scenario.javac, indent + 2);
    writeln!(json, "{pad}],").unwrap();
    writeln!(json, "{pad}\"njavac\": [").unwrap();
    write_resource_samples(json, &scenario.njavac, indent + 2);
    writeln!(json, "{pad}]").unwrap();
}

fn write_resource_samples(json: &mut String, series: &ResourceSeries, indent: usize) {
    let pad = " ".repeat(indent);
    for (index, sample) in series.samples.iter().enumerate() {
        writeln!(
            json,
            "{pad}{{\"wall_ns\": {}, \"user_us\": {}, \"system_us\": {}, \"max_rss_kb\": {}, \"minor_faults\": {}, \"major_faults\": {}, \"voluntary_switches\": {}, \"involuntary_switches\": {}}}{}",
            sample.wall_ns,
            sample.user_us,
            sample.system_us,
            sample.max_rss_kb,
            sample.minor_faults,
            sample.major_faults,
            sample.voluntary_switches,
            sample.involuntary_switches,
            if index + 1 == series.samples.len() { "" } else { "," },
        )
        .unwrap();
    }
}

fn write_phase_profile(json: &mut String, profile: &PhaseProfile, indent: usize) {
    let pad = " ".repeat(indent);
    writeln!(json, "{pad}\"samples\": [").unwrap();
    for (index, sample) in profile.samples.iter().enumerate() {
        writeln!(
            json,
            "{pad}  {{\"wall_ns\": {}, \"phases_ns\": {:?}}}{}",
            sample.wall_ns,
            sample.phases_ns,
            if index + 1 == profile.samples.len() { "" } else { "," },
        )
        .unwrap();
    }
    writeln!(json, "{pad}]").unwrap();
}

fn json_string_field(
    json: &mut String,
    indent: usize,
    name: &str,
    value: &str,
    comma: bool,
) {
    writeln!(
        json,
        "{}\"{}\": \"{}\"{}",
        " ".repeat(indent),
        escape_json(name),
        escape_json(value),
        if comma { "," } else { "" },
    )
    .unwrap();
}

fn escape_json(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            character if character.is_control() => {
                write!(&mut escaped, "\\u{:04x}", character as u32).unwrap();
            }
            character => escaped.push(character),
        }
    }
    escaped
}

fn metadata(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| "unknown".to_string())
}

fn collect_provenance(cfg: &Config) -> Result<Provenance, String> {
    let benchmark = std::env::current_exe()
        .map_err(|error| format!("cannot locate benchmark binary: {error}"))?;
    let version = Command::new(&cfg.javac)
        .arg("-version")
        .output()
        .map_err(|error| format!("cannot query javac version: {error}"))?;
    if !version.status.success() {
        return Err("javac -version failed while collecting provenance".to_string());
    }
    let javac_version = if version.stderr.is_empty() {
        String::from_utf8_lossy(&version.stdout).trim().to_string()
    } else {
        String::from_utf8_lossy(&version.stderr).trim().to_string()
    };
    Ok(Provenance {
        benchmark_binary: file_fingerprint(&benchmark)?,
        njavac_binary: file_fingerprint(Path::new(&cfg.njavac))?,
        allocation_helper: file_fingerprint(Path::new(&cfg.alloc_helper))?,
        javac_binary: file_fingerprint(Path::new(&cfg.javac))?,
        javac_version,
    })
}

fn file_fingerprint(path: &Path) -> Result<String, String> {
    let output = Command::new("sha256sum")
        .arg(path)
        .output()
        .map_err(|error| format!("cannot fingerprint {}: {error}", path.display()))?;
    if !output.status.success() {
        return Err(format!("sha256sum failed for {}", path.display()));
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
                matches!(name.trim(), "model name" | "Hardware").then(|| value.trim().to_string())
            })
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn percent_change(before: f64, after: f64) -> f64 {
    if before == 0.0 { 0.0 } else { (after / before - 1.0) * 100.0 }
}

#[cfg(test)]
mod tests {
    use super::escape_json;

    #[test]
    fn escapes_json_control_characters() {
        assert_eq!(escape_json("a\n\"b"), "a\\n\\\"b");
    }
}
