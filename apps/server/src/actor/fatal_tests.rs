use super::*;

#[path = "../../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

fn assert_fatal_rejection(error: SessionError) {
    let message = error.to_string();
    assert!(
        message.contains("session stopped after STEP_FATAL"),
        "{message}"
    );
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
            price: engine::Money::from_cents(1000),
            qty: 100,
        },
    )
    .unwrap();
    let before = (game.tick(), game.seq(), game.business_state_hash().unwrap());
    let slot_before = game.save().unwrap();
    let saved_before = serde_json::to_value(&slot_before).unwrap();
    let fatal = engine::session::StepFatal::InvariantViolation {
        location: "server.auto_step".into(),
        description: "third step injected failure".into(),
    };
    let (_sender, cmd_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
    let (event_tx, mut receiver) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let mut actor = SessionActor {
        injected_step_failure: Some((successful_steps, fatal.clone())),
        speed_meter: SpeedMeter::new(game.tick()),
        game,
        cmd_rx,
        event_tx,
        tick_interval: Duration::from_millis(1),
        base_ms: 1,
        session_id: "fatal".into(),
        running: true,
        fastest: true,
        requested_speed: RequestedSpeed::Fastest,
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
    assert_eq!(failure.failure.as_ref().unwrap().message, fatal.to_string());
    assert_eq!(
        (
            actor.game.tick(),
            actor.game.seq(),
            actor.game.business_state_hash().unwrap()
        ),
        before
    );
    assert_eq!(actor.public_revision, 4);
    assert_eq!(
        serde_json::to_value(actor.game.save().unwrap()).unwrap(),
        saved_before
    );
    assert_eq!(actor.speed_meter.started_tick, before.0);
    assert_eq!(actor.speed_meter.sample_ticks, 0);
    assert!(!actor.running);
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
    actor.handle_command(SessionCommand::Save { reply }).await;
    assert_fatal_rejection(response.await.unwrap().unwrap_err());

    let (reply, response) = oneshot::channel();
    actor
        .handle_command(SessionCommand::Enqueue {
            player_id: AccountId(0),
            intent: Intent::PlaceLimit {
                code: engine::StockCode("600101".into()),
                side: engine::Side::Buy,
                price: engine::Money::from_cents(1000),
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
    assert_eq!(
        serde_json::to_value(actor.game.save().unwrap()).unwrap(),
        saved_before
    );
    assert_eq!(actor.requested_speed, RequestedSpeed::Fastest);
    assert!(!actor.running);
    assert_eq!(actor.pause_preferences, PausePreferences::default());
    for _ in before.0..fixture::TICKS_PER_DAY {
        actor.game.step_frame().unwrap();
    }
    let civil = actor.game.end_civil_day_update().unwrap();
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
    let expected_message = fatal.to_string();
    let failure = HostFailure::step(&fatal);
    assert_eq!(failure.code, "STEP_FATAL");
    assert_eq!(failure.message, expected_message);
    assert_eq!(
        serde_json::to_value(&failure).unwrap(),
        serde_json::json!({ "code": "STEP_FATAL", "message": expected_message })
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
        "invalid save: civil boundary is inconsistent"
    );
}

#[tokio::test]
async fn stale_preferences_leave_actor_settings_unchanged() {
    let manager = SessionManager::default();
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let id = manager.new_session(setup, 1).unwrap();
    let handles = manager.lookup(&id).unwrap();
    let slot = handles.save().await.unwrap();
    handles.restore(slot).await.unwrap();

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
}
