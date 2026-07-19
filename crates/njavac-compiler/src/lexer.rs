//! Lexer: source text -> flat token stream.
//!
//! Produces a flat stream of tokens for the supported language subset. Every token
//! carries its 1-based source line so the parser can attach lines to statements
//! for a byte-identical `LineNumberTable`. Whitespace and `//` / `/* */` comments
//! are skipped.
//!
//! Supported numeric literals are scanned here: decimal/hex/octal/binary integers
//! (with `_` separators and an optional `L` suffix) and floating-point literals
//! (fraction, exponent, `f`/`d` suffix). Each is resolved to its typed value —
//! the source radix/form leaves no trace, exactly as javac keeps only the value.
//! Character literals decode escapes (including octal and `\uXXXX`) to a UTF-16
//! code unit. Source is assumed ASCII outside of literal escapes.

use crate::diagnostic::{CompileResult, Diagnostic};
use crate::span::Span;

mod literal;
mod punctuator;
mod token;

pub use token::{Token, TokenKind};

/// Tokenize `source` into a flat token stream terminated by a single `Eof`.
pub fn lex(source: &str) -> CompileResult<Vec<Token>> {
    Lexer::new(source).run()
}

struct Lexer<'a> {
    bytes: &'a [u8],
    pos: usize,
    line: u16,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Lexer {
            bytes: source.as_bytes(),
            pos: 0,
            line: 1,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<u8> {
        self.bytes.get(self.pos + 1).copied()
    }

    /// Advance one byte, tracking line numbers.
    fn bump(&mut self) -> u8 {
        let b = self.bytes[self.pos];
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
        }
        b
    }

    fn run(mut self) -> CompileResult<Vec<Token>> {
        // Presize: on this subset there is roughly one token per ~3 source bytes,
        // so this avoids the token vec reallocating as it grows (the lexer was the
        // top self-time function and RawVec::grow_one the top allocator path).
        let mut tokens = Vec::with_capacity(self.bytes.len() / 3 + 8);
        loop {
            self.skip_trivia()?;
            let line = self.line;
            let start = self.pos;
            let b = match self.peek() {
                None => {
                    tokens.push(Token {
                        kind: TokenKind::Eof,
                        line,
                        span: Span::empty(start),
                    });
                    return Ok(tokens);
                }
                Some(b) => b,
            };

            let kind = if b == b'"' {
                self.string_literal()?
            } else if b == b'\'' {
                self.char_literal()?
            } else if b.is_ascii_digit() {
                self.number()?
            } else if b == b'.' && self.peek2().is_some_and(|c| c.is_ascii_digit()) {
                self.number()?
            } else if is_ident_start(b) {
                self.ident_or_keyword()
            } else {
                self.punct()?
            };
            tokens.push(Token {
                kind,
                line,
                span: Span::new(start, self.pos),
            });
        }
    }

    /// Skip whitespace and both comment styles, repeatedly.
    fn skip_trivia(&mut self) -> CompileResult<()> {
        loop {
            match self.peek() {
                Some(b) if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' => {
                    self.bump();
                }
                Some(b'/') if self.peek2() == Some(b'/') => {
                    self.bump();
                    self.bump();
                    while let Some(b) = self.peek() {
                        if b == b'\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                Some(b'/') if self.peek2() == Some(b'*') => {
                    let start = self.pos;
                    self.bump();
                    self.bump();
                    loop {
                        match self.peek() {
                            None => {
                                return Err(Diagnostic::lexical(
                                    Span::new(start, self.pos),
                                    "unterminated block comment",
                                ));
                            }
                            Some(b'*') if self.peek2() == Some(b'/') => {
                                self.bump();
                                self.bump();
                                break;
                            }
                            _ => {
                                self.bump();
                            }
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn ident_or_keyword(&mut self) -> TokenKind {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if is_ident_continue(b) {
                self.bump();
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap();
        match text {
            "public" => TokenKind::Public,
            "class" => TokenKind::Class,
            "static" => TokenKind::Static,
            "void" => TokenKind::Void,
            "int" => TokenKind::Int,
            "long" => TokenKind::Long,
            "float" => TokenKind::Float,
            "double" => TokenKind::Double,
            "boolean" => TokenKind::Boolean,
            "char" => TokenKind::Char,
            "byte" => TokenKind::Byte,
            "short" => TokenKind::Short,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            _ => TokenKind::Ident(text.to_string()),
        }
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}
