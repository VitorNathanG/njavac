# FUTURE_WORK.md - deferred improvements

This file collects worthwhile improvements that are **not part of the active
compiler sequence**. Items have no priority order. Add and remove them freely; when
an item becomes a prerequisite or is explicitly selected for implementation, move
it to ROADMAP.md.

Language coverage does not belong here; it lives in README.md. Target architecture
and feature-triggered subsystem boundaries live in ARCHITECTURE.md.

## Toolchain and Development Loop

- **Define a sanctioned rustfmt surface.** The repository is not normalized to the
  current host rustfmt, so `cargo fmt --all` rewrites unrelated files. Pin the
  formatter and configuration through the repository toolchain, expose it through
  `make fmt-check`, and decide separately whether to perform one explicit
  normalization change.
- **Automate baseline/current profiling comparisons.** Add a helper such as
  `make profile-compare BASE=<rev> PHASE=<phase>` that builds an isolated baseline
  and compares same-host profile minima. This would make performance regressions
  easier to distinguish from host noise.
- **Guard golden-cache freshness.** `make verify` currently records only when the
  Docker volume is empty. Detect fixture or JDK changes and refresh the cache rather
  than silently comparing against stale goldens.

## Structural Diagnostics

- **Disassemble `code[]` in classdiff.** Decode the instruction stream so a `Code`
  divergence names the opcode and operands at the differing byte rather than
  reporting only a raw offset.

## Fuzzer Workflow

- **Add observation-aware minimization.** Recompile and re-observe each candidate,
  then shrink expressions through same-typed children, casts, and literals toward
  simple values so behavioral findings become fixture-ready.
- **Add a generator-validity smoke gate.** Run a small deterministic corpus that
  expects approximately zero `generator-invalid` cases, catching invalid Java from
  new generator arms before it silently lowers fuzz yield.
- **Generate grouped String arguments compositionally.** Represent String literals
  in the expression model so grouping and minimization exercise them rather than
  routing them through a separate print-only path.
- **Replay one generated case.** Support rerunning and minimizing a specific case
  from a seed without regenerating and compiling every preceding case.
- **Run a multi-seed census.** Add a command that runs several random seeds and
  reports the union of structural and observational signatures with one reproducer
  seed per signature.
- **Strengthen execution isolation when the generator reaches JVM-global effects.**
  Before generated programs can read `System.in`, call `System.exit`, create
  threads, or mutate process-global state, replace the in-process class-loader
  boundary with disposable execution processes and parent-enforced timeouts.
