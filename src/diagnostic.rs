use std::fmt;

use crate::span::Span;

pub type CompileResult<T> = Result<T, Diagnostic>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    SyntaxError,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticCode {
    LexicalError,
    ParseError,
    SemanticError,
    UnsupportedSyntax,
    UnsupportedSemantic,
    UnsupportedCodegen,
}

impl DiagnosticCode {
    pub const fn kind(self) -> ErrorKind {
        match self {
            Self::LexicalError | Self::ParseError | Self::SemanticError => ErrorKind::SyntaxError,
            Self::UnsupportedSyntax | Self::UnsupportedSemantic | Self::UnsupportedCodegen => {
                ErrorKind::Unsupported
            }
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LexicalError => "NJL0001",
            Self::ParseError => "NJP0001",
            Self::SemanticError => "NJS0001",
            Self::UnsupportedSyntax => "NJP1001",
            Self::UnsupportedSemantic => "NJS1001",
            Self::UnsupportedCodegen => "NJC1001",
        }
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub span: Span,
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
}

impl Diagnostic {
    pub fn error(code: DiagnosticCode, span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            severity: Severity::Error,
            code,
            message: message.into(),
        }
    }

    pub fn lexical(span: Span, message: impl Into<String>) -> Self {
        Self::error(DiagnosticCode::LexicalError, span, message)
    }

    pub fn parse(span: Span, message: impl Into<String>) -> Self {
        Self::error(DiagnosticCode::ParseError, span, message)
    }

    pub fn unsupported_syntax(span: Span, message: impl Into<String>) -> Self {
        Self::error(DiagnosticCode::UnsupportedSyntax, span, message)
    }

    pub const fn kind(&self) -> ErrorKind {
        self.code.kind()
    }

    pub fn render(&self, file: &str, source: &str) -> String {
        let bytes = source.as_bytes();
        let start = self.span.start.min(bytes.len());
        let line_start = bytes[..start]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1);
        let line_end = bytes[start..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |i| start + i);
        let line = bytes[..start].iter().filter(|&&b| b == b'\n').count() + 1;
        let column = start - line_start + 1;
        let underline_end = self.span.end.max(start + 1).min(line_end.max(start + 1));
        let underline_len = underline_end - start;
        let source_line = String::from_utf8_lossy(&bytes[line_start..line_end]);

        format!(
            "{file}:{line}:{column}: {}[{}]: {}\n{source_line}\n{}{}",
            self.severity.as_str(),
            self.code,
            self.message,
            " ".repeat(column - 1),
            "^".repeat(underline_len),
        )
    }
}
