use crate::ast::{BinOp, CmpOp, ExprArena, ExprId, ExprKind, LogOp, PrimitiveType};

#[derive(Clone, Copy)]
pub(super) struct BooleanFlow {
    pub when_true: bool,
    pub when_false: bool,
}

impl BooleanFlow {
    const BOTH: Self = Self {
        when_true: true,
        when_false: true,
    };

    fn exact(value: bool) -> Self {
        Self {
            when_true: value,
            when_false: !value,
        }
    }

    fn negate(self) -> Self {
        Self {
            when_true: self.when_false,
            when_false: self.when_true,
        }
    }
}

/// Compute the outcomes that semantic definite-assignment flow must consider.
/// `&&` and `||` propagate impossible outcomes structurally, while ordinary
/// boolean `&`/`|`/`^` affect flow only when the complete expression is constant.
/// `DefiniteShortCircuitPaths.java` pins the short-circuit cells; the negative
/// semantic controls pin that javac does not infer an impossible outcome from
/// `false & runtime` or `true | runtime`.
pub(super) fn boolean_flow(exprs: &ExprArena, expr: ExprId) -> BooleanFlow {
    match &exprs[expr] {
        ExprKind::BoolLit(value) => BooleanFlow::exact(*value),
        ExprKind::Not(inner) => boolean_flow(exprs, *inner).negate(),
        ExprKind::Paren(inner) => boolean_flow(exprs, *inner),
        ExprKind::Cast { ty, expr } if ty.is_boolean() => boolean_flow(exprs, *expr),
        ExprKind::Logical { op, left, right } => {
            let left = boolean_flow(exprs, *left);
            let right = boolean_flow(exprs, *right);
            match op {
                LogOp::And => BooleanFlow {
                    when_true: left.when_true && right.when_true,
                    when_false: left.when_false || (left.when_true && right.when_false),
                },
                LogOp::Or => BooleanFlow {
                    when_true: left.when_true || (left.when_false && right.when_true),
                    when_false: left.when_false && right.when_false,
                },
            }
        }
        ExprKind::Binary { .. } | ExprKind::Compare { .. } => {
            eval_boolean_constant(exprs, expr).map_or(BooleanFlow::BOTH, BooleanFlow::exact)
        }
        _ => BooleanFlow::BOTH,
    }
}

fn eval_boolean_constant(exprs: &ExprArena, expr: ExprId) -> Option<bool> {
    Some(match &exprs[expr] {
        ExprKind::BoolLit(value) => *value,
        ExprKind::Not(inner) => !eval_boolean_constant(exprs, *inner)?,
        ExprKind::Paren(inner) => eval_boolean_constant(exprs, *inner)?,
        ExprKind::Cast { ty, expr } if ty.is_boolean() => eval_boolean_constant(exprs, *expr)?,
        ExprKind::Binary { op, left, right }
            if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor) =>
        {
            let left = eval_boolean_constant(exprs, *left)?;
            let right = eval_boolean_constant(exprs, *right)?;
            match op {
                BinOp::And => left & right,
                BinOp::Or => left | right,
                BinOp::Xor => left ^ right,
                _ => unreachable!(),
            }
        }
        ExprKind::Compare { op, left, right } => {
            if let (Some(left), Some(right)) = (
                eval_numeric_constant(exprs, *left),
                eval_numeric_constant(exprs, *right),
            ) {
                compare_numeric(*op, left, right)
            } else {
                let left = eval_boolean_constant(exprs, *left)?;
                let right = eval_boolean_constant(exprs, *right)?;
                match op {
                    CmpOp::Eq => left == right,
                    CmpOp::Ne => left != right,
                    _ => return None,
                }
            }
        }
        ExprKind::Logical { op, left, right } => {
            let left = eval_boolean_constant(exprs, *left)?;
            let right = eval_boolean_constant(exprs, *right)?;
            match op {
                LogOp::And => left && right,
                LogOp::Or => left || right,
            }
        }
        _ => return None,
    })
}

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

fn compare_numeric(op: CmpOp, left: NumericConst, right: NumericConst) -> bool {
    match left.rank().max(right.rank()) {
        0 => compare_values(op, left.to_i32(), right.to_i32()),
        1 => compare_values(op, left.to_i64(), right.to_i64()),
        2 => compare_values(op, left.to_f32(), right.to_f32()),
        _ => compare_values(op, left.to_f64(), right.to_f64()),
    }
}

fn compare_values<T: PartialOrd>(op: CmpOp, left: T, right: T) -> bool {
    match op {
        CmpOp::Lt => left < right,
        CmpOp::Le => left <= right,
        CmpOp::Gt => left > right,
        CmpOp::Ge => left >= right,
        CmpOp::Eq => left == right,
        CmpOp::Ne => left != right,
    }
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
