//! 指标独立金样：固定价格序列手算精确值 + 短期下跌与长期上行同时表达。

use engine::observation::build_technical_observation;
use engine::strategy::{
    atr14, rsi14, sma, AverageTrueRange, RelativeStrengthIndex, SimpleMovingAverage,
    SMA_LONG_WINDOW, SMA_SHORT_WINDOW,
};

use super::{closes, flat_bar, kbar, price};

#[test]
fn sma20_of_arithmetic_series_matches_hand_computed_average() {
    // 收盘 100,200,...,2000 分：和 = (100+2000)*20/2 = 21000，均值 = 1050 分（整除）。
    let series: Vec<i64> = (1..=20).map(|i| i * 100).collect();
    let result = sma(&closes(&series), SMA_SHORT_WINDOW).unwrap();
    assert_eq!(
        result,
        SimpleMovingAverage {
            window: 20,
            average: price(1050),
        }
    );
}

#[test]
fn sma60_of_arithmetic_series_matches_hand_computed_average() {
    // 收盘 100,200,...,6000 分：和 = (100+6000)*60/2 = 183000，均值 = 3050 分。
    let series: Vec<i64> = (1..=60).map(|i| i * 100).collect();
    let result = sma(&closes(&series), SMA_LONG_WINDOW).unwrap();
    assert_eq!(result.window, 60);
    assert_eq!(result.average, price(3050));
}

#[test]
fn sma_rounds_half_even_at_exact_half_cent() {
    // 19×100 + 110：和 2010，2010/20 = 100.5 → 半偶舍入到 100（偶）。
    let mut down: Vec<i64> = vec![100; 19];
    down.push(110);
    assert_eq!(
        sma(&closes(&down), 20).unwrap().average,
        price(100),
        "100.5 must round to the even neighbor 100"
    );

    // 19×101 + 111：和 2030，2030/20 = 101.5 → 半偶舍入到 102（偶）。
    let mut up: Vec<i64> = vec![101; 19];
    up.push(111);
    assert_eq!(
        sma(&closes(&up), 20).unwrap().average,
        price(102),
        "101.5 must round to the even neighbor 102"
    );
}

#[test]
fn sma_uses_only_the_most_recent_window_samples() {
    // 21 个收盘：首个离群 1 分，最近 20 个恒为 1000 → 均值恰为 1000。
    let mut series = vec![1_i64];
    series.extend(std::iter::repeat_n(1000, 20));
    assert_eq!(sma(&closes(&series), 20).unwrap().average, price(1000));
}

#[test]
fn rsi_is_100_for_pure_rises_and_0_for_pure_falls() {
    // 15 个收盘每步 +100：Σ涨=1400，Σ跌=0 → RSI = 100。
    let rising: Vec<i64> = (0..15).map(|i| 1000 + i * 100).collect();
    let up = rsi14(&closes(&rising)).unwrap();
    assert_eq!(up.value, 100);
    assert!(!up.all_flat);
    assert_eq!(up.valid_samples, 14);

    // 15 个收盘每步 -100：Σ涨=0，Σ跌=1400 → RSI = 0（分母非零，不是全平）。
    let falling: Vec<i64> = (0..15).map(|i| 2400 - i * 100).collect();
    let down = rsi14(&closes(&falling)).unwrap();
    assert_eq!(down.value, 0);
    assert!(!down.all_flat);
}

#[test]
fn rsi_balanced_gains_and_losses_are_exactly_50() {
    // 1000→1100→1000 交替 7 轮：Σ涨=Σ跌=700 → 100*700/1400 = 50（整除）。
    let mut series = Vec::with_capacity(15);
    let mut current = 1000_i64;
    series.push(current);
    for step in 0..14 {
        current += if step % 2 == 0 { 100 } else { -100 };
        series.push(current);
    }
    let result = rsi14(&closes(&series)).unwrap();
    assert_eq!(result.value, 50);
    assert!(!result.all_flat);
    assert_eq!(result.valid_samples, 14);
}

#[test]
fn rsi_rounds_two_thirds_up_and_half_even_down() {
    // +100/-50 交替 7 轮：Σ涨=700，Σ跌=350 → 100*700/1050 = 66.67 → 67。
    let mut series = vec![1000_i64];
    let mut current = 1000_i64;
    for step in 0..14 {
        current += if step % 2 == 0 { 100 } else { -50 };
        series.push(current);
    }
    assert_eq!(rsi14(&closes(&series)).unwrap().value, 67);

    // 一次 +5、一次 -3、其余 12 步全平：100*5/8 = 62.5 → 半偶舍入到 62。
    let mixed = [
        1000, 1005, 1002, 1002, 1002, 1002, 1002, 1002, 1002, 1002, 1002, 1002, 1002, 1002, 1002,
    ];
    assert_eq!(
        rsi14(&closes(&mixed)).unwrap().value,
        62,
        "62.5 must round to the even neighbor 62"
    );
}

#[test]
fn rsi_all_flat_with_fourteen_valid_samples_is_exactly_neutral() {
    // 15 个相同收盘 = 14 个有效涨跌样本全为零：分母为零被显式定义为中性 50，
    // 并携带有效样本标记，不得伪装成历史不足或虚构涨跌。
    let flat = vec![1000_i64; 15];
    let result = rsi14(&closes(&flat)).unwrap();
    assert_eq!(
        result,
        RelativeStrengthIndex {
            window: 14,
            valid_samples: 14,
            value: 50,
            all_flat: true,
        }
    );
}

#[test]
fn atr14_uses_true_ranges_against_previous_close() {
    // 15 根日 K：close_i = 1000+10i，high = close+15，low = close-5。
    // TR_i = max(high-low=20, |high-close_prev|=25, |low-close_prev|=5) = 25。
    // ATR = 14×25/14 = 25 分（整除）。
    let bars: Vec<_> = (0..15)
        .map(|i| {
            let close = 1000 + i * 10;
            kbar(close + 15, close - 5, close)
        })
        .collect();
    let result = atr14(&bars).unwrap();
    assert_eq!(
        result,
        AverageTrueRange {
            window: 14,
            average: price(25),
        }
    );
}

#[test]
fn atr14_rounds_half_even_at_half_cent() {
    // 收盘恒 1000；13 根 (1001,999) 的 TR=2 + 1 根 (1004,995) 的 TR=9：
    // (13*2+9)/14 = 35/14 = 2.5 → 半偶舍入到 2。
    let mut bars = Vec::with_capacity(15);
    for i in 0..15 {
        if i == 5 {
            bars.push(kbar(1004, 995, 1000));
        } else {
            bars.push(kbar(1001, 999, 1000));
        }
    }
    assert_eq!(atr14(&bars).unwrap().average, price(2));
}

#[test]
fn atr14_takes_only_the_last_fourteen_true_ranges() {
    // 16 根日 K：首根离群 (5000,100,1000) 自身不产生 TR 样本；其后 14 根平坦
    // (1001,999,1000) 每根 TR = max(2,1,1) = 2 → ATR = 2，证明只取最近 14 个
    // 真实波幅且第 1 个样本以前一根收盘为基准。
    let mut bars = vec![kbar(5000, 100, 1000)];
    bars.extend(std::iter::repeat_n(kbar(1001, 999, 1000), 15));
    assert_eq!(atr14(&bars).unwrap().average, price(2));
}

#[test]
fn short_decline_and_long_rise_are_simultaneously_expressible() {
    // 80 个交易日：前 60 日缓升 1000+10i（至 1590），后 20 日急跌每步 -50（至 590）。
    // SMA20（只含下跌腿）= (1540+590)/2 = 1065；SMA60（含 40 个上涨 + 20 个下跌）
    // = (1395*40 + 1065*20)/60 = 1285。短期弱势与长期高位同时可得，互不覆盖。
    let series: Vec<i64> = (0..80)
        .map(|i| {
            if i < 60 {
                1000 + i * 10
            } else {
                1590 - 50 * (i - 59)
            }
        })
        .collect();
    let bars: Vec<_> = series
        .iter()
        .enumerate()
        .map(|(day, close)| flat_bar(day as u32, *close, 500))
        .collect();

    let observation = build_technical_observation(&bars, 80).unwrap();
    assert_eq!(observation.valid_sample_count, 80);
    assert_eq!(observation.sma20.as_ref().unwrap().average, price(1065));
    assert_eq!(observation.sma60.as_ref().unwrap().average, price(1285));
    // 最近 14 步全部 -50：RSI = 0；短期与长期观测同时成立。
    assert_eq!(observation.rsi14.as_ref().unwrap().value, 0);
    assert!(observation.atr14.as_ref().unwrap().average.cents() >= 0);
}

#[test]
fn technical_observation_excludes_no_trade_days_from_samples() {
    // 21 个连续交易日，第 10 日无成交（volume=0、收盘 9999 为占位）：
    // 其余 20 日真实收盘恒 1000。无成交日不是真实已发生行情，被排除后
    // 恰有 20 个有效样本，SMA20 = 1000；RSI 全平 = 50 且带有效样本。
    let bars: Vec<_> = (0..21)
        .map(|day| {
            if day == 10 {
                flat_bar(day, 9999, 0)
            } else {
                flat_bar(day, 1000, 500)
            }
        })
        .collect();

    let observation = build_technical_observation(&bars, 21).unwrap();
    assert_eq!(observation.valid_sample_count, 20);
    assert_eq!(observation.sma20.as_ref().unwrap().average, price(1000));
    let rsi = observation.rsi14.as_ref().unwrap();
    assert_eq!((rsi.value, rsi.all_flat), (50, true));
}
