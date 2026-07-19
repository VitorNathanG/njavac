//! Fixed facade boundary for the njavac Java 25 compiler.

pub mod diagnostic {
    pub use njavac_compiler::diagnostic::{
        CompileResult, Diagnostic, DiagnosticCode, ErrorKind, Severity,
    };
}

pub mod span {
    pub use njavac_compiler::span::Span;
}

/// Compile Java source text to one `.class` byte vector.
///
/// This is the fixed single-source contract. Compiler implementation modules and
/// repository instrumentation belong to unpublished workspace crates.
pub fn compile(source: &str, source_file: &str) -> diagnostic::CompileResult<Vec<u8>> {
    njavac_compiler::compile(source, source_file)
}
