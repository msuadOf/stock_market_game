//! Diagnostic state-hash contracts.
//!
//! These tests intentionally use sibling-private test seams.  Poison must remain an
//! engine-internal failure state rather than becoming a public mutation API merely so
//! an integration test can construct it.

use super::*;

fn game() -> GameSession {
    GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap()
}

#[cfg(feature = "verification-harness")]
#[test]
fn 验证追踪返回原始逐字段字节且不改变业务摘要() {
    let game = game();
    let (expected_hash, expected_fields) = super::hash::capture_business_hash_for_test(&game).unwrap();
    let (actual_hash, actual_fields) = game.verification_business_hash_trace().unwrap();
    assert_eq!(actual_hash, expected_hash);
    assert_eq!(actual_fields, expected_fields);
    assert!(!actual_fields.is_empty());
    assert_eq!(game.business_state_hash().unwrap(), expected_hash);
}

#[cfg(feature = "verification-harness")]
#[test]
fn 旧档以候选概率重建后保留已存状态() {
    let original = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let bytes = serde_json::to_vec(&original.save().unwrap()).unwrap();
    let saved = crate::decode_save_slot(&bytes, &crate::SaveDecodeLimits::default()).unwrap();
    let baseline = GameSession::restore(&saved).unwrap();
    let (candidate, trace) = crate::strategy::verification_trace_observations(|| {
        crate::strategy::verification_with_observation_math(
            |argument| f64::from_bits((-argument.exp_m1()).to_bits() + 1),
            || GameSession::restore(&saved),
        )
    });
    let candidate = candidate.unwrap();
    assert!(!trace.is_empty(), "夹具必须实际重建 NPC 概率");
    assert!(trace.iter().all(|item| item.account_id.is_some() && item.canonical_bits.is_some()));
    assert_eq!(serde_json::to_value(candidate.save().unwrap()).unwrap(), serde_json::to_value(baseline.save().unwrap()).unwrap());
}

#[cfg(feature = "verification-harness")]
#[test]
fn 验证接口返回真实已提交的交易部记录() {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    let mut game = GameSession::new(setup, 42).unwrap();
    assert!(game.verification_latest_runtime_record().is_err(), "尚未运行时不得伪造记录");
    game.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {code:StockId(0), side:crate::Side::Buy, price:crate::LimitPrice::Fixed(Money::from_cents(995)), qty:100},
    ).unwrap();
    game.step().unwrap();
    let captured = game.verification_latest_runtime_record().unwrap();
    assert_eq!(captured.tick, game.tick());
    assert!(!captured.desk_lifecycle.is_empty(), "必须捕获真实挂单生命周期事实");
    let driver = game.runtime_driver_for_test().unwrap().lock().unwrap();
    let actual = driver.world_report_log().last().unwrap();
    assert_eq!(captured.desk_lifecycle, actual.desk_lifecycle);
    assert_eq!(captured.desk_trades, actual.desk_trades);
    assert_eq!(captured.desk_receipts, actual.desk_receipts);
    assert_eq!(captured.group_rejections, actual.group_rejections);
    assert_eq!(captured.desk_auction, actual.desk_auction);
    drop(driver);
    game.step().unwrap();
    assert_eq!(game.verification_latest_runtime_record().unwrap().tick, game.tick());
    let fatal = StepFatal::InvariantViolation {
        description: "验证记录读取失败状态".to_owned(),
        location: "hash_contract_tests".to_owned(),
    };
    game.inject_step_failure(fatal.clone());
    assert_eq!(game.step().unwrap_err(), fatal);
    assert!(game.verification_latest_runtime_record().unwrap_err().contains("会话已失败"));
}

#[test]
fn ordinary_tick_shadow_shares_report_histories_and_isolates_mutation() {
    let game = game();
    let mut shadow = game.clone_for_tick_shadow().unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &game
            .state
            .companies
            .legacy()
            .expect("此调用仅用于 Legacy 公司状态")
            .closing,
        &shadow
            .state
            .companies
            .legacy()
            .expect("此调用仅用于 Legacy 公司状态")
            .closing
    ));
    assert!(std::sync::Arc::ptr_eq(
        &game.state.library,
        &shadow.state.library
    ));
    let before = game
        .state
        .companies
        .legacy()
        .expect("此调用仅用于 Legacy 公司状态")
        .closing
        .hash_projection()
        .unwrap();
    let report = game.state.library.save().reports[0].reports.clone();
    std::sync::Arc::make_mut(
        &mut shadow
            .state
            .companies
            .legacy_mut()
            .expect("此调用仅用于 Legacy 公司状态")
            .closing,
    )
    .record(report)
    .unwrap();
    assert!(!std::sync::Arc::ptr_eq(
        &game
            .state
            .companies
            .legacy()
            .expect("此调用仅用于 Legacy 公司状态")
            .closing,
        &shadow
            .state
            .companies
            .legacy()
            .expect("此调用仅用于 Legacy 公司状态")
            .closing
    ));
    assert_eq!(
        game.state
            .companies
            .legacy()
            .expect("此调用仅用于 Legacy 公司状态")
            .closing
            .hash_projection()
            .unwrap(),
        before
    );
    assert!(std::sync::Arc::ptr_eq(
        &game.state.library,
        &shadow.state.library
    ));
}

#[test]
fn tick_shadow_shares_immutable_company_registry_without_changing_hashes() {
    let game = game();
    let shadow = game.clone_for_tick_shadow().unwrap();

    assert!(std::sync::Arc::ptr_eq(
        &game
            .state
            .companies
            .legacy()
            .expect("此调用仅用于 Legacy 公司状态")
            .registry,
        &shadow
            .state
            .companies
            .legacy()
            .expect("此调用仅用于 Legacy 公司状态")
            .registry,
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
    assert!(std::sync::Arc::ptr_eq(
        &game
            .state
            .companies
            .legacy()
            .expect("此调用仅用于 Legacy 公司状态")
            .operations,
        &shadow
            .state
            .companies
            .legacy()
            .expect("此调用仅用于 Legacy 公司状态")
            .operations
    ));

    let company = *shadow
        .state
        .companies
        .legacy()
        .expect("此调用仅用于 Legacy 公司状态")
        .operations
        .companies
        .keys()
        .next()
        .unwrap();
    std::sync::Arc::make_mut(
        &mut shadow
            .state
            .companies
            .legacy_mut()
            .expect("此调用仅用于 Legacy 公司状态")
            .operations,
    )
    .company_mut(&company)
    .unwrap()
    .next_flow_seq += 1;

    assert!(!std::sync::Arc::ptr_eq(
        &game
            .state
            .companies
            .legacy()
            .expect("此调用仅用于 Legacy 公司状态")
            .operations,
        &shadow
            .state
            .companies
            .legacy()
            .expect("此调用仅用于 Legacy 公司状态")
            .operations
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

    game.state
        .last_retail_order_events
        .push(RetailOrderDiagnosticEvent::Rejected {
            account: AccountId(1),
            code: StockId(0),
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
fn company_operations_hash_cache_is_invalidated_by_authoritative_mutation() {
    let mut game = game();
    let business_before = game.business_state_hash().unwrap();
    assert_eq!(game.business_state_hash().unwrap(), business_before);
    let current_date = game.state.civil_clock.current_date();

    std::sync::Arc::make_mut(
        &mut game
            .state
            .companies
            .legacy_mut()
            .expect("此调用仅用于 Legacy 公司状态")
            .operations,
    )
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

    game.state.retail_projection_seen.insert_test_identity(
        7,
        pipeline::ReceiptLocalKey::new(
            pipeline::JournalRank::SealedBatch,
            pipeline::ReceiptSource::SealedIntent(3),
            pipeline::ReceiptTransition {
                envelope: pipeline::EnvelopeKey {
                    account: AccountId(1),
                    stock: StockId(0),
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

    let current_date = game.state.civil_clock.current_date();
    std::sync::Arc::make_mut(
        &mut shadow
            .state
            .companies
            .legacy_mut()
            .expect("此调用仅用于 Legacy 公司状态")
            .operations,
    )
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

    std::sync::Arc::make_mut(
        &mut game
            .state
            .companies
            .legacy_mut()
            .expect("此调用仅用于 Legacy 公司状态")
            .operations,
    )
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
    let expected = game
        .state
        .companies
        .legacy()
        .expect("此调用仅用于 Legacy 公司状态")
        .operations
        .hash_projection()
        .unwrap();
    let json = serde_json::to_vec(
        &game
            .state
            .companies
            .legacy()
            .expect("此调用仅用于 Legacy 公司状态")
            .operations,
    )
    .unwrap();
    let restored: crate::company::operations::CompanyOperations =
        serde_json::from_slice(&json).unwrap();

    assert_eq!(
        restored,
        *game
            .state
            .companies
            .legacy()
            .expect("此调用仅用于 Legacy 公司状态")
            .operations
    );
    assert_eq!(restored.hash_projection().unwrap(), expected);
}

#[test]
fn closing_hash_cache_is_invalidated_by_authoritative_recording() {
    let mut game = game();
    let business_before = game.business_state_hash().unwrap();
    let report = game
        .state
        .library
        .save()
        .reports
        .first()
        .expect("prehistory publishes at least one report")
        .reports
        .clone();

    std::sync::Arc::make_mut(
        &mut game
            .state
            .companies
            .legacy_mut()
            .expect("此调用仅用于 Legacy 公司状态")
            .closing,
    )
    .record(report)
    .unwrap();

    let business_after = game.business_state_hash().unwrap();
    assert_ne!(business_after, business_before);
    assert_eq!(game.business_state_hash().unwrap(), business_after);
}

#[test]
fn closing_projection_clone_mutations_are_isolated_and_serde_stable() {
    let game = game();
    let mut authority = game
        .state
        .companies
        .legacy()
        .expect("此调用仅用于 Legacy 公司状态")
        .closing
        .as_ref()
        .clone();
    let expected = authority.hash_projection().unwrap();
    let mut cloned = authority.clone();
    let reports = game.state.library.save().reports;
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
    let mut authority = game.state.library.as_ref().clone();
    let warmed = authority.hash_projection();
    assert!(
        !authority.save().reports.is_empty(),
        "fixture needs a published report"
    );
    let mut cloned = authority.clone();
    let occurred_on = CivilDate::from_iso("2030-04-20").unwrap();
    let request = |amplitude_bp| crate::information::AnnouncementRequest {
        company: crate::company::CompanyId(0),
        occurred_on,
        published_at: crate::calendar::CivilInstant::from_hms(occurred_on, 18, 0, 0).unwrap(),
        content: crate::information::AnnouncementContent::Shock(
            crate::information::AnnouncedEvent {
                kind: crate::company::ShockKind::CreditDeterioration,
                amplitude_bp,
                starts_on: occurred_on,
                expires_on: occurred_on,
            },
        ),
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
                    stock: StockId(0),
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
