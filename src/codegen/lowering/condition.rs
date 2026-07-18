use crate::ast::{CmpOp, ExprId, ExprKind, LogOp, PrimitiveType, Stmt};
use crate::classfile::VerificationType;
use crate::sema;
use crate::span::Span;

use super::super::condition::*;
use super::super::constant::*;
use super::super::instruction::*;
use super::super::ops::*;
use super::super::stack::StackTy;
use super::Gen;

impl Gen<'_> {
    // -------- control flow / labels / frames --------

    /// Lower an `if` using the control-flow shape reconstructed from pinned output. A code-free
    /// static verdict emits only the taken arm and no frame; a static-false negated
    /// shortcut leaves its source line pending only on straight-line entry. A live
    /// branch target suppresses it. Otherwise `gen_cond` lowers the condition to a
    /// `CondItem` and its chains are resolved to the then/else/end
    /// targets. When the condition is statically false only the *then* is dropped
    /// (the else still runs); the trailing `goto`+else block is emitted only when
    /// the else is actually reachable (no spurious `goto`, no dead else).
    pub(super) fn gen_if(
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
    /// chains are merged by the local conditional-item model.
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
    pub(super) fn gen_bool_value(&mut self, cond: ExprId) -> PrimitiveType {
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

    /// Merge chain `b` into chain `a`: retarget every
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
}
