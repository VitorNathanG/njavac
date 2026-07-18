# Fuzzing

The differential fuzzer generates random programs inside its modeled subset,
compiles each program with both compilers, compares exact class bytes, and invokes
an execution observer only when those bytes differ. It complements fixtures by
searching combinations maintainers did not anticipate.

## Oracle

```mermaid
flowchart TD
    Program[Seeded generated program] --> Javac[Persistent in-memory javac worker]
    Program --> Njavac[njavac in process]
    Javac --> Accepted{Did javac emit a class?}
    Accepted -->|No| Invalid[generator-invalid telemetry]
    Accepted -->|Yes| Candidate{njavac outcome}
    Candidate -->|Unsupported| Unsupported[njavac-unsupported telemetry]
    Candidate -->|Syntax diagnostic| Syntax[Hard compiler finding]
    Candidate -->|Panic| Panic[Hard compiler finding]
    Candidate -->|Class bytes| Bytes{Bytes identical?}
    Bytes -->|Yes| Exact[Exact pass]
    Bytes -->|No| Observer[Persistent execution observer]
    Observer --> Same{Observations equal?}
    Same -->|Yes| Drift[Byte-drift telemetry]
    Same -->|No| Behavior[Hard behavioral finding]
```

Javac rejection takes precedence regardless of what njavac did because it means
the generator did not produce a valid reference case. When javac accepts, the
outcomes are classified as follows:

| Outcome | Operational treatment | Product meaning |
| --- | --- | --- |
| Both emit identical bytes | Exact pass | Compatibility satisfied for this case. |
| Bytes differ, observations match | Telemetry; normal `make fuzz` can exit zero | Still a product compatibility violation. Behavioral equivalence does not satisfy njavac's byte-identity contract. |
| Bytes and observations differ | Hard finding and nonzero exit | Behavioral compiler bug as well as compatibility failure. |
| Javac rejects | `generator-invalid` telemetry | Generator escaped valid Java or the intended reference surface. |
| Javac accepts, njavac returns `Unsupported` | `njavac-unsupported` telemetry | Valid Java reached a deliberate compiler boundary. Review whether the generator exceeded current scope. |
| Javac accepts, njavac returns a syntax diagnostic | Hard compiler finding | Invalid rejection of reference-accepted input. |
| Javac accepts, njavac panics | Hard compiler finding | Internal invariant failure. |

The observer supplies empirical semantic evidence, not proof. Equal stdout,
stderr, and termination cannot reveal unobserved state. Exact bytes remain the
product requirement and the fixture acceptance oracle.

## Reproduction and control

A bare `make fuzz` chooses a fresh seed and prints it before compiling. Reproduce
the same generated sequence with the printed `SEED` value. `COUNT` controls the
number of programs and `BATCH` controls how many source units share one javac task.
The generator creates every program in a batch before either compiler runs, so a
compiler failure cannot perturb the seed-determined sequence.

Use `FUZZFLAGS` for additional options such as continuing after findings, changing
the output directory, or dumping generated sources without compiling. The binary
help is authoritative for exact flags. Parallel jobs beyond one are not
implemented; requesting them is rejected.

With keep-going enabled, the summary groups byte divergences by normalized
structural path and behavioral findings by which observation fields differ. This
is useful for a census, but a broad unexpected census is evidence that the model
or change boundary is wrong, not an invitation to patch signatures individually.

## Reference worker

`tools/FuzzJavac.java` is source-launched once by the pinned Java runtime. It keeps
one JVM hot, receives framed source strings over a pipe, and returns class bytes
from an in-memory file manager. It writes neither source nor class files to disk.
Each batch uses one javac task so compiler context setup is amortized similarly to
a CLI batch.

The class name, virtual `<Class>.java` filename, and njavac `source_file` argument
come from one generator naming chokepoint. That invariant matters because the
filename is visible in `SourceFile` and line metadata. The harness also rejects
unexpected emitted classes so a future generator cannot silently compare only one
part of a multi-class program; a missing expected class is treated as javac
rejection for that program.

The worker is an optimization, not an independent authority. `make fuzz-verify`
generates programs, compiles them through both the worker and a real pinned
`javac` CLI batch, and compares acceptance and bytes. Run it after any JDK bump or
edit to the worker, its naming, options, file manager, protocol, or batching.

The main Dockerfile does not copy either Java worker source into the image.
`make fuzz`, `make fuzz-verify`, `make fuzz-selftest`, and
`make fuzz-observe-verify` bind-mount the repository at the container workdir so
the default `tools/...` paths resolve. Running the `fuzz` image entrypoint without
that mount fails to start its workers.

## Execution observer

`tools/FuzzObserve.java` is started lazily at the first byte divergence and then
kept alive. For each reference/candidate pair it:

- Runs with JVM class verification enabled.
- Defines each class through a fresh class loader.
- Invokes static `main` with an empty argument array.
- Captures bounded stdout and stderr separately.
- Normalizes return, thrown exception, class-load failure, and timeout state plus
  exception detail.
- Supplies an empty `System.in` stream.
- Times out one class after two seconds and restarts the worker after a timeout.

Each captured output stream is capped at one MiB, and the Rust protocol reader
rejects response frames over 16 MiB. A reference timeout prevents the candidate
from running in that process; the Rust driver restarts and reverses the request so
both sides can still be observed.

This in-process isolation is valid only for the current generator, which cannot
read input, call `System.exit`, create threads, or deliberately mutate persistent
JVM-global state. A language rung that enables any of those capabilities must
strengthen or replace the execution boundary before enabling generation.

`make fuzz-observe-verify` compiles controlled probes with the reference worker and
checks return, stdout difference, invalid class loading, throws, reference and
candidate timeouts, and successful post-timeout restart.

## Generator boundary

The current model covers the eight primitive types, numeric and bitwise
expressions, casts, comparisons, boolean operations, assignments, compound
assignments, increment/decrement statements, printing, and `if`/`else` shapes.
Declarations remain at method-body top level; branch-local declarations are
disabled. Ternary expressions and loops are disabled.

Boolean generation distinguishes branch booleans from value booleans so it does
not manufacture unsupported nonempty-stack materialization. Deliberate grouping
is represented explicitly because grouping can change javac's bytecode. Generated
mutations and branch choices are printed to maximize the observer's visible trace.

This is a generator coverage boundary, not the authoritative language-support
ledger. A feature is not proven merely because the fuzzer can generate it, and a
feature absent from the generator is not necessarily unsupported by the compiler.

## Artifacts

The Make targets mount the repository, so the default ignored `fuzz-out/`
directory persists on the host.

| Outcome | Artifact |
| --- | --- |
| Behavioral finding | Raw `<Class>.java`, structural `<Class>.diff` when available, and `<Class>.observe` at the output root. |
| Syntax rejection or panic | Source and details under `compiler-findings/syntax-error/` or `compiler-findings/internal-panic/`. |
| Unsupported case | Up to the first 20 sources and diagnostics under `unsupported/`. |
| Worker/CLI mismatch | Up to the first 20 sources and available `.cli.class` and `.worker.class` files under `worker-mismatch/`. |
| Self-test | A synthetic minimized Java source and structural diff at the output root. |
| Byte-only drift | Signature and example in terminal telemetry; no normal finding bundle is written. |

Behavioral findings intentionally remain raw because the current minimizer does
not have an observation-aware predicate. Byte-only reduction could preserve a
different harmless byte drift while erasing the behavior bug. Hand-reduce the
case while repeatedly checking the same observations, then create the minimal
documented fixture described in [Fixtures and Goldens](fixtures-and-goldens.md).

## Which fuzzer gate to run

| Change | Gate |
| --- | --- |
| Compiler behavior or language rung | `make fuzz` with a recorded seed on failure |
| JDK, javac worker, worker protocol, or virtual source naming | `make fuzz-verify` |
| Finding capture, minimization, or report writing | `make fuzz-selftest` |
| Observer, timeout, output capture, or execution isolation | `make fuzz-observe-verify` |

Fuzzer compile-time statistics are useful operational feedback but are not a
benchmark. Use [Profiling](profiling.md) or `make bench` for performance claims.
