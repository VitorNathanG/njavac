//! Semantic analysis: validation, name/slot resolution, and expression typing.
//!
//! Walks `main`'s statements, resolves every name occurrence to a stable local
//! ID, and assigns each declaration a JVM slot. Parameters occupy the low slots;
//! lexical scopes reclaim their slots on exit while `max_locals` retains the
//! high-water mark. `long`/`double` are **two slots wide**.
//!
//! `type_of` computes the static type of any expression, implementing Java's
//! unary and binary numeric promotion (comparisons and `!` type to `boolean`).
//! Codegen consults it to pick load/store opcodes, conversion opcodes, `println`
//! descriptors, constant-load ladders, and comparison branch opcodes. Sema also
//! records the verifier-local state at method entry and around every statement;
//! branch-local declarations remain an explicit unsupported boundary.

mod analyzer;
mod constants;

use crate::ast::{CompilationUnit, ExprId, Name, PrimitiveType, Type};
use crate::diagnostic::{CompileResult, Diagnostic};
use crate::fxhash::{FxHashMap, FxHashSet};
use crate::span::Span;

/// Stable identity of one local declaration within a method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LocalId(usize);

/// Semantic information recorded for one parameter or local declaration.
pub struct LocalInfo {
    pub name: String,
    pub declaration: Span,
    pub ty: Type,
    pub slot: u16,
}

/// Sema's classfile-independent view of one verifier-local entry. `Long` and
/// `Double` each consume two physical local slots but occupy one entry here.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FrameLocal {
    Top,
    Integer,
    Float,
    Long,
    Double,
    Object(String),
}

struct StmtFrameLocals {
    entry: usize,
    exit: usize,
}

/// Analysis result for one method: ordered locals and occurrence resolution.
pub struct MethodInfo {
    /// Parameters followed by locals in declaration order.
    pub locals: Vec<LocalInfo>,
    resolutions: FxHashMap<Span, LocalId>,
    frame_locals: Vec<Vec<FrameLocal>>,
    entry_frame_locals: usize,
    stmt_frame_locals: FxHashMap<Span, StmtFrameLocals>,
    expr_type_base: Option<usize>,
    expr_types: Vec<Option<Type>>,
    calls: Vec<(ExprId, ResolvedCall)>,
    /// High-water local-slot count, counting `long`/`double` as two.
    pub max_locals: u16,
}

/// Classfile-independent identity of a method selected by sema. Source spelling
/// never reaches codegen; the payload records the selected overload.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ResolvedCall {
    Println { parameter_type: Type },
}

impl ResolvedCall {
    pub fn return_type(&self) -> Type {
        match self {
            ResolvedCall::Println { .. } => Type::Void,
        }
    }
}

impl MethodInfo {
    pub fn local_id(&self, name: &Name) -> LocalId {
        *self.resolutions.get(&name.span).unwrap_or_else(|| {
            panic!("sema did not resolve local occurrence `{}` at {:?}", name.text, name.span)
        })
    }

    pub fn local(&self, name: &Name) -> &LocalInfo {
        &self.locals[self.local_id(name).0]
    }

    pub fn slot(&self, name: &Name) -> u16 {
        self.local(name).slot
    }

    pub fn ty(&self, name: &Name) -> PrimitiveType {
        self.local(name).ty.primitive()
    }

    pub fn declared_type(&self, name: &Name) -> &Type {
        &self.local(name).ty
    }

    pub fn expr_type(&self, expr: ExprId) -> &Type {
        let base = self.expr_type_base.expect("method has no expression types");
        let index = expr
            .index()
            .checked_sub(base)
            .expect("expression belongs to a different method");
        self.expr_types
            .get(index)
            .and_then(Option::as_ref)
            .unwrap_or_else(|| panic!("sema did not record expression type for {expr:?}"))
    }

    pub fn call(&self, expr: ExprId) -> &ResolvedCall {
        self.calls
            .iter()
            .find(|(id, _)| *id == expr)
            .map(|(_, call)| call)
            .unwrap_or_else(|| panic!("sema did not resolve call {expr:?}"))
    }

    pub fn entry_frame_locals(&self) -> &[FrameLocal] {
        &self.frame_locals[self.entry_frame_locals]
    }

    pub fn stmt_entry_frame_locals(&self, span: Span) -> &[FrameLocal] {
        let state = self
            .stmt_frame_locals
            .get(&span)
            .unwrap_or_else(|| panic!("sema did not record statement entry at {span:?}"))
            .entry;
        &self.frame_locals[state]
    }

    pub fn stmt_exit_frame_locals(&self, span: Span) -> &[FrameLocal] {
        let state = self
            .stmt_frame_locals
            .get(&span)
            .unwrap_or_else(|| panic!("sema did not record statement exit at {span:?}"))
            .exit;
        &self.frame_locals[state]
    }
}

/// Whole-program analysis result: one `MethodInfo` per method, in method order.
pub struct Analysis {
    arena_identity: (usize, usize),
    pub(crate) methods: Vec<MethodInfo>,
}

/// Analyze a parsed compilation unit, assigning local slots for each method.
pub fn analyze(unit: &CompilationUnit) -> CompileResult<Analysis> {
    validate_class_shape(unit)?;
    Ok(Analysis {
        arena_identity: unit.exprs.identity(),
        methods: vec![analyzer::analyze_method(&unit.class.methods[0], &unit.exprs)?],
    })
}

impl Analysis {
    pub(crate) fn arena_identity(&self) -> (usize, usize) {
        self.arena_identity
    }
}

fn validate_class_shape(unit: &CompilationUnit) -> CompileResult<()> {
    let methods = &unit.class.methods;
    if methods.is_empty() {
        return Err(Diagnostic::unsupported_semantic(
            unit.class.name_span,
            "the supported class must declare main(String[])",
        ));
    }

    for method in methods {
        let mut names = FxHashSet::default();
        for param in &method.params {
            if !names.insert(param.name.text.as_str()) {
                return Err(Diagnostic::semantic(
                    param.name.span,
                    format!("duplicate parameter `{}`", param.name.text),
                ));
            }
        }
    }

    if methods.len() != 1 {
        return Err(Diagnostic::unsupported_semantic(
            methods[1].span,
            "the supported class must contain exactly one method",
        ));
    }

    let method = &methods[0];
    if method.name != "main" {
        return Err(Diagnostic::unsupported_semantic(
            method.name_span,
            "the supported method must be named `main`",
        ));
    }
    if !method.is_static {
        return Err(Diagnostic::unsupported_semantic(
            method.span,
            "the supported `main` method must be static",
        ));
    }
    if !method.return_type.is_void() {
        return Err(Diagnostic::unsupported_semantic(
            method.name_span,
            "the supported `main` method must return void",
        ));
    }
    if method.params.len() != 1 {
        let span = method.params.get(1).map_or(method.name_span, |param| param.span);
        return Err(Diagnostic::unsupported_semantic(
            span,
            "the supported `main` method must have one String[] parameter",
        ));
    }
    if !method.params[0].ty.is_string_array() {
        return Err(Diagnostic::unsupported_semantic(
            method.params[0].span,
            "the supported `main` parameter must have type String[]",
        ));
    }
    Ok(())
}

/// Unary numeric promotion: `byte`/`short`/`char` (and `boolean`) become `int`;
/// wider types are unchanged. Applied to the operand of a unary op and to the
/// left operand of a shift.
pub fn unary_promote(t: PrimitiveType) -> PrimitiveType {
    match t {
        PrimitiveType::Long => PrimitiveType::Long,
        PrimitiveType::Float => PrimitiveType::Float,
        PrimitiveType::Double => PrimitiveType::Double,
        PrimitiveType::Boolean => PrimitiveType::Boolean,
        _ => PrimitiveType::Int,
    }
}

/// Binary numeric promotion: the wider of the two operand types, with everything
/// narrower than `int` promoted to `int`.
pub fn binary_promote(a: PrimitiveType, b: PrimitiveType) -> PrimitiveType {
    use PrimitiveType::*;
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

/// The static type recorded during semantic validation.
pub fn type_of(expr: ExprId, info: &MethodInfo) -> &Type {
    info.expr_type(expr)
}
