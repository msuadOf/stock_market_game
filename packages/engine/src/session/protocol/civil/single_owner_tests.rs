//! 架构项 #6 的生产入口红测：一批有类型事件记录（EventFact）单 owner 贯穿
//! producer → outbox → candidate → commit → frame/civil → wire。
//!
//! 真实 GameSession→ProtocolSession 入口验证：
//! 1. 一记录贯穿：wire 上每条事件只随事实记录出现一次，没有独立 events 数组，
//!    事实记录也没有 canonical payload 字符串；
//! 2. 多股任意到达：所有已发布事实的 StableKey 全局唯一、seq 覆盖精确连续；
//! 3. 三权限集合不串：公司事实、公开披露与 NPC 本人获知保持不同可见集合；
//! 4. 失败无污染：tick 失败不发布任何事实，authority 可从 checkpoint 重试；
//! 5. save/restore/wire：往返后事实记录字段完整（key 全字段 + 事件载荷）。

use crate::session::{Event, GameSession, StepFatal};

/// 双股票 + 散户/机构 NPC 的最小生产配置。
fn two_stock_setup() -> crate::SessionSetup {
    use crate::*;
    SessionSetup {
        company_operations: None,
        groups: Vec::new(),
        stocks: vec![
            StockSpec {
                code: StockCode("600101".into()),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(1000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.1,
                tick: Money::from_cents(1),
                total_shares: 1_000_000,
                float_shares: 0,
            },
            StockSpec {
                code: StockCode("600102".into()),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(2000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.1,
                tick: Money::from_cents(1),
                total_shares: 1_000_000,
                float_shares: 0,
            },
        ],
        npcs: NpcSetup {
            retail_count: 2,
            inst_count: 1,
            hot_count: 0,
            retail_cash_median: Money::ZERO,
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.0,
                order_size_mean: 100,
                chase_prob: 0.0,
            },
            inst: InstParams {
                margin: 0.05,
                order_size: 100,
            },
            hot: HotParams {
                lookback: 3,
                trend_threshold: 0.02,
                order_size: 100,
            },
        },
        ticks_per_day: 8,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 5,
        t1_enabled: true,
        float_allocation: FloatAllocation::random(),
        start_date: CivilDate::from_iso("2030-01-02").unwrap(),
        simulation_policy_id: SIMULATION_POLICY_ID.into(),
    }
}

/// 1. 一记录贯穿：wire JSON 不再有独立 events 数组，事实记录没有 canonical_payload。
#[test]
fn wire_serializes_each_event_exactly_once_inside_its_fact() {
    let mut session = super::ProtocolSession::new(two_stock_setup(), 61).unwrap();
    let frame = session.step_frame().unwrap();
    let wire = serde_json::to_value(&frame).unwrap();

    assert!(
        wire.get("events").is_none(),
        "TickFrame wire 不得再冗余序列化独立 events 数组；事件只应随 facts 记录出现一次"
    );
    let facts = wire
        .get("facts")
        .and_then(|facts| facts.as_array())
        .expect("TickFrame wire 必须携带 facts");
    assert!(!facts.is_empty(), "首个 tick 至少应有一条价格事实");
    for fact in facts {
        assert!(
            fact.get("canonical_payload").is_none(),
            "事实记录不得再携带 canonical payload 字符串"
        );
        assert!(
            fact.get("event").is_some() && fact.get("key").is_some(),
            "事实记录必须由 key 与 event 组成"
        );
    }
}

/// 2. 多股任意到达：单 tick 内全部事实 key 唯一，跨 tick 的 (seq_from, seq_to] 覆盖精确。
#[test]
fn multi_stock_facts_keep_unique_keys_and_exact_coverage_across_ticks() {
    use std::collections::BTreeSet;

    let mut session = super::ProtocolSession::new(two_stock_setup(), 62).unwrap();
    let mut covered: BTreeSet<u64> = BTreeSet::new();
    for _ in 0..4 {
        let mut tick_keys = BTreeSet::new();
        let frame = session.step_frame().unwrap();
        frame.validate().unwrap();
        let mut arbitrary_arrival = frame.clone();
        arbitrary_arrival.facts.reverse();
        arbitrary_arrival.validate().unwrap();
        let mut duplicate_arrival = arbitrary_arrival;
        duplicate_arrival.facts[1] = duplicate_arrival.facts[0].clone();
        assert!(
            duplicate_arrival.validate().is_err(),
            "多股真实 producer 输出仍须按 typed key 检出重复"
        );
        assert_eq!(
            frame.seq_from,
            covered.iter().next_back().copied().unwrap_or(0)
        );
        for fact in &frame.facts {
            assert!(
                tick_keys.insert(fact.key.clone()),
                "同一 tick 内事实 key 必须唯一：{:?}",
                fact.key
            );
            let seq = fact.event.seq();
            assert!(seq > frame.seq_from && seq <= frame.seq_to);
            assert!(covered.insert(seq), "事件 seq 不得重复：{seq}");
        }
    }
}

/// 3. 三权限集合不串：公开披露集合 == refresh.public_publication_ids；
///    NPC 本人获知不改变公开集合；披露事件只引用公开集合中的 id。
#[test]
fn disclosure_public_index_and_private_acquisition_stay_separate() {
    let mut setup = two_stock_setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-01").unwrap();
    let mut session = GameSession::new(setup.clone(), 63).unwrap();
    let mut acquired = GameSession::new(setup, 63).unwrap();
    // 公司经营事实的变化不等于已经公开，也不等于某个 NPC 已获知。
    let public_before = acquired.state.library.all_publication_ids().unwrap();
    let business_before = acquired.business_state_hash().unwrap();
    let date = acquired.civil_date();
    let shock = crate::company::ActiveShock {
        kind: crate::company::ShockKind::ContractWon,
        amplitude_bp: 1000,
        starts_on: date,
        expires_on: date.next().unwrap(),
    };
    std::sync::Arc::make_mut(&mut acquired.state.operations)
        .apply_company_shock(&crate::company::CompanyId(0), shock.clone())
        .unwrap();
    std::sync::Arc::make_mut(&mut session.state.operations)
        .apply_company_shock(&crate::company::CompanyId(0), shock)
        .unwrap();
    assert_ne!(acquired.business_state_hash().unwrap(), business_before);
    assert_eq!(
        acquired.state.library.all_publication_ids().unwrap(),
        public_before
    );
    let public_id = *acquired
        .state
        .library
        .all_publication_ids()
        .unwrap()
        .first()
        .unwrap();
    let owner = *acquired.state.belief_participants.keys().next().unwrap();
    let mut private = crate::information::NpcInformationState::new(owner);
    private
        .record_acquisition(
            owner,
            &acquired.state.library,
            public_id,
            crate::calendar::CivilInstant::new(acquired.civil_date(), 0).unwrap(),
        )
        .unwrap();
    *acquired
        .state
        .belief_participants
        .get_mut(&owner)
        .unwrap()
        .information_mut() = private;

    // 从真实 GameSession 装配到 ProtocolSession；不使用伪造 frame 代替生产发布。
    let now = crate::calendar::CivilInstant::new(acquired.civil_date(), 0).unwrap();
    let viewer = crate::information::NpcObservationContext::new(
        owner,
        acquired.state.belief_participants[&owner].information(),
        &acquired.state.library,
        &(),
    )
    .unwrap();
    assert!(viewer.report(public_id).is_ok() || viewer.announcement(public_id).is_ok());
    let outsider_id = crate::AccountId(owner.0 + 1);
    *acquired
        .state
        .belief_participants
        .get_mut(&outsider_id)
        .unwrap()
        .information_mut() = crate::information::NpcInformationState::new(outsider_id);
    let outsider = crate::information::NpcObservationContext::new(
        outsider_id,
        acquired.state.belief_participants[&outsider_id].information(),
        &acquired.state.library,
        &(),
    )
    .unwrap();
    assert!(outsider.report(public_id).is_err() && outsider.announcement(public_id).is_err());
    assert!(
        acquired.state.library.report(public_id, now).is_ok()
            || acquired.state.library.announcement(public_id, now).is_ok()
    );
    let mut session = super::ProtocolSession::from_game(session, None);
    let mut acquired = super::ProtocolSession::from_game(acquired, None);
    let update = session.end_civil_day_update().unwrap();
    let acquired_update = acquired.end_civil_day_update().unwrap();
    update.validate().unwrap();

    // NPC 本人获知不得改变公开披露集合。
    assert_eq!(
        update.refresh.public_publication_ids,
        acquired_update.refresh.public_publication_ids
    );
    // 必须真的经过披露分支；空集合不能冒充权限边界已经覆盖。
    let disclosures = update
        .facts
        .iter()
        .filter(|fact| matches!(fact.event, Event::CompanyDisclosurePublished { .. }))
        .count();
    assert!(
        disclosures > 0,
        "本例必须产生真实 CompanyDisclosurePublished 事实"
    );
    assert_eq!(
        disclosures,
        acquired_update
            .facts
            .iter()
            .filter(|fact| matches!(fact.event, Event::CompanyDisclosurePublished { .. }))
            .count()
    );
    // 披露事件只能引用公开集合中的 id。
    for fact in &update.facts {
        if let Event::CompanyDisclosurePublished { publication_id, .. } = &fact.event {
            assert!(update
                .refresh
                .public_publication_ids
                .contains(&publication_id.value().to_string()));
        }
    }
}

/// 4. 失败无污染：注入 StepFatal 后 step_frame 失败且不发布任何事实；
///    回滚 checkpoint 后重试成功，seq 从失败前的游标继续。
#[test]
fn failed_tick_publishes_no_facts_and_retries_from_untouched_cursor() {
    let mut session = super::ProtocolSession::new(two_stock_setup(), 64).unwrap();
    let first = session.step_frame().unwrap();
    let mut published = std::collections::BTreeSet::new();
    published.extend(
        first
            .facts
            .iter()
            .map(|fact| (first.tick, fact.key.clone())),
    );
    let checkpoint = session.checkpoint().unwrap();
    let before_seq = session.seq();

    session
        .state
        .game
        .inject_step_failure(StepFatal::InvariantViolation {
            description: "测试注入的失败".into(),
            location: "single_owner_tests".into(),
        });
    let failure = session.step_frame().unwrap_err();
    assert!(matches!(
        failure,
        StepFatal::InvariantViolation { ref description, .. } if description == "测试注入的失败"
    ));
    assert_eq!(session.seq(), before_seq, "失败不得推进权威 seq 游标");

    session.rollback(checkpoint);
    let retry = session.step_frame().unwrap();
    retry.validate().unwrap();
    for fact in &retry.facts {
        assert!(
            published.insert((retry.tick, fact.key.clone())),
            "失败后的重试不得复用已发布 key：{:?}",
            fact.key
        );
    }
}

/// 5. save/restore/wire：恢复后的会话继续发布的事实字段完整，wire 往返不丢信息。
#[test]
fn restored_session_publishes_complete_facts_and_wire_roundtrips() {
    let mut session = super::ProtocolSession::new(two_stock_setup(), 65).unwrap();
    let first = session.step_frame().unwrap();
    for _ in 1..session.state.game.state.setup.ticks_per_day {
        session.step_frame().unwrap();
    }
    session.end_civil_day_update().unwrap();
    let save = session.save().unwrap();
    let mut restored = super::ProtocolSession::restore(&save).unwrap();
    let second = restored.step_frame().unwrap();
    second.validate().unwrap();
    assert!(restored.seq() > first.seq_to);

    let wire = serde_json::to_value(&second).unwrap();
    let parsed: crate::session::protocol::TickFrame = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(parsed.facts.len(), second.facts.len());
    for (left, right) in parsed.facts.iter().zip(second.facts.iter()) {
        assert_eq!(&left.key, &right.key);
        assert_eq!(
            left.event, right.event,
            "wire 往返必须保留完整事件字段而非只有 seq"
        );
    }
    let mut obsolete = wire.clone();
    obsolete["events"] = serde_json::json!([]);
    assert!(serde_json::from_value::<crate::session::protocol::TickFrame>(obsolete).is_err());
    let mut obsolete = wire;
    obsolete["facts"][0]["canonical_payload"] = serde_json::json!("{}");
    assert!(serde_json::from_value::<crate::session::protocol::TickFrame>(obsolete).is_err());
}

#[test]
fn real_trade_fact_retains_both_receipt_legs_and_t1_delivery() {
    use crate::session::pipeline::ReceiptKind;
    use crate::{AccountId, AccountKind, Intent, LimitPrice, Money, Side};
    let mut setup = two_stock_setup();
    setup.stocks[0].float_shares = 1_000;
    let mut game = GameSession::new(setup, 67).unwrap();
    let code = game.stock_id("600101").unwrap();
    let seller = game
        .state
        .accounts
        .iter()
        .find(|(_, account)| {
            account.kind() != AccountKind::Player
                && account
                    .position(&code)
                    .is_some_and(|position| position.qty() >= 100)
        })
        .map(|(id, _)| *id)
        .expect("真实开局必须给至少一个 NPC 分配可卖股份");
    let received = game
        .state
        .ingress_receipt_cursors
        .receive(
            seller,
            Intent::PlaceLimit {
                code,
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        )
        .unwrap();
    crate::session::pipeline::staged_decision::stage(crate::session::pipeline::NpcDecisionHandover::new(game.tick(), vec![seller], vec![received], Vec::new()));
    game.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code,
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
    )
    .unwrap();
    let mut protocol = super::ProtocolSession::from_game(game, None);
    let (frame, evidence) = protocol.step_frame_with_commit_evidence().unwrap();
    frame.validate().unwrap();
    let trades = frame
        .facts
        .iter()
        .filter(|fact| matches!(fact.event, Event::Trade { .. }))
        .collect::<Vec<_>>();
    let [trade] = trades.as_slice() else {
        panic!("真实成交必须恰好产生一条 Trade 事实：{trades:?}")
    };
    let Event::Trade {
        code: traded,
        price,
        qty,
        maker,
        taker,
        ..
    } = &trade.event
    else {
        unreachable!()
    };
    assert_eq!(
        (*traded, *price, *qty),
        (code, Money::from_cents(1_000), 100)
    );
    assert_eq!(
        std::collections::BTreeSet::from([*maker, *taker]),
        std::collections::BTreeSet::from([AccountId(0), seller])
    );
    let fills = evidence
        .receipts()
        .iter()
        .filter(|receipt| receipt.kind == ReceiptKind::Fill)
        .collect::<Vec<_>>();
    assert_eq!(
        fills.len(),
        2,
        "每条 Trade 保留真实买卖两个 Fill receipt，不重造身份"
    );
    for receipt in &fills {
        assert_eq!(receipt.envelope.stock, code);
        assert_eq!(receipt.qty_before - receipt.qty_after, *qty);
        assert_eq!(
            receipt.value_after.cents() - receipt.value_before.cents(),
            100_000
        );
        assert_eq!(receipt.local_key.transition_envelope(), &receipt.envelope);
    }
    assert_eq!(
        std::collections::BTreeSet::from([fills[0].envelope.account, fills[1].envelope.account]),
        std::collections::BTreeSet::from([*maker, *taker])
    );
    assert_eq!(
        protocol.game().snapshot().accounts[&AccountId(0)].positions[&code].t1_locked,
        100
    );
}

#[test]
fn civil_publication_copies_history_only_for_the_delivery_boundary() {
    use crate::session::protocol::take_frame_clones_for_test;
    let setup = two_stock_setup();
    let frame_count = setup.ticks_per_day as usize;
    let mut session = super::ProtocolSession::new(setup, 68).unwrap();
    let mut fact_count = 0;
    for _ in 0..frame_count {
        fact_count += session.step_frame().unwrap().facts.len();
    }
    take_frame_clones_for_test();
    let update = session.end_civil_day_update().unwrap();
    assert_eq!(update.refresh.intraday.len(), frame_count);
    assert_eq!(
        take_frame_clones_for_test(),
        (frame_count, fact_count),
        "只允许从保留的历史构造一次 owned 交付数组；只读校验及 project 不得复制 Frame/EventFact"
    );
    update.validate().unwrap();
    assert_eq!(
        take_frame_clones_for_test(),
        (0, 0),
        "对已经 owned 的 CivilUpdate 重复校验必须零复制"
    );
}

#[test]
fn projection_moves_the_same_record_batch_and_rejected_callback_installs_nothing() {
    let mut game = GameSession::new(two_stock_setup(), 66).unwrap();
    let before = game.business_state_hash().unwrap();
    let mut projected = 0;
    let failure = game.step_frame_and_commit(false, |projection, facts| {
        assert!(!facts.is_empty());
        let owner = facts.as_ptr();
        let frame = projection.project_frame(facts)?;
        assert_eq!(
            owner,
            frame.facts.as_ptr(),
            "candidate→frame 必须移动同一记录批次而非重建 event/key"
        );
        frame.validate().unwrap();
        projected += frame.facts.len();
        Err::<(), _>(StepFatal::InvariantViolation {
            location: "single_owner_tests::precommit".into(),
            description: "拒绝已构造但未提交的 publication".into(),
        })
    });
    assert!(failure.is_err());
    assert!(
        projected > 0,
        "必须真实执行到 candidate publication callback"
    );
    assert_eq!(game.business_state_hash().unwrap(), before);
    assert!(game.poison_reason().is_none());
    let mut session = super::ProtocolSession::from_game(game, None);
    let frame = session.step_frame().unwrap();
    assert_eq!(frame.tick, 1);
    frame.validate().unwrap();
}
