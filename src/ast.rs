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
//! Expressions live in a compilation-unit-owned arena and refer to children by
//! stable `ExprId`. Every statement carries the 1-based source line it starts on
//! (plus the class carries the line of its closing brace) so codegen can build
//! the LineNumberTable byte-identically to javac.

use std::borrow::Cow;

use crate::span::Span;

pub const JAVA_LANG_STRING: &str = "java/lang/String";
pub const JAVA_LANG_OBJECT: &str = "java/lang/Object";

/// One source-level name occurrence.
#[derive(Debug)]
pub struct Name {
    pub text: String,
    pub span: Span,
}

/// A whole compilation unit: exactly one top-level class.
#[derive(Debug)]
pub struct CompilationUnit {
    pub span: Span,
    pub class: Class,
    pub exprs: ExprArena,
}

/// `public class Name { ... }`.
#[derive(Debug)]
pub struct Class {
    pub span: Span,
    pub name: String,
    pub name_span: Span,
    /// Canonical JVM internal name of the superclass. Classes without an explicit
    /// `extends` clause inherit `java/lang/Object`.
    pub super_class: String,
    /// Source line of the class declaration (used for the `<init>` line entry).
    pub line: u16,
    /// Source line of the class's closing brace.
    pub close_line: u16,
    pub methods: Vec<Method>,
}

/// A method declaration, e.g. `public static void main(String[] args)`.
#[derive(Debug)]
pub struct Method {
    pub span: Span,
    pub name: String,
    pub name_span: Span,
    pub is_static: bool,
    pub return_type: Type,
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
    /// Source line of the method's closing brace (target of the trailing return).
    pub close_line: u16,
}

/// One formal parameter: a name and its type.
#[derive(Debug)]
pub struct Param {
    pub span: Span,
    pub name: Name,
    pub ty: Type,
}

/// A primitive Java type. This copyable leaf keeps numeric promotion and opcode
/// selection cheap without creating a second semantic type universe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrimitiveType {
    Int,
    Long,
    Float,
    Double,
    Boolean,
    Char,
    Byte,
    Short,
}

impl PrimitiveType {
    pub fn width(self) -> u16 {
        match self {
            PrimitiveType::Long | PrimitiveType::Double => 2,
            _ => 1,
        }
    }

    pub fn is_numeric(self) -> bool {
        self != PrimitiveType::Boolean
    }

    pub fn is_integral(self) -> bool {
        matches!(
            self,
            PrimitiveType::Int
                | PrimitiveType::Long
                | PrimitiveType::Char
                | PrimitiveType::Byte
                | PrimitiveType::Short
        )
    }

    fn descriptor(self) -> char {
        match self {
            PrimitiveType::Int => 'I',
            PrimitiveType::Long => 'J',
            PrimitiveType::Float => 'F',
            PrimitiveType::Double => 'D',
            PrimitiveType::Boolean => 'Z',
            PrimitiveType::Char => 'C',
            PrimitiveType::Byte => 'B',
            PrimitiveType::Short => 'S',
        }
    }
}

/// One Java semantic type. Class names are canonical JVM internal names; arrays
/// recursively retain their element type instead of using a one-off `String[]`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Type {
    Void,
    Primitive(PrimitiveType),
    Class(Cow<'static, str>),
    Array(Box<Type>),
}

impl Type {
    pub fn string() -> Self {
        Type::Class(Cow::Borrowed(JAVA_LANG_STRING))
    }

    pub fn string_array() -> Self {
        Type::Array(Box::new(Type::string()))
    }

    pub fn as_primitive(&self) -> Option<PrimitiveType> {
        match self {
            Type::Primitive(ty) => Some(*ty),
            Type::Void | Type::Class(_) | Type::Array(_) => None,
        }
    }

    pub fn is_void(&self) -> bool {
        matches!(self, Type::Void)
    }

    pub fn primitive(&self) -> PrimitiveType {
        self.as_primitive().expect("reference type used as a primitive")
    }

    pub fn is_boolean(&self) -> bool {
        self.as_primitive() == Some(PrimitiveType::Boolean)
    }

    pub fn is_string(&self) -> bool {
        matches!(self, Type::Class(name) if name.as_ref() == JAVA_LANG_STRING)
    }

    pub fn is_string_array(&self) -> bool {
        matches!(self, Type::Array(element) if element.is_string())
    }

    pub fn width(&self) -> u16 {
        match self {
            Type::Void => panic!("void has no local-slot width"),
            Type::Primitive(ty) => ty.width(),
            Type::Class(_) | Type::Array(_) => 1,
        }
    }

    pub fn write_descriptor(&self, out: &mut String) {
        match self {
            Type::Void => out.push('V'),
            Type::Primitive(ty) => out.push(ty.descriptor()),
            Type::Class(name) => {
                out.push('L');
                out.push_str(name);
                out.push(';');
            }
            Type::Array(element) => {
                out.push('[');
                element.write_descriptor(out);
            }
        }
    }

    pub fn verifier_name(&self) -> Option<String> {
        match self {
            Type::Void | Type::Primitive(_) => None,
            Type::Class(name) => Some(name.to_string()),
            Type::Array(_) => {
                let mut descriptor = String::new();
                self.write_descriptor(&mut descriptor);
                Some(descriptor)
            }
        }
    }
}

impl From<PrimitiveType> for Type {
    fn from(value: PrimitiveType) -> Self {
        Type::Primitive(value)
    }
}

/// A single statement, tagged with the source line it begins on.
#[derive(Debug)]
pub struct Stmt {
    pub span: Span,
    pub line: u16,
    pub kind: StmtKind,
}

/// One `if`/`else` arm, preserving whether Java source used braces.
#[derive(Debug)]
pub struct BranchBody {
    pub span: Span,
    pub braced: bool,
    pub stmts: Vec<Stmt>,
}

#[derive(Debug)]
pub enum StmtKind {
    /// `<ty> name = init;` (initializer optional).
    LocalDecl {
        ty: Type,
        name: Name,
        init: Option<ExprId>,
    },
    /// `name = value;` — plain assignment to an already-declared local.
    Assign {
        name: Name,
        value: ExprId,
    },
    /// `name <op>= value;` — compound assignment. `++`/`--` are lowered here with
    /// `op = Add`/`Sub` and `value = IntLit(1)`. Pre/post form is irrelevant in
    /// statement position (the produced value is discarded), so it is not stored.
    CompoundAssign {
        name: Name,
        op: BinOp,
        value: ExprId,
    },
    /// `if (cond) <then> [else <else>]`. Each branch is the block (or single
    /// statement) it guards. `else if` is just an `If` nested as the sole
    /// statement of `else_branch`. The enclosing `Stmt`'s line is the condition's
    /// source position; codegen marks it pending for the next emitted instruction.
    If {
        cond: ExprId,
        then_branch: BranchBody,
        else_branch: Option<BranchBody>,
    },
    /// An expression used as a statement (only `System.out.println(...)`).
    Expr(ExprId),
}

/// Stable parser-assigned identity for one expression node.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ExprId(usize);

impl ExprId {
    pub(crate) fn index(self) -> usize {
        self.0
    }
}

/// Append-only storage for every expression payload in one compilation unit.
/// Parser allocation order is child-before-parent, preserving the exact tree
/// shape while allowing semantic facts to use `ExprId` as a dense table index.
#[derive(Default, Debug)]
pub struct ExprArena {
    nodes: Vec<ExprKind>,
}

impl ExprArena {
    pub(crate) fn alloc(&mut self, kind: ExprKind) -> ExprId {
        let id = ExprId(self.nodes.len());
        self.nodes.push(kind);
        id
    }

    pub(crate) fn identity(&self) -> (usize, usize) {
        (self.nodes.as_ptr() as usize, self.nodes.len())
    }
}

impl std::ops::Index<ExprId> for ExprArena {
    type Output = ExprKind;

    fn index(&self, id: ExprId) -> &Self::Output {
        &self.nodes[id.index()]
    }
}

/// Expression payload. Recursive children are stable arena indices.
#[derive(Debug)]
pub enum ExprKind {
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
    Name(Name),
    /// Unary minus, e.g. `-x`. A literal operand is constant-folded by codegen.
    Neg(ExprId),
    /// Unary bitwise complement `~x` (int/long).
    BitNot(ExprId),
    /// Logical negation `!x` (boolean).
    Not(ExprId),
    /// A parenthesized expression. Grouping is semantically transparent, but the
    /// syntax can affect javac's boolean-item lowering and must survive parsing.
    Paren(ExprId),
    /// An explicit primitive cast `(Type) expr`.
    Cast {
        ty: Type,
        expr: ExprId,
    },
    /// A binary arithmetic / bitwise / shift expression.
    Binary {
        op: BinOp,
        left: ExprId,
        right: ExprId,
    },
    /// A relational / equality comparison (`< <= > >= == !=`). Its static type is
    /// `boolean`; codegen lowers it either as a conditional branch (condition
    /// context) or as a materialized 0/1 (value context).
    Compare {
        op: CmpOp,
        left: ExprId,
        right: ExprId,
    },
    /// Short-circuit `&&` / `||`. Distinct from the bitwise `Binary { And | Or }`
    /// on booleans (those push both operands and emit `iand`/`ior`); these lower
    /// to a jump chain (javac's `genCond`) and never evaluate the right operand
    /// when the left already decides the result.
    Logical {
        op: LogOp,
        left: ExprId,
        right: ExprId,
    },
    /// `System.out.println(arg)`.
    Println(ExprId),
}

/// The two short-circuit logical operators. Their operands are `boolean`; the
/// result is `boolean`, lowered as a conditional jump chain rather than stack ops.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogOp {
    And, // &&
    Or,  // ||
}

/// The binary operators of the subset: arithmetic, bitwise, and shift. All are
/// left-associative. Comparisons live in their own `CmpOp`/`ExprKind::Compare` and
/// short-circuit `&&`/`||` in `LogOp`/`ExprKind::Logical` (all lower to branches, not
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
