//! 真实成交广播 actor-per-session 集成测试（ADR-0005 §5，无锁、GameSession 独占）。
//!
//! 直接驱动 SessionManager（不经 HTTP），验证 actor 行为：
//! - new_session 后 actor 启动并按 base_ms/speed 推 step；
//! - subscribe broadcast 能收到 EngineUpdate 批次（内部 Event 各带 seq）；
//! - Snapshot 命令返回完整快照；
//! - SetSpeed 调整 interval；
//! - intent 入队后被 actor 接受（Ok）；
//! - 未知 session_id 查询返回 None（不静默）。

include!("../../../packages/engine/test-support/simple_company.rs");

use engine::account::StockCode;
use engine::money::Money;
use engine::session::{NpcSetup, SecurityCategory, SessionSetup, StockExchange, StockSpec};
use engine::strategy::Intent;
use engine::{AccountId, NpcDecisionDiagnostics, Side, Snapshot, TradingPhase};
use server::actor::PublicBaselineSnapshot;
use server::SessionManager;
use std::collections::BTreeMap;

#[tokio::test]
async fn shared_market_members_receive_scoped_accounts_and_actor_enforces_control() {
    let mut setup = sample_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-02").unwrap();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    let manager = SessionManager::default();
    let session = manager.new_session(setup, 8).unwrap();
    let handles = manager.lookup(&session).unwrap();
    let creator = engine::session::OpaqueSubjectId::new("creator".into()).unwrap();
    let visitor = engine::session::OpaqueSubjectId::new("visitor".into()).unwrap();
    handles.bind_creator(creator.clone()).await.unwrap();
    let member = handles.join_member(visitor.clone(), false).await.unwrap();
    assert_ne!(member.account_id, AccountId(0));
    assert_eq!(handles.join_member(visitor.clone(), true).await.unwrap(), member);
    let own = handles.snapshot_for(visitor.clone()).await.unwrap();
    assert_eq!(own.accounts.keys().copied().collect::<Vec<_>>(), vec![member.account_id]);
    assert!(handles.set_running_for(1, visitor.clone(), true).await.is_err());
    assert!(handles.set_speed_for(1, visitor.clone(), 2.0).await.is_err());
    assert!(handles.set_admission_cash_for(1, visitor.clone(), Money::ZERO).await.is_err());
    handles.set_admission_cash_for(1, creator.clone(), Money::from_cents(1200)).await.unwrap();
    assert_eq!(handles.snapshot_for(visitor.clone()).await.unwrap().accounts[&member.account_id].cash, member.admission_funding.external_cash);
    handles.enqueue_for(1, visitor.clone(), Intent::PlaceLimit { code: StockCode("600101".into()), side: Side::Buy, price: engine::LimitPrice::Fixed(Money::from_cents(1000)), qty: 100 }).await.unwrap();
    handles.set_running_for(1, creator, true).await.unwrap();
    let mut updates = handles.subscribe_events();
    let update = tokio::time::timeout(std::time::Duration::from_secs(2), updates.recv()).await.unwrap().unwrap().for_account(member.account_id);
    let engine::session::protocol::EngineUpdate::TickBatch(batch) = update.update.unwrap() else { panic!("交易日应发布TickBatch") };
    batch.validate().unwrap();
    let delta = batch.runtime_delta.unwrap();
    assert!(delta.accounts.keys().all(|id| *id == member.account_id));
    assert!(delta.working_orders.upserts.iter().all(|order| order.owner == member.account_id));
    assert_eq!(handles.working_orders_for(1, visitor.clone()).await.unwrap().1.as_array().unwrap().len(), 1);
    assert!(handles.confirmations_for(1, visitor, None).await.unwrap().1.is_empty());
    manager.remove(&session).unwrap().shutdown().await.unwrap();
}

fn protocol_events(update: &server::EngineUpdate) -> Vec<&engine::Event> {
    match update.update.as_ref().expect("healthy protocol update") {
        engine::session::protocol::EngineUpdate::TickBatch(batch) => {
            batch.validate().unwrap();
            batch
                .frames
                .iter()
                .flat_map(|frame| &frame.events)
                .collect()
        }
        engine::session::protocol::EngineUpdate::CivilUpdate(civil) => {
            civil.validate().unwrap();
            civil.events.iter().collect()
        }
    }
}

/// 与 engine/tests/session.rs sample_setup 等价的最小合法 setup。
fn sample_setup() -> SessionSetup {
    SessionSetup {
        company_system: simple_company_fixture!(engine; ["600101"]),
        stocks: vec![StockSpec {
            code: StockCode("600101".to_string()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 10_000_000,
            float_shares: 0,
        }],
        npcs: NpcSetup {
            retail_count: 2,
            inst_count: 1,
            hot_count: 1,
            retail_cash_median: Money::from_cents(10_000_000),
        },
        config: engine::GameConfig::proposed_defaults(),
        strategy_params: engine::StrategyParams {
            retail: engine::RetailParams {
                arrival_rate: 0.5,
                order_size_mean: 100,
                chase_prob: 0.2,
            },
            inst: engine::InstParams {
                margin: 0.05,
                order_size: 200,
            },
            hot: engine::HotParams {
                lookback: 3,
                trend_threshold: 0.02,
                order_size: 200,
            },
        },
        ticks_per_day: 10,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 5,
        t1_enabled: true,
        report_frequency: engine::information::ReportFrequency::Quarterly,
        float_allocation: engine::FloatAllocation::random(),
        start_date: engine::CivilDate::from_iso("2030-01-01").unwrap(),
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_string(),
        dividend_tax_mode: engine::company::cash_dividend_tax::CashDividendTaxMode::Exempt,
    }
}

fn restore_setup() -> SessionSetup {
    let mut setup = sample_setup();
    // Saturday settles into Sunday without opening another market session.
    setup.start_date = engine::CivilDate::from_iso("2030-01-05").unwrap();
    // These tests cover restore metadata and retail experience, not NPC trading.
    // Keep both retail accounts while preventing initial queued strategy requests.
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.strategy_params.retail.arrival_rate = 0.0;
    setup
}

// Restore tests need a real completed civil day, not an intraday checkpoint.
async fn prepare_completed_day(handles: &server::actor::SessionHandles, seed: u64) {
    let mut game = engine::session::protocol::ProtocolSession::new(restore_setup(), seed)
        .expect("fixture session must start");
    // A weekend fixture has no market ticks or next-opening NPC requests.
    game.end_civil_day_update().expect("closed day must settle");
    let slot = game.save().expect("completed day must be saveable");
    assert_eq!(slot.snapshot.tick, 0);
    assert!(slot.civil_clock.settled_through.is_some());
    assert!(slot
        .pending_npc
        .as_ref()
        .is_none_or(|batch| batch.intents.is_empty()));
    assert!(slot.parent_orders.values().all(|plans| plans.is_empty()));
    handles
        .restore(slot)
        .await
        .expect("completed-day fixture must restore");
}

#[tokio::test]
async fn actor_diagnostics_rejects_stale_generation_without_records() {
    let manager = SessionManager::default();
    let id = manager.new_session(sample_setup(), 7).unwrap();
    let handles = manager.lookup(&id).unwrap();

    let (_, current) = handles
        .npc_decision_diagnostics(1, AccountId(1))
        .await
        .unwrap();
    let stale = handles
        .npc_decision_diagnostics(0, AccountId(1))
        .await
        .expect_err("stale diagnostics must be explicitly rejected");

    // actor 直接使用 engine 的 feature 能力；过期 generation 始终必须拒绝。
    #[cfg(feature = "simulation-diagnostics")]
    assert_eq!(
        current,
        NpcDecisionDiagnostics::Supported {
            records: Vec::new()
        }
    );
    #[cfg(not(feature = "simulation-diagnostics"))]
    assert_eq!(current, NpcDecisionDiagnostics::Unsupported);
    assert!(
        matches!(stale, server::SendCommandError::Rejected(ref reason)
        if reason == "STALE_SESSION_GENERATION: requested 0; current generation is 1")
    );
}

#[tokio::test]
async fn manager_new_session_is_available_by_id() {
    let mgr = SessionManager::default();
    let id = mgr
        .new_session(sample_setup(), 1)
        .expect("应能创建 session");
    assert!(mgr.lookup(&id).is_some(), "lookup 已存在 session 应命中");
    assert!(
        mgr.lookup("nope").is_none(),
        "lookup 未知 session 应 None（不静默）"
    );
}

#[tokio::test]
async fn restore_rejects_a_different_publisher_clock_configuration() {
    let mgr = SessionManager::default();
    let id = mgr.new_session(restore_setup(), 3).expect("创建 session");
    let handles = mgr.lookup(&id).expect("lookup 命中");
    prepare_completed_day(&handles, 3).await;
    let mut slot = handles.save(2, None).await.expect("应能存档");
    slot.setup.ticks_per_day += 1;

    let error = handles
        .restore(slot)
        .await
        .expect_err("不能用不同交易时钟配置破坏现有 Publisher 采样槽");
    assert!(error.to_string().contains("交易时钟配置与当前会话不一致"));
}

#[tokio::test]
async fn server_actor_restore_preserves_retail_experience_exactly() {
    let mgr = SessionManager::with_base_ms(10_000);
    let id = mgr.new_session(restore_setup(), 4).expect("创建 session");
    let handles = mgr.lookup(&id).expect("lookup 命中");
    prepare_completed_day(&handles, 4).await;
    let mut before = handles.save(2, None).await.expect("应能存档");
    assert_eq!(before.retail_experience.len(), 2);
    before
        .retail_experience
        .get_mut(&AccountId(1))
        .expect("首个散户必须有经历状态")
        .observe_stock(&StockCode("600101".to_string()), 0);
    assert_eq!(
        before.retail_experience[&AccountId(1)].stocks[&StockCode("600101".to_string())]
            .last_observed_market_minute,
        0,
        "测试必须跨 actor 恢复非默认经历字段"
    );

    handles.restore(before.clone()).await.expect("应能恢复存档");
    let after = handles.save(3, None).await.expect("恢复后应能再次存档");

    assert_eq!(
        serde_json::to_value(after).unwrap(),
        serde_json::to_value(before).unwrap(),
        "server actor 不得丢失散户经历状态"
    );
}

#[tokio::test]
async fn public_baseline_characterizes_a_new_session_visible_state() {
    // Given: a newly created paused session.
    let manager = SessionManager::with_base_ms(10_000);
    let id = manager
        .new_session(sample_setup(), 5)
        .expect("fixture session must start");
    let handles = manager.lookup(&id).expect("fixture handles must exist");

    // When: the client asks the actor for its connection baseline.
    let baseline = handles
        .public_baseline()
        .await
        .expect("baseline command must succeed");

    // Then: it reflects the existing player-visible market state and starts a timeline.
    assert_eq!(baseline.timeline_generation, 1);
    assert_eq!(baseline.snapshot.markets.len(), 1);
    assert_eq!(baseline.snapshot.accounts.len(), 1);
    assert!(!baseline.public_report_ids.is_empty());
}

#[test]
fn public_baseline_serializes_only_the_player_account() {
    // Given: a snapshot with both the player and an independent NPC account.
    let mut accounts = BTreeMap::new();
    accounts.insert(
        AccountId(0),
        engine::AccountSnap {
            cash: Money::from_cents(10_000),
            positions: BTreeMap::new(),
            reserved_cash: Money::ZERO,
            reserved_sell_qty: BTreeMap::new(),
        },
    );
    accounts.insert(
        AccountId(1),
        engine::AccountSnap {
            cash: Money::from_cents(9_999_999),
            positions: BTreeMap::new(),
            reserved_cash: Money::ZERO,
            reserved_sell_qty: BTreeMap::new(),
        },
    );
    let snapshot = Snapshot {
        seq: 7,
        tick: 7,
        day: 0,
        phase: TradingPhase::Continuous,
        markets: BTreeMap::new(),
        accounts,
        daily_candles: BTreeMap::new(),
        active_daily_candles: BTreeMap::new(),
    };

    // When: the server creates a public baseline projection.
    let baseline = PublicBaselineSnapshot::from(snapshot);
    let json = serde_json::to_value(baseline).expect("baseline must serialize");

    // Then: the player remains usable while the NPC account ID and cash cannot serialize.
    assert_eq!(
        json["accounts"].as_object().map(|accounts| accounts.len()),
        Some(1)
    );
    assert_eq!(json["accounts"]["0"]["cash"], "10000");
    assert!(json["accounts"].get("1").is_none());
    assert!(!json.to_string().contains("9999999"));
}

#[tokio::test]
async fn restore_rotates_the_actor_timeline_generation() {
    let mgr = SessionManager::with_base_ms(10_000);
    let id = mgr.new_session(restore_setup(), 5).unwrap();
    let handles = mgr.lookup(&id).unwrap();
    prepare_completed_day(&handles, 5).await;
    let before = handles.public_baseline().await.unwrap();
    let slot = handles.save(2, None).await.unwrap();

    handles.restore(slot).await.unwrap();
    let after = handles.public_baseline().await.unwrap();

    assert_eq!(after.timeline_generation, before.timeline_generation + 1);
    assert_eq!(after.public_revision, before.public_revision + 1);
    assert_eq!(after.snapshot.seq, before.snapshot.seq);
}

#[tokio::test]
async fn restore_notifies_subscribers_to_gate_the_previous_public_timeline() {
    // Given: a client subscribed to a stable public timeline.
    let manager = SessionManager::with_base_ms(10_000);
    let id = manager
        .new_session(restore_setup(), 6)
        .expect("fixture session must start");
    let handles = manager.lookup(&id).expect("fixture handles must exist");
    prepare_completed_day(&handles, 6).await;
    let before = handles
        .public_baseline()
        .await
        .expect("baseline command must succeed");
    let slot = handles.save(2, None).await.expect("save must succeed");
    let mut updates = handles.subscribe_events();

    // When: the actor atomically restores the save.
    handles.restore(slot).await.expect("restore must succeed");

    // Then: subscribers receive a no-delta timeline gate with a fresh revision.
    let update = tokio::time::timeout(std::time::Duration::from_secs(1), updates.recv())
        .await
        .expect("restore must notify connected clients")
        .expect("restore notification channel must remain open");
    assert!(update.update.is_none());
    assert_eq!(update.timeline_generation, before.timeline_generation + 1);
    assert_eq!(update.public_revision, before.public_revision + 1);
}

#[tokio::test]
async fn actor_broadcasts_events_with_seq() {
    // 用很小的 base_ms 让 actor 快速跑出 step 事件。
    let mgr = SessionManager::with_base_ms(5);
    let id = mgr.new_session(sample_setup(), 42).expect("创建 session");
    let handles = mgr.lookup(&id).expect("lookup 命中");

    // 订阅事件流（必须在 step 前 subscribe，否则丢历史；连接先发快照对齐基线的设计见 ws 路由）。
    let mut rx = handles.subscribe_events();
    handles.set_running(true).await.expect("应能启动会话");

    // 等收到至少一个事件（PriceTick 每 step 一定出）。
    let mut got_price_tick = false;
    for _ in 0..200 {
        match tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv()).await {
            Ok(Ok(update)) => {
                assert!(!protocol_events(&update).is_empty(), "更新批次不得为空");
                assert!(
                    protocol_events(&update).iter().all(|ev| ev.seq() > 0),
                    "事件必须带正 seq"
                );
                if protocol_events(&update)
                    .iter()
                    .any(|ev| matches!(ev, engine::Event::PriceTick { .. }))
                {
                    got_price_tick = true;
                    break;
                }
            }
            Ok(Err(_)) => break, // lagged 或关闭
            Err(_) => continue,
        }
    }
    assert!(got_price_tick, "actor 应广播 PriceTick 事件");
}

#[tokio::test]
async fn actor_settles_a_market_day_without_a_host_failure() {
    // Given: a session that reaches a day boundary while a subscriber is connected.
    let manager = SessionManager::with_base_ms(5);
    let id = manager
        .new_session(sample_setup(), 47)
        .expect("fixture session must start");
    let handles = manager.lookup(&id).expect("fixture handles must exist");
    let mut updates = handles.subscribe_events();
    handles.set_running(true).await.expect("session must start");

    // When: the actor advances through one market day.
    for _ in 0..20 {
        let update = tokio::time::timeout(std::time::Duration::from_secs(1), updates.recv())
            .await
            .expect("actor must continue broadcasting")
            .expect("actor broadcast must remain open");

        // Then: the completed boundary contains the engine-owned civil event and no failure.
        assert!(
            update.failure.is_none(),
            "civil settlement must not fail: {:?}",
            update.failure
        );
        if protocol_events(&update)
            .iter()
            .any(|event| matches!(event, engine::Event::CivilDateAdvanced { .. }))
        {
            return;
        }
    }
    panic!("actor did not emit CivilDateAdvanced after a market day");
}

#[cfg(feature = "host-parity")]
#[tokio::test]
async fn actor_advances_one_closed_civil_day_through_its_command_queue() {
    // Given: a paused actor whose initial Tuesday has no completed market session yet.
    let manager = SessionManager::with_base_ms(10_000);
    let id = manager
        .new_session(sample_setup(), 48)
        .expect("fixture session must start");
    let handles = manager.lookup(&id).expect("fixture handles must exist");

    // When: the real actor command settles exactly one civil day.
    let report = handles
        .advance_civil_day(1)
        .await
        .expect("closed civil day must settle through the actor queue");

    // Then: it delivers the engine-owned date event and authoritative public date.
    assert!(report
        .events
        .iter()
        .any(|event| matches!(event, engine::Event::CivilDateAdvanced { .. })));
    assert_eq!(
        handles
            .public_baseline()
            .await
            .expect("baseline must succeed")
            .civil_date,
        "2030-01-02"
    );
}

#[cfg(feature = "host-parity")]
#[tokio::test]
async fn actor_rejects_stale_civil_day_command_without_mutating_the_timeline() {
    let manager = SessionManager::with_base_ms(10_000);
    let id = manager
        .new_session(sample_setup(), 49)
        .expect("fixture session must start");
    let handles = manager.lookup(&id).expect("fixture handles must exist");
    let before = handles
        .public_baseline()
        .await
        .expect("baseline must succeed");

    let rejection = handles.advance_civil_day(0).await;

    assert!(rejection.is_err());
    assert_eq!(
        handles
            .public_baseline()
            .await
            .expect("baseline must succeed")
            .civil_date,
        before.civil_date
    );
}

#[tokio::test]
async fn actor_snapshot_command_returns_player_visible_snapshot() {
    let mgr = SessionManager::with_base_ms(10_000); // 慢 interval，避免 step 干扰
    let id = mgr.new_session(sample_setup(), 42).expect("创建 session");
    let handles = mgr.lookup(&id).expect("lookup 命中");

    // 真实路径：经 SessionHandles 提供的 snapshot helper（内部发 Snapshot 命令给 actor）。
    let real_snap = handles.snapshot().await.expect("snapshot 应返回 Ok");
    assert_eq!(real_snap.markets.len(), 1, "快照应含全部 markets");
    assert_eq!(
        real_snap.accounts.len(),
        1,
        "玩家快照只应包含玩家账户，NPC 私有状态留在引擎和存档中"
    );
}

#[tokio::test]
async fn actor_enqueue_intent_accepted_for_known_player() {
    let mgr = SessionManager::with_base_ms(10_000);
    let id = mgr.new_session(sample_setup(), 42).expect("创建 session");
    let handles = mgr.lookup(&id).expect("lookup 命中");

    // 玩家 AccountId(0) 存在 → 入队 Ok（当前单玩家模式固定 player 0）。
    handles
        .enqueue(1, Intent::PlaceLimit {
            code: StockCode("600101".to_string()),
            side: Side::Buy,
            price: engine::LimitPrice::Fixed(Money::from_cents(1000)),
            qty: 100,
        })
        .await
        .expect("玩家意图应入队成功");
}

/// 固定种子的短成交 fixture：少量真实 NPC 提供买方，玩家开局库存来自守恒转移。
fn active_market_setup() -> SessionSetup {
    let mut setup = sample_setup();
    setup.npcs.retail_count = 20;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.npcs.retail_cash_median = Money::from_cents(10_000_000);
    setup.strategy_params.retail.arrival_rate = 1.0;
    setup.stocks[0].float_shares = 100_000;
    setup.start_date = engine::CivilDate::from_iso("2030-01-05").unwrap();
    setup.float_allocation = engine::FloatAllocation::class_percentages(1.0, 0.0, 0.0, engine::WithinKindDistribution::Random);
    setup
}

/// 真实 actor 成交广播契约：加载守恒编辑的日级开局库存，玩家卖单与真实 NPC 买单撮合。
/// 不凭有限随机市场一定成交的假设等待，也不 mock Trade；全部推进、结算及广播均走生产路径。
#[tokio::test]
async fn actor_market_goes_live_produces_trade_events() {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let mgr = SessionManager::with_base_ms(5);
        let id = mgr
            .new_session(active_market_setup(), 42)
            .expect("创建 session");
        let handles = mgr.lookup(&id).expect("lookup 命中");

        let mut initial =
            engine::session::protocol::ProtocolSession::new(active_market_setup(), 42)
                .expect("真实周末 fixture 应能创建");
        initial
            .end_civil_day_update()
            .expect("周末自然日应成功日结");
        let mut slot = initial.save().expect("成功自然日日结应能生成公共存档");
        assert_eq!(slot.snapshot.tick, 0, "周末 fixture 不得运行任何市场 tick");
        assert!(slot.resting_orders.values().all(Vec::is_empty));
        assert!(slot.auction_orders.values().all(Vec::is_empty));
        assert!(slot.filled_orders.values().all(Vec::is_empty));
        assert!(slot.runtime_state.live_envelopes.is_empty());
        assert!(slot.parent_orders.values().all(BTreeMap::is_empty));
        assert!(slot.pending_player.is_empty());
        assert!(slot.npc_order_lifecycles.is_empty());
        // 可编辑存档的受控开局条件：仅丢弃未受理的初始 NPC 输入，不冒充成交或撤单；
        // 已受理委托为空，后续买单必须由真实 NPC 观察重新生成。
        let pending_npc = slot.pending_npc.as_mut().expect("开局应保留当前观察截点");
        pending_npc.intents.clear();
        pending_npc.dependencies.clear();
        let code = StockCode("600101".to_string());
        let totals = |slot: &engine::session::SaveSlot| {
            slot.snapshot
                .accounts
                .values()
                .fold((0_i128, 0_u64, 0_i128), |total, account| {
                    let position = account.positions.get(&code);
                    (
                        total.0 + i128::from(account.cash.cents()),
                        total.1 + position.map_or(0, |position| u64::from(position.qty)),
                        total.2
                            + position.map_or(0, |position| i128::from(position.invested_cents)),
                    )
                })
        };
        let before = totals(&slot);
        let donor = slot
            .snapshot
            .accounts
            .iter_mut()
            .filter(|(owner, _)| **owner != AccountId(0))
            .find_map(|(_, account)| {
                account
                    .positions
                    .get_mut(&code)
                    .filter(|position| position.qty >= 100)
            })
            .expect("真实初始流通盘应提供 100 股 fixture 库存");
        assert_eq!(donor.t1_locked, 0);
        assert_eq!(donor.recovered_cents, 0);
        let invested_cents = slot.setup.stocks[0].initial_price.cents() * 100;
        donor.qty -= 100;
        donor.invested_cents -= invested_cents;
        slot.snapshot
            .accounts
            .get_mut(&AccountId(0))
            .expect("玩家账户应存在")
            .positions
            .insert(
                code.clone(),
                engine::PositionSnap {
                    qty: 100,
                    t1_locked: 0,
                    invested_cents,
                    recovered_cents: 0,
                },
            );
        assert_eq!(
            totals(&slot),
            before,
            "开局库存转移必须保持全户现金、股份与投入成本守恒，不冒充成交"
        );
        handles
            .restore(slot)
            .await
            .expect("当前完整日级存档应恢复开局库存");

        let mut rx = handles.subscribe_events();
        handles.set_running(true).await.expect("应能推进周末自然日");
        loop {
            let update = rx.recv().await.expect("自然日事件通道应保持有效");
            assert!(
                update.failure.is_none(),
                "周末推进不得失败：{:?}",
                update.failure
            );
            if update.civil_date == "2030-01-07" {
                break;
            }
        }
        handles
            .enqueue(2, Intent::PlaceLimit {
                code: StockCode("600101".to_string()),
                side: Side::Sell,
                price: engine::LimitPrice::Lowest,
                qty: 100,
            })
            .await
            .expect("玩家真实卖单应入队成功");
        handles
            .set_speed(f64::INFINITY)
            .await
            .expect("活跃市场测试应能启用最快模式");
        handles.set_running(true).await.expect("应能启动会话");

        loop {
            match rx.recv().await {
                Ok(update) => {
                    assert!(
                        update.failure.is_none(),
                        "真实成交 fixture 不得出现宿主失败：{:?}",
                        update.failure
                    );
                    if let Some(engine::Event::Trade {
                        seq,
                        code,
                        qty,
                        maker,
                        taker,
                        ..
                    }) = protocol_events(&update)
                        .iter()
                        .find(|ev| matches!(ev, engine::Event::Trade { maker, taker, .. } if *maker == AccountId(0) || *taker == AccountId(0)))
                    {
                        assert!(*seq > 0, "Trade 必须带正 seq");
                        assert!(*qty > 0, "Trade 成交量必须 >0");
                        assert_ne!(*maker, *taker, "Trade 的 maker/taker 必须是不同账户");
                        assert!(
                            *maker == AccountId(0) || *taker == AccountId(0),
                            "真实成交必须包含提供开局库存卖单的玩家"
                        );
                        assert_eq!(code.0, "600101", "成交股票代码应匹配 setup");
                        handles
                            .set_running(false)
                            .await
                            .expect("成交后应能暂停会话");
                        return;
                    }
                }
                Err(error) => panic!("真实成交事件通道不得丢失或关闭：{error}"),
            }
        }
    })
    .await
    .expect("真实 NPC 买方与玩家开局库存卖单应在共享 2 秒 deadline 内撮合并广播 Trade");
}

#[tokio::test]
async fn actor_set_speed_applied_without_error() {
    let mgr = SessionManager::with_base_ms(10_000);
    let id = mgr.new_session(sample_setup(), 42).expect("创建 session");
    let handles = mgr.lookup(&id).expect("lookup 命中");

    // 提速到 10x：不应报错；SetSpeed 仅改 interval。
    handles.set_speed(10.0).await.expect("SetSpeed 应 Ok");
    // 再设回 1x。
    handles.set_speed(1.0).await.expect("SetSpeed 应 Ok");
}

#[tokio::test]
async fn idempotent_running_command_preserves_the_completed_speed_sample() {
    let mgr = SessionManager::with_base_ms(20);
    let id = mgr.new_session(sample_setup(), 42).expect("创建 session");
    let handles = mgr.lookup(&id).expect("lookup 命中");

    handles.set_running(true).await.expect("应能启动会话");
    tokio::time::sleep(std::time::Duration::from_millis(550)).await;
    assert!(
        handles
            .speed_metrics()
            .await
            .expect("应能读取测速")
            .actual_multiplier
            .is_some(),
        "运行超过采样窗口后应已有实测倍率"
    );

    handles.set_running(true).await.expect("重复启动应幂等成功");
    assert!(
        handles
            .speed_metrics()
            .await
            .expect("应能读取测速")
            .actual_multiplier
            .is_some(),
        "幂等运行命令不应清空已完成采样"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actor_fastest_yields_after_its_slice_and_services_commands() {
    let mgr = SessionManager::with_base_ms(10_000);
    let id = mgr.new_session(sample_setup(), 42).expect("创建 session");
    let handles = mgr.lookup(&id).expect("lookup 命中");

    handles
        .set_speed(f64::INFINITY)
        .await
        .expect("Fastest 应切换为无固定周期的推进模式");
    handles.set_running(true).await.expect("应能启动会话");
    let mut events = handles.subscribe_events();
    tokio::time::timeout(std::time::Duration::from_secs(1), events.recv())
        .await
        .expect("Fastest 应持续推进并产生事件")
        .expect("Fastest 事件通道不应关闭");
    tokio::time::timeout(std::time::Duration::from_millis(250), handles.snapshot())
        .await
        .expect("Fastest 每个 14ms 时间片后必须让出执行权并在有界时间内响应快照")
        .expect("Fastest 下快照命令应成功");
}

#[tokio::test]
async fn actor_rejects_invalid_speed() {
    let mgr = SessionManager::with_base_ms(10_000);
    let id = mgr.new_session(sample_setup(), 42).expect("创建 session");
    let handles = mgr.lookup(&id).expect("lookup 命中");
    assert!(matches!(
        handles.set_speed(0.0).await,
        Err(server::SendCommandError::InvalidSpeed(0.0))
    ));
}
