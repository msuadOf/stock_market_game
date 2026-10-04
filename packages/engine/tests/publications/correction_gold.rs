//! 更正金样：公开更正 = 新版本关联旧 ID，历史不可
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
    CorrectionPublicationError, InformationError, PublicationOrigin, PublicationRequest,
    ScheduledReportKind,
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

    // 账套 → 原年结报告（12 月封月 + 年报 ReportVersion.sequence=1）→ 公开原报告。
    let mut books = correction_books();
    let mut closing = ClosingEngine::new();
    let (_monthly, original_annual_report) = closing
        .close_year(
            &mut books,
            &MemberId(COMPANY.to_string()),
            IndustryPresentation::Industrial,
            2030,
        )
        .expect("year close");
    assert_eq!(original_annual_report.sequence, 1);

    let mut library = PublicLibrary::new();
    let original_publication_id = library
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
        .expect("原年报公布");
    let original_report_before_correction = library
        .report(original_publication_id, annual_instant)
        .expect("原报告可读取")
        .clone();
    let original_publication_bytes =
        serde_json::to_string(&original_report_before_correction).expect("原报告可序列化");

    // —— 差错更正与公开合为一个提交：先验证失败回滚，再原凭证重试 ——
    let correction_instant = scheduled_instant(ScheduledReportKind::Q1, 2031, offset)
        .expect("an 18:00 phase instant after the correction");
    let correction_approval =
        CivilInstant::from_hms(correction_instant.date(), 8, 0, 0).expect("approval instant");
    let correction_entries = vec![entry(
        99,
        "2031-01-15",
        BusinessKind::CashRevenue,
        CashFlowClass::Operating,
        &[
            ("1002", PostingSide::Debit, 300),
            ("6001", PostingSide::Credit, 300),
        ],
    )];
    let books_before_failure = serde_json::to_vec(&books).expect("账套可序列化");
    let closing_before_failure = serde_json::to_vec(&closing).expect("结账状态可序列化");
    let library_before_failure = library.clone();
    let rejected_publication_instant =
        CivilInstant::from_hms(correction_instant.date(), 17, 0, 0).expect("错误相位合法");
    let failure = library
        .correct_and_publish(
            &mut closing,
            &mut books,
            &MemberId(COMPANY.to_string()),
            IndustryPresentation::Industrial,
            CorrectionRequest {
                entries: correction_entries.clone(),
                reason: "遗漏现金收入更正".to_string(),
            },
            PublicationRequest {
                company: company.clone(),
                scope: scope.clone(),
                period: AccountingPeriod::from_ymd(2030, 12).expect("annual period"),
                kind: ReportKind::Annual,
                sequence: 2,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: correction_approval,
                published_at: rejected_publication_instant,
                origin: PublicationOrigin::Correction,
                supersedes: Some(original_publication_id),
            },
        )
        .expect_err("错误发布时点必须拒绝整笔更正");
    assert!(matches!(
        &failure,
        CorrectionPublicationError::Information { reason, cause }
            if reason == "遗漏现金收入更正"
                && matches!(
                    cause.as_ref(),
                    InformationError::PublicationOutsidePhase { published_at }
                        if *published_at == rejected_publication_instant
                )
    ));
    assert_eq!(
        serde_json::to_vec(&books).expect("账套可序列化"),
        books_before_failure
    );
    assert_eq!(
        serde_json::to_vec(&closing).expect("结账状态可序列化"),
        closing_before_failure
    );
    assert_eq!(library, library_before_failure);

    let (corrected, corrected_publication_id) = library
        .correct_and_publish(
            &mut closing,
            &mut books,
            &MemberId(COMPANY.to_string()),
            IndustryPresentation::Industrial,
            CorrectionRequest {
                entries: correction_entries,
                reason: "遗漏现金收入更正".to_string(),
            },
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
                supersedes: Some(original_publication_id),
            },
        )
        .expect("修复发布条件后同凭证可重试");
    assert_eq!(corrected.sequence, 2);

    // —— 历史不可覆写：原报告重复查询逐字节不变；更正报告关联原报告 ——
    let original_report_after_correction = library
        .report(original_publication_id, correction_instant)
        .expect("原报告仍可读取");
    assert_eq!(
        serde_json::to_string(original_report_after_correction).expect("原报告可序列化"),
        original_publication_bytes,
        "original published version must be byte-identical after the correction"
    );
    let corrected_publication = library
        .report(corrected_publication_id, correction_instant)
        .expect("更正报告可读取");
    assert_eq!(
        corrected_publication.supersedes,
        Some(original_publication_id)
    );
    assert_eq!(corrected_publication.reports.version.sequence, 2);
    assert_eq!(corrected_publication.reports.version.supersedes, Some(1));
    assert_ne!(
        corrected_publication.reports.income.cumulative.net_income,
        original_report_before_correction
            .reports
            .income
            .cumulative
            .net_income
    );

    // latest_for：更正后最新版本是更正报告（id 单调 + 查询按 as_of 过滤）。
    let latest = library
        .latest_report(
            &company,
            ReportKind::Annual,
            AccountingPeriod::from_ymd(2030, 12).expect("annual period"),
            correction_instant,
        )
        .expect("a latest version exists");
    assert_eq!(latest.id, corrected_publication_id);
    // 更正时点之前，最新版本仍是原报告。
    let latest_before = library
        .latest_report(
            &company,
            ReportKind::Annual,
            AccountingPeriod::from_ymd(2030, 12).expect("annual period"),
            annual_instant,
        )
        .expect("原报告在更正之前为最新版本");
    assert_eq!(latest_before.id, original_publication_id);

    // 结账登记簿原版本同样不可变，作为披露层的对照锚。
    let original_registry_report = closing
        .version(
            &scope,
            AccountingPeriod::from_ymd(2030, 12).expect("annual period"),
            ReportKind::Annual,
            1,
        )
        .expect("登记簿保留原报告");
    assert_eq!(
        serde_json::to_string(original_registry_report).expect("登记簿原报告可序列化"),
        serde_json::to_string(&original_report_before_correction.reports)
            .expect("已公布原 ReportSet 可序列化"),
        "published bytes mirror the immutable registry version"
    );
    // 净利差 = 更正分录 300 元（金样锚：更正报告 = 原报告 + 300.00）。
    assert_eq!(
        (corrected_publication
            .reports
            .income
            .cumulative
            .net_income
            .cents()
            - original_report_before_correction
                .reports
                .income
                .cumulative
                .net_income
                .cents()) as i128,
        30_000i128,
        "correction adjustment flows into the restated annual"
    );
}
