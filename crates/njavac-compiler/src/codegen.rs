//! Code generation: typed AST -> class bytes, via the `classfile` backend.
//!
//! This stage contains byte-visible choices reconstructed from pinned black-box
//! output: constant-load opcode selection per type, the two-slot `long`/`double`
//! model, short-form load/store opcodes, `max_stack`/`max_locals`, line metadata,
//! conversion placement, compound-assignment forms, and supported constant
//! folding.
//!
//! For the documented supported forms, pinned output folds maximal eligible
//! constant subtrees with wrapping integer, IEEE-754, and Java shift behavior.
//! `fold` models those cases; other expressions are emitted structurally with a
//! running operand-stack model that tracks category-2 (`long`/`double`) values as
//! two words.
//!
//! `if`/`else` and comparisons add the first control flow. A boolean expression
//! lowers in one of two modes: as a *branch* (the condition of an `if`, emitting
//! the negated comparison opcode as a jump) or as a *value* (the true-first
//! `iconst_1`/`goto`/`iconst_0` diamond). Both force a `StackMapTable`: codegen
//! records the verifier state (locals + stack) at each branch target and hands
//! them to the backend, which picks the minimal frame encoding. Constant
//! conditions are folded away (dead branches dropped, no frame), and jumps to an
//! unconditional `goto` are threaded through according to pinned output, so a
//! method whose supported branches all fold matches its straight-line form.

mod assembler;
mod condition;
mod constant;
mod instruction;
mod lowering;
mod ops;
mod preflight;
mod stack;

use crate::ast::CompilationUnit;
use crate::classfile::{ClassFile, ConstantPool};
use crate::diagnostic::CompileResult;
use crate::sema::Analysis;

/// A complete class-file plan plus the phase-1 constant pool built while lowering
/// bytecode. Serialization owns phase-2 structural interning and class-file layout.
pub struct ClassPlan {
    class_file: ClassFile,
    constant_pool: ConstantPool,
}

impl ClassPlan {
    pub fn to_bytes(self) -> Vec<u8> {
        self.class_file.to_bytes(self.constant_pool)
    }
}

/// Build the typed bytecode and class-file model without serializing it.
pub fn plan(
    unit: &CompilationUnit,
    analysis: &Analysis,
    source_file: &str,
) -> CompileResult<ClassPlan> {
    assert_eq!(
        unit.exprs.identity(),
        analysis.arena_identity(),
        "analysis belongs to a different expression arena"
    );
    assert_eq!(
        unit.class.methods.len(),
        analysis.methods.len(),
        "analysis method count does not match the compilation unit"
    );
    preflight::preflight_codegen(unit, analysis)?;
    #[cfg(debug_assertions)]
    ops::assert_negate_op_consistent();
    let mut cp = ConstantPool::new();
    let class = &unit.class;

    let mut methods = Vec::new();
    // `<init>` first: its `Methodref` is interned before any of main's operands.
    methods.push(lowering::gen_init(&mut cp, &class.super_class, class.line));
    for (m, info) in class.methods.iter().zip(&analysis.methods) {
        methods.push(lowering::gen_method(&mut cp, m, info, &unit.exprs));
    }

    let class_file = ClassFile::new(
        0x0021, // ACC_PUBLIC | ACC_SUPER
        class.name.clone(),
        class.super_class.clone(),
        methods,
        source_file,
    );
    Ok(ClassPlan {
        class_file,
        constant_pool: cp,
    })
}

/// Compile one parsed+analyzed class into `.class` bytes.
pub fn generate(
    unit: &CompilationUnit,
    analysis: &Analysis,
    source_file: &str,
) -> CompileResult<Vec<u8>> {
    Ok(plan(unit, analysis, source_file)?.to_bytes())
}
