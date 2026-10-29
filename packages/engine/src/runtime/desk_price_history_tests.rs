//! price_history 只记录连续竞价逻辑 tick；Continue 与 Abort 保留精确值。

use super::*;

fn state(history_len: usize, ticks_per_day: u64, auction: u64, closing: u64) -> DeskState {
    DeskState::new(
        DeskSetup {
            stock: 0,
            exchange: StockExchange::Shanghai,
            initial_price: 1000,
            limit_bps: 1000,
            max_order_qty_limit: 1_000_000,
            max_order_qty_market: 150_000,
            ticks_per_day,
            auction_ticks: auction,
            closing_auction_ticks: closing,
            order_ttl_ticks: 10,
            history_len,
            initial_tick: 0,
            initial_orders: Vec::new(),
            initial_recent_prices: Vec::new(),
            initial_daily_bars: Vec::new(),
            initial_traded_days: 0,
            initial_market: None,
            initial_public_cutoff: 0,
            initial_next_serial: 1,
        },
        1,
        Arc::new(GameConfig::proposed_defaults()),
    )
}

fn begin(state: &mut DeskState, tick: u64) {
    let context = super::super::calendar_test_support::open_tick(
        tick,
        state.setup.ticks_per_day,
        state.setup.auction_ticks,
        state.setup.closing_auction_ticks,
        1,
        crate::CivilDate::from_iso("2026-01-05").unwrap(),
    );
    state.begin_tick(tick, context).unwrap();
}

fn history(state: &DeskState) -> Vec<i64> {
    state.market.recent_prices.iter().copied().collect()
}

#[test]
fn history_samples_before_continuous_phase_and_excludes_both_auction_windows() {
    let mut state = state(8, 8, 3, 2);
    let mut expected = Vec::new();
    for tick in 1..=8 {
        begin(&mut state, tick);
        state.market.last_price = 1000 + tick as i64;
        state.finish_round(tick).unwrap();
        if (4..=6).contains(&tick) {
            expected.push(1000 + tick as i64);
        }
        assert_eq!(history(&state), expected, "实际执行相位使用 tick-1：{tick}");
        assert_eq!(
            state.public_snapshot_arc(tick).unwrap().recent_prices,
            expected
        );
    }
}

fn finish_three_rounds(state: &mut DeskState) {
    for (round, price) in [1001, 1002, 1003].into_iter().enumerate() {
        state.cur_round = round as u32;
        state.market.last_price = price;
        state.finish_round(1).unwrap();
    }
}

#[test]
fn continuous_continue_keeps_one_sample_with_the_final_round_price() {
    let mut state = state(4, 10, 0, 0);
    state.market.recent_prices = VecDeque::from([900, 950]);
    begin(&mut state, 1);
    finish_three_rounds(&mut state);
    assert_eq!(history(&state), vec![900, 950, 1003]);
    assert_eq!(
        state.public_snapshot_arc(1).unwrap().recent_prices,
        history(&state)
    );
}

#[test]
fn full_window_continue_abort_restores_every_original_price() {
    let mut state = state(3, 10, 0, 0);
    state.market.recent_prices = VecDeque::from([900, 925, 950]);
    let before = state.state_hash();
    begin(&mut state, 1);
    finish_three_rounds(&mut state);
    assert_eq!(history(&state), vec![925, 950, 1003]);
    state.rollback();
    assert_eq!(history(&state), vec![900, 925, 950]);
    assert_eq!(
        state.state_hash(),
        before,
        "Abort 必须恢复整值，不能只恢复长度"
    );
    begin(&mut state, 1);
    state.market.last_price = 1004;
    state.finish_round(1).unwrap();
    assert_eq!(history(&state), vec![925, 950, 1004]);
}

#[test]
fn rotating_a_full_nonuniform_history_invalidates_the_public_view() {
    let mut state = state(2, 1000, 0, 0);
    state.market.recent_prices = VecDeque::from([1000, 1200]);
    state.market.candle = Some(DeskCandle {
        open: 1000,
        high: 1200,
        low: 1000,
        close: 1000,
        volume: 0,
    });
    begin(&mut state, 1);
    let before = state.public_snapshot_arc(1).unwrap();
    state.finish_round(1).unwrap();
    let after = state.public_snapshot_arc(1).unwrap();
    assert_eq!(before.recent_prices, vec![1000, 1200]);
    assert_eq!(after.recent_prices, vec![1200, 1000]);
    assert!(!Arc::ptr_eq(&before, &after));
}

#[test]
fn direct_zero_capacity_history_is_rejected_before_sampling() {
    let mut state = state(0, 10, 0, 0);
    begin(&mut state, 1);
    let error = state.finish_round(1).unwrap_err();
    assert!(
        matches!(error, RuntimeError::ProtocolInvariant { detail } if detail.contains("history_len"))
    );
    assert!(state.market.recent_prices.is_empty());
}
