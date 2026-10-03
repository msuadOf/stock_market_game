//! tick 内的股票私有任务：已完成股票及时推进依赖计划，无关股票继续在 Rayon pool 执行。

use super::{
    continuous_matching::{
        ContinuousExecutionRound, ContinuousStockInput, IncrementalContinuousStockCoordinator,
        IncrementalContinuousStockFinish,
    },
    stock_auction::auction_day_end::{
        AuctionExecutionRound, AuctionTickBoundary, IncrementalAuctionFinish,
        IncrementalAuctionStockCoordinator,
    },
    stock_auction_adapter::AuctionStockInput,
    StepFatal, ValidatedOperation,
};
#[cfg(any(test, feature = "verification-harness"))]
use super::{executor_perturbation, ExecutorBoundary};
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

/// payload 保留在各自 typed channel；两类完成通知共用唤醒入口，无须等待无关股票。
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
    /// 已无股票 worker，计划来源可以等待下一个 root。
    Idle,
    /// 早先轮询可能已消费 root payload；仍有股票任务时只轮询，不等待 root。
    PlanRootReady,
    StockCompleted {
        code: &'a StockCode,
        round: &'a mut R,
    },
}

pub(super) trait StockShard: Send {
    type Round: Send;

    #[cfg(any(test, feature = "verification-harness"))]
    const DISPATCH_BOUNDARY: Option<ExecutorBoundary>;

    fn apply(&mut self, operations: Vec<ValidatedOperation>) -> Result<Self::Round, StepFatal>;
}

impl StockShard for IncrementalContinuousStockCoordinator {
    type Round = ContinuousExecutionRound;

    #[cfg(any(test, feature = "verification-harness"))]
    const DISPATCH_BOUNDARY: Option<ExecutorBoundary> =
        Some(ExecutorBoundary::ContinuousStockShards);

    fn apply(&mut self, operations: Vec<ValidatedOperation>) -> Result<Self::Round, StepFatal> {
        self.apply_round(operations)
    }
}

impl StockShard for IncrementalAuctionStockCoordinator {
    type Round = AuctionExecutionRound;

    #[cfg(any(test, feature = "verification-harness"))]
    const DISPATCH_BOUNDARY: Option<ExecutorBoundary> = Some(ExecutorBoundary::AuctionStockShards);

    fn apply(&mut self, operations: Vec<ValidatedOperation>) -> Result<Self::Round, StepFatal> {
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
            let shard = IncrementalContinuousStockCoordinator::from_post_expiry(vec![input]);
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
        // root 与股票任务排空后才汇总终结结果；增量完成通知不受此屏障限制。
        let mut finished = finished
            .into_iter()
            .collect::<Result<Vec<_>, StepFatal>>()?;
        executor_perturbation::reorder(
            ExecutorBoundary::ContinuousWorkerResults,
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
            IncrementalAuctionStockCoordinator::from_post_expiry(vec![input])
                .map(|shard| (code, shard))
        })
        .collect()
}

pub(super) fn finish_auction_shards(
    shards: BTreeMap<StockCode, IncrementalAuctionStockCoordinator>,
    boundary: AuctionTickBoundary,
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
                .finish(
                    boundary.tick_after(),
                    boundary.finish_auction(),
                    boundary.finish_day(),
                )
                .map(|finished| (code, finished))
        })
        .collect::<Vec<_>>();
    #[cfg(any(test, feature = "verification-harness"))]
    let finished = {
        // 成功结果扰动前按原股票顺序选首错；此处只处理排空后的终结结果。
        let mut finished = finished
            .into_iter()
            .collect::<Result<Vec<_>, StepFatal>>()?;
        executor_perturbation::reorder(
            ExecutorBoundary::AuctionWorkerResults,
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
    // 独立股票已各自完成清算及终结；生产按股票顺序合并，verification harness
    // 只在稳定首错选择后扰动成功 payload。
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

pub(super) fn operation_code(operation: &ValidatedOperation) -> &StockCode {
    match operation {
        ValidatedOperation::Place(draft) => draft.code(),
        ValidatedOperation::Cancel { code, .. } => code,
    }
}

pub(super) fn operation_owner(operation: &ValidatedOperation) -> AccountId {
    match operation {
        ValidatedOperation::Place(draft) => draft.owner(),
        ValidatedOperation::Cancel { account, .. } => *account,
    }
}

/// worker 独占股票簿与 typed facts；调用线程中的 coordinator 及时推进就绪工作。
pub(super) fn drive_stock_stream<S, F, D>(
    available: BTreeMap<StockCode, S>,
    initial: Vec<ValidatedOperation>,
    notifications: StockStreamNotifications,
    detached: D,
    on_progress: F,
) -> Result<BTreeMap<StockCode, S>, StepFatal>
where
    S: StockShard,
    F: FnMut(StockStreamProgress<'_, S::Round>) -> Result<Vec<ValidatedOperation>, StepFatal>
        + Send,
    D: FnMut(&StockCode) -> Result<S, StepFatal> + Send,
{
    let mut coordinator = StockStreamCoordinator::new(available, notifications);
    coordinator.enqueue_ready(initial);
    coordinator.drive(detached, on_progress)
}

type StockCompletion<S> = (StockCode, S, Result<<S as StockShard>::Round, StepFatal>);

/// 只拥有本 tick 的股票工作；不持有账户、结算或 session authority。
struct StockStreamCoordinator<S: StockShard> {
    available: BTreeMap<StockCode, S>,
    pending: BTreeMap<StockCode, Vec<ValidatedOperation>>,
    in_flight: BTreeSet<StockCode>,
    sender: mpsc::Sender<StockCompletion<S>>,
    receiver: mpsc::Receiver<StockCompletion<S>>,
    notifications: StockStreamNotifications,
}

impl<S: StockShard> StockStreamCoordinator<S> {
    fn new(available: BTreeMap<StockCode, S>, notifications: StockStreamNotifications) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            available,
            pending: BTreeMap::new(),
            in_flight: BTreeSet::new(),
            sender,
            receiver,
            notifications,
        }
    }

    fn enqueue_ready(&mut self, operations: Vec<ValidatedOperation>) {
        for operation in operations {
            self.pending
                .entry(operation_code(&operation).clone())
                .or_default()
                .push(operation);
        }
    }

    fn dispatch_ready<'scope, D>(
        &mut self,
        scope: &rayon::Scope<'scope>,
        detached: &mut D,
    ) -> Result<(), StepFatal>
    where
        S: 'scope,
        D: FnMut(&StockCode) -> Result<S, StepFatal>,
    {
        let dispatchable = self
            .pending
            .keys()
            .filter(|code| !self.in_flight.contains(*code))
            .cloned()
            .collect::<Vec<_>>();
        #[cfg(any(test, feature = "verification-harness"))]
        let dispatchable = {
            let mut dispatchable = dispatchable;
            if let Some(boundary) = S::DISPATCH_BOUNDARY {
                executor_perturbation::reorder(boundary, &mut dispatchable, |code| {
                    (code.0.clone(), self.pending[code].len())
                });
            }
            dispatchable
        };
        for code in dispatchable {
            let shard = match self.available.remove(&code) {
                Some(shard) => shard,
                None => detached(&code)?,
            };
            let operations = self
                .pending
                .remove(&code)
                .ok_or_else(|| invariant("dispatchable stock lost its operations"))?;
            let worker_sender = self.sender.clone();
            let worker_notifications = self.notifications.sender.clone();
            self.in_flight.insert(code.clone());
            scope.spawn(move |_| {
                let mut shard = shard;
                let round = shard.apply(operations);
                // receiver 关闭说明整个私有 tick 已放弃；只能丢弃此项工作。
                if worker_sender.send((code, shard, round)).is_ok() {
                    let _ = worker_notifications.send(TickWorkReady::Stock);
                }
            });
        }
        Ok(())
    }

    fn poll_progress(&self) -> Result<TickWorkReady, StepFatal> {
        if rayon::current_num_threads() == 1 {
            loop {
                match self.notifications.receiver.try_recv() {
                    Ok(done) => return Ok(done),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Err(invariant(
                            "work notification channel closed during stock processing",
                        ));
                    }
                    Err(mpsc::TryRecvError::Empty) => {
                        // 单 worker 等待时帮助执行已就绪的股票任务及计划 root。
                        let _ = rayon::yield_now();
                    }
                }
            }
        } else {
            self.notifications
                .receiver
                .recv()
                .map_err(|_| invariant("work notification channel closed during stock processing"))
        }
    }

    fn receive_completion(&mut self) -> Result<(StockCode, S::Round), StepFatal> {
        // Stock notification 在 payload 成功发送后发出；跨 worker 的两种 channel 顺序可不同。
        let (code, shard, round) = self
            .receiver
            .try_recv()
            .map_err(|_| invariant("stock notification has no completed book"))?;
        if !self.in_flight.remove(&code) || self.available.insert(code.clone(), shard).is_some() {
            return Err(invariant("stock worker returned a duplicate book"));
        }
        Ok((code, round?))
    }

    fn return_idle<F>(&mut self, on_progress: &mut F) -> Result<bool, StepFatal>
    where
        F: FnMut(StockStreamProgress<'_, S::Round>) -> Result<Vec<ValidatedOperation>, StepFatal>,
    {
        crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
        let ready = on_progress(StockStreamProgress::Idle)?;
        let finished = ready.is_empty();
        self.enqueue_ready(ready);
        Ok(finished)
    }

    fn drive<F, D>(
        mut self,
        mut detached: D,
        mut on_progress: F,
    ) -> Result<BTreeMap<StockCode, S>, StepFatal>
    where
        F: FnMut(StockStreamProgress<'_, S::Round>) -> Result<Vec<ValidatedOperation>, StepFatal>
            + Send,
        D: FnMut(&StockCode) -> Result<S, StepFatal> + Send,
    {
        // coordinator 留在宿主调用线程，避免多个宿主占满 Rayon worker 后互等。
        rayon::in_place_scope(move |scope| {
            loop {
                self.dispatch_ready(scope, &mut detached)?;
                if self.in_flight.is_empty() {
                    if self.return_idle(&mut on_progress)? {
                        return Ok(self.available);
                    }
                    continue;
                }
                crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
                if matches!(self.poll_progress()?, TickWorkReady::PlanRoot) {
                    // 过时 PlanRoot 唤醒仍可轮询 root，不消费股票 payload。
                    crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
                    let ready = on_progress(StockStreamProgress::PlanRootReady)?;
                    self.enqueue_ready(ready);
                    continue;
                }
                let (code, mut round) = self.receive_completion()?;
                crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
                let ready = on_progress(StockStreamProgress::StockCompleted {
                    code: &code,
                    round: &mut round,
                })?;
                self.enqueue_ready(ready);
            }
        })
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

        fn apply(&mut self, operations: Vec<ValidatedOperation>) -> Result<Self::Round, StepFatal> {
            let [ValidatedOperation::Cancel { order_id, .. }] = operations.as_slice() else {
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

    fn cancel(code: &str, id: u64) -> ValidatedOperation {
        ValidatedOperation::Cancel {
            candidate_key: super::super::IntentCandidateKey::player(id),
            sealed_index: id,
            account: AccountId(id),
            code: StockCode(code.to_owned()),
            order_id: OrderId(id),
        }
    }

    #[derive(Debug)]
    struct EmptyShard;

    impl StockShard for EmptyShard {
        type Round = ();
        const DISPATCH_BOUNDARY: Option<ExecutorBoundary> = None;
        fn apply(&mut self, _: Vec<ValidatedOperation>) -> Result<(), StepFatal> {
            Ok(())
        }
    }

    #[test]
    fn stock_notification_without_payload_is_an_explicit_error() {
        let mut coordinator = StockStreamCoordinator::<EmptyShard>::new(
            BTreeMap::new(),
            StockStreamNotifications::new(),
        );
        coordinator
            .notifications
            .sender
            .send(TickWorkReady::Stock)
            .unwrap();
        assert!(matches!(
            coordinator.poll_progress().unwrap(),
            TickWorkReady::Stock
        ));
        assert!(coordinator
            .receive_completion()
            .unwrap_err()
            .to_string()
            .contains("no completed book"));
    }

    #[test]
    fn disconnected_notification_receiver_is_an_explicit_error() {
        let (closed_sender, receiver) = mpsc::channel();
        drop(closed_sender);
        let (sender, _other_receiver) = mpsc::channel();
        let coordinator = StockStreamCoordinator::<EmptyShard>::new(
            BTreeMap::new(),
            StockStreamNotifications { sender, receiver },
        );
        assert!(coordinator
            .poll_progress()
            .unwrap_err()
            .to_string()
            .contains("notification channel closed"));
    }

    #[test]
    fn stale_root_notification_does_not_take_stock_payload() {
        let mut coordinator = StockStreamCoordinator::<EmptyShard>::new(
            BTreeMap::new(),
            StockStreamNotifications::new(),
        );
        let code = StockCode("600001".to_owned());
        coordinator.in_flight.insert(code.clone());
        coordinator
            .sender
            .send((code.clone(), EmptyShard, Ok(())))
            .unwrap();
        coordinator
            .notifications
            .sender
            .send(TickWorkReady::PlanRoot)
            .unwrap();
        assert!(matches!(
            coordinator.poll_progress().unwrap(),
            TickWorkReady::PlanRoot
        ));
        assert_eq!(coordinator.receive_completion().unwrap().0, code);
        assert!(coordinator.in_flight.is_empty());
        assert_eq!(coordinator.available.len(), 1);
    }

    #[test]
    fn duplicate_stock_completion_is_an_explicit_error() {
        let mut coordinator = StockStreamCoordinator::<EmptyShard>::new(
            BTreeMap::new(),
            StockStreamNotifications::new(),
        );
        let code = StockCode("600001".to_owned());
        coordinator.in_flight.insert(code.clone());
        coordinator
            .sender
            .send((code.clone(), EmptyShard, Ok(())))
            .unwrap();
        coordinator.receive_completion().unwrap();
        coordinator.sender.send((code, EmptyShard, Ok(()))).unwrap();
        assert!(coordinator
            .receive_completion()
            .unwrap_err()
            .to_string()
            .contains("duplicate book"));
        assert_eq!(coordinator.available.len(), 1);
    }

    #[test]
    fn disconnected_stock_payload_receiver_is_an_explicit_error() {
        let mut coordinator = StockStreamCoordinator::<EmptyShard>::new(
            BTreeMap::new(),
            StockStreamNotifications::new(),
        );
        let (sender, receiver) = mpsc::channel();
        drop(sender);
        coordinator.receiver = receiver;
        assert!(coordinator
            .receive_completion()
            .unwrap_err()
            .to_string()
            .contains("no completed book"));
    }

    #[test]
    fn dropping_coordinator_closes_worker_payload_receiver() {
        let coordinator = StockStreamCoordinator::<EmptyShard>::new(
            BTreeMap::new(),
            StockStreamNotifications::new(),
        );
        let worker_sender = coordinator.sender.clone();
        drop(coordinator);
        assert!(worker_sender
            .send((StockCode("600001".to_owned()), EmptyShard, Ok(())))
            .is_err());
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
        second.envelopes.push(
            super::super::continuous_matching::ContinuousEnvelopeSnapshot {
                audit: envelope.audit(),
                envelope,
            },
        );

        for _ in 0..8 {
            let error = continuous_shards(vec![second.clone(), first.clone()]).unwrap_err();
            assert!(
                matches!(error, StepFatal::InvariantViolation { description, location }
                if description == "增量 StockProcessing 初始化时包含了 sealed operations"
                    && location == "pipeline::incremental_continuous_stock_shadow")
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

    #[test]
    fn concurrent_external_coordinators_leave_workers_available_for_stock_jobs() {
        let workers = rayon::current_num_threads();
        let entered = std::sync::Barrier::new(workers);
        std::thread::scope(|threads| {
            for _ in 0..workers {
                let entered = &entered;
                threads.spawn(move || {
                    let code = StockCode("A".to_owned());
                    let finished = drive_stock_stream(
                        BTreeMap::new(),
                        vec![cancel("A", 1)],
                        StockStreamNotifications::new(),
                        |code| {
                            // 分派前对齐独立宿主的 coordinator；迁入共享 pool 会占满 worker。
                            entered.wait();
                            Ok(MockShard {
                                code: code.clone(),
                                steps: 0,
                                signals: Arc::new(Signals {
                                    b_running: AtomicBool::new(true),
                                    a_followup_started: AtomicBool::new(false),
                                    overlapped: AtomicBool::new(false),
                                }),
                            })
                        },
                        |_| Ok(Vec::new()),
                    )
                    .unwrap();
                    assert_eq!(finished[&code].steps, 1);
                });
            }
        });
    }
}
