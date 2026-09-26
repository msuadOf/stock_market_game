//! Tick-private stock jobs. A completed stock can release its dependent plan work
//! while unrelated books are still running on the same Rayon pool.

#[cfg(any(test, feature = "verification-harness"))]
use super::{executor_perturbation, ExecutorBoundary};
use super::{
    p4_continuous::{
        ContinuousExecutionRound, ContinuousStockInput, IncrementalContinuousStockCoordinator,
        IncrementalContinuousStockFinish,
    },
    stock_auction::b2_auction_day_end::{
        AuctionExecutionRound, IncrementalAuctionFinish, IncrementalAuctionStockCoordinator,
    },
    stock_auction_adapter::AuctionStockInput,
    P3ValidatedOperation, StepFatal,
};
use crate::{AccountId, StockCode, TradingPhase};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc;

#[cfg(test)]
#[path = "stock_stream/auction_tests.rs"]
mod auction_tests;
#[cfg(test)]
#[path = "stock_stream/root_ready_tests.rs"]
mod root_ready_tests;

/// Payloads remain on their typed channels. Both kinds of completed work wake
/// the same coordinator, without making one wait for unrelated stock work.
#[derive(Debug)]
pub(in crate::session) enum TickWorkReady {
    PlanRoot,
    Stock,
}

pub(super) struct StockStreamNotifications {
    pub(super) sender: mpsc::Sender<TickWorkReady>,
    pub(super) receiver: mpsc::Receiver<TickWorkReady>,
}

impl StockStreamNotifications {
    pub(super) fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self { sender, receiver }
    }
}

pub(super) enum StockStreamProgress<'a, R> {
    /// No stock worker remains, so the plan source may wait for its next root.
    Idle,
    /// This notification may outlive a payload consumed by an earlier poll.
    /// Inspect roots without waiting while stock work is still in flight.
    PlanRootReady,
    StockCompleted {
        code: &'a StockCode,
        round: &'a R,
    },
}

pub(super) trait StockShard: Send {
    type Round: Send;

    #[cfg(any(test, feature = "verification-harness"))]
    const DISPATCH_BOUNDARY: Option<ExecutorBoundary>;

    fn apply(&mut self, operations: Vec<P3ValidatedOperation>) -> Result<Self::Round, StepFatal>;
}

impl StockShard for IncrementalContinuousStockCoordinator {
    type Round = ContinuousExecutionRound;

    #[cfg(any(test, feature = "verification-harness"))]
    const DISPATCH_BOUNDARY: Option<ExecutorBoundary> =
        Some(ExecutorBoundary::P4ContinuousStockShards);

    fn apply(&mut self, operations: Vec<P3ValidatedOperation>) -> Result<Self::Round, StepFatal> {
        self.apply_round(operations)
    }
}

impl StockShard for IncrementalAuctionStockCoordinator {
    type Round = AuctionExecutionRound;

    #[cfg(any(test, feature = "verification-harness"))]
    const DISPATCH_BOUNDARY: Option<ExecutorBoundary> =
        Some(ExecutorBoundary::P4AuctionStockShards);

    fn apply(&mut self, operations: Vec<P3ValidatedOperation>) -> Result<Self::Round, StepFatal> {
        self.apply_round(operations)
    }
}

pub(super) fn continuous_shards(
    inputs: Vec<ContinuousStockInput>,
) -> Result<BTreeMap<StockCode, IncrementalContinuousStockCoordinator>, StepFatal> {
    let mut initialized = inputs
        .into_par_iter()
        .map(|input| {
            let code = input.market.code().clone();
            let shard = IncrementalContinuousStockCoordinator::from_post_p0(vec![input]);
            (code, shard)
        })
        .collect::<Vec<_>>();
    initialized.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    initialized
        .into_iter()
        .map(|(code, shard)| shard.map(|shard| (code, shard)))
        .collect()
}

pub(super) fn finish_continuous_shards(
    shards: BTreeMap<StockCode, IncrementalContinuousStockCoordinator>,
    ends_day: bool,
) -> Result<IncrementalContinuousStockFinish, StepFatal> {
    let mut workers = Vec::new();
    let mut prices = BTreeMap::new();
    let mut execution_facts = Vec::new();
    let mut detached_facts = Vec::new();
    let finished = shards
        .into_iter()
        .collect::<Vec<_>>()
        .into_par_iter()
        .map(|(code, shard)| {
            shard
                .finish_for_tick(ends_day)
                .map(|finished| (code, finished))
        })
        .collect::<Vec<_>>();
    #[cfg(any(test, feature = "verification-harness"))]
    let finished = {
        // This is the final collection after all roots and stock jobs drain,
        // not a barrier on the incremental completion notifications above it.
        let mut finished = finished
            .into_iter()
            .collect::<Result<Vec<_>, StepFatal>>()?;
        executor_perturbation::reorder(
            ExecutorBoundary::P4ContinuousWorkerResults,
            &mut finished,
            |(code, result)| {
                (
                    code.0.clone(),
                    result.execution_facts.len() + result.detached_facts.len(),
                )
            },
        );
        finished
            .into_iter()
            .map(Ok::<_, StepFatal>)
            .collect::<Vec<_>>()
    };
    for result in finished {
        let (_, finished) = result?;
        workers.extend(finished.workers);
        for (code, price) in finished.prices {
            if prices.insert(code, price).is_some() {
                return Err(invariant("continuous stock finished twice"));
            }
        }
        execution_facts.extend(finished.execution_facts);
        detached_facts.extend(finished.detached_facts);
    }
    Ok(IncrementalContinuousStockFinish {
        workers,
        prices,
        execution_facts,
        detached_facts,
    })
}

pub(super) fn auction_shards(
    inputs: Vec<AuctionStockInput>,
) -> Result<BTreeMap<StockCode, IncrementalAuctionStockCoordinator>, StepFatal> {
    inputs
        .into_iter()
        .map(|input| {
            let code = input.code.clone();
            IncrementalAuctionStockCoordinator::from_post_p0(vec![input]).map(|shard| (code, shard))
        })
        .collect()
}

pub(super) fn finish_auction_shards(
    shards: BTreeMap<StockCode, IncrementalAuctionStockCoordinator>,
    tick_after: u64,
    finish_auction: bool,
    finish_day: bool,
) -> Result<IncrementalAuctionFinish, StepFatal> {
    let mut workers = BTreeMap::new();
    let mut detached_event_facts = Vec::new();
    let mut detached_lifecycle_facts = Vec::new();
    let finished = shards
        .into_iter()
        .collect::<Vec<_>>()
        .into_par_iter()
        .map(|(code, shard)| {
            shard
                .finish(tick_after, finish_auction, finish_day)
                .map(|finished| (code, finished))
        })
        .collect::<Vec<_>>();
    #[cfg(any(test, feature = "verification-harness"))]
    let finished = {
        // Select the first stock error before perturbing successful final output.
        // These are drained-stream finish results, not per-arrival notifications.
        let mut finished = finished
            .into_iter()
            .collect::<Result<Vec<_>, StepFatal>>()?;
        executor_perturbation::reorder(
            ExecutorBoundary::P4AuctionWorkerResults,
            &mut finished,
            |(code, result)| {
                (
                    code.0.clone(),
                    result.detached_lifecycle_facts.len()
                        + result
                            .workers
                            .values()
                            .map(|worker| worker.lifecycle_facts.len())
                            .sum::<usize>(),
                )
            },
        );
        finished.into_iter().map(Ok::<_, StepFatal>)
    };
    // Production merges in stock order; the verification harness may reorder
    // successful payloads only, after ordered error selection. Independent books
    // have already completed their own clearing and finalization.
    for result in finished {
        let (_, finished) = result?;
        for (code, worker) in finished.workers {
            if workers.insert(code, worker).is_some() {
                return Err(invariant("auction stock finished twice"));
            }
        }
        detached_event_facts.extend(finished.detached_event_facts);
        detached_lifecycle_facts.extend(finished.detached_lifecycle_facts);
    }
    Ok(IncrementalAuctionFinish {
        workers,
        detached_event_facts,
        detached_lifecycle_facts,
    })
}

pub(super) fn detached_continuous_shard(
    _: &StockCode,
    phase: TradingPhase,
) -> Result<IncrementalContinuousStockCoordinator, StepFatal> {
    Ok(IncrementalContinuousStockCoordinator::detached(phase))
}

pub(super) fn detached_auction_shard(
    _: &StockCode,
) -> Result<IncrementalAuctionStockCoordinator, StepFatal> {
    Ok(IncrementalAuctionStockCoordinator::detached())
}

pub(super) fn operation_code(operation: &P3ValidatedOperation) -> &StockCode {
    match operation {
        P3ValidatedOperation::Place(draft) => draft.code(),
        P3ValidatedOperation::Cancel { code, .. } => code,
    }
}

pub(super) fn operation_owner(operation: &P3ValidatedOperation) -> AccountId {
    match operation {
        P3ValidatedOperation::Place(draft) => draft.owner(),
        P3ValidatedOperation::Cancel { account, .. } => *account,
    }
}

/// A worker owns one book until it returns the book and its typed facts. The
/// coordinator alone touches P3, plans and the session, and can immediately
/// enqueue newly ready operations for any idle book.
pub(super) fn drive_stock_stream<S, F, D>(
    mut available: BTreeMap<StockCode, S>,
    initial: Vec<P3ValidatedOperation>,
    notifications: StockStreamNotifications,
    mut detached: D,
    mut on_progress: F,
) -> Result<BTreeMap<StockCode, S>, StepFatal>
where
    S: StockShard,
    F: FnMut(StockStreamProgress<'_, S::Round>) -> Result<Vec<P3ValidatedOperation>, StepFatal>
        + Send,
    D: FnMut(&StockCode) -> Result<S, StepFatal> + Send,
{
    rayon::scope(move |scope| {
        let (sender, receiver) = mpsc::channel();
        let mut pending = BTreeMap::<StockCode, Vec<P3ValidatedOperation>>::new();
        let mut in_flight = BTreeSet::<StockCode>::new();
        enqueue(&mut pending, initial);
        loop {
            let dispatchable = pending
                .keys()
                .filter(|code| !in_flight.contains(*code))
                .cloned()
                .collect::<Vec<_>>();
            #[cfg(any(test, feature = "verification-harness"))]
            let dispatchable = {
                let mut dispatchable = dispatchable;
                if let Some(boundary) = S::DISPATCH_BOUNDARY {
                    executor_perturbation::reorder(boundary, &mut dispatchable, |code| {
                        (code.0.clone(), pending[code].len())
                    });
                }
                dispatchable
            };
            for code in dispatchable {
                let shard = match available.remove(&code) {
                    Some(shard) => shard,
                    None => detached(&code)?,
                };
                let operations = pending
                    .remove(&code)
                    .ok_or_else(|| invariant("dispatchable stock lost its operations"))?;
                let worker_sender = sender.clone();
                let worker_notifications = notifications.sender.clone();
                in_flight.insert(code.clone());
                scope.spawn(move |_| {
                    let mut shard = shard;
                    let round = shard.apply(operations);
                    // A closed receiver means the whole private tick has failed.
                    if worker_sender.send((code, shard, round)).is_ok() {
                        let _ = worker_notifications.send(TickWorkReady::Stock);
                    }
                });
            }

            if in_flight.is_empty() {
                crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
                let ready = on_progress(StockStreamProgress::Idle)?;
                if ready.is_empty() {
                    return Ok(available);
                }
                enqueue(&mut pending, ready);
                continue;
            }

            crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
            let completed = if rayon::current_num_threads() == 1 {
                loop {
                    match notifications.receiver.try_recv() {
                        Ok(done) => break done,
                        Err(mpsc::TryRecvError::Disconnected) => {
                            return Err(invariant(
                                "work notification channel closed during stock processing",
                            ));
                        }
                        Err(mpsc::TryRecvError::Empty) => {
                            // The coordinator occupies the only worker, so it
                            // must help execute pending stock jobs or plan roots.
                            let _ = rayon::yield_now();
                        }
                    }
                }
            } else {
                notifications.receiver.recv().map_err(|_| {
                    invariant("work notification channel closed during stock processing")
                })?
            };
            if matches!(completed, TickWorkReady::PlanRoot) {
                crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
                enqueue(
                    &mut pending,
                    on_progress(StockStreamProgress::PlanRootReady)?,
                );
                continue;
            }
            // A stock notification is sent only after its payload. Notifications
            // from different workers need not have the same order as payloads.
            let (code, shard, round) = receiver
                .try_recv()
                .map_err(|_| invariant("stock notification has no completed book"))?;
            if !in_flight.remove(&code) || available.insert(code.clone(), shard).is_some() {
                return Err(invariant("stock worker returned a duplicate book"));
            }
            let round = round?;
            crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
            enqueue(
                &mut pending,
                on_progress(StockStreamProgress::StockCompleted {
                    code: &code,
                    round: &round,
                })?,
            );
        }
    })
}

fn enqueue(
    pending: &mut BTreeMap<StockCode, Vec<P3ValidatedOperation>>,
    operations: Vec<P3ValidatedOperation>,
) {
    for operation in operations {
        pending
            .entry(operation_code(&operation).clone())
            .or_default()
            .push(operation);
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::stock_stream".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Envelope, EnvelopeAudit, EnvelopeKey, FeeComponents};
    use super::*;
    use crate::{AccountId, GameConfig, Market, Money, OrderId, Side, TradingPhase};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    struct Signals {
        b_running: AtomicBool,
        a_followup_started: AtomicBool,
        overlapped: AtomicBool,
    }

    struct MockShard {
        code: StockCode,
        steps: usize,
        signals: Arc<Signals>,
    }

    struct MockRound {
        order: OrderId,
    }

    impl StockShard for MockShard {
        type Round = MockRound;
        const DISPATCH_BOUNDARY: Option<ExecutorBoundary> = None;

        fn apply(
            &mut self,
            operations: Vec<P3ValidatedOperation>,
        ) -> Result<Self::Round, StepFatal> {
            let [P3ValidatedOperation::Cancel { order_id, .. }] = operations.as_slice() else {
                return Err(invariant("mock received an unexpected operation batch"));
            };
            self.steps += 1;
            if self.code.0 == "A" && self.steps == 1 {
                let deadline = Instant::now() + Duration::from_secs(2);
                while !self.signals.b_running.load(Ordering::SeqCst) && Instant::now() < deadline {
                    std::thread::yield_now();
                }
            } else if self.code.0 == "A" {
                self.signals
                    .a_followup_started
                    .store(true, Ordering::SeqCst);
                self.signals.overlapped.store(
                    self.signals.b_running.load(Ordering::SeqCst),
                    Ordering::SeqCst,
                );
            } else {
                self.signals.b_running.store(true, Ordering::SeqCst);
                let deadline = Instant::now() + Duration::from_secs(2);
                while !self.signals.a_followup_started.load(Ordering::SeqCst)
                    && Instant::now() < deadline
                {
                    std::thread::yield_now();
                }
                self.signals.b_running.store(false, Ordering::SeqCst);
            }
            Ok(MockRound { order: *order_id })
        }
    }

    fn cancel(code: &str, id: u64) -> P3ValidatedOperation {
        P3ValidatedOperation::Cancel {
            candidate_key: super::super::P2CandidateKey::player(id),
            sealed_index: id,
            account: AccountId(id),
            code: StockCode(code.to_owned()),
            order_id: OrderId(id),
        }
    }

    #[test]
    fn parallel_stock_initialization_reports_the_first_stock_error() {
        let input = |code: &str| ContinuousStockInput {
            phase: TradingPhase::Continuous,
            market: Market::new(
                StockCode(code.to_owned()),
                Money::from_cents(1_000),
                0.10,
                Money::from_cents(1),
            )
            .unwrap(),
            envelopes: Vec::new(),
            operations: Vec::new(),
            config: GameConfig::proposed_defaults(),
        };
        let mut first = input("600001");
        first.operations.push(cancel("600001", 1));
        let mut second = input("600002");
        let envelope = Envelope::tick_start_existing(
            EnvelopeKey {
                account: AccountId(1),
                stock: StockCode("600002".to_owned()),
                order: OrderId(2),
                side: Side::Buy,
            },
            Money::from_cents(1_000),
            0,
            EnvelopeAudit {
                limit: Money::from_cents(1_000),
                remaining_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        );
        second
            .envelopes
            .push(super::super::p4_continuous::ContinuousEnvelopeSnapshot {
                audit: envelope.audit(),
                envelope,
            });

        for _ in 0..8 {
            let error = continuous_shards(vec![second.clone(), first.clone()]).unwrap_err();
            assert!(
                matches!(error, StepFatal::InvariantViolation { description, .. }
                if description.contains("initialization included sealed operations"))
            );
        }
    }

    #[test]
    fn completed_stock_starts_its_followup_while_another_stock_is_running() {
        let signals = Arc::new(Signals {
            b_running: AtomicBool::new(false),
            a_followup_started: AtomicBool::new(false),
            overlapped: AtomicBool::new(false),
        });
        let shards = ["A", "B"]
            .into_iter()
            .map(|code| {
                (
                    StockCode(code.to_owned()),
                    MockShard {
                        code: StockCode(code.to_owned()),
                        steps: 0,
                        signals: Arc::clone(&signals),
                    },
                )
            })
            .collect();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(3)
            .build()
            .unwrap();
        let completed = pool
            .install(|| {
                drive_stock_stream(
                    shards,
                    vec![cancel("A", 1), cancel("B", 2)],
                    StockStreamNotifications::new(),
                    |_| Err(invariant("mock must not request a detached stock")),
                    |progress| match progress {
                        StockStreamProgress::StockCompleted { code, round }
                            if code.0 == "A" && round.order == OrderId(1) =>
                        {
                            Ok(vec![cancel("A", 3)])
                        }
                        _ => Ok(Vec::new()),
                    },
                )
            })
            .unwrap();
        assert!(signals.overlapped.load(Ordering::SeqCst));
        assert_eq!(completed[&StockCode("A".to_owned())].steps, 2);
        assert_eq!(completed[&StockCode("B".to_owned())].steps, 1);
    }

    #[test]
    fn one_worker_runs_a_stock_job_without_blocking_itself() {
        let signals = Arc::new(Signals {
            b_running: AtomicBool::new(true),
            a_followup_started: AtomicBool::new(false),
            overlapped: AtomicBool::new(false),
        });
        let code = StockCode("A".to_owned());
        let shard = MockShard {
            code: code.clone(),
            steps: 0,
            signals,
        };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .unwrap();
        let finished = pool
            .install(|| {
                drive_stock_stream(
                    BTreeMap::from([(code.clone(), shard)]),
                    vec![cancel("A", 1)],
                    StockStreamNotifications::new(),
                    |_| Err(invariant("mock must not request a detached stock")),
                    |_| Ok(Vec::new()),
                )
            })
            .unwrap();
        assert_eq!(finished[&code].steps, 1);
    }
}
