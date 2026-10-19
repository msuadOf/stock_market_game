use server::identity::IdentityService;
use server::SessionManager;

pub fn guest(manager: &SessionManager) -> (engine::session::OpaqueSubjectId, String) {
    let identity = IdentityService::new(manager.database())
        .guest()
        .expect("guest identity should be created");
    let subject = engine::session::OpaqueSubjectId::new(identity.subject.subject_id)
        .expect("identity subject should be valid");
    (subject, identity.token)
}

pub async fn create_market(
    manager: &SessionManager,
    setup: engine::SessionSetup,
    seed: u64,
) -> (String, engine::session::OpaqueSubjectId, String) {
    let (subject, token) = guest(manager);
    let (id, created) = manager
        .new_shared_session(setup, seed, subject.clone())
        .expect("shared market should be created");
    assert!(created, "fresh test manager should create its shared market");
    (id, subject, token)
}

pub async fn generation(
    handles: &server::SessionHandles,
    subject: engine::session::OpaqueSubjectId,
) -> String {
    handles
        .market_context_for(subject)
        .await
        .expect("market context should be available")
        .generation
}
