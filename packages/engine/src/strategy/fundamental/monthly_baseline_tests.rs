use crate::accounting::closing::ClosingEngine;
use crate::accounting::consolidation::{MemberId, ScopeId};
use crate::accounting::reports::{
    generate_report_set, IndustryPresentation, ReportKind, ReportRequest, ReportSource,
    ReportVersion, VersionKind,
};
use crate::accounting::{
    AccountingAmount, AccountingPeriod, Books, BusinessEventId, BusinessKind, CashFlowClass,
    JournalEntry, JournalLine, LedgerAccountId, PostingSide,
};
use crate::calendar::{CivilDate, CivilInstant};
use crate::company::{CompanyId, CompanyKind};
use crate::information::{
    AccountingPolicyRef, MonthlyReportDelay, MonthlyReportPreset, MonthlyReportSchedule,
    NpcInformationState, NpcObservationContext, PublicLibrary, PublicationOrigin,
    PublicationRequest, ScheduledReportKind,
};
use crate::strategy::{
    AnalysisProfile, AnalysisWeights, BeliefBook, BeliefCause, BeliefInputs, FundamentalMethod,
    InstitutionStyle, StrategyProfile,
};
use crate::{AccountId, StockCode};
use std::collections::BTreeMap;

fn instant(date: &str) -> CivilInstant {
    CivilInstant::from_hms(CivilDate::from_iso(date).unwrap(), 18, 0, 0).unwrap()
}

#[test]
fn known_monthly_material_refreshes_newly_acquired_annual_baseline_once() {
    let mut books = Books::new(crate::company::industrial::industrial_account_chart());
    for (source, date, credit_account, amount) in [
        (1, "2027-12-31", "4001", 100000),
        (2, "2028-12-15", "6001", 10000),
        (3, "2029-01-15", "6001", 1000),
        (4, "2029-12-15", "6001", 20000),
        (5, "2030-01-15", "6001", 1200),
    ] {
        books
            .post_batch(vec![JournalEntry {
                source: BusinessEventId::new(source),
                date: CivilDate::from_iso(date).unwrap(),
                kind: if source == 1 {
                    BusinessKind::OpeningBalance
                } else {
                    BusinessKind::CashRevenue
                },
                cash_flow: if source == 1 {
                    CashFlowClass::Financing
                } else {
                    CashFlowClass::Operating
                },
                lines: vec![
                    JournalLine {
                        account: LedgerAccountId("1002".into()),
                        side: PostingSide::Debit,
                        amount: AccountingAmount::from_cents(amount),
                    },
                    JournalLine {
                        account: LedgerAccountId(credit_account.into()),
                        side: PostingSide::Credit,
                        amount: AccountingAmount::from_cents(amount),
                    },
                ],
            }])
            .unwrap();
    }
    let member = MemberId("company".into());
    let company = CompanyId("company".into());
    let scope = ScopeId::Standalone(member.clone());
    let mut closing = ClosingEngine::new();
    let mut library = PublicLibrary::new();
    let mut annual_ids = Vec::new();
    for year in [2028, 2029] {
        let period = AccountingPeriod::from_ymd(year, 12).unwrap();
        closing
            .record(
                generate_report_set(ReportRequest {
                    period,
                    kind: ReportKind::Annual,
                    source: ReportSource::Standalone {
                        id: member.clone(),
                        books: &books,
                        industry: IndustryPresentation::Industrial,
                    },
                    version: ReportVersion {
                        sequence: 1,
                        supersedes: None,
                        kind: VersionKind::Original,
                    },
                    adjustments: &BTreeMap::new(),
                })
                .unwrap(),
            )
            .unwrap();
        let published_at =
            crate::information::scheduled_instant(ScheduledReportKind::Annual, year, 0).unwrap();
        annual_ids.push(
            library
                .publish_closed(
                    &closing,
                    PublicationRequest {
                        company: company.clone(),
                        scope: scope.clone(),
                        period,
                        kind: ReportKind::Annual,
                        sequence: 1,
                        policy: AccountingPolicyRef { chart_version: 2 },
                        approved_at: CivilInstant::from_hms(published_at.date(), 8, 0, 0).unwrap(),
                        published_at,
                        origin: PublicationOrigin::SeededPrehistory {
                            fiscal_year: year,
                            kind: ScheduledReportKind::Annual,
                            offset_days: 0,
                        },
                        supersedes: None,
                    },
                )
                .unwrap(),
        );
    }
    let subsidiary_books = books.clone();
    let consolidated_period = AccountingPeriod::from_ymd(2029, 12).unwrap();
    let consolidated_scope = ScopeId::Consolidated(member.clone());
    closing
        .record(
            generate_report_set(ReportRequest {
                period: consolidated_period,
                kind: ReportKind::Annual,
                source: ReportSource::Consolidated {
                    request: crate::accounting::consolidation::ConsolidationRequest {
                        root: member.clone(),
                        members: vec![
                            crate::accounting::consolidation::GroupMember {
                                spec: crate::accounting::consolidation::MemberSpec {
                                    id: member.clone(),
                                    group_parent: None,
                                    issued_shares: 100,
                                    parent_held_shares: 0,
                                },
                                books: &books,
                            },
                            crate::accounting::consolidation::GroupMember {
                                spec: crate::accounting::consolidation::MemberSpec {
                                    id: MemberId("subsidiary".into()),
                                    group_parent: Some(member.clone()),
                                    issued_shares: 100,
                                    parent_held_shares: 80,
                                },
                                books: &subsidiary_books,
                            },
                        ],
                        intercompany_balances: vec![],
                        intercompany_sales: vec![],
                    },
                },
                version: ReportVersion {
                    sequence: 1,
                    supersedes: None,
                    kind: VersionKind::Original,
                },
                adjustments: &BTreeMap::new(),
            })
            .unwrap(),
        )
        .unwrap();
    let consolidated = library
        .publish_closed(
            &closing,
            PublicationRequest {
                company: company.clone(),
                scope: consolidated_scope,
                period: consolidated_period,
                kind: ReportKind::Annual,
                sequence: 1,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: CivilInstant::from_hms(
                    CivilDate::from_iso("2030-03-20").unwrap(),
                    8,
                    0,
                    0,
                )
                .unwrap(),
                published_at: instant("2030-03-20"),
                origin: PublicationOrigin::SeededPrehistory {
                    fiscal_year: 2029,
                    kind: ScheduledReportKind::Annual,
                    offset_days: 0,
                },
                supersedes: None,
            },
        )
        .unwrap();
    let period = AccountingPeriod::from_ymd(2030, 1).unwrap();
    closing
        .close_month(
            &mut books,
            &member,
            IndustryPresentation::Industrial,
            period,
        )
        .unwrap();
    let schedule = MonthlyReportSchedule::Preset {
        preset: MonthlyReportPreset::FirstDayEvening,
        delay: MonthlyReportDelay::None,
    };
    let published_at = schedule.instant(period, 0).unwrap();
    let monthly = library
        .publish_closed(
            &closing,
            PublicationRequest {
                company: company.clone(),
                scope,
                period,
                kind: ReportKind::Monthly,
                sequence: 1,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: published_at,
                published_at,
                origin: PublicationOrigin::MonthlyDisclosure {
                    schedule,
                    delay_days: 0,
                    seeded: false,
                },
                supersedes: None,
            },
        )
        .unwrap();
    let account = AccountId(1);
    let stock = StockCode("600001".into());
    let mut information = NpcInformationState::new(account);
    let before = instant("2030-03-01");
    information
        .record_acquisition(account, &library, annual_ids[0], before)
        .unwrap();
    information
        .record_acquisition(account, &library, monthly, before)
        .unwrap();
    let analysis = AnalysisProfile::new(
        AnalysisWeights::new(10000, 0, 0, 0, 0).unwrap(),
        Some(FundamentalMethod::EarningsMultiple),
    )
    .unwrap();
    let mut rng = crate::session::SplitMix64::new(42);
    let mut book = BeliefBook::new(
        account,
        StrategyProfile::Institution(InstitutionStyle::DeepValue),
        analysis,
        &mut rng,
    );
    let context = NpcObservationContext::new(account, &information, &library, &()).unwrap();
    let inputs = BeliefInputs {
        ctx: &context,
        company: company.clone(),
        kind: CompanyKind::Industrial,
        total_issued_shares: 100,
        as_of_trading_day: 1,
    };
    book.apply_cause(
        &stock,
        BeliefCause::NewMaterial { report: monthly },
        &inputs,
    )
    .unwrap();
    assert_eq!(
        book.entry(&stock).unwrap().used_report_ids,
        vec![annual_ids[0], monthly]
    );
    let old_entry = book.entry(&stock).unwrap().clone();
    assert!(old_entry.forecast.growth_bp.is_some());
    assert!(matches!(
        old_entry.forecast.basis,
        super::ForecastBasis::InitialTwoYear { observed_bp: 2000 }
    ));
    let redundant_revision = super::revise_forecast(
        &old_entry.forecast,
        super::GrowthObservation::TwoYear(2000),
        super::revision_lambda_bp(super::capability_center(book.profile())),
        book.assumptions().growth_deviation_bp,
    );
    assert_ne!(redundant_revision, old_entry.forecast);
    book.apply_cause(
        &stock,
        BeliefCause::NewMaterial { report: monthly },
        &inputs,
    )
    .unwrap();
    assert_eq!(book.entry(&stock).unwrap(), &old_entry);
    assert!(library.report(annual_ids[1], instant("2030-03-21")).is_ok());
    assert!(information.observed_at_of(annual_ids[1]).is_none());
    let context = NpcObservationContext::new(account, &information, &library, &()).unwrap();
    let inputs = BeliefInputs {
        ctx: &context,
        company: company.clone(),
        kind: CompanyKind::Industrial,
        total_issued_shares: 100,
        as_of_trading_day: 2,
    };
    book.apply_cause(
        &stock,
        BeliefCause::NewMaterial { report: monthly },
        &inputs,
    )
    .unwrap();
    assert_eq!(book.entry(&stock).unwrap(), &old_entry);
    information
        .record_acquisition(account, &library, consolidated, instant("2030-03-21"))
        .unwrap();
    let context = NpcObservationContext::new(account, &information, &library, &()).unwrap();
    let inputs = BeliefInputs {
        ctx: &context,
        company: company.clone(),
        kind: CompanyKind::Industrial,
        total_issued_shares: 100,
        as_of_trading_day: 2,
    };
    book.apply_cause(
        &stock,
        BeliefCause::NewMaterial { report: monthly },
        &inputs,
    )
    .unwrap();
    assert_eq!(book.entry(&stock).unwrap(), &old_entry);
    information
        .record_acquisition(account, &library, annual_ids[1], instant("2030-03-21"))
        .unwrap();
    let context = NpcObservationContext::new(account, &information, &library, &()).unwrap();
    let preferred = super::update::preferred_own_report(&context, &company)
        .unwrap()
        .unwrap();
    assert_eq!(preferred, monthly);
    let inputs = BeliefInputs {
        ctx: &context,
        company,
        kind: CompanyKind::Industrial,
        total_issued_shares: 100,
        as_of_trading_day: 2,
    };
    book.apply_cause(
        &stock,
        BeliefCause::NewMaterial { report: preferred },
        &inputs,
    )
    .unwrap();
    assert_eq!(
        book.entry(&stock).unwrap().used_report_ids,
        vec![annual_ids[1], monthly]
    );
    assert_ne!(book.entry(&stock).unwrap().valuation, old_entry.valuation);
    assert_eq!(book.entry(&stock).unwrap().forecast, old_entry.forecast);
    let refreshed = book.entry(&stock).unwrap().clone();
    book.apply_cause(
        &stock,
        BeliefCause::NewMaterial { report: preferred },
        &inputs,
    )
    .unwrap();
    assert_eq!(book.entry(&stock).unwrap(), &refreshed);
}
