mod body;
mod condition;
mod emit;

use crate::ast::{ExprArena, Method};
use crate::classfile::{CodeAttribute, ConstantPool, Method as CfMethod, VerificationType};
use crate::sema::{FrameLocal, MethodInfo};
use crate::span::Span;

use super::assembler::Emitter;
use super::instruction::*;

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
        let fallthrough_locals =
            is_cond_branch_opcode(opcode).then(|| verification_locals(self.semantic_locals));
        self.emitter.emit_branch(opcode, label, fallthrough_locals);
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
