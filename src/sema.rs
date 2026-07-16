//! Semantic analysis: validation, name/slot resolution, and expression typing.
//!
//! Walks `main`'s statements, assigns each local a JVM slot (parameters occupy
//! the low slots; locals follow in declaration order), and records each local's
//! type. `long`/`double` are **two slots wide**, so slot indices bump by width —
//! this is the allocator change the whole numeric subset rests on.
//!
//! `type_of` computes the static type of any expression, implementing Java's
//! unary and binary numeric promotion (comparisons and `!` type to `boolean`).
//! Codegen consults it to pick load/store opcodes, conversion opcodes, `println`
//! descriptors, constant-load ladders, and comparison branch opcodes. Slot
//! Validation keeps allocation to method-body declarations and rejects branch-local
//! declarations until the scoped allocator exists.

use std::collections::{HashMap, HashSet};

use crate::ast::{BinOp, CmpOp, CompilationUnit, Expr, Method, Stmt, StmtKind, Type};
use crate::diagnostic::{CompileResult, Diagnostic};
use crate::span::Span;

/// The static type of an expression / local in the subset: the eight primitives
/// plus `String` (only ever a string-literal `println` argument).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValType {
    Int,
    Long,
    Float,
    Double,
    Boolean,
    Char,
    Byte,
    Short,
    String,
}

impl ValType {
    /// Local-slot / operand-stack width in words: `long`/`double` are 2, all
    /// others (including the sub-int types, which live as `int` on the stack) 1.
    pub fn width(self) -> u16 {
        match self {
            ValType::Long | ValType::Double => 2,
            _ => 1,
        }
    }

    /// The JVM *computational* type this value occupies on the operand stack. The
    /// sub-int types (`boolean`/`char`/`byte`/`short`) are all `Int` on the stack.
    pub fn stack(self) -> StackTy {
        match self {
            ValType::Long => StackTy::Long,
            ValType::Float => StackTy::Float,
            ValType::Double => StackTy::Double,
            _ => StackTy::Int,
        }
    }

    /// Whether this is one of the sub-int integral types stored as an `int`.
    pub fn is_subint(self) -> bool {
        matches!(self, ValType::Boolean | ValType::Char | ValType::Byte | ValType::Short)
    }
}

/// The four JVM operand-stack computational types the subset can produce (plus
/// `reference`, which only `String` uses and which never participates in
/// arithmetic here).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackTy {
    Int,
    Long,
    Float,
    Double,
}

/// The `ValType` an AST declared type denotes.
pub fn valtype(ty: Type) -> ValType {
    match ty {
        Type::Int => ValType::Int,
        Type::Long => ValType::Long,
        Type::Float => ValType::Float,
        Type::Double => ValType::Double,
        Type::Boolean => ValType::Boolean,
        Type::Char => ValType::Char,
        Type::Byte => ValType::Byte,
        Type::Short => ValType::Short,
        // `String[]` is only ever `main`'s parameter, never read as a value; give
        // it a placeholder so slot allocation can size it (one slot).
        Type::StringArray => ValType::String,
    }
}

/// Analysis result for one method: local slots, local types, and the slot count.
pub struct MethodInfo {
    /// Slot index for each local (parameters included).
    pub slots: HashMap<String, u16>,
    /// Declared type of each local (parameters included).
    pub types: HashMap<String, ValType>,
    /// Number of local slots occupied by parameters + declared locals, counting
    /// `long`/`double` as two. A lower bound on `max_locals`.
    pub local_count: u16,
}

impl MethodInfo {
    pub fn slot(&self, name: &str) -> u16 {
        *self
            .slots
            .get(name)
            .unwrap_or_else(|| panic!("undeclared local: {name}"))
    }

    pub fn ty(&self, name: &str) -> ValType {
        *self
            .types
            .get(name)
            .unwrap_or_else(|| panic!("undeclared local: {name}"))
    }
}

/// Whole-program analysis result: one `MethodInfo` per method, in method order.
pub struct Analysis {
    pub methods: Vec<MethodInfo>,
}

/// Analyze a parsed compilation unit, assigning local slots for each method.
pub fn analyze(unit: &CompilationUnit) -> CompileResult<Analysis> {
    validate_class_shape(unit)?;
    Ok(Analysis { methods: vec![analyze_method(&unit.class.methods[0])?] })
}

fn validate_class_shape(unit: &CompilationUnit) -> CompileResult<()> {
    let methods = &unit.class.methods;
    if methods.is_empty() {
        return Err(Diagnostic::unsupported_semantic(
            unit.class.name_span,
            "the supported class must declare main(String[])",
        ));
    }

    for method in methods {
        let mut names = HashSet::new();
        for param in &method.params {
            if !names.insert(param.name.text.as_str()) {
                return Err(Diagnostic::semantic(
                    param.name.span,
                    format!("duplicate parameter `{}`", param.name.text),
                ));
            }
        }
    }

    if methods.len() != 1 {
        return Err(Diagnostic::unsupported_semantic(
            methods[1].span,
            "the supported class must contain exactly one method",
        ));
    }

    let method = &methods[0];
    if method.name != "main" {
        return Err(Diagnostic::unsupported_semantic(
            method.name_span,
            "the supported method must be named `main`",
        ));
    }
    if !method.is_static {
        return Err(Diagnostic::unsupported_semantic(
            method.span,
            "the supported `main` method must be static",
        ));
    }
    if method.params.len() != 1 {
        let span = method.params.get(1).map_or(method.name_span, |param| param.span);
        return Err(Diagnostic::unsupported_semantic(
            span,
            "the supported `main` method must have one String[] parameter",
        ));
    }
    if method.params[0].ty != Type::StringArray {
        return Err(Diagnostic::unsupported_semantic(
            method.params[0].span,
            "the supported `main` parameter must have type String[]",
        ));
    }
    Ok(())
}

fn analyze_method(method: &Method) -> CompileResult<MethodInfo> {
    let mut analyzer = MethodAnalyzer {
        slots: HashMap::new(),
        types: HashMap::new(),
        assigned: HashSet::new(),
        next: 0,
    };

    // Parameters take the low slots and are definitely assigned at method entry.
    for param in &method.params {
        analyzer.declare(&param.name.text, valtype(param.ty), param.name.span)?;
        analyzer.assigned.insert(param.name.text.clone());
    }
    for stmt in &method.body {
        analyzer.validate_stmt(stmt, false)?;
    }

    Ok(MethodInfo {
        slots: analyzer.slots,
        types: analyzer.types,
        local_count: analyzer.next,
    })
}

struct MethodAnalyzer {
    slots: HashMap<String, u16>,
    types: HashMap<String, ValType>,
    assigned: HashSet<String>,
    next: u16,
}

impl MethodAnalyzer {
    fn declare(&mut self, name: &str, ty: ValType, span: Span) -> CompileResult<()> {
        if self.types.contains_key(name) {
            return Err(Diagnostic::semantic(span, format!("duplicate local `{name}`")));
        }
        let next = self.next.checked_add(ty.width()).ok_or_else(|| {
            Diagnostic::unsupported_semantic(span, "method requires too many local slots")
        })?;
        self.slots.insert(name.to_string(), self.next);
        self.types.insert(name.to_string(), ty);
        self.next = next;
        Ok(())
    }

    fn validate_stmt(&mut self, stmt: &Stmt, in_branch: bool) -> CompileResult<()> {
        match &stmt.kind {
            StmtKind::LocalDecl { ty, name, init } => {
                if in_branch {
                    return Err(Diagnostic::unsupported_semantic(
                        stmt.span,
                        "local declarations inside branches are unsupported",
                    ));
                }
                let target = valtype(*ty);
                self.declare(&name.text, target, stmt.span)?;
                if let Some(init) = init {
                    let source = self.validate_expr(init, stmt.span)?;
                    self.require_assignable(target, source, init, stmt.span)?;
                    self.assigned.insert(name.text.clone());
                }
            }
            StmtKind::Assign { name, value } => {
                let target = self.local_type(&name.text, stmt.span)?;
                let source = self.validate_expr(value, stmt.span)?;
                self.require_assignable(target, source, value, stmt.span)?;
                self.assigned.insert(name.text.clone());
            }
            StmtKind::CompoundAssign { name, op, value } => {
                let target = self.read_local(&name.text, stmt.span)?;
                let source = self.validate_expr(value, stmt.span)?;
                self.require_compound(*op, target, source, stmt.span)?;
                self.assigned.insert(name.text.clone());
            }
            StmtKind::Expr(expr) => match expr {
                Expr::Println(arg) => {
                    let ty = self.validate_expr(arg, stmt.span)?;
                    if ty == ValType::String && !is_string_value(arg) {
                        return Err(Diagnostic::unsupported_semantic(
                            stmt.span,
                            "only string literals are supported as String values",
                        ));
                    }
                }
                _ => {
                    return Err(Diagnostic::semantic(
                        stmt.span,
                        "only a method invocation may be used as an expression statement",
                    ));
                }
            },
            StmtKind::If { cond, then_branch, else_branch } => {
                let ty = self.validate_expr(cond, stmt.span)?;
                if ty != ValType::Boolean {
                    return Err(Diagnostic::semantic(stmt.span, "if condition must be boolean"));
                }

                let incoming = self.assigned.clone();
                self.assigned = incoming.clone();
                for nested in &then_branch.stmts {
                    self.validate_stmt(nested, true)?;
                }
                let then_assigned = self.assigned.clone();

                self.assigned = incoming.clone();
                if let Some(else_branch) = else_branch {
                    for nested in &else_branch.stmts {
                        self.validate_stmt(nested, true)?;
                    }
                }
                let else_assigned = self.assigned.clone();
                self.assigned = then_assigned
                    .intersection(&else_assigned)
                    .cloned()
                    .collect();
            }
        }
        Ok(())
    }

    fn validate_expr(&self, expr: &Expr, span: Span) -> CompileResult<ValType> {
        let ty = match expr {
            Expr::IntLit(_) => ValType::Int,
            Expr::LongLit(_) => ValType::Long,
            Expr::FloatLit(_) => ValType::Float,
            Expr::DoubleLit(_) => ValType::Double,
            Expr::BoolLit(_) => ValType::Boolean,
            Expr::CharLit(_) => ValType::Char,
            Expr::StringLit(_) => ValType::String,
            Expr::Name(name) => {
                let ty = self.read_local(&name.text, span)?;
                if ty == ValType::String {
                    return Err(Diagnostic::unsupported_semantic(
                        span,
                        "using the String[] parameter as a value is unsupported",
                    ));
                }
                ty
            }
            Expr::Neg(inner) => {
                let ty = self.validate_expr(inner, span)?;
                self.require_numeric(ty, span, "unary `-`")?;
                unary_promote(ty)
            }
            Expr::BitNot(inner) => {
                let ty = self.validate_expr(inner, span)?;
                self.require_integral(ty, span, "unary `~`")?;
                unary_promote(ty)
            }
            Expr::Not(inner) => {
                let ty = self.validate_expr(inner, span)?;
                self.require_boolean(ty, span, "unary `!`")?;
                ValType::Boolean
            }
            Expr::Paren(inner) => self.validate_expr(inner, span)?,
            Expr::Cast { ty, expr } => {
                let source = self.validate_expr(expr, span)?;
                let target = valtype(*ty);
                if !((is_numeric(source) && is_numeric(target))
                    || (source == ValType::Boolean && target == ValType::Boolean))
                {
                    return Err(Diagnostic::semantic(span, "invalid primitive cast"));
                }
                target
            }
            Expr::Binary { op, left, right } => {
                let left_ty = self.validate_expr(left, span)?;
                let right_ty = self.validate_expr(right, span)?;
                self.validate_binary(*op, left_ty, right_ty, right, span)?
            }
            Expr::Compare { op, left, right } => {
                let left_ty = self.validate_expr(left, span)?;
                let right_ty = self.validate_expr(right, span)?;
                self.validate_compare(*op, left_ty, right_ty, span)?;
                ValType::Boolean
            }
            Expr::Logical { left, right, .. } => {
                let left_ty = self.validate_expr(left, span)?;
                let right_ty = self.validate_expr(right, span)?;
                self.require_boolean(left_ty, span, "logical operator")?;
                self.require_boolean(right_ty, span, "logical operator")?;
                ValType::Boolean
            }
            Expr::Println(_) => {
                return Err(Diagnostic::semantic(
                    span,
                    "System.out.println does not produce a value",
                ));
            }
        };
        Ok(ty)
    }

    fn validate_binary(
        &self,
        op: BinOp,
        left: ValType,
        right: ValType,
        right_expr: &Expr,
        span: Span,
    ) -> CompileResult<ValType> {
        if op == BinOp::Add && (left == ValType::String || right == ValType::String) {
            return Err(Diagnostic::unsupported_semantic(
                span,
                "String concatenation is unsupported",
            ));
        }
        if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor)
            && left == ValType::Boolean
            && right == ValType::Boolean
        {
            return Ok(ValType::Boolean);
        }
        if op.is_shift() {
            self.require_integral(left, span, "shift operator")?;
            self.require_integral(right, span, "shift operator")?;
            return Ok(unary_promote(left));
        }
        if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor) {
            self.require_integral(left, span, "bitwise operator")?;
            self.require_integral(right, span, "bitwise operator")?;
        } else {
            self.require_numeric(left, span, "arithmetic operator")?;
            self.require_numeric(right, span, "arithmetic operator")?;
        }
        let result = binary_promote(left, right);
        if matches!(op, BinOp::Div | BinOp::Rem)
            && is_integral(result)
            && eval_numeric_constant(right_expr).is_some_and(NumericConst::is_zero)
        {
            return Err(Diagnostic::semantic(
                span,
                "integral division or remainder by zero",
            ));
        }
        Ok(result)
    }

    fn validate_compare(
        &self,
        op: CmpOp,
        left: ValType,
        right: ValType,
        span: Span,
    ) -> CompileResult<()> {
        if matches!(op, CmpOp::Eq | CmpOp::Ne) {
            if left == ValType::String && right == ValType::String {
                return Err(Diagnostic::unsupported_semantic(
                    span,
                    "reference comparison is unsupported",
                ));
            }
            if (is_numeric(left) && is_numeric(right))
                || (left == ValType::Boolean && right == ValType::Boolean)
            {
                return Ok(());
            }
            return Err(Diagnostic::semantic(span, "invalid equality operands"));
        }
        self.require_numeric(left, span, "relational operator")?;
        self.require_numeric(right, span, "relational operator")
    }

    fn require_assignable(
        &self,
        target: ValType,
        source: ValType,
        expr: &Expr,
        span: Span,
    ) -> CompileResult<()> {
        if target == ValType::String {
            return Err(Diagnostic::semantic(
                span,
                "cannot assign a value to the String[] parameter",
            ));
        }
        if is_assignment_convertible(target, source)
            || (is_integral(target)
                && matches!(source, ValType::Int | ValType::Byte | ValType::Short | ValType::Char)
                && is_constant_expression(expr))
        {
            return Ok(());
        }
        Err(Diagnostic::semantic(
            span,
            format!("cannot assign {source:?} to {target:?}"),
        ))
    }

    fn require_compound(
        &self,
        op: BinOp,
        target: ValType,
        source: ValType,
        span: Span,
    ) -> CompileResult<()> {
        let valid = if op.is_shift() {
            is_integral(target) && is_integral(source)
        } else if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor) {
            (is_integral(target) && is_integral(source))
                || (target == ValType::Boolean && source == ValType::Boolean)
        } else {
            is_numeric(target) && is_numeric(source)
        };
        if valid {
            Ok(())
        } else {
            Err(Diagnostic::semantic(span, "invalid compound assignment operands"))
        }
    }

    fn local_type(&self, name: &str, span: Span) -> CompileResult<ValType> {
        self.types
            .get(name)
            .copied()
            .ok_or_else(|| Diagnostic::semantic(span, format!("undeclared local `{name}`")))
    }

    fn read_local(&self, name: &str, span: Span) -> CompileResult<ValType> {
        let ty = self.local_type(name, span)?;
        if !self.assigned.contains(name) {
            return Err(Diagnostic::semantic(
                span,
                format!("local `{name}` might not have been initialized"),
            ));
        }
        Ok(ty)
    }

    fn require_numeric(&self, ty: ValType, span: Span, context: &str) -> CompileResult<()> {
        if is_numeric(ty) {
            Ok(())
        } else {
            Err(Diagnostic::semantic(span, format!("{context} requires numeric operands")))
        }
    }

    fn require_integral(&self, ty: ValType, span: Span, context: &str) -> CompileResult<()> {
        if is_integral(ty) {
            Ok(())
        } else {
            Err(Diagnostic::semantic(span, format!("{context} requires integral operands")))
        }
    }

    fn require_boolean(&self, ty: ValType, span: Span, context: &str) -> CompileResult<()> {
        if ty == ValType::Boolean {
            Ok(())
        } else {
            Err(Diagnostic::semantic(span, format!("{context} requires boolean operands")))
        }
    }
}

fn is_numeric(ty: ValType) -> bool {
    !matches!(ty, ValType::Boolean | ValType::String)
}

fn is_integral(ty: ValType) -> bool {
    matches!(ty, ValType::Int | ValType::Long | ValType::Char | ValType::Byte | ValType::Short)
}

fn is_assignment_convertible(target: ValType, source: ValType) -> bool {
    use ValType::*;
    target == source
        || matches!(
            (source, target),
            (Byte, Short | Int | Long | Float | Double)
                | (Short, Int | Long | Float | Double)
                | (Char, Int | Long | Float | Double)
                | (Int, Long | Float | Double)
                | (Long, Float | Double)
                | (Float, Double)
        )
}

/// A syntax-only approximation used for assignment conversion. Range checking is
/// deliberately left to a later constant-analysis stage, so existing valid folded
/// initializers are not rejected here.
fn is_constant_expression(expr: &Expr) -> bool {
    match expr {
        Expr::IntLit(_)
        | Expr::LongLit(_)
        | Expr::FloatLit(_)
        | Expr::DoubleLit(_)
        | Expr::BoolLit(_)
        | Expr::CharLit(_)
        | Expr::StringLit(_) => true,
        Expr::Neg(inner) | Expr::BitNot(inner) | Expr::Not(inner) | Expr::Paren(inner) => {
            is_constant_expression(inner)
        }
        Expr::Cast { expr, .. } => is_constant_expression(expr),
        Expr::Binary { left, right, .. }
        | Expr::Compare { left, right, .. }
        | Expr::Logical { left, right, .. } => {
            is_constant_expression(left) && is_constant_expression(right)
        }
        Expr::Name(_) | Expr::Println(_) => false,
    }
}

/// Numeric constant evaluation needed only to identify an integral zero divisor.
/// It mirrors the folding arithmetic that can reach codegen's integer `/` and `%`.
#[derive(Clone, Copy)]
enum NumericConst {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
}

impl NumericConst {
    fn is_zero(self) -> bool {
        matches!(self, Self::Int(0) | Self::Long(0))
    }

    fn rank(self) -> u8 {
        match self {
            Self::Int(_) => 0,
            Self::Long(_) => 1,
            Self::Float(_) => 2,
            Self::Double(_) => 3,
        }
    }

    fn to_i32(self) -> i32 {
        match self {
            Self::Int(value) => value,
            Self::Long(value) => value as i32,
            Self::Float(value) => value as i32,
            Self::Double(value) => value as i32,
        }
    }

    fn to_i64(self) -> i64 {
        match self {
            Self::Int(value) => value as i64,
            Self::Long(value) => value,
            Self::Float(value) => value as i64,
            Self::Double(value) => value as i64,
        }
    }

    fn to_f32(self) -> f32 {
        match self {
            Self::Int(value) => value as f32,
            Self::Long(value) => value as f32,
            Self::Float(value) => value,
            Self::Double(value) => value as f32,
        }
    }

    fn to_f64(self) -> f64 {
        match self {
            Self::Int(value) => value as f64,
            Self::Long(value) => value as f64,
            Self::Float(value) => value as f64,
            Self::Double(value) => value,
        }
    }

    fn cast(self, ty: Type) -> Option<Self> {
        Some(match ty {
            Type::Int => Self::Int(self.to_i32()),
            Type::Long => Self::Long(self.to_i64()),
            Type::Float => Self::Float(self.to_f32()),
            Type::Double => Self::Double(self.to_f64()),
            Type::Byte => Self::Int((self.to_i32() as i8) as i32),
            Type::Short => Self::Int((self.to_i32() as i16) as i32),
            Type::Char => Self::Int((self.to_i32() as u16) as i32),
            Type::Boolean | Type::StringArray => return None,
        })
    }
}

fn eval_numeric_constant(expr: &Expr) -> Option<NumericConst> {
    Some(match expr {
        Expr::IntLit(value) => NumericConst::Int(*value),
        Expr::LongLit(value) => NumericConst::Long(*value),
        Expr::FloatLit(value) => NumericConst::Float(*value),
        Expr::DoubleLit(value) => NumericConst::Double(*value),
        Expr::CharLit(value) => NumericConst::Int(*value as i32),
        Expr::Neg(inner) => match eval_numeric_constant(inner)? {
            NumericConst::Int(value) => NumericConst::Int(value.wrapping_neg()),
            NumericConst::Long(value) => NumericConst::Long(value.wrapping_neg()),
            NumericConst::Float(value) => NumericConst::Float(-value),
            NumericConst::Double(value) => NumericConst::Double(-value),
        },
        Expr::BitNot(inner) => match eval_numeric_constant(inner)? {
            NumericConst::Int(value) => NumericConst::Int(!value),
            NumericConst::Long(value) => NumericConst::Long(!value),
            NumericConst::Float(_) | NumericConst::Double(_) => return None,
        },
        Expr::Paren(inner) => eval_numeric_constant(inner)?,
        Expr::Cast { ty, expr } => eval_numeric_constant(expr)?.cast(*ty)?,
        Expr::Binary { op, left, right } => {
            let left = eval_numeric_constant(left)?;
            let right = eval_numeric_constant(right)?;
            eval_numeric_binary(*op, left, right)?
        }
        Expr::BoolLit(_)
        | Expr::StringLit(_)
        | Expr::Name(_)
        | Expr::Not(_)
        | Expr::Compare { .. }
        | Expr::Logical { .. }
        | Expr::Println(_) => return None,
    })
}

fn eval_numeric_binary(op: BinOp, left: NumericConst, right: NumericConst) -> Option<NumericConst> {
    if op.is_shift() {
        // Codegen deliberately leaves this javac quirk unfolded, so it cannot
        // expose an integer folding panic in an enclosing division either.
        if op == BinOp::UShr
            && matches!(left, NumericConst::Long(_))
            && matches!(right, NumericConst::Long(_))
        {
            return None;
        }
        return Some(match left {
            NumericConst::Long(value) => {
                let distance = (right.to_i32() & 63) as u32;
                NumericConst::Long(match op {
                    BinOp::Shl => value.wrapping_shl(distance),
                    BinOp::Shr => value.wrapping_shr(distance),
                    BinOp::UShr => ((value as u64).wrapping_shr(distance)) as i64,
                    _ => unreachable!(),
                })
            }
            NumericConst::Int(value) => {
                let distance = (right.to_i32() & 31) as u32;
                NumericConst::Int(match op {
                    BinOp::Shl => value.wrapping_shl(distance),
                    BinOp::Shr => value.wrapping_shr(distance),
                    BinOp::UShr => ((value as u32).wrapping_shr(distance)) as i32,
                    _ => unreachable!(),
                })
            }
            NumericConst::Float(_) | NumericConst::Double(_) => return None,
        });
    }

    Some(match left.rank().max(right.rank()) {
        0 => {
            let (left, right) = (left.to_i32(), right.to_i32());
            NumericConst::Int(match op {
                BinOp::Add => left.wrapping_add(right),
                BinOp::Sub => left.wrapping_sub(right),
                BinOp::Mul => left.wrapping_mul(right),
                BinOp::Div if right != 0 => left.wrapping_div(right),
                BinOp::Rem if right != 0 => left.wrapping_rem(right),
                BinOp::And => left & right,
                BinOp::Or => left | right,
                BinOp::Xor => left ^ right,
                _ => return None,
            })
        }
        1 => {
            let (left, right) = (left.to_i64(), right.to_i64());
            NumericConst::Long(match op {
                BinOp::Add => left.wrapping_add(right),
                BinOp::Sub => left.wrapping_sub(right),
                BinOp::Mul => left.wrapping_mul(right),
                BinOp::Div if right != 0 => left.wrapping_div(right),
                BinOp::Rem if right != 0 => left.wrapping_rem(right),
                BinOp::And => left & right,
                BinOp::Or => left | right,
                BinOp::Xor => left ^ right,
                _ => return None,
            })
        }
        2 => {
            let (left, right) = (left.to_f32(), right.to_f32());
            NumericConst::Float(match op {
                BinOp::Add => left + right,
                BinOp::Sub => left - right,
                BinOp::Mul => left * right,
                BinOp::Div => left / right,
                BinOp::Rem => left % right,
                _ => return None,
            })
        }
        _ => {
            let (left, right) = (left.to_f64(), right.to_f64());
            NumericConst::Double(match op {
                BinOp::Add => left + right,
                BinOp::Sub => left - right,
                BinOp::Mul => left * right,
                BinOp::Div => left / right,
                BinOp::Rem => left % right,
                _ => return None,
            })
        }
    })
}

fn is_string_value(expr: &Expr) -> bool {
    match expr {
        Expr::StringLit(_) => true,
        Expr::Paren(inner) => is_string_value(inner),
        _ => false,
    }
}

/// Unary numeric promotion: `byte`/`short`/`char` (and `boolean`) become `int`;
/// wider types are unchanged. Applied to the operand of a unary op and to the
/// left operand of a shift.
pub fn unary_promote(t: ValType) -> ValType {
    match t {
        ValType::Long => ValType::Long,
        ValType::Float => ValType::Float,
        ValType::Double => ValType::Double,
        ValType::Boolean => ValType::Boolean,
        _ => ValType::Int,
    }
}

/// Binary numeric promotion: the wider of the two operand types, with everything
/// narrower than `int` promoted to `int`.
pub fn binary_promote(a: ValType, b: ValType) -> ValType {
    use ValType::*;
    if a == Double || b == Double {
        Double
    } else if a == Float || b == Float {
        Float
    } else if a == Long || b == Long {
        Long
    } else {
        Int
    }
}

/// The static type of an expression, implementing promotion. `Println` is a
/// `void` call and never appears as a value operand.
pub fn type_of(expr: &Expr, info: &MethodInfo) -> ValType {
    match expr {
        Expr::IntLit(_) => ValType::Int,
        Expr::LongLit(_) => ValType::Long,
        Expr::FloatLit(_) => ValType::Float,
        Expr::DoubleLit(_) => ValType::Double,
        Expr::BoolLit(_) => ValType::Boolean,
        Expr::CharLit(_) => ValType::Char,
        Expr::StringLit(_) => ValType::String,
        Expr::Name(n) => info.ty(&n.text),
        Expr::Neg(e) => unary_promote(type_of(e, info)),
        Expr::BitNot(e) => unary_promote(type_of(e, info)),
        Expr::Not(_) => ValType::Boolean,
        Expr::Paren(e) => type_of(e, info),
        Expr::Compare { .. } => ValType::Boolean,
        Expr::Logical { .. } => ValType::Boolean,
        Expr::Cast { ty, .. } => valtype(*ty),
        Expr::Binary { op, left, right } => {
            let lt = type_of(left, info);
            let rt = type_of(right, info);
            // `&`/`|`/`^` on two booleans is boolean (non-short-circuit logical).
            if matches!(op, BinOp::And | BinOp::Or | BinOp::Xor)
                && lt == ValType::Boolean
                && rt == ValType::Boolean
            {
                ValType::Boolean
            } else if op.is_shift() {
                unary_promote(lt)
            } else {
                binary_promote(lt, rt)
            }
        }
        Expr::Println(_) => ValType::Int,
    }
}
