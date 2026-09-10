//! 保险金样（二）：期末重估（估计改变）三向分流——CSM 再计量（组合成分，
//! 不过账）/ 保险财务损益（立即过账）/ 亏损成分变动（损益过账）。
//!
//! 手算锚（365 天组、400bp、首段已释放 100 单元；见 gold.rs 首段数字）：
//! 释放后 E_rem=58_082、RA_rem=3_630、C_rem=13_124、F_rem=2_234、LRC=72_602；
//! 剩余期限 265 天 → 贴现分母 3_650_000 + 400×265 = 3_756_000。

use super::{acct, amt, base_config, d, net_debit, policyholder, yuan};
use engine::company::insurance::{ClaimId, InsuranceBooks, InsuranceProductKind};
use engine::company::ContractId;

fn group_id() -> ContractId {
    ContractId("GRP-A".to_string())
}

/// 建立盈利组并完成首段 100 单元释放（到达重估锚点状态）。
fn released_head(mut ins: InsuranceBooks) -> InsuranceBooks {
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
        .expect("collect");
    ins.release_service(&group_id(), 100, d("2030-04-11"))
        .expect("first release");
    assert_eq!(net_debit(&ins, acct::LRC), amt(-72_602));
    ins
}

#[test]
fn remeasurement_absorbed_by_csm_posts_only_finance() {
    // 赔付估计上调至 E_rem 66_082（ΔE=+8_000，265 天）：
    // ΔPV = rhe(8_000×3_650_000/3_756_000) = 7_774 → CSM 13_124−7_774=5_350
    // （成分变化，不过账）；ΔF = 8_000−7_774 = 226 → 立即过账
    // Dr 6541 / Cr 2501。LRC = 72_602+226 = 72_828。
    let mut ins = released_head(InsuranceBooks::new(base_config()).expect("opens"));
    ins.remeasure(&group_id(), d("2030-06-01"), amt(66_082))
        .expect("remeasure")
        .expect("posts finance entry");
    assert_eq!(net_debit(&ins, acct::INSURANCE_FINANCE), amt(1_069)); // 843+226
    assert_eq!(net_debit(&ins, acct::INSURANCE_EXPENSE), amt(0)); // 无亏损成分变动
    assert_eq!(net_debit(&ins, acct::LRC), amt(-72_828));
    let group = ins.group(&group_id()).unwrap();
    assert_eq!(group.expected_claims_remaining(), amt(66_082));
    assert_eq!(group.csm(), amt(5_350));
    assert_eq!(group.loss_component(), amt(0));
    assert_eq!(group.reestimated_csm_total(), amt(7_774));
    // 组合恒等式（盈利组）在重估后保持。
    assert_eq!(
        (net_debit(&ins, acct::LRC)).neg().expect("lrc"),
        group
            .expected_claims_remaining()
            .add(group.risk_adjustment_remaining())
            .expect("id")
            .add(group.csm())
            .expect("id")
            .sub(group.finance_remaining())
            .expect("id")
    );

    // 末段 265 单元清零：收入 = 66_082+3_630+5_350 = 75_062；财务 2_234；
    // LRC = 72_828−75_062+2_234 = 0。实际赔付 1000：
    // 净利 = (28_241+75_062) − 100_000 − (843+226+2_234) = 0 = 1000−1000。
    ins.release_service(&group_id(), 265, d("2031-01-01"))
        .expect("final release");
    assert_eq!(net_debit(&ins, acct::LRC), amt(0));
    ins.record_claim(
        &group_id(),
        ClaimId("CLM-1".to_string()),
        yuan(1_000),
        d("2031-01-02"),
    )
    .expect("claim");
    ins.pay_claim(
        &group_id(),
        &ClaimId("CLM-1".to_string()),
        yuan(1_000),
        d("2031-01-02"),
    )
    .expect("pay");
    assert_eq!(
        net_debit(&ins, acct::INSURANCE_REVENUE),
        amt(-103_303) // 28_240+75_063
    );
    assert_eq!(net_debit(&ins, acct::INSURANCE_EXPENSE), amt(100_000));
    assert_eq!(net_debit(&ins, acct::INSURANCE_FINANCE), amt(3_303));
    let net_income = amt(103_303 - 100_000 - 3_303);
    assert_eq!(net_income, amt(0));
    assert_eq!(net_debit(&ins, acct::CASH), yuan(2_000));
}

#[test]
fn remeasurement_exhausting_csm_creates_loss_component() {
    // 巨额上调 ΔE=+50_000：ΔPV = rhe(50_000×3_650_000/3_756_000) = 48_589
    // > CSM 13_124 → CSM 归零 + 亏损成分 35_465 即期入损益；
    // ΔF = 50_000−48_589 = 1_411 过 6541。合计 LRC 过账 = 36_875。
    let mut ins = released_head(InsuranceBooks::new(base_config()).expect("opens"));
    ins.remeasure(&group_id(), d("2030-06-01"), amt(108_082))
        .expect("remeasure")
        .expect("posts");
    assert_eq!(net_debit(&ins, acct::INSURANCE_FINANCE), amt(2_254)); // 843+1_411
    assert_eq!(net_debit(&ins, acct::INSURANCE_EXPENSE), amt(35_465));
    assert_eq!(net_debit(&ins, acct::LRC), amt(-109_478)); // 72_603+36_875
    let group = ins.group(&group_id()).unwrap();
    assert_eq!(group.csm(), amt(0));
    assert_eq!(group.loss_component(), amt(35_465));
    // 亏损组恒等式：LRC = E_rem + RA_rem − F_rem = 108_082+3_630−2_234。
    assert_eq!(
        (net_debit(&ins, acct::LRC)).neg().expect("lrc"),
        group
            .expected_claims_remaining()
            .add(group.risk_adjustment_remaining())
            .expect("id")
            .sub(group.finance_remaining())
            .expect("id")
    );

    // 末段清零：收入 = 108_082+3_630 = 111_712；财务 2_234 → LRC 0。
    ins.release_service(&group_id(), 265, d("2031-01-01"))
        .expect("final release");
    assert_eq!(net_debit(&ins, acct::LRC), amt(0));
    ins.record_claim(
        &group_id(),
        ClaimId("CLM-1".to_string()),
        yuan(1_100),
        d("2031-01-02"),
    )
    .expect("claim");
    ins.pay_claim(
        &group_id(),
        &ClaimId("CLM-1".to_string()),
        yuan(1_100),
        d("2031-01-02"),
    )
    .expect("pay");
    // 净利 = 139_953 − 110_000 − 4_488 − 35_465 = −10_000 = 1000−1100。
    let net_income = amt(139_953 - 110_000 - 4_488 - 35_465);
    assert_eq!(net_income, amt(-10_000));
    assert_eq!(net_debit(&ins, acct::CASH), yuan(1_900));
    let equity_rolling = net_debit(&ins, acct::CAPITAL)
        .neg()
        .expect("capital")
        .add(net_income)
        .expect("equity");
    assert_eq!(equity_rolling, net_debit(&ins, acct::CASH));
}

#[test]
fn remeasurement_recovery_turns_onerous_group_profitable() {
    // 亏损组（premium 600）首段 100 单元后：收入 23_288、财务 843、
    // LRC = 81_923−23_288+843 = 59_478；备查亏损 = 21_923−rhe(21_923×100/365)
    // = 21_923−6_006 = 15_917。
    let mut ins = InsuranceBooks::new(base_config()).expect("opens");
    ins.establish_group(
        InsuranceProductKind::TermProtection,
        group_id(),
        &policyholder(),
        yuan(600),
        yuan(800),
        yuan(50),
        d("2030-01-01"),
        d("2031-01-01"),
    )
    .expect("establish onerous");
    ins.collect_premium(&group_id(), yuan(600), d("2030-01-01"))
        .expect("collect");
    ins.release_service(&group_id(), 100, d("2030-04-11"))
        .expect("first release");
    assert_eq!(net_debit(&ins, acct::LRC), amt(-59_478));
    assert_eq!(
        ins.group(&group_id()).unwrap().loss_component(),
        amt(15_917)
    );

    // 下调 ΔE=−30_000（265 天）：ΔPV = −29_153 → 全额转回亏损 15_917 并转
    // 盈利：CSM = 29_153−15_917 = 13_236。ΔF = −30_000+29_153 = −847 →
    // 财务利得（Cr 6541）。LRC = 59_478−15_917−847 = 42_714。
    ins.remeasure(&group_id(), d("2030-06-01"), amt(28_082))
        .expect("remeasure")
        .expect("posts");
    assert_eq!(net_debit(&ins, acct::INSURANCE_FINANCE), amt(-4)); // 843−847
    assert_eq!(
        net_debit(&ins, acct::INSURANCE_EXPENSE),
        amt(6_006) // 21_923 首日 − 15_917 转回
    );
    assert_eq!(net_debit(&ins, acct::LRC), amt(-42_714));
    let group = ins.group(&group_id()).unwrap();
    assert_eq!(group.csm(), amt(13_236));
    assert_eq!(group.loss_component(), amt(0));
    assert_eq!(group.expected_claims_remaining(), amt(28_082));

    // 末段清零：收入 = 28_082+3_630+13_236 = 44_948；财务 2_234 → LRC 0。
    ins.release_service(&group_id(), 265, d("2031-01-01"))
        .expect("final release");
    assert_eq!(net_debit(&ins, acct::LRC), amt(0));
    ins.record_claim(
        &group_id(),
        ClaimId("CLM-1".to_string()),
        yuan(300),
        d("2031-01-02"),
    )
    .expect("claim");
    ins.pay_claim(
        &group_id(),
        &ClaimId("CLM-1".to_string()),
        yuan(300),
        d("2031-01-02"),
    )
    .expect("pay");
    // 净利 = 68_236 − 30_000 − 2_230 − 21_923 + 15_917 = 30_000 = 600−300。
    let net_income = amt(68_236 - 30_000 - 2_230 - 21_923 + 15_917);
    assert_eq!(net_income, amt(30_000));
    assert_eq!(net_debit(&ins, acct::CASH), yuan(2_300));
    let equity_rolling = net_debit(&ins, acct::CAPITAL)
        .neg()
        .expect("capital")
        .add(net_income)
        .expect("equity");
    assert_eq!(equity_rolling, net_debit(&ins, acct::CASH));
}
