//! 负向用例（类型化拒绝 + 完整状态不变断言）：无库存交付 / 非法数量单价、
//! 重复回款与坏账核销守卫、非法税率与计量政策、超授信与无现金付款
//! （PaymentFailed / 逾期面，不补钱、不透支、引擎不崩溃）。

mod collection;
mod guards;
mod inventory;
mod policy;

use super::base_config;
use engine::company::industrial::IndustrialBooks;

/// 新建基础公司（现金 10000 元、授信 5000 元、Fixture 税率）。
pub(crate) fn fresh() -> IndustrialBooks {
    IndustrialBooks::new(base_config()).expect("base opening must construct")
}
