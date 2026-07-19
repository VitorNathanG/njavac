use crate::ast::{BinOp, CmpOp, PrimitiveType};
use super::stack::StackTy;

use super::instruction::*;

// ---- opcode/table helpers ----

/// (slot-0 short opcode, indexed opcode) for a load of type `ty`.
pub(super) fn load_ops(ty: PrimitiveType) -> (u8, u8) {
    match ty.stack() {
        StackTy::Int => (ILOAD_0, ILOAD),
        StackTy::Long => (LLOAD_0, LLOAD),
        StackTy::Float => (FLOAD_0, FLOAD),
        StackTy::Double => (DLOAD_0, DLOAD),
    }
}

pub(super) fn store_ops(ty: PrimitiveType) -> (u8, u8) {
    match ty.stack() {
        StackTy::Int => (ISTORE_0, ISTORE),
        StackTy::Long => (LSTORE_0, LSTORE),
        StackTy::Float => (FSTORE_0, FSTORE),
        StackTy::Double => (DSTORE_0, DSTORE),
    }
}

pub(super) fn binop_op(p: StackTy, op: BinOp) -> u8 {
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

pub(super) fn shift_op(result: StackTy, op: BinOp) -> u8 {
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

pub(super) fn neg_op(p: StackTy) -> u8 {
    match p {
        StackTy::Int => INEG,
        StackTy::Long => LNEG,
        StackTy::Float => FNEG,
        StackTy::Double => DNEG,
    }
}

/// The single conversion opcode between two *different* computational types.
pub(super) fn cross_conv_op(from: StackTy, to: StackTy) -> u8 {
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
pub(super) fn int_icmp_branch(op: CmpOp, jump_when: bool) -> u8 {
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
pub(super) fn int_zero_branch(op: CmpOp, jump_when: bool) -> u8 {
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
pub(super) fn negate_op(op: u8) -> u8 {
    negate_conditional(op)
}

/// Debug guard: `negate_op` must invert both branch-opcode tables and be an
/// involution, so replacing a `(op, false)` call with `negate_op((op, true))` is
/// byte-neutral. Run once per `generate()` under `debug_assertions`.
#[cfg(debug_assertions)]
pub(super) fn assert_negate_op_consistent() {
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

/// The `i2b`/`i2s`/`i2c` observed from javac when converting an int-computational
/// value of sub-int type `cur` to `to`. The pinned output emits the target opcode
/// whenever the source and target sub-int types differ. Thus even widening
/// `byte`->`short` emits `i2s`; `None` means only `cur == to`.
pub(super) fn subint_narrow_op(cur: PrimitiveType, to: PrimitiveType) -> Option<u8> {
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
