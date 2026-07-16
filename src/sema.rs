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
    BinOp, BranchBody, CallArgs, CmpOp, CompilationUnit, ExprArena, ExprId, ExprKind, Method,
    Name, PrimitiveType, Stmt, StmtKind, Type,
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
        methods: vec![analyze_method(&unit.class.methods[0], &unit.exprs)?],
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

fn analyze_method(method: &Method, exprs: &ExprArena) -> CompileResult<MethodInfo> {
    let mut frame_locals = Vec::with_capacity(method.body.len() + 2);
    frame_locals.push(Vec::new());
    let mut analyzer = MethodAnalyzer {
        locals: Vec::new(),
        resolutions: FxHashMap::default(),
        stmt_frame_locals: FxHashMap::default(),
        scopes: vec![Scope { symbols: FxHashMap::default(), allocator_base: 0 }],
        assigned: FxHashSet::default(),
        frame_locals,
        expr_type_base: None,
        expr_types: Vec::new(),
        calls: Vec::new(),
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
        analyzer.validate_stmt(stmt, false, exprs)?;
    }

    Ok(MethodInfo {
        locals: analyzer.locals,
        resolutions: analyzer.resolutions,
        frame_locals: analyzer.frame_locals,
        entry_frame_locals,
        stmt_frame_locals: analyzer.stmt_frame_locals,
        expr_type_base: analyzer.expr_type_base,
        expr_types: analyzer.expr_types,
        calls: analyzer.calls,
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
    expr_type_base: Option<usize>,
    expr_types: Vec<Option<Type>>,
    calls: Vec<(ExprId, ResolvedCall)>,
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

    fn validate_stmt(
        &mut self,
        stmt: &Stmt,
        in_branch: bool,
        exprs: &ExprArena,
    ) -> CompileResult<()> {
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
                    let source = self.validate_expr(*init, stmt.span, exprs)?;
                    self.require_assignable(&target, &source, *init, stmt.span, exprs)?;
                    self.mark_assigned(id);
                }
            }
            StmtKind::Assign { name, value } => {
                let id = self.resolve(name)?;
                let target = self.local_type(id);
                let source = self.validate_expr(*value, stmt.span, exprs)?;
                self.require_assignable(&target, &source, *value, stmt.span, exprs)?;
                self.mark_assigned(id);
            }
            StmtKind::CompoundAssign { name, op, value } => {
                let (id, target) = self.read_local(name)?;
                let source = self.validate_expr(*value, stmt.span, exprs)?;
                self.require_compound(*op, &target, &source, stmt.span)?;
                self.mark_assigned(id);
            }
            StmtKind::Expr(expr) => {
                if !matches!(&exprs[*expr], ExprKind::Call { .. }) {
                    return Err(Diagnostic::semantic(
                        stmt.span,
                        "only a method invocation may be used as an expression statement",
                    ));
                }
                let ty = self.validate_expr(*expr, stmt.span, exprs)?;
                debug_assert!(ty.is_void(), "supported expression statement returns a value");
            }
            StmtKind::If { cond, then_branch, else_branch } => {
                let ty = self.validate_expr(*cond, stmt.span, exprs)?;
                if !ty.is_boolean() {
                    return Err(Diagnostic::semantic(stmt.span, "if condition must be boolean"));
                }

                let incoming = self.assigned.clone();
                let incoming_frame = self.current_frame_locals;
                self.validate_branch(then_branch, exprs)?;
                let then_assigned = self.assigned.clone();
                let then_frame = self.current_frame_locals;

                self.assigned = incoming;
                self.current_frame_locals = incoming_frame;
                if let Some(else_branch) = else_branch {
                    self.validate_branch(else_branch, exprs)?;
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

    fn validate_branch(&mut self, body: &BranchBody, exprs: &ExprArena) -> CompileResult<()> {
        if body.braced {
            self.enter_scope();
        }
        for stmt in &body.stmts {
            self.validate_stmt(stmt, true, exprs)?;
        }
        if body.braced {
            self.exit_scope();
        }
        Ok(())
    }

    fn validate_expr(
        &mut self,
        expr: ExprId,
        span: Span,
        exprs: &ExprArena,
    ) -> CompileResult<Type> {
        let mut resolved_call = None;
        let ty = match &exprs[expr] {
            ExprKind::IntLit(_) => PrimitiveType::Int.into(),
            ExprKind::LongLit(_) => PrimitiveType::Long.into(),
            ExprKind::FloatLit(_) => PrimitiveType::Float.into(),
            ExprKind::DoubleLit(_) => PrimitiveType::Double.into(),
            ExprKind::BoolLit(_) => PrimitiveType::Boolean.into(),
            ExprKind::CharLit(_) => PrimitiveType::Char.into(),
            ExprKind::StringLit(_) => Type::string(),
            ExprKind::Name(name) => {
                let (_, ty) = self.read_local(name)?;
                if ty.as_primitive().is_none() {
                    return Err(Diagnostic::unsupported_semantic(
                        name.span,
                        "using the String[] parameter as a value is unsupported",
                    ));
                }
                ty
            }
            ExprKind::Select { .. } => {
                return Err(Diagnostic::semantic(
                    span,
                    "a qualified name is only supported as a method-call target",
                ));
            }
            ExprKind::Neg(inner) => {
                let ty = self.validate_expr(*inner, span, exprs)?;
                self.require_numeric(&ty, span, "unary `-`")?;
                unary_promote(ty.primitive()).into()
            }
            ExprKind::BitNot(inner) => {
                let ty = self.validate_expr(*inner, span, exprs)?;
                self.require_integral(&ty, span, "unary `~`")?;
                unary_promote(ty.primitive()).into()
            }
            ExprKind::Not(inner) => {
                let ty = self.validate_expr(*inner, span, exprs)?;
                self.require_boolean(&ty, span, "unary `!`")?;
                PrimitiveType::Boolean.into()
            }
            ExprKind::Paren(inner) => self.validate_expr(*inner, span, exprs)?,
            ExprKind::Cast { ty, expr } => {
                let source = self.validate_expr(*expr, span, exprs)?;
                let target = ty.clone();
                if !((is_numeric(&source) && is_numeric(&target))
                    || (source.is_boolean() && target.is_boolean()))
                {
                    return Err(Diagnostic::semantic(span, "invalid primitive cast"));
                }
                target
            }
            ExprKind::Binary { op, left, right } => {
                let left_ty = self.validate_expr(*left, span, exprs)?;
                let right_ty = self.validate_expr(*right, span, exprs)?;
                self.validate_binary(*op, &left_ty, &right_ty, *right, span, exprs)?
            }
            ExprKind::Compare { op, left, right } => {
                let left_ty = self.validate_expr(*left, span, exprs)?;
                let right_ty = self.validate_expr(*right, span, exprs)?;
                self.validate_compare(*op, &left_ty, &right_ty, span)?;
                PrimitiveType::Boolean.into()
            }
            ExprKind::Logical { left, right, .. } => {
                let left_ty = self.validate_expr(*left, span, exprs)?;
                let right_ty = self.validate_expr(*right, span, exprs)?;
                self.require_boolean(&left_ty, span, "logical operator")?;
                self.require_boolean(&right_ty, span, "logical operator")?;
                PrimitiveType::Boolean.into()
            }
            ExprKind::Call { target, args } => {
                let call = self.resolve_call(*target, args, span, exprs)?;
                let return_type = call.return_type();
                resolved_call = Some(call);
                return_type
            }
        };
        let base = *self.expr_type_base.get_or_insert(expr.index());
        let index = expr
            .index()
            .checked_sub(base)
            .expect("expression validation order crossed method boundaries");
        if self.expr_types.len() <= index {
            self.expr_types.resize_with(index + 1, || None);
        }
        match &self.expr_types[index] {
            Some(previous) => assert_eq!(previous, &ty, "expression type changed between visits"),
            None => self.expr_types[index] = Some(ty.clone()),
        }
        if let Some(call) = resolved_call {
            if let Some((_, previous)) = self.calls.iter().find(|(id, _)| *id == expr) {
                assert_eq!(previous, &call, "call resolution changed between visits");
            } else {
                self.calls.push((expr, call));
            }
        }
        Ok(ty)
    }

    /// Resolve the current subset's library call after parsing has preserved it as
    /// an ordinary dotted invocation. The selected parameter type records overload
    /// resolution (`byte`/`short` use `println(int)`).
    fn resolve_call(
        &mut self,
        target: ExprId,
        args: &CallArgs,
        span: Span,
        exprs: &ExprArena,
    ) -> CompileResult<ResolvedCall> {
        if !qualified_name_is(exprs, target, &["System", "out", "println"]) {
            return Err(Diagnostic::unsupported_semantic(
                qualified_name_span(exprs, target).unwrap_or(span),
                "only System.out.println(...) calls are supported",
            ));
        }
        if args.len() != 1 {
            return Err(Diagnostic::unsupported_semantic(
                span,
                "System.out.println requires exactly one argument in the supported subset",
            ));
        }

        let argument_expr = args.first.expect("one call argument checked above");
        let argument = self.validate_expr(argument_expr, span, exprs)?;
        if argument.is_string() && !is_string_value(exprs, argument_expr) {
            return Err(Diagnostic::unsupported_semantic(
                span,
                "only string literals are supported as String values",
            ));
        }
        let parameter = match argument.as_primitive() {
            Some(PrimitiveType::Byte | PrimitiveType::Short) => PrimitiveType::Int.into(),
            Some(_) => argument,
            None if argument.is_string() => argument,
            None => {
                return Err(Diagnostic::unsupported_semantic(
                    span,
                    "unsupported println argument type",
                ));
            }
        };
        Ok(ResolvedCall::Println { parameter_type: parameter })
    }

    fn validate_binary(
        &self,
        op: BinOp,
        left: &Type,
        right: &Type,
        right_expr: ExprId,
        span: Span,
        exprs: &ExprArena,
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
            && eval_numeric_constant(exprs, right_expr).is_some_and(NumericConst::is_zero)
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
        expr: ExprId,
        span: Span,
        exprs: &ExprArena,
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
                && is_constant_expression(exprs, expr))
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
        Type::Void => panic!("void used as a verifier local"),
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
fn is_constant_expression(exprs: &ExprArena, expr: ExprId) -> bool {
    match &exprs[expr] {
        ExprKind::IntLit(_)
        | ExprKind::LongLit(_)
        | ExprKind::FloatLit(_)
        | ExprKind::DoubleLit(_)
        | ExprKind::BoolLit(_)
        | ExprKind::CharLit(_)
        | ExprKind::StringLit(_) => true,
        ExprKind::Neg(inner)
        | ExprKind::BitNot(inner)
        | ExprKind::Not(inner)
        | ExprKind::Paren(inner) => {
            is_constant_expression(exprs, *inner)
        }
        ExprKind::Cast { expr, .. } => is_constant_expression(exprs, *expr),
        ExprKind::Binary { left, right, .. }
        | ExprKind::Compare { left, right, .. }
        | ExprKind::Logical { left, right, .. } => {
            is_constant_expression(exprs, *left) && is_constant_expression(exprs, *right)
        }
        ExprKind::Name(_) | ExprKind::Select { .. } | ExprKind::Call { .. } => false,
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

fn eval_numeric_constant(exprs: &ExprArena, expr: ExprId) -> Option<NumericConst> {
    Some(match &exprs[expr] {
        ExprKind::IntLit(value) => NumericConst::Int(*value),
        ExprKind::LongLit(value) => NumericConst::Long(*value),
        ExprKind::FloatLit(value) => NumericConst::Float(*value),
        ExprKind::DoubleLit(value) => NumericConst::Double(*value),
        ExprKind::CharLit(value) => NumericConst::Int(*value as i32),
        ExprKind::Neg(inner) => match eval_numeric_constant(exprs, *inner)? {
            NumericConst::Int(value) => NumericConst::Int(value.wrapping_neg()),
            NumericConst::Long(value) => NumericConst::Long(value.wrapping_neg()),
            NumericConst::Float(value) => NumericConst::Float(-value),
            NumericConst::Double(value) => NumericConst::Double(-value),
        },
        ExprKind::BitNot(inner) => match eval_numeric_constant(exprs, *inner)? {
            NumericConst::Int(value) => NumericConst::Int(!value),
            NumericConst::Long(value) => NumericConst::Long(!value),
            NumericConst::Float(_) | NumericConst::Double(_) => return None,
        },
        ExprKind::Paren(inner) => eval_numeric_constant(exprs, *inner)?,
        ExprKind::Cast { ty, expr } => {
            eval_numeric_constant(exprs, *expr)?.cast(ty.primitive())?
        }
        ExprKind::Binary { op, left, right } => {
            let left = eval_numeric_constant(exprs, *left)?;
            let right = eval_numeric_constant(exprs, *right)?;
            eval_numeric_binary(*op, left, right)?
        }
        ExprKind::BoolLit(_)
        | ExprKind::StringLit(_)
        | ExprKind::Name(_)
        | ExprKind::Select { .. }
        | ExprKind::Not(_)
        | ExprKind::Compare { .. }
        | ExprKind::Logical { .. }
        | ExprKind::Call { .. } => return None,
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

fn is_string_value(exprs: &ExprArena, expr: ExprId) -> bool {
    match &exprs[expr] {
        ExprKind::StringLit(_) => true,
        ExprKind::Paren(inner) => is_string_value(exprs, *inner),
        _ => false,
    }
}

fn qualified_name_is(exprs: &ExprArena, expr: ExprId, parts: &[&str]) -> bool {
    match (&exprs[expr], parts) {
        (ExprKind::Name(name), [part]) => name.text == *part,
        (ExprKind::Select { qualifier, name }, [prefix @ .., part]) => {
            name.text == *part && qualified_name_is(exprs, *qualifier, prefix)
        }
        _ => false,
    }
}

fn qualified_name_span(exprs: &ExprArena, expr: ExprId) -> Option<Span> {
    match &exprs[expr] {
        ExprKind::Name(name) | ExprKind::Select { name, .. } => Some(name.span),
        _ => None,
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

/// The static type recorded during semantic validation.
pub fn type_of(expr: ExprId, info: &MethodInfo) -> &Type {
    info.expr_type(expr)
}
