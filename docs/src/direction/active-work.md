# Active Work

This page is the ordered infrastructure and confirmed-defect queue. Work proceeds
top to bottom. Completed work is deleted rather than retained as history.

Language-feature ordering lives in [Language Rungs](language-rungs.md). Unordered,
non-active improvements live in [Deferred Work](deferred-work.md). Target
boundaries live in [Architecture Direction](architecture.md).

## 1. Wide local loads and stores

Complete JVM `wide` forms for local loads and stores whose slot is above 255.
Preserve javac-compatible physical-form selection and add a minimal regression
fixture that reaches the boundary. The current truncation is documented as a
reachable defect in [Language Support](../reference/language-support.md#known-reachable-defects).

## 2. Long branches

Add javac-compatible branch-form selection during final symbolic layout for
conditional and unconditional targets outside the signed 16-bit range. The
implementation must account for layout changes caused by widened branches rather
than patching offsets after a fixed layout.

## 3. Complete attribution facts

Record conversions and promoted types selected during semantic attribution so
lowering does not recompute semantic expression results. Expand resolved
invocation facts to carry:

- Selected owner and member.
- Invocation kind.
- Descriptor.
- Parameter types.
- Return type.

Lowering must consume these facts without reconstructing library signatures. Do
not create a generic type arena, resolver environment, or source-type hierarchy
until a language rung gives each one a concrete responsibility.

## 4. Model the typed operand stack

Replace word-depth-only tracking with typed symbolic operand-stack state. Derive
field and invocation effects from modeled types or descriptors. Frame requests
must snapshot the assembler's current stack instead of receiving a separately
maintained manual vector.

This is the byte-preserving prerequisite for non-empty-stack boolean
materialization and the conditional-expression rung. Land it before changing the
accepted language.

## 5. Resume language rungs

After the four infrastructure steps are green, continue with the ordered
[language rungs](language-rungs.md). If a rung exposes another structural
prerequisite, add the smallest tidy-first infrastructure item here and land it
separately from the feature.

## Open fuzzer findings

No open findings.

When a finding appears, handle one signature at a time: reproduce, minimize, fix,
run authoritative verification, add a documented regression fixture, and then
delete the finding from this page before starting another.
