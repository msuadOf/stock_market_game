pub async fn create_market(
    manager: &SessionManager,
    setup: engine::SessionSetup,
    seed: u64,
) -> (String, engine::session::OpaqueSubjectId, String) {
    let (subject, token) = guest(manager);
    let (id, created) = manager
        .new_shared_session(setup, seed, subject.clone())
        .expect("shared market should be created");
    assert!(
        created,
        "fresh test manager should create its shared market"
    );
    (id, subject, token)
}
