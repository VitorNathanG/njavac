use crate::model::{BinOp, CaseKind, FExpr, FStmt, PrintArg, Prog, Ty, Val, ident};

/// Scheduled source-layout coverage for
/// `fixtures/basics/MixedLineTerminators.java`.
pub(super) fn scheduled(index: u64) -> Option<Prog> {
    if index != 3 {
        return None;
    }

    let sum = FExpr::Bin(
        BinOp::Add,
        Box::new(FExpr::Bin(
            BinOp::Add,
            Box::new(FExpr::Local(0)),
            Box::new(FExpr::Local(1)),
        )),
        Box::new(FExpr::Local(2)),
    );
    Some(Prog {
        name: ident(index),
        kind: CaseKind::MixedLineTerminators,
        locals: vec![Ty::Int; 3],
        body: vec![
            FStmt::Decl {
                ty: Ty::Int,
                local: 0,
                init: Some(FExpr::Lit(Val::I(1))),
            },
            FStmt::Decl {
                ty: Ty::Int,
                local: 1,
                init: Some(FExpr::Lit(Val::I(2))),
            },
            FStmt::Decl {
                ty: Ty::Int,
                local: 2,
                init: Some(FExpr::Lit(Val::I(3))),
            },
            FStmt::Println(PrintArg::Expr(sum)),
        ],
    })
}
