// ---- opcodes ----
pub(super) const ICONST_M1: u8 = 0x02;
pub(super) const ICONST_0: u8 = 0x03;
pub(super) const LCONST_0: u8 = 0x09;
pub(super) const LCONST_1: u8 = 0x0a;
pub(super) const FCONST_0: u8 = 0x0b;
pub(super) const FCONST_1: u8 = 0x0c;
pub(super) const FCONST_2: u8 = 0x0d;
pub(super) const DCONST_0: u8 = 0x0e;
pub(super) const DCONST_1: u8 = 0x0f;
pub(super) const BIPUSH: u8 = 0x10;
pub(super) const SIPUSH: u8 = 0x11;
pub(super) const LDC: u8 = 0x12;
pub(super) const LDC_W: u8 = 0x13;
pub(super) const LDC2_W: u8 = 0x14;

// Loads: indexed form (opcode + 1-byte slot) and the slot-0 short form.
pub(super) const ILOAD: u8 = 0x15;
pub(super) const LLOAD: u8 = 0x16;
pub(super) const FLOAD: u8 = 0x17;
pub(super) const DLOAD: u8 = 0x18;
pub(super) const ILOAD_0: u8 = 0x1a;
pub(super) const LLOAD_0: u8 = 0x1e;
pub(super) const FLOAD_0: u8 = 0x22;
pub(super) const DLOAD_0: u8 = 0x26;
pub(super) const ALOAD_0: u8 = 0x2a;

// Stores.
pub(super) const ISTORE: u8 = 0x36;
pub(super) const LSTORE: u8 = 0x37;
pub(super) const FSTORE: u8 = 0x38;
pub(super) const DSTORE: u8 = 0x39;
pub(super) const ISTORE_0: u8 = 0x3b;
pub(super) const LSTORE_0: u8 = 0x3f;
pub(super) const FSTORE_0: u8 = 0x43;
pub(super) const DSTORE_0: u8 = 0x47;

// Arithmetic.
pub(super) const IADD: u8 = 0x60;
pub(super) const LADD: u8 = 0x61;
pub(super) const FADD: u8 = 0x62;
pub(super) const DADD: u8 = 0x63;
pub(super) const ISUB: u8 = 0x64;
pub(super) const LSUB: u8 = 0x65;
pub(super) const FSUB: u8 = 0x66;
pub(super) const DSUB: u8 = 0x67;
pub(super) const IMUL: u8 = 0x68;
pub(super) const LMUL: u8 = 0x69;
pub(super) const FMUL: u8 = 0x6a;
pub(super) const DMUL: u8 = 0x6b;
pub(super) const IDIV: u8 = 0x6c;
pub(super) const LDIV: u8 = 0x6d;
pub(super) const FDIV: u8 = 0x6e;
pub(super) const DDIV: u8 = 0x6f;
pub(super) const IREM: u8 = 0x70;
pub(super) const LREM: u8 = 0x71;
pub(super) const FREM: u8 = 0x72;
pub(super) const DREM: u8 = 0x73;
pub(super) const INEG: u8 = 0x74;
pub(super) const LNEG: u8 = 0x75;
pub(super) const FNEG: u8 = 0x76;
pub(super) const DNEG: u8 = 0x77;

// Shifts and bitwise.
pub(super) const ISHL: u8 = 0x78;
pub(super) const LSHL: u8 = 0x79;
pub(super) const ISHR: u8 = 0x7a;
pub(super) const LSHR: u8 = 0x7b;
pub(super) const IUSHR: u8 = 0x7c;
pub(super) const LUSHR: u8 = 0x7d;
pub(super) const IAND: u8 = 0x7e;
pub(super) const LAND: u8 = 0x7f;
pub(super) const IOR: u8 = 0x80;
pub(super) const LOR: u8 = 0x81;
pub(super) const IXOR: u8 = 0x82;
pub(super) const LXOR: u8 = 0x83;

// iinc + wide prefix.
pub(super) const IINC: u8 = 0x84;
pub(super) const WIDE: u8 = 0xc4;

// Conversions.
pub(super) const I2L: u8 = 0x85;
pub(super) const I2F: u8 = 0x86;
pub(super) const I2D: u8 = 0x87;
pub(super) const L2I: u8 = 0x88;
pub(super) const L2F: u8 = 0x89;
pub(super) const L2D: u8 = 0x8a;
pub(super) const F2I: u8 = 0x8b;
pub(super) const F2L: u8 = 0x8c;
pub(super) const F2D: u8 = 0x8d;
pub(super) const D2I: u8 = 0x8e;
pub(super) const D2L: u8 = 0x8f;
pub(super) const D2F: u8 = 0x90;
pub(super) const I2B: u8 = 0x91;
pub(super) const I2C: u8 = 0x92;
pub(super) const I2S: u8 = 0x93;

// Comparisons and branches.
pub(super) const LCMP: u8 = 0x94;
pub(super) const FCMPL: u8 = 0x95;
pub(super) const FCMPG: u8 = 0x96;
pub(super) const DCMPL: u8 = 0x97;
pub(super) const DCMPG: u8 = 0x98;
pub(super) const IFEQ: u8 = 0x99;
pub(super) const IFNE: u8 = 0x9a;
pub(super) const IFLT: u8 = 0x9b;
pub(super) const IFGE: u8 = 0x9c;
pub(super) const IFGT: u8 = 0x9d;
pub(super) const IFLE: u8 = 0x9e;
pub(super) const IF_ICMPEQ: u8 = 0x9f;
pub(super) const IF_ICMPNE: u8 = 0xa0;
pub(super) const IF_ICMPLT: u8 = 0xa1;
pub(super) const IF_ICMPGE: u8 = 0xa2;
pub(super) const IF_ICMPGT: u8 = 0xa3;
pub(super) const IF_ICMPLE: u8 = 0xa4;
pub(super) const GOTO: u8 = 0xa7;
pub(super) const GOTO_W: u8 = 0xc8;

pub(super) const ICONST_1: u8 = 0x04;

pub(super) const GETSTATIC: u8 = 0xb2;
pub(super) const INVOKEVIRTUAL: u8 = 0xb6;
pub(super) const INVOKESPECIAL: u8 = 0xb7;
pub(super) const RETURN: u8 = 0xb1;

#[derive(Clone, Copy)]
pub(super) struct StackEffect {
    pub(super) pop: u16,
    pub(super) push: u16,
}

impl StackEffect {
    const fn new(pop: u16, push: u16) -> Self {
        StackEffect { pop, push }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct InstructionAnchor(pub(super) usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct CodePosition(pub(super) usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Label(pub(super) usize);

/// One symbolic JVM instruction. Lowering chooses exact non-branch forms and
/// branch polarity/topology; final layout chooses the method-wide narrow or fat
/// physical branch representation required by the pinned compiler.
#[derive(Clone, Copy)]
pub(super) enum Instruction {
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
    WideLocal {
        opcode: u8,
        slot: u16,
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
    pub(super) fn stack_effect(self) -> StackEffect {
        match self {
            Instruction::Simple(opcode)
            | Instruction::U8 { opcode, .. }
            | Instruction::U16 { opcode, .. }
            | Instruction::WideLocal { opcode, .. } => fixed_stack_effect(opcode),
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

    pub(super) fn narrow_encoded_len(self) -> usize {
        match self {
            Instruction::Simple(_) => 1,
            Instruction::U8 { .. } => 2,
            Instruction::U16 { .. }
            | Instruction::Iinc { .. }
            | Instruction::Field { .. }
            | Instruction::Invoke { .. }
            | Instruction::Branch { .. } => 3,
            Instruction::WideLocal { .. } => 4,
            Instruction::WideIinc { .. } => 6,
        }
    }

    pub(super) fn is_goto(self) -> bool {
        matches!(self, Instruction::Branch { opcode: GOTO, .. })
    }

    pub(super) fn is_cond_branch(self) -> bool {
        matches!(self, Instruction::Branch { opcode, .. } if is_cond_branch_opcode(opcode))
    }

    pub(super) fn is_return(self) -> bool {
        matches!(self, Instruction::Simple(RETURN))
    }
}

pub(super) fn is_cond_branch_opcode(opcode: u8) -> bool {
    (IFEQ..=IF_ICMPLE).contains(&opcode)
}

/// Invert one JVM conditional branch. Fat branch encoding uses the inverse to
/// skip its following `goto_w`; lowering uses the same table for source-level
/// condition polarity.
pub(super) fn negate_conditional(opcode: u8) -> u8 {
    match opcode {
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
        other => panic!("not a conditional branch opcode: {other:#x}"),
    }
}

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
