use super::{BoolMode, Gen, CAPS};
use crate::model::{BinOp, CmpOp, FExpr, LogOp, Ty, Val};
use Ty::*;

pub(super) const NUMERIC: [Ty; 7] = [Int, Long, Float, Double, Byte, Short, Char];
pub(super) const INTEGRAL: [Ty; 5] = [Int, Long, Byte, Short, Char];
const INT_EDGES: [i32; 30] = [
    -1, 0, 1, 2, 3, 4, 5, 6, -2, 10, -10, 100, 7, 127, 128, 129, -128, -129, 255, 256, 32767,
    32768, -32768, -32769, 65535, 65536, i32::MAX, i32::MIN, i32::MAX - 1, i32::MIN + 1,
];
const LONG_EDGES: [i64; 20] = [
    -1, 0, 1, 2, 3, -2, 10, 100, 127, 128, 255, 256, i32::MAX as i64, i32::MAX as i64 + 1,
    i32::MIN as i64, i32::MIN as i64 - 1, i64::MAX, i64::MIN, i64::MAX - 1, i64::MIN + 1,
];
const FLOAT_POOL: [f32; 14] = [
    0.0, 1.0, 2.0, 3.0, 0.5, 0.25, 10.0, 100.0, -1.0, -1.5, f32::NAN, f32::INFINITY,
    f32::NEG_INFINITY, -0.0,
];
const DOUBLE_POOL: [f64; 14] = [
    0.0, 1.0, 2.0, 3.0, 0.5, 0.25, 10.0, 100.0, -1.0, -1.5, f64::NAN, f64::INFINITY,
    f64::NEG_INFINITY, -0.0,
];
const CHAR_POOL: [u16; 16] =
    [0, 1, 32, 48, 65, 97, 126, 127, 128, 255, 256, 1000, 32767, 32768, 65534, 65535];
const BYTE_LIT: [i32; 11] = [-128, -127, -1, 0, 1, 2, 63, 64, 100, 126, 127];
const SHORT_LIT: [i32; 12] = [-32768, -129, -128, -1, 0, 1, 127, 128, 255, 256, 32766, 32767];

impl Gen {
    fn numeric_upto(&mut self, r: u8) -> Ty {
        let opts: Vec<Ty> = NUMERIC.iter().copied().filter(|t| t.prank() <= r).collect();
        *self.rng.pick(&opts)
    }

    pub(super) fn local_of(&mut self, env: &[Ty], pred: impl Fn(Ty) -> bool) -> Option<usize> {
        let ids: Vec<usize> =
            env.iter().enumerate().filter(|(_, t)| pred(**t)).map(|(i, _)| i).collect();
        if ids.is_empty() { None } else { Some(*self.rng.pick(&ids)) }
    }

    pub(super) fn expr(
        &mut self,
        env: &[Ty],
        target: Ty,
        mode: BoolMode,
        budget: &mut i32,
    ) -> FExpr {
        *budget -= 1;
        if target == Boolean {
            return self.bool_expr(env, mode, budget);
        }
        if *budget <= 0 || self.rng.ratio(2, 5) {
            return self.leaf(env, target);
        }
        match target {
            Byte | Short | Char => {
                let src = *self.rng.pick(&NUMERIC);
                FExpr::Cast(target, Box::new(self.expr(env, src, BoolMode::Value, budget)))
            }
            Int | Long | Float | Double => self.numeric_compound(env, target, budget),
            Boolean => unreachable!(),
        }
    }

    fn numeric_compound(&mut self, env: &[Ty], target: Ty, budget: &mut i32) -> FExpr {
        let integral = target.is_integral();
        enum Form {
            Arith(BinOp),
            DivRem(BinOp),
            Shift(BinOp),
            Neg,
            BitNot,
            Cast,
        }
        let mut forms = vec![
            Form::Arith(BinOp::Add),
            Form::Arith(BinOp::Sub),
            Form::Arith(BinOp::Mul),
            Form::DivRem(BinOp::Div),
            Form::DivRem(BinOp::Rem),
            Form::Neg,
            Form::Cast,
        ];
        if integral {
            forms.push(Form::Arith(BinOp::BAnd));
            forms.push(Form::Arith(BinOp::BOr));
            forms.push(Form::Arith(BinOp::BXor));
            forms.push(Form::Shift(BinOp::Shl));
            forms.push(Form::Shift(BinOp::Shr));
            forms.push(Form::Shift(BinOp::Ushr));
            forms.push(Form::BitNot);
        }
        let idx = self.rng.below(forms.len());
        match &forms[idx] {
            Form::Arith(op) => {
                let other = self.numeric_upto(target.prank());
                let (a, b) = if self.rng.boolean() {
                    (
                        self.expr(env, target, BoolMode::Value, budget),
                        self.expr(env, other, BoolMode::Value, budget),
                    )
                } else {
                    (
                        self.expr(env, other, BoolMode::Value, budget),
                        self.expr(env, target, BoolMode::Value, budget),
                    )
                };
                FExpr::Bin(*op, Box::new(a), Box::new(b))
            }
            Form::DivRem(op) => {
                let a = self.expr(env, target, BoolMode::Value, budget);
                let b = if integral {
                    self.divisor(env, target)
                } else {
                    let other = self.numeric_upto(target.prank());
                    self.expr(env, other, BoolMode::Value, budget)
                };
                FExpr::Bin(*op, Box::new(a), Box::new(b))
            }
            Form::Shift(op) => {
                let a = self.expr(env, target, BoolMode::Value, budget);
                let amt_ty = *self.rng.pick(&INTEGRAL);
                let b = self.expr(env, amt_ty, BoolMode::Value, budget);
                FExpr::Bin(*op, Box::new(a), Box::new(b))
            }
            Form::Neg => FExpr::Neg(Box::new(self.expr(env, target, BoolMode::Value, budget))),
            Form::BitNot => FExpr::BitNot(Box::new(self.expr(env, target, BoolMode::Value, budget))),
            Form::Cast => {
                let src = *self.rng.pick(&NUMERIC);
                FExpr::Cast(target, Box::new(self.expr(env, src, BoolMode::Value, budget)))
            }
        }
    }

    fn divisor(&mut self, env: &[Ty], target: Ty) -> FExpr {
        let r = target.prank();
        if let Some(idx) = self.local_of(env, |t| t.is_integral() && t.prank() <= r) {
            FExpr::Local(idx)
        } else {
            let nz: Vec<i32> = INT_EDGES.iter().copied().filter(|&v| v != 0).collect();
            FExpr::Lit(Val::I(*self.rng.pick(&nz)))
        }
    }

    fn bool_expr(&mut self, env: &[Ty], mode: BoolMode, budget: &mut i32) -> FExpr {
        let value_leaf = |g: &mut Gen| -> FExpr {
            if let Some(idx) = g.local_of(env, |t| t == Boolean) {
                if g.rng.boolean() {
                    return FExpr::Local(idx);
                }
            }
            FExpr::Lit(Val::Bool(g.rng.boolean()))
        };
        if *budget <= 0 {
            return value_leaf(self);
        }
        if mode == BoolMode::Value {
            if self.rng.ratio(2, 5) {
                let op = *self.rng.pick(&[BinOp::BAnd, BinOp::BOr, BinOp::BXor]);
                let a = self.bool_expr(env, BoolMode::Value, budget);
                let b = self.bool_expr(env, BoolMode::Value, budget);
                return FExpr::Bin(op, Box::new(a), Box::new(b));
            }
            return value_leaf(self);
        }
        match self.rng.below(if CAPS.boolean_boundaries { 8 } else { 6 }) {
            0 | 1 => self.cmp_expr(env, budget),
            2 => {
                let op = *self.rng.pick(&[LogOp::And, LogOp::Or]);
                let a = self.bool_expr(env, BoolMode::Branch, budget);
                let b = self.bool_expr(env, BoolMode::Branch, budget);
                FExpr::Logic(op, Box::new(a), Box::new(b))
            }
            3 => FExpr::Not(Box::new(self.bool_expr(env, BoolMode::Branch, budget))),
            4 => {
                let op = *self.rng.pick(&[BinOp::BAnd, BinOp::BOr, BinOp::BXor]);
                let a = self.bool_expr(env, BoolMode::Value, budget);
                let b = self.bool_expr(env, BoolMode::Value, budget);
                FExpr::Bin(op, Box::new(a), Box::new(b))
            }
            5 if CAPS.boolean_boundaries => {
                FExpr::Cast(Boolean, Box::new(self.bool_expr(env, BoolMode::Branch, budget)))
            }
            6 if CAPS.boolean_boundaries => {
                FExpr::Paren(Box::new(self.bool_expr(env, BoolMode::Branch, budget)))
            }
            _ => value_leaf(self),
        }
    }

    fn cmp_expr(&mut self, env: &[Ty], budget: &mut i32) -> FExpr {
        let op = *self.rng.pick(&[
            CmpOp::Lt,
            CmpOp::Le,
            CmpOp::Gt,
            CmpOp::Ge,
            CmpOp::Eq,
            CmpOp::Ne,
        ]);
        if !op.is_order() && self.rng.ratio(1, 3) {
            let a = self.bool_expr(env, BoolMode::Value, budget);
            let b = self.bool_expr(env, BoolMode::Value, budget);
            return FExpr::Cmp(op, Box::new(a), Box::new(b));
        }
        let ct = *self.rng.pick(&NUMERIC);
        let other = self.numeric_upto(ct.prank());
        let a = self.expr(env, ct, BoolMode::Value, budget);
        let b = self.expr(env, other, BoolMode::Value, budget);
        FExpr::Cmp(op, Box::new(a), Box::new(b))
    }

    fn leaf(&mut self, env: &[Ty], target: Ty) -> FExpr {
        if let Some(idx) = self.local_of(env, |t| t == target) {
            if self.rng.boolean() {
                return FExpr::Local(idx);
            }
        }
        match target {
            Int => FExpr::Lit(Val::I(*self.rng.pick(&INT_EDGES))),
            Long => FExpr::Lit(Val::L(*self.rng.pick(&LONG_EDGES))),
            Float => FExpr::Lit(Val::F(self.rng.pick(&FLOAT_POOL).to_bits())),
            Double => FExpr::Lit(Val::D(self.rng.pick(&DOUBLE_POOL).to_bits())),
            Boolean => FExpr::Lit(Val::Bool(self.rng.boolean())),
            Char => FExpr::Lit(Val::C(*self.rng.pick(&CHAR_POOL))),
            Byte => FExpr::Cast(Byte, Box::new(FExpr::Lit(Val::I(*self.rng.pick(&BYTE_LIT))))),
            Short => FExpr::Cast(Short, Box::new(FExpr::Lit(Val::I(*self.rng.pick(&SHORT_LIT))))),
        }
    }

    pub(super) fn fresh_budget(&mut self) -> i32 {
        4 + self.rng.below(9) as i32
    }
}
