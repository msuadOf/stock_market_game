use super::b1_continuous_transaction::apply_tick_shadow_b1_continuous_transaction;
use super::*;
use crate::{AccountId, Event, Intent, Money, Side};

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

    let output = apply_tick_shadow_b1_continuous_transaction(&mut plan, None).unwrap();

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
    assert!(output.plan_chain_driver.is_none());
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
        [Event::OrderAccepted {
            seq: 1,
            account: AccountId(0),
            code: accepted,
            side: Side::Buy,
            remaining_qty: 100,
            ..
        }] if accepted == &code
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

    assert!(apply_tick_shadow_b1_continuous_transaction(&mut plan, None).is_err());

    assert_eq!(authority.business_state_hash().unwrap(), authority_before);
    plan.state
        .execute(|candidate| {
            assert_eq!(candidate.business_state_hash().unwrap(), shadow_before);
            assert_eq!(candidate.pending_player.len(), 1);
            Ok(())
        })
        .unwrap();
}
