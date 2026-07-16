# ARCHITECTURE.md - target compiler structure

This document defines the **intended long-term structure** of njavac: which layer
owns each fact, which dependencies are allowed, and how the current compact
compiler should grow without losing byte identity to the pinned `javac`.

It is a destination map, not an instruction to create empty modules. The active,
ordered infrastructure work remains in **ROADMAP.md**; language coverage and the
next language rungs remain in **README.md**; current implementation mechanics and
working conventions remain in **CLAUDE.md**. New directories described here are
created only when a concrete feature gives them a real responsibility.

---

## Architectural objective

The mature compiler has six authorities with a one-way flow:

```text
source management
    -> syntax frontend
    -> semantic attribution
    -> javac-compatible lowering
    -> symbolic JVM assembly
    -> class-file serialization
```

The central boundary is between **Java meaning** and **JVM mechanics**. Semantic
analysis decides what a source expression means; javac-compatible lowering decides
the exact shape javac uses to express that meaning; the assembler assigns code
layout and verifier state; the writer serializes an already-complete class plan.

This is deliberately not a generic compiler framework. njavac's product is
javac-identical bytes, so javac-specific decisions are first-class policy rather
than a compatibility layer applied after a generic optimizer.

## Design rules

1. Keep one Rust crate until a measured build or dependency problem justifies more.
2. Keep the plain-enum syntax tree; do not introduce an object hierarchy or visitor
   framework merely to organize files.
3. Preserve source distinctions until semantic attribution is complete. Prefix and
   postfix operations, blocks, parentheses, operator positions, and explicit type
   syntax must not be erased before consumers that need them have run.
4. Use stable IDs for semantic identity. Strings are spellings, not declarations.
5. Keep source type syntax, semantic Java types, JVM stack kinds, descriptors, and
   verification types distinct, with explicit projections between them.
6. Do not add a generic SSA or optimization IR. Lower attributed syntax through a
   javac-shaped item/control-flow model into exact symbolic JVM instructions.
7. Treat every byte-visible sequence as ordered data: constants, instructions,
   targets, frames, attributes, members, bootstrap methods, and generated classes.
8. Hash maps are lookup indexes only. Their iteration order never determines bytes.
9. Keep each javac-specific choice in a named, documented decision function and pin
   it with probes and fixtures.
10. Introduce a module when it gains one concrete responsibility, not because a
    complete Java compiler might eventually need its name.

---

## Target source tree

The eventual shape is:

```text
src/
|-- lib.rs
|-- main.rs
|
|-- compiler/
|   |-- mod.rs
|   |-- request.rs
|   |-- result.rs
|   `-- session.rs
|
|-- source/
|   |-- mod.rs
|   |-- span.rs
|   |-- source_file.rs
|   |-- source_map.rs
|   `-- unicode.rs
|
|-- diagnostic/
|   |-- mod.rs
|   |-- diagnostic.rs
|   `-- sink.rs
|
|-- frontend/
|   |-- mod.rs
|   |-- token.rs
|   |-- lexer.rs
|   |-- syntax/
|   |   |-- mod.rs
|   |   |-- ids.rs
|   |   |-- names.rs
|   |   |-- types.rs
|   |   |-- declarations.rs
|   |   |-- statements.rs
|   |   `-- expressions.rs
|   `-- parser/
|       |-- mod.rs
|       |-- declarations.rs
|       |-- statements.rs
|       |-- expressions.rs
|       `-- recovery.rs
|
|-- semantic/
|   |-- mod.rs
|   |-- model.rs
|   |-- names.rs
|   |-- symbols.rs
|   |-- scopes.rs
|   |-- enter.rs
|   |-- imports.rs
|   |-- resolve.rs
|   |-- attribution.rs
|   |-- overload.rs
|   |-- constants.rs
|   |-- flow.rs
|   |-- locals.rs
|   `-- types/
|       |-- mod.rs
|       |-- arena.rs
|       |-- conversion.rs
|       |-- descriptor.rs
|       |-- erasure.rs
|       `-- inference.rs
|
|-- backend/
|   |-- mod.rs
|   `-- jvm/
|       |-- mod.rs
|       |-- plan/
|       |   |-- mod.rs
|       |   |-- compilation.rs
|       |   |-- class.rs
|       |   |-- member.rs
|       |   `-- synthetic.rs
|       |-- lower/
|       |   |-- mod.rs
|       |   |-- class.rs
|       |   |-- method.rs
|       |   |-- statement.rs
|       |   |-- expression.rs
|       |   |-- condition.rs
|       |   |-- lvalue.rs
|       |   |-- invocation.rs
|       |   |-- switch.rs
|       |   `-- exception.rs
|       |-- bytecode/
|       |   |-- mod.rs
|       |   |-- instruction.rs
|       |   |-- assembler.rs
|       |   |-- control_flow.rs
|       |   |-- stack.rs
|       |   |-- frames.rs
|       |   `-- lines.rs
|       |-- pool/
|       |   |-- mod.rs
|       |   |-- constant_pool.rs
|       |   |-- bootstrap.rs
|       |   `-- modified_utf8.rs
|       `-- classfile/
|           |-- mod.rs
|           |-- attribute.rs
|           `-- writer.rs
|
|-- classdump/
|   |-- mod.rs
|   |-- reader.rs
|   |-- bytecode.rs
|   `-- diff.rs
|
`-- bin/
    |-- bench.rs
    |-- classdiff.rs
    |-- fuzz/
    |   |-- main.rs
    |   |-- model.rs
    |   |-- generate.rs
    |   |-- render.rs
    |   |-- javac.rs
    |   |-- oracle.rs
    |   |-- run.rs
    |   |-- finding.rs
    |   |-- minimize.rs
    |   `-- verify.rs
    `-- profile.rs
```

This tree is intentionally larger than today's compiler. It describes boundaries,
not a near-term file-creation checklist. For example, `switch.rs` appears only when
switch lowering exists, `bootstrap.rs` appears with the first `invokedynamic`
consumer, and `synthetic.rs` appears with the first generated-member family that
needs shared planning.

## Ownership by layer

| Layer | Owns | Must not own |
| ----- | ---- | ------------ |
| `source` | source identity, translated text, spans, line maps | grammar or type rules |
| `diagnostic` | messages, severities, codes, failure classification | parsing decisions |
| `frontend` | tokens and source-faithful syntax | resolution, overloads, bytecode |
| `semantic` | symbols, scopes, types, conversions, constants, flow | JVM opcode choice |
| `jvm::lower` | javac's Java-to-JVM lowering decisions | raw class serialization |
| `jvm::bytecode` | instructions, labels, stack, frames, PCs, lines | Java name resolution |
| `jvm::pool` | ordered CP/bootstrap registration | synthetic artifact discovery |
| `jvm::classfile` | exact class-file encoding | semantic analysis or synthesis |
| `classdump` | independent structural reading and diffing | compiler emission |

---

## Compilation contract

Full Java requires multiple input sources and multiple output classes. The mature
library contract is therefore compilation-shaped:

```rust
pub struct CompileRequest {
    pub sources: Vec<SourceInput>,
    pub options: CompilerOptions,
}

pub struct CompileResult {
    pub classes: Vec<ClassArtifact>,
    pub diagnostics: Vec<Diagnostic>,
    pub status: CompileStatus,
}

pub struct ClassArtifact {
    pub internal_name: InternalName,
    pub output_path: PathBuf,
    pub source: SourceId,
    pub bytes: Vec<u8>,
}
```

The current `compile(source, source_file) -> Vec<u8>` API remains a compatibility
wrapper while the supported surface still guarantees exactly one output class.

## Source positions

Positions are compilation-wide and source-relative:

```rust
pub struct SourceId(u32);
pub struct BytePos(u32);

pub struct Span {
    pub source: SourceId,
    pub start: BytePos,
    pub end: BytePos,
}
```

Spans are half-open. The source layer retains both the original text and Java's
Unicode-escape-translated character stream, with a mapping back to original input
for diagnostics. Internal positions are not truncated to class-file line widths;
that conversion belongs to final emission.

Tokens retain raw spans even when literals are decoded. Syntax nodes retain the
positions of identifiers, operators, braces, and dimensions needed by diagnostics,
line tables, local-variable ranges, and type-annotation target paths.

## Syntax tree

The syntax tree stays as ordinary Rust structs and enums, augmented by stable node
IDs and spans:

```rust
pub struct Node<T> {
    pub id: NodeId,
    pub span: Span,
    pub kind: T,
}
```

Blocks are first-class nodes because they define scopes. Calls and selections are
ordinary syntax rather than recognized library names. Desugaring is delayed until
after attribution and carries an origin back to source syntax.

Unresolved type syntax remains distinct from semantic types. It preserves qualified
names, generic arguments, wildcards, annotations, and array-dimension structure.

## Symbols and scopes

Semantic identity uses IDs:

```rust
pub struct NameId(u32);   // interned spelling
pub struct SymbolId(u32); // declaration identity
pub struct ScopeId(u32);
pub struct LocalId(u32);
pub struct MethodId(u32);
```

Scopes model Java's separate namespaces for types, values, methods, and labels.
Source/member order lives in vectors separate from lookup maps. A use site resolves
to a symbol once; downstream phases never repeat textual lookup.

## Semantic types and attribution

Canonical Java types live in an arena and are referenced by `TyId`:

```rust
pub enum TypeKind {
    Primitive(PrimitiveType),
    Void,
    Null,
    Declared { symbol: SymbolId, enclosing: Option<TyId>, arguments: Vec<TyId> },
    Array(TyId),
    TypeVariable(SymbolId),
    Wildcard { upper: Option<TyId>, lower: Option<TyId> },
    Intersection(Vec<TyId>),
    Union(Vec<TyId>),
    Method(MethodType),
    Error,
}
```

JVM views are centralized projections from semantic types: erasure, descriptors,
generic signatures, local-slot width, stack kind, and verification type. This keeps
Java types distinct from their JVM representation while eliminating parallel,
drifting conversion tables.

Semantic analysis records facts in side tables keyed by syntax IDs:

```rust
pub struct ExprInfo {
    pub ty: TyId,
    pub category: ValueCategory,
    pub constant: Option<ConstValue>,
    pub conversion: ConversionSequence,
    pub invocation: Option<ResolvedInvocation>,
}
```

The backend consumes those facts. It does not recompute expression types, resolve
names, select overloads, or infer descriptors.

Strict JLS constant values are separate from control-flow verdicts and javac's
`Item`/`CondItem` lowering state. Semantic analysis answers whether an expression is
a constant; lowering answers how javac evaluates and materializes it.

## Local layout

Semantic analysis is the single authority for local identity, scope, and layout:

```rust
pub struct SlotLayout {
    pub assignments: IndexVec<LocalId, SlotAssignment>,
    pub max_locals: u16,
    pub events: Vec<LocalStateEvent>,
}
```

The allocator models parameters, `this`, category-2 values, scope exit, sibling
scope reuse, hidden locals, catches, resources, and pattern variables according to
probed javac behavior. Code generation consumes this layout and does not maintain a
parallel declaration-order model for verifier locals.

## Ordered JVM plans

Lowering produces ordered plans before serialization:

```rust
pub struct CompilationPlan {
    pub classes: Vec<ClassPlan>,
}

pub struct ClassPlan {
    pub internal_name: InternalName,
    pub version: ClassVersion,
    pub flags: ClassFlags,
    pub superclass: Option<InternalName>,
    pub interfaces: Vec<InternalName>,
    pub fields: Vec<FieldPlan>,
    pub methods: Vec<MethodPlan>,
    pub attributes: Vec<AttributePlan>,
}
```

The plans are the authority for class, member, and attribute order. A compilation-
level synthetic registry allocates deterministic names and positions for generated
classes and members. The class writer never discovers constructors, bridges,
lambdas, enum helpers, capture fields, or `<clinit>` while writing bytes.

## Java lowering

Java lowering remains javac-native. `CondItem` belongs here because it models
javac's pending true/false chains, not a generic JVM concept. Other javac choices -
switch density, concat recipes, synthetic ordering, bridge generation - receive the
same treatment: one named policy, a complete probe corpus, and regression fixtures.

Feature-specific files appear only when their logic becomes substantial. The first
codegen split should isolate current responsibilities (`constant`, `opcode`,
`condition`, value/statement lowering, and physical method code) before growing the
full target hierarchy.

## Symbolic bytecode

Raw opcode bytes are replaced incrementally by exact symbolic instructions:

```rust
pub enum Instruction {
    Load { kind: StackKind, slot: LocalSlot, form: LoadForm },
    Store { kind: StackKind, slot: LocalSlot, form: StoreForm },
    Constant(ConstantInstruction),
    Binary { kind: StackKind, op: BinaryOperation },
    Convert(ConversionInstruction),
    Field(FieldInstruction),
    Invoke(InvokeInstruction),
    Branch { condition: BranchCondition, target: Label },
    Goto(Label),
    TableSwitch(TableSwitch),
    LookupSwitch(LookupSwitch),
    New(ClassRef),
    Dup(DupForm),
    Return(ReturnKind),
}
```

The instruction records the exact form chosen by javac-compatible lowering. The
assembler must not substitute an equivalent form. Every instruction passes through
one emission chokepoint that updates stack state and returns an instruction anchor.

All PC-bearing metadata refers to labels, instruction anchors, or half-open symbolic
ranges until final layout. This includes branches, line events, frame sites,
exception handlers, local-variable ranges, code type annotations, and uninitialized
object verification values.

Finalization performs javac-compatible reachability/goto handling, branch-form
selection, switch alignment, PC assignment, verifier analysis, metadata resolution,
and byte encoding. The backend never needs to parse its own emitted bytes to recover
instruction boundaries.

## Stack maps and lines

Frame handling separates three decisions: where javac places frames, what verifier
state exists there, and which minimal frame encoding represents that state. The
chosen immutable frame plan is shared by constant-pool collection and serialization.

Line handling uses javac's pending-position model. Marking a source position does
not immediately append a `(pc, line)` pair; the next real instruction consumes the
pending position, and a later mark may overwrite it before any instruction appears.
Final line entries are resolved only after instruction layout is stable.

## Constant pool and bootstrap methods

The current immediate, encounter-ordered constant-pool model remains fundamental:

1. Bytecode operands intern phase-1 entries when lowering encounters them.
2. The ordered class/member/attribute plan interns phase-2 structural entries.
3. Deduplication never reorders entries.
4. Pool indices never change after `ldc` versus `ldc_w` has been selected.
5. Composite-child insertion order is explicit and probed per CP entry family.
6. `Long` and `Double` consume two indices.
7. Float/double keys use javac-compatible NaN canonicalization and preserve signed
   zero.
8. Class-file strings use modified UTF-8.

`BootstrapMethods` has a separate ordered registry. A bootstrap index is an index
into that attribute, not an ordinary constant-pool child; `InvokeDynamic` and
`Dynamic` registration coordinate the two ordered structures explicitly.

## Attributes and writer

Fields, methods, `Code`, and classes hold ordered `Vec<AttributePlan>` values. One
walk of that vector is the authority for phase-2 interning and writing. Counts come
from vector lengths; body lengths come from encoded body buffers, never hand-summed
arithmetic.

An enum is preferred over a trait hierarchy while the supported attribute universe
is closed and auditable. The writer serializes complete plans and performs no Java
semantic decisions.

## Independent class reader

`classdump` stays operationally independent from the writer so a shared bug cannot
make wrong output look correct. It grows alongside the writer with instruction
decoding, modified UTF-8, new pool tags, and structural attribute readers, while
retaining raw fallback for unknown attributes and first-differing-byte reporting.

---

## Evolution triggers

The active order lives in ROADMAP.md. These triggers determine when the destination
modules become justified:

| Trigger | Structural addition |
| ------- | ------------------- |
| structured diagnostics | `source`, `diagnostic`, spans on tokens/nodes |
| nested scopes and loops | semantic symbols/scopes and authoritative slot layout |
| centralized emission | typed instruction API and `MethodAssembler` |
| first new attribute family | ordered `AttributePlan` abstraction |
| first `invokedynamic` use | bootstrap registry and new CP entry support |
| fields/general methods | ordered class/member plans |
| first generated-member family | synthetic registry |
| first multi-class source | compilation request/result and artifact set |
| packages or multiple sources | source set, resolver environment, case fixtures |
| switch | symbolic switch instructions and feature lowering module |
| exceptions | symbolic code ranges, exception table, verifier handler edges |
| generics | erasure/signature/inference modules |
| annotations | owner-specific and code-target attribute plans |

Every transition is staged so existing programs keep their exact bytes. A language
rung is not bundled with the structural refactor that enables it.

## Completion tests for the architecture

The target boundaries are established when:

- backend code never resolves locals by string or recomputes semantic expression
  types;
- semantic slot layout is the only authority for local slots and scope events;
- only the method assembler encodes instructions or mutates the code buffer;
- stack effects are centralized and operand/descriptor aware;
- PC-bearing metadata stays symbolic until finalization;
- attribute order, interning, counts, lengths, and writing share one ordered plan;
- the writer never synthesizes Java artifacts;
- constant-pool order remains explicit and encounter-driven;
- the legacy single-source API is implemented through the compilation API;
- every supported program remains byte-identical to the pinned javac.

## Explicit non-goals

- Multiple Rust crates without measured need.
- A generic compiler framework or plugin architecture.
- A visitor framework introduced only to move code between files.
- SSA or generic optimization passes.
- A spec-correct backend followed by a javac-quirk rewrite layer.
- Empty modules for distant features.
- Hash-derived output ordering.
- A whole-backend rewrite.
- Checked-in golden `.class` files.
- Control-flow optimization beyond javac's exact behavior.
