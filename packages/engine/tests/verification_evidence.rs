//! Standalone compilation bridge for the Task 9 producer.
//!
//! The production `lib.rs` registration is intentionally delivered as a
//! separate wiring patch because that shared file belongs to the B3 integration
//! window.  Re-exporting the engine surface here lets the new module and its
//! unit tests compile against the exact same `crate::...` paths before wiring.

pub use engine::*;

#[path = "../src/verification_evidence.rs"]
mod verification_evidence;

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
