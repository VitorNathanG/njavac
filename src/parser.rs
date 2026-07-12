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
//! The parser panics on malformed input; the fixtures are well-formed.

use crate::ast::{
    BinOp, Class, CmpOp, CompilationUnit, Expr, LogOp, Method, Param, Stmt, StmtKind, Type,
};
use crate::lexer::{Token, TokenKind};

/// Parse a token stream (as produced by `lexer::lex`) into a `CompilationUnit`.
///
/// Panics on any syntax error outside the supported subset.
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

    // A parameter type: a primitive or `String[]`.
    fn param_type(&mut self) -> Type {
        if let TokenKind::Ident(name) = self.peek().clone() {
            if name == "String" {
                self.bump();
                self.expect(&TokenKind::LBracket);
                self.expect(&TokenKind::RBracket);
                return Type::StringArray;
            }
        }
        self.primitive_type()
    }

    /// Consume a primitive type keyword, or panic.
    fn primitive_type(&mut self) -> Type {
        let ty = match self.peek() {
            TokenKind::Int => Type::Int,
            TokenKind::Long => Type::Long,
            TokenKind::Float => Type::Float,
            TokenKind::Double => Type::Double,
            TokenKind::Boolean => Type::Boolean,
            TokenKind::Char => Type::Char,
            TokenKind::Byte => Type::Byte,
            TokenKind::Short => Type::Short,
            other => panic!("expected a type, found {:?}", other),
        };
        self.bump();
        ty
    }

    // A single statement.
    fn statement(&mut self) -> Stmt {
        let line = self.line();
        let kind = if matches!(self.peek(), TokenKind::If) {
            self.if_statement()
        } else if is_primitive_type(self.peek()) {
            self.local_decl()
        } else if matches!(self.peek(), TokenKind::PlusPlus | TokenKind::MinusMinus) {
            // Prefix `++x;` / `--x;` — in statement position the produced value is
            // discarded, so pre/post is irrelevant.
            let op = if matches!(self.peek(), TokenKind::PlusPlus) { BinOp::Add } else { BinOp::Sub };
            self.bump();
            let name = self.expect_ident();
            self.expect(&TokenKind::Semicolon);
            StmtKind::CompoundAssign { name, op, value: Expr::IntLit(1) }
        } else if matches!(self.peek(), TokenKind::Ident(_)) {
            self.ident_statement()
        } else {
            panic!("unexpected statement start: {:?}", self.peek());
        };
        Stmt { line, kind }
    }

    // `if (cond) <then> [else <else>]`. Each arm is a brace-block or a single
    // statement; `else if` falls out naturally as an `If` in the else arm.
    fn if_statement(&mut self) -> StmtKind {
        self.expect(&TokenKind::If);
        self.expect(&TokenKind::LParen);
        let cond = self.expression();
        self.expect(&TokenKind::RParen);
        let then_branch = self.block_or_statement();
        let else_branch = if matches!(self.peek(), TokenKind::Else) {
            self.bump();
            Some(self.block_or_statement())
        } else {
            None
        };
        StmtKind::If { cond, then_branch, else_branch }
    }

    // A brace-delimited block, or a single statement (Java allows both after
    // `if (...)`/`else`). Returned as a statement list either way.
    fn block_or_statement(&mut self) -> Vec<Stmt> {
        if matches!(self.peek(), TokenKind::LBrace) {
            self.bump();
            let mut stmts = Vec::new();
            while !matches!(self.peek(), TokenKind::RBrace) {
                stmts.push(self.statement());
            }
            self.expect(&TokenKind::RBrace);
            stmts
        } else {
            vec![self.statement()]
        }
    }

    // `<ty> name = init;` (initializer optional).
    fn local_decl(&mut self) -> StmtKind {
        let ty = self.primitive_type();
        let name = self.expect_ident();
        let init = if matches!(self.peek(), TokenKind::Assign) {
            self.bump();
            Some(self.expression())
        } else {
            None
        };
        self.expect(&TokenKind::Semicolon);
        StmtKind::LocalDecl { ty, name, init }
    }

    // A statement beginning with an identifier: plain/compound assignment,
    // post-`++`/`--`, or an expression statement (`System.out.println(...)`).
    fn ident_statement(&mut self) -> StmtKind {
        // `System.out.println(...)` is the only expression statement; it is an
        // identifier followed by `.`, so anything with a `.` next is that form.
        match self.peek_kind(1) {
            TokenKind::Assign => {
                let name = self.expect_ident();
                self.expect(&TokenKind::Assign);
                let value = self.expression();
                self.expect(&TokenKind::Semicolon);
                StmtKind::Assign { name, value }
            }
            k if compound_op(k).is_some() => {
                let name = self.expect_ident();
                let op = compound_op(&self.bump().kind).unwrap();
                let value = self.expression();
                self.expect(&TokenKind::Semicolon);
                StmtKind::CompoundAssign { name, op, value }
            }
            TokenKind::PlusPlus | TokenKind::MinusMinus => {
                let name = self.expect_ident();
                let op = if matches!(self.peek(), TokenKind::PlusPlus) { BinOp::Add } else { BinOp::Sub };
                self.bump();
                self.expect(&TokenKind::Semicolon);
                StmtKind::CompoundAssign { name, op, value: Expr::IntLit(1) }
            }
            _ => {
                let expr = self.expression();
                self.expect(&TokenKind::Semicolon);
                StmtKind::Expr(expr)
            }
        }
    }

    /// Look ahead `n` tokens from the current position (saturating at Eof).
    fn peek_kind(&self, n: usize) -> &TokenKind {
        let i = (self.pos + n).min(self.tokens.len() - 1);
        &self.tokens[i].kind
    }

    // ---- expressions, loosest precedence first ----

    fn expression(&mut self) -> Expr {
        self.logical_or()
    }

    // `||` — the loosest binary level, below `&&`. Left-associative
    // (`a || b || c` = `Or(Or(a, b), c)`, matching javac's genCond nesting).
    fn logical_or(&mut self) -> Expr {
        let mut left = self.logical_and();
        while matches!(self.peek(), TokenKind::PipePipe) {
            self.bump();
            let right = self.logical_and();
            left = logical(LogOp::Or, left, right);
        }
        left
    }

    // `&&` — below `||`, above the bitwise `|`.
    fn logical_and(&mut self) -> Expr {
        let mut left = self.bit_or();
        while matches!(self.peek(), TokenKind::AmpAmp) {
            self.bump();
            let right = self.bit_or();
            left = logical(LogOp::And, left, right);
        }
        left
    }

    fn bit_or(&mut self) -> Expr {
        let mut left = self.bit_xor();
        while matches!(self.peek(), TokenKind::Pipe) {
            self.bump();
            let right = self.bit_xor();
            left = binary(BinOp::Or, left, right);
        }
        left
    }

    fn bit_xor(&mut self) -> Expr {
        let mut left = self.bit_and();
        while matches!(self.peek(), TokenKind::Caret) {
            self.bump();
            let right = self.bit_and();
            left = binary(BinOp::Xor, left, right);
        }
        left
    }

    fn bit_and(&mut self) -> Expr {
        let mut left = self.equality();
        while matches!(self.peek(), TokenKind::Amp) {
            self.bump();
            let right = self.equality();
            left = binary(BinOp::And, left, right);
        }
        left
    }

    fn equality(&mut self) -> Expr {
        let mut left = self.relational();
        loop {
            let op = match self.peek() {
                TokenKind::EqEq => CmpOp::Eq,
                TokenKind::NotEq => CmpOp::Ne,
                _ => break,
            };
            self.bump();
            let right = self.relational();
            left = compare(op, left, right);
        }
        left
    }

    fn relational(&mut self) -> Expr {
        let mut left = self.shift();
        loop {
            let op = match self.peek() {
                TokenKind::Lt => CmpOp::Lt,
                TokenKind::Le => CmpOp::Le,
                TokenKind::Gt => CmpOp::Gt,
                TokenKind::Ge => CmpOp::Ge,
                _ => break,
            };
            self.bump();
            let right = self.shift();
            left = compare(op, left, right);
        }
        left
    }

    fn shift(&mut self) -> Expr {
        let mut left = self.additive();
        loop {
            let op = match self.peek() {
                TokenKind::Shl => BinOp::Shl,
                TokenKind::Shr => BinOp::Shr,
                TokenKind::UShr => BinOp::UShr,
                _ => break,
            };
            self.bump();
            let right = self.additive();
            left = binary(op, left, right);
        }
        left
    }

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
            left = binary(op, left, right);
        }
        left
    }

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
            left = binary(op, left, right);
        }
        left
    }

    // unary -> '-' unary | '~' unary | '(' primitive ')' unary | primary
    fn unary(&mut self) -> Expr {
        match self.peek() {
            TokenKind::Minus => {
                self.bump();
                Expr::Neg(Box::new(self.unary()))
            }
            TokenKind::Tilde => {
                self.bump();
                Expr::BitNot(Box::new(self.unary()))
            }
            TokenKind::Bang => {
                self.bump();
                Expr::Not(Box::new(self.unary()))
            }
            TokenKind::LParen if self.is_cast() => {
                self.bump(); // (
                let ty = self.primitive_type();
                self.expect(&TokenKind::RParen);
                Expr::Cast { ty, expr: Box::new(self.unary()) }
            }
            _ => self.primary(),
        }
    }

    /// A `(` begins a cast iff it is immediately followed by a primitive type
    /// keyword and a `)` — reference casts are out of the subset, so this is
    /// unambiguous against a parenthesized expression.
    fn is_cast(&self) -> bool {
        is_primitive_type(self.peek_kind(1)) && matches!(self.peek_kind(2), TokenKind::RParen)
    }

    // primary -> literal | '(' expression ')' | System.out.println(arg) | name
    fn primary(&mut self) -> Expr {
        match self.peek().clone() {
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
                let inner = self.expression();
                self.expect(&TokenKind::RParen);
                inner
            }
            // `System.out.println(arg)` — the only call shape in the subset.
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
