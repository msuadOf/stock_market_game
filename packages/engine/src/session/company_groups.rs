//! Session 的集团接缝：仅重导出公司域的集团身份与领域操作。
//! 合并余额、内部交易抵销、关账与材料组装均由 company::groups 持有。

pub use crate::company::groups::{GroupHolding, GroupStructure};
pub(crate) use crate::company::groups::{
    publish_group_scheduled, seed_groups, validate_groups,
};
