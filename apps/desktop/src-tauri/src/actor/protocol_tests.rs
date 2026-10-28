use super::*;
use engine::session::protocol::ReplayGuard;
include!("../../../../../packages/engine/test-support/simple_company.rs");

#[tokio::test]
async fn company_read_command_preserves_actual_nonempty_data_and_local_owner() {
    use engine::session::protocol::CompanyReadQuery;
    let mut setup = super::tests::diagnostic_setup();
    setup.npcs.inst_count = 0;
    setup.stocks[0].float_shares = 0;
    setup.history_len = 1;
    setup.company_system = Some(simple_company_fixture!(engine; ["600101"]));
    setup.dividend_tax_mode = Some(engine::company::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1000);
    setup.par_value_per_share = Some(engine::Money::from_cents(100));
    setup.auto_corporate_foundation = true;
    let game = ProtocolSession::new(setup, 7).unwrap();
    let expected =
        serde_json::to_value(game.account_dividend_tax_status(AccountId(0)).unwrap()).unwrap();
    let before = serde_json::to_value(game.game().save().unwrap()).unwrap();
    let mut harness = ActorHarness::new_protocol_actor(
        game,
        false,
        PausePreferences::default(),
        "company",
        "one",
    );
    let (reply, received) = oneshot::channel();
    harness
        .actor
        .handle_command(SessionCommand::CompanyRead {
            generation: 1,
            query: CompanyReadQuery::AccountDividendTaxStatus,
            reply,
        })
        .await;
    let response = received.await.unwrap().unwrap();
    let wire = serde_json::to_value(response).unwrap();
    assert_eq!(wire["generation"], "1");
    assert_eq!(wire["owner"], "0");
    assert_eq!(wire["value"]["data"], expected);
    assert_eq!(wire["value"]["data"]["stocks"][0]["stock"], "600101");
    for query in [
        CompanyReadQuery::CompanyPeriodExplanation {
            company: engine::company::CompanyId(0),
            period_end: engine::CivilDate::from_iso("2029-12-31").unwrap(),
        },
        CompanyReadQuery::DividendTaxOutstandingViews,
        CompanyReadQuery::OwnerFlatWithholdingReceipts,
        CompanyReadQuery::CompanyCapabilities {
            company: engine::company::CompanyId(0),
        },
        CompanyReadQuery::OwnerRightsOfferings,
    ] {
        let kind = serde_json::to_value(&query).unwrap()["kind"].clone();
        let (reply, received) = oneshot::channel();
        harness
            .actor
            .handle_command(SessionCommand::CompanyRead {
                generation: 1,
                query,
                reply,
            })
            .await;
        let wire = serde_json::to_value(received.await.unwrap().unwrap()).unwrap();
        assert_eq!(wire["generation"], "1");
        assert_eq!(wire["owner"], "0");
        assert_eq!(wire["value"]["kind"], kind);
    }
    assert_eq!(
        before,
        serde_json::to_value(harness.actor.game.game().save().unwrap()).unwrap()
    );
}

#[tokio::test]
async fn company_read_command_checks_generation_and_preserves_typed_domain_failure() {
    use engine::session::protocol::CompanyReadQuery;
    let mut setup = super::tests::diagnostic_setup();
    setup.npcs.inst_count = 0;
    setup.history_len = 1;
    let game = ProtocolSession::new(setup, 7).unwrap();
    let before = serde_json::to_value(game.game().save().unwrap()).unwrap();
    let mut harness = ActorHarness::new_protocol_actor(
        game,
        false,
        PausePreferences::default(),
        "company",
        "one",
    );
    let (reply, received) = oneshot::channel();
    harness
        .actor
        .handle_command(SessionCommand::CompanyRead {
            generation: 2,
            query: CompanyReadQuery::AccountDividendTaxStatus,
            reply,
        })
        .await;
    assert!(matches!(
        received.await.unwrap(),
        Err(CompanyReadHostError::Session { .. })
    ));
    let (reply, received) = oneshot::channel();
    harness
        .actor
        .handle_command(SessionCommand::CompanyRead {
            generation: 1,
            query: CompanyReadQuery::AccountDividendTaxStatus,
            reply,
        })
        .await;
    assert!(matches!(
        received.await.unwrap(),
        Err(CompanyReadHostError::Query(_))
    ));
    assert_eq!(
        before,
        serde_json::to_value(harness.actor.game.game().save().unwrap()).unwrap()
    );
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
        code: engine::StockId(0),
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
    let restored = serde_json::to_value(actor.restore(1, Box::new(slot)).unwrap()).unwrap();
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

#[test]
fn baseline_catalogs_match_actual_desktop_session() {
    let mut setup = super::tests::diagnostic_setup();
    setup.npcs.inst_count = 0;
    setup.history_len = 1;
    setup.stocks[0].float_shares = 0;
    let game = ProtocolSession::new(setup, 7).unwrap();
    let expected_stocks = game.stock_catalog_view();
    let expected_companies = game.company_catalog_view().unwrap();
    let mut harness = ActorHarness::new_protocol_actor(
        game,
        false,
        PausePreferences::default(),
        "catalog",
        "one",
    );
    let baseline = harness
        .actor
        .baseline_candidate("query_baseline", "desktop.actor.query_baseline")
        .unwrap();
    assert_eq!(baseline.stock_catalog, expected_stocks);
    assert_eq!(baseline.company_catalog, expected_companies);
    assert_eq!(
        baseline.stock_catalog.len(),
        baseline.snapshot.markets.len()
    );
}

#[test]
fn committed_restore_catalog_failure_keeps_generation_and_closes_desktop_actor() {
    let mut setup = super::tests::diagnostic_setup();
    setup.npcs.inst_count = 0;
    setup.history_len = 1;
    setup.stocks[0].float_shares = 0;
    let mut game = ProtocolSession::new(setup, 7).unwrap();
    game.end_civil_day_update().unwrap();
    let saved = game.save().unwrap();
    let mut harness = ActorHarness::new_protocol_actor(
        game,
        false,
        PausePreferences::default(),
        "catalog",
        "one",
    );
    harness.actor.restore(1, Box::new(saved)).unwrap();
    let new_timeline = harness.actor.timeline_id.clone();
    let ingress = harness.actor.game.shared_ingress();
    let error = engine::session::CompanyCatalogViewError::UnknownListedStock {
        company: engine::company::CompanyId(7),
    };
    let result =
        harness
            .actor
            .finish_catalog_baseline(Err(error), "restore", "desktop.actor.restore");
    assert!(matches!(result, Err(BaselineError::Catalog(_))));
    assert_eq!(harness.actor.generation, 2);
    assert_eq!(harness.actor.timeline_id, new_timeline);
    assert!(harness.cmd_tx.is_closed());
    assert!(ingress
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: engine::StockId(0),
                id: engine::OrderId(1)
            }
        )
        .is_err());
}

#[test]
fn baseline_error_source_chain_retains_typed_catalog_leaf() {
    use std::error::Error;
    let error = BaselineRequestError::Baseline(BaselineError::Catalog(
        engine::session::CompanyCatalogViewError::UnknownListedStock {
            company: engine::company::CompanyId(7),
        },
    ));
    assert!(error
        .source()
        .unwrap()
        .is::<engine::session::CompanyCatalogViewError>());
}
