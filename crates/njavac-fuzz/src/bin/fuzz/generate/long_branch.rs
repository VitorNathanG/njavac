use crate::model::{BinOp, CaseKind, CmpOp, FExpr, FStmt, LogOp, PrintArg, Prog, Ty, Val, ident};

const CATEGORY_2_PADS: usize = 127;
const SELF_ADDS: usize = 2519;
#[cfg(test)]
const WIDE_SELF_ADD_BYTES: usize = 13;
#[cfg(test)]
const NARROW_BOUNDARY_TAIL_BYTES: usize = 20;
#[cfg(test)]
const FAT_TRIGGER_TAIL_BYTES: usize = 21;

pub(super) fn scheduled(index: u64) -> Option<Prog> {
    match index {
        0 => Some(conditional_boundary(index)),
        1 => Some(conditional_fat(index)),
        2 => Some(goto_fat(index)),
        _ => None,
    }
}

fn push_decl(locals: &mut Vec<Ty>, body: &mut Vec<FStmt>, ty: Ty, init: Option<FExpr>) -> usize {
    let local = locals.len();
    locals.push(ty);
    body.push(FStmt::Decl { ty, local, init });
    local
}

fn wide_local_prefix() -> (Vec<Ty>, Vec<FStmt>, usize) {
    let mut locals = Vec::with_capacity(CATEGORY_2_PADS + 4);
    let mut body = Vec::with_capacity(CATEGORY_2_PADS + 8);
    for _ in 0..CATEGORY_2_PADS {
        push_decl(&mut locals, &mut body, Ty::Long, None);
    }
    push_decl(&mut locals, &mut body, Ty::Int, None);
    let x = push_decl(&mut locals, &mut body, Ty::Int, Some(FExpr::Lit(Val::I(1))));
    (locals, body, x)
}

/// `x` begins at JVM slot 256. Each self-add is two wide loads, `iadd`, and a
/// wide store: 13 code bytes. The count contributes 32,747 bytes, while the
/// scenario tails select compacted branch offsets +32,767 and +32,768. The
/// matching exact fixtures are `LongBranchBoundary`, `LongBranchFat`, and
/// `LongGotoFat`.
fn self_adds(x: usize) -> Vec<FStmt> {
    (0..SELF_ADDS)
        .map(|_| FStmt::Compound {
            local: x,
            op: BinOp::Add,
            value: FExpr::Local(x),
        })
        .collect()
}

fn x_positive(x: usize) -> FExpr {
    FExpr::Cmp(
        CmpOp::Gt,
        Box::new(FExpr::Local(x)),
        Box::new(FExpr::Lit(Val::I(0))),
    )
}

fn negate_x(x: usize) -> FStmt {
    FStmt::Assign {
        local: x,
        value: FExpr::Neg(Box::new(FExpr::Local(x))),
    }
}

fn finish(index: u64, kind: CaseKind, locals: Vec<Ty>, body: Vec<FStmt>) -> Prog {
    Prog {
        name: ident(index),
        kind,
        locals,
        body,
    }
}

fn conditional_boundary(index: u64) -> Prog {
    let (locals, mut body, x) = wide_local_prefix();
    let mut then_b = self_adds(x);
    then_b.push(FStmt::Assign {
        local: x,
        value: FExpr::Local(x),
    });
    then_b.push(negate_x(x));
    body.push(FStmt::If {
        cond: x_positive(x),
        then_b,
        else_b: None,
    });
    body.push(FStmt::Println(PrintArg::Expr(FExpr::Local(x))));
    finish(index, CaseKind::LongConditionalBoundary, locals, body)
}

fn conditional_fat(index: u64) -> Prog {
    let (mut locals, mut body, x) = wide_local_prefix();
    let v1 = push_decl(&mut locals, &mut body, Ty::Int, Some(FExpr::Lit(Val::I(5))));
    let vb = push_decl(
        &mut locals,
        &mut body,
        Ty::Int,
        Some(FExpr::Cast(Ty::Byte, Box::new(FExpr::Local(v1)))),
    );

    let mut then_b = self_adds(x);
    then_b.push(negate_x(x));
    then_b.push(negate_x(x));
    body.push(FStmt::If {
        cond: x_positive(x),
        then_b,
        else_b: None,
    });

    let comparison = FExpr::Paren(Box::new(FExpr::Cmp(
        CmpOp::Gt,
        Box::new(FExpr::Local(vb)),
        Box::new(FExpr::Lit(Val::I(32766))),
    )));
    let inner = FExpr::Paren(Box::new(FExpr::Logic(
        LogOp::Or,
        Box::new(comparison),
        Box::new(FExpr::Lit(Val::Bool(false))),
    )));
    let cond = FExpr::Logic(
        LogOp::Or,
        Box::new(FExpr::Not(Box::new(inner))),
        Box::new(FExpr::Lit(Val::Bool(false))),
    );
    body.push(FStmt::If {
        cond,
        then_b: vec![FStmt::IncDec {
            local: v1,
            prefix: false,
            inc: true,
        }],
        else_b: None,
    });
    body.push(FStmt::IncDec {
        local: v1,
        prefix: false,
        inc: true,
    });
    body.push(FStmt::Println(PrintArg::Expr(FExpr::Local(x))));
    body.push(FStmt::Println(PrintArg::Expr(FExpr::Local(v1))));
    finish(index, CaseKind::LongConditionalFat, locals, body)
}

fn goto_fat(index: u64) -> Prog {
    let (locals, mut body, x) = wide_local_prefix();
    let then_b = vec![FStmt::IncDec {
        local: x,
        prefix: false,
        inc: true,
    }];
    let mut else_b = self_adds(x);
    else_b.push(negate_x(x));
    else_b.push(negate_x(x));
    body.push(FStmt::If {
        cond: x_positive(x),
        then_b,
        else_b: Some(else_b),
    });
    body.push(FStmt::Println(PrintArg::Expr(FExpr::Local(x))));
    finish(index, CaseKind::LongGotoFat, locals, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::{Gen, Rng};
    use crate::render::render;

    fn local_slot(locals: &[Ty], local: usize) -> usize {
        1 + locals[..local]
            .iter()
            .map(|ty| usize::from(matches!(ty, Ty::Long | Ty::Double)) + 1)
            .sum::<usize>()
    }

    fn assert_self_adds(stmts: &[FStmt], x: usize) {
        assert_eq!(stmts.len(), SELF_ADDS);
        assert!(stmts.iter().all(|stmt| matches!(
            stmt,
            FStmt::Compound {
                local,
                op: BinOp::Add,
                value: FExpr::Local(value),
            } if *local == x && *value == x
        )));
    }

    fn assert_negate(stmt: &FStmt, x: usize) {
        assert!(matches!(
            stmt,
            FStmt::Assign {
                local,
                value: FExpr::Neg(value),
            } if *local == x && matches!(value.as_ref(), FExpr::Local(value) if *value == x)
        ));
    }

    #[test]
    fn schedules_the_complete_long_branch_prefix() {
        assert_eq!(
            scheduled(0).unwrap().kind,
            CaseKind::LongConditionalBoundary
        );
        assert_eq!(scheduled(1).unwrap().kind, CaseKind::LongConditionalFat);
        assert_eq!(scheduled(2).unwrap().kind, CaseKind::LongGotoFat);
        assert!(scheduled(3).is_none());
    }

    #[test]
    fn pins_branch_distance_arithmetic() {
        let repeated = SELF_ADDS * WIDE_SELF_ADD_BYTES;
        assert_eq!(repeated + NARROW_BOUNDARY_TAIL_BYTES, i16::MAX as usize);
        assert_eq!(repeated + FAT_TRIGGER_TAIL_BYTES, i16::MAX as usize + 1);

        let boundary = scheduled(0).unwrap();
        let x = CATEGORY_2_PADS + 1;
        assert_eq!(
            boundary.locals[..CATEGORY_2_PADS],
            [Ty::Long; CATEGORY_2_PADS]
        );
        assert_eq!(boundary.locals[CATEGORY_2_PADS..], [Ty::Int, Ty::Int]);
        assert_eq!(local_slot(&boundary.locals, x), 256);
        let FStmt::If { then_b, .. } = &boundary.body[CATEGORY_2_PADS + 2] else {
            panic!("long conditional boundary lost its padding branch");
        };
        assert_eq!(then_b.len(), SELF_ADDS + 2);
        assert_self_adds(&then_b[..SELF_ADDS], x);
        assert!(matches!(
            &then_b[SELF_ADDS],
            FStmt::Assign { local, value: FExpr::Local(value) }
                if *local == x && *value == x
        ));
        assert_negate(&then_b[SELF_ADDS + 1], x);

        let conditional_fat = scheduled(1).unwrap();
        let FStmt::If { then_b, .. } = &conditional_fat.body[CATEGORY_2_PADS + 4] else {
            panic!("long conditional fat case lost its padding branch");
        };
        assert_eq!(then_b.len(), SELF_ADDS + 2);
        assert_self_adds(&then_b[..SELF_ADDS], x);
        assert_negate(&then_b[SELF_ADDS], x);
        assert_negate(&then_b[SELF_ADDS + 1], x);

        let goto_fat = scheduled(2).unwrap();
        let FStmt::If {
            else_b: Some(else_b),
            ..
        } = &goto_fat.body[CATEGORY_2_PADS + 2]
        else {
            panic!("long goto fat case lost its padding branch");
        };
        assert_eq!(else_b.len(), SELF_ADDS + 2);
        assert_self_adds(&else_b[..SELF_ADDS], x);
        assert_negate(&else_b[SELF_ADDS], x);
        assert_negate(&else_b[SELF_ADDS + 1], x);
    }

    #[test]
    fn scheduled_prefix_preserves_the_later_random_stream() {
        let seed = 0x6A09_E667_F3BC_C909;
        let mut mixed = Gen {
            rng: Rng::new(seed),
        };
        let mut random = Gen {
            rng: Rng::new(seed),
        };

        for index in 0..10 {
            let actual = mixed.gen_prog(index);
            let expected = random.gen_random_prog(index);
            if scheduled(index).is_none() {
                assert_eq!(render(&actual), render(&expected));
            }
        }
    }
}
