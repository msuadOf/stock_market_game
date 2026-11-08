use super::ProtocolSession;

#[test]
#[cfg(not(target_family = "wasm"))]
fn delayed_consumer_does_not_block_core_or_install_peripheral_state() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    let mut session = ProtocolSession::new(setup, 76).unwrap();
    session.step_frame().unwrap();
    let tick = session.game().tick();
    let seq = session.game().seq();
    let origin = session.game().runtime_mirror_origin();
    let history_len = session.state.intraday.len();

    // 保留前两拍的未消费值，模拟外围消费者尚未运行。
    let first = session.advance_core_tick().unwrap();
    let second = session.advance_core_tick().unwrap();
    assert_eq!(session.game().tick(), tick + 2, "核心必须完成两拍");
    assert_eq!(
        session.state.intraday.len(),
        history_len,
        "外围历史不得在消费前推进"
    );
    assert_eq!(
        session.game().runtime_mirror_origin().install_sequence,
        origin.install_sequence,
        "未消费时不得安装完整外围镜像"
    );

    let first = session
        .record_consumed_tick(first.consume().unwrap())
        .unwrap();
    let second = session
        .record_consumed_tick(second.consume().unwrap())
        .unwrap();
    assert_eq!(first.tick, tick + 1);
    assert_eq!(second.tick, tick + 2);
    assert_eq!(first.seq_from, seq);
    assert_eq!(second.seq_from, first.seq_to);
    first.validate().unwrap();
    second.validate().unwrap();
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn blocked_deferred_consumer_does_not_block_next_core_tick() {
    use std::sync::mpsc;
    use std::time::Duration;

    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    let mut session = ProtocolSession::new(setup, 77).unwrap();
    session.step_frame().unwrap();
    let tick = session.game().tick();
    let mut first = session.advance_core_tick().unwrap();
    let (release_tx, release_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    first.consume_barrier = Some((ready_tx, release_rx));
    let worker = std::thread::spawn(move || first.consume());
    ready_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let second = session.advance_core_tick().unwrap();
    let advanced_tick = session.game().tick();
    release_tx.send(()).unwrap();
    let first_frame = session
        .record_consumed_tick(worker.join().unwrap().unwrap())
        .unwrap();
    let second_frame = session
        .record_consumed_tick(second.consume().unwrap())
        .unwrap();
    assert_eq!(advanced_tick, tick + 2, "外围被阻塞时核心仍须提交下一拍");
    assert_eq!(first_frame.tick, tick + 1);
    assert_eq!(second_frame.tick, tick + 2);
    assert_eq!(first_frame.seq_to, second_frame.seq_from);
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn explicit_step_does_not_repeat_static_personal_full_read() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    let mut session = ProtocolSession::new(setup, 79).unwrap();
    session.step_frame().unwrap();
    crate::runtime::world::take_prepared_personal_full_calls_for_test();
    session.step_frame().unwrap();
    session.step_frame().unwrap();
    assert_eq!(
        crate::runtime::world::take_prepared_personal_full_calls_for_test(),
        0,
        "普通显式逐拍不得反复走静点全量个人态读取"
    );
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn delayed_player_order_uses_current_world_reservation() {
    use crate::{AccountId, Event, Intent, LimitPrice, Money, Side, StockId};
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.config.starting_cash = Money::from_cents(150_000);
    let mut session = ProtocolSession::new(setup, 80).unwrap();
    session.step_frame().unwrap();
    let buy = || Intent::PlaceLimit {
        code: StockId(0),
        side: Side::Buy,
        price: LimitPrice::Fixed(Money::from_cents(1000)),
        qty: 100,
    };
    session
        .shared_ingress()
        .enqueue_player_intent(AccountId(0), buy())
        .unwrap();
    let first = session.advance_core_tick().unwrap();
    session
        .shared_ingress()
        .enqueue_player_intent(AccountId(0), buy())
        .unwrap();
    let second = session.advance_core_tick().unwrap();
    let first = session
        .record_consumed_tick(first.consume().unwrap())
        .unwrap();
    let batch = session.tick_batch(vec![first.clone()]).unwrap();
    let reserved = batch.runtime_delta.unwrap().accounts[&AccountId(0)].reserved_cash;
    let second = session
        .record_consumed_tick(second.consume().unwrap())
        .unwrap();
    assert!(first
        .facts
        .iter()
        .any(|fact| matches!(fact.event, Event::OrderAccepted { .. })));
    assert!(reserved > Money::ZERO, "首单的同版冻结必须在外围读面中可见");
    assert!(
        second
            .facts
            .iter()
            .any(|fact| matches!(fact.event, Event::IntentRejected { .. })),
        "第二单必须看到首单已占用的资金"
    );
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn core_failure_stops_before_next_tick() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    let mut session = ProtocolSession::new(setup, 81).unwrap();
    session.step_frame().unwrap();
    let tick = session.game().tick();
    let fatal = crate::session::StepFatal::InvariantViolation {
        location: "core_failure_stops_before_next_tick".to_owned(),
        description: "注入核心失败".to_owned(),
    };
    session.state.game.inject_step_failure(fatal.clone());
    assert!(matches!(session.advance_core_tick(), Err(ref error) if error == &fatal));
    assert_eq!(session.game().tick(), tick);
    assert!(matches!(session.advance_core_tick(), Err(ref error) if error == &fatal));
    assert_eq!(session.game().tick(), tick);
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn failed_consumer_does_not_reexecute_committed_tick() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    let mut session = ProtocolSession::new(setup, 82).unwrap();
    session.step_frame().unwrap();
    let before = session.game().tick();
    let mut deferred = session.advance_core_tick().unwrap();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (_release_tx, release_rx) = std::sync::mpsc::channel();
    drop(ready_rx);
    deferred.consume_barrier = Some((ready_tx, release_rx));
    let failure = match deferred.consume() {
        Err(error) => error,
        Ok(_) => panic!("外围失败必须显式返回"),
    };
    assert_eq!(
        session.game().tick(),
        before + 1,
        "已提交 tick 不得回退重跑"
    );
    assert!(
        matches!(session.advance_core_tick(), Err(ref error) if error == &failure),
        "外围首错必须阻止下一拍"
    );
    assert!(matches!(session.step_frame(), Err(ref error) if error == &failure));
    assert!(
        matches!(session.end_civil_day_update(), Err(crate::session::SessionError::Step(ref error)) if error == &failure)
    );
    assert_eq!(session.game().tick(), before + 1, "失败后不得重复推进");
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn pending_tick_rejects_checkpoint_and_rollback() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    let mut session = ProtocolSession::new(setup, 83).unwrap();
    let checkpoint = session.checkpoint().unwrap();
    let pending = session.advance_core_tick().unwrap();
    assert!(session.checkpoint().is_err());
    assert!(session.rollback(checkpoint).is_err());
    session
        .record_consumed_tick(pending.consume().unwrap())
        .unwrap();
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn consumer_failure_preserves_successful_prefix_and_stops_next_tick() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    let mut session = ProtocolSession::new(setup, 84).unwrap();
    session.step_frame().unwrap();
    let first = session.advance_core_tick().unwrap().consume().unwrap();
    let mut second = session.advance_core_tick().unwrap();
    let committed_tick = session.game().tick();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (_release_tx, release_rx) = std::sync::mpsc::channel();
    drop(ready_rx);
    second.consume_barrier = Some((ready_tx, release_rx));
    let failure = match second.consume() {
        Err(error) => error,
        Ok(_) => panic!("外围失败必须返回首错"),
    };
    // 消费者已完成第一拍，但 owner 在第二拍失败后才取回成功前缀。
    let first = session.record_consumed_tick(first).unwrap();
    assert_eq!(first.tick + 1, committed_tick);
    let batch = session.tick_batch(vec![first.clone()]).unwrap();
    assert_eq!(batch.frames.len(), 1);
    assert_eq!(
        batch.frames[0].facts, first.facts,
        "成功事实必须完整交付一次"
    );
    assert!(
        session.tick_batch(vec![first]).is_err(),
        "成功前缀不得重复发布"
    );
    assert!(matches!(session.advance_core_tick(), Err(ref error) if error == &failure));
    assert!(
        matches!(session.end_civil_day_update(), Err(crate::session::SessionError::Step(ref error)) if error == &failure)
    );
    assert_eq!(
        session.game().tick(),
        committed_tick,
        "失败后不能越过缺口推进"
    );
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn second_member_buy_and_sell_reservations_match_delta_and_snapshot() {
    use crate::session::OpaqueSubjectId;
    use crate::{Event, Intent, LimitPrice, Money, Side, StockId};
    use std::collections::BTreeMap;

    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    let mut session = ProtocolSession::new(setup, 86).unwrap();
    let member = session
        .join_market(OpaqueSubjectId::new("第二成员".to_owned()).unwrap(), true)
        .unwrap()
        .account_id;
    assert_ne!(member, crate::AccountId(0));
    session
        .state
        .game
        .fixture_account_mut(&member)
        .unwrap()
        .fixture_set_positions(BTreeMap::from([(
            StockId(0),
            crate::Position::from_restored_parts(200, 0, 200_000, 0),
        )]));
    session.step_frame().unwrap();
    for (side, price) in [(Side::Buy, 990), (Side::Sell, 1010)] {
        session
            .shared_ingress()
            .enqueue_player_intent(
                member,
                Intent::PlaceLimit {
                    code: StockId(0),
                    side,
                    price: LimitPrice::Fixed(Money::from_cents(price)),
                    qty: 100,
                },
            )
            .unwrap();
    }
    let deferred = session.advance_core_tick().unwrap();
    let frame = session
        .record_consumed_tick(deferred.consume().unwrap())
        .unwrap();
    let accepted = frame
        .facts
        .iter()
        .filter(
            |fact| matches!(fact.event, Event::OrderAccepted { account, .. } if account == member),
        )
        .count();
    assert_eq!(accepted, 2, "买卖委托都必须真实进入簿");
    let batch = session.tick_batch(vec![frame]).unwrap();
    let delta = batch.runtime_delta.unwrap();
    let delta_account = &delta.accounts[&member];
    assert!(
        delta_account.reserved_cash > Money::ZERO,
        "买单必须冻结资金"
    );
    assert_eq!(
        delta_account.reserved_sell_qty.get(&StockId(0)),
        Some(&100),
        "卖单必须冻结股份"
    );
    let snapshot = session.flush_now().unwrap();
    let snapshot_account = &snapshot.accounts[&member];
    assert_eq!(snapshot.tick, delta.tick, "两种读面必须处于同一市场拍");
    assert_eq!(
        (
            snapshot_account.reserved_cash,
            snapshot_account.reserved_sell_qty.get(&StockId(0)).copied()
        ),
        (
            delta_account.reserved_cash,
            delta_account.reserved_sell_qty.get(&StockId(0)).copied()
        ),
        "第二成员买单资金及卖单股份预留必须与已交付 delta 一致"
    );
}
