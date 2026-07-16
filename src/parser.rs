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
    BinOp, BranchBody, Class, CmpOp, CompilationUnit, Expr, LogOp, Method, Name, Param, Stmt,
    StmtKind, Type,
};
use crate::diagnostic::{CompileResult, Diagnostic};
use crate::lexer::{Token, TokenKind};
use crate::span::Span;

/// Parse a token stream (as produced by `lexer::lex`) into a `CompilationUnit`.
pub fn parse(tokens: Vec<Token>) -> CompileResult<CompilationUnit> {
    Parser { tokens, pos: 0 }.compilation_unit()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
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
        let t = self.tokens[self.pos].clone();
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
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                let span = self.bump().span;
                Ok((name, span))
            }
            other => Err(Diagnostic::parse(
                self.span(),
                format!("expected identifier, found {:?}", other),
            )),
        }
    }

    // compilation unit -> public class
    fn compilation_unit(&mut self) -> CompileResult<CompilationUnit> {
        let class = self.class()?;
        // Everything after the top-level class must be end of input.
        if !matches!(self.peek(), TokenKind::Eof) {
            return Err(Diagnostic::parse(
                self.span(),
                format!("unexpected trailing token: {:?}", self.peek()),
            ));
        }
        Ok(CompilationUnit { span: class.span, class })
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
        Ok(Class { span, name, name_span, line, close_line, methods })
    }

    // `public static void main(String[] args) { <stmts> }`
    fn method(&mut self) -> CompileResult<Method> {
        let start = self.span();
        self.expect(&TokenKind::Public)?;
        self.expect(&TokenKind::Static)?;
        self.expect(&TokenKind::Void)?;
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
        Ok(Method { span, name, name_span, is_static: true, params, body, close_line })
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
        if let TokenKind::Ident(name) = self.peek().clone() {
            if name == "String" {
                self.bump();
                self.expect(&TokenKind::LBracket)?;
                self.expect(&TokenKind::RBracket)?;
                return Ok(Type::StringArray);
            }
        }
        self.primitive_type()
    }

    /// Consume a primitive type keyword.
    fn primitive_type(&mut self) -> CompileResult<Type> {
        let ty = match self.peek() {
            TokenKind::Int => Type::Int,
            TokenKind::Long => Type::Long,
            TokenKind::Float => Type::Float,
            TokenKind::Double => Type::Double,
            TokenKind::Boolean => Type::Boolean,
            TokenKind::Char => Type::Char,
            TokenKind::Byte => Type::Byte,
            TokenKind::Short => Type::Short,
            other => {
                return Err(Diagnostic::parse(
                    self.span(),
                    format!("expected a type, found {:?}", other),
                ));
            }
        };
        self.bump();
        Ok(ty)
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
            StmtKind::CompoundAssign { name, op, value: Expr::IntLit(1) }
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
                Ok(StmtKind::CompoundAssign { name, op, value: Expr::IntLit(1) })
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

    // ---- expressions, loosest precedence first ----

    fn expression(&mut self) -> CompileResult<Expr> {
        self.logical_or()
    }

    // `||` — the loosest binary level, below `&&`. Left-associative
    // (`a || b || c` = `Or(Or(a, b), c)`, matching javac's genCond nesting).
    fn logical_or(&mut self) -> CompileResult<Expr> {
        let mut left = self.logical_and()?;
        while matches!(self.peek(), TokenKind::PipePipe) {
            self.bump();
            let right = self.logical_and()?;
            left = logical(LogOp::Or, left, right);
        }
        Ok(left)
    }

    // `&&` — below `||`, above the bitwise `|`.
    fn logical_and(&mut self) -> CompileResult<Expr> {
        let mut left = self.bit_or()?;
        while matches!(self.peek(), TokenKind::AmpAmp) {
            self.bump();
            let right = self.bit_or()?;
            left = logical(LogOp::And, left, right);
        }
        Ok(left)
    }

    fn bit_or(&mut self) -> CompileResult<Expr> {
        let mut left = self.bit_xor()?;
        while matches!(self.peek(), TokenKind::Pipe) {
            self.bump();
            let right = self.bit_xor()?;
            left = binary(BinOp::Or, left, right);
        }
        Ok(left)
    }

    fn bit_xor(&mut self) -> CompileResult<Expr> {
        let mut left = self.bit_and()?;
        while matches!(self.peek(), TokenKind::Caret) {
            self.bump();
            let right = self.bit_and()?;
            left = binary(BinOp::Xor, left, right);
        }
        Ok(left)
    }

    fn bit_and(&mut self) -> CompileResult<Expr> {
        let mut left = self.equality()?;
        while matches!(self.peek(), TokenKind::Amp) {
            self.bump();
            let right = self.equality()?;
            left = binary(BinOp::And, left, right);
        }
        Ok(left)
    }

    fn equality(&mut self) -> CompileResult<Expr> {
        let mut left = self.relational()?;
        loop {
            let op = match self.peek() {
                TokenKind::EqEq => CmpOp::Eq,
                TokenKind::NotEq => CmpOp::Ne,
                _ => break,
            };
            self.bump();
            let right = self.relational()?;
            left = compare(op, left, right);
        }
        Ok(left)
    }

    fn relational(&mut self) -> CompileResult<Expr> {
        let mut left = self.shift()?;
        loop {
            let op = match self.peek() {
                TokenKind::Lt => CmpOp::Lt,
                TokenKind::Le => CmpOp::Le,
                TokenKind::Gt => CmpOp::Gt,
                TokenKind::Ge => CmpOp::Ge,
                _ => break,
            };
            self.bump();
            let right = self.shift()?;
            left = compare(op, left, right);
        }
        Ok(left)
    }

    fn shift(&mut self) -> CompileResult<Expr> {
        let mut left = self.additive()?;
        loop {
            let op = match self.peek() {
                TokenKind::Shl => BinOp::Shl,
                TokenKind::Shr => BinOp::Shr,
                TokenKind::UShr => BinOp::UShr,
                _ => break,
            };
            self.bump();
            let right = self.additive()?;
            left = binary(op, left, right);
        }
        Ok(left)
    }

    fn additive(&mut self) -> CompileResult<Expr> {
        let mut left = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let right = self.multiplicative()?;
            left = binary(op, left, right);
        }
        Ok(left)
    }

    fn multiplicative(&mut self) -> CompileResult<Expr> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Rem,
                _ => break,
            };
            self.bump();
            let right = self.unary()?;
            left = binary(op, left, right);
        }
        Ok(left)
    }

    // unary -> '-' unary | '~' unary | '(' primitive ')' unary | primary
    fn unary(&mut self) -> CompileResult<Expr> {
        let expr = match self.peek() {
            TokenKind::Minus => {
                self.bump();
                Expr::Neg(Box::new(self.unary()?))
            }
            TokenKind::Tilde => {
                self.bump();
                Expr::BitNot(Box::new(self.unary()?))
            }
            TokenKind::Bang => {
                self.bump();
                Expr::Not(Box::new(self.unary()?))
            }
            TokenKind::LParen if self.is_cast() => {
                self.bump(); // (
                let ty = self.primitive_type()?;
                self.expect(&TokenKind::RParen)?;
                Expr::Cast { ty, expr: Box::new(self.unary()?) }
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
    fn primary(&mut self) -> CompileResult<Expr> {
        let expr = match self.peek().clone() {
            TokenKind::IntLit(v) => {
                self.bump();
                Expr::IntLit(v)
            }
            TokenKind::LongLit(v) => {
                self.bump();
                Expr::LongLit(v)
            }
            TokenKind::FloatLit(v) => {
                self.bump();
                Expr::FloatLit(v)
            }
            TokenKind::DoubleLit(v) => {
                self.bump();
                Expr::DoubleLit(v)
            }
            TokenKind::CharLit(v) => {
                self.bump();
                Expr::CharLit(v)
            }
            TokenKind::True => {
                self.bump();
                Expr::BoolLit(true)
            }
            TokenKind::False => {
                self.bump();
                Expr::BoolLit(false)
            }
            TokenKind::StringLit(s) => {
                self.bump();
                Expr::StringLit(s)
            }
            TokenKind::LParen => {
                self.bump();
                let inner = self.expression()?;
                self.expect(&TokenKind::RParen)?;
                Expr::Paren(Box::new(inner))
            }
            // `System.out.println(arg)` — the only call shape in the subset.
            TokenKind::Ident(name) if name == "System" => {
                self.bump();
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
                Expr::Println(Box::new(arg))
            }
            TokenKind::Ident(name) => {
                let span = self.bump().span;
                Expr::Name(Name { text: name, span })
            }
            other => {
                return Err(Diagnostic::parse(
                    self.span(),
                    format!("unexpected token in expression: {:?}", other),
                ));
            }
        };
        Ok(expr)
    }
}

fn binary(op: BinOp, left: Expr, right: Expr) -> Expr {
    Expr::Binary { op, left: Box::new(left), right: Box::new(right) }
}

fn compare(op: CmpOp, left: Expr, right: Expr) -> Expr {
    Expr::Compare { op, left: Box::new(left), right: Box::new(right) }
}

fn logical(op: LogOp, left: Expr, right: Expr) -> Expr {
    Expr::Logical { op, left: Box::new(left), right: Box::new(right) }
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
