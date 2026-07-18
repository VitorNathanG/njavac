# Compilation-Unit Research

This page preserves future source-set and output-artifact leads. It is not current
support. Unless marked otherwise, entries are migrated **[U]** reports under
[Evidence and Confidence](evidence.md).

## Packages

- **[U] Class naming:** a package prefixes the class internal name with slash-
  separated components and nests the output path.
- **[U] Source file:** `SourceFile` reportedly remains the bare source basename,
  not a package-qualified path.
- **[U] Resolution:** packages require source-set and classpath-aware type/name
  resolution even when their direct class-file effect appears small.

## Imports

- **[U] Ordinary imports:** single-type and on-demand imports were reported to
  have no direct class-file trace; they affect compile-time resolution.
- **[U] Static imports:** likewise reported byte-invisible after the selected
  symbol and invocation/field facts are known.
- **[U] Module imports:** Java 25 module import declarations are covered in
  [Modern Java](modern-java.md#java-25-headline-features).

Byte-invisible syntax is not implementation-free: ambiguity, shadowing,
accessibility, overloads, and diagnostics require a resolver environment.

## Multiple top-level types

- **[U] Outputs:** one source containing several top-level types reportedly emits
  one `.class` per type, all carrying the same `SourceFile` basename.
- **[U] Flags:** non-public sibling classes were reported as `ACC_SUPER` without
  `ACC_PUBLIC`.
- **[U] Ordering and failure:** artifact order, duplicate names, partial failure,
  cross-sibling resolution, and generated companions require a compilation-level
  request/result.

Fixtures must evolve from one-file/one-class cases to case directories compiled as
one source set and compared across every emitted artifact.

## `package-info.java`

- **[U] Shape:** reported to emit `package-info.class` with flags `0x1600`
  (`ACC_INTERFACE | ACC_ABSTRACT | ACC_SYNTHETIC`), no members, and package
  annotations.
- **[U] Deprecation exception:** a package-level `@Deprecated` was reported to emit
  only the runtime-visible annotation, without the separate `Deprecated`
  attribute used on a class.

Retention variants, type annotations, package documentation without annotations,
and module interactions remain to be probed.

## Local types

- **[U] Local declarations:** local class, record, interface, and enum declarations
  reportedly behave like nested types and also receive `EnclosingMethod`.
- **[U] Local records:** retain record-generated members and reportedly still use
  the `ObjectMethods` indy bootstrap.
- **[U] Naming and capture:** binary names, numbering, captured locals,
  effectively-final checks, and constructor parameters need method-context
  evidence.

## Modules

- **[U] `module-info.java`:** reported to emit a class with `ACC_MODULE`
  (`0x8000`), `Module` and `SourceFile` attributes, and no ordinary members.
- **[U] Pool tags:** module descriptors introduce `Module` (19) and `Package`
  (20) constant-pool entries. The migrated survey notes that `javap` may label
  them misleadingly, so raw tags are authoritative.
- **[U] Attribute exclusions:** javac was reported not to emit `ModulePackages` or
  `ModuleMainClass` for ordinary source compilation; packaging/linking tools may
  add them, so njavac must not assume they belong in compiler output.
- **[U] `java.base`:** the implicit requires entry reportedly has
  `ACC_MANDATED`, while an explicitly written `requires java.base` does not.
- **[U] Version coupling:** the requires entry was reported to contain the pinned
  JDK version string (`25.0.2` in the migrated observation), making output
  toolchain-coupled.

The complete directive surface (`requires`, `exports`, `opens`, `uses`,
`provides`), target lists, modifiers, ordering, versions, and multi-module source
layout are not exhaustively surveyed.

## Target compilation API

The architecture must eventually accept multiple source inputs and return an
ordered set of class artifacts with diagnostics and status. Source identity,
output path, internal name, and originating source are per-artifact facts. See
[Architecture Direction](../direction/architecture.md#compilation-contract).
