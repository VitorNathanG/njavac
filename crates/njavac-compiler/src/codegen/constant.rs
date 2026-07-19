use super::stack::StackTy;
use crate::ast::{BinOp, CmpOp, ExprArena, ExprId, ExprKind, LogOp, PrimitiveType};

/// A compile-time constant value in one of the four JVM computational types.
/// `boolean`/`char` fold into `Int` (their code-point / 0-1 value).
#[derive(Clone, Copy)]
pub(super) enum Const {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
}

// ---- constant folding ----

/// Evaluate a maximal constant subtree to a single typed value, or `None` if any
/// leaf is a local. Uses wrapping integer arithmetic and exact IEEE-754 float
/// arithmetic with JLS shift masking, so a folded constant is bit-identical to
/// what the unfolded bytecode would compute.
pub(super) fn fold(exprs: &ExprArena, expr: ExprId) -> Option<Const> {
    fold_impl(exprs, expr, false)
}

/// Return a value only when the complete subtree is available to current lowering
/// as an immediate. Unlike `fold`, a deciding logical left operand does not hide
/// an unavailable right operand. This keeps non-strict shortcuts structural while
/// preserving the pinned output's observed `long >>> long` non-folding case.
pub(super) fn lowering_const(exprs: &ExprArena, expr: ExprId) -> Option<Const> {
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
        ExprKind::StringLit(_)
        | ExprKind::Name(_)
        | ExprKind::Select { .. }
        | ExprKind::Call { .. } => return None,
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
            // Pinned black-box output leaves constant `long >>> long` unfolded,
            // while the probed sibling shift forms fold. Returning None forces the
            // runtime `lushr`, with the distance narrowed by
            // `gen_shift_distance`, to match those observations.
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
pub(super) fn const_convert(c: Const, to: PrimitiveType) -> Const {
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
/// RHS (`+= k` → `k`, `-= k` → `-k`), or `None` when the observed magnitude normalization
/// does not apply: a non-int-family promoted type (`long`/`float`/`double` keep the
/// raw `lsub`/…), a non-additive op, or a non-constant RHS.
pub(super) fn int_additive_const_delta(
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

/// Pinned probes load an int increment as a non-negative magnitude and select the
/// operator by sign: `(|delta|, is_add)` uses `iadd` for `delta >= 0` and `isub`
/// for the probed negative cases, including `i32::MIN`. Its magnitude is
/// unrepresentable, so `wrapping_neg` returns `i32::MIN` itself, pushed as
/// `-2147483648` with `isub`; modulo arithmetic makes `x + MIN == x - MIN`.
pub(super) fn int_delta_magnitude(delta: i32) -> (i32, bool) {
    if delta >= 0 {
        (delta, true)
    } else {
        (delta.wrapping_neg(), false)
    }
}

pub(super) fn to_i32(c: Const) -> i32 {
    match c {
        Const::Int(v) => v,
        Const::Long(v) => v as i32,
        Const::Float(v) => v as i32,
        Const::Double(v) => v as i32,
    }
}
pub(super) fn to_i64(c: Const) -> i64 {
    match c {
        Const::Int(v) => v as i64,
        Const::Long(v) => v,
        Const::Float(v) => v as i64,
        Const::Double(v) => v as i64,
    }
}
pub(super) fn to_f32(c: Const) -> f32 {
    match c {
        Const::Int(v) => v as f32,
        Const::Long(v) => v as f32,
        Const::Float(v) => v,
        Const::Double(v) => v as f32,
    }
}
pub(super) fn to_f64(c: Const) -> f64 {
    match c {
        Const::Int(v) => v as f64,
        Const::Long(v) => v as f64,
        Const::Float(v) => v as f64,
        Const::Double(v) => v,
    }
}
