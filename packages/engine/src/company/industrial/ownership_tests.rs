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
    assert!(before["loss_pool"].is_array());
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
fn repeated_annual_tax_keeps_existing_non_idempotent_loss_behavior() {
    let mut books = IndustrialBooks::new(config()).expect("开局");
    books
        .pay_expense(ExpenseKind::Admin, amount(400), date("2030-01-01"))
        .expect("费用");
    let first = books.accrue_income_tax(date("2030-12-31")).expect("首计提");
    assert_eq!(first.pretax, amount(-400));
    assert_eq!(first.deferred_delta, amount(100));
    let second = books
        .accrue_income_tax(date("2030-12-31"))
        .expect("重复计提");
    assert_eq!(second.pretax, amount(-300));
    assert_eq!(second.loss_added, amount(300));
    assert_eq!(second.deferred_delta, amount(75));
    assert_eq!(books.loss_pool().len(), 2);
}

#[test]
fn zero_line_tax_still_commits_expired_loss_pool() {
    let mut books = IndustrialBooks::new(config()).expect("开局");
    let mut value = serde_json::to_value(&books).expect("序列化");
    value["loss_pool"] = serde_json::json!([{"origin_year": 2020, "remaining": "0.04"}]);
    books = serde_json::from_value(value).expect("保持现有 loss_pool 接受集");
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
        books.available_credit(&CounterpartyId("LENDER".into())),
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
        restored.available_credit(&CounterpartyId("LENDER".into())),
        Some(amount(10_000))
    );
}
