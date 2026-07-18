# Values and Expressions Research

This page preserves future-value and expression leads. It is not current support;
see [Language Support](../reference/language-support.md). Unless explicitly marked
otherwise, entries are migrated **[U]** reports under the confidence rules in
[Evidence and Confidence](evidence.md).

## Primitive-adjacent values

- **[U] `null`:** reported as the single `aconst_null` opcode with no pool entry.
- **[U] Boxing and unboxing:** reported as wrapper `valueOf` and primitive-value
  calls, required at primitive generic and varargs boundaries.
- **[U] Primitive class literals:** `int.class` reportedly lowers to
  `getstatic Integer.TYPE`, not `ldc`.
- **[U] Reference class literals:** `String.class` reportedly uses `ldc` of a
  `Class` entry. Array class literals such as `int[].class` reportedly use a
  `Class` whose name is the array descriptor (`[I`).

## Source text and literals

- **[U] Text blocks:** reported to normalize incidental whitespace and line
  escapes at compile time, producing one ordinary string constant. The complete
  normalization matrix has not been retained.
- **[U] General Unicode translation:** Java translates eligible Unicode escapes
  before tokenization. A future source layer must retain translated-to-original
  position mapping; the accepted boundary remains owned by
  [Language Support](../reference/language-support.md#character-and-string-literals).
- **[U] Direct Unicode source:** identifier classification, supplementary
  characters, surrogate handling, diagnostics, and modified UTF-8 output need a
  dedicated corpus.
- **[U] Text-block line continuation:** the backslash-newline escape is distinct
  from ordinary string-literal escapes.

Hexadecimal floating-point syntax remains unimplemented and has no retained
future probe corpus. It must cover significand forms, binary exponent, suffix,
underflow/overflow, rounding boundaries, and source spelling that converges on the
same pool value.

## Assignment and lvalues

- **[U] Assignment as an expression:** a local chain such as `a = b = c` was
  reported to retain a value with `dup` before stores.
- **[U] Array assignment values:** `arr[i] = x` used as a value was reported to
  require `dup_x2` plus the element store.
- **[U] Field assignment values:** `obj.f = x` used as a value was reported to
  require `dup_x1` plus `putfield`.
- **[U] General lvalues:** array elements, fields, qualified names, and assignment
  conversions require a focused lvalue lowering authority.

## Conditional expression

- **[U] `?:`:** reported to reuse condition lowering, merge typed arm values, and
  insert cross-arm numeric promotion such as `i2d` on an `int` arm paired with a
  `double` arm.
- **[U] Live-stack materialization:** values produced through branches while a
  surrounding value remains live require typed stack snapshots and
  non-empty-stack frame evidence. Conditional expressions and general
  boolean-value contexts are separate ordered
  [language rungs](../direction/language-rungs.md).

## String concatenation

- **[U] Constant-only concatenation:** expressions such as `"a" + "b"` were
  reported to fold to one string constant with no indy site.
- **[U] Runtime concatenation:** any runtime operand was reported to use
  `StringConcatFactory.makeConcatWithConstants` through `invokedynamic`.
- **[U] Recipe:** the migrated survey reports raw `0x01` bytes for runtime
  arguments and `0x02` for folded constants; argument types appear in the indy
  descriptor.
- **[U] Structural impact:** reported pool additions are `InvokeDynamic`,
  `MethodHandle` kind 6, bootstrap constants, `BootstrapMethods`, and an
  `InnerClasses` row involving `MethodHandles$Lookup`.

The corpus must vary constant/runtime segmentation, primitive/reference types,
grouping, empty strings, recipe-control characters, pool deduplication, and
multiple concat sites.

## Type tests and casts

- **[U] Plain `instanceof`:** reported as a straight-line `instanceof` opcode
  when its boolean result is stored directly, without necessarily creating a
  frame.
- **[U] Pattern `instanceof`:** reported to add a conditional branch, `checkcast`,
  pattern binding, scope, and stack-map merge.
- **[U] Reference casts:** reported to use `checkcast`.
- **[U] Intersection casts:** reported to emit one `checkcast` per bound in
  reverse-listed order.

Primitive patterns are separately covered in
[Modern Java](modern-java.md#java-25-headline-features).

## Objects and arrays

- **[U] Object creation:** reported as `new`, `dup`, then
  `invokespecial <init>`.
- **[U] Primitive arrays:** `new int[n]` reportedly uses `newarray`.
- **[U] Reference arrays:** `new String[n]` reportedly uses `anewarray`.
- **[U] Array initializers:** reported as allocation followed by a repeated
  duplicate/index/value/store sequence.
- **[U] Fully dimensioned arrays:** `new int[2][3]` reportedly uses
  `multianewarray` and a `Class` entry for `[[I`.
- **[U] Partially dimensioned arrays:** `new int[n][]` reportedly uses
  `anewarray` with component class `[I`, not `multianewarray`.
- **[U] Element access:** primitive and reference arrays require their typed
  load/store opcode families.
- **[U] Length:** `a.length` reportedly uses `arraylength`.
- **[U] Clone:** an array `clone()` was reported as `invokevirtual` on the array
  class followed by a covariant `checkcast`.

## Fields and calls

- **[U] Fields:** instance reads/writes require `getfield` and `putfield`; static
  fields add `getstatic` and `putstatic` plus owner/type attribution.
- **[U] Ordinary invocation:** static, virtual, special, and interface dispatch
  require separate modeled invocation kinds.
- **[U] Interface calls:** `invokeinterface` uses an `InterfaceMethodref` and a
  trailing argument-count byte.
- **[U] Super calls:** `super.m()` was reported to use `invokespecial`.
- **[U] Qualified interface-super calls:** `Iface.super.m()` was reported to use
  `invokespecial` with an `InterfaceMethodref`.
- **[U] Qualified inner creation:** `outer.new Inner()` was reported to pass the
  outer instance as the first constructor argument and emit an
  `Objects.requireNonNull` guard.
- **[U] Generic call syntax:** explicit `Foo.<T>of(...)` requires parser and
  attribution support even when erasure leaves no direct type-argument bytecode.

## Varargs

- **[U] Call sites:** a varargs call was reported to synthesize an array and fill
  it with `dup` plus element stores.
- **[U] Primitive arguments:** boxing may be required when the varargs element
  type is a reference or type variable.
- **[U] No indy:** ordinary varargs packing was reported not to use
  `invokedynamic`.

Method declaration flags are covered in
[Declarations and Types](declarations-and-types.md#methods-and-modifiers).

## Lambdas and method references

- **[U] Bootstrap:** reported to use
  `LambdaMetafactory.metafactory` with three bootstrap arguments: method type,
  implementation method handle, and instantiated method type.
- **[U] Lambda body:** reported to synthesize a `private static synthetic`
  `lambda$<method>$<n>` method after its enclosing source method.
- **[U] Constructor reference:** `Type::new` reportedly uses method-handle kind 8
  (`REF_newInvokeSpecial`).
- **[U] Static reference:** reportedly uses kind 6 (`REF_invokeStatic`).
- **[U] Unbound instance reference:** a form such as `String::length` reportedly
  uses kind 5 (`REF_invokeVirtual`).
- **[U] SAM invocation:** reported to use `InterfaceMethodref`.

Capture layout, bound references, serializable lambdas, intersection targets,
overload selection, and bridge interactions remain outside this survey and must
not be assumed.
