# Compatibility Contract

njavac's product is not merely a valid class file or behavior equivalent to
Java. For every valid Java program inside the documented
[language subset](language-support.md) that the repository-pinned reference
accepts, njavac must emit exactly the same class bytes as that reference compiler.

## Reference boundary

The reference is the GraalVM CE `25.0.2-graalce` Java 25 compiler pinned by the
Docker image, emitting class-file major version 69. A different vendor, release,
point release, option set, or classpath may legitimately emit different bytes.
Executable configuration remains the authority if the pinned version changes.

Acceptance comparisons run inside Docker. Host `javac` output is not evidence for
byte identity. The sanctioned correctness and differential workflows are
described by `make help` and the contributing guide.

## What byte-identical means

The complete emitted `.class` byte sequence must match, including choices that do
not alter runtime behavior:

- Class version, flags, superclass, interfaces, fields, methods, and their order.
- Constant-pool entry kinds, values, insertion order, deduplication, and indices.
- Exact instruction forms, operands, branch layout, and `max_stack`/`max_locals`.
- Attribute presence, order, lengths, and payloads.
- `LineNumberTable`, `StackMapTable`, and `SourceFile` details.
- Synthesized constructor shape and all referenced names and descriptors.

An equivalent opcode sequence, reordered pool, larger verifier frame, extra
attribute, or different line entry is a compatibility failure even when the JVM
runs both classes identically.

## Determinism

For the supported surface, reference output is treated as a deterministic
function of:

```text
source set + compiler build + compiler options + classpath/environment
```

Class bytes contain no filesystem timestamp or content hash. The timestamp and
SHA-256 lines shown by `javap` are presentation metadata and are excluded from
textual diagnostic diffs, but raw class comparison remains authoritative.

Three qualifications matter:

- **Environment-coupled:** resolved library descriptors and some future
  attributes can contain JDK-specific facts. A toolchain change may change valid
  output.
- **Context-dependent:** name resolution, overload selection, generic erasure,
  annotations, enclosing declarations, bootstrap descriptors, and generated
  helper classes can depend on other sources and classpath types. Full Java is
  not a pure function of one source file.
- **Implementation-defined but stable:** frame minimization, switch selection,
  string-switch lowering, synthetic naming, generated-member order, and similar
  choices are javac policies rather than the only valid JVMS encoding. njavac
  must reconstruct the pinned compiler's observable policy.

Annotation processing is outside the project contract. It is a classic source of
externally supplied and potentially nondeterministic output.

Examples of future context-dependent bytes include the selected `println`
descriptor, `Methodref` versus `InterfaceMethodref`, boxing and widening,
generic-inference casts, `Signature`, `Exceptions`, annotation class values,
`EnclosingMethod` descriptors, and bootstrap descriptors for concatenation,
lambdas, records, and pattern switches. The reported enum-switch helper also
depends on the target enum's constants and may be reused across switch sites.

## Black-box rule

The reference compiler is treated as a black box. Compatibility rules come from
repository probes, raw class bytes, structural diffs, pinned `javap`, fixtures,
and differential fuzzing. javac or OpenJDK source and implementation internals are
not authorities.

Specifications establish Java meaning and valid class-file structure. They do
not establish which valid byte sequence the pinned compiler selects. Research
confidence and evidence retention are defined in
[Evidence and confidence](../research/evidence.md).

## Current mechanics

The contract owns what must match, while the architecture guide owns how the
current compiler achieves it. See [Lowering](../architecture/lowering.md) for
physical Java-expression choices,
[Assembler and Metadata](../architecture/assembler-and-metadata.md) for symbolic
layout and PC-bearing metadata, and
[Class File](../architecture/classfile.md) for constant-pool and attribute order.
Exact local decision rules belong in code doc comments and fixtures.

## Supported-program qualification

The contract applies only when all of these are true:

- The source satisfies [Language Support](language-support.md).
- The source is valid Java accepted by the pinned reference compiler before
  njavac's result is considered.
- It does not reach any current defect or accidental-acceptance signature excluded
  by the language-support page.
- It stays within the documented source-line, method-code, and modified-UTF-8
  limits.
- Compilation uses the repository-pinned reference environment and corresponding
  njavac build.
- The same source filename is supplied to both compilers so `SourceFile` agrees.
- For CLI use, the public class name matches the source basename.

Outside that boundary, a diagnostic, panic, wrong class, accidental acceptance, or
accidental byte match does not extend the supported language. Deliberate
unsupported diagnostics, accidental-acceptance exclusions, and known defects are
listed on the support page so the contract cannot hide them.

## Evidence and gates

Fixtures are executable support claims: each supported fixture must match freshly
invoked pinned javac byte-for-byte. Cached golden comparisons are an inner-loop
optimization, not a new authority. A fresh pinned comparison is the acceptance
gate.

Differential fuzzing supplements fixtures. Exact byte divergence is compatibility
telemetry; when both classes also produce different normalized observations it is
a behavioral compiler finding. Runtime agreement does not excuse a byte
divergence, and observed agreement cannot prove the absence of unobserved semantic
differences.
