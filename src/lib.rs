//! njavac — a toy Java 25 compiler (library crate).
//!
//! Pipeline: source text -> lexer -> parser -> sema -> codegen -> class bytes.
//! For the documented supported language, the complete pipeline must preserve
//! the repository-pinned javac's behavior and retain its bytes when practical.

pub mod classfile;
pub mod classdump;
pub mod span;
pub mod diagnostic;
mod fxhash;
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod sema;
pub mod codegen;

/// Compile Java source text to `.class` bytes.
///
/// This is the one fixed contract of the front-end build: the internal module
/// boundaries and types may be redesigned freely, but this signature, its
/// behaviorally compatible class generation, and practical byte retention must
/// hold.
///
/// `source_file` is the basename used for the `SourceFile` attribute
/// (e.g. "Foo.java"); the class name itself comes from the parsed source.
pub fn compile(source: &str, source_file: &str) -> diagnostic::CompileResult<Vec<u8>> {
    let tokens = lexer::lex(source)?;
    let unit = parser::parse(tokens)?;
    let analysis = sema::analyze(&unit)?;
    codegen::generate(&unit, &analysis, source_file)
}
