//! Diagnostic state-hash contracts.
//!
//! These tests intentionally use sibling-private test seams.  Poison must remain an
//! engine-internal failure state rather than becoming a public mutation API merely so
//! an integration test can construct it.

use super::*;

fn game() -> GameSession {
    GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap()
}

#[test]
fn tick_shadow_shares_immutable_company_registry_without_changing_hashes() {
    let game = game();
    let shadow = game.clone_for_tick_shadow().unwrap();

    assert!(std::sync::Arc::ptr_eq(
        &game.company_registry,
        &shadow.company_registry,
    ));
    assert_eq!(
        shadow.business_state_hash().unwrap(),
        game.business_state_hash().unwrap()
    );
    assert_eq!(
        shadow.session_state_hash().unwrap(),
        game.session_state_hash().unwrap()
    );
}

#[test]
fn company_operations_shadow_shares_journals_until_an_actual_change() {
    let game = game();
    let business_before = game.business_state_hash().unwrap();
    let mut shadow = game.clone_for_tick_shadow().unwrap();
    assert!(std::sync::Arc::ptr_eq(&game.operations, &shadow.operations));

    let company = shadow.operations.companies.keys().next().unwrap().clone();
    std::sync::Arc::make_mut(&mut shadow.operations)
        .company_mut(&company)
        .unwrap()
        .next_flow_seq += 1;

    assert!(!std::sync::Arc::ptr_eq(
        &game.operations,
        &shadow.operations
    ));
    assert_eq!(game.business_state_hash().unwrap(), business_before);
    assert_ne!(shadow.business_state_hash().unwrap(), business_before);
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
fn company_operations_hash_cache_is_invalidated_by_authoritative_mutation() {
    let mut game = game();
    let business_before = game.business_state_hash().unwrap();
    assert_eq!(game.business_state_hash().unwrap(), business_before);
    let current_date = game.civil_clock.current_date();

    std::sync::Arc::make_mut(&mut game.operations)
        .apply_market_shock(crate::company::events::ActiveShock {
            kind: crate::company::ShockKind::MarketDemandShift,
            amplitude_bp: 100,
            starts_on: current_date,
            expires_on: current_date,
        })
        .unwrap();

    let business_after = game.business_state_hash().unwrap();
    assert_ne!(business_after, business_before);
    assert_eq!(game.business_state_hash().unwrap(), business_after);
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
fn company_operations_projection_is_stable_across_repeated_hashes_and_shadow_clone() {
    let mut game = game();

    let first = game.business_state_hash().unwrap();
    let repeated = game.business_state_hash().unwrap();
    let mut shadow = game.clone_for_tick_shadow().unwrap();

    assert_eq!(repeated, first);
    assert_eq!(shadow.business_state_hash().unwrap(), first);

    let current_date = game.civil_clock.current_date();
    std::sync::Arc::make_mut(&mut shadow.operations)
        .apply_market_shock(crate::company::events::ActiveShock {
            kind: crate::company::ShockKind::MarketDemandShift,
            amplitude_bp: 200,
            starts_on: current_date,
            expires_on: current_date,
        })
        .unwrap();

    // A warmed projection may travel into a tick shadow, but a shadow-only mutation must not
    // invalidate or replace the authority's cached projection.
    assert_eq!(game.business_state_hash().unwrap(), first);

    std::sync::Arc::make_mut(&mut game.operations)
        .apply_market_shock(crate::company::events::ActiveShock {
            kind: crate::company::ShockKind::MarketDemandShift,
            amplitude_bp: 100,
            starts_on: current_date,
            expires_on: current_date,
        })
        .unwrap();

    let authority_after = game.business_state_hash().unwrap();
    let shadow_after = shadow.business_state_hash().unwrap();
    assert_ne!(authority_after, first);
    assert_ne!(shadow_after, first);
    assert_ne!(authority_after, shadow_after);

    let restored_authority = GameSession::restore(&game.save().unwrap()).unwrap();
    let restored_shadow = GameSession::restore(&shadow.save().unwrap()).unwrap();
    assert_eq!(
        restored_authority.business_state_hash().unwrap(),
        authority_after
    );
    assert_eq!(restored_shadow.business_state_hash().unwrap(), shadow_after);
}

#[test]
fn company_operations_projection_survives_serde_round_trip_without_serializing_the_cache() {
    let game = game();
    let expected = game.operations.hash_projection().unwrap();
    let json = serde_json::to_vec(&game.operations).unwrap();
    let restored: crate::company::operations::CompanyOperations =
        serde_json::from_slice(&json).unwrap();

    assert_eq!(restored, *game.operations);
    assert_eq!(restored.hash_projection().unwrap(), expected);
}

#[test]
fn closing_hash_cache_is_invalidated_by_authoritative_recording() {
    let mut game = game();
    let business_before = game.business_state_hash().unwrap();
    let report = game
        .library
        .save()
        .reports
        .first()
        .expect("prehistory publishes at least one report")
        .reports
        .clone();

    game.closing.record(report).unwrap();

    let business_after = game.business_state_hash().unwrap();
    assert_ne!(business_after, business_before);
    assert_eq!(game.business_state_hash().unwrap(), business_after);
}

#[test]
fn closing_projection_clone_mutations_are_isolated_and_serde_stable() {
    let game = game();
    let mut authority = game.closing.clone();
    let expected = authority.hash_projection().unwrap();
    let mut cloned = authority.clone();
    let reports = game.library.save().reports;
    assert!(
        reports.len() >= 2,
        "fixture needs two published report sets"
    );

    cloned.record(reports[1].reports.clone()).unwrap();

    // OnceLock's cached value is copied by value. Mutating the warmed clone must leave the
    // original projection valid and unchanged.
    assert_eq!(authority.hash_projection().unwrap(), expected);
    authority.record(reports[0].reports.clone()).unwrap();

    let authority_after = authority.hash_projection().unwrap();
    let cloned_after = cloned.hash_projection().unwrap();
    assert_ne!(authority_after, expected);
    assert_ne!(cloned_after, expected);
    assert_ne!(authority_after, cloned_after);

    let restored_authority: crate::accounting::closing::ClosingEngine =
        serde_json::from_slice(&serde_json::to_vec(&authority).unwrap()).unwrap();
    let restored_clone: crate::accounting::closing::ClosingEngine =
        serde_json::from_slice(&serde_json::to_vec(&cloned).unwrap()).unwrap();

    assert_eq!(
        restored_authority.hash_projection().unwrap(),
        authority_after
    );
    assert_eq!(restored_clone.hash_projection().unwrap(), cloned_after);
}

#[test]
fn public_library_mixed_digest_clone_mutations_are_isolated_and_serde_stable() {
    let game = game();
    let mut authority = game.library.clone();
    let warmed = authority.hash_projection();
    assert!(
        !authority.save().reports.is_empty(),
        "fixture needs a published report"
    );
    let mut cloned = authority.clone();
    let occurred_on = CivilDate::from_iso("2030-04-20").unwrap();
    let request = |amplitude_bp| crate::information::AnnouncementRequest {
        company: crate::company::CompanyId("digest-fixture".to_owned()),
        occurred_on,
        published_at: crate::calendar::CivilInstant::from_hms(occurred_on, 18, 0, 0).unwrap(),
        event: crate::information::AnnouncedEvent {
            kind: crate::company::ShockKind::CreditDeterioration,
            amplitude_bp,
            starts_on: occurred_on,
            expires_on: occurred_on,
        },
    };

    cloned.publish_announcement(request(501)).unwrap();

    // The warmed library already contains reports. Its clone adds an announcement, exercising
    // the mixed publication digest without sharing derived state back into the original.
    assert_eq!(authority.hash_projection(), warmed);
    assert!(authority.save().announcements.is_empty());
    authority.publish_announcement(request(500)).unwrap();

    let authority_after = authority.hash_projection();
    let cloned_after = cloned.hash_projection();
    assert_ne!(authority_after, warmed);
    assert_ne!(cloned_after, warmed);
    assert_ne!(authority_after, cloned_after);
    assert!(!authority.save().reports.is_empty() && !authority.save().announcements.is_empty());
    assert!(!cloned.save().reports.is_empty() && !cloned.save().announcements.is_empty());

    let restored_authority: crate::information::PublicLibrary =
        serde_json::from_slice(&serde_json::to_vec(&authority).unwrap()).unwrap();
    let restored_clone: crate::information::PublicLibrary =
        serde_json::from_slice(&serde_json::to_vec(&cloned).unwrap()).unwrap();
    assert_eq!(restored_authority.hash_projection(), authority_after);
    assert_eq!(restored_clone.hash_projection(), cloned_after);
    assert_eq!(restored_authority, authority);
    assert_eq!(restored_clone, cloned);
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
