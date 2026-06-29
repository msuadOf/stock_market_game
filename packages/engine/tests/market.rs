//! engine market 模块集成测试（TDD 红绿循环）。
use engine::account::StockCode;
use engine::market::{MarketError, VParams};
use engine::Money;

#[test]
fn market_error_and_vparams_basics() {
    let e = MarketError::LimitExceeded {
        code: StockCode("600101".to_string()),
        price: Money::from_cents(1101),
        down: Money::from_cents(900),
        up: Money::from_cents(1100),
    };
    assert!(e.to_string().contains("600101"));
    let e2 = MarketError::InvalidVParams { reason: "bad".to_string() };
    assert!(e2.to_string().contains("bad"));

    let vp = VParams {
        long_run_mean: Money::from_cents(1000),
        mean_reversion: 0.5,
        volatility: 0.02,
    };
    assert_eq!(vp.long_run_mean.cents(), 1000);
}
