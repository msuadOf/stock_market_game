use super::*;
use crate::accounting::closing::ClosingEngine;
use crate::accounting::consolidation::{MemberId, ScopeId};
use crate::accounting::reports::{generate_report_set, IndustryPresentation, ReportKind, ReportRequest, ReportSource, ReportVersion, VersionKind};
use crate::accounting::{AccountingPeriod, Books};
use crate::calendar::{CivilDate, CivilInstant};
use crate::company::CompanyId;
use std::collections::BTreeMap;

fn fixture() -> (ClosingEngine, PublicationRequest) {
    let period = AccountingPeriod::from_ymd(2029, 12).unwrap();
    let books = Books::new(crate::company::industrial::industrial_account_chart());
    let member = MemberId("source-test".into());
    let report = generate_report_set(ReportRequest {
        period, kind: ReportKind::Annual,
        source: ReportSource::Standalone { id: member.clone(), books: &books, industry: IndustryPresentation::Industrial },
        version: ReportVersion { sequence: 1, supersedes: None, kind: VersionKind::Original },
        adjustments: &BTreeMap::new(),
    }).unwrap();
    let mut closing = ClosingEngine::new();
    closing.record(report).unwrap();
    let date = CivilDate::from_iso("2030-03-20").unwrap();
    let request = PublicationRequest {
        company: CompanyId("source-test".into()), scope: ScopeId::Standalone(member), period,
        kind: ReportKind::Annual, sequence: 1, policy: AccountingPolicyRef { chart_version: 2 },
        approved_at: CivilInstant::from_hms(date, 8, 0, 0).unwrap(),
        published_at: CivilInstant::from_hms(date, 18, 0, 0).unwrap(),
        origin: PublicationOrigin::ScheduledDisclosure { fiscal_year: 2029, kind: ScheduledReportKind::Annual, offset_days: 0 },
        supersedes: None,
    };
    (closing, request)
}

#[test]
fn simple_source_preserves_complete_common_reports_and_required_wire() {
    let (closing, request) = fixture();
    let now = request.published_at;
    let mut library = PublicLibrary::new();
    let id = library.publish_simple_closed(&closing, request).unwrap();
    let report = library.report(id, now).unwrap();
    assert_eq!(report.source, PublicationSource::SimpleGenerated);
    report.reports.validate().unwrap();
    let public = serde_json::to_value(crate::company::PublicReportSummary::from(report)).unwrap();
    assert_eq!(public["source"], "SimpleGenerated");
    assert!(public["financials"]["cash_flow"].is_object());
    assert!(public["accounting"].is_object());
    let mut encoded = serde_json::to_value(report).unwrap();
    encoded.as_object_mut().unwrap().remove("source");
    assert!(serde_json::from_value::<PublishedReport>(encoded).is_err());
}

#[test]
fn restored_correction_cannot_reference_an_absent_publication() {
    let (closing, request) = fixture();
    let mut library = PublicLibrary::new();
    library.publish_simple_closed(&closing, request).unwrap();
    let mut save = library.save();
    let correction = &mut save.reports[0];
    correction.id = PublicationId::new(1);
    correction.origin = PublicationOrigin::Correction;
    correction.supersedes = Some(PublicationId::new(0));
    correction.reports.version = ReportVersion { sequence: 2, supersedes: Some(1), kind: VersionKind::Correction { reason: "真实更正链接守卫".into() } };
    save.next_seq = 2;
    assert!(matches!(PublicLibrary::from_parts(save), Err(InformationError::CorrectionTargetUnknown { .. })));
}

#[test]
fn correction_cannot_change_the_original_model_source() {
    let (mut closing, request) = fixture();
    let mut library = PublicLibrary::new();
    let id = library.publish_simple_closed(&closing, request.clone()).unwrap();
    let mut corrected = library.report(id, request.published_at).unwrap().reports.clone();
    corrected.version = ReportVersion { sequence: 2, supersedes: Some(1), kind: VersionKind::Correction { reason: "来源不可漂移".into() } };
    closing.record(corrected).unwrap();
    let before = library.save();
    let mut correction = request;
    correction.sequence = 2;
    correction.origin = PublicationOrigin::Correction;
    correction.supersedes = Some(id);
    assert!(matches!(library.publish_closed(&closing, correction), Err(InformationError::CorrectionTargetMismatch { .. })));
    assert_eq!(library.save().next_seq, before.next_seq);
    assert_eq!(library.save().reports, before.reports);
}

#[test]
fn scheduled_original_cannot_hide_a_correction_version() {
    let (closing, request) = fixture();
    let mut library = PublicLibrary::new();
    library.publish_simple_closed(&closing, request).unwrap();
    let mut save = library.save();
    save.reports[0].reports.version = ReportVersion { sequence: 2, supersedes: Some(1), kind: VersionKind::Correction { reason: "没有公开原版本的更正".into() } };
    assert!(PublicLibrary::from_parts(save).is_err());
}

#[test]
fn correction_private_predecessor_need_not_equal_public_predecessor() {
    let (closing, request) = fixture();
    let mut library = PublicLibrary::new();
    library.publish_simple_closed(&closing, request).unwrap();
    let mut save = library.save();
    let mut correction = save.reports[0].clone();
    correction.id = PublicationId::new(1);
    correction.origin = PublicationOrigin::Correction;
    correction.supersedes = Some(PublicationId::new(0));
    correction.reports.version = ReportVersion { sequence: 3, supersedes: Some(2), kind: VersionKind::Correction { reason: "中间含未公开的私有版本".into() } };
    save.reports.push(correction);
    save.next_seq = 2;
    let restored = PublicLibrary::from_parts(save).unwrap();
    assert_eq!(restored.save().reports[1].reports.version.supersedes, Some(2));
    assert_eq!(restored.save().reports[1].source, PublicationSource::SimpleGenerated);
}

fn simple_model(prehistory_periods: u16) -> crate::company::CompanySystem {
    simple_model_with_cycle(prehistory_periods, crate::company::simple::period::SettlementCycle::Monthly)
}

fn simple_model_with_cycle(prehistory_periods: u16, settlement_cycle: crate::company::simple::period::SettlementCycle) -> crate::company::CompanySystem {
    use crate::accounting::{AccountingAmount, JournalLine, LedgerAccountId, PostingSide, TaxPolicy, VatPolicy, IncomeTaxPolicy};
    use crate::company::{CompanyKind, CompanySpec, IndustryId, CompanySystem};
    use crate::company::config::CompanySystemConfig;
    use crate::company::simple::{SimpleConfig, SimpleCompanyConfig, SimpleEnvironmentConfig};
    use crate::company::simple::finance::{SimpleBookDisplayConfig, SimpleFinanceConfig, SimpleSummaryRule};
    use crate::company::simple::period::{AnnualTrendConfig, PeriodGenerationParameters, PeriodNoiseConfig, PeriodVariableExpenseRule, SettlementCycle};
    let noise = PeriodNoiseConfig { monthly_bp: 0, quarterly_bp: 0, half_year_bp: 0, annual_bp: 0 };
    let company = CompanyId("empty-simple".into());
    let start = CivilDate::from_iso("2030-01-01").unwrap();
    let system = CompanySystem::create(vec![CompanySpec {
        id: company.clone(), name: "空历史简单公司".into(), industry: IndustryId("虚拟行业".into()),
        kind: CompanyKind::Bank, listed_stock: None, issued_shares: 1000, group_parent: None,
    }], CompanySystemConfig::Simple(SimpleConfig {
        environment: SimpleEnvironmentConfig { initial_change_bp: 0, persistence_bp: 0, noise: noise.clone() },
        prehistory_periods,
        settlement_cycle,
        companies: vec![SimpleCompanyConfig {
            company,
            kind: CompanyKind::Bank,
            generation: PeriodGenerationParameters {
                initial_revenue: AccountingAmount::from_cents(1000), initial_fixed_expense: AccountingAmount::from_cents(500),
                revenue_trend: AnnualTrendConfig::Fixed { annual_growth_bp: 0 }, demand_sensitivity_bp: 0, revenue_noise: noise.clone(),
                fixed_expense_trend: AnnualTrendConfig::Fixed { annual_growth_bp: 0 }, fixed_expense_noise: noise.clone(),
                variable_expense: PeriodVariableExpenseRule::RevenueRatio { ratio_bp: 0, noise },
            },
            finance: SimpleFinanceConfig {
                opening_lines: vec![
                    JournalLine { account: LedgerAccountId("1003".into()), side: PostingSide::Debit, amount: AccountingAmount::from_cents(100000) },
                    JournalLine { account: LedgerAccountId("4001".into()), side: PostingSide::Credit, amount: AccountingAmount::from_cents(100000) },
                ],
                tax_policy: TaxPolicy { version: 1, vat: VatPolicy { output_rate_bp: 0, input_rate_bp: 0, deductible_share_bp: 10000 }, income_tax: IncomeTaxPolicy { rate_bp: 2500, loss_carryforward_years: 5 } },
                summary_rule: SimpleSummaryRule::ReceivableRevenuePayableExpenses,
                book_display: SimpleBookDisplayConfig::DEFAULT,
            },
            preferences: crate::company::simple::SimpleCompanyPreferences::none(),
        }],
    }), start, 17).unwrap();
    system
}

fn monthly_availability_query() -> crate::company::PublicReportAvailabilityQuery {
    crate::company::PublicReportAvailabilityQuery {
        company_id: "empty-simple".into(), period_end: "2029-12-31".into(),
        kind: crate::company::PublicReportKind::Monthly,
        scope: crate::company::PublicReportScope::Standalone { entity_id: "empty-simple".into() },
    }
}

#[test]
fn public_period_availability_does_not_expose_unpublished_full_statements() {
    use crate::company::{PublicReportAvailability, PublicReportUnavailableReason};
    let system = simple_model(1);
    let query = monthly_availability_query();
    let day = CivilDate::from_iso("2030-01-01").unwrap();
    let before = CivilInstant::from_hms(day, 17, 59, 59).unwrap();
    let published = CivilInstant::from_hms(day, 18, 0, 0).unwrap();
    let frequency = ReportFrequency::Monthly { schedule: MonthlyReportSchedule::Preset {
        preset: MonthlyReportPreset::FirstDayEvening, delay: MonthlyReportDelay::None,
    } };
    let mut library = PublicLibrary::new();
    let hidden = library.query_report_availability(&system, &query, frequency, before).unwrap();
    assert!(matches!(hidden, PublicReportAvailability::Unavailable { reason: PublicReportUnavailableReason::NotYetPublished }));
    let wire = serde_json::to_value(hidden).unwrap();
    assert_eq!(wire.as_object().unwrap().len(), 2);
    assert!(wire.get("report").is_none());
    publish_simple_scheduled(&system, frequency, 17, None, published, &mut library, false).unwrap();
    let PublicReportAvailability::Available { report } = library.query_report_availability(&system, &query, frequency, published).unwrap() else { panic!("已公开同期间材料应可查询") };
    assert_eq!(report.period, query.period_end);
    assert_eq!(report.source, PublicationSource::SimpleGenerated);
    assert!(matches!(library.query_report_availability(&system, &query, frequency, before).unwrap(), PublicReportAvailability::Unavailable { reason: PublicReportUnavailableReason::NotYetPublished }));
}

#[test]
fn long_generation_period_does_not_fabricate_short_period_reports() {
    use crate::company::{PublicReportAvailability, PublicReportUnavailableReason};
    let system = simple_model_with_cycle(1, crate::company::simple::period::SettlementCycle::Annual);
    let library = PublicLibrary::new();
    let frequency = ReportFrequency::Monthly { schedule: MonthlyReportSchedule::Preset {
        preset: MonthlyReportPreset::FirstDayEvening, delay: MonthlyReportDelay::None,
    } };
    let now = CivilInstant::from_hms(CivilDate::from_iso("2030-01-01").unwrap(), 18, 0, 0).unwrap();
    assert!(matches!(library.query_report_availability(&system, &monthly_availability_query(), frequency, now).unwrap(), PublicReportAvailability::Unavailable { reason: PublicReportUnavailableReason::PeriodNotRepresented }));
    let mut future = monthly_availability_query();
    future.period_end = "2030-01-31".into();
    assert!(matches!(library.query_report_availability(&system, &future, frequency, now).unwrap(), PublicReportAvailability::Unavailable { reason: PublicReportUnavailableReason::NotYetSettled }));
    let mut old = monthly_availability_query();
    old.period_end = "2028-12-31".into();
    assert!(matches!(library.query_report_availability(&system, &old, frequency, now).unwrap(), PublicReportAvailability::Unavailable { reason: PublicReportUnavailableReason::BeforeOpening }));
    assert!(matches!(library.query_report_availability(&system, &monthly_availability_query(), ReportFrequency::Quarterly, now).unwrap(), PublicReportAvailability::Unavailable { reason: PublicReportUnavailableReason::NotScheduled }));
    let mut scope = monthly_availability_query();
    scope.scope = crate::company::PublicReportScope::Consolidated { root_entity_id: scope.company_id.clone() };
    assert!(matches!(library.query_report_availability(&system, &scope, frequency, now).unwrap(), PublicReportAvailability::Unavailable { reason: PublicReportUnavailableReason::ScopeNotRepresented }));
}

#[test]
fn public_period_query_rejects_bad_dates_scope_identity_and_extra_scope_fields() {
    let system = simple_model(0);
    let library = PublicLibrary::new();
    let now = CivilInstant::from_hms(CivilDate::from_iso("2030-01-01").unwrap(), 18, 0, 0).unwrap();
    for date in ["2029-12-30", "not-a-date"] {
        let mut query = monthly_availability_query();
        query.period_end = date.into();
        assert!(library.query_report_availability(&system, &query, ReportFrequency::Quarterly, now).is_err());
    }
    let mut query = monthly_availability_query();
    query.scope = crate::company::PublicReportScope::Standalone { entity_id: "other".into() };
    assert!(library.query_report_availability(&system, &query, ReportFrequency::Quarterly, now).is_err());
    let mut value = serde_json::to_value(monthly_availability_query()).unwrap();
    value["scope"]["Standalone"]["unknown"] = serde_json::json!(true);
    assert!(serde_json::from_value::<crate::company::PublicReportAvailabilityQuery>(value).is_err());
}

#[test]
fn empty_simple_history_is_available_but_has_no_publications() {
    let system = simple_model(0);
    let start = CivilDate::from_iso("2030-01-01").unwrap();
    let mut library = PublicLibrary::new();
    let through = CivilInstant::new(start.prev().unwrap(), 86399).unwrap();
    assert!(publish_simple_scheduled(&system, ReportFrequency::Quarterly, 17, None, through, &mut library, true).unwrap().is_empty());
    assert_eq!(library.report_count(), 0);
    assert_eq!(library.save().next_seq, 0);
}

#[test]
fn simple_monthly_disclosure_obeys_public_time_and_repeated_scans_are_idempotent() {
    let system = simple_model(1);
    let day = CivilDate::from_iso("2030-01-01").unwrap();
    let before = CivilInstant::from_hms(day, 17, 59, 59).unwrap();
    let published = CivilInstant::from_hms(day, 18, 0, 0).unwrap();
    let frequency = ReportFrequency::Monthly { schedule: MonthlyReportSchedule::Preset {
        preset: MonthlyReportPreset::FirstDayEvening, delay: MonthlyReportDelay::None,
    } };
    let mut library = PublicLibrary::new();
    assert!(publish_simple_scheduled(&system, frequency, 17, None, before, &mut library, true).unwrap().is_empty());
    assert_eq!(library.report_count(), 0);
    let ids = publish_simple_scheduled(&system, frequency, 17, None, published, &mut library, false).unwrap();
    assert_eq!(ids.len(), 1);
    let report = library.report(ids[0], published).unwrap();
    assert_eq!(report.source, PublicationSource::SimpleGenerated);
    assert_eq!(report.reports.kind, ReportKind::Monthly);
    assert_eq!(report.reports.period, AccountingPeriod::from_ymd(2029, 12).unwrap());
    assert_eq!(report.published_at, published);
    report.reports.validate().unwrap();
    assert!(matches!(library.report(ids[0], before), Err(InformationError::EarlyRead { .. })));
    let saved = library.save();
    assert!(publish_simple_scheduled(&system, frequency, 17, None, published, &mut library, false).unwrap().is_empty());
    assert_eq!(library.save().reports, saved.reports);
    assert_eq!(library.save().next_seq, saved.next_seq);
    assert!(publish_simple_scheduled(&system, frequency, 17, Some(published), before, &mut library, false).is_err());
    assert_eq!(library.save().reports, saved.reports);
}
