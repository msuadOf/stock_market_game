//! Plan identity 的账户归属、完整交付与恢复边界。

use super::bridge_tests::setup_fixture;
use super::drive::RuntimeDriver;
use crate::plans::{
    OwnerSequence, PlanBook, PlanEvent, PlanOpen, PlanOpinion, PlanPolicy, PlanTarget,
    TerminationReason, Urgency,
};
use crate::{AccountId, GameSession, Side, StockId};

fn open(owner: AccountId, stock: u32) -> PlanOpen {
    PlanOpen {
        account: owner,
        code: StockId(stock),
        direction: Side::Buy,
        target: PlanTarget::ShareCount(100),
        opinion: PlanOpinion {
            signal_score_bp: 3000,
            source: crate::plans::OpinionSource::Blended,
        },
        confidence_bp: 8000,
        urgency: Urgency::Normal,
        horizon_trading_days: 5,
        created_trading_day: 0,
    }
}

pub(super) fn seed_book() -> PlanBook {
    let mut book = PlanBook::default();
    let player = book.create(open(AccountId(0), 0)).unwrap();
    book.apply(
        player,
        PlanEvent::Terminated {
            reason: TerminationReason::Cancelled,
            trading_day: 0,
        },
    )
    .unwrap();
    book.create(open(AccountId(2), 1)).unwrap();
    let (_, _, plans) = book.into_parts();
    PlanBook::from_parts(
        PlanPolicy {
            review_signal_delta_bp: 777,
            ..PlanPolicy::default()
        },
        vec![
            OwnerSequence {
                owner: AccountId(0),
                next_sequence: Some(40),
            },
            OwnerSequence {
                owner: AccountId(1),
                next_sequence: None,
            },
            OwnerSequence {
                owner: AccountId(2),
                next_sequence: Some(90),
            },
        ],
        plans,
    )
    .unwrap()
}

#[test]
fn startup_transfers_player_terminal_and_npc_plans_with_all_owner_cursors() {
    let mut session = GameSession::new(setup_fixture(), 31).unwrap();
    let book = seed_book();
    session.set_runtime_plans(book.clone());
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    let collected = driver.collect_plans().unwrap();
    assert_eq!(collected.len(), 2);
    for (group, shard) in &collected {
        assert_eq!(shard.policy(), book.policy(), "恢复须保留原 PlanPolicy");
        for cursor in shard.owner_sequences() {
            assert_eq!(if cursor.owner.0 < 2 { 0 } else { 1 }, *group);
        }
    }
    let ids: Vec<_> = collected
        .iter()
        .flat_map(|(_, shard)| shard.plan_ids())
        .collect();
    assert_eq!(
        ids.len(),
        2,
        "所有组必须收到对应账户的完整计划，含 Player terminal"
    );
    for id in book.plan_ids() {
        assert_eq!(ids.iter().filter(|&&found| found == id).count(), 1);
    }
    let cursors: Vec<_> = collected
        .iter()
        .flat_map(|(_, shard)| shard.owner_sequences())
        .collect();
    assert_eq!(cursors.len(), 3, "无计划的 None 游标也必须移交");
    assert!(cursors.contains(&OwnerSequence {
        owner: AccountId(0),
        next_sequence: Some(40)
    }));
    assert!(cursors.contains(&OwnerSequence {
        owner: AccountId(1),
        next_sequence: None
    }));
    assert!(cursors.contains(&OwnerSequence {
        owner: AccountId(2),
        next_sequence: Some(90)
    }));
}

#[test]
fn startup_rejects_unknown_plan_owner_before_any_session_install() {
    let mut session = GameSession::new(setup_fixture(), 32).unwrap();
    let mut book = PlanBook::default();
    book.create(open(AccountId(999), 0)).unwrap();
    session.set_runtime_plans(book.clone());
    let result = RuntimeDriver::from_session_grouped(&mut session, 2);
    assert!(result.is_err(), "不在名册的计划 owner 不能成为隐形 writer");
    let retained = session.take_runtime_plans();
    assert_eq!(retained, book, "失败构造不得半安装或遗失原簿");
}

#[test]
fn every_tick_merges_all_seeded_groups_and_preserves_unchanged_group() {
    let mut session = GameSession::new(setup_fixture(), 33).unwrap();
    let book = seed_book();
    session.set_runtime_plans(book.clone());
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    for _ in 0..2 {
        driver.step_frame_equivalent(&mut session).unwrap();
        let mirror = session.take_runtime_plans();
        for id in book.plan_ids() {
            assert!(mirror.plan(id).is_ok(), "每拍必须完整合并所有组：{id:?}");
        }
        assert_eq!(mirror.policy(), book.policy());
        assert_eq!(
            mirror.owner_sequences().collect::<Vec<_>>(),
            book.owner_sequences().collect::<Vec<_>>()
        );
        session.set_runtime_plans(mirror);
    }
}

#[test]
fn two_groups_create_sequence_zero_and_complete_real_driver_delivery() {
    let mut session = GameSession::new(setup_fixture(), 34).unwrap();
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    for owner in [AccountId(0), AccountId(2)] {
        driver.mutate_plan_book_for_test(
            owner,
            Box::new(move |book| {
                let id = book.create(open(owner, 0)).unwrap();
                assert_eq!(id.owner, owner);
                assert_eq!(id.sequence, 0, "不同owner的首计划均从0分配");
            }),
        );
    }
    driver.step_frame_equivalent(&mut session).unwrap();
    let mirror = session.take_runtime_plans();
    assert_eq!(
        mirror.plan_ids().count(),
        2,
        "真实driver交付必须包含两组首计划"
    );
    for (group, book) in driver.collect_plans().unwrap() {
        let owners: Vec<_> = book.plan_ids().map(|id| id.owner).collect();
        assert_eq!(
            owners,
            vec![if group == 0 {
                AccountId(0)
            } else {
                AccountId(2)
            }],
            "一账户仅由所属组写入"
        );
    }
}

#[test]
fn complete_merge_rejects_missing_duplicate_wrong_group_and_policy_conflicts() {
    let roster = [AccountId(0), AccountId(1), AccountId(2)];
    let book = seed_book();
    let split = super::plan_ownership::split(book.clone(), &roster, 3, 1).unwrap();
    let complete = || {
        split
            .iter()
            .cloned()
            .enumerate()
            .map(|(group, book)| (group as u16, book))
            .collect::<Vec<_>>()
    };
    let merge = |parts| super::plan_ownership::merge(parts, &roster, 3, 1, *book.policy());
    assert_eq!(merge(complete()).unwrap(), book);
    let mut missing = complete();
    missing.pop();
    assert!(matches!(
        merge(missing),
        Err(super::protocol::RuntimeError::ProtocolInvariant { .. })
    ));
    let mut duplicate = complete();
    duplicate[2].0 = 1;
    assert!(merge(duplicate).is_err());
    let mut out_of_range = complete();
    out_of_range[2].0 = 3;
    assert!(merge(out_of_range).is_err());
    let mut wrong_owner = complete();
    wrong_owner.swap(0, 2);
    wrong_owner[0].0 = 0;
    wrong_owner[2].0 = 2;
    assert!(merge(wrong_owner).is_err(), "同一合法owner不得被其他组冒领");
    let mut policy = complete();
    let (_, cursors, plans) = policy[1].1.clone().into_parts();
    policy[1].1 = PlanBook::from_parts(PlanPolicy::default(), cursors, plans).unwrap();
    assert!(merge(policy).is_err(), "无当前计划的组也必须保留相同policy");
}

#[test]
fn invalid_seed_is_rejected_before_any_group_constructor_or_worker_starts() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let started = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&started);
    let mut spec = super::skeleton_tests::basic_spec(
        2,
        1,
        1,
        Box::new(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            Box::new(|_, _| super::group::TickPlan::default())
        }),
    );
    let mut unknown = PlanBook::default();
    unknown.create(open(AccountId(999), 0)).unwrap();
    spec.initial_plan_books = vec![unknown, PlanBook::default()];
    assert!(matches!(
        super::world::World::spawn(
            spec,
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法")
        ),
        Err(super::protocol::RuntimeError::ProtocolInvariant { .. })
    ));
    assert_eq!(
        started.load(Ordering::SeqCst),
        0,
        "坏种子不得启动任何组或线程"
    );
}

#[test]
fn empty_tail_groups_keep_policy_without_invented_owner_cursors() {
    let mut session = GameSession::new(setup_fixture(), 35).unwrap();
    let book = seed_book();
    session.set_runtime_plans(book.clone());
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 5).unwrap();
    let books = driver.collect_plans().unwrap();
    assert_eq!(books.len(), 5);
    for (group, shard) in books {
        assert_eq!(shard.policy(), book.policy());
        if group >= 3 {
            assert_eq!(shard.owner_sequences().count(), 0);
            assert_eq!(shard.plan_ids().count(), 0);
        }
    }
}

#[test]
fn tail_owner_force_and_defer_due_change_only_the_addressed_account() {
    let mut session = GameSession::new(setup_fixture(), 36).unwrap();
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 3).unwrap();
    driver.defer_due_for_test_at(&[AccountId(1), AccountId(2)], u64::MAX);
    let before = driver.collect_attention().unwrap();
    for (group, entries) in &before {
        if *group == 1 || *group == 2 {
            assert_eq!(entries[0].1, u64::MAX, "两个NPC必须各自在所属组推远due");
        }
    }
    driver.force_due_for_test_at(&[AccountId(2)], 1);
    driver.step_frame_equivalent(&mut session).unwrap();
    let after = driver.collect_attention().unwrap();
    assert_eq!(after[1].1[0].1, u64::MAX, "未被force的另一组账户保持原调度");
    assert_ne!(
        after[2].1[0].1,
        u64::MAX,
        "末组owner在真实Start执行后重排下一due"
    );
}

#[test]
fn origin_mixed_shards_merge_entries_and_history_cursors_without_reallocating_identity() {
    let roster = [AccountId(0), AccountId(1), AccountId(17)];
    let mut book = PlanBook::default();
    book.create(open(AccountId(1), 0)).unwrap();
    let company = book
        .create_issuer_repurchase(crate::plans::RepurchaseExecutionOpen {
            owner: AccountId(17),
            issuer: crate::company::CompanyId(3),
            stock: StockId(1),
        })
        .unwrap();
    let shards = super::plan_ownership::split(book.clone(), &roster, 2, 2).unwrap();
    assert!(matches!(
        shards[1].entry(company),
        Ok(crate::plans::PlanEntry::IssuerRepurchase(_))
    ));
    assert!(shards[0].entry(company).is_err());
    let merged = super::plan_ownership::merge(
        shards
            .into_iter()
            .enumerate()
            .map(|(group, shard)| (group as u16, shard))
            .collect(),
        &roster,
        2,
        2,
        *book.policy(),
    )
    .unwrap();
    assert_eq!(merged, book);
    assert_eq!(
        serde_json::to_value(merged).unwrap(),
        serde_json::to_value(book).unwrap()
    );
}

#[test]
fn unordered_sparse_roster_preserves_mixed_origins_terminal_and_none_cursors() {
    let roster = [AccountId(17), AccountId(0), AccountId(1)];
    let mut book = PlanBook::default();
    let investor = book.create(open(AccountId(1), 0)).unwrap();
    book.apply(
        investor,
        PlanEvent::Terminated {
            reason: TerminationReason::Cancelled,
            trading_day: 0,
        },
    )
    .unwrap();
    let issuer = book
        .create_issuer_repurchase(crate::plans::RepurchaseExecutionOpen {
            owner: AccountId(17),
            issuer: crate::company::CompanyId(3),
            stock: StockId(1),
        })
        .unwrap();
    let (policy, _, entries) = book.into_parts();
    let book = PlanBook::from_parts(
        policy,
        vec![
            OwnerSequence {
                owner: AccountId(0),
                next_sequence: None,
            },
            OwnerSequence {
                owner: AccountId(1),
                next_sequence: Some(40),
            },
            OwnerSequence {
                owner: AccountId(17),
                next_sequence: Some(90),
            },
        ],
        entries,
    )
    .unwrap();
    let shards = super::plan_ownership::split(book.clone(), &roster, 2, 2).unwrap();
    assert!(shards[0].entry(issuer).is_ok());
    assert!(shards[0].entry(investor).is_err());
    assert!(shards[1].entry(investor).is_ok());
    let merged = super::plan_ownership::merge(
        shards
            .into_iter()
            .enumerate()
            .map(|(g, book)| (g as u16, book))
            .collect(),
        &roster,
        2,
        2,
        *book.policy(),
    )
    .unwrap();
    assert_eq!(merged, book);
    assert_eq!(
        serde_json::to_value(merged).unwrap(),
        serde_json::to_value(book).unwrap()
    );
}
