use super::p3_context::build_p3_validation_context;
use super::p4_continuous_adapter::adapt_continuous_stock_inputs;
use super::*;
use crate::{AccountId, Intent, Money, Order, OrderId, Side, StockCode, TradingPhase};

#[test]
fn continuous_adapter_partitions_all_markets_deterministically_with_real_phase_and_config() {
    let game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let first = StockCode("600888".to_owned());
    let second = StockCode("600889".to_owned());
    let validation = validate(
        &game,
        vec![
            Intent::Cancel {
                code: second.clone(),
                id: OrderId(82),
            },
            Intent::Cancel {
                code: first.clone(),
                id: OrderId(81),
            },
        ],
    );
    let before = game.session_state_hash().unwrap();

    let inputs = adapt_continuous_stock_inputs(&game, &validation).unwrap();

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
    assert_eq!(
        inputs
            .iter()
            .flat_map(|input| input.operations.iter())
            .map(P3ValidatedOperation::sealed_index)
            .collect::<Vec<_>>(),
        vec![1, 0]
    );
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
    let validation = validate(&game, Vec::new());

    let inputs = adapt_continuous_stock_inputs(&game, &validation).unwrap();

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
    let validation = validate(&game, Vec::new());
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

    let inputs = adapt_continuous_stock_inputs(&game, &validation).unwrap();

    assert_eq!(inputs[0].envelopes[0].audit.nominal, nominal);
    assert_eq!(inputs[0].envelopes[0].audit.charged, nominal);
}

#[test]
fn continuous_adapter_preserves_complete_place_and_cancel_operations_with_sealed_holes() {
    let game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    let first = StockCode("600888".to_owned());
    let second = StockCode("600889".to_owned());
    let validation = validate(
        &game,
        vec![
            Intent::PlaceLimit {
                code: first.clone(),
                side: Side::Buy,
                price: Money::from_cents(900),
                qty: 100,
            },
            Intent::Cancel {
                code: second.clone(),
                id: OrderId(1),
            },
            Intent::PlaceLimit {
                code: StockCode("600999".to_owned()),
                side: Side::Buy,
                price: Money::from_cents(900),
                qty: 100,
            },
            Intent::PlaceLimit {
                code: second.clone(),
                side: Side::Buy,
                price: Money::from_cents(900),
                qty: 100,
            },
            Intent::Cancel {
                code: first.clone(),
                id: OrderId(2),
            },
        ],
    );
    let expected = [first, second]
        .into_iter()
        .map(|code| {
            validation
                .operations()
                .iter()
                .filter(|operation| match operation {
                    P3ValidatedOperation::Place(draft) => draft.code() == &code,
                    P3ValidatedOperation::Cancel { code: actual, .. } => actual == &code,
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let inputs = adapt_continuous_stock_inputs(&game, &validation).unwrap();

    assert_eq!(inputs[0].operations, expected[0]);
    assert_eq!(inputs[1].operations, expected[1]);
    assert_eq!(
        inputs[0]
            .operations
            .iter()
            .map(P3ValidatedOperation::sealed_index)
            .collect::<Vec<_>>(),
        vec![0, 4]
    );
    assert_eq!(
        inputs[1]
            .operations
            .iter()
            .map(P3ValidatedOperation::sealed_index)
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert!(inputs.iter().all(|input| input.envelopes.is_empty()));
}

#[test]
fn continuous_adapter_rejects_residual_auction_orders_in_continuous_phase() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = StockCode("600888".to_owned());
    let validation = validate(&game, Vec::new());
    game.auction_orders
        .entry(code)
        .or_default()
        .push(crate::session::AuctionOrderSnap {
            owner: AccountId(0),
            side: Side::Buy,
            limit: Money::from_cents(900),
            qty: 100,
            arrival_seq: 93,
        });

    let error = adapt_continuous_stock_inputs(&game, &validation).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("residual auction")
                && location == "pipeline::p4_continuous_adapter"
    ));
}

#[test]
fn continuous_adapter_rejects_an_unknown_stock_operation_instead_of_dropping_it() {
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let validation = validate(
        &game,
        vec![Intent::Cancel {
            code: StockCode("600999".to_owned()),
            id: OrderId(7),
        }],
    );

    let error = adapt_continuous_stock_inputs(&game, &validation).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("unknown stock")
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
    let validation = validate(&game, Vec::new());
    let first = StockCode("600888".to_owned());
    let second = StockCode("600889".to_owned());
    let first_market = game.markets.remove(&first).unwrap();
    game.markets.insert(second, first_market);

    let error = adapt_continuous_stock_inputs(&game, &validation).unwrap_err();

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
    let validation = validate(&missing, Vec::new());
    let error = adapt_continuous_stock_inputs(&missing, &validation).unwrap_err();
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
    let error = adapt_continuous_stock_inputs(&missing, &validation).unwrap_err();
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
    let validation = validate(&game, Vec::new());
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

    let error = adapt_continuous_stock_inputs(&game, &validation).unwrap_err();

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
    let validation = validate(&game, Vec::new());
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

    let error = adapt_continuous_stock_inputs(&game, &validation).unwrap_err();

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
    let validation = validate(&game, Vec::new());

    let error = adapt_continuous_stock_inputs(&game, &validation).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("Continuous")
                && location == "pipeline::p4_continuous_adapter"
    ));
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
