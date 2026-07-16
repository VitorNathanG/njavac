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
loud.** The initial audit found three immediate byte-level risks: local-slot and
verifier-state drift, distributed `max_stack` accounting, and parallel
constant-pool/attribute ordering. Phase 2.1–2.3 close their current concrete forms;
the remaining work below removes the subset-shaped type and parser boundaries
before language growth resumes.

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
taxonomy (Phase 1).** That integration is now complete: the current oracle contract
lives in CLAUDE.md §Testing.

---

## Phase 0 — Enablers

### 0.1 Differential fuzzer — ✅ DONE *(the highest-leverage item)*
`make fuzz`: random in-scope Java through an exact-byte layer followed by persistent
execution observation for byte divergences. The open behavioral finding backlog is
below. The mechanics and the 5-touch "grow it for a new rung" list live in CLAUDE.md
§Testing; deferred sub-features are in §"Deferred / opportunistic improvements".

### Fuzzer-found bug backlog

No open findings.

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

## Phase 1 — Diagnostics foundation — ✅ DONE

`Diagnostic`/`Span` and the three-way returned-syntax/returned-unsupported/internal-
panic taxonomy now run through every compiler stage, the CLI, and the fuzzer. See
CLAUDE.md §Architecture for the stage contract and §Testing for the oracle policy.

---

## Phase 2 — Keystone refactors

All of these are **byte-preserving** — they re-express the current output, and
Phase 0's net (fuzzer + differ + single-fixture verify + CI) proves it.

### 2.1 Sema: scoped symbols + sema-owned verifier locals — ✅ DONE

Landed; see CLAUDE.md §Architecture. Activating block-scoped declarations remains
language-coverage work tracked by README.md §D, not part of this byte-preserving phase.

### 2.2 Backend: `Attribute` abstraction — ✅ DONE

Landed; see CLAUDE.md §"Where byte-identity is won or lost" for the ordered
attribute model and the remaining phase-1 constant-pool ordering boundary.

### 2.3 Backend: single `emit(opcode, operands)` chokepoint — ✅ DONE

Landed; see CLAUDE.md §"Where byte-identity is won or lost". Full symbolic
instructions and metadata remain the target described by ARCHITECTURE.md
§"Symbolic bytecode".

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
- **fuzz: observation-aware minimization.** Behavioral findings are emitted raw
  because byte-only minimization can drift to an observationally-equivalent class.
  Add a predicate that recompiles and re-observes each candidate, then add type-aware
  expression shrinking (same-typed children, casts, and literals toward 0/1) so a
  behavioral finding becomes directly droppable into `fixtures/`.
- **fuzz: strengthen execution isolation with the first JVM-global capability.**
  Before a generator rung can read `System.in`, exit, create threads, or mutate
  process-global state, replace the current in-process class-loader boundary with a
  disposable execution process and a parent-enforced timeout; see CLAUDE.md
  §Testing for the current boundary.
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

Phases 0–1 and Phase 2.1–2.3 landed; Phase 2.4 is next. All tests run through Docker
via the `Makefile`.

As items land, mark them ✅ in place and record the mechanics at the fix site / in
CLAUDE.md — never restate them here, and delete a finished bug's backlog entry (per
CLAUDE.md §"Documentation: one fact, one home").
