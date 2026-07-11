//! Abstract syntax tree for the Tier-1 (straight-line int) subset.
//!
//! The subset is one public class holding a single `main` method whose body is
//! a sequence of straight-line statements: int local declarations, assignments
//! to existing locals, and `System.out.println(...)` calls. There is no control
//! flow, so the tree is flat and non-branching.
//!
//! Recursion is expressed with `Box`, matching the plain-enum style of
//! `classfile.rs`. Every statement carries the 1-based source line it starts on
//! (plus the class carries the line of its closing brace) so codegen can build
//! the LineNumberTable byte-identically to javac.

/// A whole compilation unit: exactly one top-level class.
pub struct CompilationUnit {
    pub class: Class,
}

/// `public class Name { ... }`.
pub struct Class {
    pub name: String,
    /// Source line of the class declaration (used for the `<init>` line entry).
    pub line: u16,
    /// Source line of the class's closing brace.
    pub close_line: u16,
    pub methods: Vec<Method>,
}

/// A method declaration, e.g. `public static void main(String[] args)`.
pub struct Method {
    pub name: String,
    pub is_static: bool,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    /// Source line of the method's closing brace (target of the trailing return).
    pub close_line: u16,
}

/// One formal parameter: a name and its type.
pub struct Param {
    pub name: String,
    pub ty: Type,
}

/// The handful of types the subset can name.
pub enum Type {
    Int,
    /// `String[]`, only ever `main`'s parameter.
    StringArray,
}

/// A single statement, tagged with the source line it begins on.
pub struct Stmt {
    pub line: u16,
    pub kind: StmtKind,
}

pub enum StmtKind {
    /// `int name = init;` (initializer optional).
    LocalDecl {
        name: String,
        init: Option<Expr>,
    },
    /// `name = value;` — assignment to an already-declared local.
    Assign {
        name: String,
        value: Expr,
    },
    /// An expression used as a statement (only `System.out.println(...)`).
    Expr(Expr),
}

/// An expression. `Box` breaks the recursion for the compound forms.
pub enum Expr {
    /// An `int` literal, already parsed to its value.
    IntLit(i32),
    /// A string literal with escapes already decoded to real characters.
    StringLit(String),
    /// A reference to a local variable by name.
    Name(String),
    /// Unary minus, e.g. `-x`. A literal operand is constant-folded by codegen.
    Neg(Box<Expr>),
    /// A binary arithmetic expression.
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `System.out.println(arg)`.
    Println(Box<Expr>),
}

/// The integer arithmetic operators.
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}
