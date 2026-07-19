use crate::span::Span;

/// A single lexical token plus the 1-based source line it starts on.
#[derive(Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    /// 1-based source line where the token begins.
    pub line: u16,
    /// Half-open byte range in the original source text.
    pub span: Span,
}

/// The lexical categories of the supported subset.
///
/// Note: `PartialEq` (not `Eq`) because the float literal variants carry
/// `f32`/`f64`. The parser only ever compares against non-literal kinds, so
/// partial equality is sufficient.
#[derive(Debug, PartialEq)]
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
    AmpAmp,    // &&
    PipePipe,  // ||
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
