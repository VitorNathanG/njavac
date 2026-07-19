mod attribution;

use crate::ast::{
    BranchBody, ExprArena, ExprId, ExprKind, Method, Name, PrimitiveType, Stmt, StmtKind, Type,
};
use crate::diagnostic::{CompileResult, Diagnostic};
use crate::fxhash::{FxHashMap, FxHashSet};
use crate::span::Span;

use super::{FrameLocal, LocalId, LocalInfo, MethodInfo, ResolvedCall, StmtFrameLocals};

pub(super) fn analyze_method(method: &Method, exprs: &ExprArena) -> CompileResult<MethodInfo> {
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
}
