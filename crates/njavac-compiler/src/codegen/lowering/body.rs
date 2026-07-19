use crate::ast::{BinOp, ExprId, ExprKind, Name, PrimitiveType, Stmt, StmtKind, Type};
use crate::sema::{self, ResolvedCall};

use super::super::constant::*;
use super::super::instruction::*;
use super::super::ops::*;
use super::super::stack::StackTy;
use super::Gen;

impl Gen<'_> {
    /// Emit one statement. Each statement starts with an empty operand stack; a
    /// leaf statement gets a LineNumberTable entry at its first instruction, while
    /// an `if` places its own entries (condition, then each nested statement).
    pub(super) fn gen_stmt(&mut self, stmt: &Stmt) {
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

    // -------- statements --------

    fn gen_expr_stmt(&mut self, expr: ExprId) {
        match &self.exprs[expr] {
            ExprKind::Call { args, .. } => {
                let result = self.gen_call(expr, args);
                assert!(
                    result.is_void(),
                    "value-returning expression statement is unsupported"
                );
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
                let method = self
                    .cp
                    .methodref("java/io/PrintStream", "println", descriptor);
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
    pub(super) fn gen_value(&mut self, expr: ExprId) -> Type {
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
    pub(super) fn gen_promoted_operand(&mut self, expr: ExprId, p: PrimitiveType) {
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
}
