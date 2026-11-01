//! 更正拒绝后的真实 actor／协议恢复边界；依赖已审 E2 与显式命令转发组合。

use super::*;
use engine::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, JournalLine,
    LedgerAccountId, PostingSide,
};
use engine::session::protocol::ReplayGuard;
use engine::session::CompanyReportCorrection;
include!("../../../../../packages/engine/test-support/simple_company.rs");

fn correction_game(start: &str) -> (ProtocolSession, CompanyReportCorrection) {
    let mut setup = super::tests::diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso(start).unwrap();
    setup.ticks_per_day = 2;
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 1;
    setup.stocks[0].float_shares = 0;
    setup.history_len = 1;
    setup.company_system = Some(simple_company_fixture!(engine; ["600101"]));
    setup.dividend_tax_mode = Some(engine::company::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1_000);
    setup.par_value_per_share = Some(engine::Money::from_cents(100));
    let game = ProtocolSession::new(setup, 7181).unwrap();
    let original = game
        .game()
        .save()
        .unwrap()
        .public_library
        .save()
        .reports
        .into_iter()
        .filter(|report| {
            report.company == engine::company::CompanyId(0)
                && report.source == engine::information::PublicationSource::SimpleGenerated
                && matches!(
                    report.reports.scope,
                    engine::accounting::consolidation::ScopeId::Standalone(_)
                )
        })
        .max_by_key(|report| report.id)
        .unwrap();
    let request = CompanyReportCorrection {
        operation_id: "desktop-actor-correction-recovery".into(),
        company: original.company,
        supersedes: original.id,
        entries: vec![JournalEntry {
            source: BusinessEventId::new(900_000_001),
            date: game.civil_date(),
            kind: BusinessKind::CashRevenue,
            cash_flow: CashFlowClass::Operating,
            lines: vec![
                JournalLine {
                    account: LedgerAccountId("1002".into()),
                    side: PostingSide::Debit,
                    amount: AccountingAmount::from_cents(1_000),
                },
                JournalLine {
                    account: LedgerAccountId("6001".into()),
                    side: PostingSide::Credit,
                    amount: AccountingAmount::from_cents(1_000),
                },
            ],
        }],
        reason: "actor 恢复边界私有理由".into(),
    };
    (game, request)
}

fn correction_harness(game: ProtocolSession) -> ActorHarness {
    ActorHarness::new_protocol_actor(
        game,
        false,
        PausePreferences {
            pause_after_close: true,
            pause_before_open: true,
        },
        "correction-recovery",
        "correction-timeline",
    )
}

fn ordered_actor_events(harness: &ActorHarness) -> mpsc::UnboundedReceiver<&'static str> {
    use tauri::Listener;
    let (sender, receiver) = mpsc::unbounded_channel();
    for event_name in [crate::ENGINE_EVENT_NAME, "engine-failure"] {
        let sender = sender.clone();
        harness.app.listen(event_name, move |_| {
            sender.send(event_name).unwrap();
        });
    }
    receiver
}

fn assert_ingress_open_without_receiving_input<R: Runtime>(actor: &mut SessionActor<R>) {
    // 未知账户在开放检查后、分配 receipt 前拒绝，探测不会增加待处理玩家请求。
    assert!(matches!(
        actor.game.shared_ingress().enqueue_player_intent(
            AccountId(999_999),
            Intent::Cancel {
                code: engine::StockId(0),
                id: engine::OrderId(1),
            }
        ),
        Err(SessionError::UnknownPlayer(AccountId(999_999)))
    ));
}

fn accounting_leaf(
    error: &engine::accounting::AccountingError,
) -> &engine::accounting::AccountingError {
    match error {
        engine::accounting::AccountingError::BatchAborted { cause, .. } => accounting_leaf(cause),
        error => error,
    }
}

#[tokio::test]
async fn correction_business_rejection_keeps_installed_close_and_contiguous_cancel_retry() {
    let (mut game, request) = correction_game("2030-06-07");
    let mut rejected = request.clone();
    rejected.entries[0].kind = BusinessKind::CashExpense;
    rejected.entries[0].lines[0].account = LedgerAccountId("6001".into());
    rejected.entries[0].lines[1].account = LedgerAccountId("1002".into());
    for line in &mut rejected.entries[0].lines {
        line.amount = AccountingAmount::from_cents(1_000_000_000_000);
    }
    let epoch = game.report_correction_epoch();
    game.enqueue_report_correction(&epoch, rejected.clone())
        .unwrap();
    let mut replay = ReplayGuard::new(game.tick(), game.seq());
    let first = game.step_frame().unwrap();
    replay
        .ingest(&EngineUpdate::TickBatch(Box::new(
            game.tick_batch(vec![first]).unwrap(),
        )))
        .unwrap();
    let retained_before = game.retained_market_days().len();
    let mut harness = correction_harness(game);
    harness.subscribe_failures();
    harness.subscribe_engine_events();
    let mut ordered = ordered_actor_events(&harness);

    harness.actor.run_cycle(1);

    let mut committed_ticks = Vec::new();
    while let Ok(payload) = harness.events_rx.as_mut().unwrap().try_recv() {
        let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
        let update: EngineUpdate = serde_json::from_value(value["update"].clone()).unwrap();
        let EngineUpdate::TickBatch(batch) = &update else {
            panic!("更正拒绝不能伪造 CivilUpdate")
        };
        committed_ticks.extend(batch.frames.iter().map(|frame| frame.tick));
        replay.ingest(&update).unwrap();
    }
    let payload = harness.failures_rx.as_mut().unwrap().try_recv().unwrap();
    let failure: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(failure["code"], "REPORT_CORRECTION_PREPARE_REJECTED");
    assert_eq!(failure["where"], "desktop.actor.prepare_civil_updates");
    assert_eq!(failure["cause"]["code"], "REPORT_CORRECTION_REJECTED");
    assert_eq!(
        failure["recoverable"], true,
        "健康更正拒绝必须保留取消与重试入口"
    );
    assert!(!payload.contains(&request.reason));
    assert_eq!(ordered.try_recv().unwrap(), crate::ENGINE_EVENT_NAME);
    assert_eq!(ordered.try_recv().unwrap(), "engine-failure");
    assert!(ordered.try_recv().is_err());
    assert!(harness.failures_rx.as_mut().unwrap().try_recv().is_err());
    let actor = &mut harness.actor;
    assert_eq!(
        actor.game.tick(),
        2,
        "真实已安装末拍不能退回批前 checkpoint"
    );
    assert_eq!(actor.game.day(), 1);
    assert_eq!(actor.game.civil_date(), request.entries[0].date);
    assert!(actor.game.civil_day_ready().unwrap());
    assert!(actor.game.require_healthy().is_ok());
    let close_origin = actor.game.runtime_mirror_origin();
    assert_eq!(close_origin.source_tick, 2);
    assert_eq!(
        close_origin.day_end_stage,
        engine::RuntimeMirrorDayEndStage::RetainedPublished
    );
    assert_eq!(
        actor.game.report_corrections().unwrap().pending,
        vec![rejected]
    );
    assert!(actor
        .game
        .report_corrections()
        .unwrap()
        .completed
        .is_empty());
    assert_eq!(committed_ticks, vec![2]);
    assert_eq!(actor.game.retained_market_days().len(), retained_before + 1);
    assert!(!actor.pacing.is_running());
    assert!(!harness.cmd_tx.is_closed());
    assert_ingress_open_without_receiving_input(actor);
    let (reply, received) = oneshot::channel();
    actor
        .handle_command(SessionCommand::Snapshot { reply })
        .await;
    assert_eq!(received.await.unwrap().tick, 2);

    let (reply, received) = oneshot::channel();
    actor
        .handle_command(SessionCommand::CancelReportCorrection {
            generation: 1,
            operation_id: request.operation_id.clone(),
            reply,
        })
        .await;
    received.await.unwrap().unwrap();
    assert!(actor.game.report_corrections().unwrap().pending.is_empty());
    let (reply, received) = oneshot::channel();
    actor
        .handle_command(SessionCommand::EnqueueReportCorrection {
            generation: 1,
            request: request.clone(),
            reply,
        })
        .await;
    received.await.unwrap().unwrap();
    let (reply, received) = oneshot::channel();
    actor
        .handle_command(SessionCommand::SetRunning {
            running: true,
            reply,
        })
        .await;
    received.await.unwrap();
    assert!(actor.pacing.is_running());
    actor.run_cycle(0);
    let payload = harness.events_rx.as_mut().unwrap().try_recv().unwrap();
    let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
    let update: EngineUpdate = serde_json::from_value(value["update"].clone()).unwrap();
    let EngineUpdate::CivilUpdate(civil) = &update else {
        panic!("取消后同 tick 重试必须只发布真实 CivilUpdate")
    };
    assert_eq!(civil.tick, 2);
    assert_eq!(
        civil
            .refresh
            .intraday
            .iter()
            .map(|frame| frame.tick)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    replay.ingest(&update).unwrap();
    assert!(harness.events_rx.as_mut().unwrap().try_recv().is_err());
    assert!(harness.failures_rx.as_mut().unwrap().try_recv().is_err());
    assert_eq!(actor.game.tick(), 2);
    assert_eq!(actor.game.retained_market_days().len(), retained_before + 1);
    let status = actor.game.report_corrections().unwrap();
    assert!(status.pending.is_empty());
    assert_eq!(status.completed[&request.operation_id].request, request);
    assert!(actor.game.require_healthy().is_ok());
    let retry_origin = actor.game.runtime_mirror_origin();
    assert_eq!(retry_origin.source_tick, close_origin.source_tick);
    assert_eq!(
        retry_origin.source_generation,
        close_origin.source_generation
    );
    assert_eq!(retry_origin.authority_epoch, close_origin.authority_epoch);
    assert!(
        retry_origin.install_sequence >= close_origin.install_sequence,
        "civil-only retry 不得倒退 mirror 安装序"
    );
}

#[tokio::test]
async fn correction_invalid_input_on_closed_day_keeps_real_actor_command_channel() {
    let (mut game, request) = correction_game("2030-06-08");
    let mut rejected = request.clone();
    rejected.entries[0].lines[1].account = LedgerAccountId("不存在的科目".into());
    let epoch = game.report_correction_epoch();
    game.enqueue_report_correction(&epoch, rejected.clone())
        .unwrap();
    let error = game.end_civil_day_update().unwrap_err();
    assert!(
        matches!(error, SessionError::ReportCorrectionDayEnd(ref cause)
        if cause.classification() == engine::company::CompanyErrorClass::InvalidInput)
    );
    assert!(game.require_healthy().is_ok());
    let before = game.business_state_hash().unwrap();
    let mut harness = correction_harness(game);
    harness.subscribe_failures();
    harness.subscribe_engine_events();
    harness.actor.run_cycle(0);
    let payload = harness.failures_rx.as_mut().unwrap().try_recv().unwrap();
    let failure: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(failure["cause"]["code"], "REPORT_CORRECTION_REJECTED");
    assert_eq!(failure["recoverable"], true);
    assert!(harness.events_rx.as_mut().unwrap().try_recv().is_err());
    assert_eq!(harness.actor.game.business_state_hash().unwrap(), before);
    assert!(harness.actor.game.require_healthy().is_ok());
    assert!(!harness.cmd_tx.is_closed());
    assert_ingress_open_without_receiving_input(&mut harness.actor);
    let (reply, received) = oneshot::channel();
    harness
        .actor
        .handle_command(SessionCommand::Snapshot { reply })
        .await;
    assert_eq!(received.await.unwrap().tick, 0);
    let (reply, received) = oneshot::channel();
    harness
        .actor
        .handle_command(SessionCommand::CancelReportCorrection {
            generation: 1,
            operation_id: request.operation_id.clone(),
            reply,
        })
        .await;
    received.await.unwrap().unwrap();
    assert!(harness
        .actor
        .game
        .report_corrections()
        .unwrap()
        .pending
        .is_empty());
    assert!(harness.actor.game.game().save().is_ok());
}

#[test]
fn correction_real_accounting_overflow_keeps_fatal_actor_and_closed_ingress() {
    let (mut game, mut request) = correction_game("2030-06-08");
    for line in &mut request.entries[0].lines {
        line.amount = AccountingAmount::MAX;
    }
    // MAX 进入已有正收入余额，精确命中原断言承诺的 post_batch/BatchAborted 边界。
    request.entries[0].lines[1].account =
        LedgerAccountId(engine::accounting::reports::simple_summary::REVENUE.into());
    request.entries[0].validate_invariants().unwrap();
    // BatchAborted 不公开 Error::source；用相同真实输入预检准确 leaf，不伪称 wire 已穿透。
    let (mut probe, mut probe_request) = correction_game("2030-06-08");
    for line in &mut probe_request.entries[0].lines {
        line.amount = AccountingAmount::MAX;
    }
    probe_request.entries[0].lines[1].account =
        LedgerAccountId(engine::accounting::reports::simple_summary::REVENUE.into());
    assert_eq!(probe_request, request);
    let probe_epoch = probe.report_correction_epoch();
    probe
        .enqueue_report_correction(&probe_epoch, probe_request)
        .unwrap();
    let error = probe.end_civil_day_update().unwrap_err();
    assert!(matches!(&error, SessionError::ReportCorrectionDayEnd(cause)
        if cause.classification() == engine::company::CompanyErrorClass::SystemState));
    let mut current: &(dyn std::error::Error + 'static) = &error;
    let accounting = loop {
        if let Some(accounting) = current.downcast_ref::<engine::accounting::AccountingError>() {
            break accounting;
        }
        current = current
            .source()
            .expect("真实 overflow 必须保留 AccountingError");
    };
    assert!(matches!(
        accounting_leaf(accounting),
        engine::accounting::AccountingError::AmountOverflow { .. }
    ));
    assert!(probe.require_healthy().is_err());
    let epoch = game.report_correction_epoch();
    game.enqueue_report_correction(&epoch, request).unwrap();
    let mut harness = correction_harness(game);
    harness.subscribe_failures();
    harness.subscribe_engine_events();
    harness.actor.run_cycle(0);
    let payload = harness.failures_rx.as_mut().unwrap().try_recv().unwrap();
    let failure: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(failure["cause"]["code"], "REPORT_CORRECTION_FAILED");
    assert_eq!(failure["recoverable"], false);
    let mut cause = &failure["cause"];
    let mut has_accounting_cause = false;
    while cause.is_object() {
        has_accounting_cause |= cause["code"] == "ACCOUNTING_VALIDATION_FAILED";
        cause = &cause["cause"];
    }
    assert!(has_accounting_cause);
    let first = harness
        .actor
        .game
        .require_healthy()
        .expect_err("fatal 不能被批前 checkpoint 复活");
    assert_eq!(harness.actor.game.poison_reason(), Some(&first));
    assert_eq!(harness.actor.game.step_frame().unwrap_err(), first);
    assert!(harness.events_rx.as_mut().unwrap().try_recv().is_err());
    assert!(harness.failures_rx.as_mut().unwrap().try_recv().is_err());
    assert!(!harness.actor.pacing.is_running());
    assert!(harness.cmd_tx.is_closed());
    let before_rejected_ingress = harness.actor.game.business_state_hash().unwrap();
    assert!(matches!(
        harness.actor.game.shared_ingress().enqueue_player_intent(
            AccountId(0),
            Intent::Cancel { code: engine::StockId(0), id: engine::OrderId(1) }
        ),
        Err(SessionError::Step(actual)) if actual == first
    ));
    assert_eq!(
        harness.actor.game.business_state_hash().unwrap(),
        before_rejected_ingress
    );
}

#[tokio::test]
async fn company_write_fatal_keeps_original_typed_session_source() {
    use std::error::Error;
    let (mut game, mut request) = correction_game("2030-06-08");
    for line in &mut request.entries[0].lines {
        line.amount = AccountingAmount::MAX;
    }
    request.entries[0].lines[1].account =
        LedgerAccountId(engine::accounting::reports::simple_summary::REVENUE.into());
    let epoch = game.report_correction_epoch();
    game.enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    assert!(game.end_civil_day_update().is_err());
    let first = game.require_healthy().unwrap_err();
    let mut harness = correction_harness(game);
    let (reply, received) = oneshot::channel();
    harness
        .actor
        .handle_command(SessionCommand::CancelReportCorrection {
            generation: 1,
            operation_id: request.operation_id,
            reply,
        })
        .await;
    let error = received.await.unwrap().unwrap_err();
    assert!(matches!(&error, CompanyWriteHostError::Fatal(actual) if actual == &first));
    assert_eq!(
        error
            .source()
            .and_then(|source| source.downcast_ref::<engine::session::StepFatal>()),
        Some(&first),
        "COMPANY_WRITE_FATAL_TYPED_SOURCE：宿主包装不能截断首次真实 StepFatal",
    );
}

#[test]
fn generic_civil_failure_cannot_claim_the_prepare_recovery_contract() {
    let (mut game, mut request) = correction_game("2030-06-08");
    request.entries[0].lines[1].account = LedgerAccountId("不存在的科目".into());
    let epoch = game.report_correction_epoch();
    game.enqueue_report_correction(&epoch, request).unwrap();
    let error = game.end_civil_day_update().unwrap_err();
    assert!(game.require_healthy().is_ok());
    let failure = super::failure::HostFailure::civil(error);
    assert!(
        !failure.recoverable,
        "脱离真实 prepare 调用点的同型错误不能获得 F2 权限"
    );
    assert_eq!(failure.code, "CIVIL_DAY_SETTLEMENT_FAILED");
}

#[test]
fn target_day_not_ready_has_a_dedicated_redacted_mapping() {
    let (game, _) = correction_game("2030-06-07");
    let error = game.save_current_day(game.civil_date()).unwrap_err();
    assert!(
        matches!(&error, SessionError::InvalidSave(fault) if fault.code == 3018 && fault.context.is_empty())
    );
    assert_eq!(
        super::failure::failure_description(&error),
        (
            "SAVE_TARGET_DAY_NOT_READY",
            "目标自然日尚未完成日终，当前日存档不可用（原始详情已脱敏）"
        )
    );
    for code in [3015, 3016, 3017] {
        assert_eq!(
            super::failure::failure_description(&SessionError::InvalidSave(
                engine::ValidationFault::bare(code)
            ))
            .0,
            "SESSION_STATE_INVALID"
        );
    }
}

#[test]
fn fatal_civil_after_market_install_keeps_committed_close_and_first_poison() {
    let (mut game, mut request) = correction_game("2030-06-07");
    for line in &mut request.entries[0].lines {
        line.amount = AccountingAmount::MAX;
    }
    request.entries[0].lines[1].account =
        LedgerAccountId(engine::accounting::reports::simple_summary::REVENUE.into());
    let first = game.step_frame().unwrap();
    game.tick_batch(vec![first]).unwrap();
    let epoch = game.report_correction_epoch();
    game.enqueue_report_correction(&epoch, request).unwrap();
    let mut harness = correction_harness(game);
    harness.subscribe_engine_events();
    harness.subscribe_failures();
    harness.actor.run_cycle(1);
    let payload = harness
        .events_rx
        .as_mut()
        .unwrap()
        .try_recv()
        .expect("已打包成功末拍须在 fatal 前交付");
    let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    let update: EngineUpdate = serde_json::from_value(payload["update"].clone()).unwrap();
    let EngineUpdate::TickBatch(batch) = update else {
        panic!("只能交付真实已成功市场帧")
    };
    assert_eq!(
        batch
            .frames
            .iter()
            .map(|frame| frame.tick)
            .collect::<Vec<_>>(),
        vec![2]
    );
    let payload = harness.failures_rx.as_mut().unwrap().try_recv().unwrap();
    let failure: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(failure["recoverable"], false);
    assert_eq!(
        harness.actor.game.tick(),
        2,
        "installed close 不得复活批前 mirror"
    );
    assert_eq!(harness.actor.game.retained_market_days().len(), 1);
    let origin = harness.actor.game.runtime_mirror_origin();
    assert_eq!(origin.source_tick, 2);
    assert!(origin.authority_epoch > 0 && origin.install_sequence > 0);
    assert_eq!(
        origin.day_end_stage,
        engine::RuntimeMirrorDayEndStage::RetainedPublished
    );
    let first = harness
        .actor
        .game
        .require_healthy()
        .expect_err("post-install civil fatal 必须保 poison");
    assert_eq!(harness.actor.game.step_frame().unwrap_err(), first);
    assert_eq!(
        harness.actor.game.save().unwrap_err().to_string(),
        SessionError::Step(first).to_string()
    );
    assert!(harness.events_rx.as_mut().unwrap().try_recv().is_err());
    assert!(harness.failures_rx.as_mut().unwrap().try_recv().is_err());
}

#[tokio::test]
async fn target_day_not_ready_uses_the_actual_save_error_conversion() {
    let (game, _) = correction_game("2030-06-07");
    let error = game.save_current_day(game.civil_date()).unwrap_err();
    let (cmd_tx, mut commands) = mpsc::unbounded_channel();
    let handles = SessionHandles {
        cmd_tx,
        ingress: Arc::new(RwLock::new((1, game.shared_ingress()))),
    };
    let (result, ()) = tokio::join!(handles.save(1, None), async {
        let SessionCommand::Save { reply, .. } = commands.recv().await.unwrap() else {
            panic!("必须走既有 Save 命令转换")
        };
        reply.send(Err(error)).unwrap();
    });
    assert!(
        matches!(result, Err(SendCommandError::Rejected(message)) if message == "SAVE_TARGET_DAY_NOT_READY: 目标自然日尚未完成日终，当前日存档不可用（原始详情已脱敏）")
    );
}

#[test]
fn civil_prefix_is_published_before_the_next_real_month_end_failure() {
    let (base, _) = correction_game("2030-06-29");
    let mut setup = base.export_setup_for_runtime();
    let engine::company::config::CompanySystemConfig::Simple(config) =
        setup.company_system.as_mut().unwrap()
    else {
        panic!("fixture 必须为 Simple")
    };
    config.prehistory_periods = 0;
    config.companies[0].generation.initial_revenue = AccountingAmount::MAX;
    let mut probe = ProtocolSession::new(setup.clone(), 7181).unwrap();
    probe
        .end_civil_day_update()
        .expect("真实月末 probe 的 June29 必须成功");
    let month_end_error = probe
        .end_civil_day_update()
        .expect_err("真实月末 probe 的 June30 必须溢出");
    assert!(
        matches!(&month_end_error, SessionError::CompanySystem(source) if source.classification() == engine::company::CompanyErrorClass::SystemState),
        "必须取得真实 Simple 月末系统错：{month_end_error}"
    );
    let expected_failure =
        serde_json::to_value(super::failure::HostFailure::civil(month_end_error)).unwrap();

    let game = ProtocolSession::new(setup, 7181).unwrap();
    let mut harness = correction_harness(game);
    harness.actor.pause_preferences = PausePreferences::default();
    harness.subscribe_engine_events();
    harness.subscribe_failures();
    let mut ordered = ordered_actor_events(&harness);
    harness.actor.run_cycle(0);
    assert_eq!(ordered.try_recv().unwrap(), crate::ENGINE_EVENT_NAME);
    assert_eq!(ordered.try_recv().unwrap(), "engine-failure");
    assert!(ordered.try_recv().is_err());
    let payload = harness.events_rx.as_mut().unwrap().try_recv().unwrap();
    let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
    let update: EngineUpdate = serde_json::from_value(payload["update"].clone()).unwrap();
    let EngineUpdate::CivilUpdate(civil) = update else {
        panic!("只能交付真实完成的 Closed 日")
    };
    assert_eq!(
        civil.boundary.settled_date,
        engine::CivilDate::from_iso("2030-06-29").unwrap()
    );
    let payload = harness.failures_rx.as_mut().unwrap().try_recv().unwrap();
    let failure: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(failure["recoverable"], false);
    assert_eq!(failure["code"], expected_failure["code"]);
    assert_eq!(failure["message"], expected_failure["message"]);
    assert_eq!(failure["cause"], expected_failure["cause"]);
    assert_eq!(
        harness.actor.game.civil_date(),
        engine::CivilDate::from_iso("2030-06-30").unwrap()
    );
    assert_eq!(harness.actor.game.tick(), 0);
}

#[tokio::test]
async fn rights_subscription_after_fatal_preserves_business_state() {
    let (mut game, mut request) = correction_game("2030-06-08");
    for line in &mut request.entries[0].lines {
        line.amount = AccountingAmount::MAX;
    }
    request.entries[0].lines[1].account =
        LedgerAccountId(engine::accounting::reports::simple_summary::REVENUE.into());
    let epoch = game.report_correction_epoch();
    game.enqueue_report_correction(&epoch, request).unwrap();
    assert!(game.end_civil_day_update().is_err());
    let first = game.require_healthy().unwrap_err();
    let before = game.business_state_hash().unwrap();
    let mut harness = correction_harness(game);
    let (reply, received) = oneshot::channel();
    harness
        .actor
        .handle_command(SessionCommand::SubscribeRightsOffering {
            generation: 1,
            event_id: "missing".into(),
            requested_shares: 1,
            reply,
        })
        .await;
    assert!(
        matches!(received.await.unwrap(), Err(CompanyWriteHostError::Fatal(actual)) if actual == first)
    );
    assert_eq!(harness.actor.game.business_state_hash().unwrap(), before);
}
