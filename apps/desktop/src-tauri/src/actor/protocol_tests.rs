use super::*;
use engine::session::protocol::ReplayGuard;

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
    let restored = harness.actor.restore(1, Box::new(slot)).unwrap();
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
