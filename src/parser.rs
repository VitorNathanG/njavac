//! Recursive-descent parser: tokens -> AST.
//!
//! Parses the numeric subset plus the first branch: one `public class` holding a
//! single `public static void main(String[] args)` method whose body is a
//! sequence of primitive local declarations, plain and compound assignments,
//! `++`/`--` statements, `if`/`else` statements, and `System.out.println(...)`
//! calls.
//!
//! Expression precedence (loosest to tightest), all binary levels
//! left-associative:
//!
//!   `||` < `&&` < `|` < `^` < `&` < `== !=` < `< <= > >=` < `<< >> >>>` < `+ -` < `* / %` < unary
//!
//! The short-circuit `||`/`&&` are the two loosest levels (below the bitwise `|`).
//! Unary covers `-`, `~`, `!`, and primitive casts `(T) e`. Parentheses group.
//! Each statement is tagged with the 1-based source line it begins on so codegen
//! can rebuild the `LineNumberTable` byte-identically to javac.
//!
use crate::ast::{
    BinOp, BranchBody, Class, CmpOp, CompilationUnit, ExprArena, ExprId, ExprKind, LogOp,
    Method, Name, Param, PrimitiveType, Stmt, StmtKind, Type, JAVA_LANG_OBJECT,
};
use crate::diagnostic::{CompileResult, Diagnostic};
use crate::lexer::{Token, TokenKind};
use crate::span::Span;

/// Parse a token stream (as produced by `lexer::lex`) into a `CompilationUnit`.
pub fn parse(tokens: Vec<Token>) -> CompileResult<CompilationUnit> {
    Parser { tokens, pos: 0, exprs: ExprArena::default() }.compilation_unit()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    exprs: ExprArena,
}

impl Parser {
    fn expr(&mut self, kind: ExprKind) -> ExprId {
        self.exprs.alloc(kind)
    }
    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    /// The 1-based source line of the current token.
    fn line(&self) -> u16 {
        self.tokens[self.pos].line
    }

    fn span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn previous_span(&self) -> Span {
        self.tokens[self.pos - 1].span
    }

    fn bump(&mut self) -> Token {
        let token = &mut self.tokens[self.pos];
        let t = Token {
            kind: std::mem::replace(&mut token.kind, TokenKind::Eof),
            line: token.line,
            span: token.span,
        };
        // Never advance past the terminating Eof.
        if !matches!(t.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        t
    }

    /// Consume a token whose kind equals `expected`.
    fn expect(&mut self, expected: &TokenKind) -> CompileResult<()> {
        if self.peek() == expected {
            self.bump();
            Ok(())
        } else {
            Err(Diagnostic::parse(
                self.span(),
                format!("expected {:?}, found {:?}", expected, self.peek()),
            ))
        }
    }

    /// Consume an identifier as a source-level name occurrence.
    fn expect_name(&mut self) -> CompileResult<Name> {
        let (text, span) = self.expect_ident_spanned()?;
        Ok(Name { text, span })
    }

    /// Consume an identifier, returning its name and source span.
    fn expect_ident_spanned(&mut self) -> CompileResult<(String, Span)> {
        if matches!(self.peek(), TokenKind::Ident(_)) {
            let token = self.bump();
            let TokenKind::Ident(name) = token.kind else { unreachable!() };
            Ok((name, token.span))
        } else {
            Err(Diagnostic::parse(
                self.span(),
                format!("expected identifier, found {:?}", self.peek()),
            ))
        }
    }

    // compilation unit -> public class
    fn compilation_unit(mut self) -> CompileResult<CompilationUnit> {
        let class = self.class()?;
        // Everything after the top-level class must be end of input.
        if !matches!(self.peek(), TokenKind::Eof) {
            return Err(Diagnostic::parse(
                self.span(),
                format!("unexpected trailing token: {:?}", self.peek()),
            ));
        }
        Ok(CompilationUnit { span: class.span, class, exprs: self.exprs })
    }

    // `public class Name { <methods> }`
    fn class(&mut self) -> CompileResult<Class> {
        let line = self.line();
        let start = self.span();
        self.expect(&TokenKind::Public)?;
        self.expect(&TokenKind::Class)?;
        let (name, name_span) = self.expect_ident_spanned()?;
        self.expect(&TokenKind::LBrace)?;

        let mut methods = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace) {
            methods.push(self.method()?);
        }
        let close_line = self.line();
        self.expect(&TokenKind::RBrace)?;

        let span = start.join(self.previous_span());
        Ok(Class {
            span,
            name,
            name_span,
            super_class: JAVA_LANG_OBJECT.to_owned(),
            line,
            close_line,
            methods,
        })
    }

    // `public static void main(String[] args) { <stmts> }`
    fn method(&mut self) -> CompileResult<Method> {
        let start = self.span();
        self.expect(&TokenKind::Public)?;
        self.expect(&TokenKind::Static)?;
        self.expect(&TokenKind::Void)?;
        let return_type = Type::Void;
        let (name, name_span) = self.expect_ident_spanned()?;

        self.expect(&TokenKind::LParen)?;
        let params = self.params()?;
        self.expect(&TokenKind::RParen)?;

        self.expect(&TokenKind::LBrace)?;
        let mut body = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace) {
            body.push(self.statement()?);
        }
        let close_line = self.line();
        self.expect(&TokenKind::RBrace)?;

        let span = start.join(self.previous_span());
        Ok(Method {
            span,
            name,
            name_span,
            is_static: true,
            return_type,
            params,
            body,
            close_line,
        })
    }

    // Formal parameter list. The subset only ever has `String[] args`.
    fn params(&mut self) -> CompileResult<Vec<Param>> {
        let mut params = Vec::new();
        if matches!(self.peek(), TokenKind::RParen) {
            return Ok(params);
        }
        loop {
            let start = self.span();
            let ty = self.param_type()?;
            let name = self.expect_name()?;
            let span = start.join(name.span);
            params.push(Param { span, name, ty });
            if matches!(self.peek(), TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        Ok(params)
    }

    // A parameter type: a primitive or `String[]`.
    fn param_type(&mut self) -> CompileResult<Type> {
        if matches!(self.peek(), TokenKind::Ident(name) if name == "String") {
            self.bump();
            self.expect(&TokenKind::LBracket)?;
            self.expect(&TokenKind::RBracket)?;
            return Ok(Type::string_array());
        }
        self.primitive_type()
    }

    /// Consume a primitive type keyword.
    fn primitive_type(&mut self) -> CompileResult<Type> {
        let ty = match self.peek() {
            TokenKind::Int => PrimitiveType::Int,
            TokenKind::Long => PrimitiveType::Long,
            TokenKind::Float => PrimitiveType::Float,
            TokenKind::Double => PrimitiveType::Double,
            TokenKind::Boolean => PrimitiveType::Boolean,
            TokenKind::Char => PrimitiveType::Char,
            TokenKind::Byte => PrimitiveType::Byte,
            TokenKind::Short => PrimitiveType::Short,
            other => {
                return Err(Diagnostic::parse(
                    self.span(),
                    format!("expected a type, found {:?}", other),
                ));
            }
        };
        self.bump();
        Ok(ty.into())
    }

    // A single statement.
    fn statement(&mut self) -> CompileResult<Stmt> {
        let line = self.line();
        let start = self.span();
        let kind = if matches!(self.peek(), TokenKind::If) {
            self.if_statement()?
        } else if is_primitive_type(self.peek()) {
            self.local_decl()?
        } else if matches!(self.peek(), TokenKind::PlusPlus | TokenKind::MinusMinus) {
            // Prefix `++x;` / `--x;` — in statement position the produced value is
            // discarded, so pre/post is irrelevant.
            let op = if matches!(self.peek(), TokenKind::PlusPlus) { BinOp::Add } else { BinOp::Sub };
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
                let op = if matches!(self.peek(), TokenKind::PlusPlus) { BinOp::Add } else { BinOp::Sub };
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

    /// Look ahead `n` tokens from the current position (saturating at Eof).
    fn peek_kind(&self, n: usize) -> &TokenKind {
        let i = (self.pos + n).min(self.tokens.len() - 1);
        &self.tokens[i].kind
    }

    // ---- expressions ----

    fn expression(&mut self) -> CompileResult<ExprId> {
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

    // primary -> literal | '(' expression ')' | System.out.println(arg) | name
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
            // `System.out.println(arg)` — the only call shape in the subset.
            TokenKind::Ident(name) if name == "System" => {
                self.expect(&TokenKind::Dot)?;
                let (out, out_span) = self.expect_ident_spanned()?;
                if out != "out" {
                    return Err(Diagnostic::parse(
                        out_span,
                        format!("expected System.out, found System.{out}"),
                    ));
                }
                self.expect(&TokenKind::Dot)?;
                let (println, println_span) = self.expect_ident_spanned()?;
                if println != "println" {
                    return Err(Diagnostic::parse(
                        println_span,
                        format!("expected System.out.println, found System.out.{println}"),
                    ));
                }
                self.expect(&TokenKind::LParen)?;
                let arg = self.expression()?;
                self.expect(&TokenKind::RParen)?;
                self.expr(ExprKind::Println(arg))
            }
            TokenKind::Ident(name) => {
                self.expr(ExprKind::Name(Name { text: name, span: token.span }))
            }
            other => {
                return Err(Diagnostic::parse(
                    token.span,
                    format!("unexpected token in expression: {:?}", other),
                ));
            }
        };
        Ok(expr)
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

fn is_primitive_type(k: &TokenKind) -> bool {
    matches!(
        k,
        TokenKind::Int
            | TokenKind::Long
            | TokenKind::Float
            | TokenKind::Double
            | TokenKind::Boolean
            | TokenKind::Char
            | TokenKind::Byte
            | TokenKind::Short
    )
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
