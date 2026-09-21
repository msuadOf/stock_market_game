use super::b1_continuous_transaction::{
    apply_initial_candidate_stream_for_test, apply_open_order_feedback_for_test,
    apply_tick_shadow_b1_continuous_transaction, prepare_b1_continuous_tick,
};
use super::p3_context::build_p3_validation_context;
use super::p4_continuous::{ContinuousExecutionRound, IncrementalContinuousStockCoordinator};
use super::p4_continuous_adapter::prepare_incremental_continuous_inputs;
use super::*;
use crate::{AccountId, Event, Intent, Money, Order, OrderId, Side, StockCode};

fn player_only_session() -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    GameSession::new(setup, 42).unwrap()
}

#[test]
fn joint_b1_player_batch_reaches_rebased_p9_without_legacy_bridge() {
    let mut authority = player_only_session();
    let code = authority.markets.keys().next().unwrap().clone();
    authority
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .unwrap();
    let before = authority.business_state_hash().unwrap();
    let guard = super::p9_candidate_commit::P8AuthorityGuard::capture(&authority).unwrap();
    let mut plan = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();

    let output = apply_tick_shadow_b1_continuous_transaction(&mut plan).unwrap();

    assert_eq!(authority.business_state_hash().unwrap(), before);
    assert_eq!(authority.pending_player.len(), 1);
    assert_eq!(
        output
            .candidates
            .candidates()
            .iter()
            .map(|candidate| candidate.key().clone())
            .collect::<Vec<_>>(),
        vec![P2CandidateKey::player(0)]
    );
    assert!(output.plan_reports.is_empty());
    assert!(output.receipts.is_empty());
    assert_eq!(output.p6.settlement.applied_receipts, 0);
    assert_eq!(output.p6.settlement.applied_groups, 0);
    assert_eq!(
        output.validation.accepted().cloned().collect::<Vec<_>>(),
        vec![P2CandidateKey::player(0)]
    );

    let committed =
        super::p9_candidate_commit::prepare_tick_shadow_plan_commit(&mut authority, plan, guard)
            .unwrap()
            .commit();

    assert!(authority.pending_player.is_empty());
    assert_eq!(authority.markets[&code].resting_orders().len(), 1);
    assert_eq!(authority.envelope_ledger.iter().count(), 1);
    assert!(matches!(
        committed.tick.events.as_slice(),
        [Event::PriceTick { seq: 1, tick: 1, code: first, .. },
        Event::PriceTick { seq: 2, tick: 1, code: second, .. },
        Event::OrderAccepted {
            seq: 3,
            account: AccountId(0),
            code: accepted,
            side: Side::Buy,
            remaining_qty: 100,
            ..
        }] if accepted == &code && first == &code && first < second
    ));
    assert_eq!(
        authority.business_state_hash().unwrap(),
        committed.receipt.business_hash()
    );
}

#[test]
fn joint_b1_downstream_failure_discards_all_three_source_preparation() {
    let mut authority = player_only_session();
    let code = authority.markets.keys().next().unwrap().clone();
    authority
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .unwrap();
    let mut plan = plan_tick(PhaseInput {
        session: &authority,
    })
    .unwrap();
    plan.state
        .execute(|candidate| {
            candidate.next_receipt_base = 1;
            Ok(())
        })
        .unwrap();
    let shadow_before = plan
        .state
        .execute(|candidate| candidate.business_state_hash())
        .unwrap();
    let authority_before = authority.business_state_hash().unwrap();

    assert!(apply_tick_shadow_b1_continuous_transaction(&mut plan).is_err());

    assert_eq!(authority.business_state_hash().unwrap(), authority_before);
    plan.state
        .execute(|candidate| {
            assert_eq!(candidate.business_state_hash().unwrap(), shadow_before);
            assert_eq!(candidate.pending_player.len(), 1);
            Ok(())
        })
        .unwrap();
}

#[test]
fn prepared_joint_b1_entry_has_no_fallible_tail_after_p8() {
    let mut authority = player_only_session();
    let code = authority.markets.keys().next().unwrap().clone();
    authority
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
            },
        )
        .unwrap();

    let committed = prepare_b1_continuous_tick(&mut authority).unwrap().commit();

    assert_eq!(
        committed.output.candidates.candidates()[0].key(),
        &P2CandidateKey::player(0)
    );
    assert_eq!(authority.markets[&code].resting_orders().len(), 1);
    assert_eq!(
        authority.business_state_hash().unwrap(),
        committed.commit.receipt.business_hash()
    );
}

#[test]
fn npc_cancel_feedback_releases_open_order_slots_without_replenishing_resource_budgets() {
    assert_cancel_releases_slots_only(Side::Buy);
    assert_cancel_releases_slots_only(Side::Sell);
}

#[test]
fn b1_feedback_rejects_foreign_swapped_and_duplicate_p4_fact_identities_atomically() {
    for corruption in [
        FactIdentityCorruption::Foreign,
        FactIdentityCorruption::Swapped,
        FactIdentityCorruption::Duplicate,
    ] {
        let (candidate, mut p3, outcomes, mut round) = feedback_identity_fixture();
        corrupt_fact_identities(&mut round, corruption);
        let candidate_business_before = candidate.business_state_hash().unwrap();
        let candidate_session_before = candidate.session_state_hash().unwrap();
        let p3_before = p3.checkpoint();
        let output_before = p3.output().clone();

        let error = apply_open_order_feedback_for_test(&mut p3, &outcomes, &round)
            .expect_err("malformed P4 identity must fail before candidate projection");

        assert!(matches!(error, StepFatal::InvariantViolation { .. }));
        assert_eq!(p3.checkpoint(), p3_before);
        assert_eq!(p3.output(), &output_before);
        assert_eq!(
            candidate.business_state_hash().unwrap(),
            candidate_business_before
        );
        assert_eq!(
            candidate.session_state_hash().unwrap(),
            candidate_session_before
        );
    }
}

fn assert_cancel_releases_slots_only(resting_side: Side) {
    let npc = AccountId(1);
    let player = AccountId(0);
    let mut candidate =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = candidate.markets.keys().next().unwrap().clone();
    if resting_side == Side::Sell {
        candidate
            .accounts
            .get_mut(&npc)
            .unwrap()
            .grant_position(code.clone(), 100, Money::from_cents(99_000))
            .unwrap();
    }
    let resting_order = OrderId(700);
    let placed = candidate
        .markets
        .get_mut(&code)
        .unwrap()
        .place(Order {
            id: resting_order,
            side: resting_side,
            price: Money::from_cents(990),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: npc,
            seq: 0,
        })
        .unwrap();
    assert!(placed.trades.is_empty());
    assert!(placed.resting.is_some());
    candidate.next_order_id = resting_order.0 + 1;
    candidate.hydrate_or_validate_envelope_ledger().unwrap();

    let plan = plan_tick(PhaseInput {
        session: &candidate,
    })
    .unwrap();
    let market = &candidate.markets[&code];
    let category = candidate
        .setup
        .stocks
        .iter()
        .find(|stock| stock.code == code)
        .unwrap()
        .category;
    let context = P3ValidationContext::new(
        [(
            code.clone(),
            P3StockValidation::new(
                category,
                market.up_stop().unwrap(),
                market.down_stop().unwrap(),
            ),
        )],
        1,
        [(npc, 1), (player, 0)],
        P3OpenOrderLimits {
            global: 1,
            per_account: 1,
        },
    )
    .unwrap();
    let mut p3 = P3ValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        candidate.next_order_id,
        candidate.setup.config.clone(),
        context,
    )
    .unwrap();
    let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(
        prepare_incremental_continuous_inputs(&candidate).unwrap(),
    )
    .unwrap();

    let initial = P2CandidateBatch::from_canonical(vec![
        P2Candidate::new(
            P2CandidateKey::npc(npc, 0),
            npc,
            Intent::Cancel {
                code: code.clone(),
                id: resting_order,
            },
        ),
        P2Candidate::new(
            P2CandidateKey::player(0),
            player,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(980),
                qty: 100,
            },
        ),
    ])
    .unwrap();
    let before_stream = p3.checkpoint();
    let rounds = apply_initial_candidate_stream_for_test(&mut p3, &mut p4, &initial).unwrap();

    assert_eq!(rounds.len(), 2);
    let after_stream = p3.checkpoint();
    assert_eq!(after_stream.global_open_orders(), 1);
    assert_eq!(after_stream.account_open_orders(npc), Some(0));
    assert_eq!(after_stream.account_open_orders(player), Some(1));
    assert_eq!(
        after_stream.remaining_cash(npc),
        before_stream.remaining_cash(npc),
        "cancel feedback must not return sealed cash budget"
    );
    assert_eq!(
        after_stream.remaining_sellable(npc, &code),
        before_stream.remaining_sellable(npc, &code),
        "cancel feedback must not return sealed share budget"
    );
    assert_eq!(p3.output().accepted().count(), 2);
}

#[derive(Clone, Copy, Debug)]
enum FactIdentityCorruption {
    Foreign,
    Swapped,
    Duplicate,
}

fn feedback_identity_fixture() -> (
    GameSession,
    P3ValidatorDriver,
    Vec<P3ConsumeOutcome>,
    ContinuousExecutionRound,
) {
    let candidate = player_only_session();
    let code = candidate.markets.keys().next().unwrap().clone();
    let plan = plan_tick(PhaseInput {
        session: &candidate,
    })
    .unwrap();
    let mut p3 = P3ValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        candidate.next_order_id,
        candidate.setup.config.clone(),
        build_p3_validation_context(&candidate).unwrap(),
    )
    .unwrap();
    let outcomes = p3
        .consume_round([
            limit_candidate(P2CandidateKey::player(0), code.clone(), 980),
            limit_candidate(P2CandidateKey::player(1), code, 970),
        ])
        .unwrap();
    let operations = outcomes
        .iter()
        .map(|outcome| outcome.operation().unwrap().clone())
        .collect();
    let mut p4 = IncrementalContinuousStockCoordinator::from_post_p0(
        prepare_incremental_continuous_inputs(&candidate).unwrap(),
    )
    .unwrap();
    let round = p4.apply_round(operations).unwrap();
    assert_eq!(round.facts.len(), 2);
    (candidate, p3, outcomes, round)
}

fn limit_candidate(key: P2CandidateKey, code: StockCode, price_cents: i64) -> P2Candidate {
    P2Candidate::new(
        key,
        AccountId(0),
        Intent::PlaceLimit {
            code,
            side: Side::Buy,
            price: Money::from_cents(price_cents),
            qty: 100,
        },
    )
}

fn corrupt_fact_identities(
    round: &mut ContinuousExecutionRound,
    corruption: FactIdentityCorruption,
) {
    match corruption {
        FactIdentityCorruption::Foreign => {
            round.facts[0].candidate_key = P2CandidateKey::plan_chain(99);
        }
        FactIdentityCorruption::Swapped => {
            let first_key = round.facts[0].candidate_key.clone();
            let first_sealed = round.facts[0].sealed_index;
            round.facts[0].candidate_key = round.facts[1].candidate_key.clone();
            round.facts[0].sealed_index = round.facts[1].sealed_index;
            round.facts[1].candidate_key = first_key;
            round.facts[1].sealed_index = first_sealed;
        }
        FactIdentityCorruption::Duplicate => {
            round.facts[1] = round.facts[0].clone();
        }
    }
}
