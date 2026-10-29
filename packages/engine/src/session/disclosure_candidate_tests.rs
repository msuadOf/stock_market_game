use super::*;
use crate::accounting::consolidation::{MemberId, ScopeId};
use crate::accounting::reports::{ReportKind, ReportVersion, VersionKind};
use crate::accounting::AccountingPeriod;
use crate::information::{
    AccountingPolicyRef, MonthlyReportDelay, MonthlyReportSchedule, PublicationOrigin,
    PublicationRequest,
};

fn simple_model(prehistory_periods: u16) -> crate::company::CompanySystem {
    simple_model_with_cycle(
        prehistory_periods,
        crate::company::simple::period::SettlementCycle::Monthly,
    )
}

fn simple_model_with_cycle(
    prehistory_periods: u16,
    settlement_cycle: crate::company::simple::period::SettlementCycle,
) -> crate::company::CompanySystem {
    use crate::accounting::{
        AccountingAmount, IncomeTaxPolicy, JournalLine, LedgerAccountId, PostingSide, TaxPolicy,
        VatPolicy,
    };
    use crate::company::config::CompanySystemConfig;
    use crate::company::simple::finance::{
        SimpleBookDisplayConfig, SimpleFinanceConfig, SimpleSummaryRule,
    };
    use crate::company::simple::period::{
        AnnualTrendConfig, PeriodGenerationParameters, PeriodNoiseConfig,
        PeriodVariableExpenseRule, SettlementCycle,
    };
    use crate::company::simple::{SimpleCompanyConfig, SimpleConfig, SimpleEnvironmentConfig};
    use crate::company::{CompanyKind, CompanySpec, CompanySystem, IndustryId};
    let noise = PeriodNoiseConfig {
        monthly_bp: 0,
        quarterly_bp: 0,
        half_year_bp: 0,
        annual_bp: 0,
    };
    // spec code "empty-simple" 在单公司集合内分配数值 id 0。
    let company = CompanyId(0);
    let start = CivilDate::from_iso("2030-01-01").unwrap();
    let system = CompanySystem::create(
        vec![CompanySpec {
            code: "empty-simple".into(),
            name: "空历史简单公司".into(),
            industry: IndustryId("虚拟行业".into()),
            kind: CompanyKind::Bank,
            listed_stock: Some(crate::account::StockCode("600000".into())),
            issued_shares: 1000,
            group_parent: None,
        }],
        CompanySystemConfig::Simple(SimpleConfig {
            environment: SimpleEnvironmentConfig {
                initial_change_bp: 0,
                persistence_bp: 0,
                noise: noise.clone(),
            },
            prehistory_periods,
            settlement_cycle,
            companies: vec![SimpleCompanyConfig {
                company,
                kind: CompanyKind::Bank,
                generation: PeriodGenerationParameters {
                    initial_revenue: AccountingAmount::from_cents(1000),
                    initial_fixed_expense: AccountingAmount::from_cents(500),
                    revenue_trend: AnnualTrendConfig::Fixed {
                        annual_growth_bp: 0,
                    },
                    demand_sensitivity_bp: 0,
                    revenue_noise: noise.clone(),
                    fixed_expense_trend: AnnualTrendConfig::Fixed {
                        annual_growth_bp: 0,
                    },
                    fixed_expense_noise: noise.clone(),
                    variable_expense: PeriodVariableExpenseRule::RevenueRatio {
                        ratio_bp: 0,
                        noise,
                    },
                },
                finance: SimpleFinanceConfig {
                    opening_lines: vec![
                        JournalLine {
                            account: LedgerAccountId("1003".into()),
                            side: PostingSide::Debit,
                            amount: AccountingAmount::from_cents(100000),
                        },
                        JournalLine {
                            account: LedgerAccountId("4001".into()),
                            side: PostingSide::Credit,
                            amount: AccountingAmount::from_cents(100000),
                        },
                    ],
                    tax_policy: TaxPolicy {
                        version: 1,
                        vat: VatPolicy {
                            output_rate_bp: 0,
                            input_rate_bp: 0,
                            deductible_share_bp: 10000,
                        },
                        income_tax: IncomeTaxPolicy {
                            rate_bp: 2500,
                            loss_carryforward_years: 5,
                        },
                    },
                    summary_rule: SimpleSummaryRule::ReceivableRevenuePayableExpenses,
                    book_display: SimpleBookDisplayConfig::DEFAULT,
                },
                preferences: crate::company::simple::SimpleCompanyPreferences::none(),
            }],
        }),
        start,
        17,
    )
    .unwrap();
    system
}

fn instant(second: u32) -> CivilInstant {
    CivilInstant::new(CivilDate::from_iso("2030-01-01").unwrap(), second).unwrap()
}
fn frequency(second: u32) -> ReportFrequency {
    ReportFrequency::Monthly {
        schedule: MonthlyReportSchedule::Custom {
            day: 1,
            second_of_day: second,
            delay: MonthlyReportDelay::None,
        },
    }
}
fn request(second: u32) -> PublicationRequest {
    PublicationRequest {
        company: CompanyId(0),
        scope: ScopeId::Standalone(MemberId(0)),
        period: AccountingPeriod::from_ymd(2029, 12).unwrap(),
        kind: ReportKind::Monthly,
        sequence: 1,
        policy: AccountingPolicyRef { chart_version: 3 },
        approved_at: instant(second.min(8 * 3600)),
        published_at: instant(second),
        origin: PublicationOrigin::MonthlyDisclosure {
            schedule: MonthlyReportSchedule::Custom {
                day: 1,
                second_of_day: second,
                delay: MonthlyReportDelay::None,
            },
            delay_days: 0,
            seeded: false,
        },
        supersedes: None,
    }
}
fn ctx<'a>(
    system: &'a CompanySystem,
    dispatch: &'a DisclosureDispatch,
    library: &'a PublicLibrary,
    reports: &'a [PublicationRequest],
    second: u32,
) -> DisclosurePrepareCtx<'a> {
    DisclosurePrepareCtx {
        source: DisclosureSourceRef::Simple(system),
        previous_dispatch: dispatch,
        previous_library: library,
        seed: 17,
        report_frequency: frequency(
            reports
                .first()
                .map_or(10 * 3600 + 30 * 60, |r| r.published_at.second_of_day()),
        ),
        through: instant(second),
        prehistory_opening: None,
        settled_announcements: None,
        corporate_actions: None,
    }
}
#[test]
fn custom_report_keeps_actual_second_and_only_real_through() {
    let system = simple_model(1);
    let dispatch = DisclosureDispatch::new(Some(instant(10 * 3600)));
    let library = PublicLibrary::new();
    let reports = [request(10 * 3600 + 30 * 60)];
    let candidate = prepare_disclosures(ctx(
        &system,
        &dispatch,
        &library,
        &reports,
        10 * 3600 + 31 * 60,
    ))
    .unwrap();
    assert_eq!(candidate.publications.len(), 1);
    assert_eq!(
        candidate.publications[0].published_at,
        instant(10 * 3600 + 30 * 60)
    );
    assert_eq!(candidate.visible_through, instant(10 * 3600 + 31 * 60));
    assert_eq!(
        candidate.dispatch.published_through(),
        Some(candidate.visible_through)
    );
    assert_eq!(candidate.dispatch.announced_through(), None);
    assert_eq!(library, PublicLibrary::new());
    assert_eq!(dispatch.published_through(), Some(instant(10 * 3600)));
}
#[test]
fn actual_window_is_left_open_right_closed_and_repeated_scan_is_idempotent() {
    let system = simple_model(1);
    let dispatch = DisclosureDispatch::new(Some(instant(10 * 3600)));
    let library = PublicLibrary::new();
    let reports = [request(10 * 3600 + 30 * 60)];
    let before = prepare_disclosures(ctx(
        &system,
        &dispatch,
        &library,
        &reports,
        10 * 3600 + 30 * 60 - 1,
    ))
    .unwrap();
    assert!(before.publications.is_empty());
    let at = prepare_disclosures(ctx(
        &system,
        &before.dispatch,
        &before.library,
        &reports,
        10 * 3600 + 30 * 60,
    ))
    .unwrap();
    assert_eq!(at.publications.len(), 1);
    let again = prepare_disclosures(ctx(
        &system,
        &at.dispatch,
        &at.library,
        &reports,
        10 * 3600 + 31 * 60,
    ))
    .unwrap();
    assert!(again.publications.is_empty());
    assert_eq!(again.library, at.library);
}
#[test]
fn reversed_window_is_an_error_without_mutating_inputs() {
    let system = simple_model(1);
    let dispatch = DisclosureDispatch::new(Some(instant(18 * 3600)));
    let library = PublicLibrary::new();
    assert!(prepare_disclosures(ctx(&system, &dispatch, &library, &[], 17 * 3600)).is_err());
    assert_eq!(dispatch.published_through(), Some(instant(18 * 3600)));
    assert_eq!(library, PublicLibrary::new());
}
#[test]
fn existing_foreign_source_or_policy_is_rejected_without_mutating_inputs() {
    let system = simple_model(1);
    for foreign_source in [true, false] {
        let mut library = PublicLibrary::new();
        library
            .publish_simple_closed(
                system.closing_for(&CompanyId(0)).unwrap(),
                request(10 * 3600 + 30 * 60),
            )
            .unwrap();
        let mut save = library.save();
        if foreign_source {
            save.reports[0].source = crate::information::PublicationSource::SimulationAccounting;
        } else {
            save.reports[0].policy.chart_version = 2;
        }
        let library = PublicLibrary::from_parts(save).unwrap();
        let before = library.clone();
        let dispatch = DisclosureDispatch::new(Some(instant(10 * 3600 + 30 * 60)));
        assert!(prepare_disclosures(ctx(&system, &dispatch, &library, &[], 86399)).is_err());
        assert_eq!(library, before);
        assert_eq!(
            dispatch.published_through(),
            Some(instant(10 * 3600 + 30 * 60))
        );
    }
}
#[test]
fn absent_real_period_stays_absent_instead_of_fabricating_reports() {
    let system = simple_model(0);
    let dispatch = DisclosureDispatch::default();
    let library = PublicLibrary::new();
    let candidate = prepare_disclosures(ctx(&system, &dispatch, &library, &[], 86399)).unwrap();
    assert!(candidate.publications.is_empty());
    assert_eq!(candidate.library, library);
    assert_eq!(candidate.dispatch.published_through(), Some(instant(86399)));
}
#[test]
fn civil_scan_does_not_turn_publication_instants_into_personal_observation_hooks() {
    let system = simple_model(1);
    let dispatch = DisclosureDispatch::default();
    let library = PublicLibrary::new();
    let reports = [request(10 * 3600 + 30 * 60)];
    let mut input = ctx(&system, &dispatch, &library, &reports, 86399);
    input.settled_announcements = Some(instant(0).date());
    let candidate = prepare_disclosures(input).unwrap();
    assert_eq!(candidate.publications.len(), 1);
    assert_eq!(candidate.visible_through, instant(86399));
    assert_eq!(
        candidate.dispatch.announced_through(),
        Some(instant(0).date())
    );
}

use crate::company::disclosure_correction_test_fixture as shared_owner_fixture;
use shared_owner_fixture::legacy_fixture;

#[test]
fn same_company_instant_kind_version_keeps_both_scopes_and_shuffle_keeps_ids() {
    let (operations, closing) = legacy_fixture();
    let dispatch = DisclosureDispatch::default();
    let library = PublicLibrary::new();
    let mut standalone = request(18 * 3600);
    standalone.policy.chart_version = 2;
    let mut consolidated = standalone.clone();
    consolidated.scope = ScopeId::Consolidated(MemberId(0));
    let prepare = |closing: &ClosingEngine| {
        prepare_disclosures(DisclosurePrepareCtx {
            source: DisclosureSourceRef::Legacy {
                operations: &operations,
                closing,
            },
            previous_dispatch: &dispatch,
            previous_library: &library,
            seed: 17,
            report_frequency: frequency(18 * 3600),
            through: instant(86399),
            prehistory_opening: None,
            settled_announcements: None,
            corporate_actions: None,
        })
        .unwrap()
    };
    let mut shuffled = ClosingEngine::new();
    shuffled
        .record(
            closing
                .version(
                    &consolidated.scope,
                    consolidated.period,
                    consolidated.kind,
                    1,
                )
                .unwrap()
                .clone(),
        )
        .unwrap();
    shuffled
        .record(
            closing
                .version(&standalone.scope, standalone.period, standalone.kind, 1)
                .unwrap()
                .clone(),
        )
        .unwrap();
    let forward = prepare(&closing);
    let reverse = prepare(&shuffled);
    assert_eq!(forward.library, reverse.library);
    assert_eq!(forward.publications, reverse.publications);
    assert_eq!(forward.publications.len(), 2);
    assert_eq!(
        forward.library.save().reports[0].reports.scope,
        ScopeId::Standalone(MemberId(0))
    );
    assert_eq!(
        forward.library.save().reports[1].reports.scope,
        ScopeId::Consolidated(MemberId(0))
    );
}

fn split_book(id: &str) -> crate::company::share_split::ShareSplitBook {
    use crate::company::share_split::{ShareSplitBook, ShareSplitDirection, ShareSplitEventPlan};
    let date = instant(0).date();
    ShareSplitBook::new(ShareSplitEventPlan {
        event_id: id.into(),
        approval_reference: "正式批准".into(),
        issuer: CompanyId(0),
        stock: crate::account::StockCode("600000".into()),
        exchange: crate::calendar::CalendarExchange::Sse,
        direction: ShareSplitDirection::Split,
        ratio: 2,
        approved_on: date,
        announced_on: date,
        registered_on: date.next().unwrap(),
        ex_rights_on: date.next().unwrap().next().unwrap(),
    })
    .unwrap()
}

#[test]
fn same_instant_announcement_precedes_report_but_early_custom_report_precedes_announcement() {
    let system = simple_model(1);
    let dispatch = DisclosureDispatch::default();
    let library = PublicLibrary::new();
    let mut actions = SessionCorporateActions::default();
    actions.share_splits = vec![split_book("z"), split_book("a")];
    for second in [18 * 3600, 10 * 3600 + 30 * 60] {
        let reports = [request(second)];
        let mut input = ctx(&system, &dispatch, &library, &reports, 86399);
        input.settled_announcements = Some(instant(0).date());
        input.corporate_actions = Some(&actions);
        let candidate = prepare_disclosures(input).unwrap();
        assert_eq!(candidate.publications.len(), 3);
        let report_position = if second == 18 * 3600 { 2 } else { 0 };
        assert!(matches!(
            candidate.publications[report_position].kind,
            DisclosurePublicationKind::Report { .. }
        ));
        let announcements = candidate.library.save().announcements;
        let crate::information::AnnouncementContent::ShareSplit(first) = &announcements[0].content
        else {
            panic!("应保留正式拆股公告")
        };
        assert_eq!(first.event_id, "a");
    }
}

#[test]
fn same_identity_same_payload_is_idempotent_but_changed_payload_rejects_whole_candidate() {
    let system = simple_model(1);
    let dispatch = DisclosureDispatch::default();
    let library = PublicLibrary::new();
    let book = split_book("same");
    let mut actions = SessionCorporateActions::default();
    actions.share_splits = vec![book.clone(), book];
    let prepare = |actions: &SessionCorporateActions| {
        let mut input = ctx(&system, &dispatch, &library, &[], 86399);
        input.settled_announcements = Some(instant(0).date());
        input.corporate_actions = Some(actions);
        prepare_disclosures(input)
    };
    let candidate = prepare(&actions).unwrap();
    assert_eq!(candidate.library.save().announcements.len(), 1);
    let mut bad = actions.share_splits[0].plan().clone();
    bad.ratio = 3;
    actions.share_splits[1] = crate::company::share_split::ShareSplitBook::new(bad).unwrap();
    assert!(prepare(&actions).is_err());
    assert_eq!(library, PublicLibrary::new());
    assert_eq!(dispatch, DisclosureDispatch::default());
}
#[test]
fn prehistory_stops_before_opening_midnight_and_monthly_boundary_seconds_stay_exact() {
    let system = simple_model(1);
    for second in [0, 86399] {
        let reports = [request(second)];
        let library = PublicLibrary::new();
        let dispatch = DisclosureDispatch::default();
        let mut input = ctx(&system, &dispatch, &library, &reports, 0);
        input.through = CivilInstant::new(instant(0).date().prev().unwrap(), 86399).unwrap();
        input.prehistory_opening = Some(instant(0).date());
        let history = prepare_disclosures(input).unwrap();
        assert!(history.publications.is_empty());
        let candidate = prepare_disclosures(ctx(
            &system,
            &history.dispatch,
            &history.library,
            &reports,
            second,
        ))
        .unwrap();
        assert_eq!(candidate.publications.len(), 1);
        assert_eq!(candidate.publications[0].published_at, instant(second));
        assert_eq!(
            candidate.dispatch.published_through(),
            Some(instant(second))
        );
    }
}

#[test]
fn exact_civil_endpoint_is_required_before_marking_announcements_scanned() {
    let system = simple_model(1);
    let library = PublicLibrary::new();
    let dispatch = DisclosureDispatch::default();
    let mut input = ctx(&system, &dispatch, &library, &[], 18 * 3600);
    input.settled_announcements = Some(instant(0).date());
    assert!(prepare_disclosures(input).is_err());
    let mut input = ctx(&system, &dispatch, &library, &[], 18 * 3600);
    input.report_frequency = ReportFrequency::Quarterly;
    input.settled_announcements = Some(instant(0).date());
    let candidate = prepare_disclosures(input).unwrap();
    assert_eq!(
        candidate.dispatch.announced_through(),
        Some(instant(0).date())
    );
}

fn correction_operation(
    source: u64,
    date: CivilDate,
) -> crate::accounting::closing::CorrectionRequest {
    use crate::accounting::{
        AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, JournalLine,
        LedgerAccountId, PostingSide,
    };
    crate::accounting::closing::CorrectionRequest {
        reason: "纠正历史汇总营收".into(),
        entries: vec![JournalEntry {
            source: BusinessEventId::new(source),
            date,
            kind: BusinessKind::SimpleRevenueAccrual,
            cash_flow: CashFlowClass::NonCash,
            lines: vec![
                JournalLine {
                    account: LedgerAccountId("simple_receivable".into()),
                    side: PostingSide::Debit,
                    amount: AccountingAmount::from_cents(100),
                },
                JournalLine {
                    account: LedgerAccountId("simple_revenue".into()),
                    side: PostingSide::Credit,
                    amount: AccountingAmount::from_cents(100),
                },
            ],
        }],
    }
}

#[test]
fn correction_package_keeps_private_predecessor_and_source_finance_with_the_complete_proof() {
    let mut system = simple_model(1);
    let mut base = PublicLibrary::new();
    let original = base
        .publish_simple_closed(
            system.closing_for(&CompanyId(0)).unwrap(),
            request(18 * 3600),
        )
        .unwrap();
    let date = instant(0).date().next().unwrap();
    let now = CivilInstant::from_hms(date, 18, 0, 0).unwrap();
    // 第一次真实 owner 只推进私有 source，临时公开库随该试算丢弃。
    let mut temporary_library = base.clone();
    system
        .correct_report(
            &CompanyId(0),
            correction_operation(1000, date),
            original,
            now,
            &mut temporary_library,
        )
        .unwrap();
    let before = system.clone();
    let prepared = system
        .prepare_disclosure_correction(
            &CompanyId(0),
            correction_operation(2000, date),
            original,
            now,
            &base,
        )
        .unwrap();
    assert_eq!(system, before);
    assert_ne!(
        prepared
            .source()
            .simple_book_display(&CompanyId(0))
            .unwrap(),
        system.simple_book_display(&CompanyId(0)).unwrap()
    );
    let dispatch = DisclosureDispatch::new(Some(instant(18 * 3600)));
    let input = || DisclosurePrepareCtx {
        source: DisclosureSourceRef::PreparedSimple(&prepared),
        previous_dispatch: &dispatch,
        previous_library: &base,
        seed: 17,
        report_frequency: frequency(18 * 3600),
        through: CivilInstant::new(date, 86399).unwrap(),
        prehistory_opening: None,
        settled_announcements: None,
        corporate_actions: None,
    };
    let candidate = prepare_disclosures(input()).unwrap();
    let repeated = prepare_disclosures(input()).unwrap();
    assert_eq!(candidate, repeated);
    assert_eq!(candidate.publications.len(), 1);
    let saved = candidate.library.save();
    let published = &saved.reports[1];
    assert_eq!(published.supersedes, Some(original));
    assert_eq!(published.reports.version.supersedes, Some(2));
    assert_eq!(published.reports.version.sequence, 3);
    assert_eq!(base.save().reports.len(), 1);
    let foreign_base = PublicLibrary::new();
    let mut wrong = input();
    wrong.previous_library = &foreign_base;
    assert!(matches!(
        prepare_disclosures(wrong),
        Err(DisclosurePrepareError::CorrectionBaseMismatch)
    ));
    let mut future = input();
    future.through = CivilInstant::from_hms(date, 17, 59, 59).unwrap();
    assert!(
        matches!(prepare_disclosures(future), Err(DisclosurePrepareError::CorrectionOutsideWindow { published_at, .. }) if published_at == now)
    );
    let recovered_source = prepared.into_source();
    assert_eq!(
        recovered_source
            .closing_for(&CompanyId(0))
            .unwrap()
            .version(
                &published.reports.scope,
                published.reports.period,
                published.reports.kind,
                3
            )
            .unwrap(),
        &published.reports
    );
}

fn cash_announcement_fixture() -> (
    CompanySystem,
    SessionCorporateActions,
    crate::company::cash_dividend::CashDividendPlan,
) {
    use crate::accounting::AccountingAmount;
    use crate::company::cash_dividend::{CashDividendBook, CashDividendPlan};
    use crate::company::ex_reference_price::CashDividendFormula;
    use crate::company::DividendDeclaration;
    use crate::money::Money;
    let mut system = simple_model(12);
    let date = instant(0).date();
    system
        .define_dividend_legal_facts(
            &CompanyId(0),
            AccountingAmount::from_cents(100000),
            "测试法定注册资本记录".into(),
        )
        .unwrap();
    system
        .declare_dividend(
            &CompanyId(0),
            DividendDeclaration {
                plan_id: "cash".into(),
                approved_on: date,
                total_gross: AccountingAmount::from_cents(1000),
                registered_capital: AccountingAmount::from_cents(100000),
            },
        )
        .unwrap();
    let plan = CashDividendPlan {
        plan_id: "cash".into(),
        issuer: CompanyId(0),
        stock: crate::account::StockCode("600000".into()),
        exchange: crate::calendar::CalendarExchange::Sse,
        formula: CashDividendFormula::StandardCashOnly,
        approved_on: date,
        announced_on: date,
        registered_on: date.next().unwrap(),
        ex_dividend_on: date.next().unwrap().next().unwrap(),
        payable_on: date.next().unwrap().next().unwrap(),
        gross_per_share: Money::from_cents(1),
        distributable_amount: Money::from_cents(1000),
    };
    let mut actions = SessionCorporateActions::default();
    actions.dividends = vec![CashDividendBook::new(plan.clone()).unwrap()];
    use crate::company::share_registry::{
        AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
    };
    actions.registries.push(
        ShareRegistry::new(
            plan.stock.clone(),
            plan.issuer,
            1000,
            date,
            vec![ShareHolding {
                holder: HolderId::External("披露测试外部股东".into()),
                lots: vec![ShareLot {
                    id: "现金分红批准股数".into(),
                    qty: 1000,
                    acquired_on: date,
                    source: AcquisitionSource::InitialAllocation {
                        evidence: "披露测试真实名册".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            }],
        )
        .unwrap(),
    );
    (system, actions, plan)
}

#[test]
fn cash_announcement_total_comes_from_real_approved_finance_fact() {
    use crate::company::cash_dividend::CashDividendBook;
    use crate::money::Money;
    let (system, mut actions, plan) = cash_announcement_fixture();
    let date = instant(0).date();
    let library = PublicLibrary::new();
    let dispatch = DisclosureDispatch::default();
    let mut input = ctx(&system, &dispatch, &library, &[], 86399);
    input.settled_announcements = Some(date);
    input.corporate_actions = Some(&actions);
    let candidate = prepare_disclosures(input).unwrap();
    assert_eq!(candidate.library.save().announcements.len(), 1);
    let crate::information::AnnouncementContent::CashDividend(announcement) =
        &candidate.library.save().announcements[0].content
    else {
        panic!("应发布现金分红")
    };
    assert_eq!(announcement.total_gross, Money::from_cents(1000));
    // 同一批准身份配错可分配上限不能以“正数金额”冒充对应财务批准事实。
    let mut bad_plan = plan;
    bad_plan.distributable_amount = Money::from_cents(999);
    actions.dividends = vec![CashDividendBook::new(bad_plan).unwrap()];
    let mut input = ctx(&system, &dispatch, &library, &[], 86399);
    input.settled_announcements = Some(date);
    input.corporate_actions = Some(&actions);
    assert!(prepare_disclosures(input).is_err());
    assert_eq!(library, PublicLibrary::new());
}

#[test]
fn existing_future_publication_rejects_but_unconfirmed_private_correction_stays_private() {
    let system = simple_model(1);
    let dispatch = DisclosureDispatch::default();
    let mut future_library = PublicLibrary::new();
    future_library
        .publish_simple_closed(
            system.closing_for(&CompanyId(0)).unwrap(),
            request(18 * 3600),
        )
        .unwrap();
    assert!(prepare_disclosures(ctx(&system, &dispatch, &future_library, &[], 17 * 3600)).is_err());
    let (operations, mut closing) = legacy_fixture();
    let scope = ScopeId::Standalone(MemberId(0));
    let period = AccountingPeriod::from_ymd(2029, 12).unwrap();
    let mut correction = closing
        .version(&scope, period, ReportKind::Monthly, 1)
        .unwrap()
        .clone();
    correction.version = ReportVersion {
        sequence: 2,
        supersedes: Some(1),
        kind: VersionKind::Correction {
            reason: "缺少实际更正操作事实".into(),
        },
    };
    closing.record(correction).unwrap();
    let library = PublicLibrary::new();
    let candidate = prepare_disclosures(DisclosurePrepareCtx {
        source: DisclosureSourceRef::Legacy {
            operations: &operations,
            closing: &closing,
        },
        previous_dispatch: &dispatch,
        previous_library: &library,
        seed: 17,
        report_frequency: frequency(18 * 3600),
        through: instant(86399),
        prehistory_opening: None,
        settled_announcements: None,
        corporate_actions: None,
    })
    .unwrap();
    assert_eq!(candidate.publications.len(), 2);
    assert!(candidate
        .library
        .save()
        .reports
        .iter()
        .all(|report| report.reports.version.sequence == 1));
}

#[test]
fn prehistory_marks_only_new_originals_seeded_and_preserves_existing_ids() {
    let system = simple_model(2);
    let library = PublicLibrary::new();
    let dispatch = DisclosureDispatch::default();
    let opening = instant(0).date();
    let endpoint = CivilInstant::new(opening.prev().unwrap(), 86399).unwrap();
    let mut input = ctx(&system, &dispatch, &library, &[], 0);
    input.through = endpoint;
    input.prehistory_opening = Some(opening);
    let history = prepare_disclosures(input).unwrap();
    assert_eq!(history.publications.len(), 1);
    let saved = history.library.save();
    assert!(matches!(
        saved.reports[0].origin,
        PublicationOrigin::MonthlyDisclosure { seeded: true, .. }
    ));
    assert!(saved
        .reports
        .iter()
        .all(|report| report.published_at < instant(0)));
    let mut input = ctx(&system, &history.dispatch, &history.library, &[], 0);
    input.through = endpoint;
    input.prehistory_opening = Some(opening);
    let repeated = prepare_disclosures(input).unwrap();
    assert!(repeated.publications.is_empty());
    assert_eq!(repeated.library, history.library);
    assert_eq!(repeated.library.save(), saved);
}

#[test]
fn prehistory_rejects_wrong_endpoint_and_does_not_mutate_inputs() {
    let system = simple_model(2);
    let library = PublicLibrary::new();
    let dispatch = DisclosureDispatch::default();
    let opening = instant(0).date();
    for through in [
        instant(0),
        CivilInstant::new(opening.prev().unwrap(), 86398).unwrap(),
    ] {
        let mut input = ctx(&system, &dispatch, &library, &[], 0);
        input.through = through;
        input.prehistory_opening = Some(opening);
        assert!(
            matches!(prepare_disclosures(input), Err(DisclosurePrepareError::InvalidPrehistoryWindow { opening: actual, through: actual_through }) if actual == opening && actual_through == through)
        );
        assert_eq!(library, PublicLibrary::new());
        assert_eq!(dispatch, DisclosureDispatch::default());
    }
}

#[test]
fn cash_announcement_allows_authorized_cap_above_approved_total() {
    use crate::company::cash_dividend::CashDividendBook;
    use crate::money::Money;
    let (system, mut actions, mut plan) = cash_announcement_fixture();
    plan.distributable_amount = Money::from_cents(2000);
    actions.dividends = vec![CashDividendBook::new(plan).unwrap()];
    let library = PublicLibrary::new();
    let dispatch = DisclosureDispatch::default();
    let mut input = ctx(&system, &dispatch, &library, &[], 86399);
    input.settled_announcements = Some(instant(0).date());
    input.corporate_actions = Some(&actions);
    let candidate = prepare_disclosures(input).unwrap();
    let saved = candidate.library.save();
    let AnnouncementContent::CashDividend(announcement) = &saved.announcements[0].content else {
        panic!("应保留真实现金分红金额");
    };
    assert_eq!(announcement.total_gross, Money::from_cents(1000));
    assert_eq!(
        announcement.plan.distributable_amount,
        Money::from_cents(2000)
    );
}

#[test]
fn cash_announcement_rejects_frozen_registration_total_drifting_from_approved_fact() {
    use crate::company::cash_dividend::CashDividendBook;
    use crate::money::Money;
    let (system, mut actions, mut plan) = cash_announcement_fixture();
    let date = plan.registered_on;
    plan.announced_on = date;
    plan.gross_per_share = Money::from_cents(2);
    plan.distributable_amount = Money::from_cents(2000);
    let holdings = actions.registries[0].holdings().to_vec();
    let mut registry = crate::company::share_registry::ShareRegistry::new(
        plan.stock.clone(),
        plan.issuer,
        1000,
        date,
        holdings,
    )
    .unwrap();
    let snapshot = registry
        .register(plan.plan_id.clone(), date)
        .unwrap()
        .clone();
    let mut book = CashDividendBook::new(plan).unwrap();
    book.announce(date).unwrap();
    book.register(
        snapshot,
        &crate::calendar::TradingCalendar::current_default_calendar().unwrap(),
    )
    .unwrap();
    assert_eq!(book.total_gross().unwrap(), Money::from_cents(2000));
    actions.dividends = vec![book];
    let library = PublicLibrary::new();
    let dispatch = DisclosureDispatch::default();
    let mut input = ctx(&system, &dispatch, &library, &[], 86399);
    input.through = CivilInstant::new(date, 86399).unwrap();
    input.settled_announcements = Some(date);
    input.corporate_actions = Some(&actions);
    assert!(prepare_disclosures(input).is_err());
    assert_eq!(library, PublicLibrary::new());
    assert_eq!(dispatch, DisclosureDispatch::default());
}

fn rebuild_legacy_original(
    operations: &mut CompanyOperations,
    closing: &mut ClosingEngine,
) -> ReportSet {
    use crate::accounting::reports::{
        generate_report_set, IndustryPresentation, ReportRequest, ReportSource,
    };
    use crate::accounting::{
        AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, JournalLine,
        LedgerAccountId, PostingSide,
    };
    let period = AccountingPeriod::from_ymd(2029, 12).unwrap();
    let scope = ScopeId::Standalone(MemberId(0));
    let old = closing
        .version(&scope, period, ReportKind::Monthly, 1)
        .unwrap()
        .clone();
    operations
        .company_mut(&CompanyId(0))
        .unwrap()
        .books_mut()
        .post_batch(vec![JournalEntry {
            source: BusinessEventId::new(99000),
            date: CivilDate::from_iso("2029-12-01").unwrap(),
            kind: BusinessKind::OpeningBalance,
            cash_flow: CashFlowClass::Financing,
            lines: vec![
                JournalLine {
                    account: LedgerAccountId("1002".into()),
                    side: PostingSide::Debit,
                    amount: AccountingAmount::from_cents(100),
                },
                JournalLine {
                    account: LedgerAccountId("4001".into()),
                    side: PostingSide::Credit,
                    amount: AccountingAmount::from_cents(100),
                },
            ],
        }])
        .unwrap();
    let rebuilt = generate_report_set(ReportRequest {
        period,
        kind: ReportKind::Monthly,
        source: ReportSource::Standalone {
            id: MemberId(0),
            books: operations.company(&CompanyId(0)).unwrap().books().books(),
            industry: IndustryPresentation::Industrial,
        },
        version: ReportVersion {
            sequence: 2,
            supersedes: None,
            kind: VersionKind::Original,
        },
        adjustments: &BTreeMap::new(),
    })
    .unwrap();
    assert_ne!(rebuilt.balance_sheet, old.balance_sheet);
    assert_ne!(rebuilt.cash_flow, old.cash_flow);
    closing.record(rebuilt.clone()).unwrap();
    rebuilt
}

#[test]
fn unpublished_original_rebuild_publishes_only_latest_original_per_report_key() {
    let (mut operations, mut closing) = legacy_fixture();
    let rebuilt = rebuild_legacy_original(&mut operations, &mut closing);
    let library = PublicLibrary::new();
    let dispatch = DisclosureDispatch::default();
    let candidate = prepare_disclosures(DisclosurePrepareCtx {
        source: DisclosureSourceRef::Legacy {
            operations: &operations,
            closing: &closing,
        },
        previous_dispatch: &dispatch,
        previous_library: &library,
        seed: 17,
        report_frequency: frequency(18 * 3600),
        through: instant(86399),
        prehistory_opening: None,
        settled_announcements: None,
        corporate_actions: None,
    })
    .unwrap();
    let reports = candidate.library.save().reports;
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].reports, rebuilt);
    assert_eq!(reports[1].reports.scope, ScopeId::Consolidated(MemberId(0)));
    assert_eq!(reports[1].reports.version.sequence, 1);
    assert_eq!(candidate.publications.len(), 2);
    assert_eq!(library, PublicLibrary::new());
}

#[test]
fn original_rescan_keeps_published_payload_ids_and_origin_after_private_rebuild() {
    let (mut operations, mut closing) = legacy_fixture();
    let mut library = PublicLibrary::new();
    for scope in [
        ScopeId::Standalone(MemberId(0)),
        ScopeId::Consolidated(MemberId(0)),
    ] {
        let mut publication = request(18 * 3600);
        publication.scope = scope;
        publication.policy.chart_version = 2;
        library.publish_closed(&closing, publication).unwrap();
    }
    let before = library.clone();
    let rebuilt = rebuild_legacy_original(&mut operations, &mut closing);
    assert_ne!(
        rebuilt.balance_sheet,
        before.save().reports[0].reports.balance_sheet
    );
    let dispatch = DisclosureDispatch::default();
    let candidate = prepare_disclosures(DisclosurePrepareCtx {
        source: DisclosureSourceRef::Legacy {
            operations: &operations,
            closing: &closing,
        },
        previous_dispatch: &dispatch,
        previous_library: &library,
        seed: 17,
        report_frequency: frequency(18 * 3600),
        through: instant(86399),
        prehistory_opening: None,
        settled_announcements: None,
        corporate_actions: None,
    })
    .unwrap();
    assert!(candidate.publications.is_empty());
    assert_eq!(candidate.library, before);
    assert_eq!(candidate.library.save(), before.save());
    assert_eq!(library, before);
}

#[test]
fn same_original_identity_with_different_payload_rejects_without_mutating_inputs() {
    let (mut operations, mut closing) = legacy_fixture();
    let mut conflicting = rebuild_legacy_original(&mut operations, &mut closing);
    conflicting.version.sequence = 1;
    closing.record(conflicting).unwrap();
    let library = PublicLibrary::new();
    let dispatch = DisclosureDispatch::default();
    assert!(prepare_disclosures(DisclosurePrepareCtx {
        source: DisclosureSourceRef::Legacy {
            operations: &operations,
            closing: &closing
        },
        previous_dispatch: &dispatch,
        previous_library: &library,
        seed: 17,
        report_frequency: frequency(18 * 3600),
        through: instant(86399),
        prehistory_opening: None,
        settled_announcements: None,
        corporate_actions: None,
    })
    .is_err());
    assert_eq!(library, PublicLibrary::new());
    assert_eq!(dispatch, DisclosureDispatch::default());
}
