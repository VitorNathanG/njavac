# Quickstart

This path establishes the environment, builds the compiler, and runs a fresh
byte-identity check. Run commands from the repository root.

## Inspect the command surface

```sh
make help
```

`make help` is the authoritative command catalog. If this guide and the output
disagree about a flag or target, follow the Makefile and correct the guide.

## Build locally for debugging

```sh
make check
```

This produces release binaries under `target/release/`. It is useful for compiler
debugging and direct CLI use, but it is not an acceptance test.

To exercise the CLI, create a source whose filename matches its public class:

```java
public class Hello {
    public static void main(String[] args) {
        int answer = 40 + 2;
        System.out.println(answer);
    }
}
```

Compile it with the locally built binary:

```sh
./target/release/njavac Hello.java
```

This should write `Hello.class` beside the source. Direct local compilation only
demonstrates that the compiler runs; it does not compare against the pinned
reference compiler. Remove ad hoc sources and class files when finished rather
than treating them as fixtures.

The exact accepted language and deliberate refusal boundaries live in
[Language support](../reference/language-support.md). Do not infer general Java
support from this example.

## Run the fast inner-loop gate

```sh
make verify
```

This builds the Docker image and compares njavac with cached class files produced
by the pinned `javac`. An empty cache is populated automatically. A nonempty cache
can be stale after a fixture or JDK change.

After adding, renaming, moving, or changing a fixture, refresh first:

```sh
make record
make verify
```

Do not diagnose compiler behavior from a cached mismatch until cache freshness is
known. See [Fixtures and goldens](../tooling/fixtures-and-goldens.md).

## Run fresh acceptance

```sh
make correctness
```

This invokes the pinned reference compiler afresh in Docker and byte-compares the
complete fixture suite. It is the pre-commit correctness gate. A green local build
or cached verify does not replace it.

Use the controlled benchmark only when authoritative timing is also needed:

```sh
make bench
```

## Inspect one case

Use an existing fixture for a focused cached check:

```sh
make verify FILE=fixtures/basics/Empty.java
```

Use a fresh comparison when cache state should not be involved:

```sh
make correctness FILE=fixtures/basics/Empty.java
```

For an ad hoc source, `make src-diff FILE=Hello.java` compares both compilers and
prints structural and disassembly diagnostics on a mismatch. The complete
investigation path is documented in [Differential debugging](../tooling/differential-debugging.md).

## Choose the next path

| Goal | Continue with |
| --- | --- |
| Understand current code | [Architecture overview](../architecture/overview.md) and [repository map](../reference/repository-map.md) |
| Add a Java construct | [Implementing a rung](../contributing/implementing-a-rung.md) |
| Repair a mismatch | [Fixing a divergence](../contributing/fixing-a-divergence.md) |
| Infer javac behavior | [Research method](../contributing/research-method.md) |
| Change infrastructure | [Maintainer workflow](../contributing/workflow.md) |
| Update documentation | [Documentation policy](../contributing/documentation-policy.md) |
