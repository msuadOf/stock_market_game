//! 资本开支、直线折旧、固定资产减值与应收坏账（CAS 22 §63 整个存续期 ECL
//! 简化法——官方依据已核验，docs/company-accounting.md §2.2）。全部非现金事项
//! 与现金严格分离：折旧/减值/坏账准备/核销不动现金；资本开支计投资活动现金。
//!
//! 折旧守恒（剩余基础 / 剩余月数，整数半偶舍入）：成本 1000 元、残值 0.02 元、
//! 寿命 3 月 ⇒ 基础 99_998 分：m1 = rhe(99998/3) = 33_333；
//! m2 = rhe(66665/2) = 33_332（恰 .5 取偶）；m3 = 33_333；Σ = 99_998 分毫不差。

use super::{acct, amt, base_config, d, net_debit, yuan};
use engine::accounting::{AccountingAmount, AccountingPeriod, CashFlowClass, LedgerAccountId};
use engine::accounting::{FixedAssetCode, InventoryItemCode};
use engine::company::industrial::Settlement;
use engine::company::CounterpartyId;

fn cp(id: &str) -> CounterpartyId {
    CounterpartyId(id.to_string())
}

fn machine() -> FixedAssetCode {
    FixedAssetCode("MACHINE-1".to_string())
}

#[test]
fn gold_capex_depreciation_straight_line_conservation() {
    let mut co =
        engine::company::industrial::IndustrialBooks::new(base_config()).expect("base opening");
    // 2030-01-02 资本开支 1000 元购入设备（残值 0.02 元、寿命 3 月）——投资活动。
    co.acquire_asset(
        machine(),
        yuan(1_000),
        AccountingAmount::from_cents(2),
        3,
        d("2030-01-02"),
    )
    .expect("capex acquisition");
    assert_eq!(net_debit(&co, acct::FIXED_ASSET), yuan(1_000));
    assert_eq!(net_debit(&co, acct::BANK), yuan(9_000));
    let jan = AccountingPeriod::from_ymd(2030, 1).expect("period");
    assert_eq!(
        co.books()
            .ledger()
            .cash_flow_for_period(jan, CashFlowClass::Investing),
        yuan(-1_000)
    );

    // 三个月折旧：33_333 / 33_332 / 33_333 分（每步先 preview 后 apply，数值一致）。
    let expected = [33_333_i128, 33_332, 33_333];
    for (month, &amount) in expected.iter().enumerate() {
        let preview = co
            .assets()
            .preview_depreciation(&machine())
            .expect("preview depreciation");
        let posted = co
            .depreciate_month(d("2030-01-31"))
            .expect("monthly depreciation");
        assert_eq!(posted.len(), 1);
        assert_eq!(posted[0].amount.cents(), amount, "month {}", month + 1);
        assert_eq!(preview.cents(), amount);
    }
    // 累计折旧 999.98 元；账面 = 1000 − 999.98 = 0.02 元 = 残值（守恒）。
    assert_eq!(net_debit(&co, acct::ACC_DEP), amt(-99_998));
    let asset = co.assets().get(&machine()).expect("asset registered");
    assert_eq!(
        asset.carrying_amount().expect("carrying"),
        AccountingAmount::from_cents(2)
    );
    assert_eq!(asset.remaining_months(), 0);
    // 折旧非现金：现金与经营现金流不受影响；管理费用 999.98 元。
    assert_eq!(net_debit(&co, acct::BANK), yuan(9_000));
    assert_eq!(
        co.books()
            .ledger()
            .cash_flow_for_period(jan, CashFlowClass::Operating),
        AccountingAmount::ZERO
    );
    assert_eq!(net_debit(&co, acct::ADMIN_EXP), amt(99_998));

    // 第四个月：寿命已满 → 月扫合法空扫（无新钱/新分录），状态不变。
    // （登记簿层的逐资产 FullyDepreciated 拒绝由 subledgers.rs 锁定。）
    let before = co.clone();
    let sweep = co
        .depreciate_month(d("2030-04-30"))
        .expect("sweep skips fully depreciated assets");
    assert!(sweep.is_empty());
    assert_eq!(co, before);
}

#[test]
fn gold_impairment_after_depreciation_rebases_remaining_cost() {
    let mut co =
        engine::company::industrial::IndustrialBooks::new(base_config()).expect("base opening");
    co.acquire_asset(
        machine(),
        yuan(1_000),
        AccountingAmount::from_cents(2),
        3,
        d("2030-01-02"),
    )
    .expect("capex");
    // m1 折旧 33_333 后减值 100 元：账面 666.67 − 0.02 ≥ 100（在上限内）。
    co.depreciate_month(d("2030-01-31")).expect("m1");
    co.impair_asset(&machine(), yuan(100), d("2030-02-10"))
        .expect("impairment");
    assert_eq!(net_debit(&co, acct::ACC_IMPAIR), yuan(-100));
    assert_eq!(net_debit(&co, acct::IMPAIR_LOSS), yuan(100));
    // 剩余基础 = 999.98 − 333.33 − 100 = 566.65 元，两个月均摊：
    // m2 = rhe(56_665/2) = 28_332（恰 .5 取偶）；m3 = 28_333。
    let m2 = co.depreciate_month(d("2030-02-28")).expect("m2");
    assert_eq!(m2[0].amount.cents(), 28_332);
    let m3 = co.depreciate_month(d("2030-03-31")).expect("m3");
    assert_eq!(m3[0].amount.cents(), 28_333);
    // Σ折旧 899.98 + 减值 100 = 999.98 = 成本 − 残值（守恒）；账面 = 残值。
    let asset = co.assets().get(&machine()).expect("asset");
    assert_eq!(
        asset.accumulated_depreciation(),
        AccountingAmount::from_cents(33_333 + 28_332 + 28_333)
    );
    assert_eq!(asset.accumulated_impairment(), yuan(100));
    assert_eq!(
        asset.carrying_amount().expect("carrying"),
        AccountingAmount::from_cents(2)
    );
    // 减值与折旧全程非现金：现金仍是 9000 元。
    assert_eq!(net_debit(&co, acct::BANK), yuan(9_000));
}

#[test]
fn gold_bad_debt_allowance_accrual_and_write_off_are_non_cash() {
    let mut co =
        engine::company::industrial::IndustrialBooks::new(base_config()).expect("base opening");
    // 现购商品 10 件 @ 100 元（进项 130 元），现金流出 1130 元。
    co.purchase(
        &cp("EXT-SUPP"),
        InventoryItemCode("GOODS".to_string()),
        LedgerAccountId(acct::FINISHED.to_string()),
        10,
        yuan(100),
        Settlement::Cash,
        d("2030-03-01"),
        d("2030-01-05"),
    )
    .expect("purchase");
    // 赊销 10 件 @ 300 元：收入 3000 元、销项 390 元、成本 1000 元、应收 3390 元。
    let sale = co
        .sell_credit(
            &cp("EXT-CUST"),
            InventoryItemCode("GOODS".to_string()),
            10,
            yuan(300),
            d("2030-03-01"),
            d("2030-01-15"),
        )
        .expect("credit sale");
    assert_eq!(sale.revenue, yuan(3_000));
    let cash_before = net_debit(&co, acct::BANK);

    // ECL 简化法计提坏账准备 10%：3390 × 10% = 339 元（非现金）。
    let accrue_event = co
        .update_bad_debt_allowance(1_000, d("2030-01-31"))
        .expect("allowance accrual")
        .expect("non-zero delta posts an entry");
    assert!(accrue_event.value() > 1);
    assert_eq!(net_debit(&co, acct::BAD_DEBT_ALLOW), yuan(-339));
    assert_eq!(net_debit(&co, acct::IMPAIR_LOSS), yuan(339));
    assert_eq!(net_debit(&co, acct::BANK), cash_before);

    // 准备不足时核销 → 类型化拒绝（必须先补提），完整状态不变。
    let before = co.clone();
    assert!(matches!(
        co.write_off_receivable(&sale.receivable, d("2030-02-05")),
        Err(engine::company::industrial::IndustrialError::InsufficientAllowance { .. })
    ));
    assert_eq!(co, before);

    // 补提至 100% 再核销：Dr 坏账准备 3390 / Cr 应收 3390（非现金，不碰现金）。
    co.update_bad_debt_allowance(10_000, d("2030-02-05"))
        .expect("allowance top-up");
    assert_eq!(net_debit(&co, acct::BAD_DEBT_ALLOW), yuan(-3_390));
    co.write_off_receivable(&sale.receivable, d("2030-02-06"))
        .expect("write-off");
    assert_eq!(net_debit(&co, acct::AR), AccountingAmount::ZERO);
    assert_eq!(net_debit(&co, acct::BAD_DEBT_ALLOW), AccountingAmount::ZERO);
    assert_eq!(net_debit(&co, acct::BANK), cash_before);
    assert_eq!(
        co.receivables().written_off_total().expect("written off"),
        yuan(3_390)
    );
    // 经营亏损合法（非引擎错误）：净利 = 3000 − 1000 − 339 − 3051 = −1390 元。
    assert_eq!(co.books().ledger().net_income().expect("ni"), yuan(-1_390));
    // 补提金额：3390 − 339 = 3051 元（Dr 6701 累计 3390）。
    assert_eq!(net_debit(&co, acct::IMPAIR_LOSS), yuan(3_390));
}
