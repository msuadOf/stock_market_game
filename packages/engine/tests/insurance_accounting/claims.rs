//! 保险金样（三）：赔案发生与支付分离 + 调节表对账（任务验收硬性要求）。
//!
//! 调节表（分）恒等式全部精确断言：
//! 1. LRC：期初 + 保费入账 + 首日亏损 + 重估过账（财务+亏损成分） + 财务回拨
//!    − 释放收入 = 期末（账套 LRC 余额的逐笔滚动）；
//! 2. LIC：期初 + 赔案发生 − 赔款支付 = 期末；
//! 3. CSM：初始 + 再计量 − 释放 = 期末（子账成分）；
//! 4. 现金：期初 + 保费收讫 − 赔款支付 = 期末；
//! 5. 损益滚动：释放收入 − 赔案费用 − 财务损益 ± 亏损成分 = 保费 − 已发生赔付
//!    （经济恒等式，权责发生制与收付实现制对账）。

use super::{acct, amt, base_config, d, net_debit, policyholder, yuan};
use engine::company::insurance::{ClaimId, InsuranceBooks, InsuranceProductKind};
use engine::company::ContractId;

fn group_id() -> ContractId {
    ContractId("GRP-A".to_string())
}

#[test]
fn claims_occurrence_and_payment_are_separated_and_reconcile() {
    let mut ins = InsuranceBooks::new(base_config()).expect("insurer opens");
    ins.establish_group(
        InsuranceProductKind::TermProtection,
        group_id(),
        &policyholder(),
        yuan(1_000),
        yuan(800),
        yuan(50),
        d("2030-01-01"),
        d("2031-01-01"),
    )
    .expect("establish");
    ins.collect_premium(&group_id(), yuan(1_000), d("2030-01-01"))
        .expect("collect premium");
    ins.release_service(&group_id(), 100, d("2030-04-11"))
        .expect("first release");
    // 重估 ΔE=+8_000（265 天）：ΔPV=7_774 由 CSM 吸收（不过账）；
    // ΔF=226 过 6541。LRC = 72_602+226 = 72_828（锚点见 remeasure.rs）。
    ins.remeasure(&group_id(), d("2030-06-01"), amt(66_082))
        .expect("remeasure")
        .expect("posts");

    // ── 两笔赔案先后发生（负债确认，不动现金）──
    ins.record_claim(
        &group_id(),
        ClaimId("CLM-100".to_string()),
        yuan(500),
        d("2030-07-01"),
    )
    .expect("first claim occurs");
    ins.record_claim(
        &group_id(),
        ClaimId("CLM-101".to_string()),
        yuan(300),
        d("2030-07-01"),
    )
    .expect("second claim occurs");
    assert_eq!(net_debit(&ins, acct::LIC), amt(-80_000));
    assert_eq!(net_debit(&ins, acct::INSURANCE_EXPENSE), amt(80_000));
    assert_eq!(net_debit(&ins, acct::CASH), yuan(3_000));

    // ── 部分支付：CLM-100 先付 400 元（现金出，负债降）──
    ins.pay_claim(
        &group_id(),
        &ClaimId("CLM-100".to_string()),
        yuan(400),
        d("2030-07-15"),
    )
    .expect("partial payment");
    assert_eq!(net_debit(&ins, acct::LIC), amt(-40_000));
    assert_eq!(net_debit(&ins, acct::CASH), yuan(2_600));
    let claim = ins
        .group(&group_id())
        .unwrap()
        .claim(&ClaimId("CLM-100".to_string()))
        .unwrap();
    assert_eq!(claim.incurred(), yuan(500));
    assert_eq!(claim.paid(), yuan(400));
    assert_eq!(claim.unpaid(), Ok(yuan(100)));

    // ── 期末释放剩余 265 单元（精确清零）──
    ins.release_service(&group_id(), 265, d("2031-01-01"))
        .expect("final release");
    assert_eq!(net_debit(&ins, acct::LRC), amt(0));

    // ── 调节表 1：LRC 滚动（逐笔恒等）──
    let group = ins.group(&group_id()).unwrap();
    let lrc_end = net_debit(&ins, acct::LRC).neg().expect("lrc credit");
    assert_eq!(
        lrc_end,
        group
            .premium()
            .add(group.day_one_loss())
            .expect("r1")
            .add(group.remeasure_loss_total())
            .expect("r2")
            .add(group.remeasure_finance_total())
            .expect("r3")
            .add(group.released_finance_total())
            .expect("r4")
            .sub(group.released_revenue_total())
            .expect("r5")
    );
    assert_eq!(lrc_end, amt(0));
    assert_eq!(group.released_revenue_total(), amt(103_303));
    assert_eq!(group.released_finance_total(), amt(3_077));
    assert_eq!(group.remeasure_finance_total(), amt(226));
    assert_eq!(group.remeasure_loss_total(), amt(0));

    // ── 调节表 2：LIC 滚动 = 发生 − 支付 ──
    let lic_end = net_debit(&ins, acct::LIC).neg().expect("lic credit");
    let claims_incurred = group.claims().fold(amt(0), |acc, (_, claim)| {
        acc.add(claim.incurred()).expect("claims sum")
    });
    let claims_paid = group.claims().fold(amt(0), |acc, (_, claim)| {
        acc.add(claim.paid()).expect("paid sum")
    });
    assert_eq!(claims_incurred, amt(80_000));
    assert_eq!(claims_paid, amt(40_000));
    assert_eq!(lic_end, claims_incurred.sub(claims_paid).expect("lic roll"));

    // ── 调节表 3：CSM 成分滚动 = 初始 + 再计量 − 释放 ──
    let csm_end = group.csm();
    assert_eq!(
        csm_end,
        amt(18_077)
            .sub(group.reestimated_csm_total())
            .expect("csm net")
            .sub(amt(4_953 + 5_350))
            .expect("csm released")
    );
    assert_eq!(csm_end, amt(0));
    assert_eq!(group.reestimated_csm_total(), amt(7_774));

    // ── 调节表 4：现金滚动 = 保费收讫 − 赔款支付 ──
    assert_eq!(
        net_debit(&ins, acct::CASH),
        yuan(2_000)
            .add(group.premium_collected())
            .expect("cash r1")
            .sub(claims_paid)
            .expect("cash r2")
    );

    // ── 调节表 5：损益 ↔ 经济恒等式（权责发生制对账）──
    // 释放收入 103_303 − 赔案费用 80_000 − 财务 3_303 = 20_000
    // = 保费 100_000 − 已发生赔付 80_000。
    let net_income = net_debit(&ins, acct::INSURANCE_REVENUE)
        .neg()
        .expect("rev credit")
        .sub(net_debit(&ins, acct::INSURANCE_EXPENSE))
        .expect("less expense")
        .sub(net_debit(&ins, acct::INSURANCE_FINANCE))
        .expect("less finance");
    assert_eq!(net_income, amt(20_000));
    assert_eq!(
        net_income,
        group
            .premium()
            .sub(claims_incurred)
            .expect("economic identity")
    );
    // 资产负债表：现金 = LIC + 权益滚动（LRC 为零）。
    let equity_rolling = net_debit(&ins, acct::CAPITAL)
        .neg()
        .expect("capital")
        .add(net_income)
        .expect("equity");
    assert_eq!(
        net_debit(&ins, acct::CASH),
        lic_end.add(equity_rolling).expect("L+E")
    );

    // ── 对手方资金流：保费 Inbound + 赔款 Outbound（跨边界留痕）。──
    let flows = ins.counterparties().flows();
    assert_eq!(flows.len(), 2);
    assert_eq!(flows[0].amount, yuan(1_000));
    assert_eq!(flows[0].direction, engine::company::FlowDirection::Inbound);
    assert_eq!(flows[1].amount, yuan(400));
    assert_eq!(flows[1].direction, engine::company::FlowDirection::Outbound);
}
