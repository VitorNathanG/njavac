# Prerequisites

njavac's acceptance environment is Docker. Exact class bytes and behavioral
comparisons are specific to the content-pinned reference `javac` in the
reference-derived images. A host JDK, even another Java 25 distribution, is not an
acceptance reference. The archive checksum and base
image digests are owned by the root `Dockerfile`; see
[Docker and CI](../tooling/docker-and-ci.md).

## Required

Install these host tools:

- Docker with BuildKit support and permission to run containers.
- GNU Make or a compatible `make` implementation.
- Git.
- A POSIX-compatible host shell plus the standard utilities used by recipes,
  including `grep`, `sed`, `sort`, and `id`. In-container diagnostic recipes also
  use common POSIX utilities such as `cmp`, `diff`, `basename`, and `mktemp`.
- Network access for a cold image build, which resolves base images and downloads
  toolchains.

Run `make help` from the repository root to inspect the current command surface.
Its output owns target names and short invocation hints. The `Makefile` itself is
authoritative for variable defaults and which values each recipe forwards; this
guide explains when to use them.

## No host language toolchains

A host Rust toolchain and host JDK are not required for normal maintenance. The
root Dockerfile's explicit acceptance, reference, fuzz, and profile targets are the
compiler build and execution environments exposed by Make. Never substitute direct
host Cargo, `javac`, or `javap` output for repository build, compatibility, or
performance evidence.

## Docker resources

The initial reference-derived image build installs the configured GraalVM
distribution, and the first Rust-derived image compiles the binaries. Shared
stages make later target builds incremental. The benchmark and profiler targets
also constrain CPU and memory to reduce same-host variance. If the default CPU
index does not exist on the host, select an available one with
`BENCH_CPU=<index>` on either command.

Timing results are meaningful only through `make bench` or `make profile`. Host
scheduling, power mode, thermal state, and VM noise make ad hoc timing unsuitable
for regression decisions. See [Profiling](../tooling/profiling.md) for the
process-level and in-process methodologies.

## Acceptance boundary

All correctness tests run through the repository's Docker-backed Make targets.
There is no sanctioned host acceptance run and no `cargo test` substitute.

| Activity | Execution | Evidence |
| --- | --- | --- |
| Run `make image` | Docker | Acceptance-image build only |
| Run `make profile` | Docker | Controlled phase-performance evidence only |
| Compare against host `javac` | Unsanctioned | None |
| Run `make verify` | Docker | Cached, suitable for the inner loop |
| Run `make correctness` | Docker | Fresh exact-byte fixture evidence |
| Run `make bench` | Docker | Fresh exact-byte fixture evidence plus controlled same-host timing |
| Run fuzzer and worker gates | Docker | Evidence for their documented contracts |

The detailed gate selection lives in [Command surface](../tooling/command-surface.md)
and [Maintainer workflow](../contributing/workflow.md).

## Before infrastructure work

Confirm the constraints that shape the change before designing tooling:

- Acceptance remains Docker-only.
- The reference compiler remains a black box.
- Reproducibility takes precedence over adding a host-only loop.
- The Makefile remains the sanctioned command surface.
- Checked-in golden `.class` files are not used.

Changes to Docker, the reference JDK, compiler workers, benchmark isolation, or
fixture discovery can invalidate the test oracle. Treat them as compatibility
work, not routine build cleanup, and follow the [research method](../contributing/research-method.md)
and the relevant worker gates.
