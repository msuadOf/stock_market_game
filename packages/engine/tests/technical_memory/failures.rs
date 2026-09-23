//! 类型化拒绝金样：历史不足、无成交、缺日、未来数据与时间回拨都以显式错误
//! 表达，绝不零填充、绝不虚构走势。

use engine::experience::{PersonalPriceMemory, PriceMemoryError};
use engine::observation::{build_technical_observation, ObservationError};
use engine::strategy::{atr14, rsi14, sma, TechnicalDailyBar, TechnicalError};
use engine::Money;

use super::{bar, closes, code, flat_bar, kbar, price};

#[test]
fn sma_short_history_carries_the_available_length() {
    let short: Vec<i64> = vec![1000; 19];
    assert_eq!(
        sma(&closes(&short), 20).unwrap_err(),
        TechnicalError::InsufficientHistory {
            available: 19,
            required: 20,
        }
    );
    let medium: Vec<i64> = vec![1000; 59];
    assert_eq!(
        sma(&closes(&medium), 60).unwrap_err(),
        TechnicalError::InsufficientHistory {
            available: 59,
            required: 60,
        }
    );
}

#[test]
fn rsi_short_history_is_a_typed_reject_not_a_flat_fifty() {
    // 14 个收盘只有 13 个涨跌样本：类型化拒绝并携带可用长度；
    // 对照组 15 个全平收盘 = 14 个有效样本 → 精确中性 50。
    let short: Vec<i64> = vec![1000; 14];
    assert_eq!(
        rsi14(&closes(&short)).unwrap_err(),
        TechnicalError::InsufficientHistory {
            available: 14,
            required: 15,
        }
    );
    let flat: Vec<i64> = vec![1000; 15];
    let result = rsi14(&closes(&flat)).unwrap();
    assert_eq!(
        (result.value, result.all_flat, result.valid_samples),
        (50, true, 14)
    );
}

#[test]
fn atr_short_history_carries_the_available_length() {
    // ATR14 的每个真实波幅都需要前一根收盘：14 根日 K 只有 13 个波幅样本。
    let bars = vec![kbar(1001, 999, 1000); 14];
    assert_eq!(
        atr14(&bars).unwrap_err(),
        TechnicalError::InsufficientHistory {
            available: 14,
            required: 15,
        }
    );
}

#[test]
fn sma_zero_window_is_typed() {
    assert_eq!(
        sma(&closes(&[1000]), 0).unwrap_err(),
        TechnicalError::ZeroWindow { window: 0 }
    );
}

#[test]
fn kernel_rejects_non_positive_prices_and_inverted_ranges() {
    assert_eq!(
        rsi14(&closes(&[1000, 0, 1000])).unwrap_err(),
        TechnicalError::NonPositivePrice { index: 1, cents: 0 }
    );
    assert_eq!(
        sma(&closes(&[1000, 0]), 2).unwrap_err(),
        TechnicalError::NonPositivePrice { index: 1, cents: 0 }
    );
    let inverted = [TechnicalDailyBar {
        high: price(990),
        low: price(1010),
        close: price(1000),
    }; 15];
    assert_eq!(
        atr14(&inverted).unwrap_err(),
        TechnicalError::InvertedRange {
            index: 0,
            high_cents: 990,
            low_cents: 1010,
        }
    );
}

#[test]
fn observation_with_only_no_trade_days_is_typed_insufficient_never_zero() {
    // 20 个交易日全部无成交：没有真实已发生行情样本。各指标显式
    // InsufficientHistory（可用 0），观测本身结构合法（Ok），绝不补零。
    let bars: Vec<_> = (0..20).map(|day| flat_bar(day, 1000, 0)).collect();
    let observation = build_technical_observation(&bars, 20).unwrap();
    assert_eq!(observation.valid_sample_count, 0);
    assert_eq!(
        observation.sma20.unwrap_err(),
        TechnicalError::InsufficientHistory {
            available: 0,
            required: 20,
        }
    );
    assert_eq!(
        observation.rsi14.unwrap_err(),
        TechnicalError::InsufficientHistory {
            available: 0,
            required: 15,
        }
    );
    assert_eq!(
        observation.atr14.unwrap_err(),
        TechnicalError::InsufficientHistory {
            available: 0,
            required: 15,
        }
    );
}

#[test]
fn observation_rejects_missing_trading_days() {
    // 日 K 序列缺第 2 个交易日：类型化拒绝（缺日不是可用样本为零的样本）。
    // ObservationError 未实现 PartialEq（透传 MoneyError），以字段绑定模式断言。
    let bars = [
        flat_bar(0, 1000, 5),
        flat_bar(1, 1000, 5),
        flat_bar(3, 1000, 5),
    ];
    assert!(matches!(
        build_technical_observation(&bars, 4).unwrap_err(),
        ObservationError::TradingDayGap {
            expected: 2,
            current: 3,
        }
    ));
}

#[test]
fn observation_rejects_non_increasing_days() {
    let bars = [flat_bar(1, 1000, 5), flat_bar(1, 1000, 5)];
    assert!(matches!(
        build_technical_observation(&bars, 2).unwrap_err(),
        ObservationError::NonIncreasingDay {
            previous: 1,
            current: 1,
        }
    ));
}

#[test]
fn observation_rejects_same_day_or_future_bars() {
    // 观察发生在交易日 5：当日尚未完成的 K 与未来 K 都不允许进入观测。
    let mut bars: Vec<_> = (0..5).map(|day| flat_bar(day, 1000, 5)).collect();
    bars.push(flat_bar(5, 1000, 5));
    assert!(matches!(
        build_technical_observation(&bars, 5).unwrap_err(),
        ObservationError::DailyBarNotBeforeObservation { day: 5, as_of: 5 }
    ));

    let mut future: Vec<_> = (0..5).map(|day| flat_bar(day, 1000, 5)).collect();
    future.push(flat_bar(6, 1000, 5));
    assert!(matches!(
        build_technical_observation(&future, 5).unwrap_err(),
        ObservationError::DailyBarNotBeforeObservation { day: 6, as_of: 5 }
    ));
}

#[test]
fn empty_history_is_structurally_valid_but_insufficient() {
    let observation = build_technical_observation(&[], 0).unwrap();
    assert_eq!(observation.valid_sample_count, 0);
    assert_eq!(
        observation.sma60.unwrap_err(),
        TechnicalError::InsufficientHistory {
            available: 0,
            required: 60,
        }
    );
}

#[test]
fn price_memory_rejects_time_going_backwards() {
    let mut memory = PersonalPriceMemory::default();
    let stock = code("600101");
    memory.observe_price(&stock, price(1000), 100).unwrap();

    assert_eq!(
        memory.observe_price(&stock, price(900), 99).unwrap_err(),
        PriceMemoryError::TimeWentBackwards {
            attempted: 99,
            last: 100,
        }
    );
    assert_eq!(
        memory.record_public_history_read(&stock, 50).unwrap_err(),
        PriceMemoryError::TimeWentBackwards {
            attempted: 50,
            last: 100,
        }
    );
}

#[test]
fn price_memory_rejects_non_positive_price() {
    let mut memory = PersonalPriceMemory::default();
    let stock = code("600101");
    memory.observe_price(&stock, price(1000), 100).unwrap();
    assert_eq!(
        memory.observe_price(&stock, Money::ZERO, 101).unwrap_err(),
        PriceMemoryError::NonPositiveMoney {
            field: "observed price",
            cents: 0,
        }
    );
}

#[test]
fn price_memory_rejects_public_read_of_never_observed_stock() {
    // 从未本人观察过的股票没有价格记忆条目；公开历史读取不能凭空创造亲历。
    let mut memory = PersonalPriceMemory::default();
    assert_eq!(
        memory
            .record_public_history_read(&code("600101"), 100)
            .unwrap_err(),
        PriceMemoryError::UnobservedStock {
            code: "600101".into(),
        }
    );
    // 拒绝不改变状态。
    assert_eq!(memory.stock_count(), 0);
}

#[test]
fn observation_propagates_kernel_price_rejection_per_indicator() {
    // 20 个交易日、第 1 日价格为 0：内核先做输入校验再查样本量，因此每个
    // 消费价格的指标独立携带同一类型化拒绝（样本不足断言由无成交/空历史
    // 用例覆盖）。观测结构本身（日序）合法。
    let bars: Vec<_> = (0..20)
        .map(|day| {
            if day == 1 {
                bar(day, 1000, 0, 0, 5)
            } else {
                bar(day, 1000, 990, 1000, 5)
            }
        })
        .collect();
    let observation = build_technical_observation(&bars, 20).unwrap();
    assert_eq!(observation.valid_sample_count, 20);
    let expected = TechnicalError::NonPositivePrice { index: 1, cents: 0 };
    assert_eq!(observation.sma20.unwrap_err(), expected);
    assert_eq!(observation.sma60.unwrap_err(), expected);
    assert_eq!(observation.rsi14.unwrap_err(), expected);
    assert_eq!(observation.atr14.unwrap_err(), expected);
}
