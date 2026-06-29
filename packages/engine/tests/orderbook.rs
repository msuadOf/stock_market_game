//! engine orderbook 模块集成测试（TDD 红绿循环）。
//!
//! Task 1：仅校验 OrderError 变体 to_string 携带字段、Side 相等/不等、OrderId 基础。
//! 撮合/OrderBook 行为在后续 Task 逐步补齐。
use engine::orderbook::{OrderError, OrderId, Side};

#[test]
fn order_error_and_side_basics() {
    // InvalidTick 的 to_string 应包含 tick 字段（铁律二：错误携带上下文）。
    let e1 = OrderError::InvalidTick {
        tick: engine::Money::ZERO,
    };
    assert!(e1.to_string().contains("tick"));

    // OrderNotFound 的 to_string 应包含被查订单的 id 数值。
    // OrderNotFound(OrderId) 为元组变体（与 DuplicateOrderId 一致，见 orderbook.rs 签名）。
    let e2 = OrderError::OrderNotFound(OrderId(7));
    assert!(e2.to_string().contains('7'));

    // Side 可判等（同向相等、异向不等）。
    assert_eq!(Side::Buy, Side::Buy);
    assert_ne!(Side::Buy, Side::Sell);
}
