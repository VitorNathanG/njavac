use crate::model::{ident, BinOp, CmpOp, FExpr, FStmt, LogOp, PrintArg, Prog, Ty, Val};
use Ty::*;

// SplitMix64 is deterministic, seeded, dependency-free, and rung-invariant.
pub(super) struct Rng {
    state: u64,
}

impl Rng {
    pub(super) fn new(seed: u64) -> Self {
        Rng { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `0..n` (n > 0).
    fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    fn boolean(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// True with probability `num/den`.
    fn ratio(&mut self, num: u32, den: u32) -> bool {
        (self.below(den as usize) as u32) < num
    }

    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

/// The materialization mode a boolean expression is generated in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BoolMode {
    Branch,
    Value,
}

/// Every boundary decision in the generator reads this, so the supported surface
/// is one reviewable structure rather than scattered `if`s.
struct ScopeCaps {
    decls_in_branches: bool,
    boolean_boundaries: bool,
    #[allow(dead_code)]
    has_ternary: bool,
    #[allow(dead_code)]
    has_loops: bool,
}

const CAPS: ScopeCaps = ScopeCaps {
    decls_in_branches: false,
    boolean_boundaries: true,
    has_ternary: false,
    has_loops: false,
};

const NUMERIC: [Ty; 7] = [Int, Long, Float, Double, Byte, Short, Char];
const INTEGRAL: [Ty; 5] = [Int, Long, Byte, Short, Char];
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
const STRINGS: [&str; 6] = ["", "x", "hello", "a b c", "12345", "Java"];

pub(super) struct Gen {
    pub(super) rng: Rng,
}

impl Gen {
    fn numeric_upto(&mut self, r: u8) -> Ty {
        let opts: Vec<Ty> = NUMERIC.iter().copied().filter(|t| t.prank() <= r).collect();
        *self.rng.pick(&opts)
    }

    fn local_of(&mut self, env: &[Ty], pred: impl Fn(Ty) -> bool) -> Option<usize> {
        let ids: Vec<usize> =
            env.iter().enumerate().filter(|(_, t)| pred(**t)).map(|(i, _)| i).collect();
        if ids.is_empty() { None } else { Some(*self.rng.pick(&ids)) }
    }

    fn expr(&mut self, env: &[Ty], target: Ty, mode: BoolMode, budget: &mut i32) -> FExpr {
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
        let op = *self.rng.pick(&[CmpOp::Lt, CmpOp::Le, CmpOp::Gt, CmpOp::Ge, CmpOp::Eq, CmpOp::Ne]);
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

    fn fresh_budget(&mut self) -> i32 {
        4 + self.rng.below(9) as i32
    }

    fn push_observed(out: &mut Vec<FStmt>, stmt: FStmt, local_count: usize) {
        let observed = match &stmt {
            FStmt::Decl { local, .. }
            | FStmt::Assign { local, .. }
            | FStmt::Compound { local, .. }
            | FStmt::IncDec { local, .. } => *local..*local + 1,
            FStmt::If { .. } => 0..local_count,
            _ => 0..0,
        };
        out.push(stmt);
        out.extend(observed.map(|local| {
            FStmt::Println(PrintArg::Expr(FExpr::Local(local)))
        }));
    }

    fn top_stmt(&mut self, env: &mut Vec<Ty>, depth: u32) -> FStmt {
        let has_local = !env.is_empty();
        let has_numeric = env.iter().any(|t| t.is_numeric());
        let mut menu: Vec<u8> = vec![0];
        if has_local {
            menu.push(1);
            menu.push(2);
        }
        if has_numeric {
            menu.push(3);
        }
        menu.push(4);
        menu.push(4);
        if depth < 2 {
            menu.push(5);
        }
        match *self.rng.pick(&menu.clone()) {
            0 => self.gen_decl(env),
            1 => self.gen_assign(env),
            2 => self.gen_compound(env),
            3 => self.gen_incdec(env),
            4 => self.gen_println(env),
            _ => self.gen_if(env, depth),
        }
    }

    fn gen_decl(&mut self, env: &mut Vec<Ty>) -> FStmt {
        let ty = *self.rng.pick(&[Int, Long, Float, Double, Boolean, Char, Byte, Short]);
        let mut budget = self.fresh_budget();
        let mode = if ty == Boolean { BoolMode::Branch } else { BoolMode::Value };
        let init = self.expr(env, ty, mode, &mut budget);
        let local = env.len();
        env.push(ty);
        FStmt::Decl { ty, local, init }
    }

    fn gen_assign(&mut self, env: &[Ty]) -> FStmt {
        let local = self.rng.below(env.len());
        let ty = env[local];
        let mut budget = self.fresh_budget();
        let mode = if ty == Boolean { BoolMode::Branch } else { BoolMode::Value };
        let value = self.expr(env, ty, mode, &mut budget);
        FStmt::Assign { local, value }
    }

    fn gen_compound(&mut self, env: &[Ty]) -> FStmt {
        let local = self.rng.below(env.len());
        let ty = env[local];
        let mut budget = self.fresh_budget();
        let (op, value) = if ty == Boolean {
            let op = *self.rng.pick(&[BinOp::BAnd, BinOp::BOr, BinOp::BXor]);
            (op, self.expr(env, Boolean, BoolMode::Value, &mut budget))
        } else if ty == Float || ty == Double {
            let op = *self.rng.pick(&[BinOp::Add, BinOp::Sub, BinOp::Mul, BinOp::Div, BinOp::Rem]);
            let rhs = *self.rng.pick(&[Int, Long, Float, Double]);
            (op, self.expr(env, rhs, BoolMode::Value, &mut budget))
        } else {
            let op = *self.rng.pick(&[
                BinOp::Add, BinOp::Sub, BinOp::Mul, BinOp::Div, BinOp::Rem, BinOp::BAnd, BinOp::BOr,
                BinOp::BXor, BinOp::Shl, BinOp::Shr, BinOp::Ushr,
            ]);
            let integral_rhs = op.is_shift() || matches!(op, BinOp::BAnd | BinOp::BOr | BinOp::BXor);
            let rhs = if integral_rhs { *self.rng.pick(&INTEGRAL) } else { *self.rng.pick(&NUMERIC) };
            (op, self.expr(env, rhs, BoolMode::Value, &mut budget))
        };
        FStmt::Compound { local, op, value }
    }

    fn gen_incdec(&mut self, env: &[Ty]) -> FStmt {
        let idx = self.local_of(env, |t| t.is_numeric()).unwrap();
        FStmt::IncDec { local: idx, prefix: self.rng.boolean(), inc: self.rng.boolean() }
    }

    fn gen_println(&mut self, env: &[Ty]) -> FStmt {
        let mut budget = self.fresh_budget();
        let arg = match self.rng.below(4) {
            0 => PrintArg::Str((*self.rng.pick(&STRINGS)).to_string()),
            1 => PrintArg::Expr(self.expr(env, Boolean, BoolMode::Value, &mut budget)),
            _ => {
                let ty = *self.rng.pick(&[Int, Long, Float, Double, Char]);
                PrintArg::Expr(self.expr(env, ty, BoolMode::Value, &mut budget))
            }
        };
        FStmt::Println(arg)
    }

    fn gen_if(&mut self, env: &[Ty], depth: u32) -> FStmt {
        let mut budget = self.fresh_budget();
        let cond = self.expr(env, Boolean, BoolMode::Branch, &mut budget);
        let mut then_b = self.branch_body(env, depth + 1);
        let mut else_b = if self.rng.boolean() { Some(self.branch_body(env, depth + 1)) } else { None };
        then_b.insert(0, FStmt::Println(PrintArg::Str("then".to_string())));
        if let Some(body) = &mut else_b {
            body.insert(0, FStmt::Println(PrintArg::Str("else".to_string())));
        }
        FStmt::If { cond, then_b, else_b }
    }

    fn branch_body(&mut self, env: &[Ty], depth: u32) -> Vec<FStmt> {
        debug_assert!(!CAPS.decls_in_branches);
        let n = 1 + self.rng.below(3);
        let mut out = Vec::new();
        for _ in 0..n {
            let has_local = !env.is_empty();
            let has_numeric = env.iter().any(|t| t.is_numeric());
            let mut menu: Vec<u8> = vec![4, 4];
            if has_local {
                menu.push(1);
                menu.push(2);
            }
            if has_numeric {
                menu.push(3);
            }
            if depth < 2 {
                menu.push(5);
            }
            let choice = *self.rng.pick(&menu.clone());
            let stmt = match choice {
                1 => self.gen_assign(env),
                2 => self.gen_compound(env),
                3 => self.gen_incdec(env),
                5 => self.gen_if(env, depth),
                _ => self.gen_println(env),
            };
            Self::push_observed(&mut out, stmt, env.len());
        }
        out
    }

    pub(super) fn gen_prog(&mut self, n: u64) -> Prog {
        let mut env: Vec<Ty> = Vec::new();
        let nstmt = 5 + self.rng.below(10);
        let mut body = Vec::with_capacity(nstmt);
        for i in 0..nstmt {
            let stmt = if i < 2 {
                self.gen_decl(&mut env)
            } else {
                self.top_stmt(&mut env, 0)
            };
            Self::push_observed(&mut body, stmt, env.len());
        }
        Prog { name: ident(n), locals: env, body }
    }
}
