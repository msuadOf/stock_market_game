//! Plan identity 的账户归属、完整交付与恢复边界。

use super::bridge_tests::setup_fixture;
use super::drive::RuntimeDriver;
use crate::plans::{OwnerSequence, PlanBook, PlanEvent, PlanOpen, PlanOpinion, PlanPolicy, PlanTarget, TerminationReason, Urgency};
use crate::{AccountId, GameSession, Side, StockId};

fn open(owner: AccountId, stock: u32) -> PlanOpen {
    PlanOpen {
        account: owner,
        code: StockId(stock),
        direction: Side::Buy,
        target: PlanTarget::ShareCount(100),
        opinion: PlanOpinion { signal_score_bp: 3000, source: crate::plans::OpinionSource::Blended },
        confidence_bp: 8000,
        urgency: Urgency::Normal,
        horizon_trading_days: 5,
        created_trading_day: 0,
    }
}

fn seed_book() -> PlanBook {
    let mut book = PlanBook::default();
    let player = book.create(open(AccountId(0), 0)).unwrap();
    book.apply(player, PlanEvent::Terminated { reason: TerminationReason::Cancelled, trading_day: 0 }).unwrap();
    book.create(open(AccountId(2), 1)).unwrap();
    let (_, _, plans) = book.into_parts();
    PlanBook::from_parts(
        PlanPolicy { review_signal_delta_bp: 777, ..PlanPolicy::default() },
        vec![
            OwnerSequence { owner: AccountId(0), next_sequence: Some(40) },
            OwnerSequence { owner: AccountId(1), next_sequence: None },
            OwnerSequence { owner: AccountId(2), next_sequence: Some(90) },
        ],
        plans,
    ).unwrap()
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
    let ids: Vec<_> = collected.iter().flat_map(|(_, shard)| shard.plan_ids()).collect();
    assert_eq!(ids.len(), 2, "所有组必须收到对应账户的完整计划，含 Player terminal");
    for id in book.plan_ids() {
        assert_eq!(ids.iter().filter(|&&found| found == id).count(), 1);
    }
    let cursors: Vec<_> = collected.iter().flat_map(|(_, shard)| shard.owner_sequences()).collect();
    assert_eq!(cursors.len(), 3, "无计划的 None 游标也必须移交");
    assert!(cursors.contains(&OwnerSequence { owner: AccountId(0), next_sequence: Some(40) }));
    assert!(cursors.contains(&OwnerSequence { owner: AccountId(1), next_sequence: None }));
    assert!(cursors.contains(&OwnerSequence { owner: AccountId(2), next_sequence: Some(90) }));
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
        assert_eq!(mirror.owner_sequences().collect::<Vec<_>>(), book.owner_sequences().collect::<Vec<_>>());
        session.set_runtime_plans(mirror);
    }
}

#[test]
fn two_groups_create_sequence_zero_and_complete_real_driver_delivery() {
    let mut session = GameSession::new(setup_fixture(), 34).unwrap();
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    for owner in [AccountId(0), AccountId(2)] {
        driver.mutate_plan_book_for_test(Box::new(move |book| {
            let id = book.create(open(owner, 0)).unwrap();
            assert_eq!(id.owner, owner);
            assert_eq!(id.sequence, 0, "不同owner的首计划均从0分配");
        }));
    }
    driver.step_frame_equivalent(&mut session).unwrap();
    let mirror = session.take_runtime_plans();
    assert_eq!(mirror.plan_ids().count(), 2, "真实driver交付必须包含两组首计划");
    for (group, book) in driver.collect_plans().unwrap() {
        let owners: Vec<_> = book.plan_ids().map(|id| id.owner).collect();
        assert_eq!(owners, vec![if group == 0 { AccountId(0) } else { AccountId(2) }], "一账户仅由所属组写入");
    }
}
