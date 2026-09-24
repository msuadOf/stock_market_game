use super::p3_context::build_p3_validation_context;
use super::stock_auction::b2_auction_day_end::IncrementalAuctionStockCoordinator;
use super::stock_auction::{AuctionPhase, ClearingSelection};
use super::stock_auction_adapter::prepare_incremental_auction_inputs;
use super::*;
use crate::orderbook::js_safe_u64;
use crate::{
    AccountId, GameSession, Intent, Money, Order, OrderId, Side, StockCode, StockExchange,
    TradingPhase,
};

#[test]
fn opening_adapter_builds_stock_inputs_from_post_p0_state() {
    let mut game = two_stock_opening_game();
    let first = StockCode("600888".to_owned());
    let second = StockCode("600889".to_owned());
    install_auction_orders(
        &mut game,
        first.clone(),
        vec![auction_order(0, 40, Side::Buy, 990, 100)],
    );
    install_auction_orders(
        &mut game,
        second.clone(),
        vec![auction_order(0, 50, Side::Buy, 990, 100)],
    );
    game.hydrate_or_validate_envelope_ledger().unwrap();
    let before_orders = game.auction_orders.clone();
    let before_ledger = format!("{:?}", game.envelope_ledger);

    let inputs = prepare_incremental_auction_inputs(&game).unwrap();

    assert_eq!(
        inputs
            .iter()
            .map(|input| input.code.clone())
            .collect::<Vec<_>>(),
        vec![first.clone(), second.clone()]
    );
    assert!(inputs.iter().all(|input| input.operations.is_empty()));
    assert_eq!(inputs[0].completion.state.orders().len(), 1);
    assert_eq!(inputs[0].completion.state.orders()[0].arrival_seq, 40);
    assert_eq!(
        inputs[0].completion.state.orders()[0].envelope.key().order,
        OrderId(40)
    );
    assert_eq!(inputs[1].completion.state.orders().len(), 1);
    assert_eq!(inputs[0].market.code(), &first);
    assert_eq!(inputs[1].market.code(), &second);
    assert!(inputs
        .iter()
        .all(|input| input.continuous_envelopes.is_empty()
            && input.completion.day_end_envelopes.is_empty()));
    assert!(matches!(
        inputs[0].completion.phase,
        AuctionPhase::Opening {
            elapsed_ticks: 0,
            cancelable_ticks: 300,
        }
    ));
    assert_eq!(
        inputs[0].completion.previous_close,
        Money::from_cents(1_000)
    );
    assert_eq!(inputs[0].completion.exchange, StockExchange::Shanghai);
    assert_eq!(inputs[0].completion.price_tick, Money::from_cents(1));
    assert_eq!(game.auction_orders, before_orders);
    assert_eq!(format!("{:?}", game.envelope_ledger), before_ledger);
}

#[test]
fn closing_adapter_keeps_continuous_book_out_of_auction_state_but_validates_its_ledger() {
    let mut game = closing_game();
    let code = StockCode("600888".to_owned());
    game.markets
        .get_mut(&code)
        .unwrap()
        .place(resting_buy(20, 0))
        .unwrap();
    install_auction_orders(
        &mut game,
        code.clone(),
        vec![auction_order(0, 30, Side::Buy, 990, 100)],
    );
    game.hydrate_or_validate_envelope_ledger().unwrap();
    let inputs = prepare_incremental_auction_inputs(&game).unwrap();

    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].code, code);
    assert_eq!(inputs[0].completion.phase, AuctionPhase::Closing);
    assert_eq!(inputs[0].market.code(), &code);
    assert_eq!(inputs[0].completion.state.orders().len(), 1);
    assert_eq!(inputs[0].completion.state.orders()[0].arrival_seq, 30);
    assert_eq!(inputs[0].continuous_envelopes.len(), 1);
    assert_eq!(
        inputs[0].continuous_envelopes[0].key(),
        &EnvelopeKey {
            account: AccountId(0),
            stock: code,
            order: OrderId(20),
            side: Side::Buy,
        }
    );
    assert!(inputs[0].completion.day_end_envelopes.is_empty());
    assert!(inputs[0].operations.is_empty());
}

#[test]
fn adapter_rejects_cross_stock_market_identity() {
    let mut crossed = two_stock_opening_game();
    let first = StockCode("600888".to_owned());
    let second = StockCode("600889".to_owned());
    let first_market = crossed.markets.remove(&first).unwrap();
    crossed.markets.insert(second, first_market);
    let error = prepare_incremental_auction_inputs(&crossed).unwrap_err();
    assert_adapter_error(error, "market map key");
}

#[test]
fn adapter_rejects_missing_stale_and_underfunded_ledger_evidence() {
    let code = StockCode("600888".to_owned());
    let mut missing = opening_game();
    install_auction_orders(
        &mut missing,
        code.clone(),
        vec![auction_order(0, 70, Side::Buy, 990, 100)],
    );
    let error = prepare_incremental_auction_inputs(&missing).unwrap_err();
    assert_adapter_error(error, "unledgered");

    missing.hydrate_or_validate_envelope_ledger().unwrap();
    missing.auction_orders.clear();
    let error = prepare_incremental_auction_inputs(&missing).unwrap_err();
    assert_adapter_error(error, "no matching live order");

    let mut underfunded = opening_game();
    install_auction_orders(
        &mut underfunded,
        code.clone(),
        vec![auction_order(0, 71, Side::Buy, 990, 100)],
    );
    underfunded.envelope_ledger = EnvelopeLedger::new(
        0,
        [Envelope::tick_start_existing(
            EnvelopeKey {
                account: AccountId(0),
                stock: code,
                order: OrderId(71),
                side: Side::Buy,
            },
            Money::from_cents(1),
            0,
            EnvelopeAudit {
                limit: Money::from_cents(990),
                remaining_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        )],
    )
    .unwrap();
    let error = prepare_incremental_auction_inputs(&underfunded).unwrap_err();
    assert_adapter_error(error, "live envelope resources");
}

#[test]
fn adapter_rejects_audit_and_conservation_evidence_drift() {
    let code = StockCode("600888".to_owned());
    let mut audit_drift = opening_game();
    install_auction_orders(
        &mut audit_drift,
        code.clone(),
        vec![auction_order(0, 72, Side::Buy, 990, 100)],
    );
    audit_drift.hydrate_or_validate_envelope_ledger().unwrap();
    audit_drift
        .envelope_ledger
        .audits
        .values_mut()
        .next()
        .unwrap()
        .remaining_qty = 99;
    let error = prepare_incremental_auction_inputs(&audit_drift).unwrap_err();
    assert_adapter_error(error, "audit row disagrees");

    let mut conservation_drift = opening_game();
    install_auction_orders(
        &mut conservation_drift,
        code,
        vec![auction_order(0, 73, Side::Buy, 990, 100)],
    );
    conservation_drift
        .hydrate_or_validate_envelope_ledger()
        .unwrap();
    conservation_drift
        .envelope_ledger
        .conservation
        .values_mut()
        .next()
        .unwrap()
        .sealed_spent = ResVec::new(Money::from_cents(1), 0);
    let error = prepare_incremental_auction_inputs(&conservation_drift).unwrap_err();
    assert_adapter_error(error, "conservation");
}

#[test]
fn adapter_rejects_noncanonical_arrival_order_and_nonserializable_sequences() {
    let code = StockCode("600888".to_owned());
    let mut unordered = opening_game();
    install_auction_orders(
        &mut unordered,
        code.clone(),
        vec![
            auction_order(0, 80, Side::Buy, 990, 100),
            auction_order(0, 79, Side::Buy, 980, 100),
        ],
    );
    unordered.hydrate_or_validate_envelope_ledger().unwrap();
    let error = prepare_incremental_auction_inputs(&unordered).unwrap_err();
    assert_adapter_error(error, "arrival sequence order");

    for (arrival_seq, expected) in [
        (js_safe_u64::MAX + 1, "serializable"),
        (js_safe_u64::MAX, "next arrival sequence"),
    ] {
        let mut game = opening_game();
        install_auction_orders(
            &mut game,
            code.clone(),
            vec![auction_order(0, arrival_seq, Side::Buy, 990, 100)],
        );
        game.hydrate_or_validate_envelope_ledger().unwrap();
        let error = prepare_incremental_auction_inputs(&game).unwrap_err();
        assert_adapter_error(error, expected);
    }

    let mut boundary = opening_game();
    install_auction_orders(
        &mut boundary,
        code,
        vec![auction_order(0, js_safe_u64::MAX - 1, Side::Buy, 990, 100)],
    );
    boundary.hydrate_or_validate_envelope_ledger().unwrap();
    assert!(prepare_incremental_auction_inputs(&boundary).is_ok());
}

#[test]
fn incremental_auction_checks_operation_ids_at_the_js_safe_boundary() {
    let code = StockCode("600888".to_owned());
    let place = |code: StockCode| Intent::PlaceLimit {
        code,
        side: Side::Buy,
        price: Money::from_cents(995),
        qty: 100,
    };

    let mut max_place = opening_game();
    max_place.next_order_id = js_safe_u64::MAX;
    let validation = validate(&max_place, vec![place(code.clone())]);
    let mut coordinator = IncrementalAuctionStockCoordinator::from_post_p0(
        prepare_incremental_auction_inputs(&max_place).unwrap(),
    )
    .unwrap();
    let error = coordinator
        .apply_round(validation.operations().to_vec())
        .unwrap_err();
    assert_auction_operation_error(error, "serializable next order id");

    let mut boundary_place = opening_game();
    boundary_place.next_order_id = js_safe_u64::MAX - 1;
    let validation = validate(&boundary_place, vec![place(code.clone())]);
    let mut coordinator = IncrementalAuctionStockCoordinator::from_post_p0(
        prepare_incremental_auction_inputs(&boundary_place).unwrap(),
    )
    .unwrap();
    assert!(coordinator
        .apply_round(validation.operations().to_vec())
        .is_ok());

    let cancel = opening_game();
    let validation = validate(
        &cancel,
        vec![Intent::Cancel {
            code,
            id: OrderId(js_safe_u64::MAX + 1),
        }],
    );
    let mut coordinator = IncrementalAuctionStockCoordinator::from_post_p0(
        prepare_incremental_auction_inputs(&cancel).unwrap(),
    )
    .unwrap();
    let error = coordinator
        .apply_round(validation.operations().to_vec())
        .unwrap_err();
    assert_auction_operation_error(error, "serializable authority");
}

#[test]
fn adapter_rejects_non_auction_phases_and_opening_continuous_residue() {
    let continuous =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let error = prepare_incremental_auction_inputs(&continuous).unwrap_err();
    assert_adapter_error(error, "auction phase");

    let mut opening = opening_game();
    let code = StockCode("600888".to_owned());
    opening
        .markets
        .get_mut(&code)
        .unwrap()
        .place(resting_buy(90, 0))
        .unwrap();
    opening.hydrate_or_validate_envelope_ledger().unwrap();
    let error = prepare_incremental_auction_inputs(&opening).unwrap_err();
    assert_adapter_error(error, "opening auction contains continuous");
}

#[test]
fn adapter_reuses_the_checked_selector_only_after_future_operation_application() {
    let game = opening_game();
    let inputs = prepare_incremental_auction_inputs(&game).unwrap();

    assert_eq!(
        super::stock_auction::select_clearing(
            &[],
            inputs[0].completion.previous_close,
            inputs[0].completion.exchange,
            inputs[0].completion.price_tick,
        )
        .unwrap(),
        None::<ClearingSelection>
    );
}

fn opening_game() -> GameSession {
    GameSession::new(
        crate::session::npc_working_quote_tests::quote_setup(900),
        42,
    )
    .unwrap()
}

fn two_stock_opening_game() -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.ticks_per_day = 15_300;
    setup.auction_ticks = 900;
    GameSession::new(setup, 42).unwrap()
}

fn closing_game() -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.closing_auction_ticks = 10;
    let mut game = GameSession::new(setup, 42).unwrap();
    game.tick = 90;
    assert_eq!(game.phase(), TradingPhase::ClosingAuction);
    game
}

fn validate(game: &GameSession, intents: Vec<Intent>) -> P3ValidationOutput {
    let plan = plan_tick(PhaseInput { session: game }).unwrap();
    let candidates = intents
        .into_iter()
        .enumerate()
        .map(|(index, intent)| {
            P2Candidate::new(
                P2CandidateKey::player(u64::try_from(index).unwrap()),
                AccountId(0),
                intent,
            )
        })
        .collect();
    let batch = P2CandidateBatch::new(candidates).unwrap();
    P2P3Handoff::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.next_order_id,
        game.setup.config.clone(),
        build_p3_validation_context(game).unwrap(),
    )
    .unwrap()
    .validate()
    .unwrap()
}

fn auction_order(
    account: u64,
    arrival_seq: u64,
    side: Side,
    limit_cents: i64,
    qty: u32,
) -> crate::AuctionOrderSnap {
    crate::AuctionOrderSnap {
        owner: AccountId(account),
        side,
        limit: Money::from_cents(limit_cents),
        qty,
        arrival_seq,
    }
}

fn install_auction_orders(
    game: &mut GameSession,
    code: StockCode,
    orders: Vec<crate::AuctionOrderSnap>,
) {
    for order in &orders {
        *game.auction_order_counts.entry(order.owner).or_default() += 1;
    }
    assert!(game.auction_orders.insert(code, orders).is_none());
}

fn resting_buy(order_id: u64, owner: u64) -> Order {
    Order {
        id: OrderId(order_id),
        side: Side::Buy,
        price: Money::from_cents(990),
        qty: 100,
        original_qty: 100,
        filled_qty: 0,
        filled_value: Money::ZERO,
        owner: AccountId(owner),
        seq: order_id,
    }
}

fn assert_adapter_error(error: StepFatal, needle: &str) {
    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains(needle)
                && location == "pipeline::stock_auction_adapter"
    ));
}

fn assert_auction_operation_error(error: StepFatal, needle: &str) {
    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains(needle)
                && location == "pipeline::b2_auction_day_end"
    ));
}
