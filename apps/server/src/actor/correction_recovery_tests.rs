//! 更正拒绝后的真实 actor／协议恢复边界；依赖已审 E2 与显式命令转发组合。

use super::*;
use engine::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, JournalLine,
    LedgerAccountId, PostingSide,
};
use engine::session::protocol::ReplayGuard;
use engine::session::CompanyReportCorrection;
include!("../../../../packages/engine/test-support/simple_company.rs");
#[path = "../../../../packages/engine/tests/publications/session_fixture.rs"]
mod correction_fixture;

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
    request.entries[0].lines[1].account = LedgerAccountId(engine::accounting::reports::simple_summary::REVENUE.into());
    request.entries[0].validate_invariants().unwrap();
    // BatchAborted 不公开 Error::source；用相同真实输入预检准确 leaf，不伪称 wire 已穿透。
    let (mut probe, mut probe_request) = correction_game("2030-06-08");
    for line in &mut probe_request.entries[0].lines {
        line.amount = AccountingAmount::MAX;
    }
    probe_request.entries[0].lines[1].account = LedgerAccountId(engine::accounting::reports::simple_summary::REVENUE.into());
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
    assert!(actor.fatal_rejection().is_some());
    assert!(!actor.pacing.is_running());
    assert!(matches!(
        actor.game.shared_ingress().enqueue_player_intent(
            AccountId(0),
            Intent::Cancel { code: engine::StockId(0), id: engine::OrderId(1) }
        ),
        Err(SessionError::ResourceLimit(fault)) if fault.code == 1251
    ));
}

#[tokio::test]
async fn company_writes_after_fatal_preserve_first_failure_and_stale_generation() {
    async fn dispatch(
        actor: &mut SessionActor,
        sender: &mpsc::UnboundedSender<SessionCommand>,
        subject: OpaqueSubjectId,
        request: MemberRequest,
    ) -> Result<MemberResponse, SendCommandError> {
        let (reply, received) = oneshot::channel();
        sender.send(SessionCommand::Member { subject, request, reply }).unwrap();
        let command = actor.cmd_rx.try_recv().unwrap();
        actor.handle_command(command).await;
        received.await.unwrap()
    }

    let (mut game, mut request) = correction_game("2030-06-08");
    for line in &mut request.entries[0].lines {
        line.amount = AccountingAmount::MAX;
    }
    request.entries[0].lines[1].account = LedgerAccountId(
        engine::accounting::reports::simple_summary::REVENUE.into(),
    );
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
    assert_eq!(first_failure.cause.as_ref().unwrap().code, "REPORT_CORRECTION_FAILED");
    assert!(!first_failure.recoverable);
    assert_eq!(actor.fatal_failure.as_ref(), Some(&first_failure));
    // actor 已回滚批次 checkpoint；GameSession 可恢复为健康，Host terminal 仍不可绕过。
    let first_poison = actor.game.poison_reason().cloned();
    let first_health = actor.game.require_healthy();
    assert_eq!(actor.game.market_membership(&correction_controller()).unwrap().account_id, AccountId(0));
    let first_business = actor.game.business_state_hash().unwrap();
    assert!(matches!(
        actor.game.shared_ingress().enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: engine::StockId(0),
                id: engine::OrderId(1),
            }
        ),
        Err(SessionError::ResourceLimit(fault)) if fault.code == 1251
    ));

    // 真实HTTP分别先发送Control／Resolve授权探针，必须能到内层真实鉴权。
    let control = dispatch(&mut actor, &sender, correction_controller(), MemberRequest::Control)
        .await.unwrap_or_else(|error| panic!("COMPANY_WRITE_RETAINED_FIRST_FATAL：已授权Control探针被外层终态门错误拒绝：{error}"));
    assert!(matches!(control, MemberResponse::Control));
    let resolved = dispatch(&mut actor, &sender, correction_controller(), MemberRequest::Resolve)
        .await.unwrap();
    assert!(matches!(resolved, MemberResponse::Membership(member) if member.account_id == AccountId(0)));
    let outsider = OpaqueSubjectId::new("unrelated-fatal-observer".into()).unwrap();
    for (probe, reason) in [
        (MemberRequest::Control, "没有当前市场控制能力"),
        (MemberRequest::Resolve, "没有该主体的成员关系"),
    ] {
        assert!(matches!(dispatch(&mut actor, &sender, outsider.clone(), probe).await,
            Err(SendCommandError::Rejected(actual)) if actual.contains(reason)));
    }
    // 五个必要只读／公司写例外不能移除其他Member请求原有的终态门。
    assert!(matches!(dispatch(&mut actor, &sender, correction_controller(), MemberRequest::Snapshot).await,
        Err(SendCommandError::Rejected(_))));
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
        let response = dispatch(&mut actor, &sender, correction_controller(), current).await
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

        assert!(matches!(dispatch(&mut actor, &sender, correction_controller(), stale).await,
            Err(SendCommandError::Rejected(message)) if message.contains("STALE_SESSION_GENERATION")));
        assert_eq!(actor.fatal_failure.as_ref(), Some(&first_failure));
        assert_eq!(actor.game.poison_reason(), first_poison.as_ref());
        assert_eq!(actor.game.business_state_hash().unwrap(), first_business);
        assert!(
            receiver.try_recv().is_err(),
            "过期世代 {name} 请求不得广播失败通知"
        );
    }
    for (request, reason) in [
        (MemberRequest::ProposePlayerCompanyAction { generation: 1,
            proposal: engine::session::PlayerCompanyProposal::CashDividend {
                company: engine::company::CompanyId(0), gross_per_share: engine::Money::from_cents(1_000),
            } }, "没有该主体的成员关系"),
        (MemberRequest::EnqueueReportCorrection { generation: 1, request: request.clone() }, "没有当前市场控制能力"),
        (MemberRequest::CancelReportCorrection { generation: 1, operation_id: request.operation_id.clone() }, "没有当前市场控制能力"),
    ] {
        assert!(matches!(dispatch(&mut actor, &sender, outsider.clone(), request).await,
            Err(SendCommandError::Rejected(actual)) if actual.contains(reason)));
        assert_eq!(actor.fatal_failure.as_ref(), Some(&first_failure));
        assert_eq!(actor.game.poison_reason(), first_poison.as_ref());
        assert_eq!(actor.game.business_state_hash().unwrap(), first_business);
        assert!(receiver.try_recv().is_err(), "未授权请求不得重发首错");
    }
}
