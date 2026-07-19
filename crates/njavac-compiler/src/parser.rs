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
//!   `||` < `&&` < `|` < `^` < `&` < `== !=` < `< <= > >=`
//!   < `<< >> >>>` < `+ -` < `* / %` < unary
//!
//! The short-circuit `||`/`&&` are the two loosest levels (below the bitwise `|`).
//! Unary covers `-`, `~`, `!`, and primitive casts `(T) e`. Parentheses group.
//! Each statement is tagged with the 1-based source line it begins on so codegen
//! can rebuild the `LineNumberTable` byte-identically to javac.
//!
mod expression;
mod statement;

use crate::ast::{
    Class, CompilationUnit, ExprArena, ExprId, ExprKind, JAVA_LANG_OBJECT, Method, Name, Param,
    PrimitiveType, Type,
};
use crate::diagnostic::{CompileResult, Diagnostic};
use crate::lexer::{Token, TokenKind};
use crate::span::Span;

/// Parse a token stream (as produced by `lexer::lex`) into a `CompilationUnit`.
pub fn parse(tokens: Vec<Token>) -> CompileResult<CompilationUnit> {
    Parser {
        tokens,
        pos: 0,
        exprs: ExprArena::default(),
    }
    .compilation_unit()
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
            let TokenKind::Ident(name) = token.kind else {
                unreachable!()
            };
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
        Ok(CompilationUnit {
            span: class.span,
            class,
            exprs: self.exprs,
        })
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

    /// Look ahead `n` tokens from the current position (saturating at Eof).
    fn peek_kind(&self, n: usize) -> &TokenKind {
        let i = (self.pos + n).min(self.tokens.len() - 1);
        &self.tokens[i].kind
    }
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
