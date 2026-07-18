# Profiling

njavac has two intentionally different performance measurements built from the
same pinned Rust stage and run under the same CPU and memory controls. `make bench`
uses the reference-bearing acceptance image to measure process-level wall clock;
`make profile` uses a JDK-free image to measure the compiler pipeline itself
in-process.

## Choose the measurement

| Question | Tool | What it includes |
| --- | --- | --- |
| How long does one normal whole-suite compiler invocation take on this controlled host? | `make bench` | Repeated samples, each including process startup, dynamic linking or JVM startup, and compilation of the complete fixture corpus. |
| Which njavac phase consumes compiler time? | `make profile` | Hot in-process lexing, parsing, semantic analysis, codegen planning, and serialization. No process spawn per compile. |
| Do exact-output fixtures still match javac? | `make correctness` | Fresh byte comparison, not a performance measurement. |

For tiny inputs, process startup dominates the benchmark, especially javac's JVM
startup. Do not interpret `make bench` as a phase profile. Conversely, the hot
profile excludes normal CLI startup and cannot replace benchmark timing.

`make bench` first runs the fresh correctness pass. It then warms each compiler
and collects multiple whole-suite process samples: the configured defaults use
many more njavac samples than javac samples because javac startup is expensive.
CPU affinity, quota, memory, swap, and PID controls reduce noise but do not control
host load, hypervisor scheduling, power, or thermal state. Compare nearby runs on
the same host and configuration; do not call the result deterministic.

The timed closures discard compiler output and ignore each timed process's success
status. The initial fixture result remains authoritative for the tested pass,
but a compiler that fails during a later warm-up or measured invocation can still
produce a timing sample and a successful benchmark command. Treat suspiciously
short samples or intermittent failures as invalid timing evidence and reproduce
with explicit compiler diagnostics.

## Profile method

`make profile` builds the explicit `profile` Dockerfile target, starts its binary
with the `BENCH_CPU` and `BENCH_MEM` controls, loads the fixture snapshot copied
into that image, and invokes compiler stages directly. The image uses the pinned
Debian runtime but deliberately omits the reference JDK and unrelated tools. The
profiler warms the pipeline once, then processes every fixture for each configured
round and trial.

```mermaid
flowchart LR
    Corpus[Fixture corpus] --> Warm[One warm-up compile per fixture]
    Warm --> Prefix[Cumulative phase timing]
    Prefix --> Trials[Repeat trials]
    Trials --> Minimum[Take minimum elapsed time]
    Minimum --> Difference[Difference cumulative phases]
    Difference --> Report[ns per compile and throughput]
```

In all-phase mode, the profiler measures cumulative prefixes and takes their
differences:

| Reported phase | Cumulative endpoint |
| --- | --- |
| `lex` | Tokens produced |
| `parse` | AST produced |
| `sema` | Semantic analysis produced |
| `codegen` | Class-file plan produced |
| `classfile emit` | Final class bytes serialized |

Selecting one phase measures only that cumulative prefix, which is useful when
attaching an external sampler without first spending time on every prefix. The
exact phase names and positional syntax belong to `make help` and `profile
--help`.

Progress is reported in roughly ten measured chunks per trial. Progress output is
outside the accumulated duration. Final phase values use the minimum elapsed time
across trials because scheduler, thermal, and background-process noise can add
latency but cannot make the measured work intrinsically faster.

The report includes nanoseconds per file compile, phase percentages in all-phase
mode, source MB/s, lines/s, and compiles/s. Corpus size is part of the workload;
results before and after fixture changes are not directly comparable without
accounting for that change.

## Reproducible comparisons

Keep all of these stable when comparing revisions:

- Host machine and architecture.
- Docker engine, VM allocation, and profile image contents.
- `BENCH_CPU` and `BENCH_MEM`.
- Fixture corpus.
- `ROUNDS`, `TRIALS`, and selected phase.
- Background workload and thermal state.
- macOS power mode.

Mac power mode materially changes throughput. Low Power Mode and regular/full
performance mode can produce results that differ by roughly a factor of two on the
same machine. Record the mode with every result and compare only runs made in the
same mode. Do not mix a power-saving baseline with a full-performance candidate
or present the difference as a compiler improvement.

Increase rounds when a selected phase is too short relative to timer and scheduler
noise. Increase trials when the minimum is unstable. Compare minimums from nearby
runs on the same machine rather than isolated single measurements.

## Trust boundary

`make profile` uses its controlled JDK-free image and does not invoke the
configured javac or compare class bytes. A successful or faster profile is not
compatibility evidence. Run the required correctness or fuzz gates separately.
The shared build stage, container controls, and residual host boundary are
described in [Docker and CI](docker-and-ci.md).
