use super::*;

#[test]
fn one_capture_keeps_cursor_catalog_and_owner_projection_together() {
    let setup = crate::session::npc_working_quote_tests::quote_setup(0);
    let session = ProtocolSession::new(setup, 42).unwrap();
    let captured = CapturedBaseline::capture(&session, Some(AccountId(0)), None).unwrap();
    assert_eq!(captured.snapshot.tick, session.tick());
    assert_eq!(captured.civil_date, session.civil_date());
    assert_eq!(captured.stock_catalog, session.stock_catalog_view());
    let public = CapturedBaseline::capture(&session, None, None).unwrap();
    assert!(public.working_orders.is_empty());
    assert!(public.failure.is_none());
}

#[test]
fn observing_existing_recovery_does_not_discover_or_reapply_a_new_failure() {
    let session =
        ProtocolSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let failure = HostFailure::archive("存储故障");
    let captured =
        CapturedBaseline::capture(&session, Some(AccountId(0)), Some(failure.clone())).unwrap();
    assert_eq!(captured.failure, Some(failure));
    assert!(!captured.discovered_failure);
}
