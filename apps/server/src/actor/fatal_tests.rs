use super::*;

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
        cmd_tx,
        event_tx,
        ticks_per_day: setup.ticks_per_day,
        auction_ticks: setup.auction_ticks,
        closing_auction_ticks: setup.closing_auction_ticks,
        session_token: "shared-ingress".into(),
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
        ingress: Arc::new(RwLock::new((1, game.shared_ingress()))),
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
        ingress: Arc::new(RwLock::new((7, game.shared_ingress()))),
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

    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-01").unwrap());
    let game = ProtocolSession::new(setup.clone(), 42).unwrap();
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let handles = SessionHandles {
        ingress: Arc::new(RwLock::new((1, game.shared_ingress()))),
        cmd_tx,
        event_tx: event_tx.clone(),
        ticks_per_day: setup.ticks_per_day,
        auction_ticks: setup.auction_ticks,
        closing_auction_ticks: setup.closing_auction_ticks,
        session_token: "command-burst".into(),
    };
    let mut actor = SessionActor {
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
