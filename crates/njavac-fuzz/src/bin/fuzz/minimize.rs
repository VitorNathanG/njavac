use std::path::PathBuf;

use crate::Config;
use crate::generate::{Gen, Rng};
use crate::javac::{reset_dir, run_javac_one};
use crate::model::{FExpr, FStmt, PrintArg, Prog};
use crate::oracle::{njavac_compile, selftest_outcome_capture};
use crate::render::render;

pub(super) struct MinHarness {
    javac: String,
    dir: PathBuf,
    spawns: u32,
    cap: u32,
}

impl MinHarness {
    pub(super) fn new(javac: &str, seed: u64) -> Self {
        let dir = std::env::temp_dir().join(format!("njavac-fuzz-min-{seed}"));
        reset_dir(&dir);
        MinHarness {
            javac: javac.to_string(),
            dir,
            spawns: 0,
            cap: 800,
        }
    }

    /// Compile a program with both compilers under its own fixed name.
    pub(super) fn compile_both(&mut self, prog: &Prog) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        let src = render(prog);
        let out = self.dir.join("out");
        reset_dir(&out);
        let srcfile = self.dir.join(&prog.name.java_file);
        std::fs::write(&srcfile, &src).expect("write min source");
        self.spawns += 1;
        run_javac_one(&self.javac, &out, &srcfile);
        let want = std::fs::read(out.join(format!("{}.class", prog.name.class))).ok();
        let got = njavac_compile(&src, &prog.name.source_arg).accepted_bytes();
        (want, got)
    }
}

fn stmt_paren_reductions(stmt: &FStmt) -> Vec<FStmt> {
    let mut out = Vec::new();
    match stmt {
        FStmt::Decl { ty, local, init } => {
            if let Some(init) = init {
                for e in expr_paren_reductions(init) {
                    out.push(FStmt::Decl {
                        ty: *ty,
                        local: *local,
                        init: Some(e),
                    });
                }
            }
        }
        FStmt::Assign { local, value } => {
            for e in expr_paren_reductions(value) {
                out.push(FStmt::Assign {
                    local: *local,
                    value: e,
                });
            }
        }
        FStmt::Compound { local, op, value } => {
            for e in expr_paren_reductions(value) {
                out.push(FStmt::Compound {
                    local: *local,
                    op: *op,
                    value: e,
                });
            }
        }
        FStmt::Println(PrintArg::Expr(expr)) => {
            for e in expr_paren_reductions(expr) {
                out.push(FStmt::Println(PrintArg::Expr(e)));
            }
        }
        FStmt::If {
            cond,
            then_b,
            else_b,
        } => {
            for e in expr_paren_reductions(cond) {
                out.push(FStmt::If {
                    cond: e,
                    then_b: then_b.clone(),
                    else_b: else_b.clone(),
                });
            }
            for (i, child) in then_b.iter().enumerate() {
                for reduced in stmt_paren_reductions(child) {
                    let mut body = then_b.clone();
                    body[i] = reduced;
                    out.push(FStmt::If {
                        cond: cond.clone(),
                        then_b: body,
                        else_b: else_b.clone(),
                    });
                }
            }
            if let Some(else_body) = else_b {
                for (i, child) in else_body.iter().enumerate() {
                    for reduced in stmt_paren_reductions(child) {
                        let mut body = else_body.clone();
                        body[i] = reduced;
                        out.push(FStmt::If {
                            cond: cond.clone(),
                            then_b: then_b.clone(),
                            else_b: Some(body),
                        });
                    }
                }
            }
        }
        FStmt::IncDec { .. } | FStmt::Println(PrintArg::Str(_)) => {}
    }
    out
}

fn expr_paren_reductions(expr: &FExpr) -> Vec<FExpr> {
    let mut out = Vec::new();
    match expr {
        FExpr::Paren(inner) => {
            out.push((**inner).clone());
            for reduced in expr_paren_reductions(inner) {
                out.push(FExpr::Paren(Box::new(reduced)));
            }
        }
        FExpr::Neg(inner) => {
            for e in expr_paren_reductions(inner) {
                out.push(FExpr::Neg(Box::new(e)));
            }
        }
        FExpr::BitNot(inner) => {
            for e in expr_paren_reductions(inner) {
                out.push(FExpr::BitNot(Box::new(e)));
            }
        }
        FExpr::Not(inner) => {
            for e in expr_paren_reductions(inner) {
                out.push(FExpr::Not(Box::new(e)));
            }
        }
        FExpr::Cast(ty, inner) => {
            for e in expr_paren_reductions(inner) {
                out.push(FExpr::Cast(*ty, Box::new(e)));
            }
        }
        FExpr::Bin(op, left, right) => {
            for e in expr_paren_reductions(left) {
                out.push(FExpr::Bin(*op, Box::new(e), right.clone()));
            }
            for e in expr_paren_reductions(right) {
                out.push(FExpr::Bin(*op, left.clone(), Box::new(e)));
            }
        }
        FExpr::Cmp(op, left, right) => {
            for e in expr_paren_reductions(left) {
                out.push(FExpr::Cmp(*op, Box::new(e), right.clone()));
            }
            for e in expr_paren_reductions(right) {
                out.push(FExpr::Cmp(*op, left.clone(), Box::new(e)));
            }
        }
        FExpr::Logic(op, left, right) => {
            for e in expr_paren_reductions(left) {
                out.push(FExpr::Logic(*op, Box::new(e), right.clone()));
            }
            for e in expr_paren_reductions(right) {
                out.push(FExpr::Logic(*op, left.clone(), Box::new(e)));
            }
        }
        FExpr::Lit(_) | FExpr::Local(_) => {}
    }
    out
}

/// Exercise the finding → minimize → diff_report → emit machinery without a real
/// bug. This preserves the existing selftest predicate and artifact lifecycle.
pub(super) fn selftest(cfg: &Config) -> i32 {
    println!("fuzz --selftest: exercising the finding/minimize/report pipeline");
    if let Err(detail) = selftest_outcome_capture() {
        eprintln!("SELFTEST FAILED: candidate outcome capture: {detail}");
        return 1;
    }
    println!("  candidate outcome capture/classification passes");
    let mut g = Gen {
        rng: Rng::new(cfg.seed),
    };
    let mut h = SelftestHarness {
        inner: MinHarness::new(&cfg.javac, cfg.seed),
    };
    for k in 0..200 {
        let prog = g.gen_random_prog(k);
        let (want, got) = h.inner.compile_both(&prog);
        if let (Some(_), Some(mut bytes)) = (want, got) {
            let minimized = minimize_selftest(&prog, &mut h);
            let _ = std::fs::create_dir_all(&cfg.out_dir);
            let src = render(&minimized);
            let out_java = cfg.out_dir.join(&minimized.name.java_file);
            std::fs::write(&out_java, &src).expect("write selftest source");
            let (a, _) = h.inner.compile_both(&minimized);
            if let Some(a) = a {
                if !bytes.is_empty() {
                    let last = bytes.len() - 1;
                    bytes[last] ^= 0xFF;
                }
                if let Some(rep) = njavac_classdump::diff_report(&a, &bytes) {
                    let _ = std::fs::write(
                        cfg.out_dir.join(format!("{}.diff", minimized.name.class)),
                        &rep,
                    );
                }
            }
            println!(
                "SELFTEST OK: minimized case + diff written to {}",
                cfg.out_dir.display()
            );
            return 0;
        }
    }
    eprintln!("SELFTEST FAILED: no compilable program in 200 tries (generator broken?)");
    1
}

struct SelftestHarness {
    inner: MinHarness,
}

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
        if improved {
            continue;
        }
        'expressions: for i in 0..cur.body.len() {
            for reduced in stmt_paren_reductions(&cur.body[i]) {
                let mut cand = cur.clone();
                cand.body[i] = reduced;
                let (a, b) = h.inner.compile_both(&cand);
                if a.is_some() && b.is_some() && h.inner.spawns < h.inner.cap {
                    cur = cand;
                    improved = true;
                    break 'expressions;
                }
            }
        }
    }
    cur
}
