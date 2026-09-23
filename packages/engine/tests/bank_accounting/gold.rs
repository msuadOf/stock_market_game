//! 银行全链金样：存入→手续费→贷出→双方计息→阶段转移（1→2→3）→收息→
//! 100% 计提→核销→回收→重估。全部数字手算钉死（分）。
//!
//! 手算依据：ACT/365F = 本金×bp×天数/3_650_000，整数半偶舍入 + 合同累计余数
//! （FractionUnits，1/3_650_000 分单位，任务 8 同一约定）。ECL 目标 =
//! rhe(Σ(权重×PD×LGD×账面余额)/10^12)——概率加权情景显式输入（Fixture）。
//! 第三阶段计息基数 = 账面余额 − 减值准备（CAS 22 净额法）。

use super::{acct, amt, base_config, d, net_debit, yuan};
use engine::company::bank::{BankBooks, BankProductKind, EclScenario, EclStage};
use engine::company::{ContractId, CounterpartyId};

fn dep_cp() -> CounterpartyId {
    CounterpartyId("EXT-DEP-1".to_string())
}

fn bor_cp() -> CounterpartyId {
    CounterpartyId("EXT-BOR-1".to_string())
}

/// 单一 100% 情景（核销前足额计提用：PD=LGD=100%）。
fn full_loss() -> Vec<EclScenario> {
    vec![EclScenario {
        weight_bp: 10_000,
        pd_bp: 10_000,
        lgd_bp: 10_000,
    }]
}

#[test]
fn bank_full_chain_deposit_loan_ecl_writeoff_recovery_gold() {
    let mut bank = BankBooks::new(base_config()).expect("bank opens");
    assert_eq!(net_debit(&bank, acct::CASH), yuan(2_000));

    // ── 2030-01-02：存入 1000.00 元 @150bp（181 天 → 短期 2011）──
    // K3 红线：贷客户存款（负债），不是收入；现金 2000→3000。
    bank.accept_deposit(
        BankProductKind::TermDeposit,
        ContractId("D1".to_string()),
        &dep_cp(),
        yuan(1_000),
        150,
        d("2030-01-02"),
        d("2030-07-02"),
    )
    .expect("accept deposit");
    assert_eq!(net_debit(&bank, acct::ST_DEPOSIT), yuan(-1_000));
    assert_eq!(net_debit(&bank, acct::INTEREST_INCOME), yuan(0));
    assert_eq!(net_debit(&bank, acct::CASH), yuan(3_000));

    // ── 2030-01-03：手续费收入 5.00 元（服务收现）──
    bank.earn_fee(&dep_cp(), yuan(5), d("2030-01-03"))
        .expect("fee income");
    assert_eq!(net_debit(&bank, acct::FEE_INCOME), yuan(-5));
    assert_eq!(net_debit(&bank, acct::CASH), yuan(3_005));

    // ── 2030-01-05：贷出 600.00 元 @600bp（181 天）+ 初始 12 个月 ECL ──
    // K3 红线：转贷款资产（1301），不是费用；初始准备 = 600×1%×50% = 3.00。
    bank.issue_loan(
        BankProductKind::TermLoan,
        ContractId("L1".to_string()),
        &bor_cp(),
        yuan(600),
        600,
        d("2030-01-05"),
        d("2030-07-05"),
    )
    .expect("issue loan with day-one ECL");
    assert_eq!(net_debit(&bank, acct::LOAN_PRINCIPAL), yuan(600));
    assert_eq!(net_debit(&bank, acct::CASH), yuan(2_405));
    assert_eq!(net_debit(&bank, acct::LOAN_ALLOWANCE), amt(-300)); // 3.00 元贷方
    assert_eq!(net_debit(&bank, acct::CREDIT_IMPAIR), amt(300));
    assert_eq!(
        bank.loan(&ContractId("L1".to_string())).unwrap().stage(),
        EclStage::Stage1
    );

    // ── 2030-02-01：双方计息（贷款 27 天 / 存款 30 天，毛额法）──
    // 贷款：60000×600×27 = 972,000,000 /3_650_000 = 266.30 → 266 分
    //      （余 972,000,000 − 266×3,650,000 = 1,100,000 单位）。
    // 存款：100000×150×30 = 450,000,000 /3_650_000 = 123.29 → 123 分（余 1,050,000）。
    let loan_items = bank
        .accrue_loan_interest(d("2030-02-01"))
        .expect("loan accrual");
    assert_eq!(loan_items.len(), 1);
    assert_eq!(loan_items[0].amount, amt(266));
    assert_eq!(loan_items[0].days, 27);
    assert_eq!(loan_items[0].remaining_carried.units(), 1_100_000);
    let dep_items = bank
        .accrue_deposit_interest(d("2030-02-01"))
        .expect("deposit accrual");
    assert_eq!(dep_items.len(), 1);
    assert_eq!(dep_items[0].amount, amt(123));
    assert_eq!(dep_items[0].days, 30);
    assert_eq!(dep_items[0].remaining_carried.units(), 1_050_000);
    assert_eq!(net_debit(&bank, acct::LOAN_INT_RCV), amt(266));
    assert_eq!(net_debit(&bank, acct::INTEREST_INCOME), amt(-266));
    assert_eq!(net_debit(&bank, acct::DEP_INT_PAYABLE), amt(-123));
    assert_eq!(net_debit(&bank, acct::INTEREST_EXPENSE), amt(123));
    assert_eq!(net_debit(&bank, acct::CASH), yuan(2_405)); // 计息非现金

    // ── 2030-02-10：阶段 1→2（信用风险显著增加 → 存续期 ECL）──
    // 账面余额 = 60000 + 266 = 60266；目标 = 60266×8%×50% = 2410.64 → 2411；
    // 补提 2411 − 300 = 2111。
    bank.assess_credit(
        &ContractId("L1".to_string()),
        d("2030-02-10"),
        EclStage::Stage2,
        "credit risk significantly increased",
        bank.ecl_policy().lifetime_default.clone(),
    )
    .expect("stage 1->2 transfer");
    assert_eq!(net_debit(&bank, acct::LOAN_ALLOWANCE), amt(-2_411));
    assert_eq!(net_debit(&bank, acct::CREDIT_IMPAIR), amt(2_411));

    // ── 2030-02-20：收妥贷款利息 2.66 元（不重复计收入）──
    bank.collect_loan_interest(&ContractId("L1".to_string()), amt(266), d("2030-02-20"))
        .expect("collect loan interest");
    assert_eq!(net_debit(&bank, acct::LOAN_INT_RCV), amt(0));
    assert_eq!(net_debit(&bank, acct::INTEREST_INCOME), amt(-266));
    assert_eq!(net_debit(&bank, acct::CASH), amt(240_766)); // 2405 + 2.66

    // ── 2030-03-01：阶段 2→3（已发生信用减值）──
    // 账面余额 = 60000；目标 = 60000×4% = 2400 < 2411 → 转回 11 分。
    bank.assess_credit(
        &ContractId("L1".to_string()),
        d("2030-03-01"),
        EclStage::Stage3,
        "credit-impaired (90 days past due)",
        bank.ecl_policy().lifetime_default.clone(),
    )
    .expect("stage 2->3 transfer");
    assert_eq!(net_debit(&bank, acct::LOAN_ALLOWANCE), amt(-2_400));
    assert_eq!(net_debit(&bank, acct::CREDIT_IMPAIR), amt(2_400));

    // ── 2030-03-01：第三阶段净额法计息（基数 = 60000 − 2400 = 57600）──
    // 57600×600×28 = 967,680,000 + 前余 1,100,000 = 968,780,000；
    // /3_650_000 = 265.42 → 265 分（余 968,780,000 − 265×3,650,000 = 1,530,000）。
    let loan_items = bank
        .accrue_loan_interest(d("2030-03-01"))
        .expect("net accrual");
    assert_eq!(loan_items[0].amount, amt(265));
    assert_eq!(loan_items[0].remaining_carried.units(), 1_530_000);
    let dep_items = bank
        .accrue_deposit_interest(d("2030-03-01"))
        .expect("deposit accrual 2");
    // 存款：100000×150×28 = 420,000,000 + 1,050,000 = 421,050,000 → 115 分
    // （余 421,050,000 − 115×3,650,000 = 1,300,000）。
    assert_eq!(dep_items[0].amount, amt(115));
    assert_eq!(dep_items[0].remaining_carried.units(), 1_300_000);
    assert_eq!(net_debit(&bank, acct::LOAN_INT_RCV), amt(265));
    assert_eq!(net_debit(&bank, acct::DEP_INT_PAYABLE), amt(-238)); // 123+115

    // ── 2030-03-15：支付存款利息 2.38 元（现金出，经营）──
    bank.pay_deposit_interest(&ContractId("D1".to_string()), d("2030-03-15"))
        .expect("pay deposit interest");
    assert_eq!(net_debit(&bank, acct::DEP_INT_PAYABLE), amt(0));
    assert_eq!(net_debit(&bank, acct::INTEREST_EXPENSE), amt(238));
    assert_eq!(net_debit(&bank, acct::CASH), amt(240_528)); // 2407.66 − 2.38

    // ── 2030-04-01：违约重估 100% 情景（显式情景输入）──
    // 账面余额 = 60000+265 = 60265；目标 = 60265 → 补提 60265−2400 = 57865。
    bank.assess_credit(
        &ContractId("L1".to_string()),
        d("2030-04-01"),
        EclStage::Stage3,
        "default - measure at 100%",
        full_loss(),
    )
    .expect("full provision");
    assert_eq!(net_debit(&bank, acct::LOAN_ALLOWANCE), amt(-60_265));
    assert_eq!(net_debit(&bank, acct::CREDIT_IMPAIR), amt(60_265));

    // ── 2030-04-05：核销（准备足额覆盖账面余额 60265）──
    bank.write_off(&ContractId("L1".to_string()), d("2030-04-05"))
        .expect("write off fully provisioned loan");
    assert_eq!(net_debit(&bank, acct::LOAN_PRINCIPAL), amt(0));
    assert_eq!(net_debit(&bank, acct::LOAN_INT_RCV), amt(0));
    assert_eq!(net_debit(&bank, acct::LOAN_ALLOWANCE), amt(0));
    let loan = bank.loan(&ContractId("L1".to_string())).unwrap();
    assert!(loan.is_written_off());
    assert_eq!(loan.recoverable(), amt(60_265));

    // ── 2030-04-20：回收 100.00 元（Dr 现金 / Cr 贷款损失准备）──
    bank.recover_written_off(&ContractId("L1".to_string()), yuan(100), d("2030-04-20"))
        .expect("recovery");
    assert_eq!(net_debit(&bank, acct::CASH), amt(250_528)); // 2505.28 元
    assert_eq!(net_debit(&bank, acct::LOAN_ALLOWANCE), amt(-10_000));
    assert_eq!(
        bank.loan(&ContractId("L1".to_string()))
            .unwrap()
            .recoverable(),
        amt(50_265)
    );

    // ── 2030-04-21：回收后重估（账面余额 0 → 准备转回 100.00，利得）──
    bank.assess_credit(
        &ContractId("L1".to_string()),
        d("2030-04-21"),
        EclStage::Stage3,
        "post-recovery re-measure",
        bank.ecl_policy().lifetime_default.clone(),
    )
    .expect("post-recovery reversal");
    assert_eq!(net_debit(&bank, acct::LOAN_ALLOWANCE), amt(0));
    assert_eq!(net_debit(&bank, acct::CREDIT_IMPAIR), amt(50_265)); // 502.65 元

    // ── 终态对账（分）──
    assert_eq!(net_debit(&bank, acct::CASH), amt(250_528));
    assert_eq!(net_debit(&bank, acct::ST_DEPOSIT), amt(-100_000));
    assert_eq!(net_debit(&bank, acct::CAPITAL), amt(-200_000));
    assert_eq!(net_debit(&bank, acct::INTEREST_INCOME), amt(-531)); // 5.31
    assert_eq!(net_debit(&bank, acct::FEE_INCOME), amt(-500)); // 5.00
    assert_eq!(net_debit(&bank, acct::INTEREST_EXPENSE), amt(238)); // 2.38
    assert_eq!(net_debit(&bank, acct::CREDIT_IMPAIR), amt(50_265)); // 502.65
                                                                    // 净利 = 5.31 − 2.38 + 5.00 − 502.65 = −494.72 元；权益滚动 =
                                                                    // 2000 − 494.72 = 1505.28 元；资产 2505.28 = 存款 1000 + 权益 1505.28。
    let net_income = amt(531 - 238 + 500 - 50_265);
    assert_eq!(net_income, amt(-49_472));
    let assets = net_debit(&bank, acct::CASH)
        .add(net_debit(&bank, acct::LOAN_PRINCIPAL))
        .expect("assets")
        .add(net_debit(&bank, acct::LOAN_INT_RCV))
        .expect("assets")
        .add(net_debit(&bank, acct::LOAN_ALLOWANCE))
        .expect("assets");
    // 权益滚动（贷方为正）= 实收资本贷方 2000 + 净利 −494.72 = 1505.28 元。
    let equity_rolling = net_debit(&bank, acct::CAPITAL)
        .neg()
        .expect("capital credit")
        .add(net_income)
        .expect("equity");
    assert_eq!(equity_rolling, amt(150_528));
    let liabilities_and_equity = net_debit(&bank, acct::ST_DEPOSIT)
        .neg()
        .expect("deposit credit")
        .add(equity_rolling)
        .expect("L+E");
    assert_eq!(assets, liabilities_and_equity);

    // 阶段转移史：1→2（02-10）→3（03-01），理由保留。
    let transfers = bank
        .loan(&ContractId("L1".to_string()))
        .unwrap()
        .stage_transfers();
    assert_eq!(transfers.len(), 2);
    assert_eq!(transfers[0].to_stage, EclStage::Stage2);
    assert_eq!(transfers[0].date, d("2030-02-10"));
    assert_eq!(transfers[1].to_stage, EclStage::Stage3);
    assert_eq!(transfers[1].reason, "credit-impaired (90 days past due)");

    // ── 报表分类层（CAS 30 (2026) 银行列示；任务 13 消费的分类面）──
    let lines = bank.presentation_lines().expect("presentation lines");
    assert_eq!(lines.interest_income, amt(531));
    assert_eq!(lines.interest_expense, amt(238));
    assert_eq!(lines.net_interest_income, amt(293)); // 2.93 元
    assert_eq!(lines.fee_and_commission_income, amt(500));
    assert_eq!(lines.credit_impairment_loss, amt(50_265));
    assert_eq!(lines.loans_and_advances_gross, amt(0));
    assert_eq!(lines.loan_loss_allowance, amt(0));
    assert_eq!(lines.loans_and_advances_net, amt(0));
    assert_eq!(lines.customer_deposits, amt(100_000));
    assert_eq!(lines.cash_position, amt(250_528));

    // ── 存档往返：重放路径恢复等价账套（K2 事实/派生边界）──
    let json = serde_json::to_string(&bank).expect("serialize");
    let restored: BankBooks = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored, bank);
}

#[test]
fn deposit_interest_stops_accruing_at_maturity() {
    // 定期存款到期后停息：计提日越到期日只计至到期日（显式规则，不自动转存）。
    let mut bank = BankBooks::new(base_config()).expect("bank opens");
    bank.accept_deposit(
        BankProductKind::TermDeposit,
        ContractId("D1".to_string()),
        &CounterpartyId("EXT-DEP-1".to_string()),
        yuan(1_000),
        150,
        d("2030-01-01"),
        d("2030-01-11"), // 10 天期
    )
    .expect("deposit");
    // 计提至 2030-02-01：只计 10 天 = 100000×150×10/3_650_000 = 41.09 → 41。
    let items = bank
        .accrue_deposit_interest(d("2030-02-01"))
        .expect("accrue capped at maturity");
    assert_eq!(items[0].days, 10);
    assert_eq!(items[0].amount, amt(41));
    // 再计提无新增（到期后天数 0，自然跳过）。
    let again = bank
        .accrue_deposit_interest(d("2030-03-01"))
        .expect("no further accrual");
    assert!(again.is_empty());
}

#[test]
fn unlisted_bank_test_entity_adds_no_default_stock() {
    // 默认集合恰含一家未上市银行实体；发行映射只覆盖上市工商公司映射的
    // 股票（股票清单直接由默认规格推导）——银行不增加默认股票（K3：银行
    // 无默认股票也必须可跑通经营与报表查询）。
    let companies = engine::company::default_companies(d("2030-01-01")).expect("default companies");
    let bank_configs: Vec<&engine::company::CompanyConfig> = companies
        .iter()
        .filter(|config| config.spec.kind == engine::company::CompanyKind::Bank)
        .collect();
    assert_eq!(bank_configs.len(), 1);
    assert_eq!(bank_configs[0].spec.id.0, "C-TEST-BANK");
    assert!(bank_configs[0].spec.listed_stock.is_none());
    let registry = engine::company::CompanyRegistry::new(companies).expect("registry builds");
    let stocks: Vec<(engine::account::StockCode, u64)> = registry
        .iter()
        .filter_map(|(_, company)| {
            company
                .spec()
                .listed_stock
                .as_ref()
                .map(|stock| (stock.clone(), company.spec().issued_shares))
        })
        .collect();
    // 5 家上市工商公司 → 恰好 5 只股票；银行实体不在发行映射中。
    assert_eq!(stocks.len(), 5);
    assert!(stocks
        .iter()
        .all(|(stock, _)| registry.issuer_of(stock).is_some()));
    registry
        .validate_issuer_mapping(&stocks)
        .expect("bank adds no stock");
}

#[test]
fn counterparties_track_cross_boundary_fund_flows() {
    // K2：跨模拟边界资金流逐笔登记（存款入/贷款出/收息/付息）。
    let mut bank = BankBooks::new(base_config()).expect("bank opens");
    bank.accept_deposit(
        BankProductKind::TermDeposit,
        ContractId("D1".to_string()),
        &CounterpartyId("EXT-DEP-1".to_string()),
        yuan(1_000),
        150,
        d("2030-01-02"),
        d("2030-07-02"),
    )
    .expect("deposit");
    bank.issue_loan(
        BankProductKind::TermLoan,
        ContractId("L1".to_string()),
        &CounterpartyId("EXT-BOR-1".to_string()),
        yuan(600),
        600,
        d("2030-01-05"),
        d("2030-07-05"),
    )
    .expect("loan");
    // 日终 ECL 计提是 NonCash，不记资金流：资金流 = 存款 Inbound + 贷款 Outbound。
    let flows = bank.counterparties().flows();
    assert_eq!(flows.len(), 2);
    assert_eq!(flows[0].amount, yuan(1_000));
    assert_eq!(flows[0].direction, engine::company::FlowDirection::Inbound);
    assert_eq!(flows[1].amount, yuan(600));
    assert_eq!(flows[1].direction, engine::company::FlowDirection::Outbound);
}
