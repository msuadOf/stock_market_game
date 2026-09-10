//! 拒绝路径套件（报告公布面）：时间逆序、非法报告窗口、相位/排期偏离、
//! 重复 ID、更正关系错配。全部类型化拒绝（绝不静默补默认值）。
//!
//! 共享基线（`base`/`request`）在 `super`。

use super::{base, request, OTHER};
use crate::books_fixture::correction_books;
use crate::fixture::d;
use engine::accounting::closing::ClosingEngine;
use engine::accounting::consolidation::{MemberId, ScopeId};
use engine::accounting::reports::{IndustryPresentation, ReportKind};
use engine::accounting::AccountingPeriod;
use engine::calendar::CivilInstant;
use engine::company::CompanyId;
use engine::information::{
    AccountingPolicyRef, InformationError, PublicationId, PublicationOrigin, PublicationRequest,
    PublicLibrary, PublicLibrarySave, ScheduledReportKind,
};

/// 时间逆序：公布早于批准 / 批准不晚于报告期末。
#[test]
fn time_inversion_rejected() {
    let mut base = base();
    // 公布早于批准。
    let mut req = request(&base, AccountingPeriod::from_ymd(2030, 3).expect("q1"));
    req.approved_at = CivilInstant::from_hms(base.q1_instant.date(), 19, 0, 0).unwrap();
    req.published_at = base.q1_instant;
    req.origin = PublicationOrigin::Correction;
    req.supersedes = Some(base.library.publication_ids()[0]);
    assert!(matches!(
        base.library
            .publish_closed(&base.closing, req.clone())
            .unwrap_err(),
        InformationError::PublishBeforeApproval { .. }
    ));
    // 批准落在报告期最后一天（3-31）之内：批准必须严格晚于期末。
    let mut req = request(&base, AccountingPeriod::from_ymd(2030, 3).expect("q1"));
    req.approved_at = CivilInstant::from_hms(d("2030-03-31"), 23, 0, 0).unwrap();
    assert!(matches!(
        base.library.publish_closed(&base.closing, req).unwrap_err(),
        InformationError::ApprovalPrecedesPeriodEnd { .. }
    ));
}

/// 非法报告窗口：排期引用与请求期间不一致（Q1 排期挂 6 月期、年报挂 6 月期）。
#[test]
fn illegal_report_window_rejected() {
    let mut base = base();
    let req = request(
        &base,
        AccountingPeriod::from_ymd(2030, 6).expect("h1 period"),
    );
    // Q1 排期指向 2030-03，请求期间却是 2030-06。
    assert!(matches!(
        base.library.publish_closed(&base.closing, req).unwrap_err(),
        InformationError::IllegalWindowPublication { .. }
    ));
    // 年报排期指向 12 月，请求期间 6 月。
    let mut req = request(
        &base,
        AccountingPeriod::from_ymd(2030, 6).expect("h1 period"),
    );
    req.kind = ReportKind::Annual;
    req.origin = PublicationOrigin::ScheduledDisclosure {
        fiscal_year: 2030,
        kind: ScheduledReportKind::Annual,
        offset_days: base.offset,
    };
    assert!(matches!(
        base.library.publish_closed(&base.closing, req).unwrap_err(),
        InformationError::IllegalWindowPublication { .. }
    ));
}

/// 相位与排期偏离：17:00 非披露相位；正确相位但错误日期；偏移越界。
#[test]
fn off_schedule_and_phase_rejected() {
    let mut base = base();
    // 17:00 —— 非披露相位。
    let mut req = request(&base, AccountingPeriod::from_ymd(2030, 3).expect("q1"));
    req.published_at = CivilInstant::from_hms(base.q1_instant.date(), 17, 0, 0).unwrap();
    assert!(matches!(
        base.library.publish_closed(&base.closing, req).unwrap_err(),
        InformationError::PublicationOutsidePhase { .. }
    ));
    // 正确相位、错误日期（次日 18:00）。
    let mut req = request(&base, AccountingPeriod::from_ymd(2030, 3).expect("q1"));
    req.published_at =
        CivilInstant::from_hms(base.q1_instant.date().next().expect("next day"), 18, 0, 0).unwrap();
    assert!(matches!(
        base.library.publish_closed(&base.closing, req).unwrap_err(),
        InformationError::OffSchedulePublication { .. }
    ));
    // 偏移越界（8 > 7）。
    let mut req = request(&base, AccountingPeriod::from_ymd(2030, 3).expect("q1"));
    req.origin = PublicationOrigin::ScheduledDisclosure {
        fiscal_year: 2030,
        kind: ScheduledReportKind::Q1,
        offset_days: 8,
    };
    assert!(matches!(
        base.library.publish_closed(&base.closing, req).unwrap_err(),
        InformationError::IllegalScheduleOffset { offset: 8 }
    ));
}

/// 重复 ID / 计数器不一致：恢复边界拒绝（库内 id 单调、共享计数器
/// 不重复、计数器与最大 id 严格衔接）。
#[test]
fn duplicate_and_out_of_range_ids_rejected() {
    let base = base();
    let id = base.library.publication_ids()[0];
    let report = base
        .library
        .report(id, base.q1_instant)
        .expect("readable")
        .clone();
    // 重复 id：两条同 id 记录。
    let save = PublicLibrarySave {
        next_seq: 100,
        reports: vec![report.clone(), report.clone()],
        announcements: Vec::new(),
    };
    assert!(matches!(
        PublicLibrary::from_parts(save).unwrap_err(),
        InformationError::DuplicatePublicationId { .. }
    ));
    // id ≥ next_seq：域越界。
    let save = PublicLibrarySave {
        next_seq: 0,
        reports: vec![report.clone()],
        announcements: Vec::new(),
    };
    assert!(matches!(
        PublicLibrary::from_parts(save).unwrap_err(),
        InformationError::InconsistentLibrary { .. }
    ));
    // 幻影计数器：next_seq 与最大 id 不衔接。
    let save = PublicLibrarySave {
        next_seq: 2,
        reports: vec![report],
        announcements: Vec::new(),
    };
    assert!(matches!(
        PublicLibrary::from_parts(save).unwrap_err(),
        InformationError::InconsistentLibrary { .. }
    ));
}

/// 更正关系错配：origin=Correction 必须带 supersedes；目标必须是同公司/
/// 范围/期间/种类的既有公布；未知目标拒绝。
#[test]
fn correction_link_rejected_on_mismatch() {
    let mut base = base();
    let target = base.library.publication_ids()[0];
    // Correction 不带 supersedes。
    let mut req = request(&base, AccountingPeriod::from_ymd(2030, 3).expect("q1"));
    req.origin = PublicationOrigin::Correction;
    req.supersedes = None;
    assert!(matches!(
        base.library.publish_closed(&base.closing, req).unwrap_err(),
        InformationError::OriginSupersedesMismatch
    ));
    // Scheduled 带 supersedes。
    let mut req = request(&base, AccountingPeriod::from_ymd(2030, 3).expect("q1"));
    req.supersedes = Some(target);
    assert!(matches!(
        base.library.publish_closed(&base.closing, req).unwrap_err(),
        InformationError::OriginSupersedesMismatch
    ));
    // 未知目标。
    let mut req = request(&base, AccountingPeriod::from_ymd(2030, 3).expect("q1"));
    req.origin = PublicationOrigin::Correction;
    req.supersedes = Some(PublicationId::new(999));
    assert!(matches!(
        base.library.publish_closed(&base.closing, req).unwrap_err(),
        InformationError::CorrectionTargetUnknown { target } if target.value() == 999
    ));
    // 跨公司目标：另一公司（自己的 scope + 自己的登记版本）公布后，
    // 用它作为本公司更正的目标 = 错配拒绝。
    let mut other_closing = ClosingEngine::new();
    let books = correction_books();
    other_closing
        .snapshot_interim(
            &books,
            &MemberId(OTHER.to_string()),
            IndustryPresentation::Industrial,
            AccountingPeriod::from_ymd(2030, 3).expect("q1 period"),
            ReportKind::Quarter,
        )
        .expect("other company snapshot");
    let other_id = base
        .library
        .publish_closed(
            &other_closing,
            PublicationRequest {
                company: CompanyId(OTHER.to_string()),
                scope: ScopeId::Standalone(MemberId(OTHER.to_string())),
                period: AccountingPeriod::from_ymd(2030, 3).expect("q1 period"),
                kind: ReportKind::Quarter,
                sequence: 1,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: base.approval,
                published_at: base.q1_instant,
                origin: PublicationOrigin::ScheduledDisclosure {
                    fiscal_year: 2030,
                    kind: ScheduledReportKind::Q1,
                    offset_days: base.offset,
                },
                supersedes: None,
            },
        )
        .expect("other company publishes its own version");
    let mut req = request(&base, AccountingPeriod::from_ymd(2030, 3).expect("q1"));
    req.origin = PublicationOrigin::Correction;
    req.supersedes = Some(other_id);
    assert!(matches!(
        base.library.publish_closed(&base.closing, req).unwrap_err(),
        InformationError::CorrectionTargetMismatch { .. }
    ));
    // 范围与公司不一致：scope 的 MemberId 必须镜像公司 id。
    let mut req = request(&base, AccountingPeriod::from_ymd(2030, 3).expect("q1"));
    req.company = CompanyId(OTHER.to_string());
    assert!(matches!(
        base.library.publish_closed(&base.closing, req).unwrap_err(),
        InformationError::ScopeCompanyMismatch { .. }
    ));
}
