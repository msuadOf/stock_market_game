//! 工商职责迁移的行为保护：固定开局首错、计息落分与税务提交边界。

use super::*;
use crate::accounting::{FixedAssetCode, IncomeTaxPolicy, InventoryItemCode, VatPolicy};
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
    for invalid in [
        {
            let mut invalid = valid.clone();
            invalid["income_tax_position"]
                .as_object_mut()
                .unwrap()
                .remove("assessment");
            invalid
        },
        {
            let mut invalid = valid.clone();
            invalid["income_tax_position"]["assessment"]["current_tax"] = serde_json::json!("1.00");
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
    assert!(matches!(
        restored.accrue_income_tax(date("2030-12-31")),
        Err(IndustrialError::HistoricalTaxReassessmentUnsupported {
            requested_year: 2030,
            latest_year: 2031
        })
    ));
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
