//! 资源实体：交易部 → 组的**可 move 数值载体**（所有权随实体转移）。
//!
//! 与事实流（[`crate::runtime::facts`]）的分工：Entity 做资源转移（现金/股票真正
//! 换手），FillFact/DeskReceipt 做信息回流（计划续行/回执/事件）。粒度按资源实体
//! 模型：NPC 现金一个聚合额、每 (NPC,股票) 持仓一个聚合额，不做"一元一实体"。

use super::LocalIdx;

/// 交易部 → 组线程的资源实体。所在批次（[`super::DeskBatch`]）携带股索引与
/// tick/round 防御字段；`local` 是组内名册下标，组端零查找落账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entity {
    /// 卖出成交净额（已扣卖腿费用：佣金/印花税/过户费）。
    CashProceeds {
        /// 收款账户的组内名册下标。
        local: LocalIdx,
        /// 净额（i64 分，非负）。
        amount: i64,
    },
    /// 买单终结 purse 余额退款 / 受理拒绝退款。
    CashRefund {
        /// 收款账户的组内名册下标。
        local: LocalIdx,
        /// 退款额（i64 分，非负）。
        amount: i64,
    },
    /// 买入成交：股票实体回流（组端 T+1 冻结入账）。
    BoughtShares {
        /// 入账账户的组内名册下标。
        local: LocalIdx,
        /// 股数（非负）。
        qty: u32,
    },
    /// 卖单终结余股回流（恢复可卖）。
    RefundedShares {
        /// 入账账户的组内名册下标。
        local: LocalIdx,
        /// 股数（非负）。
        qty: u32,
    },
    /// 挂单终结标记：闭合组端在途登记（foreign_active 递减）。**零额终结也必须发**，
    /// 否则在途计数泄漏。Sol 二审修复集 3：携带终结单的 desk 序号——组端按
    /// 订单身份关联释放在途票（不只按账户+股票）。
    Closed {
        /// 终结挂单所属账户的组内名册下标。
        local: LocalIdx,
        /// 终结挂单的 desk 本地序号（OrderRef.1）。
        order_serial: u64,
    },
}
