use crate::model::{BinOp, CmpOp, FExpr, FStmt, LogOp, PrintArg, Prog, Val};

pub(super) fn render(prog: &Prog) -> String {
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
            if let Some(init) = init {
                out.push_str(&format!("{pad}{} v{} = {};\n", ty.kw(), local, render_expr(init)));
            } else {
                out.push_str(&format!("{pad}{} v{};\n", ty.kw(), local));
            }
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
    render_expr_at(e, 0, false)
}

/// Render only grammar-required parentheses; `FExpr::Paren` is the sole source of
/// deliberate grouping. This distinction is byte-visible for boolean lowering.
fn render_expr_at(e: &FExpr, parent_prec: u8, right_child: bool) -> String {
    let prec = expr_prec(e);
    let body = match e {
        FExpr::Lit(v) => render_val(v),
        FExpr::Local(i) => format!("v{i}"),
        FExpr::Neg(x) => format!("- {}", render_expr_at(x, prec, false)),
        FExpr::BitNot(x) => format!("~{}", render_expr_at(x, prec, false)),
        FExpr::Not(x) => format!("!{}", render_expr_at(x, prec, false)),
        FExpr::Paren(x) => format!("({})", render_expr_at(x, 0, false)),
        FExpr::Cast(ty, x) => format!("({}) {}", ty.kw(), render_expr_at(x, prec, false)),
        FExpr::Bin(op, l, r) => format!(
            "{} {} {}",
            render_expr_at(l, prec, false),
            op.sym(),
            render_expr_at(r, prec, true)
        ),
        FExpr::Cmp(op, l, r) => format!(
            "{} {} {}",
            render_expr_at(l, prec, false),
            op.sym(),
            render_expr_at(r, prec, true)
        ),
        FExpr::Logic(op, l, r) => {
            let s = match op {
                LogOp::And => "&&",
                LogOp::Or => "||",
            };
            format!(
                "{} {} {}",
                render_expr_at(l, prec, false),
                s,
                render_expr_at(r, prec, true)
            )
        }
    };

    if !matches!(e, FExpr::Paren(_))
        && (prec < parent_prec || (right_child && prec == parent_prec))
    {
        format!("({body})")
    } else {
        body
    }
}

fn expr_prec(e: &FExpr) -> u8 {
    match e {
        FExpr::Logic(LogOp::Or, ..) => 1,
        FExpr::Logic(LogOp::And, ..) => 2,
        FExpr::Bin(BinOp::BOr, ..) => 3,
        FExpr::Bin(BinOp::BXor, ..) => 4,
        FExpr::Bin(BinOp::BAnd, ..) => 5,
        FExpr::Cmp(CmpOp::Eq | CmpOp::Ne, ..) => 6,
        FExpr::Cmp(..) => 7,
        FExpr::Bin(BinOp::Shl | BinOp::Shr | BinOp::Ushr, ..) => 8,
        FExpr::Bin(BinOp::Add | BinOp::Sub, ..) => 9,
        FExpr::Bin(BinOp::Mul | BinOp::Div | BinOp::Rem, ..) => 10,
        FExpr::Neg(_) | FExpr::BitNot(_) | FExpr::Not(_) | FExpr::Cast(..) => 11,
        FExpr::Lit(_) | FExpr::Local(_) | FExpr::Paren(_) => 12,
    }
}

fn render_val(v: &Val) -> String {
    match v {
        Val::I(x) => int_str(*x),
        Val::L(x) => {
            if *x < 0 { format!("-{}L", x.unsigned_abs()) } else { format!("{x}L") }
        }
        Val::F(bits) => float_str(*bits),
        Val::D(bits) => double_str(*bits),
        Val::Bool(b) => b.to_string(),
        Val::C(c) => char_str(*c),
    }
}

fn int_str(x: i32) -> String {
    if x < 0 { format!("-{}", x.unsigned_abs()) } else { x.to_string() }
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
