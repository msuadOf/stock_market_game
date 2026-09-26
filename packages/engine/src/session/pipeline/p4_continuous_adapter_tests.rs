use super::p4_continuous_adapter::prepare_incremental_continuous_inputs;
use super::*;
use crate::{AccountId, Money, Order, OrderId, Side, StockCode, TradingPhase};

#[test]
fn continuous_inputs_capture_all_markets_without_preloaded_operations() {
    let game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let first = StockCode("600888".to_owned());
    let second = StockCode("600889".to_owned());
    let before = game.session_state_hash().unwrap();

    let inputs = prepare_incremental_continuous_inputs(&game).unwrap();

    assert_eq!(
        inputs
            .iter()
            .map(|input| input.market.code().clone())
            .collect::<Vec<_>>(),
        vec![first, second]
    );
    assert!(inputs
        .iter()
        .all(|input| input.phase == TradingPhase::Continuous));
    assert!(inputs.iter().all(|input| input.envelopes.is_empty()));
    assert!(inputs.iter().all(|input| input.operations.is_empty()));
    for input in &inputs {
        assert_eq!(
            serde_json::to_vec(&input.config).unwrap(),
            serde_json::to_vec(&game.setup.config).unwrap()
        );
    }
    assert_eq!(game.session_state_hash().unwrap(), before);
}

#[test]
fn continuous_adapter_carries_only_tick_start_envelopes_matching_each_order_book() {
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let code = StockCode("600889".to_owned());
    let order = resting_buy(OrderId(91), AccountId(1));
    game.markets.get_mut(&code).unwrap().place(order).unwrap();
    game.hydrate_or_validate_envelope_ledger().unwrap();
    let inputs = prepare_incremental_continuous_inputs(&game).unwrap();

    let first = &inputs[0];
    let second = &inputs[1];
    assert!(first.envelopes.is_empty());
    assert_eq!(second.market.code(), &code);
    assert_eq!(second.envelopes.len(), 1);
    let snapshot = &second.envelopes[0];
    assert_eq!(snapshot.envelope.origin(), EnvelopeOrigin::TickStart);
    assert_eq!(snapshot.envelope.audit(), snapshot.audit);
    assert_eq!(snapshot.envelope.key().order, OrderId(91));
    assert_eq!(snapshot.audit.remaining_qty, 100);
}

#[test]
fn continuous_adapter_preserves_prior_fill_fee_audit_from_the_authoritative_ledger() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = StockCode("600888".to_owned());
    let owner = AccountId(1);
    let order = Order {
        id: OrderId(92),
        side: Side::Buy,
        price: Money::from_cents(900),
        qty: 100,
        original_qty: 200,
        filled_qty: 100,
        filled_value: Money::from_cents(90_000),
        owner,
        seq: 0,
    };
    game.markets.get_mut(&code).unwrap().place(order).unwrap();
    let nominal = FeeComponents {
        commission: Money::from_cents(500),
        stamp_tax: Money::ZERO,
        transfer_fee: Money::from_cents(1),
    };
    let cash = crate::session::buy_order_reservation(
        &game.setup.config,
        Money::from_cents(900),
        100,
        Money::from_cents(90_000),
    )
    .unwrap();
    game.envelope_ledger = EnvelopeLedger::new(
        0,
        [Envelope::tick_start_existing(
            EnvelopeKey {
                account: owner,
                stock: code,
                order: OrderId(92),
                side: Side::Buy,
            },
            cash,
            0,
            EnvelopeAudit {
                limit: Money::from_cents(900),
                remaining_qty: 100,
                filled_qty: 100,
                filled_value: Money::from_cents(90_000),
                nominal,
                charged: nominal,
            },
        )],
    )
    .unwrap();

    let inputs = prepare_incremental_continuous_inputs(&game).unwrap();

    assert_eq!(inputs[0].envelopes[0].audit.nominal, nominal);
    assert_eq!(inputs[0].envelopes[0].audit.charged, nominal);
}

#[test]
fn continuous_adapter_rejects_residual_auction_orders_in_continuous_phase() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = StockCode("600888".to_owned());
    game.auction_orders
        .entry(code)
        .or_default()
        .push(crate::session::AuctionOrderSnap {
            owner: AccountId(0),
            side: Side::Buy,
            limit: Money::from_cents(900),
            qty: 100,
            order_id: 93,
        });

    let error = prepare_incremental_continuous_inputs(&game).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("residual auction")
                && location == "pipeline::p4_continuous_adapter"
    ));
}

#[test]
fn continuous_adapter_rejects_a_cross_stock_market_map() {
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let first = StockCode("600888".to_owned());
    let second = StockCode("600889".to_owned());
    let first_market = game.markets.remove(&first).unwrap();
    game.markets.insert(second, first_market);

    let error = prepare_incremental_continuous_inputs(&game).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("market map key")
                && location == "pipeline::p4_continuous_adapter"
    ));
}

#[test]
fn continuous_adapter_rejects_missing_or_stale_ledger_evidence() {
    let mut missing =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = StockCode("600888".to_owned());
    missing
        .markets
        .get_mut(&code)
        .unwrap()
        .place(resting_buy(OrderId(101), AccountId(1)))
        .unwrap();
    let error = prepare_incremental_continuous_inputs(&missing).unwrap_err();
    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("order books")
                && location == "pipeline::p4_continuous_adapter"
    ));

    missing.hydrate_or_validate_envelope_ledger().unwrap();
    missing
        .markets
        .get_mut(&code)
        .unwrap()
        .cancel(OrderId(101))
        .unwrap();
    let error = prepare_incremental_continuous_inputs(&missing).unwrap_err();
    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("order books")
                && location == "pipeline::p4_continuous_adapter"
    ));
}

#[test]
fn continuous_adapter_rejects_underfunded_live_resources_with_matching_order_fields() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = StockCode("600888".to_owned());
    let owner = AccountId(1);
    game.markets
        .get_mut(&code)
        .unwrap()
        .place(resting_buy(OrderId(102), owner))
        .unwrap();
    game.envelope_ledger = EnvelopeLedger::new(
        0,
        [Envelope::tick_start_existing(
            EnvelopeKey {
                account: owner,
                stock: code,
                order: OrderId(102),
                side: Side::Buy,
            },
            Money::from_cents(1),
            0,
            EnvelopeAudit {
                limit: Money::from_cents(900),
                remaining_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        )],
    )
    .unwrap();

    let error = prepare_incremental_continuous_inputs(&game).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("live envelope resources")
                && location == "pipeline::p4_continuous_adapter"
    ));
}

#[test]
fn continuous_adapter_rejects_same_tick_envelopes_in_the_live_ledger() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = StockCode("600888".to_owned());
    game.envelope_ledger = EnvelopeLedger::new(
        0,
        [Envelope::p3_created(
            EnvelopeKey {
                account: AccountId(0),
                stock: code,
                order: OrderId(44),
                side: Side::Buy,
            },
            Money::from_cents(90_000),
            0,
            EnvelopeAudit {
                limit: Money::from_cents(900),
                remaining_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        )],
    )
    .unwrap();

    let error = prepare_incremental_continuous_inputs(&game).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("TickStart")
                && location == "pipeline::p4_continuous_adapter"
    ));
}

#[test]
fn continuous_adapter_rejects_non_continuous_phase() {
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(3), 42).unwrap();
    assert_eq!(game.phase(), TradingPhase::CallAuction);

    let error = prepare_incremental_continuous_inputs(&game).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("Continuous")
                && location == "pipeline::p4_continuous_adapter"
    ));
}

fn resting_buy(id: OrderId, owner: AccountId) -> Order {
    Order {
        id,
        side: Side::Buy,
        price: Money::from_cents(900),
        qty: 100,
        original_qty: 100,
        filled_qty: 0,
        filled_value: Money::ZERO,
        owner,
        seq: 0,
    }
}
