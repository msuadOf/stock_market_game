use super::p3_context::build_p3_validation_context;
use super::p3_p4_normalizer::normalize_p3_p4_operations;
use super::stock_auction::{AuctionPhase, ClearingSelection};
use super::stock_auction_adapter::{adapt_auction_stock_inputs, AuctionStockInput};
use super::*;
use crate::orderbook::js_safe_u64;
use crate::{
    AccountId, GameSession, Intent, Money, Order, OrderId, Side, StockCode, StockExchange,
    TradingPhase,
};

#[test]
fn opening_adapter_builds_canonical_stock_inputs_and_preserves_sealed_holes() {
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
    let validation = validate(
        &game,
        vec![
            Intent::PlaceLimit {
                code: first.clone(),
                side: Side::Buy,
                price: Money::from_cents(995),
                qty: 100,
            },
            Intent::PlaceLimit {
                code: StockCode("600999".to_owned()),
                side: Side::Buy,
                price: Money::from_cents(995),
                qty: 100,
            },
            Intent::PlaceMarket {
                code: second.clone(),
                side: Side::Buy,
                qty: 100,
            },
            Intent::Cancel {
                code: first.clone(),
                id: OrderId(40),
            },
        ],
    );
    let before_orders = game.auction_orders.clone();
    let before_ledger = format!("{:?}", game.envelope_ledger);

    let inputs = adapt(&game, &validation).unwrap();

    assert_eq!(
        inputs
            .iter()
            .map(|input| input.code.clone())
            .collect::<Vec<_>>(),
        vec![first.clone(), second.clone()]
    );
    assert_eq!(
        inputs[0]
            .operations
            .iter()
            .map(P3ValidatedOperation::sealed_index)
            .collect::<Vec<_>>(),
        vec![0, 3]
    );
    assert_eq!(
        inputs[1]
            .operations
            .iter()
            .map(P3ValidatedOperation::sealed_index)
            .collect::<Vec<_>>(),
        vec![2]
    );
    assert!(matches!(
        &inputs[1].operations[0],
        P3ValidatedOperation::Place(draft) if draft.kind() == P3PlaceKind::Market
    ));
    assert_eq!(inputs[0].completion.state.orders().len(), 1);
    assert_eq!(inputs[0].completion.state.orders()[0].arrival_seq, 40);
    assert_eq!(
        inputs[0].completion.state.orders()[0].envelope.key().order,
        OrderId(40)
    );
    assert_eq!(inputs[1].completion.state.orders().len(), 1);
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
    let validation = validate(&game, Vec::new());

    let inputs = adapt(&game, &validation).unwrap();

    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].code, code);
    assert_eq!(inputs[0].completion.phase, AuctionPhase::Closing);
    assert_eq!(inputs[0].completion.state.orders().len(), 1);
    assert_eq!(inputs[0].completion.state.orders()[0].arrival_seq, 30);
    assert!(inputs[0].operations.is_empty());
}

#[test]
fn market_place_remains_a_p3_operation_and_never_enters_the_base_auction_queue() {
    let game = opening_game();
    let code = StockCode("600888".to_owned());
    let validation = validate(
        &game,
        vec![Intent::PlaceMarket {
            code,
            side: Side::Buy,
            qty: 100,
        }],
    );

    let inputs = adapt(&game, &validation).unwrap();

    assert!(inputs[0].completion.state.orders().is_empty());
    assert!(matches!(
        &inputs[0].operations[..],
        [P3ValidatedOperation::Place(draft)] if draft.kind() == P3PlaceKind::Market
    ));
}

#[test]
fn normalized_unknown_cancel_remains_an_ordinary_rejection_outside_stock_inputs() {
    let game = opening_game();
    let validation = validate(
        &game,
        vec![Intent::Cancel {
            code: StockCode("600999".to_owned()),
            id: OrderId(7),
        }],
    );
    let normalized = normalize(&game, &validation);

    let inputs = adapt_auction_stock_inputs(&game, &validation, &normalized).unwrap();

    assert!(inputs.iter().all(|input| input.operations.is_empty()));
    assert_eq!(normalized.rejections().len(), 1);
    assert_eq!(
        normalized.rejections()[0].reason(),
        &crate::RejectionReason::UnknownStock
    );
    assert_eq!(normalized.rejections()[0].order_id(), OrderId(7));
}

#[test]
fn adapter_rejects_mismatched_normalized_handoff_and_cross_stock_market_identity() {
    let game = opening_game();
    let validation = validate(
        &game,
        vec![Intent::Cancel {
            code: StockCode("600888".to_owned()),
            id: OrderId(7),
        }],
    );
    let different = validate(
        &game,
        vec![Intent::Cancel {
            code: StockCode("600888".to_owned()),
            id: OrderId(8),
        }],
    );
    let mismatched = normalize(&game, &different);
    let error = adapt_auction_stock_inputs(&game, &validation, &mismatched).unwrap_err();
    assert_adapter_error(error, "normalized P3 handoff");

    let mut crossed = two_stock_opening_game();
    let first = StockCode("600888".to_owned());
    let second = StockCode("600889".to_owned());
    let validation = validate(&crossed, Vec::new());
    let normalized = normalize(&crossed, &validation);
    let first_market = crossed.markets.remove(&first).unwrap();
    crossed.markets.insert(second, first_market);
    let error = adapt_auction_stock_inputs(&crossed, &validation, &normalized).unwrap_err();
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
    let validation = validate(&missing, Vec::new());
    let error = adapt(&missing, &validation).unwrap_err();
    assert_adapter_error(error, "unledgered");

    missing.hydrate_or_validate_envelope_ledger().unwrap();
    missing.auction_orders.clear();
    let error = adapt(&missing, &validation).unwrap_err();
    assert_adapter_error(error, "no matching live order");

    let mut underfunded = opening_game();
    let validation = validate(&underfunded, Vec::new());
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
    let error = adapt(&underfunded, &validation).unwrap_err();
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
    let validation = validate(&audit_drift, Vec::new());
    audit_drift
        .envelope_ledger
        .audits
        .values_mut()
        .next()
        .unwrap()
        .remaining_qty = 99;
    let error = adapt(&audit_drift, &validation).unwrap_err();
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
    let validation = validate(&conservation_drift, Vec::new());
    conservation_drift
        .envelope_ledger
        .conservation
        .values_mut()
        .next()
        .unwrap()
        .sealed_spent = ResVec::new(Money::from_cents(1), 0);
    let error = adapt(&conservation_drift, &validation).unwrap_err();
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
    let validation = validate(&unordered, Vec::new());
    let error = adapt(&unordered, &validation).unwrap_err();
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
        let validation = validate(&game, Vec::new());
        let error = adapt(&game, &validation).unwrap_err();
        assert_adapter_error(error, expected);
    }

    let mut boundary = opening_game();
    install_auction_orders(
        &mut boundary,
        code,
        vec![auction_order(0, js_safe_u64::MAX - 1, Side::Buy, 990, 100)],
    );
    boundary.hydrate_or_validate_envelope_ledger().unwrap();
    let validation = validate(&boundary, Vec::new());
    assert!(adapt(&boundary, &validation).is_ok());
}

#[test]
fn adapter_checks_p3_place_and_cancel_ids_at_the_js_safe_boundary() {
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
    let error = adapt(&max_place, &validation).unwrap_err();
    assert_adapter_error(error, "next arrival sequence");

    let mut boundary_place = opening_game();
    boundary_place.next_order_id = js_safe_u64::MAX - 1;
    let validation = validate(&boundary_place, vec![place(code.clone())]);
    assert!(adapt(&boundary_place, &validation).is_ok());

    let cancel = opening_game();
    let validation = validate(
        &cancel,
        vec![Intent::Cancel {
            code,
            id: OrderId(js_safe_u64::MAX + 1),
        }],
    );
    let error = adapt(&cancel, &validation).unwrap_err();
    assert_adapter_error(error, "cancel order id");
}

#[test]
fn adapter_rejects_non_auction_phases_and_opening_continuous_residue() {
    let continuous =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let validation = validate(&continuous, Vec::new());
    let error = adapt(&continuous, &validation).unwrap_err();
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
    let validation = validate(&opening, Vec::new());
    let error = adapt(&opening, &validation).unwrap_err();
    assert_adapter_error(error, "opening auction contains continuous");
}

#[test]
fn adapter_reuses_the_checked_selector_only_after_future_operation_application() {
    let game = opening_game();
    let validation = validate(&game, Vec::new());
    let inputs = adapt(&game, &validation).unwrap();

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

fn normalize(
    game: &GameSession,
    validation: &P3ValidationOutput,
) -> super::p3_p4_normalizer::P3P4NormalizedOperations {
    normalize_p3_p4_operations(
        validation.results(),
        validation.operations(),
        game.markets.keys().cloned(),
    )
    .unwrap()
}

fn adapt(
    game: &GameSession,
    validation: &P3ValidationOutput,
) -> Result<Vec<AuctionStockInput>, StepFatal> {
    let normalized = normalize(game, validation);
    adapt_auction_stock_inputs(game, validation, &normalized)
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
    let batch = P2CandidateBatch::from_unsorted(candidates).unwrap();
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
