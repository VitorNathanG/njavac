# Language Rungs

Language work resumes only after the infrastructure sequence in
[Active Work](active-work.md). Each rung must be researched against the pinned
black-box reference, implemented without narrowing reachable valid cases for
convenience, and protected by byte-identical fixtures.

## Ordered sequence

1. **Conditional expression and full-frame boolean materialization.** Add `?:`
   with typed arms and cross-arm numeric promotion. Support materializing
   comparison and logical values while another operand-stack value is live, such
   as `System.out.println(a < b)` and `System.out.println(a && b)`. Reuse condition
   lowering and typed stack snapshots; emit javac-compatible non-empty-stack
   `full_frame` entries.
2. **Loops.** Add `while` and C-style `for` first, including backward branches and
   existing condition lowering. Complete block-scoped loop locals, slot reuse,
   and `chop_frame` behavior where those shapes require it. Research `do` and
   enhanced `for` as distinct later forms rather than assuming one lowering.
3. **String concatenation.** Add constant-only folding and the first runtime
   `invokedynamic` path with `BootstrapMethods`, new pool entries, recipe bytes,
   and the `MethodHandles$Lookup` `InnerClasses` row. Runtime operands, not the
   mere presence of `String`, decide whether an indy site exists.
4. **General class members.** Add multiple methods, fields, constructors,
   non-`void` returns, parameters, initialization, `ConstantValue`, `<clinit>`,
   and javac-compatible member ordering. This is the transition from a single
   entry-point body to general class structure.

The broader candidate surface and its current confidence are cataloged in the
[research survey](../research/evidence.md).

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

## Rung completion

```mermaid
flowchart LR
    Question[Byte-visible question] --> Corpus[Complete probe corpus]
    Corpus --> Model[Smallest explanatory model]
    Model --> Tidy[Separate prerequisite tidy]
    Tidy --> Feature[One language behavior change]
    Feature --> Gates[Fresh correctness and fuzz gates]
    Gates --> Docs[Support and research movement]
```

A rung is complete only when:

- Every reachable case is either matched or excluded by a concrete missing
  class-file subsystem agreed as a deliberate boundary.
- Current support documentation is updated and obsolete future research is
  removed or narrowed.
- Fixtures cover decision boundaries, not only a happy path.
- The fresh pinned correctness gate passes.
- The differential fuzzer covers the new syntax where its generator can do so
  safely.
