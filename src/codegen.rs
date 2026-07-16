//! Code generation: typed AST -> class bytes, via the `classfile` backend.
//!
//! This is where byte-identity is won or lost. Every choice mirrors javac's:
//! constant-load opcode selection per type, local-slot allocation with the
//! two-slot `long`/`double` model, short-form load/store opcodes, `max_stack`/
//! `max_locals`, the `LineNumberTable`, binary numeric promotion with the
//! conversion opcode placed exactly where javac puts it, the `iinc`/`iinc_w`/
//! full-form boundary for compound assignment, and cross-type constant folding.
//!
//! javac constant-folds any subtree whose leaves are all literals into a single
//! typed constant load (with wrapping integer / exact IEEE-754 arithmetic and
//! JLS shift masking), and emits real bytecode the moment a local is involved.
//! We mirror that: `fold` evaluates a maximal constant subtree; anything else is
//! emitted structurally with a running operand-stack model that tracks category-2
//! (`long`/`double`) values as two words.
//!
//! `if`/`else` and comparisons add the first control flow. A boolean expression
//! lowers in one of two modes: as a *branch* (the condition of an `if`, emitting
//! the negated comparison opcode as a jump) or as a *value* (the true-first
//! `iconst_1`/`goto`/`iconst_0` diamond). Both force a `StackMapTable`: codegen
//! records the verifier state (locals + stack) at each branch target and hands
//! them to the backend, which picks the minimal frame encoding. Constant
//! conditions are folded away (dead branches dropped, no frame), and jumps to an
//! unconditional `goto` are threaded through — both exactly as javac does, so a
//! method whose branches all fold stays byte-identical to its straight-line form.

use crate::ast::{
    BinOp, CmpOp, CompilationUnit, ExprArena, ExprId, ExprKind, LogOp, Method, Name, PrimitiveType,
    Stmt, StmtKind, Type,
};
use crate::classfile::{
    ClassFile, CodeAttribute, ConstantPool, Method as CfMethod, StackFrame, VerificationType,
};
use crate::diagnostic::{CompileResult, Diagnostic};
use crate::sema::{self, Analysis, FrameLocal, MethodInfo, StackTy};
use crate::span::Span;

// ---- opcodes ----
const ICONST_M1: u8 = 0x02;
const ICONST_0: u8 = 0x03;
const LCONST_0: u8 = 0x09;
const LCONST_1: u8 = 0x0a;
const FCONST_0: u8 = 0x0b;
const FCONST_1: u8 = 0x0c;
const FCONST_2: u8 = 0x0d;
const DCONST_0: u8 = 0x0e;
const DCONST_1: u8 = 0x0f;
const BIPUSH: u8 = 0x10;
const SIPUSH: u8 = 0x11;
const LDC: u8 = 0x12;
const LDC_W: u8 = 0x13;
const LDC2_W: u8 = 0x14;

// Loads: wide form (opcode + 1-byte slot) and the slot-0 short form.
const ILOAD: u8 = 0x15;
const LLOAD: u8 = 0x16;
const FLOAD: u8 = 0x17;
const DLOAD: u8 = 0x18;
const ILOAD_0: u8 = 0x1a;
const LLOAD_0: u8 = 0x1e;
const FLOAD_0: u8 = 0x22;
const DLOAD_0: u8 = 0x26;
const ALOAD_0: u8 = 0x2a;

// Stores.
const ISTORE: u8 = 0x36;
const LSTORE: u8 = 0x37;
const FSTORE: u8 = 0x38;
const DSTORE: u8 = 0x39;
const ISTORE_0: u8 = 0x3b;
const LSTORE_0: u8 = 0x3f;
const FSTORE_0: u8 = 0x43;
const DSTORE_0: u8 = 0x47;

// Arithmetic.
const IADD: u8 = 0x60;
const LADD: u8 = 0x61;
const FADD: u8 = 0x62;
const DADD: u8 = 0x63;
const ISUB: u8 = 0x64;
const LSUB: u8 = 0x65;
const FSUB: u8 = 0x66;
const DSUB: u8 = 0x67;
const IMUL: u8 = 0x68;
const LMUL: u8 = 0x69;
const FMUL: u8 = 0x6a;
const DMUL: u8 = 0x6b;
const IDIV: u8 = 0x6c;
const LDIV: u8 = 0x6d;
const FDIV: u8 = 0x6e;
const DDIV: u8 = 0x6f;
const IREM: u8 = 0x70;
const LREM: u8 = 0x71;
const FREM: u8 = 0x72;
const DREM: u8 = 0x73;
const INEG: u8 = 0x74;
const LNEG: u8 = 0x75;
const FNEG: u8 = 0x76;
const DNEG: u8 = 0x77;

// Shifts and bitwise.
const ISHL: u8 = 0x78;
const LSHL: u8 = 0x79;
const ISHR: u8 = 0x7a;
const LSHR: u8 = 0x7b;
const IUSHR: u8 = 0x7c;
const LUSHR: u8 = 0x7d;
const IAND: u8 = 0x7e;
const LAND: u8 = 0x7f;
const IOR: u8 = 0x80;
const LOR: u8 = 0x81;
const IXOR: u8 = 0x82;
const LXOR: u8 = 0x83;

// iinc + wide prefix.
const IINC: u8 = 0x84;
const WIDE: u8 = 0xc4;

// Conversions.
const I2L: u8 = 0x85;
const I2F: u8 = 0x86;
const I2D: u8 = 0x87;
const L2I: u8 = 0x88;
const L2F: u8 = 0x89;
const L2D: u8 = 0x8a;
const F2I: u8 = 0x8b;
const F2L: u8 = 0x8c;
const F2D: u8 = 0x8d;
const D2I: u8 = 0x8e;
const D2L: u8 = 0x8f;
const D2F: u8 = 0x90;
const I2B: u8 = 0x91;
const I2C: u8 = 0x92;
const I2S: u8 = 0x93;

// Comparisons and branches.
const LCMP: u8 = 0x94;
const FCMPL: u8 = 0x95;
const FCMPG: u8 = 0x96;
const DCMPL: u8 = 0x97;
const DCMPG: u8 = 0x98;
const IFEQ: u8 = 0x99;
const IFNE: u8 = 0x9a;
const IFLT: u8 = 0x9b;
const IFGE: u8 = 0x9c;
const IFGT: u8 = 0x9d;
const IFLE: u8 = 0x9e;
const IF_ICMPEQ: u8 = 0x9f;
const IF_ICMPNE: u8 = 0xa0;
const IF_ICMPLT: u8 = 0xa1;
const IF_ICMPGE: u8 = 0xa2;
const IF_ICMPGT: u8 = 0xa3;
const IF_ICMPLE: u8 = 0xa4;
const GOTO: u8 = 0xa7;

const ICONST_1: u8 = 0x04;

const GETSTATIC: u8 = 0xb2;
const INVOKEVIRTUAL: u8 = 0xb6;
const INVOKESPECIAL: u8 = 0xb7;
const RETURN: u8 = 0xb1;

/// A compile-time constant value in one of the four JVM computational types.
/// `boolean`/`char` fold into `Int` (their code-point / 0-1 value).
#[derive(Clone, Copy)]
enum Const {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
}

#[derive(Clone, Copy)]
struct StackEffect {
    pop: u16,
    push: u16,
}

impl StackEffect {
    const fn new(pop: u16, push: u16) -> Self {
        StackEffect { pop, push }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct InstructionAnchor(usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct CodePosition(usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Label(usize);

/// One already-selected physical JVM instruction form. Lowering chooses exact
/// forms (`ldc` vs `ldc_w`, short local forms, narrow vs wide `iinc`); the emitter
/// records that choice and derives its stack effect. Finalization only lays out
/// and encodes it; it never substitutes an equivalent form.
#[derive(Clone, Copy)]
enum Instruction {
    Simple(u8),
    U8 {
        opcode: u8,
        operand: u8,
    },
    U16 {
        opcode: u8,
        operand: u16,
    },
    Iinc {
        slot: u8,
        delta: i8,
    },
    WideIinc {
        slot: u16,
        delta: i16,
    },
    Field {
        opcode: u8,
        index: u16,
        push_words: u16,
    },
    Invoke {
        opcode: u8,
        index: u16,
        argument_words: u16,
        return_words: u16,
    },
    Branch {
        opcode: u8,
        target: Label,
    },
}

impl Instruction {
    fn stack_effect(self) -> StackEffect {
        match self {
            Instruction::Simple(opcode)
            | Instruction::U8 { opcode, .. }
            | Instruction::U16 { opcode, .. } => fixed_stack_effect(opcode),
            Instruction::Iinc { .. } | Instruction::WideIinc { .. } => StackEffect::new(0, 0),
            Instruction::Field {
                opcode: GETSTATIC,
                push_words,
                ..
            } => StackEffect::new(0, push_words),
            Instruction::Field { opcode, .. } => panic!("unsupported field opcode: {opcode:#x}"),
            Instruction::Invoke {
                argument_words,
                return_words,
                ..
            } => StackEffect::new(1 + argument_words, return_words),
            Instruction::Branch { opcode, .. } if (IF_ICMPEQ..=IF_ICMPLE).contains(&opcode) => {
                StackEffect::new(2, 0)
            }
            Instruction::Branch { opcode, .. } if (IFEQ..=IFLE).contains(&opcode) => {
                StackEffect::new(1, 0)
            }
            Instruction::Branch { opcode: GOTO, .. } => StackEffect::new(0, 0),
            Instruction::Branch { opcode, .. } => {
                panic!("unsupported branch opcode: {opcode:#x}")
            }
        }
    }

    fn encoded_len(self) -> usize {
        match self {
            Instruction::Simple(_) => 1,
            Instruction::U8 { .. } => 2,
            Instruction::U16 { .. }
            | Instruction::Iinc { .. }
            | Instruction::Field { .. }
            | Instruction::Invoke { .. }
            | Instruction::Branch { .. } => 3,
            Instruction::WideIinc { .. } => 6,
        }
    }

    fn is_goto(self) -> bool {
        matches!(self, Instruction::Branch { opcode: GOTO, .. })
    }

    fn is_cond_branch(self) -> bool {
        matches!(self, Instruction::Branch { opcode, .. } if (IFEQ..=IF_ICMPLE).contains(&opcode))
    }

    fn is_return(self) -> bool {
        matches!(self, Instruction::Simple(RETURN))
    }
}

#[derive(Clone, Copy)]
struct InstructionEntry {
    instruction: Instruction,
    live: bool,
}

#[derive(Clone, Copy)]
struct LabelBinding {
    position: Option<CodePosition>,
}

struct LineEvent {
    instruction: InstructionAnchor,
    line: u16,
}

struct FrameReq {
    position: CodePosition,
    locals: Vec<VerificationType>,
    stack: Vec<VerificationType>,
}

struct AssembledCode {
    code: Vec<u8>,
    line_numbers: Vec<(u16, u16)>,
    stack_frames: Vec<StackFrame>,
    max_stack: u16,
}

/// Symbolic per-method bytecode state. This is the single path for instruction
/// recording, pending-line consumption, and stack accounting; `finish` owns the
/// one final layout and encoding pass.
struct Emitter {
    instructions: Vec<InstructionEntry>,
    labels: Vec<LabelBinding>,
    line_events: Vec<LineEvent>,
    frames: Vec<FrameReq>,
    pending_line: Option<u16>,
    at_control_entry: bool,
    max_stack: u16,
    cur: u16,
}

impl Emitter {
    fn new() -> Self {
        Emitter {
            instructions: Vec::with_capacity(32),
            labels: Vec::new(),
            line_events: Vec::with_capacity(16),
            frames: Vec::new(),
            pending_line: None,
            at_control_entry: false,
            max_stack: 0,
            cur: 0,
        }
    }

    fn emit(&mut self, instruction: Instruction) -> InstructionAnchor {
        let anchor = InstructionAnchor(self.instructions.len());
        if let Some(line) = self.pending_line.take() {
            if self.line_events.last().map(|event| event.line) != Some(line) {
                self.line_events.push(LineEvent {
                    instruction: anchor,
                    line,
                });
            }
        }
        self.at_control_entry = false;
        self.instructions.push(InstructionEntry {
            instruction,
            live: true,
        });

        let effect = instruction.stack_effect();
        self.cur = self
            .cur
            .checked_sub(effect.pop)
            .unwrap_or_else(|| panic!("operand-stack underflow at instruction {}", anchor.0));
        self.cur = self
            .cur
            .checked_add(effect.push)
            .unwrap_or_else(|| panic!("operand-stack overflow at instruction {}", anchor.0));
        self.max_stack = self.max_stack.max(self.cur);
        anchor
    }

    fn position(&self) -> CodePosition {
        CodePosition(self.instructions.len())
    }

    fn new_label(&mut self) -> Label {
        let label = Label(self.labels.len());
        self.labels.push(LabelBinding { position: None });
        label
    }

    fn place_label(&mut self, label: Label) {
        let position = self.position();
        let binding = &mut self.labels[label.0];
        debug_assert!(binding.position.is_none(), "branch label placed twice");
        binding.position = Some(position);
    }

    fn emit_branch(&mut self, opcode: u8, target: Label) -> InstructionAnchor {
        self.emit(Instruction::Branch { opcode, target })
    }

    fn label_position(&self, label: Label) -> CodePosition {
        self.labels[label.0]
            .position
            .unwrap_or_else(|| panic!("unplaced branch label {}", label.0))
    }

    fn next_live_position(&self, position: CodePosition) -> CodePosition {
        let mut index = position.0;
        while index < self.instructions.len() && !self.instructions[index].live {
            index += 1;
        }
        CodePosition(index)
    }

    /// Follow unconditional gotos from a symbolic boundary to the final live
    /// non-goto boundary. The bound also guards malformed goto cycles.
    fn thread_from_position(&self, start: CodePosition) -> CodePosition {
        let mut position = self.next_live_position(start);
        for _ in 0..=self.instructions.len() {
            let Some(entry) = self.instructions.get(position.0).filter(|entry| entry.live) else {
                break;
            };
            let Instruction::Branch {
                opcode: GOTO,
                target,
            } = entry.instruction
            else {
                break;
            };
            let next = self.next_live_position(self.label_position(target));
            if next == position {
                break;
            }
            position = next;
        }
        position
    }

    fn thread_target(&self, label: Label) -> CodePosition {
        self.thread_from_position(self.label_position(label))
    }

    /// Delete only unreachable and goto-to-next gotos, preserving javac's
    /// observed fixpoint behavior. Tombstones keep every symbolic anchor stable.
    fn compact_gotos(&mut self) {
        if !self
            .instructions
            .iter()
            .any(|entry| entry.live && entry.instruction.is_goto())
        {
            return;
        }

        #[cfg(debug_assertions)]
        self.assert_compaction_preconditions();

        loop {
            let n = self.instructions.len();
            let mut reachable = vec![false; n];
            let mut work = vec![self.next_live_position(CodePosition(0))];
            while let Some(position) = work.pop() {
                let index = position.0;
                if index >= n || reachable[index] || !self.instructions[index].live {
                    continue;
                }
                reachable[index] = true;
                let instruction = self.instructions[index].instruction;
                match instruction {
                    Instruction::Branch { target, .. } if instruction.is_goto() => {
                        work.push(self.thread_target(target));
                    }
                    Instruction::Branch { target, .. } if instruction.is_cond_branch() => {
                        work.push(self.thread_target(target));
                        work.push(self.next_live_position(CodePosition(index + 1)));
                    }
                    _ if instruction.is_return() => {}
                    _ => work.push(self.next_live_position(CodePosition(index + 1))),
                }
            }

            let mut dead = Vec::new();
            for (index, entry) in self.instructions.iter().enumerate() {
                if !entry.live || !entry.instruction.is_goto() {
                    continue;
                }
                let Instruction::Branch { target, .. } = entry.instruction else {
                    unreachable!()
                };
                if !reachable[index]
                    || self.thread_target(target)
                        == self.next_live_position(CodePosition(index + 1))
                {
                    dead.push(index);
                }
            }
            if dead.is_empty() {
                break;
            }

            let threaded_labels: Vec<Option<CodePosition>> = self
                .labels
                .iter()
                .map(|binding| {
                    binding
                        .position
                        .map(|position| self.thread_from_position(position))
                })
                .collect();

            for &index in &dead {
                debug_assert!(
                    self.frames.iter().all(|frame| frame.position.0 != index),
                    "frame at a deleted goto"
                );
                self.instructions[index].live = false;
            }

            let normalized_labels: Vec<Option<CodePosition>> = threaded_labels
                .into_iter()
                .map(|position| position.map(|position| self.next_live_position(position)))
                .collect();
            for (binding, position) in self.labels.iter_mut().zip(normalized_labels) {
                binding.position = position;
            }
        }
    }

    #[cfg(debug_assertions)]
    fn assert_compaction_preconditions(&self) {
        for frame in &self.frames {
            debug_assert!(
                self.instructions
                    .get(frame.position.0)
                    .is_none_or(|entry| !entry.instruction.is_goto()),
                "frame requested at a goto"
            );
        }
    }

    fn layout(&self) -> Vec<u32> {
        let mut pcs = Vec::with_capacity(self.instructions.len() + 1);
        let mut pc = 0u32;
        for entry in &self.instructions {
            pcs.push(pc);
            if entry.live {
                pc = pc
                    .checked_add(entry.instruction.encoded_len() as u32)
                    .expect("method code length overflow");
            }
        }
        pcs.push(pc);
        assert!(
            pc <= u16::MAX as u32,
            "method code exceeds JVM Code attribute limit"
        );
        pcs
    }

    fn resolve_lines(&self, pcs: &[u32]) -> Vec<(u16, u16)> {
        let mut out = Vec::with_capacity(self.line_events.len());
        for event in &self.line_events {
            if !self.instructions[event.instruction.0].live {
                continue;
            }
            if out.last().map(|&(_, line)| line) != Some(event.line) {
                out.push((pcs[event.instruction.0] as u16, event.line));
            }
        }
        out
    }

    fn live_target_pcs(&self, pcs: &[u32]) -> std::collections::HashSet<u32> {
        self.instructions
            .iter()
            .filter(|entry| entry.live)
            .filter_map(|entry| match entry.instruction {
                Instruction::Branch { target, .. } => Some(pcs[self.thread_target(target).0]),
                _ => None,
            })
            .collect()
    }

    fn resolve_frames(
        &mut self,
        pcs: &[u32],
        live_targets: &std::collections::HashSet<u32>,
    ) -> Vec<StackFrame> {
        self.frames.sort_by_key(|frame| pcs[frame.position.0]);
        let mut out: Vec<StackFrame> = Vec::new();
        for frame in &self.frames {
            let offset = pcs[frame.position.0];
            if !live_targets.contains(&offset) {
                continue;
            }
            let offset = offset as u16;
            if let Some(previous) = out.last().filter(|previous| previous.offset == offset) {
                debug_assert_eq!(
                    (&previous.locals, &previous.stack),
                    (&frame.locals, &frame.stack),
                    "conflicting frame states requested at pc {offset}"
                );
                continue;
            }
            out.push(StackFrame {
                offset,
                locals: frame.locals.clone(),
                stack: frame.stack.clone(),
            });
        }
        out
    }

    fn encode(&self, pcs: &[u32]) -> Vec<u8> {
        let mut code = Vec::with_capacity(*pcs.last().unwrap() as usize);
        for (index, entry) in self.instructions.iter().enumerate() {
            if !entry.live {
                continue;
            }
            debug_assert_eq!(code.len(), pcs[index] as usize);
            let before = code.len();
            match entry.instruction {
                Instruction::Simple(opcode) => code.push(opcode),
                Instruction::U8 { opcode, operand } => {
                    code.push(opcode);
                    code.push(operand);
                }
                Instruction::U16 { opcode, operand }
                | Instruction::Field {
                    opcode,
                    index: operand,
                    ..
                }
                | Instruction::Invoke {
                    opcode,
                    index: operand,
                    ..
                } => {
                    code.push(opcode);
                    push_u16(&mut code, operand);
                }
                Instruction::Iinc { slot, delta } => {
                    code.push(IINC);
                    code.push(slot);
                    code.push(delta as u8);
                }
                Instruction::WideIinc { slot, delta } => {
                    code.push(WIDE);
                    code.push(IINC);
                    push_u16(&mut code, slot);
                    push_u16(&mut code, delta as u16);
                }
                Instruction::Branch { opcode, target } => {
                    let target_pc = pcs[self.thread_target(target).0] as i64;
                    let branch_pc = pcs[index] as i64;
                    let offset = i16::try_from(target_pc - branch_pc)
                        .expect("branch offset exceeds selected narrow form");
                    code.push(opcode);
                    code.extend_from_slice(&offset.to_be_bytes());
                }
            }
            debug_assert_eq!(code.len() - before, entry.instruction.encoded_len());
        }
        debug_assert_eq!(code.len(), *pcs.last().unwrap() as usize);
        code
    }

    fn finish(mut self) -> AssembledCode {
        self.compact_gotos();
        let pcs = self.layout();
        let live_targets = self.live_target_pcs(&pcs);
        let line_numbers = self.resolve_lines(&pcs);
        let stack_frames = self.resolve_frames(&pcs, &live_targets);
        let code = self.encode(&pcs);
        AssembledCode {
            code,
            line_numbers,
            stack_frames,
            max_stack: self.max_stack,
        }
    }
}

/// A complete class-file plan plus the phase-1 constant pool built while lowering
/// bytecode. Serialization owns phase-2 structural interning and class-file layout.
pub struct ClassPlan {
    class_file: ClassFile,
    constant_pool: ConstantPool,
}

impl ClassPlan {
    pub fn to_bytes(self) -> Vec<u8> {
        self.class_file.to_bytes(self.constant_pool)
    }
}

/// Build the typed bytecode and class-file model without serializing it.
pub fn plan(
    unit: &CompilationUnit,
    analysis: &Analysis,
    source_file: &str,
) -> CompileResult<ClassPlan> {
    assert_eq!(
        unit.exprs.identity(),
        analysis.arena_identity(),
        "analysis belongs to a different expression arena"
    );
    assert_eq!(
        unit.class.methods.len(),
        analysis.methods.len(),
        "analysis method count does not match the compilation unit"
    );
    preflight_codegen(unit, analysis)?;
    #[cfg(debug_assertions)]
    assert_negate_op_consistent();
    let mut cp = ConstantPool::new();
    let class = &unit.class;

    let mut methods = Vec::new();
    // `<init>` first: its `Methodref` is interned before any of main's operands.
    methods.push(gen_init(&mut cp, &class.super_class, class.line));
    for (m, info) in class.methods.iter().zip(&analysis.methods) {
        methods.push(gen_method(&mut cp, m, info, &unit.exprs));
    }

    let class_file = ClassFile::new(
        0x0021, // ACC_PUBLIC | ACC_SUPER
        class.name.clone(),
        class.super_class.clone(),
        methods,
        source_file,
    );
    Ok(ClassPlan {
        class_file,
        constant_pool: cp,
    })
}

/// Compile one parsed+analyzed class into `.class` bytes.
pub fn generate(
    unit: &CompilationUnit,
    analysis: &Analysis,
    source_file: &str,
) -> CompileResult<Vec<u8>> {
    Ok(plan(unit, analysis, source_file)?.to_bytes())
}

/// Reject the one valid-Java value shape that needs verifier frames the emitter
/// cannot yet represent: materializing a branch boolean over a live base stack.
/// This runs before constant-pool interning or byte emission; the corresponding
/// emitter assert remains a post-preflight invariant.
fn preflight_codegen(unit: &CompilationUnit, analysis: &Analysis) -> CompileResult<()> {
    for (method, info) in unit.class.methods.iter().zip(&analysis.methods) {
        for stmt in &method.body {
            preflight_stmt(stmt, info, &unit.exprs)?;
        }
    }
    Ok(())
}

fn preflight_stmt(stmt: &Stmt, info: &MethodInfo, exprs: &ExprArena) -> CompileResult<()> {
    match &stmt.kind {
        StmtKind::LocalDecl {
            name,
            init: Some(init),
            ..
        }
        | StmtKind::Assign { name, value: init } => {
            if info.ty(name) == PrimitiveType::Boolean {
                preflight_materialization(*init, false, stmt.span, info, exprs)?;
            } else {
                preflight_value(*init, false, stmt.span, info, exprs)?;
            }
        }
        StmtKind::LocalDecl { init: None, .. } => {}
        StmtKind::CompoundAssign { value, .. } => {
            // The target value is loaded before the RHS except when folding makes
            // the RHS code-free; `preflight_value` applies that same fold first.
            preflight_value(*value, true, stmt.span, info, exprs)?;
        }
        StmtKind::Expr(expr) => match &exprs[*expr] {
            ExprKind::Println(arg) => {
                // `getstatic System.out` leaves the receiver live while evaluating arg.
                preflight_value(*arg, true, stmt.span, info, exprs)?;
            }
            _ => unreachable!("sema accepted a non-println expression statement"),
        },
        StmtKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            preflight_cond(*cond, false, stmt.span, info, exprs)?;
            for nested in &then_branch.stmts {
                preflight_stmt(nested, info, exprs)?;
            }
            for nested in else_branch.iter().flat_map(|body| &body.stmts) {
                preflight_stmt(nested, info, exprs)?;
            }
        }
    }
    Ok(())
}

/// Mirror `gen_value`'s left-to-right evaluation enough to track whether a
/// branch-valued boolean reaches `gen_bool_value` with another value live.
fn preflight_value(
    expr: ExprId,
    base_live: bool,
    span: crate::span::Span,
    info: &MethodInfo,
    exprs: &ExprArena,
) -> CompileResult<()> {
    if matches!(&exprs[expr], ExprKind::StringLit(_)) || fold(exprs, expr).is_some() {
        return Ok(());
    }
    match &exprs[expr] {
        ExprKind::Name(_) => Ok(()),
        ExprKind::Neg(inner) | ExprKind::BitNot(inner) | ExprKind::Paren(inner) => {
            preflight_value(*inner, base_live, span, info, exprs)
        }
        ExprKind::Cast { expr, .. } => preflight_value(*expr, base_live, span, info, exprs),
        ExprKind::Binary { left, right, .. } => {
            preflight_value(*left, base_live, span, info, exprs)?;
            preflight_value(*right, true, span, info, exprs)
        }
        ExprKind::Compare { .. } | ExprKind::Not(_) | ExprKind::Logical { .. } => {
            preflight_materialization(expr, base_live, span, info, exprs)
        }
        ExprKind::IntLit(_)
        | ExprKind::LongLit(_)
        | ExprKind::FloatLit(_)
        | ExprKind::DoubleLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::CharLit(_) => Ok(()),
        ExprKind::Println(_) => unreachable!("sema accepted println as a value"),
        ExprKind::StringLit(_) => unreachable!("handled above"),
    }
}

fn preflight_materialization(
    expr: ExprId,
    base_live: bool,
    span: crate::span::Span,
    info: &MethodInfo,
    exprs: &ExprArena,
) -> CompileResult<()> {
    if base_live {
        return Err(Diagnostic::unsupported_codegen(
            span,
            "boolean value materialization with a live operand-stack value is unsupported",
        ));
    }
    preflight_cond(expr, false, span, info, exprs)
}

/// Mirror condition lowering: comparisons evaluate operands as values, logical
/// operators consume the left test before evaluating the right, and a boolean
/// cast explicitly materializes its operand.
fn preflight_cond(
    expr: ExprId,
    base_live: bool,
    span: crate::span::Span,
    info: &MethodInfo,
    exprs: &ExprArena,
) -> CompileResult<()> {
    if lowering_const(exprs, expr).is_some() {
        return Ok(());
    }
    match &exprs[expr] {
        ExprKind::Not(inner) | ExprKind::Paren(inner) => {
            preflight_cond(*inner, base_live, span, info, exprs)
        }
        ExprKind::Cast { ty, expr } if ty.is_boolean() => {
            preflight_materialization(*expr, base_live, span, info, exprs)
        }
        ExprKind::Compare { left, right, .. } => {
            preflight_value(*left, base_live, span, info, exprs)?;
            preflight_value(*right, true, span, info, exprs)
        }
        ExprKind::Logical { op, left, right } => {
            preflight_cond(*left, base_live, span, info, exprs)?;
            let left_decides = fold(exprs, *left).is_some_and(|value| match op {
                LogOp::And => to_i32(value) == 0,
                LogOp::Or => to_i32(value) != 0,
            });
            if left_decides {
                Ok(())
            } else {
                preflight_cond(*right, base_live, span, info, exprs)
            }
        }
        _ => preflight_value(expr, base_live, span, info, exprs),
    }
}

/// The implicit default constructor: `aload_0; invokespecial super.<init>; return`.
fn gen_init(cp: &mut ConstantPool, super_class: &str, class_line: u16) -> CfMethod {
    let mut emitter = Emitter::new();
    emitter.pending_line = Some(class_line);
    emitter.emit(Instruction::Simple(ALOAD_0));
    let init_ref = cp.methodref(super_class, "<init>", "()V");
    emitter.emit(Instruction::Invoke {
        opcode: INVOKESPECIAL,
        index: init_ref,
        argument_words: 0,
        return_words: 0,
    });
    emitter.emit(Instruction::Simple(RETURN));
    let assembled = emitter.finish();

    CfMethod::with_code(
        0x0001, // ACC_PUBLIC
        "<init>",
        "()V",
        CodeAttribute::new(
            assembled.max_stack,
            1,
            assembled.code,
            assembled.line_numbers,
            Vec::new(),
            assembled.stack_frames,
        ),
    )
}

/// Emit one method body.
fn gen_method(
    cp: &mut ConstantPool,
    method: &Method,
    info: &MethodInfo,
    exprs: &ExprArena,
) -> CfMethod {
    let entry_locals = verification_locals(info.entry_frame_locals());

    let mut g = Gen {
        cp,
        info,
        exprs,
        emitter: Emitter::new(),
        semantic_locals: info.entry_frame_locals(),
    };

    for stmt in &method.body {
        g.gen_stmt(stmt);
    }

    // Every void method ends with an appended `return`, mapped to the closing brace.
    g.mark_line(method.close_line);
    g.emit_op(RETURN);
    let assembled = g.emitter.finish();

    CfMethod::with_code(
        0x0009, // ACC_PUBLIC | ACC_STATIC
        method.name.clone(),
        descriptor_of(method),
        CodeAttribute::new(
            assembled.max_stack,
            info.max_locals,
            assembled.code,
            assembled.line_numbers,
            entry_locals,
            assembled.stack_frames,
        ),
    )
}

/// Build the JVM method descriptor from the parsed signature.
fn descriptor_of(method: &Method) -> String {
    let mut d = String::from("(");
    for p in &method.params {
        p.ty.write_descriptor(&mut d);
    }
    d.push(')');
    method.return_type.write_descriptor(&mut d);
    d
}

/// javac's `Items.CondItem`, restricted to njavac's side-effect-free boolean
/// subset. Lowering a boolean expression (`gen_cond`) emits every operand load
/// eagerly but leaves the *deciding branch* pending in `opcode`; the not-yet-
/// resolved jump sites are collected in `true_chain`/`false_chain`. Consumers
/// (`gen_if`, `gen_bool_value`) then resolve those chains to concrete pcs. This is
/// the one representation that expresses javac's constant short-circuit collapse
/// (`true || q`, `q && false`, …) — see the `&&`/`||` corpus.
#[derive(Clone, Copy)]
struct CondItem {
    /// The pending deciding branch, or a static verdict.
    opcode: CondOp,
    /// Chains as label ids collecting pending jump sites. `None` = the empty chain
    /// (javac's null): nothing targets it, so resolving it places no frame. A
    /// `Some` chain always has at least one live symbolic branch.
    true_chain: Option<Label>,
    false_chain: Option<Label>,
    /// True iff an un-branched boolean 0/1 is currently on the operand stack (the
    /// bare-value leaf sets it; any emitted branch consumes and clears it). It is
    /// reusable only when the other item-state dimensions also permit it.
    stack_reuse: bool,
    /// How a code-free static verdict arose. A negated shortcut is the one origin
    /// whose surrounding grouping can affect later value materialization.
    origin: CondOrigin,
    /// Whether a final reusable stack value may stay bare or must pass through
    /// javac's true/false materialization diamond.
    materialization: Materialization,
    /// Independent pending-position effect for a code-free static-false `if`.
    position: CodeFreePosition,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CondOrigin {
    Ordinary,
    Shortcut,
    NegatedShortcut,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Materialization {
    BareAllowed,
    DiamondRequired,
}

/// Pending-line provenance, ordered by merge strength. Logical nodes keep the
/// strongest state contributed by their evaluated operands.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum CodeFreePosition {
    None,
    ShortcutAwaitingNegation,
    PreserveFalseIfLine,
    PreserveThroughLogicalLeft,
}

/// The deciding branch of a `CondItem`: a real conditional test (taken when the
/// condition is *true*), or a static verdict mirroring javac's `goto_`/`dontgoto`.
#[derive(Clone, Copy)]
enum CondOp {
    Test(u8), // conditional branch opcode taken when TRUE (ifne / if_icmplt / …)
    Goto,     // statically TRUE
    DontGoto, // statically FALSE
}

impl CondItem {
    /// Statically always-true: an unconditional `goto` sense with no pending
    /// false jumps. Exactly javac's `CondItem.isTrue()`.
    fn is_true(&self) -> bool {
        matches!(self.opcode, CondOp::Goto) && self.false_chain.is_none()
    }
    /// Statically always-false: never jumps true and no pending true jumps.
    fn is_false(&self) -> bool {
        matches!(self.opcode, CondOp::DontGoto) && self.true_chain.is_none()
    }
    /// `!e`: swap the true/false chains and negate the deciding branch.
    fn negate(self) -> CondItem {
        let origin = match self.origin {
            CondOrigin::Ordinary => CondOrigin::Ordinary,
            CondOrigin::Shortcut | CondOrigin::NegatedShortcut => CondOrigin::NegatedShortcut,
        };
        CondItem {
            opcode: match self.opcode {
                CondOp::Goto => CondOp::DontGoto,
                CondOp::DontGoto => CondOp::Goto,
                CondOp::Test(op) => CondOp::Test(negate_op(op)),
            },
            true_chain: self.false_chain,
            false_chain: self.true_chain,
            // `stack_reuse` asserts the stacked 0/1 equals the boolean result; a
            // negation inverts the result, so the un-touched stack value is now the
            // *opposite* and must NOT be used as-is. Clearing this forces `!p` (and
            // `!!p`, which restores the `IFNE` opcode but stays cleared) through the
            // materialization diamond in `gen_bool_value`, matching javac, which
            // diamonds every negation rather than reusing the loaded value.
            stack_reuse: false,
            origin,
            materialization: self.materialization,
            position: match self.position {
                CodeFreePosition::PreserveThroughLogicalLeft => {
                    CodeFreePosition::PreserveThroughLogicalLeft
                }
                CodeFreePosition::ShortcutAwaitingNegation
                | CodeFreePosition::PreserveFalseIfLine => CodeFreePosition::PreserveFalseIfLine,
                CodeFreePosition::None if origin == CondOrigin::NegatedShortcut => {
                    CodeFreePosition::PreserveFalseIfLine
                }
                CodeFreePosition::None => CodeFreePosition::None,
            },
        }
    }

    /// Grouping is transparent except around a negated non-strict shortcut. In
    /// that one case javac keeps a value-materialization requirement for a later
    /// logical result, without emitting code for the grouped operand itself.
    fn parenthesize(mut self) -> CondItem {
        if self.origin == CondOrigin::NegatedShortcut {
            self.materialization = Materialization::DiamondRequired;
        }
        if self.position == CodeFreePosition::PreserveFalseIfLine {
            self.position = CodeFreePosition::PreserveThroughLogicalLeft;
        }
        self
    }

    /// An ungrouped active position used as a logical left operand becomes latent:
    /// it cannot preserve a line immediately, but a later `!` can reactivate it.
    /// Grouping after activation protects the active state through logical use.
    fn as_logical_left(mut self) -> CondItem {
        if self.position == CodeFreePosition::PreserveFalseIfLine {
            self.position = CodeFreePosition::ShortcutAwaitingNegation;
        }
        self
    }

    fn mark_shortcut(mut self) -> CondItem {
        self.origin = CondOrigin::Shortcut;
        self
    }

    fn carry_prefix(&mut self, prefix: &CondItem, crossed_join: bool) {
        let code_free_static_right = (self.is_true() || self.is_false())
            && self.true_chain.is_none()
            && self.false_chain.is_none();
        if prefix.origin == CondOrigin::Shortcut && code_free_static_right {
            // A static right operand keeps shortcut ancestry only for a later
            // negation's source-position behavior. It must not taint origin or
            // value materialization.
            self.position =
                std::cmp::max(self.position, CodeFreePosition::ShortcutAwaitingNegation);
        }
        if prefix.materialization == Materialization::DiamondRequired || crossed_join {
            self.materialization = Materialization::DiamondRequired;
        }
        self.position = std::cmp::max(self.position, prefix.position);
    }
}

/// A statically-true `CondItem` (no code emitted); javac's `goto_` verdict.
fn cond_true() -> CondItem {
    cond_static(true)
}
/// A statically-false `CondItem` (no code emitted); javac's `dontgoto` verdict.
fn cond_false() -> CondItem {
    cond_static(false)
}

fn cond_static(value: bool) -> CondItem {
    CondItem {
        opcode: if value {
            CondOp::Goto
        } else {
            CondOp::DontGoto
        },
        true_chain: None,
        false_chain: None,
        stack_reuse: false,
        origin: CondOrigin::Ordinary,
        materialization: Materialization::BareAllowed,
        position: CodeFreePosition::None,
    }
}

fn cond_stack_test() -> CondItem {
    CondItem {
        opcode: CondOp::Test(IFNE),
        true_chain: None,
        false_chain: None,
        stack_reuse: true,
        origin: CondOrigin::Ordinary,
        materialization: Materialization::BareAllowed,
        position: CodeFreePosition::None,
    }
}

/// Per-method emission state, with a running operand-stack depth (`cur`) tracked
/// in words so category-2 values count as two.
struct Gen<'a> {
    cp: &'a mut ConstantPool,
    info: &'a MethodInfo,
    exprs: &'a ExprArena,
    emitter: Emitter,
    /// The current sema-owned verifier-local snapshot. Statement generation only
    /// selects an entry or exit state; it never mutates local state independently.
    semantic_locals: &'a [FrameLocal],
}

impl std::ops::Deref for Gen<'_> {
    type Target = Emitter;

    fn deref(&self) -> &Self::Target {
        &self.emitter
    }
}

impl std::ops::DerefMut for Gen<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.emitter
    }
}

impl<'a> Gen<'a> {
    // -------- control flow / labels / frames --------

    /// Emit one statement. Each statement starts with an empty operand stack; a
    /// leaf statement gets a LineNumberTable entry at its first instruction, while
    /// an `if` places its own entries (condition, then each nested statement).
    fn gen_stmt(&mut self, stmt: &Stmt) {
        self.cur = 0;
        self.install_stmt_entry(stmt.span);
        if let StmtKind::If {
            cond,
            then_branch,
            else_branch,
        } = &stmt.kind
        {
            self.gen_if(
                stmt.span,
                stmt.line,
                *cond,
                &then_branch.stmts,
                else_branch.as_ref().map(|body| body.stmts.as_slice()),
            );
        } else {
            self.mark_line(stmt.line);
            match &stmt.kind {
                StmtKind::LocalDecl { name, init, .. } => {
                    if let Some(init) = init {
                        self.store_to(name, *init);
                    }
                }
                StmtKind::Assign { name, value } => self.store_to(name, *value),
                StmtKind::CompoundAssign { name, op, value } => {
                    self.gen_compound(name, *op, *value)
                }
                StmtKind::Expr(expr) => self.gen_expr_stmt(*expr),
                StmtKind::If { .. } => unreachable!("handled above"),
            }
        }
        self.install_stmt_exit(stmt.span);
    }

    /// `if (cond) then [else els]`, a faithful port of javac's `visitIf`. A code-free
    /// static verdict emits only the taken arm and no frame; a static-false negated
    /// shortcut leaves its source line pending only on straight-line entry. A live
    /// branch target suppresses it. Otherwise `gen_cond` lowers the condition to a
    /// `CondItem` and its chains are resolved to the then/else/end
    /// targets. When the condition is statically false only the *then* is dropped
    /// (the else still runs); the trailing `goto`+else block is emitted only when
    /// the else is actually reachable (no spurious `goto`, no dead else).
    fn gen_if(
        &mut self,
        stmt_span: Span,
        line: u16,
        cond: ExprId,
        then_b: &[Stmt],
        else_b: Option<&[Stmt]>,
    ) {
        let previous_line = self.pending_line;
        let entered_by_branch = self.at_control_entry;
        self.mark_line(line);
        let code_before = self.instructions.len();
        let c = self.gen_cond(cond);

        // A code-free verdict has no instruction to consume the condition line.
        // Restore the previous pending position unless the lowered item carries
        // javac's preserving provenance for a static-false negated shortcut.
        if self.instructions.len() == code_before {
            let taken = if c.is_true() {
                true
            } else if c.is_false() {
                false
            } else {
                unreachable!("code-free condition without a static verdict")
            };
            let preserve_false_line = !taken
                && matches!(
                    c.position,
                    CodeFreePosition::PreserveFalseIfLine
                        | CodeFreePosition::PreserveThroughLogicalLeft
                )
                && !entered_by_branch;
            if !preserve_false_line {
                self.pending_line = previous_line;
            }
            let arm = if taken { Some(then_b) } else { else_b };
            for s in arm.unwrap_or(&[]) {
                self.gen_stmt(s);
            }
            return;
        }

        let is_false = c.is_false();
        let true_chain = c.true_chain;
        let else_chain = self.jump_false(c); // emit the false branch(es); may be None

        if !is_false {
            self.install_stmt_entry(stmt_span);
            self.resolve_chain(true_chain); // then-entry (frame iff a branch lands)
            for s in then_b {
                self.gen_stmt(s);
            }
        }
        // Emit the else body only when there is a reachable else target (or the
        // condition is statically false, so the then was dropped and the else is
        // the live arm). A statically-true condition with a dead else falls through
        // to the `_` arm: no goto, no else code.
        match else_b {
            Some(els) if else_chain.is_some() || is_false => {
                // Skip the else after a live then-body with a trailing goto.
                let end = if !is_false {
                    Some(self.branch_to_new(GOTO))
                } else {
                    None
                };
                self.install_stmt_entry(stmt_span);
                self.resolve_chain(else_chain);
                for s in els {
                    self.gen_stmt(s);
                }
                if let Some(end) = end {
                    self.install_stmt_exit(stmt_span);
                    self.resolve_chain(Some(end));
                }
            }
            _ => {
                self.install_stmt_exit(stmt_span);
                self.resolve_chain(else_chain);
            }
        }
    }

    /// Lower a boolean expression to a `CondItem` (javac's `genCond`): emit its
    /// operand loads eagerly, leaving only the deciding branch pending. A
    /// complete lowering-constant subtree collapses to a static verdict with no
    /// code. Non-strict `false && q` / `true || q` instead walk structurally and
    /// mark a shortcut verdict while dropping the dead operand. `&&`/`||` short-
    /// circuit from the *left*: the left's deciding branch is emitted, its
    /// non-deciding outcome falls through into the right operand, and the two
    /// chains are merged (`Code.mergeChains`).
    fn gen_cond(&mut self, e: ExprId) -> CondItem {
        // This query requires the complete subtree to be available as a javac
        // immediate. Non-strict shortcuts (`true || local`) stay structural so
        // grouping, negation, and casts retain their observable lowering history.
        if let Some(c) = lowering_const(self.exprs, e) {
            return if to_i32(c) != 0 {
                cond_true()
            } else {
                cond_false()
            };
        }
        let exprs = self.exprs;
        match &exprs[e] {
            ExprKind::Not(inner) => self.gen_cond(*inner).negate(),
            ExprKind::Paren(inner) => self.gen_cond(*inner).parenthesize(),
            ExprKind::Cast { ty, expr } if ty.is_boolean() => {
                self.gen_bool_value(*expr);
                cond_stack_test()
            }
            ExprKind::Compare { op, left, right } => self.gen_compare_cond(*op, *left, *right),
            ExprKind::Logical {
                op: LogOp::And,
                left,
                right,
            } => {
                let lc = self.gen_cond(*left).as_logical_left();
                if lc.is_false() {
                    return lc.mark_shortcut(); // false && _ : right is dead
                }
                let crossed_join = lc.true_chain.is_some();
                let lt = lc.true_chain;
                let fj = self.jump_false(lc); // emit the left's false branch
                self.resolve_chain(lt); // left-true falls through to the right
                let mut rc = self.gen_cond(*right);
                rc.false_chain = self.merge_chains(fj, rc.false_chain);
                rc.carry_prefix(&lc, crossed_join);
                rc
            }
            ExprKind::Logical {
                op: LogOp::Or,
                left,
                right,
            } => {
                let lc = self.gen_cond(*left).as_logical_left();
                if lc.is_true() {
                    return lc.mark_shortcut(); // true || _ : right is dead
                }
                let crossed_join = lc.false_chain.is_some();
                let lf = lc.false_chain;
                let tj = self.jump_true(lc);
                self.resolve_chain(lf);
                let mut rc = self.gen_cond(*right);
                rc.true_chain = self.merge_chains(tj, rc.true_chain);
                rc.carry_prefix(&lc, crossed_join);
                rc
            }
            // A bare boolean value (a local, or `&`/`|`/`^` on booleans): load its
            // 0/1 onto the stack, pending an `ifne`(true)/`ifeq`(false) test.
            _ => {
                self.gen_value(e); // pushes 0/1 (cur += 1)
                cond_stack_test()
            }
        }
    }

    /// Lower a comparison to a `CondItem`: emit its operands (and the wide
    /// `lcmp`/`fcmp*`/`dcmp*`), but *not* the branch — the deciding test opcode
    /// (true polarity) is returned pending. Its operands are popped when the
    /// branch is finally emitted, in `emit_test_branch`.
    fn gen_compare_cond(&mut self, op: CmpOp, left: ExprId, right: ExprId) -> CondItem {
        let p = sema::binary_promote(
            sema::type_of(left, self.info).primitive(),
            sema::type_of(right, self.info).primitive(),
        );
        let opcode = match p.stack() {
            StackTy::Int => {
                // javac folds `x <op> 0` to the compare-with-zero opcodes, but only
                // when the literal `0` is the *right* operand.
                if matches!(fold(self.exprs, right), Some(Const::Int(0))) {
                    self.gen_promoted_operand(left, PrimitiveType::Int);
                    int_zero_branch(op, true)
                } else {
                    self.gen_promoted_operand(left, PrimitiveType::Int);
                    self.gen_promoted_operand(right, PrimitiveType::Int);
                    int_icmp_branch(op, true)
                }
            }
            StackTy::Long => {
                self.gen_promoted_operand(left, PrimitiveType::Long);
                self.gen_promoted_operand(right, PrimitiveType::Long);
                self.emit_op(LCMP);
                int_zero_branch(op, true)
            }
            StackTy::Float => {
                self.gen_promoted_operand(left, PrimitiveType::Float);
                self.gen_promoted_operand(right, PrimitiveType::Float);
                self.emit_op(if matches!(op, CmpOp::Lt | CmpOp::Le) {
                    FCMPG
                } else {
                    FCMPL
                });
                int_zero_branch(op, true)
            }
            StackTy::Double => {
                self.gen_promoted_operand(left, PrimitiveType::Double);
                self.gen_promoted_operand(right, PrimitiveType::Double);
                self.emit_op(if matches!(op, CmpOp::Lt | CmpOp::Le) {
                    DCMPG
                } else {
                    DCMPL
                });
                int_zero_branch(op, true)
            }
        };
        CondItem {
            opcode: CondOp::Test(opcode),
            true_chain: None,
            false_chain: None,
            stack_reuse: false,
            origin: CondOrigin::Ordinary,
            materialization: Materialization::BareAllowed,
            position: CodeFreePosition::None,
        }
    }

    /// Emit the branch that routes the FALSE outcome of `c` to a chain, returning
    /// it (javac's `CondItem.jumpFalse`). Total: a static verdict emits nothing.
    fn jump_false(&mut self, c: CondItem) -> Option<Label> {
        if c.is_true() {
            return None; // never false
        }
        if c.is_false() {
            return c.false_chain; // already all-false: residual chain, no new branch
        }
        match c.opcode {
            CondOp::Test(op) => {
                let f = self.emit_test_branch(negate_op(op));
                self.merge_chains(c.false_chain, Some(f))
            }
            // dontgoto with a live true_chain (`q || false`): the false path is an
            // unconditional jump.
            CondOp::DontGoto => {
                debug_assert_eq!(self.cur, 0, "jump_false goto with non-empty stack");
                let g = self.branch_to_new(GOTO);
                self.merge_chains(c.false_chain, Some(g))
            }
            // goto with a live false_chain (`q && true`, `a && (b||true)`): the
            // false path is exactly that chain; emit nothing.
            CondOp::Goto => c.false_chain,
        }
    }

    /// Emit the branch that routes the TRUE outcome of `c` to a chain, returning
    /// it (javac's `CondItem.jumpTrue`). Total: a static verdict emits nothing.
    fn jump_true(&mut self, c: CondItem) -> Option<Label> {
        if c.is_false() {
            return None; // never true
        }
        if c.is_true() {
            return c.true_chain;
        }
        match c.opcode {
            CondOp::Test(op) => {
                let t = self.emit_test_branch(op);
                self.merge_chains(c.true_chain, Some(t))
            }
            CondOp::Goto => {
                debug_assert_eq!(self.cur, 0, "jump_true goto with non-empty stack");
                let g = self.branch_to_new(GOTO);
                self.merge_chains(c.true_chain, Some(g))
            }
            CondOp::DontGoto => c.true_chain,
        }
    }

    /// Materialize a boolean expression as a 0/1 on the stack. The general case is
    /// the true-first diamond `iconst_1; goto Lm; Lf: iconst_0; Lm:` over
    /// `gen_cond`'s pending branch; a bare value is already on the stack (no
    /// diamond); a statically-decided item with a residual branch resolves that
    /// branch then loads the constant `iconst_0`/`iconst_1`. Only supported with an
    /// empty base operand stack (the non-empty case needs full_frames — a later
    /// rung). Codegen preflight rejects that shape, leaving this assert as an
    /// invariant guard.
    fn gen_bool_value(&mut self, cond: ExprId) -> PrimitiveType {
        assert!(
            self.cur == 0,
            "materialized boolean with non-empty operand stack is unsupported"
        );
        let c = self.gen_cond(cond);

        // A bare boolean value already sits on the stack as 0/1, un-branched, so it
        // needs no materialization diamond. Every discriminator is carried by the
        // lowered item itself: negation clears stack reuse, grouping and crossed
        // joins require a diamond, and live chains exclude straight-line reuse.
        if c.stack_reuse
            && c.true_chain.is_none()
            && c.false_chain.is_none()
            && matches!(c.opcode, CondOp::Test(_))
            && c.origin == CondOrigin::Ordinary
            && c.materialization == Materialization::BareAllowed
        {
            return PrimitiveType::Boolean;
        }

        let is_false = c.is_false();
        let is_true = c.is_true();
        let true_chain = c.true_chain;
        let fj = self.jump_false(c);

        if is_false {
            // `q && false`: the residual false branch is already emitted; resolve
            // it here, the value is always 0.
            self.resolve_chain(fj);
            self.emit_op(ICONST_0);
        } else if is_true {
            // `q || true`: statically true with a residual true branch; resolve it,
            // the value is always 1.
            self.resolve_chain(true_chain);
            self.emit_op(ICONST_1);
        } else {
            // General true-first diamond.
            self.resolve_chain(true_chain); // true-entry (frame iff a branch lands)
            self.emit_op(ICONST_1);
            let lmerge = self.branch_to_new(GOTO);
            self.resolve_chain(fj);
            self.cur = 0; // the iconst_1 lives only on the fall-through path
            self.emit_op(ICONST_0);
            self.place_label(lmerge);
            self.add_frame(vec![VerificationType::Integer]);
        }
        PrimitiveType::Boolean
    }

    /// Emit branch opcode `op` to a fresh label and return it as a one-site chain.
    fn branch_to_new(&mut self, op: u8) -> Label {
        let l = self.new_label();
        self.emit_branch_op(op, l);
        l
    }

    /// Emit a conditional *test* branch to a fresh chain and pop its operands (2
    /// for `if_icmp<cond>`, 1 for `if<cond>`/`ifne`/`ifeq`). `GOTO` must NOT route
    /// through here (it pops nothing).
    fn emit_test_branch(&mut self, op: u8) -> Label {
        self.branch_to_new(op)
    }

    /// Merge chain `b` into chain `a` (javac's `Code.mergeChains`): retarget every
    /// pending branch of `b` to `a`. Instruction order never affects output — all
    /// sites of a merged chain resolve to one position, and frames key by layout pc.
    fn merge_chains(&mut self, a: Option<Label>, b: Option<Label>) -> Option<Label> {
        match (a, b) {
            (None, x) | (x, None) => x,
            (Some(a), Some(b)) => {
                for entry in &mut self.instructions {
                    if let Instruction::Branch { target, .. } = &mut entry.instruction {
                        if *target == b {
                            *target = a;
                        }
                    }
                }
                Some(a)
            }
        }
    }

    /// Resolve a chain at the current instruction boundary: place its label and
    /// request a stack-map
    /// frame — but only when a branch actually targets it (a `Some` chain always
    /// has at least one live branch; `None` resolves to nothing, no frame).
    fn resolve_chain(&mut self, chain: Option<Label>) {
        debug_assert_eq!(self.cur, 0, "chain resolved with non-empty operand stack");
        if let Some(l) = chain {
            self.place_label(l);
            self.add_frame(Vec::new());
        }
    }

    /// Replace the source line waiting to attach to the next real instruction.
    /// This mirrors javac's pending-stat-position model: a code-free construct's
    /// line survives only if no later source position is marked before emission.
    fn mark_line(&mut self, line: u16) {
        self.pending_line = Some(line);
    }

    /// Emit one fixed, operand-free instruction through the physical chokepoint.
    fn emit_op(&mut self, opcode: u8) {
        self.emitter.emit(Instruction::Simple(opcode));
    }

    /// Reserve a fresh, not-yet-placed label.
    fn new_label(&mut self) -> Label {
        self.emitter.new_label()
    }

    /// Bind a label to the current symbolic instruction boundary.
    fn place_label(&mut self, label: Label) {
        self.emitter.place_label(label);
    }

    /// Emit a branch whose target remains symbolic until final layout.
    fn emit_branch_op(&mut self, opcode: u8, label: Label) {
        self.emitter.emit_branch(opcode, label);
    }

    /// Request a stack-map frame at the current instruction boundary, capturing
    /// the live-locals snapshot and the given operand-stack state.
    fn add_frame(&mut self, stack: Vec<VerificationType>) {
        self.at_control_entry = true;
        self.emitter.frames.push(FrameReq {
            position: self.emitter.position(),
            locals: verification_locals(self.semantic_locals),
            stack,
        });
    }

    fn install_stmt_entry(&mut self, span: Span) {
        self.semantic_locals = self.info.stmt_entry_frame_locals(span);
    }

    fn install_stmt_exit(&mut self, span: Span) {
        self.semantic_locals = self.info.stmt_exit_frame_locals(span);
    }

    // -------- statements --------

    /// `System.out.println(arg)`.
    fn gen_expr_stmt(&mut self, expr: ExprId) {
        match &self.exprs[expr] {
            ExprKind::Println(arg) => self.gen_println(*arg),
            other => panic!("unsupported expression statement: {other:?}"),
        }
    }

    fn gen_println(&mut self, arg: ExprId) {
        let field = self
            .cp
            .fieldref("java/lang/System", "out", "Ljava/io/PrintStream;");
        self.emitter.emit(Instruction::Field {
            opcode: GETSTATIC,
            index: field,
            push_words: 1,
        });

        let ty = self.gen_value(arg);
        let desc = match ty.as_primitive() {
            Some(PrimitiveType::Int | PrimitiveType::Byte | PrimitiveType::Short) => "(I)V",
            Some(PrimitiveType::Long) => "(J)V",
            Some(PrimitiveType::Float) => "(F)V",
            Some(PrimitiveType::Double) => "(D)V",
            Some(PrimitiveType::Char) => "(C)V",
            Some(PrimitiveType::Boolean) => "(Z)V",
            None if ty.is_string() => "(Ljava/lang/String;)V",
            None => unreachable!("unsupported println reference type"),
        };
        let method = self.cp.methodref("java/io/PrintStream", "println", desc);
        self.emitter.emit(Instruction::Invoke {
            opcode: INVOKEVIRTUAL,
            index: method,
            argument_words: ty.width(),
            return_words: 0,
        });
    }

    /// Assign `value` into local `name`, coercing to the local's declared type.
    fn store_to(&mut self, name: &Name, value: ExprId) {
        let target = self.info.ty(name);
        let slot = self.info.slot(name);
        self.gen_coerced(value, target);
        self.emit_store(slot, target);
    }

    /// Compound assignment `name op= value` (also `++`/`--`, which arrive as
    /// `op ∈ {Add,Sub}` with `value == 1`).
    fn gen_compound(&mut self, name: &Name, op: BinOp, value: ExprId) {
        let target = self.info.ty(name);
        let slot = self.info.slot(name);

        // iinc fast path: an `int` target, `+=`/`-=`, an int-family constant delta
        // that keeps the expression in `int`, and a slot/delta that fits.
        if target == PrimitiveType::Int
            && matches!(op, BinOp::Add | BinOp::Sub)
            && matches!(
                sema::type_of(value, self.info).as_primitive(),
                Some(
                    PrimitiveType::Int
                        | PrimitiveType::Byte
                        | PrimitiveType::Short
                        | PrimitiveType::Char
                )
            )
        {
            if let Some(c) = fold(self.exprs, value) {
                let k = to_i32(c);
                let delta = if op == BinOp::Add {
                    k
                } else {
                    k.wrapping_neg()
                };
                if slot <= 0xff && (-128..=127).contains(&delta) {
                    self.emitter.emit(Instruction::Iinc {
                        slot: slot as u8,
                        delta: delta as i8,
                    });
                    return;
                } else if (-32768..=32767).contains(&delta) {
                    self.emitter.emit(Instruction::WideIinc {
                        slot,
                        delta: delta as i16,
                    });
                    return;
                } else {
                    // Constant delta overflowing iinc_w: javac emits the POSITIVE
                    // magnitude and chooses the operator by the delta's sign, so
                    // `x -= -32768` becomes `iload; ldc 32768; iadd; istore` (not
                    // `sipush -32768; isub`) and `x += -40000` becomes `… isub`.
                    // (This also lets `+= n` and `-= -n` share one pool entry.)
                    self.emit_load(slot, PrimitiveType::Int);
                    let (mag, add) = int_delta_magnitude(delta);
                    self.emit_int_const(mag);
                    self.emit_op(if add { IADD } else { ISUB });
                    self.emit_store(slot, PrimitiveType::Int);
                    return;
                }
            }
        }

        // General form: name = (target)(name op value), computed in the promoted
        // type `p`, then narrowed back to `target`.
        let p = if op.is_shift() {
            sema::unary_promote(target)
        } else {
            sema::binary_promote(target, sema::type_of(value, self.info).primitive())
        };
        self.emit_load(slot, target);
        self.emit_convert(target, p);
        if op.is_shift() {
            self.gen_shift_distance(value);
            self.emit_shift(p, op);
        } else if let Some(delta) = int_additive_const_delta(self.exprs, op, p, value) {
            // javac normalizes an additive *constant* on an int-family target to a
            // non-negative magnitude, choosing the operator by the delta's sign — so
            // `char v -= -100` is `bipush 100; iadd` (then i2c), never `bipush -100;
            // isub`. Same split as the iinc-overflow path above; int-family only
            // (a `long`/`float`/`double` target keeps the raw `lsub`/`dsub`/`fsub`).
            let (mag, add) = int_delta_magnitude(delta);
            self.emit_int_const(mag);
            self.emit_op(if add { IADD } else { ISUB });
        } else {
            self.gen_promoted_operand(value, p);
            self.emit_binop(p, op);
        }
        self.emit_convert(p, target);
        self.emit_store(slot, target);
    }

    // -------- expression values --------

    /// Emit `value` coerced to `target` (assignment / initializer context): a
    /// constant is folded straight to a `target`-typed constant (no conversion
    /// opcode); a non-constant is emitted then widened.
    fn gen_coerced(&mut self, value: ExprId, target: PrimitiveType) {
        if target == PrimitiveType::Boolean && sema::type_of(value, self.info).is_boolean() {
            self.gen_bool_value(value);
            return;
        }
        if let Some(c) = fold(self.exprs, value) {
            self.load_const(const_convert(c, target), target);
        } else {
            let s = self.gen_nonconst(value);
            self.emit_convert(s, target);
        }
    }

    /// Emit `expr` leaving its natural-typed value on the stack; returns the type.
    fn gen_value(&mut self, expr: ExprId) -> Type {
        // Value-mode parentheses are transparent. Handle them before the
        // primitive-only path so a parenthesized String literal keeps its class
        // type instead of being projected to `PrimitiveType`.
        if let ExprKind::Paren(inner) = &self.exprs[expr] {
            return self.gen_value(*inner);
        }
        // A string literal is the one non-numeric value form (only ever a
        // `println` argument); it loads via `ldc` of a `String` constant.
        if let ExprKind::StringLit(s) = &self.exprs[expr] {
            let idx = self.cp.string(s);
            self.emit_ldc(idx);
            return Type::string();
        }
        if let Some(c) = fold(self.exprs, expr) {
            let t = sema::type_of(expr, self.info);
            let primitive = t.primitive();
            self.load_const(const_convert(c, primitive), primitive);
            t.clone()
        } else {
            self.gen_nonconst(expr).into()
        }
    }

    /// Emit `expr` as an operand of a binary op whose promoted type is `p`,
    /// widening to `p`. A constant is loaded already in `p`; a non-constant is
    /// emitted in its own type then converted.
    fn gen_promoted_operand(&mut self, expr: ExprId, p: PrimitiveType) {
        if let Some(c) = fold(self.exprs, expr) {
            self.load_const(const_convert(c, p), p);
        } else {
            let s = self.gen_nonconst(expr);
            self.emit_convert(s, p);
        }
    }

    /// Emit a non-constant expression, returning its static type.
    fn gen_nonconst(&mut self, expr: ExprId) -> PrimitiveType {
        let exprs = self.exprs;
        match &exprs[expr] {
            ExprKind::Name(n) => {
                let ty = self.info.ty(n);
                self.emit_load(self.info.slot(n), ty);
                ty
            }
            ExprKind::Neg(e) => {
                self.gen_value(*e);
                let p = sema::unary_promote(sema::type_of(*e, self.info).primitive());
                self.emit_op(neg_op(p.stack()));
                p
            }
            ExprKind::BitNot(e) => {
                self.gen_value(*e);
                let p = sema::unary_promote(sema::type_of(*e, self.info).primitive());
                self.emit_bitnot(p);
                p
            }
            ExprKind::Paren(e) => self.gen_value(*e).primitive(),
            ExprKind::Cast { ty, expr } => {
                let s = self.gen_value(*expr).primitive();
                let target = ty.primitive();
                self.emit_convert(s, target);
                target
            }
            ExprKind::Binary { op, left, right } => self.gen_binary(*op, *left, *right),
            ExprKind::Compare { .. } | ExprKind::Not(_) | ExprKind::Logical { .. } => {
                self.gen_bool_value(expr)
            }
            other => panic!("not a value expression: {other:?}"),
        }
    }

    /// Emit a shift *distance* (a shift's right operand), which the JVM always
    /// consumes as an `int`. javac narrows a *constant* distance to an int constant at
    /// compile time (`x << 40L` → `bipush 40`, not `ldc2_w 40l; l2i`); only a
    /// non-constant `long` distance keeps the runtime `l2i`.
    fn gen_shift_distance(&mut self, right: ExprId) {
        if let Some(c) = fold(self.exprs, right) {
            self.emit_int_const(to_i32(c)); // (int) narrowing of the constant
        } else {
            let at = self.gen_value(right);
            if at.primitive().stack() == StackTy::Long {
                self.emit_op(L2I);
            }
        }
    }

    fn gen_binary(&mut self, op: BinOp, left: ExprId, right: ExprId) -> PrimitiveType {
        let lt = sema::type_of(left, self.info).primitive();
        let rt = sema::type_of(right, self.info).primitive();

        // `&`/`|`/`^` on two booleans: int opcode, boolean result.
        if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor)
            && lt == PrimitiveType::Boolean
            && rt == PrimitiveType::Boolean
        {
            self.gen_value(left);
            self.gen_value(right);
            self.emit_binop(PrimitiveType::Int, op);
            return PrimitiveType::Boolean;
        }

        if op.is_shift() {
            let result = sema::unary_promote(lt);
            self.gen_promoted_operand(left, result);
            self.gen_shift_distance(right);
            self.emit_shift(result, op);
            result
        } else {
            let p = sema::binary_promote(lt, rt);
            self.gen_promoted_operand(left, p);
            self.gen_promoted_operand(right, p);
            self.emit_binop(p, op);
            p
        }
    }

    // -------- emitters --------

    /// Load a constant already in family `ty` onto the stack.
    fn load_const(&mut self, c: Const, ty: PrimitiveType) {
        match ty.stack() {
            StackTy::Int => self.emit_int_const(to_i32(c)),
            StackTy::Long => self.emit_long_const(to_i64(c)),
            StackTy::Float => self.emit_float_const(to_f32(c)),
            StackTy::Double => self.emit_double_const(to_f64(c)),
        }
    }

    /// Load an `int` constant with the tightest opcode javac would choose.
    fn emit_int_const(&mut self, v: i32) {
        match v {
            -1 => self.emit_op(ICONST_M1),
            0..=5 => self.emit_op(ICONST_0 + v as u8),
            -128..=127 => {
                self.emitter.emit(Instruction::U8 {
                    opcode: BIPUSH,
                    operand: v as u8,
                });
            }
            -32768..=32767 => {
                self.emitter.emit(Instruction::U16 {
                    opcode: SIPUSH,
                    operand: v as u16,
                });
            }
            _ => {
                let idx = self.cp.integer(v);
                self.emit_ldc(idx);
            }
        }
    }

    fn emit_long_const(&mut self, v: i64) {
        match v {
            0 => self.emit_op(LCONST_0),
            1 => self.emit_op(LCONST_1),
            _ => {
                let idx = self.cp.long(v);
                self.emitter.emit(Instruction::U16 {
                    opcode: LDC2_W,
                    operand: idx,
                });
            }
        }
    }

    fn emit_float_const(&mut self, v: f32) {
        // Compare by bit pattern: only +0.0f/+1.0f/+2.0f get the const opcodes,
        // so -0.0f (and NaN) fall through to the pool.
        match v.to_bits() {
            b if b == 0.0f32.to_bits() => self.emit_op(FCONST_0),
            b if b == 1.0f32.to_bits() => self.emit_op(FCONST_1),
            b if b == 2.0f32.to_bits() => self.emit_op(FCONST_2),
            _ => {
                let idx = self.cp.float(v);
                self.emit_ldc(idx);
            }
        }
    }

    fn emit_double_const(&mut self, v: f64) {
        match v.to_bits() {
            b if b == 0.0f64.to_bits() => self.emit_op(DCONST_0),
            b if b == 1.0f64.to_bits() => self.emit_op(DCONST_1),
            _ => {
                let idx = self.cp.double(v);
                self.emitter.emit(Instruction::U16 {
                    opcode: LDC2_W,
                    operand: idx,
                });
            }
        }
    }

    /// `ldc`/`ldc_w` of a single-word pool entry (Integer/Float/String).
    fn emit_ldc(&mut self, idx: u16) {
        if idx <= 0xff {
            self.emitter.emit(Instruction::U8 {
                opcode: LDC,
                operand: idx as u8,
            });
        } else {
            self.emitter.emit(Instruction::U16 {
                opcode: LDC_W,
                operand: idx,
            });
        }
    }

    fn emit_load(&mut self, slot: u16, ty: PrimitiveType) {
        let (short0, wide) = load_ops(ty);
        if slot <= 3 {
            self.emit_op(short0 + slot as u8);
        } else {
            self.emitter.emit(Instruction::U8 {
                opcode: wide,
                operand: slot as u8,
            });
        }
    }

    fn emit_store(&mut self, slot: u16, ty: PrimitiveType) {
        let (short0, wide) = store_ops(ty);
        if slot <= 3 {
            self.emit_op(short0 + slot as u8);
        } else {
            self.emitter.emit(Instruction::U8 {
                opcode: wide,
                operand: slot as u8,
            });
        }
    }

    fn emit_binop(&mut self, p: PrimitiveType, op: BinOp) {
        self.emit_op(binop_op(p.stack(), op));
    }

    fn emit_shift(&mut self, result: PrimitiveType, op: BinOp) {
        self.emit_op(shift_op(result.stack(), op));
    }

    /// `~x` == `x ^ -1`, with the `-1` loaded per the value's type.
    fn emit_bitnot(&mut self, p: PrimitiveType) {
        match p.stack() {
            StackTy::Long => {
                let idx = self.cp.long(-1);
                self.emitter.emit(Instruction::U16 {
                    opcode: LDC2_W,
                    operand: idx,
                });
                self.emit_op(LXOR);
            }
            _ => {
                self.emit_op(ICONST_M1);
                self.emit_op(IXOR);
            }
        }
    }

    /// Emit the conversion from `from` to `to`, if any, adjusting the stack.
    fn emit_convert(&mut self, from: PrimitiveType, to: PrimitiveType) {
        if from == to {
            return;
        }
        let fs = from.stack();
        if matches!(
            to,
            PrimitiveType::Byte | PrimitiveType::Short | PrimitiveType::Char
        ) {
            // Bring the value to the `int` computational type first.
            match fs {
                StackTy::Long => self.emit_op(L2I),
                StackTy::Float => self.emit_op(F2I),
                StackTy::Double => self.emit_op(D2I),
                StackTy::Int => {}
            }
            // Narrow within int-family only when `from` is wider than `to`.
            let cur_ty = if fs == StackTy::Int {
                from
            } else {
                PrimitiveType::Int
            };
            if let Some(op) = subint_narrow_op(cur_ty, to) {
                self.emit_op(op);
            }
        } else if fs != to.stack() {
            self.emit_op(cross_conv_op(fs, to.stack()));
        }
    }
}

// ---- opcode/table helpers ----

/// Stack effect for every currently emitted fixed-effect opcode. Descriptor-
/// dependent field/invoke instructions, branches, and `iinc` use typed variants
/// instead and never enter this table.
fn fixed_stack_effect(opcode: u8) -> StackEffect {
    let short = |base: u8| (base..=base + 3).contains(&opcode);
    match opcode {
        ICONST_M1..=0x08 | FCONST_0..=FCONST_2 | BIPUSH | SIPUSH | LDC | LDC_W => {
            StackEffect::new(0, 1)
        }
        LCONST_0..=LCONST_1 | DCONST_0..=DCONST_1 | LDC2_W => StackEffect::new(0, 2),
        ILOAD | FLOAD | ALOAD_0 => StackEffect::new(0, 1),
        LLOAD | DLOAD => StackEffect::new(0, 2),
        ISTORE | FSTORE => StackEffect::new(1, 0),
        LSTORE | DSTORE => StackEffect::new(2, 0),

        IADD | ISUB | IMUL | IDIV | IREM | IAND | IOR | IXOR | FADD | FSUB | FMUL | FDIV | FREM => {
            StackEffect::new(2, 1)
        }
        LADD | LSUB | LMUL | LDIV | LREM | LAND | LOR | LXOR | DADD | DSUB | DMUL | DDIV | DREM => {
            StackEffect::new(4, 2)
        }
        INEG | FNEG => StackEffect::new(1, 1),
        LNEG | DNEG => StackEffect::new(2, 2),
        ISHL | ISHR | IUSHR => StackEffect::new(2, 1),
        LSHL | LSHR | LUSHR => StackEffect::new(3, 2),

        I2L | I2D | F2L | F2D => StackEffect::new(1, 2),
        I2F | F2I | I2B | I2C | I2S => StackEffect::new(1, 1),
        L2I | L2F | D2I | D2F => StackEffect::new(2, 1),
        L2D | D2L => StackEffect::new(2, 2),

        LCMP | DCMPL | DCMPG => StackEffect::new(4, 1),
        FCMPL | FCMPG => StackEffect::new(2, 1),
        RETURN => StackEffect::new(0, 0),

        _ if short(ILOAD_0) || short(FLOAD_0) => StackEffect::new(0, 1),
        _ if short(LLOAD_0) || short(DLOAD_0) => StackEffect::new(0, 2),
        _ if short(ISTORE_0) || short(FSTORE_0) => StackEffect::new(1, 0),
        _ if short(LSTORE_0) || short(DSTORE_0) => StackEffect::new(2, 0),
        _ => panic!("opcode has no fixed stack effect: {opcode:#x}"),
    }
}

/// (slot-0 short opcode, wide opcode) for a load of type `ty`.
fn load_ops(ty: PrimitiveType) -> (u8, u8) {
    match ty.stack() {
        StackTy::Int => (ILOAD_0, ILOAD),
        StackTy::Long => (LLOAD_0, LLOAD),
        StackTy::Float => (FLOAD_0, FLOAD),
        StackTy::Double => (DLOAD_0, DLOAD),
    }
}

fn store_ops(ty: PrimitiveType) -> (u8, u8) {
    match ty.stack() {
        StackTy::Int => (ISTORE_0, ISTORE),
        StackTy::Long => (LSTORE_0, LSTORE),
        StackTy::Float => (FSTORE_0, FSTORE),
        StackTy::Double => (DSTORE_0, DSTORE),
    }
}

fn binop_op(p: StackTy, op: BinOp) -> u8 {
    match (p, op) {
        (StackTy::Int, BinOp::Add) => IADD,
        (StackTy::Int, BinOp::Sub) => ISUB,
        (StackTy::Int, BinOp::Mul) => IMUL,
        (StackTy::Int, BinOp::Div) => IDIV,
        (StackTy::Int, BinOp::Rem) => IREM,
        (StackTy::Int, BinOp::And) => IAND,
        (StackTy::Int, BinOp::Or) => IOR,
        (StackTy::Int, BinOp::Xor) => IXOR,
        (StackTy::Long, BinOp::Add) => LADD,
        (StackTy::Long, BinOp::Sub) => LSUB,
        (StackTy::Long, BinOp::Mul) => LMUL,
        (StackTy::Long, BinOp::Div) => LDIV,
        (StackTy::Long, BinOp::Rem) => LREM,
        (StackTy::Long, BinOp::And) => LAND,
        (StackTy::Long, BinOp::Or) => LOR,
        (StackTy::Long, BinOp::Xor) => LXOR,
        (StackTy::Float, BinOp::Add) => FADD,
        (StackTy::Float, BinOp::Sub) => FSUB,
        (StackTy::Float, BinOp::Mul) => FMUL,
        (StackTy::Float, BinOp::Div) => FDIV,
        (StackTy::Float, BinOp::Rem) => FREM,
        (StackTy::Double, BinOp::Add) => DADD,
        (StackTy::Double, BinOp::Sub) => DSUB,
        (StackTy::Double, BinOp::Mul) => DMUL,
        (StackTy::Double, BinOp::Div) => DDIV,
        (StackTy::Double, BinOp::Rem) => DREM,
        (p, op) => panic!("invalid binary op {op:?} for {p:?}"),
    }
}

fn shift_op(result: StackTy, op: BinOp) -> u8 {
    match (result, op) {
        (StackTy::Int, BinOp::Shl) => ISHL,
        (StackTy::Int, BinOp::Shr) => ISHR,
        (StackTy::Int, BinOp::UShr) => IUSHR,
        (StackTy::Long, BinOp::Shl) => LSHL,
        (StackTy::Long, BinOp::Shr) => LSHR,
        (StackTy::Long, BinOp::UShr) => LUSHR,
        (r, op) => panic!("invalid shift op {op:?} for {r:?}"),
    }
}

fn neg_op(p: StackTy) -> u8 {
    match p {
        StackTy::Int => INEG,
        StackTy::Long => LNEG,
        StackTy::Float => FNEG,
        StackTy::Double => DNEG,
    }
}

/// The single conversion opcode between two *different* computational types.
fn cross_conv_op(from: StackTy, to: StackTy) -> u8 {
    use StackTy::*;
    match (from, to) {
        (Int, Long) => I2L,
        (Int, Float) => I2F,
        (Int, Double) => I2D,
        (Long, Int) => L2I,
        (Long, Float) => L2F,
        (Long, Double) => L2D,
        (Float, Int) => F2I,
        (Float, Long) => F2L,
        (Float, Double) => F2D,
        (Double, Int) => D2I,
        (Double, Long) => D2L,
        (Double, Float) => D2F,
        (a, b) => panic!("no conversion {a:?} -> {b:?}"),
    }
}

/// Two-operand int comparison branch (`if_icmp*`). With `jump_when == false` the
/// opcode is the *negation* of `op` (branch away when the comparison is false, as
/// javac emits an `if` condition); with `true` it is `op` itself.
fn int_icmp_branch(op: CmpOp, jump_when: bool) -> u8 {
    use CmpOp::*;
    match (op, jump_when) {
        (Lt, false) => IF_ICMPGE,
        (Lt, true) => IF_ICMPLT,
        (Le, false) => IF_ICMPGT,
        (Le, true) => IF_ICMPLE,
        (Gt, false) => IF_ICMPLE,
        (Gt, true) => IF_ICMPGT,
        (Ge, false) => IF_ICMPLT,
        (Ge, true) => IF_ICMPGE,
        (Eq, false) => IF_ICMPNE,
        (Eq, true) => IF_ICMPEQ,
        (Ne, false) => IF_ICMPEQ,
        (Ne, true) => IF_ICMPNE,
    }
}

/// Single-operand compare-with-zero branch (`if*`), used for `x <op> 0` and, on
/// the result of `lcmp`/`fcmp*`/`dcmp*`, for every wide-type comparison. Same
/// negation convention as [`int_icmp_branch`].
fn int_zero_branch(op: CmpOp, jump_when: bool) -> u8 {
    use CmpOp::*;
    match (op, jump_when) {
        (Lt, false) => IFGE,
        (Lt, true) => IFLT,
        (Le, false) => IFGT,
        (Le, true) => IFLE,
        (Gt, false) => IFLE,
        (Gt, true) => IFGT,
        (Ge, false) => IFLT,
        (Ge, true) => IFGE,
        (Eq, false) => IFNE,
        (Eq, true) => IFEQ,
        (Ne, false) => IFEQ,
        (Ne, true) => IFNE,
    }
}

/// Involution over the twelve conditional-branch opcodes: the branch taken when
/// the *negated* condition holds. Kept consistent with `int_icmp_branch`/
/// `int_zero_branch` by `assert_negate_op_consistent` — this is the highest-blast-
/// radius helper (a drift here silently breaks every comparison fixture), so it is
/// derived and debug-checked rather than trusted.
fn negate_op(op: u8) -> u8 {
    match op {
        IFEQ => IFNE,
        IFNE => IFEQ,
        IFLT => IFGE,
        IFGE => IFLT,
        IFGT => IFLE,
        IFLE => IFGT,
        IF_ICMPEQ => IF_ICMPNE,
        IF_ICMPNE => IF_ICMPEQ,
        IF_ICMPLT => IF_ICMPGE,
        IF_ICMPGE => IF_ICMPLT,
        IF_ICMPGT => IF_ICMPLE,
        IF_ICMPLE => IF_ICMPGT,
        other => panic!("negate_op: not a conditional branch opcode {other:#x}"),
    }
}

/// Debug guard: `negate_op` must invert both branch-opcode tables and be an
/// involution, so replacing a `(op, false)` call with `negate_op((op, true))` is
/// byte-neutral. Run once per `generate()` under `debug_assertions`.
#[cfg(debug_assertions)]
fn assert_negate_op_consistent() {
    use CmpOp::*;
    for op in [Lt, Le, Gt, Ge, Eq, Ne] {
        debug_assert_eq!(
            negate_op(int_icmp_branch(op, true)),
            int_icmp_branch(op, false)
        );
        debug_assert_eq!(
            negate_op(int_zero_branch(op, true)),
            int_zero_branch(op, false)
        );
        debug_assert_eq!(
            negate_op(negate_op(int_icmp_branch(op, true))),
            int_icmp_branch(op, true)
        );
        debug_assert_eq!(
            negate_op(negate_op(int_zero_branch(op, true))),
            int_zero_branch(op, true)
        );
    }
}

fn verification_locals(locals: &[FrameLocal]) -> Vec<VerificationType> {
    locals
        .iter()
        .map(|local| match local {
            FrameLocal::Top => VerificationType::Top,
            FrameLocal::Integer => VerificationType::Integer,
            FrameLocal::Float => VerificationType::Float,
            FrameLocal::Long => VerificationType::Long,
            FrameLocal::Double => VerificationType::Double,
            FrameLocal::Object(name) => VerificationType::Object(name.clone()),
        })
        .collect()
}

/// The `i2b`/`i2s`/`i2c` javac emits converting an int-computational value of
/// sub-int type `cur` to sub-int `to`. javac's `Items.Item.coerce` emits the
/// narrowing op for **every** sub-int target whose typecode differs from the
/// source's — `Code.truncate` collapses byte/char/short to int, so the only pair it
/// treats as already-coerced is same-typecode-to-same. That means even the
/// *widening* `byte`->`short` emits `i2s` (numerically a no-op, but javac emits it),
/// as does an implicit `short s = someByte;` assignment. `None` therefore means only
/// `cur == to` (byte->byte / short->short / char->char).
fn subint_narrow_op(cur: PrimitiveType, to: PrimitiveType) -> Option<u8> {
    if cur == to {
        return None;
    }
    match to {
        PrimitiveType::Byte => Some(I2B),
        PrimitiveType::Short => Some(I2S),
        PrimitiveType::Char => Some(I2C),
        _ => None,
    }
}

// ---- constant folding ----

/// Evaluate a maximal constant subtree to a single typed value, or `None` if any
/// leaf is a local. Uses wrapping integer arithmetic and exact IEEE-754 float
/// arithmetic with JLS shift masking, so a folded constant is bit-identical to
/// what the unfolded bytecode would compute.
fn fold(exprs: &ExprArena, expr: ExprId) -> Option<Const> {
    fold_impl(exprs, expr, false)
}

/// Return a value only when the complete subtree is available to javac lowering as
/// an immediate. Unlike `fold`, a deciding logical left operand does not hide an
/// unavailable right operand. This keeps non-strict shortcuts structural while
/// preserving javac's observed `long >>> long` non-folding exception.
fn lowering_const(exprs: &ExprArena, expr: ExprId) -> Option<Const> {
    fold_impl(exprs, expr, true)
}

fn fold_impl(exprs: &ExprArena, expr: ExprId, strict_logical: bool) -> Option<Const> {
    Some(match &exprs[expr] {
        ExprKind::IntLit(v) => Const::Int(*v),
        ExprKind::LongLit(v) => Const::Long(*v),
        ExprKind::FloatLit(v) => Const::Float(*v),
        ExprKind::DoubleLit(v) => Const::Double(*v),
        ExprKind::BoolLit(b) => Const::Int(*b as i32),
        ExprKind::CharLit(v) => Const::Int(*v as i32),
        ExprKind::StringLit(_) | ExprKind::Name(_) | ExprKind::Println(_) => return None,
        ExprKind::Neg(e) => neg_const(fold_impl(exprs, *e, strict_logical)?),
        ExprKind::BitNot(e) => bitnot_const(fold_impl(exprs, *e, strict_logical)?),
        ExprKind::Not(e) => Const::Int((to_i32(fold_impl(exprs, *e, strict_logical)?) == 0) as i32),
        ExprKind::Paren(e) => fold_impl(exprs, *e, strict_logical)?,
        ExprKind::Cast { ty, expr } => {
            const_convert(fold_impl(exprs, *expr, strict_logical)?, ty.primitive())
        }
        ExprKind::Binary { op, left, right } => {
            let (l, r) = (
                fold_impl(exprs, *left, strict_logical)?,
                fold_impl(exprs, *right, strict_logical)?,
            );
            // javac's ConstFold folds *every* shift except `long >>> long` (unsigned
            // shift, both operands `long`) — a genuine javac quirk. Returning None
            // there forces the runtime `lushr` (with the distance narrowed by
            // `gen_shift_distance`), matching javac byte-for-byte.
            if *op == BinOp::UShr && matches!(l, Const::Long(_)) && matches!(r, Const::Long(_)) {
                return None;
            }
            eval_binary(*op, l, r)
        }
        ExprKind::Compare { op, left, right } => Const::Int(eval_compare(
            *op,
            fold_impl(exprs, *left, strict_logical)?,
            fold_impl(exprs, *right, strict_logical)?,
        ) as i32),
        // `&&`/`||` are constant only via short-circuit from the LEFT. A non-constant
        // left means the whole is NOT a compile-time constant even when the tree is
        // statically decided (`q && false`) — the left must still be evaluated, so we
        // return `None` and let `gen_cond` emit it. When the left decides, return its
        // verdict WITHOUT folding the right; otherwise the tree reduces to the right.
        ExprKind::Logical { op, left, right } => {
            let lb = to_i32(fold_impl(exprs, *left, strict_logical)?) != 0;
            if strict_logical {
                let rb = to_i32(fold_impl(exprs, *right, true)?) != 0;
                return Some(Const::Int(match op {
                    LogOp::And => (lb && rb) as i32,
                    LogOp::Or => (lb || rb) as i32,
                }));
            }
            match op {
                LogOp::And if !lb => Const::Int(0), // false && _ -> false
                LogOp::Or if lb => Const::Int(1),   // true  || _ -> true
                _ => Const::Int((to_i32(fold_impl(exprs, *right, false)?) != 0) as i32),
            }
        }
    })
}

/// Evaluate a constant comparison, with binary numeric promotion. Float/double
/// use IEEE ordering (a `NaN` operand makes every ordering and `==` false),
/// matching the `fcmp`/`dcmp` a non-folded comparison would run.
fn eval_compare(op: CmpOp, l: Const, r: Const) -> bool {
    match promote_const(l, r) {
        StackTy::Int => compare_vals(op, to_i32(l), to_i32(r)),
        StackTy::Long => compare_vals(op, to_i64(l), to_i64(r)),
        StackTy::Float => compare_vals(op, to_f32(l), to_f32(r)),
        StackTy::Double => compare_vals(op, to_f64(l), to_f64(r)),
    }
}

fn compare_vals<T: PartialOrd>(op: CmpOp, a: T, b: T) -> bool {
    match op {
        CmpOp::Lt => a < b,
        CmpOp::Le => a <= b,
        CmpOp::Gt => a > b,
        CmpOp::Ge => a >= b,
        CmpOp::Eq => a == b,
        CmpOp::Ne => a != b,
    }
}

fn neg_const(c: Const) -> Const {
    match c {
        Const::Int(v) => Const::Int(v.wrapping_neg()),
        Const::Long(v) => Const::Long(v.wrapping_neg()),
        Const::Float(v) => Const::Float(-v),
        Const::Double(v) => Const::Double(-v),
    }
}

fn bitnot_const(c: Const) -> Const {
    match c {
        Const::Int(v) => Const::Int(!v),
        Const::Long(v) => Const::Long(!v),
        _ => panic!("~ on a non-integral constant"),
    }
}

fn eval_binary(op: BinOp, l: Const, r: Const) -> Const {
    if op.is_shift() {
        // Shift distance masked with the JLS width; left operand keeps its type.
        return match l {
            Const::Long(v) => {
                let s = (to_i32(r) & 63) as u32;
                Const::Long(match op {
                    BinOp::Shl => v.wrapping_shl(s),
                    BinOp::Shr => v.wrapping_shr(s),
                    BinOp::UShr => ((v as u64).wrapping_shr(s)) as i64,
                    _ => unreachable!(),
                })
            }
            _ => {
                let v = to_i32(l);
                let s = (to_i32(r) & 31) as u32;
                Const::Int(match op {
                    BinOp::Shl => v.wrapping_shl(s),
                    BinOp::Shr => v.wrapping_shr(s),
                    BinOp::UShr => ((v as u32).wrapping_shr(s)) as i32,
                    _ => unreachable!(),
                })
            }
        };
    }
    match promote_const(l, r) {
        StackTy::Int => Const::Int(int_op(op, to_i32(l), to_i32(r))),
        StackTy::Long => Const::Long(long_op(op, to_i64(l), to_i64(r))),
        StackTy::Float => Const::Float(float_op(op, to_f32(l), to_f32(r))),
        StackTy::Double => Const::Double(double_op(op, to_f64(l), to_f64(r))),
    }
}

fn int_op(op: BinOp, a: i32, b: i32) -> i32 {
    match op {
        BinOp::Add => a.wrapping_add(b),
        BinOp::Sub => a.wrapping_sub(b),
        BinOp::Mul => a.wrapping_mul(b),
        BinOp::Div => a.wrapping_div(b),
        BinOp::Rem => a.wrapping_rem(b),
        BinOp::And => a & b,
        BinOp::Or => a | b,
        BinOp::Xor => a ^ b,
        _ => unreachable!("shift handled separately"),
    }
}

fn long_op(op: BinOp, a: i64, b: i64) -> i64 {
    match op {
        BinOp::Add => a.wrapping_add(b),
        BinOp::Sub => a.wrapping_sub(b),
        BinOp::Mul => a.wrapping_mul(b),
        BinOp::Div => a.wrapping_div(b),
        BinOp::Rem => a.wrapping_rem(b),
        BinOp::And => a & b,
        BinOp::Or => a | b,
        BinOp::Xor => a ^ b,
        _ => unreachable!("shift handled separately"),
    }
}

fn float_op(op: BinOp, a: f32, b: f32) -> f32 {
    match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => a / b,
        BinOp::Rem => a % b,
        _ => panic!("invalid float op {op:?}"),
    }
}

fn double_op(op: BinOp, a: f64, b: f64) -> f64 {
    match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => a / b,
        BinOp::Rem => a % b,
        _ => panic!("invalid double op {op:?}"),
    }
}

/// Binary numeric promotion at the constant level.
fn promote_const(l: Const, r: Const) -> StackTy {
    let rank = |c: &Const| match c {
        Const::Int(_) => 0,
        Const::Long(_) => 1,
        Const::Float(_) => 2,
        Const::Double(_) => 3,
    };
    match rank(&l).max(rank(&r)) {
        0 => StackTy::Int,
        1 => StackTy::Long,
        2 => StackTy::Float,
        _ => StackTy::Double,
    }
}

/// Convert a constant to the value it becomes when cast/assigned to `to`, using
/// Java's narrowing/widening semantics (Rust `as` matches JVM `d2i`/`l2i`/etc.).
fn const_convert(c: Const, to: PrimitiveType) -> Const {
    match to {
        PrimitiveType::Int | PrimitiveType::Boolean => Const::Int(to_i32(c)),
        PrimitiveType::Long => Const::Long(to_i64(c)),
        PrimitiveType::Float => Const::Float(to_f32(c)),
        PrimitiveType::Double => Const::Double(to_f64(c)),
        PrimitiveType::Byte => Const::Int((to_i32(c) as i8) as i32),
        PrimitiveType::Short => Const::Int((to_i32(c) as i16) as i32),
        PrimitiveType::Char => Const::Int((to_i32(c) as u16) as i32),
    }
}

/// The signed increment of an int-family additive compound-assign with a *constant*
/// RHS (`+= k` → `k`, `-= k` → `-k`), or `None` when javac's magnitude normalization
/// does not apply: a non-int-family promoted type (`long`/`float`/`double` keep the
/// raw `lsub`/…), a non-additive op, or a non-constant RHS.
fn int_additive_const_delta(
    exprs: &ExprArena,
    op: BinOp,
    p: PrimitiveType,
    value: ExprId,
) -> Option<i32> {
    if p.stack() != StackTy::Int || !matches!(op, BinOp::Add | BinOp::Sub) {
        return None;
    }
    let k = to_i32(fold(exprs, value)?);
    Some(if op == BinOp::Add {
        k
    } else {
        k.wrapping_neg()
    })
}

/// javac loads an int increment as a non-negative magnitude and picks the operator by
/// sign: `(|delta|, is_add)` — `iadd` for `delta ≥ 0`, `isub` for `delta < 0`. Every
/// negative delta uses `isub`, *including* `i32::MIN`: its magnitude is unrepresentable
/// so `wrapping_neg` returns `i32::MIN` itself, pushed as `-2147483648` with `isub`
/// (verified — javac emits `isub` for `x += i32::MIN` too, since `x + MIN == x - MIN`).
fn int_delta_magnitude(delta: i32) -> (i32, bool) {
    if delta >= 0 {
        (delta, true)
    } else {
        (delta.wrapping_neg(), false)
    }
}

fn to_i32(c: Const) -> i32 {
    match c {
        Const::Int(v) => v,
        Const::Long(v) => v as i32,
        Const::Float(v) => v as i32,
        Const::Double(v) => v as i32,
    }
}
fn to_i64(c: Const) -> i64 {
    match c {
        Const::Int(v) => v as i64,
        Const::Long(v) => v,
        Const::Float(v) => v as i64,
        Const::Double(v) => v as i64,
    }
}
fn to_f32(c: Const) -> f32 {
    match c {
        Const::Int(v) => v as f32,
        Const::Long(v) => v as f32,
        Const::Float(v) => v,
        Const::Double(v) => v as f32,
    }
}
fn to_f64(c: Const) -> f64 {
    match c {
        Const::Int(v) => v as f64,
        Const::Long(v) => v as f64,
        Const::Float(v) => v as f64,
        Const::Double(v) => v,
    }
}

fn push_u16(code: &mut Vec<u8>, v: u16) {
    code.extend_from_slice(&v.to_be_bytes());
}
