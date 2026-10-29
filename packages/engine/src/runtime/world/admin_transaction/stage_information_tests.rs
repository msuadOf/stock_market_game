use super::*;
use crate::runtime::group::TickPlan;
use crate::runtime::personal_full_validation::PersonalPresence;
use crate::runtime::protocol::AdminOperation;
use crate::runtime::skeleton_tests::basic_spec;
use crate::{AccountId, StockId};
use fixture::*;
use std::sync::Arc;
use std::time::Duration;

#[path = "../../information_fixture.rs"]
mod fixture;

fn world() -> World {
    let ids = [11, 37, 51, 79, 113].map(AccountId);
    let mut spec = basic_spec(3, 2, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.initial_cash = 100;
    spec.ticks_per_day = 100;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 0;
    spec.calendar_start_date = at(0, 0).date();
    spec.civil_start_date = at(0, 0).date();
    spec.root_projection = Some(projection(ids.to_vec()));
    spec.belief_seeds = ids
        .iter()
        .enumerate()
        .map(|(index, &id)| {
            let mut snapshot = personal(id);
            snapshot
                .watchlist
                .stocks
                .insert(StockId(if index == 4 { 1 } else { 0 }), Default::default());
            Some(snapshot)
        })
        .collect();
    spec.watchdog = crate::runtime::watchdog::WatchdogConfig {
        normal: Duration::from_millis(300),
        long: Duration::from_millis(300),
    };

    let mut world =
        World::spawn_with_calendar_source(spec, calendar_source(), calendar_bindings()).unwrap();
    let authority_epoch = world.mirror_source().authority_epoch;
    let roster = ids
        .chunks(2)
        .enumerate()
        .map(|(group, owners)| {
            (
                group as u16,
                owners
                    .iter()
                    .copied()
                    .map(|owner| (owner, PersonalPresence::Required))
                    .collect(),
            )
        })
        .collect();
    world.install_personal_presence(authority_epoch, roster);
    world
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
fn r4_stage_05_late_rejection_preserves_information_and_world_source() {
    let mut world = world();
    let mut information = payload(true);
    let announcement_id = announcement(&mut information, 0);
    let before_accounts = world.snapshot_accounts().unwrap();
    let before_plans = world.collect_plans().unwrap();
    let before_personals = world.collect_personal_full().unwrap().groups;
    let before_source = world.mirror_source();
    let before_pending = world.pending_mirror_domains();

    let insufficient = world
        .with_staged_admin_transaction(stages(111), Some(information.clone()), |_, _| -> Result<(), RuntimeError> {
            panic!("尾阶段现金不足时不得调用 inspect")
        })
        .expect_err("末扣 111 应因可用现金不足而拒绝");
    assert!(
        insufficient.to_string().contains("InsufficientCash"),
        "应保留 PaymentDebit 的资金不足原因：{insufficient}"
    );
    assert!(insufficient.to_string().contains("PaymentDebit"));
    assert!(world.dead_reason.is_none(), "业务拒绝后 World 应保持健康");
    assert_eq!(world.snapshot_accounts().unwrap(), before_accounts);
    assert_eq!(world.collect_plans().unwrap(), before_plans);
    assert_eq!(
        world.collect_personal_full().unwrap().groups,
        before_personals
    );
    assert_eq!(world.mirror_source(), before_source);
    assert_eq!(world.pending_mirror_domains(), before_pending);

    let rejected = world
        .with_staged_admin_transaction(stages(10), Some(information.clone()), |_, _| {
            Err::<(), _>(invalid("R4-stage-info-final-reject"))
        })
        .expect_err("最终 inspect 的业务拒绝应撤销全部阶段");
    assert!(rejected.to_string().contains("R4-stage-info-final-reject"));
    assert!(
        world.dead_reason.is_none(),
        "inspect 业务拒绝后 World 应保持健康"
    );
    assert_eq!(world.snapshot_accounts().unwrap(), before_accounts);
    assert_eq!(world.collect_plans().unwrap(), before_plans);
    assert_eq!(
        world.collect_personal_full().unwrap().groups,
        before_personals
    );
    assert_eq!(world.mirror_source(), before_source);
    assert_eq!(world.pending_mirror_domains(), before_pending);

    world
        .with_staged_admin_transaction(stages(10), Some(information.clone()), |candidate, _| {
            assert_eq!(candidate.groups.len(), 3);
            assert_eq!(
                candidate
                    .groups
                    .iter()
                    .map(|group| group.information.as_ref().unwrap().personals.len())
                    .collect::<Vec<_>>(),
                [2, 2, 1]
            );
            for group in &candidate.groups {
                let prepared = group.information.as_ref().unwrap();
                assert!(Arc::ptr_eq(&prepared.library, &information.library));
                assert!(Arc::ptr_eq(
                    &prepared.current_facts,
                    &information.current_facts
                ));
                for (_, owner, snapshot) in &prepared.personals {
                    if *owner != AccountId(113) {
                        assert_eq!(
                            snapshot.information.observed_at_of(announcement_id),
                            Some(at(19, 0))
                        );
                    }
                }
            }
            Ok(())
        })
        .expect("合法重试应在同一健康 World 上提交");

    let observed = world.collect_personal_full().unwrap();
    for (_, slots) in observed.groups {
        for (_, owner, personal) in slots {
            if owner != AccountId(113) {
                assert_eq!(
                    personal
                        .unwrap()
                        .information
                        .observed_at_of(announcement_id),
                    Some(at(19, 0)),
                    "成功 Commit 后完整个人态应能读取已观察公告"
                );
            }
        }
    }
    let after_accounts = world.snapshot_accounts().unwrap();
    assert_eq!(after_accounts, before_accounts);
    assert_eq!(after_accounts[1].1[1].cash, 100);
    assert!(world.dead_reason.is_none());
    world.shutdown().unwrap();
}
