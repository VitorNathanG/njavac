//! Lexer: source text -> flat token stream.
//!
//! Produces a flat stream of tokens for the Tier-1 subset. Every token carries
//! its 1-based source line so the parser can attach lines to statements for a
//! byte-identical `LineNumberTable`. Whitespace and `//` / `/* */` comments are
//! skipped. The subset is ASCII, so we scan over bytes.

/// A single lexical token plus the 1-based source line it starts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    /// 1-based source line where the token begins.
    pub line: u16,
}

/// The lexical categories of the Tier-1 subset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    // Keywords.
    Public,
    Class,
    Static,
    Void,
    Int,

    // Literals and names.
    /// An identifier (not a keyword).
    Ident(String),
    /// A decimal integer literal, kept as its raw digit text. Value resolution
    /// (including the special unsigned magnitude 2147483648 after a unary minus)
    /// is left to later phases.
    IntLit(String),
    /// A string literal with escapes already decoded to real characters.
    StringLit(String),

    // Punctuation / operators.
    LBrace,   // {
    RBrace,   // }
    LParen,   // (
    RParen,   // )
    LBracket, // [
    RBracket, // ]
    Semicolon, // ;
    Comma,    // ,
    Dot,      // .
    Assign,   // =
    Plus,     // +
    Minus,    // -
    Star,     // *
    Slash,    // /
    Percent,  // %

    /// End of input.
    Eof,
}

/// Tokenize `source` into a flat token stream terminated by a single `Eof`.
///
/// Panics on lexical errors (unterminated string/comment, unknown character,
/// bad escape). The Tier-1 fixtures are well-formed, so this stays simple.
pub fn lex(source: &str) -> Vec<Token> {
    Lexer::new(source).run()
}

struct Lexer<'a> {
    bytes: &'a [u8],
    pos: usize,
    line: u16,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Lexer { bytes: source.as_bytes(), pos: 0, line: 1 }
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

    fn run(mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        loop {
            self.skip_trivia();
            let line = self.line;
            let b = match self.peek() {
                None => {
                    tokens.push(Token { kind: TokenKind::Eof, line });
                    return tokens;
                }
                Some(b) => b,
            };

            let kind = if b == b'"' {
                self.string_literal()
            } else if b.is_ascii_digit() {
                self.int_literal()
            } else if is_ident_start(b) {
                self.ident_or_keyword()
            } else {
                self.punct()
            };
            tokens.push(Token { kind, line });
        }
    }

    /// Skip whitespace and both comment styles, repeatedly.
    fn skip_trivia(&mut self) {
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
                    self.bump();
                    self.bump();
                    loop {
                        match self.peek() {
                            None => panic!("unterminated block comment"),
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
                _ => return,
            }
        }
    }

    fn string_literal(&mut self) -> TokenKind {
        self.bump(); // opening quote
        let mut s = String::new();
        loop {
            match self.peek() {
                None => panic!("unterminated string literal"),
                Some(b'\n') => panic!("newline in string literal"),
                Some(b'"') => {
                    self.bump();
                    return TokenKind::StringLit(s);
                }
                Some(b'\\') => {
                    self.bump();
                    let e = match self.peek() {
                        None => panic!("unterminated escape in string literal"),
                        Some(e) => e,
                    };
                    self.bump();
                    let c = match e {
                        b't' => '\t',
                        b'n' => '\n',
                        b'r' => '\r',
                        b'"' => '"',
                        b'\\' => '\\',
                        b'\'' => '\'',
                        b'b' => '\u{0008}',
                        b'f' => '\u{000C}',
                        other => panic!("unknown string escape: \\{}", other as char),
                    };
                    s.push(c);
                }
                Some(b) => {
                    self.bump();
                    s.push(b as char);
                }
            }
        }
    }

    fn int_literal(&mut self) -> TokenKind {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() {
                self.bump();
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap().to_string();
        TokenKind::IntLit(text)
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
            _ => TokenKind::Ident(text.to_string()),
        }
    }

    fn punct(&mut self) -> TokenKind {
        let b = self.bump();
        match b {
            b'{' => TokenKind::LBrace,
            b'}' => TokenKind::RBrace,
            b'(' => TokenKind::LParen,
            b')' => TokenKind::RParen,
            b'[' => TokenKind::LBracket,
            b']' => TokenKind::RBracket,
            b';' => TokenKind::Semicolon,
            b',' => TokenKind::Comma,
            b'.' => TokenKind::Dot,
            b'=' => TokenKind::Assign,
            b'+' => TokenKind::Plus,
            b'-' => TokenKind::Minus,
            b'*' => TokenKind::Star,
            b'/' => TokenKind::Slash,
            b'%' => TokenKind::Percent,
            other => panic!("unexpected character: {:?}", other as char),
        }
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}
