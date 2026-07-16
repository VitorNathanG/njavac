use crate::ast::{
    BinOp, CmpOp, ExprArena, ExprId, ExprKind, LogOp, Method, Name, PrimitiveType, Stmt, StmtKind,
    Type,
};
use crate::classfile::{
    CodeAttribute, ConstantPool, Method as CfMethod, VerificationType,
};
use crate::sema::{self, FrameLocal, MethodInfo, ResolvedCall, StackTy};
use crate::span::Span;

use super::assembler::Emitter;
use super::condition::*;
use super::constant::*;
use super::instruction::*;
use super::ops::*;

/// The implicit default constructor: `aload_0; invokespecial super.<init>; return`.
pub(super) fn gen_init(cp: &mut ConstantPool, super_class: &str, class_line: u16) -> CfMethod {
    let mut emitter = Emitter::new();
    emitter.set_pending_line(Some(class_line));
    emitter.emit(Instruction::Simple(ALOAD_0));
    let init_ref = cp.methodref(super_class, "<init>", "()V");
    emitter.emit(Instruction::Invoke {
        opcode: INVOKESPECIAL,
        index: init_ref,
        argument_words: 0,
        return_words: 0,
    });
    emitter.emit(Instruction::Simple(RETURN));
    let assembled = emitter.finish();

    CfMethod::with_code(
        0x0001, // ACC_PUBLIC
        "<init>",
        "()V",
        CodeAttribute::new(
            assembled.max_stack,
            1,
            assembled.code,
            assembled.line_numbers,
            Vec::new(),
            assembled.stack_frames,
        ),
    )
}

/// Emit one method body.
pub(super) fn gen_method(
    cp: &mut ConstantPool,
    method: &Method,
    info: &MethodInfo,
    exprs: &ExprArena,
) -> CfMethod {
    let entry_locals = verification_locals(info.entry_frame_locals());

    let mut g = Gen {
        cp,
        info,
        exprs,
        emitter: Emitter::new(),
        semantic_locals: info.entry_frame_locals(),
    };

    for stmt in &method.body {
        g.gen_stmt(stmt);
    }

    // Every void method ends with an appended `return`, mapped to the closing brace.
    g.mark_line(method.close_line);
    g.emit_op(RETURN);
    let assembled = g.emitter.finish();

    CfMethod::with_code(
        0x0009, // ACC_PUBLIC | ACC_STATIC
        method.name.clone(),
        descriptor_of(method),
        CodeAttribute::new(
            assembled.max_stack,
            info.max_locals,
            assembled.code,
            assembled.line_numbers,
            entry_locals,
            assembled.stack_frames,
        ),
    )
}

/// Build the JVM method descriptor from the parsed signature.
fn descriptor_of(method: &Method) -> String {
    let mut d = String::from("(");
    for parameter in &method.params {
        parameter.ty.write_descriptor(&mut d);
    }
    d.push(')');
    method.return_type.write_descriptor(&mut d);
    d
}

/// Per-method emission state, with a running operand-stack depth (`cur`) tracked
/// in words so category-2 values count as two.
struct Gen<'a> {
    cp: &'a mut ConstantPool,
    info: &'a MethodInfo,
    exprs: &'a ExprArena,
    emitter: Emitter,
    /// The current sema-owned verifier-local snapshot. Statement generation only
    /// selects an entry or exit state; it never mutates local state independently.
    semantic_locals: &'a [FrameLocal],
}

impl<'a> Gen<'a> {
    // -------- control flow / labels / frames --------

    /// Emit one statement. Each statement starts with an empty operand stack; a
    /// leaf statement gets a LineNumberTable entry at its first instruction, while
    /// an `if` places its own entries (condition, then each nested statement).
    fn gen_stmt(&mut self, stmt: &Stmt) {
        self.emitter.reset_stack();
        self.install_stmt_entry(stmt.span);
        if let StmtKind::If {
            cond,
            then_branch,
            else_branch,
        } = &stmt.kind
        {
            self.gen_if(
                stmt.span,
                stmt.line,
                *cond,
                &then_branch.stmts,
                else_branch.as_ref().map(|body| body.stmts.as_slice()),
            );
        } else {
            self.mark_line(stmt.line);
            match &stmt.kind {
                StmtKind::LocalDecl { name, init, .. } => {
                    if let Some(init) = init {
                        self.store_to(name, *init);
                    }
                }
                StmtKind::Assign { name, value } => self.store_to(name, *value),
                StmtKind::CompoundAssign { name, op, value } => {
                    self.gen_compound(name, *op, *value)
                }
                StmtKind::Expr(expr) => self.gen_expr_stmt(*expr),
                StmtKind::If { .. } => unreachable!("handled above"),
            }
        }
        self.install_stmt_exit(stmt.span);
    }

    /// `if (cond) then [else els]`, a faithful port of javac's `visitIf`. A code-free
    /// static verdict emits only the taken arm and no frame; a static-false negated
    /// shortcut leaves its source line pending only on straight-line entry. A live
    /// branch target suppresses it. Otherwise `gen_cond` lowers the condition to a
    /// `CondItem` and its chains are resolved to the then/else/end
    /// targets. When the condition is statically false only the *then* is dropped
    /// (the else still runs); the trailing `goto`+else block is emitted only when
    /// the else is actually reachable (no spurious `goto`, no dead else).
    fn gen_if(
        &mut self,
        stmt_span: Span,
        line: u16,
        cond: ExprId,
        then_b: &[Stmt],
        else_b: Option<&[Stmt]>,
    ) {
        let previous_line = self.emitter.pending_line();
        let entered_by_branch = self.emitter.at_control_entry();
        self.mark_line(line);
        let code_before = self.emitter.instruction_count();
        let c = self.gen_cond(cond);

        // A code-free verdict has no instruction to consume the condition line.
        // Restore the previous pending position unless the lowered item carries
        // javac's preserving provenance for a static-false negated shortcut.
        if self.emitter.instruction_count() == code_before {
            let taken = if c.is_true() {
                true
            } else if c.is_false() {
                false
            } else {
                unreachable!("code-free condition without a static verdict")
            };
            let preserve_false_line = !taken
                && matches!(
                    c.position,
                    CodeFreePosition::PreserveFalseIfLine
                        | CodeFreePosition::PreserveThroughLogicalLeft
                )
                && !entered_by_branch;
            if !preserve_false_line {
                self.emitter.set_pending_line(previous_line);
            }
            let arm = if taken { Some(then_b) } else { else_b };
            for s in arm.unwrap_or(&[]) {
                self.gen_stmt(s);
            }
            return;
        }

        let is_false = c.is_false();
        let true_chain = c.true_chain;
        let else_chain = self.jump_false(c); // emit the false branch(es); may be None

        if !is_false {
            self.install_stmt_entry(stmt_span);
            self.resolve_chain(true_chain); // then-entry (frame iff a branch lands)
            for s in then_b {
                self.gen_stmt(s);
            }
        }
        // Emit the else body only when there is a reachable else target (or the
        // condition is statically false, so the then was dropped and the else is
        // the live arm). A statically-true condition with a dead else falls through
        // to the `_` arm: no goto, no else code.
        match else_b {
            Some(els) if else_chain.is_some() || is_false => {
                // Skip the else after a live then-body with a trailing goto.
                let end = if !is_false {
                    Some(self.branch_to_new(GOTO))
                } else {
                    None
                };
                self.install_stmt_entry(stmt_span);
                self.resolve_chain(else_chain);
                for s in els {
                    self.gen_stmt(s);
                }
                if let Some(end) = end {
                    self.install_stmt_exit(stmt_span);
                    self.resolve_chain(Some(end));
                }
            }
            _ => {
                self.install_stmt_exit(stmt_span);
                self.resolve_chain(else_chain);
            }
        }
    }

    /// Lower a boolean expression to a `CondItem` (javac's `genCond`): emit its
    /// operand loads eagerly, leaving only the deciding branch pending. A
    /// complete lowering-constant subtree collapses to a static verdict with no
    /// code. Non-strict `false && q` / `true || q` instead walk structurally and
    /// mark a shortcut verdict while dropping the dead operand. `&&`/`||` short-
    /// circuit from the *left*: the left's deciding branch is emitted, its
    /// non-deciding outcome falls through into the right operand, and the two
    /// chains are merged (`Code.mergeChains`).
    fn gen_cond(&mut self, e: ExprId) -> CondItem {
        // This query requires the complete subtree to be available as a javac
        // immediate. Non-strict shortcuts (`true || local`) stay structural so
        // grouping, negation, and casts retain their observable lowering history.
        if let Some(c) = lowering_const(self.exprs, e) {
            return if to_i32(c) != 0 {
                cond_true()
            } else {
                cond_false()
            };
        }
        let exprs = self.exprs;
        match &exprs[e] {
            ExprKind::Not(inner) => self.gen_cond(*inner).negate(),
            ExprKind::Paren(inner) => self.gen_cond(*inner).parenthesize(),
            ExprKind::Cast { ty, expr } if ty.is_boolean() => {
                self.gen_bool_value(*expr);
                cond_stack_test()
            }
            ExprKind::Compare { op, left, right } => self.gen_compare_cond(*op, *left, *right),
            ExprKind::Logical {
                op: LogOp::And,
                left,
                right,
            } => {
                let lc = self.gen_cond(*left).as_logical_left();
                if lc.is_false() {
                    return lc.mark_shortcut(); // false && _ : right is dead
                }
                let crossed_join = lc.true_chain.is_some();
                let lt = lc.true_chain;
                let fj = self.jump_false(lc); // emit the left's false branch
                self.resolve_chain(lt); // left-true falls through to the right
                let mut rc = self.gen_cond(*right);
                rc.false_chain = self.merge_chains(fj, rc.false_chain);
                rc.carry_prefix(&lc, crossed_join);
                rc
            }
            ExprKind::Logical {
                op: LogOp::Or,
                left,
                right,
            } => {
                let lc = self.gen_cond(*left).as_logical_left();
                if lc.is_true() {
                    return lc.mark_shortcut(); // true || _ : right is dead
                }
                let crossed_join = lc.false_chain.is_some();
                let lf = lc.false_chain;
                let tj = self.jump_true(lc);
                self.resolve_chain(lf);
                let mut rc = self.gen_cond(*right);
                rc.true_chain = self.merge_chains(tj, rc.true_chain);
                rc.carry_prefix(&lc, crossed_join);
                rc
            }
            // A bare boolean value (a local, or `&`/`|`/`^` on booleans): load its
            // 0/1 onto the stack, pending an `ifne`(true)/`ifeq`(false) test.
            _ => {
                self.gen_value(e); // pushes 0/1 (cur += 1)
                cond_stack_test()
            }
        }
    }

    /// Lower a comparison to a `CondItem`: emit its operands (and the wide
    /// `lcmp`/`fcmp*`/`dcmp*`), but *not* the branch — the deciding test opcode
    /// (true polarity) is returned pending. Its operands are popped when the
    /// branch is finally emitted, in `emit_test_branch`.
    fn gen_compare_cond(&mut self, op: CmpOp, left: ExprId, right: ExprId) -> CondItem {
        let p = sema::binary_promote(
            sema::type_of(left, self.info).primitive(),
            sema::type_of(right, self.info).primitive(),
        );
        let opcode = match p.stack() {
            StackTy::Int => {
                // javac folds `x <op> 0` to the compare-with-zero opcodes, but only
                // when the literal `0` is the *right* operand.
                if matches!(fold(self.exprs, right), Some(Const::Int(0))) {
                    self.gen_promoted_operand(left, PrimitiveType::Int);
                    int_zero_branch(op, true)
                } else {
                    self.gen_promoted_operand(left, PrimitiveType::Int);
                    self.gen_promoted_operand(right, PrimitiveType::Int);
                    int_icmp_branch(op, true)
                }
            }
            StackTy::Long => {
                self.gen_promoted_operand(left, PrimitiveType::Long);
                self.gen_promoted_operand(right, PrimitiveType::Long);
                self.emit_op(LCMP);
                int_zero_branch(op, true)
            }
            StackTy::Float => {
                self.gen_promoted_operand(left, PrimitiveType::Float);
                self.gen_promoted_operand(right, PrimitiveType::Float);
                self.emit_op(if matches!(op, CmpOp::Lt | CmpOp::Le) {
                    FCMPG
                } else {
                    FCMPL
                });
                int_zero_branch(op, true)
            }
            StackTy::Double => {
                self.gen_promoted_operand(left, PrimitiveType::Double);
                self.gen_promoted_operand(right, PrimitiveType::Double);
                self.emit_op(if matches!(op, CmpOp::Lt | CmpOp::Le) {
                    DCMPG
                } else {
                    DCMPL
                });
                int_zero_branch(op, true)
            }
        };
        CondItem {
            opcode: CondOp::Test(opcode),
            true_chain: None,
            false_chain: None,
            stack_reuse: false,
            origin: CondOrigin::Ordinary,
            materialization: Materialization::BareAllowed,
            position: CodeFreePosition::None,
        }
    }

    /// Emit the branch that routes the FALSE outcome of `c` to a chain, returning
    /// it (javac's `CondItem.jumpFalse`). Total: a static verdict emits nothing.
    fn jump_false(&mut self, c: CondItem) -> Option<Label> {
        if c.is_true() {
            return None; // never false
        }
        if c.is_false() {
            return c.false_chain; // already all-false: residual chain, no new branch
        }
        match c.opcode {
            CondOp::Test(op) => {
                let f = self.emit_test_branch(negate_op(op));
                self.merge_chains(c.false_chain, Some(f))
            }
            // dontgoto with a live true_chain (`q || false`): the false path is an
            // unconditional jump.
            CondOp::DontGoto => {
                debug_assert_eq!(
                    self.emitter.stack_depth(),
                    0,
                    "jump_false goto with non-empty stack"
                );
                let g = self.branch_to_new(GOTO);
                self.merge_chains(c.false_chain, Some(g))
            }
            // goto with a live false_chain (`q && true`, `a && (b||true)`): the
            // false path is exactly that chain; emit nothing.
            CondOp::Goto => c.false_chain,
        }
    }

    /// Emit the branch that routes the TRUE outcome of `c` to a chain, returning
    /// it (javac's `CondItem.jumpTrue`). Total: a static verdict emits nothing.
    fn jump_true(&mut self, c: CondItem) -> Option<Label> {
        if c.is_false() {
            return None; // never true
        }
        if c.is_true() {
            return c.true_chain;
        }
        match c.opcode {
            CondOp::Test(op) => {
                let t = self.emit_test_branch(op);
                self.merge_chains(c.true_chain, Some(t))
            }
            CondOp::Goto => {
                debug_assert_eq!(
                    self.emitter.stack_depth(),
                    0,
                    "jump_true goto with non-empty stack"
                );
                let g = self.branch_to_new(GOTO);
                self.merge_chains(c.true_chain, Some(g))
            }
            CondOp::DontGoto => c.true_chain,
        }
    }

    /// Materialize a boolean expression as a 0/1 on the stack. The general case is
    /// the true-first diamond `iconst_1; goto Lm; Lf: iconst_0; Lm:` over
    /// `gen_cond`'s pending branch; a bare value is already on the stack (no
    /// diamond); a statically-decided item with a residual branch resolves that
    /// branch then loads the constant `iconst_0`/`iconst_1`. Only supported with an
    /// empty base operand stack (the non-empty case needs full_frames — a later
    /// rung). Codegen preflight rejects that shape, leaving this assert as an
    /// invariant guard.
    fn gen_bool_value(&mut self, cond: ExprId) -> PrimitiveType {
        assert!(
            self.emitter.stack_depth() == 0,
            "materialized boolean with non-empty operand stack is unsupported"
        );
        let c = self.gen_cond(cond);

        // A bare boolean value already sits on the stack as 0/1, un-branched, so it
        // needs no materialization diamond. Every discriminator is carried by the
        // lowered item itself: negation clears stack reuse, grouping and crossed
        // joins require a diamond, and live chains exclude straight-line reuse.
        if c.stack_reuse
            && c.true_chain.is_none()
            && c.false_chain.is_none()
            && matches!(c.opcode, CondOp::Test(_))
            && c.origin == CondOrigin::Ordinary
            && c.materialization == Materialization::BareAllowed
        {
            return PrimitiveType::Boolean;
        }

        let is_false = c.is_false();
        let is_true = c.is_true();
        let true_chain = c.true_chain;
        let fj = self.jump_false(c);

        if is_false {
            // `q && false`: the residual false branch is already emitted; resolve
            // it here, the value is always 0.
            self.resolve_chain(fj);
            self.emit_op(ICONST_0);
        } else if is_true {
            // `q || true`: statically true with a residual true branch; resolve it,
            // the value is always 1.
            self.resolve_chain(true_chain);
            self.emit_op(ICONST_1);
        } else {
            // General true-first diamond.
            self.resolve_chain(true_chain); // true-entry (frame iff a branch lands)
            self.emit_op(ICONST_1);
            let lmerge = self.branch_to_new(GOTO);
            self.resolve_chain(fj);
            self.emitter.reset_stack(); // the iconst_1 lives only on the fall-through path
            self.emit_op(ICONST_0);
            self.place_label(lmerge);
            self.add_frame(vec![VerificationType::Integer]);
        }
        PrimitiveType::Boolean
    }

    /// Emit branch opcode `op` to a fresh label and return it as a one-site chain.
    fn branch_to_new(&mut self, op: u8) -> Label {
        let l = self.new_label();
        self.emit_branch_op(op, l);
        l
    }

    /// Emit a conditional *test* branch to a fresh chain and pop its operands (2
    /// for `if_icmp<cond>`, 1 for `if<cond>`/`ifne`/`ifeq`). `GOTO` must NOT route
    /// through here (it pops nothing).
    fn emit_test_branch(&mut self, op: u8) -> Label {
        self.branch_to_new(op)
    }

    /// Merge chain `b` into chain `a` (javac's `Code.mergeChains`): retarget every
    /// pending branch of `b` to `a`. Instruction order never affects output — all
    /// sites of a merged chain resolve to one position, and frames key by layout pc.
    fn merge_chains(&mut self, a: Option<Label>, b: Option<Label>) -> Option<Label> {
        match (a, b) {
            (None, x) | (x, None) => x,
            (Some(a), Some(b)) => {
                self.emitter.retarget_branches(b, a);
                Some(a)
            }
        }
    }

    /// Resolve a chain at the current instruction boundary: place its label and
    /// request a stack-map
    /// frame — but only when a branch actually targets it (a `Some` chain always
    /// has at least one live branch; `None` resolves to nothing, no frame).
    fn resolve_chain(&mut self, chain: Option<Label>) {
        debug_assert_eq!(
            self.emitter.stack_depth(),
            0,
            "chain resolved with non-empty operand stack"
        );
        if let Some(l) = chain {
            self.place_label(l);
            self.add_frame(Vec::new());
        }
    }

    /// Replace the source line waiting to attach to the next real instruction.
    /// This mirrors javac's pending-stat-position model: a code-free construct's
    /// line survives only if no later source position is marked before emission.
    fn mark_line(&mut self, line: u16) {
        self.emitter.set_pending_line(Some(line));
    }

    /// Emit one fixed, operand-free instruction through the physical chokepoint.
    fn emit_op(&mut self, opcode: u8) {
        self.emitter.emit(Instruction::Simple(opcode));
    }

    /// Reserve a fresh, not-yet-placed label.
    fn new_label(&mut self) -> Label {
        self.emitter.new_label()
    }

    /// Bind a label to the current symbolic instruction boundary.
    fn place_label(&mut self, label: Label) {
        self.emitter.place_label(label);
    }

    /// Emit a branch whose target remains symbolic until final layout.
    fn emit_branch_op(&mut self, opcode: u8, label: Label) {
        self.emitter.emit_branch(opcode, label);
    }

    /// Request a stack-map frame at the current instruction boundary, capturing
    /// the live-locals snapshot and the given operand-stack state.
    fn add_frame(&mut self, stack: Vec<VerificationType>) {
        self.emitter
            .request_frame(verification_locals(self.semantic_locals), stack);
    }

    fn install_stmt_entry(&mut self, span: Span) {
        self.semantic_locals = self.info.stmt_entry_frame_locals(span);
    }

    fn install_stmt_exit(&mut self, span: Span) {
        self.semantic_locals = self.info.stmt_exit_frame_locals(span);
    }

    // -------- statements --------

    fn gen_expr_stmt(&mut self, expr: ExprId) {
        match &self.exprs[expr] {
            ExprKind::Call { args, .. } => {
                let result = self.gen_call(expr, args);
                assert!(result.is_void(), "value-returning expression statement is unsupported");
            }
            other => panic!("unsupported expression statement: {other:?}"),
        }
    }

    fn gen_call(&mut self, expr: ExprId, args: &crate::ast::CallArgs) -> Type {
        let call = self.info.call(expr).clone();
        match call {
            ResolvedCall::Println { parameter_type } => {
                let field = self
                    .cp
                    .fieldref("java/lang/System", "out", "Ljava/io/PrintStream;");
                self.emitter.emit(Instruction::Field {
                    opcode: GETSTATIC,
                    index: field,
                    push_words: 1,
                });
                assert_eq!(args.len(), 1, "resolved call arity changed");
                for arg in args.iter() {
                    self.gen_value(arg);
                }
                let descriptor = match parameter_type.as_primitive() {
                    Some(PrimitiveType::Int) => "(I)V",
                    Some(PrimitiveType::Long) => "(J)V",
                    Some(PrimitiveType::Float) => "(F)V",
                    Some(PrimitiveType::Double) => "(D)V",
                    Some(PrimitiveType::Char) => "(C)V",
                    Some(PrimitiveType::Boolean) => "(Z)V",
                    Some(PrimitiveType::Byte | PrimitiveType::Short) => {
                        unreachable!("sema did not select println(int)")
                    }
                    None if parameter_type.is_string() => "(Ljava/lang/String;)V",
                    None => unreachable!("unsupported resolved println parameter"),
                };
                let method = self.cp.methodref("java/io/PrintStream", "println", descriptor);
                self.emitter.emit(Instruction::Invoke {
                    opcode: INVOKEVIRTUAL,
                    index: method,
                    argument_words: parameter_type.width(),
                    return_words: 0,
                });
                Type::Void
            }
        }
    }

    /// Assign `value` into local `name`, coercing to the local's declared type.
    fn store_to(&mut self, name: &Name, value: ExprId) {
        let target = self.info.ty(name);
        let slot = self.info.slot(name);
        self.gen_coerced(value, target);
        self.emit_store(slot, target);
    }

    /// Compound assignment `name op= value` (also `++`/`--`, which arrive as
    /// `op ∈ {Add,Sub}` with `value == 1`).
    fn gen_compound(&mut self, name: &Name, op: BinOp, value: ExprId) {
        let target = self.info.ty(name);
        let slot = self.info.slot(name);

        // iinc fast path: an `int` target, `+=`/`-=`, an int-family constant delta
        // that keeps the expression in `int`, and a slot/delta that fits.
        if target == PrimitiveType::Int
            && matches!(op, BinOp::Add | BinOp::Sub)
            && matches!(
                sema::type_of(value, self.info).as_primitive(),
                Some(
                    PrimitiveType::Int
                        | PrimitiveType::Byte
                        | PrimitiveType::Short
                        | PrimitiveType::Char
                )
            )
        {
            if let Some(c) = fold(self.exprs, value) {
                let k = to_i32(c);
                let delta = if op == BinOp::Add {
                    k
                } else {
                    k.wrapping_neg()
                };
                if slot <= 0xff && (-128..=127).contains(&delta) {
                    self.emitter.emit(Instruction::Iinc {
                        slot: slot as u8,
                        delta: delta as i8,
                    });
                    return;
                } else if (-32768..=32767).contains(&delta) {
                    self.emitter.emit(Instruction::WideIinc {
                        slot,
                        delta: delta as i16,
                    });
                    return;
                } else {
                    // Constant delta overflowing iinc_w: javac emits the POSITIVE
                    // magnitude and chooses the operator by the delta's sign, so
                    // `x -= -32768` becomes `iload; ldc 32768; iadd; istore` (not
                    // `sipush -32768; isub`) and `x += -40000` becomes `… isub`.
                    // (This also lets `+= n` and `-= -n` share one pool entry.)
                    self.emit_load(slot, PrimitiveType::Int);
                    let (mag, add) = int_delta_magnitude(delta);
                    self.emit_int_const(mag);
                    self.emit_op(if add { IADD } else { ISUB });
                    self.emit_store(slot, PrimitiveType::Int);
                    return;
                }
            }
        }

        // General form: name = (target)(name op value), computed in the promoted
        // type `p`, then narrowed back to `target`.
        let p = if op.is_shift() {
            sema::unary_promote(target)
        } else {
            sema::binary_promote(target, sema::type_of(value, self.info).primitive())
        };
        self.emit_load(slot, target);
        self.emit_convert(target, p);
        if op.is_shift() {
            self.gen_shift_distance(value);
            self.emit_shift(p, op);
        } else if let Some(delta) = int_additive_const_delta(self.exprs, op, p, value) {
            // javac normalizes an additive *constant* on an int-family target to a
            // non-negative magnitude, choosing the operator by the delta's sign — so
            // `char v -= -100` is `bipush 100; iadd` (then i2c), never `bipush -100;
            // isub`. Same split as the iinc-overflow path above; int-family only
            // (a `long`/`float`/`double` target keeps the raw `lsub`/`dsub`/`fsub`).
            let (mag, add) = int_delta_magnitude(delta);
            self.emit_int_const(mag);
            self.emit_op(if add { IADD } else { ISUB });
        } else {
            self.gen_promoted_operand(value, p);
            self.emit_binop(p, op);
        }
        self.emit_convert(p, target);
        self.emit_store(slot, target);
    }

    // -------- expression values --------

    /// Emit `value` coerced to `target` (assignment / initializer context): a
    /// constant is folded straight to a `target`-typed constant (no conversion
    /// opcode); a non-constant is emitted then widened.
    fn gen_coerced(&mut self, value: ExprId, target: PrimitiveType) {
        if target == PrimitiveType::Boolean && sema::type_of(value, self.info).is_boolean() {
            self.gen_bool_value(value);
            return;
        }
        if let Some(c) = fold(self.exprs, value) {
            self.load_const(const_convert(c, target), target);
        } else {
            let s = self.gen_nonconst(value);
            self.emit_convert(s, target);
        }
    }

    /// Emit `expr` leaving its natural-typed value on the stack; returns the type.
    fn gen_value(&mut self, expr: ExprId) -> Type {
        // Value-mode parentheses are transparent. Handle them before the
        // primitive-only path so a parenthesized String literal keeps its class
        // type instead of being projected to `PrimitiveType`.
        if let ExprKind::Paren(inner) = &self.exprs[expr] {
            return self.gen_value(*inner);
        }
        // A string literal is the one non-numeric value form (only ever a
        // `println` argument); it loads via `ldc` of a `String` constant.
        if let ExprKind::StringLit(s) = &self.exprs[expr] {
            let idx = self.cp.string(s);
            self.emit_ldc(idx);
            return Type::string();
        }
        if let Some(c) = fold(self.exprs, expr) {
            let t = sema::type_of(expr, self.info);
            let primitive = t.primitive();
            self.load_const(const_convert(c, primitive), primitive);
            t.clone()
        } else {
            self.gen_nonconst(expr).into()
        }
    }

    /// Emit `expr` as an operand of a binary op whose promoted type is `p`,
    /// widening to `p`. A constant is loaded already in `p`; a non-constant is
    /// emitted in its own type then converted.
    fn gen_promoted_operand(&mut self, expr: ExprId, p: PrimitiveType) {
        if let Some(c) = fold(self.exprs, expr) {
            self.load_const(const_convert(c, p), p);
        } else {
            let s = self.gen_nonconst(expr);
            self.emit_convert(s, p);
        }
    }

    /// Emit a non-constant expression, returning its static type.
    fn gen_nonconst(&mut self, expr: ExprId) -> PrimitiveType {
        let exprs = self.exprs;
        match &exprs[expr] {
            ExprKind::Name(n) => {
                let ty = self.info.ty(n);
                self.emit_load(self.info.slot(n), ty);
                ty
            }
            ExprKind::Neg(e) => {
                self.gen_value(*e);
                let p = sema::unary_promote(sema::type_of(*e, self.info).primitive());
                self.emit_op(neg_op(p.stack()));
                p
            }
            ExprKind::BitNot(e) => {
                self.gen_value(*e);
                let p = sema::unary_promote(sema::type_of(*e, self.info).primitive());
                self.emit_bitnot(p);
                p
            }
            ExprKind::Paren(e) => self.gen_value(*e).primitive(),
            ExprKind::Cast { ty, expr } => {
                let s = self.gen_value(*expr).primitive();
                let target = ty.primitive();
                self.emit_convert(s, target);
                target
            }
            ExprKind::Binary { op, left, right } => self.gen_binary(*op, *left, *right),
            ExprKind::Compare { .. } | ExprKind::Not(_) | ExprKind::Logical { .. } => {
                self.gen_bool_value(expr)
            }
            other => panic!("not a value expression: {other:?}"),
        }
    }

    /// Emit a shift *distance* (a shift's right operand), which the JVM always
    /// consumes as an `int`. javac narrows a *constant* distance to an int constant at
    /// compile time (`x << 40L` → `bipush 40`, not `ldc2_w 40l; l2i`); only a
    /// non-constant `long` distance keeps the runtime `l2i`.
    fn gen_shift_distance(&mut self, right: ExprId) {
        if let Some(c) = fold(self.exprs, right) {
            self.emit_int_const(to_i32(c)); // (int) narrowing of the constant
        } else {
            let at = self.gen_value(right);
            if at.primitive().stack() == StackTy::Long {
                self.emit_op(L2I);
            }
        }
    }

    fn gen_binary(&mut self, op: BinOp, left: ExprId, right: ExprId) -> PrimitiveType {
        let lt = sema::type_of(left, self.info).primitive();
        let rt = sema::type_of(right, self.info).primitive();

        // `&`/`|`/`^` on two booleans: int opcode, boolean result.
        if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor)
            && lt == PrimitiveType::Boolean
            && rt == PrimitiveType::Boolean
        {
            self.gen_value(left);
            self.gen_value(right);
            self.emit_binop(PrimitiveType::Int, op);
            return PrimitiveType::Boolean;
        }

        if op.is_shift() {
            let result = sema::unary_promote(lt);
            self.gen_promoted_operand(left, result);
            self.gen_shift_distance(right);
            self.emit_shift(result, op);
            result
        } else {
            let p = sema::binary_promote(lt, rt);
            self.gen_promoted_operand(left, p);
            self.gen_promoted_operand(right, p);
            self.emit_binop(p, op);
            p
        }
    }

    // -------- emitters --------

    /// Load a constant already in family `ty` onto the stack.
    fn load_const(&mut self, c: Const, ty: PrimitiveType) {
        match ty.stack() {
            StackTy::Int => self.emit_int_const(to_i32(c)),
            StackTy::Long => self.emit_long_const(to_i64(c)),
            StackTy::Float => self.emit_float_const(to_f32(c)),
            StackTy::Double => self.emit_double_const(to_f64(c)),
        }
    }

    /// Load an `int` constant with the tightest opcode javac would choose.
    fn emit_int_const(&mut self, v: i32) {
        match v {
            -1 => self.emit_op(ICONST_M1),
            0..=5 => self.emit_op(ICONST_0 + v as u8),
            -128..=127 => {
                self.emitter.emit(Instruction::U8 {
                    opcode: BIPUSH,
                    operand: v as u8,
                });
            }
            -32768..=32767 => {
                self.emitter.emit(Instruction::U16 {
                    opcode: SIPUSH,
                    operand: v as u16,
                });
            }
            _ => {
                let idx = self.cp.integer(v);
                self.emit_ldc(idx);
            }
        }
    }

    fn emit_long_const(&mut self, v: i64) {
        match v {
            0 => self.emit_op(LCONST_0),
            1 => self.emit_op(LCONST_1),
            _ => {
                let idx = self.cp.long(v);
                self.emitter.emit(Instruction::U16 {
                    opcode: LDC2_W,
                    operand: idx,
                });
            }
        }
    }

    fn emit_float_const(&mut self, v: f32) {
        // Compare by bit pattern: only +0.0f/+1.0f/+2.0f get the const opcodes,
        // so -0.0f (and NaN) fall through to the pool.
        match v.to_bits() {
            b if b == 0.0f32.to_bits() => self.emit_op(FCONST_0),
            b if b == 1.0f32.to_bits() => self.emit_op(FCONST_1),
            b if b == 2.0f32.to_bits() => self.emit_op(FCONST_2),
            _ => {
                let idx = self.cp.float(v);
                self.emit_ldc(idx);
            }
        }
    }

    fn emit_double_const(&mut self, v: f64) {
        match v.to_bits() {
            b if b == 0.0f64.to_bits() => self.emit_op(DCONST_0),
            b if b == 1.0f64.to_bits() => self.emit_op(DCONST_1),
            _ => {
                let idx = self.cp.double(v);
                self.emitter.emit(Instruction::U16 {
                    opcode: LDC2_W,
                    operand: idx,
                });
            }
        }
    }

    /// `ldc`/`ldc_w` of a single-word pool entry (Integer/Float/String).
    fn emit_ldc(&mut self, idx: u16) {
        if idx <= 0xff {
            self.emitter.emit(Instruction::U8 {
                opcode: LDC,
                operand: idx as u8,
            });
        } else {
            self.emitter.emit(Instruction::U16 {
                opcode: LDC_W,
                operand: idx,
            });
        }
    }

    fn emit_load(&mut self, slot: u16, ty: PrimitiveType) {
        let (short0, wide) = load_ops(ty);
        if slot <= 3 {
            self.emit_op(short0 + slot as u8);
        } else {
            self.emitter.emit(Instruction::U8 {
                opcode: wide,
                operand: slot as u8,
            });
        }
    }

    fn emit_store(&mut self, slot: u16, ty: PrimitiveType) {
        let (short0, wide) = store_ops(ty);
        if slot <= 3 {
            self.emit_op(short0 + slot as u8);
        } else {
            self.emitter.emit(Instruction::U8 {
                opcode: wide,
                operand: slot as u8,
            });
        }
    }

    fn emit_binop(&mut self, p: PrimitiveType, op: BinOp) {
        self.emit_op(binop_op(p.stack(), op));
    }

    fn emit_shift(&mut self, result: PrimitiveType, op: BinOp) {
        self.emit_op(shift_op(result.stack(), op));
    }

    /// `~x` == `x ^ -1`, with the `-1` loaded per the value's type.
    fn emit_bitnot(&mut self, p: PrimitiveType) {
        match p.stack() {
            StackTy::Long => {
                let idx = self.cp.long(-1);
                self.emitter.emit(Instruction::U16 {
                    opcode: LDC2_W,
                    operand: idx,
                });
                self.emit_op(LXOR);
            }
            _ => {
                self.emit_op(ICONST_M1);
                self.emit_op(IXOR);
            }
        }
    }

    /// Emit the conversion from `from` to `to`, if any, adjusting the stack.
    fn emit_convert(&mut self, from: PrimitiveType, to: PrimitiveType) {
        if from == to {
            return;
        }
        let fs = from.stack();
        if matches!(
            to,
            PrimitiveType::Byte | PrimitiveType::Short | PrimitiveType::Char
        ) {
            // Bring the value to the `int` computational type first.
            match fs {
                StackTy::Long => self.emit_op(L2I),
                StackTy::Float => self.emit_op(F2I),
                StackTy::Double => self.emit_op(D2I),
                StackTy::Int => {}
            }
            // Narrow within int-family only when `from` is wider than `to`.
            let cur_ty = if fs == StackTy::Int {
                from
            } else {
                PrimitiveType::Int
            };
            if let Some(op) = subint_narrow_op(cur_ty, to) {
                self.emit_op(op);
            }
        } else if fs != to.stack() {
            self.emit_op(cross_conv_op(fs, to.stack()));
        }
    }
}

fn verification_locals(locals: &[FrameLocal]) -> Vec<VerificationType> {
    locals
        .iter()
        .map(|local| match local {
            FrameLocal::Top => VerificationType::Top,
            FrameLocal::Integer => VerificationType::Integer,
            FrameLocal::Float => VerificationType::Float,
            FrameLocal::Long => VerificationType::Long,
            FrameLocal::Double => VerificationType::Double,
            FrameLocal::Object(name) => VerificationType::Object(name.clone()),
        })
        .collect()
}
