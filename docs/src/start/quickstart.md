# Quickstart

This path establishes the environment, builds the compiler, and runs a fresh
exact-byte fixture check. Run commands from the repository root.

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

## Run the deterministic suite

```sh
make test
```

This runs every deterministic pass/fail check through Docker: Rust tests, fresh
exact-byte fixtures, instrumentation equivalence, fuzzer infrastructure and
fixed-seed smoke checks, and documentation validation. Use `make correctness` only
for a narrower fresh fixture loop while developing.

Use the controlled benchmark only when authoritative timing is also needed:

```sh
make benchmark
```

The benchmark runs uninstrumented process and compiler-core samples plus isolated
phase and allocation passes under CPU and memory controls. It writes a terminal
report and ignored JSON artifact but asserts no compiler correctness. Those controls
improve same-host comparability but do not make timing deterministic or portable
between hosts.

## Compile an ad hoc source

Keep ad hoc sources below the already ignored `scratch-fuzz/` directory because
files created directly at the repository root are not ignored:

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

Then compile it with both configured compilers and inspect any physical
divergence:

```sh
make src-diff FILE=scratch-fuzz/Hello.java
```

The target uses disposable output directories in the acceptance image and
persists only terminal diagnostics. The exact accepted language and refusal
boundaries live in [Language support](../reference/language-support.md).

## Inspect one case

Use an existing fixture for a focused cached check:

```sh
make verify FILE=fixtures/basics/Empty.java
```

Use a fresh comparison when cache state should not be involved:

```sh
make correctness FILE=fixtures/basics/Empty.java
```

For ad hoc sources, use repository-relative paths without whitespace, quotes,
shell metacharacters, or a leading option-like component; these Make recipes do
not provide general shell-safe path forwarding. The complete investigation path
is documented in [Differential debugging](../tooling/differential-debugging.md).

## Choose the next path

| Goal | Continue with |
| --- | --- |
| Understand current code | [Architecture overview](../architecture/overview.md) and [repository map](../reference/repository-map.md) |
| Add a Java construct | [Implementing a rung](../contributing/implementing-a-rung.md) |
| Repair a mismatch | [Fixing a divergence](../contributing/fixing-a-divergence.md) |
| Infer javac behavior | [Research method](../contributing/research-method.md) |
| Change infrastructure | [Maintainer workflow](../contributing/workflow.md) |
| Update documentation | [Documentation policy](../contributing/documentation-policy.md) |
