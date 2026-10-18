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
    AccountValidationShards,

    AccountValidationWorkerResults,

    ContinuousStockShards,

    ContinuousWorkerResults,

    AuctionStockShards,

    AuctionWorkerResults,

    AggregatedReceiptResults,
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
    static SCOPE: RefCell<Option<std::sync::Arc<std::sync::Mutex<Scope>>>> = const { RefCell::new(None) };
}

/// 计算上下文切换时继承真实扰动捕获，不关闭门禁，也不伪造结果顺序。
#[derive(Clone)]
pub(crate) struct ExecutorBinding(Option<std::sync::Arc<std::sync::Mutex<Scope>>>);

#[cfg(test)]
impl ExecutorBinding {
    pub(super) fn empty() -> Self {
        Self(None)
    }
    pub(super) fn is_empty(&self) -> bool {
        self.0.is_none()
    }
    pub(super) fn same_as(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(left), Some(right)) => std::sync::Arc::ptr_eq(left, right),
            (None, None) => true,
            _ => false,
        }
    }

    /// 当前绑定持有的扰动观察账本强引用数（空绑定为 0）。
    /// 与 PhaseTimingBinding::strong_count 同口径，用于两段式收尾断言。
    pub(super) fn strong_count(&self) -> usize {
        self.0
            .as_ref()
            .map(|scope| std::sync::Arc::strong_count(scope))
            .unwrap_or(0)
    }
}

pub(crate) fn current_executor_binding() -> ExecutorBinding {
    ExecutorBinding(SCOPE.with_borrow(Clone::clone))
}

/// 调用线程上是否挂有活动扰动捕获（P0 重叠恢复的串行回退判据：扰动
/// 框架按串行边界纪律建账，并行准备会改变边界到达线程/次序——激活时
/// plan_tick 回退串行，保持扰动语义与既有记录可比）。
pub(in crate::session::pipeline) fn perturbation_scope_active() -> bool {
    SCOPE.with_borrow(|scope| scope.is_some())
}

pub(crate) fn with_executor_binding<R>(binding: ExecutorBinding, work: impl FnOnce() -> R) -> R {
    struct Restore(Option<std::sync::Arc<std::sync::Mutex<Scope>>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SCOPE.with_borrow_mut(|scope| *scope = self.0.take());
        }
    }
    let previous = SCOPE.with_borrow_mut(|slot| std::mem::replace(slot, binding.0));
    let _restore = Restore(previous);
    work()
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
/// never become session/save authority.
/// 显式 compute burst 继承本捕获；外层 operation 与 publication callback 仍留原调用线程。
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
        *scope = Some(std::sync::Arc::new(std::sync::Mutex::new(Scope {
            config,
            records: Vec::new(),
        })));
        Ok(())
    })?;
    let reset = ResetScope;
    let result = operation();
    let records = SCOPE.with_borrow_mut(|scope| {
        std::mem::take(
            &mut scope
                .as_ref()
                .expect("active executor scope")
                .lock()
                .expect("扰动观察账本锁被 poison")
                .records,
        )
    });
    drop(reset);
    Ok((result, records))
}

pub(super) fn merge_enabled(merge: CanonicalMerge) -> bool {
    SCOPE.with_borrow(|scope| {
        scope.as_ref().is_none_or(|scope| {
            scope
                .lock()
                .expect("扰动观察账本锁被 poison")
                .config
                .disable_merge
                != Some(merge)
        })
    })
}

pub(super) fn reorder<T>(
    boundary: ExecutorBoundary,
    values: &mut [T],
    identity: impl Fn(&T) -> (String, usize),
) {
    SCOPE.with_borrow_mut(|scope| {
        let Some(scope) = scope else { return };
        let mut scope = scope.lock().expect("扰动观察账本锁被 poison");
        let (permutation, delivery) = match boundary {
            ExecutorBoundary::AccountValidationShards => (scope.config.account_shards, false),
            ExecutorBoundary::ContinuousStockShards | ExecutorBoundary::AuctionStockShards => {
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
