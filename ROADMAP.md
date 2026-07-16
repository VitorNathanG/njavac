# ROADMAP.md - active compiler evolution

This file is the **ordered working plan** for compiler infrastructure,
architecture, and confirmed bugs. Work proceeds from top to bottom. Completed
items are deleted rather than retained as history; the lasting record is the
code, its fixtures and doc-comments, and git history.

Language-feature order belongs in README.md. The long-term destination and the
triggers for creating new subsystems belong in ARCHITECTURE.md. Current mechanics
belong in CLAUDE.md. Non-blocking ideas belong in FUTURE_WORK.md until they are
promoted into this sequence.

## Landed Foundation

The diagnostics, differential-testing tools, semantic local model, ordered
attribute model, recursive type model, structural call parsing, semantic call
resolution, and symbolic instruction assembler are established. See CLAUDE.md for
current mechanics and git history for the changes that built them.

## Active Sequence

### Separate Existing Codegen Responsibilities

Split the responsibilities already present in `src/codegen.rs` without changing
behavior or creating the full future module tree. Isolate the symbolic method
assembler, constant and opcode policy, condition lowering, and statement/value
lowering behind explicit boundaries. Keep each move independently verifiable.

### Complete the Symbolic Assembler Boundary

Make the assembler the exclusive owner of symbolic method state. Branch-chain
retargeting and frame requests must go through assembler operations rather than
direct mutation from Java lowering.

Complete instruction forms reachable by the current supported surface, one
fixture-backed bug cycle at a time:

- local loads and stores for slots above 255 using `wide`;
- long conditional and unconditional branches using javac-compatible branch-form
  selection during final layout.

### Complete Semantic Attribution Facts

Record the conversions and promoted types selected during attribution so lowering
does not recompute semantic expression results. Expand resolved invocation facts to
carry the selected owner, member, invocation kind, descriptor, parameter types, and
return type; lowering should consume those facts without reconstructing library
signatures.

Do not build the future generic type arena, resolver environment, or source-type
hierarchy until a language rung triggers those responsibilities.

### Model the Typed Operand Stack

Replace word-depth-only stack tracking with typed symbolic operand-stack state.
Derive field and invocation effects from their modeled types or descriptors, and
let frame requests snapshot the assembler's current stack rather than supplying a
parallel manual vector.

This is the infrastructure prerequisite for full-frame boolean materialization and
the conditional-expression rung in README.md. Land it as a byte-preserving change
before extending language behavior.

### Resume Language Rungs

Once the active infrastructure above is green, return to README.md §"Suggested
next rungs". Any newly triggered structural prerequisite enters this sequence as a
small tidy-first change; the feature itself remains tracked only in README.md.

## Open Fuzzer Findings

No open findings.

When a finding appears, work one signature through reproduction, minimization,
fix, authoritative verification, regression fixture, and removal from this section
before starting another.
