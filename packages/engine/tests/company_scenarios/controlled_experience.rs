use super::*;

#[test]
fn same_session_pnl_with_different_owned_experience_changes_retail_decision() {
    // Given: same-seed sessions with identical accounts and one validated three-failure history.
    let mut setup = setup("2030-01-07");
    setup.stocks.truncate(1);
    setup.npcs = NpcSetup {
        retail_count: 1,
        inst_count: 1,
        hot_count: 0,
        retail_cash_median: Money::from_cents(10_000_000),
    };
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.float_allocation = FloatAllocation::ByKind {
        retail: 0.0,
        inst: 1.0,
        hot: 0.0,
    };
    let mut fresh = GameSession::new(setup, SEED).unwrap();
    for _ in 0..1 {
        fresh.step().expect("healthy step");
    }
    let mut fresh_save = fresh.save().expect("healthy save");
    let account = *fresh_save.retail_experience.keys().next().unwrap();
    let stock = code("600101");
    let closes = fresh_save.market_minute_closes.get_mut(&stock).unwrap();
    closes.clear();
    for minute in 0..38_u64 {
        closes.push(engine::MarketMinuteClose {
            absolute_trading_minute: minute,
            close: Money::from_cents(1_030),
        });
    }
    closes.push(engine::MarketMinuteClose {
        absolute_trading_minute: 38,
        close: Money::from_cents(1_000),
    });
    closes.push(engine::MarketMinuteClose {
        absolute_trading_minute: 39,
        close: Money::from_cents(999),
    });
    let mut scarred_save = fresh_save.clone();
    let experience = scarred_save.retail_experience.get_mut(&account).unwrap();
    let moment = engine::experience::ExperienceMoment {
        civil_date: engine::CivilDate::from_iso("2029-12-01").unwrap(),
        market_minute: 0,
        trading_day: 0,
    };
    for round in 0..3_u64 {
        experience
            .record_fill_dated(
                &stock,
                engine::Side::Buy,
                Money::from_cents(1_000),
                0,
                100,
                None,
                Some(1 + round * 2),
                moment,
            )
            .unwrap();
        experience
            .observe_position_dated(&stock, Money::from_cents(940), moment)
            .unwrap();
        experience
            .record_fill_dated(
                &stock,
                engine::Side::Sell,
                Money::from_cents(900),
                100,
                0,
                Some(Money::from_cents(1_000)),
                Some(2 + round * 2),
                moment,
            )
            .unwrap();
    }
    scarred_save.next_order_id = 7;
    for save in [&mut fresh_save, &mut scarred_save] {
        let attention = save.npc_attention.get_mut(&account).unwrap();
        attention.next_attention_candidate_tick = save.snapshot.tick;
        attention.rng_state = 0_u64.wrapping_sub(0x9E37_79B9_7F4A_7C15);
    }
    let mut fresh_game = GameSession::restore(&fresh_save).unwrap();
    let mut scarred_game = GameSession::restore(&scarred_save).unwrap();

    // When: both run the same normal session decision tick.
    fresh_game.step().expect("healthy step");
    scarred_game.step().expect("healthy step");

    // Then: equal account P&L but distinct owned histories produce distinct decision traces.
    assert_eq!(
        fresh_game.account(account).unwrap().cash,
        scarred_game.account(account).unwrap().cash
    );
    assert_ne!(
        fresh_game.last_retail_decisions(),
        scarred_game.last_retail_decisions()
    );
    println!(
        "{{\"scenario\":\"session_experience\",\"account\":{},\"fresh_decisions\":{},\"scarred_decisions\":{},\"failures\":3}}",
        account.0,
        fresh_game.last_retail_decisions().len(),
        scarred_game.last_retail_decisions().len()
    );
}
