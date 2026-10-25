use super::*;
use crate::company::cash_dividend_tax::CashDividendTaxMode;
use crate::session::corporate_actions::{DividendTaxStatus, SessionCorporateActions};
use crate::{
    accounting::{
        AccountingAmount, IncomeTaxPolicy, JournalLine, LedgerAccountId, PostingSide, TaxPolicy,
        VatPolicy,
    },
    calendar::{CalendarExchange, CivilDate, TradingCalendar},
    company::{
        cash_dividend::{CashDividendBook, CashDividendPlan},
        cash_dividend_tax::{
            CashDividendTaxBook, DividendTaxLot, DividendTaxProfile, TaxAcquisitionSource,
            TaxShareClass,
        },
        config::CompanySystemConfig,
        ex_reference_price::CashDividendFormula,
        share_registry::{
            AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
        },
        simple::{
            period::{
                AnnualTrendConfig, PeriodGenerationParameters, PeriodNoiseConfig,
                PeriodVariableExpenseRule, SettlementCycle,
            },
            SimpleBookDisplayConfig, SimpleCompanyConfig, SimpleCompanyPreferences, SimpleConfig,
            SimpleEnvironmentConfig, SimpleFinanceConfig, SimpleSummaryRule,
        },
        stock_distribution::{
            StockDistributionBook, StockDistributionDeclaration, StockDistributionEventPlan,
            StockDistributionKind,
        },
        CompanyId, CompanyKind, CompanySpec, CompanySystem, DividendDeclaration, IndustryId,
    },
    stock::{StockCatalog, StockMeta},
};
use crate::{orderbook::AccountId, Money};
use std::collections::BTreeMap;

fn date(text: &str) -> CivilDate {
    CivilDate::from_iso(text).unwrap()
}
fn amount(cents: i128) -> AccountingAmount {
    AccountingAmount::from_cents(cents)
}
fn code() -> crate::StockCode {
    crate::StockCode("600101".into())
}
fn lot(id: &str, qty: u64) -> ShareLot {
    ShareLot {
        id: id.into(),
        qty,
        acquired_on: date("2030-05-31"),
        source: AcquisitionSource::InitialAllocation {
            evidence: "fixture".into(),
        },
        restriction: ShareRestriction::Unrestricted,
    }
}
fn fixture() -> (
    SessionCorporateActions,
    CompanySystem,
    TradingCalendar,
    StockCatalog,
    BTreeMap<AccountId, Money>,
) {
    let zero = || PeriodNoiseConfig {
        monthly_bp: 0,
        quarterly_bp: 0,
        half_year_bp: 0,
        annual_bp: 0,
    };
    let line = |account: &str, side, cents| JournalLine {
        account: LedgerAccountId(account.into()),
        side,
        amount: amount(cents),
    };
    let mut system = CompanySystem::create(
        vec![CompanySpec {
            code: "C1".into(),
            name: "测试公司".into(),
            industry: IndustryId("测试行业".into()),
            kind: CompanyKind::Industrial,
            listed_stock: Some(code()),
            issued_shares: 10,
            group_parent: None,
        }],
        CompanySystemConfig::Simple(SimpleConfig {
            environment: SimpleEnvironmentConfig {
                initial_change_bp: 0,
                persistence_bp: 0,
                noise: zero(),
            },
            companies: vec![SimpleCompanyConfig {
                company: CompanyId(0),
                kind: CompanyKind::Industrial,
                generation: PeriodGenerationParameters {
                    initial_revenue: amount(100_000),
                    initial_fixed_expense: amount(0),
                    revenue_trend: AnnualTrendConfig::Fixed {
                        annual_growth_bp: 0,
                    },
                    fixed_expense_trend: AnnualTrendConfig::Fixed {
                        annual_growth_bp: 0,
                    },
                    revenue_noise: zero(),
                    fixed_expense_noise: zero(),
                    demand_sensitivity_bp: 0,
                    variable_expense: PeriodVariableExpenseRule::RevenueRatio {
                        ratio_bp: 0,
                        noise: zero(),
                    },
                },
                finance: SimpleFinanceConfig {
                    opening_lines: vec![
                        line("1002", PostingSide::Debit, 3_000),
                        line("simple_capital_reserve", PostingSide::Credit, 1_000),
                        line("4001", PostingSide::Credit, 1_000),
                        line("simple_statutory_reserve", PostingSide::Credit, 1_000),
                    ],
                    tax_policy: TaxPolicy {
                        version: 1,
                        vat: VatPolicy {
                            output_rate_bp: 0,
                            input_rate_bp: 0,
                            deductible_share_bp: 10_000,
                        },
                        income_tax: IncomeTaxPolicy {
                            rate_bp: 0,
                            loss_carryforward_years: 5,
                        },
                    },
                    summary_rule: SimpleSummaryRule::ReceivableRevenuePayableExpenses,
                    book_display: SimpleBookDisplayConfig::DEFAULT,
                },
                preferences: SimpleCompanyPreferences::none(),
            }],
            prehistory_periods: 1,
            settlement_cycle: SettlementCycle::Monthly,
        }),
        date("2030-06-03"),
        19,
    )
    .unwrap();
    system
        .define_dividend_legal_facts(&CompanyId(0), amount(1_000), "fixture".into())
        .unwrap();
    let registry = ShareRegistry::new(
        code(),
        CompanyId(0),
        10,
        date("2030-06-03"),
        vec![
            ShareHolding {
                holder: HolderId::Account(AccountId(1)),
                lots: vec![lot("p", 3)],
            },
            ShareHolding {
                holder: HolderId::Account(AccountId(2)),
                lots: vec![lot("i", 3)],
            },
            ShareHolding {
                holder: HolderId::External("外部股东".into()),
                lots: vec![lot("e", 4)],
            },
        ],
    )
    .unwrap();
    let actions = SessionCorporateActions {
        registries: vec![registry],
        ..Default::default()
    };
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let catalog = StockCatalog::from_metas(vec![StockMeta::new("600101")]).unwrap();
    (
        actions,
        system,
        calendar,
        catalog,
        BTreeMap::from([(AccountId(1), Money::ZERO), (AccountId(2), Money::ZERO)]),
    )
}
fn cash_plan(
    actions: &mut SessionCorporateActions,
    system: &mut CompanySystem,
    calendar: &TradingCalendar,
) {
    let plan = CashDividendPlan::new(
        "cash".into(),
        CompanyId(0),
        code(),
        CalendarExchange::Sse,
        CashDividendFormula::StandardCashOnly,
        date("2030-06-03"),
        date("2030-06-03"),
        date("2030-06-03"),
        date("2030-06-04"),
        date("2030-06-04"),
        Money::from_cents(7),
        Money::from_cents(70),
        calendar,
    )
    .unwrap();
    system
        .declare_dividend(
            &CompanyId(0),
            DividendDeclaration {
                plan_id: "cash".into(),
                approved_on: date("2030-06-03"),
                total_gross: amount(
                    i128::from(
                        actions.registries[0]
                            .holdings()
                            .iter()
                            .filter(|h| h.holder != HolderId::IssuerTreasury)
                            .flat_map(|h| &h.lots)
                            .map(|l| l.qty)
                            .sum::<u64>(),
                    ) * 7,
                ),
                registered_capital: amount(1_000),
            },
        )
        .unwrap();
    actions.dividends.push(CashDividendBook::new(plan).unwrap());
}
fn stock_plan(actions: &mut SessionCorporateActions, system: &mut CompanySystem) {
    stock_plan_kind(actions, system, StockDistributionKind::BonusShares)
}
fn stock_plan_kind(
    actions: &mut SessionCorporateActions,
    system: &mut CompanySystem,
    kind: StockDistributionKind,
) {
    let new_shares = actions.registries[0]
        .holdings()
        .iter()
        .filter(|h| h.holder != HolderId::IssuerTreasury)
        .flat_map(|h| &h.lots)
        .map(|l| l.qty)
        .sum::<u64>()
        / 2;
    let plan = StockDistributionEventPlan {
        event_id: "stock".into(),
        approval_reference: "approved".into(),
        issuer: CompanyId(0),
        stock: code(),
        exchange: CalendarExchange::Sse,
        kind,
        approved_on: date("2030-06-03"),
        announced_on: date("2030-06-03"),
        registered_on: date("2030-06-03"),
        ex_rights_on: date("2030-06-04"),
        shares_per_existing_share_micros: 500_000,
        approved_total_new_shares: new_shares,
    };
    system
        .declare_stock_distribution(
            &CompanyId(0),
            StockDistributionDeclaration {
                event_id: "stock".into(),
                approval_reference: "approved".into(),
                kind: plan.kind.clone(),
                approved_on: plan.approved_on,
                new_shares,
                par_value_per_share: Money::from_cents(100),
                capital_increase: amount(i128::from(new_shares) * 100),
                registered_capital_at_approval: amount(1_000),
            },
        )
        .unwrap();
    actions
        .stock_distributions
        .push(StockDistributionBook::new(plan).unwrap());
}
fn prepare(
    actions: &SessionCorporateActions,
    system: &CompanySystem,
    calendar: &TradingCalendar,
    catalog: &StockCatalog,
    cash: &BTreeMap<AccountId, Money>,
    day: &str,
    tax_mode: CashDividendTaxMode,
) -> Result<DistributionsDayEndCandidate, DistributionsDayEndError> {
    actions.distributions_day_end_candidate(DistributionsDayEndInput {
        day: date(day),
        calendar,
        catalog,
        company_system: system,
        available_cash: cash,
        tax_mode,
        capital_reserve_tax_sources: &BTreeMap::new(),
        flat_withholding_bp: (tax_mode == CashDividendTaxMode::FlatWithholding).then_some(5_000),
    })
}
fn tax_book() -> CashDividendTaxBook {
    CashDividendTaxBook::new(
        AccountId(1),
        code(),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-06-03"),
        vec![DividendTaxLot {
            id: "tax:p".into(),
            qty: 3,
            acquired_on: date("2030-05-31"),
            source: TaxAcquisitionSource::InitialAllocation {
                evidence: "fixture".into(),
            },
            class: TaxShareClass::PublicMarket,
        }],
    )
    .unwrap()
}
#[test]
fn flat_preserves_gross_tax_exit_external_source_and_inputs() {
    let (mut actions, mut system, cal, catalog, cash) = fixture();
    cash_plan(&mut actions, &mut system, &cal);
    let before = (actions.clone(), system.clone(), cash.clone());
    let registered = prepare(
        &actions,
        &system,
        &cal,
        &catalog,
        &cash,
        "2030-06-03",
        CashDividendTaxMode::FlatWithholding,
    )
    .unwrap();
    let paid = prepare(
        &registered.actions,
        &registered.company_system,
        &cal,
        &catalog,
        &cash,
        "2030-06-04",
        CashDividendTaxMode::FlatWithholding,
    )
    .unwrap();
    assert_eq!((actions, system, cash.clone()), before);
    assert_eq!(paid.projected_available_cash[&AccountId(1)].cents(), 10);
    assert_eq!(paid.projected_available_cash[&AccountId(2)].cents(), 10);
    assert_eq!(paid.actions.flat_withholding_receipts.len(), 2);
    assert_eq!(
        paid.actions.flat_withholding_receipts[0].withheld.cents(),
        11
    );
    assert_eq!(paid.actions.external_receipts[0].gross.cents(), 28);
    assert!(paid.actions.dividend_tax_books.is_empty());
    assert_eq!(paid.intents.len(), 4);
    assert_eq!(paid.sources.len(), 3);
    let replay = prepare(
        &paid.actions,
        &paid.company_system,
        &cal,
        &catalog,
        &paid.projected_available_cash,
        "2030-06-04",
        CashDividendTaxMode::FlatWithholding,
    )
    .unwrap();
    assert!(replay.intents.is_empty());
    assert!(replay.sources.is_empty());
    assert_eq!(replay.actions, paid.actions);
}
#[test]
fn registered_person_sells_before_payment_but_keeps_frozen_right() {
    let (mut actions, mut system, cal, catalog, cash) = fixture();
    cash_plan(&mut actions, &mut system, &cal);
    actions.dividend_tax_books.push(tax_book());
    let mut registered = prepare(
        &actions,
        &system,
        &cal,
        &catalog,
        &cash,
        "2030-06-03",
        CashDividendTaxMode::AShareIndividual,
    )
    .unwrap();
    registered.actions.dividend_tax_books[0]
        .record_net_day("sale".into(), date("2030-06-04"), -3, None)
        .unwrap();
    let paid = prepare(
        &registered.actions,
        &registered.company_system,
        &cal,
        &catalog,
        &cash,
        "2030-06-04",
        CashDividendTaxMode::AShareIndividual,
    )
    .unwrap();
    assert_eq!(paid.actions.account_gross_receipts[0].gross.cents(), 21);
    assert_eq!(
        paid.actions.account_gross_receipts[0].tax_status,
        DividendTaxStatus::IndividualPublicMarket
    );
    assert_eq!(
        paid.actions.account_gross_receipts[1].tax_status,
        DividendTaxStatus::TreatmentNotConfigured
    );
    assert_eq!(paid.projected_available_cash[&AccountId(1)].cents(), 17);
    assert_eq!(paid.projected_available_cash[&AccountId(2)].cents(), 21);
    let mut indebted = paid.actions.clone();
    indebted.dividend_tax_books[0]
        .record_net_day("next".into(), date("2030-06-05"), 0, None)
        .unwrap();
    // 税基20%四舍五入为4分；付款已收缴，重试不重复扣款。
    assert_eq!(
        paid.actions.dividend_tax_books[0].collections()[0]
            .collected
            .cents(),
        4
    );
    assert_eq!(
        indebted.dividend_tax_books[0]
            .outstanding()
            .unwrap()
            .numerator(),
        0
    );
}
#[test]
fn stock_credit_returns_available_shares_and_all_three_candidate_totals() {
    let (mut actions, mut system, cal, catalog, cash) = fixture();
    stock_plan(&mut actions, &mut system);
    let registered = prepare(
        &actions,
        &system,
        &cal,
        &catalog,
        &cash,
        "2030-06-03",
        CashDividendTaxMode::Exempt,
    )
    .unwrap();
    let credited = prepare(
        &registered.actions,
        &registered.company_system,
        &cal,
        &catalog,
        &cash,
        "2030-06-04",
        CashDividendTaxMode::Exempt,
    )
    .unwrap();
    assert_eq!(credited.actions.registries[0].issued_shares(), 15);
    assert_eq!(
        credited
            .company_system
            .issuers()
            .get(&CompanyId(0))
            .unwrap()
            .issued_shares,
        15
    );
    assert_eq!(
        credited
            .sources
            .iter()
            .filter_map(|s| match s {
                DistributionSource::StockDistribution { shares, .. } => Some(*shares),
                _ => None,
            })
            .sum::<u64>(),
        5
    );
    assert!(credited.intents.iter().all(|i| matches!(i, DistributionIntent::StockDistributionCredit { available_on, .. } if *available_on == date("2030-06-04"))));
    assert_eq!(actions.registries[0].issued_shares(), 10);
    assert_eq!(
        system.issuers().get(&CompanyId(0)).unwrap().issued_shares,
        10
    );
    let replay = prepare(
        &credited.actions,
        &credited.company_system,
        &cal,
        &catalog,
        &cash,
        "2030-06-04",
        CashDividendTaxMode::Exempt,
    )
    .unwrap();
    assert!(replay.intents.is_empty());
    assert_eq!(replay.actions, credited.actions);
}
#[test]
fn missing_account_and_overflow_abort_whole_candidate() {
    let (mut actions, mut system, cal, catalog, cash) = fixture();
    cash_plan(&mut actions, &mut system, &cal);
    let reg = prepare(
        &actions,
        &system,
        &cal,
        &catalog,
        &cash,
        "2030-06-03",
        CashDividendTaxMode::Exempt,
    )
    .unwrap();
    for bad in [
        BTreeMap::from([(AccountId(1), Money::ZERO)]),
        BTreeMap::from([
            (AccountId(1), Money::ZERO),
            (AccountId(2), Money::from_cents(i64::MAX)),
        ]),
    ] {
        let before = (reg.actions.clone(), reg.company_system.clone());
        let error = prepare(
            &reg.actions,
            &reg.company_system,
            &cal,
            &catalog,
            &bad,
            "2030-06-04",
            CashDividendTaxMode::Exempt,
        )
        .unwrap_err();
        if !bad.contains_key(&AccountId(2)) {
            assert!(matches!(
                error.source,
                DistributionsDayEndFailure::MissingAvailableCash {
                    account: AccountId(2)
                }
            ));
        } else {
            assert!(matches!(error.source, DistributionsDayEndFailure::Money(_)));
        }

        assert_eq!((reg.actions.clone(), reg.company_system.clone()), before);
    }
}
#[test]
fn flat_tax_book_and_bad_rate_are_explicitly_rejected() {
    let (mut actions, system, cal, catalog, cash) = fixture();
    actions.dividend_tax_books.push(tax_book());
    assert!(prepare(
        &actions,
        &system,
        &cal,
        &catalog,
        &cash,
        "2030-06-03",
        CashDividendTaxMode::FlatWithholding
    )
    .is_err());
    actions.dividend_tax_books.clear();
    assert!(actions
        .distributions_day_end_candidate(DistributionsDayEndInput {
            day: date("2030-06-03"),
            calendar: &cal,
            catalog: &catalog,
            company_system: &system,
            available_cash: &cash,
            tax_mode: CashDividendTaxMode::FlatWithholding,
            capital_reserve_tax_sources: &BTreeMap::new(),
            flat_withholding_bp: Some(10_001)
        })
        .is_err());
}

#[test]
fn individual_tax_collects_partial_cash_and_keeps_unpaid_debt() {
    let (mut actions, mut system, cal, catalog, cash) = fixture();
    cash_plan(&mut actions, &mut system, &cal);
    actions.dividend_tax_books.push(tax_book());
    let reg = prepare(
        &actions,
        &system,
        &cal,
        &catalog,
        &cash,
        "2030-06-03",
        CashDividendTaxMode::AShareIndividual,
    )
    .unwrap();
    let mut held = reg.actions;
    held.dividend_tax_books[0]
        .record_net_day("held".into(), date("2030-06-04"), 0, None)
        .unwrap();
    let paid = prepare(
        &held,
        &reg.company_system,
        &cal,
        &catalog,
        &cash,
        "2030-06-04",
        CashDividendTaxMode::AShareIndividual,
    )
    .unwrap();
    assert_eq!(paid.projected_available_cash[&AccountId(1)].cents(), 21);
    let mut sold = paid.actions;
    sold.dividend_tax_books[0]
        .record_net_day("sold".into(), date("2030-06-05"), -3, None)
        .unwrap();
    let scarce = BTreeMap::from([
        (AccountId(1), Money::from_cents(1)),
        (AccountId(2), Money::ZERO),
    ]);
    let collected = prepare(
        &sold,
        &paid.company_system,
        &cal,
        &catalog,
        &scarce,
        "2030-06-05",
        CashDividendTaxMode::AShareIndividual,
    )
    .unwrap();
    let receipt = collected.actions.dividend_tax_books[0]
        .collections()
        .last()
        .unwrap();
    assert_eq!(receipt.collected.cents(), 1);
    assert_eq!(receipt.outstanding.numerator(), 3);
    assert!(receipt.needs_funds);
    assert_eq!(
        collected.projected_available_cash[&AccountId(1)],
        Money::ZERO
    );
    assert!(
        matches!(&collected.intents[..], [DistributionIntent::DividendWithholding { amount, source: WithholdingSource::IndividualCollection { .. }, .. }] if amount.cents() == 1)
    );
    let retry = prepare(
        &collected.actions,
        &collected.company_system,
        &cal,
        &catalog,
        &collected.projected_available_cash,
        "2030-06-05",
        CashDividendTaxMode::AShareIndividual,
    )
    .unwrap();
    assert!(retry.intents.is_empty());
    assert_eq!(retry.actions, collected.actions);
}

#[test]
fn exempt_keeps_explicit_tax_book_without_auto_creating_other_books() {
    let (mut actions, mut system, cal, catalog, cash) = fixture();
    cash_plan(&mut actions, &mut system, &cal);
    actions.dividend_tax_books.push(tax_book());
    let reg = prepare(
        &actions,
        &system,
        &cal,
        &catalog,
        &cash,
        "2030-06-03",
        CashDividendTaxMode::Exempt,
    )
    .unwrap();
    let mut held = reg.actions;
    held.dividend_tax_books[0]
        .record_net_day("held".into(), date("2030-06-04"), 0, None)
        .unwrap();
    let paid = prepare(
        &held,
        &reg.company_system,
        &cal,
        &catalog,
        &cash,
        "2030-06-04",
        CashDividendTaxMode::Exempt,
    )
    .unwrap();
    assert_eq!(paid.actions.dividend_tax_books.len(), 1);
    assert_eq!(
        paid.actions.account_gross_receipts[0].tax_status,
        DividendTaxStatus::IndividualPublicMarket
    );
    assert_eq!(
        paid.actions.account_gross_receipts[1].tax_status,
        DividendTaxStatus::TreatmentNotConfigured
    );
}

#[test]
fn stock_distribution_updates_tax_lot_and_bonus_basis_without_buy_freeze() {
    let (mut actions, mut system, cal, catalog, cash) = fixture();
    stock_plan(&mut actions, &mut system);
    actions.dividend_tax_books.push(tax_book());
    let reg = prepare(
        &actions,
        &system,
        &cal,
        &catalog,
        &cash,
        "2030-06-03",
        CashDividendTaxMode::AShareIndividual,
    )
    .unwrap();
    let mut held = reg.actions;
    held.dividend_tax_books[0]
        .record_net_day("held".into(), date("2030-06-04"), 0, None)
        .unwrap();
    let credited = prepare(
        &held,
        &reg.company_system,
        &cal,
        &catalog,
        &cash,
        "2030-06-04",
        CashDividendTaxMode::AShareIndividual,
    )
    .unwrap();
    let tax = &credited.actions.dividend_tax_books[0];
    let new_lot = tax
        .lots()
        .iter()
        .find(|l| matches!(l.source, TaxAcquisitionSource::CorporateAction { .. }))
        .unwrap();
    assert_eq!(new_lot.acquired_on, date("2030-06-04"));
    assert_eq!(
        tax.lots().iter().map(|l| l.qty).sum::<u64>(),
        3 + new_lot.qty
    );
    let encoded = serde_json::to_value(tax).unwrap();
    assert_eq!(encoded["stock_bonus_bases"].as_array().unwrap().len(), 1);
}

#[test]
fn unsupported_restricted_stock_credit_is_explicit_and_atomic() {
    let (mut actions, mut system, cal, catalog, cash) = fixture();
    let mut holdings = actions.registries[0].holdings().to_vec();
    holdings[0].lots[0].restriction = ShareRestriction::Restricted {
        reason: "长期限售".into(),
        release_on: date("2031-06-04"),
    };
    actions.registries[0] =
        ShareRegistry::new(code(), CompanyId(0), 10, date("2030-06-03"), holdings).unwrap();
    stock_plan(&mut actions, &mut system);
    let reg = prepare(
        &actions,
        &system,
        &cal,
        &catalog,
        &cash,
        "2030-06-03",
        CashDividendTaxMode::Exempt,
    )
    .unwrap();
    let before = (reg.actions.clone(), reg.company_system.clone());
    let error = prepare(
        &reg.actions,
        &reg.company_system,
        &cal,
        &catalog,
        &cash,
        "2030-06-04",
        CashDividendTaxMode::Exempt,
    )
    .unwrap_err();
    assert!(error.to_string().contains("长期限售"));
    assert_eq!(error.account, Some(AccountId(1)));
    assert_eq!((reg.actions, reg.company_system), before);
}

#[test]
fn issuer_treasury_never_receives_dividend_or_stock_distribution() {
    let (mut actions, mut system, cal, catalog, cash) = fixture();
    let mut holdings = actions.registries[0].holdings().to_vec();
    holdings[2].lots[0].qty = 2;
    holdings.push(ShareHolding {
        holder: HolderId::IssuerTreasury,
        lots: vec![lot("treasury", 2)],
    });
    let mut registry =
        ShareRegistry::new(code(), CompanyId(0), 10, date("2030-06-03"), holdings).unwrap();
    registry
        .set_issuer_repurchase_account(
            crate::company::share_registry::IssuerRepurchaseAccountFacts {
                account_reference: "repurchase".into(),
                source_evidence: "fixture".into(),
                established_on: date("2030-06-03"),
            },
        )
        .unwrap();
    actions.registries[0] = registry;
    cash_plan(&mut actions, &mut system, &cal);
    stock_plan(&mut actions, &mut system);
    let registered = prepare(
        &actions,
        &system,
        &cal,
        &catalog,
        &cash,
        "2030-06-03",
        CashDividendTaxMode::FlatWithholding,
    )
    .unwrap();
    let paid = prepare(
        &registered.actions,
        &registered.company_system,
        &cal,
        &catalog,
        &cash,
        "2030-06-04",
        CashDividendTaxMode::FlatWithholding,
    )
    .unwrap();
    assert_eq!(paid.actions.dividends[0].total_gross().unwrap().cents(), 56);
    assert_eq!(paid.actions.registries[0].issued_shares(), 14);
    assert_eq!(
        paid.actions.stock_distributions[0]
            .receipt()
            .unwrap()
            .issuer_treasury_shares_excluded,
        2
    );
    assert!(paid.sources.iter().all(|s| match s {
        DistributionSource::SimpleSyntheticDividend { holder, .. }
        | DistributionSource::StockDistribution { holder, .. } =>
            *holder != HolderId::IssuerTreasury,
    }));
    assert_eq!(paid.actions.external_receipts[0].gross.cents(), 14);
}

#[test]
fn zero_flat_withholding_keeps_receipts_without_zero_tax_admin_intents() {
    let (mut actions, mut system, cal, catalog, cash) = fixture();
    cash_plan(&mut actions, &mut system, &cal);
    let reg = prepare(
        &actions,
        &system,
        &cal,
        &catalog,
        &cash,
        "2030-06-03",
        CashDividendTaxMode::FlatWithholding,
    )
    .unwrap();
    for rate in [0, 1] {
        let paid = reg
            .actions
            .distributions_day_end_candidate(DistributionsDayEndInput {
                day: date("2030-06-04"),
                calendar: &cal,
                catalog: &catalog,
                company_system: &reg.company_system,
                available_cash: &cash,
                tax_mode: CashDividendTaxMode::FlatWithholding,
                capital_reserve_tax_sources: &BTreeMap::new(),
                flat_withholding_bp: Some(rate),
            })
            .unwrap();
        assert_eq!(paid.actions.flat_withholding_receipts.len(), 2);
        assert!(paid
            .actions
            .flat_withholding_receipts
            .iter()
            .all(|r| r.withheld == Money::ZERO));
        assert_eq!(paid.intents.len(), 2);
        assert!(paid
            .intents
            .iter()
            .all(|i| matches!(i, DistributionIntent::DividendGrossCredit { .. })));
        assert_eq!(paid.projected_available_cash[&AccountId(1)].cents(), 21);
    }
}

#[test]
fn negative_available_cash_error_keeps_account_and_actual_cents() {
    let (actions, system, cal, catalog, mut cash) = fixture();
    cash.insert(AccountId(2), Money::from_cents(-17));
    let error = prepare(
        &actions,
        &system,
        &cal,
        &catalog,
        &cash,
        "2030-06-03",
        CashDividendTaxMode::Exempt,
    )
    .unwrap_err();
    assert_eq!(error.account, Some(AccountId(2)));
    assert!(error.to_string().contains("-17"), "{error}");
    assert!(
        matches!(error.source, DistributionsDayEndFailure::NegativeAvailableCash { account: AccountId(2), actual } if actual.cents() == -17)
    );
}

#[test]
fn personal_capital_reserve_conversion_requires_complete_premium_source_fact() {
    let (mut actions, mut system, cal, catalog, cash) = fixture();
    stock_plan_kind(
        &mut actions,
        &mut system,
        StockDistributionKind::CapitalReserveConversion,
    );
    actions.dividend_tax_books.push(tax_book());
    let registered = prepare(
        &actions,
        &system,
        &cal,
        &catalog,
        &cash,
        "2030-06-03",
        CashDividendTaxMode::AShareIndividual,
    )
    .unwrap();
    let mut held = registered.actions;
    held.dividend_tax_books[0]
        .record_net_day("held".into(), date("2030-06-04"), 0, None)
        .unwrap();
    let before = (held.clone(), registered.company_system.clone());
    for source in [
        None,
        Some(CapitalReserveTaxSource::Other {
            evidence: "回购注销转入".into(),
        }),
        Some(CapitalReserveTaxSource::StockPremium {
            evidence: " ".into(),
            capital_increase: amount(500),
        }),
        Some(CapitalReserveTaxSource::StockPremium {
            evidence: "发行溢价权威引用".into(),
            capital_increase: amount(499),
        }),
    ] {
        let sources = source
            .map(|s| BTreeMap::from([("stock".into(), s)]))
            .unwrap_or_default();
        let result = held.distributions_day_end_candidate(DistributionsDayEndInput {
            day: date("2030-06-04"),
            calendar: &cal,
            catalog: &catalog,
            company_system: &registered.company_system,
            available_cash: &cash,
            tax_mode: CashDividendTaxMode::AShareIndividual,
            flat_withholding_bp: None,
            capital_reserve_tax_sources: &sources,
        });
        assert!(
            result.is_err(),
            "缺失、非溢价、空证据或不完整金额不得被kind静默当免税"
        );
        assert_eq!((held.clone(), registered.company_system.clone()), before);
    }
    let premium = CapitalReserveTaxSource::StockPremium {
        evidence: "发行溢价权威引用".into(),
        capital_increase: amount(500),
    };
    let sources = BTreeMap::from([("stock".into(), premium.clone())]);
    let credited = held
        .distributions_day_end_candidate(DistributionsDayEndInput {
            day: date("2030-06-04"),
            calendar: &cal,
            catalog: &catalog,
            company_system: &registered.company_system,
            available_cash: &cash,
            tax_mode: CashDividendTaxMode::AShareIndividual,
            flat_withholding_bp: None,
            capital_reserve_tax_sources: &sources,
        })
        .unwrap();
    assert!(credited.sources.iter().all(|s| matches!(s, DistributionSource::StockDistribution { capital_reserve_tax_source: Some(source), .. } if *source == premium)));
    let tax = serde_json::to_value(&credited.actions.dividend_tax_books[0]).unwrap();
    assert!(tax["stock_bonus_bases"].as_array().unwrap().is_empty());
}
