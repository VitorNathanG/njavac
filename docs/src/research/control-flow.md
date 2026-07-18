# Control-Flow Research

This page preserves future statement and control-flow leads. It is not current
support; current `if` behavior is defined in
[Language Support](../reference/language-support.md#conditions-and-boolean-values).
Unless marked otherwise, entries are migrated **[U]** reports under
[Evidence and Confidence](evidence.md).

## Loops and jumps

- **[U] `while` and `do`/`while`:** require backward branches, loop-entry and
  exit frames, condition polarity, and exact line-position behavior.
- **[U] C-style `for`:** adds initializer/update ordering and commonly reaches
  `iinc`, but must not assume every update uses that form.
- **[U] Enhanced `for` over arrays:** reported to lower through hidden array,
  length, and index state plus `arraylength` and typed element loads.
- **[U] Enhanced `for` over `Iterable`:** reported to call `iterator()`,
  `hasNext()`, and `next()` with `invokeinterface`, then insert a `checkcast` where
  required.
- **[U] `break` and `continue`:** require nested target stacks, finally/resource
  interactions, and line/frame placement.
- **[U] Labels:** labeled statements and labeled break/continue add a separate
  semantic namespace and non-local target resolution.

`while` and C-style `for` are the second ordered
[language rung](../direction/language-rungs.md).

## Integer switch

- **[U] Opcode choice:** `int` switch uses `tableswitch` or `lookupswitch`.
- **[U] Selection model:** the migrated survey reports a density cost model rather
  than a simple case-count threshold, with `{0,1}` choosing lookup and `{0,1,2}`
  choosing table in tested examples.
- **[U] Layout:** both opcodes require alignment padding, ordered keys/targets, a
  default target, and stack-map evidence at reachable entries and joins.

The two examples do not establish the complete density formula. A corpus must
cover negative keys, sparse ranges, boundary costs, source order, grouped labels,
fall-through, and code-size interactions.

## String switch

- **[U] Lowering:** reported not to use indy. The survey describes a two-pass
  lowering: `hashCode()` plus `lookupswitch`, then collision-resolving `equals()`
  guards and a hidden integer selector local.
- **[U] Hazards:** hash collisions, source order, duplicate strings, null behavior,
  selector slot choice, line positions, and frame topology need retained evidence.

## Enum switch

- **[U] Nested same-class enum:** when the enum is nested in the same top-level
  class as the switch, the survey reports direct `ordinal()` plus table/lookup
  switch and no synthetic switch-map class.
- **[U] Separate top-level enum:** even in the same source file, the survey reports
  a `<SwitchingClass>$1` synthetic helper with a `static final synthetic int[]`
  named `$SwitchMap$<Enum>`.
- **[U] Helper initialization:** the array is reportedly built in `<clinit>` with
  one `NoSuchFieldError` guard per enum constant.
- **[U] Use site:** reportedly loads the switch map, calls `ordinal()`, performs
  `iaload`, then switches on the mapped integer.
- **[U] Reuse:** one outer class reportedly reuses the helper across multiple
  switches on the same enum.

These reported context-dependent shapes need a multi-source corpus; they are not
safe to infer from one enum arrangement.

## Switch forms

- **[U] Statement details:** `case null`, comma-grouped labels, colon labels,
  fall-through, and explicit default have distinct lowering and reachability
  consequences.
- **[U] Switch expressions:** arrow labels and `yield` produce value joins and
  stack maps.
- **[U] Pattern and record-pattern switch:** reported to use
  `SwitchBootstraps.typeSwitch`, or `enumSwitch` for some enum-pattern/`case null`
  shapes, plus stack maps, new pool entries, and bootstrap-related attributes.
- **[U] Guards:** `case ... when ...` adds evaluation and restart behavior that
  must be probed with pattern ordering.
- **[U] Exhaustiveness:** exhaustive enum/pattern switches reportedly receive a
  synthetic default that throws `MatchException`, even when source has no default.

## Return and throw

- **[U] Value return:** straight-line primitive and reference returns use typed
  return opcodes and do not by themselves require `StackMapTable`.
- **[U] Bare throw:** constructing and throwing an exception uses object creation
  and `athrow`; a bare throw does not by itself create an exception table or new
  class attribute.
- **[U] Abrupt completion:** branch pruning, unreachable statements, finally
  execution, synchronized exit, and verifier state require a unified control-flow
  model before general return/throw support.

## Exceptions

- **[U] `try`/`catch`:** reported to add rows to `Code.exception_table` and
  handler frames carrying the caught object, often
  `same_locals_1_stack_item`.
- **[U] Multi-catch:** `catch (A | B e)` is a distinct union-type and exception-
  table shape rather than ordinary bitwise syntax.
- **[U] `finally`:** the survey reports duplication of the finally body into the
  normal path and a synthetic catch-all handler (`catch_type` zero) that rethrows.
- **[U] Try-with-resources:** reported to call `close()` on normal and exceptional
  paths and use nested handling with `Throwable.addSuppressed`; the migrated
  survey identified it as an early source of `full_frame`.

Exception support requires symbolic half-open code ranges, handler edges in
verifier analysis, and exact exception-table order.

## Assertions and synchronization

- **[U] `assert`:** reported to synthesize a `static final synthetic boolean`
  `$assertionsDisabled`, a `<clinit>` calling
  `Class.desiredAssertionStatus()`, `AssertionError` references, conditional
  branches, and two `StackMapTable` attributes across the generated methods in
  the surveyed minimal shape.
- **[U] `synchronized` block:** reported to use `monitorenter`/`monitorexit` plus a
  self-covering catch-all handler so monitor exit occurs on exceptional paths.

Both forms cross statement lowering, synthetic members, exception ranges, frames,
and class/member ordering; they are not parser-only additions.

## Blocks, declarations, and scopes

- **[U] General blocks:** introduce lexical scope independent of `if` arms.
- **[U] Slot reuse:** sibling scopes may reuse physical local slots while
  `max_locals` retains a high-water mark; exact javac choices require boundary
  probes with category-2 locals and holes.
- **[U] Frame transitions:** scope exit can produce `chop_frame`, while hidden
  locals and definite-assignment holes can force other encodings.
- **[U] Declarations:** multiple declarators, `final`, and `var` require syntax and
  attribution work even when some modifiers or inferred spellings are
  byte-invisible.

Current branch-local declarations remain a deliberate refusal, not partial block
support.
