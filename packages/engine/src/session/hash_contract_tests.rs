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

    game.state
        .last_retail_order_events
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

    game.state.next_order_id += 1;

    assert_ne!(game.business_state_hash().unwrap(), business_before);
    assert_ne!(game.session_state_hash().unwrap(), session_before);
}

#[test]
fn retail_projection_cursor_changes_business_hash() {
    let mut game = game();
    let business_before = game.business_state_hash().unwrap();

    game.state.retail_projection_seen.insert_test_identity(
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
fn public_library_mixed_digest_clone_mutations_are_isolated_and_serde_stable() {
    let game = game();
    let mut authority = game.state.library.as_ref().clone();
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
    shadow.state.retail_projection_seen.insert_test_identity(
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
    let expected = shadow.state.retail_projection_seen.clone();

    authority.commit_tick_shadow(shadow);

    assert_eq!(authority.state.retail_projection_seen, expected);
}

fn zero_revenue_game() -> GameSession {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    let crate::company::config::CompanySystemConfig::Simple(config) = &mut setup.company_system else {
        panic!("测试明确选择 Simple");
    };
    config.companies[0].generation.initial_revenue = crate::accounting::AccountingAmount::ZERO;
    GameSession::new(setup, 42).unwrap()
}

fn restart_command(revenue_cents: i128) -> crate::company::api::CompanyCommand {
    crate::company::api::CompanyCommand::RestartRevenue {
        company: crate::company::CompanyId("C-600888".into()),
        revenue: crate::accounting::AccountingAmount::from_cents(revenue_cents),
        source: "测试显式虚拟复业来源".into(),
    }
}

#[test]
fn simple_tick_shadow_shares_selected_system_and_public_history_without_changing_hashes() {
    let game = game();
    let shadow = game.clone_for_tick_shadow().unwrap();
    assert!(std::sync::Arc::ptr_eq(&game.state.company_system, &shadow.state.company_system));
    assert!(std::sync::Arc::ptr_eq(&game.state.library, &shadow.state.library));
    assert_eq!(shadow.business_state_hash().unwrap(), game.business_state_hash().unwrap());
    assert_eq!(shadow.session_state_hash().unwrap(), game.session_state_hash().unwrap());
}

#[test]
fn simple_shadow_shares_selected_system_until_authoritative_command() {
    let game = zero_revenue_game();
    let before = game.business_state_hash().unwrap();
    let mut shadow = game.clone_for_tick_shadow().unwrap();
    assert!(std::sync::Arc::ptr_eq(&game.state.company_system, &shadow.state.company_system));
    std::sync::Arc::make_mut(&mut shadow.state.company_system).submit_command(restart_command(100_000)).unwrap();
    assert!(!std::sync::Arc::ptr_eq(&game.state.company_system, &shadow.state.company_system));
    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_ne!(shadow.business_state_hash().unwrap(), before);
    assert!(std::sync::Arc::ptr_eq(&game.state.library, &shadow.state.library));
}

#[test]
fn simple_hash_cache_is_invalidated_by_authoritative_command_and_day_advance() {
    let mut game = zero_revenue_game();
    let before = game.business_state_hash().unwrap();
    assert_eq!(game.business_state_hash().unwrap(), before);
    std::sync::Arc::make_mut(&mut game.state.company_system).submit_command(restart_command(100_000)).unwrap();
    let after_command = game.business_state_hash().unwrap();
    assert_ne!(after_command, before);
    assert_eq!(game.business_state_hash().unwrap(), after_command);
    game.end_civil_day().unwrap();
    let after_day = game.business_state_hash().unwrap();
    assert_ne!(after_day, after_command);
    assert_eq!(game.business_state_hash().unwrap(), after_day);
}

#[test]
fn simple_projection_is_stable_across_repeated_hashes_clone_and_restore() {
    let mut game = zero_revenue_game();
    let first = game.business_state_hash().unwrap();
    assert_eq!(game.business_state_hash().unwrap(), first);
    let mut shadow = game.clone_for_tick_shadow().unwrap();
    assert_eq!(shadow.business_state_hash().unwrap(), first);
    std::sync::Arc::make_mut(&mut shadow.state.company_system).submit_command(restart_command(200_000)).unwrap();
    assert_eq!(game.business_state_hash().unwrap(), first);
    std::sync::Arc::make_mut(&mut game.state.company_system).submit_command(restart_command(100_000)).unwrap();
    let authority_after = game.business_state_hash().unwrap();
    let shadow_after = shadow.business_state_hash().unwrap();
    assert_ne!(authority_after, first);
    assert_ne!(shadow_after, first);
    assert_ne!(authority_after, shadow_after);
    let restored_authority = GameSession::restore(&game.save().unwrap()).unwrap();
    let restored_shadow = GameSession::restore(&shadow.save().unwrap()).unwrap();
    assert_eq!(restored_authority.business_state_hash().unwrap(), authority_after);
    assert_eq!(restored_shadow.business_state_hash().unwrap(), shadow_after);
}

#[test]
fn simple_projection_survives_serde_without_serializing_cache() {
    let game = game();
    let expected = game.state.company_system.hash_projection().unwrap();
    let value = serde_json::to_value(&game.state.company_system).unwrap();
    assert!(value.get("hash_cache").is_none());
    let restored: crate::company::CompanySystem = serde_json::from_value(value).unwrap();
    assert_eq!(restored, *game.state.company_system);
    assert_eq!(restored.hash_projection().unwrap(), expected);
}
