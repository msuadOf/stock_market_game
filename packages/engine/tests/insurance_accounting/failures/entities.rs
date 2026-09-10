//! 守卫类拒绝（实体面）：合同组/赔案/重估入口的金额、日期、重复与越界
//! 拒绝 + 开局子账种子守卫 + 完整状态不变断言。

use super::super::{acct, amt, base_config, d, policyholder, yuan};
use super::{claim_id, group_id, seeded};
use engine::company::insurance::{
    ClaimId, InsuranceBooks, InsuranceConfig, InsuranceError, InsuranceProductKind,
};
use engine::company::{ContractId, CounterpartyId};

#[test]
fn group_and_claim_guards_are_rejected() {
    let mut ins = seeded();
    let kind = InsuranceProductKind::TermProtection;
    let (premium, claims, ra) = (yuan(1_000), yuan(800), yuan(50));
    let (start, end) = (d("2030-01-01"), d("2031-01-01"));

    // 重复组 id。
    let before = ins.clone();
    let err = ins
        .establish_group(
            kind,
            group_id(),
            &policyholder(),
            premium,
            claims,
            ra,
            start,
            end,
        )
        .expect_err("duplicate group");
    assert!(
        matches!(err, InsuranceError::DuplicateGroup { .. }),
        "{err:?}"
    );
    assert_eq!(ins, before);

    // 非正保费 / 非正预期赔付 / 负风险调整。
    for (premium, claims, ra, what) in [
        (amt(0), yuan(800), yuan(50), "premium"),
        (yuan(-1), yuan(800), yuan(50), "premium"),
        (yuan(1), amt(0), yuan(50), "expected claims"),
        (yuan(1), amt(-5), yuan(50), "expected claims"),
        (yuan(1), yuan(1), amt(-1), "risk adjustment"),
    ] {
        let err = ins
            .establish_group(
                kind,
                ContractId("G-X".to_string()),
                &policyholder(),
                premium,
                claims,
                ra,
                start,
                end,
            )
            .expect_err("invalid amounts rejected");
        assert!(
            matches!(err, InsuranceError::NonPositiveAmount { what: w, .. } if w == what),
            "{what}: {err:?}"
        );
    }
    assert_eq!(ins, before);

    // 保障期末不晚于期初。
    let err = ins
        .establish_group(
            kind,
            ContractId("G-X".to_string()),
            &policyholder(),
            premium,
            claims,
            ra,
            start,
            start,
        )
        .expect_err("end == start");
    assert!(
        matches!(err, InsuranceError::CoverageEndNotAfterStart { .. }),
        "{err:?}"
    );

    // 未登记对手方。
    let err = ins
        .establish_group(
            kind,
            ContractId("G-X".to_string()),
            &CounterpartyId("EXT-NOPE".to_string()),
            premium,
            claims,
            ra,
            start,
            end,
        )
        .expect_err("unknown counterparty");
    assert!(matches!(err, InsuranceError::Company(_)), "{err:?}");
    assert_eq!(ins, before);

    // 未知组的各类操作。
    let unknown = ContractId("G-NOPE".to_string());
    let before = ins.clone();
    assert!(matches!(
        ins.collect_premium(&unknown, yuan(1), d("2030-05-01")),
        Err(InsuranceError::UnknownGroup { .. })
    ));
    assert!(matches!(
        ins.release_service(&unknown, 1, d("2030-05-01")),
        Err(InsuranceError::UnknownGroup { .. })
    ));
    assert!(matches!(
        ins.remeasure(&unknown, d("2030-05-01"), amt(1)),
        Err(InsuranceError::UnknownGroup { .. })
    ));
    assert!(matches!(
        ins.record_claim(&unknown, claim_id(), yuan(1), d("2030-05-01")),
        Err(InsuranceError::UnknownGroup { .. })
    ));
    assert!(matches!(
        ins.pay_claim(&unknown, &claim_id(), yuan(1), d("2030-05-01")),
        Err(InsuranceError::UnknownGroup { .. })
    ));
    assert_eq!(ins, before);

    // 已知组、未知赔案；重复赔案 id；非正赔案金额。
    let err = ins
        .pay_claim(
            &group_id(),
            &ClaimId("CLM-NOPE".to_string()),
            yuan(1),
            d("2030-05-01"),
        )
        .expect_err("unknown claim");
    assert!(
        matches!(err, InsuranceError::UnknownClaim { .. }),
        "{err:?}"
    );
    ins.record_claim(&group_id(), claim_id(), yuan(100), d("2030-05-01"))
        .expect("claim");
    let before = ins.clone();
    let err = ins
        .record_claim(&group_id(), claim_id(), yuan(1), d("2030-05-02"))
        .expect_err("duplicate claim");
    assert!(
        matches!(err, InsuranceError::DuplicateClaim { .. }),
        "{err:?}"
    );
    let err = ins
        .record_claim(
            &group_id(),
            ClaimId("CLM-2".to_string()),
            amt(0),
            d("2030-05-02"),
        )
        .expect_err("zero claim");
    assert!(
        matches!(err, InsuranceError::NonPositiveAmount { what, .. } if what == "claim"),
        "{err:?}"
    );
    assert_eq!(ins, before);
}

#[test]
fn remeasurement_guards_are_rejected() {
    let mut ins = seeded();
    // 负的剩余预期赔付。
    let before = ins.clone();
    let err = ins
        .remeasure(&group_id(), d("2030-06-01"), amt(-1))
        .expect_err("negative estimate");
    assert!(
        matches!(err, InsuranceError::NonPositiveAmount { what, .. } if what == "remaining claims estimate"),
        "{err:?}"
    );
    assert_eq!(ins, before);

    // 保障期已尽后不得引入新的剩余预期。
    ins.release_service(&group_id(), 265, d("2031-01-01"))
        .expect("final release");
    let before = ins.clone();
    let err = ins
        .remeasure(&group_id(), d("2031-02-01"), amt(1))
        .expect_err("no remaining coverage");
    assert!(
        matches!(err, InsuranceError::NoRemainingCoverage { .. }),
        "{err:?}"
    );
    assert_eq!(ins, before);
}

#[test]
fn opening_lines_cannot_seed_insurance_accounts() {
    // 开局行只允许现金 + 权益；触碰保险子账/损益科目 → 构造期拒绝
    // （经营前史由任务 14 用同一处理器生成，不从存档倒推）。
    for code in [
        acct::PREMIUM_RECEIVABLE,
        acct::LRC,
        acct::LIC,
        acct::INSURANCE_REVENUE,
        acct::INSURANCE_EXPENSE,
        acct::INSURANCE_FINANCE,
    ] {
        let bad = InsuranceConfig {
            opening_lines: vec![
                super::super::cent_line(code, engine::accounting::PostingSide::Debit, 10_000),
                super::super::cent_line(
                    acct::CAPITAL,
                    engine::accounting::PostingSide::Credit,
                    10_000,
                ),
            ],
            ..base_config()
        };
        let err = InsuranceBooks::new(bad).expect_err("seeded insurance account rejected");
        assert!(
            matches!(err, InsuranceError::OpeningInsuranceBooksSeeded { .. }),
            "{code}: {err:?}"
        );
    }
}
