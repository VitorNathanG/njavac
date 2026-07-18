# Language Rungs

Language work resumes only after the infrastructure sequence in
[Active Work](active-work.md). This page owns source-feature scope and order only.
The [rung workflow](../contributing/implementing-a-rung.md) owns completion rules,
and linked research pages preserve unverified physical-form leads as **[U]**
evidence rather than requirements.

## Ordered sequence

Each row is one implementation cycle. Repeated family names split a broad family
into ordered, independently completed source constructs.

| Order | Cycle | Source scope and research |
| --- | --- | --- |
| 1 | Conditional expression | Add `?:` with a boolean condition and primitive-valued arms in current expression contexts. See [conditional-expression research](../research/values-and-expressions.md#conditional-expression). |
| 2 | General boolean-value contexts | Permit comparison and logical expressions wherever the current primitive value surface is otherwise accepted. See [live-stack research](../research/values-and-expressions.md#conditional-expression). |
| 3 | `while` statement | Add the ordinary pre-test `while` form. See [loop research](../research/control-flow.md#loops-and-jumps). |
| 4 | C-style `for` statement | Add initializer, condition, and update clauses. Treat `do`/`while` and enhanced `for` as later, separate constructs. See [loop research](../research/control-flow.md#loops-and-jumps). |
| 5 | String concatenation: constants | Add `+` when every concatenation segment is a compile-time string value. See [concatenation research](../research/values-and-expressions.md#string-concatenation). |
| 6 | String concatenation: runtime values | Extend string `+` to expressions containing at least one runtime operand. See [concatenation research](../research/values-and-expressions.md#string-concatenation) and [class-file impact](../research/classfile-impact.md#invokedynamic-and-bootstraps). |
| 7 | Static methods: multiple declarations | Permit additional `public static void` methods with no parameters while retaining the current body surface. See [method research](../research/declarations-and-types.md#methods-and-modifiers). |
| 8 | Static methods: primitive parameters | Add primitive parameter declarations to those methods. See [method research](../research/declarations-and-types.md#methods-and-modifiers). |
| 9 | Static methods: calls | Add same-class `invokestatic` calls with arguments in the supported primitive surface. See [call research](../research/values-and-expressions.md#fields-and-calls). |
| 10 | Static methods: value returns | Add primitive non-`void` return types and value-return statements. See [method research](../research/declarations-and-types.md#methods-and-modifiers) and [return research](../research/control-flow.md#return-and-throw). |
| 11 | Static fields: declarations | Add static primitive and string-constant field declarations. See [field research](../research/declarations-and-types.md#fields-and-member-order). |
| 12 | Static fields: access | Add same-class `getstatic` and `putstatic` expressions/statements. See [field-access research](../research/values-and-expressions.md#fields-and-calls). |
| 13 | Static initialization | Add static field initializers and then explicit static initializer blocks as separate fixture-backed shapes. See [initialization research](../research/declarations-and-types.md#inheritance-constructors-and-initialization). |
| 14 | Constructors: declarations | Add explicit no-argument constructor declarations before constructor parameters or object creation. See [constructor research](../research/declarations-and-types.md#inheritance-constructors-and-initialization). |
| 15 | Constructors: parameters and overloads | Add primitive constructor parameters and multiple constructor declarations so delegation has a distinct target. See [constructor research](../research/declarations-and-types.md#inheritance-constructors-and-initialization). |
| 16 | Constructors: explicit invocation | Add explicit `this(...)` and `super(...)` constructor invocations. See [constructor research](../research/declarations-and-types.md#inheritance-constructors-and-initialization). |

The broader candidate surface and its current confidence are cataloged in the
[research survey](../research/evidence.md).
Instance methods, `this`, instance fields, object creation, and instance
initialization remain later families whose internal order must be selected only
after black-box research establishes their independently landable boundaries.

## Opportunistic syntax

Some syntax can become byte-invisible after its surrounding machinery exists:

- `var` after local type inference is modeled.
- Unnamed `_` variables and patterns after their declaration/pattern contexts
  exist.
- Text blocks after source translation and text-block normalization exist; their
  result is an ordinary string constant.
- Flexible constructor bodies after explicit constructors and constructor
  invocation ordering exist.

These are not free-standing current tasks and do not bypass the ordered rungs.
