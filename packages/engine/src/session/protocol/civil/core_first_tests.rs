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

#[test]
#[cfg(not(target_family = "wasm"))]
fn dayend_ordinary_core_ticks_skip_mirror_only_work_and_preserve_day_boundary() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    setup.runtime_sync_policy = crate::RuntimeSyncPolicy::DayEnd;
    let ticks_per_day = setup.ticks_per_day;
    let mut session = ProtocolSession::new(setup, 85).unwrap();
    let initial = session.step_frame().unwrap();
    let mut last_tick = initial.tick;
    let mut last_seq = initial.seq_to;
    crate::runtime::world::take_dayend_hotpath_for_test();

    // 仅量测两个普通核心拍；consumer 尚未运行。
    let first = session.advance_core_tick().unwrap();
    let second = session.advance_core_tick().unwrap();
    let calls = crate::runtime::world::take_dayend_hotpath_for_test();
    let mut facts = 0usize;
    for deferred in [first, second] {
        let frame = session
            .record_consumed_tick(deferred.consume().unwrap())
            .unwrap();
        frame.validate().unwrap();
        assert_eq!(frame.tick, last_tick + 1);
        assert_eq!(frame.seq_from, last_seq);
        last_tick = frame.tick;
        last_seq = frame.seq_to;
        facts += frame.facts.len();
    }
    while last_tick < ticks_per_day {
        let deferred = session.advance_core_tick().unwrap();
        let frame = session
            .record_consumed_tick(deferred.consume().unwrap())
            .unwrap();
        frame.validate().unwrap();
        assert_eq!(frame.tick, last_tick + 1);
        assert_eq!(frame.seq_from, last_seq);
        last_tick = frame.tick;
        last_seq = frame.seq_to;
        facts += frame.facts.len();
    }
    assert!(facts > 0, "必须实际交付非空逐拍事实");
    let civil = session.end_civil_day_update().unwrap();
    civil.validate().unwrap();
    assert_eq!(civil.tick, ticks_per_day);
    assert_eq!(civil.seq_from, last_seq);
    assert_eq!(civil.refresh.intraday.len() as u64, ticks_per_day);
    let first_next = session.advance_core_tick().unwrap();
    let frame = session
        .record_consumed_tick(first_next.consume().unwrap())
        .unwrap();
    assert_eq!(frame.tick, ticks_per_day + 1);
    assert_eq!(frame.seq_from, civil.seq_to);

    if cfg!(feature = "simulation-diagnostics") {
        assert!(
            calls.iter().all(|count| *count > 0),
            "诊断需要真实完整计划与逐拍投影，四项读取均须实际执行；实际计数={calls:?}"
        );
    } else {
        assert_eq!(calls, [0, 0, 0, 0],
            "默认 feature 普通拍不得收集完整计划、镜像调度、构造 Snapshot 或 PublicRuntime；实际计数={calls:?}");
    }
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn dayend_explicit_flush_after_unrecorded_mirror_inputs_uses_current_world() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    setup.runtime_sync_policy = crate::RuntimeSyncPolicy::DayEnd;
    let mut session = ProtocolSession::new(setup, 87).unwrap();
    session.step_frame().unwrap();
    let mut frames = Vec::new();
    for _ in 0..2 {
        let deferred = session.advance_core_tick().unwrap();
        frames.push(
            session
                .record_consumed_tick(deferred.consume().unwrap())
                .unwrap(),
        );
    }
    session.tick_batch(frames).unwrap();
    let tick = session.game().tick();
    let snapshot = session.flush_now().unwrap();
    assert_eq!(snapshot.tick, tick);
    assert_eq!(
        snapshot.mirror_origin.source_tick, tick,
        "显式同步必须使用当前 World"
    );
    let next = session.advance_core_tick().unwrap();
    let frame = session
        .record_consumed_tick(next.consume().unwrap())
        .unwrap();
    assert_eq!(frame.tick, tick + 1);
}

#[test]
#[cfg(all(not(target_family = "wasm"), not(feature = "simulation-diagnostics")))]
fn dayend_batch_materializes_runtime_once_after_multiple_consumers() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    setup.runtime_sync_policy = crate::RuntimeSyncPolicy::DayEnd;
    let mut session = ProtocolSession::new(setup, 88).unwrap();
    session.step_frame().unwrap();
    super::LIGHT_RUNTIME_BUILDS.with(|count| count.set(0));
    let first = session.advance_core_tick().unwrap().consume().unwrap();
    let second = session.advance_core_tick().unwrap().consume().unwrap();
    assert_eq!(
        super::LIGHT_RUNTIME_BUILDS.with(|count| count.get()),
        0,
        "逐拍 consumer 不得物化 PublicRuntime"
    );
    let first = session.record_consumed_tick(first).unwrap();
    let second = session.record_consumed_tick(second).unwrap();
    assert_eq!(
        super::LIGHT_RUNTIME_BUILDS.with(|count| count.get()),
        0,
        "逐拍登记不得物化 PublicRuntime"
    );
    let expected_tick = second.tick;
    let expected_seq = second.seq_to;
    let batch = session.tick_batch(vec![first, second]).unwrap();
    assert_eq!(
        super::LIGHT_RUNTIME_BUILDS.with(|count| count.get()),
        1,
        "同批末帧必须恰好物化一次 PublicRuntime"
    );
    let runtime = batch.runtime_delta.unwrap();
    assert_eq!(runtime.tick, expected_tick);
    assert_eq!(runtime.seq_to, expected_seq);
}

#[test]
#[cfg(all(not(target_family = "wasm"), not(feature = "simulation-diagnostics")))]
fn materialization_failure_survives_later_record_flush_and_rollback() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    setup.runtime_sync_policy = crate::RuntimeSyncPolicy::DayEnd;
    let mut session = ProtocolSession::new(setup, 89).unwrap();
    let checkpoint = session.checkpoint().unwrap();
    session.step_frame().unwrap();
    let mut first = session.advance_core_tick().unwrap().consume().unwrap();
    let second = session.advance_core_tick().unwrap().consume().unwrap();
    match &mut first.runtime {
        super::ConsumedRuntime::Light(runtime) => runtime
            .accounts
            .get_mut(&crate::AccountId(0))
            .unwrap()
            .fixture_set_cash(crate::Money::from_cents(-1)),
        _ => panic!("必须使用延迟物化路径"),
    }
    let frame = session.record_consumed_tick(first).unwrap();
    let failure = session.tick_batch(vec![frame]).unwrap_err();
    assert!(matches!(session.record_consumed_tick(second), Err(ref error) if error == &failure));
    assert!(matches!(session.flush_now(), Err(ref error) if error == &failure));
    assert!(matches!(session.checkpoint(), Err(ref error) if error == &failure));
    assert!(matches!(session.rollback(checkpoint), Err(ref error) if error == &failure));
    assert!(matches!(session.advance_core_tick(), Err(ref error) if error == &failure));
}

#[test]
#[cfg(all(not(target_family = "wasm"), not(feature = "simulation-diagnostics")))]
fn flush_rejects_pending_and_preserves_unpublished_light_tick() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    setup.runtime_sync_policy = crate::RuntimeSyncPolicy::DayEnd;
    let mut session = ProtocolSession::new(setup, 90).unwrap();
    session.step_frame().unwrap();
    let pending = session.advance_core_tick().unwrap();
    assert!(session.flush_now().is_err());
    let frame = session
        .record_consumed_tick(pending.consume().unwrap())
        .unwrap();
    session.flush_now().unwrap();
    let batch = session.tick_batch(vec![frame.clone()]).unwrap();
    assert_eq!(batch.runtime_delta.unwrap().tick, frame.tick);
    let next = session.advance_core_tick().unwrap().consume().unwrap();
    let next = session.record_consumed_tick(next).unwrap();
    session.tick_batch(vec![next]).unwrap();
    session.prepare_public_baseline();
    session.flush_now().unwrap();
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn coalesced_ticks_preserve_interval_and_facts() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    let mut session = ProtocolSession::new(setup, 81).unwrap();
    session.step_frame().unwrap();
    let first = session.advance_core_tick().unwrap();
    let second = session.advance_core_tick().unwrap();
    let from = first.tick;
    let to = second.tick;
    let seq_from = first.seq_from;
    let seq_to = second.seq_to;
    let fact_count = first.facts.len() + second.facts.len();
    let consumed = super::DeferredTick::consume_interval(vec![first, second]).unwrap();
    let frame = session.record_consumed_interval(consumed).unwrap();
    assert_eq!(frame.tick_from, Some(from));
    assert_eq!(frame.tick, to);
    assert_eq!((frame.seq_from, frame.seq_to), (seq_from, seq_to));
    assert_eq!(frame.facts.len(), fact_count);
    frame.validate().unwrap();
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn interval_failure_keeps_successful_prefix_and_failure_identity() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    let mut session = ProtocolSession::new(setup, 82).unwrap();
    session.step_frame().unwrap();
    let first = session.advance_core_tick().unwrap();
    let first_tick = first.tick;
    let mut second = session.advance_core_tick().unwrap();
    let identity = std::sync::Arc::clone(&second.identity);
    second.seq_from = second.seq_from.saturating_add(1);
    let mut interval = super::DeferredInterval::new();
    interval.push(first).unwrap();
    let error = interval.push(second).err().expect("不连续区间必须失败");
    assert_eq!(identity.get(), Some(&error));
    let frame = session
        .record_consumed_interval(interval.consume().unwrap())
        .unwrap();
    assert_eq!(frame.tick, first_tick);
    assert!(
        session.advance_core_tick().is_err(),
        "后续 tick 不得掩盖外围失败"
    );
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn failed_interval_end_view_does_not_publish_or_repeat_committed_ticks() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    setup.runtime_sync_policy = crate::RuntimeSyncPolicy::EveryTick;
    let mut session = ProtocolSession::new(setup, 83).unwrap();
    session.step_frame().unwrap();
    let history_before = session.state.intraday.len();
    let first = session.advance_core_tick().unwrap();
    let second = session.advance_core_tick().unwrap();
    let committed_tick = second.tick;
    let identity = std::sync::Arc::clone(&second.identity);
    let mut interval = super::DeferredInterval::new();
    interval.push(first).unwrap();
    interval.push(second).unwrap();
    interval.last.as_mut().unwrap().publication = None;
    let error = interval
        .consume()
        .err()
        .expect("末版 view 缺少发布版本必须失败");
    assert_eq!(identity.get(), Some(&error));
    assert_eq!(session.state.intraday.len(), history_before);
    assert_eq!(session.game().tick(), committed_tick);
    assert!(session.advance_core_tick().is_err(), "已提交 tick 不得重跑");
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn coalesced_full_day_preserves_civil_boundary_and_next_tick() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    setup.runtime_sync_policy = crate::RuntimeSyncPolicy::EveryTick;
    let ticks_per_day = setup.ticks_per_day;
    let mut session = ProtocolSession::new(setup, 84).unwrap();
    let initial = session.step_frame().unwrap();
    let mut interval = super::DeferredInterval::new();
    for _ in 1..ticks_per_day {
        interval.push(session.advance_core_tick().unwrap()).unwrap();
    }
    let frame = session
        .record_consumed_interval(interval.consume().unwrap())
        .unwrap();
    assert_eq!(
        (frame.tick_from, frame.tick),
        (Some(initial.tick + 1), ticks_per_day)
    );
    let civil = session.end_civil_day_update().unwrap();
    civil.validate().unwrap();
    assert_eq!(civil.refresh.intraday.len(), 2);
    assert_eq!(civil.seq_from, frame.seq_to);
    let next = session.advance_core_tick().unwrap();
    let next = session
        .record_consumed_tick(next.consume().unwrap())
        .unwrap();
    assert_eq!(
        (next.tick, next.seq_from),
        (ticks_per_day + 1, civil.seq_to)
    );
}

#[test]
#[cfg(all(not(target_family = "wasm"), not(feature = "simulation-diagnostics")))]
fn every_tick_core_defers_full_view_until_interval_consumption() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    setup.runtime_sync_policy = crate::RuntimeSyncPolicy::EveryTick;
    let mut session = ProtocolSession::new(setup, 86).unwrap();
    session.step_frame().unwrap();
    crate::runtime::world::take_dayend_hotpath_for_test();
    let first = session.advance_core_tick().unwrap();
    let second = session.advance_core_tick().unwrap();
    let calls = crate::runtime::world::take_dayend_hotpath_for_test();
    assert_eq!(
        &calls[2..],
        &[0, 0],
        "普通核心 tick 不应逐拍构造完整外围 view"
    );
    let mut interval = super::DeferredInterval::new();
    interval.push(first).unwrap();
    interval.push(second).unwrap();
    let frame = session
        .record_consumed_interval(interval.consume().unwrap())
        .unwrap();
    let batch = session.tick_batch(vec![frame]).unwrap();
    batch.validate().unwrap();
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn coalesced_every_tick_matches_individual_facts_samples_and_final_view() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    let mut individual = ProtocolSession::new(setup.clone(), 87).unwrap();
    let mut coalesced = ProtocolSession::new(setup, 87).unwrap();
    individual.step_frame().unwrap();
    coalesced.step_frame().unwrap();
    let mut frames = Vec::new();
    let mut interval = super::DeferredInterval::new();
    for _ in 0..4 {
        let tick = individual.advance_core_tick().unwrap();
        frames.push(
            individual
                .record_consumed_tick(tick.consume().unwrap())
                .unwrap(),
        );
        interval
            .push(coalesced.advance_core_tick().unwrap())
            .unwrap();
    }
    let combined = coalesced
        .record_consumed_interval(interval.consume().unwrap())
        .unwrap();
    let expected_facts: Vec<_> = frames
        .iter()
        .flat_map(|frame| frame.facts.iter().cloned())
        .collect();
    assert_eq!(combined.facts, expected_facts);
    let expected_count: usize = frames
        .iter()
        .map(|frame| frame.timeseries_payload.continuous_points.len())
        .sum();
    let actual_count: usize = combined
        .timeseries_payload
        .continuous_samples
        .as_ref()
        .unwrap()
        .values()
        .map(Vec::len)
        .sum();
    assert!(expected_count > 0, "夹具必须实际包含连续采样");
    assert_eq!(actual_count, expected_count);
    for (code, samples) in combined
        .timeseries_payload
        .continuous_samples
        .as_ref()
        .unwrap()
    {
        let expected: Vec<_> = frames
            .iter()
            .filter_map(|frame| {
                frame
                    .timeseries_payload
                    .continuous_points
                    .get(code)
                    .map(|point| (point, &frame.timeseries_payload.active_daily_candles[code]))
            })
            .collect();
        assert_eq!(
            samples
                .iter()
                .map(|sample| (&sample.point, &sample.candle))
                .collect::<Vec<_>>(),
            expected
        );
    }
    let expected_batch = individual.tick_batch(frames).unwrap();
    let combined_batch = coalesced.tick_batch(vec![combined]).unwrap();
    assert_eq!(
        combined_batch.runtime_snapshot,
        expected_batch.runtime_snapshot
    );
    assert_eq!(combined_batch.runtime_delta, expected_batch.runtime_delta);
}
