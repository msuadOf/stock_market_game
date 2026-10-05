use super::*;

#[tokio::test]
async fn civil_batch_preparation_defers_ingress_calendar_until_success() {
    let mut setup = super::tests::diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-01").unwrap();
    setup.npcs.inst_count = 0;
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
        let updates = SessionActor::<tauri::test::MockRuntime>::prepare_civil_updates(
            candidate,
            &PausePreferences::default(),
        )?;
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

    let mut harness = ActorHarness::new_protocol_actor(
        game,
        true,
        PausePreferences::default(),
        "calendar-publication",
        "calendar-timeline",
    );
    harness.subscribe_engine_events();
    harness.actor.run_cycle(1);
    assert_eq!(harness.actor.game.civil_date().to_iso(), "2030-01-02");
    assert_eq!(harness.actor.game.tick(), 0);
    assert!(harness.actor.pacing.is_running());
    let receiver = harness.events_rx.as_mut().unwrap();
    let payload: serde_json::Value =
        serde_json::from_str(&receiver.try_recv().unwrap()).unwrap();
    let update: EngineUpdate = serde_json::from_value(payload["update"].clone()).unwrap();
    assert!(matches!(update, EngineUpdate::CivilUpdate(_)));
    assert!(receiver.try_recv().is_err());
    source.enqueue_player_intent(AccountId(0), intent).unwrap();
    assert_eq!(harness.actor.game.game().save().unwrap().pending_player.len(), 1);
}

use engine::session::protocol::ReplayGuard;

#[tokio::test]
async fn correction_rejection_pauses_without_closing_commands_and_allows_cancel_retry() {
    let mut setup = super::tests::diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-05").unwrap();
    let mut game = ProtocolSession::new(setup, 7181).unwrap();
    let input: engine::CompanyReportCorrection = serde_json::from_value(serde_json::json!({"operation_id":"desktop-correction-1","company":"不存在的经营公司","supersedes":1,"reason":"待日终验证的更正","entries":[{"source":900000001,"date":"2030-01-05","kind":"CashRevenue","cash_flow":"Operating","lines":[{"account":"1002","side":"Debit","amount":"10"},{"account":"6001","side":"Credit","amount":"10"}]}]})).unwrap();
    let epoch = game.report_correction_epoch();
    game.enqueue_report_correction(&epoch, input.clone())
        .unwrap();
    let mut harness = ActorHarness::new_protocol_actor(
        game,
        false,
        PausePreferences::default(),
        "correction",
        "correction-timeline",
    );
    harness.subscribe_failures();
    harness.actor.tick_and_emit().await;
    let failure: serde_json::Value =
        serde_json::from_str(&harness.failures_rx.as_mut().unwrap().try_recv().unwrap()).unwrap();
    assert_eq!(failure["code"], "REPORT_CORRECTION_REJECTED");
    assert_eq!(failure["recoverable"], true);
    assert!(!harness.cmd_tx.is_closed());
    assert!(!harness.actor.pacing.is_running());
    assert_eq!(harness.actor.game.civil_date().to_iso(), "2030-01-05");
    assert_eq!(
        serde_json::to_value(harness.actor.game.report_corrections().unwrap().pending).unwrap(),
        serde_json::json!([input])
    );
    let (reply, response) = oneshot::channel();
    harness
        .actor
        .handle_command(SessionCommand::CancelReportCorrection {
            generation: 1,
            operation_id: "desktop-correction-1".into(),
            reply,
        })
        .await;
    assert_eq!(response.await.unwrap().unwrap().generation, "1");
    harness.actor.tick_and_emit().await;
    assert_eq!(harness.actor.game.civil_date().to_iso(), "2030-01-07");
}

#[tokio::test]
async fn shared_ingress_receives_player_without_actor_command_polling() {
    let mut setup = super::tests::diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-02").unwrap();
    setup.npcs.inst_count = 0;
    let mut game = ProtocolSession::new(setup, 7).unwrap();
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
    let handles = SessionHandles {
        cmd_tx,
        ingress: Arc::new(RwLock::new((1, game.shared_ingress()))),
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
                code: StockCode("600101".into()),
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

#[tokio::test]
async fn restore_rebinds_host_ingress_and_closes_previous_generation() {
    let mut setup = super::tests::diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-05").unwrap();
    let mut completed = ProtocolSession::new(setup.clone(), 7).unwrap();
    completed.end_civil_day_update().unwrap();
    let slot = completed.save().unwrap();
    let game = ProtocolSession::new(setup, 7).unwrap();
    let mut harness = ActorHarness::new_protocol_actor(
        game,
        false,
        PausePreferences::default(),
        "restore-ingress",
        "restore-timeline",
    );
    let handles = SessionHandles {
        cmd_tx: harness.cmd_tx.clone(),
        ingress: Arc::clone(&harness.actor.ingress),
    };
    let previous = handles.ingress.read().unwrap().1.clone();
    let intent = Intent::PlaceLimit {
        code: StockCode("600101".into()),
        side: engine::Side::Buy,
        price: engine::LimitPrice::Fixed(engine::Money::from_cents(900)),
        qty: 100,
    };
    let restored = harness.actor.restore(1, Box::new(slot), None).unwrap();
    assert_eq!(restored.generation, "2");
    assert!(previous
        .enqueue_player_intent(AccountId(0), intent.clone())
        .is_err());
    assert!(handles.enqueue(1, intent.clone()).await.is_err());
    handles.enqueue(2, intent.clone()).await.unwrap();
    let saved = harness.actor.game.game().save().unwrap();
    assert_eq!(saved.pending_player.len(), 1);
    assert_eq!(
        serde_json::to_value(&saved.pending_player[0].intent).unwrap(),
        serde_json::to_value(&intent).unwrap()
    );
    let (reply, applied) = oneshot::channel();
    harness
        .actor
        .handle_command(SessionCommand::Shutdown { reply })
        .await;
    applied.await.unwrap();
    assert!(handles
        .ingress
        .read()
        .unwrap()
        .1
        .enqueue_player_intent(AccountId(0), intent)
        .is_err());
}

#[tokio::test]
async fn baseline_and_restore_include_authoritative_civil_date() {
    let game = ProtocolSession::new(super::tests::diagnostic_setup(), 41).unwrap();
    let mut harness = ActorHarness::new_protocol_actor(
        game,
        false,
        PausePreferences::default(),
        "calendar",
        "calendar-timeline",
    );
    let actor = &mut harness.actor;
    for expected in ["2030-01-01", "2030-01-02"] {
        let (reply, received) = oneshot::channel();
        actor
            .handle_command(SessionCommand::QueryBaseline {
                generation: 1,
                reply,
            })
            .await;
        let baseline = serde_json::to_value(received.await.unwrap().unwrap()).unwrap();
        assert_eq!(baseline["civil_date"], expected);
        assert_eq!(baseline["snapshot"]["day"], actor.game.snapshot().day);
        if expected == "2030-01-01" {
            actor.game.end_civil_day_update().unwrap();
        }
    }
    let slot = actor.game.save().unwrap();
    let restored = serde_json::to_value(actor.restore(1, Box::new(slot), None).unwrap()).unwrap();
    assert_eq!(restored["civil_date"], "2030-01-02");
    assert_eq!(restored["generation"], "2");
}

async fn capture(fastest: bool, preferences: PausePreferences) {
    let mut setup = super::tests::diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-02").unwrap();
    let game = ProtocolSession::new(setup, 41).unwrap();
    let mut harness = ActorHarness::new_protocol_actor(
        game,
        fastest,
        preferences,
        "protocol",
        "protocol-timeline",
    );
    harness.subscribe_engine_events();
    let receiver = harness
        .events_rx
        .as_mut()
        .expect("protocol 场景须订阅引擎事件");
    let actor = &mut harness.actor;
    let mut guard = ReplayGuard::new(0, 0);
    let mut ticks = Vec::new();
    loop {
        if fastest {
            actor.run_fastest_batch().await;
        } else {
            actor.tick_and_emit().await;
        }
        while let Ok(payload) = receiver.try_recv() {
            let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
            assert!(value.get("events").is_none());
            let update: EngineUpdate = serde_json::from_value(value["update"].clone()).unwrap();
            guard.ingest(&update).unwrap();
            match &update {
                EngineUpdate::TickBatch(batch) => {
                    batch.validate().unwrap();
                    assert!(batch.runtime_snapshot.is_none());
                    let delta = batch.runtime_delta.as_ref().unwrap();
                    let last = batch.frames.last().unwrap();
                    assert_eq!(delta.tick, last.tick);
                    assert_eq!(delta.seq_from, batch.frames.first().unwrap().seq_from);
                    assert_eq!(delta.seq_to, last.seq_to);
                    assert!(delta
                        .accounts
                        .keys()
                        .all(|account| *account == engine::AccountId(0)));
                    if batch.frames.first().unwrap().tick == 1 {
                        assert!(delta.accounts.contains_key(&engine::AccountId(0)));
                        assert!(delta.working_orders.reset);
                    }
                    for frame in &batch.frames {
                        assert_eq!(frame.tick, ticks.len() as u64 + 1);
                        ticks.push(frame.tick);
                        assert!(!frame.timeseries_payload.markets.is_empty());
                    }
                }
                EngineUpdate::CivilUpdate(civil) => {
                    civil.validate().unwrap();
                    assert_eq!(ticks, (1..=30).collect::<Vec<_>>());
                    assert_eq!(civil.refresh.intraday.len(), 30);
                    assert_eq!(actor.pacing.is_running(), !preferences.pauses(civil));
                    println!("desktop fastest={fastest}: {payload}");
                    let (reply, applied) = oneshot::channel();
                    actor
                        .handle_command(SessionCommand::SetRunning {
                            running: true,
                            reply,
                        })
                        .await;
                    applied.await.unwrap();
                    actor.tick_and_emit().await;
                    let next: serde_json::Value =
                        serde_json::from_str(&receiver.try_recv().unwrap()).unwrap();
                    let next: EngineUpdate =
                        serde_json::from_value(next["update"].clone()).unwrap();
                    assert!(matches!(next, EngineUpdate::TickBatch(_)));
                    guard.ingest(&next).unwrap();
                    assert!(receiver.try_recv().is_err());
                    return;
                }
            }
        }
    }
}

#[tokio::test]
async fn fixed_complete_frames_auto_continue() {
    capture(false, PausePreferences::default()).await;
}

#[tokio::test]
async fn fastest_complete_frames_auto_continue() {
    capture(true, PausePreferences::default()).await;
}

#[tokio::test]
async fn after_close_publishes_before_pausing_and_resumes_once() {
    capture(
        false,
        PausePreferences {
            pause_after_close: true,
            pause_before_open: false,
        },
    )
    .await;
}

#[tokio::test]
async fn before_open_publishes_before_pausing_and_resumes_once() {
    capture(
        true,
        PausePreferences {
            pause_after_close: false,
            pause_before_open: true,
        },
    )
    .await;
}
