//! njavac — a toy Java 25 compiler (library crate).
//!
//! Pipeline: source text -> lexer -> parser -> sema -> codegen -> class bytes.
//! The `classfile` backend already matches javac's output byte-for-byte (it
//! reproduces javac's constant-pool interning order and attribute layout); the
//! other modules are being filled in to replace the earlier hand-lowering.

pub mod classfile;
pub mod lexer;
pub mod ast;
pub mod parser;
pub mod sema;
pub mod codegen;

/// Compile Java source text to `.class` bytes.
///
/// This is the one fixed contract of the front-end build: the internal module
/// boundaries and types may be redesigned freely, but this signature and its
/// byte-identical-to-javac behaviour must hold.
///
/// `source_file` is the basename used for the `SourceFile` attribute
/// (e.g. "Foo.java"); the class name itself comes from the parsed source.
pub fn compile(source: &str, source_file: &str) -> Vec<u8> {
    let tokens = lexer::lex(source);
    let unit = parser::parse(tokens);
    let analysis = sema::analyze(&unit);
    codegen::generate(&unit.class, &analysis, source_file)
}
