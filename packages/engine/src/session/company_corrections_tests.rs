//! 从真实 GameSession 受理更正；队列不是玩家持仓提案，也不提前修改公开材料。

use super::*;
use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, JournalLine,
    LedgerAccountId, PostingSide,
};
use crate::information::PublicationSource;
include!("../../test-support/simple_company.rs");

fn session_and_request() -> (GameSession, CompanyReportCorrection) {
    session_and_request_with_frequency(None)
}

fn session_and_request_with_frequency(
    frequency: Option<crate::information::ReportFrequency>,
) -> (GameSession, CompanyReportCorrection) {
    session_and_request_with_options("2030-06-08", frequency)
}

fn session_and_request_with_options(
    start_date: &str,
    frequency: Option<crate::information::ReportFrequency>,
) -> (GameSession, CompanyReportCorrection) {
    let setup = correction_setup(start_date, frequency);
    let session = GameSession::new_with_runtime_groups(setup, 7181, 3).unwrap();
    let request = correction_request(&session, CompanyId(0));
    (session, request)
}

fn correction_setup(
    start_date: &str,
    frequency: Option<crate::information::ReportFrequency>,
) -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.stocks[0].code = StockCode("600101".into());
    setup.start_date = CivilDate::from_iso(start_date).unwrap();
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 1;
    setup.company_system = Some(simple_company_fixture!(crate; ["600101"]));
    setup.report_frequency = frequency;
    setup.dividend_tax_mode = Some(crate::company::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1_000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup
}

fn correction_request(session: &GameSession, company: CompanyId) -> CompanyReportCorrection {
    let original = session
        .state
        .library
        .save()
        .reports
        .into_iter()
        .filter(|report| {
            report.source == PublicationSource::SimpleGenerated
                && report.company == company
                && matches!(
                    report.reports.scope,
                    crate::accounting::consolidation::ScopeId::Standalone(_)
                )
        })
        .max_by_key(|report| report.id)
        .expect("真实 Simple 新局须播种可更正公开报告");
    CompanyReportCorrection {
        operation_id: "session-correction-1".into(),
        company: original.company,
        supersedes: original.id,
        entries: vec![JournalEntry {
            source: BusinessEventId::new(900_000_001),
            date: session.civil_date(),
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
        reason: "补记已收现收入，保留历史公开版本".into(),
    }
}

#[test]
fn correction_enqueue_uses_real_session_and_preserves_request_order_without_publishing() {
    let (mut session, first) = session_and_request();
    let epoch = session.report_correction_epoch();
    let library = session.state.library.clone();
    let companies = serde_json::to_value(session.company_system().unwrap()).unwrap();
    let hash = session.business_state_hash().unwrap();
    let revision = session.external_revision();
    session
        .enqueue_report_correction(&epoch, first.clone())
        .unwrap();
    let mut second = first.clone();
    second.operation_id = "session-correction-2".into();
    second.entries[0].source = BusinessEventId::new(900_000_002);
    session
        .enqueue_report_correction(&epoch, second.clone())
        .unwrap();
    assert_eq!(
        session.report_corrections().unwrap().pending,
        vec![first, second]
    );
    assert!(session.report_corrections().unwrap().completed.is_empty());
    assert_eq!(session.state.library, library);
    assert_eq!(
        serde_json::to_value(session.company_system().unwrap()).unwrap(),
        companies
    );
    assert_ne!(session.business_state_hash().unwrap(), hash);
    assert_eq!(session.external_revision(), revision + 2);
    assert!(session.require_healthy().is_ok());
}

#[test]
fn correction_pending_payload_is_idempotent_and_conflict_is_recoverable() {
    let (mut session, request) = session_and_request();
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    let before = session.business_state_hash().unwrap();
    let revision = session.external_revision();
    session
        .enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    let mut conflict = request;
    conflict.reason = "同身份不得换载荷".into();
    assert!(matches!(
        session.enqueue_report_correction(&epoch, conflict),
        Err(ReportCorrectionError::OperationConflict {
            completed: false,
            ..
        })
    ));
    assert_eq!(session.report_corrections().unwrap().pending.len(), 1);
    assert_eq!(session.business_state_hash().unwrap(), before);
    assert_eq!(session.external_revision(), revision);
    assert!(session.require_healthy().is_ok());
}

#[test]
fn correction_cancel_removes_only_requested_pending_operation_and_rejects_missing() {
    let (mut session, request) = session_and_request();
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    let mut second = request.clone();
    second.operation_id = "session-correction-2".into();
    second.entries[0].source = BusinessEventId::new(900_000_002);
    session
        .enqueue_report_correction(&epoch, second.clone())
        .unwrap();
    let revision = session.external_revision();
    assert!(matches!(
        session.cancel_report_correction(&epoch, "absent"),
        Err(ReportCorrectionError::NotPending { .. })
    ));
    assert_eq!(session.external_revision(), revision);
    session
        .cancel_report_correction(&epoch, &request.operation_id)
        .unwrap();
    assert_eq!(session.report_corrections().unwrap().pending, vec![second]);
    assert_eq!(session.external_revision(), revision + 1);
    assert!(session.require_healthy().is_ok());
}

#[test]
fn correction_epoch_is_shared_by_shadow_and_replaced_on_restore() {
    let (mut session, request) = session_and_request();
    let save = session.save().unwrap();
    let epoch = session.report_correction_epoch();
    let mut shadow = session.clone_for_tick_shadow().unwrap();
    shadow
        .enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    assert!(session.report_corrections().unwrap().pending.is_empty());
    session.commit_tick_shadow(shadow);
    assert_eq!(
        session.report_corrections().unwrap().pending,
        vec![request.clone()]
    );
    session
        .cancel_report_correction(&epoch, &request.operation_id)
        .unwrap();
    let mut restored = GameSession::restore(&save).unwrap();
    assert!(matches!(
        restored.enqueue_report_correction(&epoch, request),
        Err(ReportCorrectionError::StaleEpoch)
    ));
    assert!(matches!(
        restored.cancel_report_correction(&epoch, "absent"),
        Err(ReportCorrectionError::StaleEpoch)
    ));
    assert!(restored.report_corrections().unwrap().pending.is_empty());
    assert!(restored.require_healthy().is_ok());
}

#[test]
fn correction_invalid_payloads_and_future_journal_never_mutate_live_queue() {
    let (mut session, request) = session_and_request();
    let epoch = session.report_correction_epoch();
    let before = session.business_state_hash().unwrap();
    let revision = session.external_revision();
    for edit in 0..5 {
        let mut invalid = request.clone();
        match edit {
            0 => invalid.operation_id = "  ".into(),
            1 => invalid.reason = "  ".into(),
            2 => invalid.entries.clear(),
            3 => invalid.entries[0].lines[0].amount = AccountingAmount::from_cents(999),
            4 => invalid.entries[0].date = session.civil_date().next().unwrap(),
            _ => unreachable!(),
        }
        let error = session
            .enqueue_report_correction(&epoch, invalid)
            .unwrap_err();
        let matches_cause = match edit {
            0 => matches!(error, ReportCorrectionError::EmptyOperationId),
            1 => matches!(error, ReportCorrectionError::EmptyReason),
            2 => matches!(error, ReportCorrectionError::EmptyEntries),
            3 => matches!(error, ReportCorrectionError::Journal { .. }),
            4 => matches!(error, ReportCorrectionError::FutureEntry { .. }),
            _ => unreachable!(),
        };
        assert!(matches_cause, "非法类别 {edit} 必须返回对应 typed 拒绝");
        assert_eq!(session.business_state_hash().unwrap(), before);
        assert_eq!(session.external_revision(), revision);
    }
    assert!(session.report_corrections().unwrap().pending.is_empty());
    assert!(session.require_healthy().is_ok());
}

#[test]
fn correction_terminal_failure_precedes_stale_epoch_and_input_errors() {
    let (mut session, request) = session_and_request();
    let (other, _) = session_and_request();
    let stale = other.report_correction_epoch();
    let fatal = StepFatal::InvariantViolation {
        location: "correction_terminal_failure_test".into(),
        description: "首个真实终止原因".into(),
    };
    session.poison_runtime_step(fatal.clone());
    assert!(matches!(session.enqueue_report_correction(&stale, request),
        Err(ReportCorrectionError::Step(actual)) if actual == fatal));
    assert!(matches!(session.cancel_report_correction(&stale, ""),
        Err(ReportCorrectionError::Step(actual)) if actual == fatal));
    assert!(matches!(session.report_corrections(),
        Err(ReportCorrectionError::Step(actual)) if actual == fatal));
}

#[test]
fn correction_pending_save_is_recoverable_and_cancel_restores_save_boundary() {
    let (mut session, request) = session_and_request();
    let before = serde_json::to_value(session.save().unwrap()).unwrap();
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    let pending = session.report_corrections().unwrap();
    let hash = session.business_state_hash().unwrap();
    let revision = session.external_revision();
    assert!(
        session.save().is_err(),
        "pending 更正不能悄悄丢失或冒充有效存档"
    );
    let refusal = session.save().unwrap_err();
    assert!(matches!(&refusal, SessionError::PendingReportCorrections));
    assert!(std::error::Error::source(&refusal).is_none());
    assert_eq!(session.report_corrections().unwrap(), pending);
    assert_eq!(session.business_state_hash().unwrap(), hash);
    assert_eq!(session.external_revision(), revision);
    assert!(session.require_healthy().is_ok());
    session
        .cancel_report_correction(&epoch, &request.operation_id)
        .unwrap();
    assert_eq!(
        serde_json::to_value(session.save().unwrap()).unwrap(),
        before
    );

    // pending 不能遮蔽已有 terminal 首错；保存拒绝也不能消费队列。
    session.enqueue_report_correction(&epoch, request).unwrap();
    let pending = session.state.report_corrections.clone();
    let hash = session.business_state_hash().unwrap();
    let revision = session.external_revision();
    let fatal = StepFatal::InvariantViolation {
        location: "correction_pending_save_terminal".to_owned(),
        description: "pending 更正前已有真实终止原因".to_owned(),
    };
    session.poison_runtime_step(fatal.clone());
    assert!(matches!(session.save(), Err(SessionError::Step(actual)) if actual == fatal));
    assert_eq!(session.poison_reason(), Some(&fatal));
    assert_eq!(session.state.report_corrections, pending);
    assert_eq!(session.business_state_hash().unwrap(), hash);
    assert_eq!(session.external_revision(), revision);
}

#[test]
fn correction_day_end_completes_at_real_eighteen_and_restores_exact_identity() {
    let (mut session, request) = session_and_request();
    let epoch = session.report_correction_epoch();
    let old_library = session.state.library.clone();
    session
        .enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    let report = session.end_civil_day().unwrap();
    let status = session.report_corrections().unwrap();
    assert!(status.pending.is_empty());
    let completed = &status.completed[&request.operation_id];
    assert_eq!(completed.request, request);
    assert_eq!(completed.committed_at, report.disclosure_instant);
    assert_eq!(completed.committed_at.second_of_day(), 18 * 3_600);
    assert!(!completed.publications.is_empty());
    for old in old_library.save().reports {
        assert_eq!(
            session
                .state
                .library
                .report(old.id, report.disclosure_instant)
                .unwrap(),
            &old
        );
    }
    assert!(completed.publications.iter().any(|id| {
        session
            .state
            .library
            .report(*id, report.disclosure_instant)
            .unwrap()
            .supersedes
            == Some(request.supersedes)
    }));
    assert!(matches!(
        session.cancel_report_correction(&epoch, &request.operation_id),
        Err(ReportCorrectionError::AlreadyCompleted { .. })
    ));
    let save = session.save().unwrap();
    let mut restored = GameSession::restore(&save).unwrap();
    let restored_epoch = restored.report_correction_epoch();
    let before = restored.business_state_hash().unwrap();
    restored
        .enqueue_report_correction(&restored_epoch, request.clone())
        .unwrap();
    assert_eq!(restored.business_state_hash().unwrap(), before);
    assert_eq!(restored.report_corrections().unwrap(), status);
    // 由真实正常日终与 restore 产出非空样本，供 Web strict save 消费；不拼装 fixture。
    println!(
        "B4E_COMPLETED_SAVE_JSON={}",
        serde_json::to_string(&save).unwrap()
    );
    let mut conflict = request;
    conflict.reason = "完成后仍不得同身份换载荷".into();
    assert!(matches!(
        restored.enqueue_report_correction(&restored_epoch, conflict),
        Err(ReportCorrectionError::OperationConflict {
            completed: true,
            ..
        })
    ));
    if let Some(path) = std::env::var_os("SMG_CORRECTION_WEB_ROUNDTRIP_SAVE") {
        let bytes = std::fs::read(path).expect("读取 Web strict save 回写原件失败");
        let roundtrip: SaveSlot =
            serde_json::from_slice(&bytes).expect("Web 回写必须通过完整 SaveSlot 严格反序列化");
        assert_eq!(
            serde_json::to_value(&roundtrip).unwrap(),
            serde_json::to_value(&save).unwrap()
        );
        let roundtrip_session = GameSession::restore(&roundtrip).unwrap();
        assert_eq!(roundtrip_session.report_corrections().unwrap(), status);
        println!("B4E_COMPLETED_SAVE_WEB_ROUNDTRIP_OK");
    }
}

#[test]
fn correction_revision_exhaustion_preserves_queue_and_exact_retry_is_still_idempotent() {
    let (mut session, request) = session_and_request();
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    session
        .revision
        .store(u64::MAX, std::sync::atomic::Ordering::Relaxed);
    let before = session.business_state_hash().unwrap();
    session
        .enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    let mut second = request.clone();
    second.operation_id = "revision-exhausted-correction".into();
    second.entries[0].source = BusinessEventId::new(900_000_002);
    assert!(matches!(
        session.enqueue_report_correction(&epoch, second),
        Err(ReportCorrectionError::RevisionExhausted)
    ));
    assert!(matches!(
        session.cancel_report_correction(&epoch, &request.operation_id),
        Err(ReportCorrectionError::RevisionExhausted)
    ));
    assert_eq!(session.business_state_hash().unwrap(), before);
    assert_eq!(session.external_revision(), u64::MAX);
    assert_eq!(session.report_corrections().unwrap().pending, vec![request]);
    assert!(session.require_healthy().is_ok());
}

#[test]
fn correction_restore_rejects_balanced_completed_payload_that_differs_from_real_journal() {
    let (mut session, request) = session_and_request();
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    session.end_civil_day().unwrap();
    let actual_entry = session
        .company_system()
        .unwrap()
        .finance(&request.company)
        .unwrap()
        .books()
        .journal()
        .entries()
        .find(|entry| entry.source == request.entries[0].source)
        .unwrap();
    assert_eq!(actual_entry, &request.entries[0]);
    assert_eq!(
        actual_entry.lines[0].amount,
        AccountingAmount::from_cents(1_000)
    );
    let save = session.save().unwrap();
    let mut edited = serde_json::to_value(&save).unwrap();
    let completed = edited["report_correction_operations"]
        .as_object_mut()
        .expect("完成的更正必须保存为最少真实事实")
        .get_mut(&request.operation_id)
        .unwrap();
    let mut altered: CompletedReportCorrection = serde_json::from_value(completed.clone()).unwrap();
    for line in &mut altered.request.entries[0].lines {
        line.amount = AccountingAmount::from_cents(2_000);
    }
    altered.request.entries[0].validate_invariants().unwrap();
    *completed = serde_json::to_value(altered).unwrap();
    let corrupted: SaveSlot = serde_json::from_value(edited).unwrap();
    let error = GameSession::restore(&corrupted)
        .err()
        .expect("完成载荷虽仍借贷平衡，也必须与真实 Journal 的完整凭证一致");
    assert!(
        matches!(&error, SessionError::ReportCorrectionRestore(cause)
        if cause.violation() == &ReportCorrectionRestoreViolation::CompanyFact)
    );
    let restore_cause = std::error::Error::source(&error).unwrap();
    assert!(restore_cause.is::<ReportCorrectionRestoreError>());
    assert!(restore_cause
        .source()
        .unwrap()
        .is::<crate::company::CompanyCorrectionError>());
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        restored.report_corrections().unwrap(),
        session.report_corrections().unwrap()
    );
}

#[test]
fn correction_restore_keeps_earlier_completion_after_a_later_legal_correction() {
    let (mut session, first_request) = session_and_request();
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, first_request.clone())
        .unwrap();
    session.end_civil_day().unwrap();
    let first =
        session.report_corrections().unwrap().completed[&first_request.operation_id].clone();
    let first_main = first
        .publications
        .iter()
        .copied()
        .find(|id| {
            session
                .state
                .library
                .report(*id, first.committed_at)
                .unwrap()
                .supersedes
                == Some(first_request.supersedes)
        })
        .unwrap();
    let mut second_request = first_request.clone();
    second_request.operation_id = "session-correction-later".into();
    second_request.supersedes = first_main;
    // 首次更正的税务级联也分配事件身份；第二请求必须使用真实未占用游标。
    let finance = session
        .company_system()
        .unwrap()
        .finance(&first_request.company)
        .unwrap();
    let next_event_id = finance.next_event_id();
    assert!(!finance
        .books()
        .journal()
        .entries()
        .any(|entry| entry.source.value() == next_event_id));
    second_request.entries[0].source = BusinessEventId::new(next_event_id);
    second_request.entries[0].date = session.civil_date();
    second_request.reason = "后续合法更正保留前次公开与操作事实".into();
    session
        .enqueue_report_correction(&epoch, second_request.clone())
        .unwrap();
    session.end_civil_day().unwrap();
    let status = session.report_corrections().unwrap();
    assert_eq!(status.completed.len(), 2);
    assert_eq!(status.completed[&first_request.operation_id], first);
    assert_eq!(
        status.completed[&second_request.operation_id].request,
        second_request
    );
    let save = session.save().unwrap();
    let mut restored = GameSession::restore(&save).unwrap();
    assert_eq!(restored.report_corrections().unwrap(), status);
    let restored_epoch = restored.report_correction_epoch();
    let before = restored.business_state_hash().unwrap();
    let revision = restored.external_revision();
    restored
        .enqueue_report_correction(&restored_epoch, first_request)
        .unwrap();
    assert_eq!(restored.business_state_hash().unwrap(), before);
    assert_eq!(restored.external_revision(), revision);
}

#[test]
fn correction_restore_rejects_reused_journal_source_across_same_company_completions() {
    let (mut session, first_request) = session_and_request();
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, first_request.clone())
        .unwrap();
    session.end_civil_day().unwrap();
    let first =
        session.report_corrections().unwrap().completed[&first_request.operation_id].clone();
    let first_main = first
        .publications
        .iter()
        .copied()
        .find(|id| {
            session
                .state
                .library
                .report(*id, first.committed_at)
                .unwrap()
                .supersedes
                == Some(first_request.supersedes)
        })
        .unwrap();
    let finance = session
        .company_system()
        .unwrap()
        .finance(&first_request.company)
        .unwrap();
    let next_event_id = finance.next_event_id();
    assert!(!finance
        .books()
        .journal()
        .entries()
        .any(|entry| entry.source.value() == next_event_id));
    let mut second_request = first_request.clone();
    second_request.operation_id = "session-correction-source-reuse".into();
    second_request.supersedes = first_main;
    second_request.entries[0].source = BusinessEventId::new(next_event_id);
    second_request.entries[0].date = session.civil_date();
    second_request.reason = "第二次合法更正有自己的真实调整凭证".into();
    session
        .enqueue_report_correction(&epoch, second_request.clone())
        .unwrap();
    session.end_civil_day().unwrap();
    let status = session.report_corrections().unwrap();
    assert_eq!(status.completed.len(), 2);
    assert_eq!(status.completed[&first_request.operation_id], first);
    assert_eq!(
        status.completed[&second_request.operation_id].request,
        second_request
    );
    let save = session.save().unwrap();
    assert_eq!(
        GameSession::restore(&save)
            .unwrap()
            .report_corrections()
            .unwrap(),
        status
    );
    // 两次真实过账后只改第二项输入归属；Journal、Closing 和 Library 均保持原件。
    let mut corrupted = save.clone();
    corrupted
        .report_correction_operations
        .get_mut(&second_request.operation_id)
        .unwrap()
        .request
        .entries = first_request.entries.clone();
    let error = GameSession::restore(&corrupted)
        .err()
        .expect("同公司两项 completed 不得消费同一真实 Journal source");
    assert!(
        matches!(&error, SessionError::ReportCorrectionRestore(cause)
        if cause.operation_id() == second_request.operation_id
            && cause.violation() == &ReportCorrectionRestoreViolation::DuplicateEntrySource {
                source: first_request.entries[0].source,
            })
    );
    assert_eq!(
        GameSession::restore(&save)
            .unwrap()
            .report_corrections()
            .unwrap(),
        status
    );
}

#[test]
fn correction_restore_allows_same_journal_source_for_different_companies() {
    let mut setup = correction_setup("2030-06-08", None);
    let mut shenzhen = setup.stocks[0].clone();
    shenzhen.code = StockCode("000002".into());
    shenzhen.exchange = StockExchange::Shenzhen;
    setup.stocks.insert(0, shenzhen);
    setup.company_system = Some(simple_company_fixture!(crate; ["000002", "600101"]));
    let mut session = GameSession::new_with_runtime_groups(setup, 7181, 3).unwrap();
    let first_request = correction_request(&session, CompanyId(0));
    let mut second_request = correction_request(&session, CompanyId(1));
    second_request.operation_id = "session-correction-other-company".into();
    assert_ne!(first_request.company, second_request.company);
    assert_eq!(
        first_request.entries[0].source,
        second_request.entries[0].source
    );
    let epoch = session.report_correction_epoch();
    for request in [&first_request, &second_request] {
        session
            .enqueue_report_correction(&epoch, request.clone())
            .unwrap();
    }
    session.end_civil_day().unwrap();
    let status = session.report_corrections().unwrap();
    assert!(status.pending.is_empty());
    assert_eq!(status.completed.len(), 2);
    for request in [&first_request, &second_request] {
        assert_eq!(&status.completed[&request.operation_id].request, request);
        assert_eq!(
            session
                .company_system()
                .unwrap()
                .finance(&request.company)
                .unwrap()
                .books()
                .journal()
                .entries()
                .find(|entry| entry.source == request.entries[0].source),
            Some(&request.entries[0])
        );
    }
    let save = session.save().unwrap();
    let mut restored = GameSession::restore(&save).unwrap();
    assert_eq!(restored.report_corrections().unwrap(), status);
    let restored_epoch = restored.report_correction_epoch();
    let before = restored.business_state_hash().unwrap();
    let revision = restored.external_revision();
    for request in [first_request, second_request] {
        restored
            .enqueue_report_correction(&restored_epoch, request)
            .unwrap();
    }
    assert_eq!(restored.report_corrections().unwrap(), status);
    assert_eq!(restored.business_state_hash().unwrap(), before);
    assert_eq!(restored.external_revision(), revision);
}

#[test]
fn correction_monthly_scan_endpoint_keeps_real_eighteen_publication_and_next_day_clock() {
    use crate::information::{MonthlyReportDelay, MonthlyReportSchedule, ReportFrequency};
    let (mut session, request) =
        session_and_request_with_frequency(Some(ReportFrequency::Monthly {
            schedule: MonthlyReportSchedule::Custom {
                day: 1,
                second_of_day: 23 * 3_600 + 50 * 60,
                delay: MonthlyReportDelay::None,
            },
        }));
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    let report = session.end_civil_day().unwrap();
    assert_eq!(report.disclosure_instant.second_of_day(), 86_399);
    assert_eq!(session.civil_date(), report.next_date);
    let status = session.report_corrections().unwrap();
    assert!(status.pending.is_empty());
    let completed = &status.completed[&request.operation_id];
    let actual_eighteen = crate::CivilInstant::from_hms(report.settled_date, 18, 0, 0).unwrap();
    assert_eq!(completed.committed_at, actual_eighteen);
    assert_eq!(completed.request, request);
    assert!(!completed.publications.is_empty());
    for id in &completed.publications {
        assert_eq!(
            session
                .state
                .library
                .report(*id, report.disclosure_instant)
                .unwrap()
                .published_at,
            actual_eighteen
        );
    }
}

fn correction_error_has_cause<T: std::error::Error + 'static>(
    error: &(dyn std::error::Error + 'static),
    check: impl Fn(&T) -> bool,
) -> bool {
    let mut current = Some(error);
    while let Some(cause) = current {
        if cause.downcast_ref::<T>().is_some_and(&check) {
            return true;
        }
        current = cause.source();
    }
    false
}

fn correction_accounting_leaf(
    error: &crate::accounting::AccountingError,
) -> &crate::accounting::AccountingError {
    match error {
        crate::accounting::AccountingError::BatchAborted { cause, .. } => {
            correction_accounting_leaf(cause)
        }
        error => error,
    }
}

#[test]
fn correction_day_end_unknown_company_or_account_preserves_pending_and_allows_cancel_retry() {
    for unknown_company in [true, false] {
        let (mut session, mut request) = session_and_request();
        if unknown_company {
            request.company = CompanyId(999_999);
        } else {
            request.entries[0].lines[1].account = LedgerAccountId("不存在的科目".into());
        }
        let epoch = session.report_correction_epoch();
        session
            .enqueue_report_correction(&epoch, request.clone())
            .unwrap();
        let pending = session.report_corrections().unwrap();
        let before = session.business_state_hash().unwrap();
        let revision = session.external_revision();
        let date = session.civil_date();
        let error = session.end_civil_day().unwrap_err();
        assert!(matches!(&error, SessionError::ReportCorrectionDayEnd(cause)
            if cause.classification() == crate::company::CompanyErrorClass::InvalidInput));
        if unknown_company {
            assert!(correction_error_has_cause::<
                crate::company::CompanyCorrectionError,
            >(&error, |cause| matches!(
                cause,
                crate::company::CompanyCorrectionError::InvalidInput { .. }
            )));
        } else {
            assert!(correction_error_has_cause::<
                crate::accounting::AccountingError,
            >(
                &error,
                |cause| matches!(correction_accounting_leaf(cause), crate::accounting::AccountingError::UnknownAccount { account, .. }
                    if account == &LedgerAccountId("不存在的科目".into()))
            ));
        }
        assert_eq!(session.report_corrections().unwrap(), pending);
        assert_eq!(session.business_state_hash().unwrap(), before);
        assert_eq!(session.external_revision(), revision);
        assert_eq!(session.civil_date(), date);
        assert!(session.require_healthy().is_ok());
        session
            .cancel_report_correction(&epoch, &request.operation_id)
            .unwrap();
        session.end_civil_day().unwrap();
    }
}

#[test]
fn correction_day_end_negative_cash_is_typed_recoverable_and_keeps_the_whole_day() {
    let (mut session, mut request) = session_and_request();
    request.entries[0].kind = BusinessKind::CashExpense;
    request.entries[0].lines[0].account = LedgerAccountId("6001".into());
    request.entries[0].lines[1].account = LedgerAccountId("1002".into());
    for line in &mut request.entries[0].lines {
        line.amount = AccountingAmount::from_cents(1_000_000_000_000);
    }
    request.entries[0].validate_invariants().unwrap();
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    let pending = session.report_corrections().unwrap();
    let before = session.business_state_hash().unwrap();
    let revision = session.external_revision();
    let date = session.civil_date();
    let error = session.end_civil_day().unwrap_err();
    assert!(matches!(&error, SessionError::ReportCorrectionDayEnd(cause)
        if cause.classification() == crate::company::CompanyErrorClass::BusinessCondition));
    assert!(correction_error_has_cause::<
        crate::accounting::AccountingError,
    >(
        &error,
        |cause| matches!(correction_accounting_leaf(cause), crate::accounting::AccountingError::NegativeCashProhibited { account, .. }
            if account == &LedgerAccountId("1002".into()))
    ));
    assert_eq!(session.report_corrections().unwrap(), pending);
    assert_eq!(session.business_state_hash().unwrap(), before);
    assert_eq!(session.external_revision(), revision);
    assert_eq!(session.civil_date(), date);
    assert!(session.require_healthy().is_ok());
    session
        .cancel_report_correction(&epoch, &request.operation_id)
        .unwrap();
    session.end_civil_day().unwrap();
}

#[test]
fn correction_month_end_posts_pending_adjustment_before_period_close_and_restores_real_sources() {
    let (mut session, request) = session_and_request_with_options("2030-06-30", None);
    assert!(matches!(
        session
            .state
            .civil_clock
            .calendar()
            .day_status(crate::calendar::CalendarExchange::Sse, session.civil_date(),)
            .unwrap(),
        crate::calendar::DayStatus::Closed(_)
    ));
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    let report = session.end_civil_day().unwrap();
    assert_eq!(report.settled_date, request.entries[0].date);
    let period = crate::accounting::AccountingPeriod::of_date(report.settled_date);
    let finance = session
        .company_system()
        .unwrap()
        .finance(&request.company)
        .unwrap();
    assert_eq!(
        finance.books().journal().period_status(period),
        crate::accounting::PeriodStatus::Closed
    );
    assert_eq!(
        finance
            .books()
            .journal()
            .entries()
            .find(|entry| entry.source == request.entries[0].source),
        Some(&request.entries[0])
    );
    let status = session.report_corrections().unwrap();
    assert!(status.pending.is_empty());
    assert_eq!(status.completed[&request.operation_id].request, request);
    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(restored.report_corrections().unwrap(), status);
}

#[test]
fn correction_external_closed_system_state_keeps_typed_cause_and_terminal_first_fatal() {
    let (mut session, mut request) = session_and_request();
    let receivable = LedgerAccountId("1122".into());
    let balance = session
        .company_system()
        .unwrap()
        .finance(&request.company)
        .unwrap()
        .books()
        .ledger()
        .account_balance(&receivable);
    assert!(
        balance.debit_total().is_positive(),
        "真实 opening 应收账款借方已有余额"
    );
    request.entries[0].kind = BusinessKind::CreditSale;
    request.entries[0].cash_flow = CashFlowClass::NonCash;
    request.entries[0].lines[0].account = receivable;
    for line in &mut request.entries[0].lines {
        line.amount = AccountingAmount::MAX;
    }
    request.entries[0].validate_invariants().unwrap();
    let epoch = session.report_correction_epoch();
    session.enqueue_report_correction(&epoch, request).unwrap();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    assert!(session.runtime_driver.is_none());
    let pending = session.state.report_corrections.clone();
    let before = session.business_state_hash().unwrap();
    let revision = session.external_revision();
    let authority_epoch = session.runtime_authority_epoch;
    let date = session.civil_date();
    let error = session.end_civil_day().unwrap_err();
    assert!(matches!(&error, SessionError::ReportCorrectionDayEnd(cause)
        if cause.classification() == crate::company::CompanyErrorClass::SystemState));
    assert!(correction_error_has_cause::<
        crate::accounting::AccountingError,
    >(&error, |cause| matches!(
        correction_accounting_leaf(cause),
        crate::accounting::AccountingError::AmountOverflow { .. }
    )));
    assert!(session.runtime_driver.is_none());
    assert_eq!(session.state.report_corrections, pending);
    assert_eq!(session.business_state_hash().unwrap(), before);
    assert_eq!(session.external_revision(), revision);
    assert_eq!(session.runtime_authority_epoch, authority_epoch);
    assert_eq!(session.civil_date(), date);
    let fatal = session
        .poison_reason()
        .expect("SystemState 须封存首个 terminal 错误")
        .clone();
    assert!(matches!(session.save(), Err(SessionError::Step(actual)) if actual == fatal));
    assert!(matches!(session.end_civil_day(), Err(SessionError::Step(actual)) if actual == fatal));
    assert_eq!(
        driver.step_frame_equivalent(&mut session).unwrap_err(),
        fatal
    );
    assert_eq!(session.poison_reason(), Some(&fatal));
    assert_eq!(session.state.report_corrections, pending);
}

#[test]
fn correction_external_manual_close_business_rejection_keeps_close_and_allows_civil_only_retry() {
    let mut setup = correction_setup("2030-06-07", None);
    setup.ticks_per_day = 4;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    let mut session = GameSession::new_with_runtime_groups(setup, 7181, 3).unwrap();
    let mut request = correction_request(&session, CompanyId(0));
    request.entries[0].lines[1].account = LedgerAccountId("不存在的科目".into());
    request.entries[0].validate_invariants().unwrap();
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    let pending = session.report_corrections().unwrap();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    session.runtime_step_with_install(&mut driver).unwrap();
    for _ in 0..3 {
        driver.step_frame_equivalent(&mut session).unwrap();
        driver.install_market_views(&mut session).unwrap();
        driver.install_candles(&mut session).unwrap();
    }
    let date = session.civil_date();
    let committed = driver.committed_calendar_tick().unwrap().unwrap();
    assert_eq!(
        (session.tick(), session.day(), driver.world_committed_tick()),
        (4, 0, 4)
    );
    // 本例 NPC 为零，只覆盖真实 empty Drain，不冒充非空订单或退款覆盖。
    let error = driver
        .day_end_with_runtime_market_day(&mut session)
        .unwrap_err();
    assert!(
        error.contains("自然日推进失败"),
        "manual 当前 String API 的真实错误：{error}"
    );
    assert_eq!(
        (session.tick(), session.day(), driver.world_committed_tick()),
        (4, 1, 4)
    );
    assert_eq!(session.civil_date(), date);
    assert!(session.runtime_driver.is_none());
    session
        .require_healthy()
        .expect("明确 InvalidInput 不应把已安装收盘转成 terminal");
    assert_eq!(session.report_corrections().unwrap(), pending);
    let retained = driver.publication().load();
    assert_eq!(
        retained.retained.days.len(),
        1,
        "真实收盘 retained 已安装且不可撤回"
    );
    assert!(driver
        .has_committed_market_close_for_civil(session.tick(), date)
        .unwrap());
    // manual String 不冒 typed cause；通过真正 civil 入口取得并核原错误链。
    let error = session.end_civil_day().unwrap_err();
    assert!(matches!(&error, SessionError::ReportCorrectionDayEnd(cause)
        if cause.classification() == crate::company::CompanyErrorClass::InvalidInput));
    assert!(correction_error_has_cause::<
        crate::accounting::AccountingError,
    >(
        &error,
        |cause| matches!(correction_accounting_leaf(cause),
            crate::accounting::AccountingError::UnknownAccount { account, .. }
            if account == &LedgerAccountId("不存在的科目".into()))
    ));
    session.require_healthy().unwrap();
    assert_eq!(session.report_corrections().unwrap(), pending);
    assert!(std::sync::Arc::ptr_eq(
        &retained.retained,
        &driver.publication().load().retained
    ));
    session
        .cancel_report_correction(&epoch, &request.operation_id)
        .unwrap();
    session
        .end_civil_day()
        .expect("只重试 civil，不重复已安装市场收盘");
    assert_eq!(
        (session.tick(), session.day(), driver.world_committed_tick()),
        (4, 1, 4)
    );
    assert!(session.civil_date() > date);
    assert!(session.runtime_driver.is_none());
    let after = driver.committed_calendar_tick().unwrap().unwrap();
    assert!(std::sync::Arc::ptr_eq(&committed.calendar, &after.calendar));
    assert_eq!(committed.source, after.source);
    assert!(std::sync::Arc::ptr_eq(
        &retained.retained,
        &driver.publication().load().retained
    ));
    session.require_healthy().unwrap();
}

#[test]
fn protocol_checkpoint_cannot_retract_full_close_after_a_typed_healthy_correction_rejection() {
    for negative_cash in [false, true] {
        let mut setup = correction_setup("2030-06-06", None);
        setup.ticks_per_day = 4;
        setup.auction_ticks = 0;
        setup.closing_auction_ticks = 0;
        setup.npcs.retail_count = 0;
        setup.npcs.inst_count = 0;
        setup.npcs.hot_count = 0;
        setup.runtime_sync_policy = crate::RuntimeSyncPolicy::OnDemand;
        let mut protocol = protocol::ProtocolSession::new(setup, 7182).unwrap();
        let calls = super::calendar_entry_cache_tests::EntryScope::enter();
        for _ in 0..4 {
            protocol.step_frame().unwrap();
        }
        let old_update = protocol.end_civil_day_update().unwrap();
        let old_save = serde_json::to_vec(&protocol.save().unwrap()).unwrap();
        let old_key = protocol::SaveCandidateKey {
            settled_date: old_update.boundary.settled_date,
            seq: old_update.seq_to,
        };
        let mut request = correction_request(protocol.game(), CompanyId(0));
        if negative_cash {
            request.entries[0].kind = BusinessKind::CashExpense;
            request.entries[0].lines[0].account = LedgerAccountId("6001".into());
            request.entries[0].lines[1].account = LedgerAccountId("1002".into());
            for line in &mut request.entries[0].lines {
                line.amount = AccountingAmount::from_cents(1_000_000_000_000);
            }
        } else {
            request.entries[0].lines[1].account = LedgerAccountId("不存在的科目".into());
        }
        request.entries[0].validate_invariants().unwrap();
        let epoch = protocol.game().report_correction_epoch();
        protocol
            .enqueue_report_correction(&epoch, request.clone())
            .unwrap();
        let checkpoint = protocol.checkpoint().unwrap();
        for _ in 0..4 {
            protocol.step_frame().unwrap();
        }
        let date = protocol.game().civil_date();
        let origin = protocol.game().runtime_mirror_origin();
        assert_eq!((protocol.game().tick(), origin.source_tick), (8, 8));
        let installed = serde_json::to_value(protocol.game().snapshot_inner(true, true)).unwrap();
        let (publication, retained, starts) = {
            let driver = protocol
                .game()
                .runtime_driver
                .as_ref()
                .unwrap()
                .lock()
                .unwrap();
            let retained = driver.publication().load().retained.clone();
            assert_eq!(retained.days.len(), 2, "两个真实收盘都已发布 retained");
            (
                driver.publication(),
                retained,
                driver.world_report_log().len(),
            )
        };
        let pending = protocol.game().report_corrections().unwrap();
        let baseline_calls = calls.counts();
        assert!(
            baseline_calls.start > 0 && baseline_calls.drain > 0,
            "计数必须先见证当前 World 的真实 Start 与收盘 Drain，不能以未接线的零作证"
        );
        let failure = protocol.end_civil_day_update_classified().unwrap_err();
        assert_eq!(
            failure.recovery(),
            protocol::CivilRecovery::CorrectionPrepareRejected
        );
        let error = failure.into_error();
        let expected_class = if negative_cash {
            crate::company::CompanyErrorClass::BusinessCondition
        } else {
            crate::company::CompanyErrorClass::InvalidInput
        };
        assert!(
            matches!(&error, SessionError::ReportCorrectionDayEnd(cause)
            if cause.classification() == expected_class),
            "必须由真实更正 prepare 拒绝：{error}"
        );
        protocol.game().require_healthy().unwrap();

        let rejected = protocol.rollback(checkpoint).unwrap_err();

        assert!(
            matches!(rejected, StepFatal::InvariantViolation { ref location, .. }
            if location == "ProtocolSession::rollback")
        );
        protocol.game().require_healthy().unwrap();
        assert_eq!(protocol.game().runtime_mirror_origin(), origin);
        assert_eq!(
            serde_json::to_value(protocol.game().snapshot_inner(true, true)).unwrap(),
            installed
        );
        assert_eq!(protocol.game().report_corrections().unwrap(), pending);
        assert_eq!(
            serde_json::to_vec(&protocol.save().unwrap()).unwrap(),
            old_save
        );
        assert_eq!(
            serde_json::to_vec(&protocol.save_candidate(&old_key).unwrap()).unwrap(),
            old_save
        );
        {
            let driver = protocol
                .game()
                .runtime_driver
                .as_ref()
                .unwrap()
                .lock()
                .unwrap();
            assert!(std::sync::Arc::ptr_eq(&publication, &driver.publication()));
            assert!(std::sync::Arc::ptr_eq(
                &retained,
                &driver.publication().load().retained
            ));
            assert_eq!(driver.world_report_log().len(), starts);
        }
        protocol
            .cancel_report_correction(&epoch, &request.operation_id)
            .unwrap();
        protocol.end_civil_day_update().unwrap();
        assert!(protocol.game().civil_date() > date);
        assert_eq!((protocol.game().tick(), protocol.game().day()), (8, 2));
        let driver = protocol
            .game()
            .runtime_driver
            .as_ref()
            .unwrap()
            .lock()
            .unwrap();
        assert!(std::sync::Arc::ptr_eq(&publication, &driver.publication()));
        assert!(std::sync::Arc::ptr_eq(
            &retained,
            &driver.publication().load().retained
        ));
        assert_eq!(
            driver.world_report_log().len(),
            starts,
            "取消后只重试 civil，不重复 Start/Drain"
        );
        assert_eq!(
            calls.counts(),
            baseline_calls,
            "真实拒绝、rollback 拒绝、取消及 civil retry 均无额外 Start/Drain 尝试"
        );
    }
}

#[test]
fn classified_civil_failure_exposes_only_current_real_correction_prepare_recovery() {
    for negative_cash in [false, true] {
        let mut protocol =
            protocol::ProtocolSession::new(correction_setup("2030-06-08", None), 7183).unwrap();
        let mut request = correction_request(protocol.game(), CompanyId(0));
        if negative_cash {
            request.entries[0].kind = BusinessKind::CashExpense;
            request.entries[0].lines[0].account = LedgerAccountId("6001".into());
            request.entries[0].lines[1].account = LedgerAccountId("1002".into());
            for line in &mut request.entries[0].lines {
                line.amount = AccountingAmount::from_cents(1_000_000_000_000);
            }
        } else {
            request.entries[0].lines[1].account = LedgerAccountId("不存在的科目".into());
        }
        request.entries[0].validate_invariants().unwrap();
        let epoch = protocol.game().report_correction_epoch();
        protocol
            .enqueue_report_correction(&epoch, request.clone())
            .unwrap();
        let failure = protocol.end_civil_day_update_classified().unwrap_err();
        let class = if negative_cash {
            crate::company::CompanyErrorClass::BusinessCondition
        } else {
            crate::company::CompanyErrorClass::InvalidInput
        };
        assert!(
            matches!(failure.error(), SessionError::ReportCorrectionDayEnd(cause)
            if cause.classification() == class)
        );
        protocol.game().require_healthy().unwrap();
        assert_eq!(failure.recovery(), protocol::CivilRecovery::CorrectionPrepareRejected,
            "CURRENT_CORRECTION_PREPARE_PROVENANCE_REQUIRED：只能由本次真实更正 prepare 产生恢复资格");
        let plain = failure.into_error();
        assert_eq!(
            protocol::CivilDayEndFailure::from(plain).recovery(),
            protocol::CivilRecovery::None,
            "抽出的领域错误不保留可重放来源资格"
        );
        protocol
            .cancel_report_correction(&epoch, &request.operation_id)
            .unwrap();
        protocol.end_civil_day_update_classified().unwrap();

        // 外持 driver 的 lease 不能证明 World 健康；降权不改变既有 typed 拒绝及取消重试。
        let mut external = GameSession::new(correction_setup("2030-06-08", None), 7183).unwrap();
        let external_epoch = external.report_correction_epoch();
        external
            .enqueue_report_correction(&external_epoch, request.clone())
            .unwrap();
        let driver = crate::runtime::drive::RuntimeDriver::from_session(&mut external).unwrap();
        let failure = external
            .with_prepared_civil_day_classified(|_, _| Ok(()))
            .unwrap_err();
        assert!(
            matches!(failure.error(), SessionError::ReportCorrectionDayEnd(cause)
            if cause.classification() == class)
        );
        assert_eq!(failure.recovery(), protocol::CivilRecovery::None);
        external.require_healthy().unwrap();
        external
            .cancel_report_correction(&external_epoch, &request.operation_id)
            .unwrap();
        external
            .end_civil_day()
            .expect("外持来源降权后仍保留原 cancel/retry 行为");
        assert!(external.runtime_driver.is_none());
        assert_eq!(driver.world_committed_tick(), 0);
    }
}

#[test]
fn captured_prepare_error_returned_by_callback_has_no_current_recovery_provenance() {
    for full_close in [false, true] {
        let mut setup = correction_setup(
            if full_close {
                "2030-06-07"
            } else {
                "2030-06-08"
            },
            None,
        );
        setup.ticks_per_day = 4;
        setup.auction_ticks = 0;
        setup.closing_auction_ticks = 0;
        setup.npcs.retail_count = 0;
        setup.npcs.inst_count = 0;
        setup.npcs.hot_count = 0;
        let mut game = GameSession::new_with_runtime_groups(setup, 7184, 2).unwrap();
        let mut request = correction_request(&game, CompanyId(0));
        request.entries[0].lines[1].account = LedgerAccountId("不存在的科目".into());
        let epoch = game.report_correction_epoch();
        game.enqueue_report_correction(&epoch, request.clone())
            .unwrap();
        let mut driver = if full_close {
            let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
            for _ in 0..4 {
                game.runtime_step_with_install(&mut driver).unwrap();
            }
            Some(driver)
        } else {
            None
        };
        let captured = game
            .with_prepared_civil_day_classified(|_, _| Ok(()))
            .unwrap_err()
            .into_error();
        assert!(
            matches!(&captured, SessionError::ReportCorrectionDayEnd(cause)
            if cause.classification() == crate::company::CompanyErrorClass::InvalidInput)
        );
        game.require_healthy().unwrap();
        game.cancel_report_correction(&epoch, &request.operation_id)
            .unwrap();
        let before = game.business_state_hash().unwrap();
        let date = game.civil_date();
        let mut reached = false;
        // SessionError 不实现 Clone；move 同一原错误比另造同型错误更严格。
        let failure = game
            .with_prepared_civil_day_classified::<()>(|_, _| {
                reached = true;
                Err(captured)
            })
            .unwrap_err();
        assert!(
            reached,
            "取消坏更正后实际 prepare 成功，错误确实来自 callback"
        );
        assert_eq!(
            failure.recovery(),
            protocol::CivilRecovery::None,
            "旧 prepare 原错误经 callback 返回不得继承本次恢复资格"
        );
        assert!(
            matches!(failure.error(), SessionError::ReportCorrectionDayEnd(cause)
            if cause.classification() == crate::company::CompanyErrorClass::InvalidInput)
        );
        assert_eq!(game.business_state_hash().unwrap(), before);
        assert_eq!(game.civil_date(), date);
        if let Some(driver) = &mut driver {
            let fatal = game
                .poison_reason()
                .expect("Full 后 callback 失败仍 terminal")
                .clone();
            assert_eq!(driver.step_frame_equivalent(&mut game).unwrap_err(), fatal);
            assert_eq!(driver.world_committed_tick(), 4);
        } else {
            game.require_healthy()
                .expect("冷 Closed callback 拒绝保原 generic 可逆边界");
            game.end_civil_day().unwrap();
        }
    }
}

#[test]
fn correction_external_full_close_business_rejection_keeps_cancel_retry_available() {
    for negative_cash in [false, true] {
        let mut setup = correction_setup("2030-06-07", None);
        setup.ticks_per_day = 4;
        setup.auction_ticks = 0;
        setup.closing_auction_ticks = 0;
        setup.npcs.retail_count = 0;
        setup.npcs.inst_count = 0;
        setup.npcs.hot_count = 0;
        let mut session = GameSession::new_with_runtime_groups(setup, 7181, 3).unwrap();
        let mut request = correction_request(&session, CompanyId(0));
        if negative_cash {
            request.entries[0].kind = BusinessKind::CashExpense;
            request.entries[0].lines[0].account = LedgerAccountId("6001".into());
            request.entries[0].lines[1].account = LedgerAccountId("1002".into());
            for line in &mut request.entries[0].lines {
                line.amount = AccountingAmount::from_cents(1_000_000_000_000);
            }
        } else {
            request.entries[0].lines[1].account = LedgerAccountId("不存在的科目".into());
        }
        request.entries[0].validate_invariants().unwrap();
        let epoch = session.report_correction_epoch();
        session
            .enqueue_report_correction(&epoch, request.clone())
            .unwrap();
        let pending = session.report_corrections().unwrap();
        let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
        for _ in 0..4 {
            session.runtime_step_with_install(&mut driver).unwrap();
        }
        let date = session.civil_date();
        assert_eq!(
            (session.tick(), session.day(), driver.world_committed_tick()),
            (4, 1, 4)
        );
        assert_eq!(
            session.runtime_installed_market_close,
            Some((session.runtime_authority_epoch, 4, date)),
            "必须经过真实 Full 收盘安装"
        );
        let before = session.business_state_hash().unwrap();
        let retained = driver.publication().load();
        let error = session.end_civil_day().unwrap_err();
        let expected = if negative_cash {
            crate::company::CompanyErrorClass::BusinessCondition
        } else {
            crate::company::CompanyErrorClass::InvalidInput
        };
        assert!(
            matches!(&error, SessionError::ReportCorrectionDayEnd(cause)
            if cause.classification() == expected),
            "必须取得真实更正业务拒绝：{error}"
        );
        assert!(correction_error_has_cause::<
            crate::accounting::AccountingError,
        >(
            &error,
            |cause| matches!(correction_accounting_leaf(cause),
                crate::accounting::AccountingError::UnknownAccount { .. } if !negative_cash)
                || matches!(correction_accounting_leaf(cause),
                crate::accounting::AccountingError::NegativeCashProhibited { .. } if negative_cash)
        ));
        assert!(session.require_healthy().is_ok(),
            "FULL_CLOSE_CORRECTION_RETRY_REQUIRED：Full 收盘后的明确业务拒绝仍须可取消重试；negative_cash={negative_cash}");
        assert_eq!(session.business_state_hash().unwrap(), before);
        assert_eq!(session.report_corrections().unwrap(), pending);
        assert_eq!(session.civil_date(), date);
        assert!(std::sync::Arc::ptr_eq(
            &retained.retained,
            &driver.publication().load().retained
        ));
        session
            .cancel_report_correction(&epoch, &request.operation_id)
            .unwrap();
        session
            .end_civil_day()
            .expect("取消真实业务拒绝后只重试 civil 日结");
        assert_eq!(
            (session.tick(), session.day(), driver.world_committed_tick()),
            (4, 1, 4)
        );
        assert!(session.civil_date() > date);
        assert!(session.runtime_driver.is_none());
        assert!(std::sync::Arc::ptr_eq(
            &retained.retained,
            &driver.publication().load().retained
        ));
        session.require_healthy().unwrap();
    }
}

#[test]
fn correction_external_full_close_callback_cannot_reuse_typed_prepare_rejection() {
    let mut setup = correction_setup("2030-06-07", None);
    setup.ticks_per_day = 4;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    let mut session = GameSession::new_with_runtime_groups(setup, 7181, 3).unwrap();
    let mut request = correction_request(&session, CompanyId(0));
    request.entries[0].lines[1].account = LedgerAccountId("不存在的科目".into());
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, request.clone())
        .unwrap();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    for _ in 0..4 {
        session.runtime_step_with_install(&mut driver).unwrap();
    }
    // 先取得真实 prepare 的 typed cause，不在测试中另造错误类型或分类。
    let original = session.end_civil_day().unwrap_err();
    assert!(
        matches!(&original, SessionError::ReportCorrectionDayEnd(cause)
        if cause.classification() == crate::company::CompanyErrorClass::InvalidInput)
    );
    session.require_healthy().unwrap();
    session
        .cancel_report_correction(&epoch, &request.operation_id)
        .unwrap();
    let before = session.business_state_hash().unwrap();
    let date = session.civil_date();
    let retained = driver.publication().load();
    let mut reached = false;
    let result: Result<(), SessionError> = session.with_prepared_civil_day(|_, _| {
        reached = true;
        Err(original)
    });
    assert!(
        reached,
        "取消坏更正后必须真实完成 prepare 并进入输出 callback"
    );
    assert!(
        matches!(result, Err(SessionError::ReportCorrectionDayEnd(cause))
        if cause.classification() == crate::company::CompanyErrorClass::InvalidInput)
    );
    let fatal = session
        .poison_reason()
        .expect("输出 callback 返回相同 typed 业务错误仍须 terminal，不能借 prepare 例外续跑")
        .clone();
    assert_eq!(session.business_state_hash().unwrap(), before);
    assert_eq!(session.civil_date(), date);
    assert_eq!(
        (session.tick(), session.day(), driver.world_committed_tick()),
        (4, 1, 4)
    );
    assert!(std::sync::Arc::ptr_eq(
        &retained.retained,
        &driver.publication().load().retained
    ));
    assert!(matches!(session.save(), Err(SessionError::Step(actual)) if actual == fatal));
    assert!(matches!(session.end_civil_day(), Err(SessionError::Step(actual)) if actual == fatal));
    assert_eq!(
        driver.step_frame_equivalent(&mut session).unwrap_err(),
        fatal
    );
}

#[test]
fn correction_tax_cascade_overflow_preserves_actual_accounting_source() {
    let (mut session, mut request) = session_and_request();
    // 保留无历史余额的 6001；首笔过账成功后，真实税务年度利润汇总才发生溢出。
    assert_eq!(
        request.entries[0].lines[1].account,
        LedgerAccountId("6001".into())
    );
    for line in &mut request.entries[0].lines {
        line.amount = AccountingAmount::MAX;
    }
    request.entries[0].validate_invariants().unwrap();
    let epoch = session.report_correction_epoch();
    session.enqueue_report_correction(&epoch, request).unwrap();
    let error = session.end_civil_day().unwrap_err();
    assert!(
        matches!(&error, SessionError::ReportCorrectionDayEnd(source)
        if source.classification() == crate::company::CompanyErrorClass::SystemState)
    );
    assert!(correction_error_has_cause::<
        crate::company::IncomeTaxOwnerError,
    >(&error, |source| matches!(
        source,
        crate::company::IncomeTaxOwnerError::Accounting(
            crate::accounting::AccountingError::AmountOverflow { .. }
        )
    )));
    assert!(
        correction_error_has_cause::<crate::accounting::AccountingError>(
            &error,
            |source| matches!(
                source,
                crate::accounting::AccountingError::AmountOverflow { .. }
            )
        ),
        "CORRECTION_TAX_OVERFLOW_SOURCE_MISSING：真实税务 AmountOverflow 必须保留在标准 source 链"
    );
    assert!(session.require_healthy().is_err());
}

#[cfg(feature = "simulation-diagnostics")]
#[test]
fn correction_manual_system_state_keeps_same_first_fatal_in_terminal_drain() {
    let mut setup = correction_setup("2030-06-07", None);
    setup.ticks_per_day = 4;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    let mut session = GameSession::new_with_runtime_groups(setup, 7181, 3).unwrap();
    let mut request = correction_request(&session, CompanyId(0));
    let receivable = LedgerAccountId("1122".into());
    assert!(session
        .company_system()
        .unwrap()
        .finance(&request.company)
        .unwrap()
        .books()
        .ledger()
        .account_balance(&receivable)
        .debit_total()
        .is_positive());
    request.entries[0].kind = BusinessKind::CreditSale;
    request.entries[0].cash_flow = CashFlowClass::NonCash;
    request.entries[0].lines[0].account = receivable;
    for line in &mut request.entries[0].lines {
        line.amount = AccountingAmount::MAX;
    }
    request.entries[0].validate_invariants().unwrap();
    let epoch = session.report_correction_epoch();
    session.enqueue_report_correction(&epoch, request).unwrap();
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: StockId(0),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(995)),
                qty: 100,
            },
        )
        .unwrap();
    let pending = session.take_runtime_player_intents();
    assert_eq!(pending.len(), 1, "非空 Drain 必须来自唯一真实受理意图");
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    let upper = driver
        .world_public_views()
        .iter()
        .map(|view| view.daily_upper_limit)
        .collect::<Vec<_>>();
    driver.inject_player_intents(&pending, &upper).unwrap();
    let events = session.runtime_step_with_install(&mut driver).unwrap();
    let order = events
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted {
                id,
                account: AccountId(0),
                remaining_qty: 100,
                ..
            } => Some(*id),
            _ => None,
        })
        .expect("真实未成交委托必须留给非空 Drain");
    for _ in 0..3 {
        driver.step_frame_equivalent(&mut session).unwrap();
        driver.install_market_views(&mut session).unwrap();
        driver.install_candles(&mut session).unwrap();
    }
    let date = session.civil_date();
    let error = driver
        .day_end_with_runtime_market_day(&mut session)
        .unwrap_err();
    let fatal = session
        .poison_reason()
        .expect("真实 AmountOverflow 必须封存首错")
        .clone();
    let terminal = driver
        .terminal_causal_snapshot_for_test()
        .expect("真实 Drain 失败必须留证");
    assert!(terminal.0.is_some());
    assert_eq!(
        terminal
            .1
            .iter()
            .filter(|fact| matches!(fact.kind,
        crate::runtime::diagnostics::DeskCausalKind::DayEnded { order: raw, qty: 100, .. }
            if crate::OrderId((u64::from(raw.0) << 32) | raw.1) == order))
            .count(),
        1
    );
    assert_eq!(
        terminal.2, fatal,
        "MANUAL_CORRECTION_FIRST_FATAL_DRIFT：Drain 与 Session 必须保留同一个首错"
    );
    assert_eq!(error, fatal.to_string());
    assert_eq!(
        (session.tick(), session.day(), driver.world_committed_tick()),
        (4, 1, 4)
    );
    assert_eq!(session.civil_date(), date);
    assert!(driver.collect_resting_facts_live().unwrap()[0].is_empty());
    assert!(matches!(session.save(), Err(SessionError::Step(actual)) if actual == fatal));
    assert_eq!(
        driver.step_frame_equivalent(&mut session).unwrap_err(),
        fatal
    );
    assert_eq!(driver.terminal_causal_snapshot_for_test(), Some(terminal));
}

/// 当前 Rust producer 的短档必须与 Web 共用同一原始字节；手工补字段或改档会让此测试失效。
#[test]
fn current_completed_correction_fixture_matches_producer_and_restores() {
    let mut setup = correction_setup("2030-06-08", None);
    if let Some(crate::company::config::CompanySystemConfig::Simple(mut config)) =
        setup.company_system.take()
    {
        config.prehistory_periods = 6;
        setup.company_system = Some(crate::company::config::CompanySystemConfig::Simple(config));
    } else {
        panic!("更正夹具必须使用 Simple 公司系统");
    }
    setup.history_len = 1;
    let mut session = GameSession::new_with_runtime_groups(setup, 7181, 3).unwrap();
    let request = correction_request(&session, CompanyId(0));
    let operation_id = request.operation_id.clone();
    let epoch = session.report_correction_epoch();
    session.enqueue_report_correction(&epoch, request).unwrap();
    session.end_civil_day().unwrap();
    let save = session.save().unwrap();
    let generated = serde_json::to_vec(&save).unwrap();
    assert!(
        generated.len() <= 160_000,
        "代表性短档不得超过 160000 bytes，实际 {}",
        generated.len()
    );
    assert_eq!(
        save.setup.runtime_sync_policy,
        crate::RuntimeSyncPolicy::EveryTick
    );
    if let Some(path) = std::env::var_os("SMG_WRITE_CURRENT_CORRECTION_FIXTURE") {
        std::fs::write(path, &generated).expect("写出当前 producer 夹具失败");
    }
    let fixture = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/correction/current-completed-save.json"),
    )
    .expect("当前 Rust producer 夹具必须存在");
    assert_eq!(
        generated, fixture,
        "当前 producer 输出漂移；必须重新生成夹具，不能手改 JSON"
    );

    let decoded = crate::decode_save_slot(&generated, &crate::SaveDecodeLimits::default())
        .expect("当前夹具必须通过 Rust 完整 SaveSlot 读取");
    let restored = GameSession::restore(&decoded).expect("当前夹具必须通过 Rust 正常恢复");
    let completed = &restored.report_corrections().unwrap().completed[&operation_id];
    assert_eq!(completed.committed_at.second_of_day(), 18 * 3_600);
    assert!(!completed.publications.is_empty());
    let value = serde_json::to_value(&save).unwrap();
    assert_eq!(
        value["company_state"]["CompanySystem"]["system"]["implementation"]["state"]["companies"]
            ["0"]["finance"]["closing"]["restatements"][0][1][0][0],
        serde_json::json!(900_000_001),
        "代表性短档必须保留真实更正重述 BusinessEventId"
    );
    let status = restored.report_corrections().unwrap();
    if let Some(path) = std::env::var_os("SMG_CURRENT_CORRECTION_WEB_ROUNDTRIP_SAVE") {
        let bytes = std::fs::read(path).expect("读取当前 Web strict save 回写失败");
        let roundtrip: SaveSlot =
            serde_json::from_slice(&bytes).expect("Web 回写必须通过完整 SaveSlot 严格反序列化");
        assert_eq!(
            serde_json::to_value(&roundtrip).unwrap(),
            serde_json::to_value(&save).unwrap()
        );
        let decoded = crate::decode_save_slot(&bytes, &crate::SaveDecodeLimits::default())
            .expect("Web 回写必须通过当前 Rust 完整 SaveSlot 读取");
        let roundtrip_session =
            GameSession::restore(&decoded).expect("Web 回写必须通过 Rust 正常恢复");
        assert_eq!(roundtrip_session.report_corrections().unwrap(), status);
        println!("CURRENT_CORRECTION_WEB_ROUNDTRIP_OK");
    }
}
