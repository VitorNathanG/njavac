# Modern Java Research

This page preserves a broad, non-exhaustive survey of headline language features
from Java 8 through Java 25. It is not a statement of current support. Unless
marked otherwise, entries are migrated **[U]** reports under
[Evidence and Confidence](evidence.md).

## Java 8 through Java 24

| Feature | Reported byte-visible shape | Confidence |
| --- | --- | --- |
| Lambdas and method references (Java 8) | `LambdaMetafactory` indy, method handles/types, synthetic methods as needed | **[U]** |
| `var` locals (Java 10) | Reported byte-identical to the inferred explicit type under default debug output, which has no `LocalVariableTable` here | **[U]** |
| Switch expressions (Java 14) | Value-producing control flow and stack maps | **[U]** |
| Text blocks (Java 15) | Compile-time normalization to one string constant | **[U]** |
| Records (Java 16) | `Record`, generated members, `MethodParameters`, and `ObjectMethods` indy | **[U]** |
| Pattern `instanceof` (Java 16) | Test, branch, cast, binding, scope, and stack maps | **[U]** |
| Sealed classes (Java 17) | `PermittedSubclasses`; `non-sealed` reported byte-invisible | **[U]** |
| Pattern switch and record patterns (Java 21) | Switch bootstrap indy, stack maps, pool entries, generated default behavior | **[U]** |
| Unnamed variables and patterns `_` (Java 22) | Reported to leave no `_` pool name; the storage is simply unnamed | **[U]** |

Detailed leads are split by responsibility:

- [Values and Expressions](values-and-expressions.md) for lambdas, references,
  text blocks, and patterns.
- [Control Flow](control-flow.md) for switch expressions and pattern switches.
- [Declarations and Types](declarations-and-types.md) for records and sealed
  declarations.

The table is intentionally not a release-by-release enumeration. Features not
listed are unverified, not implicitly simple or unsupported forever.

## Java 25 headline features

### Compact source files and instance `main`

- **[U] Status:** migrated survey records JEP 512 as final in Java 25.
- **[U] Source shape:** a source file may omit an explicit class and declare an
  instance `void main()`.
- **[U] Output name:** reported class name is the source basename, which must be a
  legal Java identifier.
- **[U] Flags and method:** reported class is `ACC_FINAL`; `main` is an instance,
  non-static method with no static shim.
- **[U] Constructor:** reported synthesized default constructor has plain flags
  `0x0000`, not `ACC_SYNTHETIC`.
- **[U] Attributes:** reported class carries only `SourceFile` at class level in
  the minimal probe.

This is not parser sugar over current support. It requires source-derived type
identity, the first supported instance method, and a new synthesized-constructor
shape.

### Module import declarations

- **[U] Status:** migrated survey records JEP 511 as final in Java 25.
- **[U] Encoding:** `import module M;` was reported compile-time-only with no
  direct class-file trace.

This must not be confused with compiling `module-info.java`, which introduces
module/package pool tags and a `Module` attribute. Module imports still require a
module-aware resolver.

### Flexible constructor bodies

- **[U] Status:** migrated survey records JEP 513 as final in Java 25.
- **[U] Encoding:** permitted statements before `this(...)` or `super(...)` were
  reported to use existing statement and constructor bytecode without a new
  class-file subsystem.

Exact semantic restrictions, initialization safety, source positions, and
ordering relative to field/instance initializers require a retained corpus.

### Primitive types in patterns, `instanceof`, and `switch`

- **[U] Status:** migrated survey records JEP 507 as preview in Java 25.
- **[U] Preview stamp:** compiling with `--enable-preview` reportedly sets class
  minor version to `65535` (`0xffff`).
- **[U] Lowering:** reported to use the `typeSwitch` family plus a
  `ConstantBootstraps.primitiveClass` bootstrap in relevant shapes.

The complete primitive conversion, exactness, dominance, exhaustiveness, boxing,
null, and switch-restart matrix is not preserved and must be researched before
design.

## Byte-invisible does not mean free

`var`, unnamed variables, module imports, `non-sealed`, `strictfp`, and some
constructor-body relaxation may leave no dedicated class-file marker after
semantic analysis. They still require correct parsing, attribution, resolution,
scope, diagnostics, and interaction with byte-visible surrounding constructs.

Such features can be opportunistic only after their owning semantic and lowering
subsystems exist, as described in [Language Rungs](../direction/language-rungs.md#opportunistic-syntax).
