//! 工商职责迁移的行为保护：固定开局首错、计息落分与税务提交边界。

use super::*;
use crate::accounting::{
    BusinessEventId, FixedAssetCode, IncomeTaxPolicy, InventoryItemCode, VatPolicy,
};
use crate::calendar::CivilDate;
use crate::company::{CounterpartyKind, CreditLine, ExternalCounterparty};

fn date(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("合法 Fixture 日期")
}

fn amount(cents: i128) -> AccountingAmount {
    AccountingAmount::from_cents(cents)
}

#[test]
fn standalone_current_tax_assets_keep_deferred_assets_and_each_comparison_direction_separate() {
    use crate::accounting::BusinessEventId;
    use crate::accounting::reports::{
        generate_report_set, BsLine, Comparative, IndustryPresentation, ReportKind, ReportRequest,
        ReportSource, ReportVersion, VersionKind,
    };
    for opening_tax in [100_i128, -100] {
        let mut books = crate::accounting::Books::new(industrial_account_chart());
        books
            .post_batch(vec![JournalEntry {
                source: BusinessEventId::new(1),
                date: date("2029-12-31"),
                kind: BusinessKind::OpeningBalance,
                cash_flow: CashFlowClass::Financing,
                lines: vec![
                    line(
                        "1002",
                        PostingSide::Debit,
                        amount(10_000 - opening_tax - 50),
                    ),
                    line("1811", PostingSide::Debit, amount(50)),
                    line(
                        "222104",
                        if opening_tax > 0 {
                            PostingSide::Debit
                        } else {
                            PostingSide::Credit
                        },
                        amount(opening_tax.abs()),
                    ),
                    line("4001", PostingSide::Credit, amount(10_000)),
                ],
            }])
            .unwrap();
        let adjustment: i128 = if opening_tax > 0 { 300 } else { -250 };
        books
            .post_batch(vec![JournalEntry {
                source: BusinessEventId::new(2),
                date: date("2030-06-30"),
                kind: BusinessKind::TaxAccrual,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    line(
                        "6801",
                        if adjustment > 0 {
                            PostingSide::Debit
                        } else {
                            PostingSide::Credit
                        },
                        amount(adjustment.abs()),
                    ),
                    line(
                        "222104",
                        if adjustment > 0 {
                            PostingSide::Credit
                        } else {
                            PostingSide::Debit
                        },
                        amount(adjustment.abs()),
                    ),
                    line("6801", PostingSide::Debit, amount(40)),
                    line("222101", PostingSide::Credit, amount(40)),
                ],
            }])
            .unwrap();
        let cash = books.ledger().cash_total().unwrap();
        let presentation =
            crate::accounting::reports::industrial::industrial_presentation_lines(books.ledger())
                .unwrap();
        let current_asset = if opening_tax > 0 { 0 } else { 150 };
        let current_liability = if opening_tax > 0 { 240 } else { 40 };
        assert_eq!(presentation.current_tax_assets, amount(current_asset));
        assert_eq!(presentation.deferred_tax_assets, amount(50));
        assert_eq!(presentation.taxes_payable_net, amount(current_liability));
        let report = generate_report_set(ReportRequest {
            period: crate::accounting::AccountingPeriod::from_ymd(2030, 12).unwrap(),
            kind: ReportKind::Annual,
            source: ReportSource::Standalone {
                id: crate::accounting::consolidation::MemberId("TAX-PRESENTATION".into()),
                books: &books,
                industry: IndustryPresentation::Industrial,
            },
            version: ReportVersion {
                sequence: 1,
                supersedes: None,
                kind: VersionKind::Original,
            },
            adjustments: &std::collections::BTreeMap::new(),
        })
        .unwrap();
        let line_value = |lines: &[(BsLine, AccountingAmount)], target| {
            lines
                .iter()
                .find(|(line, _)| *line == target)
                .map(|(_, value)| *value)
                .unwrap_or(AccountingAmount::ZERO)
        };
        assert_eq!(
            line_value(&report.balance_sheet.asset_lines, BsLine::CurrentTaxAssets),
            amount(current_asset)
        );
        assert_eq!(
            line_value(&report.balance_sheet.asset_lines, BsLine::DeferredTaxAssets),
            amount(50)
        );
        assert_eq!(
            line_value(&report.balance_sheet.liability_lines, BsLine::TaxesPayable),
            amount(current_liability)
        );
        let Comparative::Available(prior) = &report.balance_sheet.prior_year_end else {
            panic!("真实开局余额必须提供上年末比较项")
        };
        assert_eq!(
            line_value(prior, BsLine::CurrentTaxAssets),
            amount(opening_tax.max(0))
        );
        assert_eq!(line_value(prior, BsLine::DeferredTaxAssets), amount(50));
        assert_eq!(
            line_value(prior, BsLine::TaxesPayable),
            amount((-opening_tax).max(0))
        );
        report.validate().unwrap();
        assert_eq!(books.ledger().cash_total().unwrap(), cash);
    }
}

#[test]
fn audit_credit_arithmetic_error_is_not_reported_as_missing_credit() {
    let mut books = IndustrialBooks::new(config()).unwrap();
    let start = date("2030-01-01");
    for (id, principal) in [("A", AccountingAmount::MAX), ("B", amount(1))] {
        let id = ContractId(id.into());
        books
            .contracts_mut()
            .register(crate::company::OperatingContract {
                id: id.clone(),
                role: crate::company::ContractRole::Borrowing,
                counterparty: CounterpartyId("LENDER".into()),
                principal,
                annual_rate_bp: 0,
                start_date: start,
                maturity_date: date("2030-02-01"),
                basis: crate::company::DayCountBasis::Act365F,
            })
            .unwrap();
        books.loans.insert_registered_loan(id, principal, start);
    }
    let before = serde_json::to_value(&books).unwrap();
    assert!(matches!(
        books.available_credit(&CounterpartyId("LENDER".into())),
        Err(IndustrialError::Accounting(
            crate::accounting::AccountingError::AmountOverflow { .. }
        ))
    ));
    assert_eq!(serde_json::to_value(&books).unwrap(), before);
}

fn config() -> IndustrialConfig {
    let lender = CounterpartyId("LENDER".into());
    IndustrialConfig {
        chart: industrial_account_chart(),
        as_of: date("2029-12-31"),
        opening_lines: vec![
            line(chart::acct::BANK, PostingSide::Debit, amount(10_000)),
            line("4001", PostingSide::Credit, amount(10_000)),
        ],
        opening_inventory: vec![],
        opening_assets: vec![],
        opening_debt: None,
        counterparties: vec![ExternalCounterparty {
            id: lender.clone(),
            kind: CounterpartyKind::Lender,
            name: "虚构贷款人".into(),
        }],
        budget: OperatingBudget::new(
            AccountingAmount::ZERO,
            vec![CreditLine {
                lender,
                limit: amount(10_000),
            }],
        )
        .expect("合法 Fixture 授信"),
        // Fixture 合成税率；不声称真实税法参数。
        tax_policy: TaxPolicy {
            version: 1,
            vat: VatPolicy {
                output_rate_bp: 1300,
                input_rate_bp: 1300,
                deductible_share_bp: 10_000,
            },
            income_tax: IncomeTaxPolicy {
                rate_bp: 2500,
                loss_carryforward_years: 5,
            },
        },
    }
}

#[test]
fn opening_first_error_keeps_seed_counterparty_debt_order() {
    let mut cfg = config();
    cfg.opening_inventory.push(OpeningInventoryItem {
        account: LedgerAccountId("1405".into()),
        item: InventoryItemCode("GOODS".into()),
        quantity: 1,
        cost: amount(1),
    });
    cfg.opening_assets.push(OpeningAssetItem {
        code: FixedAssetCode("ASSET".into()),
        cost: amount(2),
        salvage_value: amount(0),
        life_months: 12,
    });
    cfg.counterparties.push(cfg.counterparties[0].clone());
    cfg.opening_debt = Some(OpeningDebtTerms {
        lender: CounterpartyId("MISSING".into()),
        principal: amount(3),
        annual_rate_bp: 1,
        maturity_date: date("2031-01-01"),
    });
    let mut bad_policy = cfg.clone();
    bad_policy.tax_policy.version = 0;
    bad_policy.opening_lines[0].amount = amount(10_001);
    assert!(matches!(
        IndustrialBooks::new(bad_policy),
        Err(IndustrialError::Tax(_))
    ));
    let mut bad_post = cfg.clone();
    bad_post.opening_lines[0].amount = amount(10_001);
    assert!(matches!(
        IndustrialBooks::new(bad_post),
        Err(IndustrialError::Accounting(_))
    ));
    assert!(matches!(IndustrialBooks::new(cfg.clone()),
        Err(IndustrialError::OpeningSeedMismatch { account, .. }) if account.0 == "1405"));
    cfg.opening_inventory.clear();
    assert!(matches!(IndustrialBooks::new(cfg.clone()),
        Err(IndustrialError::OpeningSeedMismatch { account, .. }) if account.0 == "1601"));
    cfg.opening_assets.clear();
    assert!(matches!(
        IndustrialBooks::new(cfg.clone()),
        Err(IndustrialError::Company(
            crate::company::CompanyError::DuplicateCounterparty { .. }
        ))
    ));
    cfg.counterparties.pop();
    assert!(matches!(
        IndustrialBooks::new(cfg),
        Err(IndustrialError::Company(
            crate::company::CompanyError::UnknownCounterparty { .. }
        ))
    ));
}

#[test]
fn positive_days_zero_interest_keeps_carry_date_and_event_gap() {
    let mut books = IndustrialBooks::new(config()).expect("开局");
    let loan = ContractId("TINY".into());
    books
        .borrow(
            loan.clone(),
            &CounterpartyId("LENDER".into()),
            amount(1),
            1,
            date("2030-01-01"),
            date("2030-12-31"),
        )
        .expect("借款");
    let items = books.accrue_interest(date("2030-01-02")).expect("计息");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].amount, amount(0));
    assert_eq!(books.loan(&loan).expect("贷款").carried().units(), 1);
    assert_eq!(
        books.loan(&loan).expect("贷款").last_accrual_date(),
        date("2030-01-02")
    );
    let outcome = books
        .repay_principal(&loan, amount(1), date("2030-01-03"))
        .expect("还本");
    assert_eq!(
        outcome.events,
        vec![crate::accounting::BusinessEventId::new(4)]
    );
    let state = books.loan(&loan).expect("贷款");
    assert_eq!(state.outstanding(), amount(0));
    assert_eq!(state.carried().units(), 2);
    assert_eq!(state.last_accrual_date(), date("2030-01-03"));
}

#[test]
fn rejected_repayment_keeps_preview_uncommitted_and_serialized_shape() {
    let mut books = IndustrialBooks::new(config()).expect("开局");
    let loan = ContractId("TINY".into());
    books
        .borrow(
            loan.clone(),
            &CounterpartyId("LENDER".into()),
            amount(1),
            1,
            date("2030-01-01"),
            date("2030-12-31"),
        )
        .expect("借款");
    books
        .pay_expense(ExpenseKind::Admin, amount(10_001), date("2030-01-01"))
        .expect("耗尽现金");
    let before = serde_json::to_value(&books).expect("序列化");
    assert!(before["income_tax_position"]["loss_pool"].is_array());
    assert_eq!(
        before["loans"]["TINY"]["outstanding"],
        serde_json::to_value(amount(1)).expect("金额")
    );
    assert!(matches!(
        books.repay_principal(&loan, amount(1), date("2030-01-02")),
        Err(IndustrialError::PaymentFailed { .. })
    ));
    assert_eq!(serde_json::to_value(&books).expect("序列化"), before);
    let restored: IndustrialBooks = serde_json::from_value(before.clone()).expect("恢复");
    assert_eq!(serde_json::to_value(restored).expect("序列化"), before);
}

#[test]
fn repeated_annual_tax_is_idempotent_and_preserves_pretax_loss() {
    let mut books = IndustrialBooks::new(config()).expect("开局");
    books
        .pay_expense(ExpenseKind::Admin, amount(400), date("2030-01-01"))
        .expect("费用");
    let first = books.accrue_income_tax(date("2030-12-31")).expect("首计提");
    assert_eq!(first.pretax, amount(-400));
    assert_eq!(first.deferred_delta, amount(100));
    let before = serde_json::to_value(&books).expect("保存首计提");
    let second = books
        .accrue_income_tax(date("2030-12-31"))
        .expect("重复计提");
    assert_eq!(second.pretax, amount(-400));
    assert_eq!(second.event, None);
    assert_eq!(second.deferred_delta, amount(0));
    assert_eq!(books.loss_pool().len(), 1);
    assert_eq!(books.loss_pool()[0].remaining, amount(400));
    assert_eq!(serde_json::to_value(&books).expect("重复计提"), before);
}

#[test]
fn tax_reassessment_posts_only_delta_and_does_not_reuse_losses() {
    let mut books = IndustrialBooks::new(config()).expect("开局");
    books
        .pay_expense(ExpenseKind::Admin, amount(400), date("2030-01-01"))
        .unwrap();
    books.accrue_income_tax(date("2030-12-31")).unwrap();
    for (source, revenue, when) in [(9000, 800, "2031-03-31"), (9001, 400, "2031-06-30")] {
        books
            .books_mut()
            .post_batch(vec![JournalEntry {
                source: crate::accounting::BusinessEventId::new(source),
                date: date(when),
                kind: BusinessKind::CashRevenue,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    line(chart::acct::BANK, PostingSide::Debit, amount(revenue)),
                    line("6001", PostingSide::Credit, amount(revenue)),
                ],
            }])
            .unwrap();
        let outcome = books.accrue_income_tax(date(when)).unwrap();
        assert_eq!(outcome.loss_offset_used, amount(400));
        assert_eq!(
            outcome.pretax,
            amount(if source == 9000 { 800 } else { 1200 })
        );
        assert_eq!(
            outcome.current_tax,
            amount(if source == 9000 { 100 } else { 200 })
        );
        assert_eq!(outcome.current_tax_delta, amount(100));
        assert_eq!(
            books.net_of(chart::acct::CIT_PAYABLE).unwrap(),
            amount(if source == 9000 { -100 } else { -200 })
        );
        assert!(books.loss_pool().is_empty());
        let before = books.clone();
        assert_eq!(books.accrue_income_tax(date(when)).unwrap().event, None);
        assert_eq!(books, before);
    }
}

#[test]
fn tax_reassessment_after_payment_can_reverse_tax_without_refunding_cash() {
    let mut books = IndustrialBooks::new(config()).unwrap();
    books
        .books_mut()
        .post_batch(vec![JournalEntry {
            source: crate::accounting::BusinessEventId::new(9000),
            date: date("2030-03-31"),
            kind: BusinessKind::CashRevenue,
            cash_flow: CashFlowClass::Operating,
            lines: vec![
                line(chart::acct::BANK, PostingSide::Debit, amount(800)),
                line("6001", PostingSide::Credit, amount(800)),
            ],
        }])
        .unwrap();
    books.accrue_income_tax(date("2030-03-31")).unwrap();
    books
        .pay_income_tax(amount(200), date("2030-04-01"))
        .unwrap();
    books
        .pay_expense(ExpenseKind::Admin, amount(400), date("2030-06-01"))
        .unwrap();
    let cash = books.net_of(chart::acct::BANK).unwrap();
    let outcome = books.accrue_income_tax(date("2030-06-30")).unwrap();
    assert_eq!(outcome.pretax, amount(400));
    assert_eq!(outcome.current_tax, amount(100));
    assert_eq!(outcome.current_tax_delta, amount(-100));
    assert_eq!(books.net_of(chart::acct::CIT_PAYABLE).unwrap(), amount(100));
    assert_eq!(books.net_of(chart::acct::BANK).unwrap(), cash);
    let report = crate::accounting::reports::generate_report_set(
        crate::accounting::reports::ReportRequest {
            period: crate::accounting::AccountingPeriod::from_ymd(2030, 6).unwrap(),
            kind: crate::accounting::reports::ReportKind::Monthly,
            source: crate::accounting::reports::ReportSource::Standalone {
                id: crate::accounting::consolidation::MemberId("TAX".into()),
                books: books.books(),
                industry: crate::accounting::reports::IndustryPresentation::Industrial,
            },
            version: crate::accounting::reports::ReportVersion {
                sequence: 1,
                supersedes: None,
                kind: crate::accounting::reports::VersionKind::Original,
            },
            adjustments: books.income_tax_restatements(),
        },
    )
    .unwrap();
    assert!(report
        .balance_sheet
        .asset_lines
        .iter()
        .any(|(line, value)| format!("{line:?}") == "CurrentTaxAssets" && *value == amount(100)));
    assert!(report
        .balance_sheet
        .liability_lines
        .iter()
        .all(
            |(line, value)| *line != crate::accounting::reports::BsLine::TaxesPayable
                || !value.is_negative()
        ));
    report.validate().unwrap();
    let restored: IndustrialBooks =
        serde_json::from_value(serde_json::to_value(&books).unwrap()).unwrap();
    books = restored;
    let before = books.clone();
    assert_eq!(
        books.accrue_income_tax(date("2030-06-30")).unwrap().event,
        None
    );
    assert_eq!(books, before);
}

#[test]
fn failed_tax_reassessment_keeps_annual_baseline_and_can_retry() {
    let mut books = IndustrialBooks::new(config()).unwrap();
    books
        .pay_expense(ExpenseKind::Admin, amount(400), date("2030-01-01"))
        .unwrap();
    books.accrue_income_tax(date("2030-03-31")).unwrap();
    books
        .pay_expense(ExpenseKind::Admin, amount(400), date("2030-06-01"))
        .unwrap();
    books
        .books_mut()
        .close_period(crate::accounting::AccountingPeriod::from_ymd(2030, 6).unwrap())
        .unwrap();
    let before = books.clone();
    assert!(matches!(
        books.accrue_income_tax(date("2030-06-30")),
        Err(IndustrialError::Accounting(_))
    ));
    assert_eq!(books, before);
    let outcome = books.accrue_income_tax(date("2030-07-01")).unwrap();
    assert_eq!(outcome.pretax, amount(-800));
    assert_eq!(outcome.deferred_delta, amount(100));
    assert_eq!(
        books.loss_pool(),
        &[LossEntry {
            origin_year: 2030,
            remaining: amount(800)
        }]
    );
}

#[test]
fn first_tax_assessment_rejects_current_or_future_opening_losses_atomically() {
    let mut books = IndustrialBooks::new(config()).unwrap();
    books
        .pay_expense(ExpenseKind::Admin, amount(400), date("2030-01-01"))
        .unwrap();
    for origin_year in [2030, 2031] {
        let mut saved = serde_json::to_value(&books).unwrap();
        saved["income_tax_position"]["loss_pool"] =
            serde_json::json!([{ "origin_year": origin_year, "remaining": "1.00" }]);
        let mut restored: IndustrialBooks = serde_json::from_value(saved).unwrap();
        let before = restored.clone();
        assert!(matches!(
            restored.accrue_income_tax(date("2030-12-31")),
            Err(IndustrialError::IncomeTaxStateInconsistent { .. })
        ));
        assert_eq!(restored, before);
    }
}

#[test]
fn tax_state_restore_requires_annual_baseline_and_rejects_inconsistent_pool() {
    let mut books = IndustrialBooks::new(config()).unwrap();
    books
        .pay_expense(ExpenseKind::Admin, amount(400), date("2030-01-01"))
        .unwrap();
    books.accrue_income_tax(date("2030-12-31")).unwrap();
    let valid = serde_json::to_value(&books).unwrap();
    let mut hidden_chain = valid.clone();
    hidden_chain["income_tax_position"]["assessments"] = serde_json::json!({});
    hidden_chain["income_tax_position"]["loss_pool"] = serde_json::json!([]);
    hidden_chain["income_tax_position"]["initial_deferred_tax_asset"] = serde_json::json!("1.00");
    assert!(serde_json::from_value::<IndustrialBooks>(hidden_chain)
        .unwrap_err()
        .to_string()
        .contains("空年度评估链"));
    for invalid in [
        {
            let mut invalid = valid.clone();
            invalid["income_tax_position"]
                .as_object_mut()
                .unwrap()
                .remove("assessments");
            invalid
        },
        {
            let mut invalid = valid.clone();
            invalid["income_tax_position"]["assessments"]["2030"]["current_tax"] =
                serde_json::json!("1.00");
            invalid
        },
        {
            let mut invalid = valid.clone();
            invalid["income_tax_position"]["loss_pool"][0]["remaining"] = serde_json::json!("3.00");
            invalid
        },
        {
            let mut invalid = valid.clone();
            invalid
                .as_object_mut()
                .unwrap()
                .remove("income_tax_position");
            invalid["loss_pool"] = serde_json::json!([]);
            invalid
        },
    ] {
        assert!(serde_json::from_value::<IndustrialBooks>(invalid).is_err());
    }
    let mut restored: IndustrialBooks = serde_json::from_value(valid).unwrap();
    let before = restored.clone();
    assert_eq!(
        restored
            .accrue_income_tax(date("2030-12-31"))
            .unwrap()
            .event,
        None
    );
    assert_eq!(restored, before);
    restored.accrue_income_tax(date("2031-12-31")).unwrap();
    let before = restored.clone();
    assert_eq!(
        restored
            .accrue_income_tax(date("2030-12-31"))
            .unwrap()
            .event,
        None
    );
    assert_eq!(restored, before);
}

#[test]
fn zero_line_tax_still_commits_expired_loss_pool() {
    let mut books = IndustrialBooks::new(config()).expect("开局");
    let mut value = serde_json::to_value(&books).expect("序列化");
    value["income_tax_position"]["loss_pool"] =
        serde_json::json!([{"origin_year": 2020, "remaining": "0.04"}]);
    books = serde_json::from_value(value).expect("恢复明确配置的亏损池");
    let outcome = books.accrue_income_tax(date("2030-12-31")).expect("计提");
    assert_eq!(outcome.event, None);
    assert_eq!(outcome.losses_expired, amount(4));
    assert!(books.loss_pool().is_empty());
}

#[test]
fn failed_tax_post_keeps_loss_pool_and_event_id_unchanged() {
    let mut books = IndustrialBooks::new(config()).expect("开局");
    books
        .pay_expense(ExpenseKind::Admin, amount(400), date("2030-01-01"))
        .expect("费用");
    books
        .books_mut()
        .close_period(crate::accounting::AccountingPeriod::from_ymd(2030, 12).expect("期间"))
        .expect("封期");
    let before = serde_json::to_value(&books).expect("序列化");
    assert!(matches!(
        books.accrue_income_tax(date("2030-12-31")),
        Err(IndustrialError::Accounting(_))
    ));
    assert_eq!(serde_json::to_value(&books).expect("序列化"), before);
}

#[test]
fn historical_tax_cascade_recomputes_offsets_without_refunding_or_double_counting() {
    let mut books = IndustrialBooks::new(config()).unwrap();
    books
        .pay_expense(ExpenseKind::Admin, amount(400), date("2030-01-01"))
        .unwrap();
    books.accrue_income_tax(date("2030-12-31")).unwrap();
    for (source, year) in [(9000, 2031), (9001, 2032)] {
        books
            .books_mut()
            .post_batch(vec![JournalEntry {
                source: BusinessEventId::new(source),
                date: CivilDate::from_ymd(year, 1, 1).unwrap(),
                kind: BusinessKind::CashRevenue,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    line(chart::acct::BANK, PostingSide::Debit, amount(800)),
                    line(chart::acct::REVENUE, PostingSide::Credit, amount(800)),
                ],
            }])
            .unwrap();
        books
            .accrue_income_tax(CivilDate::from_ymd(year, 12, 31).unwrap())
            .unwrap();
    }
    books
        .pay_income_tax(amount(300), date("2033-01-01"))
        .unwrap();
    books
        .books_mut()
        .post_batch(vec![JournalEntry {
            source: BusinessEventId::new(9002),
            date: date("2033-01-02"),
            kind: BusinessKind::InterestAccrual,
            cash_flow: CashFlowClass::NonCash,
            lines: vec![
                line(chart::acct::FIN_EXP, PostingSide::Debit, amount(400)),
                line(chart::acct::INT_PAYABLE, PostingSide::Credit, amount(400)),
            ],
        }])
        .unwrap();
    let cash = books.net_of(chart::acct::BANK).unwrap();
    let adjustments = std::collections::BTreeMap::from([(
        BusinessEventId::new(9002),
        crate::accounting::AccountingPeriod::from_ymd(2030, 1).unwrap(),
    )]);
    let outcome = books
        .reassess_income_tax(2030, date("2033-01-31"), &adjustments)
        .unwrap();
    assert_eq!(outcome.pretax, amount(-800));
    assert_eq!(outcome.current_tax_delta, amount(-100));
    assert_eq!(outcome.deferred_delta, amount(0));
    assert_eq!(books.net_of(chart::acct::CIT_PAYABLE).unwrap(), amount(100));
    assert_eq!(books.net_of(chart::acct::BANK).unwrap(), cash);
    assert_eq!(books.net_of(chart::acct::DTA).unwrap(), amount(0));
    assert!(books.loss_pool().is_empty());
    let encoded = serde_json::to_value(&books).unwrap();
    assert_eq!(
        encoded["income_tax_position"]["assessments"]["2031"]["current_tax"],
        "0.00"
    );
    assert_eq!(
        encoded["income_tax_position"]["assessments"]["2032"]["current_tax"],
        "2.00"
    );
    let mut restored: IndustrialBooks = serde_json::from_value(encoded).unwrap();
    let before = restored.clone();
    assert_eq!(
        restored
            .reassess_income_tax(2030, date("2033-01-31"), &adjustments)
            .unwrap()
            .event,
        None
    );
    assert_eq!(restored, before);
    let new_year = restored.accrue_income_tax(date("2033-12-31")).unwrap();
    assert_eq!(new_year.pretax, amount(0));
    assert_eq!(new_year.loss_added, amount(0));
    assert_eq!(new_year.current_tax_delta, amount(0));
}

fn tax_correction_fixture() -> (
    IndustrialBooks,
    crate::accounting::closing::ClosingEngine,
    crate::information::PublicLibrary,
    crate::accounting::consolidation::MemberId,
    crate::information::PublicationId,
) {
    use crate::accounting::reports::IndustryPresentation;
    use crate::information::*;
    let mut books = IndustrialBooks::new(config()).unwrap();
    let member = crate::accounting::consolidation::MemberId("TAX-COMPANY".into());
    let mut closing = crate::accounting::closing::ClosingEngine::new();
    books
        .pay_expense(ExpenseKind::Admin, amount(400), date("2030-01-01"))
        .unwrap();
    books.accrue_income_tax(date("2030-12-31")).unwrap();
    closing
        .close_year(
            books.books_mut(),
            &member,
            IndustryPresentation::Industrial,
            2030,
        )
        .unwrap();
    books
        .books_mut()
        .post_batch(vec![JournalEntry {
            source: BusinessEventId::new(9000),
            date: date("2031-01-01"),
            kind: BusinessKind::CashRevenue,
            cash_flow: CashFlowClass::Operating,
            lines: vec![
                line(chart::acct::BANK, PostingSide::Debit, amount(800)),
                line(chart::acct::REVENUE, PostingSide::Credit, amount(800)),
            ],
        }])
        .unwrap();
    books.accrue_income_tax(date("2031-12-31")).unwrap();
    closing
        .close_year(
            books.books_mut(),
            &member,
            IndustryPresentation::Industrial,
            2031,
        )
        .unwrap();
    books
        .pay_income_tax(amount(100), date("2032-01-01"))
        .unwrap();
    let mut library = PublicLibrary::new();
    let published_at = scheduled_instant(ScheduledReportKind::Annual, 2030, 0).unwrap();
    let original = library
        .publish_closed(
            &closing,
            PublicationRequest {
                company: crate::company::CompanyId(member.0.clone()),
                scope: crate::accounting::consolidation::ScopeId::Standalone(member.clone()),
                period: crate::accounting::AccountingPeriod::from_ymd(2030, 12).unwrap(),
                kind: crate::accounting::reports::ReportKind::Annual,
                sequence: 1,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: crate::calendar::CivilInstant::from_hms(published_at.date(), 8, 0, 0)
                    .unwrap(),
                published_at,
                origin: PublicationOrigin::SeededPrehistory {
                    fiscal_year: 2030,
                    kind: ScheduledReportKind::Annual,
                    offset_days: 0,
                },
                supersedes: None,
            },
        )
        .unwrap();
    (books, closing, library, member, original)
}

#[test]
fn report_and_tax_correction_commits_same_effective_years_atomically_and_can_retry() {
    use crate::information::*;
    let (mut books, mut closing, mut library, member, original) = tax_correction_fixture();
    let posted_on = date("2032-01-21");
    let before_books = books.clone();
    let before_closing = serde_json::to_value(&closing).unwrap();
    let before_library = library.clone();
    let make_request = |hour| {
        (
            crate::accounting::closing::CorrectionRequest {
                reason: "补记前期利息费用".into(),
                entries: vec![JournalEntry {
                    source: BusinessEventId::new(9900),
                    date: posted_on,
                    kind: BusinessKind::InterestAccrual,
                    cash_flow: CashFlowClass::NonCash,
                    lines: vec![
                        line(chart::acct::FIN_EXP, PostingSide::Debit, amount(400)),
                        line(chart::acct::INT_PAYABLE, PostingSide::Credit, amount(400)),
                    ],
                }],
            },
            PublicationRequest {
                company: crate::company::CompanyId(member.0.clone()),
                scope: crate::accounting::consolidation::ScopeId::Standalone(member.clone()),
                period: crate::accounting::AccountingPeriod::from_ymd(2030, 12).unwrap(),
                kind: crate::accounting::reports::ReportKind::Annual,
                sequence: 2,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: crate::calendar::CivilInstant::from_hms(posted_on, 8, 0, 0).unwrap(),
                published_at: crate::calendar::CivilInstant::from_hms(posted_on, hour, 0, 0)
                    .unwrap(),
                origin: PublicationOrigin::Correction,
                supersedes: Some(original),
            },
        )
    };
    let (correction, publication) = make_request(17);
    assert!(
        matches!(books.correct_and_publish_with_tax(IndustrialReportCorrection { closing: &mut closing, library: &mut library, member: &member, correction, publication, posted_on }),
        Err(IndustrialCorrectionError::Publication(crate::information::CorrectionPublicationError::Information { cause, .. }))
        if matches!(*cause, crate::information::InformationError::PublicationOutsidePhase { .. }))
    );
    assert_eq!(books, before_books);
    assert_eq!(serde_json::to_value(&closing).unwrap(), before_closing);
    assert_eq!(library, before_library);
    let (correction, publication) = make_request(18);
    books
        .correct_and_publish_with_tax(IndustrialReportCorrection {
            closing: &mut closing,
            library: &mut library,
            member: &member,
            correction,
            publication,
            posted_on,
        })
        .unwrap();
    assert_eq!(
        books.net_of(chart::acct::BANK).unwrap(),
        before_books.net_of(chart::acct::BANK).unwrap()
    );
    assert_eq!(books.net_of(chart::acct::CIT_PAYABLE).unwrap(), amount(100));
    assert_eq!(
        books.income_tax_restatements(),
        closing
            .restatement_periods(&crate::accounting::consolidation::ScopeId::Standalone(
                member.clone()
            ))
            .unwrap()
    );
    let saved = library.save();
    assert_eq!(saved.reports.len(), 2);
    assert_eq!(
        saved.reports[0].reports.income.cumulative.net_income,
        amount(-300)
    );
    assert_eq!(
        saved.reports[1].reports.income.cumulative.net_income,
        amount(-600)
    );
    assert_eq!(saved.reports[1].supersedes, Some(original));
    assert_eq!(
        serde_json::from_value::<IndustrialBooks>(serde_json::to_value(&books).unwrap()).unwrap(),
        books
    );
}

#[test]
fn tax_report_correction_rejects_future_dates_and_unstructured_subledger_changes_atomically() {
    use crate::information::*;
    let (mut books, mut closing, mut library, member, original) = tax_correction_fixture();
    books
        .borrow(
            ContractId("GUARD-LOAN".into()),
            &CounterpartyId("LENDER".into()),
            amount(100),
            365,
            date("2032-01-02"),
            date("2033-01-02"),
        )
        .unwrap();
    let posted_on = date("2032-01-21");
    let before_books = books.clone();
    let before_closing = serde_json::to_value(&closing).unwrap();
    let before_library = library.clone();
    for (account, entry_date, posting_date) in [
        ("1122", posted_on, posted_on),
        ("2202", posted_on, posted_on),
        ("1403", posted_on, posted_on),
        ("1405", posted_on, posted_on),
        ("5001", posted_on, posted_on),
        ("1601", posted_on, posted_on),
        ("1602", posted_on, posted_on),
        ("1603", posted_on, posted_on),
        ("2001", posted_on, posted_on),
        ("2501", posted_on, posted_on),
        ("1811", posted_on, posted_on),
        ("222104", posted_on, posted_on),
        ("6801", posted_on, posted_on),
        ("2231", posted_on, posted_on),
        ("6603", date("2032-01-22"), posted_on),
        ("6603", posted_on, date("2032-01-22")),
    ] {
        let result = books.correct_and_publish_with_tax(IndustrialReportCorrection {
            closing: &mut closing,
            library: &mut library,
            member: &member,
            posted_on: posting_date,
            correction: crate::accounting::closing::CorrectionRequest {
                reason: "守卫前期调整".into(),
                entries: vec![JournalEntry {
                    source: BusinessEventId::new(9900),
                    date: entry_date,
                    kind: BusinessKind::InterestAccrual,
                    cash_flow: CashFlowClass::NonCash,
                    lines: vec![
                        line(account, PostingSide::Debit, amount(400)),
                        line("4001", PostingSide::Credit, amount(400)),
                    ],
                }],
            },
            publication: PublicationRequest {
                company: crate::company::CompanyId(member.0.clone()),
                scope: crate::accounting::consolidation::ScopeId::Standalone(member.clone()),
                period: crate::accounting::AccountingPeriod::from_ymd(2030, 12).unwrap(),
                kind: crate::accounting::reports::ReportKind::Annual,
                sequence: 2,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: crate::calendar::CivilInstant::from_hms(posted_on, 8, 0, 0).unwrap(),
                published_at: crate::calendar::CivilInstant::from_hms(posted_on, 18, 0, 0).unwrap(),
                origin: PublicationOrigin::Correction,
                supersedes: Some(original),
            },
        });
        if account == "6603" {
            assert!(matches!(
                result,
                Err(IndustrialCorrectionError::Industrial {
                    cause: IndustrialError::IncomeTaxStateInconsistent { .. },
                    ..
                })
            ));
        } else {
            assert!(
                matches!(result, Err(IndustrialCorrectionError::Industrial { cause: IndustrialError::StructuredCorrectionRequired { account: rejected }, .. }) if rejected.0 == account)
            );
        }
        assert_eq!(books, before_books);
        assert_eq!(serde_json::to_value(&closing).unwrap(), before_closing);
        assert_eq!(library, before_library);
    }
}

#[test]
fn tax_correction_revises_published_quarter_and_unpublished_annual_without_rewriting_old_versions()
{
    use crate::accounting::reports::*;
    use crate::information::*;
    let (mut books, mut closing, mut library, member, original) = tax_correction_fixture();
    let scope = crate::accounting::consolidation::ScopeId::Standalone(member.clone());
    let quarter = crate::accounting::AccountingPeriod::from_ymd(2031, 3).unwrap();
    assert_eq!(
        books.books().journal().period_status(quarter),
        crate::accounting::PeriodStatus::Open
    );
    closing
        .snapshot_interim(
            books.books(),
            &member,
            IndustryPresentation::Industrial,
            quarter,
            ReportKind::Quarter,
        )
        .unwrap();
    let quarter_at = scheduled_instant(ScheduledReportKind::Q1, 2031, 0).unwrap();
    let quarter_id = library
        .publish_closed(
            &closing,
            PublicationRequest {
                company: crate::company::CompanyId(member.0.clone()),
                scope: scope.clone(),
                period: quarter,
                kind: ReportKind::Quarter,
                sequence: 1,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: crate::calendar::CivilInstant::from_hms(quarter_at.date(), 8, 0, 0)
                    .unwrap(),
                published_at: quarter_at,
                origin: PublicationOrigin::SeededPrehistory {
                    fiscal_year: 2031,
                    kind: ScheduledReportKind::Q1,
                    offset_days: 0,
                },
                supersedes: None,
            },
        )
        .unwrap();
    let annual_period = crate::accounting::AccountingPeriod::from_ymd(2031, 12).unwrap();
    let old_private = closing
        .version(&scope, annual_period, ReportKind::Annual, 1)
        .unwrap()
        .clone();
    let old_public = library.save().reports;
    let posted_on = date("2032-01-21");
    books
        .correct_and_publish_with_tax(IndustrialReportCorrection {
            closing: &mut closing,
            library: &mut library,
            member: &member,
            posted_on,
            correction: crate::accounting::closing::CorrectionRequest {
                reason: "补记前期利息".into(),
                entries: vec![JournalEntry {
                    source: BusinessEventId::new(9900),
                    date: posted_on,
                    kind: BusinessKind::InterestAccrual,
                    cash_flow: CashFlowClass::NonCash,
                    lines: vec![
                        line(chart::acct::FIN_EXP, PostingSide::Debit, amount(400)),
                        line(chart::acct::INT_PAYABLE, PostingSide::Credit, amount(400)),
                    ],
                }],
            },
            publication: PublicationRequest {
                company: crate::company::CompanyId(member.0.clone()),
                scope: scope.clone(),
                period: crate::accounting::AccountingPeriod::from_ymd(2030, 12).unwrap(),
                kind: ReportKind::Annual,
                sequence: 2,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: crate::calendar::CivilInstant::from_hms(posted_on, 8, 0, 0).unwrap(),
                published_at: crate::calendar::CivilInstant::from_hms(posted_on, 18, 0, 0).unwrap(),
                origin: PublicationOrigin::Correction,
                supersedes: Some(original),
            },
        })
        .unwrap();
    let updated_public = library.save().reports;
    assert_eq!(&updated_public[..2], old_public.as_slice());
    assert_eq!(updated_public.len(), 4);
    let revised_quarter = updated_public
        .iter()
        .find(|report| report.supersedes == Some(quarter_id))
        .unwrap();
    assert!(matches!(
        revised_quarter.origin,
        PublicationOrigin::Correction
    ));
    assert_eq!(
        revised_quarter.reports.balance_sheet.total_liabilities,
        amount(400)
    );
    assert_eq!(
        revised_quarter.reports.balance_sheet.total_equity,
        amount(10200)
    );
    assert_eq!(
        books.books().journal().period_status(quarter),
        crate::accounting::PeriodStatus::Open
    );
    assert_eq!(
        closing
            .version(&scope, annual_period, ReportKind::Annual, 1)
            .unwrap(),
        &old_private
    );
    let sequence = ensure_original_registered(
        &mut closing,
        books.books(),
        &member,
        IndustryPresentation::Industrial,
        annual_period,
        ReportKind::Annual,
    )
    .unwrap();
    assert_eq!(sequence, 2);
    assert_eq!(
        closing
            .version(&scope, annual_period, ReportKind::Annual, 1)
            .unwrap(),
        &old_private
    );
    assert_eq!(
        ensure_original_registered(
            &mut closing,
            books.books(),
            &member,
            IndustryPresentation::Industrial,
            annual_period,
            ReportKind::Annual
        )
        .unwrap(),
        sequence
    );
    let annual_at = scheduled_instant(ScheduledReportKind::Annual, 2031, 0).unwrap();
    let annual_id = library
        .publish_closed(
            &closing,
            PublicationRequest {
                company: crate::company::CompanyId(member.0.clone()),
                scope: scope.clone(),
                period: annual_period,
                kind: ReportKind::Annual,
                sequence,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: crate::calendar::CivilInstant::from_hms(annual_at.date(), 8, 0, 0)
                    .unwrap(),
                published_at: annual_at,
                origin: PublicationOrigin::ScheduledDisclosure {
                    fiscal_year: 2031,
                    kind: ScheduledReportKind::Annual,
                    offset_days: 0,
                },
                supersedes: None,
            },
        )
        .unwrap();
    let saved = library.save();
    let published = saved
        .reports
        .iter()
        .find(|report| report.id == annual_id)
        .unwrap();
    assert_eq!(published.reports.version.sequence, 2);
    assert!(matches!(
        published.origin,
        PublicationOrigin::ScheduledDisclosure { .. }
    ));
    assert!(matches!(
        published.reports.version.kind,
        VersionKind::Original
    ));
    assert_eq!(published.supersedes, None);
    assert_eq!(
        published.reports.balance_sheet.total_liabilities,
        amount(400)
    );
    assert_eq!(published.reports.balance_sheet.total_equity, amount(10000));
    assert!(
        serde_json::from_value::<PublicLibrary>(serde_json::to_value(&library).unwrap()).is_ok()
    );
}

#[test]
fn tax_correction_cannot_publish_later_annual_revision_before_its_original_and_rolls_back_everything(
) {
    use crate::information::*;
    let (mut books, mut closing, mut library, member, original) = tax_correction_fixture();
    let later_at = scheduled_instant(ScheduledReportKind::Annual, 2031, 0).unwrap();
    library
        .publish_closed(
            &closing,
            PublicationRequest {
                company: crate::company::CompanyId(member.0.clone()),
                scope: crate::accounting::consolidation::ScopeId::Standalone(member.clone()),
                period: crate::accounting::AccountingPeriod::from_ymd(2031, 12).unwrap(),
                kind: crate::accounting::reports::ReportKind::Annual,
                sequence: 1,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: crate::calendar::CivilInstant::from_hms(later_at.date(), 8, 0, 0)
                    .unwrap(),
                published_at: later_at,
                origin: PublicationOrigin::ScheduledDisclosure {
                    fiscal_year: 2031,
                    kind: ScheduledReportKind::Annual,
                    offset_days: 0,
                },
                supersedes: None,
            },
        )
        .unwrap();
    let before_books = books.clone();
    let before_closing = serde_json::to_value(&closing).unwrap();
    let before_library = library.clone();
    let posted_on = date("2032-01-21");
    let result = books.correct_and_publish_with_tax(IndustrialReportCorrection {
        closing: &mut closing,
        library: &mut library,
        member: &member,
        posted_on,
        correction: crate::accounting::closing::CorrectionRequest {
            reason: "不能提前公布未来更正版".into(),
            entries: vec![JournalEntry {
                source: BusinessEventId::new(9900),
                date: posted_on,
                kind: BusinessKind::InterestAccrual,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    line(chart::acct::FIN_EXP, PostingSide::Debit, amount(400)),
                    line(chart::acct::INT_PAYABLE, PostingSide::Credit, amount(400)),
                ],
            }],
        },
        publication: PublicationRequest {
            company: crate::company::CompanyId(member.0.clone()),
            scope: crate::accounting::consolidation::ScopeId::Standalone(member.clone()),
            period: crate::accounting::AccountingPeriod::from_ymd(2030, 12).unwrap(),
            kind: crate::accounting::reports::ReportKind::Annual,
            sequence: 2,
            policy: AccountingPolicyRef { chart_version: 2 },
            approved_at: crate::calendar::CivilInstant::from_hms(posted_on, 8, 0, 0).unwrap(),
            published_at: crate::calendar::CivilInstant::from_hms(posted_on, 18, 0, 0).unwrap(),
            origin: PublicationOrigin::Correction,
            supersedes: Some(original),
        },
    });
    assert!(
        matches!(result, Err(IndustrialCorrectionError::Industrial { cause: IndustrialError::IncomeTaxStateInconsistent { detail }, .. }) if detail.contains("后续报告"))
    );
    assert_eq!(books, before_books);
    assert_eq!(serde_json::to_value(&closing).unwrap(), before_closing);
    assert_eq!(library, before_library);
}

#[test]
fn opening_implicit_debt_roundtrip_keeps_short_account_and_credit_usage() {
    let mut cfg = config();
    cfg.opening_lines[0].amount = amount(10_001);
    cfg.opening_lines
        .push(line(chart::acct::ST_DEBT, PostingSide::Credit, amount(1)));
    cfg.opening_debt = Some(OpeningDebtTerms {
        lender: CounterpartyId("LENDER".into()),
        principal: amount(1),
        annual_rate_bp: 1,
        maturity_date: date("2032-01-01"),
    });
    let books = IndustrialBooks::new(cfg).expect("开局隐式借款");
    assert_eq!(
        books
            .available_credit(&CounterpartyId("LENDER".into()))
            .unwrap(),
        Some(amount(9_999))
    );
    let saved = serde_json::to_string(&books).expect("保存");
    let mut restored: IndustrialBooks = serde_json::from_str(&saved).expect("恢复");
    assert_eq!(serde_json::to_string(&restored).expect("再次保存"), saved);
    let loan = ContractId(OPENING_DEBT_CONTRACT_ID.into());
    let repayment = restored
        .repay_principal(&loan, amount(1), date("2030-01-01"))
        .expect("还本");
    assert_eq!(
        repayment.events,
        vec![crate::accounting::BusinessEventId::new(2)]
    );
    assert_eq!(
        restored.net_of(chart::acct::ST_DEBT).expect("短期借款"),
        amount(0)
    );
    assert_eq!(
        restored.net_of(chart::acct::LT_DEBT).expect("长期借款"),
        amount(0)
    );
    assert_eq!(restored.loan(&loan).expect("隐式贷款").carried().units(), 1);
    assert_eq!(
        restored
            .available_credit(&CounterpartyId("LENDER".into()))
            .unwrap(),
        Some(amount(10_000))
    );
}

#[test]
fn operating_tax_correction_binds_company_owner_and_rejects_group_drift_without_partial_commit() {
    use crate::company::operations::*;
    use crate::company::{CompanyId, CompanyKind, CompanySpec, IndustryId};
    use crate::information::*;
    let (mut books, mut closing, mut library, member, original) = tax_correction_fixture();
    for (id, kind) in [
        ("CUSTOMER", CounterpartyKind::Customer),
        ("SUPPLIER", CounterpartyKind::Supplier),
    ] {
        books
            .counterparties_mut()
            .register(ExternalCounterparty {
                id: CounterpartyId(id.into()),
                name: "虚构经营对手方".into(),
                kind,
            })
            .unwrap();
    }
    let company = CompanyId(member.0.clone());
    let spec = CompanySpec {
        id: company.clone(),
        name: "虚构税务测试公司".into(),
        industry: IndustryId("test-industrial".into()),
        kind: CompanyKind::Industrial,
        listed_stock: None,
        issued_shares: 100,
        group_parent: None,
    };
    let flow = FlowParams::Industrial(IndustrialFlowParams {
        customer: CounterpartyId("CUSTOMER".into()),
        supplier: CounterpartyId("SUPPLIER".into()),
        raw_item: InventoryItemCode("RAW".into()),
        finished_item: InventoryItemCode("FINISHED".into()),
        raw_account: LedgerAccountId("1403".into()),
        finished_account: LedgerAccountId("1405".into()),
        base_daily_demand_units: 1,
        unit_price_excl_vat: amount(100),
        receivable_credit_days: 5,
        raw_replenish_target_units: 1,
        raw_unit_cost_excl_vat: amount(10),
        daily_production_units: 1,
        daily_conversion_cost: amount(10),
        daily_admin_expense: amount(1),
        bad_debt_base_bp: 100,
        asset_impairment_fraction_bp: 100,
    });
    let operations_config = CompanyOperationsConfig {
        seed: 21,
        shock_params: crate::company::events::ShockParams {
            version: 1,
            market_candidate_bp: 0,
            industry_candidate_bp: 0,
            company_candidate_bp: 0,
            duration_min_days: 1,
            duration_max_days: 2,
            market_demand_band_bp: 100,
            industry_cost_band_bp: 100,
            company_demand_band_bp: 100,
            credit_deterioration_add_bp: 100,
        },
        companies: vec![OperatingCompanyConfig {
            spec: spec.clone(),
            books: IndustryBooks::Industrial(books.clone()),
            flow: flow.clone(),
        }],
    };
    let posted_on = date("2032-01-21");
    let mut operations = CompanyOperations::new(operations_config.clone(), posted_on).unwrap();
    let before_operations = serde_json::to_value(&operations).unwrap();
    let before_closing = serde_json::to_value(&closing).unwrap();
    let before_library = library.clone();
    let publication = |owner: &CompanyId, scope| PublicationRequest {
        company: owner.clone(),
        scope,
        period: crate::accounting::AccountingPeriod::from_ymd(2030, 12).unwrap(),
        kind: crate::accounting::reports::ReportKind::Annual,
        sequence: 2,
        policy: AccountingPolicyRef { chart_version: 2 },
        approved_at: crate::calendar::CivilInstant::from_hms(posted_on, 8, 0, 0).unwrap(),
        published_at: crate::calendar::CivilInstant::from_hms(posted_on, 18, 0, 0).unwrap(),
        origin: PublicationOrigin::Correction,
        supersedes: Some(original),
    };
    assert!(matches!(
        operations.correct_industrial_report(
            &company,
            OperatingReportCorrection {
                closing: &mut closing,
                library: &mut library,
                correction: crate::accounting::closing::CorrectionRequest {
                    entries: Vec::new(),
                    reason: "验证经营 owner".into()
                },
                publication: publication(
                    &company,
                    crate::accounting::consolidation::ScopeId::Consolidated(member.clone())
                ),
                posted_on,
            }
        ),
        Err(IndustrialCorrectionError::Industrial {
            cause: IndustrialError::IncomeTaxStateInconsistent { .. },
            ..
        })
    ));
    assert_eq!(
        serde_json::to_value(&operations).unwrap(),
        before_operations
    );
    assert_eq!(serde_json::to_value(&closing).unwrap(), before_closing);
    assert_eq!(library, before_library);
    operations
        .correct_industrial_report(
            &company,
            OperatingReportCorrection {
                closing: &mut closing,
                library: &mut library,
                posted_on,
                correction: crate::accounting::closing::CorrectionRequest {
                    reason: "由真实经营 owner 补记利息".into(),
                    entries: vec![JournalEntry {
                        source: BusinessEventId::new(9900),
                        date: posted_on,
                        kind: BusinessKind::InterestAccrual,
                        cash_flow: CashFlowClass::NonCash,
                        lines: vec![
                            line(chart::acct::FIN_EXP, PostingSide::Debit, amount(400)),
                            line(chart::acct::INT_PAYABLE, PostingSide::Credit, amount(400)),
                        ],
                    }],
                },
                publication: publication(
                    &company,
                    crate::accounting::consolidation::ScopeId::Standalone(member.clone()),
                ),
            },
        )
        .unwrap();
    assert_eq!(
        operations
            .industrial_books(&company)
            .unwrap()
            .net_of(chart::acct::CIT_PAYABLE)
            .unwrap(),
        amount(100)
    );
    assert_eq!(
        operations
            .industrial_books(&company)
            .unwrap()
            .net_of(chart::acct::BANK)
            .unwrap(),
        books.net_of(chart::acct::BANK).unwrap()
    );
    let before_closing = serde_json::to_value(&closing).unwrap();
    let before_library = library.clone();
    let parent = CompanyId("TAX-PARENT".into());
    let mut grouped_config = operations_config;
    grouped_config.companies[0].spec.group_parent = Some(parent.clone());
    grouped_config.companies.push(OperatingCompanyConfig {
        spec: CompanySpec {
            id: parent.clone(),
            ..spec
        },
        books: IndustryBooks::Industrial(books),
        flow,
    });
    let mut grouped = CompanyOperations::new(grouped_config, posted_on).unwrap();
    let before_grouped = serde_json::to_value(&grouped).unwrap();
    for id in [company.clone(), parent] {
        let result = grouped.correct_industrial_report(
            &id,
            OperatingReportCorrection {
                closing: &mut closing,
                library: &mut library,
                correction: crate::accounting::closing::CorrectionRequest {
                    entries: Vec::new(),
                    reason: "不留集团报表旧金额".into(),
                },
                publication: publication(
                    &id,
                    crate::accounting::consolidation::ScopeId::Standalone(
                        crate::accounting::consolidation::MemberId(id.0.clone()),
                    ),
                ),
                posted_on,
            },
        );
        assert!(
            matches!(result, Err(IndustrialCorrectionError::Industrial { cause: IndustrialError::GroupedTaxCorrectionUnsupported { company: rejected }, .. }) if rejected == id)
        );
        assert_eq!(serde_json::to_value(&grouped).unwrap(), before_grouped);
        assert_eq!(serde_json::to_value(&closing).unwrap(), before_closing);
        assert_eq!(library, before_library);
    }
}
