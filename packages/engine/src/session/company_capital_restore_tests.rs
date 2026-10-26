use super::*;
use crate::StockCode;
use crate::accounting::{
    IncomeTaxPolicy, JournalLine, LedgerAccountId, PostingSide, TaxPolicy, VatPolicy,
};
use crate::calendar::CalendarExchange;
use crate::company::config::CompanySystemConfig;
use crate::company::share_registry::{
    AcquisitionSource, DayNetChange, HolderId, MovementScope, NetAcquisition, ShareDayRequest,
    ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
};
use crate::company::simple::period::{
    AnnualTrendConfig, PeriodGenerationParameters, PeriodNoiseConfig, PeriodVariableExpenseRule,
    SettlementCycle,
};
use crate::company::simple::{
    SimpleBookDisplayConfig, SimpleCompanyConfig, SimpleCompanyPreferences, SimpleConfig,
    SimpleEnvironmentConfig, SimpleFinanceConfig, SimpleSummaryRule,
};
use crate::company::stock_distribution::{
    StockDistributionBook, StockDistributionDeclaration, StockDistributionEventPlan,
    StockDistributionKind, holder_credit_lots,
};
use crate::company::{CompanyKind, CompanySpec, IndustryId};
use crate::orderbook::AccountId;
use crate::stock::StockMeta;

fn date(text: &str) -> CivilDate {
    CivilDate::from_iso(text).unwrap()
}

fn code() -> StockCode {
    StockCode("600101".into())
}

fn lot(id: &str, qty: u64) -> ShareLot {
    ShareLot {
        id: id.into(),
        qty,
        acquired_on: date("2030-05-31"),
        source: AcquisitionSource::InitialAllocation {
            evidence: "资本恢复测试的显式开局分配".into(),
        },
        restriction: ShareRestriction::Unrestricted,
    }
}

struct Fixture {
    initial: IssuerRegistry,
    system: CompanySystem,
    actions: SessionCorporateActions,
    catalog: StockCatalog,
    calendar: TradingCalendar,
}

impl Fixture {
    fn new() -> Self {
        let zero = || PeriodNoiseConfig {
            monthly_bp: 0,
            quarterly_bp: 0,
            half_year_bp: 0,
            annual_bp: 0,
        };
        let spec = CompanySpec {
            code: "C1".into(),
            name: "资本恢复测试公司".into(),
            industry: IndustryId("测试行业".into()),
            kind: CompanyKind::Industrial,
            listed_stock: Some(code()),
            issued_shares: 1_000,
            group_parent: None,
        };
        let line = |account: &str, side, cents| JournalLine {
            account: LedgerAccountId(account.into()),
            side,
            amount: AccountingAmount::from_cents(cents),
        };
        let mut system = CompanySystem::create(
            vec![spec.clone()],
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
                        initial_revenue: AccountingAmount::from_cents(100_000),
                        initial_fixed_expense: AccountingAmount::ZERO,
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
                            line("1002", PostingSide::Debit, 300_000),
                            line("4001", PostingSide::Credit, 100_000),
                            line("simple_capital_reserve", PostingSide::Credit, 200_000),
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
            .define_dividend_legal_facts(
                &CompanyId(0),
                AccountingAmount::from_cents(100_000),
                "测试手工装配的注册资本来源".into(),
            )
            .unwrap();
        let registry = ShareRegistry::new(
            code(),
            CompanyId(0),
            1_000,
            date("2030-06-03"),
            vec![ShareHolding {
                holder: HolderId::Account(AccountId(7)),
                lots: vec![lot("initial-owner-7", 1_000)],
            }],
        )
        .unwrap();
        Self {
            initial: IssuerRegistry::new(vec![spec]).unwrap(),
            system,
            actions: SessionCorporateActions {
                registries: vec![registry],
                ..Default::default()
            },
            catalog: StockCatalog::from_metas(vec![StockMeta::new("600101")]).unwrap(),
            calendar: TradingCalendar::current_default_calendar().unwrap(),
        }
    }

    fn validate(&self, through: &str) -> Result<CapitalReplaySummary, CompanyCapitalRestoreError> {
        validate_company_capital_restore(CapitalRestoreInput {
            setup_issuers: &self.initial,
            system: &self.system,
            actions: &self.actions,
            catalog: &self.catalog,
            calendar: &self.calendar,
            committed_through: date(through),
            initial_par: Some(Money::from_cents(100)),
        })
    }

    fn distribution(&mut self, event: &str, register: &str, ex: &str, new_shares: u64) {
        let capital = self
            .system
            .dividend_legal_facts(&CompanyId(0))
            .unwrap()
            .unwrap()
            .registered_capital;
        self.system
            .declare_stock_distribution(
                &CompanyId(0),
                StockDistributionDeclaration {
                    event_id: event.into(),
                    approval_reference: format!("批准-{event}"),
                    kind: StockDistributionKind::CapitalReserveConversion,
                    approved_on: date("2030-06-03"),
                    new_shares,
                    par_value_per_share: Money::from_cents(100),
                    capital_increase: AccountingAmount::from_cents(i128::from(new_shares) * 100),
                    registered_capital_at_approval: capital,
                },
            )
            .unwrap();
        self.actions.stock_distributions.push(
            StockDistributionBook::new(StockDistributionEventPlan {
                event_id: event.into(),
                approval_reference: format!("批准-{event}"),
                issuer: CompanyId(0),
                stock: code(),
                exchange: CalendarExchange::Sse,
                kind: StockDistributionKind::CapitalReserveConversion,
                approved_on: date("2030-06-03"),
                announced_on: date("2030-06-03"),
                registered_on: date(register),
                ex_rights_on: date(ex),
                shares_per_existing_share_micros: 100_000,
                approved_total_new_shares: new_shares,
            })
            .unwrap(),
        );
    }

    fn credit_distribution(&mut self) {
        let book = self.actions.stock_distributions.last_mut().unwrap();
        let plan = book.plan().clone();
        let registry = &mut self.actions.registries[0];
        book.announce(plan.announced_on).unwrap();
        let snapshot = registry
            .register(plan.event_id.clone(), plan.registered_on)
            .unwrap()
            .clone();
        let receipt = book.register(snapshot, &self.calendar).unwrap().clone();
        registry
            .close_day(ShareDayRequest {
                event_id: format!("market-{}", plan.ex_rights_on),
                day: plan.ex_rights_on,
                scope: MovementScope::PublicMarket,
                changes: vec![],
            })
            .unwrap();
        let changes = holder_credit_lots(&receipt, plan.ex_rights_on)
            .unwrap()
            .into_iter()
            .map(|lot| DayNetChange {
                holder: lot.holder,
                change: i128::from(lot.qty),
                acquisition: Some(NetAcquisition {
                    lot_id: format!("stock-distribution:{}:owner-7", plan.event_id),
                    source: AcquisitionSource::CorporateAction {
                        event: plan.event_id.clone(),
                    },
                    restriction: lot.restriction,
                }),
                consume: None,
            })
            .collect();
        registry
            .close_day(ShareDayRequest {
                event_id: format!("stock-distribution:{}", plan.event_id),
                day: plan.ex_rights_on,
                scope: MovementScope::NonTradingTransfer {
                    basis: plan.approval_reference.clone(),
                },
                changes,
            })
            .unwrap();
        book.mark_credited(plan.ex_rights_on).unwrap();
        self.system
            .record_stock_distribution_credit(
                &plan.issuer,
                &plan.event_id,
                plan.ex_rights_on,
                plan.approved_total_new_shares,
            )
            .unwrap();
    }
}

#[test]
fn capital_01_unchanged_issuer_and_registry_keep_initial_shares() {
    let f = Fixture::new();
    f.system.validate_restored().unwrap();
    let result = f.validate("2030-06-03").unwrap();
    let company = &result.companies[&CompanyId(0)];
    assert_eq!(company.stock, Some(StockId(0)));
    assert_eq!(company.issued_shares, 1_000);
    assert_eq!(company.receipt_frontier, 0);
    assert_eq!(
        company.registered_capital,
        Some(AccountingAmount::from_cents(100_000))
    );
}

#[test]
fn capital_02_no_registry_is_legal_only_with_unchanged_issuer() {
    let mut f = Fixture::new();
    f.actions.registries.clear();
    let result = f.validate("2030-06-03").unwrap();
    assert_eq!(result.companies[&CompanyId(0)].issued_shares, 1_000);
    assert_eq!(result.companies[&CompanyId(0)].receipt_frontier, 0);
}

#[test]
fn capital_03_stable_name_change_is_not_hidden_by_equal_shares() {
    let mut f = Fixture::new();
    let mut altered = f.initial.get(&CompanyId(0)).unwrap().clone();
    altered.name = "被篡改的开局公司名".into();
    f.initial = IssuerRegistry::new(vec![altered]).unwrap();
    assert!(matches!(
        f.validate("2030-06-03"),
        Err(CompanyCapitalRestoreError::StableIdentity {
            company: CompanyId(0),
            field: "name"
        })
    ));
}

#[test]
fn capital_04_matching_current_registry_cannot_replace_initial_share_root() {
    let mut f = Fixture::new();
    let mut altered = f.initial.get(&CompanyId(0)).unwrap().clone();
    altered.issued_shares = 2_200;
    f.initial = IssuerRegistry::new(vec![altered]).unwrap();
    assert!(matches!(
        f.validate("2030-06-03"),
        Err(CompanyCapitalRestoreError::IssuedShares {
            context: CapitalRestoreContext {
                company: Some(CompanyId(0)),
                stock: Some(StockId(0)),
                ..
            },
            expected: 2_200,
            actual: 1_000,
        })
    ));
}

#[test]
fn capital_05_public_market_transfer_preserves_issued_capital() {
    let mut f = Fixture::new();
    f.actions.registries[0]
        .close_day(ShareDayRequest {
            event_id: "market-2030-06-04".into(),
            day: date("2030-06-04"),
            scope: MovementScope::PublicMarket,
            changes: vec![
                DayNetChange {
                    holder: HolderId::Account(AccountId(7)),
                    change: -100,
                    acquisition: None,
                    consume: None,
                },
                DayNetChange {
                    holder: HolderId::Account(AccountId(9)),
                    change: 100,
                    acquisition: Some(NetAcquisition {
                        lot_id: "market-owner-9".into(),
                        source: AcquisitionSource::SecondaryMarket {
                            settlement: "market-2030-06-04".into(),
                        },
                        restriction: ShareRestriction::Unrestricted,
                    }),
                    consume: None,
                },
            ],
        })
        .unwrap();
    let result = f.validate("2030-06-04").unwrap();
    assert_eq!(result.companies[&CompanyId(0)].issued_shares, 1_000);
    assert_eq!(result.companies[&CompanyId(0)].receipt_frontier, 1);
    assert_eq!(
        result.companies[&CompanyId(0)].registered_capital,
        Some(AccountingAmount::from_cents(100_000))
    );
}

#[test]
fn capital_06_committed_cutoff_rejects_real_future_receipt_with_position() {
    let mut f = Fixture::new();
    f.actions.registries[0]
        .close_day(ShareDayRequest {
            event_id: "market-2030-06-04".into(),
            day: date("2030-06-04"),
            scope: MovementScope::PublicMarket,
            changes: vec![],
        })
        .unwrap();
    assert!(matches!(
        f.validate("2030-06-03"),
        Err(CompanyCapitalRestoreError::FutureFact {
            context: CapitalRestoreContext { company: Some(CompanyId(0)), stock: Some(StockId(0)), kind: Some(CorporateCapitalKind::PublicMarket), receipt_index: Some(0), day: Some(day), .. },
            committed_through,
        }) if day == date("2030-06-04") && committed_through == date("2030-06-03")
    ));
}

#[test]
fn capital_07_nontrading_receipt_needs_real_bound_book_and_finance_fact() {
    let mut f = Fixture::new();
    f.actions.registries[0]
        .close_day(ShareDayRequest {
            event_id: "stock-distribution:unapproved".into(),
            day: date("2030-06-03"),
            scope: MovementScope::NonTradingTransfer {
                basis: "not-approved".into(),
            },
            changes: vec![DayNetChange {
                holder: HolderId::Account(AccountId(7)),
                change: 100,
                acquisition: Some(NetAcquisition {
                    lot_id: "unapproved-new-shares".into(),
                    source: AcquisitionSource::CorporateAction {
                        event: "unapproved".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }),
                consume: None,
            }],
        })
        .unwrap();
    assert!(matches!(
        f.validate("2030-06-03"),
        Err(CompanyCapitalRestoreError::UnboundReceipt {
            context: CapitalRestoreContext { company: Some(CompanyId(0)), stock: Some(StockId(0)), event_id: Some(event), receipt_index: Some(0), .. },
        }) if event == "stock-distribution:unapproved"
    ));
}

#[test]
fn capital_08_real_credited_distribution_replays_once_after_serde_roundtrip() {
    let mut f = Fixture::new();
    f.distribution("dist-a", "2030-06-03", "2030-06-04", 100);
    f.credit_distribution();
    f.system = serde_json::from_str(&serde_json::to_string(&f.system).unwrap()).unwrap();
    f.actions = serde_json::from_str(&serde_json::to_string(&f.actions).unwrap()).unwrap();
    let summary = f.validate("2030-06-04").unwrap();
    let company = &summary.companies[&CompanyId(0)];
    assert_eq!(company.issued_shares, 1_100);
    assert_eq!(company.receipt_frontier, 2);
    assert_eq!(
        company.registered_capital,
        Some(AccountingAmount::from_cents(110_000))
    );
    assert_eq!(
        f.actions.stock_distributions[0]
            .registration()
            .unwrap()
            .settled_receipts(),
        0
    );
}

#[test]
fn capital_09_approved_uncredited_distribution_keeps_legal_future_schedule() {
    let mut f = Fixture::new();
    let registered_on = f
        .calendar
        .next_trading_day(CalendarExchange::Sse, date("2030-06-04"))
        .unwrap();
    let ex_rights_on = f
        .calendar
        .next_trading_day(CalendarExchange::Sse, registered_on)
        .unwrap();
    f.distribution(
        "future-dist",
        &registered_on.to_string(),
        &ex_rights_on.to_string(),
        100,
    );
    f.actions.stock_distributions[0]
        .validate_with_calendar(&f.calendar)
        .unwrap();
    assert!(
        f.system.stock_distribution_facts(&CompanyId(0)).unwrap()[0]
            .credited_on
            .is_none()
    );
    let summary = f.validate("2030-06-03").unwrap();
    assert_eq!(summary.companies[&CompanyId(0)].issued_shares, 1_000);
    assert_eq!(
        summary.companies[&CompanyId(0)].registered_capital,
        Some(AccountingAmount::from_cents(100_000))
    );
}

#[test]
fn capital_10_finance_fact_without_book_is_not_silently_ignored() {
    let mut f = Fixture::new();
    f.distribution("orphan-finance", "2030-06-03", "2030-06-04", 100);
    f.actions.stock_distributions.clear();
    assert!(
        matches!(f.validate("2030-06-03"), Err(CompanyCapitalRestoreError::MissingBook {
        context: CapitalRestoreContext { company: Some(CompanyId(0)), stock: Some(StockId(0)), kind: Some(CorporateCapitalKind::StockDistribution), event_id: Some(event), .. },
    }) if event == "orphan-finance")
    );
}

#[test]
fn capital_11_book_without_finance_fact_reports_exact_action() {
    let mut f = Fixture::new();
    let mut other = Fixture::new();
    other.distribution("orphan-book", "2030-06-03", "2030-06-04", 100);
    f.actions.stock_distributions = other.actions.stock_distributions;
    assert!(
        matches!(f.validate("2030-06-03"), Err(CompanyCapitalRestoreError::MissingFinanceFact {
        context: CapitalRestoreContext { company: Some(CompanyId(0)), stock: Some(StockId(0)), kind: Some(CorporateCapitalKind::StockDistribution), event_id: Some(event), .. },
    }) if event == "orphan-book")
    );
}

#[test]
fn capital_12_credited_distribution_requires_registry_in_same_saved_source() {
    let mut f = Fixture::new();
    f.distribution("dist-missing-registry", "2030-06-03", "2030-06-04", 100);
    f.credit_distribution();
    f.actions.registries.clear();
    assert!(
        matches!(f.validate("2030-06-04"), Err(CompanyCapitalRestoreError::MissingRegistry {
        context: CapitalRestoreContext { company: Some(CompanyId(0)), stock: Some(StockId(0)), kind: Some(CorporateCapitalKind::StockDistribution), event_id: Some(event), .. },
    }) if event == "dist-missing-registry")
    );
}
