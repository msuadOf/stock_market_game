use super::*;
use std::collections::BTreeMap;
use crate::{Money, orderbook::AccountId};
use crate::session::corporate_actions::{SessionCorporateActions, DividendTaxStatus};
use crate::company::cash_dividend_tax::CashDividendTaxMode;
use crate::{
    accounting::{AccountingAmount, IncomeTaxPolicy, JournalLine, LedgerAccountId, PostingSide, TaxPolicy, VatPolicy},
    calendar::{CalendarExchange, CalendarPolicy, CivilDate, TradingCalendar},
    company::{cash_dividend::{CashDividendBook, CashDividendPlan}, cash_dividend_tax::{CashDividendTaxBook, DividendTaxLot, DividendTaxProfile, TaxAcquisitionSource, TaxShareClass}, config::CompanySystemConfig, DividendDeclaration, ex_reference_price::CashDividendFormula, share_registry::{AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction}, simple::{period::{AnnualTrendConfig, PeriodGenerationParameters, PeriodNoiseConfig, PeriodVariableExpenseRule, SettlementCycle}, SimpleBookDisplayConfig, SimpleCompanyConfig, SimpleCompanyPreferences, SimpleConfig, SimpleEnvironmentConfig, SimpleFinanceConfig, SimpleSummaryRule}, stock_distribution::{StockDistributionBook, StockDistributionDeclaration, StockDistributionEventPlan, StockDistributionKind}, CompanyId, CompanyKind, CompanySpec, CompanySystem, IndustryId},
    stock::{StockCatalog, StockMeta},
};

fn date(text: &str) -> CivilDate { CivilDate::from_iso(text).unwrap() }
fn amount(cents: i128) -> AccountingAmount { AccountingAmount::from_cents(cents) }
fn code() -> crate::StockCode { crate::StockCode("600101".into()) }
fn lot(id: &str, qty: u64) -> ShareLot {
    ShareLot { id: id.into(), qty, acquired_on: date("2030-05-31"), source: AcquisitionSource::InitialAllocation { evidence: "fixture".into() }, restriction: ShareRestriction::Unrestricted }
}
fn fixture() -> (SessionCorporateActions, CompanySystem, TradingCalendar, StockCatalog, BTreeMap<AccountId, Money>) {
    let zero = || PeriodNoiseConfig { monthly_bp: 0, quarterly_bp: 0, half_year_bp: 0, annual_bp: 0 };
    let line = |account: &str, side, cents| JournalLine { account: LedgerAccountId(account.into()), side, amount: amount(cents) };
    let mut system = CompanySystem::create(vec![CompanySpec { code: "C1".into(), name: "测试公司".into(), industry: IndustryId("测试行业".into()), kind: CompanyKind::Industrial, listed_stock: Some(code()), issued_shares: 10, group_parent: None }], CompanySystemConfig::Simple(SimpleConfig {
        environment: SimpleEnvironmentConfig { initial_change_bp: 0, persistence_bp: 0, noise: zero() },
        companies: vec![SimpleCompanyConfig { company: CompanyId(0), kind: CompanyKind::Industrial,
            generation: PeriodGenerationParameters { initial_revenue: amount(100_000), initial_fixed_expense: amount(0), revenue_trend: AnnualTrendConfig::Fixed { annual_growth_bp: 0 }, fixed_expense_trend: AnnualTrendConfig::Fixed { annual_growth_bp: 0 }, revenue_noise: zero(), fixed_expense_noise: zero(), demand_sensitivity_bp: 0, variable_expense: PeriodVariableExpenseRule::RevenueRatio { ratio_bp: 0, noise: zero() } },
            finance: SimpleFinanceConfig { opening_lines: vec![line("1002", PostingSide::Debit, 2_000), line("4001", PostingSide::Credit, 1_000), line("simple_statutory_reserve", PostingSide::Credit, 1_000)], tax_policy: TaxPolicy { version: 1, vat: VatPolicy { output_rate_bp: 0, input_rate_bp: 0, deductible_share_bp: 10_000 }, income_tax: IncomeTaxPolicy { rate_bp: 0, loss_carryforward_years: 5 } }, summary_rule: SimpleSummaryRule::ReceivableRevenuePayableExpenses, book_display: SimpleBookDisplayConfig::DEFAULT }, preferences: SimpleCompanyPreferences::none() }], prehistory_periods: 1, settlement_cycle: SettlementCycle::Monthly,
    }), date("2030-06-03"), 19).unwrap();
    system.define_dividend_legal_facts(&CompanyId(0), amount(1_000), "fixture".into()).unwrap();
    let registry = ShareRegistry::new(code(), CompanyId(0), 10, date("2030-06-03"), vec![
        ShareHolding { holder: HolderId::Account(AccountId(1)), lots: vec![lot("p", 3)] },
        ShareHolding { holder: HolderId::Account(AccountId(2)), lots: vec![lot("i", 3)] },
        ShareHolding { holder: HolderId::External("外部股东".into()), lots: vec![lot("e", 4)] },
    ]).unwrap();
    let actions = SessionCorporateActions { registries: vec![registry], ..Default::default() };
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let catalog = StockCatalog::from_metas(vec![StockMeta::new("600101")]).unwrap();
    (actions, system, calendar, catalog, BTreeMap::from([(AccountId(1), Money::ZERO), (AccountId(2), Money::ZERO)]))
}
fn cash_plan(actions: &mut SessionCorporateActions, system: &mut CompanySystem, calendar: &TradingCalendar) {
    let plan = CashDividendPlan::new("cash".into(), CompanyId(0), code(), CalendarExchange::Sse, CashDividendFormula::StandardCashOnly, date("2030-06-03"), date("2030-06-03"), date("2030-06-03"), date("2030-06-04"), date("2030-06-04"), Money::from_cents(7), Money::from_cents(70), calendar).unwrap();
    system.declare_dividend(&CompanyId(0), DividendDeclaration { plan_id: "cash".into(), approved_on: date("2030-06-03"), total_gross: amount(70), registered_capital: amount(1_000) }).unwrap();
    actions.dividends.push(CashDividendBook::new(plan).unwrap());
}
fn stock_plan(actions: &mut SessionCorporateActions, system: &mut CompanySystem) {
    let plan = StockDistributionEventPlan { event_id: "stock".into(), approval_reference: "approved".into(), issuer: CompanyId(0), stock: code(), exchange: CalendarExchange::Sse, kind: StockDistributionKind::BonusShares, approved_on: date("2030-06-03"), announced_on: date("2030-06-03"), registered_on: date("2030-06-03"), ex_rights_on: date("2030-06-04"), shares_per_existing_share_micros: 500_000, approved_total_new_shares: 5 };
    system.declare_stock_distribution(&CompanyId(0), StockDistributionDeclaration { event_id: "stock".into(), approval_reference: "approved".into(), kind: plan.kind.clone(), approved_on: plan.approved_on, new_shares: 5, par_value_per_share: Money::from_cents(100), capital_increase: amount(500), registered_capital_at_approval: amount(1_000) }).unwrap();
    actions.stock_distributions.push(StockDistributionBook::new(plan).unwrap());
}
fn prepare(actions: &SessionCorporateActions, system: &CompanySystem, calendar: &TradingCalendar, catalog: &StockCatalog, cash: &BTreeMap<AccountId, Money>, day: &str, tax_mode: CashDividendTaxMode) -> Result<DistributionsDayEndCandidate, DistributionsDayEndError> {
    actions.distributions_day_end_candidate(DistributionsDayEndInput { day: date(day), calendar, catalog, company_system: system, available_cash: cash, tax_mode, flat_withholding_bp: (tax_mode == CashDividendTaxMode::FlatWithholding).then_some(5_000) })
}
fn tax_book() -> CashDividendTaxBook {
    CashDividendTaxBook::new(AccountId(1), code(), DividendTaxProfile::IndividualPublicMarket, date("2030-06-03"), vec![DividendTaxLot { id: "tax:p".into(), qty: 3, acquired_on: date("2030-05-31"), source: TaxAcquisitionSource::InitialAllocation { evidence: "fixture".into() }, class: TaxShareClass::PublicMarket }]).unwrap()
}
#[test]
fn flat_preserves_gross_tax_exit_external_source_and_inputs() {
    let (mut actions, mut system, cal, catalog, cash) = fixture(); cash_plan(&mut actions, &mut system, &cal);
    let before = (actions.clone(), system.clone(), cash.clone());
    let registered = prepare(&actions, &system, &cal, &catalog, &cash, "2030-06-03", CashDividendTaxMode::FlatWithholding).unwrap();
    let paid = prepare(&registered.actions, &registered.company_system, &cal, &catalog, &cash, "2030-06-04", CashDividendTaxMode::FlatWithholding).unwrap();
    assert_eq!((actions, system, cash.clone()), before);
    assert_eq!(paid.projected_available_cash[&AccountId(1)].cents(), 10);
    assert_eq!(paid.projected_available_cash[&AccountId(2)].cents(), 10);
    assert_eq!(paid.actions.flat_withholding_receipts.len(), 2);
    assert_eq!(paid.actions.flat_withholding_receipts[0].withheld.cents(), 11);
    assert_eq!(paid.actions.external_receipts[0].gross.cents(), 28);
    assert!(paid.actions.dividend_tax_books.is_empty());
    assert_eq!(paid.intents.len(), 4);
    assert_eq!(paid.sources.len(), 3);
    let replay = prepare(&paid.actions, &paid.company_system, &cal, &catalog, &paid.projected_available_cash, "2030-06-04", CashDividendTaxMode::FlatWithholding).unwrap();
    assert!(replay.intents.is_empty()); assert!(replay.sources.is_empty()); assert_eq!(replay.actions, paid.actions);
}
#[test]
fn registered_person_sells_before_payment_but_keeps_right_and_partial_tax_debt() {
    let (mut actions, mut system, cal, catalog, cash) = fixture(); cash_plan(&mut actions, &mut system, &cal); actions.dividend_tax_books.push(tax_book());
    let mut registered = prepare(&actions, &system, &cal, &catalog, &cash, "2030-06-03", CashDividendTaxMode::AShareIndividual).unwrap();
    registered.actions.dividend_tax_books[0].record_net_day("sale".into(), date("2030-06-04"), -3, None).unwrap();
    let paid = prepare(&registered.actions, &registered.company_system, &cal, &catalog, &cash, "2030-06-04", CashDividendTaxMode::AShareIndividual).unwrap();
    assert_eq!(paid.actions.account_gross_receipts[0].gross.cents(), 21);
    assert_eq!(paid.actions.account_gross_receipts[0].tax_status, DividendTaxStatus::IndividualPublicMarket);
    assert_eq!(paid.actions.account_gross_receipts[1].tax_status, DividendTaxStatus::TreatmentNotConfigured);
    assert_eq!(paid.projected_available_cash[&AccountId(1)].cents(), 17);
    assert_eq!(paid.projected_available_cash[&AccountId(2)].cents(), 21);
    let mut indebted = paid.actions.clone();
    indebted.dividend_tax_books[0].record_net_day("next".into(), date("2030-06-05"), 0, None).unwrap();
    // 税基20%四舍五入为4分；付款已收缴，重试不重复扣款。
    assert_eq!(paid.actions.dividend_tax_books[0].collections()[0].collected.cents(), 4);
    assert_eq!(indebted.dividend_tax_books[0].outstanding().unwrap().numerator(), 0);
}
#[test]
fn stock_credit_returns_available_shares_and_all_three_candidate_totals() {
    let (mut actions, mut system, cal, catalog, cash) = fixture(); stock_plan(&mut actions, &mut system);
    let registered = prepare(&actions, &system, &cal, &catalog, &cash, "2030-06-03", CashDividendTaxMode::Exempt).unwrap();
    let credited = prepare(&registered.actions, &registered.company_system, &cal, &catalog, &cash, "2030-06-04", CashDividendTaxMode::Exempt).unwrap();
    assert_eq!(credited.actions.registries[0].issued_shares(), 15);
    assert_eq!(credited.company_system.issuers().get(&CompanyId(0)).unwrap().issued_shares, 15);
    assert_eq!(credited.sources.iter().filter_map(|s| match s { DistributionSource::StockDistribution { shares, .. } => Some(*shares), _ => None }).sum::<u64>(), 5);
    assert!(credited.intents.iter().all(|i| matches!(i, DistributionIntent::StockDistributionCredit { available_on, .. } if *available_on == date("2030-06-04"))));
    assert_eq!(actions.registries[0].issued_shares(), 10); assert_eq!(system.issuers().get(&CompanyId(0)).unwrap().issued_shares, 10);
    let replay = prepare(&credited.actions, &credited.company_system, &cal, &catalog, &cash, "2030-06-04", CashDividendTaxMode::Exempt).unwrap();
    assert!(replay.intents.is_empty()); assert_eq!(replay.actions, credited.actions);
}
#[test]
fn missing_account_and_overflow_abort_whole_candidate() {
    let (mut actions, mut system, cal, catalog, cash) = fixture(); cash_plan(&mut actions, &mut system, &cal);
    let reg = prepare(&actions, &system, &cal, &catalog, &cash, "2030-06-03", CashDividendTaxMode::Exempt).unwrap();
    for bad in [BTreeMap::from([(AccountId(1), Money::ZERO)]), BTreeMap::from([(AccountId(1), Money::ZERO), (AccountId(2), Money::from_cents(i64::MAX))])] {
        let before = (reg.actions.clone(), reg.company_system.clone());
        assert!(prepare(&reg.actions, &reg.company_system, &cal, &catalog, &bad, "2030-06-04", CashDividendTaxMode::Exempt).is_err());
        assert_eq!((reg.actions.clone(), reg.company_system.clone()), before);
    }
}
#[test]
fn flat_tax_book_and_bad_rate_are_explicitly_rejected() {
    let (mut actions, system, cal, catalog, cash) = fixture(); actions.dividend_tax_books.push(tax_book());
    assert!(prepare(&actions, &system, &cal, &catalog, &cash, "2030-06-03", CashDividendTaxMode::FlatWithholding).is_err());
    actions.dividend_tax_books.clear();
    assert!(actions.distributions_day_end_candidate(DistributionsDayEndInput { day: date("2030-06-03"), calendar: &cal, catalog: &catalog, company_system: &system, available_cash: &cash, tax_mode: CashDividendTaxMode::FlatWithholding, flat_withholding_bp: Some(10_001) }).is_err());
}
