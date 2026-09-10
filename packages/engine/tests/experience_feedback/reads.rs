//! 读侧派生输入（任务 22/23 的目标/紧迫度接缝）：失败影响衰减、长期被套、
//! 真实获利恢复与风险压力复用账户峰值。

use engine::experience::{FAILURE_DECAY_TRADING_DAYS, LONG_STUCK_TRADING_DAYS};
use engine::{RetailExperienceState, Side};

use super::{buy, code, failed_round_trip, moment, price, sell};

#[test]
fn failure_influence_decays_one_tier_per_twenty_trading_days() {
    let code = code();
    let mut state = RetailExperienceState::new(price(1_000_000)).unwrap();
    // 三次真实受挫都发生在交易日 0；衰减锚点是最后一次受挫。
    for round in 0..3_u64 {
        failed_round_trip(&mut state, &code, 10 + round * 2, 0, round * 10);
    }
    assert_eq!(state.consecutive_failed_buys, 3);

    let influence = |day: u64| state.failure_influence(&moment(day, 500)).unwrap();

    assert_eq!(influence(0), 3, "衰减未满一档时影响不变");
    assert_eq!(influence(FAILURE_DECAY_TRADING_DAYS - 1), 3);
    assert_eq!(
        influence(FAILURE_DECAY_TRADING_DAYS),
        2,
        "满 20 个交易日无新受挫，减弱一档"
    );
    assert_eq!(influence(FAILURE_DECAY_TRADING_DAYS * 2), 1);
    assert_eq!(influence(FAILURE_DECAY_TRADING_DAYS * 3), 0, "影响衰减到零");
    assert_eq!(
        influence(FAILURE_DECAY_TRADING_DAYS * 10),
        0,
        "衰减饱和于零"
    );

    // 衰减只调影响档：真实亏损事实（计数与登记）永不删除。
    assert_eq!(state.consecutive_failed_buys, 3);
    assert_eq!(state.feedback.failure_events.len(), 3);
}

#[test]
fn failure_influence_without_dated_events_stays_at_the_live_count() {
    // 未接双时钟事件（遗留路径）：没有失败事件日期就没有可衰减的锚点，
    // 影响恒等于当前计数——不虚构衰减，也不清零。
    let mut state = RetailExperienceState::new(price(1_000_000)).unwrap();
    state.consecutive_failed_buys = 2;
    assert_eq!(state.failure_influence(&moment(500, 500)).unwrap(), 2);
}

#[test]
fn long_stuck_requires_twenty_trading_days_and_below_cost_observation() {
    let code = code();
    let mut state = RetailExperienceState::new(price(1_000_000)).unwrap();
    buy(&mut state, &code, 1_000, 0, 100, 1, 0, 10);

    let cost = price(1_000);
    let stuck = |state: &RetailExperienceState, day: u64| {
        state.is_long_stuck(&code, cost, &moment(day, 500)).unwrap()
    };

    state
        .observe_position_dated(&code, price(950), moment(5, 20))
        .unwrap();
    assert!(
        !stuck(&state, LONG_STUCK_TRADING_DAYS - 1),
        "持有不满 20 交易日不算长期被套"
    );
    assert!(
        stuck(&state, LONG_STUCK_TRADING_DAYS),
        "满 20 交易日且最近本人观察低于成本"
    );

    // 本人观察回到成本之上：被套状态解除（判定用当前成本，观察事实保留）。
    state
        .observe_position_dated(&code, price(1_050), moment(LONG_STUCK_TRADING_DAYS, 30))
        .unwrap();
    assert!(!stuck(&state, LONG_STUCK_TRADING_DAYS));

    // 观察价介于新旧成本之间：以读取时传入的权威成本判定。
    state
        .observe_position_dated(&code, price(1_020), moment(LONG_STUCK_TRADING_DAYS + 1, 40))
        .unwrap();
    assert!(!stuck(&state, LONG_STUCK_TRADING_DAYS + 1));
    assert!(
        state
            .is_long_stuck(
                &code,
                price(1_025),
                &moment(LONG_STUCK_TRADING_DAYS + 1, 41)
            )
            .unwrap()
    );
}

#[test]
fn profitable_exit_recovers_failure_influence() {
    // 恢复规则：真实获利退出把失败计数减一（遗留语义），影响随之恢复；
    // 退出历史登记 realized_profit 事实。
    let code = code();
    let mut state = RetailExperienceState::new(price(1_000_000)).unwrap();
    buy(&mut state, &code, 1_000, 0, 100, 1, 0, 10);
    state
        .observe_position_dated(&code, price(940), moment(0, 11))
        .unwrap();
    assert_eq!(state.failure_influence(&moment(0, 12)).unwrap(), 1);

    sell(&mut state, &code, 1_100, 100, 0, 1_000, 2, 0, 12);
    assert_eq!(state.consecutive_failed_buys, 0);
    assert_eq!(state.failure_influence(&moment(500, 500)).unwrap(), 0);
    assert_eq!(
        state.feedback.failure_events.len(),
        1,
        "获利退出不删除失败事实"
    );
    assert_eq!(state.feedback.exit_records.len(), 1);
    assert!(state.feedback.exit_records[0].realized_profit);
}

#[test]
fn opening_allocation_counts_toward_long_stuck() {
    // 开局分配的真实持仓也是经历：被套计时从分配日起。
    let code = code();
    let mut state = RetailExperienceState::new(price(1_000_000)).unwrap();
    state
        .initialize_holding_dated(&code, Some(price(1_000)), price(1_000), moment(0, 1))
        .unwrap();
    state
        .observe_position_dated(&code, price(900), moment(LONG_STUCK_TRADING_DAYS, 2))
        .unwrap();
    assert!(
        state
            .is_long_stuck(&code, price(1_000), &moment(LONG_STUCK_TRADING_DAYS, 3))
            .unwrap()
    );
}

#[test]
fn experience_drawdown_reuses_the_observed_equity_peak() {
    // 风险压力输入复用既有账户峰值字段，不另建一份账户损益。
    let mut state = RetailExperienceState::new(price(1_000_000)).unwrap();
    assert_eq!(
        state.experience_drawdown_from_peak(price(1_000_000)),
        Some(0.0)
    );

    state.observe_equity(price(1_100_000)).unwrap();
    let drawdown = state.experience_drawdown_from_peak(price(990_000)).unwrap();
    assert!((drawdown - 0.1).abs() < 1e-9, "drawdown = {drawdown}");

    // 峰值只升不降：权益超过旧峰值时回撤为负，由消费方按阈值解释。
    let above = state
        .experience_drawdown_from_peak(price(1_200_000))
        .unwrap();
    assert!(above < 0.0);

    let no_reference = RetailExperienceState::without_equity_reference();
    assert_eq!(
        no_reference.experience_drawdown_from_peak(price(1_000)),
        None
    );
}

#[test]
fn fills_themselves_are_own_observations_for_long_stuck() {
    // 成交价是本人亲历价：不加观察调用也能构成低于成本的本人观察，
    // 判定时以读取方传入的当前权威成本为准。
    let code = code();
    let mut state = RetailExperienceState::new(price(1_000_000)).unwrap();
    buy(&mut state, &code, 1_200, 0, 100, 1, 0, 10);
    state
        .record_fill_dated(
            &code,
            Side::Buy,
            price(900),
            100,
            200,
            Some(price(1_200)),
            Some(2),
            moment(LONG_STUCK_TRADING_DAYS, 11),
        )
        .unwrap();
    let epoch = &state.feedback.stocks[&code];
    assert_eq!(
        epoch.last_own_observation.as_ref().unwrap().price,
        price(900)
    );
    assert!(
        state
            .is_long_stuck(&code, price(1_000), &moment(LONG_STUCK_TRADING_DAYS, 12))
            .unwrap(),
        "最近本人所见 900 < 权威成本 1000 且持有满 20 交易日"
    );
}
