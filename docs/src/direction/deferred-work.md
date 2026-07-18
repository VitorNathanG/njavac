# Deferred Work

These improvements are worthwhile but are not part of the ordered active
sequence. They have no priority order. When an item becomes a prerequisite or is
explicitly selected, move it to [Active Work](active-work.md) rather than copying
it.

Language coverage belongs in [Language Rungs](language-rungs.md), not here.

## Toolchain and development loop

- **Define a sanctioned rustfmt surface.** The repository is not normalized to
  the current host rustfmt, so `cargo fmt --all` rewrites unrelated files. Pin the
  formatter and configuration through the repository toolchain, expose it through
  `make fmt-check`, and decide separately whether to perform one explicit
  normalization change.
- **Automate baseline/current profiling comparisons.** Add a helper such as
  `make profile-compare BASE=<rev> PHASE=<phase>` that builds an isolated baseline
  image and compares profile minima under the same container controls, making
  regressions easier to distinguish from residual host noise.
- **Guard golden-cache freshness.** `make verify` currently records only when its
  Docker volume is empty. Detect fixture or pinned-JDK changes and refresh the
  cache instead of silently comparing against stale goldens.

## Structural diagnostics

- **Disassemble `code[]` in classdiff.** Decode instructions so a `Code`
  divergence identifies the opcode and operands at the differing byte instead of
  reporting only a raw offset.

## Fuzzer workflow

- **Add observation-aware minimization.** Recompile and re-observe each candidate,
  then shrink expressions through same-typed children, casts, and literals toward
  simple values so behavioral findings become fixture-ready.
- **Add a generator-validity smoke gate.** Run a small deterministic corpus that
  expects approximately zero `generator-invalid` cases, catching invalid Java
  from new generator arms before it silently lowers fuzz yield.
- **Generate grouped String arguments compositionally.** Represent string
  literals in the expression model so grouping and minimization exercise them
  instead of routing them through a separate print-only path.
- **Replay one generated case.** Support rerunning and minimizing a specific case
  from a seed without regenerating and compiling every preceding case.
- **Run a multi-seed census.** Add a command that runs several random seeds and
  reports the union of structural and observational signatures with one
  reproducer seed per signature.
- **Strengthen execution isolation when the generator reaches JVM-global
  effects.** Before generated programs can read `System.in`, call `System.exit`,
  create threads, or mutate process-global state, replace the in-process
  class-loader boundary with disposable execution processes and parent-enforced
  timeouts.
