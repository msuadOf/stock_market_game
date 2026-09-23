//! 保险金样（一）：盈利组全链 / 部分释放余数守恒 / 亏损组首日亏损。
//! 全部数字手算钉死（分）。
//!
//! 手算依据：PV = rhe(claims × 3_650_000 / (3_650_000 + 400×days))；
//! 365 天分母 = 3_650_000 + 400×365 = 3_796_000（4% 简单贴现一年：
//! 800/1.04 = 769.23 元，恰与整数除法吻合）。
//! 盈利组：premium 1000 / claims 800 / RA 50 → PV = rhe(80_000×3_650_000/
//! 3_796_000) = 76_923；CSM₀ = 100_000 − 76_923 − 5_000 = 18_077；
//! F = 80_000 − 76_923 = 3_077。
//! 亏损组：premium 600（其余同）→ CSM₀ = −21_923 → 首日亏损 219.23 元。

use super::{acct, amt, base_config, d, net_debit, policyholder, yuan};
use engine::company::insurance::{ClaimId, InsuranceBooks, InsuranceProductKind};
use engine::company::ContractId;

fn group_id() -> ContractId {
    ContractId("GRP-A".to_string())
}

#[test]
fn insurance_profitable_group_full_chain_gold() {
    let mut ins = InsuranceBooks::new(base_config()).expect("insurer opens");
    assert_eq!(net_debit(&ins, acct::CASH), yuan(2_000));

    // ── 2030-01-01：建立盈利组（365 天保障，2030-01-01→2031-01-01）──
    // 保费 1000 元、预期赔付 800 元、风险调整 50 元。应收保费挂账（非现金）：
    // K3 红线：保费不立即计收入——6051 为零，2501 贷记 1000 元。
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
    .expect("establish profitable group");
    assert_eq!(net_debit(&ins, acct::PREMIUM_RECEIVABLE), yuan(1_000));
    assert_eq!(net_debit(&ins, acct::LRC), yuan(-1_000));
    assert_eq!(net_debit(&ins, acct::INSURANCE_REVENUE), amt(0)); // 不是收入
    assert_eq!(net_debit(&ins, acct::INSURANCE_EXPENSE), amt(0)); // 盈利组无损益
    let group = ins.group(&group_id()).unwrap();
    assert_eq!(group.csm(), amt(18_077));
    assert_eq!(group.loss_component(), amt(0));
    assert_eq!(group.finance_remaining(), amt(3_077));
    assert_eq!(group.units_total(), 365);

    // ── 保费收讫：现金 +1000，应收清零（经营活动）。──
    ins.collect_premium(&group_id(), yuan(1_000), d("2030-01-01"))
        .expect("collect premium");
    assert_eq!(net_debit(&ins, acct::CASH), yuan(3_000));
    assert_eq!(net_debit(&ins, acct::PREMIUM_RECEIVABLE), amt(0));

    // ── 一次性释放全部 365 责任单元（含期末精确清零）──
    // 收入 = 预期赔付 80_000 + RA 5_000 + CSM 18_077 = 103_077；
    // 保险财务损益 = 3_077（贴现回拨）。LRC = 100_000 − 103_077 + 3_077 = 0。
    ins.release_service(&group_id(), 365, d("2031-01-01"))
        .expect("release full coverage");
    assert_eq!(net_debit(&ins, acct::INSURANCE_REVENUE), amt(-103_077));
    assert_eq!(net_debit(&ins, acct::INSURANCE_FINANCE), amt(3_077));
    assert_eq!(net_debit(&ins, acct::LRC), amt(0));
    let group = ins.group(&group_id()).unwrap();
    assert_eq!(group.expected_claims_remaining(), amt(0));
    assert_eq!(group.risk_adjustment_remaining(), amt(0));
    assert_eq!(group.csm(), amt(0));
    assert_eq!(group.finance_remaining(), amt(0));

    // ── 赔案发生 800 元与支付分离：先负债、后现金。──
    ins.record_claim(
        &group_id(),
        ClaimId("CLM-1".to_string()),
        yuan(800),
        d("2031-01-02"),
    )
    .expect("claim occurs");
    assert_eq!(net_debit(&ins, acct::LIC), amt(-80_000));
    assert_eq!(net_debit(&ins, acct::INSURANCE_EXPENSE), amt(80_000));
    assert_eq!(net_debit(&ins, acct::CASH), yuan(3_000)); // 发生不动现金
    ins.pay_claim(
        &group_id(),
        &ClaimId("CLM-1".to_string()),
        yuan(800),
        d("2031-01-05"),
    )
    .expect("claim paid");
    assert_eq!(net_debit(&ins, acct::LIC), amt(0));
    assert_eq!(net_debit(&ins, acct::CASH), yuan(2_200));

    // ── 终态对账（分）──
    // 净利 = 1030.77 − 800 − 30.77 = 200.00 = 保费 1000 − 赔付 800。
    // 权益滚动 = 2000 + 200 = 2200 = 现金（负债全零）。
    let net_income = amt(103_077 - 80_000 - 3_077);
    assert_eq!(net_income, amt(20_000));
    let equity_rolling = net_debit(&ins, acct::CAPITAL)
        .neg()
        .expect("capital credit")
        .add(net_income)
        .expect("equity");
    assert_eq!(equity_rolling, yuan(2_200));
    assert_eq!(net_debit(&ins, acct::CASH), equity_rolling);

    // ── 报表分类层（CAS 25 §84/§85 + CAS 30 §55(二)）。──
    let lines = ins.presentation_lines().expect("presentation lines");
    assert_eq!(lines.insurance_revenue, amt(103_077));
    assert_eq!(lines.insurance_expense, amt(80_000));
    assert_eq!(lines.insurance_service_result, amt(23_077)); // 230.77 元
    assert_eq!(lines.insurance_finance_expense, amt(3_077));
    assert_eq!(lines.lrc_balance, amt(0));
    assert_eq!(lines.lic_balance, amt(0));
    assert_eq!(lines.premiums_receivable, amt(0));
    assert_eq!(lines.cash_position, yuan(2_200));

    // ── 存档往返：重放路径恢复等价账套（K2 事实/派生边界）。──
    let json = serde_json::to_string(&ins).expect("serialize");
    let restored: InsuranceBooks = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(restored, ins);
}

#[test]
fn insurance_partial_release_conserves_every_component() {
    // 两段释放（100 + 265 单元）：逐组件 rhe 分摊 + 余数链守恒 + 期末精确清零。
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
        .expect("collect");

    // 首段 100 单元：claims rhe(80_000×100/365)=21_918（余 −70）；
    // RA rhe(5_000×100/365)=1_370（余 −50）；CSM rhe(18_077×100/365)=4_953
    // （余 −145）；财务 rhe(3_077×100/365)=843（余 5）。
    // 收入 = 21_918+1_370+4_953 = 28_241；LRC = 100_000−28_241+843 = 72_602。
    ins.release_service(&group_id(), 100, d("2030-04-11"))
        .expect("first release");
    assert_eq!(net_debit(&ins, acct::INSURANCE_REVENUE), amt(-28_241));
    assert_eq!(net_debit(&ins, acct::INSURANCE_FINANCE), amt(843));
    assert_eq!(net_debit(&ins, acct::LRC), amt(-72_602));
    let group = ins.group(&group_id()).unwrap();
    assert_eq!(group.expected_claims_remaining(), amt(58_082));
    assert_eq!(group.risk_adjustment_remaining(), amt(3_630));
    assert_eq!(group.csm(), amt(13_124));
    assert_eq!(group.finance_remaining(), amt(2_234));
    assert_eq!(group.units_released(), 100);
    // 组合恒等式：LRC = E_rem + RA_rem + CSM − F_rem（盈利组）。
    assert_eq!(
        (net_debit(&ins, acct::LRC)).neg().expect("lrc credit"),
        group
            .expected_claims_remaining()
            .add(group.risk_adjustment_remaining())
            .expect("id")
            .add(group.csm())
            .expect("id")
            .sub(group.finance_remaining())
            .expect("id")
    );

    // 末段 265 单元（期末精确清零）：全部剩余一次释放。
    // 收入 = 58_082+3_630+13_124 = 74_836；财务 2_234；LRC → 0。
    ins.release_service(&group_id(), 265, d("2031-01-01"))
        .expect("final release");
    assert_eq!(net_debit(&ins, acct::INSURANCE_REVENUE), amt(-103_077));
    assert_eq!(net_debit(&ins, acct::INSURANCE_FINANCE), amt(3_077));
    assert_eq!(net_debit(&ins, acct::LRC), amt(0));
    let group = ins.group(&group_id()).unwrap();
    assert_eq!(group.expected_claims_remaining(), amt(0));
    assert_eq!(group.risk_adjustment_remaining(), amt(0));
    assert_eq!(group.csm(), amt(0));
    assert_eq!(group.finance_remaining(), amt(0));
    assert_eq!(group.units_released(), 365);
}

#[test]
fn insurance_onerous_group_day_one_loss_gold() {
    // 亏损组：CSM₀ = 60_000 − 76_923 − 5_000 = −21_923 → 首日亏损 219.23 元
    // 即期入损益（CAS 25 §27/§46），CSM 恒为零。
    let mut ins = InsuranceBooks::new(base_config()).expect("insurer opens");
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
    .expect("establish onerous group");
    // 应收保费 600 + 首日亏损 21_923 → LRC 贷方 81_923。
    assert_eq!(net_debit(&ins, acct::PREMIUM_RECEIVABLE), yuan(600));
    assert_eq!(net_debit(&ins, acct::LRC), amt(-81_923));
    assert_eq!(net_debit(&ins, acct::INSURANCE_EXPENSE), amt(21_923));
    let group = ins.group(&group_id()).unwrap();
    assert_eq!(group.csm(), amt(0));
    assert_eq!(group.loss_component(), amt(21_923));
    assert_eq!(group.day_one_loss(), amt(21_923));

    ins.collect_premium(&group_id(), yuan(600), d("2030-01-01"))
        .expect("collect");
    // 全量释放：收入 = 80_000 + 5_000（无 CSM）；财务 3_077；
    // LRC = 81_923 − 85_000 + 3_077 = 0。亏损备查成分随单元比例释放为零。
    ins.release_service(&group_id(), 365, d("2031-01-01"))
        .expect("release");
    assert_eq!(net_debit(&ins, acct::INSURANCE_REVENUE), amt(-85_000));
    assert_eq!(net_debit(&ins, acct::INSURANCE_FINANCE), amt(3_077));
    assert_eq!(net_debit(&ins, acct::LRC), amt(0));
    let group = ins.group(&group_id()).unwrap();
    assert_eq!(group.loss_component(), amt(0));

    ins.record_claim(
        &group_id(),
        ClaimId("CLM-1".to_string()),
        yuan(800),
        d("2031-01-02"),
    )
    .expect("claim");
    ins.pay_claim(
        &group_id(),
        &ClaimId("CLM-1".to_string()),
        yuan(800),
        d("2031-01-02"),
    )
    .expect("pay");
    // 净利 = 850 − 800 − 219.23 − 30.77 = −200 = 600 − 800。
    // 权益滚动 = 2000 − 200 = 1800 = 现金。
    let net_income = amt(85_000 - 80_000 - 21_923 - 3_077);
    assert_eq!(net_income, amt(-20_000));
    let equity_rolling = net_debit(&ins, acct::CAPITAL)
        .neg()
        .expect("capital")
        .add(net_income)
        .expect("equity");
    assert_eq!(equity_rolling, yuan(1_800));
    assert_eq!(net_debit(&ins, acct::CASH), equity_rolling);

    // 亏损组部分释放后的组合恒等式：LRC = E_rem + RA_rem − F_rem（备查亏损
    // 成分不加余额）。首段后：收入 23_288、财务 843、
    // LRC = 81_923 − 23_288 + 843 = 59_478；备查亏损 = 21_923 − 6_006 = 15_917。
    let mut ins2 = InsuranceBooks::new(base_config()).expect("insurer opens");
    ins2.establish_group(
        InsuranceProductKind::TermProtection,
        group_id(),
        &policyholder(),
        yuan(600),
        yuan(800),
        yuan(50),
        d("2030-01-01"),
        d("2031-01-01"),
    )
    .expect("establish");
    ins2.release_service(&group_id(), 100, d("2030-04-11"))
        .expect("partial release on onerous group");
    assert_eq!(net_debit(&ins2, acct::LRC), amt(-59_478));
    let group = ins2.group(&group_id()).unwrap();
    assert_eq!(group.loss_component(), amt(15_917));
    assert_eq!(
        (net_debit(&ins2, acct::LRC)).neg().expect("lrc credit"),
        group
            .expected_claims_remaining()
            .add(group.risk_adjustment_remaining())
            .expect("id")
            .sub(group.finance_remaining())
            .expect("id")
    );
}
