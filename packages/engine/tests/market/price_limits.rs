//! A 股涨跌停与连续竞价价格笼子语义锁（自 ccf0490 逐字迁移；任务 26 修复轮恢复）。
//! 覆盖：正数四舍五入 + 至少一个最小价位、显式溢出错误、笼子参考价优先序
//!（卖一/买一回退）、低价十档放宽、涨跌停边界闭区间接受与越界拒绝。

use super::*;

#[test]
fn price_limits_use_positive_half_up_rounding_and_at_least_one_tick() {
    // 15 × 110% = 16.5：正数四舍五入应为 17，而不是银行家舍入到 16。
    let main = Market::new(
        StockCode("ROUND".to_string()),
        Money::from_cents(15),
        0.10,
        Money::from_cents(1),
    )
    .unwrap();
    assert_eq!(main.up_stop().unwrap(), Money::from_cents(17));
    assert_eq!(main.down_stop().unwrap(), Money::from_cents(14));

    // 2 × 105% 和 2 × 95% 都会舍入回 2；规则要求至少上下移动一个最小价位。
    let low_price_narrow_limit = Market::new(
        StockCode("LOW_LIMIT".to_string()),
        Money::from_cents(2),
        0.05,
        Money::from_cents(1),
    )
    .unwrap();
    assert_eq!(
        low_price_narrow_limit.up_stop().unwrap(),
        Money::from_cents(3)
    );
    assert_eq!(
        low_price_narrow_limit.down_stop().unwrap(),
        Money::from_cents(1)
    );
}

#[test]
fn price_limit_overflow_is_an_explicit_error_not_a_panic() {
    let mut market = mk_market();
    market.set_last_close(Money::from_cents(i64::MAX));
    assert!(market.up_stop().is_err());
}

#[test]
fn continuous_price_cage_uses_the_exchange_reference_price_order() {
    let mut market = mk_market();
    assert_eq!(
        market.continuous_limit_reference(Side::Buy),
        Money::from_cents(1000)
    );
    assert_eq!(
        market.continuous_limit_reference(Side::Sell),
        Money::from_cents(1000)
    );

    market.place(buy(1, 990, 100)).unwrap();
    assert_eq!(
        market.continuous_limit_reference(Side::Buy),
        Money::from_cents(990),
        "买入无卖一时回退到买一"
    );
    assert_eq!(
        market.continuous_limit_reference(Side::Sell),
        Money::from_cents(990),
        "卖出优先使用买一"
    );

    market.place(sell(2, 1010, 100)).unwrap();
    assert_eq!(
        market.continuous_limit_reference(Side::Buy),
        Money::from_cents(1010),
        "买入优先使用卖一"
    );
    assert_eq!(
        market.continuous_limit_reference(Side::Sell),
        Money::from_cents(990),
        "卖出仍优先使用买一"
    );
}

#[test]
fn continuous_price_cage_uses_the_wider_ten_tick_range_for_low_prices() {
    let market = Market::new(
        StockCode("LOW_PRICE".to_string()),
        Money::from_cents(285),
        0.10,
        Money::from_cents(1),
    )
    .unwrap();

    assert_eq!(
        market.continuous_limit_bound(Side::Buy).unwrap(),
        Money::from_cents(295),
        "买入上限应取参考价 102% 与参考价加十个最小价位中的较高者"
    );
    assert_eq!(
        market.continuous_limit_bound(Side::Sell).unwrap(),
        Money::from_cents(275),
        "卖出下限应取参考价 98% 与参考价减十个最小价位中的较低者"
    );
}

#[test]
fn place_rejects_price_above_up_stop() {
    let mut m = mk_market(); // up_stop=1100
    let err = m.place(buy(1, 1101, 100)).unwrap_err(); // 11.01 > 11.00
    assert!(matches!(err, MarketError::LimitExceeded { .. }));
    assert!(m.best_bid().is_none()); // book 未被改动
}

#[test]
fn place_accepts_boundary_up_stop() {
    let mut m = mk_market(); // up_stop=1100
    let r = m.place(buy(1, 1100, 100)).unwrap(); // 边界价合法
    assert!(r.resting.is_some());
    assert_eq!(m.best_bid(), Some(Money::from_cents(1100)));
}

#[test]
fn place_rejects_price_below_down_stop() {
    let mut m = mk_market(); // down_stop=900
    let err = m.place(sell(1, 899, 100)).unwrap_err(); // 8.99 < 9.00
    assert!(matches!(err, MarketError::LimitExceeded { .. }));
    assert!(m.best_ask().is_none());
}
