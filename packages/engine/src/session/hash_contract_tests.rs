//! ADR-0017 P8 hash-boundary characterization.
//!
//! These tests intentionally use sibling-private test seams.  Poison must remain an
//! engine-internal failure state rather than becoming a public mutation API merely so
//! an integration test can construct it.

use super::*;

fn game() -> GameSession {
    GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap()
}

#[test]
fn typed_failure_poison_changes_session_hash_but_not_business_hash() {
    let mut game = game();
    let business_before = game.business_state_hash().unwrap();
    let session_before = game.session_state_hash().unwrap();
    let fatal = StepFatal::InvariantViolation {
        description: "hash contract injected failure".to_owned(),
        location: "hash_contract_tests::typed_failure".to_owned(),
    };

    game.inject_step_failure(fatal.clone());

    assert_eq!(game.step().unwrap_err(), fatal);
    assert_eq!(game.business_state_hash().unwrap(), business_before);
    assert_ne!(game.session_state_hash().unwrap(), session_before);
    assert_eq!(game.poison_reason(), Some(&fatal));
}

#[test]
fn diagnostic_cache_changes_session_hash_but_not_business_hash() {
    let mut game = game();
    let business_before = game.business_state_hash().unwrap();
    let session_before = game.session_state_hash().unwrap();

    game.last_retail_order_events
        .push(RetailOrderDiagnosticEvent::Rejected {
            account: AccountId(1),
            code: StockCode("600888".to_owned()),
            reason: RejectionReason::InsufficientCash,
        });

    assert_eq!(game.business_state_hash().unwrap(), business_before);
    assert_ne!(game.session_state_hash().unwrap(), session_before);
}

#[test]
fn authoritative_counter_changes_business_hash() {
    let mut game = game();
    let business_before = game.business_state_hash().unwrap();
    let session_before = game.session_state_hash().unwrap();

    game.next_order_id += 1;

    assert_ne!(game.business_state_hash().unwrap(), business_before);
    assert_ne!(game.session_state_hash().unwrap(), session_before);
}

#[test]
fn retail_projection_cursor_changes_business_hash() {
    let mut game = game();
    let business_before = game.business_state_hash().unwrap();

    game.retail_projection_seen.insert_test_identity(
        7,
        pipeline::ReceiptLocalKey::new(
            pipeline::JournalRank::SealedBatch,
            pipeline::ReceiptSource::SealedIntent(3),
            pipeline::ReceiptTransition {
                envelope: pipeline::EnvelopeKey {
                    account: AccountId(1),
                    stock: StockCode("600888".to_owned()),
                    order: OrderId(9),
                    side: Side::Buy,
                },
                ordinal: 0,
            },
        )
        .unwrap(),
    );

    assert_ne!(game.business_state_hash().unwrap(), business_before);
}

#[test]
fn tick_shadow_commit_preserves_retail_projection_cursor() {
    let mut authority = game();
    let mut shadow = authority.clone_for_tick_shadow().unwrap();
    shadow.retail_projection_seen.insert_test_identity(
        11,
        pipeline::ReceiptLocalKey::new(
            pipeline::JournalRank::SealedBatch,
            pipeline::ReceiptSource::SealedIntent(5),
            pipeline::ReceiptTransition {
                envelope: pipeline::EnvelopeKey {
                    account: AccountId(1),
                    stock: StockCode("600888".to_owned()),
                    order: OrderId(12),
                    side: Side::Sell,
                },
                ordinal: 0,
            },
        )
        .unwrap(),
    );
    let expected = shadow.retail_projection_seen.clone();

    authority.commit_tick_shadow(shadow);

    assert_eq!(authority.retail_projection_seen, expected);
}
