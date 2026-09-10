//! 双时钟纪律拒绝（任务 20 QA failure）：事件时间回拨（公历/市场分钟/交易日
//! 三时钟分别拒绝）与评估时刻早于已登记经历（未来经历，双时钟）。

use engine::ExperienceError;
use engine::calendar::CivilDate;
use engine::experience::ExperienceMoment;

use super::err_state;

#[test]
fn civil_date_clock_rejects_backwards_events() {
    let mut state = err_state();
    super::buy(&mut state, &super::code(), 1_000, 0, 100, 1, 10, 100);

    let backwards = ExperienceMoment {
        civil_date: CivilDate::from_ymd(2029, 12, 31).unwrap(),
        market_minute: 200,
        trading_day: 11,
    };
    let err = state
        .observe_position_dated(&super::code(), super::price(950), backwards)
        .unwrap_err();
    assert_eq!(
        err,
        ExperienceError::CivilTimeWentBackwards {
            attempted: CivilDate::from_ymd(2029, 12, 31).unwrap(),
            last: super::moment(10, 100).civil_date,
        }
    );
}

#[test]
fn market_minute_clock_rejects_backwards_events() {
    let mut state = err_state();
    super::buy(&mut state, &super::code(), 1_000, 0, 100, 1, 10, 100);

    let backwards = ExperienceMoment {
        civil_date: super::moment(11, 99).civil_date,
        market_minute: 99,
        trading_day: 11,
    };
    let err = state
        .observe_position_dated(&super::code(), super::price(950), backwards)
        .unwrap_err();
    assert_eq!(
        err,
        ExperienceError::MarketMinuteWentBackwards {
            attempted: 99,
            last: 100,
        }
    );
    // 峰值观察被拒绝后不得推进任何状态。
    assert_eq!(
        state.stocks[&super::code()].peak_price_since_entry,
        Some(super::price(1_000))
    );
}

#[test]
fn trading_day_clock_rejects_backwards_events() {
    let mut state = err_state();
    super::buy(&mut state, &super::code(), 1_000, 0, 100, 1, 10, 100);

    let backwards = ExperienceMoment {
        civil_date: super::moment(10, 200).civil_date,
        market_minute: 200,
        trading_day: 9,
    };
    let err = state
        .observe_position_dated(&super::code(), super::price(950), backwards)
        .unwrap_err();
    assert_eq!(
        err,
        ExperienceError::TradingDayWentBackwards {
            attempted: 9,
            last: 10,
        }
    );
}

#[test]
fn as_of_before_recorded_experience_is_rejected_on_both_clocks() {
    let code = super::code();
    let mut state = err_state();
    super::buy(&mut state, &code, 1_000, 0, 100, 1, 3, 300);
    state
        .observe_position_dated(&code, super::price(940), super::moment(3, 301))
        .unwrap();

    // 公历时钟落后于已登记经历：拒绝评估（未来经历守卫）。
    let past_civil = ExperienceMoment {
        civil_date: CivilDate::from_ymd(2029, 12, 31).unwrap(),
        market_minute: 400,
        trading_day: 3,
    };
    assert_eq!(
        state.failure_influence(&past_civil).unwrap_err(),
        ExperienceError::AsOfBeforeLatestEvent {
            as_of: past_civil,
            latest: super::moment(3, 301),
        }
    );

    // 市场分钟时钟落后：同样拒绝。
    let past_minute = ExperienceMoment {
        civil_date: super::moment(3, 400).civil_date,
        market_minute: 300,
        trading_day: 3,
    };
    assert!(matches!(
        state.failure_influence(&past_minute).unwrap_err(),
        ExperienceError::AsOfBeforeLatestEvent { .. }
    ));

    // 交易日时钟落后：同样拒绝（衰减时间不得回拨）。
    let past_day = ExperienceMoment {
        civil_date: super::moment(3, 400).civil_date,
        market_minute: 400,
        trading_day: 2,
    };
    assert!(matches!(
        state.failure_influence(&past_day).unwrap_err(),
        ExperienceError::AsOfBeforeLatestEvent { .. }
    ));

    // 长期被套读取同样受双时钟守卫约束。
    assert!(matches!(
        state
            .is_long_stuck(&code, super::price(1_000), &past_minute)
            .unwrap_err(),
        ExperienceError::AsOfBeforeLatestEvent { .. }
    ));
}
