//! 非法 PD/LGD/权重与核销守卫：ECL 情景字段越界、概率权重和 ≠ 10000bp、
//! 政策级非法在构造期拒绝；准备不足核销、重复核销、非核销贷款回收、
//! 回收超可回收额、已核销贷款改阶段。

use super::super::{d, yuan};
use super::{base_config, full_loss, loan_id, with_loan};
use engine::company::bank::{BankBooks, BankError, EclPolicy, EclScenario, EclStage};

#[test]
fn illegal_pd_lgd_and_weights_are_rejected() {
    let mut bank = with_loan();
    let before = bank.clone();
    // PD 超 10000bp / 负 LGD / 权重和 ≠ 10000 → EclInvalid。
    let bad_pd = vec![EclScenario {
        weight_bp: 10_000,
        pd_bp: 10_001,
        lgd_bp: 5_000,
    }];
    assert!(matches!(
        bank.assess_credit(
            &loan_id(),
            d("2030-02-01"),
            EclStage::Stage1,
            "bad pd",
            bad_pd
        ),
        Err(BankError::EclInvalid { .. })
    ));
    let bad_lgd = vec![EclScenario {
        weight_bp: 10_000,
        pd_bp: 100,
        lgd_bp: -1,
    }];
    assert!(matches!(
        bank.assess_credit(
            &loan_id(),
            d("2030-02-01"),
            EclStage::Stage1,
            "bad lgd",
            bad_lgd
        ),
        Err(BankError::EclInvalid { .. })
    ));
    let bad_weight = vec![
        EclScenario {
            weight_bp: 6_000,
            pd_bp: 100,
            lgd_bp: 5_000,
        },
        EclScenario {
            weight_bp: 3_999,
            pd_bp: 800,
            lgd_bp: 5_000,
        },
    ];
    assert!(matches!(
        bank.assess_credit(
            &loan_id(),
            d("2030-02-01"),
            EclStage::Stage1,
            "bad weight",
            bad_weight
        ),
        Err(BankError::EclInvalid { .. })
    ));
    assert_eq!(bank, before);
    // EclPolicy 本身非法（空情景表 / 非法情景）→ 构造期拒绝。
    let mut cfg = base_config();
    cfg.ecl_policy = EclPolicy {
        version: 1,
        stage1_default: Vec::new(),
        lifetime_default: vec![EclScenario {
            weight_bp: 10_000,
            pd_bp: 800,
            lgd_bp: 5_000,
        }],
    };
    assert!(matches!(
        BankBooks::new(cfg),
        Err(BankError::EclInvalid { .. })
    ));
    let mut cfg = base_config();
    cfg.ecl_policy = EclPolicy {
        version: 1,
        stage1_default: vec![EclScenario {
            weight_bp: 10_000,
            pd_bp: 20_000,
            lgd_bp: 5_000,
        }],
        lifetime_default: vec![EclScenario {
            weight_bp: 10_000,
            pd_bp: 800,
            lgd_bp: 5_000,
        }],
    };
    assert!(matches!(
        BankBooks::new(cfg),
        Err(BankError::EclInvalid { .. })
    ));
}

#[test]
fn duplicate_write_off_and_related_guards_are_rejected() {
    let mut bank = with_loan();
    // 未足额计提先核销 → InsufficientAllowance（准备 3.00 < 账面 600+息）。
    bank.accrue_loan_interest(d("2030-02-01")).expect("accrue");
    let before = bank.clone();
    assert!(matches!(
        bank.write_off(&loan_id(), d("2030-02-10")),
        Err(BankError::InsufficientAllowance { .. })
    ));
    assert_eq!(bank, before);
    // 100% 计提后核销成功；重复核销 → LoanAlreadyWrittenOff。
    bank.assess_credit(
        &loan_id(),
        d("2030-02-15"),
        EclStage::Stage3,
        "default",
        full_loss(),
    )
    .expect("full provision");
    bank.write_off(&loan_id(), d("2030-02-20"))
        .expect("write off");
    let after = bank.clone();
    assert!(matches!(
        bank.write_off(&loan_id(), d("2030-02-21")),
        Err(BankError::LoanAlreadyWrittenOff { .. })
    ));
    assert_eq!(bank, after);
    // 回收超可回收额 → RecoveryBeyondRecoverable。
    assert!(matches!(
        bank.recover_written_off(&loan_id(), yuan(700), d("2030-02-25")),
        Err(BankError::RecoveryBeyondRecoverable { .. })
    ));
    // 活跃贷款回收 → LoanNotWrittenOff。
    let mut bank2 = with_loan();
    assert!(matches!(
        bank2.recover_written_off(&loan_id(), yuan(1), d("2030-02-25")),
        Err(BankError::LoanNotWrittenOff { .. })
    ));
    // 已核销贷款改阶段 → StageTransferOnWrittenOff（同阶段重估允许，
    // 回收后转回路径在 gold 覆盖）。
    assert!(matches!(
        bank.assess_credit(
            &loan_id(),
            d("2030-03-01"),
            EclStage::Stage2,
            "stage change on written-off",
            full_loss(),
        ),
        Err(BankError::StageTransferOnWrittenOff { .. })
    ));
}
