//! 保险类型化拒绝测试（失败路径）：每个拒绝后账套与子账**字节不变**
//! （`assert_eq!(ins, before)` 完整状态对比），事件 id 不倒退。
//!
//! 公共前驱夹具 `seeded`：已收保费、完成首段释放的健康组。

mod entities;
mod guards;
mod unsupported;

use super::{base_config, d, policyholder, yuan};
use engine::company::insurance::{ClaimId, InsuranceBooks, InsuranceProductKind};
use engine::company::ContractId;

pub(crate) fn group_id() -> ContractId {
    ContractId("GRP-A".to_string())
}

pub(crate) fn claim_id() -> ClaimId {
    ClaimId("CLM-1".to_string())
}

/// 已收保费、完成首段释放的健康组（拒绝路径的公共前驱）。
pub(crate) fn seeded() -> InsuranceBooks {
    let mut ins = InsuranceBooks::new(base_config()).expect("opens");
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
        .expect("release");
    ins
}
