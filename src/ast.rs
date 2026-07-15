//! Abstract syntax tree for the numeric subset plus the first branch.
//!
//! The subset is one public class holding a single `main` method whose body is a
//! sequence of primitive local declarations, assignments (plain and compound) to
//! existing locals, `++`/`--`, `if`/`else` statements, and `System.out.println(...)`
//! calls. Expressions are genuinely typed (`int`/`long`/`float`/`double`/`boolean`/
//! `char`/`byte`/`short`) with the full arithmetic, bitwise, shift, comparison,
//! and conversion surface; `if`/`else` introduces the first control flow (and thus
//! the `StackMapTable`). Locals are still declared at method-body scope.
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

/// The types the subset can name. The eight primitives plus `String[]`, which
/// only ever appears as `main`'s parameter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Type {
    Int,
    Long,
    Float,
    Double,
    Boolean,
    Char,
    Byte,
    Short,
    /// `String[]`, only ever `main`'s parameter.
    StringArray,
}

/// A single statement, tagged with the source line it begins on.
pub struct Stmt {
    pub line: u16,
    pub kind: StmtKind,
}

pub enum StmtKind {
    /// `<ty> name = init;` (initializer optional).
    LocalDecl {
        ty: Type,
        name: String,
        init: Option<Expr>,
    },
    /// `name = value;` — plain assignment to an already-declared local.
    Assign {
        name: String,
        value: Expr,
    },
    /// `name <op>= value;` — compound assignment. `++`/`--` are lowered here with
    /// `op = Add`/`Sub` and `value = IntLit(1)`. Pre/post form is irrelevant in
    /// statement position (the produced value is discarded), so it is not stored.
    CompoundAssign {
        name: String,
        op: BinOp,
        value: Expr,
    },
    /// `if (cond) <then> [else <else>]`. Each branch is the block (or single
    /// statement) it guards. `else if` is just an `If` nested as the sole
    /// statement of `else_branch`. The enclosing `Stmt`'s line is the condition's
    /// source position; codegen marks it pending for the next emitted instruction.
    If {
        cond: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Option<Vec<Stmt>>,
    },
    /// An expression used as a statement (only `System.out.println(...)`).
    Expr(Expr),
}

/// An expression. `Box` breaks the recursion for the compound forms.
pub enum Expr {
    /// An `int` literal, already parsed to its 32-bit value.
    IntLit(i32),
    /// A `long` literal (`123L`).
    LongLit(i64),
    /// A `float` literal (`1.5f`).
    FloatLit(f32),
    /// A `double` literal (`1.5`, `1e9`).
    DoubleLit(f64),
    /// A `boolean` literal (`true`/`false`).
    BoolLit(bool),
    /// A character literal (`'a'`), stored as its UTF-16 code unit. Its static
    /// type is `char`; it loads by magnitude like an `int`.
    CharLit(u16),
    /// A string literal with escapes already decoded to real characters.
    StringLit(String),
    /// A reference to a local variable by name.
    Name(String),
    /// Unary minus, e.g. `-x`. A literal operand is constant-folded by codegen.
    Neg(Box<Expr>),
    /// Unary bitwise complement `~x` (int/long).
    BitNot(Box<Expr>),
    /// Logical negation `!x` (boolean).
    Not(Box<Expr>),
    /// An explicit primitive cast `(Type) expr`.
    Cast {
        ty: Type,
        expr: Box<Expr>,
    },
    /// A binary arithmetic / bitwise / shift expression.
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// A relational / equality comparison (`< <= > >= == !=`). Its static type is
    /// `boolean`; codegen lowers it either as a conditional branch (condition
    /// context) or as a materialized 0/1 (value context).
    Compare {
        op: CmpOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// Short-circuit `&&` / `||`. Distinct from the bitwise `Binary { And | Or }`
    /// on booleans (those push both operands and emit `iand`/`ior`); these lower
    /// to a jump chain (javac's `genCond`) and never evaluate the right operand
    /// when the left already decides the result.
    Logical {
        op: LogOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `System.out.println(arg)`.
    Println(Box<Expr>),
}

/// The two short-circuit logical operators. Their operands are `boolean`; the
/// result is `boolean`, lowered as a conditional jump chain rather than stack ops.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogOp {
    And, // &&
    Or,  // ||
}

/// The binary operators of the subset: arithmetic, bitwise, and shift. All are
/// left-associative. Comparisons live in their own `CmpOp`/`Expr::Compare` and the
/// short-circuit `&&`/`||` in `LogOp`/`Expr::Logical` (all lower to branches, not
/// stack ops). The bitwise `And`/`Or` here are the non-short-circuit `&`/`|`. `?:`
/// is not yet supported.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And, // &
    Or,  // |
    Xor, // ^
    Shl, // <<
    Shr, // >>
    UShr, // >>>
}

impl BinOp {
    /// Whether this is a shift operator. Shifts are special: the right operand is
    /// always an `int` (never widened to the left operand's type).
    pub fn is_shift(self) -> bool {
        matches!(self, BinOp::Shl | BinOp::Shr | BinOp::UShr)
    }
}

/// The relational / equality operators. Their operands undergo binary numeric
/// promotion; the result is always `boolean`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmpOp {
    Lt, // <
    Le, // <=
    Gt, // >
    Ge, // >=
    Eq, // ==
    Ne, // !=
}
