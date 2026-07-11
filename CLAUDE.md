# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

njavac is a toy Java 25 → JVM bytecode compiler written in Rust. Its defining
constraint is **byte-identical output to the reference `javac`** (GraalVM CE
`25.0.2-graalce`, class-file major version 69): for a supported program, njavac's
`.class` must equal javac's `.class` byte-for-byte. Everything about the design
follows from that one invariant.

Current scope is the **"straight-line numeric" subset**: one `public class` with
a `static void main`, locals of any of the eight primitives (`int`/`long`/`float`/
`double`/`boolean`/`char`/`byte`/`short`, with the two-slot `long`/`double`
model), the full arithmetic/bitwise/shift/unary operator set (`+ - * / % & | ^ ~
<< >> >>>`), compound assignment and `++`/`--`, primitive casts with binary
numeric promotion, every literal form, and `System.out.println` of any primitive
or a string literal. Deliberately out of scope (each is a future rung): any
control flow — which forces a `StackMapTable` — string concatenation
(`invokedynamic`), objects/arrays/methods, multiple methods. See `README.md` for
the checked-off feature map and the ordered next rungs.

## Commands

```bash
cargo build --release                       # build lib + the njavac/bench/profile bins
./target/release/njavac <in.java> <out.class>   # compile one file
```

The reference toolchain is `~/.sdkman/candidates/java/25.0.2-graalce/bin/{javac,javap}`.
Byte-identity is specific to that exact JDK build — a different `javac` version
can legitimately produce different golden bytes.

## Testing = the benchmark (there is no `cargo test`)

The `bench` bin is the entire test suite **and** the benchmark. It has two
passes over `fixtures/*.java`:

1. **Correctness (always, any host).** Compiles every fixture with both `javac`
   and njavac and byte-compares. Byte-identity is deterministic, so this runs
   anywhere and is the acceptance gate: **exits non-zero on any mismatch**, and
   prints a noise-stripped `javap -v` divergence (the `Classfile`/`Last modified`/
   `SHA-256` header lines are filtered out) to localize the first failure.
2. **Timing (deterministic harness only).** Times compiling the whole suite with
   each compiler. Host timings are noise (JVM-startup jitter, scheduler,
   thermal), so timing is gated to run only inside the Docker harness.

```bash
./target/release/bench          # correctness over all fixtures; timing skipped with a note
./docker-bench.sh               # build the pinned image, run correctness + deterministic timing
NJAVAC_BENCH_ALLOW_HOST=1 ./target/release/bench   # force host timing (noisy; for quick checks)
```

Key points, several of which are non-obvious:

- **javac is the live reference.** There are no committed golden `.class` files;
  the bench compiles with the real `javac` each run, which also self-validates
  the environment. Do not reintroduce checked-in goldens.
- **Run counts are per-compiler and asymmetric**: njavac is timed 1000×, javac
  5× (`--njavac-runs` / `--javac-runs`), because javac pays ~700 ms of JVM
  startup per run. njavac spawns once per file.
- **Adding a test = drop a `.java` in `fixtures/`.** The filename must match the
  `public class` name (so both compilers emit `<Name>.class`). Aim new fixtures
  at byte-identity edge cases (constant-load opcode boundaries, slot allocation,
  LineNumberTable, constant folding).
- **There is no single-fixture flag.** To iterate on one case, either point
  `--fixtures` at a directory containing just that file, or run the pair by hand:
  `javac -d /tmp/j F.java && njavac F.java /tmp/n.class && cmp /tmp/j/F.class /tmp/n.class`,
  then `javap -v -p` both and diff.
- Env/flags: `JAVAC`/`JAVAP` (or `--javac`/`--javap`) override tool paths (the
  Docker image sets them); `--fixtures`, `--warmup`, `--out-dir` exist too.
  `docker-bench.sh` honors `BENCH_CPU` (default core 2) and `BENCH_MEM` (2g).

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
- **`main`** is a thin CLI; the class name comes from the source, and the
  `SourceFile` attribute from the input file's basename.

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
their raw **bit pattern** so `-0.0`/`NaN` dedup as distinct entries, matching
javac. The dedup map uses a custom FxHash purely for speed; the hash never
affects output, and serialization is deliberately allocation-free (child indices
resolved through borrowed lookup tables, not cloned `Entry` keys). Always re-run
the bench's correctness pass after changes.

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

## Determinism / Docker

`Dockerfile` installs the *same* `25.0.2-graalce` (via SDKMAN) so the container
reproduces the golden bytes; the JDK is the base layer and cargo/SDKMAN use
BuildKit cache mounts. Timing repeatability comes from the `docker run` flags in
`docker-bench.sh` (pinned single core, fixed memory, no swap), not the image.
