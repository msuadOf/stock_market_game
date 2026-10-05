use super::*;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct FinancialFixture {
    company_registry: std::sync::Arc<crate::company::CompanyRegistry>,
    operations: std::sync::Arc<crate::company::operations::CompanyOperations>,
    closing: std::sync::Arc<crate::accounting::closing::ClosingEngine>,
    library: std::sync::Arc<crate::information::PublicLibrary>,
}

impl FinancialFixture {
    fn hash_projection(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(&(
            &self.company_registry,
            self.operations.hash_projection()?,
            self.closing.hash_projection()?,
            self.library.hash_projection(),
        ))
    }
}

fn financial_fixture() -> FinancialFixture {
    let (registry, history) = company_assembly::financial_fixture_tests::hash_financial_fixture();
    FinancialFixture {
        company_registry: std::sync::Arc::new(registry),
        operations: std::sync::Arc::new(history.ops),
        closing: std::sync::Arc::new(history.closing),
        library: std::sync::Arc::new(history.library),
    }
}

#[test]
fn ordinary_tick_shadow_shares_report_histories_and_isolates_mutation() {
    let game = financial_fixture();
    let mut shadow = game.clone();
    assert!(std::sync::Arc::ptr_eq(
        &game.closing,
        &shadow.closing
    ));
    assert!(std::sync::Arc::ptr_eq(
        &game.library,
        &shadow.library
    ));
    let before = game.closing.hash_projection().unwrap();
    let report = game.library.save().reports[0].reports.clone();
    std::sync::Arc::make_mut(&mut shadow.closing)
        .record(report)
        .unwrap();
    assert!(!std::sync::Arc::ptr_eq(
        &game.closing,
        &shadow.closing
    ));
    assert_eq!(game.closing.hash_projection().unwrap(), before);
    assert!(std::sync::Arc::ptr_eq(
        &game.library,
        &shadow.library
    ));
}

#[test]
fn tick_shadow_shares_immutable_company_registry_without_changing_hashes() {
    let game = financial_fixture();
    let shadow = game.clone();

    assert!(std::sync::Arc::ptr_eq(
        &game.company_registry,
        &shadow.company_registry,
    ));
    assert_eq!(
        serde_json::to_vec(&shadow).unwrap(),
        serde_json::to_vec(&game).unwrap()
    );
    assert_eq!(
        shadow.hash_projection().unwrap(),
        game.hash_projection().unwrap()
    );
}

#[test]
fn company_operations_shadow_shares_journals_until_an_actual_change() {
    let game = financial_fixture();
    let business_before = game.hash_projection().unwrap();
    let mut shadow = game.clone();
    assert!(std::sync::Arc::ptr_eq(
        &game.operations,
        &shadow.operations
    ));

    let company = shadow
        .operations
        .companies
        .keys()
        .next()
        .unwrap()
        .clone();
    std::sync::Arc::make_mut(&mut shadow.operations)
        .company_mut(&company)
        .unwrap()
        .next_flow_seq += 1;

    assert!(!std::sync::Arc::ptr_eq(
        &game.operations,
        &shadow.operations
    ));
    assert_eq!(game.hash_projection().unwrap(), business_before);
    assert_ne!(shadow.hash_projection().unwrap(), business_before);
}

#[test]
fn company_operations_hash_cache_is_invalidated_by_authoritative_mutation() {
    let mut game = financial_fixture();
    let business_before = game.hash_projection().unwrap();
    assert_eq!(game.hash_projection().unwrap(), business_before);
    let current_date = CivilDate::from_iso("2030-01-01").unwrap();

    std::sync::Arc::make_mut(&mut game.operations)
        .apply_market_shock(crate::company::events::ActiveShock {
            kind: crate::company::ShockKind::MarketDemandShift,
            amplitude_bp: 100,
            starts_on: current_date,
            expires_on: current_date,
        })
        .unwrap();

    let business_after = game.hash_projection().unwrap();
    assert_ne!(business_after, business_before);
    assert_eq!(game.hash_projection().unwrap(), business_after);
}

#[test]
fn company_operations_projection_is_stable_across_repeated_hashes_and_shadow_clone() {
    let mut game = financial_fixture();

    let first = game.hash_projection().unwrap();
    let repeated = game.hash_projection().unwrap();
    let mut shadow = game.clone();

    assert_eq!(repeated, first);
    assert_eq!(shadow.hash_projection().unwrap(), first);

    let current_date = CivilDate::from_iso("2030-01-01").unwrap();
    std::sync::Arc::make_mut(&mut shadow.operations)
        .apply_market_shock(crate::company::events::ActiveShock {
            kind: crate::company::ShockKind::MarketDemandShift,
            amplitude_bp: 200,
            starts_on: current_date,
            expires_on: current_date,
        })
        .unwrap();

    assert_eq!(game.hash_projection().unwrap(), first);

    std::sync::Arc::make_mut(&mut game.operations)
        .apply_market_shock(crate::company::events::ActiveShock {
            kind: crate::company::ShockKind::MarketDemandShift,
            amplitude_bp: 100,
            starts_on: current_date,
            expires_on: current_date,
        })
        .unwrap();

    let authority_after = game.hash_projection().unwrap();
    let shadow_after = shadow.hash_projection().unwrap();
    assert_ne!(authority_after, first);
    assert_ne!(shadow_after, first);
    assert_ne!(authority_after, shadow_after);

    let restored_authority: FinancialFixture = serde_json::from_slice(&serde_json::to_vec(&game).unwrap()).unwrap();
    let restored_shadow: FinancialFixture = serde_json::from_slice(&serde_json::to_vec(&shadow).unwrap()).unwrap();
    assert_eq!(
        restored_authority.hash_projection().unwrap(),
        authority_after
    );
    assert_eq!(restored_shadow.hash_projection().unwrap(), shadow_after);
}

#[test]
fn company_operations_projection_survives_serde_round_trip_without_serializing_the_cache() {
    let game = financial_fixture();
    let expected = game.operations.hash_projection().unwrap();
    let json = serde_json::to_vec(&game.operations).unwrap();
    let restored: crate::company::operations::CompanyOperations =
        serde_json::from_slice(&json).unwrap();

    assert_eq!(restored, *game.operations);
    assert_eq!(restored.hash_projection().unwrap(), expected);
}

#[test]
fn closing_hash_cache_is_invalidated_by_authoritative_recording() {
    let mut game = financial_fixture();
    let business_before = game.hash_projection().unwrap();
    let report = game
        .library
        .save()
        .reports
        .first()
        .expect("prehistory publishes at least one report")
        .reports
        .clone();

    std::sync::Arc::make_mut(&mut game.closing)
        .record(report)
        .unwrap();

    let business_after = game.hash_projection().unwrap();
    assert_ne!(business_after, business_before);
    assert_eq!(game.hash_projection().unwrap(), business_after);
}

#[test]
fn closing_projection_clone_mutations_are_isolated_and_serde_stable() {
    let game = financial_fixture();
    let mut authority = game.closing.as_ref().clone();
    let expected = authority.hash_projection().unwrap();
    let mut cloned = authority.clone();
    let reports = game.library.save().reports;
    assert!(
        reports.len() >= 2,
        "fixture needs two published report sets"
    );

    cloned.record(reports[1].reports.clone()).unwrap();

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
