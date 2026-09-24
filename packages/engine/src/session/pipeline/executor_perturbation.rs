//! Scoped harness controls at real production executor/collector boundaries.
//!
//! This module is absent from ordinary builds. The scope belongs to the coordinator
//! thread: enter it *inside* `ThreadPool::install`, then call the normal public step.
//! Rayon workers do not read thread-local controls. Only their coordinator changes
//! dispatch/result vectors; business operations within an entity remain ordered.

use super::StepFatal;
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub enum ExecutorPermutation {
    #[default]
    Canonical,
    Reverse,
    RotateLeft,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum CanonicalMerge {
    Stock,
    Completion,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize)]
pub struct ExecutorPerturbation {
    pub account_shards: ExecutorPermutation,
    pub stock_shards: ExecutorPermutation,
    pub worker_results: ExecutorPermutation,
    pub disable_merge: Option<CanonicalMerge>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum ExecutorBoundary {
    P3AccountShards,
    P3WorkerResults,
    P4ContinuousStockShards,
    P4ContinuousWorkerResults,
    P4AuctionStockShards,
    P4AuctionWorkerResults,
    P5ReceiptResults,
}

/// Actual identities, captured after delivery permutation and before the real merge.
/// Item counts let a verifier reject empty-shard or empty-result demonstrations.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ExecutorOrderRecord {
    pub boundary: ExecutorBoundary,
    pub identities: Vec<String>,
    pub item_counts: Vec<usize>,
}

struct Scope {
    config: ExecutorPerturbation,
    records: Vec<ExecutorOrderRecord>,
}

thread_local! {
    static SCOPE: RefCell<Option<Scope>> = const { RefCell::new(None) };
}

struct ResetScope;
impl Drop for ResetScope {
    fn drop(&mut self) {
        SCOPE.with_borrow_mut(|scope| *scope = None);
    }
}

/// Runs the supplied public-engine operation with explicit scheduling controls.
///
/// The operation's return value (including its own error) is preserved, so a failed
/// negative control still yields its actual pre-canonical evidence. Nested scopes
/// are rejected. Controls are cleared on normal return and stack unwinding, and
/// never become session/save authority. Do not move the operation to another thread.
pub fn with_executor_perturbation<R>(
    config: ExecutorPerturbation,
    operation: impl FnOnce() -> R,
) -> Result<(R, Vec<ExecutorOrderRecord>), StepFatal> {
    SCOPE.with_borrow_mut(|scope| {
        if scope.is_some() {
            return Err(StepFatal::InvariantViolation {
                description: "nested executor perturbation scopes are not supported".to_owned(),
                location: "pipeline::with_executor_perturbation".to_owned(),
            });
        }
        *scope = Some(Scope {
            config,
            records: Vec::new(),
        });
        Ok(())
    })?;
    let reset = ResetScope;
    let result = operation();
    let records = SCOPE.with_borrow_mut(|scope| {
        std::mem::take(&mut scope.as_mut().expect("active executor scope").records)
    });
    drop(reset);
    Ok((result, records))
}

pub(super) fn merge_enabled(merge: CanonicalMerge) -> bool {
    SCOPE.with_borrow(|scope| {
        scope
            .as_ref()
            .is_none_or(|scope| scope.config.disable_merge != Some(merge))
    })
}

pub(super) fn reorder<T>(
    boundary: ExecutorBoundary,
    values: &mut [T],
    identity: impl Fn(&T) -> (String, usize),
) {
    SCOPE.with_borrow_mut(|scope| {
        let Some(scope) = scope else { return };
        let (permutation, delivery) = match boundary {
            ExecutorBoundary::P3AccountShards => (scope.config.account_shards, false),
            ExecutorBoundary::P4ContinuousStockShards | ExecutorBoundary::P4AuctionStockShards => {
                (scope.config.stock_shards, false)
            }
            _ => (scope.config.worker_results, true),
        };
        // Delivery order is selected independently of dispatch order. Sort only the
        // actual returned vector (never payloads) to choose its stable permutation.
        // The downstream production merge remains responsible for business ordering.
        if delivery {
            values.sort_by_cached_key(|value| identity(value).0);
        }
        match permutation {
            ExecutorPermutation::Canonical => {}
            ExecutorPermutation::Reverse => values.reverse(),
            ExecutorPermutation::RotateLeft if values.len() > 1 => values.rotate_left(1),
            ExecutorPermutation::RotateLeft => {}
        }
        let (identities, item_counts) = values.iter().map(identity).unzip();
        scope.records.push(ExecutorOrderRecord {
            boundary,
            identities,
            item_counts,
        });
    });
}
