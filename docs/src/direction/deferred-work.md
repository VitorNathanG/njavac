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
- **Automate baseline/current benchmark comparisons.** Build an isolated baseline
  compiler and compare its unified benchmark JSON with the candidate under the
  same current runner, corpus fingerprint, and container controls. Interleave
  subject order and report per-metric deltas without introducing a composite
  score. Decide whether immutable versioned workload cohorts are necessary only
  if evolving fixture fingerprints become a concrete obstacle.
- **Harden capability-image runtime boundaries.** Add read-only diagnostic mounts,
  disabled networking, dropped capabilities, `no-new-privileges`, explicit PID
  limits, and tmpfs scratch paths. Follow with non-root users and read-only root
  filesystems only after golden-volume and fuzzer-output ownership are modeled and
  regression-tested.
- **Harden Docker build inputs and caches.** Qualify architecture-dependent cache
  IDs, lock concurrent cache sharing, pin the Dockerfile frontend by digest, add
  Dockerfile-specific ignore files, and attach useful OCI source/toolchain labels.
  Introduce `docker-bake.hcl` only when multi-target or multi-platform duplication
  makes direct Make-wrapped builds difficult to maintain.
- **Expand CI image assurance.** Export and restore BuildKit caches, pin workflow
  actions by commit, build both supported architectures after Docker or JDK
  changes, and keep performance evidence on native architecture rather than
  emulation.
- **Add narrower runtime images only on concrete demand.** A classdiff-only image
  can reduce cold diagnostic setup, and a minimal njavac image can support
  distribution. Do not add either until a measured workflow or product requirement
  justifies another capability target.
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
