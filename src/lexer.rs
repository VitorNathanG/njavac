//! Lexer: source text -> flat token stream.
//!
//! Produces a flat stream of tokens for the Tier-2 numeric subset. Every token
//! carries its 1-based source line so the parser can attach lines to statements
//! for a byte-identical `LineNumberTable`. Whitespace and `//` / `/* */` comments
//! are skipped.
//!
//! Numeric literals are fully scanned here: decimal/hex/octal/binary integers
//! (with `_` separators and an optional `L` suffix) and floating-point literals
//! (fraction, exponent, `f`/`d` suffix). Each is resolved to its typed value —
//! the source radix/form leaves no trace, exactly as javac keeps only the value.
//! Character literals decode escapes (including octal and `\uXXXX`) to a UTF-16
//! code unit. Source is assumed ASCII outside of literal escapes.

/// A single lexical token plus the 1-based source line it starts on.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    /// 1-based source line where the token begins.
    pub line: u16,
}

/// The lexical categories of the Tier-2 subset.
///
/// Note: `PartialEq` (not `Eq`) because the float literal variants carry
/// `f32`/`f64`. The parser only ever compares against non-literal kinds, so
/// partial equality is sufficient.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    // Keywords.
    Public,
    Class,
    Static,
    Void,
    Int,
    Long,
    Float,
    Double,
    Boolean,
    Char,
    Byte,
    Short,
    True,
    False,
    If,
    Else,

    // Literals and names.
    /// An identifier (not a keyword).
    Ident(String),
    /// An integer literal, resolved to its 32-bit value (any radix/form).
    IntLit(i32),
    /// A `long` literal, resolved to its 64-bit value.
    LongLit(i64),
    /// A `float` literal.
    FloatLit(f32),
    /// A `double` literal.
    DoubleLit(f64),
    /// A character literal, as its UTF-16 code unit.
    CharLit(u16),
    /// A string literal with escapes already decoded to real characters.
    StringLit(String),

    // Punctuation / operators.
    LBrace,    // {
    RBrace,    // }
    LParen,    // (
    RParen,    // )
    LBracket,  // [
    RBracket,  // ]
    Semicolon, // ;
    Comma,     // ,
    Dot,       // .
    Assign,    // =
    Plus,      // +
    Minus,     // -
    Star,      // *
    Slash,     // /
    Percent,   // %
    Amp,       // &
    Pipe,      // |
    Caret,     // ^
    Tilde,     // ~
    Bang,      // !
    Shl,       // <<
    Shr,       // >>
    UShr,      // >>>
    Lt,        // <
    Gt,        // >
    Le,        // <=
    Ge,        // >=
    EqEq,      // ==
    NotEq,     // !=
    PlusPlus,  // ++
    MinusMinus, // --
    PlusEq,    // +=
    MinusEq,   // -=
    StarEq,    // *=
    SlashEq,   // /=
    PercentEq, // %=
    AmpEq,     // &=
    PipeEq,    // |=
    CaretEq,   // ^=
    ShlEq,     // <<=
    ShrEq,     // >>=
    UShrEq,    // >>>=

    /// End of input.
    Eof,
}

/// Tokenize `source` into a flat token stream terminated by a single `Eof`.
///
/// Panics on lexical errors (unterminated string/comment, unknown character,
/// bad escape). The fixtures are well-formed, so this stays simple.
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
            } else if b == b'\'' {
                self.char_literal()
            } else if b.is_ascii_digit() {
                self.number()
            } else if b == b'.' && self.peek2().is_some_and(|c| c.is_ascii_digit()) {
                self.number()
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
                    let cp = self.decode_escape();
                    s.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                }
                Some(b) => {
                    self.bump();
                    s.push(b as char);
                }
            }
        }
    }

    fn char_literal(&mut self) -> TokenKind {
        self.bump(); // opening '
        let cp = match self.peek() {
            None => panic!("unterminated character literal"),
            Some(b'\'') => panic!("empty character literal"),
            Some(b'\\') => {
                self.bump();
                self.decode_escape()
            }
            Some(b) => {
                self.bump();
                b as u32
            }
        };
        match self.peek() {
            Some(b'\'') => {
                self.bump();
            }
            _ => panic!("unterminated character literal"),
        }
        TokenKind::CharLit(cp as u16)
    }

    /// Decode a string/char escape, the backslash already consumed. Returns the
    /// resulting code point. Handles the simple escapes, `\s` (Java 15), octal
    /// escapes `\0`..`\377`, and `\uXXXX` (one or more `u`s, then 4 hex digits).
    fn decode_escape(&mut self) -> u32 {
        let e = match self.peek() {
            None => panic!("unterminated escape"),
            Some(e) => e,
        };
        // Octal escape: \0 .. \377 (1-3 octal digits; 3 only when the first is 0-3).
        if (b'0'..=b'7').contains(&e) {
            let d0 = (self.bump() - b'0') as u32;
            let max_more = if d0 <= 3 { 2 } else { 1 };
            let mut val = d0;
            for _ in 0..max_more {
                match self.peek() {
                    Some(c) if (b'0'..=b'7').contains(&c) => {
                        self.bump();
                        val = val * 8 + (c - b'0') as u32;
                    }
                    _ => break,
                }
            }
            return val;
        }
        self.bump();
        match e {
            b't' => 0x09,
            b'n' => 0x0A,
            b'r' => 0x0D,
            b'"' => 0x22,
            b'\'' => 0x27,
            b'\\' => 0x5C,
            b'b' => 0x08,
            b'f' => 0x0C,
            b's' => 0x20, // Java 15: escaped space
            b'u' => {
                // One or more 'u's may follow the backslash; skip the extras.
                while self.peek() == Some(b'u') {
                    self.bump();
                }
                let mut val: u32 = 0;
                for _ in 0..4 {
                    let h = match self.peek() {
                        Some(c) if c.is_ascii_hexdigit() => c,
                        _ => panic!("bad \\u escape"),
                    };
                    self.bump();
                    val = val * 16 + (h as char).to_digit(16).unwrap();
                }
                val
            }
            other => panic!("unknown escape: \\{}", other as char),
        }
    }

    /// Scan a numeric literal (integer or floating point) and resolve it to a
    /// typed value token.
    fn number(&mut self) -> TokenKind {
        // Hex / binary integer prefixes: 0x.. / 0b.. (never floating in the subset).
        if self.peek() == Some(b'0') && matches!(self.peek2(), Some(b'x' | b'X' | b'b' | b'B')) {
            self.bump(); // 0
            let radix: u32 = if matches!(self.bump(), b'x' | b'X') { 16 } else { 2 };
            let mut digits = String::new();
            while let Some(c) = self.peek() {
                if c == b'_' {
                    self.bump();
                } else if (radix == 16 && c.is_ascii_hexdigit())
                    || (radix == 2 && (c == b'0' || c == b'1'))
                {
                    digits.push(c as char);
                    self.bump();
                } else {
                    break;
                }
            }
            if matches!(self.peek(), Some(b'L' | b'l')) {
                self.bump();
                return TokenKind::LongLit(long_from_str(&digits, radix));
            }
            return TokenKind::IntLit(int_from_str(&digits, radix));
        }

        // Decimal / octal integer, or floating point.
        let mut text = String::new();
        let mut is_float = false;
        while let Some(c) = self.peek() {
            if c == b'_' {
                self.bump();
            } else if c.is_ascii_digit() {
                text.push(c as char);
                self.bump();
            } else {
                break;
            }
        }
        // Fractional part. In the subset a `.` following digits is always a
        // decimal point (there is no `123.member`), so this is unambiguous.
        if self.peek() == Some(b'.') {
            is_float = true;
            text.push('.');
            self.bump();
            while let Some(c) = self.peek() {
                if c == b'_' {
                    self.bump();
                } else if c.is_ascii_digit() {
                    text.push(c as char);
                    self.bump();
                } else {
                    break;
                }
            }
        }
        // Exponent.
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            text.push('e');
            self.bump();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                text.push(self.bump() as char);
            }
            while let Some(c) = self.peek() {
                if c == b'_' {
                    self.bump();
                } else if c.is_ascii_digit() {
                    text.push(c as char);
                    self.bump();
                } else {
                    break;
                }
            }
        }
        // Type suffix.
        match self.peek() {
            Some(b'L' | b'l') => {
                self.bump();
                let radix = if is_octal(&text) { 8 } else { 10 };
                return TokenKind::LongLit(long_from_str(&text, radix));
            }
            Some(b'f' | b'F') => {
                self.bump();
                return TokenKind::FloatLit(text.parse().expect("float literal"));
            }
            Some(b'd' | b'D') => {
                self.bump();
                return TokenKind::DoubleLit(text.parse().expect("double literal"));
            }
            _ => {}
        }
        if is_float {
            TokenKind::DoubleLit(text.parse().expect("double literal"))
        } else if is_octal(&text) {
            TokenKind::IntLit(int_from_str(&text, 8))
        } else {
            TokenKind::IntLit(int_from_str(&text, 10))
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

    /// Punctuation and operators, longest-match first for the multi-char forms.
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
            b'~' => TokenKind::Tilde,
            b'!' => self.if_eq(TokenKind::NotEq, TokenKind::Bang),
            b'=' => self.if_eq(TokenKind::EqEq, TokenKind::Assign),
            b'+' => self.after('+', TokenKind::PlusPlus, TokenKind::PlusEq, TokenKind::Plus),
            b'-' => self.after('-', TokenKind::MinusMinus, TokenKind::MinusEq, TokenKind::Minus),
            b'*' => self.if_eq(TokenKind::StarEq, TokenKind::Star),
            b'/' => self.if_eq(TokenKind::SlashEq, TokenKind::Slash),
            b'%' => self.if_eq(TokenKind::PercentEq, TokenKind::Percent),
            b'&' => self.if_eq2(b'&', None, TokenKind::AmpEq, TokenKind::Amp),
            b'|' => self.if_eq2(b'|', None, TokenKind::PipeEq, TokenKind::Pipe),
            b'^' => self.if_eq(TokenKind::CaretEq, TokenKind::Caret),
            b'<' => self.less(),
            b'>' => self.greater(),
            other => panic!("unexpected character: {:?}", other as char),
        }
    }

    /// For `+`/`-`: a doubled form (`++`/`--`), an `=` form (`+=`), else single.
    fn after(&mut self, ch: char, doubled: TokenKind, eq: TokenKind, single: TokenKind) -> TokenKind {
        match self.peek() {
            Some(c) if c == ch as u8 => {
                self.bump();
                doubled
            }
            Some(b'=') => {
                self.bump();
                eq
            }
            _ => single,
        }
    }

    /// `op=` if the next byte is `=`, else `op`.
    fn if_eq(&mut self, eq: TokenKind, single: TokenKind) -> TokenKind {
        if self.peek() == Some(b'=') {
            self.bump();
            eq
        } else {
            single
        }
    }

    /// For `&`/`|`: reject the logical doubled form (out of subset), handle `op=`,
    /// else single. `_doubled` is unused but documents intent.
    fn if_eq2(&mut self, ch: u8, _doubled: Option<TokenKind>, eq: TokenKind, single: TokenKind) -> TokenKind {
        match self.peek() {
            Some(c) if c == ch => panic!("logical `{}{}` is not in the subset", ch as char, ch as char),
            Some(b'=') => {
                self.bump();
                eq
            }
            _ => single,
        }
    }

    /// `<`, `<=`, `<<`, `<<=`.
    fn less(&mut self) -> TokenKind {
        if self.peek() == Some(b'<') {
            self.bump();
            if self.peek() == Some(b'=') {
                self.bump();
                TokenKind::ShlEq
            } else {
                TokenKind::Shl
            }
        } else if self.peek() == Some(b'=') {
            self.bump();
            TokenKind::Le
        } else {
            TokenKind::Lt
        }
    }

    /// `>`, `>=`, `>>`, `>>=`, `>>>`, `>>>=`.
    fn greater(&mut self) -> TokenKind {
        if self.peek() != Some(b'>') {
            return if self.peek() == Some(b'=') {
                self.bump();
                TokenKind::Ge
            } else {
                TokenKind::Gt
            };
        }
        self.bump(); // second >
        if self.peek() == Some(b'>') {
            self.bump(); // third >
            if self.peek() == Some(b'=') {
                self.bump();
                TokenKind::UShrEq
            } else {
                TokenKind::UShr
            }
        } else if self.peek() == Some(b'=') {
            self.bump();
            TokenKind::ShrEq
        } else {
            TokenKind::Shr
        }
    }
}

/// True if a decimal-form integer text is actually octal (leading zero, length
/// greater than one, e.g. "010"). A bare "0" is decimal zero.
fn is_octal(text: &str) -> bool {
    text.len() > 1 && text.starts_with('0')
}

/// Parse an integer literal's digit text (underscores already stripped) in the
/// given radix to its 32-bit pattern. Values are read into a `u32` so the full
/// unsigned range (`0xFFFFFFFF` -> `-1`) round-trips to the right `i32` bits.
fn int_from_str(digits: &str, radix: u32) -> i32 {
    u32::from_str_radix(digits, radix)
        .unwrap_or_else(|_| panic!("integer literal out of range: {digits} (radix {radix})"))
        as i32
}

/// Parse a `long` literal's digit text in the given radix to its 64-bit pattern,
/// via `u64` for the same full-range round-trip.
fn long_from_str(digits: &str, radix: u32) -> i64 {
    u64::from_str_radix(digits, radix)
        .unwrap_or_else(|_| panic!("long literal out of range: {digits} (radix {radix})"))
        as i64
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b == b'$'
}

fn is_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}
