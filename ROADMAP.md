# ROADMAP.md — infrastructure & architecture evolution

This is the **make-the-compiler-extensible** plan: the infrastructure and
structural refactors that let njavac grow toward a fuller Java compiler while never
losing byte-identity to `javac`. It owns *ordered infra work not yet done*, the
record of what landed, and the open bug backlog; the full charter and its boundary
against README.md (language coverage) and CLAUDE.md (mechanics + conventions) are
defined once in **CLAUDE.md §"Documentation: one fact, one home"**. When an item here
lands, check it off with a one-line "as built" and record the mechanics in CLAUDE.md
— don't restate them here.

It came out of a three-way audit (front-end, back-end, dev/test infra) in 2026-07;
this is the agreed sequencing.

---

## The core diagnosis

njavac is a **clean core wearing a subset-shaped skin.** The architecture is not
sloppy: `compile()` is a tidy four-stage pipeline, `classfile.rs`'s breadth-first
constant-pool interning is a faithful model of javac's `PoolWriter`, and the
opcode-selection helpers (`int_icmp_branch`, `classify_frame`, `subint_narrow_op`)
are exemplary. What feels "hardcoded" is that every layer bakes in one assumption
— *the input is always valid and always in-subset* — and a few data structures are
shaped specifically for straight-line numeric code.

**The load-bearing insight: the dangerous failures as this grows are silent, not
loud.** Three distinct future bugs all produce wrong *bytes* rather than a crash:

1. **Wrong local slots.** `sema::analyze_method` walks only *top-level*
   `LocalDecl`s and never reclaims a slot on scope exit; `next` only ever
   increases. javac *reuses* slots when sibling scopes close. The moment a local
   lives inside an `if`/loop block, njavac's slot numbers diverge — and
   `codegen`'s parallel `Gen::locals` snapshot (which "only ever grows, no
   `chop`") diverges with it. This is the assumption most entangled with
   byte-identity.
2. **Wrong `max_stack`.** The operand-stack model is hand-maintained with per-site
   literal deltas and comments (`self.cur -= 3; // two longs -> one int`). Every
   new opcode (`dup*`, array load/store, `invoke*` with computed arg/return
   widths) is another chance to miscount, and a wrong `max_stack` is a silent
   mismatch.
3. **Wrong constant-pool / attribute order.** Interning order and writing order
   are two hand-maintained sequences in `ClassFile::to_bytes` that must agree.
   Fine at one class attribute (`SourceFile`); fragile at five
   (`BootstrapMethods`, `InnerClasses`, `Signature`, `Exceptions`, annotations).

That reframes the plan. **The first investment is not a refactor — it is the
safety net and feedback loop that make the refactors verifiable.** We don't touch
the slot allocator until a fuzzer and a structural differ can instantly prove
whether byte-identity broke.

---

## Sequencing at a glance

| Phase | Theme | Why this order |
| ----- | ----- | -------------- |
| **0** | Enablers (fuzzer, single-fixture verify, structural differ, CI) | Cheap, immediately useful every turn; converts byte-identity into an automatic check *before* we touch load-bearing code. |
| **1** | Diagnostics foundation (`Diagnostic` + `Span` + 3-way taxonomy) | Makes the fuzzer *trustworthy* (a real bug stops looking like "unsupported"); prerequisite for type-checking and parser recovery. |
| **2** | Keystone refactors (sema scoping, attribute abstraction, `emit()` chokepoint, recursive `Type`) | Byte-preserving structural wins that unlock whole *families* of rungs — now provable by Phase 0's net. |
| **3** | Resume language rungs (`&& || ?:`, loops, methods, string concat) | Each is now much cheaper and safer on the extensible foundation. |

The connective thread between phases: **the fuzzer (Phase 0) depends on the
taxonomy (Phase 1).** Today a genuine njavac bug and a legitimately-unsupported
construct both surface as the same `catch_unwind` → `"unsupported"`. A fuzzer
would silently skip a real bug as "just unsupported." So a v1 fuzzer ships in
Phase 0 using the current reject-by-panic contract, and Phase 1 sharpens it by
making `Unsupported` (skip) genuinely distinct from an njavac invariant violation
(a real finding).

---

## Phase 0 — Enablers

### 0.1 Differential fuzzer — ✅ DONE *(the highest-leverage item)*
`make fuzz`: random in-scope Java vs the pinned javac, in-process, byte-compared;
auto-minimizes a mismatch into a droppable fixture. The only hard-fail signal is *both
compilers accept, bytes differ* — by definition an njavac bug. Found a real bug family
on its first run (backlog below). The mechanics and the 5-touch "grow it for a new
rung" list live in CLAUDE.md §Testing; deferred sub-features are in §"Deferred /
opportunistic improvements".

### Fuzzer-found bug backlog

**Open — grouping provenance is erased around a negated shortcut** (`Fuzz0766276`,
seed `18006986217243057667`, case 766276). Minimal shape:
`v = (!(true || (1L >>> 1L) > 0L)) || v;`. javac materializes the final `v` through
an `ifeq`/`iconst_1`/`goto`/`iconst_0` diamond and emits `StackMapTable`; njavac emits
a bare `iload`/`istore`, so the first structural divergence is the missing
`"StackMapTable"` pool entry. The pool is only the symptom.

The complete black-box split is syntax-sensitive. An unparenthesized
`!(true || N) || v` (and its `!!`/`!!!` siblings) reduces to a bare `v`; wrapping
the complete negated left operand, `(!(true || N)) || v`, forces the final diamond
without emitting an `iconst`/test for that left operand. Parentheses remain
transparent for literals, strict comparisons, locals, live `!local`, non-negated
shortcuts, residual logical items, and bitwise boolean values. njavac currently
cannot represent that distinction because `parser::primary` erases grouping, while
`has_tainted_not` reconstructs an over-broad approximation and `contains_name`
misses the name-free `long >>> long` non-folding quirk.

The fix needs `Expr::Paren`, a strict lowering-constant query separate from
short-circuit verdicts, and explicit condition-item origin/materialization/position
state. It must remove the AST reconstruction and frame-count heuristics, preserving
the existing label/fixup/frame machinery. Cover grouped and ungrouped local,
`long >>> long`, ordinarily-foldable shift, one/two/three `!`, logical wrapper,
materialization, merge, and pending-line siblings before changing behavior.

Targeted census confirms both deciding directions, a final bitwise-boolean leaf,
nested logical wrappers, and arithmetic/cast wrappers around
`N = long >>> long`. The code-free static-false form independently preserves its
line when it ends an outer then-arm, for grouped, ungrouped, local, and name-free
forms; a following statement overwrites that pending line. Controls that already
match include no surrounding `!`, an evaluated (not dropped) `N`, `long >> long`,
`long >>> int`, and ordinary `if` branch use. These are one root-cause family, not
separate pool/frame/line bugs.

**Open — boolean cast loses a left-deciding short-circuit item.** Minimal shapes:
`r = ((boolean) (true || v)) && v;` and `if ((boolean) (false && v)) ... else ...`.
javac materializes the cast operand (`iconst_1; ifeq` or `iconst_0; ifne`) before
the outer logical/statement consumer; njavac's `fold` treats `Cast` transparently,
so `gen_cond`/`gen_if` collapse it to a verdict and omit the residual branch,
diamond, frames, and sometimes a whole arm shape. A cast around the tainted-`!`
family fails for the same reason even when `has_tainted_not` correctly forces the
final diamond. Direct cast assignment and casts of a literal, local, ordinary
`!local`, or fully constant expression all match. The fuzzer does not currently
generate boolean casts, so this family needs a hand-built truth table before its own
fix cycle; it is related to, but not fixed by, broadening the taint predicate above.

### 0.2 Single-fixture verify — ✅ DONE
`make verify FILE=<f>` (cached) / `make bench FILE=<f>` (online): compile one fixture,
byte-compare, print the localized diff on mismatch. See CLAUDE.md §Testing.

### 0.3 Structured class-file differ — ✅ DONE
The `classdiff` bin (`make diff A=… B=…`) and the first-structural-divergence report
the bench prints on any mismatch — works even when `javap` output matches. It is the
mirror of the `classfile` writer (`njavac::classdump`). See CLAUDE.md §Testing.

### 0.4 CI correctness gate — ✅ DONE
`.github/workflows/ci.yml` runs `make correctness` (correctness only, no timing/fuzz)
on push/PR in the pinned image — the unconditional backstop against a byte-breaking
commit reaching `main`. A cold `docker build` each run for now (no GHA layer cache yet).

### 0.5 Fast offline gate (volume-backed) — ✅ DONE
`make verify` byte-compares njavac against goldens the pinned in-image javac recorded
into a Docker volume — no javac spawns, ~1.3s for the whole suite (`make record`
refreshes after a fixture/JDK change). `make bench` stays the authoritative
from-scratch check. See CLAUDE.md §Testing.

---

## Phase 1 — Diagnostics foundation

### 1.1 `Diagnostic` + `Span` + a three-way error taxonomy
- **What.** Introduce `Diagnostic { span, severity, message, code }` and a byte
  `Span { start, end }` carried on tokens (alongside — not replacing — the `line`,
  which is still the cheapest source for `LineNumberTable`) and attached to AST
  nodes. Thread `Result`/a diagnostic sink through `lex`/`parse`/`analyze`/
  `generate`. Crucially, distinguish three kinds from day one:
  - **`SyntaxError`** — the user wrote invalid Java.
  - **`Unsupported`** — valid Java that njavac doesn't support yet (the honest
    state of a subset compiler).
  - **`panic!`** — reserved *only* for "njavac invariant violated," i.e. a real
    bug.
  Replace the ~12 user-facing panics in `parser`/`sema` and `main`'s
  `catch_unwind` (which today collapses all three kinds into one opaque
  `"unsupported (compiler error)"`).
- **Why.** Two payoffs. (1) It makes the fuzzer sound: `Unsupported` → skip, a
  `panic`/internal error → a genuine finding, instead of both looking identical.
  (2) It makes every future rung localizable for a human or an agent: the message
  says *which of the four stages to open*. It is also the prerequisite that makes
  parser error-recovery and sema type-checking meaningful.
- **Effort.** Medium, but almost entirely mechanical. Touches the one explicitly
  redesignable contract (`compile()`'s signature) and `main`.
- **Key files.** `parser.rs` (~10 panic sites), `sema.rs` (undeclared-local
  panics), `lexer.rs` (lexical errors), `main.rs` (`catch_unwind`), `lib.rs`.
- **Done when.** A malformed source yields a spanned `SyntaxError`, an out-of-scope
  construct yields `Unsupported`, and `panic!` survives only as an invariant
  guard.

---

## Phase 2 — Keystone refactors

All of these are **byte-preserving** — they re-express the current output, and
Phase 0's net (fuzzer + differ + single-fixture verify + CI) proves it.

### 2.1 Sema: scoped symbol table + slot-reclaiming allocator  *(keystone)*
- **What.** Replace the two flat `HashMap`s and the top-level-only walk with a
  stack of scopes (`enter_scope`/`exit_scope`/`declare`/`resolve`) and a slot
  allocator that **reclaims slots on scope exit** (per-method high-water mark with
  free-on-pop). Make sema a real pass that descends into `if`/loop/block bodies
  and *emits diagnostics* instead of assuming validity. Fold codegen's parallel
  `Gen::locals` snapshot into consuming sema's per-scope slot info rather than
  maintaining its own monotonic copy.
- **Why.** This is the keystone for language growth: block scope, loops, multiple
  methods, and eventually fields all sit on it. It is also the one refactor where
  the current design produces *silent* byte-mismatches (wrong slots, missing
  `chop_frame`) rather than clean errors — so it must be done, under the safety
  net, *before* the rungs that need it.
- **Effort.** Medium–large.
- **Key files.** `sema.rs` (`analyze_method`, `MethodInfo`), `codegen.rs`
  (`Gen::locals` and the frame snapshot).

### 2.2 Backend: `Attribute` abstraction  *(keystone)*
- **What.** Introduce an `Attribute` concept (name + `intern_constants(&mut cp)` +
  `write_body(&mut buf, &cp)`); give `Method`, the `Code` attribute, and
  `ClassFile` each a `Vec<Attribute>`. Then `attributes_count = vec.len()` and
  `attribute_length` is *measured* from the body buffer (the pattern
  `stack_map_body` already uses), eliminating the hand-summed `Code` length
  arithmetic and the hardcoded counts in `to_bytes`.
- **Why.** Turns every future attribute (`BootstrapMethods`, `InnerClasses`,
  `Signature`, `Exceptions`, `ConstantValue`, annotations, the `Code` exception
  table) from surgery on `to_bytes` into a localized one-variant addition, and
  collapses the duplicated intern-order-vs-write-order lists into one ordered
  `Vec`. It is the hard prerequisite for the next real language rung (string
  concat → `invokedynamic` → `BootstrapMethods`).
- **Order authority (the keystone's teeth).** Phase-2 interning must be *derived
  by walking the single ordered `Vec`* — `intern_constants` walks the write plan
  in the exact order `to_bytes` emits it, so interner and byte-writer share ONE
  sequence and `attribute_length` is *measured* from the body buffer (as
  `stack_map_body` already does), not hand-summed. That makes the phase-2 half of
  the intern-order-vs-write-order hazard (the "Wrong constant-pool / attribute
  order" known issue, #3) *unrepresentable by construction*, not merely
  test-catchable. **Scope it honestly:** this closes only phase 2. Phase-1
  composite/BFS pool *insertion* order (codegen-driven, `classfile.rs`) stays a
  separate hand-maintained order the keystone does not make unrepresentable — so
  don't overclaim "impossible."
- **Effort.** Medium.
- **Key files.** `classfile.rs` (`ClassFile::to_bytes`, the attribute writers).
- **Related.** `invokedynamic`/`Dynamic` pool entries break the "every child is a
  pool entry" invariant (their first component indexes the `BootstrapMethods`
  attribute, not the pool). The attribute abstraction is where that cross-structure
  channel gets added: a `BootstrapMethods` collector with `intern_bootstrap(...) ->
  u16`.

### 2.3 Backend: single `emit(opcode, operands)` chokepoint
- **What.** Funnel all bytecode emission through one method backed by an
  **opcode → stack-effect (in/out words) table**, so `cur`/`max_stack` update in
  exactly one place instead of per-site literal deltas.
- **Why.** A wrong `max_stack` is a silent byte mismatch; centralizing the
  accounting is what stops new opcodes (`dup*`, array, `invoke*`) from silently
  corrupting it. Directly protects AI-driven iteration.
- **Effort.** Medium. Same bytes, computed once.
- **Key files.** `codegen.rs` (all emitters, the `push`/`pop` model).

### 2.4 Front-end: recursive `Type`, collapse `Type`/`ValType`, `#[derive(Debug)]`
- **What.** Make `Type` recursive (`Primitive | Class(name) | Array(Box<Type>) |
  …`), retiring the `StringArray` special case; unify it with sema's parallel
  `ValType` so "add a type" is one edit site, not four (`Type`, `ValType`,
  `valtype()`, `descriptor_of`/`param_vti`/`local_vti`). Add `#[derive(Debug)]` to
  the AST to delete the hand-maintained `DebugExpr` shim in `codegen.rs` (a
  cross-file invariant that silently rots on every new `Expr` variant).
- **Why.** The `StringArray` hack and the twin enums are the concrete source of the
  "hardcoded, not extensible" feeling. `derive(Debug)` is tiny and pure win.
- **Effort.** Small (`derive`) + medium (`Type`). Note `Type` becoming non-`Copy`
  ripples through `Param`/`Cast`/`LocalDecl`/sema.
- **Key files.** `ast.rs`, `sema.rs`, `codegen.rs`.

### 2.5 Front-end: parser precedence table (Pratt)
- **What.** Replace the eight hand-rolled binary-precedence ladder methods
  (`bit_or`→…→`multiplicative`) with a single precedence-climbing loop driven by a
  `binding_power(TokenKind) -> Option<(u8, u8)>` table. Keep recursive descent for
  statements/declarations.
- **Why.** Java has ~15 binary levels plus `?:`, `instanceof`, assignment-as-
  expression, lambda, and cast disambiguation. Each new level is currently a new
  hand-wired method (a silent byte-mismatch risk if wired at the wrong rung); the
  table makes it a one-row edit. Pays off precisely at "operators grow 5×."
- **Effort.** Medium. Recovery (sync-to-`;`/`}`) is a *separate, later* concern —
  only meaningful after Phase 1's diagnostic sink exists.
- **Key files.** `parser.rs`.

### 2.6 De-hardcode the `main`/println/void/Object shape
- **What.** Parameterize `gen_init` on the superclass (not hardwired `Object`),
  generalize `descriptor_of`/`param_vti` off `StringArray`/`)V`, and turn
  `System.out.println` from a bespoke `Expr::Println` statement into an ordinary
  method-call expression resolved in sema (deleting the parser's
  name-resolution-in-the-parser layering violation).
- **Why.** Small and mechanical, but it's what turns "one `main`" into "a real
  class with methods" — the unlock for the multiple-methods rung.
- **Effort.** Small.
- **Key files.** `codegen.rs` (`gen_init`, `descriptor_of`, `param_vti`,
  `gen_expr_stmt`, `gen_println`), `parser.rs` (`primary`'s `System.out.println`
  walk), `ast.rs`/`sema.rs` (a general `Call` node).

---

## Cross-cutting notes

- **Keep the plain-enum AST.** A visitor/fold framework would be premature at this
  size and would fight the borrow checker for little gain. The friction is the
  *closed, subset-specific type universe* and the *missing diagnostic/scope
  infrastructure*, not the enum style. Fix those and the enums scale fine.
- **Do not build a "correct core + javac-quirk overlay."** The project's entire
  premise is byte-identity, so the whole compiler *is* the javac-matching layer;
  splitting it would add complexity for its own sake, and several quirks (folding,
  frame minimization) are too entangled with emission to separate cleanly. The
  pragmatic form of this discipline is what the code already does well: keep each
  javac-matching *decision* in a named pure function with a javac-referenced
  rationale. Enforce that convention as switch/enum/record land (their heuristics —
  `tableswitch`-vs-`lookupswitch` density, String-switch hashing, `$SwitchMap$`
  synthesis — should each be one documented function, not inlined).
- **No checked-in golden `.class` files.** javac stays the live reference; the
  optional offline cache (0.5) is a git-ignored cache, never a source of truth.

---

## Deferred / opportunistic improvements

Smaller wins noticed mid-work that aren't big enough for a phase and don't block
anything — captured here so they surface proactively instead of waiting until
someone trips on them. End-of-cycle reflection (CLAUDE.md §Working conventions)
files its "what would help" items here.

- **Formatting: define a sanctioned rustfmt surface.** The repository is not
  normalized to the current host rustfmt, so `cargo fmt --all` rewrites unrelated
  files and obscures focused diffs. Pin the formatter/config (preferably through a
  `make fmt-check` command using the repository's Rust toolchain) and decide whether
  to do one explicit normalization change; until then, avoid repo-wide formatting.

- **classdiff: disassemble the `code[]` array.** Today the bytecode is one raw-hex
  field, so a `Code` divergence localizes to a byte offset but not an opcode.
  Decoding the instruction stream would name the diverging op (and its operands).
- **`make verify` staleness guard.** `verify` only auto-records when the volume is
  *empty*; if fixtures change and you forget `make record`, it silently compares
  against stale goldens. A freshness check (re-record when any fixture is newer than
  the volume) would remove the footgun. `make correctness` already sidesteps it by
  always using live javac.
- **fuzz: expression-level minimization (v1.1).** The minimizer is statement-level
  only, but the fuzzer's own findings (constant folding) live *inside* a
  declaration's initializer, so a minimized case stays at ~6 decls with full
  initializer expressions instead of a one-liner. Add node-level shrinking (replace a
  `Bin`/`Cmp`/`Logic` node with a child, drop casts/parens, shrink literals toward
  0/1), each gated by the existing three-conjunct predicate, so a fixture is directly
  droppable into `fixtures/`. Wanted before working the fuzzer-found bug backlog.
- **fuzz: generator-validity smoke gate.** The `v ^= 1.5` generator bug (compound
  bitwise with a float RHS → javac-reject) was caught by eyeballing `--dump-sources`,
  not systematically. A cheap gate asserting `generator-invalid ≈ 0` over a small
  probe corpus would catch a new rung's generation code emitting invalid Java the
  moment it regresses (today it silently lowers yield).
- **fuzz: "replay case N of seed S".** A mode to re-run/re-minimize one specific
  finding (e.g. `Fuzz0000264`) without sweeping from the seed — a triage convenience
  for working the backlog.
- **fuzz: multi-seed census helper.** Now that a bare `make fuzz` uses a fresh random
  seed, one run is a spot check — gauging "is the tail clearing?" means eyeballing
  several runs. A `make fuzz-census [RUNS=n]` that runs n random seeds in one command
  and prints the *union* of distinct finding signatures (with a reproduce-seed per
  signature) would make progress-tracking one command. Noticed while clearing the
  goto-compaction / materialization tail.
## Status

Phase 0 landed (0.1–0.3 and 0.5; 0.4 CI gate deferred by decision); Phase 1–3 not
started. All tests run through Docker via the `Makefile`.

As items land, mark them ✅ in place and record the mechanics at the fix site / in
CLAUDE.md — never restate them here, and delete a finished bug's backlog entry (per
CLAUDE.md §"Documentation: one fact, one home").
