# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

njavac is a toy Java 25 → JVM bytecode compiler written in Rust. Its defining
constraint is **byte-identical output to the reference `javac`** (GraalVM CE
`25.0.2-graalce`, class-file major version 69): for a supported program, njavac's
`.class` must equal javac's `.class` byte-for-byte. Everything about the design
follows from that one invariant.

Current scope is a **numeric subset with early control flow** — one `public class`
with a `static void main`, the eight primitives (two-slot `long`/`double`), the full
arithmetic/bitwise/shift/unary operator set with binary numeric promotion and
constant folding, compound assignment and `++`/`--`, every literal form,
`System.out.println` of a primitive or string literal, comparisons + `if`/`else`
(which carries the **`StackMapTable`**), and short-circuit `&&`/`||`. **README.md owns
the authoritative per-rung coverage map (§A–§I) and the ordered next rungs — track
feature-level scope there, not here.** This headline stays deliberately coarse so it
can't drift the way a hand-maintained feature list does (it once still listed
`&&`/`||` as out of scope a whole rung after it landed).

## Working conventions

**Documentation: one fact, one home — link, don't copy.** Keep the docs in lockstep
with the code, in the *same* commit as the change. There are five homes, each with a
charter and a boundary; when a doc needs a fact that lives in another, it **points to
it by section name** instead of restating it. The failure this rule exists to kill is
a fact written into two files' prose, where one copy later rots (the `&&`/`||` scope
line sat "out of scope" in one doc a whole rung after it shipped, described in full,
in another) — if a change makes you write the same prose twice, collapse one copy to
a pointer.

- **README.md — the language-coverage map.** *What compiles today* and *the ordered
  next language rungs*: the §A–§I checkbox map, the requirement tags, the scope
  prose, and the forward-looking byte-identity gotchas for features not built yet.
  The source of truth for "is X supported?". **Update when** a rung lands or the
  supported surface moves. **Not here:** how a feature is implemented (→ CLAUDE.md
  §Architecture), tooling/infra plans (→ ROADMAP), how-we-work rules (→ CLAUDE.md).
- **CLAUDE.md (this file) — how the compiler works and how we work.** Two charters:
  (a) **mechanics** — the byte-identity implementation *as it exists now* (the
  constant pool, `StackMapTable`, codegen lowering, the pipeline); (b) **conventions**
  — standing instructions and agreed ways of working, this taxonomy included.
  **Update when** the implementation of a built feature changes, or the user gives a
  standing instruction (if the user tells you to do something a certain way, write it
  down here). **Not here:** the coverage checklist (→ README — do not re-enumerate
  the supported surface; it drifts), infra work not yet built (→ ROADMAP), a lone
  decision's fine detail that belongs in a code doc-comment at its function.
- **ARCHITECTURE.md — the target structure.** The intended long-term layer/module
  boundaries, dependency rules, core contracts, byte-identity invariants, and the
  concrete triggers for creating those modules. **Update when** the agreed
  destination architecture changes. **Not here:** current mechanics (→ CLAUDE.md),
  ordered implementation work or bugs (→ ROADMAP), language order (→ README).
- **ROADMAP.md — the active infrastructure evolution plan.** *Open, ordered
  infra/refactor work* and the *open bug backlog* — a to-do list, **not a changelog**.
  **When an item is done, delete its entry** (shrink a landed infra phase to a
  one-line ✅ + pointer at most); never accumulate "✅ FIXED — here is the full story"
  writeups. The record of finished work is the code + its doc-comment at the fix site,
  the regression fixture, and the git commit — not a ROADMAP entry. **Update when**
  work is planned, triaged, or **completed (by removing it)**. **Not here:**
  language-rung order (→ README), the mechanics of anything landed (→ CLAUDE.md or the
  code), how-we-work rules (→ CLAUDE.md).
- **Code doc-comments & `make help` — the finest grain.** A specific javac-matching
  decision's rationale lives in the doc-comment on its function; the prose docs
  *reference the function*, they don't re-derive it. The command/flag list's source
  of truth is `make help`; docs describe a command's *purpose*, not a flag catalog.

Apply the same "one home + a pointer" discipline to anything else a change touches (a
new fixture subfolder, a CLI flag, an env var, a doc comment now wrong): write it
once, where a future reader looks first.

**Tidy first, then change behavior.** When a feature needs a structural refactor,
follow Beck's tidy-first discipline: land the smallest behavior-preserving cleanup
under the existing gates before implementing the feature, and keep the two changes
in separate commits. Do not mix module movement, renaming, or abstraction work into
the behavioral change that motivated it.

**Stop at model contradictions.** Before extending an uncommitted diff, inspect
`git status`, its total diff, and the last green commit. If a supposedly
byte-preserving change produces a broad divergence census, or a new probe disproves
the current model, stop immediately: return to the last verified boundary and
redesign from a complete corpus. Do not stack local fixes onto a disproven model.
Keep one behavior hypothesis and one independently committable change in flight;
the branch-local spike that reached 2,095 divergences is the cautionary tale.

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
means reverse-engineering a hidden behavior model (boolean jump chains, the
`switch` density heuristic, the concat recipe). *"This is bigger than I expected"
is never a reason to scope a case out.* Refuse **only** what genuinely needs a
class-file subsystem this emitter does not yet have (non-empty-stack boolean
materialization → `full_frame`; string concat → `invokedynamic`) — a principled
subset edge, and even then say so and get agreement rather than silently narrowing
the rung. (The `&&`/`||` cycle is the cautionary tale: the first instinct was to
refuse the constant-operand cases as "too big"; the right move was to model javac's
observable jump-chain behavior exactly — build the ground-truth corpus and
reverse-engineer it, per the
`byte-identity-rung` skill §1/§6.)

**The reference compiler is a black box.** Derive implementation rules only from
the pinned javac's observable outputs through repository probes, diffs, fixtures,
and fuzzing. Do not inspect, copy, decompile, or base a design on javac/OpenJDK
source code or internal implementation details. Names in existing njavac comments
that compare a local abstraction to javac are descriptions of already-inferred
behavior, not permission to use javac internals as an implementation authority.
Build a complete probe corpus, infer the smallest model that explains it, and test
that model's predictions before changing code.

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
to copy. **Document the fix where it lives** — a doc-comment on the changed function
carrying the reverse-engineered javac rule — and when the bug is fixed and committed,
**delete its backlog entry** instead of annotating it "✅ done". The lasting record is
that code comment + the fixture + the commit; the backlog stays a list of what is
still *open*.

## Commands

The `Makefile` is the command surface — run `make help` to list it (`verify`,
`record`, `bench`, `fuzz`, `fuzz-verify`, `fuzz-selftest`, `probe`, `src-diff`,
`diff`, `image`, `check`).
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
`make probe FILE=Probe.java`, or diff **both** compilers on an arbitrary source
(byte-compare + classdiff + `javap -c` diff) with `make src-diff FILE=Probe.java` —
the triage inner loop for a program that is not a fixture. Byte-identity is specific
to that exact JDK build — a different `javac` version can legitimately produce
different golden bytes.

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
  random *in-scope* Java (`src/bin/fuzz/`), compiles each with njavac (in-process)
  and the pinned javac (via a **persistent in-memory worker** — see Performance
  below), then applies a two-layer oracle: exact byte comparison first, and a
  persistent execution observer only for byte-divergent classes. **A bare `make
  fuzz` uses a fresh random
  seed each run** (explores new programs every time) and prints it, so any finding
  reproduces with `make fuzz SEED=<n>`; pass `SEED=n` to pin it. `make fuzz-verify`
  proves the compile worker matches the javac CLI; `make fuzz-observe-verify`
  exercises observer return, output difference, exception, invalid-class, timeout,
  and restart paths. Behavioral findings land in `fuzz-out/` (git-ignored) as raw
  `.java`, `.diff`, and `.observe` files; they stay raw until minimization has an
  observation-aware predicate. Key facts a future rung's generator MUST preserve:
  - **The oracle contract — one sentence.** *Both compilers accept, their bytes
    differ, and their observations differ* is the behavioral hard-fail signal;
    byte-only divergences are compatibility telemetry, a javac reject is
    `generator-invalid` telemetry regardless of the njavac outcome, a returned
    `Unsupported` diagnostic is `njavac-unsupported` telemetry, and a returned
    syntax diagnostic or internal panic after javac accepts is a hard compiler
    finding. Observation compares stdout, stderr, and normalized
    return/throw/load-failure/timeout state. This is empirical semantic evidence,
    not a proof: wrong unobserved state can remain a false negative.
  - **Harness invariants.** (1) the `ident()` chokepoint: class name
    == filename == `source_file` arg (the `.class` couples to all three via the class
    name, `SourceFile`, and `LineNumberTable`), reused by generation, the worker
    request, and the in-process `compile()`; (2)
    `assert_batch_classes` — the worker returns exactly the batch's class set, so a
    stale or `$`-aux class (a future concat/switch generator over-reaching) is a hard
    error; (3) generate-all-IR-before-any-compile, so a transient hiccup changes
    tallies but never the seed-determined program sequence; (4) instrumentation adds
    no RNG calls, so removing trace statements recovers the same seed-determined IR.
  - **The generator scope boundary.** Declarations only at method-body top level (sema
    allocates slots for top-level decls only). A *branch-boolean* (`< <= > >= == != &&
    || !`, including boolean cast/grouping boundaries) may only be materialized on an
    empty base stack — an `if` cond or a boolean decl/assign RHS; a *value-boolean*
    (literal, local, `&|^`) is used everywhere else. This is the
    `BoolMode`/`ScopeCaps` split; getting it wrong only lowers yield. The renderer is
    precedence-aware: only `FExpr::Paren` emits deliberate grouping, and the
    self-test minimizer can remove one grouping boundary at a time. Every generated
    mutation is followed by a local-value print, every branch starts with a
    `then`/`else` marker, and each `if` is followed by a snapshot of visible locals;
    complex branch booleans are never printed directly because that would require
    the unsupported non-empty-stack materialization path.
  - **Performance.** Both compilers now run without a process spawn and without
    touching disk. njavac is in-process; the pinned javac runs in a **persistent
    in-memory worker** (`tools/FuzzJavac.java`, driven by `JavacWorker` in
    `src/bin/fuzz/javac.rs`) — ONE hot JVM for the whole run, sources handed over a pipe and
    `.class` bytes captured in memory (no source files, no class files, no dir
    scans). A whole batch (default 1000) compiles in one worker `getTask`, which
    amortizes javac's compiler `Context` exactly as the old `@argfile` batch did —
    a *fresh* `Context` per program was far slower than the CLI it replaced. This
    cut a 6000-case run ~12s → ~5s (a 200k run does ~5000 programs/s, fully
    in-memory). The summary prints per-compiler **compile time** (javac vs njavac)
    and total **lines compiled**. Byte-identity of the worker to the `javac` CLI is
    an *empirical* invariant (it rests on the `JavaCompiler` API defaulting to the
    CLI's options and the in-memory `JavaFileManager` reproducing the file-derived
    bytes), so **`make fuzz-verify` is the gate** — it compiles N programs through
    the worker AND a real CLI spawn and byte-compares; the CLI stays authoritative,
    so **run it after any JDK bump or worker edit** (a divergence means the worker
    is invalid). Byte divergences lazily start a second persistent JVM
    (`tools/FuzzObserve.java`, driven by `ObserveWorker` in
    `src/bin/fuzz/observe.rs`), which loads each side through a fresh class loader,
    invokes `main`, captures bounded output, and restarts after a timeout. This
    in-process isolation is valid for the **current generator**, which cannot access
    `System.in`, mutate JVM-global state, call `System.exit`, or create threads; the
    first rung that can do any of those must replace or strengthen the execution
    boundary before enabling that generator capability. `make fuzz-observe-verify`
    is the worker gate. `--jobs` is deferred (asserts `==1`). `--keep-going`
    enumerates structural byte signatures and observation-field signatures;
    `--dump-sources` prints generated sources (no compile) for a determinism check.
    **A new rung grows the fuzzer by a 5-touch
    list** (add an `FExpr`/`FStmt` variant, a gen arm, a render arm, a minimize
    pass, a `ScopeCaps` flag) — run `make fuzz` as part of landing it, and
    `make fuzz-verify` if the rung reaches new class-file territory; run
    `make fuzz-observe-verify` after observer edits. The open
    fuzzer-found bug backlog lives in ROADMAP.md §"Fuzzer-found bug backlog" (kept
    there and not re-characterized here, so this pointer can't go stale).

- **Single-fixture verify (0.2).** `make verify FILE=<File.java>` (fast, cached
  goldens) or `make bench FILE=<File.java>` (online) compiles just that fixture
  inside the container, byte-compares, prints the localized diff on mismatch, and
  skips timing. This is the edit→verify inner loop — not a hand-run
  `javac && njavac && cmp`.
- **Structured differ (0.3).** On any mismatch the bench prints a **classdiff** —
  the first *substantive* structurally-divergent field with its byte offset and
  readable context (`methods[0].attr[0].Code.max_stack`, `cp[17].bytes`) — *before*
  the javap diff. A *derived* field (a count/byte-length like `constant_pool_count`
  or `attr[..].length` that differs only as a *consequence* of the content it
  measures) is demoted to a one-line note so the headline is the cause, not the
  symptom (`is_derived` in `classdump.rs`); the report still leads with the
  first-differing-byte offset, so the ordered ground truth is never overruled. This
  is also what makes the fuzzer's `--keep-going` census cluster by cause. It works
  even when javap output matches ("bytes differ, javap agrees"). The same engine
  (`njavac::classdump`) backs the `classdiff` bin, baked into the image; diff two
  class files with `make diff A=a.class B=b.class`.
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
itself, `profile` calls the pipeline in-process in a hot loop and reports a
per-phase breakdown (lex / parse / sema / codegen plan / classfile serialization).

```bash
make profile [ROUNDS=n] [TRIALS=n] [PHASE=all|lex|parse|sema|codegen|full]
```

Each cumulative phase/trial reports progress in ten measured chunks; progress I/O
is outside the accumulated duration. Defaults are 1000 rounds, 5 trials, and all
phases; select one phase for a native sampling profile without first running the
other cumulative prefixes. The final breakdown reports the **min over trials** —
the robust estimator, since host noise can only ever *add* time. Single-shot host
timing lies; always compare mins on the same host and corpus.

## Architecture

The pipeline lives in
`src/lib.rs::compile(source, source_file) -> CompileResult<Vec<u8>>`:

```
source → lexer::lex → parser::parse → sema::analyze → codegen::generate → .class bytes
```

- **`diagnostic` / `span`** → the fallible stage contract: half-open source-byte
  ranges plus stable syntax/unsupported diagnostic codes; internal invariant
  failures remain Rust panics rather than a diagnostic category.
- **`lexer`** → flat `Vec<Token>`, each carrying a half-open source-byte `Span`
  plus the existing 1-based line used for a byte-identical `LineNumberTable`.
- **`ast`** → plain enums, `Box` for recursion; declarations/statements carry
  source spans while statements/braces retain their byte-visible line facts.
- **`parser`** → recursive descent; precedence unary → `* / %` → `+ -`.
- **`sema`** → supported-class-shape validation, operand-family checks, and
  occurrence-based local resolution: each declaration gets a stable `LocalId`,
  every `Name` span maps to it, and definite assignment is tracked by ID. Its
  lexical scope stack reclaims two-slot-aware allocations on braced-scope exit
  while retaining the `max_locals` high-water mark; unbraced branch bodies share
  their enclosing scope. From definite-assigned `LocalId`s and physical slots it
  records method-entry plus per-statement entry/exit verifier-local snapshots;
  interior unassigned holes are `Top`, category-2 locals occupy one verifier entry
  but two slots, and trailing `Top` is trimmed. Snapshots are shared immutable
  slices and rebuilt only when definite assignment changes, so statements whose
  verifier state is unchanged reuse one snapshot. Branch-local declarations remain
  an explicit unsupported boundary. `type_of` implements unary/binary numeric
  promotion from the resolved records.
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
  `ClassFile::to_bytes` interns `this_class`/`super_class`, per-method
  names/descriptors, and the structural constants reached by recursively walking
  the same ordered attribute vectors that writing uses (phase 2). Each attribute
  name precedes its body constants and children.
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
(`0xffc00000`) that only matches javac once canonicalized. Text is deduplicated
once into pool-local `TextId`s; `Entry` keys and their child references carry
those integer identities, so composite lookup never re-hashes string contents.
Text-ID order has no byte-level meaning: only the separate ordered `entries`
vector determines pool insertion and serialization order. Both maps use custom
FxHash purely for speed; hashing never affects output, and serialization resolves
child entries directly through the frozen integer-keyed intern map. Always re-run
the bench's correctness pass after changes.

The class-file model uses the owned `Attribute` enum for `Code`,
`LineNumberTable`, `StackMapTable`, and `SourceFile`. Methods and classes hold
ordered attribute vectors; `CodeAttribute` holds its ordered child vector. Shared
recursive `intern_attributes` and `write_attributes` traversals make those vectors
the sole phase-2 interning and writing order, derive counts from vector lengths,
and measure every body in a temporary buffer.

The **`StackMapTable`** also lives here. Its attribute carries frames as full
verifier-state snapshots (`entry_locals` + `StackFrame { offset, locals, stack }`);
`write_stack_map_body` derives each frame's `offset_delta` (first = its offset, then
`offset − prev − 1` — the −1 inter-frame bias) and picks the **smallest** frame
form (`same`/`same_locals_1_stack_item`(+`_extended`)/`append`/`chop`/`full_frame`)
via `classify_frame`. The pool ordering rules extend to it: the `"StackMapTable"`
Utf8 is interned per-method right after `LineNumberTable`, **only when the method
has frames** (a method whose branches all fold stays byte-identical to its
straight-line form); a `full_frame`'s `Object` locals (here just `args`'s
`[Ljava/lang/String;`) are interned right after that Utf8. Within `Code`, the
constructor orders children as **`LineNumberTable` then `StackMapTable`**.

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

Before interning or emission, codegen preflights value evaluation for the one
valid-Java shape its current frames cannot represent: branch-boolean materialization
while another operand-stack value is live (`println(a < b)`, or a branch-valued
right operand of boolean `&`/`|`/`^`). It returns `NJC1001`; the corresponding
empty-stack assert in `gen_bool_value` remains an internal post-validation invariant.

Comparisons, `if`/`else`, and short-circuit `&&`/`||` share a second lowering mode
built around **`gen_cond(&Expr) -> CondItem`**. Besides the deciding `CondOp` and
true/false chains, each item explicitly carries `stack_reuse`, `CondOrigin`,
`Materialization`, and `CodeFreePosition`. These are independent facts: the final
value may be an ordinary reusable stack value while a grouped evaluated prefix or
crossed control-flow join still requires javac's materialization diamond; a
code-free false verdict may independently preserve an `if` source position.

`lowering_const` is deliberately stricter than the general short-circuit-aware
`fold`: a logical tree is an immediate only when its **complete** subtree is
available, including javac's observed `long >>> long` non-folding exception.
Otherwise `gen_cond` walks it structurally. A deciding `true || q`/`false && q`
becomes `Shortcut`; `!` turns that into `NegatedShortcut` (and repeated `!` keeps
that origin). The parser preserves grouping as `Expr::Paren`; grouping is transparent
except directly around `NegatedShortcut`, where it sets `DiamondRequired` without
emitting code. Thus `!(true||p) || q` leaves `q` bare, while `(!(true||p)) || q`
diamonds `q`. A boolean cast is stronger: it materializes its operand once and
returns a fresh reusable stack test, so repeated casts add no duplicate diamond.
When an evaluated shortcut prefix reaches a code-free static right operand, that
logical node carries latent `CodeFreePosition` provenance regardless of the right
operand's verdict. A later `!` promotes it to line preservation without changing
`CondOrigin` or value materialization.

A **chain is an `Option<usize>` label id**; `None` is empty and places no frame.
`jump_false`/`jump_true` are total over tests and static verdicts. `&&`/`||`
short-circuit from the left, merge chains, propagate effects only from evaluated
operands, and mark the right item `DiamondRequired` when resolving a live chain
crosses a join. Dead operands contribute no origin, materialization, or position
state.

The consumers use only this explicit state. **`gen_if`** transactionally marks its
line, lowers the condition, and restores the prior pending line for a code-free
verdict except `PreserveFalseIfLine`; it drops only the untaken arm and keeps the
existing reachable-else topology. **`gen_bool_value`** reuses a bare stack value
only for an ordinary, chain-free, `BareAllowed` test. Other items materialize as a
residual static `iconst_0`/`iconst_1` or the general true-first
`iconst_1`/`goto`/`iconst_0` diamond — still asserting an **empty base stack**, so
`println(a && b)`/`println(a < b)` (non-empty stack → `full_frame`) stay a refused
later rung. Loops/`?:` will reuse `gen_cond` verbatim.

The physical machinery is unchanged and reused: forward branches use the
label/fixup table backpatched in `resolve_branches` (which **threads jumps through
unconditional `goto`s**), and `build_frames` emits a frame only at pcs that survive
as real jump targets — `gen_cond` only decides *at which pcs* `resolve_chain` calls
`add_frame`. Because njavac emits branches eagerly, a nested constant-operand
short-circuit (`(!(x>k || false)) || false`) leaves behind `goto`s javac's aliveness
model never keeps, so **`compact_gotos` runs before `resolve_branches`** — a
post-emission fixpoint that deletes exactly the `goto`s that are unreachable or jump
to the next instruction (javac's `Code.resolve` compaction), remapping every
pc-bearing table (`fixups`, `labels` — *threaded* to the goto's ultimate
non-goto destination, not collapsed onto the next byte — `line_numbers`, frame
offsets). It is a no-op on any program javac already matches (empty death set → no
bytes move). Codegen selects sema's statement-entry snapshot before emission and
its exit snapshot afterward; initializer-internal frames therefore exclude the
declared local, branch-entry chains use the enclosing `if` entry, and final joins
use its definite-assignment intersection. Codegen never grows a parallel local-state
model. Branch-local declarations remain unsupported, so scoped `chop` behavior is
not yet reachable. `negate_op` (the 12-opcode branch involution) is debug-asserted
against `int_icmp_branch`/
`int_zero_branch` in `assert_negate_op_consistent` since a drift there would
silently break every comparison.

Line numbers follow javac's pending-stat-position model. `mark_line` replaces the
source line waiting to attach; the next instruction opcode emitted through
`emit_op` consumes it, so a code-free statement's line can be overwritten by a
later statement before any `LineNumberTable` entry exists. `CodeFreePosition` on
the lowered item preserves the line for a static-false negated shortcut reached by
straight-line execution, making `if (!(true || local))` attach to a following live
skip-else `goto`. Using that negated shortcut ungrouped as the left operand of a
surrounding `&&`/`||` demotes the active position back to latent regardless of its
verdict; a later `!` can reactivate it. Grouping after activation protects the
position through logical use. Position states merge by strength (`None` < latent <
active < grouped), independently of origin and materialization. A code-free
statement beginning at a live branch target also suppresses preservation; plain
static false/true verdicts restore the prior pending position. `Gen` carries the
transient `at_control_entry` fact from `add_frame` until the next `emit_op`. If
`compact_gotos` removes a goto, it removes an entry attached to that instruction as
well.

## Determinism / Docker

`Dockerfile` installs the *same* `25.0.2-graalce` (via SDKMAN) so the container
reproduces the golden bytes; the JDK is the base layer and cargo/SDKMAN use
BuildKit cache mounts. Timing repeatability comes from the `docker run` flags in
the `Makefile`'s `bench` target (pinned single core, fixed memory, no swap), not
the image.
