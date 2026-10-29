use super::*;
use crate::runtime::group::TickPlan;
use crate::runtime::skeleton_tests::basic_spec;
use std::cell::Cell;

fn world() -> World {
    let mut spec = basic_spec(3, 2, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.initial_cash = 100;
    World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar().unwrap(),
    )
    .expect("真实三组 World 应成功构造")
}

fn stages(debit: i64) -> Vec<AdminOperationStage> {
    vec![
        AdminOperationStage {
            operations: vec![(
                1,
                vec![AdminOperation::CashDividend {
                    local: 1,
                    stock: 0,
                    amount: 40,
                }],
            )],
        },
        AdminOperationStage {
            operations: vec![(
                1,
                vec![
                    AdminOperation::WithholdingTax {
                        local: 1,
                        stock: 1,
                        amount: 25,
                    },
                    AdminOperation::WithholdingTax {
                        local: 1,
                        stock: 0,
                        amount: 5,
                    },
                ],
            )],
        },
        AdminOperationStage {
            operations: vec![(
                1,
                vec![AdminOperation::PaymentDebit {
                    local: 1,
                    stock: 0,
                    amount: debit,
                }],
            )],
        },
    ]
}

#[test]
fn r4_stage_01_credit_tax_debit_uses_real_working_stage_accounts() {
    let mut world = world();
    let before = world.snapshot_accounts().unwrap();
    let inspections = Cell::new(0);
    let projection = world
        .with_staged_admin_transaction(stages(10), None, |candidate, evidence| {
            inspections.set(inspections.get() + 1);
            assert_eq!(
                evidence.operation_indices,
                vec![(1, 0, 0), (1, 1, 1), (1, 1, 2), (1, 2, 3)]
            );
            assert_eq!(evidence.tax_stages.len(), 1);
            let tax = &evidence.tax_stages[0];
            assert_eq!(tax.stage_index, 1);
            assert_eq!(tax.groups.len(), 1);
            let group = &tax.groups[0];
            assert_eq!((group.group_index, group.group), (0, 1));
            assert_eq!(group.accounts.len(), 1);
            let (local, actual_before, actual_after) = &group.accounts[0];
            assert_eq!(*local, 1);
            let mut expected_before = before[1].1[1].clone();
            expected_before.cash = 140;
            let mut expected_after = expected_before.clone();
            expected_after.cash = 110;
            assert_eq!(actual_before.as_ref(), &expected_before);
            assert_eq!(actual_after.as_ref(), &expected_after);
            assert_eq!(evidence.tax_projections.len(), 1);
            let projection = &evidence.tax_projections[0];
            assert_eq!(projection.id, candidate.id);
            assert_eq!(projection.stage_index, Some(1));
            assert_eq!(
                projection
                    .operations
                    .iter()
                    .map(|op| (
                        op.group_index,
                        op.group,
                        op.operation_index,
                        op.local,
                        op.stock,
                        op.amount
                    ))
                    .collect::<Vec<_>>(),
                vec![(0, 1, 1, 1, 1, 25), (0, 1, 2, 1, 0, 5)]
            );
            Ok(projection.clone())
        })
        .expect("R4 stage 真实 credit→tax→debit 必须共用同 pending 并成功提交");
    assert_eq!(inspections.get(), 1, "只能在最终候选 inspect 一次");
    assert_eq!(
        projection
            .operations
            .iter()
            .map(|op| op.amount)
            .sum::<i64>(),
        30
    );
    assert_eq!(
        world.snapshot_accounts().unwrap(),
        before,
        "净额相等不能替代中间税阶段来源"
    );
    world.shutdown().unwrap();
}

#[test]
fn r4_stage_02_late_debit_rejection_aborts_credit_and_tax() {
    let mut world = world();
    let before = world.snapshot_accounts().unwrap();
    let called = Cell::new(false);
    let error = world
        .with_staged_admin_transaction(stages(111), None, |_, _| {
            called.set(true);
            Ok(())
        })
        .expect_err("真实尾阶段现金不足应拒绝");
    assert!(
        error.to_string().contains("PaymentDebit")
            && error.to_string().contains("InsufficientCash")
            && error.to_string().contains("requested: 111")
            && error.to_string().contains("available: 110"),
        "尾阶段必须由真实扣款拒绝，不能被未实现入口吞掉：{error}"
    );
    assert!(!called.get(), "失败阶段不得到最终 inspect");
    assert_eq!(world.snapshot_accounts().unwrap(), before);
    world
        .with_staged_admin_transaction(stages(10), None, |_, _| Ok(()))
        .expect("业务拒绝不应杀死健康 World");
    assert_eq!(world.snapshot_accounts().unwrap(), before);
    world.shutdown().unwrap();
}

#[test]
fn r4_stage_03_final_inspect_rejection_aborts_tax_and_keeps_original_cause() {
    let mut world = world();
    let before = world.snapshot_accounts().unwrap();
    let called = Cell::new(0);
    let error = world
        .with_staged_admin_transaction(stages(10), None, |_, evidence| {
            called.set(called.get() + 1);
            assert_eq!(evidence.tax_projections.len(), 1);
            Err::<(), _>(invalid("R4-stage-final-inspect-original-cause"))
        })
        .expect_err("最终 inspect 应拒绝整个真实阶段候选");
    assert!(
        error
            .to_string()
            .contains("R4-stage-final-inspect-original-cause"),
        "最终 inspect 原因必须保留，不能被未实现入口吞掉：{error}"
    );
    assert_eq!(called.get(), 1);
    assert_eq!(world.snapshot_accounts().unwrap(), before);
    world
        .with_staged_admin_transaction(stages(10), None, |_, _| Ok(()))
        .expect("inspect 业务拒绝之后合法事务应仍可成功");
    assert_eq!(world.snapshot_accounts().unwrap(), before);
    world.shutdown().unwrap();
}

#[test]
fn r4_stage_04_join_borrows_original_operands_and_rejects_corrupt_indices() {
    let mut w = world();
    let request = stages(10);
    let pointers: Vec<_> = request
        .iter()
        .flat_map(|stage| &stage.operations)
        .flat_map(|(_, operations)| operations)
        .map(std::ptr::from_ref)
        .collect();
    w.with_staged_admin_transaction(request, None, |_, prepared| {
        assert_eq!(
            prepared
                .operands()
                .iter()
                .map(|row| std::ptr::from_ref(row.operation))
                .collect::<Vec<_>>(),
            pointers,
            "join必须借World保留的原typed请求，不得回执另造operand"
        );
        assert_eq!(
            prepared
                .operands()
                .iter()
                .map(|row| (row.group, row.stage_index, row.operation_index))
                .collect::<Vec<_>>(),
            vec![(1, 0, 0), (1, 1, 1), (1, 1, 2), (1, 2, 3)]
        );
        Ok(())
    })
    .unwrap();
    w.shutdown().unwrap();
    for mode in 0..6 {
        let mut w = world();
        w.group_txs[1]
            .send(GroupIn::AdminFaultForTest {
                phase: AdminPhase::Prepare,
                fault: crate::runtime::protocol::AdminFault::StagePrepareCorruption(mode),
            })
            .unwrap();
        let inspected = Cell::new(false);
        let error = w
            .with_staged_admin_transaction(stages(10), None, |_, _| {
                inspected.set(true);
                Ok(())
            })
            .expect_err("损坏的真实stage回执必须拒绝");
        assert!(
            !inspected.get(),
            "协议来源损坏不能进入用户inspect：mode={mode}"
        );
        assert!(
            w.dead_reason.is_some(),
            "协议来源损坏必须terminal：mode={mode} {error}"
        );
        assert!(
            w.handles.is_empty(),
            "terminal必须join全部worker：mode={mode}"
        );
    }
}
