//! 未支持合同类型化拒绝（K3 红线）：分红 / 投连 / 再保险（分出与分入）
//! 一律 `UnsupportedContract`（docs/company-accounting.md §6），不冒充已实现、
//! 不静默 fallback、不把此类保费流变成股东支付。每个拒绝后账套字节不变。

use super::super::{amt, base_config, d, policyholder, yuan};
use engine::company::insurance::{InsuranceBooks, InsuranceError, InsuranceProductKind};
use engine::company::ContractId;

fn group_id() -> ContractId {
    ContractId("GRP-X".to_string())
}

#[test]
fn unsupported_product_kinds_are_typed_rejected() {
    let mut ins = InsuranceBooks::new(base_config()).expect("opens");
    for kind in [
        InsuranceProductKind::Participating,
        InsuranceProductKind::UnitLinked,
        InsuranceProductKind::ReinsuranceCeded,
        InsuranceProductKind::ReinsuranceAccepted,
    ] {
        let before = ins.clone();
        let err = ins
            .establish_group(
                kind,
                group_id(),
                &policyholder(),
                yuan(1_000),
                yuan(800),
                yuan(50),
                d("2030-01-01"),
                d("2031-01-01"),
            )
            .expect_err("unsupported kind rejected");
        assert!(
            matches!(err, InsuranceError::UnsupportedContract { kind: k, .. } if k == kind),
            "unexpected error: {err:?}"
        );
        assert_eq!(ins, before); // 字节不变：无合同、无负债、无收入。
    }
    // 未支持类型在已支持组上也不得混入（组类型在建立时定型）。
    let err = ins
        .establish_group(
            InsuranceProductKind::ReinsuranceCeded,
            group_id(),
            &policyholder(),
            amt(1),
            amt(1),
            amt(0),
            d("2030-01-01"),
            d("2031-01-01"),
        )
        .expect_err("reinsurance still rejected");
    assert!(
        matches!(
            err,
            InsuranceError::UnsupportedContract {
                kind: InsuranceProductKind::ReinsuranceCeded,
                ..
            }
        ),
        "{err:?}"
    );
}
