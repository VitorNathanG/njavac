use super::{is_primitive_type, Parser};
use crate::ast::{BinOp, CallArgs, CmpOp, ExprId, ExprKind, LogOp, Name};
use crate::diagnostic::{CompileResult, Diagnostic};
use crate::lexer::TokenKind;

impl Parser {
    pub(super) fn expression(&mut self) -> CompileResult<ExprId> {
        self.expression_bp(0)
    }

    /// Parse every binary/logical level from one binding-power table. All current
    /// operators are left-associative, so the right binding power is one greater
    /// than the left (`a+b+c` becomes `(a+b)+c`).
    fn expression_bp(&mut self, min_bp: u8) -> CompileResult<ExprId> {
        let mut left = self.unary()?;
        loop {
            let Some((op, left_bp, right_bp)) = infix_binding_power(self.peek()) else {
                break;
            };
            if left_bp < min_bp {
                break;
            }
            self.bump();
            let right = self.expression_bp(right_bp)?;
            let kind = op.apply(left, right);
            left = self.expr(kind);
        }
        Ok(left)
    }

    // unary -> '-' unary | '~' unary | '(' primitive ')' unary | primary
    fn unary(&mut self) -> CompileResult<ExprId> {
        let expr = match self.peek() {
            TokenKind::Minus => {
                self.bump();
                let inner = self.unary()?;
                self.expr(ExprKind::Neg(inner))
            }
            TokenKind::Tilde => {
                self.bump();
                let inner = self.unary()?;
                self.expr(ExprKind::BitNot(inner))
            }
            TokenKind::Bang => {
                self.bump();
                let inner = self.unary()?;
                self.expr(ExprKind::Not(inner))
            }
            TokenKind::LParen if self.is_cast() => {
                self.bump(); // (
                let ty = self.primitive_type()?;
                self.expect(&TokenKind::RParen)?;
                let inner = self.unary()?;
                self.expr(ExprKind::Cast { ty, expr: inner })
            }
            _ => return self.primary(),
        };
        Ok(expr)
    }

    /// A `(` begins a cast iff it is immediately followed by a primitive type
    /// keyword and a `)` — reference casts are out of the subset, so this is
    /// unambiguous against a parenthesized expression.
    fn is_cast(&self) -> bool {
        is_primitive_type(self.peek_kind(1)) && matches!(self.peek_kind(2), TokenKind::RParen)
    }

    // primary -> literal | '(' expression ')' | qualified-call | name
    fn primary(&mut self) -> CompileResult<ExprId> {
        let token = self.bump();
        let expr = match token.kind {
            TokenKind::IntLit(v) => self.expr(ExprKind::IntLit(v)),
            TokenKind::LongLit(v) => self.expr(ExprKind::LongLit(v)),
            TokenKind::FloatLit(v) => self.expr(ExprKind::FloatLit(v)),
            TokenKind::DoubleLit(v) => self.expr(ExprKind::DoubleLit(v)),
            TokenKind::CharLit(v) => self.expr(ExprKind::CharLit(v)),
            TokenKind::True => self.expr(ExprKind::BoolLit(true)),
            TokenKind::False => self.expr(ExprKind::BoolLit(false)),
            TokenKind::StringLit(s) => self.expr(ExprKind::StringLit(s)),
            TokenKind::LParen => {
                let inner = self.expression()?;
                self.expect(&TokenKind::RParen)?;
                self.expr(ExprKind::Paren(inner))
            }
            TokenKind::Ident(name) => self.name_or_call(Name { text: name, span: token.span })?,
            other => {
                return Err(Diagnostic::parse(
                    token.span,
                    format!("unexpected token in expression: {:?}", other),
                ));
            }
        };
        Ok(expr)
    }

    /// Parse a local name or a dotted method invocation without resolving any
    /// component. Sema owns the supported-target and overload decisions.
    fn name_or_call(&mut self, first: Name) -> CompileResult<ExprId> {
        let mut target = self.expr(ExprKind::Name(first));
        let mut qualified = false;
        while matches!(self.peek(), TokenKind::Dot) {
            self.bump();
            let name = self.expect_name()?;
            target = self.expr(ExprKind::Select {
                qualifier: target,
                name,
            });
            qualified = true;
        }
        if !matches!(self.peek(), TokenKind::LParen) {
            if !qualified {
                return Ok(target);
            }
            return Err(Diagnostic::parse(
                self.previous_span(),
                "a qualified name is only supported as a method-call target",
            ));
        }

        self.bump();
        let mut first = None;
        let mut rest = Vec::new();
        if !matches!(self.peek(), TokenKind::RParen) {
            first = Some(self.expression()?);
            while matches!(self.peek(), TokenKind::Comma) {
                self.bump();
                rest.push(self.expression()?);
            }
        }
        self.expect(&TokenKind::RParen)?;
        Ok(self.expr(ExprKind::Call {
            target,
            args: CallArgs { first, rest },
        }))
    }
}

#[derive(Clone, Copy)]
enum InfixOp {
    Binary(BinOp),
    Compare(CmpOp),
    Logical(LogOp),
}

impl InfixOp {
    fn apply(self, left: ExprId, right: ExprId) -> ExprKind {
        match self {
            InfixOp::Binary(op) => ExprKind::Binary { op, left, right },
            InfixOp::Compare(op) => ExprKind::Compare { op, left, right },
            InfixOp::Logical(op) => ExprKind::Logical { op, left, right },
        }
    }
}

fn infix_binding_power(kind: &TokenKind) -> Option<(InfixOp, u8, u8)> {
    let (op, precedence) = match kind {
        TokenKind::PipePipe => (InfixOp::Logical(LogOp::Or), 1),
        TokenKind::AmpAmp => (InfixOp::Logical(LogOp::And), 2),
        TokenKind::Pipe => (InfixOp::Binary(BinOp::Or), 3),
        TokenKind::Caret => (InfixOp::Binary(BinOp::Xor), 4),
        TokenKind::Amp => (InfixOp::Binary(BinOp::And), 5),
        TokenKind::EqEq => (InfixOp::Compare(CmpOp::Eq), 6),
        TokenKind::NotEq => (InfixOp::Compare(CmpOp::Ne), 6),
        TokenKind::Lt => (InfixOp::Compare(CmpOp::Lt), 7),
        TokenKind::Le => (InfixOp::Compare(CmpOp::Le), 7),
        TokenKind::Gt => (InfixOp::Compare(CmpOp::Gt), 7),
        TokenKind::Ge => (InfixOp::Compare(CmpOp::Ge), 7),
        TokenKind::Shl => (InfixOp::Binary(BinOp::Shl), 8),
        TokenKind::Shr => (InfixOp::Binary(BinOp::Shr), 8),
        TokenKind::UShr => (InfixOp::Binary(BinOp::UShr), 8),
        TokenKind::Plus => (InfixOp::Binary(BinOp::Add), 9),
        TokenKind::Minus => (InfixOp::Binary(BinOp::Sub), 9),
        TokenKind::Star => (InfixOp::Binary(BinOp::Mul), 10),
        TokenKind::Slash => (InfixOp::Binary(BinOp::Div), 10),
        TokenKind::Percent => (InfixOp::Binary(BinOp::Rem), 10),
        _ => return None,
    };
    Some((op, precedence, precedence + 1))
}
