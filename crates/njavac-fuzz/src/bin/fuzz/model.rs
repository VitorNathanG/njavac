#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Ty {
    Int,
    Long,
    Float,
    Double,
    Boolean,
    Char,
    Byte,
    Short,
}

impl Ty {
    pub(super) fn kw(self) -> &'static str {
        match self {
            Ty::Int => "int",
            Ty::Long => "long",
            Ty::Float => "float",
            Ty::Double => "double",
            Ty::Boolean => "boolean",
            Ty::Char => "char",
            Ty::Byte => "byte",
            Ty::Short => "short",
        }
    }

    /// Binary-numeric-promotion rank: the wider wins. Sub-int types all rank as
    /// `int` (0). `boolean` is not numeric (255).
    pub(super) fn prank(self) -> u8 {
        match self {
            Ty::Long => 1,
            Ty::Float => 2,
            Ty::Double => 3,
            Ty::Boolean => 255,
            _ => 0,
        }
    }

    pub(super) fn is_numeric(self) -> bool {
        self != Ty::Boolean
    }

    /// Integral in the Java sense (participates in shift / `~` / integral `%`).
    pub(super) fn is_integral(self) -> bool {
        matches!(self, Ty::Int | Ty::Long | Ty::Byte | Ty::Short | Ty::Char)
    }
}

/// A literal value; its Java type is implied by the variant. Float and double
/// bits preserve generated values such as signed zero and distinct NaN payloads;
/// class-file pooling separately canonicalizes NaNs.
#[derive(Clone, Debug)]
pub(super) enum Val {
    I(i32),
    L(i64),
    F(u32),
    D(u64),
    Bool(bool),
    C(u16),
}

#[derive(Clone, Copy, Debug)]
pub(super) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BAnd,
    BOr,
    BXor,
    Shl,
    Shr,
    Ushr,
}

impl BinOp {
    pub(super) fn sym(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::BAnd => "&",
            BinOp::BOr => "|",
            BinOp::BXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
            BinOp::Ushr => ">>>",
        }
    }

    pub(super) fn is_shift(self) -> bool {
        matches!(self, BinOp::Shl | BinOp::Shr | BinOp::Ushr)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl CmpOp {
    pub(super) fn sym(self) -> &'static str {
        match self {
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
            CmpOp::Eq => "==",
            CmpOp::Ne => "!=",
        }
    }

    pub(super) fn is_order(self) -> bool {
        matches!(self, CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum LogOp {
    And,
    Or,
}

#[derive(Clone, Debug)]
pub(super) enum FExpr {
    Lit(Val),
    Local(usize),
    Neg(Box<FExpr>),
    BitNot(Box<FExpr>),
    Not(Box<FExpr>),
    Paren(Box<FExpr>),
    Cast(Ty, Box<FExpr>),
    Bin(BinOp, Box<FExpr>, Box<FExpr>),
    Cmp(CmpOp, Box<FExpr>, Box<FExpr>),
    Logic(LogOp, Box<FExpr>, Box<FExpr>),
}

#[derive(Clone, Debug)]
pub(super) enum PrintArg {
    Str(String),
    Expr(FExpr),
}

#[derive(Clone, Debug)]
pub(super) enum FStmt {
    Decl {
        ty: Ty,
        local: usize,
        init: Option<FExpr>,
    },
    Assign {
        local: usize,
        value: FExpr,
    },
    Compound {
        local: usize,
        op: BinOp,
        value: FExpr,
    },
    IncDec {
        local: usize,
        prefix: bool,
        inc: bool,
    },
    Println(PrintArg),
    If {
        cond: FExpr,
        then_b: Vec<FStmt>,
        else_b: Option<Vec<FStmt>>,
    },
}

/// Whether a program is random input or a guaranteed structural coverage case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CaseKind {
    Random,
    LongConditionalBoundary,
    LongConditionalFat,
    LongGotoFat,
}

impl CaseKind {
    pub(super) fn is_scheduled(self) -> bool {
        self != CaseKind::Random
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            CaseKind::Random => "random",
            CaseKind::LongConditionalBoundary => "long-conditional-32767",
            CaseKind::LongConditionalFat => "long-conditional-32768",
            CaseKind::LongGotoFat => "long-goto-32768",
        }
    }
}

/// A whole program: the class name (via `ident`), the flat local-type env in
/// declaration/slot order, and the `main` body. `Local(usize)` indexes `locals`.
#[derive(Clone, Debug)]
pub(super) struct Prog {
    pub(super) name: Ident,
    pub(super) kind: CaseKind,
    /// The flat local-type env in declaration/slot order. Not read after
    /// construction today (render derives everything from `Decl`/`Local`), but the
    /// scope-agnostic flat env is what makes block scope (loops) an env-only change.
    #[allow(dead_code)]
    pub(super) locals: Vec<Ty>,
    pub(super) body: Vec<FStmt>,
}

/// THE naming chokepoint. Class name, source filename, and the `source_file`
/// argument to `njavac::compile` are the SAME token.
#[derive(Clone, Debug)]
pub(super) struct Ident {
    pub(super) class: String,
    pub(super) java_file: String,
    pub(super) source_arg: String,
}

pub(super) fn ident(n: u64) -> Ident {
    let class = format!("Fuzz{n:07}");
    let java_file = format!("{class}.java");
    Ident {
        source_arg: java_file.clone(),
        java_file,
        class,
    }
}
