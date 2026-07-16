//! Semantic analysis: name/slot resolution and expression typing.
//!
//! Walks `main`'s statements, assigns each local a JVM slot (parameters occupy
//! the low slots; locals follow in declaration order), and records each local's
//! type. `long`/`double` are **two slots wide**, so slot indices bump by width —
//! this is the allocator change the whole numeric subset rests on.
//!
//! `type_of` computes the static type of any expression, implementing Java's
//! unary and binary numeric promotion (comparisons and `!` type to `boolean`).
//! Codegen consults it to pick load/store opcodes, conversion opcodes, `println`
//! descriptors, constant-load ladders, and comparison branch opcodes. Slot
//! allocation still walks only method-body declarations (branch bodies introduce
//! no new locals in this subset), so this stays a single linear pass.

use std::collections::HashMap;

use crate::ast::{BinOp, CompilationUnit, Expr, Method, StmtKind, Type};
use crate::diagnostic::CompileResult;

/// The static type of an expression / local in the subset: the eight primitives
/// plus `String` (only ever a string-literal `println` argument).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValType {
    Int,
    Long,
    Float,
    Double,
    Boolean,
    Char,
    Byte,
    Short,
    String,
}

impl ValType {
    /// Local-slot / operand-stack width in words: `long`/`double` are 2, all
    /// others (including the sub-int types, which live as `int` on the stack) 1.
    pub fn width(self) -> u16 {
        match self {
            ValType::Long | ValType::Double => 2,
            _ => 1,
        }
    }

    /// The JVM *computational* type this value occupies on the operand stack. The
    /// sub-int types (`boolean`/`char`/`byte`/`short`) are all `Int` on the stack.
    pub fn stack(self) -> StackTy {
        match self {
            ValType::Long => StackTy::Long,
            ValType::Float => StackTy::Float,
            ValType::Double => StackTy::Double,
            _ => StackTy::Int,
        }
    }

    /// Whether this is one of the sub-int integral types stored as an `int`.
    pub fn is_subint(self) -> bool {
        matches!(self, ValType::Boolean | ValType::Char | ValType::Byte | ValType::Short)
    }
}

/// The four JVM operand-stack computational types the subset can produce (plus
/// `reference`, which only `String` uses and which never participates in
/// arithmetic here).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackTy {
    Int,
    Long,
    Float,
    Double,
}

/// The `ValType` an AST declared type denotes.
pub fn valtype(ty: Type) -> ValType {
    match ty {
        Type::Int => ValType::Int,
        Type::Long => ValType::Long,
        Type::Float => ValType::Float,
        Type::Double => ValType::Double,
        Type::Boolean => ValType::Boolean,
        Type::Char => ValType::Char,
        Type::Byte => ValType::Byte,
        Type::Short => ValType::Short,
        // `String[]` is only ever `main`'s parameter, never read as a value; give
        // it a placeholder so slot allocation can size it (one slot).
        Type::StringArray => ValType::String,
    }
}

/// Analysis result for one method: local slots, local types, and the slot count.
pub struct MethodInfo {
    /// Slot index for each local (parameters included).
    pub slots: HashMap<String, u16>,
    /// Declared type of each local (parameters included).
    pub types: HashMap<String, ValType>,
    /// Number of local slots occupied by parameters + declared locals, counting
    /// `long`/`double` as two. A lower bound on `max_locals`.
    pub local_count: u16,
}

impl MethodInfo {
    pub fn slot(&self, name: &str) -> u16 {
        *self
            .slots
            .get(name)
            .unwrap_or_else(|| panic!("undeclared local: {name}"))
    }

    pub fn ty(&self, name: &str) -> ValType {
        *self
            .types
            .get(name)
            .unwrap_or_else(|| panic!("undeclared local: {name}"))
    }
}

/// Whole-program analysis result: one `MethodInfo` per method, in method order.
pub struct Analysis {
    pub methods: Vec<MethodInfo>,
}

/// Analyze a parsed compilation unit, assigning local slots for each method.
pub fn analyze(unit: &CompilationUnit) -> CompileResult<Analysis> {
    let methods = unit.class.methods.iter().map(analyze_method).collect();
    Ok(Analysis { methods })
}

fn analyze_method(method: &Method) -> MethodInfo {
    let mut slots: HashMap<String, u16> = HashMap::new();
    let mut types: HashMap<String, ValType> = HashMap::new();
    let mut next: u16 = 0;

    let mut declare = |name: &str, ty: ValType, next: &mut u16| {
        slots.insert(name.to_string(), *next);
        types.insert(name.to_string(), ty);
        *next += ty.width();
    };

    // Parameters take the low slots (only `String[] args` in the subset).
    for param in &method.params {
        declare(&param.name, valtype(param.ty), &mut next);
    }
    // Locals follow in declaration order, each bumping by its width.
    for stmt in &method.body {
        if let StmtKind::LocalDecl { ty, name, .. } = &stmt.kind {
            declare(name, valtype(*ty), &mut next);
        }
    }

    MethodInfo { slots, types, local_count: next }
}

/// Unary numeric promotion: `byte`/`short`/`char` (and `boolean`) become `int`;
/// wider types are unchanged. Applied to the operand of a unary op and to the
/// left operand of a shift.
pub fn unary_promote(t: ValType) -> ValType {
    match t {
        ValType::Long => ValType::Long,
        ValType::Float => ValType::Float,
        ValType::Double => ValType::Double,
        ValType::Boolean => ValType::Boolean,
        _ => ValType::Int,
    }
}

/// Binary numeric promotion: the wider of the two operand types, with everything
/// narrower than `int` promoted to `int`.
pub fn binary_promote(a: ValType, b: ValType) -> ValType {
    use ValType::*;
    if a == Double || b == Double {
        Double
    } else if a == Float || b == Float {
        Float
    } else if a == Long || b == Long {
        Long
    } else {
        Int
    }
}

/// The static type of an expression, implementing promotion. `Println` is a
/// `void` call and never appears as a value operand.
pub fn type_of(expr: &Expr, info: &MethodInfo) -> ValType {
    match expr {
        Expr::IntLit(_) => ValType::Int,
        Expr::LongLit(_) => ValType::Long,
        Expr::FloatLit(_) => ValType::Float,
        Expr::DoubleLit(_) => ValType::Double,
        Expr::BoolLit(_) => ValType::Boolean,
        Expr::CharLit(_) => ValType::Char,
        Expr::StringLit(_) => ValType::String,
        Expr::Name(n) => info.ty(n),
        Expr::Neg(e) => unary_promote(type_of(e, info)),
        Expr::BitNot(e) => unary_promote(type_of(e, info)),
        Expr::Not(_) => ValType::Boolean,
        Expr::Paren(e) => type_of(e, info),
        Expr::Compare { .. } => ValType::Boolean,
        Expr::Logical { .. } => ValType::Boolean,
        Expr::Cast { ty, .. } => valtype(*ty),
        Expr::Binary { op, left, right } => {
            let lt = type_of(left, info);
            let rt = type_of(right, info);
            // `&`/`|`/`^` on two booleans is boolean (non-short-circuit logical).
            if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor)
                && lt == ValType::Boolean
                && rt == ValType::Boolean
            {
                ValType::Boolean
            } else if op.is_shift() {
                unary_promote(lt)
            } else {
                binary_promote(lt, rt)
            }
        }
        Expr::Println(_) => ValType::Int,
    }
}
