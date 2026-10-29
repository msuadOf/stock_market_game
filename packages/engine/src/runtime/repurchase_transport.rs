//! 默认构建的公司请求与有序来源事实；不承担授权、余额或资源落账。
use super::{FillFact, OrderRef, Owner, ReceiptKind};

/// 复用原数值票的稳定请求身份；交易日序必须来自已验证 calendar。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepurchaseRequestKey {
    pub plan_id: crate::plans::PlanId,
    pub stock: crate::StockId,
    pub requested_trading_day: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepurchaseReturnKind {
    PriceImprovement,
    TerminalRefund,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepurchaseSourceKind {
    Accepted {
        price: i64,
        qty: u32,
        purse_initial: i64,
    },
    Rejected {
        reason: ReceiptKind,
    },
    Fill(FillFact),
    Return {
        amount: i64,
        kind: RepurchaseReturnKind,
    },
    Closed,
}

/// order_ref 只来自本 desk 实际取号；拒收亦保真实身份，不伪装 Accepted。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepurchaseSourceFact {
    pub request_key: RepurchaseRequestKey,
    pub owner: Owner,
    pub order_ref: OrderRef,
    pub kind: RepurchaseSourceKind,
}
