# Language Support

njavac supports a deliberately small Java 25 subset. Support has two preconditions:
the source must be valid Java accepted by the repository-pinned reference compiler
under the same filename, options, and environment, and it must satisfy every
structural, statement, expression, source-text, and size limit on this page. For
supported programs, the compatibility promise is defined by the
[compatibility contract](compatibility-contract.md).

Acceptance by njavac alone does not establish support. In particular, these known
accidental or overly broad acceptances are excluded:

- A public class name that does not match the reference source filename.
- An out-of-range implicit constant assignment to `byte`, `short`, or `char`.
- An integral `/` or `%` expression whose right operand is a constant zero; sema
  currently rejects these valid runtime expressions.
- A main-parameter `String` or `System.out.println` target whose leading simple
  name is shadowed and therefore does not denote the hard-coded library class.
- Source containing carriage returns, including bare-CR line endings and CRLF.

The current front end may also accept malformed literal details that the reference
rejects. The valid-reference precondition is authoritative for such cases; an
accidental class match does not add them to the subset.

Future Java coverage is research, not support. See the pages under
[`research/`](../research/evidence.md) and the ordered
[language rungs](../direction/language-rungs.md).

## Compilation-unit shape

A supported source file has exactly this outer shape:

```java
public class Example {
    public static void main(String[] args) {
        // supported statements
    }
}
```

The exact limits are:

- One source file produces one class.
- The file contains exactly one top-level type, a `public class` with no explicit
  `extends`, `implements`, type parameters, annotations, or other modifiers.
- The public class name must equal the source filename without `.java`. The
  library API does not enforce this relationship, but its supported use requires
  an exact bare filename matching the declaration. The CLI derives the output
  filename from the source filename.
- The class contains exactly one method and no fields, constructors, initializer
  blocks, nested types, or other members.
- The method is exactly `public static void main(String[] name)`. The parameter
  name is arbitrary, but its value may not be read or assigned.
- `String` must be spelled exactly as the unqualified name in the parameter and
  must denote `java.lang.String`. In particular, the public class may not itself
  be named `String`; njavac otherwise hard-codes the parameter as
  `java/lang/String` despite Java's shadowing rules.
- The method return type is `void`; `return` statements are not supported.
- Packages, imports, modules, and additional top-level types are not supported.

njavac emits the implicit no-argument constructor, `SourceFile`, and
`LineNumberTable`. Methods with reachable branch targets also carry a
`StackMapTable`.

## Local declarations and scope

Locals may use all eight primitive types:

| Type | Supported use |
| --- | --- |
| `boolean` | Locals, assignment, boolean operators, comparisons, conditions, printing |
| `byte`, `short`, `char`, `int` | Locals and the int-family numeric operations |
| `long` | Locals and category-2 integral operations |
| `float`, `double` | Locals and floating-point arithmetic and comparison |

`long` and `double` consume two local slots and two operand-stack words. All
other primitive values consume one.

Declaration limits are narrower than the parser's block syntax:

- A declaration has one declarator: `type name;` or `type name = expression;`.
- Declarations are supported only as direct children of the method body.
- A declaration inside any `if` or `else` arm is deliberately refused with
  `NJS1001`, even when the arm is braced.
- Standalone nested blocks are not supported.
- Multiple declarators, `final`, `var`, array locals, reference locals, and local
  type declarations are not supported.
- Local reads require definite assignment. Assignment in both arms of an
  `if`/`else` makes a predeclared local definitely assigned after the join;
  assignment in only one arm does not.
- Local names are ASCII Java-like identifiers: ASCII letters, digits after the
  first character, `_`, and `$`. Java's wider Unicode identifier set is not
  supported.

Slots are allocated with category-2 widths and method-level high-water tracking.
Although semantic slot numbers are `u16`, local loads and stores above slot 255
are a known reachable assembler defect described under
[Known reachable defects](#known-reachable-defects).

## Statements

The method body and branch arms may contain:

- Primitive local declarations at method-body top level only.
- Plain assignment to an existing primitive local: `name = expression;`.
- Compound assignment: `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`,
  `<<=`, `>>=`, and `>>>=`.
- Prefix or postfix `++` and `--` in statement position only.
- `System.out.println(expression);` with exactly one argument.
- `if`, `else if`, and `else`, nested or unnested, with either a braced arm or a
  single supported non-declaration statement.

Assignment is not an expression, so chaining such as `a = b = 1` is not
supported. Prefix and postfix increment/decrement do not produce usable values.
No other expression statement or method call is supported.

The parser deliberately reports `NJP1001` for statement forms beginning with
`while`, `for`, `do`, `switch`, `return`, `throw`, `try`, `synchronized`,
`assert`, `break`, or `continue`. Labels and standalone blocks are also outside
the grammar.

## Literals

### Integer literals

Supported integer forms are:

- Decimal, hexadecimal (`0x`/`0X`), octal (a leading zero), and binary
  (`0b`/`0B`).
- Valid underscore-separated forms in those radices.
- Optional `L` or `l` for `long`.
- The full unsigned source patterns that Java uses to spell negative `int` and
  `long` bit patterns, such as `0xFFFFFFFF`.

The source radix and underscores leave no class-file trace. Unary `-` combines
with literals to cover the signed minimum values. Unary `+` is not supported.

### Floating-point literals

Supported floating forms are decimal only:

- A decimal point, including forms such as `.5` and `1.`.
- A decimal exponent introduced by `e` or `E`, with an optional sign.
- `f`/`F` and `d`/`D` suffixes; an unsuffixed floating literal is `double`.
- Valid underscore-separated decimal forms.

Hexadecimal floating-point literals such as `0x1.0p4` are not supported. Special
values are reachable by folding ordinary arithmetic: infinities, NaNs, and signed
zero retain javac-compatible value behavior; NaNs are canonicalized when entered
in the constant pool while negative zero remains distinct from positive zero.

### Character and string literals

Character literals and string literals support:

- The simple escapes `\t`, `\n`, `\r`, `\"`, `\'`, `\\`, `\b`, `\f`, and
  Java 15's `\s`.
- Octal escapes from `\0` through `\377`, using Java's one-to-three-digit rule.
- A limited `\uXXXX` form inside a character or string literal, with one or more
  `u` characters and exactly four hexadecimal digits. It is supported only for a
  BMP non-surrogate code unit whose Java pre-lexing translation would leave the
  surrounding literal's delimiters, escapes, and line structure unchanged.

Unicode support is intentionally partial:

- There is no Java Unicode-escape translation pass before lexing. An escape that
  would form punctuation, a keyword, a comment delimiter, or a line terminator
  is therefore unsupported outside literal decoding.
- Source is assumed ASCII outside literal escapes. Direct non-ASCII identifiers
  and literal text are not supported.
- A character escape is stored as one UTF-16 code unit, but surrogate escapes are
  outside the supported subset. A surrogate escape in a string is not faithfully
  retained by the current scalar-value decoder.
- Text blocks and their line-continuation escape are not supported.

String values are supported only as a literal, optionally parenthesized, supplied
directly to `System.out.println`. There are no `String` locals, concatenation,
comparison, or general string expressions. Class-file strings are written with
JVM modified UTF-8, including the special encoding for NUL. Every encoded
`CONSTANT_Utf8` payload must fit in 65,535 bytes; exceeding that class-file limit
currently panics during serialization rather than returning a diagnostic.

`true` and `false` are supported. `null`, class literals, and text blocks are not.

## Expressions

Parentheses and primitive casts `(primitiveType) expression` are supported.
Numeric casts cover the JVM conversion set and int-family narrowing. A boolean
cast is allowed only from `boolean`; reference and intersection casts are not.

Supported operators, from lower to higher precedence, are:

| Family | Operators | Operand limits |
| --- | --- | --- |
| Short-circuit logical | `||`, `&&` | `boolean` only |
| Non-short-circuit bitwise | `|`, `^`, `&` | both operands integral, or both `boolean` |
| Equality | `==`, `!=` | both numeric, or both `boolean`; no references |
| Relational | `<`, `<=`, `>`, `>=` | numeric only |
| Shift | `<<`, `>>`, `>>>` | integral only; result follows unary promotion of the left operand |
| Additive | `+`, `-` | numeric only; no string concatenation |
| Multiplicative | `*`, `/`, `%` | numeric only |
| Unary | `-`, `~`, `!` | numeric, integral, and `boolean` respectively |

All binary levels are left-associative. Java binary numeric promotion is applied
across `int`, `long`, `float`, and `double`; `byte`, `short`, and `char` promote
to `int`. Shift distances are consumed as `int`, including runtime narrowing of
a `long` distance. Assignment conversion supports primitive widening and Java's
constant-expression narrowing into `byte`, `short`, and `char`. Compound
assignment narrows the promoted result back to the target type.

Implicit constant narrowing is supported only when the folded value is in the
target type's Java range. Sema currently checks constant-expression shape but not
that range, so njavac can accept and wrap an invalid out-of-range assignment. Such
an accidental acceptance is excluded by the valid-reference precondition.

Integral `/` and `%` have an additional current defect: sema rejects an expression
whenever its right operand alone evaluates to constant zero. Ordinary Java
expressions such as `1 / 0`, `x / 0`, and `x % (1 - 1)` are valid and complete
abruptly at runtime; they are not compile-time errors merely because the divisor
is constant. Runtime zero reached through a local divisor, such as `x / divisor`,
is accepted.

Supported constant primitive subtrees are folded with wrapping integer arithmetic,
IEEE-754 floating arithmetic, Java shift masking, comparisons, casts, and boolean
logic. Expressions involving locals normally emit runtime operations, subject to
the integral zero-divisor over-rejection above. Pinned black-box output establishes
one retained exception: a constant `long >>> long` expression is not folded.

## Printing

`System.out.println` must be spelled as that exact dotted target, `System` must
denote `java.lang.System`, and the call must take exactly one argument. Resolution
is currently textual: njavac does not detect a local or class declaration that
shadows `System`, so any such accidental acceptance is outside the subset.
Supported overloads are:

- `(I)V` for `int`, `byte`, and `short`.
- `(J)V`, `(F)V`, and `(D)V` for `long`, `float`, and `double`.
- `(C)V` and `(Z)V` for `char` and `boolean`.
- `(Ljava/lang/String;)V` for a string literal only.

No zero-argument call, multi-argument call, other target, other method, or general
method invocation is supported.

## Conditions and boolean values

Comparisons, `!`, `&&`, and `||` are supported directly as `if` conditions.
Constant conditions may remove the dead arm entirely. Non-constant conditions
produce symbolic branches and the required minimal `StackMapTable` frames.

A comparison or short-circuit expression may also be materialized into a
`boolean` local or assigned to one when evaluation begins with an empty operand
stack. Grouping and boolean casts are preserved where they affect javac's
observable branch, materialization, frame, and line-number shape.

Materializing a branch-valued boolean while another value remains live on the
operand stack is deliberately refused with `NJC1001`. Reachable examples include:

```java
System.out.println(a < b);
System.out.println(a && b);
boolean r = valueBoolean & (a < b);
```

The receiver or left operand is already live in these forms. Supporting them
requires typed operand-stack snapshots and non-empty-stack `full_frame` emission.
Constant-folded or plain local/literal booleans that do not need a branch diamond
remain printable.

## Comments and source metadata

Space, tab, line feed, `//` comments, and `/* ... */` comments are supported.
Line feed (`LF`, `\n`) is the only supported line terminator; supported source
contains no carriage-return bytes. The lexer skips a carriage return as trivia but
does not increment its line counter, and a `//` comment terminates only at LF, so a
bare CR does not end that comment. These accidental behaviors do not support CR or
CRLF source. Block comments do not nest.

Source line numbers use a 1-based `u16` counter. A supported source therefore has
at most 65,534 LF bytes, so traversal never advances beyond line 65,535; crossing
that boundary can overflow or panic rather than return a diagnostic. Positions are
tracked with a pending-line model and emitted as `LineNumberTable`; the source
basename is emitted as `SourceFile`.

## Class-file size limits

Each assembled method body must be at most 65,535 code bytes. The assembler checks
that JVM limit after goto compaction and panics when it is exceeded. Each modified
UTF-8 payload and source line must also fit the `u16` limits documented above.
These failures are not structured unsupported diagnostics; the valid-reference
precondition still excludes source that the pinned compiler rejects for its own
class-file limits.

## Deliberate refusals

The stable unsupported diagnostic families are:

| Code | Boundary |
| --- | --- |
| `NJP1001` | Recognized but unsupported statement syntax |
| `NJS1001` | Unsupported class shape, call target/value, branch-local declaration, or other semantic surface |
| `NJC1001` | Valid supported-front-end shape requiring live-stack boolean materialization |

Other out-of-subset syntax may fail earlier with an ordinary lexical, parse, or
semantic diagnostic rather than an unsupported code. Unsupported diagnostics are
part of the compiler's current boundary; they are not claims that arbitrary Java
outside this page is diagnosed gracefully.

## Known reachable defects

Two current assembler gaps are reachable from otherwise supported source. They
are defects, not deliberate subset exclusions:

- **Local slots above 255.** Loads and stores use a one-byte operand and currently
  truncate a larger semantic slot instead of emitting the JVM `wide` prefix.
  `wide iinc` already exists, but general wide loads/stores do not. A method with
  enough top-level primitive locals can therefore produce wrong bytes and invalid
  behavior.
- **Long branches.** Every conditional and unconditional branch is currently
  encoded in the narrow signed-16-bit form. If final layout places a target more
  than 32,767 bytes away (or less than -32,768), assembly panics instead of
  selecting javac-compatible long forms and conditional expansion.

Their repair order and the semantic/source defects excluded earlier on this page
are tracked in [active work](../direction/active-work.md). Until fixed, the
byte-identity contract excludes programs that reach those signatures even though
their syntax and semantics otherwise fit the general operator or source surface.

## Explicitly unsupported areas

The current compiler has no support for reference values beyond the unread
`String[]` parameter and a direct string literal print. It also has no loops,
`switch`, conditional expression, exceptions, synchronization, assertions,
arrays, objects, fields, general methods, constructors, packages, imports,
generics, annotations, lambdas, records, enums, interfaces, modules, or preview
features. These areas are cataloged as future research rather than implied by
Java 25 syntax acceptance.
