use super::*;
use crate::StockCode;
use crate::accounting::{
    IncomeTaxPolicy, JournalLine, LedgerAccountId, PostingSide, TaxPolicy, VatPolicy,
};
use crate::calendar::CalendarExchange;
use crate::company::config::CompanySystemConfig;
use crate::company::issuer_repurchase::{
    IssuerRepurchaseBook, IssuerRepurchaseFinanceFact, IssuerRepurchasePlan,
    RepurchaseCompletionPolicy, RepurchaseFillRecord, RepurchasePurpose,
};
use crate::company::rights_offering::{
    HolderRightsSettlement, RightsOfferingBook, RightsOfferingDeclaration, RightsOfferingEventPlan,
    RightsOfferingMode, RightsSettlementReceipt, RightsSubscriptionRecord,
    RightsSubscriptionStrategy, SubscriptionOrigin,
};
use crate::company::share_registry::{
    AcquisitionSource, DayNetChange, HolderId, MovementScope, NetAcquisition, ShareDayRequest,
    ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
};
use crate::company::share_split::{
    ShareSplitBook, ShareSplitDeclaration, ShareSplitDirection, ShareSplitEventPlan,
    holder_inherited_restriction,
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
use crate::session::{SecurityCategory, StockExchange, StockSpec};
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
    setup_stocks: Vec<StockSpec>,
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
            setup_stocks: vec![StockSpec {
                code: code(),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(100),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
                tick: Money::from_cents(1),
                total_shares: 1_000,
                float_shares: 1_000,
            }],
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
            setup_stocks: &self.setup_stocks,
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

impl Fixture {
    fn redenominate(&mut self, direction: ShareSplitDirection) {
        let after_par = match direction {
            ShareSplitDirection::Split => 50,
            ShareSplitDirection::Consolidate => 200,
        };
        self.system
            .declare_share_split(
                &CompanyId(0),
                ShareSplitDeclaration {
                    event_id: "transform-a".into(),
                    approval_reference: "批准-transform-a".into(),
                    direction: direction.clone(),
                    approved_on: date("2030-06-03"),
                    ratio: 2,
                    par_value_before: Money::from_cents(100),
                    par_value_after: Money::from_cents(after_par),
                    registered_capital_at_approval: AccountingAmount::from_cents(100_000),
                },
            )
            .unwrap();
        let plan = ShareSplitEventPlan {
            event_id: "transform-a".into(),
            approval_reference: "批准-transform-a".into(),
            issuer: CompanyId(0),
            stock: code(),
            exchange: CalendarExchange::Sse,
            direction,
            ratio: 2,
            approved_on: date("2030-06-03"),
            announced_on: date("2030-06-03"),
            registered_on: date("2030-06-03"),
            ex_rights_on: date("2030-06-04"),
        };
        let mut book = ShareSplitBook::new(plan.clone()).unwrap();
        book.announce(plan.announced_on).unwrap();
        let registry = &mut self.actions.registries[0];
        let snapshot = registry
            .register(plan.event_id.clone(), plan.registered_on)
            .unwrap()
            .clone();
        let receipt = book.register(snapshot, &self.calendar).unwrap().clone();
        registry
            .close_day(ShareDayRequest {
                event_id: "market-2030-06-04".into(),
                day: plan.ex_rights_on,
                scope: MovementScope::PublicMarket,
                changes: vec![],
            })
            .unwrap();
        let outcome = &receipt.holders[0];
        let change = i128::from(outcome.new_shares) - i128::from(outcome.original_shares);
        let (acquisition, consume) = if change > 0 {
            (
                Some(NetAcquisition {
                    lot_id: "transform-a-owner-7".into(),
                    source: AcquisitionSource::CorporateAction {
                        event: plan.event_id.clone(),
                    },
                    restriction: holder_inherited_restriction(outcome, plan.ex_rights_on).unwrap(),
                }),
                None,
            )
        } else {
            (
                None,
                Some(vec![crate::company::share_registry::LotConsumption {
                    lot_id: "initial-owner-7".into(),
                    qty: u64::try_from(-change).unwrap(),
                }]),
            )
        };
        registry
            .close_day(ShareDayRequest {
                event_id: "share-transform:transform-a".into(),
                day: plan.ex_rights_on,
                scope: MovementScope::ShareReDenomination {
                    basis: plan.approval_reference,
                },
                changes: vec![DayNetChange {
                    holder: outcome.holder.clone(),
                    change,
                    acquisition,
                    consume,
                }],
            })
            .unwrap();
        book.mark_settled(plan.ex_rights_on).unwrap();
        self.system
            .record_share_split_credit(
                &CompanyId(0),
                &plan.event_id,
                plan.ex_rights_on,
                receipt.issued_shares_before,
                receipt.issued_shares_after,
            )
            .unwrap();
        self.actions.share_splits.push(book);
    }
}

#[test]
fn capital_14_real_split_restores_2000_shares_without_changing_capital() {
    let mut f = Fixture::new();
    f.redenominate(ShareSplitDirection::Split);
    f.system = serde_json::from_str(&serde_json::to_string(&f.system).unwrap()).unwrap();
    f.actions = serde_json::from_str(&serde_json::to_string(&f.actions).unwrap()).unwrap();
    let summary = f.validate("2030-06-04").unwrap();
    assert_eq!(summary.companies[&CompanyId(0)].issued_shares, 2_000);
    assert_eq!(
        summary.companies[&CompanyId(0)].registered_capital,
        Some(AccountingAmount::from_cents(100_000))
    );
    assert_eq!(summary.companies[&CompanyId(0)].receipt_frontier, 2);
    assert_eq!(
        f.system
            .outstanding_par_value_at(&CompanyId(0), date("2030-06-04"))
            .unwrap(),
        Some(Money::from_cents(50))
    );
}

#[test]
fn capital_15_real_consolidation_preserves_capital_and_explicit_lot_consumption() {
    let mut f = Fixture::new();
    f.redenominate(ShareSplitDirection::Consolidate);
    let summary = f.validate("2030-06-04").unwrap();
    assert_eq!(summary.companies[&CompanyId(0)].issued_shares, 500);
    assert_eq!(
        summary.companies[&CompanyId(0)].registered_capital,
        Some(AccountingAmount::from_cents(100_000))
    );
    let consumed = f.actions.registries[0].receipts()[1].request.changes[0]
        .consume
        .as_ref()
        .unwrap();
    assert_eq!(
        consumed,
        &vec![crate::company::share_registry::LotConsumption {
            lot_id: "initial-owner-7".into(),
            qty: 500
        }]
    );
}

#[test]
fn capital_16_changed_current_issuer_2200_does_not_pass_real_split_replay() {
    let mut f = Fixture::new();
    f.redenominate(ShareSplitDirection::Split);
    let mut saved = serde_json::to_value(&f.system).unwrap();
    saved["issuers"]["0"]["issued_shares"] = serde_json::json!("2200");
    f.system = serde_json::from_value(saved).unwrap();
    assert!(matches!(
        f.validate("2030-06-04"),
        Err(CompanyCapitalRestoreError::IssuedShares {
            context: CapitalRestoreContext {
                company: Some(CompanyId(0)),
                stock: Some(StockId(0)),
                ..
            },
            expected: 2_000,
            actual: 2_200,
        })
    ));
}

impl Fixture {
    fn rights(&mut self, paid_shares: u64, settle: bool) -> CivilDate {
        let mut plan = RightsOfferingEventPlan {
            event_id: "rights-a".into(),
            approval_reference: "批准-rights-a".into(),
            issuer: CompanyId(0),
            stock: code(),
            exchange: CalendarExchange::Sse,
            approved_on: date("2030-06-03"),
            announced_on: date("2030-06-03"),
            registered_on: date("2030-06-03"),
            payment_start_on: date("2030-06-04"),
            payment_deadline_on: date("2030-06-04"),
            ex_rights_on: date("2030-06-04"),
            settlement_on: date("2030-06-04"),
            listing_on: date("2030-06-04"),
            price_per_share: Money::from_cents(200),
            mode: RightsOfferingMode::RightsToAllShareholders {
                shares_per_existing_share_micros: 500_000,
            },
            npc_subscription_strategy: RightsSubscriptionStrategy::FullByDefault,
        };
        plan.derive_schedule(&self.calendar, CalendarExchange::Sse, 1)
            .unwrap();
        self.system
            .declare_rights_offering(
                &CompanyId(0),
                RightsOfferingDeclaration {
                    event_id: plan.event_id.clone(),
                    approval_reference: plan.approval_reference.clone(),
                    approved_on: plan.approved_on,
                    price_per_share: plan.price_per_share,
                    par_value_per_share: Money::from_cents(100),
                    registered_capital_at_approval: AccountingAmount::from_cents(100_000),
                },
            )
            .unwrap();
        let mut book = RightsOfferingBook::new(plan.clone()).unwrap();
        if settle {
            book.announce(plan.announced_on).unwrap();
            let registry = &mut self.actions.registries[0];
            let snapshot = registry
                .register(plan.event_id.clone(), plan.registered_on)
                .unwrap()
                .clone();
            book.entitle(snapshot, &self.calendar).unwrap();
            let paid_amount = Money::from_cents(i64::try_from(paid_shares).unwrap() * 200);
            book.record_subscription(RightsSubscriptionRecord {
                holder: HolderId::Account(AccountId(7)),
                requested_shares: 500,
                price_per_share: plan.price_per_share,
                submitted_on: plan.payment_start_on,
                origin: SubscriptionOrigin::Explicit,
                paid_shares,
                paid_amount,
                waived_shares: 500 - paid_shares,
            })
            .unwrap();
            book.close_payment_window(plan.payment_deadline_on).unwrap();
            let failed = book.determine_failure().unwrap();
            let refund = if failed { paid_amount } else { Money::ZERO };
            book.settle(
                plan.settlement_on,
                RightsSettlementReceipt {
                    event_id: plan.event_id.clone(),
                    settlement_on: plan.settlement_on,
                    failed,
                    total_paid_shares: paid_shares,
                    total_paid_amount: paid_amount,
                    refunded_total: refund,
                    holders: vec![HolderRightsSettlement {
                        holder: HolderId::Account(AccountId(7)),
                        paid_shares,
                        paid_amount,
                        waived_shares: 500 - paid_shares,
                        refunded_amount: refund,
                    }],
                },
            )
            .unwrap();
            let mut day = registry.settled_on();
            while day < plan.settlement_on {
                day = day.next().unwrap();
                registry
                    .close_day(ShareDayRequest {
                        event_id: format!("market-{day}"),
                        day,
                        scope: MovementScope::PublicMarket,
                        changes: vec![],
                    })
                    .unwrap();
            }
            if !failed {
                registry
                    .close_day(ShareDayRequest {
                        event_id: "rights-offering:rights-a".into(),
                        day: plan.settlement_on,
                        scope: MovementScope::NonTradingTransfer {
                            basis: plan.approval_reference.clone(),
                        },
                        changes: vec![DayNetChange {
                            holder: HolderId::Account(AccountId(7)),
                            change: i128::from(paid_shares),
                            acquisition: Some(NetAcquisition {
                                lot_id: "rights-a-owner-7".into(),
                                source: AcquisitionSource::CorporateAction {
                                    event: plan.event_id.clone(),
                                },
                                restriction: ShareRestriction::Unrestricted,
                            }),
                            consume: None,
                        }],
                    })
                    .unwrap();
            }
            self.system
                .record_rights_offering_settlement(
                    &CompanyId(0),
                    &plan.event_id,
                    plan.settlement_on,
                    if failed { 0 } else { paid_shares },
                    if failed {
                        AccountingAmount::ZERO
                    } else {
                        AccountingAmount::from_money(paid_amount)
                    },
                )
                .unwrap();
        }
        self.actions.rights_offerings.push(book);
        plan.settlement_on
    }
}

#[test]
fn capital_17_successful_rights_uses_actual_settled_shares_and_no_credited_flag() {
    let mut f = Fixture::new();
    let through = f.rights(400, true);
    assert!(f.actions.rights_offerings[0].credited_on().is_none());
    let summary = f.validate(&through.to_string()).unwrap();
    assert_eq!(summary.companies[&CompanyId(0)].issued_shares, 1_400);
    assert_eq!(
        summary.companies[&CompanyId(0)].registered_capital,
        Some(AccountingAmount::from_cents(140_000))
    );
    assert_eq!(
        summary.companies[&CompanyId(0)].receipt_frontier,
        f.actions.registries[0].receipts().len()
    );
}

#[test]
fn capital_18_failed_rights_refund_does_not_issue_subscribed_shares() {
    let mut f = Fixture::new();
    let through = f.rights(100, true);
    let receipt = f.actions.rights_offerings[0].settlement().unwrap();
    assert!(receipt.failed);
    assert_eq!(receipt.total_paid_shares, 100);
    assert_eq!(receipt.refunded_total, Money::from_cents(20_000));
    let summary = f.validate(&through.to_string()).unwrap();
    assert_eq!(summary.companies[&CompanyId(0)].issued_shares, 1_000);
    assert_eq!(
        summary.companies[&CompanyId(0)].registered_capital,
        Some(AccountingAmount::from_cents(100_000))
    );
}

#[test]
fn capital_19_approved_rights_keeps_future_payment_and_listing_without_delta() {
    let mut f = Fixture::new();
    let through = f.rights(0, false);
    assert!(through > date("2030-06-03"));
    let summary = f.validate("2030-06-03").unwrap();
    assert_eq!(summary.companies[&CompanyId(0)].issued_shares, 1_000);
    assert_eq!(summary.companies[&CompanyId(0)].receipt_frontier, 0);
}

impl Fixture {
    fn repurchase(&mut self, cancel: bool, consume_initial_treasury: bool) {
        let plan = IssuerRepurchasePlan {
            event_id: "buy-a".into(),
            approval_reference: "批准-buy-a".into(),
            issuer: CompanyId(0),
            stock: code(),
            exchange: CalendarExchange::Sse,
            approved_on: date("2030-06-03"),
            announced_on: date("2030-06-03"),
            window_start_on: date("2030-06-03"),
            window_deadline_on: date("2030-06-04"),
            price_cap_per_share: Money::from_cents(100),
            total_budget: Money::from_cents(20_000),
            max_shares: 100,
            purpose: RepurchasePurpose::ReduceCapital,
            completion_policy: RepurchaseCompletionPolicy::CancelOnCompletion,
            approval_unit_event_id: None,
        };
        self.system
            .declare_issuer_repurchase(
                &CompanyId(0),
                IssuerRepurchaseFinanceFact {
                    event_id: plan.event_id.clone(),
                    approval_reference: plan.approval_reference.clone(),
                    approved_on: plan.approved_on,
                    synthetic_funding: AccountingAmount::from_money(plan.total_budget),
                    purpose: plan.purpose.clone(),
                    spent: None,
                    withdrawn_remainder: None,
                    completed_on: None,
                    cancelled_shares: 0,
                    cancelled_on: None,
                    capital_reduction: None,
                },
            )
            .unwrap();
        let mut book = IssuerRepurchaseBook::new(plan.clone()).unwrap();
        book.announce(plan.announced_on).unwrap();
        book.record_fill(
            RepurchaseFillRecord {
                stock: code(),
                day: date("2030-06-04"),
                shares: 100,
                gross: Money::from_cents(10_000),
                fees: Money::ZERO,
            },
            &[],
        )
        .unwrap();
        if consume_initial_treasury {
            self.actions.registries[0] = ShareRegistry::new(
                code(),
                CompanyId(0),
                1_000,
                date("2030-06-03"),
                vec![
                    ShareHolding {
                        holder: HolderId::Account(AccountId(7)),
                        lots: vec![lot("initial-owner-7", 900)],
                    },
                    ShareHolding {
                        holder: HolderId::IssuerTreasury,
                        lots: vec![lot("initial-treasury", 100)],
                    },
                ],
            )
            .unwrap();
        }
        let registry = &mut self.actions.registries[0];
        registry
            .set_issuer_repurchase_account(
                crate::company::share_registry::IssuerRepurchaseAccountFacts {
                    account_reference: "回购专户".into(),
                    source_evidence: "测试显式配置".into(),
                    established_on: date("2030-06-03"),
                },
            )
            .unwrap();
        registry
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
                        holder: HolderId::IssuerTreasury,
                        change: 100,
                        acquisition: Some(NetAcquisition {
                            lot_id: "actual-buy-a-lot".into(),
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
        if cancel {
            book.complete(date("2030-06-04"), Money::from_cents(10_000))
                .unwrap();
            self.system
                .record_issuer_repurchase_completion(
                    &CompanyId(0),
                    &plan.event_id,
                    date("2030-06-04"),
                    AccountingAmount::from_cents(10_000),
                    AccountingAmount::from_cents(10_000),
                )
                .unwrap();
            registry
                .close_day(ShareDayRequest {
                    event_id: "issuer-repurchase-cancellation:buy-a".into(),
                    day: date("2030-06-04"),
                    scope: MovementScope::IssuerRepurchaseCancellation {
                        basis: plan.approval_reference.clone(),
                    },
                    changes: vec![DayNetChange {
                        holder: HolderId::IssuerTreasury,
                        change: -100,
                        acquisition: None,
                        consume: Some(vec![crate::company::share_registry::LotConsumption {
                            lot_id: if consume_initial_treasury {
                                "initial-treasury".into()
                            } else {
                                "actual-buy-a-lot".into()
                            },
                            qty: 100,
                        }]),
                    }],
                })
                .unwrap();
            book.record_cancellation(date("2030-06-04"), 100, &[])
                .unwrap();
            self.system
                .record_issuer_repurchase_cancellation(
                    &CompanyId(0),
                    &plan.event_id,
                    date("2030-06-04"),
                    100,
                    1_000,
                )
                .unwrap();
        }
        self.actions.issuer_repurchases.push(book);
    }

    fn validate_manual(
        &self,
        through: &str,
    ) -> Result<CapitalReplaySummary, CompanyCapitalRestoreError> {
        validate_company_capital_restore(CapitalRestoreInput {
            setup_stocks: &self.setup_stocks,
            setup_issuers: &self.initial,
            system: &self.system,
            actions: &self.actions,
            catalog: &self.catalog,
            calendar: &self.calendar,
            committed_through: date(through),
            initial_par: None,
        })
    }
}

#[test]
fn capital_20_manual_repurchase_only_reconstructs_capital_without_approval_snapshot() {
    let mut f = Fixture::new();
    f.repurchase(true, false);
    assert_eq!(
        f.system
            .dividend_legal_facts(&CompanyId(0))
            .unwrap()
            .unwrap()
            .registered_capital,
        AccountingAmount::from_cents(90_000)
    );
    let summary = f.validate_manual("2030-06-04").unwrap();
    assert_eq!(summary.companies[&CompanyId(0)].issued_shares, 900);
    assert_eq!(
        summary.companies[&CompanyId(0)].registered_capital,
        Some(AccountingAmount::from_cents(90_000))
    );
    assert_eq!(summary.companies[&CompanyId(0)].receipt_frontier, 2);
}

#[test]
fn capital_21_treasury_market_buy_is_zero_delta_before_cancellation() {
    let mut f = Fixture::new();
    f.repurchase(false, false);
    let summary = f.validate_manual("2030-06-04").unwrap();
    assert_eq!(summary.companies[&CompanyId(0)].issued_shares, 1_000);
    assert_eq!(
        summary.companies[&CompanyId(0)].registered_capital,
        Some(AccountingAmount::from_cents(100_000))
    );
}

#[test]
fn capital_22_cancellation_cannot_consume_initial_treasury_instead_of_bought_lot() {
    let mut f = Fixture::new();
    f.repurchase(true, true);
    assert!(
        matches!(f.validate_manual("2030-06-04"), Err(CompanyCapitalRestoreError::ActionMismatch {
        context: CapitalRestoreContext { company: Some(CompanyId(0)), stock: Some(StockId(0)), kind: Some(CorporateCapitalKind::IssuerRepurchaseCancellation), event_id: Some(event), receipt_index: Some(1), .. }, field: "repurchase-lot-source",
    }) if event == "buy-a")
    );
}

#[test]
fn capital_23_cancellation_disposal_payload_must_match_real_bought_lot() {
    let mut f = Fixture::new();
    f.repurchase(true, false);
    let mut wire = serde_json::to_value(&f.actions).unwrap();
    wire["registries"][0]["receipts"][1]["disposals"][0]["lot"]["source"] =
        serde_json::json!({"InitialAllocation": {"evidence": "同id与数量但伪造的处置来源"}});
    f.actions = serde_json::from_value(wire).unwrap();
    assert!(
        matches!(f.validate_manual("2030-06-04"), Err(CompanyCapitalRestoreError::ActionMismatch {
        context: CapitalRestoreContext { company: Some(CompanyId(0)), stock: Some(StockId(0)), kind: Some(CorporateCapitalKind::IssuerRepurchaseCancellation), event_id: Some(event), receipt_index: Some(1), .. }, field: "repurchase-lot-source",
    }) if event == "buy-a")
    );
}

#[test]
fn capital_24_repurchase_purpose_cannot_drift_between_book_and_finance() {
    let mut f = Fixture::new();
    f.repurchase(true, false);
    let mut wire = serde_json::to_value(&f.actions).unwrap();
    wire["issuer_repurchases"][0]["plan"]["purpose"] = serde_json::json!("EmployeeIncentive");
    f.actions = serde_json::from_value(wire).unwrap();
    assert!(
        matches!(f.validate_manual("2030-06-04"), Err(CompanyCapitalRestoreError::ActionMismatch {
        context: CapitalRestoreContext { company: Some(CompanyId(0)), kind: Some(CorporateCapitalKind::IssuerRepurchaseCancellation), event_id: Some(event), .. }, field: "purpose",
    }) if event == "buy-a")
    );
}

#[test]
fn capital_25_stable_identity_checks_each_represented_field() {
    for field in ["code", "name", "industry", "kind", "listed_stock"] {
        let mut f = Fixture::new();
        let mut spec = f.initial.get(&CompanyId(0)).unwrap().clone();
        match field {
            "code" => spec.code = "DIFFERENT".into(),
            "name" => spec.name = "不同名称".into(),
            "industry" => spec.industry = IndustryId("不同的行业".into()),
            "kind" => spec.kind = CompanyKind::Bank,
            "listed_stock" => spec.listed_stock = None,
            _ => unreachable!(),
        }
        f.initial = IssuerRegistry::new(vec![spec]).unwrap();
        assert!(
            matches!(f.validate("2030-06-03"), Err(CompanyCapitalRestoreError::StableIdentity { company: CompanyId(0), field: actual }) if actual == field),
            "{field}"
        );
    }
}

#[test]
fn capital_26_unlisted_company_and_independent_company_stock_id_stay_distinct() {
    let mut f = Fixture::new();
    let mut listed = f.initial.get(&CompanyId(0)).unwrap().clone();
    listed.code = "Z-listed".into();
    let mut unlisted = listed.clone();
    unlisted.code = "A-unlisted".into();
    unlisted.listed_stock = None;
    unlisted.issued_shares = 17;
    let mut config = f.system.config();
    let CompanySystemConfig::Simple(ref mut simple) = config else {
        unreachable!()
    };
    let mut unlisted_config = simple.companies[0].clone();
    unlisted_config.company = CompanyId(0);
    simple.companies[0].company = CompanyId(1);
    simple.companies.push(unlisted_config);
    f.initial = IssuerRegistry::new(vec![listed.clone(), unlisted.clone()]).unwrap();
    f.system =
        CompanySystem::create(vec![listed, unlisted], config, date("2030-06-03"), 19).unwrap();
    f.system
        .define_dividend_legal_facts(
            &CompanyId(1),
            AccountingAmount::from_cents(100_000),
            "测试显式法定根".into(),
        )
        .unwrap();
    let mut actions = serde_json::to_value(&f.actions).unwrap();
    actions["registries"][0]["issuer"] = serde_json::json!(1);
    f.actions = serde_json::from_value(actions).unwrap();
    let summary = f.validate("2030-06-03").unwrap();
    assert_eq!(summary.companies[&CompanyId(0)].stock, None);
    assert_eq!(summary.companies[&CompanyId(0)].issued_shares, 17);
    assert_eq!(summary.companies[&CompanyId(1)].stock, Some(StockId(0)));
    assert_eq!(summary.companies[&CompanyId(1)].issued_shares, 1_000);
}

#[test]
fn capital_27_duplicate_formal_book_is_rejected_with_exact_action() {
    let mut f = Fixture::new();
    f.distribution("duplicate-dist", "2030-06-03", "2030-06-04", 100);
    f.actions
        .stock_distributions
        .push(f.actions.stock_distributions[0].clone());
    assert!(
        matches!(f.validate("2030-06-03"), Err(CompanyCapitalRestoreError::Duplicate {
        context: CapitalRestoreContext { company: Some(CompanyId(0)), kind: Some(CorporateCapitalKind::StockDistribution), event_id: Some(event), .. },
    }) if event == "duplicate-dist")
    );
}

#[test]
fn capital_28_wrong_receipt_basis_is_not_accepted_by_matching_prefix() {
    let mut f = Fixture::new();
    f.distribution("dist-a", "2030-06-03", "2030-06-04", 100);
    f.credit_distribution();
    let mut wire = serde_json::to_value(&f.actions).unwrap();
    wire["registries"][0]["receipts"][1]["request"]["scope"] =
        serde_json::json!({"NonTradingTransfer": {"basis": "不同批准"}});
    f.actions = serde_json::from_value(wire).unwrap();
    assert!(
        matches!(f.validate("2030-06-04"), Err(CompanyCapitalRestoreError::ActionMismatch {
        context: CapitalRestoreContext { kind: Some(CorporateCapitalKind::StockDistribution), event_id: Some(event), receipt_index: Some(1), .. }, field: "receipt-scope-or-day",
    }) if event == "dist-a")
    );
}

#[test]
fn capital_29_same_day_registration_frontier_keeps_before_and_after_capital() {
    let mut f = Fixture::new();
    f.distribution("dist-a", "2030-06-03", "2030-06-04", 100);
    f.credit_distribution();
    let registry = &mut f.actions.registries[0];
    let after = registry
        .register("after-credit".into(), date("2030-06-04"))
        .unwrap();
    assert_eq!(after.settled_receipts(), 2);
    assert_eq!(after.issued_shares(), 1_100);
    assert_eq!(
        registry.registration("dist-a").unwrap().settled_receipts(),
        0
    );
    assert_eq!(
        registry.registration("dist-a").unwrap().issued_shares(),
        1_000
    );
    assert_eq!(
        f.validate("2030-06-04").unwrap().companies[&CompanyId(0)].issued_shares,
        1_100
    );
}

#[test]
fn capital_30_replay_u64_overflow_reports_actual_receipt_position() {
    let mut f = Fixture::new();
    f.distribution("overflow-dist", "2030-06-03", "2030-06-04", 100);
    f.credit_distribution();
    let mut spec = f.initial.get(&CompanyId(0)).unwrap().clone();
    spec.issued_shares = u64::MAX;
    f.initial = IssuerRegistry::new(vec![spec]).unwrap();
    assert!(matches!(
        f.validate("2030-06-04"),
        Err(CompanyCapitalRestoreError::Arithmetic {
            context: CapitalRestoreContext {
                company: Some(CompanyId(0)),
                stock: Some(StockId(0)),
                kind: Some(CorporateCapitalKind::StockDistribution),
                event_id: Some(event),
                receipt_index: Some(1),
                ..
            },
        }) if event == "overflow-dist"
    ));
}

#[test]
fn capital_31_replay_cannot_reduce_issued_capital_to_zero() {
    let mut f = Fixture::new();
    f.repurchase(true, false);
    let mut spec = f.initial.get(&CompanyId(0)).unwrap().clone();
    spec.issued_shares = 100;
    f.initial = IssuerRegistry::new(vec![spec]).unwrap();
    assert!(matches!(
        f.validate_manual("2030-06-04"),
        Err(CompanyCapitalRestoreError::Arithmetic {
            context: CapitalRestoreContext {
                company: Some(CompanyId(0)),
                stock: Some(StockId(0)),
                receipt_index: Some(1),
                ..
            },
        })
    ));
}

#[test]
fn capital_32_two_same_day_credits_keep_registration_basis_for_each_book() {
    let mut f = Fixture::new();
    f.distribution("dist-a", "2030-06-03", "2030-06-04", 100);
    f.distribution("dist-b", "2030-06-03", "2030-06-04", 100);
    for book in &mut f.actions.stock_distributions {
        let plan = book.plan().clone();
        book.announce(plan.announced_on).unwrap();
        let frozen = f.actions.registries[0]
            .register(plan.event_id, plan.registered_on)
            .unwrap()
            .clone();
        book.register(frozen, &f.calendar).unwrap();
    }
    f.actions.stock_distributions.swap(0, 1);
    f.credit_distribution();
    f.actions.stock_distributions.swap(0, 1);
    f.credit_distribution();
    assert_eq!(
        f.actions.stock_distributions[1]
            .receipt()
            .unwrap()
            .issued_shares_before,
        1_000
    );
    let summary = f.validate("2030-06-04").unwrap();
    assert_eq!(summary.companies[&CompanyId(0)].issued_shares, 1_200);
    assert_eq!(summary.companies[&CompanyId(0)].receipt_frontier, 3);
    assert_eq!(
        summary.companies[&CompanyId(0)].registered_capital,
        Some(AccountingAmount::from_cents(120_000))
    );
}

#[test]
fn capital_33_credited_book_and_finance_require_exact_registry_receipt() {
    let mut f = Fixture::new();
    f.distribution("dist-a", "2030-06-03", "2030-06-04", 100);
    f.credit_distribution();
    let mut wire = serde_json::to_value(&f.actions).unwrap();
    wire["registries"][0]["receipts"] = serde_json::json!([]);
    wire["registries"][0]["issued_shares"] = serde_json::json!("1000");
    wire["registries"][0]["settled_on"] = serde_json::to_value(date("2030-06-03")).unwrap();
    wire["registries"][0]["holdings"][0]["lots"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    f.actions = serde_json::from_value(wire).unwrap();
    assert!(
        matches!(f.validate("2030-06-04"), Err(CompanyCapitalRestoreError::MissingReceipt {
        context: CapitalRestoreContext { company: Some(CompanyId(0)), kind: Some(CorporateCapitalKind::StockDistribution), event_id: Some(event), .. },
    }) if event == "dist-a")
    );
}

#[test]
fn capital_34_validator_uses_explicit_frozen_calendar_instead_of_default() {
    let mut f = Fixture::new();
    f.distribution("dist-a", "2030-06-03", "2030-06-04", 100);
    let mut spec = f.calendar.policy().spec();
    // 仅为证明输入 policy 权威的 synthetic fixture，不是对2030官方休市的预测。
    spec.official_coverage
        .push(crate::calendar::OfficialCoverageEntry::new(
            CalendarExchange::Sse,
            2030,
            vec![(date("2030-06-03"), date("2030-06-03"))],
            "synthetic-capital-restore-policy".into(),
            "synthetic-fixture-not-official-source".into(),
        ));
    f.calendar =
        TradingCalendar::from_policy(crate::calendar::CalendarPolicy::from_parts(spec).unwrap())
            .unwrap();
    assert!(
        matches!(f.validate("2030-06-03"), Err(CompanyCapitalRestoreError::StockDistribution {
        context: CapitalRestoreContext { company: Some(CompanyId(0)), kind: Some(CorporateCapitalKind::StockDistribution), event_id: Some(event), .. },
        source: crate::company::stock_distribution::StockDistributionError::InvalidTradingDate { date: day, .. },
    }) if event == "dist-a" && day == date("2030-06-03"))
    );
}

#[test]
fn capital_35_announced_fact_cannot_be_later_than_committed_cutoff() {
    let mut f = Fixture::new();
    let registered_on = f
        .calendar
        .next_trading_day(CalendarExchange::Sse, date("2030-06-04"))
        .unwrap();
    let ex = f
        .calendar
        .next_trading_day(CalendarExchange::Sse, registered_on)
        .unwrap();
    f.distribution(
        "future-announced",
        &registered_on.to_string(),
        &ex.to_string(),
        100,
    );
    let mut plan = f.actions.stock_distributions[0].plan().clone();
    plan.announced_on = date("2030-06-04");
    let mut book = StockDistributionBook::new(plan).unwrap();
    book.announce(date("2030-06-04")).unwrap();
    f.actions.stock_distributions[0] = book;
    assert!(
        matches!(f.validate("2030-06-03"), Err(CompanyCapitalRestoreError::FutureFact {
        context: CapitalRestoreContext { company: Some(CompanyId(0)), kind: Some(CorporateCapitalKind::StockDistribution), event_id: Some(event), day: Some(day), .. }, committed_through,
    }) if event == "future-announced" && day == date("2030-06-04") && committed_through == date("2030-06-03"))
    );
}

#[test]
fn capital_36_closed_payment_window_is_an_occurred_fact_even_without_subscriptions() {
    let mut f = Fixture::new();
    f.rights(0, false);
    let book = &mut f.actions.rights_offerings[0];
    let plan = book.plan().clone();
    book.announce(plan.announced_on).unwrap();
    let snapshot = f.actions.registries[0]
        .register(plan.event_id.clone(), plan.registered_on)
        .unwrap()
        .clone();
    book.entitle(snapshot, &f.calendar).unwrap();
    book.close_payment_window(plan.payment_deadline_on).unwrap();
    assert!(
        matches!(f.validate("2030-06-03"), Err(CompanyCapitalRestoreError::FutureFact {
        context: CapitalRestoreContext { company: Some(CompanyId(0)), kind: Some(CorporateCapitalKind::RightsOffering), event_id: Some(event), day: Some(day), .. }, committed_through,
    }) if event == "rights-a" && day == plan.payment_deadline_on && committed_through == date("2030-06-03"))
    );
}

#[test]
fn capital_37_setup_stocks_must_cover_catalog() {
    let mut f = Fixture::new();
    f.setup_stocks.clear();
    assert!(
        matches!(f.validate("2030-06-03"), Err(CompanyCapitalRestoreError::SetupStockMapping {
        stock_code, reason: SetupStockMappingReason::Missing,
    }) if stock_code == code().0)
    );
}

#[test]
fn capital_38_setup_stocks_must_not_duplicate_code() {
    let mut f = Fixture::new();
    f.setup_stocks.push(f.setup_stocks[0].clone());
    assert!(
        matches!(f.validate("2030-06-03"), Err(CompanyCapitalRestoreError::SetupStockMapping {
        stock_code, reason: SetupStockMappingReason::Duplicate,
    }) if stock_code == code().0)
    );
}

#[test]
fn capital_39_setup_stocks_must_not_add_unknown_security() {
    let mut f = Fixture::new();
    let mut extra = f.setup_stocks[0].clone();
    extra.code = StockCode("000001".into());
    extra.exchange = StockExchange::Shenzhen;
    f.setup_stocks.push(extra);
    assert!(
        matches!(f.validate("2030-06-03"), Err(CompanyCapitalRestoreError::SetupStockMapping {
        stock_code, reason: SetupStockMappingReason::Unexpected,
    }) if stock_code == "000001")
    );
}

fn assert_exchange_error(
    f: &Fixture,
    through: &str,
    expected_kind: CorporateCapitalKind,
    expected_event: &str,
) {
    assert!(
        matches!(f.validate(through), Err(CompanyCapitalRestoreError::ExchangeMismatch {
        context: CapitalRestoreContext { company: Some(CompanyId(0)), stock: Some(StockId(0)), kind: Some(kind), event_id: Some(event), .. },
        expected: CalendarExchange::Sse, actual: CalendarExchange::Szse,
    }) if kind == expected_kind && event == expected_event)
    );
}

#[test]
fn capital_40_approved_distribution_uses_actual_setup_exchange() {
    let mut f = Fixture::new();
    f.distribution("exchange-dist", "2030-06-03", "2030-06-04", 100);
    let mut plan = f.actions.stock_distributions[0].plan().clone();
    plan.exchange = CalendarExchange::Szse;
    f.actions.stock_distributions[0] = StockDistributionBook::new(plan).unwrap();
    f.actions.stock_distributions[0]
        .validate_with_calendar(&f.calendar)
        .unwrap();
    assert_exchange_error(
        &f,
        "2030-06-03",
        CorporateCapitalKind::StockDistribution,
        "exchange-dist",
    );
}

#[test]
fn capital_41_settled_split_uses_actual_setup_exchange() {
    let mut f = Fixture::new();
    f.redenominate(ShareSplitDirection::Split);
    let mut wire = serde_json::to_value(&f.actions).unwrap();
    wire["share_splits"][0]["plan"]["exchange"] =
        serde_json::to_value(CalendarExchange::Szse).unwrap();
    f.actions = serde_json::from_value(wire).unwrap();
    f.actions.share_splits[0]
        .validate_with_calendar(&f.calendar)
        .unwrap();
    assert_exchange_error(
        &f,
        "2030-06-04",
        CorporateCapitalKind::ShareReDenomination,
        "transform-a",
    );
}

#[test]
fn capital_42_settled_rights_uses_actual_setup_exchange() {
    let mut f = Fixture::new();
    let settled_on = f.rights(400, true);
    let mut wire = serde_json::to_value(&f.actions).unwrap();
    wire["rights_offerings"][0]["plan"]["exchange"] =
        serde_json::to_value(CalendarExchange::Szse).unwrap();
    f.actions = serde_json::from_value(wire).unwrap();
    f.actions.rights_offerings[0]
        .validate_with_calendar(&f.calendar)
        .unwrap();
    assert_exchange_error(
        &f,
        &settled_on.to_string(),
        CorporateCapitalKind::RightsOffering,
        "rights-a",
    );
}

#[test]
fn capital_43_cancelled_repurchase_uses_actual_setup_exchange() {
    let mut f = Fixture::new();
    f.repurchase(true, false);
    let mut wire = serde_json::to_value(&f.actions).unwrap();
    wire["issuer_repurchases"][0]["plan"]["exchange"] =
        serde_json::to_value(CalendarExchange::Szse).unwrap();
    f.actions = serde_json::from_value(wire).unwrap();
    f.actions.issuer_repurchases[0]
        .validate_with_calendar(&f.calendar)
        .unwrap();
    assert_exchange_error(
        &f,
        "2030-06-04",
        CorporateCapitalKind::IssuerRepurchaseCancellation,
        "buy-a",
    );
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

#[test]
fn capital_13_future_registration_without_receipts_is_not_committed() {
    let mut f = Fixture::new();
    let mut future = ShareRegistry::new(
        code(),
        CompanyId(0),
        1_000,
        date("2030-06-04"),
        vec![ShareHolding {
            holder: HolderId::Account(AccountId(7)),
            lots: vec![lot("initial-owner-7", 1_000)],
        }],
    )
    .unwrap();
    future
        .register("future-registration".into(), date("2030-06-04"))
        .unwrap();
    assert!(future.receipts().is_empty());
    future.validate().unwrap();
    f.actions.registries = vec![future];
    assert!(
        matches!(f.validate("2030-06-03"), Err(CompanyCapitalRestoreError::FutureFact {
        context: CapitalRestoreContext { company: Some(CompanyId(0)), stock: Some(StockId(0)), day: Some(day), .. }, committed_through,
    }) if day == date("2030-06-04") && committed_through == date("2030-06-03"))
    );
}
