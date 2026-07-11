# njavac

A toy **Java 25 → JVM bytecode** compiler written in Rust whose one hard
constraint is **byte-identical output to the reference `javac`** (GraalVM CE
`25.0.2-graalce`, class-file major version 69). See [`CLAUDE.md`](CLAUDE.md) for
the architecture and the benchmark that is also the test suite.

This document tracks **language coverage**: what the compiler accepts today and,
below, an enumeration of the Java 25 syntax that is **not yet implemented**.

Every requirement tag below was **empirically verified** against
`25.0.2-graalce` by compiling throwaway programs and reading `javap -v -p`
(raw class bytes where `javap` obscures pool tags). Where the first draft's tags
were wrong, the row says so.

---

## Implemented today (Tier-1: "straight-line int")

The entire supported language is:

- **One** `public class Name { … }` — the file's only top-level type.
- **One** method, exactly `public static void main(String[] args)`.
- Types: `int` locals; `String[]` only as `main`'s parameter (never read).
- Statements: `int` local declaration (initializer optional), assignment to an
  already-declared local, and `System.out.println(…)` as an expression statement.
- Expressions: `int` literals, string literals, local reads, unary minus,
  binary `+ - * / %`, parentheses. Literal-only subtrees are constant-folded.
- `System.out.println` of an `int` or a `String` literal.
- Line (`//`) and block (`/* */`) comments; `LineNumberTable` + `SourceFile`;
  the implicit no-arg `<init>`.

---

## How to read the requirement tags

Each unimplemented item notes the class-file subsystem it forces, because that —
not the parsing — is where byte-identity is won or lost.

- **[SMT]** — needs a `StackMapTable`. Forced by a **branch target / stack-map
  merge**, i.e. real control flow, a *materialized* boolean, or a *pattern*
  `instanceof`. **NOT** forced by a type, a straight-line `return x;`, a bare
  `throw`, or a plain `instanceof` stored straight to a local.
- **[indy]** — needs `invokedynamic` + a `BootstrapMethods` attribute. **Every**
  indy site also drags in an `InnerClasses` row for `MethodHandles$Lookup` — a
  third class attribute the Tier-1 emitter doesn't produce yet.
- **[pool]** — introduces new constant-pool entry **kinds**. Beyond the obvious
  `Long`/`Double`/`Float`, this includes `InterfaceMethodref` (tag 11, distinct
  from `Methodref` tag 10), `MethodHandle`, `MethodType`, `InvokeDynamic`, and —
  for modules only — `Module` (19) / `Package` (20). `CONSTANT_Dynamic` (17) is
  **not** needed by any feature below. **Gotcha:** a `Long`/`Double` entry
  consumes **two** logical pool indices.
- **[attr]** — introduces new class-file **attributes**. **Correction to the
  first draft:** the `exception_table` (try/catch) is a *sub-structure of the
  `Code` attribute*, **not** a class-file attribute. The real attribute a
  `throws` clause produces is `Exceptions`, which is unrelated.

---

## Not yet implemented

### A. Primitive types & values

- [ ] `boolean` (+ `true`/`false`) — type itself is free (`iconst_0/1`,
      `istore`); **[SMT]** only when used in a branch. Verified: `boolean b =
      true;` emits no StackMapTable.
- [ ] `long` — **[pool: Long, `ldc2_w`]**, plus **`lconst_0`/`lconst_1`** fast
      path for `0L`/`1L`, and a **two-slot** local model (`lstore`/`lload`)
- [ ] `double` — **[pool: Double, `ldc2_w`]**, **`dconst_0`/`dconst_1`** fast
      path, **two-slot** local model
- [ ] `float` — **[pool: Float]**, but loaded with **single-word `ldc`** (not
      `ldc2_w`), **`fconst_0/1/2`** fast path, and it is **single-slot**
- [ ] `byte`, `short`, `char` — **int-family, no new pool kind**; stored as their
      numeric value; narrowing needs an explicit `(byte)`→`i2b`, `(char)`→`i2c`,
      `(short)`→`i2s` cast; a `char` literal `'a'` loads as `bipush 97`
- [ ] **Two-slot local model** for `long`/`double` — the biggest slot-allocator
      change: slot indices skip by 2 (`lstore_1`, `lstore_3`, …), and
      `max_locals` grows accordingly
- [ ] Numeric conversions — the full opcode set (`i2l l2i i2d d2i i2f f2i d2l d2f
      f2d i2b i2c i2s`), **including implicit widening** in mixed-type arithmetic
      (`long + int` emits `iload; i2l; ladd`)
- [ ] `null` — single opcode `aconst_null`, no pool entry
- [ ] Boxing / unboxing — `Integer.valueOf(I)` / `intValue()` etc. via
      `invokestatic`/`invokevirtual`; unavoidable at **varargs** and **generic**
      call sites (see §C, §G)

### B. Literals

- [ ] `long` literals (`123L`), `float`/`double` literals (`1.5`, `2f`, `1e9`)
- [ ] Hex / octal / binary literals (`0xFF`, `010`, `0b1010`) and underscores
      (`1_000_000`) — all **normalized to numeric value**; the source radix/form
      leaves no trace (`0xFF`→`sipush 255`, `010`→`bipush 8`)
- [ ] Character literals (`'a'`) — become their numeric code, never a pool entry
- [ ] Text blocks (`"""…"""`) — Java 15; compile to a **single `String`
      constant** after javac's incidental-whitespace stripping (see gotchas)
- [ ] Unicode escapes in source (`\uXXXX`); non-ASCII content — stored as
      **modified UTF-8** in the `Utf8` entry (`"étude"` → `… c3 a9 …`)
- [ ] Remaining escape sequences the lexer doesn't yet decode — octal (`\0`–
      `\377`), `\s` (space, Java 15), and the text-block line-continuation `\`
      before a newline (join without a break); the current lexer handles only
      `\t \n \r \" \\ \' \b \f`
- [ ] Class literals — split codegen: reference type `String.class` → `ldc`
      of a `Class`; **primitive `int.class` → `getstatic Integer.TYPE`**
      (a `Fieldref`, not `ldc`)

### C. Operators & expressions

- [ ] Comparison `< > <= >= == !=`, logical `&& || !`, ternary `?:` — **[SMT]**,
      but only when the result is **materialized as a value** (the
      `if_icmp*`/`iconst_1`/`goto`/`iconst_0` diamond). Used directly as a branch
      condition (`if (a < 5)`) they emit just `if_icmpge` with no diamond —
      codegen must know the usage context
- [ ] Bitwise / shift `& | ^ ~ << >> >>>`
- [ ] Compound assignment `+= -= *= /= %= &= |= ^= <<= >>= >>>=` — a narrowing
      target inserts a hidden narrowing cast (`byte b += 1;` → `… iadd; i2b;
      istore`)
- [ ] Increment / decrement `++ --` (the `iinc` fast path for local `int`s)
- [ ] Assignment **as an expression** — `a = b = c;` leaves the value on the
      stack via `dup` (`dup; istore; istore`); `arr[i] = x` used as a value uses
      `dup_x2; iastore`, `obj.f = x` uses `dup_x1; putfield` — the `dup_x*`
      family is otherwise unused by Tier-1
- [ ] **String concatenation with `+`** — **[indy]** `makeConcatWithConstants`,
      **but only if a runtime operand is present**; constant-only (`"a"+"b"`)
      folds to a single `ldc "ab"`. The recipe `Utf8` uses a raw `0x01` byte per runtime arg (and `0x02`
      for a folded constant); arg types live in the indy descriptor. Pulls in `InvokeDynamic`, `MethodHandle` (kind 6), and an
      `InnerClasses` row
- [ ] `instanceof` — **plain** `o instanceof T` is a single opcode, **no [SMT]**;
      **pattern** `o instanceof T t` adds `ifeq`+`checkcast`+bind → **[SMT]**
- [ ] Cast expressions `(Type) expr` (`checkcast` for references); an
      **intersection cast** `(A & B) e` emits **one `checkcast` per bound** (in
      reverse-listed order); a **numeric promotion** inside `?:`
      (`f ? intVal : dblVal`) inserts `i2d` on the narrower arm (no [SMT])
- [ ] `new` — the **`new` / `dup` / `invokespecial <init>`** triple (object
      creation); array creation `new int[n]`→`newarray`, `new String[n]`→
      `anewarray`, `int[]{…}` initializer (a `dup`/index/store loop), and
      multidimensional `new int[2][3]`→`multianewarray` (+ a `Class "[[I"` entry)
- [ ] Array element access `a[i]` (`iaload`/`iastore`/`aaload`/`aastore`),
      `a.length` (`arraylength`), and `a.clone()` (`invokevirtual` on the array
      class `"[I".clone` + a covariant `checkcast`). Partially-dimensioned
      `new int[n][]` is `anewarray "[I"`, **not** `multianewarray`. An **array
      class literal** `int[].class` is `ldc` of a `Class "[I"` (array types are
      reference types) — unlike the scalar `int.class` → `getstatic Integer.TYPE`
- [ ] Field access / assignment on objects (`getfield`/`putfield`)
- [ ] Method invocation — `invokestatic`/`invokevirtual`/`invokespecial`, and
      `invokeinterface` (carries a trailing arg-count byte) which needs the
      **`InterfaceMethodref`** pool kind; `super.m()` → `invokespecial`;
      qualified `Iface.super.m()` (default method) → `invokespecial` on an
      `InterfaceMethodref`; qualified inner `outer.new Inner()` passes the outer
      instance as the first `<init>` arg with an `Objects.requireNonNull` guard
- [ ] Varargs call sites — a **synthetic array** (`anewarray`/`dup`/`aastore`) +
      boxing for primitives; **not** invokedynamic
- [ ] Lambda expressions & method references — **[indy]**
      `LambdaMetafactory.metafactory` (3 BSM args: `MethodType`, `MethodHandle`,
      `MethodType`); a lambda synthesizes a `private static synthetic
      lambda$<method>$<n>` (emitted after its enclosing method); a
      **constructor ref** `Type::new` uses `MethodHandle` kind 8
      (`REF_newInvokeSpecial`), a **static** ref uses kind 6, an **unbound
      instance** ref (`String::length`) uses kind 5 (`REF_invokeVirtual`); the
      SAM call uses `InterfaceMethodref`
- [ ] Generic type arguments at call sites (`Foo.<T>of(…)`)

### D. Statements & control flow — all **[SMT]**

- [ ] `if` / `else if` / `else` — even a **one-armed `if`** needs a StackMapTable
      (one `append`/`same` frame at the fall-through merge)
- [ ] `while`, `do … while` (backward `goto`), C-style `for` (`iinc`), enhanced
      `for` — **two distinct lowerings**: over an array = hidden
      index/`arraylength`/`iaload`; over an `Iterable` =
      `iterator()`/`hasNext()`/`next()`/`checkcast` (`invokeinterface`)
- [ ] `switch` on `int` — `tableswitch` **vs** `lookupswitch` chosen by a
      **density cost model**, not case count (`{0,1}`→lookup, `{0,1,2}`→table)
- [ ] `switch` on `String` — **not indy**; two-pass `hashCode()`+`lookupswitch`
      then `equals()` guards + a hidden `int` selector local
- [ ] `switch` on `enum` (named constants) — **not indy**, and the lowering
      depends on where the enum lives (verified directly): if the enum is nested
      in the **same top-level class** as the switch → direct `ordinal()` +
      `table`/`lookupswitch`, **no synthetic**; if the enum is a **separate
      top-level type** (even in the same file) → javac emits a
      `<SwitchingClass>$1` synthetic class holding a `static final synthetic
      int[] $SwitchMap$<Enum>` (built in a `<clinit>` guarded by
      `catch (NoSuchFieldError)`), and the site does `getstatic $SwitchMap;
      ordinal(); iaload; switch`. The synthetic is created once per outer class
      and **reused** across multiple switches on that enum
- [ ] `switch` **expression** (`yield`, arrow labels) — **[SMT]**
- [ ] `switch` on **patterns** / record patterns / guarded (`case … when …`) —
      **[indy]** `SwitchBootstraps.typeSwitch` (or `enumSwitch` for enum patterns
      / `case null`) **+ [SMT] + [pool] + [attr]**; the heaviest control-flow form
- [ ] `case null`, comma-grouped `case a, b:`, fall-through — sub-forms with
      distinct lowering
- [ ] `break`, `continue` (incl. labeled), labeled statements
- [ ] `return` **with a value** (`ireturn`/`areturn`) — **needs no [SMT]** on its
      own; the §D blanket tag does not apply to a straight-line value return
- [ ] `throw` — `new`/`dup`/`invokespecial`/`athrow`; **no exception table, no
      attribute** by itself
- [ ] `try` / `catch` — an `exception_table` row **inside `Code`** (not a
      standalone attribute) **+ [SMT]** (a `same_locals_1_stack_item` frame
      carrying the caught type); **multi-catch** `A | B` is a distinct shape
- [ ] `try` / `finally` — the `finally` body is **duplicated** into the normal
      path *and* a synthetic catch-`any` (catch_type 0) handler that rethrows
- [ ] try-with-resources — `close()` on both paths + `Throwable.addSuppressed`
      nested handler; first construct to emit `full_frame`
- [ ] `assert` — **far more than [SMT]**: synthesizes a `static final synthetic
      boolean $assertionsDisabled` field, a whole `<clinit>` calling
      `Class.desiredAssertionStatus()`, an `AssertionError` `Class`+`Methodref`,
      and **two** StackMapTables
- [ ] `synchronized` block — `monitorenter`/`monitorexit` + a self-covering
      catch-`any` handler → **[SMT] + exception table**
- [ ] Exhaustive pattern/enum `switch` — javac injects a synthetic
      `throw new MatchException(...)` default (a byte-identity hazard)
- [ ] Block statements / nested scopes and local-slot reuse
- [ ] Multiple local declarators (`int a = 1, b = 2;`); `final` locals; `var`
      (Java 10, byte-invisible — see §H)

### E. Type & member declarations

- [ ] More than one method; **method overloading**; non-`void` return types;
      parameters beyond `String[] args` — **no new attribute**, just pool
      descriptors (overloads differ only by descriptor)
- [ ] `throws` clause → the **`Exceptions`** attribute (distinct from the
      `try`/`catch` exception table)
- [ ] **Class header `extends` / `implements`** — a non-`Object` superclass sets
      `super_class` to that class (and the synthesized `<init>` calls *its*
      `<init>`, not `Object.<init>`); `implements` populates the `interfaces`
      table. Today every Tier-1 class implicitly extends `Object` with an empty
      interfaces list
- [ ] Instance methods; `this`; explicit constructors, `this(…)`/`super(…)`
      chaining — **no new attribute**, just `Methodref`s
- [ ] Fields — instance & `static`; instance initializers fold into `<init>`,
      static ones into a synthesized **`<clinit>`**; a compile-time-constant
      `static final`/`final` primitive also emits a **`ConstantValue`** attribute
      (an instance `final` gets *both* `ConstantValue` and a runtime `putfield`)
- [ ] Static & instance initializer blocks (merge into `<clinit>`/`<init>` in
      source order)
- [ ] Access & other modifiers — flag bits `ACC_PRIVATE/PROTECTED/FINAL/ABSTRACT/
      SYNCHRONIZED/NATIVE/TRANSIENT/VOLATILE`, plus **`ACC_VARARGS (0x0080)`** set
      on a varargs *method declaration* (`f(int... xs)`). **`strictfp` is
      byte-invisible** (a no-op since Java 17); `native`/`abstract` methods carry
      no `Code`
- [ ] Nested & inner, local, anonymous classes → **`InnerClasses`** +
      **`NestHost`**/**`NestMembers`** + (local/anon only) **`EnclosingMethod`**;
      a synthetic `final synthetic this$0` capture field + an
      `Objects.requireNonNull` capture idiom in the ctor; **`MethodParameters`**
      on synthetic ctors. `NestMembers` and `InnerClasses` use **different**
      member orders
- [ ] `interface` (incl. `default`/`static`/`private` methods) — fields are
      implicitly `public static final` + `ConstantValue`
- [ ] `abstract class`
- [ ] `enum` — Java 5 — emits a **`Signature`** attribute *even with no user
      generics*, plus synthetic `$VALUES` field and `values()`/`valueOf(String)`/
      `$values()` methods (fixed order) and `MethodParameters`. **Constant-specific
      bodies** (`RED { … }`) generate an **anonymous subclass** per such constant
      (+ its `InnerClasses` rows); an enum may also declare fields, a private
      constructor, and `implements` an interface
- [ ] `record` — Java 16 — **`Record`** attribute **+ [indy]**
      `ObjectMethods.bootstrap` for `equals`/`hashCode`/`toString` (+ its
      `InnerClasses`/`BootstrapMethods`) + `MethodParameters` + accessors
- [ ] `sealed` / `non-sealed` / `permits` — Java 17 — `sealed` → the
      **`PermittedSubclasses`** attribute (no flag bit); **`non-sealed` is
      byte-invisible**
- [ ] Annotation **use** & `@interface` — RUNTIME retention →
      **`RuntimeVisibleAnnotations`**, CLASS → **`RuntimeInvisibleAnnotations`**,
      SOURCE (`@Override`) → **nothing**; `@Deprecated` uniquely emits *both* a
      `Deprecated` attribute and `RuntimeVisibleAnnotations`; `@interface` sets
      `ACC_ANNOTATION`. Parameter annotations → `RuntimeVisible/InvisibleParameterAnnotations`.
      An `@interface` element with a `default` emits an **`AnnotationDefault`**
      attribute. The `element_value` union uses literal tag bytes (`I Z C … s`=
      String, `c`=class, `e`=enum, `[`=array, `@`=nested) — byte-exact and a
      hazard for every annotation-carrying feature
- [ ] **Type annotations** (`@Target(TYPE_USE)`, e.g. `List<@NN String>`,
      `(@NN String) o`, `new @NN int[3]`) → **`RuntimeVisibleTypeAnnotations`** /
      **`RuntimeInvisibleTypeAnnotations`**, which can land on the **field,
      method, AND `Code`** attributes; each encodes a `target_type`
      (`FIELD`/`METHOD_RETURN`/`METHOD_FORMAL_PARAMETER`/`METHOD_RECEIVER`/
      `THROWS`/`CAST`/`NEW`/`LOCAL_VARIABLE`/…) + a `type_path` (`[ARRAY]`,
      `TYPE_ARGUMENT(n)`)
- [ ] **Repeating annotations** (`@Repeatable`) — two `@X` on one element
      synthesize the container `@XContainer` holding a `value` array in
      `RuntimeVisible/InvisibleAnnotations`; no standalone `@X` entries survive
- [ ] Synthetic **bridge methods** (`ACC_BRIDGE|ACC_SYNTHETIC`, emitted last)
      from covariant/generic overrides
- [ ] `<clinit>` / `<init>` synthesis and **member ordering** — `<clinit>` is
      always emitted **last**; fields/methods otherwise follow source order

### F. Compilation-unit structure

- [ ] `package` declarations — prefixes the `this_class` internal name
      (`pkg/Name`) and nests the output path; **`SourceFile` stays the bare
      basename**; nothing else changes
- [ ] `import` — single-type, on-demand (`.*`), and `static` — **zero class-file
      trace** (compile-time name resolution only)
- [ ] Multiple top-level types in one file → one `.class` each, all stamped with
      the same `SourceFile`; the non-public siblings are `ACC_SUPER` only
- [ ] `package-info.java` — emits a `package-info.class` with flags `0x1600`
      (`ACC_INTERFACE|ACC_ABSTRACT|ACC_SYNTHETIC`), **no members**, carrying the
      package annotations in `RuntimeVisibleAnnotations` (a package-level
      `@Deprecated` emits *only* the annotation here — **no** `Deprecated`
      attribute, unlike on a class)
- [ ] Local type declarations inside a method — a local `class`/`record`/
      `interface`/`enum` behaves like a nested type but also gets an
      **`EnclosingMethod`** attribute (a local `record` still pulls in the
      `ObjectMethods` **[indy]**)
- [ ] `module-info.java` — Java 9 — **[pool: Module (19), Package (20)]** (javap
      mislabels these "Unknown"), **[attr: Module]** (+ `SourceFile`), class flag
      `ACC_MODULE 0x8000`. javac emits **only** `Module`+`SourceFile` — **not**
      `ModulePackages`/`ModuleMainClass` (those are added by `jar`/`jlink`, so
      njavac must NOT emit them). The implicit `requires java.base` carries
      `ACC_MANDATED`; an explicitly-written one does not; the requires entry
      records a version string = the JDK build (`"25.0.2"`)

### G. Generics — Java 5

- [ ] Generic type & method declarations, bounded types, wildcards, the diamond
      `<>` (which compiles **identically** to an explicit type argument)
- [ ] The **`Signature`** attribute — lands on the **class, fields, AND methods**
      (not just the class); the descriptor stays fully **erased**. An
      **intersection bound** `<T extends A & B>` writes `::` in the signature
      (`<T::LA;:LB;>`), erases to the **first** bound, and inserts a `checkcast`
      to a non-first bound at each use site
- [ ] `checkcast` inserted on every generic **read** whose erased return type is
      wider than the source type; autoboxing at generic call sites
- [ ] Bridge methods from generic overrides (see §E)

### H. Modern language features (Java 8 → 24)

- [ ] Lambdas & method references — Java 8 — **[indy]** (§C)
- [ ] `var` — Java 10 — **byte-invisible**; `var x = 42;` is identical to
      `int x = 42;` (no `LocalVariableTable` at default settings)
- [ ] Switch expressions — Java 14 — **[SMT]**
- [ ] Text blocks — Java 15 — a single `String` constant (§B)
- [ ] Records — Java 16 — **[attr: Record] + [indy]** (§E)
- [ ] Pattern matching for `instanceof` — Java 16 — **[SMT]** (§C)
- [ ] Sealed classes — Java 17 — **[attr: PermittedSubclasses]** (§E)
- [ ] Pattern matching for `switch` & **record patterns** — Java 21 —
      **[indy: typeSwitch] + [SMT] + [pool] + [attr]** (§D)
- [ ] Unnamed variables & patterns (`_`) — Java 22 — **byte-invisible** (no `_`
      in the pool; the slot is simply unnamed)

### I. Java 25 headline language features

- [ ] **Compact source files & instance `main`** (JEP 512, **final** in 25) —
      a file with no class declaration + a `void main()`. Verified output shape:
      the class is named after the **file basename** (which must be a legal Java
      identifier), is `ACC_FINAL`, holds an **instance** (non-static) `main`, and
      gets a **synthesized default constructor** (plain flags `0x0000`, *not*
      `ACC_SYNTHETIC`); only a `SourceFile` attribute; no static shim. This is a
      genuinely new codegen shape (first non-static method + first synthesized
      ctor)
- [ ] **Module import declarations** (`import module M;`) (JEP 511, **final** in
      25) — **compile-time only, zero class-file trace** (do not confuse with the
      `module-info` pool/attr tag in §F)
- [ ] **Flexible constructor bodies** (statements before `this(…)`/`super(…)`)
      (JEP 513, **final** in 25) — just relaxed statement ordering before the
      `invokespecial <init>`; no new subsystem
- [ ] **Primitive types in patterns, `instanceof`, and `switch`** (JEP 507,
      **preview** in 25) — needs `--enable-preview`, which stamps the class-file
      **minor version `0xFFFF` (65535)**; uses `typeSwitch` plus a
      `ConstantBootstraps.primitiveClass` bootstrap

---

## Cross-cutting byte-identity gotchas

Discovered while verifying the above; these bite regardless of which feature
pulls them in:

- **StackMapTable frame selection is an optimizer, not a fixed choice.** javac
  picks the *smallest* frame that encodes the delta — `same` (0–63),
  `same_locals_1_stack_item` (64–127), `chop` (248–250), `same_frame_extended`
  (251), `append` (252–254), `full_frame` (255) — with a −1 `offset_delta` bias
  between consecutive frames. Matching this exactly is the hard part of all of §D.
- **`Long`/`Double` constants consume two pool indices** — the pool counter must
  skip one after each.
- **Modified UTF-8** for all `Utf8` entries (differs from standard UTF-8 only for
  NUL and supplementary chars, but the writer must be correct).
- **The string-concat recipe uses literal `0x01`/`0x02` bytes**, which `javap`
  *renders* as `\u0001`/`\u0002`; getting the raw byte wrong is a silent mismatch.
- **MethodHandle reference-kind numbers are serialized bytes** — kind 6
  (`REF_invokeStatic`) for bootstrap/lambda-impl targets, kind 8
  (`REF_newInvokeSpecial`) for constructor refs.
- **Member ordering is deterministic and load-bearing**: `<clinit>` last;
  synthetic `lambda$`/bridge methods after the real ones; enum and record
  generated members in a fixed order; `NestMembers` vs `InnerClasses` in
  *different* orders.
- **`--enable-preview` sets minor version `0xFFFF`** on every class in the
  compilation — a 2-byte header difference.
- **Attribute emission order matters** — e.g. a generic class writes `Signature`
  then `SourceFile`.
- **Constant folding generalizes** to `long`/`double`/`float` with the correct
  wrapping / IEEE semantics, exactly as the current `int` path does — a folded
  constant must be bit-identical to the unfolded computation. The **IEEE
  landmines**: `Infinity` (`1.0/0.0`), `NaN` (`0.0f/0.0f`), signed `-0.0` (a
  *distinct* pool entry from `0.0`), and `Long.MIN_VALUE` all fold to exact bit
  patterns — a bit-off writer mismatches silently.
- **Dead-branch elimination on constant conditions** — `staticFinalTrue ? a : b`
  and `if (false) …` compile to just the live arm; javac prunes provably-dead
  code, so codegen must too.

---

## Determinism

javac's output is a **deterministic function of (source + JDK build +
classpath)** — there is no timestamp or hash in the class bytes (the `Last
modified`/`SHA-256` `javap` prints are filesystem metadata, stripped by the
bench). That determinism is the premise the whole project rests on. The one
classic source of genuine non-determinism, **annotation processing**, is out of
scope. Three qualifications shape njavac's design:

- **Environment-coupled** (deterministic, but tied to the exact toolchain):
  `module-info`'s `requires java.base` records the JDK version string
  (`"25.0.2"`); every resolved library descriptor is pinned to the JDK. A
  different GraalVM point release legitimately changes bytes — the same caveat
  CLAUDE.md gives for the golden classes.
- **Context-dependent** (not a pure function of the one `.java` file): anything
  that resolves *other* types embeds their resolved descriptors/internal names —
  overload resolution (which `println(...)`), the `Methodref`-vs-
  `InterfaceMethodref` split, boxing/widening choices, generic inference and
  `checkcast` targets, `Signature`/`Exceptions`/annotation `Class`-values, the
  `EnclosingMethod` descriptor, and every bootstrap descriptor
  (`StringConcatFactory`/`LambdaMetafactory`/`ObjectMethods`/`SwitchBootstraps`).
  The `$SwitchMap$` synthetic depends on the *target enum's* constant set (from
  another unit) and is cached and reused across switches. This is why the
  current int-only subset is tractable *without* a type environment — and why
  most of §C–§H will need one.
- **Implementation-defined-but-stable** (javac's choices, not mandated by the
  JVMS): StackMapTable frame minimization, the `tableswitch`-vs-`lookupswitch`
  density heuristic, String-switch hash bucketing, the very existence/shape of
  the `$SwitchMap$` helper, the `Objects.requireNonNull` capture idiom, and
  synthetic/lambda naming + member ordering. All reproducible only by matching
  this exact javac build — i.e. reverse-engineering, not spec-reading.

---

## Suggested next rungs

Cheapest-first, given the byte-identity constraint:

1. **`boolean` + comparisons + `if`/`else`** — the first branch, which forces the
   **`StackMapTable`** (and its frame-selection optimizer). The single hardest
   class-file subsystem and the gate to all of §D; do it once and most control
   flow follows.
2. **`while` / `for`** — backward branches; reuse the `StackMapTable` machinery.
3. **String concatenation** — the first **`invokedynamic`** + `BootstrapMethods`
   (+ the `InnerClasses`/`MethodHandles$Lookup` row), unlocking realistic
   `println`. Remember the runtime-operand condition and the raw recipe bytes.
4. **`long` / `double`** — new pool kinds, the two-slot value model, and the
   `lconst`/`dconst`/`ldc2_w` load ladder.
5. **Multiple methods, fields, constructors** — the move from "one main" to real
   class structure (`<clinit>`, `ConstantValue`, member ordering).

Byte-invisible freebies you can add any time once the surrounding machinery
exists, at zero class-file cost: **`var`**, **unnamed `_`**, **text blocks**
(just a `String` constant), and **flexible constructor bodies**.

Add a fixture in `fixtures/` for every new rung (filename must match the
`public class` name) and keep the benchmark's correctness pass green — it
byte-compares against the live `javac` on every run.
