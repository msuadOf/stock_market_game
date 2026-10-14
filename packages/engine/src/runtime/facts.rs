//! 事实流：信息回流（不做资源转移）。
//!
//! 与 [`crate::runtime::entities`]（资源转移）的分工：FillFact 驱动计划续行、
//! retail/institutional experience、receipt 聚合与 Event 流——这些消费者需要价格与
//! 费用明细，不能只有金额；DeskReceipt 让业务失败显式可见（铁律二：不静默）。

use super::{OrderRef, Owner};
use crate::orderbook::Side;

/// 成交事实（信息回流）。资金/股票的实际换手由 [`super::Entity`] 承载；本结构只
/// 携带消费者所需明细。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FillFact {
    /// 成交挂单的复合标识。
    pub order: OrderRef,
    /// 挂单所有者。
    pub owner: Owner,
    /// 成交方向。
    pub side: Side,
    /// 成交价（i64 分，正）。
    pub price: i64,
    /// 成交数量（正）。
    pub qty: u32,
    /// 佣金分项（i64 分，非负；双边收取，含最低佣金口径在费用计算侧）。
    pub commission: i64,
    /// 印花税分项（i64 分，非负；**仅卖方**）。
    pub stamp_tax: i64,
    /// 过户费分项（i64 分，非负；双边）。
    pub transfer_fee: i64,
}

/// `FillFact` 防御校验失败（显式失败载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FillFactError {
    /// 成交数量非正。
    #[error("FillFact.qty 非法：{qty}（成交数量必须为正）")]
    InvalidQty {
        /// 非法的数量值。
        qty: u32,
    },
    /// 成交价非正。
    #[error("FillFact.price 非法：{price}（成交价必须为正，单位分）")]
    InvalidPrice {
        /// 非法的价格值。
        price: i64,
    },
    /// 佣金为负。
    #[error("FillFact.commission 为负：{commission}")]
    NegativeCommission {
        /// 非法的佣金值。
        commission: i64,
    },
    /// 印花税为负。
    #[error("FillFact.stamp_tax 为负：{stamp_tax}")]
    NegativeStampTax {
        /// 非法的印花税值。
        stamp_tax: i64,
    },
    /// 过户费为负。
    #[error("FillFact.transfer_fee 为负：{transfer_fee}")]
    NegativeTransferFee {
        /// 非法的过户费值。
        transfer_fee: i64,
    },
    /// 印花税出现在买腿（A 股印花税仅向卖方收取）。
    #[error("FillFact.stamp_tax 出现在买腿：stamp_tax={stamp_tax}（印花税仅卖方）")]
    StampTaxOnBuy {
        /// 买腿上非法出现的印花税值。
        stamp_tax: i64,
    },
}

impl FillFact {
    /// 防御校验：成交事实数值域非法=显式失败。交易部产出侧与组端消费侧均可调用；
    /// 违反即实现 bug（撮合/费用计算错误），不允许静默落账。
    pub fn validate(&self) -> Result<(), FillFactError> {
        if self.qty == 0 {
            return Err(FillFactError::InvalidQty { qty: self.qty });
        }
        if self.price <= 0 {
            return Err(FillFactError::InvalidPrice { price: self.price });
        }
        if self.commission < 0 {
            return Err(FillFactError::NegativeCommission {
                commission: self.commission,
            });
        }
        if self.stamp_tax < 0 {
            return Err(FillFactError::NegativeStampTax {
                stamp_tax: self.stamp_tax,
            });
        }
        if self.transfer_fee < 0 {
            return Err(FillFactError::NegativeTransferFee {
                transfer_fee: self.transfer_fee,
            });
        }
        // A 股印花税仅向卖方收取（trading-rules 费用表）；买腿非零即实现 bug。
        if self.side == Side::Buy && self.stamp_tax != 0 {
            return Err(FillFactError::StampTaxOnBuy {
                stamp_tax: self.stamp_tax,
            });
        }
        Ok(())
    }
}

/// 受理/撤单回执（业务失败显式可见，不静默）。资源侧的退款/退股由
/// [`super::Entity`]（`CashRefund`/`RefundedShares`）承载，回执只携带拒绝原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeskReceipt {
    /// 目标挂单的复合标识（受理拒绝时为被拒的新单标识）。
    pub order: OrderRef,
    /// 相关账户。
    pub owner: Owner,
    /// 回执种类。
    pub kind: ReceiptKind,
}

/// 回执种类：交易部受理校验与撤单窗口的业务拒绝（对齐 trading-rules 语义表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptKind {
    /// 带外价拒收：申报价在涨跌停带外。
    PriceOutOfBand,
    /// 买入非整手拒收（须为 100 股整数倍）。
    NotRoundLot,
    /// 超单笔申报上限拒收（主板/创业板差异化上限）。
    ExceedsOrderCap,
    /// 相位拒收（如 PreOpen 09:25–09:30 不接受申报）。
    PhaseRejected,
    /// 不可撤窗口撤单被拒（开盘竞价 09:20 后 / 收盘竞价 14:57 后）。
    CancelWindowClosed,
    /// 已全成无法撤单（显式业务失败，非错误路径）。
    AlreadyFilled,
}
