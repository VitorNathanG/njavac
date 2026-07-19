use super::{Lexer, TokenKind};
use crate::diagnostic::{CompileResult, Diagnostic};
use crate::span::Span;

impl Lexer<'_> {
    /// Punctuation and operators, longest-match first for the multi-char forms.
    pub(super) fn punct(&mut self) -> CompileResult<TokenKind> {
        let b = self.bump();
        let kind = match b {
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
            b'-' => self.after(
                '-',
                TokenKind::MinusMinus,
                TokenKind::MinusEq,
                TokenKind::Minus,
            ),
            b'*' => self.if_eq(TokenKind::StarEq, TokenKind::Star),
            b'/' => self.if_eq(TokenKind::SlashEq, TokenKind::Slash),
            b'%' => self.if_eq(TokenKind::PercentEq, TokenKind::Percent),
            b'&' => self.if_eq2(b'&', TokenKind::AmpAmp, TokenKind::AmpEq, TokenKind::Amp),
            b'|' => self.if_eq2(
                b'|',
                TokenKind::PipePipe,
                TokenKind::PipeEq,
                TokenKind::Pipe,
            ),
            b'^' => self.if_eq(TokenKind::CaretEq, TokenKind::Caret),
            b'<' => self.less(),
            b'>' => self.greater(),
            b'?' | b':' | b'@' => {
                return Err(Diagnostic::unsupported_syntax(
                    Span::new(self.pos - 1, self.pos),
                    format!("unsupported Java punctuator: {}", b as char),
                ));
            }
            other => {
                return Err(Diagnostic::lexical(
                    Span::new(self.pos - 1, self.pos),
                    format!("unexpected character: {:?}", other as char),
                ));
            }
        };
        Ok(kind)
    }

    /// For `+`/`-`: a doubled form (`++`/`--`), an `=` form (`+=`), else single.
    fn after(
        &mut self,
        ch: char,
        doubled: TokenKind,
        eq: TokenKind,
        single: TokenKind,
    ) -> TokenKind {
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

    /// For `&`/`|`: the doubled logical form (`&&`/`||`), the `op=` compound form,
    /// else the single operator. Longest-match: the doubled form is checked first.
    fn if_eq2(
        &mut self,
        ch: u8,
        doubled: TokenKind,
        eq: TokenKind,
        single: TokenKind,
    ) -> TokenKind {
        match self.peek() {
            Some(c) if c == ch => {
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
