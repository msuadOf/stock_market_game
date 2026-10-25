use super::*;
use crate::company::cash_dividend_tax::{
    DividendTaxLot, DividendTaxProfile, TaxAcquisitionSource, TaxShareClass,
};
use crate::company::share_registry::{
    AcquisitionSource, DayNetChange, HolderId, MovementScope, NetAcquisition, ShareDayRequest,
    ShareHolding, ShareLot, ShareRestriction,
};
use crate::company::share_split::{ShareSplitDirection, ShareSplitEventPlan, ShareSplitStatus};
use crate::company::CompanyId;

fn date(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}
fn lot(id: &str, qty: u64) -> ShareLot {
    ShareLot {
        id: id.into(),
        qty,
        acquired_on: date("2020-01-02"),
        source: AcquisitionSource::InitialAllocation {
            evidence: id.into(),
        },
        restriction: ShareRestriction::Unrestricted,
    }
}

struct Fixture {
    registry: ShareRegistry,
    book: ShareSplitBook,
    tax: Vec<CashDividendTaxBook>,
    chain: Vec<SplitUnitContext>,
    buckets: Vec<TreasuryBucketTransform>,
}
impl Fixture {
    fn new(direction: ShareSplitDirection, ratio: u64, shares: u64, treasury: u64) -> Self {
        let stock = StockCode("600001".into());
        let mut holdings = vec![ShareHolding {
            holder: HolderId::Account(AccountId(1)),
            lots: vec![lot("a1", shares)],
        }];
        if treasury > 0 {
            holdings.push(ShareHolding {
                holder: HolderId::IssuerTreasury,
                lots: vec![
                    lot("initial", treasury / 2),
                    lot("plan", treasury - treasury / 2),
                ],
            });
        }
        let mut registry = ShareRegistry::new(
            stock.clone(),
            CompanyId(0),
            shares + treasury,
            date("2030-06-03"),
            holdings,
        )
        .unwrap();
        let plan = ShareSplitEventPlan {
            event_id: "split-1".into(),
            approval_reference: "resolution-1".into(),
            issuer: CompanyId(0),
            stock: stock.clone(),
            exchange: crate::calendar::CalendarExchange::Sse,
            direction: direction.clone(),
            ratio,
            approved_on: date("2030-06-01"),
            announced_on: date("2030-06-02"),
            registered_on: date("2030-06-03"),
            ex_rights_on: date("2030-06-04"),
        };
        let chain = vec![SplitUnitContext {
            event_id: plan.event_id.clone(),
            ex_on: plan.ex_rights_on,
            direction,
            ratio,
        }];
        let mut book = ShareSplitBook::new(plan).unwrap();
        book.announce(date("2030-06-02")).unwrap();
        book.register(
            registry
                .register("split-1".into(), date("2030-06-03"))
                .unwrap()
                .clone(),
            &crate::calendar::TradingCalendar::current_default_calendar().unwrap(),
        )
        .unwrap();
        let tax = vec![CashDividendTaxBook::new(
            AccountId(1),
            stock,
            DividendTaxProfile::IndividualPublicMarket,
            date("2030-06-03"),
            vec![DividendTaxLot {
                id: "tax:a1".into(),
                qty: shares,
                acquired_on: date("2020-01-02"),
                source: TaxAcquisitionSource::InitialAllocation {
                    evidence: "a1".into(),
                },
                class: TaxShareClass::PublicMarket,
            }],
        )
        .unwrap()];
        Self {
            registry,
            book,
            tax,
            chain,
            buckets: vec![],
        }
    }
    fn with_external(mut self, external_shares: u64) -> Self {
        let account_shares = self.tax[0].lots()[0].qty;
        self.registry = ShareRegistry::new(
            self.registry.stock().clone(),
            CompanyId(0),
            account_shares + external_shares,
            date("2030-06-03"),
            vec![
                ShareHolding {
                    holder: HolderId::Account(AccountId(1)),
                    lots: vec![lot("a1", account_shares)],
                },
                ShareHolding {
                    holder: HolderId::External("external".into()),
                    lots: vec![lot("external", external_shares)],
                },
            ],
        )
        .unwrap();
        self.book = ShareSplitBook::new(self.book.plan().clone()).unwrap();
        self.book.announce(date("2030-06-02")).unwrap();
        self.book
            .register(
                self.registry
                    .register("split-1".into(), date("2030-06-03"))
                    .unwrap()
                    .clone(),
                &crate::calendar::TradingCalendar::current_default_calendar().unwrap(),
            )
            .unwrap();
        self
    }
    fn input(&self) -> ShareTransformDayEndInput<'_> {
        ShareTransformDayEndInput {
            day: date("2030-06-04"),
            book: &self.book,
            registry_in_registered_units: &self.registry,
            dividend_tax_books: &self.tax,
            unit_chain: &self.chain,
            treasury_buckets: &self.buckets,
        }
    }
    fn run(&self) -> Result<ShareTransformDayEndCandidate, ShareTransformDayEndError> {
        share_transform_day_end_candidate(self.input())
    }
}
fn shares(registry: &ShareRegistry, holder: HolderId) -> u64 {
    registry
        .holdings()
        .iter()
        .find(|h| h.holder == holder)
        .map_or(0, |h| h.lots.iter().map(|l| l.qty).sum())
}

#[test]
fn frozen_registration_is_consumed_only_on_ex_day_without_mutating_input() {
    let fixture = Fixture::new(ShareSplitDirection::Split, 2, 6, 0);
    let before = fixture.registry.clone();
    let mut early = fixture.input();
    early.day = date("2030-06-03");
    assert!(share_transform_day_end_candidate(early).is_err());
    let result = fixture.run().unwrap();
    assert_eq!(result.phase, ShareTransformCandidatePhase::PreNewUnitMarket);
    assert_eq!(result.unit_event_id, "split-1");
    assert_eq!(result.settled_book.status(), &ShareSplitStatus::Settled);
    assert_eq!(
        shares(
            &result.registry_before_new_unit_market,
            HolderId::Account(AccountId(1))
        ),
        12
    );
    assert_eq!(fixture.registry, before);
    assert_eq!(fixture.book.status(), &ShareSplitStatus::Registered);
    assert_eq!(fixture.run().unwrap(), result);
}

#[test]
fn split_credit_retains_old_tax_lot_and_adds_explicit_current_domain_acquisition() {
    let fixture = Fixture::new(ShareSplitDirection::Split, 2, 6, 0);
    let result = fixture.run().unwrap();
    let intent = &result.account_intents[0];
    assert_eq!((intent.registered_before, intent.converted_after), (6, 12));
    assert_eq!(intent.amount, ShareTransformAmount::ShareCredit { qty: 6 });
    assert_eq!(
        intent.cost_policy,
        ShareTransformCostPolicy::PreserveUnlessActualPositionCloses
    );
    let lots = result.dividend_tax_books_before_new_unit_market[0].lots();
    assert_eq!(lots.len(), 2);
    assert_eq!(lots[0], fixture.tax[0].lots()[0]);
    assert_eq!(lots[1].acquired_on, date("2030-06-04"));
    assert_eq!(lots[1].qty, 6);
}

#[test]
fn consolidation_emits_reduction_and_preserves_surviving_tax_acquisition_date() {
    let fixture = Fixture::new(ShareSplitDirection::Consolidate, 3, 6, 0);
    let result = fixture.run().unwrap();
    assert_eq!(
        result.account_intents[0].amount,
        ShareTransformAmount::ShareReduction { qty: 4 }
    );
    assert_eq!(
        result.dividend_tax_books_before_new_unit_market[0].lots()[0].acquired_on,
        date("2020-01-02")
    );
    assert_eq!(
        result.dividend_tax_books_before_new_unit_market[0].lots()[0].qty,
        2
    );
    assert_eq!(
        shares(
            &result.registry_before_new_unit_market,
            HolderId::Account(AccountId(1))
        ),
        2
    );
}

#[test]
fn registered_zero_outcome_does_not_claim_actual_account_or_cost_is_zero() {
    let fixture = Fixture::new(ShareSplitDirection::Consolidate, 3, 1, 0).with_external(5);
    let result = fixture.run().unwrap();
    assert_eq!(result.account_intents[0].converted_after, 0);
    assert_eq!(
        result.account_intents[0].amount,
        ShareTransformAmount::ShareReduction { qty: 1 }
    );
    assert_eq!(
        result.account_intents[0].cost_policy,
        ShareTransformCostPolicy::PreserveUnlessActualPositionCloses
    );
    assert!(result.dividend_tax_books_before_new_unit_market[0]
        .lots()
        .is_empty());
}

#[test]
fn treasury_buckets_keep_initial_inventory_separate_from_actual_account() {
    let mut fixture = Fixture::new(ShareSplitDirection::Consolidate, 2, 6, 8);
    fixture.buckets = vec![
        TreasuryBucketTransform {
            key: "initial".into(),
            original_lot_ids: vec!["initial".into()],
            shares_before: 4,
            shares_after: 2,
            account: None,
        },
        TreasuryBucketTransform {
            key: "plan:buyback".into(),
            original_lot_ids: vec!["plan".into()],
            shares_before: 4,
            shares_after: 2,
            account: Some(AccountId(99)),
        },
    ];
    let result = fixture.run().unwrap();
    let treasury: Vec<_> = result
        .account_intents
        .iter()
        .filter(|i| i.account == AccountId(99))
        .collect();
    assert_eq!(treasury.len(), 1);
    assert_eq!(
        (treasury[0].registered_before, treasury[0].converted_after),
        (4, 2)
    );
    assert_eq!(
        shares(
            &result.registry_before_new_unit_market,
            HolderId::IssuerTreasury
        ),
        4
    );
    assert_eq!(result.treasury_adjustments, fixture.buckets);
}

#[test]
fn invalid_treasury_partition_fails_without_modifying_registry_or_tax() {
    let mut fixture = Fixture::new(ShareSplitDirection::Consolidate, 2, 6, 8);
    fixture.buckets = vec![TreasuryBucketTransform {
        key: "bad".into(),
        original_lot_ids: vec!["plan".into()],
        shares_before: 8,
        shares_after: 4,
        account: Some(AccountId(99)),
    }];
    let registry = fixture.registry.clone();
    let tax = fixture.tax.clone();
    let error = fixture.run().unwrap_err();
    assert_eq!(error.event_id, "split-1");
    assert_eq!(error.day, date("2030-06-04"));
    assert_eq!(fixture.registry, registry);
    assert_eq!(fixture.tax, tax);
}

#[test]
fn missing_duplicate_or_wrong_unit_chain_is_rejected() {
    let mut fixture = Fixture::new(ShareSplitDirection::Split, 2, 6, 0);
    fixture.chain.clear();
    assert!(fixture.run().is_err());
    fixture.chain.push(SplitUnitContext {
        event_id: "split-1".into(),
        ex_on: date("2030-06-04"),
        direction: ShareSplitDirection::Split,
        ratio: 3,
    });
    assert!(fixture.run().is_err());
    fixture.chain[0].ratio = 2;
    fixture.chain.push(fixture.chain[0].clone());
    assert!(fixture.run().is_err());
}

#[test]
fn new_unit_market_activity_cannot_be_passed_as_registered_unit_registry() {
    let mut fixture = Fixture::new(ShareSplitDirection::Consolidate, 3, 6, 0);
    // 用可行外部持有人库存构造完整真实市场日结，避免凭不守恒输入证明拒绝。
    let mut registry = ShareRegistry::new(
        fixture.registry.stock().clone(),
        CompanyId(0),
        9,
        date("2030-06-03"),
        vec![
            ShareHolding {
                holder: HolderId::Account(AccountId(1)),
                lots: vec![lot("a1", 6)],
            },
            ShareHolding {
                holder: HolderId::External("seller".into()),
                lots: vec![lot("e1", 3)],
            },
        ],
    )
    .unwrap();
    let mut book = ShareSplitBook::new(fixture.book.plan().clone()).unwrap();
    book.announce(date("2030-06-02")).unwrap();
    book.register(
        registry
            .register("split-1".into(), date("2030-06-03"))
            .unwrap()
            .clone(),
        &crate::calendar::TradingCalendar::current_default_calendar().unwrap(),
    )
    .unwrap();
    registry
        .close_day(ShareDayRequest {
            event_id: "market-ex".into(),
            day: date("2030-06-04"),
            scope: MovementScope::PublicMarket,
            changes: vec![
                DayNetChange {
                    holder: HolderId::Account(AccountId(1)),
                    change: 3,
                    acquisition: Some(NetAcquisition {
                        lot_id: "new-unit".into(),
                        source: AcquisitionSource::SecondaryMarket {
                            settlement: "market-ex".into(),
                        },
                        restriction: ShareRestriction::Unrestricted,
                    }),
                    consume: None,
                },
                DayNetChange {
                    holder: HolderId::External("seller".into()),
                    change: -3,
                    acquisition: None,
                    consume: None,
                },
            ],
        })
        .unwrap();
    fixture.book = book;
    fixture.registry = registry;
    assert!(fixture.run().is_err());
}

#[test]
fn current_registry_api_blocks_public_market_after_same_day_transform() {
    let fixture = Fixture::new(ShareSplitDirection::Split, 2, 6, 0);
    let mut result = fixture.run().unwrap();
    let error = result
        .registry_before_new_unit_market
        .close_day(ShareDayRequest {
            event_id: "market-ex".into(),
            day: date("2030-06-04"),
            scope: MovementScope::PublicMarket,
            changes: vec![],
        })
        .unwrap_err();
    assert!(error.to_string().contains("previous natural day"));
}

#[test]
fn frozen_total_zero_is_domain_valid_but_registry_rejects_and_candidate_is_unchanged() {
    let fixture = Fixture::new(ShareSplitDirection::Consolidate, 3, 1, 0);
    fixture.book.validate().unwrap();
    assert_eq!(fixture.book.receipt().unwrap().issued_shares_after, 0);
    let before = fixture.registry.clone();
    let error = fixture.run().unwrap_err();
    assert!(error.detail.contains("issued share conservation mismatch"));
    assert_eq!(fixture.registry, before);
}

#[test]
fn sequential_unit_events_keep_first_registration_and_settlement_history() {
    let fixture = Fixture::new(ShareSplitDirection::Split, 2, 6, 0);
    let first = fixture.run().unwrap();
    let mut registry = first.registry_before_new_unit_market;
    let mut tax = first.dividend_tax_books_before_new_unit_market;
    let calendar = crate::calendar::TradingCalendar::current_default_calendar().unwrap();
    let registered_on = calendar
        .next_trading_day(crate::calendar::CalendarExchange::Sse, date("2030-06-04"))
        .unwrap();
    let ex_on = calendar
        .next_trading_day(crate::calendar::CalendarExchange::Sse, registered_on)
        .unwrap();
    let mut day = registry.settled_on().next().unwrap();
    while day <= registered_on {
        registry
            .close_day(ShareDayRequest {
                event_id: format!("market-between:{day}"),
                day,
                scope: MovementScope::PublicMarket,
                changes: vec![],
            })
            .unwrap();
        tax[0]
            .record_net_day(format!("tax-between:{day}"), day, 0, None)
            .unwrap();
        day = day.next().unwrap();
    }
    let mut plan = fixture.book.plan().clone();
    plan.event_id = "split-2".into();
    plan.approved_on = date("2030-06-04");
    plan.announced_on = date("2030-06-04");
    plan.registered_on = registered_on;
    plan.ex_rights_on = ex_on;
    plan.direction = ShareSplitDirection::Consolidate;
    plan.ratio = 3;
    let mut book = ShareSplitBook::new(plan.clone()).unwrap();
    book.announce(plan.announced_on).unwrap();
    book.register(
        registry
            .register(plan.event_id.clone(), plan.registered_on)
            .unwrap()
            .clone(),
        &crate::calendar::TradingCalendar::current_default_calendar().unwrap(),
    )
    .unwrap();
    while day < ex_on {
        registry
            .close_day(ShareDayRequest {
                event_id: format!("market-before-ex:{day}"),
                day,
                scope: MovementScope::PublicMarket,
                changes: vec![],
            })
            .unwrap();
        tax[0]
            .record_net_day(format!("tax-before-ex:{day}"), day, 0, None)
            .unwrap();
        day = day.next().unwrap();
    }
    // 显式空市场日界只推进自然日，不包含任何 ex 日新单位成交；R4 仍需另解真实成交相位。
    registry
        .close_day(ShareDayRequest {
            event_id: "ex-empty-baseline".into(),
            day: ex_on,
            scope: MovementScope::PublicMarket,
            changes: vec![],
        })
        .unwrap();
    tax[0]
        .record_net_day("ex-empty-tax-baseline".into(), ex_on, 0, None)
        .unwrap();
    let chain = vec![
        fixture.chain[0].clone(),
        SplitUnitContext {
            event_id: plan.event_id.clone(),
            ex_on: plan.ex_rights_on,
            direction: plan.direction,
            ratio: plan.ratio,
        },
    ];
    let result = share_transform_day_end_candidate(ShareTransformDayEndInput {
        day: ex_on,
        book: &book,
        registry_in_registered_units: &registry,
        dividend_tax_books: &tax,
        unit_chain: &chain,
        treasury_buckets: &[],
    })
    .unwrap();
    assert_eq!(
        shares(
            &result.registry_before_new_unit_market,
            HolderId::Account(AccountId(1))
        ),
        4
    );
    assert_eq!(
        result.account_intents[0].amount,
        ShareTransformAmount::ShareReduction { qty: 8 }
    );
    assert_eq!(
        result
            .registry_before_new_unit_market
            .registration("split-1"),
        registry.registration("split-1")
    );
    assert_eq!(
        result.registry_before_new_unit_market.receipts().len(),
        registry.receipts().len() + 1
    );
    assert_eq!(
        result.dividend_tax_books_before_new_unit_market[0].lots()[0].acquired_on,
        date("2030-06-04")
    );
}

#[test]
fn duplicate_treasury_lot_and_wrong_tax_quantity_are_rejected() {
    let mut fixture = Fixture::new(ShareSplitDirection::Consolidate, 2, 6, 8);
    fixture.buckets = vec![TreasuryBucketTransform {
        key: "bad".into(),
        original_lot_ids: vec!["initial".into(), "initial".into()],
        shares_before: 8,
        shares_after: 4,
        account: None,
    }];
    assert!(fixture.run().unwrap_err().detail.contains("重复归属"));
    let mut fixture = Fixture::new(ShareSplitDirection::Consolidate, 2, 6, 0);
    fixture.tax[0]
        .record_net_day("tax-mixed".into(), date("2030-06-04"), -1, None)
        .unwrap();
    assert!(fixture.run().unwrap_err().detail.contains("税账数量"));
}

#[test]
fn treasury_exact_ratio_cannot_be_reallocated_arbitrarily_between_buckets() {
    let mut fixture = Fixture::new(ShareSplitDirection::Consolidate, 2, 6, 8);
    fixture.buckets = vec![
        TreasuryBucketTransform {
            key: "initial".into(),
            original_lot_ids: vec!["initial".into()],
            shares_before: 4,
            shares_after: 3,
            account: None,
        },
        TreasuryBucketTransform {
            key: "plan:buyback".into(),
            original_lot_ids: vec!["plan".into()],
            shares_before: 4,
            shares_after: 1,
            account: Some(AccountId(99)),
        },
    ];
    assert!(
        fixture.run().is_err(),
        "每桶都可整除时不能用汇总守恒掩盖跨桶库存改写"
    );
}
