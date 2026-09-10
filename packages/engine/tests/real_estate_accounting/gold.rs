//! 全链金样（单场景）：购地 → 开发（含预售签约/收款）→ 计提（正常资本化）→
//! 暂停（≥90 日中断 → 资本化暂停，利息费用化）→ 复工 → 完工（资本化终止）→
//! 交付（冲合同负债 + 挂应收尾款 + 确认收入 + 结转成本）→ 尾款回收 →
//! 减值 → 付息 → 还本 → 终态勾稽 + 列报 + serde 往返。
//!
//! 手算基线（分；利率 730bp、本金 10000 元 ⇒ 每日利息恰 200 分，无余数）：
//! - 资本化：89d（1/1–3/30）=17,800 + 16d（7/15–7/30）=3,200 + 1d（7/31）=200
//!   ⇒ 合计 21,200；
//! - 费用化：91d（3/31–6/29，开放中断 T−S=91≥90）=18,200 + 15d（6/30–7/14，
//!   已闭合长中断）=3,000 + 31d（8/1–8/31，完工后）=6,200 ⇒ 合计 27,400；
//! - 利息总额 48,600 = 243d × 200（89+91+31+1+31 = 243 ✓）。
//! - 项目成本 = 土地 1,200,000 + 开发 800,000 + 资本化利息 21,200 = 2,021,200；
//!   交付 6/10 套 ⇒ 结转 rhe(2,021,200×6/10) = 1,212,720，余 808,480。
//! - 终态：现金 3,951,400；开发存货 808,480 − 减值准备 108,480；净利
//!   3,000,000 − 1,212,720 − 27,400 − 108,480 = 1,651,400；资产 4,651,400
//!   = 负债 0 + 权益 4,651,400 ✓。

use super::{acct, amt, base_config, d, net_debit, yuan};
use engine::accounting::reports::real_estate::real_estate_presentation_lines;
use engine::company::real_estate::{InterestSplitItem, ProjectId, RealEstateBooks};
use engine::company::ContractId;
use engine::company::CounterpartyId;

const LAND: &str = "EXT-LAND-1";
const CON: &str = "EXT-CON-1";
const BUY: &str = "EXT-BUY-1";
const LEND: &str = "EXT-LEND-1";

fn pid() -> ProjectId {
    ProjectId("P-1".to_string())
}

fn cid() -> ContractId {
    ContractId("C-1".to_string())
}

/// 计提项一致性：天数拆分完备、金额与手算基线精确相等。
fn assert_split(item: &InterestSplitItem, days: i64, cap: i64, cap_amt: i128, exp_amt: i128) {
    assert_eq!(item.days, days);
    assert_eq!(item.capitalized_days, cap);
    assert_eq!(item.expensed_days, days - cap);
    assert_eq!(item.capitalized_amount, amt(cap_amt));
    assert_eq!(item.expensed_amount, amt(exp_amt));
}

#[test]
fn chain_gold_land_develop_presale_suspend_deliver_wind_down() {
    let mut re = RealEstateBooks::new(base_config()).expect("assembly");
    let mut last_event: u64 = 1; // 开局凭证之后事件 id 单调
    let track = |id: u64, last: &mut u64| {
        assert!(*last < id, "event ids must be strictly monotonic");
        *last = id;
    };

    // —— 购地（1/1）：10 套，土地成本 12000 元 ——
    let e = re
        .acquire_land(
            pid(),
            &CounterpartyId(LAND.to_string()),
            10,
            yuan(12_000),
            d("2030-01-01"),
        )
        .expect("land");
    track(e.value(), &mut last_event);
    assert_eq!(net_debit(&re, acct::DEV_INVENTORY), yuan(12_000));
    assert_eq!(net_debit(&re, acct::CASH), yuan(18_000));

    // —— 项目借款（1/1）：10000 元 @730bp，2031-06-30 到期（长期 2501）——
    let e = re
        .borrow_project_loan(
            cid(),
            &CounterpartyId(LEND.to_string()),
            yuan(10_000),
            730,
            d("2030-01-01"),
            d("2031-06-30"),
            Some(pid()),
        )
        .expect("borrow");
    track(e.value(), &mut last_event);
    assert_eq!(net_debit(&re, acct::LT_DEBT).neg().unwrap(), yuan(10_000));
    assert_eq!(net_debit(&re, acct::CASH), yuan(28_000));

    // —— 开发成本（1/1 起工 6000 元；2/15 追加 2000 元）——
    let e = re
        .incur_development(
            &pid(),
            &CounterpartyId(CON.to_string()),
            yuan(6_000),
            d("2030-01-01"),
        )
        .expect("dev 1");
    track(e.value(), &mut last_event);
    assert_eq!(
        re.project(&pid()).expect("project").dev_started_on(),
        Some(d("2030-01-01"))
    );

    // —— 预售：签约 6 套、总价 30000 元；收款 18000 元（不是收入！）——
    re.sign_presale(
        cid(),
        &pid(),
        &CounterpartyId(BUY.to_string()),
        6,
        yuan(30_000),
        d("2030-02-10"),
    )
    .expect("sign presale");
    let e = re
        .collect_presale(&cid(), yuan(18_000), d("2030-02-10"))
        .expect("presale collect");
    track(e.value(), &mut last_event);
    assert_eq!(
        net_debit(&re, acct::CONTRACT_LIAB).neg().unwrap(),
        yuan(18_000)
    );
    // K3 红线：预售收款一分钱都不进收入。
    assert_eq!(net_debit(&re, acct::REVENUE), amt(0));

    let e = re
        .incur_development(
            &pid(),
            &CounterpartyId(CON.to_string()),
            yuan(2_000),
            d("2030-02-15"),
        )
        .expect("dev 2");
    track(e.value(), &mut last_event);
    assert_eq!(net_debit(&re, acct::CASH), yuan(38_000));

    // —— 计提 1（through 3/31）：89d 全部在资本化窗口内 ——
    let items = re.accrue_interest(d("2030-03-31")).expect("accrue 1");
    assert_eq!(items.len(), 1);
    assert_split(&items[0], 89, 89, 17_800, 0);
    assert_eq!(net_debit(&re, acct::DEV_INVENTORY), amt(2_017_800));
    assert_eq!(net_debit(&re, acct::FIN_EXP), amt(0));
    assert_eq!(
        net_debit(&re, acct::INT_PAYABLE).neg().unwrap(),
        amt(17_800)
    );

    // —— 暂停开发（3/31 起中断）——
    re.suspend_development(&pid(), d("2030-03-31"))
        .expect("suspend");

    // —— 计提 2（through 6/30）：开放中断 T−S = 91 ≥ 90 ⇒ 91d 全费用化 ——
    let items = re.accrue_interest(d("2030-06-30")).expect("accrue 2");
    assert_split(&items[0], 91, 0, 0, 18_200);
    // 资本化暂停：资产不涨，利息进损益。
    assert_eq!(net_debit(&re, acct::DEV_INVENTORY), amt(2_017_800));
    assert_eq!(net_debit(&re, acct::FIN_EXP), amt(18_200));

    // —— 复工（7/15）：闭合长中断 [3/31, 7/15)，106d ≥ 90 ——
    re.resume_development(&pid(), d("2030-07-15"))
        .expect("resume");

    // —— 计提 3（through 7/31）：中断内 15d 费用化 + 复工后 16d 资本化 ——
    let items = re.accrue_interest(d("2030-07-31")).expect("accrue 3");
    assert_split(&items[0], 31, 16, 3_200, 3_000);
    assert_eq!(net_debit(&re, acct::DEV_INVENTORY), amt(2_021_000));
    assert_eq!(net_debit(&re, acct::FIN_EXP), amt(21_200));

    // —— 完工（8/1）：资本化终止 ——
    re.complete_project(&pid(), d("2030-08-01"))
        .expect("complete");
    assert_eq!(
        re.project(&pid()).unwrap().completed_on(),
        Some(d("2030-08-01"))
    );

    // —— 计提 4（through 8/1）：7/31 单日仍资本化（早于完工日）——
    let items = re.accrue_interest(d("2030-08-01")).expect("accrue 4");
    assert_split(&items[0], 1, 1, 200, 0);
    assert_eq!(net_debit(&re, acct::DEV_INVENTORY), amt(2_021_200));

    // —— 交付（8/10）：冲合同负债 + 挂应收尾款 + 收入 + 结转成本 ——
    let outcome = re.deliver(&cid(), d("2030-08-10")).expect("deliver");
    track(outcome.event.value(), &mut last_event);
    assert_eq!(outcome.revenue, yuan(30_000));
    assert_eq!(outcome.liability_cleared, yuan(18_000));
    assert_eq!(outcome.receivable_amount, yuan(12_000));
    assert_eq!(outcome.cost_of_sales, amt(1_212_720));
    assert_eq!(net_debit(&re, acct::CONTRACT_LIAB), amt(0));
    assert_eq!(net_debit(&re, acct::AR), yuan(12_000));
    assert_eq!(net_debit(&re, acct::REVENUE).neg().unwrap(), yuan(30_000));
    assert_eq!(net_debit(&re, acct::COGS), amt(1_212_720));
    assert_eq!(net_debit(&re, acct::DEV_INVENTORY), amt(808_480));
    assert_eq!(re.project(&pid()).unwrap().remaining_units(), 4);

    // —— 尾款回收（8/20）：只清应收，不重复计收入 ——
    let e = re
        .collect_final(
            &outcome.receivable.clone().expect("ar item"),
            yuan(12_000),
            d("2030-08-20"),
        )
        .expect("final collect");
    track(e.value(), &mut last_event);
    assert_eq!(net_debit(&re, acct::AR), amt(0));
    assert_eq!(net_debit(&re, acct::REVENUE).neg().unwrap(), yuan(30_000));

    // —— 减值（8/25）：可变现净值 7000 元 ⇒ 补提 108,480 分 ——
    let e = re
        .update_inventory_impairment(yuan(7_000), d("2030-08-25"))
        .expect("impairment")
        .expect("impairment posts an entry");
    track(e.value(), &mut last_event);
    assert_eq!(
        net_debit(&re, acct::DEV_IMPAIR_ALLOW).neg().unwrap(),
        amt(108_480)
    );
    assert_eq!(net_debit(&re, acct::IMPAIR_LOSS), amt(108_480));

    // —— 计提 5（through 9/1）：完工后 31d 全费用化（不无限资本化红线）——
    let items = re.accrue_interest(d("2030-09-01")).expect("accrue 5");
    assert_split(&items[0], 31, 0, 0, 6_200);
    assert_eq!(net_debit(&re, acct::DEV_INVENTORY), amt(808_480));
    assert_eq!(net_debit(&re, acct::FIN_EXP), amt(27_400));

    // —— 付息（9/20）：48,600 分 = 243d × 200 ——
    let e = re
        .pay_interest(&cid(), d("2030-09-20"))
        .expect("pay interest");
    track(e.value(), &mut last_event);
    assert_eq!(net_debit(&re, acct::INT_PAYABLE), amt(0));
    assert_eq!(net_debit(&re, acct::CASH), amt(4_951_400));

    // —— 还本（10/1）——
    let e = re
        .repay_principal(&cid(), yuan(10_000), d("2030-10-01"))
        .expect("repay");
    track(e.value(), &mut last_event);
    assert_eq!(net_debit(&re, acct::LT_DEBT), amt(0));
    assert_eq!(net_debit(&re, acct::CASH), amt(3_951_400));

    // ===== 终态勾稽 =====
    // 现金：3,000,000 + 1,000,000 − 1,200,000 − 800,000 + 1,800,000
    //       + 1,200,000 − 48,600 − 1,000,000 = 3,951,400
    let cash_flows =
        3_000_000 + 1_000_000 - 1_200_000 - 800_000 + 1_800_000 + 1_200_000 - 48_600 - 1_000_000;
    assert_eq!(net_debit(&re, acct::CASH), amt(cash_flows));
    // 项目余额 ↔ 1541：Σ项目结存成本 == 开发存货科目余额。
    assert_eq!(re.development_inventory_total().expect("sum"), amt(808_480));
    assert_eq!(
        re.development_inventory_total().unwrap(),
        net_debit(&re, acct::DEV_INVENTORY)
    );
    // 守恒：Σ结转成本 + 期末结存 == Σ投入成本（土地 + 开发 + 资本化利息）。
    let p = re.project(&pid()).unwrap();
    assert_eq!(p.carried_out_cost(), amt(1_212_720));
    assert_eq!(
        p.carried_out_cost().add(p.remaining_cost()).unwrap(),
        p.land_cost()
            .add(p.development_cost())
            .unwrap()
            .add(p.capitalized_interest())
            .unwrap()
    );
    // 应收/合同负债清零；净利 = 收入 − 成本 − 费用化利息 − 减值。
    assert_eq!(net_debit(&re, acct::AR), amt(0));
    assert_eq!(net_debit(&re, acct::CONTRACT_LIAB), amt(0));
    let ni = net_debit(&re, acct::REVENUE)
        .neg()
        .unwrap()
        .sub(net_debit(&re, acct::COGS))
        .unwrap()
        .sub(net_debit(&re, acct::FIN_EXP))
        .unwrap()
        .sub(net_debit(&re, acct::IMPAIR_LOSS))
        .unwrap();
    assert_eq!(ni, amt(1_651_400));
    // 资产 = 负债 + 权益（资产 4,651,400 = 实收资本 3,000,000 + 净利 1,651,400）。
    let assets = net_debit(&re, acct::CASH)
        .add(net_debit(&re, acct::DEV_INVENTORY))
        .unwrap()
        .add(net_debit(&re, acct::DEV_IMPAIR_ALLOW))
        .unwrap();
    assert_eq!(assets, amt(4_651_400));
    assert_eq!(
        assets,
        net_debit(&re, acct::CAPITAL)
            .neg()
            .unwrap()
            .add(ni)
            .unwrap()
    );

    // ===== 列报分类层 =====
    let lines = real_estate_presentation_lines(re.books().ledger()).expect("presentation");
    assert_eq!(lines.development_inventory_gross, amt(808_480));
    assert_eq!(lines.development_inventory_impairment, amt(108_480));
    assert_eq!(lines.development_inventory_net, amt(700_000));
    assert_eq!(lines.contract_liabilities, amt(0));
    assert_eq!(lines.final_payment_receivable, amt(0));
    assert_eq!(lines.cash_position, amt(3_951_400));
    assert_eq!(lines.operating_revenue, yuan(30_000));
    assert_eq!(lines.operating_cost, amt(1_212_720));
    assert_eq!(lines.finance_cost, amt(27_400));
    assert_eq!(lines.impairment_loss, amt(108_480));

    // ===== 利息链守恒：Σ(资本化 21,200 + 费用化 27,400) = 48,600，余数 0 =====
    let loan = re.loan(&cid()).expect("loan");
    assert_eq!(loan.accrued_unpaid(), amt(0));
    assert_eq!(loan.carried_cap().units(), 0);
    assert_eq!(loan.carried_exp().units(), 0);

    // ===== serde 往返（字节一致 + 相等）=====
    let bytes = serde_json::to_vec(&re).expect("serialize");
    let restored: RealEstateBooks = serde_json::from_slice(&bytes).expect("deserialize");
    assert_eq!(restored, re);
}
