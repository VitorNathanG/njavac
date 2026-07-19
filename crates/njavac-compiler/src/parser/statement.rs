use super::{is_primitive_type, Parser};
use crate::ast::{BinOp, BranchBody, ExprKind, Stmt, StmtKind};
use crate::diagnostic::{CompileResult, Diagnostic};
use crate::lexer::TokenKind;

impl Parser {
    // A single statement.
    pub(super) fn statement(&mut self) -> CompileResult<Stmt> {
        let line = self.line();
        let start = self.span();
        let kind = if matches!(self.peek(), TokenKind::If) {
            self.if_statement()?
        } else if is_primitive_type(self.peek()) {
            self.local_decl()?
        } else if matches!(self.peek(), TokenKind::PlusPlus | TokenKind::MinusMinus) {
            // Prefix `++x;` / `--x;` — in statement position the produced value is
            // discarded, so pre/post is irrelevant.
            let op = if matches!(self.peek(), TokenKind::PlusPlus) {
                BinOp::Add
            } else {
                BinOp::Sub
            };
            self.bump();
            let name = self.expect_name()?;
            self.expect(&TokenKind::Semicolon)?;
            let value = self.expr(ExprKind::IntLit(1));
            StmtKind::CompoundAssign { name, op, value }
        } else if let TokenKind::Ident(name) = self.peek() {
            if is_unsupported_statement_keyword(name) {
                return Err(Diagnostic::unsupported_syntax(
                    self.span(),
                    format!("unsupported Java statement: {name}"),
                ));
            }
            self.ident_statement()?
        } else {
            return Err(Diagnostic::parse(
                self.span(),
                format!("unexpected statement start: {:?}", self.peek()),
            ));
        };
        let span = start.join(self.previous_span());
        Ok(Stmt { span, line, kind })
    }

    // `if (cond) <then> [else <else>]`. Each arm is a brace-block or a single
    // statement; `else if` falls out naturally as an `If` in the else arm.
    fn if_statement(&mut self) -> CompileResult<StmtKind> {
        self.expect(&TokenKind::If)?;
        self.expect(&TokenKind::LParen)?;
        let cond = self.expression()?;
        self.expect(&TokenKind::RParen)?;
        let then_branch = self.block_or_statement()?;
        let else_branch = if matches!(self.peek(), TokenKind::Else) {
            self.bump();
            Some(self.block_or_statement()?)
        } else {
            None
        };
        Ok(StmtKind::If { cond, then_branch, else_branch })
    }

    // A brace-delimited block, or a single statement (Java allows both after
    // `if (...)`/`else`). Local declarations are block statements, not statements,
    // so Java requires braces around them here.
    fn block_or_statement(&mut self) -> CompileResult<BranchBody> {
        let start = self.span();
        if matches!(self.peek(), TokenKind::LBrace) {
            self.bump();
            let mut stmts = Vec::new();
            while !matches!(self.peek(), TokenKind::RBrace) {
                stmts.push(self.statement()?);
            }
            self.expect(&TokenKind::RBrace)?;
            Ok(BranchBody {
                span: start.join(self.previous_span()),
                braced: true,
                stmts,
            })
        } else {
            let stmt = self.statement()?;
            if matches!(stmt.kind, StmtKind::LocalDecl { .. }) {
                return Err(Diagnostic::parse(
                    stmt.span,
                    "a local declaration requires a braced if/else body",
                ));
            }
            Ok(BranchBody { span: stmt.span, braced: false, stmts: vec![stmt] })
        }
    }

    // `<ty> name = init;` (initializer optional).
    fn local_decl(&mut self) -> CompileResult<StmtKind> {
        let ty = self.primitive_type()?;
        let name = self.expect_name()?;
        let init = if matches!(self.peek(), TokenKind::Assign) {
            self.bump();
            Some(self.expression()?)
        } else {
            None
        };
        self.expect(&TokenKind::Semicolon)?;
        Ok(StmtKind::LocalDecl { ty, name, init })
    }

    // A statement beginning with an identifier: plain/compound assignment,
    // post-`++`/`--`, or an expression statement (`System.out.println(...)`).
    fn ident_statement(&mut self) -> CompileResult<StmtKind> {
        // `System.out.println(...)` is the only expression statement; it is an
        // identifier followed by `.`, so anything with a `.` next is that form.
        match self.peek_kind(1) {
            TokenKind::Assign => {
                let name = self.expect_name()?;
                self.expect(&TokenKind::Assign)?;
                let value = self.expression()?;
                self.expect(&TokenKind::Semicolon)?;
                Ok(StmtKind::Assign { name, value })
            }
            k if compound_op(k).is_some() => {
                let name = self.expect_name()?;
                let op = compound_op(&self.bump().kind).unwrap();
                let value = self.expression()?;
                self.expect(&TokenKind::Semicolon)?;
                Ok(StmtKind::CompoundAssign { name, op, value })
            }
            TokenKind::PlusPlus | TokenKind::MinusMinus => {
                let name = self.expect_name()?;
                let op = if matches!(self.peek(), TokenKind::PlusPlus) {
                    BinOp::Add
                } else {
                    BinOp::Sub
                };
                self.bump();
                self.expect(&TokenKind::Semicolon)?;
                let value = self.expr(ExprKind::IntLit(1));
                Ok(StmtKind::CompoundAssign { name, op, value })
            }
            _ => {
                let expr = self.expression()?;
                self.expect(&TokenKind::Semicolon)?;
                Ok(StmtKind::Expr(expr))
            }
        }
    }
}

/// The compound-assignment operator a token denotes, if any.
fn compound_op(k: &TokenKind) -> Option<BinOp> {
    Some(match k {
        TokenKind::PlusEq => BinOp::Add,
        TokenKind::MinusEq => BinOp::Sub,
        TokenKind::StarEq => BinOp::Mul,
        TokenKind::SlashEq => BinOp::Div,
        TokenKind::PercentEq => BinOp::Rem,
        TokenKind::AmpEq => BinOp::And,
        TokenKind::PipeEq => BinOp::Or,
        TokenKind::CaretEq => BinOp::Xor,
        TokenKind::ShlEq => BinOp::Shl,
        TokenKind::ShrEq => BinOp::Shr,
        TokenKind::UShrEq => BinOp::UShr,
        _ => return None,
    })
}

fn is_unsupported_statement_keyword(name: &str) -> bool {
    matches!(
        name,
        "while"
            | "for"
            | "do"
            | "switch"
            | "return"
            | "throw"
            | "try"
            | "synchronized"
            | "assert"
            | "break"
            | "continue"
    )
}
