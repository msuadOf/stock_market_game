//! 候选评分与软预算分配的账户内验证。

mod failures;
mod gold;

use engine::plans::{
    AllocationClass, AllocationExperience, AllocationRequest, SignalContribution, SignalScore,
};
use engine::{Money, PlanId, Side, StockId};

pub(crate) fn money(cents: i64) -> Money {
    Money::from_cents(cents)
}

/// 夹具股票身份：按五股表代码升序 rank（002156=1、300260=2、600101=3、
/// 600610=4）；未登记代码显式失败（铁律二）。
pub(crate) fn code(value: &str) -> StockId {
    match value {
        "002156" => StockId(1),
        "300260" => StockId(2),
        "600101" => StockId(3),
        "600610" => StockId(4),
        other => panic!("测试夹具未登记代码 {other} 的 StockId rank"),
    }
}

pub(crate) fn available(score: i32) -> SignalContribution {
    SignalContribution::available(SignalScore::new(score).unwrap())
}

pub(crate) fn buy_request(
    plan: u64,
    stock: &str,
    class: AllocationClass,
    confidence_bp: u32,
    requested_cents: i64,
) -> AllocationRequest {
    AllocationRequest {
        plan_id: PlanId(plan),
        code: code(stock),
        side: Side::Buy,
        class,
        confidence_bp,
        requested_cash: money(requested_cents),
        fee_reserve: money(500),
        requested_sell_qty: 0,
        sellable_qty: 0,
        experience: AllocationExperience::default(),
    }
}
