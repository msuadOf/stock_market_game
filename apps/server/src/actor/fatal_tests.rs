#[cfg(not(feature = "simulation-diagnostics"))]
const HISTORICAL_WINDOW: usize = 8;
use super::session_fixture as fixture;
use super::*;
include!("../../../../packages/engine/test-support/simple_company.rs");

#[test]
fn company_read_member_request_projects_the_actual_nonzero_members_data() {
    use engine::session::protocol::{CompanyReadQuery, CompanyReadResult};
    let (mut actor, _) = catalog_actor_fixture();
    let mut setup = actor.game.export_setup_for_runtime();
    setup.company_system = Some(simple_company_fixture!(engine; ["600101"]));
    setup.dividend_tax_mode = Some(engine::company::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1000);
    setup.par_value_per_share = Some(engine::Money::from_cents(100));
    setup.auto_corporate_foundation = true;
    actor.game = ProtocolSession::new(setup, 7).unwrap();
    actor.ingress = Arc::new(RwLock::new((1, actor.game.shared_ingress())));
    let subject = OpaqueSubjectId::new("company-query-nonzero".into()).unwrap();
    let owner = actor
        .game
        .join_market(subject.clone(), true)
        .unwrap()
        .account_id;
    *actor.members.write().unwrap() = actor.game.trading_subject_accounts();
    assert_ne!(owner, AccountId(0));
    let expected = actor.game.account_dividend_tax_status(owner).unwrap();
    let before = serde_json::to_value(actor.game.game().save().unwrap()).unwrap();
    let response = actor
        .handle_member_request(
            subject,
            MemberRequest::CompanyRead {
                generation: 1,
                query: CompanyReadQuery::AccountDividendTaxStatus,
            },
        )
        .unwrap();
    let MemberResponse::CompanyRead(
        1,
        response_owner,
        Ok(CompanyReadResult::AccountDividendTaxStatus(view)),
    ) = response
    else {
        panic!("响应类型错误")
    };
    assert_eq!(response_owner, owner);
    assert_eq!(view, expected);
    assert_eq!(view.stocks[0].stock.0, "600101");
    assert_eq!(
        before,
        serde_json::to_value(actor.game.game().save().unwrap()).unwrap()
    );
}

#[test]
fn company_read_member_request_uses_current_nonzero_membership_and_rejects_stale_generation() {
    use engine::session::protocol::{CompanyReadError, CompanyReadQuery};
    let (mut actor, _) = catalog_actor_fixture();
    let subject = OpaqueSubjectId::new("company-query-member".into()).unwrap();
    let owner = actor
        .game
        .join_market(subject.clone(), true)
        .unwrap()
        .account_id;
    assert_ne!(owner, AccountId(0));
    let before = serde_json::to_value(actor.game.game().save().unwrap()).unwrap();
    let response = actor
        .handle_member_request(
            subject.clone(),
            MemberRequest::CompanyRead {
                generation: 1,
                query: CompanyReadQuery::AccountDividendTaxStatus,
            },
        )
        .unwrap();
    let MemberResponse::CompanyRead(generation, response_owner, result) = response else {
        panic!("响应类型错误")
    };
    assert_eq!((generation, response_owner), (1, owner));
    assert!(matches!(result, Err(CompanyReadError::Corporate(_))));
    assert!(actor
        .handle_member_request(
            subject,
            MemberRequest::CompanyRead {
                generation: 2,
                query: CompanyReadQuery::OwnerRightsOfferings
            }
        )
        .is_err());
    let outsider = OpaqueSubjectId::new("not-joined".into()).unwrap();
    assert!(actor
        .handle_member_request(
            outsider,
            MemberRequest::CompanyRead {
                generation: 1,
                query: CompanyReadQuery::AccountDividendTaxStatus
            }
        )
        .is_err());
    assert_eq!(
        before,
        serde_json::to_value(actor.game.game().save().unwrap()).unwrap()
    );
}

#[cfg(feature = "simulation-diagnostics")]
#[test]
fn diagnostic_ticks_are_consumed_synchronously_without_queue() {
    let (mut actor, mut updates) = catalog_actor_fixture();
    for tick in 1..=2 {
        let deferred = actor.game.advance_core_tick().unwrap();
        actor.queue_tick(deferred).unwrap();
        assert!(actor.consumer.is_none(), "诊断逐拍消费不得建立异步积压队列");
        let ProtocolUpdate::TickBatch(batch) = updates.try_recv().unwrap().update.unwrap() else {
            panic!("诊断必须逐拍交付完整结果")
        };
        assert_eq!(batch.frames.len(), 1);
        assert_eq!(batch.frames[0].tick, tick);
        assert_eq!(batch.frames[0].tick_from, None);
    }
}

#[test]
fn production_actor_steps_and_delivers_tick_batches() {
    // 批 9 A3（compute 注入退役）：原 compute_context_production_actor_
    // does_not_follow_callers_pool 的池隔离断言面（observer 采样/
    // ptr_eq 绑定）随 A1/A2 的注入面消亡整体退役——std-thread 模型
    // 决策/撮合在组/交易部线程，ComputeContext 池采样仅旧管线
    // （=0 逃生门）路径存在。保留的等价断言：actor 构造（无注入）
    // → run_protocol_batch 推进 tick → TickBatch 逐拍交付（原测试
    // 的交付面不变）。
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let game = ProtocolSession::new(setup, 7).unwrap();
    let (_sender, cmd_rx) = mpsc::unbounded_channel();
    let (event_tx, mut receiver) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let mut actor = SessionActor {
        ingress: Arc::new(RwLock::new((1, game.shared_ingress()))),
        members: Arc::new(RwLock::new(game.trading_subject_accounts())),
        controllers: std::collections::BTreeSet::new(),
        archive: None,
        day_end_exports: DayEndExports::new(DEFAULT_DAY_END_EXPORT_BYTES),
        injected_step_failure: None,
        pacing: ServerPacing::new(1, game.tick()),
        game,
        cmd_rx,
        event_tx,
        session_id: "no-compute".into(),
        public_revision: 0,
        timeline_generation: 1,
        pause_preferences: PausePreferences::default(),
        fatal_failure: None,
        deferred_drop_reported: false,
        consumer: None,
    };
    actor.run_protocol_batch(1);
    actor.collect_ticks(true).unwrap();
    actor.run_protocol_batch(1);
    actor.collect_ticks(true).unwrap();
    assert!(actor.fatal_failure.is_none());
    assert_eq!(actor.game.tick(), 2);
    for expected in 1..=2 {
        let update = receiver.try_recv().unwrap().update.unwrap();
        let ProtocolUpdate::TickBatch(batch) = update else {
            panic!("必须提交市场 tick")
        };
        assert_eq!(batch.frames.last().unwrap().tick, expected);
    }
}

#[cfg(not(feature = "simulation-diagnostics"))]
#[test]
fn blocked_production_consumer_does_not_block_next_core_tick() {
    let _guard = CONSUMER_PROBE_LOCK.lock().unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let probe = Arc::new(ConsumerBlockProbe {
        committed: AtomicUsize::new(0),
        entered: entered_tx,
        release: std::sync::Mutex::new(release_rx),
    });
    *CONSUMER_BLOCK_PROBE.lock().unwrap() = Some(Arc::clone(&probe));
    let (mut actor, mut updates) =
        catalog_actor_fixture_with_policy(engine::RuntimeSyncPolicy::DayEnd);
    actor.game.step_frame().unwrap();
    actor.session_id = "consumer-block-probe".into();
    let actor_thread = std::thread::spawn(move || {
        actor.run_protocol_batch(2);
        actor.collect_ticks(true).unwrap();
        actor
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_millis(200);
    while probe.committed.load(Ordering::SeqCst) < 2 && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    let committed_while_blocked = probe.committed.load(Ordering::SeqCst);
    release_tx.send(()).unwrap();
    actor_thread.join().unwrap();
    *CONSUMER_BLOCK_PROBE.lock().unwrap() = None;
    assert_eq!(committed_while_blocked, 2);
    let update = updates.try_recv().unwrap().update.unwrap();
    let ProtocolUpdate::TickBatch(batch) = update else {
        panic!("必须交付市场帧")
    };
    assert_eq!(batch.frames.len(), 1);
    assert_eq!(
        (batch.frames[0].tick_from, batch.frames[0].tick),
        (Some(2), 3)
    );
    batch.validate().unwrap();
}

#[cfg(not(feature = "simulation-diagnostics"))]
#[test]
fn blocked_consumer_coalesces_beyond_eight_ticks() {
    let _guard = CONSUMER_PROBE_LOCK.lock().unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let probe = Arc::new(ConsumerBlockProbe {
        committed: AtomicUsize::new(0),
        entered: entered_tx,
        release: std::sync::Mutex::new(release_rx),
    });
    *CONSUMER_BLOCK_PROBE.lock().unwrap() = Some(Arc::clone(&probe));
    let (mut actor, _) = catalog_actor_fixture_with_policy(engine::RuntimeSyncPolicy::DayEnd);
    actor.game.step_frame().unwrap();
    actor.session_id = "consumer-block-probe".into();
    for _ in 0..HISTORICAL_WINDOW {
        actor.run_protocol_batch(1);
    }
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let pending = actor.consumer.as_ref().unwrap().pending;
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        actor.run_protocol_batch(1);
        let tick = actor.game.tick();
        finished_tx.send(tick).unwrap();
        actor.collect_ticks(true).unwrap();
    });
    let tick = finished_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let committed_while_blocked = probe.committed.load(Ordering::SeqCst);
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    *CONSUMER_BLOCK_PROBE.lock().unwrap() = None;
    assert_eq!(pending, HISTORICAL_WINDOW);
    assert_eq!(committed_while_blocked, HISTORICAL_WINDOW + 1);
    assert_eq!(tick, HISTORICAL_WINDOW as u64 + 2);
}

#[cfg(not(feature = "simulation-diagnostics"))]
#[test]
fn blocked_full_consumer_coalesces_beyond_eight_ticks() {
    let _guard = CONSUMER_PROBE_LOCK.lock().unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let probe = Arc::new(ConsumerBlockProbe {
        committed: AtomicUsize::new(0),
        entered: entered_tx,
        release: std::sync::Mutex::new(release_rx),
    });
    *CONSUMER_BLOCK_PROBE.lock().unwrap() = Some(Arc::clone(&probe));
    let (mut actor, mut updates) = catalog_actor_fixture();
    actor.game.step_frame().unwrap();
    actor.session_id = "consumer-block-probe".into();
    for _ in 0..HISTORICAL_WINDOW {
        actor.run_protocol_batch(1);
    }
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let pending = actor.consumer.as_ref().unwrap().pending;
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        actor.run_protocol_batch(1);
        let tick = actor.game.tick();
        finished_tx.send(tick).unwrap();
        actor.collect_ticks(true).unwrap();
    });
    let before_release = finished_rx.recv_timeout(Duration::from_millis(100));
    let committed_while_blocked = probe.committed.load(Ordering::SeqCst);
    release_tx.send(()).unwrap();
    let tick = before_release.expect("Full 累计区间不得阻塞核心推进");
    worker.join().unwrap();
    *CONSUMER_BLOCK_PROBE.lock().unwrap() = None;
    assert_eq!(pending, HISTORICAL_WINDOW);
    assert_eq!(committed_while_blocked, HISTORICAL_WINDOW + 1);
    let update = updates.try_recv().unwrap().update.unwrap();
    let ProtocolUpdate::TickBatch(batch) = update else {
        panic!("必须交付 Full 累计区间")
    };
    assert_eq!(batch.frames.len(), 1);
    assert_eq!(
        (batch.frames[0].tick_from, batch.frames[0].tick),
        (Some(2), tick)
    );
    assert_eq!(tick, HISTORICAL_WINDOW as u64 + 2);
}

#[cfg(not(feature = "simulation-diagnostics"))]
#[test]
fn fixed_cycle_coalesces_all_ticks_across_blocked_consumer() {
    let _guard = CONSUMER_PROBE_LOCK.lock().unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let probe = Arc::new(ConsumerBlockProbe {
        committed: AtomicUsize::new(0),
        entered: entered_tx,
        release: std::sync::Mutex::new(release_rx),
    });
    *CONSUMER_BLOCK_PROBE.lock().unwrap() = Some(Arc::clone(&probe));
    let (mut actor, mut updates) =
        catalog_actor_fixture_with_policy(engine::RuntimeSyncPolicy::DayEnd);
    actor.game.step_frame().unwrap();
    actor.session_id = "consumer-block-probe".into();
    actor.injected_step_failure = Some((
        HISTORICAL_WINDOW + 2,
        engine::session::StepFatal::InvariantViolation {
            location: "test.fixed_cycle".into(),
            description: "不应触发".into(),
        },
        Arc::new(std::sync::Mutex::new(StepFailureCapture::default())),
    ));
    let worker = std::thread::spawn(move || {
        actor.run_protocol_batch(HISTORICAL_WINDOW + 1);
        actor.collect_ticks(true).unwrap();
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while probe.committed.load(Ordering::SeqCst) < HISTORICAL_WINDOW + 1
        && std::time::Instant::now() < deadline
    {
        std::thread::yield_now();
    }
    let committed = probe.committed.load(Ordering::SeqCst);
    release_tx.send(()).unwrap();
    worker.join().unwrap();
    *CONSUMER_BLOCK_PROBE.lock().unwrap() = None;
    assert_eq!(committed, HISTORICAL_WINDOW + 1);
    let update = updates.try_recv().unwrap().update.unwrap();
    let ProtocolUpdate::TickBatch(batch) = update else {
        panic!("必须发布市场帧")
    };
    assert_eq!(batch.frames.len(), 1);
    assert_eq!(
        (
            batch.frames.first().unwrap().tick_from.unwrap(),
            batch.frames.last().unwrap().tick
        ),
        (2, HISTORICAL_WINDOW as u64 + 2)
    );
    assert!(matches!(
        updates.try_recv(),
        Err(broadcast::error::TryRecvError::Closed)
    ));
}

#[cfg(not(feature = "simulation-diagnostics"))]
#[test]
fn disconnected_consumer_delivers_fixed_prefix_before_fatal() {
    let (mut actor, mut updates) = catalog_actor_fixture();
    let first = actor.game.advance_core_tick().unwrap().consume().unwrap();
    let (input, receiver) = std::sync::mpsc::channel();
    drop(receiver);
    let (sender, output) = std::sync::mpsc::channel();
    assert!(sender.send((1, Ok(first))).is_ok());
    drop(sender);
    actor.consumer = Some(TickConsumer {
        input: Some(input),
        #[cfg(not(feature = "simulation-diagnostics"))]
        slot: None,
        output,
        pending: 1,
        worker: None,
        cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        notify: Arc::new(tokio::sync::Notify::new()),
    });
    actor.run_protocol_batch_inner(1);
    let first = updates.try_recv().unwrap();
    let ProtocolUpdate::TickBatch(batch) = first.update.unwrap() else {
        panic!("必须先交付成功前缀")
    };
    assert_eq!(batch.frames.len(), 1);
    assert_eq!(batch.frames[0].tick, 1);
    let fatal = updates.try_recv().unwrap();
    assert!(fatal.update.is_none());
    assert!(fatal.failure.unwrap().message.contains("已提交"));
    assert_eq!(actor.game.tick(), 2);
    assert!(matches!(
        updates.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
}

#[cfg(not(feature = "simulation-diagnostics"))]
#[tokio::test]
async fn consumer_completion_wakes_actor_before_next_tick_timer() {
    let _guard = CONSUMER_PROBE_LOCK.lock().unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let probe = Arc::new(ConsumerBlockProbe {
        committed: AtomicUsize::new(0),
        entered: entered_tx,
        release: std::sync::Mutex::new(release_rx),
    });
    *CONSUMER_BLOCK_PROBE.lock().unwrap() = Some(probe);
    let (mut actor, mut updates) = catalog_actor_fixture();
    let (commands, receiver) = mpsc::unbounded_channel();
    actor.cmd_rx = receiver;
    actor.session_id = "consumer-block-probe".into();
    actor.pacing.tick_interval = Duration::from_secs(10);
    actor.run_protocol_batch(1);
    entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let task = tokio::spawn(actor.run());
    release_tx.send(()).unwrap();
    let update = tokio::time::timeout(Duration::from_secs(1), updates.recv()).await;
    let (reply, finished) = oneshot::channel();
    commands.send(SessionCommand::Shutdown { reply }).unwrap();
    finished.await.unwrap();
    task.await.unwrap();
    *CONSUMER_BLOCK_PROBE.lock().unwrap() = None;
    let update = update.unwrap().unwrap().update.unwrap();
    let ProtocolUpdate::TickBatch(batch) = update else {
        panic!("必须发布市场帧")
    };
    assert_eq!(batch.frames.len(), 1);
    assert_eq!(batch.frames[0].tick, 1);
}

#[tokio::test]
async fn snapshot_and_baseline_match_consumed_resting_order_delta() {
    let (mut actor, mut updates) = catalog_actor_fixture();
    actor
        .game
        .enqueue_player_intent(
            AccountId(0),
            engine::Intent::PlaceLimit {
                code: engine::StockId(0),
                side: engine::Side::Buy,
                price: engine::LimitPrice::Fixed(engine::Money::from_cents(1000)),
                qty: 100,
            },
        )
        .unwrap();
    actor.run_protocol_batch(1);
    actor.collect_ticks(true).unwrap();
    let ProtocolUpdate::TickBatch(batch) = updates.try_recv().unwrap().update.unwrap() else {
        panic!("必须交付市场帧")
    };
    let delta = batch.runtime_delta.unwrap();
    assert!(!delta.working_orders.upserts.is_empty());
    let account = delta.accounts.get(&AccountId(0)).unwrap();
    assert!(account.reserved_cash > engine::Money::ZERO);

    let baseline = actor.public_baseline_candidate().unwrap();
    assert_eq!(
        baseline.snapshot.accounts.get(&AccountId(0)).unwrap(),
        account
    );
    assert_eq!(
        baseline.snapshot.markets[&engine::StockId(0)].best_bid,
        Some(engine::Money::from_cents(1000))
    );

    let (reply, response) = oneshot::channel();
    actor
        .handle_command(SessionCommand::Snapshot { reply })
        .await;
    let snapshot = response.await.unwrap();
    assert_eq!(snapshot.accounts.get(&AccountId(0)).unwrap(), account);
    assert_eq!(
        snapshot.markets[&engine::StockId(0)].bids,
        baseline.snapshot.markets[&engine::StockId(0)].bids
    );
}

#[tokio::test]
async fn session_manager_defaults_without_compute_injection() {
    // 批 9 A3：compute_workers/with_compute 面退役（A1/A2）——等价
    // 断言：默认构造可用且 new_session 正常登记。
    let manager = SessionManager::with_base_ms(1);
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let id = manager
        .new_session(setup, 7)
        .expect("default manager creates sessions");
    assert!(manager.sessions.contains_key(&id));
}

#[tokio::test]
async fn new_session_capacity_limit_still_guards_registration() {
    // 批 9 A3：Compute 失败变体退役（A1）——保留的容量/登记等价面：
    // Capacity 拒绝时不登记（原断言 active_count==0/sessions 空的
    // 等价语义，用容量满构造不可行（MAX_SESSIONS 大），改验正常
    // 登记计数一致——失败路径的"不登记"由 fetch_update 先于构造
    // 的既有顺序承载）。
    let manager = SessionManager::with_base_ms(1);
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let _ = manager.new_session(setup, 7).expect("session registers");
    assert_eq!(manager.active_session_count(), 1);
    assert_eq!(manager.sessions.len(), 1);
}

#[tokio::test]
async fn shared_ingress_receives_player_without_actor_command_polling() {
    let mut setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    let mut game = ProtocolSession::new(setup.clone(), 7).unwrap();
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
    let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let handles = SessionHandles {
        ingress: Arc::new(RwLock::new((1, game.shared_ingress()))),
        members: Arc::new(RwLock::new(game.trading_subject_accounts())),
        cmd_tx,
        event_tx,
        ticks_per_day: setup.ticks_per_day,
        auction_ticks: setup.auction_ticks,
        closing_auction_ticks: setup.closing_auction_ticks,
        session_token: "shared-ingress".into(),
        startup_setup: setup.clone(),
        startup_seed: 7,
        startup_resumed: false,
    };
    tokio::time::timeout(
        Duration::from_millis(100),
        handles.enqueue(
            1,
            engine::Intent::PlaceLimit {
                code: engine::StockId(0),
                side: engine::Side::Buy,
                price: engine::LimitPrice::Fixed(engine::Money::from_cents(900)),
                qty: 100,
            },
        ),
    )
    .await
    .expect("actor 未消费命令时，Player 仍必须登记到共享 ingress")
    .unwrap();
    assert!(cmd_rx.try_recv().is_err());
    let rejected = handles
        .enqueue_as(
            1,
            AccountId(999),
            Intent::Cancel {
                code: engine::StockId(0),
                id: engine::OrderId(1),
            },
        )
        .await;
    assert!(
        matches!(rejected, Err(SendCommandError::Rejected(message)) if message.contains("999"))
    );
    game.step_frame().unwrap();
    assert_eq!(game.player_working_orders().len(), 1);
}

#[test]
fn producer_failure_contains_real_location_and_recovery_details() {
    let error = engine::session::StepFatal::InvariantViolation {
        location: "ReceiptAggregation::validate".into(),
        description: "receipt chain broke".into(),
    };
    let value = serde_json::to_value(HostFailure::step(&error)).unwrap();
    assert_eq!(value["where"], "ReceiptAggregation::validate");
    assert_eq!(value["context"]["operation"], "step");
    assert_eq!(value["recoverable"], false);
    assert!(value["recoveryActions"]
        .as_array()
        .is_some_and(|actions| !actions.is_empty()));
    assert!(
        value["cause"].is_null(),
        "StepFatal has no source: do not invent a cause"
    );
}

fn assert_fatal_rejection(error: SessionError) {
    // fault1291：宿主致命失败后停机（数值码断言，ADR 0041）。
    assert!(matches!(
        &error,
        SessionError::InvalidSave(fault) if fault.code == 1291
    ));
}

fn assert_failed_cycle_preserves_business_and_ingress_facts(
    saved_before: &serde_json::Value,
    saved_after: engine::SaveSlot,
    successful_steps: usize,
) {
    let mut before_business = saved_before.clone();
    let before_cursors = before_business
        .as_object_mut()
        .unwrap()
        .remove("ingress_receipt_cursors")
        .unwrap();
    let mut after_business = serde_json::to_value(&saved_after).unwrap();
    let after_cursors = after_business
        .as_object_mut()
        .unwrap()
        .remove("ingress_receipt_cursors")
        .unwrap();
    if successful_steps == 0 {
        assert_eq!(
            after_business, before_business,
            "没有成功 tick 时业务事实和未消费 receipt 不变"
        );
    } else {
        assert_eq!(
            saved_after.snapshot.tick,
            saved_before["snapshot"]["tick"].as_u64().unwrap() + successful_steps as u64,
            "后续失败不能撤销已经成功安装的市场前缀"
        );
    }
    let previous: engine::session::IngressReceiptCursors =
        serde_json::from_value(before_cursors.clone()).unwrap();
    for (account, ordinal) in previous.next_account_ordinal {
        assert!(
            saved_after
                .ingress_receipt_cursors
                .next_account_ordinal
                .get(&account)
                .is_some_and(|current| *current >= ordinal),
            "account receipt cursor 不得回退或丢失：{account:?}"
        );
    }
    for (code, ordinal) in previous.next_stock_ordinal {
        assert!(
            saved_after
                .ingress_receipt_cursors
                .next_stock_ordinal
                .get(&code)
                .is_some_and(|current| *current >= ordinal),
            "stock receipt cursor 不得回退或丢失：{code:?}"
        );
    }
    if successful_steps == 0 {
        assert_eq!(
            after_cursors, before_cursors,
            "未执行成功 tick 时不得凭空登记 NPC receipt"
        );
    }
    // T3 批四：pending_npc 载体删除——受理序核对仅覆盖 pending_player
    //（NPC 侧 gap 事实由 cursors 对照承载）。
    for receipt in saved_after.pending_player.iter() {
        assert!(saved_after
            .ingress_receipt_cursors
            .next_account_ordinal
            .get(&receipt.owner)
            .is_some_and(|cursor| receipt.account_ordinal < *cursor));
        let code = match &receipt.intent {
            Intent::PlaceLimit { code, .. }
            | Intent::PlaceMarket { code, .. }
            | Intent::Cancel { code, .. } => code,
        };
        assert!(saved_after
            .ingress_receipt_cursors
            .next_stock_ordinal
            .get(code)
            .is_some_and(|cursor| receipt.stock_ordinal < *cursor));
    }
}

async fn capture(successful_steps: usize) {
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let mut game = ProtocolSession::new(setup, 7).unwrap();
    let retained = game.step_frame().unwrap();
    game.enqueue_player_intent(
        engine::AccountId(0),
        engine::Intent::PlaceLimit {
            code: engine::StockId(0),
            side: engine::Side::Buy,
            price: engine::LimitPrice::Fixed(engine::Money::from_cents(1000)),
            qty: 100,
        },
    )
    .unwrap();
    let before = (game.tick(), game.seq(), game.business_state_hash().unwrap());
    let slot_before = game.game().save().unwrap();
    let saved_before = serde_json::to_value(&slot_before).unwrap();
    let fatal = engine::session::StepFatal::InvariantViolation {
        location: "server.auto_step".into(),
        description: "third step injected failure".into(),
    };
    let (_sender, cmd_rx) = mpsc::unbounded_channel();
    let (event_tx, mut receiver) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let capture = Arc::new(std::sync::Mutex::new(StepFailureCapture::default()));
    let mut actor = SessionActor {
        ingress: Arc::new(RwLock::new((1, game.shared_ingress()))),
        members: Arc::new(RwLock::new(Default::default())),
        controllers: std::collections::BTreeSet::new(),
        archive: None,
        day_end_exports: DayEndExports::new(DEFAULT_DAY_END_EXPORT_BYTES),
        injected_step_failure: Some((successful_steps, fatal.clone(), Arc::clone(&capture))),
        pacing: ServerPacing {
            speed_meter: SpeedMeter::new(game.tick()),
            tick_interval: Duration::from_millis(1),
            base_ms: 1,
            running: true,
            fastest: true,
            requested_speed: RequestedSpeed::Fastest,
        },
        game,
        cmd_rx,
        event_tx,
        session_id: "fatal".into(),

        public_revision: 4,
        timeline_generation: 1,
        pause_preferences: PausePreferences::default(),
        fatal_failure: None,
        deferred_drop_reported: false,
        consumer: None,
    };

    actor.run_protocol_batch(if successful_steps == 0 { 1 } else { 8 });

    let mut replay = engine::session::protocol::ReplayGuard::new(before.0, before.1);
    let mut delivered = Vec::new();
    let mut delivered_frames = Vec::new();
    let failure = loop {
        let message = receiver.try_recv().unwrap();
        if let Some(update) = &message.update {
            let ProtocolUpdate::TickBatch(batch) = update else {
                panic!("失败前缀只能是已提交市场帧")
            };
            delivered.extend(batch.frames.iter().map(|frame| frame.tick));
            delivered_frames.extend(batch.frames.iter().cloned());
            replay.ingest(update).unwrap();
        } else {
            break message;
        }
    };
    assert_eq!(
        delivered,
        (1..=successful_steps as u64)
            .map(|offset| before.0 + offset)
            .collect::<Vec<_>>()
    );
    assert!(failure.update.is_none());
    assert_eq!(failure.failure.as_ref().unwrap().code, "STEP_FATAL");
    assert_eq!(
        failure.failure.as_ref().unwrap().message,
        "引擎不变量校验失败（原始详情已脱敏）"
    );
    let after = (
        actor.game.tick(),
        actor.game.seq(),
        actor.game.business_state_hash().unwrap(),
    );
    let (expected_saved, expected_origin, expected_business, expected_frames) = {
        let capture = capture.lock().unwrap();
        (
            capture.saved.clone().unwrap(),
            capture.origin.unwrap(),
            capture.business.unwrap(),
            capture.frames.clone(),
        )
    };
    assert_eq!(
        serde_json::to_value(actor.game.game().save().unwrap()).unwrap(),
        expected_saved,
        "失败处理不能改写同 Session 失败前已提交的全部事实"
    );
    assert_eq!(actor.game.runtime_mirror_origin(), expected_origin);
    assert_eq!(
        actor.game.business_state_hash().unwrap(),
        expected_business,
        "真实业务指纹必须与同 Session fatal 返回前完全一致"
    );
    assert_eq!(
        serde_json::to_value(&delivered_frames).unwrap(),
        serde_json::to_value(&expected_frames).unwrap(),
        "全部成功前缀事实必须原样交付且恰好一次"
    );
    assert_eq!(after.0, before.0 + successful_steps as u64);
    assert!(after.1 >= before.1);
    if successful_steps == 0 {
        assert_eq!(after, before);
    }
    assert_eq!(actor.public_revision, 4);
    assert_failed_cycle_preserves_business_and_ingress_facts(
        &saved_before,
        actor.game.game().save().unwrap(),
        successful_steps,
    );
    assert_eq!(actor.pacing.speed_meter.started_tick, before.0);
    assert_eq!(actor.pacing.speed_meter.sample_ticks, 0);
    assert!(!actor.pacing.is_running());
    assert!(actor
        .game
        .shared_ingress()
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: engine::StockId(0),
                id: engine::OrderId(1)
            }
        )
        .is_err());
    actor.run_fastest_batch();
    assert!(receiver.try_recv().is_err());

    let (reply, response) = oneshot::channel();
    actor
        .handle_command(SessionCommand::PublicBaseline { reply })
        .await;
    let baseline = response.await.unwrap().unwrap();
    assert_eq!(baseline.failure, Some(failure.failure.clone().unwrap()));
    assert!(
        serde_json::to_value(&baseline)
            .unwrap()
            .get("failure")
            .is_none(),
        "latched failure is transport metadata and must not alter Baseline JSON"
    );

    let (reply, response) = oneshot::channel();
    actor
        .handle_command(SessionCommand::Save {
            generation: actor.timeline_generation,
            candidate: None,
            reply,
        })
        .await;
    assert_fatal_rejection(response.await.unwrap().unwrap_err());

    let (reply, response) = oneshot::channel();
    actor
        .handle_command(SessionCommand::Enqueue {
            player_id: AccountId(0),
            intent: Intent::PlaceLimit {
                code: engine::StockId(0),
                side: engine::Side::Buy,
                price: engine::LimitPrice::Fixed(engine::Money::from_cents(1000)),
                qty: 100,
            },
            reply,
        })
        .await;
    assert_fatal_rejection(response.await.unwrap().unwrap_err());

    let (reply, response) = oneshot::channel();
    actor
        .handle_command(SessionCommand::Restore {
            slot: Box::new(slot_before),
            reply,
        })
        .await;
    assert_fatal_rejection(response.await.unwrap().unwrap_err());

    let (reply, response) = oneshot::channel();
    actor
        .handle_command(SessionCommand::SetSpeed { speed: 2.0, reply })
        .await;
    assert_fatal_rejection(response.await.unwrap().unwrap_err());

    let (reply, response) = oneshot::channel();
    actor
        .handle_command(SessionCommand::SetRunning {
            running: true,
            reply,
        })
        .await;
    assert_fatal_rejection(response.await.unwrap().unwrap_err());

    let (reply, response) = oneshot::channel();
    actor
        .handle_command(SessionCommand::SetPausePreferences {
            generation: actor.timeline_generation,
            preferences: PausePreferences {
                pause_after_close: true,
                pause_before_open: true,
            },
            reply,
        })
        .await;
    assert_fatal_rejection(response.await.unwrap().unwrap_err());

    assert_eq!(
        (
            actor.game.tick(),
            actor.game.seq(),
            actor.game.business_state_hash().unwrap()
        ),
        after
    );
    assert_failed_cycle_preserves_business_and_ingress_facts(
        &saved_before,
        actor.game.game().save().unwrap(),
        successful_steps,
    );
    assert_eq!(actor.pacing.requested_speed, RequestedSpeed::Fastest);
    assert!(!actor.pacing.is_running());
    assert_eq!(actor.pause_preferences, PausePreferences::default());
    let mut verification = actor.game.fork_for_verification().unwrap();
    for _ in after.0..fixture::TICKS_PER_DAY {
        verification.step_frame().unwrap();
    }
    let civil = verification.end_civil_day_update().unwrap();
    assert_eq!(
        civil.refresh.intraday.len(),
        usize::try_from(fixture::TICKS_PER_DAY).unwrap()
    );
    let expected_prefix = std::iter::once(retained)
        .chain(expected_frames.iter().cloned())
        .collect::<Vec<_>>();
    assert_eq!(
        serde_json::to_value(&civil.refresh.intraday[..expected_prefix.len()]).unwrap(),
        serde_json::to_value(expected_prefix).unwrap(),
        "日终 retained 必须保留失败前全部真实帧"
    );
    println!("server failed after {successful_steps} successful steps; committed tick={} seq={}; host_stopped=1; {}", before.0, before.1, serde_json::to_string(&failure).unwrap());
}

#[tokio::test]
async fn fastest_third_step_failure_preserves_committed_prefix_and_stops() {
    capture(2).await;
}

#[tokio::test]
async fn fixed_first_step_failure_preserves_current_state_and_stops() {
    capture(0).await;
}

#[test]
fn step_fatal_maps_to_the_stable_host_code() {
    let fatal = engine::session::StepFatal::InvariantViolation {
        location: "server.step".into(),
        description: "receipt chain broke".into(),
    };
    let expected_message = "引擎不变量校验失败（原始详情已脱敏）";
    let failure = HostFailure::step(&fatal);
    assert_eq!(failure.code, "STEP_FATAL");
    assert_eq!(failure.message, expected_message);
    let value = serde_json::to_value(&failure).unwrap();
    assert_eq!(value["code"], "STEP_FATAL");
    assert_eq!(value["message"], expected_message);
    assert_eq!(value["where"], "server.step");
    assert_eq!(value["cause"], serde_json::Value::Null);
    assert_eq!(value["context"], serde_json::json!({ "operation": "step" }));
    assert_eq!(value["recoverable"], false);
    assert_eq!(
        value["recoveryActions"],
        serde_json::json!([
            "停止当前会话；重新打开上一份有效日终存档或新局",
            "复制脱敏错误详情反馈"
        ])
    );
}

#[test]
fn civil_settlement_error_maps_to_the_cross_host_failure_code() {
    let failure = HostFailure::civil(&SessionError::InvalidSave(
        engine::error::ValidationFault::bare(3012),
    ));
    assert_eq!(failure.code, "CIVIL_DAY_SETTLEMENT_FAILED");
    assert_eq!(
        failure.message,
        "日终存档或协议状态校验失败（原始详情已脱敏）"
    );
}

#[tokio::test]
async fn stale_preferences_leave_actor_settings_unchanged() {
    let manager = SessionManager::default();
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-05").unwrap());
    let mut completed = ProtocolSession::new(setup.clone(), 1).unwrap();
    completed.end_civil_day_update().unwrap();
    let slot = completed.save().unwrap();
    let id = manager.new_session(setup, 1).unwrap();
    let handles = manager.lookup(&id).unwrap();
    let old_ingress = handles.ingress.read().unwrap().1.clone();
    handles.restore(slot).await.unwrap();
    let intent = Intent::PlaceLimit {
        code: engine::StockId(0),
        side: engine::Side::Buy,
        price: engine::LimitPrice::Fixed(engine::Money::from_cents(900)),
        qty: 100,
    };
    assert!(old_ingress
        .enqueue_player_intent(AccountId(0), intent.clone())
        .is_err());
    assert!(handles.enqueue(1, intent.clone()).await.is_err());
    handles.enqueue(2, intent.clone()).await.unwrap();

    let result = handles
        .set_pause_preferences(
            1,
            PausePreferences {
                pause_after_close: true,
                pause_before_open: true,
            },
        )
        .await;

    assert!(matches!(result, Err(SendCommandError::Rejected(_))));
    handles
        .set_pause_preferences(2, PausePreferences::default())
        .await
        .unwrap();
    manager.remove(&id).unwrap().shutdown().await.unwrap();
    assert!(matches!(
        handles.enqueue(2, intent).await,
        Err(SendCommandError::ActorGone)
    ));
}

#[tokio::test]
async fn authority_reads_reject_stale_generations() {
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let game = ProtocolSession::new(setup, 1).unwrap();
    let (_sender, cmd_rx) = mpsc::unbounded_channel();
    let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let mut actor = SessionActor {
        ingress: Arc::new(RwLock::new((7, game.shared_ingress()))),
        members: Arc::new(RwLock::new(game.trading_subject_accounts())),
        controllers: std::collections::BTreeSet::new(),
        archive: None,
        day_end_exports: DayEndExports::new(DEFAULT_DAY_END_EXPORT_BYTES),
        #[cfg(test)]
        injected_step_failure: None,
        pacing: ServerPacing {
            speed_meter: SpeedMeter::new(game.tick()),
            tick_interval: Duration::from_millis(1),
            base_ms: 1,
            running: false,
            fastest: false,
            requested_speed: RequestedSpeed::Fixed { multiplier: 1.0 },
        },
        game,
        cmd_rx,
        event_tx,
        session_id: "authority-read".into(),

        public_revision: 0,
        timeline_generation: 7,
        pause_preferences: PausePreferences::default(),
        fatal_failure: None,
        deferred_drop_reported: false,
        consumer: None,
    };

    let (reply, response) = oneshot::channel();
    actor
        .handle_command(SessionCommand::PlayerWorkingOrders {
            generation: 6,
            reply,
        })
        .await;
    assert!(matches!(
        response.await.unwrap(),
        Err(SendCommandError::Rejected(message)) if message.contains("STALE_SESSION_GENERATION")
    ));

    let (reply, response) = oneshot::channel();
    actor
        .handle_command(SessionCommand::PlayerWorkingOrders {
            generation: 7,
            reply,
        })
        .await;
    let (actual_generation, orders) = response.await.unwrap().unwrap();
    assert_eq!(actual_generation, 7);
    assert_eq!(orders, serde_json::json!([]));

    let (reply, response) = oneshot::channel();
    actor
        .handle_command(SessionCommand::NpcDecisionDiagnostics {
            generation: 6,
            account: AccountId(0),
            reply,
        })
        .await;
    assert!(matches!(
        response.await.unwrap(),
        Err(SendCommandError::Rejected(message)) if message.contains("STALE_SESSION_GENERATION")
    ));
}

// 公开 save API 只提供日终存档；读取暂停 actor 的权威 checkpoint，
// 保留请求尚未消费时的队列断言。
#[tokio::test]
async fn command_burst_keeps_submission_order_after_callers_stop_waiting() {
    use futures_util::FutureExt;

    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-01").unwrap());
    let game = ProtocolSession::new(setup.clone(), 42).unwrap();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let handles = SessionHandles {
        ingress: Arc::new(RwLock::new((1, game.shared_ingress()))),
        members: Arc::new(RwLock::new(game.trading_subject_accounts())),
        cmd_tx,
        event_tx: event_tx.clone(),
        ticks_per_day: setup.ticks_per_day,
        auction_ticks: setup.auction_ticks,
        closing_auction_ticks: setup.closing_auction_ticks,
        session_token: "command-burst".into(),
        startup_setup: setup.clone(),
        startup_seed: 42,
        startup_resumed: false,
    };
    let mut actor = SessionActor {
        ingress: Arc::clone(&handles.ingress),
        members: Arc::new(RwLock::new(Default::default())),
        controllers: std::collections::BTreeSet::new(),
        archive: None,
        day_end_exports: DayEndExports::new(DEFAULT_DAY_END_EXPORT_BYTES),
        injected_step_failure: None,
        pacing: ServerPacing {
            speed_meter: SpeedMeter::new(game.tick()),
            tick_interval: Duration::from_millis(10_000),
            base_ms: 10_000,
            running: false,
            fastest: false,
            requested_speed: RequestedSpeed::Fixed { multiplier: 1.0 },
        },
        game,
        cmd_rx,
        event_tx,
        session_id: "command-burst".into(),

        public_revision: 0,
        timeline_generation: 1,
        pause_preferences: PausePreferences::default(),
        fatal_failure: None,
        deferred_drop_reported: false,
        consumer: None,
    };
    let expected: Vec<_> = (1..=64)
        .map(|lot_count| {
            (
                AccountId(0),
                Intent::PlaceLimit {
                    code: engine::StockId(0),
                    side: engine::Side::Buy,
                    price: engine::LimitPrice::Fixed(engine::Money::from_cents(1000)),
                    qty: lot_count * 100,
                },
            )
        })
        .collect();
    // 真实 handle 在 actor 忙碌时直接登记，不依赖调用者继续等待命令回执。
    for (_, intent) in &expected {
        handles
            .enqueue(1, intent.clone())
            .now_or_never()
            .unwrap()
            .unwrap();
    }
    assert!(matches!(
        actor.cmd_rx.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    let saved = actor.game.game().save().unwrap();
    assert_eq!(saved.snapshot.tick, 0, "暂停的市场不能提前消费玩家请求");
    assert_eq!(saved.pending_player.len(), expected.len());
    assert!(saved
        .pending_player
        .windows(2)
        .all(|pair| pair[0].account_ordinal < pair[1].account_ordinal
            && pair[0].stock_ordinal < pair[1].stock_ordinal));
    assert_eq!(
        serde_json::to_value(
            saved
                .pending_player
                .into_iter()
                .map(|received| (received.owner, received.intent))
                .collect::<Vec<_>>()
        )
        .unwrap(),
        serde_json::to_value(expected).unwrap(),
        "请求须按投递顺序各保留一次"
    );
}
#[test]
fn producer_failure_preserves_real_source_chain_without_private_state() {
    let error =
        engine::SessionError::Closing(engine::accounting::closing::ClosingError::Accounting(
            engine::accounting::AccountingError::AmountOverflow {
                op: "add",
                detail: "token=private-secret account cash=987654321".into(),
            },
        ));
    let value = serde_json::to_value(HostFailure::civil(&error)).unwrap();
    assert_eq!(value["code"], "CIVIL_DAY_SETTLEMENT_FAILED");
    assert!(value["cause"].is_object());
    assert!(value["cause"]["cause"].is_object());
    assert!(value["cause"]["cause"]["cause"].is_null());
    let encoded = value.to_string();
    assert!(!encoded.contains("private-secret"));
    assert!(!encoded.contains("987654321"));
    assert!(!encoded.contains("recovery_actions"));
}
#[test]
fn producer_failure_redacts_real_bare_amount_overflow_operands() {
    let private_operand = 2_718_281_828_459_045_235_i128;
    let unknown = std::io::Error::other(private_operand.to_string());
    assert_eq!(
        failure_description(&unknown),
        ("ERROR_DETAILS_REDACTED", "原始错误类型未识别，详情未公开")
    );
    assert!(failure_cause(&unknown).is_none());
    let overflow = engine::accounting::AccountingAmount::MAX
        .add(engine::accounting::AccountingAmount::from_cents(
            private_operand,
        ))
        .unwrap_err();
    let engine::accounting::AccountingError::AmountOverflow { ref detail, .. } = overflow else {
        panic!("fixture must produce a real amount overflow")
    };
    assert_eq!(detail, &format!("{} + {private_operand}", i128::MAX));
    let error = engine::SessionError::Closing(
        engine::accounting::closing::ClosingError::Accounting(overflow),
    );
    let value = serde_json::to_value(HostFailure::civil(&error)).unwrap();
    println!("A08_BARE_AMOUNT_FAILURE={value}");
    let encoded = value.to_string();
    assert!(
        !encoded.contains(&private_operand.to_string()),
        "bare private operand leaked"
    );
    assert!(
        !encoded.contains(&i128::MAX.to_string()),
        "bare private operand leaked"
    );
    assert_eq!(value["cause"]["code"], "CLOSING_ACCOUNTING_FAILED");
    assert_eq!(
        value["cause"]["cause"]["code"],
        "ACCOUNTING_AMOUNT_OVERFLOW"
    );
    assert_eq!(
        value["cause"]["cause"]["message"],
        "公司会计金额运算溢出（操作数已脱敏）"
    );
    assert!(value["cause"]["cause"]["cause"].is_null());
}

/// Host 不再构造可复活 authority 的批 checkpoint；零拍批须保持真实来源与受理事实。
#[test]
fn zero_step_batch_keeps_progress_origin_and_ingress_facts() {
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let game = ProtocolSession::new(setup, 7).unwrap();
    let (_sender, cmd_rx) = mpsc::unbounded_channel();
    let (event_tx, _receiver) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let mut actor = SessionActor {
        ingress: Arc::new(RwLock::new((1, game.shared_ingress()))),
        members: Arc::new(RwLock::new(game.trading_subject_accounts())),
        controllers: std::collections::BTreeSet::new(),
        archive: None,
        day_end_exports: DayEndExports::new(DEFAULT_DAY_END_EXPORT_BYTES),
        injected_step_failure: None,
        pacing: ServerPacing {
            speed_meter: SpeedMeter::new(game.tick()),
            tick_interval: Duration::from_millis(1),
            base_ms: 1,
            running: true,
            fastest: true,
            requested_speed: RequestedSpeed::Fastest,
        },
        game,
        cmd_rx,
        event_tx,
        session_id: "graveyard".into(),

        public_revision: 0,
        timeline_generation: 1,
        pause_preferences: PausePreferences::default(),
        fatal_failure: None,
        deferred_drop_reported: false,
        consumer: None,
    };
    let before = serde_json::to_value(actor.game.game().save().unwrap()).unwrap();
    let origin = actor.game.runtime_mirror_origin();
    actor.run_protocol_batch(0);
    assert_eq!(
        serde_json::to_value(actor.game.game().save().unwrap()).unwrap(),
        before
    );
    assert_eq!(actor.game.runtime_mirror_origin(), origin);
    assert!(actor.game.require_healthy().is_ok());
    assert!(actor.fatal_failure.is_none());
}

fn catalog_actor_fixture() -> (SessionActor, broadcast::Receiver<EngineUpdate>) {
    catalog_actor_fixture_with_policy(engine::RuntimeSyncPolicy::default())
}

fn catalog_actor_fixture_with_policy(
    policy: engine::RuntimeSyncPolicy,
) -> (SessionActor, broadcast::Receiver<EngineUpdate>) {
    let mut setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    setup.runtime_sync_policy = policy;
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.history_len = 1;
    setup.stocks[0].float_shares = 0;
    let game = ProtocolSession::new(setup, 7).unwrap();
    let (_sender, cmd_rx) = mpsc::unbounded_channel();
    let (event_tx, receiver) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    (
        SessionActor {
            ingress: Arc::new(RwLock::new((1, game.shared_ingress()))),
            members: Arc::new(RwLock::new(game.trading_subject_accounts())),
            controllers: std::collections::BTreeSet::new(),
            archive: None,
            day_end_exports: DayEndExports::new(DEFAULT_DAY_END_EXPORT_BYTES),
            injected_step_failure: None,
            pacing: ServerPacing::new(1, game.tick()),
            game,
            cmd_rx,
            event_tx,
            session_id: "catalog-fixture".into(),
            public_revision: 0,
            timeline_generation: 1,
            pause_preferences: PausePreferences::default(),
            fatal_failure: None,
            deferred_drop_reported: false,
            consumer: None,
        },
        receiver,
    )
}

#[test]
fn public_baseline_catalogs_match_the_same_actor_session() {
    let (mut actor, _) = catalog_actor_fixture();
    let expected_stocks = actor.game.stock_catalog_view();
    let expected_companies = actor.game.company_catalog_view().unwrap();
    let baseline = actor.public_baseline_candidate().unwrap();
    assert_eq!(baseline.stock_catalog, expected_stocks);
    assert_eq!(baseline.company_catalog, expected_companies);
    assert_eq!(
        baseline.stock_catalog.len(),
        baseline.snapshot.markets.len()
    );
    assert_eq!(baseline.stock_catalog[0].code, "600101");
}

#[test]
fn catalog_failure_has_no_baseline_and_stops_actual_actor_ingress() {
    let (mut actor, mut receiver) = catalog_actor_fixture();
    let ingress = actor.game.shared_ingress();
    let error = engine::session::CompanyCatalogViewError::UnknownListedStock {
        company: engine::company::CompanyId(7),
    };
    assert_eq!(
        actor
            .finish_public_baseline(Err(error.clone()))
            .unwrap_err(),
        error
    );
    let failure = actor.fatal_failure.as_ref().unwrap();
    assert_eq!(failure.code, "CATALOG_BASELINE_FAILED");
    assert_eq!(
        failure.context.fault,
        Some(engine::ValidationFault::new(3019, vec![7]))
    );
    assert!(actor.fatal_rejection().is_some());
    assert!(ingress
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: engine::StockId(0),
                id: engine::OrderId(1)
            }
        )
        .is_err());
    let frame = receiver.try_recv().unwrap();
    assert!(frame.update.is_none());
    assert_eq!(frame.failure.unwrap().code, "CATALOG_BASELINE_FAILED");
}
