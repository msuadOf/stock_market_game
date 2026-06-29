//! engine market 模块集成测试（TDD 红绿循环）。
use engine::account::StockCode;
use engine::market::{Market, MarketError, VParams};
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

fn mk_market() -> Market {
    // last_close=last_price=10.00，limit=0.10，V=10.00，tick=0.01
    Market::new(
        StockCode("600101".to_string()),
        Money::from_cents(1000),
        0.10,
        Money::from_cents(1000),
        Money::from_cents(1),
    )
    .unwrap()
}

#[test]
fn market_new_and_limit_stops() {
    let m = mk_market();
    assert_eq!(m.last_price().cents(), 1000);
    assert_eq!(m.last_close().cents(), 1000);
    assert_eq!(m.fundamental_value().cents(), 1000);
    assert_eq!(m.up_stop().cents(), 1100); // 1000 × 1.10
    assert_eq!(m.down_stop().cents(), 900); // 1000 × 0.90
}

#[test]
fn market_new_rejects_invalid() {
    // limit_pct ∉ (0,1)
    assert!(Market::new(
        StockCode("x".to_string()),
        Money::from_cents(1000),
        1.5,
        Money::from_cents(1000),
        Money::from_cents(1)
    )
    .is_err());
    assert!(Market::new(
        StockCode("x".to_string()),
        Money::from_cents(1000),
        0.0,
        Money::from_cents(1000),
        Money::from_cents(1)
    )
    .is_err());
    // initial_price ≤ 0
    assert!(Market::new(
        StockCode("x".to_string()),
        Money::ZERO,
        0.10,
        Money::from_cents(1000),
        Money::from_cents(1)
    )
    .is_err());
    // v_initial ≤ 0
    assert!(Market::new(
        StockCode("x".to_string()),
        Money::from_cents(1000),
        0.10,
        Money::ZERO,
        Money::from_cents(1)
    )
    .is_err());
}
