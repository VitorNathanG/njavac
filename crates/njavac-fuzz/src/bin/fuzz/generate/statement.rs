use super::expression::{INTEGRAL, NUMERIC};
use super::{BoolMode, Gen, CAPS};
use crate::model::{ident, BinOp, CmpOp, FExpr, FStmt, LogOp, PrintArg, Prog, Ty, Val};
use Ty::*;

const STRINGS: [&str; 6] = ["", "x", "hello", "a b c", "12345", "Java"];

impl Gen {
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
        out.extend(observed.map(|local| FStmt::Println(PrintArg::Expr(FExpr::Local(local)))));
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
        FStmt::Decl { ty, local, init: Some(init) }
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
                BinOp::Add,
                BinOp::Sub,
                BinOp::Mul,
                BinOp::Div,
                BinOp::Rem,
                BinOp::BAnd,
                BinOp::BOr,
                BinOp::BXor,
                BinOp::Shl,
                BinOp::Shr,
                BinOp::Ushr,
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

    fn definite_condition(&mut self, env: &[Ty], verdict: bool) -> FExpr {
        match self.rng.below(6) {
            0 => FExpr::Lit(Val::Bool(verdict)),
            1 => FExpr::Not(Box::new(FExpr::Lit(Val::Bool(!verdict)))),
            2 => FExpr::Paren(Box::new(FExpr::Lit(Val::Bool(verdict)))),
            3 => FExpr::Cast(Boolean, Box::new(FExpr::Lit(Val::Bool(verdict)))),
            4 => {
                let (left, right) = if verdict { (1, 2) } else { (2, 1) };
                FExpr::Cmp(
                    CmpOp::Lt,
                    Box::new(FExpr::Lit(Val::I(left))),
                    Box::new(FExpr::Lit(Val::I(right))),
                )
            }
            _ => {
                let left = self
                    .local_of(env, |ty| ty == Boolean)
                    .map(FExpr::Local)
                    .unwrap_or(FExpr::Lit(Val::Bool(self.rng.boolean())));
                if verdict {
                    FExpr::Logic(
                        LogOp::Or,
                        Box::new(left),
                        Box::new(FExpr::Lit(Val::Bool(true))),
                    )
                } else {
                    FExpr::Logic(
                        LogOp::And,
                        Box::new(left),
                        Box::new(FExpr::Lit(Val::Bool(false))),
                    )
                }
            }
        }
    }

    fn gen_definite_assignment_path(&mut self, env: &mut Vec<Ty>, out: &mut Vec<FStmt>) {
        if self.rng.boolean() {
            let ty = *self.rng.pick(&[Int, Long, Float, Double, Boolean, Char, Byte, Short]);
            let local = env.len();
            let prior_env = env.clone();
            env.push(ty);
            out.push(FStmt::Decl { ty, local, init: None });

            let verdict = self.rng.boolean();
            let cond = self.definite_condition(&prior_env, verdict);
            let mut budget = self.fresh_budget();
            let mode = if ty == Boolean { BoolMode::Branch } else { BoolMode::Value };
            let assign = FStmt::Assign {
                local,
                value: self.expr(&prior_env, ty, mode, &mut budget),
            };
            let dead_read = FStmt::Println(PrintArg::Expr(FExpr::Local(local)));
            let (then_b, else_b) = if verdict {
                (vec![assign], Some(vec![dead_read]))
            } else {
                (vec![dead_read], Some(vec![assign]))
            };
            Self::push_observed(out, FStmt::If { cond, then_b, else_b }, env.len());
        } else {
            let local = env.len();
            let prior_env = env.clone();
            env.push(Boolean);
            out.push(FStmt::Decl { ty: Boolean, local, init: None });

            let verdict = self.rng.boolean();
            let runtime = self
                .local_of(&prior_env, |ty| ty == Boolean)
                .map(FExpr::Local)
                .unwrap_or(FExpr::Lit(Val::Bool(self.rng.boolean())));
            let deciding = if verdict {
                FExpr::Logic(
                    LogOp::Or,
                    Box::new(runtime),
                    Box::new(FExpr::Lit(Val::Bool(true))),
                )
            } else {
                FExpr::Logic(
                    LogOp::And,
                    Box::new(runtime),
                    Box::new(FExpr::Lit(Val::Bool(false))),
                )
            };
            let deciding = match self.rng.below(3) {
                0 => deciding,
                1 => FExpr::Paren(Box::new(deciding)),
                _ => FExpr::Cast(Boolean, Box::new(deciding)),
            };
            let cond = if verdict {
                FExpr::Logic(
                    LogOp::Or,
                    Box::new(deciding),
                    Box::new(FExpr::Local(local)),
                )
            } else {
                FExpr::Logic(
                    LogOp::And,
                    Box::new(deciding),
                    Box::new(FExpr::Local(local)),
                )
            };
            out.push(FStmt::If {
                cond,
                then_b: vec![FStmt::Println(PrintArg::Str("then".to_string()))],
                else_b: Some(vec![FStmt::Println(PrintArg::Str("else".to_string()))]),
            });
            Self::push_observed(
                out,
                FStmt::Assign { local, value: FExpr::Lit(Val::Bool(self.rng.boolean())) },
                env.len(),
            );
        }
    }

    pub(crate) fn gen_prog(&mut self, n: u64) -> Prog {
        let mut env: Vec<Ty> = Vec::new();
        let nstmt = 5 + self.rng.below(10);
        let mut body = Vec::with_capacity(nstmt);
        for i in 0..nstmt {
            let stmt = if i < 2 { self.gen_decl(&mut env) } else { self.top_stmt(&mut env, 0) };
            Self::push_observed(&mut body, stmt, env.len());
        }
        if CAPS.definite_assignment_paths && self.rng.ratio(1, 3) {
            self.gen_definite_assignment_path(&mut env, &mut body);
        }
        Prog { name: ident(n), locals: env, body }
    }
}
