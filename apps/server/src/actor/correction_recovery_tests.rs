//! 更正拒绝后的真实 actor／协议恢复边界；依赖已审 E2 与显式命令转发组合。

use super::session_fixture as correction_fixture;
use super::*;
use engine::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, JournalLine,
    LedgerAccountId, PostingSide,
};
use engine::session::protocol::ReplayGuard;
use engine::session::CompanyReportCorrection;
include!("../../../../packages/engine/test-support/simple_company.rs");

fn correction_controller() -> OpaqueSubjectId {
    OpaqueSubjectId::new("b4e-correction-controller".into()).unwrap()
}

fn correction_game(start: &str) -> (ProtocolSession, CompanyReportCorrection) {
    let mut setup = correction_fixture::civil_setup(engine::CivilDate::from_iso(start).unwrap());
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
    let mut game = ProtocolSession::new(setup, 7181).unwrap();
    game.bind_market_creator(correction_controller()).unwrap();
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
        operation_id: "server-actor-correction-recovery".into(),
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

fn correction_actor(
    game: ProtocolSession,
) -> (
    SessionActor,
    mpsc::UnboundedSender<SessionCommand>,
    broadcast::Receiver<EngineUpdate>,
) {
    let (sender, cmd_rx) = mpsc::unbounded_channel();
    let (event_tx, receiver) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    let mut pacing = ServerPacing::new(1, game.tick());
    pacing.set_running(true, game.tick());
    let actor = SessionActor {
        ingress: Arc::new(RwLock::new((1, game.shared_ingress()))),
        members: Arc::new(RwLock::new(game.trading_subject_accounts())),
        controllers: std::collections::BTreeSet::from([correction_controller()]),
        archive: None,
        injected_step_failure: None,
        pacing,
        game,
        cmd_rx,
        event_tx,
        session_id: "correction-recovery".into(),
        public_revision: 0,
        timeline_generation: 1,
        pause_preferences: PausePreferences {
            pause_after_close: true,
            pause_before_open: true,
        },
        fatal_failure: None,
        deferred_drop_reported: false,
        consumer: None,
        fixed_batches: std::collections::VecDeque::new(),
        fixed_frames: Vec::new(),
    };
    (actor, sender, receiver)
}

fn assert_ingress_open_without_receiving_input(actor: &mut SessionActor) {
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
        .ingest(&ProtocolUpdate::TickBatch(Box::new(
            game.tick_batch(vec![first]).unwrap(),
        )))
        .unwrap();
    let retained_before = game.retained_market_days().len();
    let (mut actor, sender, mut receiver) = correction_actor(game);

    actor.run_protocol_batch(1);
    actor.collect_ticks(true).unwrap();

    let mut failure = None;
    let mut committed_ticks = Vec::new();
    while let Ok(message) = receiver.try_recv() {
        if let Some(update) = message.update {
            assert!(failure.is_none(), "必须先交付已提交末拍，再通知更正拒绝");
            let ProtocolUpdate::TickBatch(batch) = &update else {
                panic!("更正拒绝不能伪造 CivilUpdate")
            };
            committed_ticks.extend(batch.frames.iter().map(|frame| frame.tick));
            replay.ingest(&update).unwrap();
        }
        if let Some(actual) = message.failure {
            assert!(failure.replace(actual).is_none());
        }
    }
    let failure = failure.expect("真实更正业务拒绝必须通知用户");
    assert_eq!(failure.code, "REPORT_CORRECTION_PREPARE_REJECTED");
    assert_eq!(failure.r#where, "server.actor.prepare_civil_updates");
    assert_eq!(
        failure.cause.as_ref().unwrap().code,
        "REPORT_CORRECTION_REJECTED"
    );
    assert!(failure.recoverable, "健康更正拒绝必须保留取消与重试入口");
    assert!(!serde_json::to_string(&failure)
        .unwrap()
        .contains(&request.reason));
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
    assert_eq!(actor.public_revision, 0);
    assert!(actor.fatal_failure.is_none());
    assert!(!actor.pacing.is_running());
    assert!(!sender.is_closed());
    assert_ingress_open_without_receiving_input(&mut actor);
    let (reply, received) = oneshot::channel();
    actor
        .handle_command(SessionCommand::Snapshot { reply })
        .await;
    assert_eq!(received.await.unwrap().tick, 2);

    actor
        .handle_member_request(
            correction_controller(),
            MemberRequest::CancelReportCorrection {
                generation: 1,
                operation_id: request.operation_id.clone(),
            },
        )
        .unwrap();
    assert!(actor.game.report_corrections().unwrap().pending.is_empty());
    actor
        .handle_member_request(
            correction_controller(),
            MemberRequest::EnqueueReportCorrection {
                generation: 1,
                request: request.clone(),
            },
        )
        .unwrap();
    let (reply, received) = oneshot::channel();
    actor
        .handle_command(SessionCommand::SetRunning {
            running: true,
            reply,
        })
        .await;
    received.await.unwrap().unwrap();
    assert!(actor.pacing.is_running());
    actor.run_protocol_batch(0);
    let message = receiver.try_recv().unwrap();
    assert!(message.failure.is_none());
    let update = message.update.unwrap();
    let ProtocolUpdate::CivilUpdate(civil) = &update else {
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
    assert!(receiver.try_recv().is_err());
    assert_eq!(actor.game.tick(), 2);
    assert_eq!(actor.game.retained_market_days().len(), retained_before + 1);
    let status = actor.game.report_corrections().unwrap();
    assert!(status.pending.is_empty());
    assert_eq!(status.completed[&request.operation_id].request, request);
    assert_eq!(actor.public_revision, 1);
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
    let (mut actor, sender, mut receiver) = correction_actor(game);
    actor.run_protocol_batch(0);
    let message = receiver.try_recv().unwrap();
    assert!(message.update.is_none());
    let failure = message.failure.unwrap();
    assert_eq!(
        failure.cause.as_ref().unwrap().code,
        "REPORT_CORRECTION_REJECTED"
    );
    assert!(failure.recoverable);
    assert_eq!(actor.game.business_state_hash().unwrap(), before);
    assert!(actor.game.require_healthy().is_ok());
    assert!(actor.fatal_failure.is_none());
    assert!(!sender.is_closed());
    assert_ingress_open_without_receiving_input(&mut actor);
    let (reply, received) = oneshot::channel();
    actor
        .handle_command(SessionCommand::Snapshot { reply })
        .await;
    assert_eq!(received.await.unwrap().tick, 0);
    actor
        .handle_member_request(
            correction_controller(),
            MemberRequest::CancelReportCorrection {
                generation: 1,
                operation_id: request.operation_id.clone(),
            },
        )
        .unwrap();
    assert!(actor.game.report_corrections().unwrap().pending.is_empty());
    assert!(actor.game.game().save().is_ok());
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
    let (mut actor, _sender, mut receiver) = correction_actor(game);
    actor.run_protocol_batch(0);
    let message = receiver.try_recv().unwrap();
    assert!(
        message.update.is_none(),
        "零 tick 致命拒绝不得发布 CivilUpdate"
    );
    let failure = message.failure.unwrap();
    assert_eq!(
        failure.cause.as_ref().unwrap().code,
        "REPORT_CORRECTION_FAILED"
    );
    assert!(!failure.recoverable);
    let mut cause = failure.cause.as_deref();
    let mut has_accounting_cause = false;
    while let Some(current) = cause {
        has_accounting_cause |= current.code == "ACCOUNTING_VALIDATION_FAILED";
        cause = current.cause.as_deref();
    }
    assert!(has_accounting_cause);
    assert!(receiver.try_recv().is_err(), "一个失败只能通知一次");
    assert!(actor.fatal_failure.is_some());
    let first = actor
        .game
        .require_healthy()
        .expect_err("fatal 不能被批前 checkpoint 复活");
    assert_eq!(actor.game.poison_reason(), Some(&first));
    assert_eq!(actor.game.step_frame().unwrap_err(), first);
    assert!(actor.fatal_rejection().is_some());
    assert!(!actor.pacing.is_running());
    let before_rejected_ingress = actor.game.business_state_hash().unwrap();
    assert!(matches!(
        actor.game.shared_ingress().enqueue_player_intent(
            AccountId(0),
            Intent::Cancel { code: engine::StockId(0), id: engine::OrderId(1) }
        ),
        Err(SessionError::Step(actual)) if actual == first
    ));
    assert_eq!(
        actor.game.business_state_hash().unwrap(),
        before_rejected_ingress
    );
}

async fn dispatch(
    actor: &mut SessionActor,
    sender: &mpsc::UnboundedSender<SessionCommand>,
    subject: OpaqueSubjectId,
    request: MemberRequest,
) -> Result<MemberResponse, SendCommandError> {
    let (reply, received) = oneshot::channel();
    sender
        .send(SessionCommand::Member {
            subject,
            request,
            reply,
        })
        .unwrap();
    let command = actor.cmd_rx.try_recv().unwrap();
    actor.handle_command(command).await;
    received.await.unwrap()
}

#[tokio::test]
async fn company_writes_after_fatal_preserve_first_failure_and_stale_generation() {
    let (mut game, mut request) = correction_game("2030-06-08");
    for line in &mut request.entries[0].lines {
        line.amount = AccountingAmount::MAX;
    }
    request.entries[0].lines[1].account =
        LedgerAccountId(engine::accounting::reports::simple_summary::REVENUE.into());
    request.entries[0].validate_invariants().unwrap();

    let epoch = game.report_correction_epoch();
    game.enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    let (mut actor, sender, mut receiver) = correction_actor(game);
    actor.run_protocol_batch(0);

    let message = receiver.try_recv().unwrap();
    assert!(message.update.is_none(), "致命更正失败不得发布 CivilUpdate");
    let first_failure = message.failure.expect("致命更正失败必须广播首错");
    assert_eq!(first_failure.code, "CIVIL_DAY_SETTLEMENT_FAILED");
    assert_eq!(
        first_failure.cause.as_ref().unwrap().code,
        "REPORT_CORRECTION_FAILED"
    );
    assert!(!first_failure.recoverable);
    assert_eq!(actor.fatal_failure.as_ref(), Some(&first_failure));
    // Host 和 engine 都须保留真实首次 fatal，不能借批前 checkpoint 复活 authority。
    let first_poison = actor.game.poison_reason().cloned();
    assert!(
        first_poison.is_some(),
        "真实 civil fatal 必须保留 engine poison"
    );
    let first_health = actor.game.require_healthy();
    assert_eq!(
        actor
            .game
            .market_membership(&correction_controller())
            .unwrap()
            .account_id,
        AccountId(0)
    );
    let first_business = actor.game.business_state_hash().unwrap();
    assert!(matches!(
        actor.game.shared_ingress().enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: engine::StockId(0),
                id: engine::OrderId(1),
            }
        ),
        Err(SessionError::Step(actual)) if Some(&actual) == first_poison.as_ref()
    ));
    assert_eq!(actor.game.business_state_hash().unwrap(), first_business);

    // 真实HTTP分别先发送Control／Resolve授权探针，必须能到内层真实鉴权。
    let control = dispatch(
        &mut actor,
        &sender,
        correction_controller(),
        MemberRequest::Control,
    )
    .await
    .unwrap_or_else(|error| {
        panic!("COMPANY_WRITE_RETAINED_FIRST_FATAL：已授权Control探针被外层终态门错误拒绝：{error}")
    });
    assert!(matches!(control, MemberResponse::Control));
    let resolved = dispatch(
        &mut actor,
        &sender,
        correction_controller(),
        MemberRequest::Resolve,
    )
    .await
    .unwrap();
    assert!(
        matches!(resolved, MemberResponse::Membership(member) if member.account_id == AccountId(0))
    );
    let outsider = OpaqueSubjectId::new("unrelated-fatal-observer".into()).unwrap();
    for (probe, reason) in [
        (MemberRequest::Control, "没有当前市场控制能力"),
        (MemberRequest::Resolve, "没有该主体的成员关系"),
    ] {
        assert!(
            matches!(dispatch(&mut actor, &sender, outsider.clone(), probe).await,
            Err(SendCommandError::Rejected(actual)) if actual.contains(reason))
        );
    }
    // 五个必要只读／公司写例外不能移除其他Member请求原有的终态门。
    assert!(matches!(
        dispatch(
            &mut actor,
            &sender,
            correction_controller(),
            MemberRequest::Snapshot
        )
        .await,
        Err(SendCommandError::Rejected(_))
    ));
    assert_eq!(actor.fatal_failure.as_ref(), Some(&first_failure));
    assert_eq!(actor.game.business_state_hash().unwrap(), first_business);
    assert!(receiver.try_recv().is_err());

    let proposal = MemberRequest::ProposePlayerCompanyAction {
        generation: 1,
        proposal: engine::session::PlayerCompanyProposal::CashDividend {
            company: engine::company::CompanyId(0),
            gross_per_share: engine::Money::from_cents(1_000),
        },
    };
    let enqueue = MemberRequest::EnqueueReportCorrection {
        generation: 1,
        request: request.clone(),
    };
    let cancel = MemberRequest::CancelReportCorrection {
        generation: 1,
        operation_id: request.operation_id.clone(),
    };

    for (name, current, stale) in [
        (
            "proposal",
            proposal,
            MemberRequest::ProposePlayerCompanyAction {
                generation: 0,
                proposal: engine::session::PlayerCompanyProposal::CashDividend {
                    company: engine::company::CompanyId(0),
                    gross_per_share: engine::Money::from_cents(1_000),
                },
            },
        ),
        (
            "enqueue",
            enqueue,
            MemberRequest::EnqueueReportCorrection {
                generation: 0,
                request: request.clone(),
            },
        ),
        (
            "cancel",
            cancel,
            MemberRequest::CancelReportCorrection {
                generation: 0,
                operation_id: request.operation_id.clone(),
            },
        ),
    ] {
        let response = dispatch(&mut actor, &sender, correction_controller(), current)
            .await
            .unwrap_or_else(|error| panic!("当前世代 {name} 写请求应保留首个fatal：{error}"));
        match response {
            MemberResponse::CompanyWriteFatal(actual) => {
                assert_eq!(actual, first_failure, "{name} 必须保留完整广播首错");
            }
            _ => panic!("当前世代 {name} 写请求未返回 CompanyWriteFatal"),
        }
        assert_eq!(actor.fatal_failure.as_ref(), Some(&first_failure));
        assert_eq!(actor.game.poison_reason(), first_poison.as_ref());
        assert_eq!(actor.game.require_healthy(), first_health);
        assert_eq!(actor.game.business_state_hash().unwrap(), first_business);
        assert!(
            receiver.try_recv().is_err(),
            "{name} 请求不得重复广播失败通知"
        );

        assert!(
            matches!(dispatch(&mut actor, &sender, correction_controller(), stale).await,
            Err(SendCommandError::Rejected(message)) if message.contains("STALE_SESSION_GENERATION"))
        );
        assert_eq!(actor.fatal_failure.as_ref(), Some(&first_failure));
        assert_eq!(actor.game.poison_reason(), first_poison.as_ref());
        assert_eq!(actor.game.business_state_hash().unwrap(), first_business);
        assert!(
            receiver.try_recv().is_err(),
            "过期世代 {name} 请求不得广播失败通知"
        );
    }
    for (request, reason) in [
        (
            MemberRequest::ProposePlayerCompanyAction {
                generation: 1,
                proposal: engine::session::PlayerCompanyProposal::CashDividend {
                    company: engine::company::CompanyId(0),
                    gross_per_share: engine::Money::from_cents(1_000),
                },
            },
            "没有该主体的成员关系",
        ),
        (
            MemberRequest::EnqueueReportCorrection {
                generation: 1,
                request: request.clone(),
            },
            "没有当前市场控制能力",
        ),
        (
            MemberRequest::CancelReportCorrection {
                generation: 1,
                operation_id: request.operation_id.clone(),
            },
            "没有当前市场控制能力",
        ),
    ] {
        assert!(
            matches!(dispatch(&mut actor, &sender, outsider.clone(), request).await,
            Err(SendCommandError::Rejected(actual)) if actual.contains(reason))
        );
        assert_eq!(actor.fatal_failure.as_ref(), Some(&first_failure));
        assert_eq!(actor.game.poison_reason(), first_poison.as_ref());
        assert_eq!(actor.game.business_state_hash().unwrap(), first_business);
        assert!(receiver.try_recv().is_err(), "未授权请求不得重发首错");
    }
}

#[test]
fn generic_civil_failure_cannot_claim_the_prepare_recovery_contract() {
    let (mut game, mut request) = correction_game("2030-06-08");
    request.entries[0].lines[1].account = LedgerAccountId("不存在的科目".into());
    let epoch = game.report_correction_epoch();
    game.enqueue_report_correction(&epoch, request).unwrap();
    let error = game.end_civil_day_update().unwrap_err();
    assert!(game.require_healthy().is_ok());
    let failure = HostFailure::civil(&error);
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
        failure_description(&error),
        (
            "SAVE_TARGET_DAY_NOT_READY",
            "目标自然日尚未完成日终，当前日存档不可用（原始详情已脱敏）"
        )
    );
    for code in [3015, 3016, 3017] {
        assert_eq!(
            failure_description(&SessionError::InvalidSave(engine::ValidationFault::bare(
                code
            )))
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
    let (mut actor, _sender, mut receiver) = correction_actor(game);
    actor.run_protocol_batch(1);
    actor.collect_ticks(true).unwrap();
    let update = receiver
        .try_recv()
        .unwrap()
        .update
        .expect("已打包成功末拍须在 fatal 前交付");
    let ProtocolUpdate::TickBatch(batch) = update else {
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
    let failure = receiver.try_recv().unwrap().failure.unwrap();
    assert!(!failure.recoverable);
    assert_eq!(actor.game.tick(), 2, "installed close 不得复活批前 mirror");
    assert_eq!(actor.game.retained_market_days().len(), 1);
    let origin = actor.game.runtime_mirror_origin();
    assert_eq!(origin.source_tick, 2);
    assert!(origin.authority_epoch > 0 && origin.install_sequence > 0);
    assert_eq!(
        origin.day_end_stage,
        engine::RuntimeMirrorDayEndStage::RetainedPublished
    );
    let first = actor
        .game
        .require_healthy()
        .expect_err("post-install civil fatal 必须保 poison");
    assert_eq!(actor.game.step_frame().unwrap_err(), first);
    assert_eq!(
        actor.game.save().unwrap_err().to_string(),
        SessionError::Step(first).to_string()
    );
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn target_day_not_ready_uses_the_actual_save_error_conversion() {
    let (game, _) = correction_game("2030-06-07");
    let setup = game.game().save().unwrap().setup;
    let error = game.save_current_day(game.civil_date()).unwrap_err();
    let (cmd_tx, mut commands) = mpsc::unbounded_channel();
    let (event_tx, _) = broadcast::channel(1);
    let handles = SessionHandles {
        cmd_tx,
        ingress: Arc::new(RwLock::new((1, game.shared_ingress()))),
        members: Arc::new(RwLock::new(game.trading_subject_accounts())),
        event_tx,
        ticks_per_day: 2,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        session_token: "private-test-token".into(),
        startup_setup: setup,
        startup_seed: 7181,
        startup_resumed: false,
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

// 只供接收端状态机测试使用：真实 typed cause 来自 engine，wire 身份按 publisher fixture 显式绑定。
impl HostFailure {
    pub(crate) fn correction_prepare_rejection_fixture() -> Self {
        let (mut game, mut request) = correction_game("2030-06-08");
        request.entries[0].lines[1].account = LedgerAccountId("不存在的科目".into());
        let epoch = game.report_correction_epoch();
        game.enqueue_report_correction(&epoch, request).unwrap();
        let error = game.end_civil_day_update().unwrap_err();
        assert!(game.require_healthy().is_ok());
        let mut failure = HostFailure::civil(&error);
        failure.code = "REPORT_CORRECTION_PREPARE_REJECTED";
        failure.r#where = "server.actor.prepare_civil_updates".into();
        failure.recoverable = true;
        failure.context.operation = "endCivilDay";
        failure.context.tick = Some(6);
        failure.context.seq = Some(4);
        failure.context.day = Some(1);
        failure.context.generation = Some("7".into());
        assert_eq!(
            failure.cause.as_ref().unwrap().code,
            "REPORT_CORRECTION_REJECTED"
        );
        failure
    }
}

fn poisoned_actor_for_command_boundaries() -> (SessionActor, engine::session::StepFatal) {
    let (mut game, mut request) = correction_game("2030-06-08");
    for line in &mut request.entries[0].lines {
        line.amount = AccountingAmount::MAX;
    }
    request.entries[0].lines[1].account =
        LedgerAccountId(engine::accounting::reports::simple_summary::REVENUE.into());
    let epoch = game.report_correction_epoch();
    game.enqueue_report_correction(&epoch, request).unwrap();
    let error = game.end_civil_day_update().unwrap_err();
    let first = game.require_healthy().unwrap_err();
    let (mut actor, _sender, _receiver) = correction_actor(game);
    actor.stop_with_failure(HostFailure::civil(&error));
    (actor, first)
}

#[tokio::test]
async fn actual_save_after_engine_fatal_preserves_first_step_after_generation_validation() {
    let (mut actor, first) = poisoned_actor_for_command_boundaries();
    for generation in [2, 1] {
        let (reply, response) = oneshot::channel();
        actor
            .handle_command(SessionCommand::Save {
                generation,
                candidate: None,
                reply,
            })
            .await;
        let error = response.await.unwrap().unwrap_err();
        if generation == 2 {
            assert!(matches!(error, SessionError::InvalidSave(fault) if fault.code == 1290));
        } else {
            assert!(matches!(error, SessionError::Step(actual) if actual == first));
        }
    }
}

#[tokio::test]
async fn normal_reads_after_engine_fatal_preserve_generation_then_first_step() {
    let (mut actor, first) = poisoned_actor_for_command_boundaries();
    for generation in [2, 1] {
        let (reply, response) = oneshot::channel();
        actor
            .handle_command(SessionCommand::PlayerWorkingOrders { generation, reply })
            .await;
        let orders = response
            .await
            .unwrap()
            .expect_err("正常委托读不能把 terminal 状态作为成功响应");
        let (reply, response) = oneshot::channel();
        actor
            .handle_command(SessionCommand::StockHistory {
                generation,
                code: engine::StockId(0),
                reply,
            })
            .await;
        let history = response
            .await
            .unwrap()
            .expect_err("正常历史读不能把 terminal 状态作为成功响应");
        let (reply, response) = oneshot::channel();
        actor
            .handle_command(SessionCommand::InitialAllocation { generation, reply })
            .await;
        let allocation = response
            .await
            .unwrap()
            .expect_err("正常初始分配读不能把 terminal 状态作为成功响应");
        for error in [orders, history, allocation] {
            let SendCommandError::Rejected(message) = error else {
                panic!("必须沿既有拒绝容器返回")
            };
            if generation == 2 {
                assert!(message.contains("fault[1290]"));
            } else {
                assert_eq!(message, SessionError::Step(first.clone()).to_string());
            }
        }
    }
}

#[test]
fn civil_prefix_is_published_and_archived_before_the_next_real_month_end_failure() {
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
    let expected_failure = HostFailure::civil(&month_end_error);

    let mut game = ProtocolSession::new(setup, 7181).unwrap();
    game.bind_market_creator(correction_controller()).unwrap();
    let (mut actor, _sender, mut receiver) = correction_actor(game);
    actor.pause_preferences = PausePreferences::default();
    let database = native_store::NativeDatabase::open_in_memory().unwrap();
    actor.archive = Some(
        native_store::ArchiveWriter::activate(database.clone(), "prefix-before-fatal").unwrap(),
    );
    actor.run_protocol_batch(0);
    let first = receiver.try_recv().unwrap();
    let Some(ProtocolUpdate::CivilUpdate(civil)) = first.update else {
        panic!("首个成功 Closed 日必须在下一日真实失败前交付")
    };
    assert!(first.failure.is_none());
    assert_eq!(
        civil.boundary.settled_date,
        engine::CivilDate::from_iso("2030-06-29").unwrap()
    );
    let failure = receiver
        .try_recv()
        .unwrap()
        .failure
        .expect("月末真实溢出必须显式失败");
    assert!(!failure.recoverable);
    assert_eq!(failure.code, expected_failure.code);
    assert_eq!(failure.message, expected_failure.message);
    assert_eq!(failure.cause, expected_failure.cause);
    assert_ne!(
        failure.code, "DAY_END_ARCHIVE_FAILED",
        "不得在 poison 后才捕获已成功候选，冒充额外保存失败"
    );
    assert!(receiver.try_recv().is_err());
    let saved = database
        .load("prefix-before-fatal")
        .unwrap()
        .expect("已成功日终候选必须真实写入");
    assert_eq!(
        saved.civil_clock.settled_through,
        Some(civil.boundary.settled_date)
    );
    assert_eq!(saved.snapshot.seq, civil.seq_to);
    assert_eq!(
        actor.game.civil_date(),
        engine::CivilDate::from_iso("2030-06-30").unwrap()
    );
    assert_eq!(actor.game.tick(), 0);
}

#[test]
fn archive_write_failure_keeps_the_previous_candidate_and_the_committed_civil_update() {
    let (game, _) = correction_game("2030-06-09");
    let (mut actor, _sender, mut receiver) = correction_actor(game);
    let database = native_store::NativeDatabase::open_in_memory().unwrap();
    actor.archive = Some(
        native_store::ArchiveWriter::activate(database.clone(), "archive-failure-control").unwrap(),
    );
    actor.run_protocol_batch(0);
    let previous = database.load("archive-failure-control").unwrap().unwrap();
    assert!(receiver.try_recv().unwrap().failure.is_none());
    database
        .with_connection(|connection| {
            connection.execute_batch("PRAGMA query_only = ON")?;
            Ok(())
        })
        .unwrap();
    actor.run_protocol_batch(1);
    actor.collect_ticks(true).unwrap();
    actor.run_protocol_batch(1);
    actor.collect_ticks(true).unwrap();
    let mut failure = None;
    let mut civil = None;
    while let Ok(message) = receiver.try_recv() {
        if let Some(actual) = message.failure {
            assert!(failure.replace(actual).is_none());
        }
        if let Some(ProtocolUpdate::CivilUpdate(actual)) = message.update {
            assert!(civil.replace(actual).is_none());
        }
    }
    let failure = failure.expect("真实 SQLite 写入失败必须显式通知");
    assert_eq!(failure.code, "DAY_END_ARCHIVE_FAILED");
    assert!(failure.recoverable);
    let civil = civil.expect("持久化失败不能撤销已完成 CivilUpdate");
    assert_eq!(
        civil.boundary.settled_date,
        engine::CivilDate::from_iso("2030-06-10").unwrap()
    );
    assert!(actor.fatal_failure.is_none());
    assert!(actor.game.require_healthy().is_ok());
    assert_eq!(
        serde_json::to_value(database.load("archive-failure-control").unwrap().unwrap()).unwrap(),
        serde_json::to_value(previous).unwrap()
    );
    let key = engine::session::protocol::SaveCandidateKey {
        seq: civil.seq_to,
        settled_date: civil.boundary.settled_date,
    };
    let exact = actor.game.save_candidate(&key).unwrap();
    assert_eq!(
        exact.civil_clock.settled_through,
        Some(civil.boundary.settled_date)
    );
    assert_eq!(exact.snapshot.seq, civil.seq_to);
}

#[tokio::test]
async fn rights_subscription_requires_member_and_current_generation() {
    let (game, _) = correction_game("2030-06-03");
    let (mut actor, sender, _receiver) = correction_actor(game);
    let request = || MemberRequest::SubscribeRightsOffering {
        generation: 1,
        event_id: "missing".into(),
        requested_shares: 1,
    };
    let outsider = OpaqueSubjectId::new("rights-outsider".into()).unwrap();
    assert!(matches!(
        dispatch(&mut actor, &sender, outsider, request()).await,
        Err(SendCommandError::Rejected(_))
    ));
    assert!(matches!(
        dispatch(&mut actor, &sender, correction_controller(), request()).await,
        Ok(MemberResponse::RightsSubscription(1, Err(_)))
    ));
    assert!(
        matches!(dispatch(&mut actor, &sender, correction_controller(), MemberRequest::SubscribeRightsOffering {
        generation: 0, event_id: "missing".into(), requested_shares: 1,
    }).await, Err(SendCommandError::Rejected(reason)) if reason == "STALE_SESSION_GENERATION")
    );
}

fn rights_member_game() -> (ProtocolSession, String) {
    use engine::company::rights_offering::{
        RightsOfferingBook, RightsOfferingDeclaration, RightsOfferingEventPlan, RightsOfferingMode,
        RightsSubscriptionStrategy,
    };
    use engine::company::share_registry::{HolderId, ShareRegistry};
    use engine::session::SavedSessionCompanies;
    let mut setup =
        correction_fixture::civil_setup(engine::CivilDate::from_iso("2030-06-03").unwrap());
    setup.ticks_per_day = 1;
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.strategy_params.retail.arrival_rate = 0.0;
    setup.stocks[0].total_shares = 1_200;
    setup.stocks[0].float_shares = 1_200;
    setup.history_len = 1;
    setup.company_system = Some(simple_company_fixture!(engine; ["600101"]));
    setup.dividend_tax_mode = Some(engine::company::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1_000);
    setup.par_value_per_share = Some(engine::Money::from_cents(100));
    setup.auto_corporate_foundation = true;
    setup.rights_offering_enabled = true;
    let mut initial = engine::GameSession::new(setup, 7181).unwrap();
    initial
        .bind_market_creator(correction_controller())
        .unwrap();
    let subject = OpaqueSubjectId::new("rights-second-member".into()).unwrap();
    let member = initial.join_market(subject, false).unwrap();
    assert_ne!(member.account_id, AccountId(0));
    let mut saved = initial.save().unwrap();
    let npc_position = saved
        .snapshot
        .accounts
        .get_mut(&AccountId(1))
        .unwrap()
        .positions
        .get_mut(&engine::StockId(0))
        .unwrap();
    let mut player_position = npc_position.clone();
    player_position.qty = 100;
    player_position.t1_locked = 0;
    player_position.invested_cents = 0;
    player_position.recovered_cents = 0;
    npc_position.qty -= 200;
    saved
        .snapshot
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .positions
        .insert(engine::StockId(0), player_position.clone());
    saved
        .snapshot
        .accounts
        .get_mut(&member.account_id)
        .unwrap()
        .positions
        .insert(engine::StockId(0), player_position);
    let SavedSessionCompanies::CompanySystem {
        system,
        corporate_actions,
    } = &mut saved.company_state
    else {
        panic!("需要公司系统")
    };
    let old = &corporate_actions.registries[0];
    let mut holdings = old.holdings().to_vec();
    let npc = holdings
        .iter_mut()
        .find(|holding| holding.holder == HolderId::Account(AccountId(1)))
        .unwrap();
    let mut player = npc.clone();
    npc.lots[0].qty -= 200;
    player.holder = HolderId::Account(AccountId(0));
    player.lots[0].qty = 100;
    player.lots[0].id = "host-player-initial".into();
    let mut second = player.clone();
    second.holder = HolderId::Account(member.account_id);
    second.lots[0].id = "host-second-player-initial".into();
    holdings.push(player);
    holdings.push(second);
    corporate_actions.registries[0] = ShareRegistry::new(
        old.stock().clone(),
        *old.issuer(),
        old.issued_shares(),
        old.settled_on(),
        holdings,
    )
    .unwrap();
    let company = engine::company::CompanyId(0);
    let capital = system
        .dividend_legal_facts(&company)
        .unwrap()
        .unwrap()
        .registered_capital;
    let day = initial.civil_date();
    let mut plan = RightsOfferingEventPlan {
        event_id: "host-rights".into(),
        approval_reference: "宿主测试批准".into(),
        issuer: company,
        stock: engine::StockCode("600101".into()),
        exchange: engine::calendar::CalendarExchange::Sse,
        approved_on: day,
        announced_on: day,
        registered_on: day,
        payment_start_on: day,
        payment_deadline_on: day,
        ex_rights_on: day,
        settlement_on: day,
        listing_on: day,
        price_per_share: engine::Money::from_cents(100),
        mode: RightsOfferingMode::RightsToAllShareholders {
            shares_per_existing_share_micros: 100_000,
        },
        npc_subscription_strategy: RightsSubscriptionStrategy::FullByDefault,
    };
    plan.derive_schedule(
        &engine::calendar::TradingCalendar::current_default_calendar().unwrap(),
        plan.exchange,
        1,
    )
    .unwrap();
    system
        .declare_rights_offering(
            &company,
            RightsOfferingDeclaration {
                event_id: plan.event_id.clone(),
                approval_reference: plan.approval_reference.clone(),
                approved_on: day,
                price_per_share: plan.price_per_share,
                par_value_per_share: engine::Money::from_cents(100),
                registered_capital_at_approval: capital,
            },
        )
        .unwrap();
    corporate_actions
        .rights_offerings
        .push(RightsOfferingBook::new(plan.clone()).unwrap());
    let mut game = engine::GameSession::restore(&saved).unwrap();
    game.step().unwrap();
    game.end_civil_day().unwrap();
    let saved = game.save().unwrap();
    (ProtocolSession::restore(&saved).unwrap(), plan.event_id)
}

#[tokio::test]
async fn rights_member_success_is_queued_once_after_waiter_cancellation() {
    let (game, event_id) = rights_member_game();
    let (mut actor, sender, _) = correction_actor(game);
    let request = || MemberRequest::SubscribeRightsOffering {
        generation: 1,
        event_id: event_id.clone(),
        requested_shares: 1,
    };
    let (reply, received) = oneshot::channel();
    drop(received);
    sender
        .send(SessionCommand::Member {
            subject: correction_controller(),
            request: request(),
            reply,
        })
        .unwrap();
    let command = actor.cmd_rx.try_recv().unwrap();
    actor.handle_command(command).await;
    let actions = actor.game.game().corporate_actions().unwrap();
    assert_eq!(actions.rights_subscription_queue.len(), 1);
    assert_eq!(actions.rights_subscription_queue[0].account, AccountId(0));
    assert!(matches!(
        dispatch(&mut actor, &sender, correction_controller(), request()).await,
        Ok(MemberResponse::RightsSubscription(
            1,
            Err(
                engine::session::protocol::CompanyWriteError::RightsSubscription {
                    source: engine::session::RightsSubscriptionError::Duplicate { .. },
                    ..
                }
            )
        ))
    ));
    assert_eq!(
        actor
            .game
            .game()
            .corporate_actions()
            .unwrap()
            .rights_subscription_queue
            .len(),
        1
    );
}

#[tokio::test]
async fn rights_member_success_receipt_uses_trusted_identity_and_decimal_shares() {
    let (game, event_id) = rights_member_game();
    let (mut actor, sender, _) = correction_actor(game);
    let response = dispatch(
        &mut actor,
        &sender,
        correction_controller(),
        MemberRequest::SubscribeRightsOffering {
            generation: 1,
            event_id: event_id.clone(),
            requested_shares: 2,
        },
    )
    .await
    .unwrap();
    let MemberResponse::RightsSubscription(generation, Ok(receipt)) = response else {
        panic!("本人认购应成功")
    };
    assert_eq!(generation, 1);
    let wire = serde_json::to_value(receipt).unwrap();
    assert_eq!(wire["event_id"], event_id);
    assert_eq!(wire["requested_shares"], "2");
    assert_eq!(wire["submitted_on"], "2030-06-04");
    let queue = &actor
        .game
        .game()
        .corporate_actions()
        .unwrap()
        .rights_subscription_queue;
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].account, AccountId(0));
}

#[tokio::test]
async fn rights_second_member_success_is_attributed_to_member_account() {
    let (game, event_id) = rights_member_game();
    let subject = OpaqueSubjectId::new("rights-second-member".into()).unwrap();
    let expected = game.market_membership(&subject).unwrap().account_id;
    assert_ne!(expected, AccountId(0));
    let (mut actor, sender, _) = correction_actor(game);
    let result = dispatch(
        &mut actor,
        &sender,
        subject,
        MemberRequest::SubscribeRightsOffering {
            generation: 1,
            event_id,
            requested_shares: 2,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        result,
        MemberResponse::RightsSubscription(1, Ok(_))
    ));
    let queue = &actor
        .game
        .game()
        .corporate_actions()
        .unwrap()
        .rights_subscription_queue;
    assert_eq!(queue.len(), 1);
    assert_eq!(queue[0].account, expected);
}
