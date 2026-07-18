# Prerequisites

njavac's acceptance environment is Docker. The repository pins the reference
`javac` and build toolchain in its image because class-file identity is specific
to an exact compiler build. A host JDK, even another Java 25 distribution, is not
an acceptance reference.

## Required

Install these host tools:

- Docker with BuildKit support and permission to run containers.
- GNU Make or a compatible `make` implementation.
- Git.
- Network access for the first image build, which downloads the pinned
  toolchains.

Run `make help` from the repository root to inspect the current command surface.
The `Makefile` is authoritative for target names, variables, and invocation
syntax; this guide explains when to use them.

## Optional local tools

A host Rust toolchain is useful for local compiler-internal debugging through
`make check` and for local profiling through `make profile`. These are not test
or acceptance commands. Their output cannot establish byte identity because they
do not select the repository-pinned reference `javac`.

A host JDK is not required for normal maintenance. Never use host `javac` or
`javap` output as compatibility evidence.

## Docker resources

The initial image build installs the pinned GraalVM distribution and compiles the
Rust binaries, so it is slower and more network-intensive than later cached
builds. The benchmark target also constrains CPU and memory for repeatability.
If its default CPU index does not exist on the host, select an available one with
the `BENCH_CPU` variable shown by `make help`.

Timing results are meaningful only in the benchmark harness. Host scheduling,
power mode, thermal state, and JVM startup noise make ad hoc timing unsuitable
for regression decisions. See [Profiling](../tooling/profiling.md) for the
separate in-process methodology.

## Acceptance boundary

All correctness tests run through the repository's Docker-backed Make targets.
There is no sanctioned host acceptance run and no `cargo test` substitute.

| Activity | Host execution allowed? | Acceptance evidence? |
| --- | ---: | ---: |
| Build binaries with `make check` | Yes | No |
| Run a locally built `njavac` while debugging | Yes | No |
| Profile with `make profile` | Yes | No |
| Compare against host `javac` | No | No |
| Run `make verify` | Through Docker | Cached, suitable for the inner loop |
| Run `make correctness` | Through Docker | Yes, fresh and authoritative |
| Run `make bench` | Through Docker | Yes, plus controlled timing |
| Run fuzzer and worker gates | Through Docker | Yes for their documented contracts |

The detailed gate selection lives in [Command surface](../tooling/command-surface.md)
and [Maintainer workflow](../contributing/workflow.md).

## Before infrastructure work

Confirm the constraints that shape the change before designing tooling:

- Acceptance remains Docker-only.
- The reference compiler remains a black box.
- Reproducibility takes precedence over a faster host-only loop.
- The Makefile remains the sanctioned command surface.
- Checked-in golden `.class` files are not used.

Changes to Docker, the reference JDK, compiler workers, benchmark isolation, or
fixture discovery can invalidate the test oracle. Treat them as compatibility
work, not routine build cleanup, and follow the [research method](../contributing/research-method.md)
and the relevant worker gates.
