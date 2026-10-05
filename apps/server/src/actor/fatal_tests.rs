use super::*;

struct ScopedIngressGate(engine::SharedSessionIngress);

impl Drop for ScopedIngressGate {
    fn drop(&mut self) {
        self.0
            .verification_release()
            .expect("验证退出必须释放真实NPC gate");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn authenticated_http_scoped_ingress_registers_while_real_actor_npc_is_busy() {
    use tower::ServiceExt;

    let manager = SessionManager::with_base_ms(1);
    let identities = crate::identity::IdentityService::new(manager.database());
    let creator = identities.guest().unwrap();
    let trader = identities.guest().unwrap();
    let subject = OpaqueSubjectId::new(trader.subject.subject_id.clone()).unwrap();
    let mut setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.strategy_params.retail.arrival_rate = 1.0;
    setup.strategy_params.retail.chase_prob = 0.0;
    setup.strategy_params.inst.order_size = 100;
    setup.strategy_params.hot.lookback = 2;
    setup.strategy_params.hot.order_size = 100;
    setup.stocks[0].code = engine::StockCode("600888".into());
    setup.stocks[0].float_shares = 0;
    setup.ticks_per_day = 4;
    setup.history_len = 10;
    let mut prepared = ProtocolSession::new(setup.clone(), 1).unwrap();
    prepared
        .bind_market_creator(OpaqueSubjectId::new(creator.subject.subject_id).unwrap())
        .unwrap();
    let account = prepared
        .join_market(subject.clone(), false)
        .unwrap()
        .account_id;
    assert_ne!(account, AccountId(0));
    let mut checkpoint = prepared.game().save().unwrap();
    let attention = checkpoint.npc_attention.get_mut(&AccountId(1)).unwrap();
    attention.next_attention_candidate_tick = 1;
    attention.rng_state = 3;
    let prepared = ProtocolSession::restore_verification_checkpoint(&checkpoint).unwrap();
    let source = prepared.shared_ingress();
    source
        .verification_arm("npc_decision", 1, Some(AccountId(1)))
        .unwrap();
    let _gate = ScopedIngressGate(source.clone());
    manager.active_count.fetch_add(1, Ordering::AcqRel);
    let session_id = manager
        .register_game(prepared, setup, 1, "current", false, None)
        .unwrap();
    let handles = manager.lookup(&session_id).unwrap();
    handles.set_running(true).await.unwrap();
    let gate_deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !source.verification_snapshot().unwrap().entered {
        assert!(
            std::time::Instant::now() < gate_deadline,
            "真实Actor production NPC worker必须进入gate"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let intent = Intent::PlaceLimit {
        code: engine::StockCode("600888".into()),
        side: engine::Side::Buy,
        price: engine::LimitPrice::Fixed(engine::Money::from_cents(1000)),
        qty: 100,
    };
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/api/intent")
        .header("authorization", format!("Bearer {}", trader.token))
        .header("content-type", "application/json")
        .body(axum::body::Body::from(
            serde_json::json!({ "session_id": session_id, "generation": "1", "intent": intent })
                .to_string(),
        ))
        .unwrap();
    let application = crate::app_router_with_manager(manager.clone());
    let (request_reply, request_result) = std::sync::mpsc::channel();
    let request_worker = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = runtime.block_on(async {
            tokio::time::timeout(Duration::from_millis(250), application.oneshot(request)).await
        });
        request_reply.send(result).unwrap();
    });
    let response = request_result
        .recv_timeout(Duration::from_secs(1))
        .expect("独立HTTP runtime必须在短deadline内返回")
        .expect("认证交易请求不得等待busy Actor消费命令")
        .unwrap();
    request_worker.join().unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let trace = source.verification_snapshot().unwrap();
    assert!(trace.entered && !trace.released);
    assert_eq!(trace.players.len(), 1);
    assert_eq!(trace.players[0].owner, account);
    assert!(trace.npc_receipts.is_empty());
    source.verification_release().unwrap();
    handles.set_running(false).await.unwrap();
    let trace = source.verification_snapshot().unwrap();
    assert!(trace
        .npc_receipts
        .iter()
        .any(|receipt| receipt.receipt.owner == AccountId(1)));
    assert!(trace
        .npc_receipts
        .iter()
        .all(|receipt| trace.players[0].stock_ordinal < receipt.receipt.stock_ordinal));
    assert_eq!(
        source.receipt_cursors().unwrap().next_account_ordinal[&account],
        1
    );
    assert!(handles
        .enqueue_for(0, subject.clone(), intent.clone())
        .await
        .is_err());
    assert!(handles
        .enqueue_for(
            1,
            OpaqueSubjectId::new("not-admitted".into()).unwrap(),
            intent.clone()
        )
        .await
        .is_err());
    source.close().unwrap();
    assert!(handles.enqueue_for(1, subject, intent).await.is_err());
    manager
        .remove(&session_id)
        .unwrap()
        .shutdown()
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scoped_subject_cache_switches_with_generation_and_closes_retired_source() {
    let mut setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    let old_subject = OpaqueSubjectId::new("old-trader".into()).unwrap();
    let new_subject = OpaqueSubjectId::new("new-trader".into()).unwrap();
    let mut game = ProtocolSession::new(setup.clone(), 1).unwrap();
    game.bind_market_creator(old_subject.clone()).unwrap();
    let old_source = game.shared_ingress();
    let ingress = Arc::new(RwLock::new((
        1,
        old_source.clone(),
        game.trading_subject_accounts(),
    )));
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (event_tx, _receiver) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let handles = SessionHandles {
        cmd_tx,
        ingress: Arc::clone(&ingress),
        event_tx: event_tx.clone(),
        ticks_per_day: setup.ticks_per_day,
        auction_ticks: setup.auction_ticks,
        closing_auction_ticks: setup.closing_auction_ticks,
        session_token: "trusted-test".into(),
        startup_setup: setup.clone(),
        startup_seed: 1,
        startup_resumed: false,
    };
    let actor = SessionActor {
        controllers: std::collections::BTreeSet::from([old_subject.clone()]),
        archive: None,
        ingress: Arc::clone(&ingress),
        injected_step_failure: None,
        pacing: ServerPacing::new(1000, game.tick()),
        game,
        cmd_rx,
        event_tx,
        session_id: "scoped-cache-race".into(),
        fastest_budget: Arc::new(Semaphore::new(1)),
        public_revision: 0,
        timeline_generation: 1,
        pause_preferences: PausePreferences::default(),
        fatal_failure: None,
    };
    let mut replacement = ProtocolSession::new(setup, 2).unwrap();
    replacement
        .bind_market_creator(new_subject.clone())
        .unwrap();
    let new_source = replacement.shared_ingress();
    let intent = Intent::PlaceLimit {
        code: engine::StockCode("600101".into()),
        side: engine::Side::Buy,
        price: engine::LimitPrice::Fixed(engine::Money::from_cents(1000)),
        qty: 100,
    };
    let prior_generation = ingress.read().unwrap();
    let (started, receive_started) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut actor = actor;
        started.send(()).unwrap();
        actor.replace_timeline(replacement, None).unwrap();
        actor
    });
    receive_started
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    assert_eq!(prior_generation.0, 1);
    let account = prior_generation.2[&old_subject];
    prior_generation
        .1
        .enqueue_player_intent(account, intent.clone())
        .unwrap();
    drop(prior_generation);
    let actor = worker.join().unwrap();
    assert_eq!(actor.timeline_generation, 2);
    assert!(actor.require_market_control(&old_subject).is_ok());
    assert_eq!(
        old_source.receipt_cursors().unwrap().next_account_ordinal[&account],
        1
    );
    assert!(handles
        .enqueue_for(1, old_subject.clone(), intent.clone())
        .await
        .is_err());
    assert!(handles
        .enqueue_for(2, old_subject, intent.clone())
        .await
        .is_err());
    assert!(new_source.recorded_player_inputs().unwrap().is_empty());
    assert!(old_source
        .enqueue_player_intent(account, intent.clone())
        .is_err());
    handles.enqueue_for(2, new_subject, intent).await.unwrap();
    assert_eq!(
        new_source.receipt_cursors().unwrap().next_account_ordinal[&AccountId(0)],
        1
    );
}

#[tokio::test]
async fn loading_economic_members_keeps_current_control_without_granting_archive_members() {
    let current_subject = OpaqueSubjectId::new("current-controller".into()).unwrap();
    let archived_subject = OpaqueSubjectId::new("archive-member".into()).unwrap();
    let mut setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-05").unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    let mut archived = ProtocolSession::new(setup.clone(), 9).unwrap();
    archived
        .bind_market_creator(archived_subject.clone())
        .unwrap();
    archived.end_civil_day_update().unwrap();
    let saved = archived.save().unwrap();
    assert!(
        !serde_json::to_value(&saved).unwrap()["market_memberships"]["members"]["archive-member"]
            .as_object()
            .unwrap()
            .contains_key("capabilities")
    );
    let mut game = ProtocolSession::new(setup.clone(), 8).unwrap();
    game.bind_market_creator(current_subject.clone()).unwrap();
    let (_sender, cmd_rx) = mpsc::unbounded_channel();
    let (event_tx, _receiver) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let mut actor = SessionActor {
        controllers: std::collections::BTreeSet::from([current_subject.clone()]),
        archive: None,
        ingress: Arc::new(RwLock::new((
            1,
            game.shared_ingress(),
            game.trading_subject_accounts(),
        ))),
        injected_step_failure: None,
        pacing: ServerPacing::new(1000, game.tick()),
        game,
        cmd_rx,
        event_tx,
        session_id: "control-independent".into(),
        fastest_budget: Arc::new(Semaphore::new(1)),
        public_revision: 0,
        timeline_generation: 1,
        pause_preferences: PausePreferences::default(),
        fatal_failure: None,
    };
    let (reply, receive) = oneshot::channel();
    actor
        .handle_command(SessionCommand::Restore {
            archive_slot_id: None,
            subject: Some(current_subject.clone()),
            generation: Some(1),
            slot: Box::new(saved),
            reply,
        })
        .await;
    assert!(receive.await.unwrap().unwrap().accounts.is_empty());
    assert!(actor.require_market_control(&current_subject).is_ok());
    assert!(actor.require_market_control(&archived_subject).is_err());
    let MemberResponse::Context(context) = actor
        .handle_member_request(current_subject.clone(), MemberRequest::Context)
        .unwrap()
    else {
        panic!("应返回当前市场上下文")
    };
    assert!(context.can_control && context.needs_rejoin);
    assert!(context.member.is_none());
    assert_eq!(context.seed, "9");
    assert!(actor
        .handle_member_request(current_subject.clone(), MemberRequest::Snapshot)
        .is_err());
    actor
        .handle_member_request(
            current_subject.clone(),
            MemberRequest::SetRunning {
                generation: 2,
                running: false,
            },
        )
        .unwrap();
    actor
        .handle_member_request(
            current_subject.clone(),
            MemberRequest::AdmissionCash {
                generation: 2,
                cash: engine::Money::from_cents(12345),
            },
        )
        .unwrap();
    assert!(actor
        .handle_member_request(
            current_subject.clone(),
            MemberRequest::Join {
                generation: Some(1),
                confirmed_rejoin: true
            }
        )
        .is_err());
    assert!(actor
        .handle_member_request(
            current_subject.clone(),
            MemberRequest::Join {
                generation: Some(2),
                confirmed_rejoin: false
            }
        )
        .is_err());
    let MemberResponse::Membership(member) = actor
        .handle_member_request(
            current_subject.clone(),
            MemberRequest::Join {
                generation: Some(2),
                confirmed_rejoin: true,
            },
        )
        .unwrap()
    else {
        panic!("应返回重新加入的成员")
    };
    assert_eq!(
        actor.game.account(member.account_id).unwrap().cash(),
        engine::Money::from_cents(12345)
    );
    assert!(actor
        .game
        .account(member.account_id)
        .unwrap()
        .positions()
        .is_empty());
    assert_eq!(
        actor
            .game
            .join_market(current_subject.clone(), true)
            .unwrap(),
        member
    );
    assert!(actor
        .handle_member_request(
            archived_subject,
            MemberRequest::Reset {
                generation: 2,
                setup: Box::new(setup.clone()),
                seed: 10
            }
        )
        .is_err());
    actor
        .handle_member_request(
            current_subject.clone(),
            MemberRequest::Reset {
                generation: 2,
                setup: Box::new(setup),
                seed: 10,
            },
        )
        .unwrap();
    assert_eq!(actor.timeline_generation, 3);
    assert!(actor.require_market_control(&current_subject).is_ok());
}

#[test]
fn civil_batch_preparation_defers_ingress_calendar_until_success() {
    let mut setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-01").unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.ticks_per_day = 2;
    let mut game = ProtocolSession::new(setup, 7).unwrap();
    let source = game.shared_ingress();
    let intent = engine::Intent::Cancel {
        code: engine::StockCode("600101".into()),
        id: engine::OrderId(1),
    };
    assert!(source
        .enqueue_player_intent(AccountId(0), intent.clone())
        .is_err());
    let before = (game.tick(), game.seq(), game.business_state_hash().unwrap());
    let result: Result<(), SessionError> = game.with_publication_transaction(|candidate| {
        let updates = SessionActor::prepare_civil_updates(candidate, &PausePreferences::default())?;
        assert_eq!(updates.len(), 1);
        assert_eq!(candidate.civil_date().to_iso(), "2030-01-02");
        assert!(source
            .enqueue_player_intent(AccountId(0), intent.clone())
            .is_err());
        Err(SessionError::InvalidSave("后续批次准备失败".into()))
    });
    assert!(
        matches!(result, Err(SessionError::InvalidSave(message)) if message == "后续批次准备失败")
    );
    assert_eq!(game.civil_date().to_iso(), "2030-01-01");
    assert_eq!(
        (game.tick(), game.seq(), game.business_state_hash().unwrap()),
        before
    );
    assert!(source
        .enqueue_player_intent(AccountId(0), intent.clone())
        .is_err());
    assert!(game.save().is_err());

    let (_sender, cmd_rx) = mpsc::unbounded_channel();
    let (event_tx, mut receiver) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let mut actor = SessionActor {
        controllers: std::collections::BTreeSet::new(),
        archive: None,
        ingress: Arc::new(RwLock::new((
            1,
            source.clone(),
            game.trading_subject_accounts(),
        ))),
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
        session_id: "calendar-publication".into(),
        fastest_budget: Arc::new(Semaphore::new(1)),
        public_revision: 4,
        timeline_generation: 1,
        pause_preferences: PausePreferences::default(),
        fatal_failure: None,
    };
    actor.run_protocol_batch(1);
    assert_eq!(actor.game.civil_date().to_iso(), "2030-01-02");
    assert_eq!(actor.game.tick(), 0);
    assert_eq!(actor.public_revision, 5);
    assert!(actor.fatal_failure.is_none());
    assert!(matches!(
        receiver.try_recv().unwrap().update,
        Some(ProtocolUpdate::CivilUpdate(_))
    ));
    assert!(receiver.try_recv().is_err());
    source.enqueue_player_intent(AccountId(0), intent).unwrap();
    assert_eq!(actor.game.game().save().unwrap().pending_player.len(), 1);
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
        ingress: Arc::new(RwLock::new((
            1,
            game.shared_ingress(),
            game.trading_subject_accounts(),
        ))),
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
                code: engine::StockCode("600101".into()),
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
                code: engine::StockCode("600101".into()),
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

#[path = "../../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

fn assert_fatal_rejection(error: SessionError) {
    let message = error.to_string();
    assert!(
        message.contains("session stopped after STEP_FATAL"),
        "{message}"
    );
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
    assert_eq!(
        after_business, before_business,
        "失败 cycle 必须完整回滚业务状态与全部未消费 receipt"
    );
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
    } else {
        assert_ne!(
            after_cursors, before_cursors,
            "失败 cycle 的已接收 NPC 事实必须保留真实 ordinal gap"
        );
    }
    for receipt in saved_after.pending_player.iter().chain(
        saved_after
            .pending_npc
            .as_ref()
            .into_iter()
            .flat_map(|batch| batch.intents.iter()),
    ) {
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
            code: engine::StockCode("600101".into()),
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
    let mut actor = SessionActor {
        controllers: std::collections::BTreeSet::new(),
        archive: None,
        ingress: Arc::new(RwLock::new((
            1,
            game.shared_ingress(),
            game.trading_subject_accounts(),
        ))),
        injected_step_failure: Some((successful_steps, fatal.clone())),
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
        fastest_budget: Arc::new(Semaphore::new(1)),
        public_revision: 4,
        timeline_generation: 1,
        pause_preferences: PausePreferences::default(),
        fatal_failure: None,
    };

    actor.run_protocol_batch(if successful_steps == 0 { 1 } else { 8 });

    let failure = receiver.try_recv().unwrap();
    assert!(failure.update.is_none());
    assert_eq!(failure.failure.as_ref().unwrap().code, "STEP_FATAL");
    assert_eq!(
        failure.failure.as_ref().unwrap().message,
        "引擎不变量校验失败（原始详情已脱敏）"
    );
    assert_eq!(
        (
            actor.game.tick(),
            actor.game.seq(),
            actor.game.business_state_hash().unwrap()
        ),
        before
    );
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
                code: engine::StockCode("600101".into()),
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
    let baseline = response.await.unwrap();
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
                code: engine::StockCode("600101".into()),
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
            archive_slot_id: None,
            subject: None,
            generation: None,
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
        before
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
    for _ in before.0..fixture::TICKS_PER_DAY {
        verification.step_frame().unwrap();
    }
    let civil = verification.end_civil_day_update().unwrap();
    assert_eq!(
        civil.refresh.intraday.len(),
        usize::try_from(fixture::TICKS_PER_DAY).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&civil.refresh.intraday[0]).unwrap(),
        serde_json::to_value(retained).unwrap()
    );
    println!("server failed after {successful_steps} successful steps; restored tick={} seq={}; healthy=0; {}", before.0, before.1, serde_json::to_string(&failure).unwrap());
}

#[tokio::test]
async fn fastest_third_step_failure_restores_the_entire_cycle() {
    capture(2).await;
}

#[tokio::test]
async fn fixed_first_step_failure_restores_the_entire_cycle() {
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
        "civil boundary is inconsistent".into(),
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
        code: engine::StockCode("600101".into()),
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
        controllers: std::collections::BTreeSet::new(),
        archive: None,
        ingress: Arc::new(RwLock::new((
            7,
            game.shared_ingress(),
            game.trading_subject_accounts(),
        ))),
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
        fastest_budget: Arc::new(Semaphore::new(1)),
        public_revision: 0,
        timeline_generation: 7,
        pause_preferences: PausePreferences::default(),
        fatal_failure: None,
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

    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let game = ProtocolSession::new(setup.clone(), 42).unwrap();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let handles = SessionHandles {
        ingress: Arc::new(RwLock::new((
            1,
            game.shared_ingress(),
            game.trading_subject_accounts(),
        ))),
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
        controllers: std::collections::BTreeSet::new(),
        archive: None,
        ingress: Arc::clone(&handles.ingress),
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
        fastest_budget: Arc::new(Semaphore::new(1)),
        public_revision: 0,
        timeline_generation: 1,
        pause_preferences: PausePreferences::default(),
        fatal_failure: None,
    };
    let expected: Vec<_> = (1..=64)
        .map(|lot_count| {
            (
                AccountId(0),
                Intent::PlaceLimit {
                    code: engine::StockCode("600101".into()),
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
