# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

njavac is a toy Java 25 → JVM bytecode compiler written in Rust. Its defining
constraint is **byte-identical output to the reference `javac`** (GraalVM CE
`25.0.2-graalce`, class-file major version 69): for a supported program, njavac's
`.class` must equal javac's `.class` byte-for-byte. Everything about the design
follows from that one invariant.

Current scope is the **numeric subset plus the first branch**: one `public class`
with a `static void main`, locals of any of the eight primitives (`int`/`long`/
`float`/`double`/`boolean`/`char`/`byte`/`short`, with the two-slot `long`/`double`
model), the full arithmetic/bitwise/shift/unary operator set (`+ - * / % & | ^ ~
<< >> >>>`), compound assignment and `++`/`--`, primitive casts with binary
numeric promotion, every literal form, `System.out.println` of any primitive or a
string literal, and — the newest rung — **comparisons (`< <= > >= == !=`, `!`) and
`if`/`else if`/`else`**, which brings in the **`StackMapTable`** (frame selection,
the −1 offset-delta bias, dead-branch folding, jump-to-`goto` threading). Still
out of scope (each a future rung): `&& || ?:` and full-frame boolean
materialization (`println(a < b)`), loops and `switch`, string concatenation
(`invokedynamic`), objects/arrays/methods, multiple methods. See `README.md` for
the checked-off feature map and the ordered next rungs.

## Working conventions

**Keep the docs in lockstep with the code, in the *same* commit as the change.**
Where a change is documented depends on what kind it is:

- **README.md** is the **language-coverage** record: update the checked-off
  feature map (§A–§I, `[ ]`→`[x]`) and any conceptual/scope prose whenever a rung
  lands or the supported surface moves. It is the source of truth for "what
  compiles today" and the ordered next rungs.
- **CLAUDE.md** (this file) is **how the compiler works and how we work on it**:
  record architectural specifics and byte-identity gotchas here, and *also* any
  standing instruction the user gives or way of working we agree on — so it
  survives across sessions. If the user tells you to do something a certain way,
  write it down here.
- **ROADMAP.md** is the **infrastructure & architecture evolution** plan: the
  ordered work that makes the codebase ready to take new language rungs cheaply
  and safely (fuzzer/verify/CI tooling, diagnostics, the sema-scoping and
  attribute-abstraction keystones). It is orthogonal to README's *language*-rung
  list. When one of its items lands, check it off there and record the resulting
  mechanics here. Read it when planning infrastructure or a large refactor.
- Apply the same discipline to whatever else a change touches (a new fixture
  subfolder, a CLI flag, an env var, a doc comment that is now wrong): document
  it where a future reader would look for it.

**Commit and push directly to `main`; never branch.** This repo does not use
feature branches — commit straight onto `main` and `git push` to
`origin/main` (`github.com/VitorNathanG/njavac`, a private repo). Do **not**
create a branch, even when starting from the default branch (this overrides the
generic "branch off the default" habit). Once you commit, run `git push` — an
unpushed commit is invisible to the next session and to any backup. (Committing
itself still happens only when the user asks; pushing directly to `main` is the
standing follow-through once a commit exists.)

**Run 100% of tests through Docker; local test runs are disallowed.** Only the
pinned GraalVM `javac` in the image reproduces the golden bytes, so the `Makefile`
is the single sanctioned command surface — `make verify` (fast, cached pinned
goldens) and `make bench` (authoritative online + timing) both build the image and
run inside it; see §Testing. `make check` is a *local* release build for
compiler-internal debugging only, never for acceptance.

**Reflect at the end of each development cycle.** When a cycle wraps up — a rung
landed, a feature shipped, an infra change finished — stop and reflect, then bring
it to the user as a **proposal** (a message they can accept, defer, or drop), not
a silent change. Cover three things:

- **What went well** — the moves worth repeating.
- **What went badly** — where time was lost, where I fumbled, where the code or
  docs fought back, or a wrong/unchecked assumption forced a rework.
- **What would help us do better** — concrete and actionable: a utility script or
  task-runner target that removes a repeated manual step; documentation that was
  missing, wrong, or could be written better; a refactoring worth doing now (or
  that, in hindsight, should have been done *before* touching the code); a new,
  **properly-scoped** Claude Code skill to guide a recurring task; a
  test/fixture/tooling gap to close.

The point is continuous improvement — always look for a better way to do the job
well, and capture the lesson (usually right here in CLAUDE.md, or as a skill/
script) so the next cycle starts ahead of this one. File the concrete
"what would help" items in ROADMAP.md §"Deferred / opportunistic improvements" so
they surface proactively instead of evaporating into the chat log — the correctness
slowness sat there noticed-but-unwritten until it had to be tripped over. Keep this
close to heart.

**Always ask with the question tool; never make the user hand-write an answer.**
When you need a decision, preference, or clarification, present it through the
AskUserQuestion tool with concrete, mutually-exclusive options (your recommended
option first, labelled). Do not pose an open-ended prose question the user has to
type a reply to — they can always pick "Other" to write free-form, but the default
must be a click, not a paragraph. This covers end-of-cycle reflection proposals and
every other fork where their input decides the next step.

**Confirm the environment before you build for it.** Before writing tooling or
infrastructure, check the ground rules that shape its design — where tests run
(here: Docker only), the deployment target, any reproducibility or policy
constraints. Surfacing these up front avoids building the right thing for the wrong
environment (the local test loop that had to be reworked for the Docker-only policy
is the cautionary tale) and the rework that follows.

**No concessions: match javac for every reachable case.** The default is to
reproduce javac's exact bytes for everything a construct can reach — even when that
means reverse-engineering a hidden model (javac's `CondItem` jump-chains, the
`switch` density heuristic, the concat recipe). *"This is bigger than I expected"
is never a reason to scope a case out.* Refuse **only** what genuinely needs a
class-file subsystem this emitter does not yet have (non-empty-stack boolean
materialization → `full_frame`; string concat → `invokedynamic`) — a principled
subset edge, and even then say so and get agreement rather than silently narrowing
the rung. (The `&&`/`||` cycle is the cautionary tale: the first instinct was to
refuse the constant-operand cases as "too big"; the right move was to model javac's
`genCond` exactly — build the ground-truth corpus and reverse-engineer it, per the
`byte-identity-rung` skill §1/§6.)

**Every bug fix lands with a documented regression fixture.** A fix — especially a
fuzzer-found one — is not done until a **committed** `fixtures/` case pins the exact
scenario it repairs, so the fuzzer's finding can never silently come back. The
fixture must be **minimal** (the smallest program that still exercised the bug — do
not paste the raw fuzzer minimization, which keeps whole initializers; hand-reduce
it), **clearly documents** at the top what byte-identity edge it stresses and how it
used to diverge, and lives in the topical subfolder that fits (`folding/`,
`compound-assign/`, …). Work one bug per cycle: reproduce → fix → verify the fix
(`make correctness` green **and** `make fuzz` shows the signature gone) → add the
fixture → commit+push → only then start the next bug. `NanCanon.java` is the pattern
to copy.

## Commands

The `Makefile` is the command surface — run `make help` to list it (`verify`,
`record`, `bench`, `fuzz`, `fuzz-selftest`, `probe`, `diff`, `image`, `check`).
Building and running the compiler itself:

```bash
make check                                  # local release build (lib + njavac/bench/classdiff/profile/fuzz bins)
./target/release/njavac [-d <dir>] <file.java> [<file.java> ...]   # the njavac CLI, javac-like
```

The CLI mirrors javac's surface: any number of `.java` sources in a single
invocation, each class written to `<Name>.class` under `-d <dir>` (or beside its
source if `-d` is omitted). One source failing does not abort the rest; the
process exits non-zero if any did. `make check` is a *local* build for
compiler-internal debugging; byte-identity is only ever validated through Docker
(see §Testing).

The reference toolchain is the pinned GraalVM CE `25.0.2-graalce` `javac`/`javap`
baked into the image; inspect its output for any program with
`make probe FILE=Probe.java`. Byte-identity is specific to that exact JDK build —
a different `javac` version can legitimately produce different golden bytes.

## Testing = the benchmark (there is no `cargo test`)

The `bench` bin is the fixture-based test suite **and** the benchmark — the
acceptance gate. The differential **fuzzer** (`make fuzz`; §Dev-loop tooling 0.1
below) complements it by generating random in-scope programs to surface
divergences no hand-written fixture covers. `bench` has two passes over
`fixtures/*.java`:

1. **Correctness (always).** Compiles every fixture with both `javac` and njavac
   and byte-compares: **exits non-zero on any mismatch**, printing a structural
   `classdiff` (byte-offset precise) then a noise-stripped `javap -v` divergence
   (the `Classfile`/`Last modified`/`SHA-256` header lines are filtered out) to
   localize the first failure.
2. **Timing (deterministic harness only).** Times compiling the whole suite with
   each compiler. Host timings are noise (JVM-startup jitter, scheduler, thermal),
   so timing runs only inside the Docker harness.

**All tests run through Docker; local runs are disallowed.** Byte-identity is only
*reproducible* against the exact pinned `javac` (GraalVM CE 25.0.2-graalce, major
69) baked into the image — a host with any other `javac` build can legitimately
emit different golden bytes, so a green *local* run proves nothing. The `Makefile`
is the command surface (**`make help`** lists it, so it stays the single source of
truth); its gates all build and run `bench` inside the pinned image:

- **`make verify`** — fast (~1s): njavac vs goldens the *pinned* javac recorded into
  a persisted Docker volume. The everyday inner loop; the cache can go stale, so
  re-record with `make record` after changing fixtures or the JDK.
- **`make correctness`** — fresh & authoritative (~3s): full online check against
  freshly-invoked pinned javac, no timing. The pre-commit gate.
- **`make bench`** — authoritative + deterministic timing (~15s). Add `FILE=<f>` to
  any gate to check a single fixture.

`make check` builds the binaries locally for compiler-internal debugging; running
them directly (or `NJAVAC_BENCH_ALLOW_HOST=1 bench` to force host timing) is for
debugging only and is **not** a sanctioned way to validate byte-identity.

**Dev-loop tooling** (ROADMAP.md §Phase 0; CI gate 0.4 deferred), all invoked
*through Docker* per the policy above:

- **Differential fuzzer (0.1).** `make fuzz [SEED=n COUNT=n BATCH=n]` generates
  random *in-scope* Java (`src/bin/fuzz.rs`), compiles each with the pinned javac and
  njavac (in-process), and byte-compares. `make fuzz-selftest` exercises the
  finding→minimize→report machinery. Findings land in `fuzz-out/` (git-ignored) as a
  minimized `.java` + a `.diff`. Key facts a future rung's generator MUST preserve:
  - **The oracle contract — one sentence.** *Both compilers accept and the bytes
    differ* is the ONLY hard-fail signal (it is, by definition, an njavac bug);
    a javac reject is `generator-invalid` telemetry, an njavac panic is
    `njavac-reject` telemetry (not a finding until Phase 1's taxonomy). Corollary:
    the generator's in-subset discipline is a **yield** lever (keeps javac accepting),
    never a soundness lever — generator over-reach can't manufacture a false finding.
  - **Three invariants that keep it sound.** (1) the `ident()` chokepoint: class name
    == filename == `source_file` arg (the `.class` couples to all three via the class
    name, `SourceFile`, and `LineNumberTable`), reused by generation, the batch
    writer, the in-process `compile()`, AND every minimizer candidate; (2) `reset_dir`
    per batch + an exact-file-set assertion (no stale `.class`, no `$`-aux class a
    future concat/switch generator might over-reach into); (3) generate-all-IR-before-
    any-IO, so a transient hiccup changes tallies but never the seed-determined program
    sequence.
  - **The generator scope boundary.** Declarations only at method-body top level (sema
    allocates slots for top-level decls only). A *branch-boolean* (`< <= > >= == != &&
    || !`) may only be materialized on an empty base stack — an `if` cond or a boolean
    decl/assign RHS; a *value-boolean* (literal, local, `&|^`) is used everywhere else.
    This is the `BoolMode`/`ScopeCaps` split; getting it wrong only lowers yield.
  - **Performance.** njavac runs in-process; ONE `javac -d <dir> @argfile` per batch
    (default 1000) amortizes JVM startup — `@argfile` is required (a big argv blows
    `ARG_MAX`), and scratch lives on the normal FS (`/dev/shm` is only 64 MB). `--jobs`
    is deferred (asserts `==1`). `--keep-going` enumerates distinct finding signatures
    (normalized structural divergence paths); `--no-min` skips minimization for a fast
    census; `--dump-sources` prints generated sources (no compile) for a determinism
    check. **A new rung grows the fuzzer by a 5-touch list** (add an `FExpr`/`FStmt`
    variant, a gen arm, a render arm, a minimize pass, a `ScopeCaps` flag) — run
    `make fuzz` as part of landing it. The current fuzzer-found bug backlog (a
    constant-folding NaN-canonicalization bug and a mixed-type folding gap) lives in
    ROADMAP.md §"Fuzzer-found bug backlog".

- **Single-fixture verify (0.2).** `make verify FILE=<File.java>` (fast, cached
  goldens) or `make bench FILE=<File.java>` (online) compiles just that fixture
  inside the container, byte-compares, prints the localized diff on mismatch, and
  skips timing. This is the edit→verify inner loop — not a hand-run
  `javac && njavac && cmp`.
- **Structured differ (0.3).** On any mismatch the bench prints a **classdiff** —
  the first *structurally*-divergent field with its byte offset and readable
  context (`methods[0].attr[0].Code.max_stack`, `cp[17].bytes`) — *before* the
  javap diff. It localizes to the cause and works even when javap output matches
  ("bytes differ, javap agrees"). The same engine (`njavac::classdump`) backs the
  `classdiff` bin, baked into the image; diff two class files with
  `make diff A=a.class B=b.class`.
- **Fast offline gate (0.5).** `make verify` records goldens from the **pinned**
  javac *inside* the image (one batch javac invocation) and persists them to a
  Docker volume (`njavac-goldens`), then byte-compares njavac against that cache
  with **no javac spawns** — ~1.3s for the whole suite vs ~30s online, entirely in
  Docker. It auto-records when the volume is empty; **re-record with `make record`
  after changing fixtures or the JDK**, or the cache goes stale. `make bench` stays
  the authoritative from-scratch check. Under the hood these are `bench --record` /
  `bench --offline --golden-dir <dir>`; the cache is never committed and never
  hand-edited. (A locally-recorded cache would
  be untrusted — the goldens must come from the pinned in-image javac, which is
  exactly what the volume holds.)

Key points, several of which are non-obvious:

- **javac is the live reference.** There are no committed golden `.class` files;
  the bench compiles with the real `javac` each run, which also self-validates
  the environment. Do not reintroduce checked-in goldens.
- **Run counts are per-compiler and asymmetric**: njavac is timed 1000×, javac
  5× (`--njavac-runs` / `--javac-runs`), because javac pays ~700 ms of JVM
  startup per run. Both timing runs are a **single invocation over the whole
  suite** — one javac process vs one njavac process — so the numbers are a fair
  apples-to-apples wall-clock (process startup + compiling every file), not
  njavac's old spawn-per-file model.
- **Adding a test = drop a `.java` under `fixtures/`.** Fixtures are grouped into
  **topical subfolders** (`basics/`, `literals/`, `operators/`, `conversions/`,
  `compound-assign/`, `folding/`, `types/`, `println/`, `branches/`); the bench and profiler
  discover `*.java` **recursively**, so any depth works. A file's directory does
  **not** affect its bytes — the `SourceFile` attribute is the bare basename
  (`main.rs` uses `file_name()`), so moving a fixture between folders is
  byte-neutral. The filename must match the `public class` name (so both
  compilers emit `<Name>.class`), and basenames must stay **globally unique**
  (the output `.class` dir is flat). Aim new fixtures at byte-identity edge cases
  (constant-load opcode boundaries, slot allocation, LineNumberTable, folding).
  Note: once `package`/`import`/multi-type land, a fixture will need to become a
  **directory of `.java` files compiled together** (output nested by package);
  the recursive discovery already walks the tree, but the per-fixture compile
  step (one `javac`/`njavac` call, compared by basename) will need to grow into a
  compile-the-whole-case-dir + compare-every-emitted-`.class` shape.
- **Iterating on one case** is a first-class command now — `make verify
  FILE=<File.java>` (fast) or `make bench FILE=<File.java>` (online) (see "Dev-loop
  tooling" above), which prints the structural classdiff + javap diff on mismatch.
  No more hand-run `javac && njavac && cmp`.
- Env/flags: the underlying `bench` binary takes `JAVAC`/`JAVAP` (or
  `--javac`/`--javap`) tool-path overrides (the Docker image sets them) and
  `--fixtures`, `--warmup`, `--out-dir`, `--record`, `--offline`, `--golden-dir`;
  `make bench` honors `BENCH_CPU` (default core 2) and `BENCH_MEM` (2g).

### Profiling (`profile` bin)

The bench measures wall-clock of *process spawns*; for these tiny inputs that is
almost entirely OS process creation, not compilation. To profile the compiler
itself, `profile` calls `compile()` in-process in a hot loop and reports a
per-phase breakdown (lex / parse / sema / codegen+emit).

```bash
./target/release/profile [rounds] [trials]   # defaults: 30000 rounds, 5 trials
```

It reports the **min over trials** — the robust estimator, since host noise can
only ever *add* time. Single-shot host timing lies; always compare mins.

## Architecture

The pipeline lives in `src/lib.rs::compile(source, source_file) -> Vec<u8>`:

```
source → lexer::lex → parser::parse → sema::analyze → codegen::generate → .class bytes
```

- **`lexer`** → flat `Vec<Token>`, each carrying a 1-based source line (needed
  for a byte-identical `LineNumberTable`).
- **`ast`** → plain enums, `Box` for recursion; statements/braces carry lines.
- **`parser`** → recursive descent; precedence unary → `* / %` → `+ -`.
- **`sema`** → local-slot allocation (two-slot `long`/`double` model), per-local
  typing, and `type_of` implementing unary/binary numeric promotion (enough to
  drive descriptor, conversion-opcode, and constant-load selection).
- **`codegen`** → typed bytecode + `max_stack`/`max_locals` + `LineNumberTable`,
  via the `classfile` backend.
- **`main`** is a thin javac-like CLI (`njavac [-d <dir>] <file.java> …`): it
  compiles each source in one invocation, deriving the output `<Name>.class` and
  the `SourceFile` attribute from the input file's basename (the class name comes
  from the source). A per-file compile error is caught so one bad source does not
  abort the batch — the process just exits non-zero.
- **`classdump`** is not part of the pipeline — it is the *inverse*: a structural
  reader that parses `.class` bytes back into an ordered list of fields (byte
  offset + path + value), and a `diff_report` that localizes the first structural
  divergence between two class files. It is the mirror of the `classfile` writer
  and the byte-identity debugging tool (the `classdiff` bin, and the diff the
  bench prints on a mismatch). See §Testing.

### Where byte-identity is won or lost

**`src/classfile.rs` (the constant pool) is the linchpin.** javac emits pool
entries in a specific order, and reproducing it exactly is what makes bytes
match. Two rules encoded here:

- **Two-phase interning.** During code generation, every bytecode operand is
  interned in the exact order the bytecode references it (phase 1); then
  `ClassFile::to_bytes` interns the structural names — `this_class`, per-method
  name/descriptor/attribute names, `SourceFile` — in writing order (phase 2).
- **Breadth-first composite interning.** A `Methodref` takes its own slot, then
  its `Class` and `NameAndType`, then *their* `Utf8` children (a FIFO queue per
  top-level intern). This BFS order is why the pool matches javac.

If you touch the constant pool, **preserve entry insertion order** — it is the
only thing the class file depends on. `Long`/`Double` entries each **consume two
pool indices** (the pool tracks an explicit `next_index`, so the second slot is a
phantom and `constant_pool_count` includes it); `Float`/`Double` are keyed by
their **bit pattern** *after NaN canonicalization* — `ConstantPool::float`/`double`
collapse **every** NaN to javac's canonical `0x7fc00000` / `0x7ff8000000000000`
(what `Float.floatToIntBits` / `Double.doubleToLongBits` write) before interning,
so all NaN dedup to one entry while `-0.0` (not a NaN) stays distinct from `+0.0`.
This is load-bearing: a folded `-(0.0f/0.0f)` carries a sign-flipped NaN
(`0xffc00000`) that only matches javac once canonicalized. The dedup map uses a
custom FxHash purely for speed; the hash never
affects output, and serialization is deliberately allocation-free (child indices
resolved through borrowed lookup tables, not cloned `Entry` keys). Always re-run
the bench's correctness pass after changes.

The **`StackMapTable`** also lives here. Each method carries its frames as full
verifier-state snapshots (`entry_locals` + `StackFrame { offset, locals, stack }`);
`stack_map_body` derives each frame's `offset_delta` (first = its offset, then
`offset − prev − 1` — the −1 inter-frame bias) and picks the **smallest** frame
form (`same`/`same_locals_1_stack_item`(+`_extended`)/`append`/`chop`/`full_frame`)
via `classify_frame`. The pool ordering rules extend to it: the `"StackMapTable"`
Utf8 is interned per-method right after `LineNumberTable`, **only when the method
has frames** (a method whose branches all fold stays byte-identical to its
straight-line form); a `full_frame`'s `Object` locals (here just `args`'s
`[Ljava/lang/String;`) are interned right after that Utf8. Within `Code`, the
sub-attributes are written **`LineNumberTable` then `StackMapTable`**.

**`src/codegen.rs`** mirrors javac's exact choices with a fully typed emitter:
the per-type constant-load ladders (`iconst`/`bipush`/`sipush`/`ldc` by
magnitude; `lconst`/`ldc2_w`; `fconst`/`ldc`; `dconst`/`ldc2_w`, floats compared
by *bit pattern* so `-0.0` pools separately); per-type load/store families with
the slot-0..3 short forms; binary numeric promotion that places each `i2l`/`i2d`/…
conversion exactly where javac does (left operand widened before the right is
pushed, right operand just before the op); the `iinc`/`iinc_w`/full-form boundary
for compound assignment (decided on the *effective* delta); `~` lowered to
`… ixor`; a running operand-stack model that counts category-2 values as two
words; the trailing `return` mapped to the closing-brace line. The load-bearing
rule: javac **constant-folds literal subtrees** (`100 % 7` → `iconst_2`,
`1 + 2L` → `ldc2_w 3L`) with wrapping integer / exact IEEE-754 arithmetic and JLS
shift masking, but emits real bytecode once a local is involved — so a folded
constant is bit-identical to the unfolded computation.

Comparisons, `if`/`else`, and short-circuit `&&`/`||` share a second lowering
mode built around **`gen_cond(&Expr) -> CondItem`** — a faithful port of javac's
`Gen.genCond` + `Items.CondItem` + `Code.mergeChains`, restricted to this
side-effect-free boolean subset. A `CondItem` is `{ opcode: CondOp, true_chain,
false_chain, value_on_stack }`: `gen_cond` emits every operand load eagerly but
leaves only the **deciding branch** pending (`CondOp::Test(op)` = the true-polarity
branch; `Goto`/`DontGoto` = a static verdict), collecting the not-yet-resolved
jump sites in the two chains. A **chain is an `Option<usize>` label id**; `None` is
the empty chain (javac's null — nothing targets it, so it places no frame), and
`merge_chains` = `Code.mergeChains` (retarget every fixup of one label to another —
fixup order never affects output). `jump_false`/`jump_true` materialize a
`CondItem`'s branch to a chain and are **total**: they check `is_true()`/`is_false()`
first (a static verdict emits nothing) then emit per `CondOp`. `!e` is
`gen_cond(e).negate()` (swap chains, `negate_op` the opcode). `&&`/`||` short-circuit
**from the left**: the left's deciding branch is emitted, its non-deciding outcome
resolves (falls through) into the right, and the chains merge — this is the *only*
representation that reproduces javac's constant-operand cases (`true || q` drops
the dead right operand, `q && false` keeps the residual `iload q` then forces
`iconst_0`). The linchpin for that is **`fold`'s short-circuit-aware `Logical`
arm**: it folds only when the *left* decides or the whole tree is constant, so a
live left with a constant right returns `None` (must still be emitted), never a
whole-constant collapse.

The three consumers: **`gen_if`** is a faithful `visitIf` port — a whole-constant
condition (`fold_bool`) drops to the taken arm; otherwise `is_false` skips only the
*then* (the else still runs), and the trailing `goto`+else is emitted only when the
else target is reachable (no spurious `goto`, no dead else). **`gen_bool_value`**
materializes to 0/1 in one of three shapes: a bare value already on the stack
(`value_on_stack`, no diamond), a statically-decided item with a residual branch
(resolve it, then `iconst_0`/`iconst_1`), or the general true-first
`iconst_1`/`goto`/`iconst_0` diamond — still asserting an **empty base stack**, so
`println(a && b)`/`println(a < b)` (non-empty stack → `full_frame`) stay a refused
later rung. Loops/`?:` will reuse `gen_cond` verbatim.

The physical machinery is unchanged and reused: forward branches use the
label/fixup table backpatched in `resolve_branches` (which **threads jumps through
unconditional `goto`s**), and `build_frames` emits a frame only at pcs that survive
as real jump targets — `gen_cond` only decides *at which pcs* `resolve_chain` calls
`add_frame`. The running-locals snapshot (`Gen::locals`) grows as method-body
locals are declared and is what each frame captures — the push stays *after*
`gen_stmt` so a frame inside a declaration's own initializer (`boolean r = a && b`)
snapshots locals *without* the new local, matching javac. Branch bodies declare no
locals in this subset, so the snapshot only ever grows (no `chop`). `negate_op` (the
12-opcode branch involution) is debug-asserted against `int_icmp_branch`/
`int_zero_branch` in `assert_negate_op_consistent` since a drift there would
silently break every comparison.

## Determinism / Docker

`Dockerfile` installs the *same* `25.0.2-graalce` (via SDKMAN) so the container
reproduces the golden bytes; the JDK is the base layer and cargo/SDKMAN use
BuildKit cache mounts. Timing repeatability comes from the `docker run` flags in
the `Makefile`'s `bench` target (pinned single core, fixed memory, no swap), not
the image.
