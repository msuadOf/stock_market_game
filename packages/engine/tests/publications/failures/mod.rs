//! 拒绝路径套件：提前读取、未定稿/不平报表（报告公布面的其余拒绝在
//! `publication_failures`，公告面在 `announcement_failures`）。全部类型化
//! 拒绝（绝不静默补默认值）。

mod announcement_failures;
mod publication_failures;

use crate::books_fixture::correction_books;
use crate::fixture::OPS_SEED;
use engine::accounting::closing::ClosingEngine;
use engine::accounting::consolidation::{MemberId, ScopeId};
use engine::accounting::reports::{IndustryPresentation, ReportKind};
use engine::accounting::AccountingPeriod;
use engine::calendar::CivilInstant;
use engine::company::{CompanyId, ShockKind};
use engine::information::{
    scheduled_instant, stable_company_offset, AccountingPolicyRef, AnnouncedEvent,
    AnnouncementRequest, InformationError, PublicLibrary, PublicLibrarySave, PublicationOrigin,
    PublicationRequest, ScheduledReportKind,
};

const COMPANY: &str = "C-FAIL";
const OTHER: &str = "C-OTHER";

struct Base {
    closing: ClosingEngine,
    library: PublicLibrary,
    scope: ScopeId,
    offset: u8,
    q1_instant: CivilInstant,
    approval: CivilInstant,
}

/// 合法基线：Q1 2030 快照 + 一次合法公布（后续拒绝用例在其上构造）。
fn base() -> Base {
    let company = CompanyId(COMPANY.to_string());
    let offset = stable_company_offset(OPS_SEED, &company);
    let mut closing = ClosingEngine::new();
    let books = correction_books();
    let scope = ScopeId::Standalone(MemberId(COMPANY.to_string()));
    closing
        .snapshot_interim(
            &books,
            &MemberId(COMPANY.to_string()),
            IndustryPresentation::Industrial,
            AccountingPeriod::from_ymd(2030, 3).expect("q1 period"),
            ReportKind::Quarter,
        )
        .expect("q1 snapshot registers");
    let q1_instant = scheduled_instant(ScheduledReportKind::Q1, 2030, offset).expect("schedule");
    let approval = CivilInstant::from_hms(q1_instant.date(), 8, 0, 0).expect("approval");
    let mut library = PublicLibrary::new();
    library
        .publish_closed(
            &closing,
            PublicationRequest {
                company: company.clone(),
                scope: scope.clone(),
                period: AccountingPeriod::from_ymd(2030, 3).expect("q1 period"),
                kind: ReportKind::Quarter,
                sequence: 1,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: approval,
                published_at: q1_instant,
                origin: PublicationOrigin::ScheduledDisclosure {
                    fiscal_year: 2030,
                    kind: ScheduledReportKind::Q1,
                    offset_days: offset,
                },
                supersedes: None,
            },
        )
        .expect("baseline publication");
    Base {
        closing,
        library,
        scope,
        offset,
        q1_instant,
        approval,
    }
}

fn request(base: &Base, period: AccountingPeriod) -> PublicationRequest {
    PublicationRequest {
        company: CompanyId(COMPANY.to_string()),
        scope: base.scope.clone(),
        period,
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
    }
}

/// 提前读取：报告与公告在公布时点之前均类型化拒绝。
#[test]
fn early_read_rejected() {
    let base = base();
    let before = CivilInstant::from_hms(base.q1_instant.date(), 17, 59, 59).expect("17:59");
    let id = base.library.publication_ids()[0];
    assert!(matches!(
        base.library.report(id, before).unwrap_err(),
        InformationError::EarlyRead { .. }
    ));
    let mut library = base.library;
    let announcement_id = library
        .publish_announcement(AnnouncementRequest {
            company: CompanyId(COMPANY.to_string()),
            occurred_on: base.q1_instant.date(),
            published_at: base.q1_instant,
            event: AnnouncedEvent {
                kind: ShockKind::ContractWon,
                amplitude_bp: 100,
                starts_on: base.q1_instant.date(),
                expires_on: base.q1_instant.date(),
            },
        })
        .expect("announcement publishes at its phase");
    assert!(matches!(
        library.announcement(announcement_id, before).unwrap_err(),
        InformationError::EarlyRead { .. }
    ));
}

/// 未定稿：结账登记簿不存在的版本（期间未结 / 序号越界）不可公开。
#[test]
fn unfinalized_report_rejected() {
    let mut base = base();
    // 序号 2：登记簿只有 sequence 1。
    let mut req = request(&base, AccountingPeriod::from_ymd(2030, 3).expect("q1"));
    req.sequence = 2;
    assert!(matches!(
        base.library.publish_closed(&base.closing, req).unwrap_err(),
        InformationError::ReportNotFinalized { sequence: 2, .. }
    ));
    // 期间未结：2030-06 半年报窗口从未快照（6 月落在合法窗口，但登记簿为空）。
    let mut req = request(&base, AccountingPeriod::from_ymd(2030, 6).expect("h1"));
    req.kind = ReportKind::HalfYear;
    req.origin = PublicationOrigin::ScheduledDisclosure {
        fiscal_year: 2030,
        kind: ScheduledReportKind::HalfYear,
        offset_days: base.offset,
    };
    req.published_at = scheduled_instant(ScheduledReportKind::HalfYear, 2030, base.offset).unwrap();
    req.approved_at = CivilInstant::from_hms(req.published_at.date(), 8, 0, 0).unwrap();
    assert!(matches!(
        base.library.publish_closed(&base.closing, req).unwrap_err(),
        InformationError::ReportNotFinalized { .. }
    ));
}

/// 不平报表：恢复边界拒绝勾稽失败的公开版本（篡改总资产 +1 分）。
#[test]
fn unbalanced_report_rejected_at_restore_boundary() {
    let base = base();
    let id = base.library.publication_ids()[0];
    let report = base
        .library
        .report(id, base.q1_instant)
        .expect("readable")
        .clone();
    let bumped = engine::accounting::AccountingAmount::from_cents(
        report.reports.balance_sheet.total_assets.cents() + 1,
    );
    let mut value = serde_json::to_value(&report).expect("serializes");
    value["reports"]["balance_sheet"]["total_assets"] =
        serde_json::to_value(bumped).expect("amount serializes");
    let tampered_report: engine::information::PublishedReport =
        serde_json::from_value(value).expect("shape still deserializes");
    assert_ne!(
        serde_json::to_string(&tampered_report.reports.balance_sheet.total_assets).unwrap(),
        serde_json::to_string(&report.reports.balance_sheet.total_assets).unwrap(),
        "fixture sanity: tamper changed the value"
    );
    let save = PublicLibrarySave {
        next_seq: 100,
        reports: vec![tampered_report],
        announcements: Vec::new(),
    };
    assert!(matches!(
        PublicLibrary::from_parts(save).unwrap_err(),
        InformationError::ReportNotPublishable(_)
    ));
}
