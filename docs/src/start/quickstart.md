# Quickstart

This path establishes the environment, builds the compiler, and runs a fresh
byte-identity check. Run commands from the repository root.

## Inspect the command surface

```sh
make help
```

`make help` is the authoritative command catalog. If this guide and the output
disagree about a target or its short hint, follow the Makefile and correct the
guide. `make help` is not a variable catalog; variable defaults and forwarding
remain visible in `Makefile`.

## Run the fast inner-loop gate

```sh
make verify
```

This builds the Docker image and compares njavac with cached class files produced
by the configured in-image `javac`. An empty cache is populated automatically. A nonempty cache
can be stale after a fixture or JDK change.

After adding, renaming, moving, or changing a fixture, refresh first:

```sh
make record
```

`make record` refreshes the complete current suite and already runs offline
verification. Running `make verify` immediately afterward repeats that check.

Do not diagnose compiler behavior from a cached mismatch until cache freshness is
known. See [Fixtures and goldens](../tooling/fixtures-and-goldens.md).

## Run fresh acceptance

```sh
make correctness
```

This invokes the configured reference compiler afresh in Docker and byte-compares the
complete fixture suite. It is the pre-commit correctness gate. A green local build
or cached verify does not replace it.

Use the controlled benchmark only when authoritative timing is also needed:

```sh
make bench
```

The benchmark collects repeated process samples under CPU and memory controls.
Those controls improve same-host comparability but do not make timing
deterministic or portable between hosts.

## Optional host build

A host Rust toolchain is optional. When compiler-internal debugging or direct CLI
use is useful, build release binaries locally:

```sh
make check
```

This writes binaries under `target/release/`; it is not an acceptance test. Keep
ad hoc sources below the already ignored `scratch-fuzz/` directory because files
created directly at the repository root are not ignored:

```sh
mkdir -p scratch-fuzz
```

Create `scratch-fuzz/Hello.java` with a filename matching its public class:

```java
public class Hello {
    public static void main(String[] args) {
        int answer = 40 + 2;
        System.out.println(answer);
    }
}
```

Then compile it with the host binary:

```sh
./target/release/njavac scratch-fuzz/Hello.java
```

This writes `scratch-fuzz/Hello.class`. It demonstrates only that the host binary
runs; it does not compare against the configured reference compiler. The exact
accepted language and refusal boundaries live in
[Language support](../reference/language-support.md).

## Inspect one case

Use an existing fixture for a focused cached check:

```sh
make verify FILE=fixtures/basics/Empty.java
```

Use a fresh comparison when cache state should not be involved:

```sh
make correctness FILE=fixtures/basics/Empty.java
```

For an ad hoc source, `make src-diff FILE=scratch-fuzz/Hello.java` compares both
compilers and prints structural and disassembly diagnostics on a mismatch. Use
repository-relative paths without whitespace, quotes, shell metacharacters, or a
leading option-like component; these Make recipes do not provide general
shell-safe path forwarding. The complete investigation path is documented in
[Differential debugging](../tooling/differential-debugging.md).

## Choose the next path

| Goal | Continue with |
| --- | --- |
| Understand current code | [Architecture overview](../architecture/overview.md) and [repository map](../reference/repository-map.md) |
| Add a Java construct | [Implementing a rung](../contributing/implementing-a-rung.md) |
| Repair a mismatch | [Fixing a divergence](../contributing/fixing-a-divergence.md) |
| Infer javac behavior | [Research method](../contributing/research-method.md) |
| Change infrastructure | [Maintainer workflow](../contributing/workflow.md) |
| Update documentation | [Documentation policy](../contributing/documentation-policy.md) |
