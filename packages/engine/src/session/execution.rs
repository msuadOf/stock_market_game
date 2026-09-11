//! 会话执行接缝（机构母单与 NPC 工作单）：从 session.rs 按责任抽出（W1-Task 3）。
//! 母单物化、工作单对账、现金预算与订单记录保持原行为与存档格式。

mod budget;
mod orders;
mod reconcile;
mod records;

use super::*;
use crate::plans::PlanId;

/// 可恢复的大资金母单：目标与实际成交分离，后续子单不得超过 remaining_qty。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ParentOrderPlan {
    pub code: StockCode,
    pub side: Side,
    pub target_qty: u32,
    pub filled_qty: u32,
    /// 单次重新报价所允许的最大子单数量。
    pub child_qty: u32,
    /// 当前在订单簿或集合竞价队列中的子单；只有该订单的真实成交可以推进母单。
    pub active_child_order_id: Option<OrderId>,
    /// 上述子单尚未成交的数量；与订单簿中的剩余数量严格一致。
    pub active_child_remaining_qty: Option<u32>,
    /// 显式计划执行接缝的所有者；未接计划的既有母单保持 None 且序列化字节不变。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub linked_plan_id: Option<PlanId>,
    pub limit_price: Money,
    #[serde(with = "super::u64_decimal")]
    #[ts(type = "string")]
    pub expires_market_minute: u64,
}

impl ParentOrderPlan {
    pub fn remaining_qty(&self) -> u32 {
        self.target_qty
            .checked_sub(self.filled_qty)
            .expect("parent-order filled quantity exceeds target")
    }
}
