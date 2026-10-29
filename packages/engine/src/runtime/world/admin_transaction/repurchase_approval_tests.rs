//! APPROVAL-C 真实 World 首红：同事务两组创建、晚拒绝与控制相位回执。
use super::*;
use crate::company::CompanyId;
use crate::runtime::group::TickPlan;
use crate::runtime::information_fixture as fixture;
use crate::runtime::protocol::{
    AdminFault, AdminPlanReference, AdminTreasuryBinding, RepurchaseExecutionProjection,
    RepurchaseMaterialRestriction, RepurchasePreparePayload, RepurchaseProjectionInput,
};
use crate::runtime::skeleton_tests::basic_spec;
use crate::{AccountId, Money};
fn world() -> World {
    let mut spec = basic_spec(3, 2, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.civil_start_date = fixture::at(9, 0).date();
    spec.calendar_start_date = spec.civil_start_date;
    spec.ticks_per_day = 100;
    spec.root_projection = Some(fixture::projection(
        [11, 37, 51, 79, 113].map(AccountId).to_vec(),
    ));
    spec.initial_cash_override = Some(vec![vec![0; 2]; 3]);
    spec.initial_shares = vec![vec![vec![0; 2]; 2]; 3];
    spec.admin_treasury_bindings = vec![
        AdminTreasuryBinding {
            owner: (1, 1),
            account: AccountId(79),
            issuer: CompanyId(0),
            stock: 0,
        },
        AdminTreasuryBinding {
            owner: (2, 0),
            account: AccountId(113),
            issuer: CompanyId(1),
            stock: 1,
        },
    ];
    spec.watchdog = WatchdogConfig {
        normal: Duration::from_millis(300),
        long: Duration::from_millis(300),
    };
    World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .unwrap()
}
fn ops(local: u32, stock: u16, amount: i64) -> Vec<AdminOperation> {
    vec![
        AdminOperation::OpenIssuerRepurchase {
            local,
            stock,
            issuer: CompanyId(u32::from(stock)),
        },
        AdminOperation::SyntheticBudgetCredit {
            local,
            stock,
            execution_plan: AdminPlanReference::CreatedInThisPrepare { operation_index: 0 },
            amount,
        },
    ]
}
fn projection(
    local: u32,
    stock: u16,
    account: AccountId,
    amount: i64,
) -> RepurchaseProjectionInput {
    let day = fixture::at(9, 0).date();
    RepurchaseProjectionInput {
        local,
        stock,
        issuer: CompanyId(u32::from(stock)),
        execution_plan: AdminPlanReference::CreatedInThisPrepare { operation_index: 0 },
        projection: RepurchaseExecutionProjection {
            account,
            issuer: CompanyId(u32::from(stock)),
            stock,
            announced: false,
            announced_on: day,
            window_start_on: day,
            window_deadline_on: day,
            exchange: crate::calendar::CalendarExchange::Szse,
            purpose: crate::company::issuer_repurchase::RepurchasePurpose::ReduceCapital,
            funding_amount: Money::from_cents(amount),
            price_cap: Money::from_cents(1234),
            max_shares: 1000,
            unit_as_of: day,
            unit_revision: 0,
            book_through_cursor: 0,
            book_gross: Money::ZERO,
            book_fees: Money::ZERO,
            recycled: Money::ZERO,
            inventory_shares: 0,
            last_order_day: None,
            eligibility_revision: 7,
            eligibility_as_of: fixture::at(9, 0),
            issuance_in_progress: Some(false),
            material_restriction: RepurchaseMaterialRestriction::NotModelled,
        },
    }
}
fn payload() -> RepurchasePreparePayload {
    RepurchasePreparePayload {
        groups: vec![
            (1, vec![projection(1, 0, AccountId(79), 101)]),
            (2, vec![projection(0, 1, AccountId(113), 203)]),
        ],
    }
}
#[test]
fn approval_c_world_01_two_groups_open_credit_commit_one_source_and_next_tick() {
    let mut world = world();
    let source = world.mirror_source();
    let published = world.publication.load();
    let ids = world
        .with_repurchase_transaction(
            vec![(1, ops(1, 0, 101)), (2, ops(0, 1, 203))],
            None,
            payload(),
            |candidate| {
                assert_eq!(candidate.groups.len(), 2);
                let mut ids = Vec::new();
                for group in &candidate.groups {
                    let prepared = group
                        .repurchase
                        .as_ref()
                        .expect("同 prepare 必须有完整 book/projection");
                    assert!(group.mirror_domains.plans_pending);
                    assert!(group.mirror_domains.accounts_full);
                    assert_eq!(prepared.created.len(), 1);
                    assert_eq!(prepared.created[0].operation_index, 0);
                    assert_eq!(prepared.book.plan_ids().count(), 1);
                    assert_eq!(prepared.projections.len(), 1);
                    let created = &prepared.created[0];
                    assert_eq!(created.group, group.group);
                    assert_eq!(
                        prepared.book.entry(created.plan_id).unwrap().plan_id(),
                        created.plan_id
                    );
                    assert_eq!(
                        group.accounts[0].1.cash,
                        if group.group == 1 { 101 } else { 203 }
                    );
                    ids.push(created.plan_id);
                }
                Ok(ids)
            },
        )
        .unwrap();
    assert_eq!(
        ids.iter().map(|id| id.owner).collect::<Vec<_>>(),
        vec![AccountId(79), AccountId(113)]
    );
    assert_eq!(
        world.mirror_source().non_tick_revision,
        source.non_tick_revision + 1
    );
    assert!(world.pending_mirror_domains().plans_pending);
    assert!(Arc::ptr_eq(&published, &world.publication.load()));
    let plans = world.collect_plans().unwrap();
    assert_eq!(
        plans
            .iter()
            .map(|(_, b)| b.plan_ids().count())
            .sum::<usize>(),
        2
    );
    world.run_ticks(1).unwrap();
    let accounts = world.snapshot_accounts().unwrap();
    assert_eq!(accounts[1].1[1].cash, 101);
    assert_eq!(accounts[2].1[0].cash, 203);
    world.shutdown().unwrap();
}
#[test]
fn approval_c_world_02_late_group_and_inspect_reject_restore_sequence_money_and_source() {
    for inspect_reject in [false, true] {
        let mut world = world();
        let source = world.mirror_source();
        let pending = world.pending_mirror_domains();
        let before = world.snapshot_accounts().unwrap();
        let plans = world.collect_plans().unwrap();
        let mut inspected = false;
        let result = world.with_repurchase_transaction(
            vec![
                (1, ops(1, 0, 101)),
                (2, ops(0, 1, if inspect_reject { 203 } else { 0 })),
            ],
            None,
            payload(),
            |_| {
                inspected = true;
                Err::<(), _>(invalid("完整公司候选 inspect 拒绝"))
            },
        );
        assert!(result.is_err());
        assert_eq!(inspected, inspect_reject, "合法两组必须到同一次 inspect");
        assert_eq!(world.snapshot_accounts().unwrap(), before);
        assert_eq!(world.collect_plans().unwrap(), plans);
        assert_eq!(world.mirror_source(), source);
        assert_eq!(world.pending_mirror_domains(), pending);
        assert!(world.dead_reason.is_none());
        world
            .with_repurchase_transaction(
                vec![(1, ops(1, 0, 101)), (2, ops(0, 1, 203))],
                None,
                payload(),
                |candidate| {
                    for group in &candidate.groups {
                        assert_eq!(
                            group.repurchase.as_ref().unwrap().created[0]
                                .plan_id
                                .sequence,
                            0
                        );
                    }
                    Ok(())
                },
            )
            .unwrap();
        world.shutdown().unwrap();
    }
}
#[test]
fn approval_c_world_03_control_receipts_must_not_contain_prepare_repurchase_candidates() {
    for phase in [AdminPhase::Commit, AdminPhase::Abort] {
        let mut world = world();
        world.group_txs[0]
            .send(GroupIn::AdminFaultForTest {
                phase,
                fault: AdminFault::RepurchaseInControlReceipt,
            })
            .unwrap();
        let result = world.with_admin_transaction(
            vec![(
                0,
                vec![AdminOperation::CashDividend {
                    local: 0,
                    stock: 0,
                    amount: 7,
                }],
            )],
            |_| {
                if phase == AdminPhase::Abort {
                    Err(invalid("明确触发 Abort"))
                } else {
                    Ok(())
                }
            },
        );
        assert!(result.is_err(), "控制相位额外 Prepare 候选不得被忽略");
        assert!(
            world.dead_reason.is_some(),
            "坏控制回执必须走既有 terminal，不能伪报业务未发生"
        );
    }
}
#[test]
fn approval_c_world_04_prepare_created_book_and_projection_corruption_are_transport_errors() {
    for mode in 0..8 {
        let mut world = world();
        let source = world.mirror_source();
        let published = world.publication.load();
        world.group_txs[1]
            .send(GroupIn::AdminFaultForTest {
                phase: AdminPhase::Prepare,
                fault: AdminFault::RepurchasePrepareCorruption(mode),
            })
            .unwrap();
        let result = world.with_repurchase_transaction(
            vec![(1, ops(1, 0, 101)), (2, ops(0, 1, 203))],
            None,
            payload(),
            |_| -> Result<(), RuntimeError> {
                panic!("损坏的完整候选不得进入 Session inspect")
            },
        );
        assert!(result.is_err());
        assert!(
            world.dead_reason.is_some(),
            "协议候选损坏必须 terminal：mode={mode}"
        );
        assert_eq!(world.mirror_source(), source);
        assert!(Arc::ptr_eq(&published, &world.publication.load()));
    }
}
