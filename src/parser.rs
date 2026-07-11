//! Recursive-descent parser: tokens -> AST.
//!
//! Parses the Tier-1 straight-line subset: one `public class` holding a single
//! `public static void main(String[] args)` method whose body is a sequence of
//! `int` local declarations, assignments, and `System.out.println(...)` calls.
//!
//! Expression precedence (tightest first): unary minus, then `* / %`, then
//! `+ -`, both binary levels left-associative; parentheses group. Each statement
//! is tagged with the 1-based source line it begins on so codegen can rebuild the
//! `LineNumberTable` byte-identically to javac.
//!
//! The parser panics on malformed input; the Tier-1 fixtures are well-formed.

use crate::ast::{
    BinOp, Class, CompilationUnit, Expr, Method, Param, Stmt, StmtKind, Type,
};
use crate::lexer::{Token, TokenKind};

/// Parse a token stream (as produced by `lexer::lex`) into a `CompilationUnit`.
///
/// Panics on any syntax error outside the Tier-1 subset.
pub fn parse(tokens: Vec<Token>) -> CompilationUnit {
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

    fn bump(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        // Never advance past the terminating Eof.
        if !matches!(t.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        t
    }

    /// Consume a token whose kind equals `expected`, or panic.
    fn expect(&mut self, expected: &TokenKind) {
        if self.peek() == expected {
            self.bump();
        } else {
            panic!("expected {:?}, found {:?}", expected, self.peek());
        }
    }

    /// Consume an identifier, returning its name, or panic.
    fn expect_ident(&mut self) -> String {
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                self.bump();
                name
            }
            other => panic!("expected identifier, found {:?}", other),
        }
    }

    // compilation unit -> public class
    fn compilation_unit(&mut self) -> CompilationUnit {
        let class = self.class();
        // Everything after the top-level class must be end of input.
        if !matches!(self.peek(), TokenKind::Eof) {
            panic!("unexpected trailing tokens: {:?}", self.peek());
        }
        CompilationUnit { class }
    }

    // `public class Name { <methods> }`
    fn class(&mut self) -> Class {
        let line = self.line();
        self.expect(&TokenKind::Public);
        self.expect(&TokenKind::Class);
        let name = self.expect_ident();
        self.expect(&TokenKind::LBrace);

        let mut methods = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace) {
            methods.push(self.method());
        }
        let close_line = self.line();
        self.expect(&TokenKind::RBrace);

        Class { name, line, close_line, methods }
    }

    // `public static void main(String[] args) { <stmts> }`
    fn method(&mut self) -> Method {
        self.expect(&TokenKind::Public);
        self.expect(&TokenKind::Static);
        self.expect(&TokenKind::Void);
        let name = self.expect_ident();

        self.expect(&TokenKind::LParen);
        let params = self.params();
        self.expect(&TokenKind::RParen);

        self.expect(&TokenKind::LBrace);
        let mut body = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace) {
            body.push(self.statement());
        }
        let close_line = self.line();
        self.expect(&TokenKind::RBrace);

        Method { name, is_static: true, params, body, close_line }
    }

    // Formal parameter list. The subset only ever has `String[] args`.
    fn params(&mut self) -> Vec<Param> {
        let mut params = Vec::new();
        if matches!(self.peek(), TokenKind::RParen) {
            return params;
        }
        loop {
            let ty = self.param_type();
            let name = self.expect_ident();
            params.push(Param { name, ty });
            if matches!(self.peek(), TokenKind::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        params
    }

    // A parameter type: `int` or `String[]`.
    fn param_type(&mut self) -> Type {
        match self.peek().clone() {
            TokenKind::Int => {
                self.bump();
                Type::Int
            }
            TokenKind::Ident(name) if name == "String" => {
                self.bump();
                self.expect(&TokenKind::LBracket);
                self.expect(&TokenKind::RBracket);
                Type::StringArray
            }
            other => panic!("unexpected parameter type: {:?}", other),
        }
    }

    // A single statement.
    fn statement(&mut self) -> Stmt {
        let line = self.line();
        let kind = match self.peek() {
            // `int name = init;` (initializer optional).
            TokenKind::Int => {
                self.bump();
                let name = self.expect_ident();
                let init = if matches!(self.peek(), TokenKind::Assign) {
                    self.bump();
                    Some(self.expression())
                } else {
                    None
                };
                self.expect(&TokenKind::Semicolon);
                StmtKind::LocalDecl { name, init }
            }
            // Either `name = value;` (assignment) or an expression statement
            // (`System.out.println(...)`). Distinguish by the token after the
            // leading identifier.
            TokenKind::Ident(_) => {
                if matches!(self.peek_kind(1), TokenKind::Assign) {
                    let name = self.expect_ident();
                    self.expect(&TokenKind::Assign);
                    let value = self.expression();
                    self.expect(&TokenKind::Semicolon);
                    StmtKind::Assign { name, value }
                } else {
                    let expr = self.expression();
                    self.expect(&TokenKind::Semicolon);
                    StmtKind::Expr(expr)
                }
            }
            other => panic!("unexpected statement start: {:?}", other),
        };
        Stmt { line, kind }
    }

    /// Look ahead `n` tokens from the current position (saturating at Eof).
    fn peek_kind(&self, n: usize) -> &TokenKind {
        let i = (self.pos + n).min(self.tokens.len() - 1);
        &self.tokens[i].kind
    }

    // expression -> additive
    fn expression(&mut self) -> Expr {
        self.additive()
    }

    // additive -> multiplicative ( (+|-) multiplicative )*   (left-associative)
    fn additive(&mut self) -> Expr {
        let mut left = self.multiplicative();
        loop {
            let op = match self.peek() {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let right = self.multiplicative();
            left = Expr::Binary { op, left: Box::new(left), right: Box::new(right) };
        }
        left
    }

    // multiplicative -> unary ( (*|/|%) unary )*   (left-associative)
    fn multiplicative(&mut self) -> Expr {
        let mut left = self.unary();
        loop {
            let op = match self.peek() {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Rem,
                _ => break,
            };
            self.bump();
            let right = self.unary();
            left = Expr::Binary { op, left: Box::new(left), right: Box::new(right) };
        }
        left
    }

    // unary -> '-' unary | primary
    fn unary(&mut self) -> Expr {
        if matches!(self.peek(), TokenKind::Minus) {
            self.bump();
            // Special case `-2147483648`: the magnitude overflows i32 on its own
            // but is exactly `i32::MIN` once negated. Fold it directly so the
            // literal fits the `i32` AST slot.
            if let TokenKind::IntLit(text) = self.peek() {
                if text == "2147483648" {
                    self.bump();
                    return Expr::IntLit(i32::MIN);
                }
            }
            let operand = self.unary();
            Expr::Neg(Box::new(operand))
        } else {
            self.primary()
        }
    }

    // primary -> int literal | string literal | '(' expression ')'
    //          | System.out.println(arg) | name
    fn primary(&mut self) -> Expr {
        match self.peek().clone() {
            TokenKind::IntLit(text) => {
                self.bump();
                let value: i32 = text
                    .parse()
                    .unwrap_or_else(|_| panic!("integer literal out of range: {}", text));
                Expr::IntLit(value)
            }
            TokenKind::StringLit(s) => {
                self.bump();
                Expr::StringLit(s)
            }
            TokenKind::LParen => {
                self.bump();
                let inner = self.expression();
                self.expect(&TokenKind::RParen);
                inner
            }
            // `System.out.println(arg)` — the only call shape in the subset.
            // `System` is an identifier; recognize the fixed member chain.
            TokenKind::Ident(name) if name == "System" => {
                self.bump();
                self.expect(&TokenKind::Dot);
                let out = self.expect_ident();
                if out != "out" {
                    panic!("expected System.out, found System.{}", out);
                }
                self.expect(&TokenKind::Dot);
                let println = self.expect_ident();
                if println != "println" {
                    panic!("expected System.out.println, found System.out.{}", println);
                }
                self.expect(&TokenKind::LParen);
                let arg = self.expression();
                self.expect(&TokenKind::RParen);
                Expr::Println(Box::new(arg))
            }
            TokenKind::Ident(name) => {
                self.bump();
                Expr::Name(name)
            }
            other => panic!("unexpected token in expression: {:?}", other),
        }
    }
}
