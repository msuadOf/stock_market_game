//! Standalone compilation bridge for the Task 9 producer.
//!
//! The production `lib.rs` registration is intentionally delivered as a
//! separate wiring patch because that shared file belongs to the B3 integration
//! window.  Re-exporting the engine surface here lets the new module and its
//! unit tests compile against the exact same `crate::...` paths before wiring.

pub use engine::*;

#[path = "../src/verification_evidence.rs"]
mod verification_evidence;
