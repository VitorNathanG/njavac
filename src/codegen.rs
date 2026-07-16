//! Code generation: typed AST -> class bytes, via the `classfile` backend.
//!
//! This is where byte-identity is won or lost. Every choice mirrors javac's:
//! constant-load opcode selection per type, local-slot allocation with the
//! two-slot `long`/`double` model, short-form load/store opcodes, `max_stack`/
//! `max_locals`, the `LineNumberTable`, binary numeric promotion with the
//! conversion opcode placed exactly where javac puts it, the `iinc`/`iinc_w`/
//! full-form boundary for compound assignment, and cross-type constant folding.
//!
//! javac constant-folds any subtree whose leaves are all literals into a single
//! typed constant load (with wrapping integer / exact IEEE-754 arithmetic and
//! JLS shift masking), and emits real bytecode the moment a local is involved.
//! We mirror that: `fold` evaluates a maximal constant subtree; anything else is
//! emitted structurally with a running operand-stack model that tracks category-2
//! (`long`/`double`) values as two words.
//!
//! `if`/`else` and comparisons add the first control flow. A boolean expression
//! lowers in one of two modes: as a *branch* (the condition of an `if`, emitting
//! the negated comparison opcode as a jump) or as a *value* (the true-first
//! `iconst_1`/`goto`/`iconst_0` diamond). Both force a `StackMapTable`: codegen
//! records the verifier state (locals + stack) at each branch target and hands
//! them to the backend, which picks the minimal frame encoding. Constant
//! conditions are folded away (dead branches dropped, no frame), and jumps to an
//! unconditional `goto` are threaded through — both exactly as javac does, so a
//! method whose branches all fold stays byte-identical to its straight-line form.

mod assembler;
mod condition;
mod constant;
mod instruction;
mod lowering;
mod ops;
mod preflight;

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
    methods.push(lowering::gen_init(
        &mut cp,
        &class.super_class,
        class.line,
    ));
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
