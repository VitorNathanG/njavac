use super::{Lexer, TokenKind};
use crate::diagnostic::{CompileResult, Diagnostic};
use crate::span::Span;

impl Lexer<'_> {
    pub(super) fn string_literal(&mut self) -> CompileResult<TokenKind> {
        let start = self.pos;
        self.bump(); // opening quote
        let mut s = String::new();
        loop {
            match self.peek() {
                None => {
                    return Err(Diagnostic::lexical(
                        Span::new(start, self.pos),
                        "unterminated string literal",
                    ));
                }
                Some(b'\n') => {
                    return Err(Diagnostic::lexical(
                        Span::new(start, self.pos),
                        "newline in string literal",
                    ));
                }
                Some(b'"') => {
                    self.bump();
                    return Ok(TokenKind::StringLit(s));
                }
                Some(b'\\') => {
                    self.bump();
                    let cp = self.decode_escape()?;
                    s.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                }
                Some(b) => {
                    self.bump();
                    s.push(b as char);
                }
            }
        }
    }

    pub(super) fn char_literal(&mut self) -> CompileResult<TokenKind> {
        let start = self.pos;
        self.bump(); // opening '
        let cp = match self.peek() {
            None => {
                return Err(Diagnostic::lexical(
                    Span::new(start, self.pos),
                    "unterminated character literal",
                ));
            }
            Some(b'\'') => {
                self.bump();
                return Err(Diagnostic::lexical(
                    Span::new(start, self.pos),
                    "empty character literal",
                ));
            }
            Some(b'\\') => {
                self.bump();
                self.decode_escape()?
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
            _ => {
                return Err(Diagnostic::lexical(
                    Span::new(start, self.pos),
                    "unterminated character literal",
                ));
            }
        }
        Ok(TokenKind::CharLit(cp as u16))
    }

    /// Decode a string/char escape, the backslash already consumed. Returns the
    /// resulting code point. Handles the simple escapes, `\s` (Java 15), octal
    /// escapes `\0`..`\377`, and `\uXXXX` (one or more `u`s, then 4 hex digits).
    fn decode_escape(&mut self) -> CompileResult<u32> {
        let start = self.pos.saturating_sub(1);
        let e = match self.peek() {
            None => {
                return Err(Diagnostic::lexical(
                    Span::new(start, self.pos),
                    "unterminated escape",
                ));
            }
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
            return Ok(val);
        }
        self.bump();
        let cp = match e {
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
                        _ => {
                            return Err(Diagnostic::lexical(
                                Span::new(start, self.pos),
                                "bad \\u escape",
                            ));
                        }
                    };
                    self.bump();
                    val = val * 16 + (h as char).to_digit(16).unwrap();
                }
                val
            }
            other => {
                return Err(Diagnostic::lexical(
                    Span::new(start, self.pos),
                    format!("unknown escape: \\{}", other as char),
                ));
            }
        };
        Ok(cp)
    }

    /// Scan a numeric literal (integer or floating point) and resolve it to a
    /// typed value token.
    pub(super) fn number(&mut self) -> CompileResult<TokenKind> {
        let start = self.pos;
        // Hex / binary integer prefixes: 0x.. / 0b.. (never floating in the subset).
        if self.peek() == Some(b'0') && matches!(self.peek2(), Some(b'x' | b'X' | b'b' | b'B')) {
            self.bump(); // 0
            let radix: u32 = if matches!(self.bump(), b'x' | b'X') {
                16
            } else {
                2
            };
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
                return long_from_str(&digits, radix)
                    .map(TokenKind::LongLit)
                    .map_err(|message| Diagnostic::lexical(Span::new(start, self.pos), message));
            }
            return int_from_str(&digits, radix)
                .map(TokenKind::IntLit)
                .map_err(|message| Diagnostic::lexical(Span::new(start, self.pos), message));
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
                return long_from_str(&text, radix)
                    .map(TokenKind::LongLit)
                    .map_err(|message| Diagnostic::lexical(Span::new(start, self.pos), message));
            }
            Some(b'f' | b'F') => {
                self.bump();
                return text.parse().map(TokenKind::FloatLit).map_err(|_| {
                    Diagnostic::lexical(Span::new(start, self.pos), "malformed float literal")
                });
            }
            Some(b'd' | b'D') => {
                self.bump();
                return text.parse().map(TokenKind::DoubleLit).map_err(|_| {
                    Diagnostic::lexical(Span::new(start, self.pos), "malformed double literal")
                });
            }
            _ => {}
        }
        if is_float {
            text.parse().map(TokenKind::DoubleLit).map_err(|_| {
                Diagnostic::lexical(Span::new(start, self.pos), "malformed double literal")
            })
        } else if is_octal(&text) {
            int_from_str(&text, 8)
                .map(TokenKind::IntLit)
                .map_err(|message| Diagnostic::lexical(Span::new(start, self.pos), message))
        } else {
            int_from_str(&text, 10)
                .map(TokenKind::IntLit)
                .map_err(|message| Diagnostic::lexical(Span::new(start, self.pos), message))
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
fn int_from_str(digits: &str, radix: u32) -> Result<i32, String> {
    u32::from_str_radix(digits, radix)
        .map(|value| value as i32)
        .map_err(|_| format!("malformed or out-of-range integer literal (radix {radix})"))
}

/// Parse a `long` literal's digit text in the given radix to its 64-bit pattern,
/// via `u64` for the same full-range round-trip.
fn long_from_str(digits: &str, radix: u32) -> Result<i64, String> {
    u64::from_str_radix(digits, radix)
        .map(|value| value as i64)
        .map_err(|_| format!("malformed or out-of-range long literal (radix {radix})"))
}
