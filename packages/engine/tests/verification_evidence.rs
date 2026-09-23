//! Public-surface integration check for the Task 9 evidence producer.
//!
//! Unit tests compile inside the production module. This integration target
//! deliberately imports only the crate's public API so it cannot create a
//! second, type-incompatible copy of the evidence module.

#[test]
fn full_update_stream_projection_is_exposed_by_the_engine_crate() {
    let _projector = engine::verification_evidence::UpdateStreamProjector::new();
    let _project_update = engine::verification_evidence::UpdateStreamProjector::project_update;
    let empty: [engine::verification_evidence::RuntimeUpdateRef<'_>; 0] = [];

    assert_eq!(
        engine::verification_evidence::project_update_stream(&empty).unwrap_err(),
        engine::verification_evidence::EvidenceError::InvalidUpdate {
            detail: "full runtime update stream contains no updates".to_owned(),
        }
    );
}
