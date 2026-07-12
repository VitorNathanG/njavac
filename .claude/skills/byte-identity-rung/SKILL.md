---
name: byte-identity-rung
description: >-
  Workflow for implementing a new language rung in njavac — bringing a Java
  construct (a new operator, literal form, statement, type, control-flow shape,
  attribute, etc.) under the invariant that the emitted .class is byte-identical
  to the reference javac. Use when extending the compiler's supported surface,
  when planning the next rung from README/ROADMAP, or when a fixture's bytes
  diverge from javac's and you need to localize and fix it.
---

# Implementing a byte-identity rung

njavac's one invariant: for a supported program, its `.class` must equal the
**pinned** javac's (`GraalVM CE 25.0.2-graalce`, class-file major 69)
**byte-for-byte**. A "rung" is one new construct brought under that invariant.
This is the repeatable workflow; the detailed mechanics live in CLAUDE.md, the
coverage map in README, the infrastructure plan in ROADMAP.

**Every command here is a `make` target — run it and it runs inside the pinned
Docker image, so results are reproducible. Never invoke `javac`, `javap`, `njavac`,
or `docker` by hand; `make help` lists the surface.**

## 0. Scope and constraints first
- Read the README feature map (§A–§I) and "next rungs" to place this work.
- All correctness is validated in Docker; a local run proves nothing about bytes
  (only the pinned javac reproduces them). `make check` builds locally for
  debugging only.

## 1. Learn javac's exact choice — before writing any code
The whole game is copying javac's choices, not inventing your own. Write a minimal
`Probe.java` (anywhere under the repo) and disassemble it with the **pinned** javac:

```
make probe FILE=Probe.java      # runs javac + javap -v -p in the image
```

Read which **opcodes** it picks, the **constant-pool entries and their order**, any
new **attribute**, the **StackMapTable** frames, the **LineNumberTable** entries,
and whether javac **constant-folds** the construct. Vary the probe across
boundaries (literal magnitudes, slot indices, branch offsets, operand types) to
find exactly where the bytes change — those boundaries become your fixtures.

## 2. Locate the code (the pipeline)
`source → lexer → parser → sema → codegen → classfile`
- **lexer / ast / parser** — the source surface (tokens, AST nodes, precedence).
- **sema** (`type_of`, slot allocation) — typing, numeric promotion, local slots.
- **codegen** — opcode selection, the operand-stack model (`max_stack`), constant
  folding, the branch/label/fixup machinery, LineNumberTable.
- **classfile** — the constant pool (**insertion order is the linchpin**),
  attributes, StackMapTable. Touch this the most carefully.

## 3. The byte-identity gotchas (CLAUDE.md §"Where byte-identity is won or lost")
- **Constant pool**: entries in javac's exact insertion order (two-phase interning
  + breadth-first children). `Long`/`Double` consume two indices; `Float`/`Double`
  dedup by raw bit pattern (so `-0.0`/`NaN` pool separately).
- **StackMapTable**: pick the smallest frame form; the first frame's delta is its
  offset, later frames use `offset − prev − 1` (the −1 bias); emit the attribute
  (and its Utf8) **only** when the method has frames.
- **LineNumberTable**: emit an entry only when the source line changes.
- **Folding**: constant-fold literal subtrees exactly as javac (wrapping integer /
  exact IEEE-754 / JLS shift masking), but emit real bytecode once a local is
  involved — a folded constant must be bit-identical to the unfolded computation.

## 4. Add fixtures at the edge cases
Drop `.java` files under `fixtures/<topic>/` (discovered recursively). Aim them at
the byte boundaries found in step 1: constant-load opcode transitions, slot
allocation, frame shapes, folding, attribute presence/absence. Basenames must be
globally unique and match the `public class` name.

## 5. Verify — always via `make` (Docker underneath)
```
make verify                          # fast gate over the whole suite
make verify FILE=fixtures/x/F.java   # one fixture
make bench                           # authoritative: full online run + timing
make diff A=a.class B=b.class        # structural class-file diff by hand
```
On a mismatch the gate prints a structural **classdiff** (byte-offset precise, with
a path like `methods[0].attr[0].Code.max_stack`) before the javap diff — read it to
localize the *cause*, not a downstream symptom. After changing fixtures, run
`make record` to refresh the golden volume.

## 6. Refuse, never mis-compile
Out-of-subset input must be **rejected**, not compiled to wrong bytes. An
`assert!`/`panic!` is caught by the CLI as "unsupported (compiler error)" and no
`.class` is written. Never weaken a check to make something "work" — a wrong byte
is worse than an honest refusal.

## 7. Docs in lockstep — in the *same* commit
- **README**: check off the feature map, update scope prose.
- **CLAUDE.md**: record the new mechanics and any byte-identity gotcha.
- **ROADMAP**: check off any infrastructure item this touched.

## 8. Reflect at the end of the cycle
Produce a short proposal — what went well, what went badly, what would help next
time (a script, better docs, a refactor, another skill) — and bring it to the user
through the question tool. Capture durable lessons in CLAUDE.md.
