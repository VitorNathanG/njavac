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

use crate::ast::{BinOp, Class, CmpOp, Expr, LogOp, Method, Stmt, StmtKind, Type};
use crate::classfile::{ClassFile, ConstantPool, Method as CfMethod, StackFrame, VerificationType};
use crate::sema::{self, Analysis, MethodInfo, StackTy, ValType};

// ---- opcodes ----
const ICONST_M1: u8 = 0x02;
const ICONST_0: u8 = 0x03;
const LCONST_0: u8 = 0x09;
const LCONST_1: u8 = 0x0a;
const FCONST_0: u8 = 0x0b;
const FCONST_1: u8 = 0x0c;
const FCONST_2: u8 = 0x0d;
const DCONST_0: u8 = 0x0e;
const DCONST_1: u8 = 0x0f;
const BIPUSH: u8 = 0x10;
const SIPUSH: u8 = 0x11;
const LDC: u8 = 0x12;
const LDC_W: u8 = 0x13;
const LDC2_W: u8 = 0x14;

// Loads: wide form (opcode + 1-byte slot) and the slot-0 short form.
const ILOAD: u8 = 0x15;
const LLOAD: u8 = 0x16;
const FLOAD: u8 = 0x17;
const DLOAD: u8 = 0x18;
const ILOAD_0: u8 = 0x1a;
const LLOAD_0: u8 = 0x1e;
const FLOAD_0: u8 = 0x22;
const DLOAD_0: u8 = 0x26;
const ALOAD_0: u8 = 0x2a;

// Stores.
const ISTORE: u8 = 0x36;
const LSTORE: u8 = 0x37;
const FSTORE: u8 = 0x38;
const DSTORE: u8 = 0x39;
const ISTORE_0: u8 = 0x3b;
const LSTORE_0: u8 = 0x3f;
const FSTORE_0: u8 = 0x43;
const DSTORE_0: u8 = 0x47;

// Arithmetic.
const IADD: u8 = 0x60;
const LADD: u8 = 0x61;
const FADD: u8 = 0x62;
const DADD: u8 = 0x63;
const ISUB: u8 = 0x64;
const LSUB: u8 = 0x65;
const FSUB: u8 = 0x66;
const DSUB: u8 = 0x67;
const IMUL: u8 = 0x68;
const LMUL: u8 = 0x69;
const FMUL: u8 = 0x6a;
const DMUL: u8 = 0x6b;
const IDIV: u8 = 0x6c;
const LDIV: u8 = 0x6d;
const FDIV: u8 = 0x6e;
const DDIV: u8 = 0x6f;
const IREM: u8 = 0x70;
const LREM: u8 = 0x71;
const FREM: u8 = 0x72;
const DREM: u8 = 0x73;
const INEG: u8 = 0x74;
const LNEG: u8 = 0x75;
const FNEG: u8 = 0x76;
const DNEG: u8 = 0x77;

// Shifts and bitwise.
const ISHL: u8 = 0x78;
const LSHL: u8 = 0x79;
const ISHR: u8 = 0x7a;
const LSHR: u8 = 0x7b;
const IUSHR: u8 = 0x7c;
const LUSHR: u8 = 0x7d;
const IAND: u8 = 0x7e;
const LAND: u8 = 0x7f;
const IOR: u8 = 0x80;
const LOR: u8 = 0x81;
const IXOR: u8 = 0x82;
const LXOR: u8 = 0x83;

// iinc + wide prefix.
const IINC: u8 = 0x84;
const WIDE: u8 = 0xc4;

// Conversions.
const I2L: u8 = 0x85;
const I2F: u8 = 0x86;
const I2D: u8 = 0x87;
const L2I: u8 = 0x88;
const L2F: u8 = 0x89;
const L2D: u8 = 0x8a;
const F2I: u8 = 0x8b;
const F2L: u8 = 0x8c;
const F2D: u8 = 0x8d;
const D2I: u8 = 0x8e;
const D2L: u8 = 0x8f;
const D2F: u8 = 0x90;
const I2B: u8 = 0x91;
const I2C: u8 = 0x92;
const I2S: u8 = 0x93;

// Comparisons and branches.
const LCMP: u8 = 0x94;
const FCMPL: u8 = 0x95;
const FCMPG: u8 = 0x96;
const DCMPL: u8 = 0x97;
const DCMPG: u8 = 0x98;
const IFEQ: u8 = 0x99;
const IFNE: u8 = 0x9a;
const IFLT: u8 = 0x9b;
const IFGE: u8 = 0x9c;
const IFGT: u8 = 0x9d;
const IFLE: u8 = 0x9e;
const IF_ICMPEQ: u8 = 0x9f;
const IF_ICMPNE: u8 = 0xa0;
const IF_ICMPLT: u8 = 0xa1;
const IF_ICMPGE: u8 = 0xa2;
const IF_ICMPGT: u8 = 0xa3;
const IF_ICMPLE: u8 = 0xa4;
const GOTO: u8 = 0xa7;

const ICONST_1: u8 = 0x04;

const GETSTATIC: u8 = 0xb2;
const INVOKEVIRTUAL: u8 = 0xb6;
const INVOKESPECIAL: u8 = 0xb7;
const RETURN: u8 = 0xb1;

/// A compile-time constant value in one of the four JVM computational types.
/// `boolean`/`char` fold into `Int` (their code-point / 0-1 value).
#[derive(Clone, Copy)]
enum Const {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
}

/// Compile one parsed+analyzed class into `.class` bytes.
pub fn generate(class: &Class, analysis: &Analysis, source_file: &str) -> Vec<u8> {
    #[cfg(debug_assertions)]
    assert_negate_op_consistent();
    let mut cp = ConstantPool::new();

    let mut methods = Vec::new();
    // `<init>` first: its `Methodref` is interned before any of main's operands.
    methods.push(gen_init(&mut cp, class.line));
    for (m, info) in class.methods.iter().zip(&analysis.methods) {
        methods.push(gen_method(&mut cp, m, info));
    }

    let class_file = ClassFile {
        access_flags: 0x0021, // ACC_PUBLIC | ACC_SUPER
        this_class: class.name.clone(),
        super_class: "java/lang/Object".to_string(),
        source_file: source_file.to_string(),
        methods,
    };
    class_file.to_bytes(cp)
}

/// The implicit default constructor: `aload_0; invokespecial Object.<init>; return`.
fn gen_init(cp: &mut ConstantPool, class_line: u16) -> CfMethod {
    let mut code = Vec::new();
    code.push(ALOAD_0);
    let init_ref = cp.methodref("java/lang/Object", "<init>", "()V");
    code.push(INVOKESPECIAL);
    push_u16(&mut code, init_ref);
    code.push(RETURN);

    CfMethod {
        access_flags: 0x0001, // ACC_PUBLIC
        name: "<init>".to_string(),
        descriptor: "()V".to_string(),
        max_stack: 1,
        max_locals: 1,
        code,
        line_numbers: vec![(0, class_line)],
        entry_locals: Vec::new(),
        stack_frames: Vec::new(),
    }
}

/// Emit one method body.
fn gen_method(cp: &mut ConstantPool, method: &Method, info: &MethodInfo) -> CfMethod {
    // The verifier's method-entry locals: one entry per parameter (the seed for
    // stack-map frame deltas). `main`'s single `String[] args` is one `Object`.
    let entry_locals: Vec<VerificationType> = method.params.iter().map(|p| param_vti(p.ty)).collect();

    let mut g = Gen {
        cp,
        info,
        code: Vec::new(),
        line_numbers: Vec::new(),
        max_stack: 0,
        cur: 0,
        locals: entry_locals.clone(),
        labels: Vec::new(),
        fixups: Vec::new(),
        frames: Vec::new(),
    };

    for stmt in &method.body {
        g.gen_stmt(stmt);
        // Maintain the running assigned-locals snapshot: a top-level declaration
        // brings its local into scope for every subsequent branch's frames. (In
        // this subset such locals are declared with an initializer, so they are
        // definitely assigned from here on.) The push MUST stay *after* gen_stmt:
        // a frame emitted while materializing the declaration's own initializer
        // (e.g. `boolean r = a && b`) snapshots `self.locals` without `r` — that is
        // exactly what makes javac's frame there `append` without the new local.
        if let StmtKind::LocalDecl { ty, .. } = &stmt.kind {
            g.locals.push(local_vti(sema::valtype(*ty)));
        }
    }

    // Every void method ends with an appended `return`, mapped to the closing brace.
    let ret_pc = g.code.len() as u16;
    g.code.push(RETURN);
    g.add_line(ret_pc, method.close_line);

    let live_targets = g.resolve_branches();
    let stack_frames = g.build_frames(&live_targets);

    CfMethod {
        access_flags: 0x0009, // ACC_PUBLIC | ACC_STATIC
        name: method.name.clone(),
        descriptor: descriptor_of(method),
        max_stack: g.max_stack,
        max_locals: info.local_count.max(1),
        code: g.code,
        line_numbers: g.line_numbers,
        entry_locals,
        stack_frames,
    }
}

/// Build the JVM method descriptor from the parsed signature.
fn descriptor_of(method: &Method) -> String {
    let mut d = String::from("(");
    for p in &method.params {
        d.push_str(match p.ty {
            Type::Int => "I",
            Type::Long => "J",
            Type::Float => "F",
            Type::Double => "D",
            Type::Boolean => "Z",
            Type::Char => "C",
            Type::Byte => "B",
            Type::Short => "S",
            Type::StringArray => "[Ljava/lang/String;",
        });
    }
    d.push_str(")V");
    d
}

/// A pending branch: its operand's byte position in `code` (a 2-byte s16 offset,
/// relative to the branch opcode) and the label it targets. Resolved once every
/// label's pc is known.
struct Fixup {
    branch_pc: u32,
    operand_pos: usize,
    label: usize,
}

/// A requested stack-map frame: the verifier state (locals + operand stack) at a
/// branch target, keyed by absolute bytecode offset. The serializer turns these
/// into the minimal frame encodings.
struct FrameReq {
    offset: u16,
    locals: Vec<VerificationType>,
    stack: Vec<VerificationType>,
}

/// javac's `Items.CondItem`, restricted to njavac's side-effect-free boolean
/// subset. Lowering a boolean expression (`gen_cond`) emits every operand load
/// eagerly but leaves the *deciding branch* pending in `opcode`; the not-yet-
/// resolved jump sites are collected in `true_chain`/`false_chain`. Consumers
/// (`gen_if`, `gen_bool_value`) then resolve those chains to concrete pcs. This is
/// the one representation that expresses javac's constant short-circuit collapse
/// (`true || q`, `q && false`, …) — see the `&&`/`||` corpus.
struct CondItem {
    /// The pending deciding branch, or a static verdict.
    opcode: CondOp,
    /// Chains as label ids collecting pending jump sites. `None` = the empty chain
    /// (javac's null): nothing targets it, so resolving it places no frame. A
    /// `Some` chain always has ≥1 live fixup.
    true_chain: Option<usize>,
    false_chain: Option<usize>,
    /// True iff an un-branched boolean 0/1 is currently on the operand stack (the
    /// bare-value leaf sets it; any emitted branch consumes and clears it). The
    /// only item whose materialization needs no diamond.
    value_on_stack: bool,
}

/// The deciding branch of a `CondItem`: a real conditional test (taken when the
/// condition is *true*), or a static verdict mirroring javac's `goto_`/`dontgoto`.
enum CondOp {
    Test(u8), // conditional branch opcode taken when TRUE (ifne / if_icmplt / …)
    Goto,     // statically TRUE
    DontGoto, // statically FALSE
}

impl CondItem {
    /// Statically always-true: an unconditional `goto` sense with no pending
    /// false jumps. Exactly javac's `CondItem.isTrue()`.
    fn is_true(&self) -> bool {
        matches!(self.opcode, CondOp::Goto) && self.false_chain.is_none()
    }
    /// Statically always-false: never jumps true and no pending true jumps.
    fn is_false(&self) -> bool {
        matches!(self.opcode, CondOp::DontGoto) && self.true_chain.is_none()
    }
    /// `!e`: swap the true/false chains and negate the deciding branch.
    fn negate(self) -> CondItem {
        CondItem {
            opcode: match self.opcode {
                CondOp::Goto => CondOp::DontGoto,
                CondOp::DontGoto => CondOp::Goto,
                CondOp::Test(op) => CondOp::Test(negate_op(op)),
            },
            true_chain: self.false_chain,
            false_chain: self.true_chain,
            value_on_stack: self.value_on_stack,
        }
    }
}

/// A statically-true `CondItem` (no code emitted); javac's `goto_` verdict.
fn cond_true() -> CondItem {
    CondItem { opcode: CondOp::Goto, true_chain: None, false_chain: None, value_on_stack: false }
}
/// A statically-false `CondItem` (no code emitted); javac's `dontgoto` verdict.
fn cond_false() -> CondItem {
    CondItem { opcode: CondOp::DontGoto, true_chain: None, false_chain: None, value_on_stack: false }
}

/// Per-method emission state, with a running operand-stack depth (`cur`) tracked
/// in words so category-2 values count as two.
struct Gen<'a> {
    cp: &'a mut ConstantPool,
    info: &'a MethodInfo,
    code: Vec<u8>,
    line_numbers: Vec<(u16, u16)>,
    max_stack: u16,
    cur: u16,
    /// The assigned, in-scope locals in slot order (params first), as verifier
    /// types — the snapshot each branch target's frame captures.
    locals: Vec<VerificationType>,
    /// pc where each label is placed (`u32::MAX` until placed).
    labels: Vec<u32>,
    fixups: Vec<Fixup>,
    frames: Vec<FrameReq>,
}

impl<'a> Gen<'a> {
    fn push(&mut self, w: u16) {
        self.cur += w;
        if self.cur > self.max_stack {
            self.max_stack = self.cur;
        }
    }
    fn pop(&mut self, w: u16) {
        self.cur -= w;
    }

    // -------- control flow / labels / frames --------

    /// Emit one statement. Each statement starts with an empty operand stack; a
    /// leaf statement gets a LineNumberTable entry at its first instruction, while
    /// an `if` places its own entries (condition, then each nested statement).
    fn gen_stmt(&mut self, stmt: &Stmt) {
        self.cur = 0;
        if let StmtKind::If { cond, then_branch, else_branch } = &stmt.kind {
            self.gen_if(stmt.line, cond, then_branch, else_branch.as_deref());
            return;
        }
        let pc = self.code.len() as u16;
        match &stmt.kind {
            StmtKind::LocalDecl { name, init, .. } => {
                if let Some(init) = init {
                    self.store_to(name, init);
                }
            }
            StmtKind::Assign { name, value } => self.store_to(name, value),
            StmtKind::CompoundAssign { name, op, value } => self.gen_compound(name, *op, value),
            StmtKind::Expr(expr) => self.gen_expr_stmt(expr),
            StmtKind::If { .. } => unreachable!("handled above"),
        }
        if self.code.len() as u16 > pc {
            self.add_line(pc, stmt.line);
        }
    }

    /// `if (cond) then [else els]`, a faithful port of javac's `visitIf`. A
    /// whole-constant condition folds away (only the taken arm is emitted, no
    /// branch, no frame — the dead-branch rule); otherwise `gen_cond` lowers the
    /// condition to a `CondItem` and its chains are resolved to the then/else/end
    /// targets. When the condition is statically false only the *then* is dropped
    /// (the else still runs); the trailing `goto`+else block is emitted only when
    /// the else is actually reachable (no spurious `goto`, no dead else).
    fn gen_if(&mut self, line: u16, cond: &Expr, then_b: &[Stmt], else_b: Option<&[Stmt]>) {
        if let Some(taken) = fold_bool(cond) {
            let arm = if taken { Some(then_b) } else { else_b };
            for s in arm.unwrap_or(&[]) {
                self.gen_stmt(s);
            }
            return;
        }

        // The gen_cond path always emits ≥1 instruction (a non-constant operand is
        // present), so the condition's line entry never lands on a phantom pc.
        let cond_pc = self.code.len() as u16;
        self.add_line(cond_pc, line);

        let c = self.gen_cond(cond);
        let is_false = c.is_false();
        let true_chain = c.true_chain;
        let else_chain = self.jump_false(c); // emit the false branch(es); may be None

        if !is_false {
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
                let end = if !is_false { Some(self.branch_to_new(GOTO)) } else { None };
                self.resolve_chain(else_chain);
                for s in els {
                    self.gen_stmt(s);
                }
                if let Some(end) = end {
                    self.resolve_chain(Some(end));
                }
            }
            _ => self.resolve_chain(else_chain),
        }
    }

    /// Lower a boolean expression to a `CondItem` (javac's `genCond`): emit its
    /// operand loads eagerly, leaving only the deciding branch pending. A
    /// whole-constant subtree collapses to a static verdict with no code (this is
    /// how `false && q` / `true || q` drop their dead operand). `&&`/`||` short-
    /// circuit from the *left*: the left's deciding branch is emitted, its non-
    /// deciding outcome falls through into the right operand, and the two chains
    /// are merged (`Code.mergeChains`).
    fn gen_cond(&mut self, e: &Expr) -> CondItem {
        // `fold`'s Logical arm is short-circuit-aware, so this fires only when the
        // left operand decides or the whole tree is constant — never for a live
        // left with a constant right (`q && false`), which must still emit `q`.
        if let Some(c) = fold(e) {
            return if to_i32(c) != 0 { cond_true() } else { cond_false() };
        }
        match e {
            Expr::Not(inner) => self.gen_cond(inner).negate(),
            Expr::Compare { op, left, right } => self.gen_compare_cond(*op, left, right),
            Expr::Logical { op: LogOp::And, left, right } => {
                let lc = self.gen_cond(left);
                if lc.is_false() {
                    return lc; // false && _ : right is dead
                }
                let lt = lc.true_chain;
                let fj = self.jump_false(lc); // emit the left's false branch
                self.resolve_chain(lt); // left-true falls through to the right
                let rc = self.gen_cond(right);
                CondItem {
                    opcode: rc.opcode,
                    value_on_stack: rc.value_on_stack,
                    true_chain: rc.true_chain,
                    false_chain: self.merge_chains(fj, rc.false_chain),
                }
            }
            Expr::Logical { op: LogOp::Or, left, right } => {
                let lc = self.gen_cond(left);
                if lc.is_true() {
                    return lc; // true || _ : right is dead
                }
                let lf = lc.false_chain;
                let tj = self.jump_true(lc);
                self.resolve_chain(lf);
                let rc = self.gen_cond(right);
                CondItem {
                    opcode: rc.opcode,
                    value_on_stack: rc.value_on_stack,
                    true_chain: self.merge_chains(tj, rc.true_chain),
                    false_chain: rc.false_chain,
                }
            }
            // A bare boolean value (a local, or `&`/`|`/`^` on booleans): load its
            // 0/1 onto the stack, pending an `ifne`(true)/`ifeq`(false) test.
            other => {
                self.gen_value(other); // pushes 0/1 (cur += 1)
                CondItem {
                    opcode: CondOp::Test(IFNE),
                    true_chain: None,
                    false_chain: None,
                    value_on_stack: true,
                }
            }
        }
    }

    /// Lower a comparison to a `CondItem`: emit its operands (and the wide
    /// `lcmp`/`fcmp*`/`dcmp*`), but *not* the branch — the deciding test opcode
    /// (true polarity) is returned pending. Its operands are popped when the
    /// branch is finally emitted, in `emit_test_branch`.
    fn gen_compare_cond(&mut self, op: CmpOp, left: &Expr, right: &Expr) -> CondItem {
        let p = sema::binary_promote(sema::type_of(left, self.info), sema::type_of(right, self.info));
        let opcode = match p.stack() {
            StackTy::Int => {
                // javac folds `x <op> 0` to the compare-with-zero opcodes, but only
                // when the literal `0` is the *right* operand.
                if matches!(fold(right), Some(Const::Int(0))) {
                    self.gen_promoted_operand(left, ValType::Int);
                    int_zero_branch(op, true)
                } else {
                    self.gen_promoted_operand(left, ValType::Int);
                    self.gen_promoted_operand(right, ValType::Int);
                    int_icmp_branch(op, true)
                }
            }
            StackTy::Long => {
                self.gen_promoted_operand(left, ValType::Long);
                self.gen_promoted_operand(right, ValType::Long);
                self.code.push(LCMP);
                self.cur -= 3; // two longs (4w) -> one int
                int_zero_branch(op, true)
            }
            StackTy::Float => {
                self.gen_promoted_operand(left, ValType::Float);
                self.gen_promoted_operand(right, ValType::Float);
                self.code.push(if matches!(op, CmpOp::Lt | CmpOp::Le) { FCMPG } else { FCMPL });
                self.cur -= 1; // two floats -> one int
                int_zero_branch(op, true)
            }
            StackTy::Double => {
                self.gen_promoted_operand(left, ValType::Double);
                self.gen_promoted_operand(right, ValType::Double);
                self.code.push(if matches!(op, CmpOp::Lt | CmpOp::Le) { DCMPG } else { DCMPL });
                self.cur -= 3; // two doubles (4w) -> one int
                int_zero_branch(op, true)
            }
        };
        CondItem { opcode: CondOp::Test(opcode), true_chain: None, false_chain: None, value_on_stack: false }
    }

    /// Emit the branch that routes the FALSE outcome of `c` to a chain, returning
    /// it (javac's `CondItem.jumpFalse`). Total: a static verdict emits nothing.
    fn jump_false(&mut self, c: CondItem) -> Option<usize> {
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
                debug_assert_eq!(self.cur, 0, "jump_false goto with non-empty stack");
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
    fn jump_true(&mut self, c: CondItem) -> Option<usize> {
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
                debug_assert_eq!(self.cur, 0, "jump_true goto with non-empty stack");
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
    /// rung; `println(a < b)`/`println(a && b)` stay refused by this assert).
    fn gen_bool_value(&mut self, cond: &Expr) -> ValType {
        assert!(self.cur == 0, "materialized boolean with non-empty operand stack is unsupported");
        let c = self.gen_cond(cond);

        // A bare boolean value already sits on the stack as 0/1, un-branched. All
        // four conjuncts matter: `a && b && c` carries value_on_stack up from its
        // last leaf but has a live false_chain, so it must NOT take this shortcut.
        if c.value_on_stack
            && c.true_chain.is_none()
            && c.false_chain.is_none()
            && matches!(c.opcode, CondOp::Test(_))
        {
            return ValType::Boolean;
        }

        let is_false = c.is_false();
        let is_true = c.is_true();
        let true_chain = c.true_chain;
        let fj = self.jump_false(c);

        if is_false {
            // `q && false`: the residual false branch is already emitted; resolve
            // it here, the value is always 0.
            self.resolve_chain(fj);
            self.code.push(ICONST_0);
            self.push(1);
        } else if is_true {
            // `q || true`: statically true with a residual true branch; resolve it,
            // the value is always 1.
            self.resolve_chain(true_chain);
            self.code.push(ICONST_1);
            self.push(1);
        } else {
            // General true-first diamond.
            self.resolve_chain(true_chain); // true-entry (frame iff a branch lands)
            self.code.push(ICONST_1);
            self.push(1);
            let lmerge = self.branch_to_new(GOTO);
            self.resolve_chain(fj);
            self.cur = 0; // the iconst_1 lives only on the fall-through path
            self.code.push(ICONST_0);
            self.push(1);
            self.place_label(lmerge);
            self.add_frame(vec![VerificationType::Integer]);
        }
        ValType::Boolean
    }

    /// Emit branch opcode `op` to a fresh label and return it as a one-site chain.
    fn branch_to_new(&mut self, op: u8) -> usize {
        let l = self.new_label();
        self.emit_branch_op(op, l);
        l
    }

    /// Emit a conditional *test* branch to a fresh chain and pop its operands (2
    /// for `if_icmp<cond>`, 1 for `if<cond>`/`ifne`/`ifeq`). `GOTO` must NOT route
    /// through here (it pops nothing).
    fn emit_test_branch(&mut self, op: u8) -> usize {
        let l = self.branch_to_new(op);
        self.pop(if (IF_ICMPEQ..=IF_ICMPLE).contains(&op) { 2 } else { 1 });
        l
    }

    /// Merge chain `b` into chain `a` (javac's `Code.mergeChains`): retarget every
    /// pending fixup of `b` to `a`. Fixup order never affects output — all sites of
    /// a merged chain resolve to one pc, frames key by pc, threading keys by target.
    fn merge_chains(&mut self, a: Option<usize>, b: Option<usize>) -> Option<usize> {
        match (a, b) {
            (None, x) | (x, None) => x,
            (Some(a), Some(b)) => {
                for fx in &mut self.fixups {
                    if fx.label == b {
                        fx.label = a;
                    }
                }
                Some(a)
            }
        }
    }

    /// Resolve a chain at the current pc: place its label and request a stack-map
    /// frame — but only when a branch actually targets it (a `Some` chain always
    /// has ≥1 live fixup; `None` resolves to nothing, no frame).
    fn resolve_chain(&mut self, chain: Option<usize>) {
        debug_assert_eq!(self.cur, 0, "chain resolved with non-empty operand stack");
        if let Some(l) = chain {
            self.place_label(l);
            self.add_frame(Vec::new());
        }
    }

    /// Append a LineNumberTable entry, unless it would repeat the previous entry's
    /// line — javac emits an entry only when the source line changes, so several
    /// statements (or a condition and its same-line body) share one entry.
    fn add_line(&mut self, pc: u16, line: u16) {
        if self.line_numbers.last().map(|&(_, l)| l) != Some(line) {
            self.line_numbers.push((pc, line));
        }
    }

    /// Reserve a fresh, not-yet-placed label.
    fn new_label(&mut self) -> usize {
        self.labels.push(u32::MAX);
        self.labels.len() - 1
    }

    /// Bind a label to the current pc.
    fn place_label(&mut self, label: usize) {
        self.labels[label] = self.code.len() as u32;
    }

    /// Emit a branch opcode with a placeholder 2-byte offset, recording a fixup.
    fn emit_branch_op(&mut self, opcode: u8, label: usize) {
        let branch_pc = self.code.len() as u32;
        self.code.push(opcode);
        let operand_pos = self.code.len();
        self.code.push(0);
        self.code.push(0);
        self.fixups.push(Fixup { branch_pc, operand_pos, label });
    }

    /// Request a stack-map frame at the current pc, capturing the live-locals
    /// snapshot and the given operand-stack state.
    fn add_frame(&mut self, stack: Vec<VerificationType>) {
        self.frames.push(FrameReq {
            offset: self.code.len() as u16,
            locals: self.locals.clone(),
            stack,
        });
    }

    /// Backpatch every branch's 2-byte offset now that all labels are placed, and
    /// return the set of pcs that remain live jump targets. javac threads a jump
    /// whose target is an unconditional `goto` straight to that goto's ultimate
    /// destination — so `goto L; L: goto M` becomes a jump to `M`, and `L` (now
    /// reached only by fall-through) no longer carries a stack-map frame.
    fn resolve_branches(&mut self) -> Vec<u16> {
        let targets: Vec<u16> =
            self.fixups.iter().map(|fx| self.thread_target(fx.label) as u16).collect();
        for (fx_i, &target) in targets.iter().enumerate() {
            let (operand_pos, branch_pc) = {
                let fx = &self.fixups[fx_i];
                (fx.operand_pos, fx.branch_pc)
            };
            let offset = (target as i32 - branch_pc as i32) as i16;
            let [hi, lo] = offset.to_be_bytes();
            self.code[operand_pos] = hi;
            self.code[operand_pos + 1] = lo;
        }
        targets
    }

    /// Follow a chain of unconditional `goto`s from `label` to the final pc.
    fn thread_target(&self, label: usize) -> u32 {
        let mut pc = self.labels[label];
        debug_assert!(pc != u32::MAX, "unplaced branch label");
        for _ in 0..=self.fixups.len() {
            if self.code.get(pc as usize) != Some(&GOTO) {
                break;
            }
            // The goto at `pc` is itself a fixup; follow the label it targets.
            match self.fixups.iter().find(|fx| fx.branch_pc == pc) {
                Some(fx) => {
                    let next = self.labels[fx.label];
                    if next == pc {
                        break; // self-loop guard
                    }
                    pc = next;
                }
                None => break,
            }
        }
        pc
    }

    /// Collect the requested frames into serializer-ready form: one per distinct
    /// pc that survives as a real jump target (post-threading), in offset order.
    fn build_frames(&mut self, live_targets: &[u16]) -> Vec<StackFrame> {
        let live: std::collections::HashSet<u16> = live_targets.iter().copied().collect();
        self.frames.sort_by_key(|f| f.offset);
        let mut out: Vec<StackFrame> = Vec::new();
        for f in &self.frames {
            if !live.contains(&f.offset) {
                continue; // a merge that threading turned into pure fall-through
            }
            if out.last().is_some_and(|p| p.offset == f.offset) {
                continue; // multiple branches merge at one pc
            }
            out.push(StackFrame { offset: f.offset, locals: f.locals.clone(), stack: f.stack.clone() });
        }
        out
    }

    // -------- statements --------

    /// `System.out.println(arg)`.
    fn gen_expr_stmt(&mut self, expr: &Expr) {
        match expr {
            Expr::Println(arg) => self.gen_println(arg),
            other => panic!("unsupported expression statement: {:?}", DebugExpr(other)),
        }
    }

    fn gen_println(&mut self, arg: &Expr) {
        let field = self.cp.fieldref("java/lang/System", "out", "Ljava/io/PrintStream;");
        self.code.push(GETSTATIC);
        push_u16(&mut self.code, field);
        self.push(1); // PrintStream objectref

        let ty = self.gen_value(arg);
        let desc = match ty {
            ValType::Int | ValType::Byte | ValType::Short => "(I)V",
            ValType::Long => "(J)V",
            ValType::Float => "(F)V",
            ValType::Double => "(D)V",
            ValType::Char => "(C)V",
            ValType::Boolean => "(Z)V",
            ValType::String => "(Ljava/lang/String;)V",
        };
        let method = self.cp.methodref("java/io/PrintStream", "println", desc);
        self.code.push(INVOKEVIRTUAL);
        push_u16(&mut self.code, method);
        self.pop(1 + ty.width()); // objectref + arg consumed, void return
    }

    /// Assign `value` into local `name`, coercing to the local's declared type.
    fn store_to(&mut self, name: &str, value: &Expr) {
        let target = self.info.ty(name);
        let slot = self.info.slot(name);
        self.gen_coerced(value, target);
        self.emit_store(slot, target);
    }

    /// Compound assignment `name op= value` (also `++`/`--`, which arrive as
    /// `op ∈ {Add,Sub}` with `value == 1`).
    fn gen_compound(&mut self, name: &str, op: BinOp, value: &Expr) {
        let target = self.info.ty(name);
        let slot = self.info.slot(name);

        // iinc fast path: an `int` target, `+=`/`-=`, an int-family constant delta
        // that keeps the expression in `int`, and a slot/delta that fits.
        if target == ValType::Int
            && matches!(op, BinOp::Add | BinOp::Sub)
            && matches!(sema::type_of(value, self.info), ValType::Int | ValType::Byte | ValType::Short | ValType::Char)
        {
            if let Some(c) = fold(value) {
                let k = to_i32(c);
                let delta = if op == BinOp::Add { k } else { k.wrapping_neg() };
                if slot <= 0xff && (-128..=127).contains(&delta) {
                    self.code.push(IINC);
                    self.code.push(slot as u8);
                    self.code.push(delta as i8 as u8);
                    return;
                } else if (-32768..=32767).contains(&delta) {
                    self.code.push(WIDE);
                    self.code.push(IINC);
                    push_u16(&mut self.code, slot);
                    push_u16(&mut self.code, delta as i16 as u16);
                    return;
                } else {
                    // Constant delta overflowing iinc_w: javac emits the POSITIVE
                    // magnitude and chooses the operator by the delta's sign, so
                    // `x -= -32768` becomes `iload; ldc 32768; iadd; istore` (not
                    // `sipush -32768; isub`) and `x += -40000` becomes `… isub`.
                    // (This also lets `+= n` and `-= -n` share one pool entry.)
                    self.emit_load(slot, ValType::Int);
                    let (mag, add) = int_delta_magnitude(delta);
                    self.emit_int_const(mag);
                    self.push(1);
                    self.code.push(if add { IADD } else { ISUB });
                    self.pop(1);
                    self.emit_store(slot, ValType::Int);
                    return;
                }
            }
        }

        // General form: name = (target)(name op value), computed in the promoted
        // type `p`, then narrowed back to `target`.
        let p = if op.is_shift() {
            sema::unary_promote(target)
        } else {
            sema::binary_promote(target, sema::type_of(value, self.info))
        };
        self.emit_load(slot, target);
        self.emit_convert(target, p);
        if op.is_shift() {
            let at = self.gen_value(value);
            if at.stack() == StackTy::Long {
                self.code.push(L2I);
                self.pop(1);
            }
            self.emit_shift(p, op);
        } else if let Some(delta) = int_additive_const_delta(op, p, value) {
            // javac normalizes an additive *constant* on an int-family target to a
            // non-negative magnitude, choosing the operator by the delta's sign — so
            // `char v -= -100` is `bipush 100; iadd` (then i2c), never `bipush -100;
            // isub`. Same split as the iinc-overflow path above; int-family only
            // (a `long`/`float`/`double` target keeps the raw `lsub`/`dsub`/`fsub`).
            let (mag, add) = int_delta_magnitude(delta);
            self.emit_int_const(mag);
            self.push(1);
            self.code.push(if add { IADD } else { ISUB });
            self.pop(1);
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
    fn gen_coerced(&mut self, value: &Expr, target: ValType) {
        if let Some(c) = fold(value) {
            self.load_const(const_convert(c, target), target);
        } else {
            let s = self.gen_nonconst(value);
            self.emit_convert(s, target);
        }
    }

    /// Emit `expr` leaving its natural-typed value on the stack; returns the type.
    fn gen_value(&mut self, expr: &Expr) -> ValType {
        // A string literal is the one non-numeric value form (only ever a
        // `println` argument); it loads via `ldc` of a `String` constant.
        if let Expr::StringLit(s) = expr {
            let idx = self.cp.string(s);
            self.emit_ldc(idx);
            self.push(1);
            return ValType::String;
        }
        if let Some(c) = fold(expr) {
            let t = sema::type_of(expr, self.info);
            self.load_const(const_convert(c, t), t);
            t
        } else {
            self.gen_nonconst(expr)
        }
    }

    /// Emit `expr` as an operand of a binary op whose promoted type is `p`,
    /// widening to `p`. A constant is loaded already in `p`; a non-constant is
    /// emitted in its own type then converted.
    fn gen_promoted_operand(&mut self, expr: &Expr, p: ValType) {
        if let Some(c) = fold(expr) {
            self.load_const(const_convert(c, p), p);
        } else {
            let s = self.gen_nonconst(expr);
            self.emit_convert(s, p);
        }
    }

    /// Emit a non-constant expression, returning its static type.
    fn gen_nonconst(&mut self, expr: &Expr) -> ValType {
        match expr {
            Expr::Name(n) => {
                let ty = self.info.ty(n);
                self.emit_load(self.info.slot(n), ty);
                ty
            }
            Expr::Neg(e) => {
                self.gen_value(e);
                let p = sema::unary_promote(sema::type_of(e, self.info));
                self.code.push(neg_op(p.stack()));
                p
            }
            Expr::BitNot(e) => {
                self.gen_value(e);
                let p = sema::unary_promote(sema::type_of(e, self.info));
                self.emit_bitnot(p);
                p
            }
            Expr::Cast { ty, expr } => {
                let s = self.gen_value(expr);
                let target = sema::valtype(*ty);
                self.emit_convert(s, target);
                target
            }
            Expr::Binary { op, left, right } => self.gen_binary(*op, left, right),
            Expr::Compare { .. } | Expr::Not(_) | Expr::Logical { .. } => self.gen_bool_value(expr),
            other => panic!("not a value expression: {:?}", DebugExpr(other)),
        }
    }

    fn gen_binary(&mut self, op: BinOp, left: &Expr, right: &Expr) -> ValType {
        let lt = sema::type_of(left, self.info);
        let rt = sema::type_of(right, self.info);

        // `&`/`|`/`^` on two booleans: int opcode, boolean result.
        if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor)
            && lt == ValType::Boolean
            && rt == ValType::Boolean
        {
            self.gen_value(left);
            self.gen_value(right);
            self.emit_binop(ValType::Int, op);
            return ValType::Boolean;
        }

        if op.is_shift() {
            let result = sema::unary_promote(lt);
            self.gen_promoted_operand(left, result);
            let at = self.gen_value(right);
            if at.stack() == StackTy::Long {
                self.code.push(L2I);
                self.pop(1); // long amount narrowed to int
            }
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
    fn load_const(&mut self, c: Const, ty: ValType) {
        match ty.stack() {
            StackTy::Int => self.emit_int_const(to_i32(c)),
            StackTy::Long => self.emit_long_const(to_i64(c)),
            StackTy::Float => self.emit_float_const(to_f32(c)),
            StackTy::Double => self.emit_double_const(to_f64(c)),
        }
        self.push(ty.width());
    }

    /// Load an `int` constant with the tightest opcode javac would choose.
    fn emit_int_const(&mut self, v: i32) {
        match v {
            -1 => self.code.push(ICONST_M1),
            0..=5 => self.code.push(ICONST_0 + v as u8),
            -128..=127 => {
                self.code.push(BIPUSH);
                self.code.push(v as u8);
            }
            -32768..=32767 => {
                self.code.push(SIPUSH);
                push_u16(&mut self.code, v as u16);
            }
            _ => {
                let idx = self.cp.integer(v);
                self.emit_ldc(idx);
            }
        }
    }

    fn emit_long_const(&mut self, v: i64) {
        match v {
            0 => self.code.push(LCONST_0),
            1 => self.code.push(LCONST_1),
            _ => {
                let idx = self.cp.long(v);
                self.code.push(LDC2_W);
                push_u16(&mut self.code, idx);
            }
        }
    }

    fn emit_float_const(&mut self, v: f32) {
        // Compare by bit pattern: only +0.0f/+1.0f/+2.0f get the const opcodes,
        // so -0.0f (and NaN) fall through to the pool.
        match v.to_bits() {
            b if b == 0.0f32.to_bits() => self.code.push(FCONST_0),
            b if b == 1.0f32.to_bits() => self.code.push(FCONST_1),
            b if b == 2.0f32.to_bits() => self.code.push(FCONST_2),
            _ => {
                let idx = self.cp.float(v);
                self.emit_ldc(idx);
            }
        }
    }

    fn emit_double_const(&mut self, v: f64) {
        match v.to_bits() {
            b if b == 0.0f64.to_bits() => self.code.push(DCONST_0),
            b if b == 1.0f64.to_bits() => self.code.push(DCONST_1),
            _ => {
                let idx = self.cp.double(v);
                self.code.push(LDC2_W);
                push_u16(&mut self.code, idx);
            }
        }
    }

    /// `ldc`/`ldc_w` of a single-word pool entry (Integer/Float/String).
    fn emit_ldc(&mut self, idx: u16) {
        if idx <= 0xff {
            self.code.push(LDC);
            self.code.push(idx as u8);
        } else {
            self.code.push(LDC_W);
            push_u16(&mut self.code, idx);
        }
    }

    fn emit_load(&mut self, slot: u16, ty: ValType) {
        let (short0, wide) = load_ops(ty);
        if slot <= 3 {
            self.code.push(short0 + slot as u8);
        } else {
            self.code.push(wide);
            self.code.push(slot as u8);
        }
        self.push(ty.width());
    }

    fn emit_store(&mut self, slot: u16, ty: ValType) {
        let (short0, wide) = store_ops(ty);
        if slot <= 3 {
            self.code.push(short0 + slot as u8);
        } else {
            self.code.push(wide);
            self.code.push(slot as u8);
        }
        self.pop(ty.width());
    }

    fn emit_binop(&mut self, p: ValType, op: BinOp) {
        self.code.push(binop_op(p.stack(), op));
        self.pop(p.width()); // two operands (2w) collapse to one (w)
    }

    fn emit_shift(&mut self, result: ValType, op: BinOp) {
        self.code.push(shift_op(result.stack(), op));
        self.pop(1); // value(w) + amount(1) -> value(w)
    }

    /// `~x` == `x ^ -1`, with the `-1` loaded per the value's type.
    fn emit_bitnot(&mut self, p: ValType) {
        match p.stack() {
            StackTy::Long => {
                let idx = self.cp.long(-1);
                self.code.push(LDC2_W);
                push_u16(&mut self.code, idx);
                self.push(2);
                self.code.push(LXOR);
                self.pop(2);
            }
            _ => {
                self.code.push(ICONST_M1);
                self.push(1);
                self.code.push(IXOR);
                self.pop(1);
            }
        }
    }

    /// Emit the conversion from `from` to `to`, if any, adjusting the stack.
    fn emit_convert(&mut self, from: ValType, to: ValType) {
        if from == to {
            return;
        }
        let fs = from.stack();
        if matches!(to, ValType::Byte | ValType::Short | ValType::Char) {
            // Bring the value to the `int` computational type first.
            match fs {
                StackTy::Long => self.code.push(L2I),
                StackTy::Float => self.code.push(F2I),
                StackTy::Double => self.code.push(D2I),
                StackTy::Int => {}
            }
            // Narrow within int-family only when `from` is wider than `to`.
            let cur_ty = if fs == StackTy::Int { from } else { ValType::Int };
            if let Some(op) = subint_narrow_op(cur_ty, to) {
                self.code.push(op);
            }
        } else if fs != to.stack() {
            self.code.push(cross_conv_op(fs, to.stack()));
        }
        // Net stack change: one value of `from.width()` becomes one of `to.width()`.
        let delta = to.width() as i32 - from.width() as i32;
        self.cur = (self.cur as i32 + delta) as u16;
        if self.cur > self.max_stack {
            self.max_stack = self.cur;
        }
    }
}

// ---- opcode/table helpers ----

/// (slot-0 short opcode, wide opcode) for a load of type `ty`.
fn load_ops(ty: ValType) -> (u8, u8) {
    match ty.stack() {
        StackTy::Int => (ILOAD_0, ILOAD),
        StackTy::Long => (LLOAD_0, LLOAD),
        StackTy::Float => (FLOAD_0, FLOAD),
        StackTy::Double => (DLOAD_0, DLOAD),
    }
}

fn store_ops(ty: ValType) -> (u8, u8) {
    match ty.stack() {
        StackTy::Int => (ISTORE_0, ISTORE),
        StackTy::Long => (LSTORE_0, LSTORE),
        StackTy::Float => (FSTORE_0, FSTORE),
        StackTy::Double => (DSTORE_0, DSTORE),
    }
}

fn binop_op(p: StackTy, op: BinOp) -> u8 {
    match (p, op) {
        (StackTy::Int, BinOp::Add) => IADD,
        (StackTy::Int, BinOp::Sub) => ISUB,
        (StackTy::Int, BinOp::Mul) => IMUL,
        (StackTy::Int, BinOp::Div) => IDIV,
        (StackTy::Int, BinOp::Rem) => IREM,
        (StackTy::Int, BinOp::And) => IAND,
        (StackTy::Int, BinOp::Or) => IOR,
        (StackTy::Int, BinOp::Xor) => IXOR,
        (StackTy::Long, BinOp::Add) => LADD,
        (StackTy::Long, BinOp::Sub) => LSUB,
        (StackTy::Long, BinOp::Mul) => LMUL,
        (StackTy::Long, BinOp::Div) => LDIV,
        (StackTy::Long, BinOp::Rem) => LREM,
        (StackTy::Long, BinOp::And) => LAND,
        (StackTy::Long, BinOp::Or) => LOR,
        (StackTy::Long, BinOp::Xor) => LXOR,
        (StackTy::Float, BinOp::Add) => FADD,
        (StackTy::Float, BinOp::Sub) => FSUB,
        (StackTy::Float, BinOp::Mul) => FMUL,
        (StackTy::Float, BinOp::Div) => FDIV,
        (StackTy::Float, BinOp::Rem) => FREM,
        (StackTy::Double, BinOp::Add) => DADD,
        (StackTy::Double, BinOp::Sub) => DSUB,
        (StackTy::Double, BinOp::Mul) => DMUL,
        (StackTy::Double, BinOp::Div) => DDIV,
        (StackTy::Double, BinOp::Rem) => DREM,
        (p, op) => panic!("invalid binary op {op:?} for {p:?}"),
    }
}

fn shift_op(result: StackTy, op: BinOp) -> u8 {
    match (result, op) {
        (StackTy::Int, BinOp::Shl) => ISHL,
        (StackTy::Int, BinOp::Shr) => ISHR,
        (StackTy::Int, BinOp::UShr) => IUSHR,
        (StackTy::Long, BinOp::Shl) => LSHL,
        (StackTy::Long, BinOp::Shr) => LSHR,
        (StackTy::Long, BinOp::UShr) => LUSHR,
        (r, op) => panic!("invalid shift op {op:?} for {r:?}"),
    }
}

fn neg_op(p: StackTy) -> u8 {
    match p {
        StackTy::Int => INEG,
        StackTy::Long => LNEG,
        StackTy::Float => FNEG,
        StackTy::Double => DNEG,
    }
}

/// The single conversion opcode between two *different* computational types.
fn cross_conv_op(from: StackTy, to: StackTy) -> u8 {
    use StackTy::*;
    match (from, to) {
        (Int, Long) => I2L,
        (Int, Float) => I2F,
        (Int, Double) => I2D,
        (Long, Int) => L2I,
        (Long, Float) => L2F,
        (Long, Double) => L2D,
        (Float, Int) => F2I,
        (Float, Long) => F2L,
        (Float, Double) => F2D,
        (Double, Int) => D2I,
        (Double, Long) => D2L,
        (Double, Float) => D2F,
        (a, b) => panic!("no conversion {a:?} -> {b:?}"),
    }
}

/// Two-operand int comparison branch (`if_icmp*`). With `jump_when == false` the
/// opcode is the *negation* of `op` (branch away when the comparison is false, as
/// javac emits an `if` condition); with `true` it is `op` itself.
fn int_icmp_branch(op: CmpOp, jump_when: bool) -> u8 {
    use CmpOp::*;
    match (op, jump_when) {
        (Lt, false) => IF_ICMPGE, (Lt, true) => IF_ICMPLT,
        (Le, false) => IF_ICMPGT, (Le, true) => IF_ICMPLE,
        (Gt, false) => IF_ICMPLE, (Gt, true) => IF_ICMPGT,
        (Ge, false) => IF_ICMPLT, (Ge, true) => IF_ICMPGE,
        (Eq, false) => IF_ICMPNE, (Eq, true) => IF_ICMPEQ,
        (Ne, false) => IF_ICMPEQ, (Ne, true) => IF_ICMPNE,
    }
}

/// Single-operand compare-with-zero branch (`if*`), used for `x <op> 0` and, on
/// the result of `lcmp`/`fcmp*`/`dcmp*`, for every wide-type comparison. Same
/// negation convention as [`int_icmp_branch`].
fn int_zero_branch(op: CmpOp, jump_when: bool) -> u8 {
    use CmpOp::*;
    match (op, jump_when) {
        (Lt, false) => IFGE, (Lt, true) => IFLT,
        (Le, false) => IFGT, (Le, true) => IFLE,
        (Gt, false) => IFLE, (Gt, true) => IFGT,
        (Ge, false) => IFLT, (Ge, true) => IFGE,
        (Eq, false) => IFNE, (Eq, true) => IFEQ,
        (Ne, false) => IFEQ, (Ne, true) => IFNE,
    }
}

/// Involution over the twelve conditional-branch opcodes: the branch taken when
/// the *negated* condition holds. Kept consistent with `int_icmp_branch`/
/// `int_zero_branch` by `assert_negate_op_consistent` — this is the highest-blast-
/// radius helper (a drift here silently breaks every comparison fixture), so it is
/// derived and debug-checked rather than trusted.
fn negate_op(op: u8) -> u8 {
    match op {
        IFEQ => IFNE, IFNE => IFEQ,
        IFLT => IFGE, IFGE => IFLT,
        IFGT => IFLE, IFLE => IFGT,
        IF_ICMPEQ => IF_ICMPNE, IF_ICMPNE => IF_ICMPEQ,
        IF_ICMPLT => IF_ICMPGE, IF_ICMPGE => IF_ICMPLT,
        IF_ICMPGT => IF_ICMPLE, IF_ICMPLE => IF_ICMPGT,
        other => panic!("negate_op: not a conditional branch opcode {other:#x}"),
    }
}

/// Debug guard: `negate_op` must invert both branch-opcode tables and be an
/// involution, so replacing a `(op, false)` call with `negate_op((op, true))` is
/// byte-neutral. Run once per `generate()` under `debug_assertions`.
#[cfg(debug_assertions)]
fn assert_negate_op_consistent() {
    use CmpOp::*;
    for op in [Lt, Le, Gt, Ge, Eq, Ne] {
        debug_assert_eq!(negate_op(int_icmp_branch(op, true)), int_icmp_branch(op, false));
        debug_assert_eq!(negate_op(int_zero_branch(op, true)), int_zero_branch(op, false));
        debug_assert_eq!(negate_op(negate_op(int_icmp_branch(op, true))), int_icmp_branch(op, true));
        debug_assert_eq!(negate_op(negate_op(int_zero_branch(op, true))), int_zero_branch(op, true));
    }
}

/// The verifier type of a method parameter.
fn param_vti(ty: Type) -> VerificationType {
    match ty {
        Type::Long => VerificationType::Long,
        Type::Float => VerificationType::Float,
        Type::Double => VerificationType::Double,
        Type::StringArray => VerificationType::Object("[Ljava/lang/String;".to_string()),
        // int/boolean/char/byte/short all verify as int.
        _ => VerificationType::Integer,
    }
}

/// The verifier type of a local of value type `t` (the sub-int types are `int`).
fn local_vti(t: ValType) -> VerificationType {
    match t {
        ValType::Long => VerificationType::Long,
        ValType::Float => VerificationType::Float,
        ValType::Double => VerificationType::Double,
        _ => VerificationType::Integer,
    }
}

/// The `i2b`/`i2s`/`i2c` needed to narrow an int-computational value of type
/// `cur` to sub-int `to`, or `None` when `cur` already fits `to`.
fn subint_narrow_op(cur: ValType, to: ValType) -> Option<u8> {
    match to {
        ValType::Byte => (cur != ValType::Byte).then_some(I2B),
        ValType::Short => (!matches!(cur, ValType::Byte | ValType::Short)).then_some(I2S),
        ValType::Char => (cur != ValType::Char).then_some(I2C),
        _ => None,
    }
}

// ---- constant folding ----

/// Evaluate a maximal constant subtree to a single typed value, or `None` if any
/// leaf is a local. Uses wrapping integer arithmetic and exact IEEE-754 float
/// arithmetic with JLS shift masking, so a folded constant is bit-identical to
/// what the unfolded bytecode would compute.
fn fold(expr: &Expr) -> Option<Const> {
    Some(match expr {
        Expr::IntLit(v) => Const::Int(*v),
        Expr::LongLit(v) => Const::Long(*v),
        Expr::FloatLit(v) => Const::Float(*v),
        Expr::DoubleLit(v) => Const::Double(*v),
        Expr::BoolLit(b) => Const::Int(*b as i32),
        Expr::CharLit(v) => Const::Int(*v as i32),
        Expr::StringLit(_) | Expr::Name(_) | Expr::Println(_) => return None,
        Expr::Neg(e) => neg_const(fold(e)?),
        Expr::BitNot(e) => bitnot_const(fold(e)?),
        Expr::Not(e) => Const::Int((to_i32(fold(e)?) == 0) as i32),
        Expr::Cast { ty, expr } => const_convert(fold(expr)?, sema::valtype(*ty)),
        Expr::Binary { op, left, right } => eval_binary(*op, fold(left)?, fold(right)?),
        Expr::Compare { op, left, right } => {
            Const::Int(eval_compare(*op, fold(left)?, fold(right)?) as i32)
        }
        // `&&`/`||` are constant only via short-circuit from the LEFT. A non-constant
        // left means the whole is NOT a compile-time constant even when the tree is
        // statically decided (`q && false`) — the left must still be evaluated, so we
        // return `None` and let `gen_cond` emit it. When the left decides, return its
        // verdict WITHOUT folding the right; otherwise the tree reduces to the right.
        Expr::Logical { op, left, right } => {
            let lb = to_i32(fold(left)?) != 0;
            match op {
                LogOp::And if !lb => Const::Int(0),                  // false && _ -> false
                LogOp::Or if lb => Const::Int(1),                    // true  || _ -> true
                _ => Const::Int((to_i32(fold(right)?) != 0) as i32), // reduces to the right
            }
        }
    })
}

/// Whether `cond` is a compile-time constant boolean, and its value — javac's
/// dead-branch predicate. A boolean literal or a comparison/logical expression
/// over constants folds; anything reading a (non-`final`) local does not.
fn fold_bool(cond: &Expr) -> Option<bool> {
    fold(cond).map(|c| to_i32(c) != 0)
}

/// Evaluate a constant comparison, with binary numeric promotion. Float/double
/// use IEEE ordering (a `NaN` operand makes every ordering and `==` false),
/// matching the `fcmp`/`dcmp` a non-folded comparison would run.
fn eval_compare(op: CmpOp, l: Const, r: Const) -> bool {
    match promote_const(l, r) {
        StackTy::Int => compare_vals(op, to_i32(l), to_i32(r)),
        StackTy::Long => compare_vals(op, to_i64(l), to_i64(r)),
        StackTy::Float => compare_vals(op, to_f32(l), to_f32(r)),
        StackTy::Double => compare_vals(op, to_f64(l), to_f64(r)),
    }
}

fn compare_vals<T: PartialOrd>(op: CmpOp, a: T, b: T) -> bool {
    match op {
        CmpOp::Lt => a < b,
        CmpOp::Le => a <= b,
        CmpOp::Gt => a > b,
        CmpOp::Ge => a >= b,
        CmpOp::Eq => a == b,
        CmpOp::Ne => a != b,
    }
}

fn neg_const(c: Const) -> Const {
    match c {
        Const::Int(v) => Const::Int(v.wrapping_neg()),
        Const::Long(v) => Const::Long(v.wrapping_neg()),
        Const::Float(v) => Const::Float(-v),
        Const::Double(v) => Const::Double(-v),
    }
}

fn bitnot_const(c: Const) -> Const {
    match c {
        Const::Int(v) => Const::Int(!v),
        Const::Long(v) => Const::Long(!v),
        _ => panic!("~ on a non-integral constant"),
    }
}

fn eval_binary(op: BinOp, l: Const, r: Const) -> Const {
    if op.is_shift() {
        // Shift distance masked with the JLS width; left operand keeps its type.
        return match l {
            Const::Long(v) => {
                let s = (to_i32(r) & 63) as u32;
                Const::Long(match op {
                    BinOp::Shl => v.wrapping_shl(s),
                    BinOp::Shr => v.wrapping_shr(s),
                    BinOp::UShr => ((v as u64).wrapping_shr(s)) as i64,
                    _ => unreachable!(),
                })
            }
            _ => {
                let v = to_i32(l);
                let s = (to_i32(r) & 31) as u32;
                Const::Int(match op {
                    BinOp::Shl => v.wrapping_shl(s),
                    BinOp::Shr => v.wrapping_shr(s),
                    BinOp::UShr => ((v as u32).wrapping_shr(s)) as i32,
                    _ => unreachable!(),
                })
            }
        };
    }
    match promote_const(l, r) {
        StackTy::Int => Const::Int(int_op(op, to_i32(l), to_i32(r))),
        StackTy::Long => Const::Long(long_op(op, to_i64(l), to_i64(r))),
        StackTy::Float => Const::Float(float_op(op, to_f32(l), to_f32(r))),
        StackTy::Double => Const::Double(double_op(op, to_f64(l), to_f64(r))),
    }
}

fn int_op(op: BinOp, a: i32, b: i32) -> i32 {
    match op {
        BinOp::Add => a.wrapping_add(b),
        BinOp::Sub => a.wrapping_sub(b),
        BinOp::Mul => a.wrapping_mul(b),
        BinOp::Div => a.wrapping_div(b),
        BinOp::Rem => a.wrapping_rem(b),
        BinOp::And => a & b,
        BinOp::Or => a | b,
        BinOp::Xor => a ^ b,
        _ => unreachable!("shift handled separately"),
    }
}

fn long_op(op: BinOp, a: i64, b: i64) -> i64 {
    match op {
        BinOp::Add => a.wrapping_add(b),
        BinOp::Sub => a.wrapping_sub(b),
        BinOp::Mul => a.wrapping_mul(b),
        BinOp::Div => a.wrapping_div(b),
        BinOp::Rem => a.wrapping_rem(b),
        BinOp::And => a & b,
        BinOp::Or => a | b,
        BinOp::Xor => a ^ b,
        _ => unreachable!("shift handled separately"),
    }
}

fn float_op(op: BinOp, a: f32, b: f32) -> f32 {
    match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => a / b,
        BinOp::Rem => a % b,
        _ => panic!("invalid float op {op:?}"),
    }
}

fn double_op(op: BinOp, a: f64, b: f64) -> f64 {
    match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => a / b,
        BinOp::Rem => a % b,
        _ => panic!("invalid double op {op:?}"),
    }
}

/// Binary numeric promotion at the constant level.
fn promote_const(l: Const, r: Const) -> StackTy {
    let rank = |c: &Const| match c {
        Const::Int(_) => 0,
        Const::Long(_) => 1,
        Const::Float(_) => 2,
        Const::Double(_) => 3,
    };
    match rank(&l).max(rank(&r)) {
        0 => StackTy::Int,
        1 => StackTy::Long,
        2 => StackTy::Float,
        _ => StackTy::Double,
    }
}

/// Convert a constant to the value it becomes when cast/assigned to `to`, using
/// Java's narrowing/widening semantics (Rust `as` matches JVM `d2i`/`l2i`/etc.).
fn const_convert(c: Const, to: ValType) -> Const {
    match to {
        ValType::Int | ValType::Boolean => Const::Int(to_i32(c)),
        ValType::Long => Const::Long(to_i64(c)),
        ValType::Float => Const::Float(to_f32(c)),
        ValType::Double => Const::Double(to_f64(c)),
        ValType::Byte => Const::Int((to_i32(c) as i8) as i32),
        ValType::Short => Const::Int((to_i32(c) as i16) as i32),
        ValType::Char => Const::Int((to_i32(c) as u16) as i32),
        ValType::String => c,
    }
}

/// The signed increment of an int-family additive compound-assign with a *constant*
/// RHS (`+= k` → `k`, `-= k` → `-k`), or `None` when javac's magnitude normalization
/// does not apply: a non-int-family promoted type (`long`/`float`/`double` keep the
/// raw `lsub`/…), a non-additive op, or a non-constant RHS.
fn int_additive_const_delta(op: BinOp, p: ValType, value: &Expr) -> Option<i32> {
    if p.stack() != StackTy::Int || !matches!(op, BinOp::Add | BinOp::Sub) {
        return None;
    }
    let k = to_i32(fold(value)?);
    Some(if op == BinOp::Add { k } else { k.wrapping_neg() })
}

/// javac loads an int increment as a non-negative magnitude and picks the operator by
/// sign: `(|delta|, is_add)` — `iadd` for `delta ≥ 0`, `isub` for `delta < 0`.
/// `i32::MIN` has no representable magnitude, so it stays `iadd i32::MIN`.
fn int_delta_magnitude(delta: i32) -> (i32, bool) {
    if delta >= 0 {
        (delta, true)
    } else {
        (delta.wrapping_neg(), delta == i32::MIN)
    }
}

fn to_i32(c: Const) -> i32 {
    match c {
        Const::Int(v) => v,
        Const::Long(v) => v as i32,
        Const::Float(v) => v as i32,
        Const::Double(v) => v as i32,
    }
}
fn to_i64(c: Const) -> i64 {
    match c {
        Const::Int(v) => v as i64,
        Const::Long(v) => v,
        Const::Float(v) => v as i64,
        Const::Double(v) => v as i64,
    }
}
fn to_f32(c: Const) -> f32 {
    match c {
        Const::Int(v) => v as f32,
        Const::Long(v) => v as f32,
        Const::Float(v) => v,
        Const::Double(v) => v as f32,
    }
}
fn to_f64(c: Const) -> f64 {
    match c {
        Const::Int(v) => v as f64,
        Const::Long(v) => v as f64,
        Const::Float(v) => v as f64,
        Const::Double(v) => v,
    }
}

fn push_u16(code: &mut Vec<u8>, v: u16) {
    code.extend_from_slice(&v.to_be_bytes());
}

/// Minimal `Debug` shim for panic messages, since `ast::Expr` does not derive it.
struct DebugExpr<'a>(&'a Expr);
impl std::fmt::Debug for DebugExpr<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.0 {
            Expr::IntLit(_) => "IntLit",
            Expr::LongLit(_) => "LongLit",
            Expr::FloatLit(_) => "FloatLit",
            Expr::DoubleLit(_) => "DoubleLit",
            Expr::BoolLit(_) => "BoolLit",
            Expr::CharLit(_) => "CharLit",
            Expr::StringLit(_) => "StringLit",
            Expr::Name(_) => "Name",
            Expr::Neg(_) => "Neg",
            Expr::BitNot(_) => "BitNot",
            Expr::Not(_) => "Not",
            Expr::Cast { .. } => "Cast",
            Expr::Binary { .. } => "Binary",
            Expr::Compare { .. } => "Compare",
            Expr::Logical { .. } => "Logical",
            Expr::Println(_) => "Println",
        };
        f.write_str(kind)
    }
}
