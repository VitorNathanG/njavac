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

use crate::ast::{
    BinOp, BranchBody, CmpOp, CompilationUnit, Expr, Method, Name, PrimitiveType, Stmt,
    StmtKind, Type,
};
use crate::diagnostic::{CompileResult, Diagnostic};
use crate::fxhash::{FxHashMap, FxHashSet};
use crate::span::Span;

impl PrimitiveType {
    /// The JVM *computational* type this value occupies on the operand stack. The
    /// sub-int types (`boolean`/`char`/`byte`/`short`) are all `Int` on the stack.
    pub fn stack(self) -> StackTy {
        match self {
            PrimitiveType::Long => StackTy::Long,
            PrimitiveType::Float => StackTy::Float,
            PrimitiveType::Double => StackTy::Double,
            _ => StackTy::Int,
        }
    }
}

/// The four JVM operand-stack computational types produced by primitive values.
/// References stay in `Type` and never enter numeric opcode selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackTy {
    Int,
    Long,
    Float,
    Double,
}

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
    /// High-water local-slot count, counting `long`/`double` as two.
    pub max_locals: u16,
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
    pub methods: Vec<MethodInfo>,
}

/// Analyze a parsed compilation unit, assigning local slots for each method.
pub fn analyze(unit: &CompilationUnit) -> CompileResult<Analysis> {
    validate_class_shape(unit)?;
    Ok(Analysis { methods: vec![analyze_method(&unit.class.methods[0])?] })
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

fn analyze_method(method: &Method) -> CompileResult<MethodInfo> {
    let mut frame_locals = Vec::with_capacity(method.body.len() + 2);
    frame_locals.push(Vec::new());
    let mut analyzer = MethodAnalyzer {
        locals: Vec::new(),
        resolutions: FxHashMap::default(),
        stmt_frame_locals: FxHashMap::default(),
        scopes: vec![Scope { symbols: FxHashMap::default(), allocator_base: 0 }],
        assigned: FxHashSet::default(),
        frame_locals,
        current_frame_locals: 0,
        next_slot: 0,
        max_locals: 0,
    };

    // Parameters take the low slots and are definitely assigned at method entry.
    for param in &method.params {
        let id = analyzer.declare(&param.name, param.ty.clone())?;
        analyzer.assigned.insert(id);
    }
    analyzer.refresh_frame_locals();
    let entry_frame_locals = analyzer.current_frame_locals;
    for stmt in &method.body {
        analyzer.validate_stmt(stmt, false)?;
    }

    Ok(MethodInfo {
        locals: analyzer.locals,
        resolutions: analyzer.resolutions,
        frame_locals: analyzer.frame_locals,
        entry_frame_locals,
        stmt_frame_locals: analyzer.stmt_frame_locals,
        max_locals: analyzer.max_locals,
    })
}

struct Scope {
    symbols: FxHashMap<String, LocalId>,
    allocator_base: u16,
}

struct MethodAnalyzer {
    locals: Vec<LocalInfo>,
    resolutions: FxHashMap<Span, LocalId>,
    stmt_frame_locals: FxHashMap<Span, StmtFrameLocals>,
    scopes: Vec<Scope>,
    assigned: FxHashSet<LocalId>,
    /// Arena of immutable snapshots, extended only when `assigned` changes.
    frame_locals: Vec<Vec<FrameLocal>>,
    current_frame_locals: usize,
    next_slot: u16,
    max_locals: u16,
}

impl MethodAnalyzer {
    fn enter_scope(&mut self) {
        self.scopes.push(Scope {
            symbols: FxHashMap::default(),
            allocator_base: self.next_slot,
        });
    }

    fn exit_scope(&mut self) {
        let scope = self.scopes.pop().expect("scope stack underflow");
        assert!(!self.scopes.is_empty(), "cannot exit the method scope");
        let mut assignment_changed = false;
        for id in scope.symbols.values() {
            assignment_changed |= self.assigned.remove(id);
        }
        self.next_slot = scope.allocator_base;
        if assignment_changed {
            self.refresh_frame_locals();
        }
    }

    fn declare(&mut self, name: &Name, ty: Type) -> CompileResult<LocalId> {
        if self.scopes.iter().any(|scope| scope.symbols.contains_key(&name.text)) {
            return Err(Diagnostic::semantic(
                name.span,
                format!("duplicate local `{}`", name.text),
            ));
        }
        let next = self.next_slot.checked_add(ty.width()).ok_or_else(|| {
            Diagnostic::unsupported_semantic(name.span, "method requires too many local slots")
        })?;
        let id = LocalId(self.locals.len());
        self.locals.push(LocalInfo {
            name: name.text.clone(),
            declaration: name.span,
            ty,
            slot: self.next_slot,
        });
        self.scopes
            .last_mut()
            .expect("method scope is missing")
            .symbols
            .insert(name.text.clone(), id);
        self.record_resolution(name, id);
        self.next_slot = next;
        self.max_locals = self.max_locals.max(next);
        Ok(id)
    }

    fn resolve(&mut self, name: &Name) -> CompileResult<LocalId> {
        let id = self
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.symbols.get(&name.text).copied())
            .ok_or_else(|| {
                Diagnostic::semantic(name.span, format!("undeclared local `{}`", name.text))
            })?;
        self.record_resolution(name, id);
        Ok(id)
    }

    fn record_resolution(&mut self, name: &Name, id: LocalId) {
        let previous = self.resolutions.insert(name.span, id);
        assert!(previous.is_none(), "name occurrence resolved more than once");
    }

    fn local_type(&self, id: LocalId) -> Type {
        self.locals[id.0].ty.clone()
    }

    /// Build verifier locals from definite assignment and physical slot positions.
    /// Unassigned interior slots become `Top`; trailing `Top` entries are omitted.
    /// A category-2 local advances two slots but contributes one verifier entry.
    fn build_frame_locals(&self) -> Vec<FrameLocal> {
        let last_assigned_slot = self
            .assigned
            .iter()
            .map(|id| {
                let local = &self.locals[id.0];
                local.slot + local.ty.width()
            })
            .max()
            .unwrap_or(0);
        let mut starts: Vec<Option<FrameLocal>> = vec![None; last_assigned_slot as usize];
        for id in &self.assigned {
            let local = &self.locals[id.0];
            let previous = starts[local.slot as usize].replace(frame_local(&local.ty));
            assert!(previous.is_none(), "assigned locals overlap at slot {}", local.slot);
        }

        let mut locals = Vec::new();
        let mut slot = 0usize;
        while slot < starts.len() {
            match starts[slot].take() {
                Some(frame @ (FrameLocal::Long | FrameLocal::Double)) => {
                    locals.push(frame);
                    slot += 2;
                }
                Some(frame) => {
                    locals.push(frame);
                    slot += 1;
                }
                None => {
                    locals.push(FrameLocal::Top);
                    slot += 1;
                }
            }
        }
        while locals.last() == Some(&FrameLocal::Top) {
            locals.pop();
        }
        locals
    }

    fn refresh_frame_locals(&mut self) {
        self.frame_locals.push(self.build_frame_locals());
        self.current_frame_locals = self.frame_locals.len() - 1;
    }

    fn mark_assigned(&mut self, id: LocalId) {
        if self.assigned.insert(id) {
            self.refresh_frame_locals();
        }
    }

    fn read_local(&mut self, name: &Name) -> CompileResult<(LocalId, Type)> {
        let id = self.resolve(name)?;
        if !self.assigned.contains(&id) {
            return Err(Diagnostic::semantic(
                name.span,
                format!("local `{}` might not have been initialized", name.text),
            ));
        }
        Ok((id, self.local_type(id)))
    }

    fn validate_stmt(&mut self, stmt: &Stmt, in_branch: bool) -> CompileResult<()> {
        let entry = self.current_frame_locals;
        match &stmt.kind {
            StmtKind::LocalDecl { ty, name, init } => {
                if in_branch {
                    return Err(Diagnostic::unsupported_semantic(
                        stmt.span,
                        "local declarations inside branches are unsupported",
                    ));
                }
                let target = ty.clone();
                let id = self.declare(name, target.clone())?;
                if let Some(init) = init {
                    let source = self.validate_expr(init, stmt.span)?;
                    self.require_assignable(&target, &source, init, stmt.span)?;
                    self.mark_assigned(id);
                }
            }
            StmtKind::Assign { name, value } => {
                let id = self.resolve(name)?;
                let target = self.local_type(id);
                let source = self.validate_expr(value, stmt.span)?;
                self.require_assignable(&target, &source, value, stmt.span)?;
                self.mark_assigned(id);
            }
            StmtKind::CompoundAssign { name, op, value } => {
                let (id, target) = self.read_local(name)?;
                let source = self.validate_expr(value, stmt.span)?;
                self.require_compound(*op, &target, &source, stmt.span)?;
                self.mark_assigned(id);
            }
            StmtKind::Expr(expr) => match expr {
                Expr::Println(arg) => {
                    let ty = self.validate_expr(arg, stmt.span)?;
                    if ty.is_string() && !is_string_value(arg) {
                        return Err(Diagnostic::unsupported_semantic(
                            stmt.span,
                            "only string literals are supported as String values",
                        ));
                    }
                }
                _ => {
                    return Err(Diagnostic::semantic(
                        stmt.span,
                        "only a method invocation may be used as an expression statement",
                    ));
                }
            },
            StmtKind::If { cond, then_branch, else_branch } => {
                let ty = self.validate_expr(cond, stmt.span)?;
                if !ty.is_boolean() {
                    return Err(Diagnostic::semantic(stmt.span, "if condition must be boolean"));
                }

                let incoming = self.assigned.clone();
                let incoming_frame = self.current_frame_locals;
                self.validate_branch(then_branch)?;
                let then_assigned = self.assigned.clone();
                let then_frame = self.current_frame_locals;

                self.assigned = incoming;
                self.current_frame_locals = incoming_frame;
                if let Some(else_branch) = else_branch {
                    self.validate_branch(else_branch)?;
                }
                let else_assigned = self.assigned.clone();
                let else_frame = self.current_frame_locals;
                self.assigned = then_assigned.intersection(&else_assigned).cloned().collect();
                if self.assigned == then_assigned {
                    self.current_frame_locals = then_frame;
                } else if self.assigned == else_assigned {
                    self.current_frame_locals = else_frame;
                } else {
                    self.refresh_frame_locals();
                }
            }
        }
        let previous = self.stmt_frame_locals.insert(
            stmt.span,
            StmtFrameLocals { entry, exit: self.current_frame_locals },
        );
        assert!(previous.is_none(), "duplicate statement span recorded at {:?}", stmt.span);
        Ok(())
    }

    fn validate_branch(&mut self, body: &BranchBody) -> CompileResult<()> {
        if body.braced {
            self.enter_scope();
        }
        for stmt in &body.stmts {
            self.validate_stmt(stmt, true)?;
        }
        if body.braced {
            self.exit_scope();
        }
        Ok(())
    }

    fn validate_expr(&mut self, expr: &Expr, span: Span) -> CompileResult<Type> {
        let ty = match expr {
            Expr::IntLit(_) => PrimitiveType::Int.into(),
            Expr::LongLit(_) => PrimitiveType::Long.into(),
            Expr::FloatLit(_) => PrimitiveType::Float.into(),
            Expr::DoubleLit(_) => PrimitiveType::Double.into(),
            Expr::BoolLit(_) => PrimitiveType::Boolean.into(),
            Expr::CharLit(_) => PrimitiveType::Char.into(),
            Expr::StringLit(_) => Type::string(),
            Expr::Name(name) => {
                let (_, ty) = self.read_local(name)?;
                if ty.as_primitive().is_none() {
                    return Err(Diagnostic::unsupported_semantic(
                        name.span,
                        "using the String[] parameter as a value is unsupported",
                    ));
                }
                ty
            }
            Expr::Neg(inner) => {
                let ty = self.validate_expr(inner, span)?;
                self.require_numeric(&ty, span, "unary `-`")?;
                unary_promote(ty.primitive()).into()
            }
            Expr::BitNot(inner) => {
                let ty = self.validate_expr(inner, span)?;
                self.require_integral(&ty, span, "unary `~`")?;
                unary_promote(ty.primitive()).into()
            }
            Expr::Not(inner) => {
                let ty = self.validate_expr(inner, span)?;
                self.require_boolean(&ty, span, "unary `!`")?;
                PrimitiveType::Boolean.into()
            }
            Expr::Paren(inner) => self.validate_expr(inner, span)?,
            Expr::Cast { ty, expr } => {
                let source = self.validate_expr(expr, span)?;
                let target = ty.clone();
                if !((is_numeric(&source) && is_numeric(&target))
                    || (source.is_boolean() && target.is_boolean()))
                {
                    return Err(Diagnostic::semantic(span, "invalid primitive cast"));
                }
                target
            }
            Expr::Binary { op, left, right } => {
                let left_ty = self.validate_expr(left, span)?;
                let right_ty = self.validate_expr(right, span)?;
                self.validate_binary(*op, &left_ty, &right_ty, right, span)?
            }
            Expr::Compare { op, left, right } => {
                let left_ty = self.validate_expr(left, span)?;
                let right_ty = self.validate_expr(right, span)?;
                self.validate_compare(*op, &left_ty, &right_ty, span)?;
                PrimitiveType::Boolean.into()
            }
            Expr::Logical { left, right, .. } => {
                let left_ty = self.validate_expr(left, span)?;
                let right_ty = self.validate_expr(right, span)?;
                self.require_boolean(&left_ty, span, "logical operator")?;
                self.require_boolean(&right_ty, span, "logical operator")?;
                PrimitiveType::Boolean.into()
            }
            Expr::Println(_) => {
                return Err(Diagnostic::semantic(
                    span,
                    "System.out.println does not produce a value",
                ));
            }
        };
        Ok(ty)
    }

    fn validate_binary(
        &self,
        op: BinOp,
        left: &Type,
        right: &Type,
        right_expr: &Expr,
        span: Span,
    ) -> CompileResult<Type> {
        if op == BinOp::Add && (left.is_string() || right.is_string()) {
            return Err(Diagnostic::unsupported_semantic(
                span,
                "String concatenation is unsupported",
            ));
        }
        if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor)
            && left.is_boolean()
            && right.is_boolean()
        {
            return Ok(PrimitiveType::Boolean.into());
        }
        if op.is_shift() {
            self.require_integral(left, span, "shift operator")?;
            self.require_integral(right, span, "shift operator")?;
            return Ok(unary_promote(left.primitive()).into());
        }
        if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor) {
            self.require_integral(left, span, "bitwise operator")?;
            self.require_integral(right, span, "bitwise operator")?;
        } else {
            self.require_numeric(left, span, "arithmetic operator")?;
            self.require_numeric(right, span, "arithmetic operator")?;
        }
        let result = binary_promote(left.primitive(), right.primitive());
        if matches!(op, BinOp::Div | BinOp::Rem)
            && result.is_integral()
            && eval_numeric_constant(right_expr).is_some_and(NumericConst::is_zero)
        {
            return Err(Diagnostic::semantic(
                span,
                "integral division or remainder by zero",
            ));
        }
        Ok(result.into())
    }

    fn validate_compare(
        &self,
        op: CmpOp,
        left: &Type,
        right: &Type,
        span: Span,
    ) -> CompileResult<()> {
        if matches!(op, CmpOp::Eq | CmpOp::Ne) {
            if left.is_string() && right.is_string() {
                return Err(Diagnostic::unsupported_semantic(
                    span,
                    "reference comparison is unsupported",
                ));
            }
            if (is_numeric(left) && is_numeric(right))
                || (left.is_boolean() && right.is_boolean())
            {
                return Ok(());
            }
            return Err(Diagnostic::semantic(span, "invalid equality operands"));
        }
        self.require_numeric(left, span, "relational operator")?;
        self.require_numeric(right, span, "relational operator")
    }

    fn require_assignable(
        &self,
        target: &Type,
        source: &Type,
        expr: &Expr,
        span: Span,
    ) -> CompileResult<()> {
        if target.as_primitive().is_none() {
            return Err(Diagnostic::semantic(
                span,
                "cannot assign a value to the String[] parameter",
            ));
        }
        if is_assignment_convertible(target, source)
            || (is_integral(target)
                && matches!(
                    source.as_primitive(),
                    Some(
                        PrimitiveType::Int
                            | PrimitiveType::Byte
                            | PrimitiveType::Short
                            | PrimitiveType::Char
                    )
                )
                && is_constant_expression(expr))
        {
            return Ok(());
        }
        Err(Diagnostic::semantic(
            span,
            format!("cannot assign {source:?} to {target:?}"),
        ))
    }

    fn require_compound(
        &self,
        op: BinOp,
        target: &Type,
        source: &Type,
        span: Span,
    ) -> CompileResult<()> {
        let valid = if op.is_shift() {
            is_integral(target) && is_integral(source)
        } else if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor) {
            (is_integral(target) && is_integral(source))
                || (target.is_boolean() && source.is_boolean())
        } else {
            is_numeric(target) && is_numeric(source)
        };
        if valid {
            Ok(())
        } else {
            Err(Diagnostic::semantic(span, "invalid compound assignment operands"))
        }
    }

    fn require_numeric(&self, ty: &Type, span: Span, context: &str) -> CompileResult<()> {
        if is_numeric(ty) {
            Ok(())
        } else {
            Err(Diagnostic::semantic(span, format!("{context} requires numeric operands")))
        }
    }

    fn require_integral(&self, ty: &Type, span: Span, context: &str) -> CompileResult<()> {
        if is_integral(ty) {
            Ok(())
        } else {
            Err(Diagnostic::semantic(span, format!("{context} requires integral operands")))
        }
    }

    fn require_boolean(&self, ty: &Type, span: Span, context: &str) -> CompileResult<()> {
        if ty.is_boolean() {
            Ok(())
        } else {
            Err(Diagnostic::semantic(span, format!("{context} requires boolean operands")))
        }
    }
}

fn frame_local(ty: &Type) -> FrameLocal {
    match ty {
        Type::Primitive(PrimitiveType::Long) => FrameLocal::Long,
        Type::Primitive(PrimitiveType::Float) => FrameLocal::Float,
        Type::Primitive(PrimitiveType::Double) => FrameLocal::Double,
        Type::Primitive(_) => FrameLocal::Integer,
        Type::Class(_) | Type::Array(_) => {
            FrameLocal::Object(ty.verifier_name().expect("reference type has no verifier name"))
        }
    }
}

fn is_numeric(ty: &Type) -> bool {
    ty.as_primitive().is_some_and(PrimitiveType::is_numeric)
}

fn is_integral(ty: &Type) -> bool {
    ty.as_primitive().is_some_and(PrimitiveType::is_integral)
}

fn is_assignment_convertible(target: &Type, source: &Type) -> bool {
    use PrimitiveType::*;
    let (Some(target), Some(source)) = (target.as_primitive(), source.as_primitive()) else {
        return target == source;
    };
    target == source
        || matches!(
            (source, target),
            (Byte, Short | Int | Long | Float | Double)
                | (Short, Int | Long | Float | Double)
                | (Char, Int | Long | Float | Double)
                | (Int, Long | Float | Double)
                | (Long, Float | Double)
                | (Float, Double)
        )
}

/// A syntax-only approximation used for assignment conversion. Range checking is
/// deliberately left to a later constant-analysis stage, so existing valid folded
/// initializers are not rejected here.
fn is_constant_expression(expr: &Expr) -> bool {
    match expr {
        Expr::IntLit(_)
        | Expr::LongLit(_)
        | Expr::FloatLit(_)
        | Expr::DoubleLit(_)
        | Expr::BoolLit(_)
        | Expr::CharLit(_)
        | Expr::StringLit(_) => true,
        Expr::Neg(inner) | Expr::BitNot(inner) | Expr::Not(inner) | Expr::Paren(inner) => {
            is_constant_expression(inner)
        }
        Expr::Cast { expr, .. } => is_constant_expression(expr),
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            is_constant_expression(left) && is_constant_expression(right)
        }
        Expr::Name(_) | Expr::Println(_) => false,
    }
}

/// Numeric constant evaluation needed only to identify an integral zero divisor.
/// It mirrors the folding arithmetic that can reach codegen's integer `/` and `%`.
#[derive(Clone, Copy)]
enum NumericConst {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
}

impl NumericConst {
    fn is_zero(self) -> bool {
        matches!(self, Self::Int(0) | Self::Long(0))
    }

    fn rank(self) -> u8 {
        match self {
            Self::Int(_) => 0,
            Self::Long(_) => 1,
            Self::Float(_) => 2,
            Self::Double(_) => 3,
        }
    }

    fn to_i32(self) -> i32 {
        match self {
            Self::Int(value) => value,
            Self::Long(value) => value as i32,
            Self::Float(value) => value as i32,
            Self::Double(value) => value as i32,
        }
    }

    fn to_i64(self) -> i64 {
        match self {
            Self::Int(value) => value as i64,
            Self::Long(value) => value,
            Self::Float(value) => value as i64,
            Self::Double(value) => value as i64,
        }
    }

    fn to_f32(self) -> f32 {
        match self {
            Self::Int(value) => value as f32,
            Self::Long(value) => value as f32,
            Self::Float(value) => value,
            Self::Double(value) => value as f32,
        }
    }

    fn to_f64(self) -> f64 {
        match self {
            Self::Int(value) => value as f64,
            Self::Long(value) => value as f64,
            Self::Float(value) => value as f64,
            Self::Double(value) => value,
        }
    }

    fn cast(self, ty: PrimitiveType) -> Option<Self> {
        Some(match ty {
            PrimitiveType::Int => Self::Int(self.to_i32()),
            PrimitiveType::Long => Self::Long(self.to_i64()),
            PrimitiveType::Float => Self::Float(self.to_f32()),
            PrimitiveType::Double => Self::Double(self.to_f64()),
            PrimitiveType::Byte => Self::Int((self.to_i32() as i8) as i32),
            PrimitiveType::Short => Self::Int((self.to_i32() as i16) as i32),
            PrimitiveType::Char => Self::Int((self.to_i32() as u16) as i32),
            PrimitiveType::Boolean => return None,
        })
    }
}

fn eval_numeric_constant(expr: &Expr) -> Option<NumericConst> {
    Some(match expr {
        Expr::IntLit(value) => NumericConst::Int(*value),
        Expr::LongLit(value) => NumericConst::Long(*value),
        Expr::FloatLit(value) => NumericConst::Float(*value),
        Expr::DoubleLit(value) => NumericConst::Double(*value),
        Expr::CharLit(value) => NumericConst::Int(*value as i32),
        Expr::Neg(inner) => match eval_numeric_constant(inner)? {
            NumericConst::Int(value) => NumericConst::Int(value.wrapping_neg()),
            NumericConst::Long(value) => NumericConst::Long(value.wrapping_neg()),
            NumericConst::Float(value) => NumericConst::Float(-value),
            NumericConst::Double(value) => NumericConst::Double(-value),
        },
        Expr::BitNot(inner) => match eval_numeric_constant(inner)? {
            NumericConst::Int(value) => NumericConst::Int(!value),
            NumericConst::Long(value) => NumericConst::Long(!value),
            NumericConst::Float(_) | NumericConst::Double(_) => return None,
        },
        Expr::Paren(inner) => eval_numeric_constant(inner)?,
        Expr::Cast { ty, expr } => eval_numeric_constant(expr)?.cast(ty.primitive())?,
        Expr::Binary { op, left, right } => {
            let left = eval_numeric_constant(left)?;
            let right = eval_numeric_constant(right)?;
            eval_numeric_binary(*op, left, right)?
        }
        Expr::BoolLit(_)
        | Expr::StringLit(_)
        | Expr::Name(_)
        | Expr::Not(_)
        | Expr::Compare { .. }
        | Expr::Logical { .. }
        | Expr::Println(_) => return None,
    })
}

fn eval_numeric_binary(op: BinOp, left: NumericConst, right: NumericConst) -> Option<NumericConst> {
    if op.is_shift() {
        // Codegen deliberately leaves this javac quirk unfolded, so it cannot
        // expose an integer folding panic in an enclosing division either.
        if op == BinOp::UShr
            && matches!(left, NumericConst::Long(_))
            && matches!(right, NumericConst::Long(_))
        {
            return None;
        }
        return Some(match left {
            NumericConst::Long(value) => {
                let distance = (right.to_i32() & 63) as u32;
                NumericConst::Long(match op {
                    BinOp::Shl => value.wrapping_shl(distance),
                    BinOp::Shr => value.wrapping_shr(distance),
                    BinOp::UShr => ((value as u64).wrapping_shr(distance)) as i64,
                    _ => unreachable!(),
                })
            }
            NumericConst::Int(value) => {
                let distance = (right.to_i32() & 31) as u32;
                NumericConst::Int(match op {
                    BinOp::Shl => value.wrapping_shl(distance),
                    BinOp::Shr => value.wrapping_shr(distance),
                    BinOp::UShr => ((value as u32).wrapping_shr(distance)) as i32,
                    _ => unreachable!(),
                })
            }
            NumericConst::Float(_) | NumericConst::Double(_) => return None,
        });
    }

    Some(match left.rank().max(right.rank()) {
        0 => {
            let (left, right) = (left.to_i32(), right.to_i32());
            NumericConst::Int(match op {
                BinOp::Add => left.wrapping_add(right),
                BinOp::Sub => left.wrapping_sub(right),
                BinOp::Mul => left.wrapping_mul(right),
                BinOp::Div if right != 0 => left.wrapping_div(right),
                BinOp::Rem if right != 0 => left.wrapping_rem(right),
                BinOp::And => left & right,
                BinOp::Or => left | right,
                BinOp::Xor => left ^ right,
                _ => return None,
            })
        }
        1 => {
            let (left, right) = (left.to_i64(), right.to_i64());
            NumericConst::Long(match op {
                BinOp::Add => left.wrapping_add(right),
                BinOp::Sub => left.wrapping_sub(right),
                BinOp::Mul => left.wrapping_mul(right),
                BinOp::Div if right != 0 => left.wrapping_div(right),
                BinOp::Rem if right != 0 => left.wrapping_rem(right),
                BinOp::And => left & right,
                BinOp::Or => left | right,
                BinOp::Xor => left ^ right,
                _ => return None,
            })
        }
        2 => {
            let (left, right) = (left.to_f32(), right.to_f32());
            NumericConst::Float(match op {
                BinOp::Add => left + right,
                BinOp::Sub => left - right,
                BinOp::Mul => left * right,
                BinOp::Div => left / right,
                BinOp::Rem => left % right,
                _ => return None,
            })
        }
        _ => {
            let (left, right) = (left.to_f64(), right.to_f64());
            NumericConst::Double(match op {
                BinOp::Add => left + right,
                BinOp::Sub => left - right,
                BinOp::Mul => left * right,
                BinOp::Div => left / right,
                BinOp::Rem => left % right,
                _ => return None,
            })
        }
    })
}

fn is_string_value(expr: &Expr) -> bool {
    match expr {
        Expr::StringLit(_) => true,
        Expr::Paren(inner) => is_string_value(inner),
        _ => false,
    }
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

/// The static type of an expression, implementing promotion. `Println` is a
/// `void` call and never appears as a value operand.
pub fn type_of(expr: &Expr, info: &MethodInfo) -> Type {
    match expr {
        Expr::IntLit(_) => PrimitiveType::Int.into(),
        Expr::LongLit(_) => PrimitiveType::Long.into(),
        Expr::FloatLit(_) => PrimitiveType::Float.into(),
        Expr::DoubleLit(_) => PrimitiveType::Double.into(),
        Expr::BoolLit(_) => PrimitiveType::Boolean.into(),
        Expr::CharLit(_) => PrimitiveType::Char.into(),
        Expr::StringLit(_) => Type::string(),
        Expr::Name(n) => info.declared_type(n).clone(),
        Expr::Neg(e) => unary_promote(type_of(e, info).primitive()).into(),
        Expr::BitNot(e) => unary_promote(type_of(e, info).primitive()).into(),
        Expr::Not(_) => PrimitiveType::Boolean.into(),
        Expr::Paren(e) => type_of(e, info),
        Expr::Compare { .. } => PrimitiveType::Boolean.into(),
        Expr::Logical { .. } => PrimitiveType::Boolean.into(),
        Expr::Cast { ty, .. } => ty.clone(),
        Expr::Binary { op, left, right } => {
            let lt = type_of(left, info);
            let rt = type_of(right, info);
            // `&`/`|`/`^` on two booleans is boolean (non-short-circuit logical).
            if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor)
                && lt.is_boolean()
                && rt.is_boolean()
            {
                PrimitiveType::Boolean.into()
            } else if op.is_shift() {
                unary_promote(lt.primitive()).into()
            } else {
                binary_promote(lt.primitive(), rt.primitive()).into()
            }
        }
        Expr::Println(_) => unreachable!("println does not have a value type"),
    }
}
