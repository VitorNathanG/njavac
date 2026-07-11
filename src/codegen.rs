//! Code generation: typed AST -> class bytes, via the `classfile` backend.
//!
//! This is where byte-identity is won or lost: constant-load opcode selection,
//! local-slot allocation and short-form load/store opcodes, max_stack/max_locals,
//! and the LineNumberTable.
//!
//! javac folds constant integer subtrees, so `int z = 100 % 7;` compiles to a
//! single `iconst_2`, not `bipush 100; bipush 7; irem`. We mirror that: an
//! expression whose whole subtree is literal is evaluated at compile time (with
//! two's-complement wrapping, matching the JVM) and emitted as one constant load;
//! any subtree that touches a local is emitted as real bytecode.
//!
//! Constants are interned into a single `ConstantPool` in exactly the order the
//! bytecode references them: `<init>`'s operands first, then `main`'s, then (in
//! `ClassFile::to_bytes`) the structural names. That reproduces javac's pool.

use crate::ast::{BinOp, Class, Expr, Method, StmtKind};
use crate::classfile::{ClassFile, ConstantPool, Method as CfMethod};
use crate::sema::{Analysis, MethodInfo, ValType};

// Opcodes used by the subset.
const ICONST_M1: u8 = 0x02;
const ICONST_0: u8 = 0x03;
const BIPUSH: u8 = 0x10;
const SIPUSH: u8 = 0x11;
const LDC: u8 = 0x12;
const LDC_W: u8 = 0x13;
const ILOAD: u8 = 0x15;
const ILOAD_0: u8 = 0x1a;
const ISTORE: u8 = 0x36;
const ISTORE_0: u8 = 0x3b;
const INEG: u8 = 0x74;
const IADD: u8 = 0x60;
const ISUB: u8 = 0x64;
const IMUL: u8 = 0x68;
const IDIV: u8 = 0x6c;
const IREM: u8 = 0x70;
const GETSTATIC: u8 = 0xb2;
const INVOKEVIRTUAL: u8 = 0xb6;
const INVOKESPECIAL: u8 = 0xb7;
const ALOAD_0: u8 = 0x2a;
const RETURN: u8 = 0xb1;

/// Compile one parsed+analyzed class into `.class` bytes.
pub fn generate(class: &Class, analysis: &Analysis, source_file: &str) -> Vec<u8> {
    let mut cp = ConstantPool::new();

    let mut methods = Vec::new();
    // `<init>` first: its body (and its single `Methodref`) is interned before
    // any of main's operands, matching javac's pool order.
    methods.push(gen_init(&mut cp, class.line));

    // Then each declared method. In the subset that is just `main`.
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

/// Emit one method body. All fixtures use `main`, but the shape is general for
/// any static void method over the subset.
fn gen_method(cp: &mut ConstantPool, method: &Method, info: &MethodInfo) -> CfMethod {
    let mut g = Gen { cp, info, code: Vec::new(), line_numbers: Vec::new(), max_stack: 0 };

    for stmt in &method.body {
        // Each statement gets a LineNumberTable entry at its starting pc.
        let pc = g.code.len() as u16;
        g.line_numbers.push((pc, stmt.line));
        match &stmt.kind {
            StmtKind::LocalDecl { name, init } => {
                if let Some(init) = init {
                    g.gen_expr(init);
                    let slot = g.slot(name);
                    g.emit_istore(slot);
                }
                // A bare `int x;` with no initializer emits nothing (and javac
                // gives it no line entry either, but the subset always initializes).
            }
            StmtKind::Assign { name, value } => {
                g.gen_expr(value);
                let slot = g.slot(name);
                g.emit_istore(slot);
            }
            StmtKind::Expr(expr) => {
                g.gen_expr_stmt(expr);
            }
        }
    }

    // Every void method ends with an appended `return`, mapped to the closing
    // brace line.
    let ret_pc = g.code.len() as u16;
    g.code.push(RETURN);
    g.line_numbers.push((ret_pc, method.close_line));

    // max_locals: parameters + locals. `main`'s highest referenced slot never
    // exceeds this, so local_count is exact.
    let max_locals = info.local_count.max(1);

    CfMethod {
        access_flags: 0x0009, // ACC_PUBLIC | ACC_STATIC
        name: method.name.clone(),
        descriptor: descriptor_of(method),
        max_stack: g.max_stack,
        max_locals,
        code: g.code,
        line_numbers: g.line_numbers,
    }
}

/// Build the JVM method descriptor from the parsed signature. In the subset the
/// only method is `([Ljava/lang/String;)V`, but this stays general.
fn descriptor_of(method: &Method) -> String {
    use crate::ast::Type;
    let mut d = String::from("(");
    for p in &method.params {
        match p.ty {
            Type::Int => d.push('I'),
            Type::StringArray => d.push_str("[Ljava/lang/String;"),
        }
    }
    d.push_str(")V");
    d
}

/// Per-method emission state.
struct Gen<'a> {
    cp: &'a mut ConstantPool,
    info: &'a MethodInfo,
    code: Vec<u8>,
    line_numbers: Vec<(u16, u16)>,
    /// Running maximum operand-stack depth.
    max_stack: u16,
    // `cur_stack` is threaded through the expression emitters rather than stored,
    // so the model stays local to each emit.
}

impl<'a> Gen<'a> {
    fn slot(&self, name: &str) -> u16 {
        *self
            .info
            .slots
            .get(name)
            .unwrap_or_else(|| panic!("undeclared local: {name}"))
    }

    /// Note that the operand stack reached `depth`, updating the peak.
    fn observe(&mut self, depth: u16) {
        if depth > self.max_stack {
            self.max_stack = depth;
        }
    }

    /// Emit an expression that is used as a statement. The only such form is
    /// `System.out.println(arg)`.
    fn gen_expr_stmt(&mut self, expr: &Expr) {
        match expr {
            Expr::Println(arg) => self.gen_println(arg),
            other => panic!("unsupported expression statement: {:?}", DebugExpr(other)),
        }
    }

    /// `System.out.println(arg)`:
    ///   getstatic System.out ; <arg> ; invokevirtual println(desc)
    /// Peak stack is 2 (objectref + the one argument, both category-1 here).
    fn gen_println(&mut self, arg: &Expr) {
        let field = self.cp.fieldref(
            "java/lang/System",
            "out",
            "Ljava/io/PrintStream;",
        );
        self.code.push(GETSTATIC);
        push_u16(&mut self.code, field);
        self.observe(1); // PrintStream objectref on the stack

        // The argument is pushed on top of the objectref, so its depth model
        // starts at 1.
        let arg_ty = self.gen_value(arg, 1);

        let desc = match arg_ty {
            ValType::Int => "(I)V",
            ValType::String => "(Ljava/lang/String;)V",
        };
        let method = self.cp.methodref("java/io/PrintStream", "println", desc);
        self.code.push(INVOKEVIRTUAL);
        push_u16(&mut self.code, method);
        // invokevirtual pops objectref + arg; net stack after is base - 1... but
        // it was consumed; nothing left. No need to observe (peak already seen).
    }

    /// Emit code that leaves the value of `expr` on the operand stack (for a
    /// statement-level value like an initializer or assignment RHS), tracking the
    /// stack from an empty base. Returns the value's static type.
    fn gen_expr(&mut self, expr: &Expr) -> ValType {
        self.gen_value(expr, 0)
    }

    /// Emit `expr`, leaving one value on the stack. `base` is the stack depth
    /// already present below this value (e.g. 1 when the value sits atop a
    /// println objectref). Returns the value's static type.
    fn gen_value(&mut self, expr: &Expr, base: u16) -> ValType {
        match expr {
            Expr::StringLit(s) => {
                let idx = self.cp.string(s);
                self.emit_ldc(idx);
                self.observe(base + 1);
                ValType::String
            }
            _ => {
                // All remaining forms are `int`-typed. Constant-fold when possible.
                if let Some(v) = fold(expr) {
                    self.emit_int_const(v);
                    self.observe(base + 1);
                } else {
                    self.gen_int_nonconst(expr, base);
                }
                ValType::Int
            }
        }
    }

    /// Emit a non-constant `int` expression, tracking peak stack. `base` is the
    /// depth below the value this produces.
    fn gen_int_nonconst(&mut self, expr: &Expr, base: u16) {
        match expr {
            Expr::IntLit(v) => {
                self.emit_int_const(*v);
                self.observe(base + 1);
            }
            Expr::Name(name) => {
                let slot = self.slot(name);
                self.emit_iload(slot);
                self.observe(base + 1);
            }
            Expr::Neg(inner) => {
                // A constant operand would have been folded already; here it is a
                // real value negated with `ineg`.
                self.gen_int_operand(inner, base);
                self.code.push(INEG);
                // ineg is stack-neutral; peak already observed by the operand.
            }
            Expr::Binary { op, left, right } => {
                // Java evaluates left-to-right: left occupies `base+1`, then right
                // pushes to `base+2`, then the op collapses back to `base+1`.
                self.gen_int_operand(left, base);
                self.gen_int_operand(right, base + 1);
                self.code.push(binop_opcode(op));
                // After the op, depth is base+1 (already observed via left/right).
            }
            other => panic!("not an int expression: {:?}", DebugExpr(other)),
        }
    }

    /// Emit an `int` operand within a larger expression: fold if constant, else
    /// recurse. `base` is the depth below the operand's result.
    fn gen_int_operand(&mut self, expr: &Expr, base: u16) {
        if let Some(v) = fold(expr) {
            self.emit_int_const(v);
            self.observe(base + 1);
        } else {
            self.gen_int_nonconst(expr, base);
        }
    }

    // ---- primitive emitters ----

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

    /// `ldc`/`ldc_w` of a pool entry, choosing the 1-byte form when the index fits.
    fn emit_ldc(&mut self, idx: u16) {
        if idx <= 0xff {
            self.code.push(LDC);
            self.code.push(idx as u8);
        } else {
            self.code.push(LDC_W);
            push_u16(&mut self.code, idx);
        }
    }

    fn emit_iload(&mut self, slot: u16) {
        if slot <= 3 {
            self.code.push(ILOAD_0 + slot as u8);
        } else {
            self.code.push(ILOAD);
            self.code.push(slot as u8);
        }
    }

    fn emit_istore(&mut self, slot: u16) {
        if slot <= 3 {
            self.code.push(ISTORE_0 + slot as u8);
        } else {
            self.code.push(ISTORE);
            self.code.push(slot as u8);
        }
    }
}

/// Try to evaluate `expr` to a compile-time `int`. Returns `None` if any leaf is
/// a local reference (or a non-int form). Arithmetic uses two's-complement
/// wrapping and truncating division, matching JVM `iadd`/`idiv`/etc. so a folded
/// constant is bit-identical to what the unfolded bytecode would compute.
fn fold(expr: &Expr) -> Option<i32> {
    match expr {
        Expr::IntLit(v) => Some(*v),
        Expr::Name(_) => None,
        Expr::StringLit(_) => None,
        Expr::Println(_) => None,
        Expr::Neg(inner) => fold(inner).map(|v| v.wrapping_neg()),
        Expr::Binary { op, left, right } => {
            let l = fold(left)?;
            let r = fold(right)?;
            Some(match op {
                BinOp::Add => l.wrapping_add(r),
                BinOp::Sub => l.wrapping_sub(r),
                BinOp::Mul => l.wrapping_mul(r),
                // wrapping_div/rem handle i32::MIN / -1 without panicking, matching
                // the JVM's defined result (idiv of MIN by -1 yields MIN).
                BinOp::Div => l.wrapping_div(r),
                BinOp::Rem => l.wrapping_rem(r),
            })
        }
    }
}

fn binop_opcode(op: &BinOp) -> u8 {
    match op {
        BinOp::Add => IADD,
        BinOp::Sub => ISUB,
        BinOp::Mul => IMUL,
        BinOp::Div => IDIV,
        BinOp::Rem => IREM,
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
            Expr::StringLit(_) => "StringLit",
            Expr::Name(_) => "Name",
            Expr::Neg(_) => "Neg",
            Expr::Binary { .. } => "Binary",
            Expr::Println(_) => "Println",
        };
        f.write_str(kind)
    }
}
