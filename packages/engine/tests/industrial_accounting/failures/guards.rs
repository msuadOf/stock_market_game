//! 超授信 / 无授信 / 无现金付款（PaymentFailed，不透支不补钱）/ 逾期应付面 /
//! 计息守卫 / 开局种子对账守卫。

use super::super::{acct, base_config, cent_line, d, net_debit, yuan};
use super::fresh;
use engine::accounting::InventoryItemCode;
use engine::accounting::LedgerAccountId;
use engine::accounting::PostingSide;
use engine::company::industrial::{
    IndustrialConfig, IndustrialError, OpeningDebtTerms, Settlement,
};
use engine::company::{ContractId, CounterpartyId};

fn cp(id: &str) -> CounterpartyId {
    CounterpartyId(id.to_string())
}

fn goods() -> InventoryItemCode {
    InventoryItemCode("GOODS".to_string())
}

#[test]
fn borrowing_beyond_credit_line_is_rejected_state_unchanged() {
    let mut co = fresh();
    let before = co.clone();
    // 授信 5000 元：借 5001 元 → DebtBeyondCreditLine（携带占用/申请/上限）。
    let err = co
        .borrow(
            ContractId("L1".to_string()),
            &cp("EXT-BANK"),
            yuan(5_001),
            400,
            d("2030-01-05"),
            d("2030-07-05"),
        )
        .unwrap_err();
    match err {
        IndustrialError::DebtBeyondCreditLine {
            lender,
            outstanding,
            requested,
            limit,
        } => {
            assert_eq!(lender, cp("EXT-BANK"));
            assert_eq!(requested, yuan(5_001));
            assert_eq!(limit, yuan(5_000));
            assert_eq!(outstanding, yuan(0));
        }
        other => panic!("expected DebtBeyondCreditLine, got {other:?}"),
    }
    assert_eq!(co, before);
    // 无授信贷款人（供应商无授信）→ NoCreditLine。
    assert!(matches!(
        co.borrow(
            ContractId("L2".to_string()),
            &cp("EXT-SUPP"),
            yuan(1),
            400,
            d("2030-01-05"),
            d("2030-07-05"),
        ),
        Err(IndustrialError::NoCreditLine { .. })
    ));
    // 未登记对手方 → UnknownCounterparty。
    assert!(matches!(
        co.borrow(
            ContractId("L3".to_string()),
            &cp("EXT-NONE"),
            yuan(1),
            400,
            d("2030-01-05"),
            d("2030-07-05"),
        ),
        Err(IndustrialError::Company(
            engine::company::CompanyError::UnknownCounterparty { .. }
        ))
    ));
    // 重复合同 id → 拒绝（先借合法 1000 元，再复用 id）。
    co.borrow(
        ContractId("L1".to_string()),
        &cp("EXT-BANK"),
        yuan(1_000),
        400,
        d("2030-01-05"),
        d("2030-07-05"),
    )
    .expect("borrow within credit");
    let mid = co.clone();
    assert!(matches!(
        co.borrow(
            ContractId("L1".to_string()),
            &cp("EXT-BANK"),
            yuan(1),
            400,
            d("2030-01-06"),
            d("2030-07-06"),
        ),
        Err(IndustrialError::Company(
            engine::company::CompanyError::DuplicateContract { .. }
        ))
    ));
    assert_eq!(co, mid);
}

#[test]
fn payment_without_cash_fails_typed_and_company_keeps_running() {
    let mut co = fresh();
    let before = co.clone();
    // 现金 10000 元，付 10001 元费用 → PaymentFailed（负现金禁令的领域映射），
    // 不透支、不自动补钱。
    assert!(matches!(
        co.pay_expense(
            engine::company::industrial::ExpenseKind::Admin,
            yuan(10_001),
            d("2030-01-10"),
        ),
        Err(IndustrialError::PaymentFailed { .. })
    ));
    assert_eq!(co, before);
    assert_eq!(net_debit(&co, acct::BANK), yuan(10_000));
    // 公司继续运行：合法小额费用照常支付。
    co.pay_expense(
        engine::company::industrial::ExpenseKind::Admin,
        yuan(100),
        d("2030-01-11"),
    )
    .expect("company keeps running");
    assert_eq!(net_debit(&co, acct::BANK), yuan(9_900));

    // 缴税超额同样 PaymentFailed：现购 1 件 @ 100 元（进项 13）+ 赊销 1 件
    // @ 1000 元（销项 130）→ 应纳 117 元；先耗尽现金再缴 → PaymentFailed。
    co.purchase(
        &cp("EXT-SUPP"),
        goods(),
        LedgerAccountId(acct::FINISHED.to_string()),
        1,
        yuan(100),
        Settlement::Cash,
        d("2030-03-01"),
        d("2030-01-05"),
    )
    .expect("cash purchase");
    co.sell_credit(
        &cp("EXT-CUST"),
        goods(),
        1,
        yuan(1_000),
        d("2030-02-15"),
        d("2030-01-06"),
    )
    .expect("sale");
    // 现金 9787 元（10000 − 100 前述费用 − 113 采购含税）：全部付掉 → 0。
    co.pay_expense(
        engine::company::industrial::ExpenseKind::Selling,
        yuan(9_787),
        d("2030-01-12"),
    )
    .expect("drain cash to zero");
    assert_eq!(net_debit(&co, acct::BANK), yuan(0));
    let drained = co.clone();
    assert!(matches!(
        co.pay_vat(d("2030-01-31")),
        Err(IndustrialError::PaymentFailed { .. })
    ));
    assert_eq!(co, drained);
    // 应交税额仍在（负债不消失）：销项 130 挂账、进项 13 留抵。
    assert_eq!(net_debit(&co, acct::VAT_OUT), yuan(-130));
    assert_eq!(net_debit(&co, acct::VAT_IN), yuan(13));
}

#[test]
fn unpaid_payable_past_due_surfaces_as_overdue_without_bailout() {
    let mut co = fresh();
    // 现金耗至 100 元，赊购 1130 元（到期 2030-02-01）。
    co.pay_expense(
        engine::company::industrial::ExpenseKind::Admin,
        yuan(9_900),
        d("2030-01-05"),
    )
    .expect("drain cash");
    let purchase = co
        .purchase(
            &cp("EXT-SUPP"),
            goods(),
            LedgerAccountId(acct::FINISHED.to_string()),
            10,
            yuan(100),
            Settlement::Credit,
            d("2030-02-01"),
            d("2030-01-10"),
        )
        .expect("credit purchase");
    let ap = purchase.payable.clone().expect("payable opened");
    assert!(co.payables().overdue(d("2030-01-31")).is_empty());
    // 到期日现金不足 → PaymentFailed；应付保持 open 并进入逾期面。
    let before = co.clone();
    assert!(matches!(
        co.settle_payable(&ap, d("2030-02-01")),
        Err(IndustrialError::PaymentFailed { .. })
    ));
    assert_eq!(co, before);
    let overdue: Vec<&engine::accounting::OpenItemId> = co
        .payables()
        .overdue(d("2030-02-02"))
        .iter()
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(overdue, vec![&ap]);
    assert_eq!(co.payables().total_open().expect("ap"), yuan(1_130));
    // 不自动派钱、不透支：现金不变。
    assert_eq!(net_debit(&co, acct::BANK), yuan(100));
}

#[test]
fn interest_guards_reject_backwards_accrual_and_empty_payment() {
    let mut co = fresh();
    co.borrow(
        ContractId("L1".to_string()),
        &cp("EXT-BANK"),
        yuan(1_000),
        400,
        d("2030-01-10"),
        d("2030-07-10"),
    )
    .expect("borrow");
    co.accrue_interest(d("2030-01-31")).expect("accrue");
    // 时间回拨计提 → 类型化拒绝。
    let mut before = co.clone();
    assert!(matches!(
        co.accrue_interest(d("2030-01-30")),
        Err(IndustrialError::AccrualNotForward { .. })
    ));
    assert_eq!(co, before);
    // 未计提利息的合同付息 → NothingAccrued。
    co.borrow(
        ContractId("L2".to_string()),
        &cp("EXT-BANK"),
        yuan(500),
        400,
        d("2030-01-31"),
        d("2030-07-31"),
    )
    .expect("borrow 2 starts today");
    before = co.clone();
    match co.pay_interest(&ContractId("L2".to_string()), d("2030-01-31")) {
        Err(IndustrialError::NothingAccrued { contract }) => {
            assert_eq!(contract.0, "L2");
        }
        other => panic!("expected NothingAccrued, got {other:?}"),
    }
    // 未知合同 → UnknownLoan；超未偿本金还款 → 本金越界。
    assert!(matches!(
        co.pay_interest(&ContractId("NOPE".to_string()), d("2030-01-31")),
        Err(IndustrialError::UnknownLoan { .. })
    ));
    assert!(matches!(
        co.repay_principal(&ContractId("L1".to_string()), yuan(1_001), d("2030-02-05")),
        Err(IndustrialError::PrincipalBeyondOutstanding { .. })
    ));
    assert_eq!(co, before);
}

#[test]
fn opening_seed_guards_reject_ledger_subledger_drift() {
    // 开局借款条款与 2001 余额不符 → OpeningDebtMismatch（不允许隐性漂移）。
    let mut cfg = base_config();
    cfg.opening_lines = vec![
        cent_line(acct::BANK, PostingSide::Debit, 1_100_000),
        cent_line(acct::CAPITAL, PostingSide::Credit, 1_000_000),
        cent_line(acct::ST_DEBT, PostingSide::Credit, 100_000),
    ];
    cfg.opening_debt = Some(OpeningDebtTerms {
        lender: cp("EXT-BANK"),
        principal: yuan(2_000),
        annual_rate_bp: 400,
        maturity_date: d("2031-06-30"),
    });
    assert!(matches!(
        engine::company::industrial::IndustrialBooks::new(cfg),
        Err(IndustrialError::OpeningDebtMismatch { .. })
    ));

    // 开局存货种子与 1405 余额不符 → OpeningSeedMismatch。
    let mut cfg = base_config();
    cfg.opening_lines = vec![
        cent_line(acct::BANK, PostingSide::Debit, 1_000_000),
        cent_line(acct::FINISHED, PostingSide::Debit, 200_000),
        cent_line(acct::CAPITAL, PostingSide::Credit, 1_200_000),
    ];
    cfg.opening_inventory = vec![engine::company::industrial::OpeningInventoryItem {
        account: LedgerAccountId(acct::FINISHED.to_string()),
        item: goods(),
        quantity: 10,
        cost: yuan(1_000), // 总账 2000 元 ≠ 种子 1000 元
    }];
    assert!(matches!(
        engine::company::industrial::IndustrialBooks::new(cfg),
        Err(IndustrialError::OpeningSeedMismatch { .. })
    ));

    // 开局累计折旧（1602 非零）暂不支持 → 类型化拒绝（诚实边界，前史由任务 14
    // 生成）。资产种子与 1601 对齐，确保命中的是累计折旧守卫而非种子对账守卫。
    let mut cfg = base_config();
    cfg.opening_lines = vec![
        cent_line(acct::BANK, PostingSide::Debit, 900_000),
        cent_line(acct::FIXED_ASSET, PostingSide::Debit, 200_000),
        cent_line(acct::ACC_DEP, PostingSide::Credit, 100_000),
        cent_line(acct::CAPITAL, PostingSide::Credit, 1_000_000),
    ];
    cfg.opening_assets = vec![engine::company::industrial::OpeningAssetItem {
        code: engine::accounting::FixedAssetCode("OPEN-M".to_string()),
        cost: yuan(2_000),
        salvage_value: engine::accounting::AccountingAmount::ZERO,
        life_months: 24,
    }];
    assert!(matches!(
        engine::company::industrial::IndustrialBooks::new(cfg),
        Err(IndustrialError::OpeningAccumulatedDepreciation { .. })
    ));

    // 非法开局行本身（不平衡）→ 透传 Accounting 错误（OpeningPost 语义）。
    let mut cfg: IndustrialConfig = base_config();
    cfg.opening_lines = vec![
        cent_line(acct::BANK, PostingSide::Debit, 1_000_001),
        cent_line(acct::CAPITAL, PostingSide::Credit, 1_000_000),
    ];
    assert!(matches!(
        engine::company::industrial::IndustrialBooks::new(cfg),
        Err(IndustrialError::Accounting(_))
    ));
}
