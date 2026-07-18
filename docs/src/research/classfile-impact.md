# Class-File Impact

This page is a routing map for class-file subsystems likely to be forced by future
language features. It does not mean a feature is fully researched. Confidence
labels follow [Evidence and Confidence](evidence.md); migrated future claims are
**[U]** until retained probes establish them.

## Impact tags

### Stack maps

**[U]** `[SMT]` means a construct introduces branch targets or verifier-state
merges that require a `StackMapTable`. A type alone, straight-line value return,
bare `throw`, or plain `instanceof` stored directly need not create a frame.
Materialized comparisons, pattern `instanceof`, loops, switches, and exception
handlers do.

Frame encoding is a byte-identity decision, not just verifier validity. The exact
placement and physical form for each future control-flow family remain **[U]** and
require a retained matrix; legal forms or behavior seen in a different source
shape do not establish the pinned compiler's choice.

### Invokedynamic and bootstraps

**[U]** `[indy]` means an `invokedynamic` site plus a `BootstrapMethods` attribute.
The migrated survey reports that each relevant javac indy family also introduces
an `InnerClasses` row for `MethodHandles$Lookup`; this must be reprobed per family.

Reported consumers include runtime string concatenation, lambdas and method
references, record object methods, and pattern switches. Their bootstrap owner,
name, descriptor, ordered static arguments, and recipe payload are all byte-visible.

### Constant-pool kinds

**[U]** `[pool]` marks additional entry kinds reported for future language
families. Surveyed kinds include:

- `InterfaceMethodref` (tag 11), distinct from `Methodref` (tag 10).
- `MethodHandle`, including its one-byte reference kind.
- `MethodType`.
- `InvokeDynamic`.
- `Module` (tag 19) and `Package` (tag 20) for module descriptors.

**[U]** The survey found no language feature in its catalog that required
`CONSTANT_Dynamic` (tag 17). That is not an exhaustive proof that Java 25 javac
never emits it.

### Attributes and code substructures

**[U]** `[attr]` marks a new class-file attribute family. Important reported
examples include `BootstrapMethods`, `InnerClasses`, `Exceptions`, `Signature`,
`ConstantValue`, `Record`, `PermittedSubclasses`, annotation attributes,
`EnclosingMethod`, nest attributes, `MethodParameters`, and `Module`.

An exception handler table is not a standalone attribute. It is a substructure of
`Code`. A method declaration's `throws` clause instead produces the unrelated
`Exceptions` attribute.

## Cross-cutting byte hazards

These leads must be included in future probe matrices:

- **[U] Future concat:** the reported concatenation recipe contains literal byte
  `0x01` for runtime arguments and `0x02` for folded constants. `javap` rendering
  them as Unicode escapes is not raw-byte evidence.
- **[U] Future handles:** reported reference kinds include 6
  (`REF_invokeStatic`) for bootstrap/static lambda targets and 8
  (`REF_newInvokeSpecial`) for constructor references.
- **[U] Future synthesis:** member order is deterministic and byte-visible;
  reported rules include `<clinit>` last, lambda and bridge methods after source
  methods, fixed enum/record generated-member order, and different orderings for
  `NestMembers` and `InnerClasses`.
- **[U] Future preview:** `--enable-preview` reportedly writes minor version
  `65535` (`0xffff`) on every emitted class.
- **[U] Future attributes:** attribute emission order matters; the survey example
  for a generic class was `Signature` before `SourceFile`.
- **[U] Future folding:** each newly supported expression family needs independent
  tests for exact folded bits and for what remains runtime code.
- **[U] Future reachability:** conditional expressions, switches, assertions, and
  other branching constructs need family-specific aliveness evidence.

## Feature-to-subsystem map

This table is triage, not an exhaustive implementation checklist.

| Future family | Reported impact | Confidence |
| --- | --- | --- |
| `?:` with live surrounding values | Typed stack and non-empty-stack `full_frame` | **[U]** |
| Loops and ordinary switches | More branches and stack maps; switch alignment/opcodes | **[U]** |
| Runtime string concatenation | Indy, bootstrap registry, pool kinds, `InnerClasses` | **[U]** |
| General calls and interfaces | Descriptors and `InterfaceMethodref` | **[U]** |
| Exceptions and synchronization | `Code.exception_table`, handler frames, symbolic ranges | **[U]** |
| Fields and constants | Field plans and possibly `ConstantValue` | **[U]** |
| Generics | Erasure plus `Signature`; bridges and inserted casts | **[U]** |
| Nested/local/anonymous types | Multiple artifacts, nest/inner/enclosing attributes, captures | **[U]** |
| Lambdas/method references | Indy, method handles/types, synthetic methods | **[U]** |
| Records | `Record`, indy object methods, generated members | **[U]** |
| Pattern switch | Indy switch bootstrap, stack maps, pool and attributes | **[U]** |
| Modules | Module/package pool tags and `Module` attribute | **[U]** |
| Annotations | Owner-specific annotation attributes and element-value encoding | **[U]** |

The architecture triggers for these subsystems live in
[Architecture Direction](../direction/architecture.md#evolution-triggers).
