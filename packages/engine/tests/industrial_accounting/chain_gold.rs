//! 全链金样：期初（含开局借款种子）→ 现付采购 → 赊购 → 生产 → 赊销 → 回款 →
//! 再借款 → 计息 → 付息。所有断言值为手算精确数（注释写元，执行值「分」），
//! 终态满足：试算平衡、资产 − 净负债 = 权益滚动、现金流三分类对账。
//!
//! 开局借款治理决策（task-7 review O2，本任务裁定）：开局 2001 余额以**隐式合同**
//! （id = `OPENING-DEBT`）在同一套计息/付息/授信机制下治理——构造时校验开局
//! 2001 贷方余额 == 配置本金（不匹配 → 类型化拒绝），授信占用自动包含开局债务
//! （容量 = 限额 − 开局 − 已登记未偿）。

use super::{acct, amt, base_config, cent_line, d, net_debit, yuan};
use engine::accounting::InventoryItemCode;
use engine::accounting::{AccountingAmount, AccountingPeriod, CashFlowClass, PostingSide};
use engine::company::industrial::{OpeningDebtTerms, Settlement};
use engine::company::{ContractId, CounterpartyId};

fn cp(id: &str) -> CounterpartyId {
    CounterpartyId(id.to_string())
}

fn raw() -> InventoryItemCode {
    InventoryItemCode("RAW".to_string())
}

fn fg() -> InventoryItemCode {
    InventoryItemCode("FG".to_string())
}

#[test]
fn gold_full_chain_order_production_credit_sale_collection_interest() {
    // 开局：现金 12000 元 = 实收资本 10000 元 + 短期借款 2000 元（OPENING-DEBT
    // 种子，年利率 400bp，到期 2031-06-30）。授信 5000 元（开局已占 2000 元）。
    let mut cfg = base_config();
    cfg.opening_lines = vec![
        cent_line(acct::BANK, PostingSide::Debit, 1_200_000),
        cent_line(acct::CAPITAL, PostingSide::Credit, 1_000_000),
        cent_line(acct::ST_DEBT, PostingSide::Credit, 200_000),
    ];
    cfg.opening_debt = Some(OpeningDebtTerms {
        lender: cp("EXT-BANK"),
        principal: yuan(2_000),
        annual_rate_bp: 400,
        maturity_date: d("2031-06-30"),
    });
    let mut co = engine::company::industrial::IndustrialBooks::new(cfg)
        .expect("opening with seeded debt must construct");

    // 1) 2030-01-05 现付采购原材料 100 件 @ 30 元（价外，Fixture 税率 13%）：
    //    货款 3000 元 + 进项税 390 元，现金流出 3390 元（全部可抵扣）。
    let purchase_cash = co
        .purchase(
            &cp("EXT-SUPP"),
            raw(),
            engine::accounting::LedgerAccountId(acct::RAW.to_string()),
            100,
            yuan(30),
            Settlement::Cash,
            d("2030-03-05"),
            d("2030-01-05"),
        )
        .expect("cash purchase");
    assert_eq!(purchase_cash.goods, yuan(3_000));
    assert_eq!(purchase_cash.input_vat.deductible, yuan(390));
    assert_eq!(
        purchase_cash.input_vat.non_deductible,
        AccountingAmount::ZERO
    );
    assert_eq!(co.inventory().quantity(&raw()), 100);
    assert_eq!(co.inventory().total_cost(&raw()), yuan(3_000));
    assert_eq!(net_debit(&co, acct::VAT_IN), yuan(390)); // 进项借方

    // 2) 2030-01-08 赊购原材料 100 件 @ 50 元：货款 5000 元 + 进项 650 元，
    //    应付账款 5650 元（到期 2030-02-28），**不动现金**。
    let cash_after_first = net_debit(&co, acct::BANK);
    let purchase_credit = co
        .purchase(
            &cp("EXT-SUPP"),
            raw(),
            engine::accounting::LedgerAccountId(acct::RAW.to_string()),
            100,
            yuan(50),
            Settlement::Credit,
            d("2030-02-28"),
            d("2030-01-08"),
        )
        .expect("credit purchase");
    assert_eq!(purchase_credit.goods, yuan(5_000));
    assert_eq!(purchase_credit.input_vat.deductible, yuan(650));
    assert_eq!(net_debit(&co, acct::BANK), cash_after_first);
    // 子账：RAW 200 件、成本 8000 元（移动加权平均单价 40 元）。
    assert_eq!(co.inventory().quantity(&raw()), 200);
    assert_eq!(co.inventory().total_cost(&raw()), yuan(8_000));
    assert_eq!(co.payables().total_open().expect("ap total"), yuan(5_650));
    assert_eq!(net_debit(&co, acct::VAT_IN), yuan(1_040));
    assert_eq!(
        purchase_credit.payable.as_ref().map(|id| id.0.clone()),
        Some(format!("AP-{}", purchase_credit.event.value()))
    );

    // 3) 2030-01-10 生产：领用原材料 150 件（移动加权成本 6000 元）+ 现金加工费
    //    1000 元，完工 10 件产成品（成本合计 7000 元，单位 700 元）。
    let production = co
        .produce(
            raw(),
            150,
            fg(),
            10,
            engine::accounting::LedgerAccountId(acct::FINISHED.to_string()),
            yuan(1_000),
            d("2030-01-10"),
        )
        .expect("production");
    assert_eq!(production.material_cost, yuan(6_000));
    assert_eq!(production.total_cost, yuan(7_000));
    // 子账对账：RAW 50 件 2000 元；FG 10 件 7000 元；在产品（5001）清零。
    assert_eq!(co.inventory().quantity(&raw()), 50);
    assert_eq!(co.inventory().total_cost(&raw()), yuan(2_000));
    assert_eq!(co.inventory().quantity(&fg()), 10);
    assert_eq!(co.inventory().total_cost(&fg()), yuan(7_000));
    assert_eq!(net_debit(&co, acct::RAW), yuan(2_000));
    assert_eq!(net_debit(&co, acct::FINISHED), yuan(7_000));
    assert_eq!(net_debit(&co, acct::WIP), AccountingAmount::ZERO);

    // 4) 2030-01-15 赊销产成品 8 件 @ 1200 元（价外）：收入 9600 元、销项税
    //    1248 元、销售成本 5600 元（加权 700 元/件）、应收 10848 元。
    //    **赊销不加现金**。
    let cash_before_sale = net_debit(&co, acct::BANK);
    let sale = co
        .sell_credit(
            &cp("EXT-CUST"),
            fg(),
            8,
            yuan(1_200),
            d("2030-02-15"),
            d("2030-01-15"),
        )
        .expect("credit sale");
    assert_eq!(sale.revenue, yuan(9_600));
    assert_eq!(sale.output_vat, yuan(1_248));
    assert_eq!(sale.cost_of_goods_sold, yuan(5_600));
    assert_eq!(net_debit(&co, acct::BANK), cash_before_sale);
    // 收入一次确认（6001 贷方 9600 元）；应收子账挂 10848 元；销项贷方 1248 元。
    assert_eq!(net_debit(&co, acct::REVENUE), yuan(-9_600));
    assert_eq!(net_debit(&co, acct::AR), yuan(10_848));
    assert_eq!(net_debit(&co, acct::VAT_OUT), yuan(-1_248));
    assert_eq!(
        co.receivables().total_open().expect("ar total"),
        yuan(10_848)
    );
    // FG 子账：2 件 1400 元；总账 1405 同额（跨层对账）。
    assert_eq!(co.inventory().quantity(&fg()), 2);
    assert_eq!(co.inventory().total_cost(&fg()), yuan(1_400));
    assert_eq!(net_debit(&co, acct::FINISHED), yuan(1_400));

    // 5) 2030-01-20 全额回款 10848 元：现金 +、应收清零，**收入不变（不重复计）**。
    co.collect(&sale.receivable, yuan(10_848), d("2030-01-20"))
        .expect("collection");
    assert_eq!(net_debit(&co, acct::REVENUE), yuan(-9_600));
    assert_eq!(net_debit(&co, acct::AR), AccountingAmount::ZERO);
    assert_eq!(
        co.receivables().total_open().expect("ar total"),
        AccountingAmount::ZERO
    );
    // 回款后现金 = 12000 − 3390 − 1000 + 10848 = 18458 元。
    assert_eq!(net_debit(&co, acct::BANK), yuan(18_458));

    // 6) 2030-01-25 再借款 2000 元 @ 500bp，到期 2030-07-25（181 天 ≤ 365 → 短期）。
    //    授信检查：开局 2000 + 新借 2000 = 4000 ≤ 5000（容量含开局债务）。
    co.borrow(
        ContractId("LOAN-2".to_string()),
        &cp("EXT-BANK"),
        yuan(2_000),
        500,
        d("2030-01-25"),
        d("2030-07-25"),
    )
    .expect("borrowing within credit");
    // 2001 是负债（贷方余额）：净借方 = −4000 元。
    assert_eq!(net_debit(&co, acct::ST_DEBT), yuan(-4_000));

    // 7) 2030-01-31 计息（ACT/365F，余数守恒）：
    //    OPENING-DEBT：200000 分 × 400bp × 31 天 / 3_650_000 = 679.452 → 679 分
    //      （余数 1_650_000 单位继续累计）；LOAN-2：200000 × 500 × 6 / 3_650_000
    //      = 164.383 → 164 分（余数 1_400_000）。均为非现金计提。
    let accruals = co
        .accrue_interest(d("2030-01-31"))
        .expect("interest accrual");
    assert_eq!(accruals.len(), 2);
    let opening = accruals
        .iter()
        .find(|item| item.contract.0 == "OPENING-DEBT")
        .expect("opening debt accrual");
    let loan2 = accruals
        .iter()
        .find(|item| item.contract.0 == "LOAN-2")
        .expect("loan-2 accrual");
    assert_eq!(opening.amount, AccountingAmount::from_cents(679));
    assert_eq!(opening.days, 31);
    assert_eq!(loan2.amount, AccountingAmount::from_cents(164));
    assert_eq!(loan2.days, 6);
    // 财务费用 6.79 + 1.64 = 8.43 元；应付利息贷方同额（非现金）。
    assert_eq!(net_debit(&co, acct::FIN_EXP), amt(843));
    assert_eq!(net_debit(&co, acct::INT_PAYABLE), amt(-843));

    // 8) 2030-01-31 结息：支付 OPENING-DEBT 已提未付利息 6.79 元（筹资活动现金）。
    co.pay_interest(&ContractId("OPENING-DEBT".to_string()), d("2030-01-31"))
        .expect("interest payment");
    assert_eq!(net_debit(&co, acct::INT_PAYABLE), amt(-164));

    // ===== 终态对账（全部手算精确值）=====
    // 现金 = 18458 + 2000 − 6.79 = 20451.21 元。
    assert_eq!(net_debit(&co, acct::BANK), amt(2_045_121));
    // 净利 = 9600 − 5600 − 8.43 = 3991.57 元。
    assert_eq!(co.books().ledger().net_income().expect("ni"), amt(399_157));
    // 净负债 = 借款 4000 + 应付 5650 + 销项 1248 − 进项 1040 + 应付利息 1.64
    //        = 9859.64 元（进项税额借方余额自然冲减，财会〔2016〕22号净额口径）。
    assert_eq!(
        co.books().ledger().liabilities_total().expect("liab"),
        amt(985_964)
    );
    // 权益滚动 = 10000 + 3991.57 = 13991.57 元。
    assert_eq!(
        co.books().ledger().equity_rolling().expect("equity"),
        amt(1_399_157)
    );
    // 资产（现金 20451.21 + 原材料 2000 + 库存商品 1400 = 23851.21）− 净负债
    // 9859.64 = 13991.57 元 = 权益滚动（资产负债表勾稽成立）。
    let trial = co.books().ledger().trial_balance().expect("trial");
    assert_eq!(trial.total_debits, trial.total_credits);
    // 现金流分类（2030-01）：经营 = −3390 − 1000 + 10848 = +6458 元；
    // 筹资 = +2000 − 6.79 = 1993.21 元；投资 = 0。
    let jan = AccountingPeriod::from_ymd(2030, 1).expect("period");
    assert_eq!(
        co.books()
            .ledger()
            .cash_flow_for_period(jan, CashFlowClass::Operating),
        yuan(6_458)
    );
    assert_eq!(
        co.books()
            .ledger()
            .cash_flow_for_period(jan, CashFlowClass::Financing),
        amt(199_321)
    );
    assert_eq!(
        co.books()
            .ledger()
            .cash_flow_for_period(jan, CashFlowClass::Investing),
        AccountingAmount::ZERO
    );
    // 期初（2029-12）筹资现金流 = 开局投入 12000 元。
    let dec = AccountingPeriod::from_ymd(2029, 12).expect("period");
    assert_eq!(
        co.books()
            .ledger()
            .cash_flow_for_period(dec, CashFlowClass::Financing),
        yuan(12_000)
    );

    // 借款子账：OPENING-DEBT 未提利息 0、余数 1_650_000 单位；LOAN-2 未提 164 分。
    let opening_loan = co
        .loan(&ContractId("OPENING-DEBT".to_string()))
        .expect("opening loan state");
    assert_eq!(opening_loan.accrued_unpaid(), AccountingAmount::ZERO);
    assert_eq!(opening_loan.carried().units(), 1_650_000);
    let loan2_state = co
        .loan(&ContractId("LOAN-2".to_string()))
        .expect("loan-2 state");
    assert_eq!(
        loan2_state.accrued_unpaid(),
        AccountingAmount::from_cents(164)
    );
    assert_eq!(loan2_state.carried().units(), 1_400_000);

    // 对手方资金流：供应商付出 3390；客户收 10848；银行 +2000 − 6.79。
    assert_eq!(
        co.counterparties()
            .net_position(&cp("EXT-SUPP"))
            .expect("supp net"),
        yuan(-3_390)
    );
    assert_eq!(
        co.counterparties()
            .net_position(&cp("EXT-CUST"))
            .expect("cust net"),
        yuan(10_848)
    );
    assert_eq!(
        co.counterparties()
            .net_position(&cp("EXT-BANK"))
            .expect("bank net"),
        amt(199_321)
    );
    assert_eq!(co.counterparties().flow_count(), 4);

    // 剩余授信 = 5000 − 开局 2000 − 已登记 2000 = 1000 元（容量含开局债务）。
    assert_eq!(
        co.available_credit(&cp("EXT-BANK")).expect("credit"),
        yuan(1_000)
    );

    // 9) 2030-02-28 结清赊购应付 5650 元（经营活动现金流出）。
    let ap_id = purchase_credit
        .payable
        .clone()
        .expect("credit purchase opens AP");
    co.settle_payable(&ap_id, d("2030-02-28"))
        .expect("payable settlement");
    assert_eq!(net_debit(&co, acct::PAYABLE), AccountingAmount::ZERO);
    assert_eq!(
        co.payables().total_open().expect("ap total"),
        AccountingAmount::ZERO
    );
    // 无逾期：应付在到期日结清。
    assert!(co.payables().overdue(d("2030-02-28")).is_empty());

    // 10) 2030-02-28 归还 OPENING-DEBT 本金 2000 元：还本前自动计提 2 月利息
    //     （28 天，衔接 1 月余数）：(200000×400×28 + 1650000)/3_650_000
    //     = 614.15 → 614 分（余数 550_000）——非现金；再还本（筹资现金流出）。
    let repayment = co
        .repay_principal(
            &ContractId("OPENING-DEBT".to_string()),
            yuan(2_000),
            d("2030-02-28"),
        )
        .expect("principal repayment with auto accrual");
    assert_eq!(repayment.events.len(), 2); // 计提分录 + 还本分录
    let opening_after = co
        .loan(&ContractId("OPENING-DEBT".to_string()))
        .expect("opening loan state");
    assert_eq!(opening_after.outstanding(), AccountingAmount::ZERO);
    assert_eq!(
        opening_after.accrued_unpaid(),
        AccountingAmount::from_cents(614)
    );
    assert_eq!(opening_after.carried().units(), 550_000);
    // 还本后：短期借款只剩 LOAN-2 的 2000 元；财务费用 8.43 + 6.14 = 14.57 元；
    // 应付利息 = 1.64（LOAN-2）+ 6.14（OPENING 未付）= 7.78 元。
    assert_eq!(net_debit(&co, acct::ST_DEBT), yuan(-2_000));
    assert_eq!(net_debit(&co, acct::FIN_EXP), amt(1_457));
    assert_eq!(net_debit(&co, acct::INT_PAYABLE), amt(-778));
    // 现金 = 20451.21 − 5650 − 2000 = 12801.21 元（2 月计提非现金，不动现金）。
    assert_eq!(net_debit(&co, acct::BANK), amt(1_280_121));
    // 净利 = 9600 − 5600 − 14.57 = 3985.43 元；权益滚动 = 13985.43 元；
    // 净负债 = 2000 + 1248 − 1040 + 7.78 = 2215.78 元；资产 − 净负债 = 权益。
    assert_eq!(co.books().ledger().net_income().expect("ni"), amt(398_543));
    assert_eq!(
        co.books().ledger().equity_rolling().expect("equity"),
        amt(1_398_543)
    );
    assert_eq!(
        co.books().ledger().liabilities_total().expect("liab"),
        amt(221_578)
    );
    let feb = AccountingPeriod::from_ymd(2030, 2).expect("period");
    assert_eq!(
        co.books()
            .ledger()
            .cash_flow_for_period(feb, CashFlowClass::Operating),
        yuan(-5_650)
    );
    assert_eq!(
        co.books()
            .ledger()
            .cash_flow_for_period(feb, CashFlowClass::Financing),
        yuan(-2_000)
    );
    // 还款释放授信：剩余 = 5000 − 0（开局已还清）− 2000（LOAN-2）= 3000 元。
    assert_eq!(
        co.available_credit(&cp("EXT-BANK")).expect("credit"),
        yuan(3_000)
    );

    // 序列化往返：整账套（账本 + 全部子账 + 借款状态）恢复后等价。
    let json = serde_json::to_string(&co).expect("serialize");
    let restored: engine::company::industrial::IndustrialBooks =
        serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored, co);
}
