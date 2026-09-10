//! 四大金样 + K2 主金样（单位：注释「元」，执行「分」）。
//!
//! 主金样数值（计划 Verification strategy 固定）：
//! 期初 cash/equity = 1000；借 500；现金收入 200；现金费用 80；计提利息 10；
//! Fixture 税 22（未付）⇒ cash 1620、负债 532、权益 1088、净利 88、
//! 经营 CF 120、筹资 CF 500。

use super::{acct, books, entry, yuan};
use engine::accounting::{AccountingPeriod, BusinessKind, CashFlowClass, PostingSide};

/// K2 主金样：借贷总额相等、现金/权益滚动、现金流分类全部对账。
#[test]
fn gold_primary_scenario_cash_equity_and_cash_flow() {
    let mut b = books();
    // 期初（2029-12-31，属上一年度期间；其现金流计入 2029-12，不计入 2030-01）。
    b.post_batch(vec![entry(
        1,
        "2029-12-31",
        BusinessKind::OpeningBalance,
        CashFlowClass::Financing, // 实收资本投入属筹资活动（CAS 31 口径）
        &[
            (acct::CASH, PostingSide::Debit, 1000),
            (acct::CAPITAL, PostingSide::Credit, 1000),
        ],
    )])
    .expect("opening balance must post");

    b.post_batch(vec![
        entry(
            2,
            "2030-01-05",
            BusinessKind::LoanDisbursement,
            CashFlowClass::Financing,
            &[
                (acct::CASH, PostingSide::Debit, 500),
                (acct::LOAN, PostingSide::Credit, 500),
            ],
        ),
        entry(
            3,
            "2030-01-06",
            BusinessKind::CashRevenue,
            CashFlowClass::Operating,
            &[
                (acct::CASH, PostingSide::Debit, 200),
                (acct::REVENUE, PostingSide::Credit, 200),
            ],
        ),
        entry(
            4,
            "2030-01-07",
            BusinessKind::CashExpense,
            CashFlowClass::Operating,
            &[
                (acct::OPEX, PostingSide::Debit, 80),
                (acct::CASH, PostingSide::Credit, 80),
            ],
        ),
        entry(
            5,
            "2030-01-08",
            BusinessKind::InterestAccrual,
            CashFlowClass::NonCash,
            &[
                (acct::FIN_EXP, PostingSide::Debit, 10),
                (acct::INT_PAYABLE, PostingSide::Credit, 10),
            ],
        ),
        entry(
            6,
            "2030-01-09",
            BusinessKind::TaxAccrual,
            CashFlowClass::NonCash,
            &[
                (acct::TAX_EXP, PostingSide::Debit, 22),
                (acct::TAX_PAYABLE, PostingSide::Credit, 22),
            ],
        ),
    ])
    .expect("january business batch must post");

    let jan = AccountingPeriod::from_ymd(2030, 1).expect("period");
    let dec = AccountingPeriod::from_ymd(2029, 12).expect("period");

    // cash = 1000 + 500 + 200 - 80 = 1620 元
    assert_eq!(b.ledger().cash_total().expect("cash total"), yuan(1620));
    // 负债 = 借款 500 + 应付利息 10 + 应交税费 22 = 532 元
    assert_eq!(b.ledger().liabilities_total().expect("liab"), yuan(532));
    // 权益滚动 = 实收资本 1000 + 净利 88 = 1088 元（合法状态，未结账口径）
    assert_eq!(b.ledger().equity_rolling().expect("equity"), yuan(1088));
    // 净利 = 收入 200 - 费用(80+10+22) = 88 元
    assert_eq!(b.ledger().net_income().expect("ni"), yuan(88));
    // 经营 CF = +200 - 80 = 120；筹资 CF = 500（仅 2030-01 期间）
    assert_eq!(
        b.ledger()
            .cash_flow_for_period(jan, CashFlowClass::Operating),
        yuan(120)
    );
    assert_eq!(
        b.ledger()
            .cash_flow_for_period(jan, CashFlowClass::Financing),
        yuan(500)
    );
    // 计提利息/税为非现金：不影响任何现金流类别
    assert_eq!(
        b.ledger().cash_flow_for_period(jan, CashFlowClass::NonCash),
        yuan(0)
    );
    // 期初投入计入其所属期间 2029-12 的筹资 CF（期初不是本期流量）
    assert_eq!(
        b.ledger()
            .cash_flow_for_period(dec, CashFlowClass::Financing),
        yuan(1000)
    );
    // 借贷总额相等（每笔入账时已验，这里对总账合计复核）
    let trial = b.ledger().trial_balance().expect("trial balance");
    assert_eq!(trial.total_debits, trial.total_credits);
    assert_eq!(trial.total_debits, yuan(1000 + 500 + 200 + 80 + 10 + 22));
}

/// 赊销金样：赊销只增应收与收入（不动现金），回款只增现金减应收（不重复计收入）。
#[test]
fn gold_credit_sale_then_collection_no_double_revenue() {
    let mut b = books();
    b.post_batch(vec![entry(
        1,
        "2029-12-31",
        BusinessKind::OpeningBalance,
        CashFlowClass::Financing,
        &[
            (acct::CASH, PostingSide::Debit, 100),
            (acct::CAPITAL, PostingSide::Credit, 100),
        ],
    )])
    .expect("opening");

    b.post_batch(vec![entry(
        2,
        "2030-01-05",
        BusinessKind::CreditSale,
        CashFlowClass::Operating,
        &[
            (acct::AR, PostingSide::Debit, 300),
            (acct::REVENUE, PostingSide::Credit, 300),
        ],
    )])
    .expect("credit sale");
    // 赊销后：收入 300，现金不动（仍 100），应收 300
    assert_eq!(b.ledger().net_income().expect("ni"), yuan(300));
    assert_eq!(b.ledger().cash_total().expect("cash"), yuan(100));
    assert_eq!(
        b.ledger()
            .account_net_debit(&engine::accounting::LedgerAccountId(acct::AR.to_string()))
            .expect("ar balance"),
        yuan(300)
    );

    b.post_batch(vec![entry(
        3,
        "2030-01-10",
        BusinessKind::ReceivableCollection,
        CashFlowClass::Operating,
        &[
            (acct::CASH, PostingSide::Debit, 300),
            (acct::AR, PostingSide::Credit, 300),
        ],
    )])
    .expect("collection");
    // 回款后：现金 400，应收清零，收入仍 300（不重复）
    assert_eq!(b.ledger().cash_total().expect("cash"), yuan(400));
    assert_eq!(
        b.ledger()
            .account_net_debit(&engine::accounting::LedgerAccountId(acct::AR.to_string()))
            .expect("ar balance"),
        yuan(0)
    );
    assert_eq!(b.ledger().net_income().expect("ni"), yuan(300));
    // 经营 CF 只有回款 300（赊销期间无现金流量）
    let jan = AccountingPeriod::from_ymd(2030, 1).expect("period");
    assert_eq!(
        b.ledger()
            .cash_flow_for_period(jan, CashFlowClass::Operating),
        yuan(300)
    );
}

/// 税金支付金样：计提未付后支付——负债清零、现金减少、费用不变。
#[test]
fn gold_tax_accrual_then_payment_clears_liability_only() {
    let mut b = books();
    b.post_batch(vec![entry(
        1,
        "2029-12-31",
        BusinessKind::OpeningBalance,
        CashFlowClass::Financing,
        &[
            (acct::CASH, PostingSide::Debit, 100),
            (acct::CAPITAL, PostingSide::Credit, 100),
        ],
    )])
    .expect("opening");

    b.post_batch(vec![entry(
        2,
        "2030-01-05",
        BusinessKind::TaxAccrual,
        CashFlowClass::NonCash,
        &[
            (acct::TAX_EXP, PostingSide::Debit, 22),
            (acct::TAX_PAYABLE, PostingSide::Credit, 22),
        ],
    )])
    .expect("tax accrual");
    b.post_batch(vec![entry(
        3,
        "2030-01-20",
        BusinessKind::TaxPayment,
        CashFlowClass::Operating, // 支付的各项税费属经营活动（CAS 31 口径）
        &[
            (acct::TAX_PAYABLE, PostingSide::Debit, 22),
            (acct::CASH, PostingSide::Credit, 22),
        ],
    )])
    .expect("tax payment");

    let tax_payable = engine::accounting::LedgerAccountId(acct::TAX_PAYABLE.to_string());
    // 负债清零、现金 100-22=78、费用不变（净利仍 -22）
    assert_eq!(
        b.ledger()
            .account_net_debit(&tax_payable)
            .expect("tax payable"),
        yuan(0)
    );
    assert_eq!(b.ledger().cash_total().expect("cash"), yuan(78));
    assert_eq!(b.ledger().net_income().expect("ni"), yuan(-22));
    assert_eq!(b.ledger().liabilities_total().expect("liab"), yuan(0));
    let jan = AccountingPeriod::from_ymd(2030, 1).expect("period");
    assert_eq!(
        b.ledger()
            .cash_flow_for_period(jan, CashFlowClass::Operating),
        yuan(-22)
    );
}

/// 折旧金样：非现金费用——费用上升、累计折旧（资产备抵）上升，现金与 CF 不受影响。
#[test]
fn gold_depreciation_is_non_cash() {
    let mut b = books();
    b.post_batch(vec![entry(
        1,
        "2029-12-31",
        BusinessKind::OpeningBalance,
        CashFlowClass::Financing,
        &[
            (acct::CASH, PostingSide::Debit, 100),
            (acct::CAPITAL, PostingSide::Credit, 100),
        ],
    )])
    .expect("opening");

    b.post_batch(vec![entry(
        2,
        "2030-01-31",
        BusinessKind::Depreciation,
        CashFlowClass::NonCash,
        &[
            (acct::OPEX, PostingSide::Debit, 50),
            (acct::ACC_DEP, PostingSide::Credit, 50),
        ],
    )])
    .expect("depreciation");

    let acc_dep = engine::accounting::LedgerAccountId(acct::ACC_DEP.to_string());
    assert_eq!(
        b.ledger().account_net_debit(&acc_dep).expect("acc dep"),
        yuan(-50) // 贷方余额 50 元（备抵资产）
    );
    assert_eq!(b.ledger().cash_total().expect("cash"), yuan(100));
    assert_eq!(b.ledger().net_income().expect("ni"), yuan(-50));
    let jan = AccountingPeriod::from_ymd(2030, 1).expect("period");
    assert_eq!(
        b.ledger()
            .cash_flow_for_period(jan, CashFlowClass::Operating),
        yuan(0)
    );
    assert_eq!(
        b.ledger().cash_flow_for_period(jan, CashFlowClass::NonCash),
        yuan(0)
    );
}
