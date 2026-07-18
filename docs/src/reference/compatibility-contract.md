# Compatibility Contract

For every valid Java program inside the documented
[language subset](language-support.md) that the repository-pinned reference
accepts, njavac must emit a valid class file with equivalent behavior. njavac
also retains the reference compiler's exact class bytes whenever practical.
Identity is the preferred result because it preserves every observable physical
choice and is much cheaper to test than executing both outputs, but it is not the
only acceptable representation.

## Reference boundary

The reference is the GraalVM CE `25.0.2-graalce` Java 25 compiler pinned by the
Docker image, emitting class-file major version 69. A different vendor, release,
point release, option set, or classpath may legitimately emit different bytes.
Executable configuration remains the authority if the pinned version changes.

Reference comparisons run inside Docker. Host `javac` output is not evidence for
either exact identity or behavioral compatibility with the pinned reference. The
sanctioned fixture and differential workflows are described by `make help` and
the contributing guide.

## Behavioral requirement

Equivalent behavior means that, under the same supported inputs and pinned
environment, the candidate class loads and verifies and has the same observable
Java and JVM effects as the reference output. Relevant effects include returned or
thrown outcomes, output and state changes, linkage, reflection-visible structure,
and metadata-mediated behavior when the changed representation can expose them.

Evidence must cover the surface that differs. Running `main` once can establish
the generated program's modeled output trace, but it cannot establish equivalence
for an unobserved exception path, reflective consumer, verifier boundary, or
metadata-dependent tool.

## Byte-retention policy

An exact result has the same complete emitted `.class` byte sequence, including
choices that do not alter runtime behavior:

- Class version, flags, superclass, interfaces, fields, methods, and their order.
- Constant-pool entry kinds, values, insertion order, deduplication, and indices.
- Exact instruction forms, operands, branch layout, and `max_stack`/`max_locals`.
- Attribute presence, order, lengths, and payloads.
- `LineNumberTable`, `StackMapTable`, and `SourceFile` details.
- Synthesized constructor shape and all referenced names and descriptors.

These surfaces remain ordered, byte-visible data and njavac should reconstruct the
reference choice rather than diverge accidentally. A difference triggers
structural diagnosis and behavioral validation; it does not by itself prove a
compatibility defect.

When reference-compiler optimization obscures its particular physical choice or
makes that representation impractical to reconstruct, a different valid class-file
representation is acceptable if evidence appropriate to the affected surface
establishes equivalent behavior. This is the only exception to the byte-retention
policy, and it must be deliberate and documented. It does not authorize arbitrary
opcode substitution in the assembler, unstable output, or treating one successful
execution as proof of equivalence for metadata, reflection, exceptions, or other
behavior that execution did not observe.

## Determinism

For the supported surface, reference output is treated as a deterministic
function of:

```text
source set + compiler build + compiler options + classpath/environment
```

Class bytes contain no filesystem timestamp or content hash. The timestamp and
SHA-256 lines shown by `javap` are presentation metadata and are excluded from
textual diagnostic diffs. Raw class comparison remains authoritative for physical
identity and is the preferred fast path before more expensive behavioral checks.

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
  reconstructs the pinned compiler's observable policy when practical; a deliberate
  alternative remains subject to the optimization exception above.

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
not establish which valid byte sequence the pinned compiler selects. Raw-byte
research supports the byte-retention goal, while behavioral evidence determines
whether a divergent representation under the optimization exception is
compatible. Research confidence and evidence retention are defined in
[Evidence and confidence](../research/evidence.md).

## Current mechanics

The contract owns compatibility and byte retention, while the architecture guide
owns how the current compiler achieves them. See
[Lowering](../architecture/lowering.md) for
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

The current fixtures are executable exact-output claims: each fixture must match
freshly invoked pinned javac byte-for-byte. Cached golden comparisons are an
inner-loop optimization, not a new authority. A fresh pinned comparison is the
exact-byte fixture gate; do not weaken that gate to admit a deliberate divergence.

Differential fuzzing supplements fixtures with a two-stage oracle. Exact bytes are
the cheap fast pass. On divergence, the observer attempts to load and execute both
outputs; different normalized observations are a behavioral compiler finding,
while matching observations satisfy the fuzzer's behavioral check for its current
generated surface and leave byte-retention telemetry.

The observer supplies empirical evidence, not universal proof. Acceptance of a
persistent nonidentical representation under the optimization exception requires a
durable regression oracle covering the behavior the changed physical surface can
affect. The current exact fixture harness has no behavioral-exception mode, and
byte-drift telemetry is not a durable test. Until a sanctioned behavioral
regression gate exists for a case, the divergence remains a candidate rather than
an accepted part of the supported surface; do not relax the golden comparison.
