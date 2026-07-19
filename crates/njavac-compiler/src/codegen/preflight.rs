use crate::ast::{CompilationUnit, ExprArena, ExprId, ExprKind, LogOp, PrimitiveType, Stmt, StmtKind};
use crate::diagnostic::{CompileResult, Diagnostic};
use crate::sema::{Analysis, MethodInfo};
use crate::span::Span;

use super::constant::{fold, lowering_const, to_i32};

/// Reject the one valid-Java value shape that needs verifier frames the emitter
/// cannot yet represent: materializing a branch boolean over a live base stack.
/// This runs before constant-pool interning or byte emission; the corresponding
/// emitter assert remains a post-preflight invariant.
pub(super) fn preflight_codegen(
    unit: &CompilationUnit,
    analysis: &Analysis,
) -> CompileResult<()> {
    for (method, info) in unit.class.methods.iter().zip(&analysis.methods) {
        for stmt in &method.body {
            preflight_stmt(stmt, info, &unit.exprs)?;
        }
    }
    Ok(())
}

fn preflight_stmt(stmt: &Stmt, info: &MethodInfo, exprs: &ExprArena) -> CompileResult<()> {
    match &stmt.kind {
        StmtKind::LocalDecl {
            name,
            init: Some(init),
            ..
        }
        | StmtKind::Assign { name, value: init } => {
            if info.ty(name) == PrimitiveType::Boolean {
                preflight_materialization(*init, false, stmt.span, info, exprs)?;
            } else {
                preflight_value(*init, false, stmt.span, info, exprs)?;
            }
        }
        StmtKind::LocalDecl { init: None, .. } => {}
        StmtKind::CompoundAssign { value, .. } => {
            // The target value is loaded before the RHS except when folding makes
            // the RHS code-free; `preflight_value` applies that same fold first.
            preflight_value(*value, true, stmt.span, info, exprs)?;
        }
        StmtKind::Expr(expr) => match &exprs[*expr] {
            ExprKind::Call { args, .. } => {
                // The resolved receiver remains live while every argument is evaluated.
                for arg in args.iter() {
                    preflight_value(arg, true, stmt.span, info, exprs)?;
                }
            }
            _ => unreachable!("sema accepted a non-call expression statement"),
        },
        StmtKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            preflight_cond(*cond, false, stmt.span, info, exprs)?;
            for nested in &then_branch.stmts {
                preflight_stmt(nested, info, exprs)?;
            }
            for nested in else_branch.iter().flat_map(|body| &body.stmts) {
                preflight_stmt(nested, info, exprs)?;
            }
        }
    }
    Ok(())
}

/// Mirror `gen_value`'s left-to-right evaluation enough to track whether a
/// branch-valued boolean reaches `gen_bool_value` with another value live.
fn preflight_value(
    expr: ExprId,
    base_live: bool,
    span: Span,
    info: &MethodInfo,
    exprs: &ExprArena,
) -> CompileResult<()> {
    if matches!(&exprs[expr], ExprKind::StringLit(_)) || fold(exprs, expr).is_some() {
        return Ok(());
    }
    match &exprs[expr] {
        ExprKind::Name(_) => Ok(()),
        ExprKind::Select { .. } => unreachable!("sema accepted a selection as a value"),
        ExprKind::Neg(inner) | ExprKind::BitNot(inner) | ExprKind::Paren(inner) => {
            preflight_value(*inner, base_live, span, info, exprs)
        }
        ExprKind::Cast { expr, .. } => preflight_value(*expr, base_live, span, info, exprs),
        ExprKind::Binary { left, right, .. } => {
            preflight_value(*left, base_live, span, info, exprs)?;
            preflight_value(*right, true, span, info, exprs)
        }
        ExprKind::Compare { .. } | ExprKind::Not(_) | ExprKind::Logical { .. } => {
            preflight_materialization(expr, base_live, span, info, exprs)
        }
        ExprKind::IntLit(_)
        | ExprKind::LongLit(_)
        | ExprKind::FloatLit(_)
        | ExprKind::DoubleLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::CharLit(_) => Ok(()),
        ExprKind::Call { .. } => unreachable!("sema accepted a void call as a value"),
        ExprKind::StringLit(_) => unreachable!("handled above"),
    }
}

fn preflight_materialization(
    expr: ExprId,
    base_live: bool,
    span: Span,
    info: &MethodInfo,
    exprs: &ExprArena,
) -> CompileResult<()> {
    if base_live {
        return Err(Diagnostic::unsupported_codegen(
            span,
            "boolean value materialization with a live operand-stack value is unsupported",
        ));
    }
    preflight_cond(expr, false, span, info, exprs)
}

/// Mirror condition lowering: comparisons evaluate operands as values, logical
/// operators consume the left test before evaluating the right, and a boolean
/// cast explicitly materializes its operand.
fn preflight_cond(
    expr: ExprId,
    base_live: bool,
    span: Span,
    info: &MethodInfo,
    exprs: &ExprArena,
) -> CompileResult<()> {
    if lowering_const(exprs, expr).is_some() {
        return Ok(());
    }
    match &exprs[expr] {
        ExprKind::Not(inner) | ExprKind::Paren(inner) => {
            preflight_cond(*inner, base_live, span, info, exprs)
        }
        ExprKind::Cast { ty, expr } if ty.is_boolean() => {
            preflight_materialization(*expr, base_live, span, info, exprs)
        }
        ExprKind::Compare { left, right, .. } => {
            preflight_value(*left, base_live, span, info, exprs)?;
            preflight_value(*right, true, span, info, exprs)
        }
        ExprKind::Logical { op, left, right } => {
            preflight_cond(*left, base_live, span, info, exprs)?;
            let left_decides = fold(exprs, *left).is_some_and(|value| match op {
                LogOp::And => to_i32(value) == 0,
                LogOp::Or => to_i32(value) != 0,
            });
            if left_decides {
                Ok(())
            } else {
                preflight_cond(*right, base_live, span, info, exprs)
            }
        }
        _ => preflight_value(expr, base_live, span, info, exprs),
    }
}
