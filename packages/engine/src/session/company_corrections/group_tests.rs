use super::*;
use crate::accounting::consolidation::{MemberId, ScopeId};
use crate::accounting::closing::{ClosingEngine, CorrectionRequest};
use crate::accounting::reports::{IndustryPresentation, ReportKind, VersionKind};
use crate::accounting::{
    AccountingPeriod, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, JournalLine,
    LedgerAccountId, PostingSide,
};
use crate::calendar::{CivilDate, CivilInstant};
use crate::company::operations::{
    CompanyOperations, CompanyOperationsConfig, OperatingReportCorrection,
};
use crate::company::CompanyId;
use crate::information::{
    scheduled_instant, AccountingPolicyRef, PublicationOrigin, PublicationRequest, PublicLibrary,
    PublicationId, ScheduledReportKind,
};
use crate::session::company_groups::{
    ensure_group_report, publish_group_scheduled, GroupHolding, GroupStructure,
};

fn ops() -> (CompanyOperations, GroupStructure) {
    let mut shocks = crate::company::events::ShockParams::current_default_parameters();
    shocks.market_candidate_bp = 0;
    shocks.industry_candidate_bp = 0;
    shocks.company_candidate_bp = 0;
    let operations = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 7181,
            shock_params: shocks,
            companies: vec![
                super::company("root", None),
                super::company("sub", Some("root")),
            ],
        },
        date("2030-05-01"),
    )
    .unwrap();
    let group = GroupStructure {
        root: crate::company::CompanyId("root".into()),
        holdings: vec![GroupHolding {
            company: crate::company::CompanyId("sub".into()),
            parent_held_shares: 80,
        }],
    };
    (operations, group)
}

fn date(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

fn yuan(value: i128) -> crate::accounting::AccountingAmount {
    crate::accounting::AccountingAmount::from_cents(value * 100)
}

fn annual_period() -> AccountingPeriod {
    AccountingPeriod::from_ymd(2029, 12).unwrap()
}

fn report_scope(company: &str) -> ScopeId {
    ScopeId::Standalone(MemberId(company.into()))
}

fn prepare_annual_reports(
    operations: &mut CompanyOperations,
    group: &GroupStructure,
    closing: &mut ClosingEngine,
    library: &mut PublicLibrary,
    publish_group: bool,
) -> Vec<PublicationId> {
    let period = annual_period();
    let mut ids = Vec::new();
    for id in ["root", "sub"] {
        let member = MemberId(id.into());
        let company = operations.company_mut(&CompanyId(id.into())).unwrap();
        closing
            .close_year(
                company.books_mut(),
                &member,
                IndustryPresentation::Industrial,
                2029,
            )
            .unwrap();
        let published_at = scheduled_instant(ScheduledReportKind::Annual, 2029, 0).unwrap();
        ids.push(
            library
                .publish_closed(
                    closing,
                    PublicationRequest {
                        company: CompanyId(id.into()),
                        scope: report_scope(id),
                        period,
                        kind: ReportKind::Annual,
                        sequence: 1,
                        policy: AccountingPolicyRef { chart_version: 2 },
                        approved_at: CivilInstant::from_hms(published_at.date(), 8, 0, 0)
                            .unwrap(),
                        published_at,
                        origin: PublicationOrigin::SeededPrehistory {
                            fiscal_year: 2029,
                            kind: ScheduledReportKind::Annual,
                            offset_days: 0,
                        },
                        supersedes: None,
                    },
                )
                .unwrap(),
        );
    }
    ensure_group_report(group, operations, closing, period, ReportKind::Annual).unwrap();
    if publish_group {
        let published_at = scheduled_instant(ScheduledReportKind::Annual, 2029, 0).unwrap();
        ids.push(
            library
                .publish_closed(
                    closing,
                    PublicationRequest {
                        company: group.root.clone(),
                        scope: ScopeId::Consolidated(MemberId("root".into())),
                        period,
                        kind: ReportKind::Annual,
                        sequence: 1,
                        policy: AccountingPolicyRef { chart_version: 2 },
                        approved_at: CivilInstant::from_hms(published_at.date(), 8, 0, 0)
                            .unwrap(),
                        published_at,
                        origin: PublicationOrigin::SeededPrehistory {
                            fiscal_year: 2029,
                            kind: ScheduledReportKind::Annual,
                            offset_days: 0,
                        },
                        supersedes: None,
                    },
                )
                .unwrap(),
        );
    }
    ids
}

fn correction(
    original: PublicationId,
    company: &str,
    source: u64,
    posted_on: CivilDate,
    published_at: CivilInstant,
) -> (CorrectionRequest, PublicationRequest) {
    let scope = report_scope(company);
    let request = PublicationRequest {
        company: CompanyId(company.into()),
        scope,
        period: annual_period(),
        kind: ReportKind::Annual,
        sequence: 2,
        policy: AccountingPolicyRef { chart_version: 2 },
        approved_at: CivilInstant::from_hms(published_at.date(), 8, 0, 0).unwrap(),
        published_at,
        origin: PublicationOrigin::Correction,
        supersedes: Some(original),
    };
    (
        CorrectionRequest {
            reason: "补记已确认的前期管理费用".into(),
            entries: vec![JournalEntry {
                source: BusinessEventId::new(source),
                date: posted_on,
                kind: BusinessKind::CashExpense,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    JournalLine {
                        account: LedgerAccountId("6602".into()),
                        side: PostingSide::Debit,
                        amount: yuan(100),
                    },
                    JournalLine {
                        account: LedgerAccountId("1002".into()),
                        side: PostingSide::Credit,
                        amount: yuan(100),
                    },
                ],
            }],
        },
        request,
    )
}

#[test]
fn low_level_group_corrections_preserve_public_history_and_minority_attribution() {
    let (mut operations, group) = ops();
    let mut closing = ClosingEngine::new();
    let mut library = PublicLibrary::new();
    let originals = prepare_annual_reports(
        &mut operations,
        &group,
        &mut closing,
        &mut library,
        true,
    );
    let original_reports = library.save().reports;
    let child_original = originals[1];
    let group_original = originals[2];
    let posted_on = date("2031-01-05");
    let published_at = CivilInstant::from_hms(posted_on, 18, 0, 0).unwrap();
    let (request, publication) =
        correction(child_original, "sub", 900_000_001, posted_on, published_at);
    operations
        .correct_company_report_for_session(
            &CompanyId("sub".into()),
            OperatingReportCorrection {
                closing: &mut closing,
                library: &mut library,
                correction: request,
                publication,
                posted_on,
            },
        )
        .unwrap();
    let generated = crate::session::company_groups::refresh_group_reports_after_correction(
        &group,
        &operations,
        &mut closing,
        &mut library,
        annual_period(),
        "补记已确认的前期管理费用",
        CivilInstant::from_hms(posted_on, 8, 0, 0).unwrap(),
        published_at,
    )
    .unwrap();
    let child_corrected = library.report(generated[0], published_at).unwrap().clone();
    assert_eq!(child_corrected.reports.version.sequence, 2);
    assert_eq!(child_corrected.supersedes, Some(group_original));
    assert_eq!(child_corrected.reports.income.cumulative.net_income, yuan(-75));
    assert_eq!(child_corrected.reports.income.minority_net_income, Some(yuan(-15)));
    assert_eq!(child_corrected.reports.income.net_income_to_parent, Some(yuan(-60)));
    assert_eq!(child_corrected.reports.balance_sheet.total_equity, yuan(1925));
    assert_eq!(child_corrected.reports.balance_sheet.equity_to_parent, yuan(1740));

    let (request, publication) =
        correction(originals[0], "root", 900_000_002, posted_on, published_at);
    operations
        .correct_company_report_for_session(
            &CompanyId("root".into()),
            OperatingReportCorrection {
                closing: &mut closing,
                library: &mut library,
                correction: request,
                publication,
                posted_on,
            },
        )
        .unwrap();
    let generated = crate::session::company_groups::refresh_group_reports_after_correction(
        &group,
        &operations,
        &mut closing,
        &mut library,
        annual_period(),
        "补记已确认的前期管理费用",
        CivilInstant::from_hms(posted_on, 8, 0, 0).unwrap(),
        published_at,
    )
    .unwrap();
    let root_corrected = library.report(generated[0], published_at).unwrap();
    assert_eq!(root_corrected.reports.version.sequence, 3);
    assert_eq!(root_corrected.supersedes, Some(child_corrected.id));
    assert_eq!(root_corrected.reports.income.cumulative.net_income, yuan(-150));
    assert_eq!(root_corrected.reports.income.minority_net_income, Some(yuan(-15)));
    assert_eq!(root_corrected.reports.income.net_income_to_parent, Some(yuan(-135)));
    assert_eq!(root_corrected.reports.balance_sheet.equity_to_parent, yuan(1665));
    for original in &original_reports {
        assert_eq!(library.report(original.id, published_at).unwrap(), original);
    }
    assert_eq!(library.report(child_corrected.id, published_at).unwrap(), &child_corrected);
    for id in ["root", "sub"] {
        assert_eq!(
            operations
                .company(&CompanyId(id.into()))
                .unwrap()
                .books()
                .books()
                .ledger()
                .cash_total()
                .unwrap(),
            yuan(900)
        );
    }
}

#[test]
fn low_level_private_group_original_is_rebuilt_before_scheduled_publication() {
    let (mut operations, group) = ops();
    let mut closing = ClosingEngine::new();
    let mut library = PublicLibrary::new();
    let originals = prepare_annual_reports(
        &mut operations,
        &group,
        &mut closing,
        &mut library,
        false,
    );
    for id in ["root", "sub"] {
        let company = operations.company_mut(&CompanyId(id.into())).unwrap();
        closing
            .close_year(
                company.books_mut(),
                &MemberId(id.into()),
                IndustryPresentation::Industrial,
                2030,
            )
            .unwrap();
    }
    let later_period = AccountingPeriod::from_ymd(2030, 12).unwrap();
    ensure_group_report(
        &group,
        &operations,
        &mut closing,
        later_period,
        ReportKind::Annual,
    )
    .unwrap();
    assert!(!library.save().reports.iter().any(|report| {
        report.reports.scope == ScopeId::Consolidated(MemberId("root".into()))
    }));
    let scope = ScopeId::Consolidated(MemberId("root".into()));
    let old = closing
        .versions(&scope, later_period, ReportKind::Annual)
        .last()
        .unwrap()
        .clone();
    let posted_on = date("2031-01-05");
    let published_at = CivilInstant::from_hms(posted_on, 18, 0, 0).unwrap();
    let (request, publication) =
        correction(originals[1], "sub", 900_000_003, posted_on, published_at);
    operations
        .correct_company_report_for_session(
            &CompanyId("sub".into()),
            OperatingReportCorrection {
                closing: &mut closing,
                library: &mut library,
                correction: request,
                publication,
                posted_on,
            },
        )
        .unwrap();
    crate::session::company_groups::refresh_group_reports_after_correction(
        &group,
        &operations,
        &mut closing,
        &mut library,
        annual_period(),
        "补记已确认的前期管理费用",
        CivilInstant::from_hms(posted_on, 8, 0, 0).unwrap(),
        published_at,
    )
    .unwrap();
    let versions = closing.versions(&scope, later_period, ReportKind::Annual);
    assert_eq!(versions[0], old);
    let latest = versions.last().unwrap();
    assert_eq!(latest.version.sequence, 2);
    assert_eq!(latest.version.kind, VersionKind::Original);
    assert_eq!(latest.version.supersedes, None);
    let expected = latest.clone();
    let offset = crate::information::stable_company_offset(operations.seed, &group.root);
    let instant = scheduled_instant(ScheduledReportKind::Annual, 2030, offset).unwrap();
    let id = publish_group_scheduled(
        &group,
        &operations,
        &mut closing,
        &mut library,
        instant,
        2030,
        ScheduledReportKind::Annual,
        crate::information::ReportFrequency::Quarterly,
    )
    .unwrap();
    assert_eq!(library.report(id, instant).unwrap().reports, expected);
}

#[test]
fn low_level_failed_group_refresh_rolls_back_candidate_and_allows_same_source_retry() {
    let (mut operations, group) = ops();
    let mut closing = ClosingEngine::new();
    let mut library = PublicLibrary::new();
    let originals = prepare_annual_reports(
        &mut operations,
        &group,
        &mut closing,
        &mut library,
        true,
    );
    let posted_on = date("2031-01-05");
    let published_at = CivilInstant::from_hms(posted_on, 18, 0, 0).unwrap();
    let before_operations = operations.clone();
    let before_closing = closing.clone();
    let before_library = library.clone();

    let mut candidate_operations = operations.clone();
    let mut candidate_closing = closing.clone();
    let mut candidate_library = library.clone();
    let (request, publication) =
        correction(originals[1], "sub", 900_000_004, posted_on, published_at);
    candidate_operations
        .correct_company_report_for_session(
            &CompanyId("sub".into()),
            OperatingReportCorrection {
                closing: &mut candidate_closing,
                library: &mut candidate_library,
                correction: request,
                publication,
                posted_on,
            },
        )
        .unwrap();
    let mut invalid_group = group.clone();
    invalid_group.holdings[0].company = CompanyId("missing-member".into());
    assert!(crate::session::company_groups::refresh_group_reports_after_correction(
        &invalid_group,
        &candidate_operations,
        &mut candidate_closing,
        &mut candidate_library,
        annual_period(),
        "补记已确认的前期管理费用",
        CivilInstant::from_hms(posted_on, 8, 0, 0).unwrap(),
        published_at,
    )
    .is_err());
    assert_eq!(operations, before_operations);
    assert_eq!(closing, before_closing);
    assert_eq!(library, before_library);

    let (request, publication) =
        correction(originals[1], "sub", 900_000_004, posted_on, published_at);
    operations
        .correct_company_report_for_session(
            &CompanyId("sub".into()),
            OperatingReportCorrection {
                closing: &mut closing,
                library: &mut library,
                correction: request,
                publication,
                posted_on,
            },
        )
        .unwrap();
    let publications = crate::session::company_groups::refresh_group_reports_after_correction(
        &group,
        &operations,
        &mut closing,
        &mut library,
        annual_period(),
        "补记已确认的前期管理费用",
        CivilInstant::from_hms(posted_on, 8, 0, 0).unwrap(),
        published_at,
    )
    .unwrap();
    assert_eq!(publications.len(), 1);
    assert_eq!(
        library
            .report(publications[0], published_at)
            .unwrap()
            .supersedes,
        Some(originals[2])
    );
}

#[test]
fn low_level_group_report_uses_real_closed_members_and_keeps_consolidated_scope() {
    let (mut operations, group) = ops();
    let period = AccountingPeriod::from_ymd(2030, 4).unwrap();
    let mut closing = crate::accounting::closing::ClosingEngine::new();
    for id in ["root", "sub"] {
        let member = MemberId(id.into());
        closing
            .close_month(
                operations
                    .company_mut(&crate::company::CompanyId(id.into()))
                    .unwrap()
                    .books_mut(),
                &member,
                crate::accounting::reports::IndustryPresentation::Industrial,
                period,
            )
            .unwrap();
    }
    ensure_group_report(&group, &operations, &mut closing, period, ReportKind::Monthly).unwrap();
    let report = closing
        .version(
            &ScopeId::Consolidated(MemberId("root".into())),
            period,
            ReportKind::Monthly,
            1,
        )
        .unwrap();
    report.validate().unwrap();
    assert_eq!(report.scope, ScopeId::Consolidated(MemberId("root".into())));
    assert_eq!(report.income.minority_net_income, Some(crate::accounting::AccountingAmount::ZERO));
}

#[test]
fn low_level_group_publication_requires_real_closing_and_uses_group_scope() {
    let (mut operations, group) = ops();
    let mut closing = crate::accounting::closing::ClosingEngine::new();
    let mut library = PublicLibrary::new();
    let period = AccountingPeriod::from_ymd(2030, 4).unwrap();
    let instant = crate::calendar::CivilInstant::from_hms(
        crate::calendar::CivilDate::from_iso("2030-05-02").unwrap(),
        9,
        50,
        0,
    )
    .unwrap();
    let result = publish_group_scheduled(
        &group,
        &operations,
        &mut closing,
        &mut library,
        instant,
        2030,
        crate::information::ScheduledReportKind::Monthly { month: 4 },
        crate::information::ReportFrequency::Monthly {
            schedule: crate::information::MonthlyReportSchedule::Custom {
                day: 2,
                second_of_day: 35_400,
                delay: crate::information::MonthlyReportDelay::None,
            },
        },
    );
    assert!(result.is_err());
    assert_eq!(library.report_count(), 0);
    for id in ["root", "sub"] {
        let company = operations
            .company_mut(&crate::company::CompanyId(id.into()))
            .unwrap();
        closing
            .close_month(
                company.books_mut(),
                &MemberId(id.into()),
                crate::accounting::reports::IndustryPresentation::Industrial,
                period,
            )
            .unwrap();
    }
    let id = publish_group_scheduled(
        &group,
        &operations,
        &mut closing,
        &mut library,
        instant,
        2030,
        crate::information::ScheduledReportKind::Monthly { month: 4 },
        crate::information::ReportFrequency::Monthly {
            schedule: crate::information::MonthlyReportSchedule::Custom {
                day: 2,
                second_of_day: 35_400,
                delay: crate::information::MonthlyReportDelay::None,
            },
        },
    )
    .unwrap();
    let published = library.report(id, instant).unwrap();
    assert_eq!(published.reports.scope, ScopeId::Consolidated(MemberId("root".into())));
    assert_eq!(published.id, PublicationId::new(0));
}
