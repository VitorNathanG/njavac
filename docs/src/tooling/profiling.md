# Benchmarking and Profiling

`make benchmark` is njavac's single performance command. This page owns its
methodology, persisted report contract, outcome interpretation, and comparison
rules. The command runs uninstrumented measurements, phase attribution, and
allocation attribution before it publishes one report. It does not assert language
or class-file correctness.

## Test and benchmark separation

`make test` is the repository-wide deterministic pass/fail gate. It owns Rust
unit and integration tests, fresh exact-byte correctness, instrumentation
equivalence, fixed-seed fuzzer checks, and documentation validation. Run it before
retaining benchmark evidence.

`make benchmark` is excluded from `make test`. A benchmark may fail when it cannot
complete or validate a measurement, but it must not become another correctness
gate. Its output contains performance and resource evidence only. A future
performance pass/fail policy must compare compatible reports using an explicitly
accepted performance threshold; timing must never determine whether a correctness
test passes.

## Run and retain a benchmark

Run the complete controlled command from the repository root:

```sh
make benchmark BENCH_POWER_MODE=full
```

`BENCH_POWER_MODE` is a maintainer-supplied label. Set it to the actual host mode
when retaining evidence. The default `unknown` is suitable for smoke runs, not for
comparison evidence. `make benchmark-help` prints the current Make controls,
effective defaults, Docker controls, and in-image binary help.

The command writes one no-clobber JSON file under ignored
`benchmark-results/`. Its default name contains the full Git revision, an explicit
`-dirty` marker when applicable, UTC time, and a run identifier. The runner
serializes the complete document with serde, writes and syncs a same-directory
temporary file, then atomically hard-links it to the final name. An interrupted or
failed write cannot appear at the final `.json` path. An existing final path is
never overwritten.

`make benchmark` does not accept `FILE`. Use
`make correctness FILE=fixtures/.../Case.java` for focused exact-byte testing.

## Work units and controls

```mermaid
flowchart LR
    Performance[Uninstrumented performance]
    Performance --> Phase[Timing-observer attribution]
    Phase --> Allocation[Counting-allocator attribution]
    Allocation --> Document[Validated ReportDocument]
    Document --> Publish[Terminal + atomic JSON publication]
```

The public Make target has one mode: complete performance/resource reporting.
The in-image harness retains explicit correctness, golden-cache, and
instrumentation-verification modes for `make test`, `make correctness`,
`make verify`, and `make record`; those modes do not publish performance reports.
`--json`, `--samples`, `--warmup`, `--rounds`, and `--allocation-rounds` apply only
to report mode. Malformed, missing, zero where positive is required, and
overflowing values return status 2 rather than panicking. `--warmup 0` is valid.

| Control | Work unit |
| --- | --- |
| `SAMPLES` | Raw observations for each process scenario, hot compilation, and phase profile |
| `WARMUP` | Untimed process invocations and hot corpus compilations before samples |
| `ROUNDS` | Complete corpus compilations inside each hot and phase sample |
| `ALLOCATION_ROUNDS` | Complete corpus compilations in the allocation helper |
| `BENCH_CPU` | Docker-visible pinned CPU index; the recipe also applies a one-CPU quota |
| `BENCH_MEM` | Container memory limit and identical memory-plus-swap limit |
| `RESULTS`, `RESULT_FILE` | Host result directory and no-clobber JSON filename |

The workload is the recursively discovered fixture snapshot copied into the
acceptance image. Discovery propagates directory-entry and metadata errors,
requires at least one source, sorts paths, and rejects duplicate class basenames
before touching flat output directories. The report records source and output
quantities, the selected minimal-input fixture and its quantities, plus an ordered
content fingerprint.

## Timed and excluded work

| Measurement | Timed boundary | Excluded work |
| --- | --- | --- |
| Minimal-input fresh-process compile | Child process start through successful exit while compiling `fixtures/basics/Empty.java` | Output cleanup and output-existence validation |
| Whole-corpus CLI compile | One child process start through successful exit for every fixture | Output cleanup and output-existence validation |
| Hot in-process corpus compile | Configured calls to `njavac::compile` over already loaded sources, including returned-byte destruction | Process startup, source reads, and class writes |
| Phase profile | Configured calls to the production pipeline with direct stage timers, including caller-owned result-byte destruction | Source reads and output writes |
| Allocation profile | No timing claim; counters cover configured observed compilations after a live-byte baseline snapshot | Fixture loading and helper process startup |

Process samples run through a fresh resource-helper process. Successful measured
compiler output remains discarded. If a measured child fails, the parent replays
the exact command outside the timed region and reports compiler identity, scenario,
warm-up or sample number, argument-preserving command text, status, and stderr.
Every measured process output is required to exist. Byte equivalence belongs to
`make test`, not the timed command.

Before measurement, executable resolution, binary fingerprints, javac version,
report destination availability, source loading, and an untimed ordinary njavac
compile used only for output-size normalization are completed. Instrumentation
equivalence is verified separately by `make test`.

Compiler-owned phase events must form this exact start/finish sequence for every
successful compilation:

| JSON phase | Exact boundary |
| --- | --- |
| `lex` | Source lexing through token production |
| `parse` | Token consumption through compilation-unit and expression-arena production |
| `semantic_analysis` | Semantic attribution through `Analysis` production |
| `codegen_planning` | Backend preflight and lowering through ordered `ClassPlan` production |
| `classfile_serialization_and_plan_drop` | Consuming plan serialization, including destruction of consumed plan state |
| `analysis_and_syntax_drop` | Explicit destruction of compiler-owned analysis and syntax state |
| `result_bytes_drop` | Benchmark-caller destruction of the already validated returned byte vector |

Missing, duplicate, nested, and out-of-order callbacks fail. A compiler diagnostic
may produce only the well-formed prefix ending at the failing compiler-owned
phase. `result_bytes_drop` is not a `CompilePhase` and remains benchmark-owned.

The allocation helper accounts from process startup, loads fixtures while
accounting, then snapshots live bytes immediately before measured work. Final live
bytes must equal that baseline exactly; underflow fails instead of saturating.
Peak live growth is `maximum live bytes - baseline live bytes`, not process RSS.
A successful realloc counts one requested allocation of the new size and one
release of the old layout size. A failed realloc changes no counters because the
original allocation remains valid. Per-phase releases may exceed requests when an
allocation crosses a phase boundary.

## Terminal and JSON field definitions

The canonical serde `ReportDocument` is schema version 3 and methodology version
3. Strict parsing rejects unknown fields, missing required fields, incompatible
versions, positional phase arrays, non-finite values, and an allocation final-live
value that differs from its baseline. Terminal rendering and JSON serialization
consume the same document and its once-computed summaries.

Schema version changes when structure, required fields, field names, units, enum
values, meaning, or interpretation changes. Methodology version changes when timed
boundaries, scheduling, corpus selection, warm-ups, rounds, statistics, allocation
accounting, or throughput denominators change. JSON whitespace or object-order
changes alone require neither bump.

| JSON path or family | Role | Unit | Definition |
| --- | --- | --- | --- |
| `schema_version`, `methodology_version` | Invariant | integer | Exact parser and measurement contracts |
| `evidence_status` | Invariant | enum | Currently always `exploratory` |
| `metadata.generated_at_unix_seconds` | Diagnostic | Unix seconds | Report construction time |
| `metadata.revision` | Diagnostic | text | Full Git SHA plus `-dirty` when applicable |
| Remaining `metadata.*` | Diagnostic | text | OS, architecture, host CPU label, power label, image ID, and Docker controls |
| `provenance.*_sha256` | Invariant | SHA-256 | Exact benchmark, njavac, allocation-helper, and javac binaries |
| `provenance.javac_version` | Invariant | text | Successful preflight `javac -version` output |
| `workload.files` | Diagnostic | count | Number of fixtures |
| `workload.source_bytes`, `output_class_bytes` | Diagnostic | bytes | UTF-8 input and ordinary njavac output-size totals used for normalization |
| `workload.physical_lines`, `nonblank_lines` | Diagnostic | count | Formatting-sensitive source line totals |
| `workload.minimal_input_*` | Diagnostic | path, bytes, or count | Identity and exact quantities of the one-fixture fresh-process scenario |
| `workload.fingerprint` | Invariant | text | Ordered path-and-source workload fingerprint |
| `configuration.*` | Diagnostic | count | Effective samples, warm-ups, rounds, and allocation rounds |
| `outcomes.measurement` | Invariant | enum | All measurement sections completed |
| `measurements.performance.*.*.samples.*.wall_ns` | Primary | ns | Raw compiler-child wall sample |
| `measurements.performance.*.*.samples.*.user_us`, `system_us` | Secondary | us | Linux `RUSAGE_CHILDREN` CPU counters |
| `measurements.performance.*.*.samples.*.max_rss_kib` | Secondary | KiB | Linux `ru_maxrss` peak resident set |
| Process fault and context-switch fields | Diagnostic | count | Linux `RUSAGE_CHILDREN` counters |
| `hot_in_process_corpus_compile.samples_ns` | Primary | ns | Raw configured-round hot sample |
| `phase_profile.samples.wall_ns` | Diagnostic | ns | Raw wall time around one instrumented sample |
| `phase_profile.samples.phases_ns.*` | Diagnostic | ns | Raw exclusive named-phase duration |
| `phase_profile.samples.unattributed_wall_ns` | Diagnostic | ns | Wall time minus all named phase durations; negative is invalid |
| `allocations.phases.*.allocation_calls` | Primary | count | Successful allocation/reallocation requests by phase |
| Allocation requested/released fields | Primary/secondary | bytes | Counter deltas under the documented realloc rule |
| `baseline_live_bytes`, `final_live_bytes` | Invariant | bytes | Values required to be equal |
| `peak_live_growth_bytes` | Primary | bytes | Peak live bytes above baseline |
| `total_requested_bytes`, `total_released_bytes` | Primary/secondary | bytes | Complete allocation-workload deltas |
| Summary `wall_ns` fields | Derived | ns | Minimum, median, mean, population standard deviation, and median absolute deviation |
| `median_cpu_total_us` | Derived | us | Median of `user_us + system_us` per process sample |
| `median_max_rss_kib` | Derived | KiB | Median raw peak RSS |
| `median_corpus_pass_wall_ns` | Derived | ns | Hot median sample divided by `ROUNDS` |
| `effective_files_per_second` | Derived | files/s | File count divided by applicable median wall time |
| `normalized_source_mb_per_second` | Derived | decimal MB/s | Source bytes divided by applicable median wall time |
| `normalized_output_mb_per_second` | Derived | decimal MB/s | Ordinary njavac output bytes divided by applicable median wall time |
| `physical_lines_per_second` | Derived | lines/s | Physical line count divided by applicable median wall time |
| Phase `median_ns_per_file` | Derived | ns/file | Phase median divided by `ROUNDS * workload.files` |
| Phase `share_percent` | Derived | percent | Phase median divided by sum of named phase medians |
| Phase allocation-per-file fields | Derived | count or bytes/file | Phase total divided by `ALLOCATION_ROUNDS * workload.files` |
| `median_unattributed_wall_ns` | Derived | ns | Median raw unattributed wall value |
| `unattributed_wall_percent` | Derived | percent | Unattributed median divided by profile wall median |
| `profile_wall_delta_percent` | Diagnostic | percent | `(profile wall median / hot wall median - 1) * 100` |
| `metric_contract` | Invariant | structured text | Machine-readable role, unit, and formula/boundary inventory |
| `warnings` | Diagnostic | structured list | Reserved typed warnings; currently empty |

Normalized MB/s values translate fixed in-memory quantities; they do not claim
filesystem bandwidth. Physical lines are not semantic LOC. Effective per-phase
rates are hypothetical translations of one exclusive phase duration, not complete
pipeline throughput. Instrumented timing is diagnostic attribution; only the
uninstrumented wall samples are latency evidence.

## Outcome interpretation

A JSON report exists only after every warm-up and sample, measured-process output
existence, phase-sequence validation, allocation balance, document validation,
serialization, flush, sync, and final no-clobber publication succeed. Operational
failure returns status 1 and leaves no accepted final report. Usage failure returns
status 2. Report success is not a correctness result.

Every current report says `evidence_status: exploratory`. This is not a warning or
a failed run; it means the project has not designated a numerical baseline or
regression threshold. Negative profile wall delta means temporal host noise
exceeded observable timer cost, not negative instrumentation overhead.

## Compare two reports

Before comparing numbers, require equality of:

- `schema_version` and `methodology_version`.
- Complete successful outcomes and `evidence_status` suitable for the intended use.
- Workload fingerprint and all workload quantities.
- Architecture, image ID, CPU/memory controls, power mode, and effective configuration.
- Compiler and runner provenance expected for the comparison.

Then inspect raw distributions, medians, population standard deviation, and median
absolute deviation. Compare nearby complete runs on the same host. Docker controls
do not fix host load, virtualization scheduling, power, thermal state, or cache
state. Do not compare a power-saving run with a full-performance run as compiler
movement. The benchmark provides no composite score and no CI-safe timing gate.

## Baseline eligibility

No current numerical baseline exists. Reports created before schema/methodology 3,
reports from dirty revisions, reduced smoke configurations, and all files already
present under local `benchmark-results/` are exploratory. They must not define a
threshold or be rewritten to resemble the current contract.

A future baseline requires all of these before designation:

- A clean full commit SHA and exact retained raw reports.
- Frozen schema, methodology, workload, and maintainer-selected acceptance policy.
- Matching workload, architecture, image, CPU, memory, power, and configuration
  across the retained run series.
- A passing `make test` result for the clean revision under evaluation.
- Complete measurement and allocation-integrity outcomes in every retained report.
- Review of raw distributions under the accepted sampling methodology.

The sampling acceptance policy and historical workload/cohort policy remain open
maintainer decisions. Until those decisions are made, schema 3 reports remain
exploratory even when every operational check passes.

## Glossary

| Term | Meaning |
| --- | --- |
| Sample | One recorded observation in a scenario |
| Warm-up | Equivalent untimed work before recorded samples |
| Round | One complete fixture-corpus compilation inside a hot, phase, or allocation work unit |
| Scenario | One exact timed boundary and workload shape |
| Corpus | Canonically ordered source set represented by the workload fingerprint |
| Fresh process | A new compiler process; filesystem and VM caches are not flushed |
| Hot compile | In-process `njavac::compile` calls over already loaded source text |
| Phase attribution | Instrumented exclusive timing around named production-pipeline boundaries |
| Allocation attribution | Counting-allocator deltas by named phase, without a timing claim |
| Baseline | Explicitly accepted retained report series used for later regression comparison; none exists today |

No benchmark report section is compatibility evidence. Use `make test` for the
deterministic repository gate and [Docker and CI](docker-and-ci.md) for image and
runtime mechanics.
