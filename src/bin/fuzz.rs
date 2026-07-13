//! njavac's differential fuzzer (ROADMAP §0.1).
//!
//! Generates random **in-scope** Java (`main` bodies over the supported numeric +
//! branch + short-circuit subset), compiles each program with BOTH the pinned
//! `javac` and njavac (in-process), and byte-compares. On a mismatch it
//! auto-minimizes to a `fixtures/`-ready `.java` and localizes the divergence with
//! the same `classdump::diff_report` the bench uses. Seed-reproducible
//! (`fuzz <seed>`). Dependency-free (`std` only).
//!
//! ## Why this is sound (no false positives)
//!
//! The ONLY hard-fail signal is *both compilers accept a program (each emits a
//! `.class`) and the bytes differ* — which is, by definition, an njavac bug, since
//! byte-identity-to-javac IS the spec. Everything else is skip/telemetry:
//!
//! | outcome                          | meaning                       | action              |
//! | -------------------------------- | ----------------------------- | ------------------- |
//! | both accept, **bytes differ**    | njavac bug                    | FINDING → minimize  |
//! | both accept, bytes equal         | correct                       | pass                |
//! | javac rejects (no `.class`)      | generator emitted bad Java    | `generator-invalid` |
//! | njavac panics, javac accepted    | valid Java njavac can't do    | `njavac-reject`     |
//!
//! Generator over-reach can never cause a false finding: if njavac *accepts*
//! out-of-scope code and bytes differ, that's a real bug; if it *rejects*, it's
//! telemetry. So the generator's in-subset discipline is a **yield** lever, not a
//! soundness lever. Three harness invariants make the equivalence airtight — the
//! `ident()` naming chokepoint, `reset_dir` (no stale `.class`) + the exact-file-set
//! assertion, and generate-all-IR-before-any-IO determinism.
//!
//! ## Performance
//!
//! njavac runs in-process (µs); javac's ~0.4s JVM startup is the wall, so we batch
//! N independent single-class programs into ONE `javac -d <dir> @argfile`
//! invocation (proven byte-identical by the bench). `@argfile` is required: a large
//! batch as argv would blow `ARG_MAX`. Scratch lives on the normal FS (container
//! `/dev/shm` is only 64 MB).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

// ===========================================================================
// §1  PRNG — SplitMix64 (deterministic, seeded, dependency-free). RUNG-INVARIANT.
// ===========================================================================

struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
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

// ===========================================================================
// §2  IR (Ty, Val, ops, FExpr, FStmt, Prog, Ident).  +1 variant per rung.
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    Int,
    Long,
    Float,
    Double,
    Boolean,
    Char,
    Byte,
    Short,
}
use Ty::*;

impl Ty {
    fn kw(self) -> &'static str {
        match self {
            Int => "int",
            Long => "long",
            Float => "float",
            Double => "double",
            Boolean => "boolean",
            Char => "char",
            Byte => "byte",
            Short => "short",
        }
    }
    /// Binary-numeric-promotion rank: the wider wins. Sub-int types all rank as
    /// `int` (0). `boolean` is not numeric (255).
    fn prank(self) -> u8 {
        match self {
            Long => 1,
            Float => 2,
            Double => 3,
            Boolean => 255,
            _ => 0, // Int, Byte, Short, Char
        }
    }
    fn is_numeric(self) -> bool {
        self != Boolean
    }
    /// Integral in the Java sense (participates in shift / `~` / integral `%`).
    fn is_integral(self) -> bool {
        matches!(self, Int | Long | Byte | Short | Char)
    }
}

const NUMERIC: [Ty; 7] = [Int, Long, Float, Double, Byte, Short, Char];
const INTEGRAL: [Ty; 5] = [Int, Long, Byte, Short, Char];

/// A literal value; its Java type is implied by the variant. Floats/doubles keyed
/// by bit-pattern so `-0.0`/`NaN`/`±Inf` stay distinct (matches classfile pooling).
#[derive(Clone, Debug)]
enum Val {
    I(i32),
    L(i64),
    F(u32),
    D(u64),
    Bool(bool),
    C(u16),
}

#[derive(Clone, Copy, Debug)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BAnd,
    BOr,
    BXor,
    Shl,
    Shr,
    Ushr,
}

impl BinOp {
    fn sym(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::BAnd => "&",
            BinOp::BOr => "|",
            BinOp::BXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
            BinOp::Ushr => ">>>",
        }
    }
    fn is_shift(self) -> bool {
        matches!(self, BinOp::Shl | BinOp::Shr | BinOp::Ushr)
    }
}

#[derive(Clone, Copy, Debug)]
enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl CmpOp {
    fn sym(self) -> &'static str {
        match self {
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
            CmpOp::Eq => "==",
            CmpOp::Ne => "!=",
        }
    }
    fn is_order(self) -> bool {
        matches!(self, CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge)
    }
}

#[derive(Clone, Copy, Debug)]
enum LogOp {
    And,
    Or,
}

#[derive(Clone, Debug)]
enum FExpr {
    Lit(Val),
    Local(usize),
    Neg(Box<FExpr>),
    BitNot(Box<FExpr>),
    Not(Box<FExpr>),
    Cast(Ty, Box<FExpr>),
    Bin(BinOp, Box<FExpr>, Box<FExpr>),
    Cmp(CmpOp, Box<FExpr>, Box<FExpr>),
    Logic(LogOp, Box<FExpr>, Box<FExpr>),
}

#[derive(Clone, Debug)]
enum PrintArg {
    Str(String),
    Expr(FExpr),
}

#[derive(Clone, Debug)]
enum FStmt {
    Decl { ty: Ty, local: usize, init: FExpr },
    Assign { local: usize, value: FExpr },
    Compound { local: usize, op: BinOp, value: FExpr },
    IncDec { local: usize, prefix: bool, inc: bool },
    Println(PrintArg),
    If { cond: FExpr, then_b: Vec<FStmt>, else_b: Option<Vec<FStmt>> },
}

/// A whole program: the class name (via `ident`), the flat local-type env in
/// declaration/slot order, and the `main` body. `Local(usize)` indexes `locals`.
#[derive(Clone, Debug)]
struct Prog {
    name: Ident,
    /// The flat local-type env in declaration/slot order. Not read after
    /// construction today (render derives everything from `Decl`/`Local`), but the
    /// scope-agnostic flat env is what makes block scope (loops) an env-only change.
    #[allow(dead_code)]
    locals: Vec<Ty>,
    body: Vec<FStmt>,
}

/// THE naming chokepoint. Class name, source filename, and the `source_file`
/// argument to `njavac::compile` are the SAME token — this is what forecloses the
/// highest-severity false positive (the `.class` bytes couple to the class name,
/// the `SourceFile` attribute, and the `LineNumberTable`). Used by generation, the
/// batch writer, the in-process compile call, and every minimizer candidate.
#[derive(Clone, Debug)]
struct Ident {
    class: String,
    java_file: String,
    source_arg: String,
}

fn ident(n: u64) -> Ident {
    let class = format!("Fuzz{n:07}");
    let java_file = format!("{class}.java");
    Ident { source_arg: java_file.clone(), java_file, class }
}

/// The materialization mode a boolean expression is generated in. A `Branch`
/// boolean (a comparison / `&&` / `||` / `!`) may only live where njavac
/// materializes it on an EMPTY base stack — an `if` condition or a `boolean`
/// decl/assign RHS. A `Value` boolean is a plain 0/1 (literal, local, or `&|^` of
/// value-booleans) usable anywhere a boolean value is needed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BoolMode {
    Branch,
    Value,
}

// ===========================================================================
// §6  ScopeCaps — the movable in-scope boundary as data. Edit flags per rung.
// ===========================================================================

/// Every boundary decision in the generator reads this, so the supported surface
/// is one reviewable structure rather than scattered `if`s. Today: branch-booleans
/// only as `if` conditions / boolean decl+assign RHS; no decls inside branch
/// bodies (sema allocates slots only for top-level decls); no `?:`, no loops.
struct ScopeCaps {
    decls_in_branches: bool,
    /// Flipped true by the `?:` and loops rungs respectively — each flip *is* the
    /// statement "booleans may now live in this new context / block scope exists."
    #[allow(dead_code)]
    has_ternary: bool,
    #[allow(dead_code)]
    has_loops: bool,
}

const CAPS: ScopeCaps = ScopeCaps { decls_in_branches: false, has_ternary: false, has_loops: false };

// ===========================================================================
//    Boundary literal pools — biased toward the constant-load opcode seams and
//    the IEEE landmines, because a green run only *means* something if the
//    generator actually reaches where bytes diverge.
// ===========================================================================

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

// ===========================================================================
// §3  Generator — boundary-first, type-directed, valid-by-construction.
//     Each validity rule is a YIELD lever (keeps javac accepting), never a
//     soundness lever.
// ===========================================================================

struct Gen {
    rng: Rng,
}

impl Gen {
    /// Numeric types with promotion rank ≤ `r` (the legal "other operand" set that
    /// keeps a binary op's result at the anchor type).
    fn numeric_upto(&mut self, r: u8) -> Ty {
        let opts: Vec<Ty> = NUMERIC.iter().copied().filter(|t| t.prank() <= r).collect();
        *self.rng.pick(&opts)
    }

    fn local_of(&mut self, env: &[Ty], pred: impl Fn(Ty) -> bool) -> Option<usize> {
        let ids: Vec<usize> =
            env.iter().enumerate().filter(|(_, t)| pred(**t)).map(|(i, _)| i).collect();
        if ids.is_empty() {
            None
        } else {
            Some(*self.rng.pick(&ids))
        }
    }

    // ---- expressions -------------------------------------------------------

    /// Generate an expression whose static type is exactly `target` (for numerics)
    /// or a boolean in the given `mode`. Always well-typed and in-subset.
    fn expr(&mut self, env: &[Ty], target: Ty, mode: BoolMode, budget: &mut i32) -> FExpr {
        *budget -= 1;
        if target == Boolean {
            return self.bool_expr(env, mode, budget);
        }
        // Leaf when out of budget or by chance.
        if *budget <= 0 || self.rng.ratio(2, 5) {
            return self.leaf(env, target);
        }
        match target {
            Byte | Short | Char => {
                // Sub-int values arise via a narrowing cast (there is no byte/short
                // literal, and `b1 + b2` is `int`), or a leaf.
                let src = *self.rng.pick(&NUMERIC);
                FExpr::Cast(target, Box::new(self.expr(env, src, BoolMode::Value, budget)))
            }
            Int | Long | Float | Double => self.numeric_compound(env, target, budget),
            Boolean => unreachable!(),
        }
    }

    fn numeric_compound(&mut self, env: &[Ty], target: Ty, budget: &mut i32) -> FExpr {
        let integral = target.is_integral(); // Int or Long here
        // Available forms for this target.
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
                // max(op_a, op_b) == target: anchor one side at exactly `target`,
                // the other at rank ≤ target. Random anchor side for diversity.
                let other = self.numeric_upto(target.prank());
                let (a, b) = if self.rng.boolean() {
                    (self.expr(env, target, BoolMode::Value, budget),
                     self.expr(env, other, BoolMode::Value, budget))
                } else {
                    (self.expr(env, other, BoolMode::Value, budget),
                     self.expr(env, target, BoolMode::Value, budget))
                };
                FExpr::Bin(*op, Box::new(a), Box::new(b))
            }
            Form::DivRem(op) => {
                // Numerator anchors the type; divisor is kept non-constant-zero:
                // a live local (never folds to a constant) or a nonzero literal.
                let a = self.expr(env, target, BoolMode::Value, budget);
                let b = if integral {
                    self.divisor(env, target)
                } else {
                    // float/double `/0.0` is legal (Inf/NaN) — no restriction.
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

    /// The divisor of an integral `/` or `%`: a live local (non-constant, so the
    /// whole expression can never be a compile-time division by zero) or, failing
    /// that, a guaranteed-nonzero literal.
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
        // Value-boolean leaf: a boolean literal or boolean local.
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
            // Only value-booleans: leaf, or `&|^` of value-booleans.
            if self.rng.ratio(2, 5) {
                let op = *self.rng.pick(&[BinOp::BAnd, BinOp::BOr, BinOp::BXor]);
                let a = self.bool_expr(env, BoolMode::Value, budget);
                let b = self.bool_expr(env, BoolMode::Value, budget);
                return FExpr::Bin(op, Box::new(a), Box::new(b));
            }
            return value_leaf(self);
        }
        // Branch mode: comparisons, &&/||, !, or a value-boolean at the leaf.
        // Constant boolean operands are up-weighted so the short-circuit
        // constant-operand corners (`true && q`, `q && false`) appear often.
        match self.rng.below(6) {
            0 | 1 => self.cmp_expr(env, budget),
            2 => {
                let op = *self.rng.pick(&[LogOp::And, LogOp::Or]);
                let a = self.bool_expr(env, BoolMode::Branch, budget);
                let b = self.bool_expr(env, BoolMode::Branch, budget);
                FExpr::Logic(op, Box::new(a), Box::new(b))
            }
            3 => FExpr::Not(Box::new(self.bool_expr(env, BoolMode::Branch, budget))),
            4 => {
                // value-boolean `&|^` (still fine on an empty base stack)
                let op = *self.rng.pick(&[BinOp::BAnd, BinOp::BOr, BinOp::BXor]);
                let a = self.bool_expr(env, BoolMode::Value, budget);
                let b = self.bool_expr(env, BoolMode::Value, budget);
                FExpr::Bin(op, Box::new(a), Box::new(b))
            }
            _ => value_leaf(self),
        }
    }

    fn cmp_expr(&mut self, env: &[Ty], budget: &mut i32) -> FExpr {
        let op = *self.rng.pick(&[CmpOp::Lt, CmpOp::Le, CmpOp::Gt, CmpOp::Ge, CmpOp::Eq, CmpOp::Ne]);
        if !op.is_order() && self.rng.ratio(1, 3) {
            // boolean equality: both operands boolean
            let a = self.bool_expr(env, BoolMode::Value, budget);
            let b = self.bool_expr(env, BoolMode::Value, budget);
            return FExpr::Cmp(op, Box::new(a), Box::new(b));
        }
        // numeric comparison with binary promotion
        let ct = *self.rng.pick(&NUMERIC);
        let other = self.numeric_upto(ct.prank());
        let a = self.expr(env, ct, BoolMode::Value, budget);
        let b = self.expr(env, other, BoolMode::Value, budget);
        FExpr::Cmp(op, Box::new(a), Box::new(b))
    }

    fn leaf(&mut self, env: &[Ty], target: Ty) -> FExpr {
        // Prefer an existing local of the exact type half the time.
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
            // No byte/short literal exists — a constant of these types is a cast.
            Byte => FExpr::Cast(Byte, Box::new(FExpr::Lit(Val::I(*self.rng.pick(&BYTE_LIT))))),
            Short => FExpr::Cast(Short, Box::new(FExpr::Lit(Val::I(*self.rng.pick(&SHORT_LIT))))),
        }
    }

    // ---- statements --------------------------------------------------------

    fn fresh_budget(&mut self) -> i32 {
        4 + self.rng.below(9) as i32
    }

    /// One top-level statement. `env` grows when this returns a `Decl`.
    fn top_stmt(&mut self, env: &mut Vec<Ty>, depth: u32) -> FStmt {
        let has_local = !env.is_empty();
        let has_numeric = env.iter().any(|t| t.is_numeric());
        // Weighted menu of what is possible right now.
        let mut menu: Vec<u8> = vec![0]; // 0 = Decl (always possible)
        if has_local {
            menu.push(1); // Assign
            menu.push(2); // Compound
        }
        if has_numeric {
            menu.push(3); // IncDec
        }
        menu.push(4); // Println
        menu.push(4);
        if depth < 2 {
            menu.push(5); // If
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
        // Generate the initializer BEFORE the local is in scope (no self-reference).
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
            // integral local: any arithmetic/bitwise/shift. The LHS is a variable,
            // so even `v /= 0` is only a *runtime* error, never a compile error.
            let op = *self.rng.pick(&[
                BinOp::Add, BinOp::Sub, BinOp::Mul, BinOp::Div, BinOp::Rem, BinOp::BAnd, BinOp::BOr,
                BinOp::BXor, BinOp::Shl, BinOp::Shr, BinOp::Ushr,
            ]);
            // Shifts and bitwise `&|^` require an integral RHS (`v ^= 1.5` is a
            // compile error); only the arithmetic ops accept a float/double RHS.
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
                // a primitive with a direct println overload
                let ty = *self.rng.pick(&[Int, Long, Float, Double, Char]);
                PrintArg::Expr(self.expr(env, ty, BoolMode::Value, &mut budget))
            }
        };
        FStmt::Println(arg)
    }

    fn gen_if(&mut self, env: &[Ty], depth: u32) -> FStmt {
        let mut budget = self.fresh_budget();
        let cond = self.expr(env, Boolean, BoolMode::Branch, &mut budget);
        let then_b = self.branch_body(env, depth + 1);
        let else_b = if self.rng.boolean() { Some(self.branch_body(env, depth + 1)) } else { None };
        FStmt::If { cond, then_b, else_b }
    }

    /// A branch body: no declarations (sema allocates slots only for top-level
    /// decls, so a block-scoped local would be undeclared to njavac).
    fn branch_body(&mut self, env: &[Ty], depth: u32) -> Vec<FStmt> {
        debug_assert!(!CAPS.decls_in_branches);
        let n = 1 + self.rng.below(3);
        let mut out = Vec::new();
        for _ in 0..n {
            let has_local = !env.is_empty();
            let has_numeric = env.iter().any(|t| t.is_numeric());
            let mut menu: Vec<u8> = vec![4, 4]; // Println always
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
            out.push(match choice {
                1 => self.gen_assign(env),
                2 => self.gen_compound(env),
                3 => self.gen_incdec(env),
                5 => self.gen_if(env, depth),
                _ => self.gen_println(env),
            });
        }
        out
    }

    fn gen_prog(&mut self, n: u64) -> Prog {
        let mut env: Vec<Ty> = Vec::new();
        let nstmt = 5 + self.rng.below(10);
        let mut body = Vec::with_capacity(nstmt);
        for i in 0..nstmt {
            // Seed a couple of locals up front so later statements have material.
            if i < 2 {
                body.push(self.gen_decl(&mut env));
            } else {
                body.push(self.top_stmt(&mut env, 0));
            }
        }
        Prog { name: ident(n), locals: env, body }
    }
}

// ===========================================================================
// §4  Render — IR → Java source, via `ident()`. Fully parenthesized (so the
//     parse tree is unambiguous and no tokens paste), one statement per line
//     (so the LineNumberTable genuinely varies and is exercised).
// ===========================================================================

fn render(prog: &Prog) -> String {
    let mut s = String::new();
    s.push_str(&format!("public class {} {{\n", prog.name.class));
    s.push_str("    public static void main(String[] args) {\n");
    for st in &prog.body {
        render_stmt(st, 2, &mut s);
    }
    s.push_str("    }\n");
    s.push_str("}\n");
    s
}

fn render_stmt(st: &FStmt, indent: usize, out: &mut String) {
    let pad = "    ".repeat(indent);
    match st {
        FStmt::Decl { ty, local, init } => {
            out.push_str(&format!("{pad}{} v{} = {};\n", ty.kw(), local, render_expr(init)));
        }
        FStmt::Assign { local, value } => {
            out.push_str(&format!("{pad}v{} = {};\n", local, render_expr(value)));
        }
        FStmt::Compound { local, op, value } => {
            out.push_str(&format!("{pad}v{} {}= {};\n", local, op.sym(), render_expr(value)));
        }
        FStmt::IncDec { local, prefix, inc } => {
            let opsym = if *inc { "++" } else { "--" };
            if *prefix {
                out.push_str(&format!("{pad}{}v{};\n", opsym, local));
            } else {
                out.push_str(&format!("{pad}v{}{};\n", local, opsym));
            }
        }
        FStmt::Println(arg) => {
            let a = match arg {
                PrintArg::Str(s) => format!("\"{s}\""),
                PrintArg::Expr(e) => render_expr(e),
            };
            out.push_str(&format!("{pad}System.out.println({a});\n"));
        }
        FStmt::If { cond, then_b, else_b } => {
            out.push_str(&format!("{pad}if ({}) {{\n", render_expr(cond)));
            for s in then_b {
                render_stmt(s, indent + 1, out);
            }
            out.push_str(&format!("{pad}}}"));
            if let Some(eb) = else_b {
                out.push_str(" else {\n");
                for s in eb {
                    render_stmt(s, indent + 1, out);
                }
                out.push_str(&format!("{pad}}}\n"));
            } else {
                out.push('\n');
            }
        }
    }
}

fn render_expr(e: &FExpr) -> String {
    match e {
        FExpr::Lit(v) => render_val(v),
        FExpr::Local(i) => format!("v{i}"),
        FExpr::Neg(x) => format!("(-({}))", render_expr(x)),
        FExpr::BitNot(x) => format!("(~({}))", render_expr(x)),
        FExpr::Not(x) => format!("(!({}))", render_expr(x)),
        FExpr::Cast(ty, x) => format!("(({})({}))", ty.kw(), render_expr(x)),
        FExpr::Bin(op, l, r) => format!("({} {} {})", render_expr(l), op.sym(), render_expr(r)),
        FExpr::Cmp(op, l, r) => format!("({} {} {})", render_expr(l), op.sym(), render_expr(r)),
        FExpr::Logic(op, l, r) => {
            let s = match op {
                LogOp::And => "&&",
                LogOp::Or => "||",
            };
            format!("({} {} {})", render_expr(l), s, render_expr(r))
        }
    }
}

fn render_val(v: &Val) -> String {
    match v {
        Val::I(x) => int_str(*x),
        Val::L(x) => {
            if *x < 0 {
                format!("-{}L", x.unsigned_abs())
            } else {
                format!("{x}L")
            }
        }
        Val::F(bits) => float_str(*bits),
        Val::D(bits) => double_str(*bits),
        Val::Bool(b) => b.to_string(),
        Val::C(c) => char_str(*c),
    }
}

/// A decimal int literal. Negatives render as unary-minus of the magnitude, which
/// also correctly handles `i32::MIN` (`-2147483648`, the sole legal spelling).
fn int_str(x: i32) -> String {
    if x < 0 {
        format!("-{}", x.unsigned_abs())
    } else {
        x.to_string()
    }
}

fn float_str(bits: u32) -> String {
    let f = f32::from_bits(bits);
    if f.is_nan() {
        "(0.0f / 0.0f)".to_string()
    } else if f.is_infinite() {
        if f > 0.0 { "(1.0f / 0.0f)".to_string() } else { "(-1.0f / 0.0f)".to_string() }
    } else {
        let mut s = format!("{f}");
        if !s.contains(['.', 'e', 'E']) {
            s.push_str(".0");
        }
        s.push('f');
        s
    }
}

fn double_str(bits: u64) -> String {
    let f = f64::from_bits(bits);
    if f.is_nan() {
        "(0.0 / 0.0)".to_string()
    } else if f.is_infinite() {
        if f > 0.0 { "(1.0 / 0.0)".to_string() } else { "(-1.0 / 0.0)".to_string() }
    } else {
        let mut s = format!("{f}");
        if !s.contains(['.', 'e', 'E']) {
            s.push_str(".0");
        }
        s
    }
}

/// A `char` literal, always safely renderable: named escapes for the line
/// terminators / quote / backslash, direct for printable ASCII, `\uXXXX` for
/// everything else (none of which is a Java line terminator, quote, or backslash,
/// so `\u` translation stays inside the literal).
fn char_str(c: u16) -> String {
    match c {
        0x0a => "'\\n'".to_string(),
        0x0d => "'\\r'".to_string(),
        0x09 => "'\\t'".to_string(),
        0x08 => "'\\b'".to_string(),
        0x0c => "'\\f'".to_string(),
        0x00 => "'\\0'".to_string(),
        0x27 => "'\\''".to_string(),
        0x5c => "'\\\\'".to_string(),
        0x20..=0x7e => format!("'{}'", (c as u8) as char),
        _ => format!("'\\u{c:04x}'"),
    }
}

// ===========================================================================
// §7/§8  Oracle + driver.  RUNG-INVARIANT.
// ===========================================================================

struct Config {
    seed: u64,
    /// Whether the seed was pinned on the command line (positional or `--seed`).
    /// When false, a bare `make fuzz` picks a fresh random seed each run so every
    /// invocation explores new programs; the chosen seed is printed to reproduce it.
    seed_set: bool,
    count: u64,
    batch: u64,
    javac: String,
    out_dir: PathBuf,
    keep_going: bool,
    no_min: bool,
    dump_sources: bool,
    selftest: bool,
}

impl Config {
    fn from_args() -> Config {
        let home = std::env::var("HOME").unwrap_or_default();
        let default_javac = format!("{home}/.sdkman/candidates/java/25.0.2-graalce/bin/javac");
        let mut cfg = Config {
            seed: 0,
            seed_set: false,
            count: 5000,
            batch: 1000,
            javac: std::env::var("JAVAC").unwrap_or(default_javac),
            out_dir: PathBuf::from("fuzz-out"),
            keep_going: false,
            no_min: false,
            dump_sources: false,
            selftest: false,
        };
        let mut positional = 0;
        let mut args = std::env::args().skip(1);
        while let Some(a) = args.next() {
            match a.as_str() {
                "--seed" => {
                    cfg.seed = args.next().and_then(|v| v.parse().ok()).expect("--seed needs a u64");
                    cfg.seed_set = true;
                }
                "--count" => cfg.count = args.next().and_then(|v| v.parse().ok()).unwrap_or(cfg.count),
                "--batch" => cfg.batch = args.next().and_then(|v| v.parse().ok()).unwrap_or(cfg.batch),
                "--out-dir" => cfg.out_dir = PathBuf::from(args.next().expect("--out-dir needs a path")),
                "--javac" => cfg.javac = args.next().expect("--javac needs a path"),
                "--jobs" => {
                    let j: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                    assert_eq!(j, 1, "--jobs > 1 is not implemented in v1 (single-threaded batched)");
                }
                "--keep-going" => cfg.keep_going = true,
                "--no-min" => cfg.no_min = true,
                "--dump-sources" => cfg.dump_sources = true,
                "--selftest" => cfg.selftest = true,
                "-h" | "--help" => {
                    println!(
                        "usage: fuzz [<seed>] [<count>] [--seed N] [--count N] [--batch N] \
                         [--keep-going] [--no-min] [--out-dir DIR] [--jobs 1] [--dump-sources] \
                         [--selftest] [--javac PATH]\n\
                         \n  <seed> / --seed  pin the seed; OMIT for a fresh random seed each run\
                         \n                   (printed so a finding reproduces with `make fuzz SEED=<n>`)\
                         \n  --keep-going     don't stop at the first finding; enumerate distinct ones\
                         \n  --no-min         skip minimization (fast enumeration; emits raw repros)"
                    );
                    std::process::exit(0);
                }
                s if s.starts_with('-') => {
                    eprintln!("fuzz: unknown flag {s}");
                    std::process::exit(2);
                }
                s => {
                    let v: u64 = s.parse().unwrap_or_else(|_| {
                        eprintln!("fuzz: bad positional {s}");
                        std::process::exit(2)
                    });
                    if positional == 0 {
                        cfg.seed = v;
                        cfg.seed_set = true;
                    } else if positional == 1 {
                        cfg.count = v;
                    }
                    positional += 1;
                }
            }
        }
        cfg
    }
}

fn reset_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).expect("create dir");
}

/// Compile `src` in-process, catching a panic (out-of-scope input). `None` ⇒
/// njavac rejected. The `source_arg` MUST be the same token as the class/filename.
fn njavac_compile(src: &str, source_arg: &str) -> Option<Vec<u8>> {
    let src = src.to_string();
    let arg = source_arg.to_string();
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| njavac::compile(&src, &arg))).ok()
}

/// One `javac -d <out> @<argfile>` invocation. Returns whether it exited zero.
fn run_javac_batch(javac: &str, out: &Path, argfile: &Path) -> bool {
    Command::new(javac)
        .arg("-d")
        .arg(out)
        .arg(format!("@{}", argfile.display()))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_javac_one(javac: &str, out: &Path, src: &Path) -> bool {
    Command::new(javac)
        .arg("-d")
        .arg(out)
        .arg(src)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[derive(Default)]
struct Tally {
    pass: u64,
    generator_invalid: u64,
    njavac_reject: u64,
    findings: u64,
}

/// A fresh seed for a bare `make fuzz` (no external RNG crate): mix the wall-clock
/// nanoseconds with the pid through the SplitMix64 finalizer for good bit spread.
/// Only entropy source needed — the run itself is fully deterministic in `cfg.seed`.
fn random_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = nanos ^ (std::process::id() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn main() {
    // Speak in one voice: swallow the default panic dump (out-of-scope inputs
    // panic by design and are caught).
    std::panic::set_hook(Box::new(|_| {}));
    let mut cfg = Config::from_args();

    if cfg.dump_sources {
        dump_sources(&cfg);
        return;
    }
    if cfg.selftest {
        std::process::exit(selftest(&cfg));
    }

    // A bare `make fuzz` explores a fresh random seed every run; pin it (positional
    // or SEED=/--seed) to reproduce. Randomize only here, after the deterministic
    // self-check / dump-sources paths have taken their fixed default.
    if !cfg.seed_set {
        cfg.seed = random_seed();
    }

    let scratch = std::env::temp_dir().join(format!("njavac-fuzz-{}", cfg.seed));
    let src_dir = scratch.join("src");
    let javac_out = scratch.join("out");
    reset_dir(&scratch);
    std::fs::create_dir_all(&src_dir).expect("create src dir");

    let mut g = Gen { rng: Rng::new(cfg.seed) };
    let mut tally = Tally::default();
    let mut sigs: HashMap<String, SigInfo> = HashMap::new();
    let mut reject_dumped = 0u32;

    println!(
        "fuzz: seed={} count={} batch={} javac={}\n  reproduce this exact run with: make fuzz SEED={}",
        cfg.seed, cfg.count, cfg.batch, cfg.javac, cfg.seed
    );

    let mut n: u64 = 0;
    while n < cfg.count {
        let this = cfg.batch.min(cfg.count - n);

        // 1. Generate ALL IR + sources for the batch BEFORE any I/O — so a
        //    transient FS/javac hiccup changes tallies but never the program
        //    sequence (seed-reproducibility).
        let progs: Vec<Prog> = (0..this).map(|k| g.gen_prog(n + k)).collect();
        let sources: Vec<String> = progs.iter().map(render).collect();

        // 2/3. Fresh output dir + write sources + argfile.
        reset_dir(&javac_out);
        let mut argfile_body = String::new();
        for (p, s) in progs.iter().zip(&sources) {
            let path = src_dir.join(&p.name.java_file);
            std::fs::write(&path, s).expect("write source");
            argfile_body.push_str(&path.display().to_string());
            argfile_body.push('\n');
        }
        let argfile = scratch.join("files.txt");
        std::fs::write(&argfile, &argfile_body).expect("write argfile");

        // 4. One javac invocation over the whole batch.
        run_javac_batch(&cfg.javac, &javac_out, &argfile);

        // 5. Batch integrity: if javac emitted NOTHING for a non-empty batch it
        //    hard-failed — recompile individually so we never miscount N
        //    generator-invalids or read a stale pass.
        let emitted = |p: &Prog| javac_out.join(format!("{}.class", p.name.class)).exists();
        if this > 0 && !progs.iter().any(emitted) {
            eprintln!("fuzz: batch emitted 0 classes — recompiling individually to isolate");
            for p in &progs {
                let _ = run_javac_one(&cfg.javac, &javac_out, &src_dir.join(&p.name.java_file));
            }
        }

        // 6. Exact-file-set guard: no unexpected `.class` (esp. `$`-aux classes a
        //    future concat/switch generator might over-reach into).
        assert_no_unexpected_classes(&javac_out, &progs);

        // 7. Compare each program.
        for (p, s) in progs.iter().zip(&sources) {
            let want = std::fs::read(javac_out.join(format!("{}.class", p.name.class)));
            let got = njavac_compile(s, &p.name.source_arg);
            match (want, got) {
                (Err(_), _) => tally.generator_invalid += 1,
                (Ok(_), None) => {
                    tally.njavac_reject += 1;
                    if reject_dumped < 20 {
                        let rd = cfg.out_dir.join("rejects");
                        let _ = std::fs::create_dir_all(&rd);
                        let _ = std::fs::write(rd.join(&p.name.java_file), s);
                        reject_dumped += 1;
                    }
                }
                (Ok(a), Some(b)) if a == b => tally.pass += 1,
                (Ok(a), Some(b)) => {
                    tally.findings += 1;
                    let rep = njavac::classdump::diff_report(&a, &b);
                    // Dedupe by the NORMALIZED structural divergence path, so the
                    // same njavac bug in programs of different sizes collapses to
                    // one signature instead of every program looking "distinct".
                    let sig = finding_sig(rep.as_deref());
                    let first = !sigs.contains_key(&sig);
                    let info = sigs.entry(sig.clone()).or_insert_with(|| SigInfo {
                        count: 0,
                        example: p.name.class.clone(),
                    });
                    info.count += 1;
                    if first {
                        println!(
                            "\nNEW FINDING [{sig}]: {} ({} vs {} bytes)",
                            p.name.class, a.len(), b.len()
                        );
                        report_finding(&cfg, p, s, rep.as_deref());
                    }
                    if !cfg.keep_going {
                        summary(&tally, cfg.count);
                        print_sig_breakdown(&sigs);
                        std::process::exit(1);
                    }
                }
            }
        }

        n += this;
        println!(
            "  progress {n}/{}  pass={} gen-invalid={} njavac-reject={} findings={}",
            cfg.count, tally.pass, tally.generator_invalid, tally.njavac_reject, tally.findings
        );
    }

    summary(&tally, cfg.count);
    print_sig_breakdown(&sigs);
    std::process::exit(if tally.findings > 0 { 1 } else { 0 });
}

/// One distinct finding class: how many programs hit it, and one example.
struct SigInfo {
    count: u64,
    example: String,
}

/// A stable signature for a finding: the normalized structural divergence path
/// from `diff_report` (bracketed indices collapsed to `N`), so the SAME njavac bug
/// dedupes across programs of different sizes. Falls back to a generic tag.
fn finding_sig(report: Option<&str>) -> String {
    let Some(rep) = report else { return "bytes-differ".to_string() };
    for line in rep.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("path") {
            if let Some((_, val)) = rest.split_once(':') {
                return normalize_indices(val.trim());
            }
        }
    }
    "bytes-differ".to_string()
}

/// Collapse every run of digits to a single `N` (`cp[17].bytes` -> `cp[N].bytes`).
fn normalize_indices(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_digits = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('N');
                in_digits = true;
            }
        } else {
            out.push(c);
            in_digits = false;
        }
    }
    out
}

fn print_sig_breakdown(sigs: &HashMap<String, SigInfo>) {
    if sigs.is_empty() {
        return;
    }
    println!("\ndistinct finding signatures ({}):", sigs.len());
    let mut v: Vec<(&String, &SigInfo)> = sigs.iter().collect();
    v.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(a.0.cmp(b.0)));
    for (sig, info) in v {
        println!("  {:>6} x  {}   (e.g. {})", info.count, sig, info.example);
    }
}

fn assert_no_unexpected_classes(javac_out: &Path, progs: &[Prog]) {
    let expected: HashSet<String> = progs.iter().map(|p| format!("{}.class", p.name.class)).collect();
    if let Ok(rd) = std::fs::read_dir(javac_out) {
        for e in rd.flatten() {
            let fname = e.file_name().to_string_lossy().into_owned();
            if fname.ends_with(".class") && !expected.contains(&fname) {
                panic!(
                    "fuzz: unexpected class {fname} in javac output — the generator over-reached \
                     into auxiliary classes (this would compare half a program)"
                );
            }
        }
    }
}

fn summary(t: &Tally, count: u64) {
    println!(
        "\nfuzz done: {count} cases  pass={} gen-invalid={} njavac-reject={} findings={}",
        t.pass, t.generator_invalid, t.njavac_reject, t.findings
    );
    if t.findings > 0 {
        println!("  -> {} byte-mismatch finding(s); see the fuzz-out/ dir", t.findings);
    }
}

/// Write a finding to the out-dir: the `.java` (ready for `fixtures/`) and a
/// `.diff` localizing the divergence. Unless `--no-min`, the program is minimized
/// first — disk-gated, so the emitted source is re-confirmed to reproduce by
/// construction. `orig_rep` is the divergence of the un-minimized program, used
/// for the `.diff` in `--no-min` mode.
fn report_finding(cfg: &Config, prog: &Prog, src: &str, orig_rep: Option<&str>) {
    let _ = std::fs::create_dir_all(&cfg.out_dir);

    if cfg.no_min {
        // Fast path: emit the raw program + its divergence, no extra javac spawns.
        let out_java = cfg.out_dir.join(&prog.name.java_file);
        std::fs::write(&out_java, src).expect("write finding source");
        if let Some(rep) = orig_rep {
            let _ = std::fs::write(cfg.out_dir.join(format!("{}.diff", prog.name.class)), rep);
        }
        println!("  wrote raw finding to {}", out_java.display());
        return;
    }

    let mut harness = MinHarness::new(&cfg.javac, cfg.seed);
    let minimized = minimize(prog, &mut harness);
    let msrc = render(&minimized);
    let out_java = cfg.out_dir.join(&minimized.name.java_file);
    std::fs::write(&out_java, &msrc).expect("write finding source");

    // Recompute both outputs under the minimized program for the .diff, which
    // also re-confirms the emitted fixture reproduces under real compile mechanics.
    let (want, got) = harness.compile_both(&minimized);
    if let (Some(a), Some(b)) = (want, got) {
        if a != b {
            if let Some(rep) = njavac::classdump::diff_report(&a, &b) {
                let _ = std::fs::write(cfg.out_dir.join(format!("{}.diff", minimized.name.class)), rep);
            }
        } else {
            eprintln!("fuzz: WARNING minimizer produced a non-reproducing case — see the .java");
        }
    }
    println!("  wrote minimized finding to {}", out_java.display());
}

// ===========================================================================
// §5  Minimizer — statement-level ddmin, disk-gated on the three-conjunct
//     predicate (javac accepts ∧ njavac accepts ∧ bytes differ), spawn-capped,
//     under a FIXED name via `ident()`. +1 reduction per rung.
// ===========================================================================

struct MinHarness {
    javac: String,
    dir: PathBuf,
    spawns: u32,
    cap: u32,
}

impl MinHarness {
    fn new(javac: &str, seed: u64) -> Self {
        let dir = std::env::temp_dir().join(format!("njavac-fuzz-min-{seed}"));
        reset_dir(&dir);
        MinHarness { javac: javac.to_string(), dir, spawns: 0, cap: 800 }
    }

    /// Compile a program with both compilers under its own (fixed) name.
    fn compile_both(&mut self, prog: &Prog) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        let src = render(prog);
        let out = self.dir.join("out");
        reset_dir(&out);
        let srcfile = self.dir.join(&prog.name.java_file);
        std::fs::write(&srcfile, &src).expect("write min source");
        self.spawns += 1;
        run_javac_one(&self.javac, &out, &srcfile);
        let want = std::fs::read(out.join(format!("{}.class", prog.name.class))).ok();
        let got = njavac_compile(&src, &prog.name.source_arg);
        (want, got)
    }

    /// The three-conjunct predicate: both accept AND the bytes still diverge.
    fn diverges(&mut self, prog: &Prog) -> bool {
        if self.spawns >= self.cap {
            return false;
        }
        match self.compile_both(prog) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        }
    }
}

/// Greedy statement-level reduction to a fixpoint (or the spawn cap): remove
/// top-level non-decl statements, drop `else` branches, and shrink branch bodies,
/// keeping only reductions that preserve the divergence. Decls are retained (they
/// define the locals; removing one would shift `Local` indices).
fn minimize(prog: &Prog, h: &mut MinHarness) -> Prog {
    let mut cur = prog.clone();
    loop {
        let mut improved = false;

        // (a) remove each top-level non-decl statement
        for i in (0..cur.body.len()).rev() {
            if matches!(cur.body[i], FStmt::Decl { .. }) {
                continue;
            }
            let mut cand = cur.clone();
            cand.body.remove(i);
            if h.diverges(&cand) {
                cur = cand;
                improved = true;
                break;
            }
        }
        if improved {
            continue;
        }

        // (b) drop `else` branches and shrink branch bodies
        for i in 0..cur.body.len() {
            if let FStmt::If { else_b: Some(_), .. } = &cur.body[i] {
                let mut cand = cur.clone();
                if let FStmt::If { else_b, .. } = &mut cand.body[i] {
                    *else_b = None;
                }
                if h.diverges(&cand) {
                    cur = cand;
                    improved = true;
                    break;
                }
            }
            // try shrinking each branch body by dropping its last statement
            if let FStmt::If { then_b, .. } = &cur.body[i] {
                if then_b.len() > 1 {
                    let mut cand = cur.clone();
                    if let FStmt::If { then_b, .. } = &mut cand.body[i] {
                        then_b.pop();
                    }
                    if h.diverges(&cand) {
                        cur = cand;
                        improved = true;
                        break;
                    }
                }
            }
        }
        if improved {
            continue;
        }

        break;
    }
    cur
}

// ===========================================================================
//    --dump-sources and --selftest (verification hooks)
// ===========================================================================

/// Print each generated source with no compilation — a javac-independent
/// "run twice, diff" determinism check and a generator eyeball.
fn dump_sources(cfg: &Config) {
    let mut g = Gen { rng: Rng::new(cfg.seed) };
    for k in 0..cfg.count {
        let prog = g.gen_prog(k);
        println!("// ===== {} =====", prog.name.class);
        print!("{}", render(&prog));
    }
}

/// Exercise the finding → minimize → diff_report → emit machinery without a real
/// bug: treat every *compilable* program as "interesting" (predicate ignores byte
/// equality) so minimize runs, then emit the minimized source plus a diff between
/// njavac's bytes and a one-byte-perturbed copy. Proves the plumbing fires and
/// localizes — NOT that ddmin converges on a real divergence.
fn selftest(cfg: &Config) -> i32 {
    println!("fuzz --selftest: exercising the finding/minimize/report pipeline");
    let mut g = Gen { rng: Rng::new(cfg.seed) };
    // Find a program both compilers accept.
    let mut h = SelftestHarness { inner: MinHarness::new(&cfg.javac, cfg.seed) };
    for k in 0..200 {
        let prog = g.gen_prog(k);
        let (want, got) = h.inner.compile_both(&prog);
        if let (Some(_), Some(mut bytes)) = (want, got) {
            // Minimize under the "compiles" predicate.
            let minimized = minimize_selftest(&prog, &mut h);
            let _ = std::fs::create_dir_all(&cfg.out_dir);
            let src = render(&minimized);
            let out_java = cfg.out_dir.join(&minimized.name.java_file);
            std::fs::write(&out_java, &src).expect("write selftest source");
            // Perturb one byte and localize.
            let (a, _) = h.inner.compile_both(&minimized);
            if let Some(a) = a {
                if !bytes.is_empty() {
                    let last = bytes.len() - 1;
                    bytes[last] ^= 0xFF;
                }
                if let Some(rep) = njavac::classdump::diff_report(&a, &bytes) {
                    let _ = std::fs::write(
                        cfg.out_dir.join(format!("{}.diff", minimized.name.class)),
                        &rep,
                    );
                }
            }
            println!("SELFTEST OK: minimized case + diff written to {}", cfg.out_dir.display());
            return 0;
        }
    }
    eprintln!("SELFTEST FAILED: no compilable program in 200 tries (generator broken?)");
    1
}

struct SelftestHarness {
    inner: MinHarness,
}

/// Statement reduction under the "both compile" predicate (byte equality ignored).
fn minimize_selftest(prog: &Prog, h: &mut SelftestHarness) -> Prog {
    let mut cur = prog.clone();
    let mut improved = true;
    while improved {
        improved = false;
        for i in (0..cur.body.len()).rev() {
            if matches!(cur.body[i], FStmt::Decl { .. }) {
                continue;
            }
            let mut cand = cur.clone();
            cand.body.remove(i);
            let (a, b) = h.inner.compile_both(&cand);
            if a.is_some() && b.is_some() && h.inner.spawns < h.inner.cap {
                cur = cand;
                improved = true;
                break;
            }
        }
    }
    cur
}
