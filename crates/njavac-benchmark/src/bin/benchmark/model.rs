use serde::{Deserialize, Serialize};

use super::phase::{PhaseName, PhaseValues};

pub(super) const SCHEMA_VERSION: u32 = 3;
pub(super) const METHODOLOGY_VERSION: u32 = 3;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReportDocument {
    pub schema_version: u32,
    pub methodology_version: u32,
    pub evidence_status: EvidenceStatus,
    pub metadata: Metadata,
    pub provenance: Provenance,
    pub workload: WorkloadIdentity,
    pub configuration: BenchmarkConfiguration,
    pub outcomes: ReportOutcomes,
    pub measurements: Measurements,
    pub summaries: ReportSummaries,
    pub analysis: ReportAnalysis,
    pub metric_contract: Vec<MetricDefinition>,
    pub warnings: Vec<ReportWarning>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum EvidenceStatus {
    Exploratory,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Metadata {
    pub generated_at_unix_seconds: u64,
    pub revision: String,
    pub operating_system: String,
    pub architecture: String,
    pub cpu: String,
    pub power_mode: String,
    pub image_id: String,
    pub cpu_control: String,
    pub memory_control: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Provenance {
    pub benchmark_binary_sha256: String,
    pub njavac_binary_sha256: String,
    pub allocation_helper_sha256: String,
    pub javac_binary_sha256: String,
    pub javac_version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct WorkloadIdentity {
    pub fingerprint: String,
    pub files: u64,
    pub source_bytes: u64,
    pub physical_lines: u64,
    pub nonblank_lines: u64,
    pub output_class_bytes: u64,
    pub minimal_input_fixture: String,
    pub minimal_input_source_bytes: u64,
    pub minimal_input_physical_lines: u64,
    pub minimal_input_output_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BenchmarkConfiguration {
    pub samples: u64,
    pub warmup: u64,
    pub rounds: u64,
    pub allocation_rounds: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CompletionOutcome {
    Complete,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReportOutcomes {
    pub measurement: CompletionOutcome,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Measurements {
    pub performance: PerformanceMeasurements,
    pub phase_profile: PhaseProfile,
    pub allocations: AllocationProfile,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PerformanceMeasurements {
    pub minimal_input_fresh_process_compile: ProcessScenario,
    pub whole_corpus_cli_compile: ProcessScenario,
    pub hot_in_process_corpus_compile: ScalarSeries,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProcessScenario {
    pub files: u64,
    pub source_bytes: u64,
    pub physical_lines: u64,
    pub output_bytes: u64,
    pub javac: ResourceSeries,
    pub njavac: ResourceSeries,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResourceSeries {
    pub samples: Vec<ResourceSample>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResourceSample {
    pub wall_ns: u64,
    pub user_us: u64,
    pub system_us: u64,
    pub max_rss_kib: u64,
    pub minor_faults: u64,
    pub major_faults: u64,
    pub voluntary_context_switches: u64,
    pub involuntary_context_switches: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ScalarSeries {
    pub samples_ns: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PhaseProfile {
    pub samples: Vec<PhaseSample>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PhaseSample {
    pub wall_ns: u64,
    pub phases_ns: PhaseValues<u64>,
    pub unattributed_wall_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AllocationProfile {
    pub phases: PhaseValues<AllocationMetric>,
    pub baseline_live_bytes: u64,
    pub peak_live_growth_bytes: u64,
    pub final_live_bytes: u64,
    pub total_requested_bytes: u64,
    pub total_released_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AllocationMetric {
    pub allocation_calls: u64,
    pub requested_bytes: u64,
    pub released_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReportSummaries {
    pub minimal_input_fresh_process_compile: ProcessScenarioSummary,
    pub whole_corpus_cli_compile: ProcessScenarioSummary,
    pub hot_in_process_corpus_compile: HotSummary,
    pub phase_profile: PhaseProfileSummary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProcessScenarioSummary {
    pub javac: ProcessSeriesSummary,
    pub njavac: ProcessSeriesSummary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProcessSeriesSummary {
    pub wall_ns: Summary,
    pub median_cpu_total_us: f64,
    pub median_max_rss_kib: f64,
    pub effective_files_per_second: f64,
    pub normalized_source_mb_per_second: f64,
    pub normalized_output_mb_per_second: f64,
    pub physical_lines_per_second: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HotSummary {
    pub sample_wall_ns: Summary,
    pub median_corpus_pass_wall_ns: f64,
    pub effective_files_per_second: f64,
    pub normalized_source_mb_per_second: f64,
    pub normalized_output_mb_per_second: f64,
    pub physical_lines_per_second: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PhaseProfileSummary {
    pub wall_ns: Summary,
    pub phases: PhaseValues<PhaseMetricSummary>,
    pub median_unattributed_wall_ns: f64,
    pub unattributed_wall_percent: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PhaseMetricSummary {
    pub median_ns_per_sample: f64,
    pub median_ns_per_file: f64,
    pub share_percent: f64,
    pub effective_files_per_second: f64,
    pub normalized_source_mb_per_second: f64,
    pub physical_lines_per_second: f64,
    pub allocation_calls_per_file: f64,
    pub requested_bytes_per_file: f64,
    pub released_bytes_per_file: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Summary {
    pub min: f64,
    pub median: f64,
    pub mean: f64,
    pub population_standard_deviation: f64,
    pub median_absolute_deviation: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReportAnalysis {
    pub profile_wall_delta_percent: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MetricRole {
    Primary,
    Secondary,
    Derived,
    Diagnostic,
    Invariant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MetricDefinition {
    pub path: String,
    pub role: MetricRole,
    pub unit: String,
    pub formula_or_boundary: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReportWarning {
    pub code: String,
    pub message: String,
}

impl ReportDocument {
    pub(super) fn new(
        metadata: Metadata,
        provenance: Provenance,
        workload: WorkloadIdentity,
        configuration: BenchmarkConfiguration,
        measurements: Measurements,
    ) -> Result<Self, String> {
        let summaries = ReportSummaries::from_measurements(&measurements, &workload, &configuration)?;
        let profile_wall = summaries.phase_profile.wall_ns.median;
        let hot_wall = summaries.hot_in_process_corpus_compile.sample_wall_ns.median;
        if hot_wall <= 0.0 {
            return Err("hot compiler wall time must be positive".to_string());
        }
        let document = Self {
            schema_version: SCHEMA_VERSION,
            methodology_version: METHODOLOGY_VERSION,
            evidence_status: EvidenceStatus::Exploratory,
            metadata,
            provenance,
            workload,
            configuration,
            outcomes: ReportOutcomes {
                measurement: CompletionOutcome::Complete,
            },
            measurements,
            summaries,
            analysis: ReportAnalysis {
                profile_wall_delta_percent: (profile_wall / hot_wall - 1.0) * 100.0,
            },
            metric_contract: metric_contract(),
            warnings: Vec::new(),
        };
        document.validate()?;
        Ok(document)
    }

    #[allow(dead_code)]
    pub(super) fn from_json(bytes: &[u8]) -> Result<Self, String> {
        let document: Self = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid benchmark report JSON: {error}"))?;
        document.validate()?;
        Ok(document)
    }

    pub(super) fn validate(&self) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "unsupported benchmark schema version {} (expected {SCHEMA_VERSION})",
                self.schema_version,
            ));
        }
        if self.methodology_version != METHODOLOGY_VERSION {
            return Err(format!(
                "unsupported benchmark methodology version {} (expected {METHODOLOGY_VERSION})",
                self.methodology_version,
            ));
        }
        if self.measurements.allocations.final_live_bytes
            != self.measurements.allocations.baseline_live_bytes
        {
            return Err("allocation final live bytes do not match the measured baseline".to_string());
        }
        if self.measurements.allocations.total_requested_bytes
            != self.measurements.allocations.total_released_bytes
        {
            return Err("allocation requested and released totals do not balance".to_string());
        }
        self.validate_raw_measurements()?;
        let expected_summaries = ReportSummaries::from_measurements(
            &self.measurements,
            &self.workload,
            &self.configuration,
        )?;
        if self.summaries != expected_summaries {
            return Err("persisted summaries do not match raw measurements".to_string());
        }
        let expected_analysis = ReportAnalysis {
            profile_wall_delta_percent: (
                self.summaries.phase_profile.wall_ns.median
                    / self.summaries.hot_in_process_corpus_compile.sample_wall_ns.median
                    - 1.0
            ) * 100.0,
        };
        if self.analysis != expected_analysis {
            return Err("persisted analysis does not match report summaries".to_string());
        }
        if self.metric_contract != metric_contract() {
            return Err("persisted metric contract is not canonical for this schema".to_string());
        }
        let value = serde_json::to_value(self)
            .map_err(|error| format!("benchmark report cannot be represented as JSON: {error}"))?;
        reject_non_finite_or_null(&value, "report")?;
        Ok(())
    }

    fn validate_raw_measurements(&self) -> Result<(), String> {
        let expected_samples = usize::try_from(self.configuration.samples)
            .map_err(|_| "configured sample count does not fit usize")?;
        if expected_samples == 0 {
            return Err("configured sample count must be positive".to_string());
        }
        for (name, scenario) in [
            (
                "minimal_input_fresh_process_compile",
                &self.measurements.performance.minimal_input_fresh_process_compile,
            ),
            (
                "whole_corpus_cli_compile",
                &self.measurements.performance.whole_corpus_cli_compile,
            ),
        ] {
            if scenario.javac.samples.len() != expected_samples
                || scenario.njavac.samples.len() != expected_samples
            {
                return Err(format!("{name} raw sample count does not match configuration"));
            }
        }
        if self
            .measurements
            .performance
            .hot_in_process_corpus_compile
            .samples_ns
            .len()
            != expected_samples
            || self.measurements.phase_profile.samples.len() != expected_samples
        {
            return Err("hot or phase raw sample count does not match configuration".to_string());
        }
        let batch = &self.measurements.performance.whole_corpus_cli_compile;
        if batch.files != self.workload.files
            || batch.source_bytes != self.workload.source_bytes
            || batch.physical_lines != self.workload.physical_lines
            || batch.output_bytes != self.workload.output_class_bytes
        {
            return Err("whole-corpus process quantities do not match workload identity".to_string());
        }
        let minimal = &self
            .measurements
            .performance
            .minimal_input_fresh_process_compile;
        if minimal.files != 1
            || minimal.source_bytes != self.workload.minimal_input_source_bytes
            || minimal.physical_lines != self.workload.minimal_input_physical_lines
            || minimal.output_bytes != self.workload.minimal_input_output_bytes
        {
            return Err("minimal-input process quantities do not match workload identity".to_string());
        }
        for sample in &self.measurements.phase_profile.samples {
            let attributed = PhaseName::ALL.into_iter().try_fold(0_u64, |total, phase| {
                total.checked_add(*sample.phases_ns.get(phase))
            });
            let attributed = attributed.ok_or("phase duration sum overflow")?;
            if attributed.checked_add(sample.unattributed_wall_ns) != Some(sample.wall_ns) {
                return Err("phase attribution and unattributed time do not equal profile wall time".to_string());
            }
        }
        let phase_requested = PhaseName::ALL.into_iter().try_fold(0_u64, |total, phase| {
            total.checked_add(self.measurements.allocations.phases.get(phase).requested_bytes)
        });
        let phase_released = PhaseName::ALL.into_iter().try_fold(0_u64, |total, phase| {
            total.checked_add(self.measurements.allocations.phases.get(phase).released_bytes)
        });
        if phase_requested != Some(self.measurements.allocations.total_requested_bytes)
            || phase_released != Some(self.measurements.allocations.total_released_bytes)
        {
            return Err("allocation phase totals do not match complete workload totals".to_string());
        }
        Ok(())
    }
}

impl ReportSummaries {
    fn from_measurements(
        measurements: &Measurements,
        workload: &WorkloadIdentity,
        configuration: &BenchmarkConfiguration,
    ) -> Result<Self, String> {
        if workload.files == 0 || configuration.rounds == 0 || configuration.allocation_rounds == 0 {
            return Err("benchmark summary denominators must be positive".to_string());
        }
        let phase_operations = configuration
            .rounds
            .checked_mul(workload.files)
            .ok_or("phase operation count overflow")? as f64;
        let allocation_operations = configuration
            .allocation_rounds
            .checked_mul(workload.files)
            .ok_or("allocation operation count overflow")? as f64;
        let phase_medians = measurements.phase_profile.median_phases()?;
        let phase_total = PhaseName::ALL
            .into_iter()
            .map(|phase| *phase_medians.get(phase))
            .sum::<f64>();
        if phase_total <= 0.0 {
            return Err("phase time must be positive".to_string());
        }
        let phases = phase_medians.try_map(|median_ns| {
            if median_ns <= 0.0 {
                return Err("every phase median must be positive".to_string());
            }
            Ok(PhaseMetricSummary {
                median_ns_per_sample: median_ns,
                median_ns_per_file: median_ns / phase_operations,
                share_percent: median_ns / phase_total * 100.0,
                effective_files_per_second: phase_operations / median_ns * 1_000_000_000.0,
                normalized_source_mb_per_second: configuration.rounds as f64
                    * workload.source_bytes as f64
                    / median_ns
                    * 1000.0,
                physical_lines_per_second: configuration.rounds as f64
                    * workload.physical_lines as f64
                    / median_ns
                    * 1_000_000_000.0,
                ..PhaseMetricSummary::default()
            })
        })?;
        let mut phases = phases;
        for phase in PhaseName::ALL {
            let allocation = measurements.allocations.phases.get(phase);
            let summary = phases.get_mut(phase);
            summary.allocation_calls_per_file = allocation.allocation_calls as f64 / allocation_operations;
            summary.requested_bytes_per_file = allocation.requested_bytes as f64 / allocation_operations;
            summary.released_bytes_per_file = allocation.released_bytes as f64 / allocation_operations;
        }

        let phase_wall = measurements.phase_profile.wall_summary()?;
        let median_unattributed_wall_ns = median(
            measurements
                .phase_profile
                .samples
                .iter()
                .map(|sample| sample.unattributed_wall_ns as f64)
                .collect(),
        )?;
        Ok(Self {
            minimal_input_fresh_process_compile: measurements
                .performance
                .minimal_input_fresh_process_compile
                .summarize()?,
            whole_corpus_cli_compile: measurements
                .performance
                .whole_corpus_cli_compile
                .summarize()?,
            hot_in_process_corpus_compile: measurements
                .performance
                .hot_in_process_corpus_compile
                .summarize(workload, configuration)?,
            phase_profile: PhaseProfileSummary {
                wall_ns: phase_wall,
                phases,
                median_unattributed_wall_ns,
                unattributed_wall_percent: median_unattributed_wall_ns
                    / phase_wall.median
                    * 100.0,
            },
        })
    }
}

impl ProcessScenario {
    fn summarize(&self) -> Result<ProcessScenarioSummary, String> {
        Ok(ProcessScenarioSummary {
            javac: self.javac.summarize(self)?,
            njavac: self.njavac.summarize(self)?,
        })
    }
}

impl ResourceSeries {
    fn summarize(&self, scenario: &ProcessScenario) -> Result<ProcessSeriesSummary, String> {
        let wall_ns = summary(self.samples.iter().map(|sample| sample.wall_ns as f64).collect())?;
        if wall_ns.median <= 0.0 {
            return Err("process wall time must be positive".to_string());
        }
        Ok(ProcessSeriesSummary {
            median_cpu_total_us: median(
                self.samples
                    .iter()
                    .map(|sample| (sample.user_us + sample.system_us) as f64)
                    .collect(),
            )?,
            median_max_rss_kib: median(
                self.samples.iter().map(|sample| sample.max_rss_kib as f64).collect(),
            )?,
            effective_files_per_second: scenario.files as f64 / wall_ns.median * 1_000_000_000.0,
            normalized_source_mb_per_second: scenario.source_bytes as f64 / wall_ns.median * 1000.0,
            normalized_output_mb_per_second: scenario.output_bytes as f64 / wall_ns.median * 1000.0,
            physical_lines_per_second: scenario.physical_lines as f64
                / wall_ns.median
                * 1_000_000_000.0,
            wall_ns,
        })
    }
}

impl ScalarSeries {
    fn summarize(
        &self,
        workload: &WorkloadIdentity,
        configuration: &BenchmarkConfiguration,
    ) -> Result<HotSummary, String> {
        let sample_wall_ns = summary(self.samples_ns.iter().map(|&value| value as f64).collect())?;
        let pass_ns = sample_wall_ns.median / configuration.rounds as f64;
        if pass_ns <= 0.0 {
            return Err("hot corpus-pass wall time must be positive".to_string());
        }
        Ok(HotSummary {
            sample_wall_ns,
            median_corpus_pass_wall_ns: pass_ns,
            effective_files_per_second: workload.files as f64 / pass_ns * 1_000_000_000.0,
            normalized_source_mb_per_second: workload.source_bytes as f64 / pass_ns * 1000.0,
            normalized_output_mb_per_second: workload.output_class_bytes as f64 / pass_ns * 1000.0,
            physical_lines_per_second: workload.physical_lines as f64 / pass_ns * 1_000_000_000.0,
        })
    }
}

impl PhaseProfile {
    fn wall_summary(&self) -> Result<Summary, String> {
        summary(self.samples.iter().map(|sample| sample.wall_ns as f64).collect())
    }

    fn median_phases(&self) -> Result<PhaseValues<f64>, String> {
        let med = |phase| {
            median(
                self.samples
                    .iter()
                    .map(|sample| *sample.phases_ns.get(phase) as f64)
                    .collect(),
            )
        };
        Ok(PhaseValues {
            lex: med(PhaseName::Lex)?,
            parse: med(PhaseName::Parse)?,
            semantic_analysis: med(PhaseName::SemanticAnalysis)?,
            codegen_planning: med(PhaseName::CodegenPlanning)?,
            classfile_serialization_and_plan_drop: med(
                PhaseName::ClassfileSerializationAndPlanDrop,
            )?,
            analysis_and_syntax_drop: med(PhaseName::AnalysisAndSyntaxDrop)?,
            result_bytes_drop: med(PhaseName::ResultBytesDrop)?,
        })
    }
}

pub(super) fn summary(mut values: Vec<f64>) -> Result<Summary, String> {
    if values.is_empty() {
        return Err("statistics require at least one sample".to_string());
    }
    values.sort_by(f64::total_cmp);
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values.iter().map(|value| (value - mean).powi(2)).sum::<f64>()
        / values.len() as f64;
    let center = median_sorted(&values);
    let mut deviations: Vec<f64> = values.iter().map(|value| (value - center).abs()).collect();
    deviations.sort_by(f64::total_cmp);
    Ok(Summary {
        min: values[0],
        median: center,
        mean,
        population_standard_deviation: variance.sqrt(),
        median_absolute_deviation: median_sorted(&deviations),
    })
}

fn median(mut values: Vec<f64>) -> Result<f64, String> {
    if values.is_empty() {
        return Err("median requires at least one sample".to_string());
    }
    values.sort_by(f64::total_cmp);
    Ok(median_sorted(&values))
}

fn median_sorted(values: &[f64]) -> f64 {
    if values.len() % 2 == 1 {
        values[values.len() / 2]
    } else {
        (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
    }
}

fn reject_non_finite_or_null(value: &serde_json::Value, path: &str) -> Result<(), String> {
    match value {
        serde_json::Value::Null => Err(format!("non-finite or null report value at {path}")),
        serde_json::Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                reject_non_finite_or_null(value, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        serde_json::Value::Object(values) => {
            for (name, value) in values {
                reject_non_finite_or_null(value, &format!("{path}.{name}"))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn metric_contract() -> Vec<MetricDefinition> {
    use MetricRole::{Derived, Diagnostic, Invariant, Primary, Secondary};
    [
        ("metadata.generated_at_unix_seconds", Diagnostic, "s_since_unix_epoch", "report generation time"),
        ("workload.files", Diagnostic, "count", "number of source fixtures in canonical path order"),
        ("workload.source_bytes", Diagnostic, "bytes", "sum of UTF-8 source byte lengths"),
        ("workload.physical_lines", Diagnostic, "count", "newline-delimited physical source lines"),
        ("workload.nonblank_lines", Diagnostic, "count", "physical lines containing non-whitespace text"),
        ("workload.output_class_bytes", Diagnostic, "bytes", "sum of exact expected class-file lengths"),
        ("workload.minimal_input_fixture", Diagnostic, "path", "fixture selected for the minimal-input process scenario"),
        ("workload.minimal_input_source_bytes", Diagnostic, "bytes", "minimal-input fixture UTF-8 byte length"),
        ("workload.minimal_input_physical_lines", Diagnostic, "count", "minimal-input fixture physical line count"),
        ("workload.minimal_input_output_bytes", Diagnostic, "bytes", "minimal-input fixture expected class length"),
        ("configuration.*", Diagnostic, "count", "effective warm-up, sample, or corpus-round control"),
        ("measurements.performance.{minimal_input_fresh_process_compile,whole_corpus_cli_compile}.{files,physical_lines}", Diagnostic, "count", "scenario workload quantities"),
        ("measurements.performance.{minimal_input_fresh_process_compile,whole_corpus_cli_compile}.{source_bytes,output_bytes}", Diagnostic, "bytes", "scenario workload quantities"),
        ("measurements.performance.*.*.samples.*.wall_ns", Primary, "ns", "compiler-child wall time"),
        ("measurements.performance.*.*.samples.*.user_us", Secondary, "us", "Linux RUSAGE_CHILDREN user CPU"),
        ("measurements.performance.*.*.samples.*.system_us", Secondary, "us", "Linux RUSAGE_CHILDREN system CPU"),
        ("measurements.performance.*.*.samples.*.max_rss_kib", Secondary, "KiB", "Linux ru_maxrss peak resident set"),
        ("measurements.performance.*.*.samples.*.*_faults", Diagnostic, "count", "Linux RUSAGE_CHILDREN fault count"),
        ("measurements.performance.*.*.samples.*.*_context_switches", Diagnostic, "count", "Linux RUSAGE_CHILDREN context-switch count"),
        ("measurements.performance.hot_in_process_corpus_compile.samples_ns", Primary, "ns", "wall time for configured corpus rounds"),
        ("measurements.phase_profile.samples.*.wall_ns", Diagnostic, "ns", "wall time around all instrumented corpus rounds"),
        ("measurements.phase_profile.samples.*.phases_ns.*", Diagnostic, "ns", "exclusive named-phase duration"),
        ("measurements.phase_profile.samples.*.unattributed_wall_ns", Diagnostic, "ns", "profile wall time minus all named-phase durations"),
        ("measurements.allocations.phases.*.allocation_calls", Primary, "count", "successful allocation or realloc requests in the named phase"),
        ("measurements.allocations.phases.*.requested_bytes", Primary, "bytes", "requested allocation bytes in the named phase"),
        ("measurements.allocations.phases.*.released_bytes", Secondary, "bytes", "released layout bytes in the named phase"),
        ("measurements.allocations.baseline_live_bytes", Invariant, "bytes", "tracked live bytes immediately before measured allocation work"),
        ("measurements.allocations.peak_live_growth_bytes", Primary, "bytes", "maximum tracked live bytes minus baseline"),
        ("measurements.allocations.final_live_bytes", Invariant, "bytes", "must equal baseline_live_bytes"),
        ("measurements.allocations.total_requested_bytes", Primary, "bytes", "requested-byte counter delta over the allocation workload"),
        ("measurements.allocations.total_released_bytes", Secondary, "bytes", "released-byte counter delta over the allocation workload"),
        ("summaries.{minimal_input_fresh_process_compile,whole_corpus_cli_compile}.{javac,njavac}.wall_ns.*", Derived, "ns", "process wall minimum, median, mean, population standard deviation, or median absolute deviation"),
        ("summaries.{minimal_input_fresh_process_compile,whole_corpus_cli_compile}.{javac,njavac}.median_cpu_total_us", Derived, "us", "median of user_us + system_us per raw process sample"),
        ("summaries.{minimal_input_fresh_process_compile,whole_corpus_cli_compile}.{javac,njavac}.median_max_rss_kib", Derived, "KiB", "median raw max_rss_kib"),
        ("summaries.{minimal_input_fresh_process_compile,whole_corpus_cli_compile}.{javac,njavac}.effective_files_per_second", Derived, "files_per_second", "scenario files divided by median process wall time"),
        ("summaries.{minimal_input_fresh_process_compile,whole_corpus_cli_compile}.{javac,njavac}.normalized_source_mb_per_second", Derived, "decimal_MB_per_second", "scenario source bytes divided by median process wall time"),
        ("summaries.{minimal_input_fresh_process_compile,whole_corpus_cli_compile}.{javac,njavac}.normalized_output_mb_per_second", Derived, "decimal_MB_per_second", "scenario output bytes divided by median process wall time"),
        ("summaries.{minimal_input_fresh_process_compile,whole_corpus_cli_compile}.{javac,njavac}.physical_lines_per_second", Derived, "lines_per_second", "scenario physical lines divided by median process wall time"),
        ("summaries.hot_in_process_corpus_compile.sample_wall_ns.*", Derived, "ns", "hot sample wall minimum, median, mean, population standard deviation, or median absolute deviation"),
        ("summaries.hot_in_process_corpus_compile.median_corpus_pass_wall_ns", Derived, "ns", "median sample wall ns divided by configured rounds"),
        ("summaries.hot_in_process_corpus_compile.effective_files_per_second", Derived, "files_per_second", "workload files divided by median corpus-pass wall time"),
        ("summaries.hot_in_process_corpus_compile.normalized_source_mb_per_second", Derived, "decimal_MB_per_second", "workload source bytes divided by median corpus-pass wall time"),
        ("summaries.hot_in_process_corpus_compile.normalized_output_mb_per_second", Derived, "decimal_MB_per_second", "workload output bytes divided by median corpus-pass wall time"),
        ("summaries.hot_in_process_corpus_compile.physical_lines_per_second", Derived, "lines_per_second", "workload physical lines divided by median corpus-pass wall time"),
        ("summaries.phase_profile.wall_ns.*", Derived, "ns", "profile wall minimum, median, mean, population standard deviation, or median absolute deviation"),
        ("summaries.phase_profile.phases.*.median_ns_per_sample", Derived, "ns", "median exclusive named-phase duration"),
        ("summaries.phase_profile.phases.*.median_ns_per_file", Derived, "ns_per_file", "phase median divided by rounds times files"),
        ("summaries.phase_profile.phases.*.share_percent", Derived, "percent", "phase median divided by sum of named phase medians times 100"),
        ("summaries.phase_profile.phases.*.effective_files_per_second", Derived, "files_per_second", "phase operations divided by phase median"),
        ("summaries.phase_profile.phases.*.normalized_source_mb_per_second", Derived, "decimal_MB_per_second", "configured source bytes divided by phase median"),
        ("summaries.phase_profile.phases.*.physical_lines_per_second", Derived, "lines_per_second", "configured physical lines divided by phase median"),
        ("summaries.phase_profile.phases.*.allocation_calls_per_file", Derived, "count_per_file", "phase allocation calls divided by allocation rounds times files"),
        ("summaries.phase_profile.phases.*.requested_bytes_per_file", Derived, "bytes_per_file", "phase requested bytes divided by allocation rounds times files"),
        ("summaries.phase_profile.phases.*.released_bytes_per_file", Derived, "bytes_per_file", "phase released bytes divided by allocation rounds times files"),
        ("summaries.phase_profile.median_unattributed_wall_ns", Derived, "ns", "median raw unattributed wall ns"),
        ("summaries.phase_profile.unattributed_wall_percent", Derived, "percent", "median unattributed wall ns divided by median profile wall ns times 100"),
        ("analysis.profile_wall_delta_percent", Diagnostic, "percent", "(profile median wall / hot median wall - 1) * 100"),
    ]
    .into_iter()
    .map(|(path, role, unit, formula_or_boundary)| MetricDefinition {
        path: path.to_string(),
        role,
        unit: unit.to_string(),
        formula_or_boundary: formula_or_boundary.to_string(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_resource() -> ResourceSample {
        ResourceSample {
            wall_ns: 10,
            user_us: 2,
            system_us: 3,
            max_rss_kib: 4,
            minor_faults: 5,
            major_faults: 6,
            voluntary_context_switches: 7,
            involuntary_context_switches: 8,
        }
    }

    fn sample_scenario() -> ProcessScenario {
        ProcessScenario {
            files: 1,
            source_bytes: 1,
            physical_lines: 1,
            output_bytes: 1,
            javac: ResourceSeries { samples: vec![sample_resource()] },
            njavac: ResourceSeries { samples: vec![sample_resource()] },
        }
    }

    fn sample_phases(value: u64) -> PhaseValues<u64> {
        PhaseValues {
            lex: value,
            parse: value,
            semantic_analysis: value,
            codegen_planning: value,
            classfile_serialization_and_plan_drop: value,
            analysis_and_syntax_drop: value,
            result_bytes_drop: value,
        }
    }

    fn sample_allocations() -> PhaseValues<AllocationMetric> {
        PhaseValues {
            lex: AllocationMetric::default(),
            parse: AllocationMetric::default(),
            semantic_analysis: AllocationMetric::default(),
            codegen_planning: AllocationMetric::default(),
            classfile_serialization_and_plan_drop: AllocationMetric::default(),
            analysis_and_syntax_drop: AllocationMetric::default(),
            result_bytes_drop: AllocationMetric::default(),
        }
    }

    fn document() -> ReportDocument {
        ReportDocument::new(
            Metadata {
                generated_at_unix_seconds: 1,
                revision: "revision\n\"quoted\"".into(),
                operating_system: "linux".into(),
                architecture: "aarch64".into(),
                cpu: "cpu \\ unicode \u{2603}".into(),
                power_mode: "full".into(),
                image_id: "sha256:image".into(),
                cpu_control: "2".into(),
                memory_control: "2g".into(),
            },
            Provenance {
                benchmark_binary_sha256: "sha256:benchmark".into(),
                njavac_binary_sha256: "sha256:njavac".into(),
                allocation_helper_sha256: "sha256:allocation".into(),
                javac_binary_sha256: "sha256:javac".into(),
                javac_version: "javac 25".into(),
            },
            WorkloadIdentity {
                fingerprint: "sha256:workload".into(),
                files: 1,
                source_bytes: 1,
                physical_lines: 1,
                nonblank_lines: 1,
                output_class_bytes: 1,
                minimal_input_fixture: "X.java".into(),
                minimal_input_source_bytes: 1,
                minimal_input_physical_lines: 1,
                minimal_input_output_bytes: 1,
            },
            BenchmarkConfiguration { samples: 1, warmup: 0, rounds: 1, allocation_rounds: 1 },
            Measurements {
                performance: PerformanceMeasurements {
                    minimal_input_fresh_process_compile: sample_scenario(),
                    whole_corpus_cli_compile: sample_scenario(),
                    hot_in_process_corpus_compile: ScalarSeries { samples_ns: vec![10] },
                },
                phase_profile: PhaseProfile {
                    samples: vec![PhaseSample {
                        wall_ns: 10,
                        phases_ns: sample_phases(1),
                        unattributed_wall_ns: 3,
                    }],
                },
                allocations: AllocationProfile {
                    phases: sample_allocations(),
                    baseline_live_bytes: 10,
                    peak_live_growth_bytes: 5,
                    final_live_bytes: 10,
                    total_requested_bytes: 0,
                    total_released_bytes: 0,
                },
            },
        )
        .unwrap()
    }

    #[test]
    fn statistics_cover_single_even_odd_and_identical_samples() {
        assert_eq!(summary(vec![]), Err("statistics require at least one sample".to_string()));
        assert_eq!(summary(vec![3.0]).unwrap().median, 3.0);
        assert_eq!(summary(vec![1.0, 3.0]).unwrap().median, 2.0);
        let odd = summary(vec![1.0, 2.0, 100.0]).unwrap();
        assert_eq!(odd.median, 2.0);
        assert_eq!(odd.median_absolute_deviation, 1.0);
        let identical = summary(vec![7.0, 7.0, 7.0]).unwrap();
        assert_eq!(identical.population_standard_deviation, 0.0);
        assert_eq!(identical.median_absolute_deviation, 0.0);
        assert_eq!(SCHEMA_VERSION, 3);
        assert_eq!(METHODOLOGY_VERSION, 3);
        let _: Summary = odd;
    }

    #[test]
    fn complete_report_round_trips_strictly_and_rejects_invalid_contracts() {
        let report = document();
        let bytes = serde_json::to_vec_pretty(&report).unwrap();
        assert_eq!(ReportDocument::from_json(&bytes).unwrap(), report);

        let mut unknown: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        unknown.as_object_mut().unwrap().insert("unknown".into(), true.into());
        assert!(ReportDocument::from_json(&serde_json::to_vec(&unknown).unwrap()).is_err());

        let mut wrong_version = report.clone();
        wrong_version.schema_version = 1;
        assert!(ReportDocument::from_json(&serde_json::to_vec(&wrong_version).unwrap()).is_err());

        let mut non_finite = report.clone();
        non_finite.analysis.profile_wall_delta_percent = f64::NAN;
        assert!(non_finite.validate().is_err());

        let mut contradictory = document();
        contradictory.summaries.hot_in_process_corpus_compile.effective_files_per_second += 1.0;
        assert!(contradictory.validate().is_err());

        let mut missing_samples = document();
        missing_samples.measurements.phase_profile.samples.clear();
        assert!(missing_samples.validate().is_err());

        let mut wrong_minimal_identity = document();
        wrong_minimal_identity.workload.minimal_input_source_bytes += 1;
        assert!(wrong_minimal_identity.validate().is_err());
    }
}
