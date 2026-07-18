# Compatibility Contract

njavac's product is not merely a valid class file or behavior equivalent to
Java. For every program inside the documented
[language subset](language-support.md), njavac must emit exactly the same class
bytes as the repository-pinned reference `javac`.

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

## Current byte-visible invariants

The current backend depends on these established invariants:

- Bytecode-referenced constants are interned in encounter order before structural
  class constants.
- Composite constant-pool entries intern their missing children breadth-first.
- `Long` and `Double` entries consume two logical pool indices.
- Float and double constants are keyed by javac-compatible canonical NaN bits,
  while negative zero remains distinct from positive zero.
- JVM text uses modified UTF-8, encoding NUL as `c0 80` and UTF-16 surrogate code
  units independently.
- Attribute vectors are the single authority for interning and write order.
- Stack-map deltas use the first frame's absolute offset and then
  `offset - previous - 1`; the smallest valid frame encoding is selected.
- Source positions are pending until consumed by a real instruction. Code-free
  statements do not automatically create line entries.
- Branches, frame requests, and line events remain symbolic until final layout;
  dead and goto-to-next unconditional branches are compacted without destabilizing
  their anchors.

The exact local decision rules belong in code doc comments and fixtures rather
than being duplicated here.

## Supported-program qualification

The contract applies only when all of these are true:

- The source satisfies [Language Support](language-support.md).
- It does not reach either known wide-local or long-branch assembler defect.
- Compilation uses the repository-pinned reference environment and corresponding
  njavac build.
- The same source filename is supplied to both compilers so `SourceFile` agrees.
- For CLI use, the public class name matches the source basename.

Outside that boundary, a diagnostic, panic, wrong class, or accidental match does
not extend the supported language. Deliberate unsupported diagnostics and known
defects are listed on the support page so the contract cannot hide them.

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
