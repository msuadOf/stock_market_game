//! 增值税结算与当期+递延所得税金样（Fixture 合成税率，价外税模型）。
//!
//! 增值税（财会〔2016〕22号，已核验）：销项/进项分开核算，应纳 = 销项 − 可抵扣
//! 进项；进项富余结转（222102 借方余额留存），不模拟退税。
//! 所得税：可抵扣亏损 FIFO 结转（本政策 5 年），递延所得税资产 = 未用亏损 ×
//! 税率**全额确认**——游戏简化（CAS 18 全文取证受阻，docs §2.1 登记 ⛔，
//! 不确认确认门槛/折现，不声称完整 CAS 18 合规）。

use super::{acct, base_config, cent_line, d, net_debit, yuan};
use engine::accounting::InventoryItemCode;
use engine::accounting::{
    AccountingAmount, IncomeTaxPolicy, LedgerAccountId, LossEntry, PostingSide,
};
use engine::company::industrial::{OpeningInventoryItem, Settlement};
use engine::company::CounterpartyId;

fn cp(id: &str) -> CounterpartyId {
    CounterpartyId(id.to_string())
}

fn goods() -> InventoryItemCode {
    InventoryItemCode("GOODS".to_string())
}

#[test]
fn gold_vat_netting_pays_output_minus_deductible_input() {
    let mut co =
        engine::company::industrial::IndustrialBooks::new(base_config()).expect("base opening");
    // 现购 10 件 @ 100 元：进项 130 元；赊销 5 件 @ 400 元：销项 260 元。
    co.purchase(
        &cp("EXT-SUPP"),
        goods(),
        LedgerAccountId(acct::FINISHED.to_string()),
        10,
        yuan(100),
        Settlement::Cash,
        d("2030-03-05"),
        d("2030-01-05"),
    )
    .expect("purchase");
    co.sell_credit(
        &cp("EXT-CUST"),
        goods(),
        5,
        yuan(400),
        d("2030-02-15"),
        d("2030-01-10"),
    )
    .expect("sale");
    assert_eq!(net_debit(&co, acct::VAT_OUT), yuan(-260));
    assert_eq!(net_debit(&co, acct::VAT_IN), yuan(130));
    // 应纳 = 260 − 130 = 130 元：Dr 销项 260 / Cr 进项 130 / Cr 现金 130。
    co.pay_vat(d("2030-01-31")).expect("vat settlement");
    assert_eq!(net_debit(&co, acct::VAT_OUT), AccountingAmount::ZERO);
    assert_eq!(net_debit(&co, acct::VAT_IN), AccountingAmount::ZERO);
    // 现金 = 10000 − 1130（采购含税） − 130（缴税）= 8740 元。
    assert_eq!(net_debit(&co, acct::BANK), yuan(8_740));
    // 两种税都结清后再缴 → 类型化拒绝（无可缴税额），状态不变。
    let before = co.clone();
    assert!(matches!(
        co.pay_vat(d("2030-02-28")),
        Err(engine::company::industrial::IndustrialError::NothingToPay)
    ));
    assert_eq!(co, before);
}

#[test]
fn gold_vat_excess_input_carries_forward_without_cash() {
    let mut co =
        engine::company::industrial::IndustrialBooks::new(base_config()).expect("base opening");
    // 现购 10 件 @ 100 元（进项 130 元）；仅赊销 2 件 @ 100 元（销项 26 元）。
    co.purchase(
        &cp("EXT-SUPP"),
        goods(),
        LedgerAccountId(acct::FINISHED.to_string()),
        10,
        yuan(100),
        Settlement::Cash,
        d("2030-03-05"),
        d("2030-01-05"),
    )
    .expect("purchase");
    co.sell_credit(
        &cp("EXT-CUST"),
        goods(),
        2,
        yuan(100),
        d("2030-02-15"),
        d("2030-01-10"),
    )
    .expect("sale");
    let cash_before = net_debit(&co, acct::BANK);
    // 销项 26 < 进项 130：只做净结转（Dr 222101 26 / Cr 222102 26），**不动现金**；
    // 富余进项 104 元结转留抵（222102 借方余额）。
    co.pay_vat(d("2030-01-31")).expect("vat netting");
    assert_eq!(net_debit(&co, acct::VAT_OUT), AccountingAmount::ZERO);
    assert_eq!(net_debit(&co, acct::VAT_IN), yuan(104));
    assert_eq!(net_debit(&co, acct::BANK), cash_before);
}

#[test]
fn gold_income_tax_loss_carryforward_current_and_deferred() {
    // 开局：现金 5000 元 + 库存商品 200 件 @ 10 元（子账种子 = 2000 元/1405）。
    let mut cfg = base_config();
    cfg.opening_lines = vec![
        cent_line(acct::BANK, PostingSide::Debit, 500_000),
        cent_line(acct::FINISHED, PostingSide::Debit, 200_000),
        cent_line(acct::CAPITAL, PostingSide::Credit, 700_000),
    ];
    cfg.opening_inventory = vec![OpeningInventoryItem {
        account: LedgerAccountId(acct::FINISHED.to_string()),
        item: goods(),
        quantity: 200,
        cost: yuan(2_000),
    }];
    let mut co = engine::company::industrial::IndustrialBooks::new(cfg)
        .expect("opening with inventory seed");

    // 2030 年：管理费用 1000 元 ⇒ 期间税前 −1000 元。**经营亏损不是引擎错误**。
    co.pay_expense(
        engine::company::industrial::ExpenseKind::Admin,
        yuan(1_000),
        d("2030-12-15"),
    )
    .expect("expense");
    let y1 = co
        .accrue_income_tax(d("2030-12-31"))
        .expect("loss-year tax accrual");
    assert_eq!(y1.pretax, yuan(-1_000));
    assert_eq!(y1.current_tax, AccountingAmount::ZERO);
    assert_eq!(y1.loss_added, yuan(1_000));
    // 递延所得税资产 = 1000 × 25% = 250 元（Dr 1811 / Cr 6801，非现金）。
    assert_eq!(y1.deferred_delta, yuan(250));
    assert_eq!(net_debit(&co, acct::DTA), yuan(250));
    assert_eq!(net_debit(&co, acct::TAX_EXP), yuan(-250));
    // 净利 = −1000 + 250 = −750 元（递延所得税收益调减亏损；合法负值）。
    assert_eq!(co.books().ledger().net_income().expect("ni"), yuan(-750));

    // 2031 年：赊销 200 件 @ 40 元（收入 8000、销项 1040、成本 2000）并回款。
    let sale = co
        .sell_credit(
            &cp("EXT-CUST"),
            goods(),
            200,
            yuan(40),
            d("2031-07-01"),
            d("2031-06-20"),
        )
        .expect("sale");
    co.collect(&sale.receivable, yuan(9_040), d("2031-07-01"))
        .expect("collection");
    let y2 = co
        .accrue_income_tax(d("2031-12-31"))
        .expect("profit-year tax accrual");
    // 2031 期间税前 = 8000 − 2000 = 6000 元；弥补 2030 年亏损 1000 元（FIFO）；
    // 应税 5000 × 25% = 1250 元当期税；DTA 目标 0 → 转回 250 元。
    assert_eq!(y2.pretax, yuan(6_000));
    assert_eq!(y2.loss_offset_used, yuan(1_000));
    assert_eq!(y2.current_tax, yuan(1_250));
    assert_eq!(y2.deferred_delta, yuan(-250));
    assert_eq!(net_debit(&co, acct::CIT_PAYABLE), yuan(-1_250));
    assert_eq!(net_debit(&co, acct::DTA), AccountingAmount::ZERO);
    assert_eq!(net_debit(&co, acct::TAX_EXP), yuan(1_250)); // −250（y1 收益）+ 1500（y2 当期+转回）
                                                            // 全周期净利 = 8000 − 2000 − 1000 − 1250（净所得税费用）= 3750 元。
    assert_eq!(co.books().ledger().net_income().expect("ni"), yuan(3_750));

    // 2032-01-15 缴所得税 1250 元（经营活动现金流出，负债清零）。
    co.pay_income_tax(yuan(1_250), d("2032-01-15"))
        .expect("income tax payment");
    assert_eq!(net_debit(&co, acct::CIT_PAYABLE), AccountingAmount::ZERO);
    assert_eq!(
        net_debit(&co, acct::BANK),
        yuan(5_000 - 1_000 - 1_250 + 9_040)
    );
}

/// 纯函数单元：亏损到期出池（2028 年亏损 5 年结转至 2033 年止，2034 过期）。
#[test]
fn tax_unit_loss_expiry_drops_old_losses_before_offset() {
    let policy = IncomeTaxPolicy {
        rate_bp: 2_500,
        loss_carryforward_years: 5,
    };
    let pool = vec![LossEntry {
        origin_year: 2028,
        remaining: yuan(1_000),
    }];
    let result =
        engine::accounting::compute_income_tax(yuan(1_000), 2034, &pool, &policy).expect("compute");
    assert_eq!(result.losses_expired, yuan(1_000));
    assert_eq!(result.loss_offset_used, AccountingAmount::ZERO);
    assert_eq!(result.current_tax, yuan(250));
    assert!(result.ending_pool.is_empty());
    assert_eq!(result.deferred_tax_asset, AccountingAmount::ZERO);
}

/// 纯函数单元：FIFO 弥补（先用 2029 年 300 元，再用 2031 年 200 元）+ 期末 DTA。
#[test]
fn tax_unit_loss_offset_is_fifo_and_deferred_on_remainder() {
    let policy = IncomeTaxPolicy {
        rate_bp: 2_500,
        loss_carryforward_years: 5,
    };
    let pool = vec![
        LossEntry {
            origin_year: 2029,
            remaining: yuan(300),
        },
        LossEntry {
            origin_year: 2031,
            remaining: yuan(800),
        },
    ];
    let result =
        engine::accounting::compute_income_tax(yuan(500), 2033, &pool, &policy).expect("compute");
    assert_eq!(result.loss_offset_used, yuan(500));
    assert_eq!(result.current_tax, AccountingAmount::ZERO);
    assert_eq!(result.ending_pool.len(), 1);
    assert_eq!(result.ending_pool[0].origin_year, 2031);
    assert_eq!(result.ending_pool[0].remaining, yuan(600));
    // 期末未用亏损 600 × 25% = 150 元 DTA（全额确认简化）。
    assert_eq!(result.deferred_tax_asset, yuan(150));
}

/// 纯函数单元：进项按政策比例拆分（不可抵扣部分归集入成本）。
#[test]
fn tax_unit_input_vat_split_by_deductible_share() {
    let vat = engine::accounting::VatPolicy {
        output_rate_bp: 1_300,
        input_rate_bp: 1_300,
        deductible_share_bp: 5_000,
    };
    let split = engine::accounting::split_input_vat(yuan(1_000), &vat).expect("split");
    assert_eq!(split.deductible, yuan(65));
    assert_eq!(split.non_deductible, yuan(65));
    let out = engine::accounting::output_vat_on(yuan(1_000), &vat).expect("output");
    assert_eq!(out, yuan(130));
}
