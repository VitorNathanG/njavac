# Benchmarking and Profiling

`make benchmark` is njavac's single performance command. It runs fresh
correctness, uninstrumented process and compiler-core measurements, isolated
phase profiling, and isolated allocation profiling. One command and report cover
these measurements without using instrumented timings as authoritative latency or
throughput.

## Report sequence

```mermaid
flowchart LR
    Correct[Fresh byte correctness] --> Performance[Uninstrumented performance]
    Performance --> Phase[Phase-instrumented pass]
    Phase --> Alloc[Allocation-instrumented helper]
    Alloc --> Report[Terminal + JSON report]
```

| Pass | What it measures | Evidence boundary |
| --- | --- | --- |
| Correctness | One live pinned-javac and njavac compilation followed by exact class-byte comparison. | Fresh fixture compatibility evidence. Not timed. |
| Performance | Fresh-process startup, whole-corpus CLI behavior, and hot in-process compilation. | Controlled same-host latency, throughput, CPU, and peak-RSS evidence. No phase timers or counting allocator. |
| Phase | Exclusive lex, parse, semantic-analysis, codegen-planning, and class-file-emission time around the production pipeline. | Attribution evidence. Its wall time is compared with the earlier uninstrumented hot measurement, but the delta also includes temporal host noise. |
| Allocation | Allocation calls, requested bytes, and peak compiler-managed live allocation through an internal helper with a counting global allocator. | Allocation evidence only. Its timing is not performance evidence. |

Any failed correctness, warm-up, measured compiler process, profiled compilation,
or allocation-helper invocation fails the command. A failed compiler cannot be
reported as an unusually fast sample.

## Uninstrumented measurements

The fresh-process scenarios invoke each compiler through a new resource-helper
process for every sample. The helper times only the compiler child and reads
Linux child resource accounting after it exits. Samples include:

- Wall time.
- User and system CPU time.
- Peak resident set size.
- Minor and major page faults.
- Voluntary and involuntary context switches.
- Successful exit status.

The startup scenario compiles `fixtures/basics/Empty.java`. "Fresh process" does
not mean cold filesystem or VM caches; Docker cannot reliably flush the host and
Docker-VM caches. The batch scenario compiles the complete fixture corpus in one
CLI invocation and includes source reads and class writes. Before each warm-up and
sample, the runner removes the expected outputs outside the timed region; after
the child exits, it requires every expected class and byte length.

The hot compiler-core scenario loads sources before timing and repeatedly calls
`njavac::compile` in-process. It excludes process startup and source/class-file
I/O. The report normalizes the measured corpus passes into files/s, input MB/s,
physical lines/s, and output MB/s.

Every scenario reports raw samples plus minimum, median, mean, population standard
deviation, and median absolute deviation. The median is the primary displayed
comparison. Compiler order alternates across samples. The exact sample, warm-up,
round, allocation-round, CPU, and memory controls belong to `make help`, the
`Makefile`, and `benchmark --help`.

## Phase attribution

The library's ordinary `compile` entry point uses a statically dispatched no-op
observer. The optimizer therefore sees no timers or allocation counters on the
production path. The phase pass calls the same pipeline with a timing observer at
these boundaries:

| Phase | Endpoint |
| --- | --- |
| `lex` | Tokens produced. |
| `parse` | Compilation unit and expression arena produced. |
| `sema` | Semantic analysis produced. |
| `codegen plan` | Ordered class plan produced. |
| `classfile emit` | Final class bytes serialized. |
| `cleanup` | Compiler-owned syntax and analysis state released before returning class bytes. |
| `result drop` | Benchmark caller releases the returned class-byte vector after consuming it. |

The report measures each phase directly during the same compilation. It does not
subtract independently selected cumulative minima, so phase values cannot become
negative through cross-trial noise. Phase rows include exclusive ns/file, share
of observed phase time, effective files/s, source MB/s, physical lines/s,
allocation calls/file, requested allocation bytes/file, and deallocated bytes/file.

Timer calls perturb the profiled compilation. The report compares phase-pass wall
time with the earlier uninstrumented hot measurement and prints the observed
wall-time delta. That delta includes temporal host noise and is not an isolated
timer-cost measurement. A nonpositive delta means noise exceeded the profiler
cost; it is not negative overhead. Phase time and phase throughput remain
attribution data; only the uninstrumented sections support headline performance
claims.

## Allocation attribution

`benchmark_alloc` is an internal image helper, not a public command. Its counting
allocator is present only in the allocation process, so the uninstrumented and
phase-timing binaries do not pay an allocator-wrapper branch. The helper loads the
corpus before enabling counters, then snapshots allocation calls, requested bytes,
and deallocated bytes at production phase boundaries.

Peak live allocation covers allocations made while compiler tracking is enabled.
It is not process RSS, and per-phase requested bytes are not a retained-heap
measurement. The helper requires final tracked live allocation to return to zero.
Process peak RSS remains in the uninstrumented fresh-process report.

## Workload identity and artifacts

The current benchmark uses the exact fixture snapshot copied into the acceptance
image. The report records file count, UTF-8 source bytes, physical lines, nonblank
lines, emitted class bytes, and an ordered content fingerprint. Fixture changes
change the fingerprint, so results with different fingerprints are not directly
comparable.

`make benchmark` writes a revision- and UTC-time-named structured result under
`benchmark-results/` by default. Reports therefore accumulate without becoming
tracked repository content. The ignored host directory is bind-mounted into the
benchmark container and the container runs as the host UID/GID. The JSON contains
schema and methodology versions, generation time, acceptance image ID, environment and control
metadata, workload identity, benchmark/njavac/allocation-helper/javac binary
SHA-256 fingerprints, javac version, configuration, every raw process and hot sample,
phase samples, allocation totals, and the observed profile wall-time delta.

The command does not repeat the uninstrumented workload or assign a stability
verdict. `SAMPLES` describes the distribution within this one performance pass.
Retain raw samples and reproduce surprising movement before making a performance
claim.

## Reproducible comparisons

Keep these stable when comparing reports:

- Host machine and architecture.
- Docker engine and VM allocation.
- Benchmark image contents and compiler revision.
- CPU, memory, sample, warm-up, and round controls.
- Corpus fingerprint.
- Background workload and thermal state.
- Host power mode.

`BENCH_POWER_MODE` records the maintainer-supplied power-mode label. The default is
`unknown`; set it when retaining performance evidence. macOS Low Power Mode can
materially change throughput, so never compare a power-saving run with a full-
performance run as though the difference came from the compiler.

Docker CPU affinity, quota, memory, swap, and PID controls reduce noise but do not
control host load, hypervisor scheduling, power, or thermal state. Compare nearby
runs on the same host. The benchmark does not produce deterministic or portable
timings, a composite score, or a CI-safe performance gate.

## Trust boundary

Only the initial fresh byte comparison is compatibility evidence. The remaining
report sections measure performance and resource use. Faster phase, allocation,
or process results cannot establish Java or class-file correctness. The shared
image, runtime controls, and residual host boundary are described in
[Docker and CI](docker-and-ci.md).
