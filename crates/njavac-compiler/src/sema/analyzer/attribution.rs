use crate::ast::{BinOp, CallArgs, CmpOp, ExprArena, ExprId, ExprKind, PrimitiveType, Type};
use crate::diagnostic::{CompileResult, Diagnostic};
use crate::span::Span;

use super::super::constants::{eval_numeric_constant, is_constant_expression, NumericConst};
use super::super::{binary_promote, unary_promote, ResolvedCall};
use super::MethodAnalyzer;

fn is_numeric(ty: &Type) -> bool {
    ty.as_primitive().is_some_and(PrimitiveType::is_numeric)
}

fn is_integral(ty: &Type) -> bool {
    ty.as_primitive().is_some_and(PrimitiveType::is_integral)
}

fn is_assignment_convertible(target: &Type, source: &Type) -> bool {
    use PrimitiveType::*;
    let (Some(target), Some(source)) = (target.as_primitive(), source.as_primitive()) else {
        return target == source;
    };
    target == source
        || matches!(
            (source, target),
            (Byte, Short | Int | Long | Float | Double)
                | (Short, Int | Long | Float | Double)
                | (Char, Int | Long | Float | Double)
                | (Int, Long | Float | Double)
                | (Long, Float | Double)
                | (Float, Double)
        )
}

impl MethodAnalyzer {
    pub(super) fn validate_expr(
        &mut self,
        expr: ExprId,
        span: Span,
        exprs: &ExprArena,
    ) -> CompileResult<Type> {
        let mut resolved_call = None;
        let ty = match &exprs[expr] {
            ExprKind::IntLit(_) => PrimitiveType::Int.into(),
            ExprKind::LongLit(_) => PrimitiveType::Long.into(),
            ExprKind::FloatLit(_) => PrimitiveType::Float.into(),
            ExprKind::DoubleLit(_) => PrimitiveType::Double.into(),
            ExprKind::BoolLit(_) => PrimitiveType::Boolean.into(),
            ExprKind::CharLit(_) => PrimitiveType::Char.into(),
            ExprKind::StringLit(_) => Type::string(),
            ExprKind::Name(name) => {
                let (_, ty) = self.read_local(name)?;
                if ty.as_primitive().is_none() {
                    return Err(Diagnostic::unsupported_semantic(
                        name.span,
                        "using the String[] parameter as a value is unsupported",
                    ));
                }
                ty
            }
            ExprKind::Select { .. } => {
                return Err(Diagnostic::semantic(
                    span,
                    "a qualified name is only supported as a method-call target",
                ));
            }
            ExprKind::Neg(inner) => {
                let ty = self.validate_expr(*inner, span, exprs)?;
                self.require_numeric(&ty, span, "unary `-`")?;
                unary_promote(ty.primitive()).into()
            }
            ExprKind::BitNot(inner) => {
                let ty = self.validate_expr(*inner, span, exprs)?;
                self.require_integral(&ty, span, "unary `~`")?;
                unary_promote(ty.primitive()).into()
            }
            ExprKind::Not(inner) => {
                let ty = self.validate_expr(*inner, span, exprs)?;
                self.require_boolean(&ty, span, "unary `!`")?;
                PrimitiveType::Boolean.into()
            }
            ExprKind::Paren(inner) => self.validate_expr(*inner, span, exprs)?,
            ExprKind::Cast { ty, expr } => {
                let source = self.validate_expr(*expr, span, exprs)?;
                let target = ty.clone();
                if !((is_numeric(&source) && is_numeric(&target))
                    || (source.is_boolean() && target.is_boolean()))
                {
                    return Err(Diagnostic::semantic(span, "invalid primitive cast"));
                }
                target
            }
            ExprKind::Binary { op, left, right } => {
                let left_ty = self.validate_expr(*left, span, exprs)?;
                let right_ty = self.validate_expr(*right, span, exprs)?;
                self.validate_binary(*op, &left_ty, &right_ty, *right, span, exprs)?
            }
            ExprKind::Compare { op, left, right } => {
                let left_ty = self.validate_expr(*left, span, exprs)?;
                let right_ty = self.validate_expr(*right, span, exprs)?;
                self.validate_compare(*op, &left_ty, &right_ty, span)?;
                PrimitiveType::Boolean.into()
            }
            ExprKind::Logical { left, right, .. } => {
                let left_ty = self.validate_expr(*left, span, exprs)?;
                let right_ty = self.validate_expr(*right, span, exprs)?;
                self.require_boolean(&left_ty, span, "logical operator")?;
                self.require_boolean(&right_ty, span, "logical operator")?;
                PrimitiveType::Boolean.into()
            }
            ExprKind::Call { target, args } => {
                let call = self.resolve_call(*target, args, span, exprs)?;
                let return_type = call.return_type();
                resolved_call = Some(call);
                return_type
            }
        };
        let base = *self.expr_type_base.get_or_insert(expr.index());
        let index = expr
            .index()
            .checked_sub(base)
            .expect("expression validation order crossed method boundaries");
        if self.expr_types.len() <= index {
            self.expr_types.resize_with(index + 1, || None);
        }
        match &self.expr_types[index] {
            Some(previous) => assert_eq!(previous, &ty, "expression type changed between visits"),
            None => self.expr_types[index] = Some(ty.clone()),
        }
        if let Some(call) = resolved_call {
            if let Some((_, previous)) = self.calls.iter().find(|(id, _)| *id == expr) {
                assert_eq!(previous, &call, "call resolution changed between visits");
            } else {
                self.calls.push((expr, call));
            }
        }
        Ok(ty)
    }

    /// Resolve the current subset's library call after parsing has preserved it as
    /// an ordinary dotted invocation. The selected parameter type records overload
    /// resolution (`byte`/`short` use `println(int)`).
    fn resolve_call(
        &mut self,
        target: ExprId,
        args: &CallArgs,
        span: Span,
        exprs: &ExprArena,
    ) -> CompileResult<ResolvedCall> {
        if !qualified_name_is(exprs, target, &["System", "out", "println"]) {
            return Err(Diagnostic::unsupported_semantic(
                qualified_name_span(exprs, target).unwrap_or(span),
                "only System.out.println(...) calls are supported",
            ));
        }
        if args.len() != 1 {
            return Err(Diagnostic::unsupported_semantic(
                span,
                "System.out.println requires exactly one argument in the supported subset",
            ));
        }

        let argument_expr = args.first.expect("one call argument checked above");
        let argument = self.validate_expr(argument_expr, span, exprs)?;
        if argument.is_string() && !is_string_value(exprs, argument_expr) {
            return Err(Diagnostic::unsupported_semantic(
                span,
                "only string literals are supported as String values",
            ));
        }
        let parameter = match argument.as_primitive() {
            Some(PrimitiveType::Byte | PrimitiveType::Short) => PrimitiveType::Int.into(),
            Some(_) => argument,
            None if argument.is_string() => argument,
            None => {
                return Err(Diagnostic::unsupported_semantic(
                    span,
                    "unsupported println argument type",
                ));
            }
        };
        Ok(ResolvedCall::Println { parameter_type: parameter })
    }

    fn validate_binary(
        &self,
        op: BinOp,
        left: &Type,
        right: &Type,
        right_expr: ExprId,
        span: Span,
        exprs: &ExprArena,
    ) -> CompileResult<Type> {
        if op == BinOp::Add && (left.is_string() || right.is_string()) {
            return Err(Diagnostic::unsupported_semantic(
                span,
                "String concatenation is unsupported",
            ));
        }
        if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor)
            && left.is_boolean()
            && right.is_boolean()
        {
            return Ok(PrimitiveType::Boolean.into());
        }
        if op.is_shift() {
            self.require_integral(left, span, "shift operator")?;
            self.require_integral(right, span, "shift operator")?;
            return Ok(unary_promote(left.primitive()).into());
        }
        if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor) {
            self.require_integral(left, span, "bitwise operator")?;
            self.require_integral(right, span, "bitwise operator")?;
        } else {
            self.require_numeric(left, span, "arithmetic operator")?;
            self.require_numeric(right, span, "arithmetic operator")?;
        }
        let result = binary_promote(left.primitive(), right.primitive());
        if matches!(op, BinOp::Div | BinOp::Rem)
            && result.is_integral()
            && eval_numeric_constant(exprs, right_expr).is_some_and(NumericConst::is_zero)
        {
            return Err(Diagnostic::semantic(
                span,
                "integral division or remainder by zero",
            ));
        }
        Ok(result.into())
    }

    fn validate_compare(
        &self,
        op: CmpOp,
        left: &Type,
        right: &Type,
        span: Span,
    ) -> CompileResult<()> {
        if matches!(op, CmpOp::Eq | CmpOp::Ne) {
            if left.is_string() && right.is_string() {
                return Err(Diagnostic::unsupported_semantic(
                    span,
                    "reference comparison is unsupported",
                ));
            }
            if (is_numeric(left) && is_numeric(right))
                || (left.is_boolean() && right.is_boolean())
            {
                return Ok(());
            }
            return Err(Diagnostic::semantic(span, "invalid equality operands"));
        }
        self.require_numeric(left, span, "relational operator")?;
        self.require_numeric(right, span, "relational operator")
    }

    pub(super) fn require_assignable(
        &self,
        target: &Type,
        source: &Type,
        expr: ExprId,
        span: Span,
        exprs: &ExprArena,
    ) -> CompileResult<()> {
        if target.as_primitive().is_none() {
            return Err(Diagnostic::semantic(
                span,
                "cannot assign a value to the String[] parameter",
            ));
        }
        if is_assignment_convertible(target, source)
            || (is_integral(target)
                && matches!(
                    source.as_primitive(),
                    Some(
                        PrimitiveType::Int
                            | PrimitiveType::Byte
                            | PrimitiveType::Short
                            | PrimitiveType::Char
                    )
                )
                && is_constant_expression(exprs, expr))
        {
            return Ok(());
        }
        Err(Diagnostic::semantic(
            span,
            format!("cannot assign {source:?} to {target:?}"),
        ))
    }

    pub(super) fn require_compound(
        &self,
        op: BinOp,
        target: &Type,
        source: &Type,
        span: Span,
    ) -> CompileResult<()> {
        let valid = if op.is_shift() {
            is_integral(target) && is_integral(source)
        } else if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor) {
            (is_integral(target) && is_integral(source))
                || (target.is_boolean() && source.is_boolean())
        } else {
            is_numeric(target) && is_numeric(source)
        };
        if valid {
            Ok(())
        } else {
            Err(Diagnostic::semantic(span, "invalid compound assignment operands"))
        }
    }

    fn require_numeric(&self, ty: &Type, span: Span, context: &str) -> CompileResult<()> {
        if is_numeric(ty) {
            Ok(())
        } else {
            Err(Diagnostic::semantic(span, format!("{context} requires numeric operands")))
        }
    }

    fn require_integral(&self, ty: &Type, span: Span, context: &str) -> CompileResult<()> {
        if is_integral(ty) {
            Ok(())
        } else {
            Err(Diagnostic::semantic(span, format!("{context} requires integral operands")))
        }
    }

    fn require_boolean(&self, ty: &Type, span: Span, context: &str) -> CompileResult<()> {
        if ty.is_boolean() {
            Ok(())
        } else {
            Err(Diagnostic::semantic(span, format!("{context} requires boolean operands")))
        }
    }
}

fn is_string_value(exprs: &ExprArena, expr: ExprId) -> bool {
    match &exprs[expr] {
        ExprKind::StringLit(_) => true,
        ExprKind::Paren(inner) => is_string_value(exprs, *inner),
        _ => false,
    }
}

fn qualified_name_is(exprs: &ExprArena, expr: ExprId, parts: &[&str]) -> bool {
    match (&exprs[expr], parts) {
        (ExprKind::Name(name), [part]) => name.text == *part,
        (ExprKind::Select { qualifier, name }, [prefix @ .., part]) => {
            name.text == *part && qualified_name_is(exprs, *qualifier, prefix)
        }
        _ => false,
    }
}

fn qualified_name_span(exprs: &ExprArena, expr: ExprId) -> Option<Span> {
    match &exprs[expr] {
        ExprKind::Name(name) | ExprKind::Select { name, .. } => Some(name.span),
        _ => None,
    }
}
