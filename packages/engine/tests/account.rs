//! engine account + strategy-trait 模块集成测试（TDD 红绿循环）。
// `Strategy` 当前在 Task 1 骨架测试中尚未直接使用，但后续 account 任务（持有 `Box<dyn Strategy>`、
// 注入 NPC 策略）会消费它——此处提前 import 以保持 plan 预期的导入集合，待消费后移除 allow。
#[allow(unused_imports)]
use engine::strategy::{Intent, MarketView, Strategy};
use engine::orderbook::Side;
use engine::Money;

#[test]
fn intent_and_marketview_construct() {
    let i = Intent::Pass;
    let i2 = Intent::PlaceLimit { side: Side::Buy, price: Money::from_cents(1000), qty: 100 };
    assert!(matches!(i, Intent::Pass));
    assert!(matches!(i2, Intent::PlaceLimit { side: Side::Buy, qty: 100, .. }));

    let mv = MarketView {
        best_bid: Some(Money::from_cents(999)),
        best_ask: Some(Money::from_cents(1001)),
        last_price: Money::from_cents(1000),
    };
    assert_eq!(mv.last_price.cents(), 1000);
}
