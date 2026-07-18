use super::instruction::{Label, IFNE};
use super::ops::negate_op;

/// njavac's empirically reconstructed conditional-item model for the supported
/// side-effect-free boolean subset. Lowering (`gen_cond`) emits every operand load
/// eagerly but leaves the *deciding branch* pending in `opcode`; the not-yet-
/// resolved jump sites are collected in `true_chain`/`false_chain`. Consumers
/// (`gen_if`, `gen_bool_value`) then resolve those chains to concrete pcs. This is
/// the representation that matches observed constant short-circuit collapse such
/// as `true || q` and `q && false`.
#[derive(Clone, Copy)]
pub(super) struct CondItem {
    /// The pending deciding branch, or a static verdict.
    pub(super) opcode: CondOp,
    /// Chains as label ids collecting pending jump sites. `None` = the empty chain
    /// (javac's null): nothing targets it, so resolving it places no frame. A
    /// `Some` chain always has at least one live symbolic branch.
    pub(super) true_chain: Option<Label>,
    pub(super) false_chain: Option<Label>,
    /// True iff an un-branched boolean 0/1 is currently on the operand stack (the
    /// bare-value leaf sets it; any emitted branch consumes and clears it). It is
    /// reusable only when the other item-state dimensions also permit it.
    pub(super) stack_reuse: bool,
    /// How a code-free static verdict arose. A negated shortcut is the one origin
    /// whose surrounding grouping can affect later value materialization.
    pub(super) origin: CondOrigin,
    /// Whether a final reusable stack value may stay bare or must pass through
    /// javac's true/false materialization diamond.
    pub(super) materialization: Materialization,
    /// Independent pending-position effect for a code-free static-false `if`.
    pub(super) position: CodeFreePosition,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CondOrigin {
    Ordinary,
    Shortcut,
    NegatedShortcut,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Materialization {
    BareAllowed,
    DiamondRequired,
}

/// Pending-line provenance, ordered by merge strength. Logical nodes keep the
/// strongest state contributed by their evaluated operands.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum CodeFreePosition {
    None,
    ShortcutAwaitingNegation,
    PreserveFalseIfLine,
    PreserveThroughLogicalLeft,
}

/// The deciding branch of a `CondItem`: a real conditional test (taken when the
/// condition is *true*), or a static verdict mirroring javac's `goto_`/`dontgoto`.
#[derive(Clone, Copy)]
pub(super) enum CondOp {
    Test(u8), // conditional branch opcode taken when TRUE (ifne / if_icmplt / …)
    Goto,     // statically TRUE
    DontGoto, // statically FALSE
}

impl CondItem {
    /// Statically always-true: an unconditional `goto` sense with no pending
    /// false jumps. Exactly javac's `CondItem.isTrue()`.
    pub(super) fn is_true(&self) -> bool {
        matches!(self.opcode, CondOp::Goto) && self.false_chain.is_none()
    }
    /// Statically always-false: never jumps true and no pending true jumps.
    pub(super) fn is_false(&self) -> bool {
        matches!(self.opcode, CondOp::DontGoto) && self.true_chain.is_none()
    }
    /// `!e`: swap the true/false chains and negate the deciding branch.
    pub(super) fn negate(self) -> CondItem {
        let origin = match self.origin {
            CondOrigin::Ordinary => CondOrigin::Ordinary,
            CondOrigin::Shortcut | CondOrigin::NegatedShortcut => CondOrigin::NegatedShortcut,
        };
        CondItem {
            opcode: match self.opcode {
                CondOp::Goto => CondOp::DontGoto,
                CondOp::DontGoto => CondOp::Goto,
                CondOp::Test(op) => CondOp::Test(negate_op(op)),
            },
            true_chain: self.false_chain,
            false_chain: self.true_chain,
            // `stack_reuse` asserts the stacked 0/1 equals the boolean result; a
            // negation inverts the result, so the un-touched stack value is now the
            // *opposite* and must NOT be used as-is. Clearing this forces `!p` (and
            // `!!p`, which restores the `IFNE` opcode but stays cleared) through the
            // materialization diamond in `gen_bool_value`, matching javac, which
            // diamonds every negation rather than reusing the loaded value.
            stack_reuse: false,
            origin,
            materialization: self.materialization,
            position: match self.position {
                CodeFreePosition::PreserveThroughLogicalLeft => {
                    CodeFreePosition::PreserveThroughLogicalLeft
                }
                CodeFreePosition::ShortcutAwaitingNegation
                | CodeFreePosition::PreserveFalseIfLine => CodeFreePosition::PreserveFalseIfLine,
                CodeFreePosition::None if origin == CondOrigin::NegatedShortcut => {
                    CodeFreePosition::PreserveFalseIfLine
                }
                CodeFreePosition::None => CodeFreePosition::None,
            },
        }
    }

    /// Grouping is transparent except around a negated non-strict shortcut. In
    /// that one case javac keeps a value-materialization requirement for a later
    /// logical result, without emitting code for the grouped operand itself.
    pub(super) fn parenthesize(mut self) -> CondItem {
        if self.origin == CondOrigin::NegatedShortcut {
            self.materialization = Materialization::DiamondRequired;
        }
        if self.position == CodeFreePosition::PreserveFalseIfLine {
            self.position = CodeFreePosition::PreserveThroughLogicalLeft;
        }
        self
    }

    /// An ungrouped active position used as a logical left operand becomes latent:
    /// it cannot preserve a line immediately, but a later `!` can reactivate it.
    /// Grouping after activation protects the active state through logical use.
    pub(super) fn as_logical_left(mut self) -> CondItem {
        if self.position == CodeFreePosition::PreserveFalseIfLine {
            self.position = CodeFreePosition::ShortcutAwaitingNegation;
        }
        self
    }

    pub(super) fn mark_shortcut(mut self) -> CondItem {
        self.origin = CondOrigin::Shortcut;
        self
    }

    pub(super) fn carry_prefix(&mut self, prefix: &CondItem, crossed_join: bool) {
        let code_free_static_right = (self.is_true() || self.is_false())
            && self.true_chain.is_none()
            && self.false_chain.is_none();
        if prefix.origin == CondOrigin::Shortcut && code_free_static_right {
            // A static right operand keeps shortcut ancestry only for a later
            // negation's source-position behavior. It must not taint origin or
            // value materialization.
            self.position =
                std::cmp::max(self.position, CodeFreePosition::ShortcutAwaitingNegation);
        }
        if prefix.materialization == Materialization::DiamondRequired || crossed_join {
            self.materialization = Materialization::DiamondRequired;
        }
        self.position = std::cmp::max(self.position, prefix.position);
    }
}

/// A statically-true `CondItem` (no code emitted); javac's `goto_` verdict.
pub(super) fn cond_true() -> CondItem {
    cond_static(true)
}
/// A statically-false `CondItem` (no code emitted); javac's `dontgoto` verdict.
pub(super) fn cond_false() -> CondItem {
    cond_static(false)
}

fn cond_static(value: bool) -> CondItem {
    CondItem {
        opcode: if value {
            CondOp::Goto
        } else {
            CondOp::DontGoto
        },
        true_chain: None,
        false_chain: None,
        stack_reuse: false,
        origin: CondOrigin::Ordinary,
        materialization: Materialization::BareAllowed,
        position: CodeFreePosition::None,
    }
}

pub(super) fn cond_stack_test() -> CondItem {
    CondItem {
        opcode: CondOp::Test(IFNE),
        true_chain: None,
        false_chain: None,
        stack_reuse: true,
        origin: CondOrigin::Ordinary,
        materialization: Materialization::BareAllowed,
        position: CodeFreePosition::None,
    }
}
