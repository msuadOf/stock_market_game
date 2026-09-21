use std::collections::BTreeSet;

use engine::{Money, RetailExperienceState, Side, StockCode};

fn code() -> StockCode {
    StockCode("600101".into())
}

#[test]
fn one_filled_buy_records_only_one_adverse_episode_until_another_fill() {
    let code = code();
    let mut state = RetailExperienceState::new(Money::from_cents(1_000_000)).unwrap();
    state
        .record_fill(&code, Side::Buy, Money::from_cents(1_000), 0, 100, None, 10)
        .unwrap();

    state
        .observe_position(&code, Money::from_cents(940), 20)
        .unwrap();
    state
        .observe_position(&code, Money::from_cents(900), 30)
        .unwrap();
    assert_eq!(state.consecutive_failed_buys, 1);

    state
        .record_fill(
            &code,
            Side::Buy,
            Money::from_cents(900),
            100,
            200,
            Some(Money::from_cents(1_000)),
            31,
        )
        .unwrap();
    state
        .observe_position(&code, Money::from_cents(850), 40)
        .unwrap();
    assert_eq!(state.consecutive_failed_buys, 2);
}

#[test]
fn partial_fills_of_one_order_do_not_reset_an_adverse_episode() {
    let code = code();
    let mut state = RetailExperienceState::new(Money::from_cents(1_000_000)).unwrap();
    state
        .record_fill_with_order(
            &code,
            Side::Buy,
            Money::from_cents(1_000),
            0,
            100,
            None,
            10,
            Some(77),
        )
        .unwrap();
    state
        .observe_position(&code, Money::from_cents(940), 11)
        .unwrap();
    state
        .record_fill_with_order(
            &code,
            Side::Buy,
            Money::from_cents(950),
            100,
            200,
            Some(Money::from_cents(1_000)),
            12,
            Some(77),
        )
        .unwrap();
    state
        .observe_position(&code, Money::from_cents(900), 13)
        .unwrap();

    assert_eq!(state.consecutive_failed_buys, 1);
}

#[test]
fn unheld_observation_is_a_watchlist_entry_without_a_trade_reference() {
    let code = code();
    let mut state = RetailExperienceState::new(Money::from_cents(1_000_000)).unwrap();
    state.observe_stock(&code, 42);

    let stock = &state.stocks[&code];
    assert_eq!(stock.last_observed_market_minute, 42);
    assert_eq!(stock.entry_reference_price, None);
    assert_eq!(stock.last_buy_price, None);
}

#[test]
fn exit_starts_cooldown_and_reentry_resets_stock_reference_but_keeps_account_experience() {
    let code = code();
    let mut state = RetailExperienceState::new(Money::from_cents(1_000_000)).unwrap();
    state
        .record_fill(&code, Side::Buy, Money::from_cents(1_000), 0, 100, None, 10)
        .unwrap();
    state
        .observe_position(&code, Money::from_cents(900), 11)
        .unwrap();
    state
        .record_fill(
            &code,
            Side::Sell,
            Money::from_cents(900),
            100,
            0,
            Some(Money::from_cents(1_000)),
            12,
        )
        .unwrap();

    assert_eq!(state.consecutive_failed_buys, 1);
    assert!(state.is_in_post_exit_cooldown(&code, 12));
    let exited = state.stocks.get(&code).unwrap();
    assert_eq!(exited.entry_reference_price, None);
    assert_eq!(exited.peak_price_since_entry, None);

    state
        .record_fill(&code, Side::Buy, Money::from_cents(800), 0, 100, None, 200)
        .unwrap();
    let reentered = state.stocks.get(&code).unwrap();
    assert_eq!(
        reentered.entry_reference_price,
        Some(Money::from_cents(800))
    );
    assert_eq!(
        reentered.peak_price_since_entry,
        Some(Money::from_cents(800))
    );
    assert!(!state.is_in_post_exit_cooldown(&code, 200));
    assert_eq!(state.consecutive_failed_buys, 1);
}

#[test]
fn profitable_realized_trade_repairs_confidence_without_erasing_the_reference_equity() {
    let code = code();
    let reference = Money::from_cents(1_000_000);
    let mut state = RetailExperienceState::new(reference).unwrap();
    state.consecutive_failed_buys = 2;
    state
        .record_fill(&code, Side::Buy, Money::from_cents(1_000), 0, 100, None, 1)
        .unwrap();
    state
        .record_fill(
            &code,
            Side::Sell,
            Money::from_cents(1_100),
            100,
            0,
            Some(Money::from_cents(1_000)),
            2,
        )
        .unwrap();

    assert_eq!(state.consecutive_failed_buys, 1);
    assert_eq!(state.reference_equity, Some(reference));
}

#[test]
fn account_and_position_peaks_only_move_up_and_survive_serde_roundtrip() {
    let code = code();
    let mut state = RetailExperienceState::new(Money::from_cents(1_000_000)).unwrap();
    state
        .record_fill(&code, Side::Buy, Money::from_cents(1_000), 0, 100, None, 1)
        .unwrap();
    state.observe_equity(Money::from_cents(1_100_000)).unwrap();
    state.observe_equity(Money::from_cents(1_050_000)).unwrap();
    state
        .observe_position(&code, Money::from_cents(1_200), 2)
        .unwrap();
    state
        .observe_position(&code, Money::from_cents(1_100), 3)
        .unwrap();

    assert_eq!(state.peak_equity, Some(Money::from_cents(1_100_000)));
    assert_eq!(
        state.stocks[&code].peak_price_since_entry,
        Some(Money::from_cents(1_200))
    );
    let restored: RetailExperienceState =
        serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    assert_eq!(restored, state);
}

#[test]
fn watchlist_pruning_keeps_every_holding_and_only_the_most_recent_unheld_stocks() {
    let mut state = RetailExperienceState::new(Money::from_cents(1_000_000)).unwrap();
    for index in 0..12_u32 {
        let code = StockCode(format!("60{index:04}"));
        state
            .record_fill(
                &code,
                Side::Buy,
                Money::from_cents(1_000),
                0,
                100,
                None,
                u64::from(index),
            )
            .unwrap();
        state
            .record_fill(
                &code,
                Side::Sell,
                Money::from_cents(1_000),
                100,
                0,
                Some(Money::from_cents(1_000)),
                u64::from(index),
            )
            .unwrap();
    }
    let held = StockCode("600000".into());
    state
        .record_fill(&held, Side::Buy, Money::from_cents(1_000), 0, 100, None, 20)
        .unwrap();

    state.prune_watchlist(&BTreeSet::from([held.clone()]));

    assert!(state.stocks.contains_key(&held));
    assert_eq!(state.stocks.keys().filter(|code| **code != held).count(), 8);
    assert!(!state.stocks.contains_key(&StockCode("600001".into())));
    assert!(state.stocks.contains_key(&StockCode("600011".into())));
}
