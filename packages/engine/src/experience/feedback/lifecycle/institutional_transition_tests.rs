use super::*;

fn moment(minute: u64) -> ExperienceMoment {
    ExperienceMoment {
        civil_date: CivilDate::from_iso("2030-01-07").unwrap(),
        market_minute: minute,
        trading_day: 2,
    }
}

fn holding() -> (RetailExperienceState, StockCode) {
    let code = StockCode("600101".to_owned());
    let mut state = RetailExperienceState::without_equity_reference();
    state
        .record_institutional_fill_dated(
            &code,
            Side::Buy,
            Money::from_cents(1000),
            0,
            100,
            Money::from_cents(2),
            false,
            1,
            moment(10),
        )
        .unwrap();
    (state, code)
}

#[test]
fn institutional_observation_preserves_guards_threshold_and_personal_facts() {
    let (mut state, code) = holding();
    let before = state.clone();
    assert_eq!(
        state.observe_institution_position_dated(&code, Money::ZERO, moment(9), 800),
        Err(ExperienceError::NonPositiveMoney {
            field: "observed institution position price",
            cents: 0
        })
    );
    for backward in [
        ExperienceMoment {
            civil_date: CivilDate::from_iso("2030-01-06").unwrap(),
            ..moment(11)
        },
        moment(9),
        ExperienceMoment {
            trading_day: 1,
            ..moment(11)
        },
    ] {
        assert!(state
            .observe_institution_position_dated(&code, Money::from_cents(920), backward, 800)
            .is_err());
        assert_eq!(state, before);
    }
    for missing_legacy in [true, false] {
        let mut incomplete = before.clone();
        if missing_legacy {
            incomplete.stocks.remove(&code);
        } else {
            incomplete.feedback.stocks.remove(&code);
        }
        let untouched = incomplete.clone();
        assert_eq!(
            incomplete.observe_institution_position_dated(
                &code,
                Money::from_cents(920),
                moment(11),
                800
            ),
            Err(ExperienceError::NoActiveEntry {
                code: code.0.clone()
            })
        );
        assert_eq!(incomplete, untouched);
    }
    state
        .observe_institution_position_dated(&code, Money::from_cents(921), moment(11), 800)
        .unwrap();
    assert!(state.feedback.failure_events.is_empty());
    state
        .observe_institution_position_dated(&code, Money::from_cents(920), moment(12), 800)
        .unwrap();
    state
        .observe_institution_position_dated(&code, Money::from_cents(900), moment(13), 800)
        .unwrap();
    assert_eq!(state.feedback.failure_events.len(), 1);
    assert_eq!(state.feedback.failure_events[0].order_id, Some(1));
    assert_eq!(state.feedback.failure_events[0].moment, moment(12));
    assert_eq!(state.consecutive_failed_buys, 0);
    assert_eq!(
        state.feedback.stocks[&code].last_own_observation,
        Some(OwnObservation {
            price: Money::from_cents(900),
            moment: moment(13)
        })
    );
}

#[test]
fn stale_institutional_holding_clear_preserves_history_and_unrelated_fields() {
    let (mut state, code) = holding();
    state
        .observe_institution_position_dated(&code, Money::from_cents(900), moment(11), 800)
        .unwrap();
    let stock = state.stocks.get_mut(&code).unwrap();
    stock.last_sell_order_id = Some(7);
    stock.cooldown_until_market_minute = Some(99);
    let mut expected = state.clone();
    expected.feedback.stocks.remove(&code);
    let stock = expected.stocks.get_mut(&code).unwrap();
    stock.entry_reference_price = None;
    stock.peak_price_since_entry = None;
    stock.last_buy_price = None;
    stock.last_buy_order_id = None;
    stock.adverse_move_recorded = false;
    state.clear_stale_institutional_holding(&code);
    assert_eq!(state, expected);
    state.clear_stale_institutional_holding(&code);
    assert_eq!(state, expected);
    state.clear_stale_institutional_holding(&StockCode("600102".to_owned()));
    assert_eq!(state, expected);
}
#[test]
fn retail_dated_failures_preserve_legacy_partial_writes_and_feedback_success_boundary() {
    let (mut state, code) = holding();
    state.consecutive_failed_buys = u16::MAX;
    let feedback_before = state.feedback.clone();
    let mut expected = state.stocks[&code].clone();
    expected.last_observed_market_minute = 11;
    assert_eq!(
        state.observe_position_dated(&code, Money::from_cents(900), moment(11)),
        Err(ExperienceError::CounterOverflow)
    );
    assert_eq!(state.stocks[&code], expected);
    assert_eq!(state.feedback, feedback_before);

    expected.last_trade_market_minute = 12;
    expected.last_observed_market_minute = 12;
    assert_eq!(
        state.record_fill_dated(
            &code,
            Side::Sell,
            Money::from_cents(900),
            100,
            50,
            Some(Money::from_cents(1000)),
            Some(2),
            moment(12)
        ),
        Err(ExperienceError::CounterOverflow)
    );
    assert_eq!(state.stocks[&code], expected);
    assert_eq!(state.feedback, feedback_before);

    state.consecutive_failed_buys = 0;
    expected.last_trade_market_minute = u64::MAX;
    expected.last_observed_market_minute = u64::MAX;
    expected.entry_reference_price = None;
    expected.peak_price_since_entry = None;
    expected.last_buy_price = None;
    expected.last_buy_order_id = None;
    expected.last_sell_order_id = Some(3);
    expected.adverse_move_recorded = false;
    assert_eq!(
        state.record_fill_dated(
            &code,
            Side::Sell,
            Money::from_cents(1000),
            100,
            0,
            Some(Money::from_cents(1000)),
            Some(3),
            moment(u64::MAX)
        ),
        Err(ExperienceError::MarketMinuteOverflow {
            minute: u64::MAX,
            increment: super::super::super::POST_EXIT_COOLDOWN_MINUTES
        })
    );
    assert_eq!(state.stocks[&code], expected);
    assert_eq!(state.feedback, feedback_before);
}

#[test]
fn institutional_initialization_and_fee_failures_preserve_original_guard_order() {
    let (state, code) = holding();
    for (current_price, reference, observed, expected) in [
        (
            Money::ZERO,
            Some(Money::ZERO),
            moment(9),
            ExperienceError::NonPositiveMoney {
                field: "initial holding price",
                cents: 0,
            },
        ),
        (
            Money::from_cents(1000),
            Some(Money::ZERO),
            moment(9),
            ExperienceError::NonPositiveMoney {
                field: "initial entry reference",
                cents: 0,
            },
        ),
        (
            Money::from_cents(1000),
            None,
            moment(9),
            ExperienceError::MarketMinuteWentBackwards {
                attempted: 9,
                last: 10,
            },
        ),
        (
            Money::from_cents(1000),
            None,
            moment(11),
            ExperienceError::ActiveEntryAlreadyExists {
                code: code.0.clone(),
            },
        ),
    ] {
        let mut rejected = state.clone();
        assert_eq!(
            rejected.initialize_institutional_holding_dated(
                &code,
                reference,
                current_price,
                observed
            ),
            Err(expected)
        );
        assert_eq!(rejected, state);
    }
    let mut overflow = state.clone();
    overflow
        .feedback
        .stocks
        .get_mut(&code)
        .unwrap()
        .institutional_fees_paid = Some(Money::from_cents(i64::MAX));
    let before = overflow.clone();
    assert_eq!(
        overflow.record_institutional_fill_dated(
            &code,
            Side::Buy,
            Money::from_cents(1000),
            100,
            200,
            Money::from_cents(1),
            false,
            2,
            moment(11)
        ),
        Err(ExperienceError::InstitutionalFeesOverflow)
    );
    assert_eq!(overflow, before);

    let mut initialized = RetailExperienceState::without_equity_reference();
    initialized
        .initialize_institutional_holding_dated(&code, None, Money::from_cents(1000), moment(10))
        .unwrap();
    assert_eq!(
        initialized.feedback.stocks[&code].institutional_fees_paid,
        Some(Money::ZERO)
    );
    assert_eq!(initialized.stocks[&code].last_buy_order_id, None);
    assert_eq!(
        initialized.feedback.stocks[&code].last_own_observation,
        Some(OwnObservation {
            price: Money::from_cents(1000),
            moment: moment(10)
        })
    );
}

#[test]
fn dated_writers_keep_distinct_map_key_sets_and_original_acceptance() {
    let (mut retail, code) = holding();
    retail.stocks.remove(&code);
    retail
        .observe_position_dated(&code, Money::from_cents(900), moment(11))
        .unwrap();
    assert_eq!(retail.stocks[&code].entry_reference_price, None);
    assert_eq!(retail.stocks[&code].last_observed_market_minute, 11);
    assert!(retail.feedback.stocks.contains_key(&code));
    retail
        .record_fill_dated(
            &code,
            Side::Sell,
            Money::from_cents(900),
            100,
            0,
            Some(Money::from_cents(1000)),
            Some(2),
            moment(12),
        )
        .unwrap();
    assert!(retail.stocks.contains_key(&code));
    assert!(!retail.feedback.stocks.contains_key(&code));
    assert_eq!(retail.feedback.exit_records.len(), 1);
    assert_eq!(
        retail.feedback.exit_records[0].cooldown_until_market_minute,
        Some(132)
    );
    assert!(retail.feedback.failure_events.is_empty());
    let before = retail.clone();
    assert_eq!(
        retail.record_fill_dated(
            &code,
            Side::Sell,
            Money::from_cents(1000),
            0,
            0,
            None,
            Some(3),
            moment(13)
        ),
        Err(ExperienceError::NoActiveEntry {
            code: code.0.clone()
        })
    );
    assert_eq!(retail, before);
    assert_eq!(
        retail.record_institutional_fill_dated(
            &code,
            Side::Sell,
            Money::from_cents(1000),
            0,
            0,
            Money::ZERO,
            false,
            3,
            moment(13)
        ),
        Err(ExperienceError::InvalidPositionTransition {
            side: Side::Sell,
            before_qty: 0,
            after_qty: 0
        })
    );
    assert_eq!(retail, before);
}
#[test]
fn institutional_fill_accepts_epoch_without_legacy_and_preserves_closed_legacy_rows() {
    let (mut state, code) = holding();
    state.stocks.remove(&code);
    state
        .record_institutional_fill_dated(
            &code,
            Side::Buy,
            Money::from_cents(1100),
            100,
            200,
            Money::from_cents(3),
            false,
            2,
            moment(11),
        )
        .unwrap();
    assert_eq!(state.stocks[&code].last_buy_order_id, Some(2));
    assert_eq!(state.stocks[&code].entry_reference_price, None);
    assert_eq!(state.feedback.stocks[&code].entry_moment, moment(10));
    assert_eq!(
        state.feedback.stocks[&code].institutional_fees_paid,
        Some(Money::from_cents(5))
    );
    state
        .record_institutional_fill_dated(
            &code,
            Side::Sell,
            Money::from_cents(1200),
            200,
            0,
            Money::from_cents(4),
            true,
            3,
            moment(12),
        )
        .unwrap();
    let closed = state.stocks[&code].clone();
    assert!(!state.feedback.stocks.contains_key(&code));
    assert_eq!(closed.last_sell_order_id, Some(3));
    assert_eq!(closed.cooldown_until_market_minute, None);
    assert_eq!(
        state.feedback.exit_records[0].cooldown_until_market_minute,
        None
    );
    state
        .record_institutional_fill_dated(
            &code,
            Side::Buy,
            Money::from_cents(1300),
            0,
            100,
            Money::from_cents(1),
            false,
            4,
            moment(13),
        )
        .unwrap();
    assert_eq!(state.feedback.stocks[&code].entry_moment, moment(13));
    assert_eq!(
        state.feedback.stocks[&code].institutional_fees_paid,
        Some(Money::from_cents(1))
    );
    assert_eq!(
        state.stocks[&code].last_sell_order_id,
        closed.last_sell_order_id
    );
    assert_eq!(state.feedback.exit_records.len(), 1);
}
