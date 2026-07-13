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

### 0.1 Differential fuzzer  *(the single highest-leverage item)* — ✅ DONE (2026-07)
- **What.** A dependency-free `src/bin/fuzz.rs` that generates random *in-scope*
  Java (`main` bodies: N primitive locals, literals biased toward the constant-load
  boundaries + IEEE landmines, random operator/cast/compound-assign trees, nested
  `if/else`, and boolean expression trees over `&& || ! < > == …` incl. constant
  operands), compiles each with both compilers, and byte-compares. On mismatch it
  auto-minimizes (statement-level ddmin) and dumps the reduced `.java` ready to drop
  into `fixtures/`. Seed-based (`fuzz <seed>`) for reproducibility.
- **Why.** The byte-identity property is unusually fuzzer-friendly: the oracle is
  free and total (real `javac`), the predicate is a trivial `cmp`, and njavac
  already *cleanly refuses* out-of-scope input. The generator emits valid in-scope
  Java and the only hard-fail signal is *both compilers accept, bytes differ* — by
  definition an njavac bug. It grows one rung at a time and is a permanent net.
- **As built.** Shipped 2026-07, designed via a 4-lens agent panel. Its living
  mechanics — the oracle contract, the three soundness invariants, the generator
  scope boundary, the performance model, and the commands/flags — are documented
  once in **CLAUDE.md §Testing** (a new rung grows the fuzzer by the 5-touch list
  there); they are not restated here. It found a real constant-folding bug family on
  its first run (backlog below).
- **Deferred to v1.1.** Expression-level minimization (v1 is statement-level, so a
  minimized fixture keeps its decls' full initializers); `--jobs` parallelism.

### Fuzzer-found bug backlog (2026-07 census: 5000 cases)
The first sweep found njavac diverging on ~18% of random in-scope programs
(`generator-invalid=0` / `njavac-reject=0`, so all are confirmed real byte-identity
bugs). A second sweep (2026-07-12) re-diagnosed each signature against a javac
ground-truth probe corpus; the original two-root-cause summary was **partly wrong**
(the `-2L + 1.0f` "mixed-type fold" it named already folds byte-identically today),
and a third, unrelated root cause surfaced. The real breakdown is **three** causes:

- **A. NaN not canonicalized** — ✅ **FIXED (2026-07-12).** `float v = -(0.0f/0.0f)`
  folded to a sign-flipped NaN (`0xFFC00000`) where javac canonicalizes to
  `0x7FC00000`. Fix: `ConstantPool::float`/`double` collapse every NaN to the
  canonical bits before interning (matching `Float.floatToIntBits`), leaving `-0.0`
  distinct. Removed the `cp[N].float_bits` (52) + `double_hi` (53) signatures; census
  894 → 790 findings. Regression fixture: `fixtures/folding/NanCanon.java`.
- **B. `long >>> long` shift** (~most of the remaining findings; `constant_pool_count`,
  `methods[N].attr[N].length`, `Code.code`, `cp[N].long_hi/long_lo/tag`). Reverse-
  engineered rule: javac constant-folds **every** shift *except* `long >>> long`
  (unsigned shift, left `long`, right static type `long`) — a genuine javac ConstFold
  quirk. And a constant shift *distance* is always narrowed to an `int` constant
  (`bipush 40`), never `ldc2_w long; l2i`. njavac over-folds `long>>>long` and emits
  the long distance + `l2i`. **Two coupled changes:** (B2) `fold`'s `Expr::Binary` arm
  returns `None` for `UShr` when both operands fold to `Const::Long`; (B1) a
  `gen_shift_distance` helper (used in `gen_binary` and the `gen_compound` shift arm)
  narrows a constant distance via `emit_int_const(to_i32(c))`. B1 alone also fixes the
  independent `int y = x << 40L` divergence (max_stack 2 vs 3). Repros: `Fuzz0000551`
  (`long a = 127L >>> 62L`), and `int y = x << 40L`.
- **C. Compound-assign with a negative constant on a narrowing target** — ✅ **FIXED
  (2026-07-12).** `char v -= -100` emitted the raw `bipush -100; isub; i2c` where javac
  normalizes to `bipush 100; iadd; i2c` (non-negative magnitude, operator chosen by the
  effective delta's sign). The `int` iinc-overflow path already normalized; the general
  narrowing path (char/short/byte) did not. Fix: a shared `int_delta_magnitude` helper +
  an `int_additive_const_delta` guard in the general path — `StackTy::Int` + additive op
  + constant RHS only, so `long`/`float`/`double` keep the raw `lsub`/`dsub`/`fsub`.
  Removed the `cp[N].int` signature and most `Code.code` findings (census 790 → 737).
  Regression fixture: `fixtures/compound-assign/CompoundNegConst.java`.

Fix each as its own cycle with the fuzzer as the regression gate (fix → `make
correctness` green + `make fuzz` shows the signature gone → commit a minimal,
documented regression fixture in the fitting `fixtures/` subfolder, per CLAUDE.md
§"Every bug fix lands with a documented regression fixture"). **Remaining: B** (`long
>>> long` shift) — the whole 737-finding tail.

### 0.2 Single-fixture verify command — ✅ DONE (2026-07)
- **What.** Teach `bench` to accept a single `.java` *file* (not just a
  directory): compile just that one, byte-compare, and on mismatch print the
  existing `print_first_divergence` diff, then exit. Expose it as a first-class
  command and document it in CLAUDE.md as *the* verify command, replacing the
  "there is no single-fixture flag / hand-run the pipeline" paragraph.
- **Why.** Today iterating on one case means hand-running
  `javac && njavac && cmp && javap -diff` from memory — error-prone (wrong `-d`,
  stale artifacts, forgetting the header-line filtering the bench already does).
  This collapses it to one canonical, correct, localized command. Highest
  effort-to-payoff ratio for day-to-day (and agent) iteration.
- **Effort.** Small (~1 hr) — reuses 100% of the existing correctness + diff
  machinery in `bench.rs`.
- **As built.** `bench` takes a `<File.java>` positional; under the Docker-only
  test policy it is invoked through the `Makefile` — `make verify FILE=<File.java>`
  (fast) or `make bench FILE=<File.java>` (online). (An earlier local wrapper and
  the raw `docker-*.sh` scripts were folded into the self-contained Makefile.)
- **Done when.** `make verify FILE=fixtures/branches/IfElse.java` prints pass or a
  localized diff. ✅

### 0.3 Structured class-file differ — ✅ DONE (2026-07)
- **What.** A tool (bin or a `bench --raw`/`--pool-diff` mode) that parses both
  `.class` byte streams into a typed tree (pool entries with resolved cross-refs,
  methods, each attribute, decoded StackMapTable frames) and reports the *first
  structurally-divergent node with a byte offset* — e.g. `constant_pool[17]:
  javac=Methodref(...) njavac=NameAndType(...) at byte 0x84`.
- **Why.** The current `javap -v` diff goes blind exactly when it matters — the
  bench itself flags "bytes differ but javap matches → likely a trailing/attribute
  byte" and gives nothing actionable. It also diffs *text* order, so a one-byte
  pool-count shift cascades into hundreds of lines whose "first" divergence is far
  from the cause. njavac already has the writer half; the reader is its mirror and
  reuses `Entry`/`Method`/`StackFrame`. Pure tooling, zero byte-identity risk.
- **Effort.** Small–medium (~half a day).
- **Done when.** Given two `.class` files it names the first structural divergence
  with a byte offset, even when `javap` output matches.

### 0.4 CI correctness gate — DEFERRED
- **What.** A minimal `.github/workflows/ci.yml` that runs the **correctness pass
  only** (timing is host/harness-gated and pointless in CI). Prefer reusing the
  existing Docker image (`docker build` then run the bench in correctness-only
  mode) since that is the exact pinned `25.0.2-graalce` environment that
  guarantees the golden bytes.
- **Why.** Nothing today prevents a commit that breaks byte-identity from reaching
  `main`, and the standing rule is push-after-every-commit. The reproducible-javac
  hard part is already solved — we are one YAML file from a real backstop.
- **Effort.** Small (~1–2 hr).
- **Done when.** A push/PR runs the correctness pass on the pinned toolchain and
  fails red on any mismatch.

### 0.5 Fast offline gate (volume-backed) — ✅ DONE (2026-07)
- **What.** `bench --record` writes `javac` outputs to a cache dir; `bench
  --offline --golden-dir <dir>` byte-compares njavac against that cache with no
  `javac` invocation. Recording batches the whole suite into **one** javac
  invocation (one JVM startup, not one per fixture).
- **As built (Docker-only policy).** The original design was a *local* javac-free
  loop, which the "all tests via Docker; local runs disallowed" policy forbids — a
  host-recorded cache could reflect a non-pinned `javac`. The on-policy form is
  `make verify`: it records the goldens **inside the image** (pinned javac) into a
  **Docker volume** (`njavac-goldens`), then runs `bench --offline` against that
  volume. Everything stays in Docker; the volume is just cache storage populated by
  the pinned compiler, never committed, never hand-edited. Auto-records when the
  volume is empty; `make record` forces a refresh after fixtures/JDK change.
- **Why.** Makes the *mandatory* Docker correctness gate fast: ~1.3s for the whole
  183-fixture suite (warm volume) vs ~30s for a full online run, because the online
  path pays one javac JVM-startup per fixture and the offline path pays none.
  `make bench` stays the authoritative from-scratch check (live pinned javac) plus
  timing.
- **Caveat.** The volume can go stale — re-record (`make record`) after changing
  fixtures or rebuilding on a new JDK.
- **Effort.** Small (~2 hr).

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

## Status

- **Phase 0** — landed 0.1 (differential fuzzer, 2026-07 — found a real
  constant-folding bug family on its first run; see the backlog above), 0.2
  (single-fixture verify), 0.3 (structured class-file differ), 0.5 (fast offline
  gate, volume-backed & on-policy); commands documented in CLAUDE.md §Testing. **0.4
  (CI gate) remains deferred** by decision. All test execution runs through Docker
  via the `Makefile` (`make verify` fast / `make bench` authoritative); local runs
  are disallowed.
- **Phase 1–3** — not started.

As items land, check them off here and record the resulting mechanics in CLAUDE.md
(and any new language surface in README.md), in the same commit as the change.
