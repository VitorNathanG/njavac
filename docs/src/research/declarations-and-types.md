# Declarations and Types Research

This page preserves future declaration, member, type, generic, and annotation
leads. It is not current support. Unless marked otherwise, entries are migrated
**[U]** reports under [Evidence and Confidence](evidence.md).

## Methods and modifiers

- **[U] General methods:** multiple methods, overloads, non-`void` returns, and
  arbitrary parameters primarily change descriptors, method plans, code, and
  ordering. Overloads differ by descriptor rather than name.
- **[U] `throws`:** reported to produce an `Exceptions` attribute. This is
  unrelated to the exception table inside a method's `Code`.
- **[U] Instance methods:** introduce `this` in slot zero and non-static dispatch.
- **[U] Abstract/native methods:** reported to carry no `Code` attribute.
- **[U] Varargs declaration:** reported to set `ACC_VARARGS` (`0x0080`) on the
  method; varargs call-site packing is separate.
- **[U] Modifiers:** access, final, abstract, synchronized, native, transient, and
  volatile map to declaration-appropriate flags.
- **[U] `strictfp`:** reported byte-invisible since Java 17.

## Inheritance, constructors, and initialization

- **[U] `extends`:** a non-`Object` superclass changes `super_class`, and a
  synthesized constructor must call that superclass constructor rather than
  `Object.<init>`.
- **[U] `implements`:** populates the ordered interfaces table.
- **[U] Explicit constructors:** introduce constructor descriptors, instance
  initialization, and `this(...)`/`super(...)` chaining.
- **[U] Initializer ordering:** instance field initializers and initializer blocks
  are reported to merge into constructors in source order; static forms merge
  into `<clinit>`.
- **[U] Flexible constructor bodies:** Java 25 allows statements before an
  explicit constructor invocation under defined restrictions; see
  [Modern Java](modern-java.md#java-25-headline-features).

## Fields and member order

- **[U] Fields:** require ordered field plans and static/instance get/put lowering.
- **[U] Compile-time constants:** a `static final` primitive/string constant was
  reported to carry `ConstantValue`.
- **[U] Instance final constants:** the migrated survey reported both a
  `ConstantValue` attribute and runtime `putfield`; this unusual detail needs
  careful reprobe before implementation.
- **[U] `<clinit>`:** reported to be emitted last, while ordinary fields and
  methods otherwise retain source order.
- **[U] Generated members:** bridges, lambda bodies, enum helpers, record members,
  and other synthetics have family-specific deterministic positions.

## Nested, inner, local, and anonymous classes

- **[U] Attributes:** reported to use `InnerClasses`, `NestHost`/`NestMembers`, and
  for local/anonymous classes `EnclosingMethod`.
- **[U] Outer capture:** a non-static inner class reportedly receives a
  `final synthetic this$0` field and constructor parameter, with an
  `Objects.requireNonNull` capture idiom.
- **[U] Constructor metadata:** synthetic constructors reportedly use
  `MethodParameters`.
- **[U] Ordering:** `NestMembers` and `InnerClasses` reportedly enumerate members
  in different orders.
- **[U] Anonymous and local naming:** deterministic binary names and source-order
  numbering require complete multi-artifact evidence.

Local type placement in source units is also covered in
[Compilation Units](compilation-units.md#local-types).

## Interfaces and abstract classes

- **[U] Interfaces:** include abstract, default, static, and private methods, with
  invocation-kind and flag differences.
- **[U] Interface fields:** reported implicitly `public static final` with
  `ConstantValue` where applicable.
- **[U] Abstract classes:** combine class initialization/constructors with abstract
  methods that have no `Code`.
- **[U] Default-method super calls:** require `invokespecial` with an
  `InterfaceMethodref`, as cataloged in
  [Values and Expressions](values-and-expressions.md#fields-and-calls).

## Enums

- **[U] Core shape:** enum output reportedly includes a `Signature` even without
  user-written generics, a synthetic `$VALUES` field, and generated
  `values()`, `valueOf(String)`, and `$values()` methods in fixed order.
- **[U] Constructor parameters:** generated enum constructors and methods involve
  `MethodParameters` and hidden name/ordinal arguments.
- **[U] Constant-specific bodies:** a constant such as `RED { ... }` reportedly
  produces an anonymous subclass and corresponding `InnerClasses` rows.
- **[U] User declarations:** enums may also have fields, private constructors, and
  implemented interfaces, interleaved with generated artifacts according to
  javac-specific order.

Switching on enums has context-dependent synthetic behavior documented in
[Control Flow](control-flow.md#enum-switch).

## Records

- **[U] Structural output:** records reportedly use the `Record` attribute,
  component fields, constructor, accessors, and `MethodParameters`.
- **[U] Object methods:** generated `equals`, `hashCode`, and `toString` reportedly
  use `ObjectMethods.bootstrap` through indy, bringing `BootstrapMethods` and an
  `InnerClasses` row.
- **[U] Ordering and customization:** compact/canonical constructors, explicitly
  declared accessors, generic records, annotations, and nested records need a
  broader corpus than the migrated survey.

## Sealed types

- **[U] `sealed` and `permits`:** reported to produce
  `PermittedSubclasses` with no dedicated sealed flag bit.
- **[U] `non-sealed`:** reported byte-invisible.
- **[U] Inference:** omitted permits lists and compilation-unit context may affect
  which subclasses are recorded and in what order.

## Generics

- **[U] Surface:** generic classes, interfaces, methods, bounded variables,
  wildcards, and diamond inference require semantic type arenas, erasure,
  inference, and overload integration.
- **[U] Diamond:** reported byte-identical to the equivalent explicit type
  arguments after attribution.
- **[U] `Signature`:** reported on classes, fields, and methods while ordinary
  descriptors remain erased.
- **[U] Intersection bounds:** `<T extends A & B>` was reported to encode an empty
  class bound followed by interface bounds (`<T::LA;:LB;>` in the surveyed shape),
  erase to the first bound, and insert `checkcast` at uses requiring a non-first
  bound.
- **[U] Generic reads:** reported to insert `checkcast` whenever an erased return
  type is wider than the source type.
- **[U] Primitive arguments:** boxing and unboxing appear at generic call sites.
- **[U] Bridges:** covariant or generic overrides may synthesize methods with
  `ACC_BRIDGE | ACC_SYNTHETIC`, reportedly after ordinary methods.

Signatures, erasure, inference, bridge need, and inserted casts are separate
authorities and should not be collapsed into descriptor generation.

## Annotations

- **[U] Retention:** runtime retention reportedly uses
  `RuntimeVisibleAnnotations`, class retention uses
  `RuntimeInvisibleAnnotations`, and source retention such as `@Override` emits
  no annotation entry.
- **[U] `@Deprecated`:** reported to emit both `Deprecated` and
  `RuntimeVisibleAnnotations` on ordinary declarations.
- **[U] Annotation declarations:** `@interface` sets `ACC_ANNOTATION`; an element
  default reportedly uses `AnnotationDefault`.
- **[U] Parameters:** parameter annotations use visible/invisible parameter
  annotation attributes.
- **[U] Element values:** the union reportedly uses literal tag bytes (`I`, `Z`,
  `C`, and the other primitive tags; `s` for string, `c` for class, `e` for enum,
  `[` for array, and `@` for nested annotation). Pool order and nested ordering
  require dedicated evidence.
- **[U] Type annotations:** visible/invisible type-annotation attributes can occur
  on classes, fields, methods, and `Code`, with target types for fields, returns,
  formal parameters, receivers, throws, casts, `new`, locals, and more, plus
  `type_path` elements for arrays and type arguments.
- **[U] Repeating annotations:** repeated annotations reportedly synthesize one
  container annotation whose `value` is an array; standalone repeated entries do
  not remain.

Type-use placement, local-variable target ranges, source-retention disappearance,
annotation defaults, and generated-member propagation make annotations an
owner-specific attribute system rather than one generic blob.
