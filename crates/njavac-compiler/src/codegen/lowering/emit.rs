use super::super::stack::StackTy;
use crate::ast::{BinOp, PrimitiveType};

use super::super::constant::*;
use super::super::instruction::*;
use super::super::ops::*;
use super::Gen;

impl Gen<'_> {
    // -------- emitters --------

    /// Load a constant already in family `ty` onto the stack.
    pub(super) fn load_const(&mut self, c: Const, ty: PrimitiveType) {
        match ty.stack() {
            StackTy::Int => self.emit_int_const(to_i32(c)),
            StackTy::Long => self.emit_long_const(to_i64(c)),
            StackTy::Float => self.emit_float_const(to_f32(c)),
            StackTy::Double => self.emit_double_const(to_f64(c)),
        }
    }

    /// Load an `int` constant with the tightest opcode javac would choose.
    pub(super) fn emit_int_const(&mut self, v: i32) {
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
    pub(super) fn emit_ldc(&mut self, idx: u16) {
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

    pub(super) fn emit_load(&mut self, slot: u16, ty: PrimitiveType) {
        let (short0, indexed) = load_ops(ty);
        self.emit_local_access(slot, short0, indexed);
    }

    pub(super) fn emit_store(&mut self, slot: u16, ty: PrimitiveType) {
        let (short0, indexed) = store_ops(ty);
        self.emit_local_access(slot, short0, indexed);
    }

    /// Select the pinned compiler's local form: short through slot 3, indexed
    /// through 255, and `wide` from 256. `WideLocalBoundary.java` covers the
    /// transition and every currently supported primitive opcode family.
    fn emit_local_access(&mut self, slot: u16, short0: u8, indexed: u8) {
        match slot {
            0..=3 => self.emit_op(short0 + slot as u8),
            4..=255 => {
                self.emitter.emit(Instruction::U8 {
                    opcode: indexed,
                    operand: slot as u8,
                });
            }
            _ => {
                self.emitter.emit(Instruction::WideLocal {
                    opcode: indexed,
                    slot,
                });
            }
        }
    }

    pub(super) fn emit_binop(&mut self, p: PrimitiveType, op: BinOp) {
        self.emit_op(binop_op(p.stack(), op));
    }

    pub(super) fn emit_shift(&mut self, result: PrimitiveType, op: BinOp) {
        self.emit_op(shift_op(result.stack(), op));
    }

    /// `~x` == `x ^ -1`, with the `-1` loaded per the value's type.
    pub(super) fn emit_bitnot(&mut self, p: PrimitiveType) {
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
    pub(super) fn emit_convert(&mut self, from: PrimitiveType, to: PrimitiveType) {
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
