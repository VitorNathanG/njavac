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

use crate::ast::{BinOp, Class, Expr, Method, StmtKind, Type};
use crate::classfile::{ClassFile, ConstantPool, Method as CfMethod};
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
    }
}

/// Emit one method body.
fn gen_method(cp: &mut ConstantPool, method: &Method, info: &MethodInfo) -> CfMethod {
    let mut g = Gen { cp, info, code: Vec::new(), line_numbers: Vec::new(), max_stack: 0, cur: 0 };

    for stmt in &method.body {
        // Record the pc; attach a LineNumberTable entry only if the statement
        // actually emits code (a bare `int x;` emits nothing and gets no entry).
        let pc = g.code.len() as u16;
        g.cur = 0;
        match &stmt.kind {
            StmtKind::LocalDecl { name, init, .. } => {
                if let Some(init) = init {
                    g.store_to(name, init);
                }
            }
            StmtKind::Assign { name, value } => {
                g.store_to(name, value);
            }
            StmtKind::CompoundAssign { name, op, value } => {
                g.gen_compound(name, *op, value);
            }
            StmtKind::Expr(expr) => {
                g.gen_expr_stmt(expr);
            }
        }
        if g.code.len() as u16 > pc {
            g.line_numbers.push((pc, stmt.line));
        }
    }

    // Every void method ends with an appended `return`, mapped to the closing brace.
    let ret_pc = g.code.len() as u16;
    g.code.push(RETURN);
    g.line_numbers.push((ret_pc, method.close_line));

    CfMethod {
        access_flags: 0x0009, // ACC_PUBLIC | ACC_STATIC
        name: method.name.clone(),
        descriptor: descriptor_of(method),
        max_stack: g.max_stack,
        max_locals: info.local_count.max(1),
        code: g.code,
        line_numbers: g.line_numbers,
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

/// Per-method emission state, with a running operand-stack depth (`cur`) tracked
/// in words so category-2 values count as two.
struct Gen<'a> {
    cp: &'a mut ConstantPool,
    info: &'a MethodInfo,
    code: Vec<u8>,
    line_numbers: Vec<(u16, u16)>,
    max_stack: u16,
    cur: u16,
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
                    let (mag, add) = if delta >= 0 {
                        (delta, true)
                    } else {
                        // |i32::MIN| is unrepresentable; keep `iadd MIN` for it.
                        (delta.wrapping_neg(), delta == i32::MIN)
                    };
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
        Expr::Cast { ty, expr } => const_convert(fold(expr)?, sema::valtype(*ty)),
        Expr::Binary { op, left, right } => eval_binary(*op, fold(left)?, fold(right)?),
    })
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
            Expr::Cast { .. } => "Cast",
            Expr::Binary { .. } => "Binary",
            Expr::Println(_) => "Println",
        };
        f.write_str(kind)
    }
}
