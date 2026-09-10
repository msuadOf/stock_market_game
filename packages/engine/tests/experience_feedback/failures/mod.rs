//! 负向拒绝（任务 20 QA failure）：部分成交重复计次、丢失持仓生命周期、
//! 非法金额/溢出与多股隔离。双时钟回拨与未来经历在 `clocks.rs`。

mod clocks;

use engine::experience::ExitRecord;
use engine::{ExperienceError, Money, RetailExperienceState, Side, StockCode};

use super::{buy, code, moment, price};

fn err_state() -> RetailExperienceState {
    RetailExperienceState::new(price(1_000_000)).unwrap()
}

#[test]
fn partial_fills_of_one_order_confirm_failure_exactly_once() {
    let code = code();
    let mut state = err_state();
    // 同一订单 77 跨 tick 部分成交：心理上只是一次买入（部分成交后参照价=950）。
    buy(&mut state, &code, 1_000, 0, 100, 77, 0, 10);
    state
        .record_fill_dated(
            &code,
            Side::Buy,
            price(950),
            100,
            200,
            Some(price(1_000)),
            Some(77),
            moment(0, 11),
        )
        .unwrap();

    state
        .observe_position_dated(&code, price(900), moment(0, 12))
        .unwrap();
    assert_eq!(state.consecutive_failed_buys, 1);
    assert_eq!(state.feedback.failure_events.len(), 1);

    // 同一受挫内的后续观察不得重复计次（legacy 旗标 + 失败登记都不动）。
    state
        .observe_position_dated(&code, price(880), moment(0, 13))
        .unwrap();
    assert_eq!(state.consecutive_failed_buys, 1);
    assert_eq!(
        state.feedback.failure_events.len(),
        1,
        "部分成交/同回合观察不得重复登记失败"
    );

    // 新订单买入开启新的可确认回合——那才是第二次失败。
    buy(&mut state, &code, 1_000, 200, 300, 78, 0, 20);
    state
        .observe_position_dated(&code, price(940), moment(0, 21))
        .unwrap();
    assert_eq!(state.consecutive_failed_buys, 2);
    assert_eq!(state.feedback.failure_events.len(), 2);
}

#[test]
fn buy_from_zero_over_an_active_entry_is_rejected() {
    // 丢失退出生命周期：上一段持仓从未清仓却又出现从零建仓。
    let code = code();
    let mut state = err_state();
    buy(&mut state, &code, 1_000, 0, 100, 1, 0, 10);

    let err = state
        .record_fill_dated(
            &code,
            Side::Buy,
            price(900),
            0,
            100,
            None,
            Some(2),
            moment(1, 20),
        )
        .unwrap_err();
    assert_eq!(
        err,
        ExperienceError::ActiveEntryAlreadyExists {
            code: code.0.clone()
        }
    );
    assert!(
        state.feedback.stocks.contains_key(&code),
        "拒绝不得破坏既有生命周期"
    );
}

#[test]
fn exit_without_an_active_entry_is_rejected() {
    let code = code();
    let mut state = err_state();
    let err = state
        .record_fill_dated(
            &code,
            Side::Sell,
            price(900),
            100,
            0,
            Some(price(1_000)),
            Some(1),
            moment(0, 10),
        )
        .unwrap_err();
    assert_eq!(
        err,
        ExperienceError::NoActiveEntry {
            code: code.0.clone()
        }
    );
    assert!(state.feedback.exit_records.is_empty());
}

#[test]
fn observation_without_an_active_entry_is_rejected() {
    // 未持仓/未建仓的观察走关注列表语义，不得凭空生成持仓观察或失败经历。
    let code = code();
    let mut state = err_state();
    let err = state
        .observe_position_dated(&code, price(940), moment(0, 10))
        .unwrap_err();
    assert_eq!(
        err,
        ExperienceError::NoActiveEntry {
            code: code.0.clone()
        }
    );
    assert!(state.feedback.failure_events.is_empty());
    assert_eq!(state.consecutive_failed_buys, 0);

    // 长期被套读取对无生命周期股票同样是类型化拒绝，不静默返回 false。
    assert_eq!(
        state
            .is_long_stuck(&code, price(1_000), &moment(0, 11))
            .unwrap_err(),
        ExperienceError::NoActiveEntry {
            code: code.0.clone()
        }
    );
}

#[test]
fn non_positive_prices_and_costs_are_rejected() {
    let code = code();
    let mut state = err_state();
    buy(&mut state, &code, 1_000, 0, 100, 1, 0, 10);

    assert!(matches!(
        state
            .observe_position_dated(&code, Money::from_cents(0), moment(0, 11))
            .unwrap_err(),
        ExperienceError::NonPositiveMoney {
            field: "observed position price",
            ..
        }
    ));
    assert!(matches!(
        state
            .is_long_stuck(&code, Money::from_cents(0), &moment(0, 12))
            .unwrap_err(),
        ExperienceError::NonPositiveMoney {
            field: "long-stuck cost",
            ..
        }
    ));
}

#[test]
fn inconsistent_restored_feedback_fails_validation() {
    // 恢复边界（任务 27 接线）前的手工篡改：乱序/越界登记必须被 validate 拒绝。
    let code = code();
    let mut state = err_state();
    buy(&mut state, &code, 1_000, 0, 100, 1, 5, 50);
    state
        .observe_position_dated(&code, price(940), moment(6, 51))
        .unwrap();
    assert_eq!(state.feedback.validate(), Ok(()));

    let mut tampered = state.feedback.clone();
    tampered.failure_events[0].moment.trading_day = 999; // 晚于最新时刻
    assert!(matches!(
        tampered.validate(),
        Err(ExperienceError::InconsistentFeedback { .. })
    ));

    let mut no_cooldown = state.feedback.clone();
    no_cooldown.exit_records.push(ExitRecord {
        code: code.clone(),
        cooldown_until_market_minute: 10,
        realized_profit: false,
        moment: moment(6, 51),
    });
    assert!(matches!(
        no_cooldown.validate(),
        Err(ExperienceError::InconsistentFeedback { .. })
    ));
}

#[test]
fn cooldown_minute_overflow_propagates_without_touching_feedback() {
    let code = code();
    let mut state = err_state();
    buy(&mut state, &code, 1_000, 0, 100, 1, 0, u64::MAX - 10);

    let err = state
        .record_fill_dated(
            &code,
            Side::Sell,
            price(900),
            100,
            0,
            Some(price(1_000)),
            Some(2),
            moment(0, u64::MAX - 9),
        )
        .unwrap_err();
    assert!(matches!(err, ExperienceError::MarketMinuteOverflow { .. }));
    assert!(
        state.feedback.stocks.contains_key(&code),
        "清仓被拒时生命周期不得被移除"
    );
    assert!(state.feedback.exit_records.is_empty());
}

#[test]
fn dedup_guard_uses_stock_code_from_the_fill_not_a_stale_copy() {
    // 多股并存时各股生命周期互不串扰：B 股建仓不得被 A 股既有周期拦截，
    // 也不得让 A 股的失败登记记到 B 股头上。
    let a = StockCode("600101".into());
    let b = StockCode("600102".into());
    let mut state = err_state();
    buy(&mut state, &a, 1_000, 0, 100, 1, 0, 10);
    buy(&mut state, &b, 2_000, 0, 100, 2, 0, 10);

    state
        .observe_position_dated(&b, price(1_890), moment(0, 11))
        .unwrap(); // 1890×100 ≤ 2000×95
    assert_eq!(state.feedback.failure_events.len(), 1);
    assert_eq!(state.feedback.failure_events[0].code, b);
    assert_eq!(state.feedback.failure_events[0].order_id, Some(2));
    assert_eq!(state.consecutive_failed_buys, 1);
}
