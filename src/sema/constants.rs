use crate::ast::{BinOp, ExprArena, ExprId, ExprKind, PrimitiveType};

/// A syntax-only approximation used for assignment conversion. It recognizes the
/// constant-expression shape but does not prove that a folded value fits a narrow
/// assignment target; callers must not treat it as a complete JLS range check.
pub(super) fn is_constant_expression(exprs: &ExprArena, expr: ExprId) -> bool {
    match &exprs[expr] {
        ExprKind::IntLit(_)
        | ExprKind::LongLit(_)
        | ExprKind::FloatLit(_)
        | ExprKind::DoubleLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::CharLit(_)
        | ExprKind::StringLit(_) => true,
        ExprKind::Neg(inner)
        | ExprKind::BitNot(inner)
        | ExprKind::Not(inner)
        | ExprKind::Paren(inner) => is_constant_expression(exprs, *inner),
        ExprKind::Cast { expr, .. } => is_constant_expression(exprs, *expr),
        ExprKind::Binary { left, right, .. }
        | ExprKind::Compare { left, right, .. }
        | ExprKind::Logical { left, right, .. } => {
            is_constant_expression(exprs, *left) && is_constant_expression(exprs, *right)
        }
        ExprKind::Name(_) | ExprKind::Select { .. } | ExprKind::Call { .. } => false,
    }
}

/// Numeric constant evaluation needed only to identify an integral zero divisor.
/// It mirrors the folding arithmetic that can reach codegen's integer `/` and `%`.
#[derive(Clone, Copy)]
pub(super) enum NumericConst {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
}

impl NumericConst {
    pub(super) fn is_zero(self) -> bool {
        matches!(self, Self::Int(0) | Self::Long(0))
    }

    fn rank(self) -> u8 {
        match self {
            Self::Int(_) => 0,
            Self::Long(_) => 1,
            Self::Float(_) => 2,
            Self::Double(_) => 3,
        }
    }

    fn to_i32(self) -> i32 {
        match self {
            Self::Int(value) => value,
            Self::Long(value) => value as i32,
            Self::Float(value) => value as i32,
            Self::Double(value) => value as i32,
        }
    }

    fn to_i64(self) -> i64 {
        match self {
            Self::Int(value) => value as i64,
            Self::Long(value) => value,
            Self::Float(value) => value as i64,
            Self::Double(value) => value as i64,
        }
    }

    fn to_f32(self) -> f32 {
        match self {
            Self::Int(value) => value as f32,
            Self::Long(value) => value as f32,
            Self::Float(value) => value,
            Self::Double(value) => value as f32,
        }
    }

    fn to_f64(self) -> f64 {
        match self {
            Self::Int(value) => value as f64,
            Self::Long(value) => value as f64,
            Self::Float(value) => value as f64,
            Self::Double(value) => value,
        }
    }

    fn cast(self, ty: PrimitiveType) -> Option<Self> {
        Some(match ty {
            PrimitiveType::Int => Self::Int(self.to_i32()),
            PrimitiveType::Long => Self::Long(self.to_i64()),
            PrimitiveType::Float => Self::Float(self.to_f32()),
            PrimitiveType::Double => Self::Double(self.to_f64()),
            PrimitiveType::Byte => Self::Int((self.to_i32() as i8) as i32),
            PrimitiveType::Short => Self::Int((self.to_i32() as i16) as i32),
            PrimitiveType::Char => Self::Int((self.to_i32() as u16) as i32),
            PrimitiveType::Boolean => return None,
        })
    }
}

pub(super) fn eval_numeric_constant(exprs: &ExprArena, expr: ExprId) -> Option<NumericConst> {
    Some(match &exprs[expr] {
        ExprKind::IntLit(value) => NumericConst::Int(*value),
        ExprKind::LongLit(value) => NumericConst::Long(*value),
        ExprKind::FloatLit(value) => NumericConst::Float(*value),
        ExprKind::DoubleLit(value) => NumericConst::Double(*value),
        ExprKind::CharLit(value) => NumericConst::Int(*value as i32),
        ExprKind::Neg(inner) => match eval_numeric_constant(exprs, *inner)? {
            NumericConst::Int(value) => NumericConst::Int(value.wrapping_neg()),
            NumericConst::Long(value) => NumericConst::Long(value.wrapping_neg()),
            NumericConst::Float(value) => NumericConst::Float(-value),
            NumericConst::Double(value) => NumericConst::Double(-value),
        },
        ExprKind::BitNot(inner) => match eval_numeric_constant(exprs, *inner)? {
            NumericConst::Int(value) => NumericConst::Int(!value),
            NumericConst::Long(value) => NumericConst::Long(!value),
            NumericConst::Float(_) | NumericConst::Double(_) => return None,
        },
        ExprKind::Paren(inner) => eval_numeric_constant(exprs, *inner)?,
        ExprKind::Cast { ty, expr } => eval_numeric_constant(exprs, *expr)?.cast(ty.primitive())?,
        ExprKind::Binary { op, left, right } => {
            let left = eval_numeric_constant(exprs, *left)?;
            let right = eval_numeric_constant(exprs, *right)?;
            eval_numeric_binary(*op, left, right)?
        }
        ExprKind::BoolLit(_)
        | ExprKind::StringLit(_)
        | ExprKind::Name(_)
        | ExprKind::Select { .. }
        | ExprKind::Not(_)
        | ExprKind::Compare { .. }
        | ExprKind::Logical { .. }
        | ExprKind::Call { .. } => return None,
    })
}

fn eval_numeric_binary(
    op: BinOp,
    left: NumericConst,
    right: NumericConst,
) -> Option<NumericConst> {
    if op.is_shift() {
        // Codegen deliberately leaves this javac quirk unfolded, so it cannot
        // expose an integer folding panic in an enclosing division either.
        if op == BinOp::UShr
            && matches!(left, NumericConst::Long(_))
            && matches!(right, NumericConst::Long(_))
        {
            return None;
        }
        return Some(match left {
            NumericConst::Long(value) => {
                let distance = (right.to_i32() & 63) as u32;
                NumericConst::Long(match op {
                    BinOp::Shl => value.wrapping_shl(distance),
                    BinOp::Shr => value.wrapping_shr(distance),
                    BinOp::UShr => ((value as u64).wrapping_shr(distance)) as i64,
                    _ => unreachable!(),
                })
            }
            NumericConst::Int(value) => {
                let distance = (right.to_i32() & 31) as u32;
                NumericConst::Int(match op {
                    BinOp::Shl => value.wrapping_shl(distance),
                    BinOp::Shr => value.wrapping_shr(distance),
                    BinOp::UShr => ((value as u32).wrapping_shr(distance)) as i32,
                    _ => unreachable!(),
                })
            }
            NumericConst::Float(_) | NumericConst::Double(_) => return None,
        });
    }

    Some(match left.rank().max(right.rank()) {
        0 => {
            let (left, right) = (left.to_i32(), right.to_i32());
            NumericConst::Int(match op {
                BinOp::Add => left.wrapping_add(right),
                BinOp::Sub => left.wrapping_sub(right),
                BinOp::Mul => left.wrapping_mul(right),
                BinOp::Div if right != 0 => left.wrapping_div(right),
                BinOp::Rem if right != 0 => left.wrapping_rem(right),
                BinOp::And => left & right,
                BinOp::Or => left | right,
                BinOp::Xor => left ^ right,
                _ => return None,
            })
        }
        1 => {
            let (left, right) = (left.to_i64(), right.to_i64());
            NumericConst::Long(match op {
                BinOp::Add => left.wrapping_add(right),
                BinOp::Sub => left.wrapping_sub(right),
                BinOp::Mul => left.wrapping_mul(right),
                BinOp::Div if right != 0 => left.wrapping_div(right),
                BinOp::Rem if right != 0 => left.wrapping_rem(right),
                BinOp::And => left & right,
                BinOp::Or => left | right,
                BinOp::Xor => left ^ right,
                _ => return None,
            })
        }
        2 => {
            let (left, right) = (left.to_f32(), right.to_f32());
            NumericConst::Float(match op {
                BinOp::Add => left + right,
                BinOp::Sub => left - right,
                BinOp::Mul => left * right,
                BinOp::Div => left / right,
                BinOp::Rem => left % right,
                _ => return None,
            })
        }
        _ => {
            let (left, right) = (left.to_f64(), right.to_f64());
            NumericConst::Double(match op {
                BinOp::Add => left + right,
                BinOp::Sub => left - right,
                BinOp::Mul => left * right,
                BinOp::Div => left / right,
                BinOp::Rem => left % right,
                _ => return None,
            })
        }
    })
}
