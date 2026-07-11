//! Semantic analysis: name/slot resolution and expression typing (int vs
//! String), just enough to drive descriptor and opcode selection.
//!
//! The Tier-1 subset has one class with one `main` method. Analysis walks the
//! method's statements, assigns each `int` local a JVM slot (static method =>
//! parameters occupy the low slots, locals follow in declaration order), and
//! records the static type of the single `println` argument so codegen can pick
//! the right descriptor. There is no control flow and no user-defined types, so
//! this stays a single linear pass.

use std::collections::HashMap;

use crate::ast::{CompilationUnit, Expr, Method, StmtKind, Type};

/// The static type of an expression in the subset: either `int` or `String`.
///
/// Only ever needed to choose the `println` descriptor and (for future use) to
/// validate operands; all arithmetic operands are `int`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValType {
    Int,
    String,
}

/// Analysis result for a single method: how many local slots it needs and, for
/// each local name, which slot it lives in. Codegen consults `slot_of` to emit
/// the right load/store opcode.
pub struct MethodInfo {
    /// slot index for each local variable name (parameters included).
    pub slots: HashMap<String, u16>,
    /// Number of local slots occupied by parameters + declared locals. This is a
    /// lower bound on `max_locals`; codegen still takes the max with the highest
    /// slot it actually references (they coincide for the subset).
    pub local_count: u16,
}

/// Whole-program analysis result: one `MethodInfo` per method, in method order.
pub struct Analysis {
    pub methods: Vec<MethodInfo>,
}

/// Analyze a parsed compilation unit, assigning local slots for each method.
///
/// Panics on a reference to an undeclared name (the fixtures are well-formed).
pub fn analyze(unit: &CompilationUnit) -> Analysis {
    let methods = unit.class.methods.iter().map(analyze_method).collect();
    Analysis { methods }
}

fn analyze_method(method: &Method) -> MethodInfo {
    let mut slots: HashMap<String, u16> = HashMap::new();
    let mut next: u16 = 0;

    // Parameters take the low slots. In this subset the only parameter is
    // `String[] args`, one slot wide; an `int` parameter would also be one slot.
    for param in &method.params {
        let width = match param.ty {
            Type::Int => 1,
            Type::StringArray => 1,
        };
        slots.insert(param.name.clone(), next);
        next += width;
    }

    // Locals follow in declaration order. Each `int` occupies one slot; a slot is
    // reserved at the declaration point (before the initializer is evaluated, but
    // that ordering never matters here since the name is not in scope yet).
    for stmt in &method.body {
        if let StmtKind::LocalDecl { name, .. } = &stmt.kind {
            slots.insert(name.clone(), next);
            next += 1;
        }
    }

    MethodInfo { slots, local_count: next }
}

/// Static type of an expression. `Println` is a `void` call and never appears as
/// an operand, so it is not a value-typed form here; codegen types the argument.
pub fn type_of(expr: &Expr) -> ValType {
    match expr {
        Expr::IntLit(_) => ValType::Int,
        Expr::StringLit(_) => ValType::String,
        // All names in the subset are `int` locals (only `int` locals are
        // declarable, and `args` is never read).
        Expr::Name(_) => ValType::Int,
        Expr::Neg(_) => ValType::Int,
        Expr::Binary { .. } => ValType::Int,
        // Not a value in expression position; treated as int by convention.
        Expr::Println(_) => ValType::Int,
    }
}
