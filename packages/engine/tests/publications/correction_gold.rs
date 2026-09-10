//! 更正金样（K4 / 任务 13 契约）：公开更正 = 新版本关联旧 ID，历史不可
//! 覆写——原版本重复查询逐字节不变；结账登记簿中的原版本同样不变。

use crate::books_fixture::{correction_books, entry};
use crate::fixture::OPS_SEED;
use engine::accounting::closing::{ClosingEngine, CorrectionRequest};
use engine::accounting::consolidation::{MemberId, ScopeId};
use engine::accounting::reports::{IndustryPresentation, ReportKind};
use engine::accounting::{AccountingPeriod, BusinessKind, CashFlowClass, PostingSide};
use engine::calendar::CivilInstant;
use engine::company::CompanyId;
use engine::information::{
    scheduled_instant, stable_company_offset, AccountingPolicyRef, PublicLibrary,
    PublicationOrigin, PublicationRequest, ScheduledReportKind,
};

const COMPANY: &str = "C-CORR";

#[test]
fn correction_preserves_previous_version() {
    let company = CompanyId(COMPANY.to_string());
    let offset = stable_company_offset(OPS_SEED, &company);
    let scope = ScopeId::Standalone(MemberId(COMPANY.to_string()));
    let annual_instant = scheduled_instant(ScheduledReportKind::Annual, 2030, offset)
        .expect("annual 2030 schedule legal");
    let approval =
        CivilInstant::from_hms(annual_instant.date(), 8, 0, 0).expect("approval instant");

    // 账套 → 年结 v1（12 月封月 + 年报版本）→ 公开 v1。
    let mut books = correction_books();
    let mut closing = ClosingEngine::new();
    let (_monthly, annual_v1) = closing
        .close_year(
            &mut books,
            &MemberId(COMPANY.to_string()),
            IndustryPresentation::Industrial,
            2030,
        )
        .expect("year close");
    assert_eq!(annual_v1.sequence, 1);

    let mut library = PublicLibrary::new();
    let v1_id = library
        .publish_closed(
            &closing,
            PublicationRequest {
                company: company.clone(),
                scope: scope.clone(),
                period: AccountingPeriod::from_ymd(2030, 12).expect("annual period"),
                kind: ReportKind::Annual,
                sequence: 1,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: approval,
                published_at: annual_instant,
                origin: PublicationOrigin::ScheduledDisclosure {
                    fiscal_year: 2030,
                    kind: ScheduledReportKind::Annual,
                    offset_days: offset,
                },
                supersedes: None,
            },
        )
        .expect("annual v1 publishes");
    let v1_before = library
        .report(v1_id, annual_instant)
        .expect("v1 readable")
        .clone();
    let v1_bytes = serde_json::to_string(&v1_before).expect("v1 serializes");

    // —— 差错更正：调整分录过账于开放期间 2031-01 → 年报 v2（supersedes v1）——
    let corrected = closing
        .correct(
            &mut books,
            &MemberId(COMPANY.to_string()),
            IndustryPresentation::Industrial,
            (
                AccountingPeriod::from_ymd(2030, 12).expect("annual period"),
                ReportKind::Annual,
            ),
            CorrectionRequest {
                entries: vec![entry(
                    99,
                    "2031-01-15",
                    BusinessKind::CashRevenue,
                    CashFlowClass::Operating,
                    &[
                        ("1002", PostingSide::Debit, 300),
                        ("6001", PostingSide::Credit, 300),
                    ],
                )],
                reason: "遗漏现金收入更正".to_string(),
            },
        )
        .expect("correction registers v2");
    assert_eq!(corrected.sequence, 2);

    // 公开更正：新 PublishedReport 关联旧 ID，公布在更正后的下一个 18:00 相位。
    let correction_instant = scheduled_instant(ScheduledReportKind::Q1, 2031, offset)
        .expect("an 18:00 phase instant after the correction");
    let correction_approval =
        CivilInstant::from_hms(correction_instant.date(), 8, 0, 0).expect("approval instant");
    let v2_id = library
        .publish_closed(
            &closing,
            PublicationRequest {
                company: company.clone(),
                scope: scope.clone(),
                period: AccountingPeriod::from_ymd(2030, 12).expect("annual period"),
                kind: ReportKind::Annual,
                sequence: 2,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: correction_approval,
                published_at: correction_instant,
                origin: PublicationOrigin::Correction,
                supersedes: Some(v1_id),
            },
        )
        .expect("correction publishes as a new version");

    // —— 历史不可覆写：v1 重复查询逐字节不变；v2 关联 v1 ——
    let v1_after = library
        .report(v1_id, correction_instant)
        .expect("v1 still readable");
    assert_eq!(
        serde_json::to_string(v1_after).expect("v1 serializes"),
        v1_bytes,
        "original published version must be byte-identical after the correction"
    );
    let v2 = library
        .report(v2_id, correction_instant)
        .expect("v2 readable");
    assert_eq!(v2.supersedes, Some(v1_id));
    assert_eq!(v2.reports.version.sequence, 2);
    assert_eq!(v2.reports.version.supersedes, Some(1));
    assert_ne!(
        v2.reports.income.cumulative.net_income,
        v1_before.reports.income.cumulative.net_income
    );

    // latest_for：更正后最新版本是 v2（id 单调 + 查询按 as_of 过滤）。
    let latest = library
        .latest_report(
            &company,
            ReportKind::Annual,
            AccountingPeriod::from_ymd(2030, 12).expect("annual period"),
            correction_instant,
        )
        .expect("a latest version exists");
    assert_eq!(latest.id, v2_id);
    // 更正时点之前，最新版本仍是 v1。
    let latest_before = library
        .latest_report(
            &company,
            ReportKind::Annual,
            AccountingPeriod::from_ymd(2030, 12).expect("annual period"),
            annual_instant,
        )
        .expect("v1 was the latest at its time");
    assert_eq!(latest_before.id, v1_id);

    // 结账登记簿原版本同样不可变（任务 13 契约在披露层的对照锚）。
    let registry_v1 = closing
        .version(
            &scope,
            AccountingPeriod::from_ymd(2030, 12).expect("annual period"),
            ReportKind::Annual,
            1,
        )
        .expect("registry keeps v1");
    assert_eq!(
        serde_json::to_string(registry_v1).expect("registry v1 serializes"),
        serde_json::to_string(&v1_before.reports).expect("published v1 set serializes"),
        "published bytes mirror the immutable registry version"
    );
    // 净利差 = 更正分录 300 元（金样锚：v2 = v1 + 300.00）。
    assert_eq!(
        (v2.reports.income.cumulative.net_income.cents()
            - v1_before.reports.income.cumulative.net_income.cents()) as i128,
        30_000i128,
        "correction adjustment flows into the restated annual"
    );
}
