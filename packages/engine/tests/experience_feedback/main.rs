//! K5 经历反馈验收（任务 20）：真实经历接入信心、忍耐与风险压力。
//!
//! 规定场景（计划任务 20 Acceptance）在三个文件落地：
//! - 本文件：补仓不清除账户损失、清仓再入不抹去冷静期历史、未成交计划
//!   不记失败交易、状态 serde 往返与旧档默认；
//! - `seam.rs`：同损益不同经历在受控风格下选择不同、非恐慌者不必止损
//!   （两者都走既有 `decide_retail_position_with_experience` 读缝，不改
//!   判断函数体）；
//! - `reads.rs`：读侧派生输入（衰减/长期被套/恢复/风险压力）；
//! - `failures/`：负向拒绝（重复计次、未来经历、丢失生命周期、时间回拨）。

mod failures;
mod reads;
mod seam;

use engine::calendar::CivilDate;
use engine::experience::ExperienceMoment;
use engine::{Money, RetailExperienceState, Side, StockCode};

pub(crate) fn code() -> StockCode {
    StockCode("600101".into())
}

pub(crate) fn price(cents: i64) -> Money {
    Money::from_cents(cents)
}

/// 双时钟测试时刻：交易日序与市场分钟独立指定；公历日期随交易日单调推进
/// （每年 336 个"交易日"等价映射，避免月末进位干扰，仅测试用途）。
pub(crate) fn moment(trading_day: u64, market_minute: u64) -> ExperienceMoment {
    let year = 2030 + i32::try_from(trading_day / 336).unwrap();
    let month = 1 + u8::try_from((trading_day % 336) / 28).unwrap();
    let day = 1 + u8::try_from(trading_day % 28).unwrap();
    ExperienceMoment {
        civil_date: CivilDate::from_ymd(year, month, day).unwrap(),
        market_minute,
        trading_day,
    }
}

pub(crate) fn buy(
    state: &mut RetailExperienceState,
    code: &StockCode,
    cents: i64,
    before_qty: u32,
    after_qty: u32,
    order_id: u64,
    trading_day: u64,
    market_minute: u64,
) {
    state
        .record_fill_dated(
            code,
            Side::Buy,
            price(cents),
            before_qty,
            after_qty,
            None,
            Some(order_id),
            moment(trading_day, market_minute),
        )
        .unwrap();
}

pub(crate) fn sell(
    state: &mut RetailExperienceState,
    code: &StockCode,
    cents: i64,
    before_qty: u32,
    after_qty: u32,
    cost_cents: i64,
    order_id: u64,
    trading_day: u64,
    market_minute: u64,
) {
    state
        .record_fill_dated(
            code,
            Side::Sell,
            price(cents),
            before_qty,
            after_qty,
            Some(price(cost_cents)),
            Some(order_id),
            moment(trading_day, market_minute),
        )
        .unwrap();
}

/// 一次完整受挫回合：建仓 → 本人确认不利 → 亏损清仓。consecutive +1、
/// 失败事件 +1、退出历史 +1（realized_profit=false）。
pub(crate) fn failed_round_trip(
    state: &mut RetailExperienceState,
    code: &StockCode,
    order_id: u64,
    trading_day: u64,
    market_minute: u64,
) {
    buy(
        state,
        code,
        1_000,
        0,
        100,
        order_id,
        trading_day,
        market_minute,
    );
    state
        .observe_position_dated(code, price(940), moment(trading_day, market_minute + 1))
        .unwrap();
    sell(
        state,
        code,
        900,
        100,
        0,
        1_000,
        order_id + 1,
        trading_day,
        market_minute + 2,
    );
}

#[test]
fn adding_to_a_position_keeps_account_failure_facts_and_entry_epoch() {
    let code = code();
    let mut state = RetailExperienceState::new(price(1_000_000)).unwrap();
    buy(&mut state, &code, 1_000, 0, 100, 1, 0, 10);
    state
        .observe_position_dated(&code, price(940), moment(0, 11))
        .unwrap();
    assert_eq!(state.feedback.failure_events.len(), 1);
    let entry_epoch_day = state.feedback.stocks[&code].entry_moment.trading_day;

    // 补仓（新订单、加量不清零）：账户失败事实与入场生命周期都不得被清除。
    buy(&mut state, &code, 950, 100, 200, 2, 1, 20);

    assert_eq!(state.consecutive_failed_buys, 1, "补仓不减失败计数");
    assert_eq!(
        state.feedback.failure_events.len(),
        1,
        "补仓不删除已登记的失败事件"
    );
    assert_eq!(
        state.feedback.stocks[&code].entry_moment.trading_day, entry_epoch_day,
        "补仓不重置持仓入场生命周期"
    );

    // 补仓后的新一轮不利观察按新订单确认第二次失败——旧事实仍在。
    state
        .observe_position_dated(&code, price(900), moment(1, 21))
        .unwrap();
    assert_eq!(state.consecutive_failed_buys, 2);
    assert_eq!(state.feedback.failure_events.len(), 2);
    assert_eq!(state.feedback.failure_events[0].order_id, Some(1));
    assert_eq!(state.feedback.failure_events[1].order_id, Some(2));
}

#[test]
fn reentry_after_clearing_keeps_calm_down_history() {
    let code = code();
    let mut state = RetailExperienceState::new(price(1_000_000)).unwrap();
    buy(&mut state, &code, 1_000, 0, 100, 1, 0, 10);
    sell(&mut state, &code, 900, 100, 0, 1_000, 2, 0, 12);

    let cooldown_until = 12 + engine::experience::POST_EXIT_COOLDOWN_MINUTES;
    assert!(state.is_in_post_exit_cooldown(&code, 130));
    assert_eq!(state.feedback.exit_records.len(), 1);
    assert_eq!(
        state.feedback.exit_records[0].cooldown_until_market_minute,
        cooldown_until
    );

    // 冷静期窗口内再入场：活跃冷却按原语义解除，但冷静期历史不抹去。
    buy(&mut state, &code, 800, 0, 100, 3, 1, 130);
    assert!(!state.is_in_post_exit_cooldown(&code, 130));
    assert_eq!(
        state.feedback.exit_records.len(),
        1,
        "再入场不得抹去退出/冷静期历史"
    );
    assert_eq!(
        state.feedback.exit_records[0].cooldown_until_market_minute,
        cooldown_until
    );
    assert_eq!(state.feedback.exit_records[0].realized_profit, false);
}

#[test]
fn unfilled_or_cancelled_orders_never_become_failure_experience() {
    let code = code();
    let mut state = RetailExperienceState::new(price(1_000_000)).unwrap();

    // 订单 77 提交后被撤，从未成交：经历层没有任何写入路径能感知它。
    // 另一真实订单 88 成交并被本人观察确认失败。
    buy(&mut state, &code, 1_000, 0, 100, 88, 0, 10);
    state
        .observe_position_dated(&code, price(940), moment(0, 11))
        .unwrap();

    assert_eq!(state.feedback.failure_events.len(), 1);
    assert_eq!(state.feedback.failure_events[0].order_id, Some(88));
    assert!(
        state
            .feedback
            .failure_events
            .iter()
            .all(|event| event.order_id != Some(77)),
        "未成交订单不得被记为失败交易"
    );
    assert_eq!(state.consecutive_failed_buys, 1);
}

#[test]
fn feedback_state_survives_serde_roundtrip_and_old_saves_default_it() {
    let code = code();
    let mut state = RetailExperienceState::new(price(1_000_000)).unwrap();
    failed_round_trip(&mut state, &code, 1, 0, 0);
    buy(&mut state, &code, 1_000, 0, 100, 3, 1, 5);
    // 960 > 1000×95%：不再触发不利确认，只推进本人所见。
    state
        .observe_position_dated(&code, price(960), moment(2, 60))
        .unwrap();

    let json = serde_json::to_string(&state).unwrap();
    let restored: RetailExperienceState = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, state);
    assert_eq!(restored.feedback.failure_events.len(), 1);
    assert_eq!(restored.feedback.exit_records.len(), 1);
    assert_eq!(restored.feedback.stocks[&code].entry_moment.trading_day, 1);

    // 旧格式存档没有 feedback 字段：恢复边界取默认空反馈，不拒绝、不臆造。
    let mut legacy = serde_json::from_str::<serde_json::Value>(&json).unwrap();
    if let serde_json::Value::Object(map) = &mut legacy {
        map.remove("feedback");
    }
    let legacy_state: RetailExperienceState = serde_json::from_str(&legacy.to_string()).unwrap();
    assert_eq!(legacy_state.feedback, Default::default());
    assert_eq!(legacy_state.consecutive_failed_buys, 1);
}
