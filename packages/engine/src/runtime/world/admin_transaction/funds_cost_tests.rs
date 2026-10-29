//! FUNDS-P1 正式通道 fixture：复用唯一行政事务验证资金、实缴成本与 INFO 的共同原子性。

use super::*;
use crate::runtime::group::{GroupAccount, TickPlan};
use crate::runtime::protocol::{AdminOperation, GroupIn, UpMsg};
use crate::runtime::skeleton_tests::basic_spec;

fn spec() -> WorldSpec {
    let mut spec = basic_spec(2, 1, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 100;
    spec.watchdog = WatchdogConfig {
        normal: Duration::from_millis(300),
        long: Duration::from_millis(300),
    };
    spec.initial_cash_override = Some(vec![vec![2400; 2]; 2]);
    spec.initial_shares = vec![vec![vec![20]; 2]; 2];
    spec.initial_invested = vec![vec![vec![10_000]; 2]; 2];
    spec.initial_recovered = vec![vec![vec![2000]; 2]; 2];
    spec
}

fn accounts(world: &mut World) -> Vec<Vec<GroupAccount>> {
    for tx in &world.group_txs {
        tx.send(GroupIn::EchoAccounts).unwrap();
    }
    let mut result = vec![Vec::new(); world.n_groups];
    for _ in 0..world.n_groups {
        match recv_deadline(&world.up_rx, Phase::Seal, &world.watchdog).unwrap() {
            UpMsg::GroupEcho { group, accounts } => result[group as usize] = accounts,
            other => panic!("P1真实账户回声错位：{other:?}"),
        }
    }
    result
}

fn debit(local: u32, amount: i64) -> AdminOperation {
    AdminOperation::PaymentDebit {
        local,
        stock: 0,
        amount,
    }
}
fn shares(local: u32, qty: u32, subscription_cost: i64) -> AdminOperation {
    AdminOperation::SharesWithCost {
        local,
        stock: 0,
        qty,
        subscription_cost,
    }
}

#[test]
fn funds_p1_01_two_groups_commit_exact_cash_and_cost_then_real_next_tick_keeps_it() {
    let mut world = World::spawn(
        spec(),
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .unwrap();
    let before = accounts(&mut world);
    let mut expected = before.clone();
    expected[0][0].cash = 0;
    expected[0][1].cash += 5600;
    expected[1][1].shares[0] += 7;
    expected[1][1].invested_cents[0] += 5600;
    expected[1][1].ever_held[0] = false;
    let candidate = world
        .with_admin_transaction(
            vec![
                (
                    0,
                    vec![
                        debit(0, 2400),
                        AdminOperation::SubscriptionRefund {
                            local: 1,
                            stock: 0,
                            amount: 5600,
                        },
                    ],
                ),
                (1, vec![shares(1, 7, 5600)]),
            ],
            |candidate| {
                assert_eq!(candidate.groups.len(), 2);
                assert_eq!(
                    candidate.groups[0]
                        .accounts
                        .iter()
                        .map(|(local, _)| *local)
                        .collect::<Vec<_>>(),
                    vec![0, 1]
                );
                assert_eq!(
                    candidate.groups[1]
                        .accounts
                        .iter()
                        .map(|(local, _)| *local)
                        .collect::<Vec<_>>(),
                    vec![1]
                );
                for group in &candidate.groups {
                    for (local, actual) in &group.accounts {
                        assert_eq!(
                            actual.as_ref(),
                            &expected[group.group as usize][*local as usize]
                        );
                    }
                }
                Ok(candidate.clone())
            },
        )
        .unwrap();
    assert_eq!(candidate.groups.len(), 2);
    assert_eq!(accounts(&mut world), expected);
    world.run_ticks(1).unwrap();
    assert_eq!(accounts(&mut world), expected);
    world.shutdown().unwrap();
}

#[test]
fn funds_p1_02_second_group_insufficient_cash_aborts_first_group_without_partial_payment() {
    let mut world = World::spawn(
        spec(),
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .unwrap();
    let before = accounts(&mut world);
    let mut inspected = false;
    let error = world
        .with_admin_transaction(
            vec![(0, vec![debit(0, 2000)]), (1, vec![debit(1, 2401)])],
            |_| {
                inspected = true;
                Ok(())
            },
        )
        .unwrap_err();
    assert!(!inspected);
    let detail = error.to_string();
    assert!(
        detail.contains("2401") && detail.contains("2400"),
        "错误必须保留真实申请与可用现金：{detail}"
    );
    assert_eq!(accounts(&mut world), before);
    world
        .with_admin_transaction(
            vec![(0, vec![debit(0, 2000)]), (1, vec![debit(1, 2400)])],
            |_| Ok(()),
        )
        .unwrap();
    let mut expected = before;
    expected[0][0].cash = 400;
    expected[1][1].cash = 0;
    assert_eq!(accounts(&mut world), expected);
    world.shutdown().unwrap();
}

#[test]
fn funds_p1_03_inspect_rejection_restores_all_cash_cost_frozen_and_pending_fields() {
    let mut world = World::spawn(
        spec(),
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .unwrap();
    let before = accounts(&mut world);
    assert!(world
        .with_admin_transaction(
            vec![(0, vec![shares(0, 7, 5600)]), (1, vec![debit(1, 2000)])],
            |_| {
                Err::<(), _>(RuntimeError::ProtocolInvariant {
                    detail: "P1候选勾稽拒绝".into(),
                })
            }
        )
        .is_err());
    assert_eq!(accounts(&mut world), before);
    world.run_ticks(1).unwrap();
    assert_eq!(accounts(&mut world), before);
    world.shutdown().unwrap();
}

#[test]
fn funds_p1_04_same_group_late_failure_does_not_keep_earlier_new_shares_or_cost() {
    let mut world = World::spawn(
        spec(),
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .unwrap();
    let before = accounts(&mut world);
    assert!(world
        .with_admin_transaction(vec![(0, vec![shares(0, 7, 5600), debit(1, 2401)])], |_| Ok(
            ()
        ))
        .is_err());
    assert_eq!(accounts(&mut world), before);
    world.shutdown().unwrap();
}

#[test]
fn funds_p1_05_cost_overflow_in_second_group_restores_other_group_payment() {
    let mut spec = spec();
    spec.initial_invested[1][1][0] = i64::MAX;
    let mut world = World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .unwrap();
    let before = accounts(&mut world);
    assert!(world
        .with_admin_transaction(
            vec![(0, vec![debit(0, 2400)]), (1, vec![shares(1, 7, 5600)])],
            |_| Ok(())
        )
        .is_err());
    assert_eq!(accounts(&mut world), before);
    world.shutdown().unwrap();
}

#[test]
fn funds_p1_06_zero_old_position_gets_original_paid_cost_and_remains_unfrozen_after_tick() {
    let mut spec = spec();
    spec.initial_shares[1][1][0] = 0;
    spec.initial_invested[1][1][0] = 0;
    spec.initial_recovered[1][1][0] = 0;
    let mut world = World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .unwrap();
    let before = accounts(&mut world);
    world
        .with_admin_transaction(vec![(1, vec![shares(1, 7, 5600)])], |_| Ok(()))
        .unwrap();
    let mut expected = before;
    expected[1][1].shares[0] = 7;
    expected[1][1].invested_cents[0] = 5600;
    expected[1][1].ever_held[0] = false;
    assert_eq!(accounts(&mut world), expected);
    world.run_ticks(1).unwrap();
    assert_eq!(accounts(&mut world), expected);
    assert_eq!(expected[1][1].frozen_today[0], 0);
    world.shutdown().unwrap();
}

#[test]
fn funds_p1_07_real_resting_buy_purse_cannot_be_used_for_subscription() {
    use crate::orderbook::Side;
    use crate::runtime::group::PlaceSpec;
    let mut spec = spec();
    spec.initial_cash_override = Some(vec![vec![200_000; 2]; 2]);
    spec.script_for = Box::new(|group| {
        Box::new(move |tick, _| {
            let mut plan = TickPlan::default();
            if group == 0 && tick == 1 {
                plan.places.push(PlaceSpec {
                    local: 0,
                    stock: 0,
                    side: Side::Buy,
                    price: 1000,
                    qty: 100,
                });
            }
            plan
        })
    });
    let mut world = World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .unwrap();
    world.run_ticks(1).unwrap();
    let before = accounts(&mut world);
    assert!(
        before[0][0].buy_purse_held_cents > 0,
        "正控必须真正挂出有purse的买单"
    );
    let resting_before = world.collect_desk_resting().unwrap();
    let requested = before[0][0].cash.checked_add(1).unwrap();
    assert!(
        requested
            <= before[0][0]
                .cash
                .checked_add(before[0][0].buy_purse_held_cents)
                .unwrap()
    );
    assert!(world
        .with_admin_transaction(vec![(0, vec![debit(0, requested)])], |_| Ok(()))
        .is_err());
    assert_eq!(accounts(&mut world), before);
    assert_eq!(world.collect_desk_resting().unwrap(), resting_before);
    world.shutdown().unwrap();
}

#[test]
fn funds_p1_08_same_local_debit_before_refund_cannot_be_netted_or_reordered() {
    let mut spec = spec();
    spec.initial_cash_override = Some(vec![vec![100; 2]; 2]);
    let mut world = World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .unwrap();
    let before = accounts(&mut world);
    let refund = || AdminOperation::SubscriptionRefund {
        local: 0,
        stock: 0,
        amount: 100,
    };
    let mut inspected = false;
    let error = world
        .with_admin_transaction(vec![(0, vec![debit(0, 150), refund()])], |_| {
            inspected = true;
            Ok(())
        })
        .unwrap_err();
    assert!(!inspected);
    let detail = error.to_string();
    assert!(
        detail.contains("150") && detail.contains("100"),
        "顺序付款失败须保留本步申请与可用现金：{detail}"
    );
    assert_eq!(accounts(&mut world), before);
    world
        .with_admin_transaction(vec![(0, vec![refund(), debit(0, 150)])], |_| Ok(()))
        .unwrap();
    let mut expected = before;
    expected[0][0].cash = 50;
    assert_eq!(accounts(&mut world), expected);
    world.run_ticks(1).unwrap();
    assert_eq!(accounts(&mut world), expected);
    world.shutdown().unwrap();
}

#[path = "../../information_fixture.rs"]
mod information_fixture;

fn information_world() -> World {
    use crate::AccountId;
    let ids = [11, 37, 51, 79, 113].map(AccountId);
    let mut spec = basic_spec(3, 2, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 100;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 0;
    spec.calendar_start_date = information_fixture::at(0, 0).date();
    spec.civil_start_date = information_fixture::at(0, 0).date();
    spec.root_projection = Some(information_fixture::projection(ids.to_vec()));
    spec.belief_seeds = ids
        .iter()
        .enumerate()
        .map(|(index, &id)| {
            let mut personal = information_fixture::personal(id);
            personal.watchlist.stocks.insert(
                crate::StockId(if index == 4 { 1 } else { 0 }),
                Default::default(),
            );
            Some(personal)
        })
        .collect();
    spec.watchdog = WatchdogConfig {
        normal: Duration::from_millis(300),
        long: Duration::from_millis(300),
    };
    World::spawn_with_calendar_source(
        spec,
        information_fixture::calendar_source(),
        information_fixture::calendar_bindings(),
        #[cfg(all(target_family = "wasm", feature = "verification-harness"))]
        None,
    )
    .unwrap()
}

fn assert_information_candidate(
    candidate: &AdminCandidate,
    publication: crate::information::PublicationId,
) {
    assert_eq!(
        candidate
            .groups
            .iter()
            .map(|group| group.information.as_ref().unwrap().personals.len())
            .collect::<Vec<_>>(),
        vec![2, 2, 1],
        "同观察点重试必须仍生成全部个人候选，不能提前消费 cadence"
    );
    for group in &candidate.groups {
        for (_, account, personal) in &group.information.as_ref().unwrap().personals {
            assert_eq!(personal.information.owner(), *account);
            if *account != crate::AccountId(113) {
                assert_eq!(
                    personal.information.observed_at_of(publication),
                    Some(information_fixture::at(19, 0))
                );
            }
        }
    }
}

#[test]
fn funds_p1_09_information_and_paid_share_cost_abort_together_on_last_group_rejection() {
    let mut world = information_world();
    let before = accounts(&mut world);
    let published_before = world.publication.load();
    let committed_tick_before = world.global.committed_tick;
    let mut payload = information_fixture::payload(true);
    let publication = information_fixture::announcement(&mut payload, 0);
    let mut inspected = false;
    assert!(world
        .with_information_transaction(
            vec![
                (0, vec![shares(0, 7, 5600)]),
                (2, vec![debit(0, before[2][0].cash.checked_add(1).unwrap())]),
            ],
            payload.clone(),
            |_| {
                inspected = true;
                Ok(())
            },
        )
        .is_err());
    assert!(!inspected);
    assert!(world.dead_reason.is_none());
    assert_eq!(accounts(&mut world), before);
    assert!(
        Arc::ptr_eq(&published_before, &world.publication.load()),
        "同overlay拒绝不得撤回已发布PublishedTick"
    );
    assert_eq!(world.global.committed_tick, committed_tick_before);
    let mut expected = before;
    expected[0][0].shares[0] += 7;
    expected[0][0].invested_cents[0] += 5600;
    expected[0][0].ever_held[0] = false;
    world
        .with_information_transaction(vec![(0, vec![shares(0, 7, 5600)])], payload, |candidate| {
            assert_information_candidate(candidate, publication);
            assert_eq!(candidate.groups[0].accounts[0].0, 0);
            assert_eq!(candidate.groups[0].accounts[0].1.as_ref(), &expected[0][0]);
            Ok(())
        })
        .unwrap();
    assert_eq!(accounts(&mut world), expected);
    assert_eq!(world.global.committed_tick, 0);
    world.shutdown().unwrap();
}

#[test]
fn funds_p1_10_information_inspect_rejection_restores_cost_and_observation_before_retry() {
    let mut world = information_world();
    let before = accounts(&mut world);
    let published_before = world.publication.load();
    let committed_tick_before = world.global.committed_tick;
    let mut expected = before.clone();
    expected[0][0].shares[0] += 7;
    expected[0][0].invested_cents[0] += 5600;
    expected[0][0].ever_held[0] = false;
    let mut payload = information_fixture::payload(true);
    let publication = information_fixture::announcement(&mut payload, 0);
    let error = world
        .with_information_transaction(
            vec![(0, vec![shares(0, 7, 5600)])],
            payload.clone(),
            |candidate| {
                assert_information_candidate(candidate, publication);
                assert_eq!(candidate.groups[0].accounts[0].1.as_ref(), &expected[0][0]);
                Err::<(), _>(RuntimeError::ProtocolInvariant {
                    detail: "P1资产与信息共同候选拒绝".into(),
                })
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains("P1资产与信息共同候选拒绝"));
    assert!(world.dead_reason.is_none());
    assert_eq!(accounts(&mut world), before);
    assert!(
        Arc::ptr_eq(&published_before, &world.publication.load()),
        "同overlay拒绝不得撤回已发布PublishedTick"
    );
    assert_eq!(world.global.committed_tick, committed_tick_before);
    world
        .with_information_transaction(vec![(0, vec![shares(0, 7, 5600)])], payload, |candidate| {
            assert_information_candidate(candidate, publication);
            assert_eq!(candidate.groups[0].accounts[0].1.as_ref(), &expected[0][0]);
            Ok(())
        })
        .unwrap();
    assert_eq!(accounts(&mut world), expected);
    world.shutdown().unwrap();
}

#[test]
fn funds_p1_11_information_with_paid_share_cost_keeps_original_terminal_transport_contract() {
    use crate::runtime::protocol::{AdminFault, AdminPhase};
    for panic_worker in [false, true] {
        let mut world = information_world();
        let original = Arc::new("P1与INFO原始panic".to_string());
        if panic_worker {
            world.group_txs[1]
                .send(GroupIn::AdminFaultForTest {
                    phase: AdminPhase::Prepare,
                    fault: AdminFault::Panic(original.clone()),
                })
                .unwrap();
        } else {
            world.admin_send_fault_for_test = Some((AdminPhase::Commit, 1));
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            world.with_information_transaction(
                vec![(0, vec![shares(0, 7, 5600)])],
                information_fixture::payload(true),
                |_| Ok(()),
            )
        }));
        if panic_worker {
            let payload = result.unwrap_err().downcast::<Arc<String>>().unwrap();
            assert!(Arc::ptr_eq(&payload, &original));
        } else {
            assert!(result.unwrap().is_err());
        }
        assert!(world.dead_reason.is_some());
        assert!(world.handles.is_empty());
    }
}
