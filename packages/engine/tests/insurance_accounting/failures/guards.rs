//! 守卫类拒绝（流量面）：负/零服务单元、越保障期释放、非法贴现参数、
//! 支付超现金（PaymentFailed）、保费/赔款超余额 + 完整状态不变断言。

use super::super::{acct, amt, base_config, d, net_debit, yuan};
use super::{claim_id, group_id, seeded};
use engine::company::insurance::{
    DiscountAssumption, InsuranceBooks, InsuranceConfig, InsuranceError,
};

#[test]
fn negative_or_zero_service_units_are_rejected() {
    let mut ins = seeded();
    for units in [0, -5] {
        let before = ins.clone();
        let err = ins
            .release_service(&group_id(), units, d("2030-05-01"))
            .expect_err("non-positive units rejected");
        assert!(
            matches!(err, InsuranceError::NonPositiveServiceUnits { units: u } if u == units),
            "unexpected error: {err:?}"
        );
        assert_eq!(ins, before); // 字节不变
    }
}

#[test]
fn service_units_beyond_coverage_are_rejected() {
    let mut ins = seeded();
    let before = ins.clone();
    let err = ins
        .release_service(&group_id(), 266, d("2030-05-01")) // 剩余 265
        .expect_err("over-release rejected");
    assert!(
        matches!(
            &err,
            InsuranceError::ServiceUnitsBeyondCoverage { remaining, .. } if *remaining == 265
        ),
        "unexpected error: {err:?}"
    );
    assert_eq!(ins, before);

    // 全部释放后再释放任何正单元同样拒绝。
    ins.release_service(&group_id(), 265, d("2031-01-01"))
        .expect("final release");
    let before = ins.clone();
    let err = ins
        .release_service(&group_id(), 1, d("2031-01-02"))
        .expect_err("no coverage left");
    assert!(
        matches!(
            &err,
            InsuranceError::ServiceUnitsBeyondCoverage { remaining, .. } if *remaining == 0
        ),
        "unexpected error: {err:?}"
    );
    assert_eq!(ins, before);
}

#[test]
fn illegal_discount_params_are_rejected_at_construction() {
    // 负利率与零利率都拒绝（贴现假设必须显式为正；不虚构无贴现计量）。
    for rate_bp in [0, -1] {
        let config = InsuranceConfig {
            discount: DiscountAssumption {
                version: 1,
                rate_bp,
            },
            ..base_config()
        };
        let err = InsuranceBooks::new(config).expect_err("illegal rate rejected");
        assert!(
            matches!(err, InsuranceError::InvalidDiscountRate { rate_bp: r } if r == rate_bp),
            "unexpected error: {err:?}"
        );
    }
}

#[test]
fn claim_payment_beyond_cash_fails_typed_and_leaves_state_unchanged() {
    // 现金 3000 元（开局 2000 + 保费 1000）；赔付 5000 元超可支付现金 →
    // PaymentFailed（K2 客户流动性约束：险企继续运行，无透支无自动补钱）。
    let mut ins = seeded();
    ins.record_claim(&group_id(), claim_id(), yuan(5_000), d("2030-05-01"))
        .expect("claim occurs (负债确认不受现金约束)");
    assert_eq!(net_debit(&ins, acct::LIC), amt(-500_000));
    let before = ins.clone();
    let err = ins
        .pay_claim(&group_id(), &claim_id(), yuan(5_000), d("2030-05-02"))
        .expect_err("payment beyond cash");
    assert!(
        matches!(err, InsuranceError::PaymentFailed { .. }),
        "unexpected error: {err:?}"
    );
    assert_eq!(ins, before);
    // 较小金额照常支付（同一赔案部分支付合法）。
    ins.pay_claim(&group_id(), &claim_id(), yuan(2_000), d("2030-05-03"))
        .expect("smaller payment succeeds");
    assert_eq!(net_debit(&ins, acct::CASH), yuan(1_000));
}

#[test]
fn premium_collection_beyond_receivable_is_rejected() {
    let mut ins = seeded();
    let before = ins.clone();
    let err = ins
        .collect_premium(&group_id(), yuan(1), d("2030-05-01")) // 已全额收讫
        .expect_err("no receivable left");
    assert!(
        matches!(
            &err,
            InsuranceError::PremiumBeyondReceivable { outstanding, .. } if *outstanding == amt(0)
        ),
        "unexpected error: {err:?}"
    );
    assert_eq!(ins, before);
}

#[test]
fn claim_payment_beyond_outstanding_is_rejected() {
    let mut ins = seeded();
    ins.record_claim(&group_id(), claim_id(), yuan(100), d("2030-05-01"))
        .expect("claim");
    ins.pay_claim(&group_id(), &claim_id(), yuan(40), d("2030-05-02"))
        .expect("partial pay");
    let before = ins.clone();
    let err = ins
        .pay_claim(&group_id(), &claim_id(), yuan(6_100), d("2030-05-03")) // 未付仅 60
        .expect_err("beyond outstanding");
    assert!(
        matches!(
            &err,
            InsuranceError::ClaimPaymentBeyondOutstanding { outstanding, .. } if *outstanding == yuan(60)
        ),
        "unexpected error: {err:?}"
    );
    assert_eq!(ins, before);
}
