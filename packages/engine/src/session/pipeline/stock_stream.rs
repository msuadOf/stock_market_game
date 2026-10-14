//! tick 内的股票私有任务：已完成股票及时推进依赖计划，无关股票继续在 Rayon pool 执行。
//!
//! # 调度活性契约（round4 显式限定；round4-fix v2 措辞精确化；round5-fix v3 补第四条）
//!
//! 本模块的活性保证范围**显式限定**为四条前提：
//! 1. **任务独立**：每条流（`drive_stock_stream`）提交的任务不等待其他池
//!    任务的启动/完成信号；
//! 2. **有限完成**：任务在有限步内完成并发出完成通知；
//! 3. **协调者回调不做不可协作的阻塞等待**（round4-fix v2 精确化）：
//!    `detached` / `on_progress` 等待其他池任务（如计划 root）时必须用
//!    `try_recv + yield_work` 协作让位（让出的 worker 可执行它正在等待的
//!    任务本身），禁止阻塞 recv/Condvar/纯自旋式不可协作等待；
//! 4. **等待目标已无条件入队**（round5-fix v3 新增）：协调者/回调的等待
//!    目标必须已无条件入队（或已在池上执行），且该目标自身独立、有限
//!    完成、**不依赖当前协调者的后续推进**——Astra round5 给出符合旧三条
//!    字面的循环等待反例：协作等待一个尚未入队、要靠等待者自己后续推进
//!    才入队的目标时，人人都在让位而队列里没有可推进的工作，让位无法
//!    兑现成进展；仅当目标已在队中或已在执行，让出的 worker 才有真任务
//!    可取。
//!
//! 台账联合不变式 `drives + spawned ≤ workers` 只对**多 worker 池
//! （workers > 1）**声明（round5-③ 限定）；单 worker 池恒准入、spawned
//! 不记账，其活性不靠台账，由 `poll_progress` 单 worker 分支的
//! `try_recv + yield_work` 协作循环自足保证（见契约文档第三节单 worker
//! 特例）。在此契约下，协调者与池无死锁；论证与生产核证证据见
//! `.tmp/spine/round5-fix/liveness-contract.md`（Astra round4 裁定认可的
//! 支持范围；round4-fix v2 含 NPC 排队嵌套链补证与第 3 条措辞精确化；
//! round5-fix v3 补第 4 条复核与单 worker 限定）。契约外形状（任务互等
//! 启动信号、任务间依赖链、跨流任务等待、等待未无条件入队的目标）
//! **不支持**，相关旧 mock 已重分类为契约外形状记录测试。

use super::{
    continuous_matching::{
        ContinuousExecutionRound, ContinuousStockInput, IncrementalContinuousStockCoordinator,
        IncrementalContinuousStockFinish, StockFinishFatal, StockMarketDataPrior,
    },
    ready_stock_stream::FeedOutcome,
    StepFatal, ValidatedOperation,
};
#[cfg(any(test, feature = "verification-harness"))]
use super::{executor_perturbation, ExecutorBoundary};
use crate::compute::parallel::*;
use crate::{AccountId, StockId, TradingPhase};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc;

/// payload 保留在各自 typed channel；完成通知仅承载股票完成（T5 必修 1：
/// 执行期旧 root-feed 唤醒通道已删除——root 在 capture 期闭环）。
#[derive(Debug)]
pub(in crate::session) enum TickWorkReady {
    Stock,
}

/// poll_progress 的推进信号：就绪的唤醒消息，或池外协调者等到的容量恢复
/// （须交回 drive 循环重新派发，round3 P1new——池外线程不执行股票工作）。
#[derive(Debug)]
enum PollOutcome {
    Ready(TickWorkReady),
    CapacityRecovered,
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
    /// T4（Astra T4 节点审·必修 1）：typed feedback 归 owner——本股任务在
    /// 完成点自行完成受理结果↔事实匹配与轮次校验，协调者只收已匹配结果。
    StockCompleted {
        code: &'a StockId,
        round: &'a mut R,
        outcomes: &'a [super::CandidateValidationOutcome],
    },
}

/// T4：owner 完成点的 typed feedback 匹配器（fn 指针，任务内执行）——
/// 生产实现=`market_tick_transaction::match_continuous_feedback`
/// （原 ReadyStockStream::take_completed+validate_execution_round 迁移）。
pub(super) type RoundFeedbackFn<R> = fn(
    &StockId,
    &mut R,
    Vec<super::CandidateValidationOutcome>,
) -> Result<Vec<super::CandidateValidationOutcome>, StepFatal>;

/// 测试直通匹配器（mock shard 无 Continuous 事实面；期望原样转交）。
pub(in crate::session::pipeline) fn passthrough_round_feedback<R>(
    _: &StockId,
    _: &mut R,
    expected: Vec<super::CandidateValidationOutcome>,
) -> Result<Vec<super::CandidateValidationOutcome>, StepFatal> {
    Ok(expected)
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

pub(super) fn continuous_shards(
    inputs: Vec<ContinuousStockInput>,
    priors: BTreeMap<StockId, StockMarketDataPrior>,
) -> Result<BTreeMap<StockId, IncrementalContinuousStockCoordinator>, StepFatal> {
    let mut initialized = inputs
        .into_par_iter()
        .map(|input| {
            let code = input.market.code().clone();
            let shard = IncrementalContinuousStockCoordinator::from_post_expiry(vec![input]);
            (code, shard)
        })
        .collect::<Vec<_>>();
    initialized.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    let mut shards = initialized
        .into_iter()
        .map(|(code, shard)| shard.map(|shard| (code, shard)))
        .collect::<Result<BTreeMap<_, _>, StepFatal>>()?;
    // T4（行情面）：先验逐股注入 owner；先验面与 shard 面恰一致（多/缺皆结构错误）。
    if priors.len() != shards.len() {
        return Err(invariant(
            "行情先验面与股票 shard 面不一致（多或缺股票）",
        ));
    }
    for (code, prior) in priors {
        shards
            .get_mut(&code)
            .ok_or_else(|| invariant("行情先验指向不存在的股票 shard"))?
            .install_market_prior(&code, prior)?;
    }
    Ok(shards)
}

pub(super) fn finish_market_shards(
    shards: BTreeMap<StockId, IncrementalContinuousStockCoordinator>,
    boundary: super::phase::MarketTickBoundary,
    exchanges: &BTreeMap<StockId, crate::StockExchange>,
) -> Result<IncrementalContinuousStockFinish, StockFinishFatal> {
    collect_finished_shards(shards, |code, shard| {
        if let Some(exchange) = exchanges.get(&code) {
            shard.finish_for_market_tick(boundary, *exchange)
        } else if shard.is_detached() {
            shard.finish_for_tick(false)
        } else {
            Err(StockFinishFatal::Other(invariant("股票边界缺少交易所事实")))
        }
    })
}

fn collect_finished_shards(
    shards: BTreeMap<StockId, IncrementalContinuousStockCoordinator>,
    finish: impl Fn(
            StockId,
            IncrementalContinuousStockCoordinator,
        ) -> Result<IncrementalContinuousStockFinish, StockFinishFatal>
        + Sync,
) -> Result<IncrementalContinuousStockFinish, StockFinishFatal> {
    let mut workers = Vec::new();
    let mut prices = BTreeMap::new();
    let mut execution_facts = Vec::new();
    let mut detached_facts = Vec::new();
    let mut auctions = BTreeMap::new();
    let finished = shards
        .into_iter()
        .collect::<Vec<_>>()
        .into_par_iter()
        .map(|(code, shard)| finish(code, shard).map(|finished| (code, finished)))
        .collect::<Vec<_>>();
    #[cfg(any(test, feature = "verification-harness"))]
    let finished = {
        // root 与股票任务排空后才汇总终结结果；增量完成通知不受此屏障限制。
        let mut finished = finished
            .into_iter()
            .collect::<Result<Vec<_>, StockFinishFatal>>()?;
        executor_perturbation::reorder(
            ExecutorBoundary::ContinuousWorkerResults,
            &mut finished,
            |(code, result)| {
                (
                    code.to_string(),
                    result.execution_facts.len() + result.detached_facts.len(),
                )
            },
        );
        finished
            .into_iter()
            .map(Ok::<_, StockFinishFatal>)
            .collect::<Vec<_>>()
    };
    for result in finished {
        let (_, finished) = result?;
        workers.extend(finished.workers);
        for (code, price) in finished.prices {
            if prices.insert(code, price).is_some() {
                return Err(StockFinishFatal::Other(invariant(
                    "continuous stock finished twice",
                )));
            }
        }
        execution_facts.extend(finished.execution_facts);
        detached_facts.extend(finished.detached_facts);
        for (code, auction) in finished.auctions {
            if auctions.insert(code, auction).is_some() {
                return Err(StockFinishFatal::Other(invariant(
                    "auction stock finished twice",
                )));
            }
        }
    }
    Ok(IncrementalContinuousStockFinish {
        workers,
        prices,
        execution_facts,
        detached_facts,
        auctions,
    })
}

pub(super) fn detached_continuous_shard(
    _: &StockId,
    phase: TradingPhase,
) -> Result<IncrementalContinuousStockCoordinator, StepFatal> {
    Ok(IncrementalContinuousStockCoordinator::detached(phase))
}

pub(super) fn operation_code(operation: &ValidatedOperation) -> &StockId {
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
/// T4（Astra 必修 1）：`expected`（首批受理结果期望）与 `feedback`（owner 完成
/// 点的 typed feedback 匹配器）随流传入——匹配在本股任务内执行，协调者只收
/// 已匹配结果（真实账户授权与跨股依赖仍走窄交接，receive 顺序不变）。
pub(super) fn drive_stock_stream_with_feedback<S, F, D>(
    compute: &std::sync::Arc<crate::compute::ComputeContext>,
    available: BTreeMap<StockId, S>,
    initial: Vec<ValidatedOperation>,
    expected: Vec<super::CandidateValidationOutcome>,
    notifications: StockStreamNotifications,
    detached: D,
    feedback: RoundFeedbackFn<S::Round>,
    on_progress: F,
) -> Result<BTreeMap<StockId, S>, StepFatal>
where
    S: StockShard,
    F: FnMut(StockStreamProgress<'_, S::Round>) -> Result<FeedOutcome, StepFatal>
        + Send,
    D: FnMut(&StockId) -> Result<S, StepFatal> + Send,
{
    let mut coordinator = StockStreamCoordinator::new(compute.clone(), available, notifications);
    coordinator.initial_ops = Some(initial);
    coordinator.initial_expected = Some(expected);
    coordinator.drive(Vec::new(), feedback, detached, on_progress)
}

/// 直通形态（测试/mock 驱动面）：无受理期望、直通匹配器——drive 机制
/// 不变，生产路径走 `drive_stock_stream_with_feedback`。
#[cfg(test)]
pub(super) fn drive_stock_stream<S, F, D>(
    compute: &std::sync::Arc<crate::compute::ComputeContext>,
    available: BTreeMap<StockId, S>,
    initial: Vec<ValidatedOperation>,
    notifications: StockStreamNotifications,
    detached: D,
    on_progress: F,
) -> Result<BTreeMap<StockId, S>, StepFatal>
where
    S: StockShard,
    F: FnMut(StockStreamProgress<'_, S::Round>) -> Result<FeedOutcome, StepFatal>
        + Send,
    D: FnMut(&StockId) -> Result<S, StepFatal> + Send,
{
    drive_stock_stream_with_feedback(
        compute,
        available,
        initial,
        Vec::new(),
        notifications,
        detached,
        passthrough_round_feedback,
        on_progress,
    )
}

type StockCompletion<S> = (
    StockId,
    S,
    Result<
        (
            <S as StockShard>::Round,
            Vec<super::CandidateValidationOutcome>,
        ),
        StepFatal,
    >,
);

/// 只拥有本 tick 的股票工作；不持有账户、结算或 session authority。
struct StockStreamCoordinator<S: StockShard> {
    compute: std::sync::Arc<crate::compute::ComputeContext>,
    available: BTreeMap<StockId, S>,
    /// 就绪批=（操作, 受理结果期望）成对（T4 必修 1：期望随任务派发，
    /// 匹配在 owner 完成点执行）。
    pending: BTreeMap<StockId, (Vec<ValidatedOperation>, Vec<super::CandidateValidationOutcome>)>,
    in_flight: BTreeSet<StockId>,
    /// 首批就绪操作与期望（drive 入口成对入队）。
    initial_ops: Option<Vec<ValidatedOperation>>,
    initial_expected: Option<Vec<super::CandidateValidationOutcome>>,
    sender: mpsc::Sender<StockCompletion<S>>,
    receiver: mpsc::Receiver<StockCompletion<S>>,
    notifications: StockStreamNotifications,
    /// 协调线程是否物理位于本上下文池的 worker 上（构造时在协调线程上
    /// 判定，池身份取 `ComputeContext` 自身而非 Rayon 全局注册表）。
    on_pool_worker: bool,
    /// 占槽登记守卫：Some = 入口条件准入成功（已登记占池，可派发预留）；
    /// None 且 `on_pool_worker` 为真 = 入口准入被拒（round4 ①-必修a）——
    /// 不进入池内执行路径（不登记、不做派发预留，只在自己线程内联推进
    /// 自有工作，物理上仍是池内 worker，资源域不变）；None 且
    /// `on_pool_worker` 为假 = 池外协调者（经台账条件派发预留，容量等待
    /// 由池外线程承担）。Drop 自动退出占池协调者计数。
    pool_slot: Option<crate::compute::PoolDriveSlot>,
}

impl<S: StockShard> StockStreamCoordinator<S> {
    fn new(
        compute: std::sync::Arc<crate::compute::ComputeContext>,
        available: BTreeMap<StockId, S>,
        notifications: StockStreamNotifications,
    ) -> Self {
        let on_pool_worker = compute.is_worker();
        let pool_slot = if on_pool_worker {
            compute.try_enter_pool_drive_slot()
        } else {
            None
        };
        let (sender, receiver) = mpsc::channel();
        Self {
            compute,
            available,
            pending: BTreeMap::new(),
            in_flight: BTreeSet::new(),
            initial_ops: None,
            initial_expected: None,
            sender,
            receiver,
            notifications,
            on_pool_worker,
            pool_slot,
        }
    }

    /// 是否可经台账派发预留把任务放入池队列：池外协调者恒可（预留本身
    /// 原子条件准入）；池内协调者须已登记占槽——入口被拒者绝不派发预留
    /// （未登记实体占着 worker 再预留会重新打破 drives+spawned≤workers
    /// 的持续不变式，round4 ①-必修a；该不变式只对 workers>1 声明，
    /// round5-③），其就绪工作全部由内联路径推进。
    fn spawn_admitted(&self) -> bool {
        !self.on_pool_worker || self.pool_slot.is_some()
    }

    fn enqueue_ready(
        &mut self,
        operations: Vec<ValidatedOperation>,
        expected: Vec<super::CandidateValidationOutcome>,
    ) -> Result<(), StepFatal> {
        // 配对纪律（T4 必修 1）：生产路径操作与受理期望恒 1:1（成对产出）；
        // 期望为空=无期望形态（测试直通驱动），操作照常入队。其余长度错配=
        // 结构错误显式失败，不以截断静默丢操作。
        if !expected.is_empty() && expected.len() != operations.len() {
            return Err(invariant(
                "股票就绪批的操作与受理期望数量不一致",
            ));
        }
        if expected.is_empty() {
            for operation in operations {
                self.pending
                    .entry(operation_code(&operation).clone())
                    .or_default()
                    .0
                    .push(operation);
            }
            return Ok(());
        }
        for (operation, outcome) in operations.into_iter().zip(expected) {
            let entry = self
                .pending
                .entry(operation_code(&operation).clone())
                .or_default();
            entry.0.push(operation);
            entry.1.push(outcome);
        }
        Ok(())
    }

    fn dispatch_ready<'scope, D>(
        &mut self,
        scope: &crate::compute::Scope<'scope>,
        detached: &mut D,
        feedback: RoundFeedbackFn<S::Round>,
    ) -> Result<(), StepFatal>
    where
        S: 'scope,
        D: FnMut(&StockId) -> Result<S, StepFatal>,
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
                    (code.to_string(), self.pending[code].0.len())
                });
            }
            dispatchable
        };
        // 派发顺序说明（round4 ①(4) FIFO 前提修正）：Rayon `Scope::spawn`
        // 对队列任务的取出顺序无全局 FIFO 保证，本实现不以任何队列顺序
        // 论证活性。契约内活性论证（任务独立且有限 + 入口/派发两点台账
        // 条件准入，见 .tmp/spine/round5-fix/liveness-contract.md 与
        // poll_progress 注释）：任意空闲 worker 取任意排队任务即可推进，
        // 协调者内联推进自有 pending。此处取降序只是确定的取尾口径
        // （派发顺序本身无语义，扰动 harness 允许任意序），与内联取尾
        // （last_undispatched）共用同一端点，减少无谓顺序抖动。
        // 入口准入被拒的池内协调者（round4 ①-必修a）不走派发路径：
        // 不做派发预留，全部就绪工作留在 pending 由内联推进。
        if self.spawn_admitted() {
            for code in dispatchable.into_iter().rev() {
                // 先构造 shard（detached 必须可重入：预留被拒后内联路径会
                // 再调用），再在派发点原子预留——预留读到的占池计数因此晚于
                // 一切先行握手，「按旧计数预留、按新占池现实派发」的窗口闭合
                // （round3 反例 b）。
                let shard = match self.available.remove(&code) {
                    Some(shard) => shard,
                    None => detached(&code)?,
                };
                let Some(slot) = self.compute.reserve_spawn_slot() else {
                    // 派发点原子准入失败：占池协调者与在飞派发已占满全池。
                    // 任务留在 pending（shard 放回 available），由池内协调者在
                    // 自己槽上内联、或池外协调者等待容量后重新派发推进。
                    self.available.insert(code, shard);
                    break;
                };
                // 派发门（测试/验证专用钩子）：预留已成功、spawn 尚未入队——
                // 把「预留返回与 spawn 之间」的计数窗口钉成可测的确定交错
                // （round4 ①-必修a 红测试）；未安装时为空操作。
                #[cfg(any(test, feature = "verification-harness"))]
                self.compute.run_dispatch_gate();
                let (operations, expected) = self
                    .pending
                    .remove(&code)
                    .ok_or_else(|| invariant("dispatchable stock lost its operations"))?;
                let worker_sender = self.sender.clone();
                let worker_notifications = self.notifications.sender.clone();
                let worker_compute = self.compute.clone();
                // 绑定在派发线程（协调者）捕获并显式传入执行体：Rayon 的
                // in_place_scope spawn 不传播 TLS，worker 只读自身线程会拿到
                // 空绑定或残留绑定，与内联路径读协调者绑定分叉（Astra P2-②）。
                let bindings = StockTaskBindings::capture();
                self.in_flight.insert(code.clone());
                scope.spawn(move |_| {
                    execute_stock_task(
                        &worker_compute,
                        &worker_sender,
                        &worker_notifications,
                        bindings,
                        Some(slot),
                        code,
                        shard,
                        operations,
                        expected,
                        feedback,
                    );
                });
            }
        }
        Ok(())
    }

    /// 是否仍有未派发的就绪股票（在飞股票的续行操作只能等其簿返回）。
    fn has_undispatched(&self) -> bool {
        self.pending
            .keys()
            .any(|code| !self.in_flight.contains(code))
    }

    /// 尾端待派发股票（与降序派发共用同一取尾口径）。
    /// 措辞更正（round4 ①(4)/④）：旧注释曾以「两条推进链自队尾连续收敛、
    /// 队尾 (workers−占池协调者数)+1 个位置构成立即可触及窗口」的鸽笼计数
    /// 论证依赖池队列 FIFO 语义，并把窗口外残余登记为『定理』——Rayon
    /// `Scope::spawn` 无全局 FIFO 保证，该论证不成立，已删除（Astra round4
    /// 裁定 V2 不准入的原因之一）。活性保证改为显式限定的**调度活性契约**
    /// （每条流的任务自身独立、有限步完成；契约外形状不支持，见
    /// .tmp/spine/round5-fix/liveness-contract.md）：协调者内联推进自有 pending
    /// 的队尾一项，任意空闲 worker 推进任意排队任务，不依赖取出顺序。
    fn last_undispatched(&self) -> Option<StockId> {
        self.pending
            .keys()
            .rev()
            .find(|code| !self.in_flight.contains(*code))
            .cloned()
    }

    /// 非阻塞式帮助：在协调线程上直接执行一项**自有待派发**股票任务。
    /// 只运行从未进入池队列的本 tick 私有工作，绝不经 yield_work 抢占池中
    /// 任意任务（树 B 缺陷：被抢占的股票任务可能阻塞等待只有协调循环才能
    /// 派发的工作，协调者一旦进入即与自身循环等待）。
    fn run_one_pending_inline<D>(
        &mut self,
        detached: &mut D,
        feedback: RoundFeedbackFn<S::Round>,
    ) -> Result<bool, StepFatal>
    where
        D: FnMut(&StockId) -> Result<S, StepFatal>,
    {
        let Some(code) = self.last_undispatched() else {
            return Ok(false);
        };
        let (operations, expected) = self
            .pending
            .remove(&code)
            .ok_or_else(|| invariant("dispatchable stock lost its operations"))?;
        let shard = match self.available.remove(&code) {
            Some(shard) => shard,
            None => detached(&code)?,
        };
        self.in_flight.insert(code.clone());
        let sender = self.sender.clone();
        let notifications = self.notifications.sender.clone();
        let compute = self.compute.clone();
        // 与 spawn 派发同点捕获（协调线程），两路径绑定语义一致。
        let bindings = StockTaskBindings::capture();
        execute_stock_task(
            &compute,
            &sender,
            &notifications,
            bindings,
            None,
            code,
            shard,
            operations,
            expected,
            feedback,
        );
        Ok(true)
    }

    fn poll_progress<D>(
        &mut self,
        detached: &mut D,
        feedback: RoundFeedbackFn<S::Round>,
    ) -> Result<PollOutcome, StepFatal>
    where
        D: FnMut(&StockId) -> Result<S, StepFatal>,
    {
        // 活性结构（round4 契约限定版；契约全文与生产核证见
        // .tmp/spine/round5-fix/liveness-contract.md）：
        // - 契约前提（round5-fix v3 四条）：每条流提交的任务自身独立（不
        //   等待其他池任务的启动/完成信号）且有限步完成；协调者回调不做
        //   不可协作的阻塞等待——try_recv+yield_work 协作让位允许（让出的
        //   worker 可执行等待中的任务本身），阻塞 recv/Condvar/纯自旋禁止
        //   （round4-fix v2 精确化，契约第 3 条）；且**等待目标已无条件
        //   入队（或已在池上执行）、自身独立有限完成、不依赖当前协调者的
        //   后续推进**（契约第 4 条，round5-fix v3：协作等待一个靠等待者
        //   自己才入队的目标会让位空转成环，见模块头）。
        // - 单 worker 池：等待时协作帮助执行（yield_work）；此时没有其他线程
        //   可运行股票任务，不帮助必然饿死。台账不变式不适用（单 worker 池
        //   恒准入、spawned 不记账，round5-③），活性由此协作循环自足保证。
        // - 多 worker 池 + 协调者在池内 worker 上（登记或入口被拒皆同）+
        //   仍有待派发工作：在自己线程内联执行一项自有待派发任务（其完成
        //   通知随后由同一循环收取），无待派发工作时退回阻塞 recv——等待的
        //   只会是自己已派发的在飞任务（已无条件入队，契约第 4 条）。
        // - 多 worker 池 + 池外协调者 + 仍有待派发工作：绝不在自身线程执行
        //   股票工作（资源域，round3 P1new）；以真实同步原语等待容量恢复后
        //   返回 CapacityRecovered 交回 drive 循环重新派发。协调者不占槽，
        //   等待的容量释放只来自他人 finite 占槽/在飞任务的完成（不依赖
        //   等待者自身的推进，契约第 4 条对容量等待的化用，见契约文档第二
        //   节第 6 行核证），等待不会锁死池；自身在飞任务的完成释放也会
        //   唤醒它。
        // - 其余情况：阻塞 recv——契约内在飞任务有限完成并唤醒协调者。
        // 契约内活性论证（不依赖队列 FIFO；round4 ①(4)(5)；workers>1，
        // round5-③ 限定）：入口与派发
        // 两点台账条件准入使 drives+spawned≤workers 持续成立 ⇒ 占 worker
        // 的实体只有「已登记协调者、运行/排队中的已派发任务、入口被拒的
        // 内联协调者」三类；运行中任务有限完成释放 worker；入口被拒者只
        // 内联自有有限工作后退出；已登记协调者有 pending 即内联推进、无
        // pending 则只等自己在飞任务（契约第 4 条保证这些在飞任务可被任意
        // 空闲 worker 执行，不需要等待者先推进），而在飞任务数 ≤
        // workers−drives，任意空闲 worker 取任意排队任务即可执行——归纳于
        // 有限任务集合，全部完成且无循环等待。
        if self.compute.workers() == 1 {
            loop {
                match self.notifications.receiver.try_recv() {
                    Ok(done) => return Ok(PollOutcome::Ready(done)),
                    Err(mpsc::TryRecvError::Empty) => crate::compute::yield_work(),
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Err(invariant(
                            "work notification channel closed during stock processing",
                        ));
                    }
                }
            }
        }
        loop {
            match self.notifications.receiver.try_recv() {
                Ok(done) => return Ok(PollOutcome::Ready(done)),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(invariant(
                        "work notification channel closed during stock processing",
                    ));
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if self.has_undispatched() {
                if self.on_pool_worker {
                    if !self.run_one_pending_inline(detached, feedback)? {
                        break;
                    }
                } else {
                    self.compute.wait_for_spawn_capacity();
                    if self.compute.has_spawn_capacity() {
                        return Ok(PollOutcome::CapacityRecovered);
                    }
                }
                continue;
            }
            break;
        }
        self.notifications
            .receiver
            .recv()
            .map(|done| PollOutcome::Ready(done))
            .map_err(|_| invariant("work notification channel closed during stock processing"))
    }

    fn receive_completion(
        &mut self,
    ) -> Result<(StockId, S::Round, Vec<super::CandidateValidationOutcome>), StepFatal> {
        // Stock notification 在 payload 成功发送后发出；跨 worker 的两种 channel 顺序可不同。
        let (code, shard, round) = self
            .receiver
            .try_recv()
            .map_err(|_| invariant("stock notification has no completed book"))?;
        if !self.in_flight.remove(&code) || self.available.insert(code.clone(), shard).is_some() {
            return Err(invariant("stock worker returned a duplicate book"));
        }
        let (round, outcomes) = round?;
        Ok((code, round, outcomes))
    }

    /// Idle 回复的处置：Operations 入队继续；Complete=显式完成（空 Vec 不再
    /// 单独构成完成）。Sol 终审·冗余收口：原 PendingCapacity（执行期 root-wait
    /// 容量上提）臂随 root capture 期闭环删除退役；drive 循环的容量等待只保留
    /// undispatched 分支（股票派发容量，真实同步原语）。
    fn return_idle<F>(&mut self, on_progress: &mut F) -> Result<bool, StepFatal>
    where
        F: FnMut(StockStreamProgress<'_, S::Round>) -> Result<FeedOutcome, StepFatal>,
    {
        crate::verification_evidence::enter_phase(super::TickPhase::DecisionAndContinuation);
        match on_progress(StockStreamProgress::Idle)? {
            FeedOutcome::Operations { operations, outcomes } => {
                self.enqueue_ready(operations, outcomes)?;
                Ok(false)
            }
            FeedOutcome::Complete => Ok(true),
        }
    }

    fn drive<F, D>(
        mut self,
        expected: Vec<super::CandidateValidationOutcome>,
        feedback: RoundFeedbackFn<S::Round>,
        mut detached: D,
        mut on_progress: F,
    ) -> Result<BTreeMap<StockId, S>, StepFatal>
    where
        F: FnMut(StockStreamProgress<'_, S::Round>) -> Result<FeedOutcome, StepFatal>
            + Send,
        D: FnMut(&StockId) -> Result<S, StepFatal> + Send,
    {
        // 首批就绪操作与期望成对入队（T4 必修 1）。
        let (initial_ops, initial_expected) = (self.initial_ops.take(), self.initial_expected.take());
        if let Some(ops) = initial_ops {
            self.enqueue_ready(ops, initial_expected.unwrap_or_default())?;
        }
        // coordinator 留在调用线程：股票任务经会话上下文进入其池（架构项 14），
        // 多个宿主的协调者不占用池内 worker，也不因 caller 位置换资源域。
        let compute = self.compute.clone();
        compute.in_place_scope(move |scope| {
            loop {
                self.dispatch_ready(scope, &mut detached, feedback)?;
                if self.in_flight.is_empty() {
                    if self.has_undispatched() {
                        // 准入为 0（台账满：登记协调者与在飞派发占满全池）时
                        // 全部工作无法派发；此刻仍有就绪股票工作，不得向计划
                        // 来源宣称 Idle。在池内 worker 上的协调者（登记或入口
                        // 被拒皆同，round4 ①-必修a）在自己线程内联执行；池外
                        // 协调者不在自身线程跑股票工作（资源域，round3 P1new），
                        // 以真实同步原语等容量后重新派发——它不占槽，等待不会
                        // 锁死池，占槽者在各自槽完成后释放。
                        crate::verification_evidence::enter_phase(
                            super::TickPhase::StockProcessing,
                        );
                        if self.on_pool_worker || self.compute.workers() == 1 {
                            self.run_one_pending_inline(&mut detached, feedback)?;
                        } else {
                            self.compute.wait_for_spawn_capacity();
                        }
                        continue;
                    }
                    if self.return_idle(&mut on_progress)? {
                        // T4 必修 1：原 ReadyStockStream 终态断言迁到 drive 出口
                        // ——期望随任务成对派发，仍有未派发就绪批=typed feedback
                        // 缺失（错误文案不变）。
                        if !self.pending.is_empty() {
                            return Err(invariant(
                                "StockProcessing 的已受理候选缺少 typed feedback",
                            ));
                        }
                        return Ok(self.available);
                    }
                    continue;
                }
                crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
                match self.poll_progress(&mut detached, feedback)? {
                    PollOutcome::CapacityRecovered => continue,
                    PollOutcome::Ready(TickWorkReady::Stock) => {}
                }
                let (code, mut round, outcomes) = self.receive_completion()?;
                crate::verification_evidence::enter_phase(super::TickPhase::DecisionAndContinuation);
                match on_progress(StockStreamProgress::StockCompleted {
                    code: &code,
                    round: &mut round,
                    outcomes: &outcomes,
                })? {
                    FeedOutcome::Operations { operations, outcomes } => {
                        self.enqueue_ready(operations, outcomes)?
                    }
                    FeedOutcome::Complete => {
                        return Err(invariant(
                            "非阻塞进度路径不应产生 Complete",
                        ));
                    }
                }
            }
        })
    }
}

/// 派发线程（协调者）捕获的任务绑定（计时 + 扰动）；跨线程显式传递给
/// 执行体，worker 不读自身线程 TLS——Rayon `in_place_scope` 的 spawn 不
/// 自动传播 TLS，两路径必须共用同一捕获点才能语义一致（参照
/// `compute.rs::spawn_two_stage` 的派发线程捕获模式）。
struct StockTaskBindings {
    timing: crate::verification_evidence::PhaseTimingBinding,
    #[cfg(any(test, feature = "verification-harness"))]
    executor: crate::session::pipeline::ExecutorBinding,
}

impl StockTaskBindings {
    /// 在派发线程捕获当前绑定；spawn 与内联帮助共用此捕获点。
    fn capture() -> Self {
        Self {
            timing: crate::verification_evidence::current_phase_timing_binding(),
            #[cfg(any(test, feature = "verification-harness"))]
            executor: crate::session::pipeline::current_executor_binding(),
        }
    }
}

/// 单项股票任务的执行体（worker spawn 与协调者内联帮助共用）。
/// 绑定由派发线程捕获并显式传入；两段式：股票计算在绑定内完成，
/// payload/通知发送移到绑定释放之后——协调者可见的完成信号严格晚于
/// 最后一个绑定克隆的析构。spawn 路径携带派发预留守卫：完成信号可见
/// 之后才归还派发槽（协调者的 try_recv 先于容量唤醒看到完成）；内联
/// 路径无预留（协调者自己的槽）。
fn execute_stock_task<S: StockShard>(
    compute: &std::sync::Arc<crate::compute::ComputeContext>,
    sender: &mpsc::Sender<StockCompletion<S>>,
    notifications: &mpsc::Sender<TickWorkReady>,
    bindings: StockTaskBindings,
    spawn_slot: Option<crate::compute::SpawnSlotGuard>,
    code: StockId,
    shard: S,
    operations: Vec<ValidatedOperation>,
    expected: Vec<super::CandidateValidationOutcome>,
    feedback: RoundFeedbackFn<S::Round>,
) {
    let StockTaskBindings {
        timing,
        #[cfg(any(test, feature = "verification-harness"))]
        executor,
    } = bindings;
    let bound = move || {
        compute.record(crate::compute::ComputeWork::StockPrepare);
        compute.record(crate::compute::ComputeWork::StockMatch);
        let mut shard = shard;
        let round = shard.apply(operations).and_then(|mut round| {
            // T4（Astra 必修 1）：typed feedback 匹配在本股任务完成点执行
            // （原 tick 协调者侧 take_completed+validate_execution_round 迁移）。
            feedback(&code, &mut round, expected).map(|outcomes| (round, outcomes))
        });
        (code, shard, round)
    };
    let payload = crate::verification_evidence::with_phase_timing_binding(timing, move || {
        #[cfg(any(test, feature = "verification-harness"))]
        {
            crate::session::pipeline::with_executor_binding(executor, bound)
        }
        #[cfg(not(any(test, feature = "verification-harness")))]
        {
            bound()
        }
    });
    // receiver 关闭说明整个私有 tick 已放弃；只能丢弃此项工作。
    if sender.send(payload).is_ok() {
        let _ = notifications.send(TickWorkReady::Stock);
    }
    drop(spawn_slot);
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
        code: StockId,
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
            if self.code.0 == 0 && self.steps == 1 {
                let deadline = Instant::now() + Duration::from_secs(2);
                while !self.signals.b_running.load(Ordering::SeqCst) && Instant::now() < deadline {
                    std::thread::yield_now();
                }
            } else if self.code.0 == 0 {
                // 先在 B 仍在运行时捕获重叠事实，再发布 A followup 已启动：
                // B 只会在观察到 a_followup_started 之后清 b_running；若先发布再读，
                // B 可能在两次操作之间清掉标志，把真实重叠误记为未重叠（测量窗口竞争）。
                let b_still_running = self.signals.b_running.load(Ordering::SeqCst);
                self.signals
                    .a_followup_started
                    .store(true, Ordering::SeqCst);
                self.signals
                    .overlapped
                    .store(b_still_running, Ordering::SeqCst);
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

    fn cancel(code: StockId, id: u64) -> ValidatedOperation {
        ValidatedOperation::Cancel {
            candidate_key: super::super::IntentCandidateKey::player(id),
            sealed_index: id,
            account: AccountId(id),
            code,
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
            crate::compute::ComputeContext::with_workers(1).unwrap(),
            BTreeMap::new(),
            StockStreamNotifications::new(),
        );
        coordinator
            .notifications
            .sender
            .send(TickWorkReady::Stock)
            .unwrap();
        let mut detached = |_: &StockId| -> Result<EmptyShard, StepFatal> {
            Err(invariant("fixture must not dispatch a detached stock"))
        };
        assert!(matches!(
            coordinator.poll_progress(&mut detached, passthrough_round_feedback).unwrap(),
            PollOutcome::Ready(TickWorkReady::Stock)
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
        let mut coordinator = StockStreamCoordinator::<EmptyShard>::new(
            crate::compute::ComputeContext::with_workers(1).unwrap(),
            BTreeMap::new(),
            StockStreamNotifications { sender, receiver },
        );
        let mut detached = |_: &StockId| -> Result<EmptyShard, StepFatal> {
            Err(invariant("fixture must not dispatch a detached stock"))
        };
        assert!(coordinator
            .poll_progress(&mut detached, passthrough_round_feedback)
            .unwrap_err()
            .to_string()
            .contains("notification channel closed"));
    }

    // T5（必修 1）：stale_root_notification 用例随 PlanRoot 唤醒形状删除退役
    //（root 在 capture 期闭环，执行期不再有 root 唤醒交错）。
    #[test]
    fn duplicate_stock_completion_is_an_explicit_error() {
        let mut coordinator = StockStreamCoordinator::<EmptyShard>::new(
            crate::compute::ComputeContext::with_workers(1).unwrap(),
            BTreeMap::new(),
            StockStreamNotifications::new(),
        );
        let code = StockId(0);
        coordinator.in_flight.insert(code.clone());
        coordinator
            .sender
            .send((code.clone(), EmptyShard, Ok(((), Vec::new()))))
            .unwrap();
        coordinator.receive_completion().unwrap();
        coordinator.sender.send((code, EmptyShard, Ok(((), Vec::new())))).unwrap();
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
            crate::compute::ComputeContext::with_workers(1).unwrap(),
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
            crate::compute::ComputeContext::with_workers(1).unwrap(),
            BTreeMap::new(),
            StockStreamNotifications::new(),
        );
        let worker_sender = coordinator.sender.clone();
        drop(coordinator);
        assert!(worker_sender
            .send((StockId(0), EmptyShard, Ok(((), Vec::new()))))
            .is_err());
    }

    #[test]
    fn parallel_stock_initialization_reports_the_first_stock_error() {
        let input = |code: StockId| ContinuousStockInput {
            policy: crate::session::pipeline::phase::PhasePolicy::new(
                TradingPhase::Continuous,
                0,
                0,
            ),
            market: Market::new(code, Money::from_cents(1_000), 0.10, Money::from_cents(1))
                .unwrap(),
            envelopes: Vec::new(),
            operations: Vec::new(),
            config: GameConfig::proposed_defaults(),
        };
        let mut first = input(StockId(0));
        first.operations.push(cancel(StockId(0), 1));
        let mut second = input(StockId(1));
        let envelope = Envelope::tick_start_existing(
            EnvelopeKey {
                account: AccountId(1),
                stock: StockId(1),
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
            .push(super::super::continuous_matching::ContinuousEnvelopeSnapshot { envelope });

        for _ in 0..8 {
            // 该负例在 from_post_expiry 即失败；先验面传空不改变首错位置。
            let error =
                continuous_shards(vec![second.clone(), first.clone()], BTreeMap::new()).unwrap_err();
            assert!(
                matches!(error, StepFatal::InvariantViolation { description, location }
                if description == "增量 StockProcessing 初始化时包含了 sealed operations"
                    && location == "pipeline::incremental_continuous_stock_shadow")
            );
        }
    }

    /// Astra P1 同池饱和夹具：两支股票互相等待对方启动（真实 mpsc 等待语义）。
    /// 【契约外形状记录（round4 ①(3) 重分类）】两股任务互等启动信号超出
    /// 调度活性契约的支持范围（契约要求每条流的任务自身独立，见
    /// .tmp/spine/round5-fix/liveness-contract.md）——本测试保留为契约外证据，
    /// 其 2s 截止是形状记录的完成方式，**不再作为契约内活性判据**；契约内
    /// 活性门由 independent_finite_tasks_progress_under_saturation_and_
    /// off_pool_mix 无阀承担。
    struct SaturatedPairShard {
        started: mpsc::Sender<()>,
        peer_started: mpsc::Receiver<()>,
    }

    impl StockShard for SaturatedPairShard {
        /// 是否在截止前等到对方启动。
        type Round = bool;
        const DISPATCH_BOUNDARY: Option<ExecutorBoundary> = None;

        fn apply(&mut self, _: Vec<ValidatedOperation>) -> Result<bool, StepFatal> {
            self.started.send(()).unwrap();
            Ok(self
                .peer_started
                .recv_timeout(Duration::from_secs(2))
                .is_ok())
        }
    }

    /// Astra P1 同池饱和活性：协调者经 pool.install 进入同一池（占用一个 worker 槽），
    /// 两支互相等待的股票任务超出剩余槽位。纯阻塞 recv 会形成循环等待：
    /// 在飞任务等未派发工作启动、未派发工作等空 worker 槽、最后一个槽被
    /// 阻塞在 recv 上的协调者占据。
    /// 【契约外形状记录（round4 ①(3)）】本断言的对象是任务互等形状的
    /// 完成事实（互等双方都等到对方启动），不是契约内无阀活性；契约外
    /// 形状以 2s 截止保险阀收尾并在此登记，不作活性判据。
    #[test]
    fn same_pool_saturation_with_coordinator_on_pool_still_completes_all_stocks() {
        let (a_started, wait_a) = mpsc::channel();
        let (b_started, wait_b) = mpsc::channel();
        let shards = BTreeMap::from([
            (
                StockId(0),
                SaturatedPairShard {
                    started: a_started,
                    peer_started: wait_b,
                },
            ),
            (
                StockId(1),
                SaturatedPairShard {
                    started: b_started,
                    peer_started: wait_a,
                },
            ),
        ]);
        let pool = std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let shared = crate::compute::ComputeContext::from_pool(pool.clone());
        let mut rounds = BTreeMap::new();
        let completed = pool
            .install(|| {
                drive_stock_stream(
                    &shared,
                    shards,
                    vec![cancel(StockId(0), 1), cancel(StockId(1), 2)],
                    StockStreamNotifications::new(),
                    |_| Err(invariant("fixture must not create a detached stock")),
                    |progress| {
                        // 契约 v5：return_idle 完成=显式 Complete；空 Vec 只表示
                        // 本轮无就绪操作（非完成）。
                        let idle = matches!(progress, StockStreamProgress::Idle);
                        if let StockStreamProgress::StockCompleted { code, round, .. } = progress {
                            rounds.insert(code.clone(), *round);
                        }
                        Ok(if idle {
                            FeedOutcome::Complete
                        } else {
                            FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() }
                        })
                    },
                )
            })
            .unwrap();
        assert_eq!(completed.len(), 2, "两只股票都必须返回其私有簿");
        assert_eq!(
            rounds,
            BTreeMap::from([(StockId(0), true), (StockId(1), true)]),
            "同池饱和下互相等待的两支股票必须在不触发截止保险阀的情况下完成"
        );
    }

    /// 【契约外形状记录（round4 ①(3) 登记项，先于 round2 存在）】本测试
    /// 断言的对象是**增量续行的重叠语义**（已完成股票的后续操作在另一股
    /// 仍在跑时启动），非活性；其夹具以跨股信号（stock0 等待 stock1 运行
    /// 标志）钉住重叠交错的确定性——任务间信号属契约外形状，2s 截止为
    /// 交错钉住的保险，不作活性判据。生产语义本身（续行与在跑股重叠）
    /// 不要求任务互等。
    #[test]
    fn completed_stock_starts_its_followup_while_another_stock_is_running() {
        let signals = Arc::new(Signals {
            b_running: AtomicBool::new(false),
            a_followup_started: AtomicBool::new(false),
            overlapped: AtomicBool::new(false),
        });
        let shards = [StockId(0), StockId(1)]
            .into_iter()
            .map(|code| {
                (
                    code,
                    MockShard {
                        code,
                        steps: 0,
                        signals: Arc::clone(&signals),
                    },
                )
            })
            .collect();
        let pool = std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(3)
                .build()
                .unwrap(),
        );
        let shared = crate::compute::ComputeContext::from_pool(pool.clone());
        let completed = pool
            .install(|| {
                drive_stock_stream(
                    &shared,
                    shards,
                    vec![cancel(StockId(0), 1), cancel(StockId(1), 2)],
                    StockStreamNotifications::new(),
                    |_| Err(invariant("mock must not request a detached stock")),
                    |progress| match progress {
                        StockStreamProgress::StockCompleted { code, round, .. }
                            if code.0 == 0 && round.order == OrderId(1) =>
                        {
                            Ok(FeedOutcome::Operations { operations: vec![cancel(StockId(0), 3)], outcomes: Vec::new() })
                        }
                        StockStreamProgress::Idle => Ok(FeedOutcome::Complete),
                        _ => Ok(FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() }),
                    },
                )
            })
            .unwrap();
        assert!(signals.overlapped.load(Ordering::SeqCst));
        assert_eq!(completed[&StockId(0)].steps, 2);
        assert_eq!(completed[&StockId(1)].steps, 1);
    }

    #[test]
    fn one_worker_runs_a_stock_job_without_blocking_itself() {
        let signals = Arc::new(Signals {
            b_running: AtomicBool::new(true),
            a_followup_started: AtomicBool::new(false),
            overlapped: AtomicBool::new(false),
        });
        let code = StockId(0);
        let shard = MockShard {
            code: code.clone(),
            steps: 0,
            signals,
        };
        let pool = std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .unwrap(),
        );
        let shared = crate::compute::ComputeContext::from_pool(pool.clone());
        let finished = pool
            .install(|| {
                drive_stock_stream(
                    &shared,
                    BTreeMap::from([(code.clone(), shard)]),
                    vec![cancel(StockId(0), 1)],
                    StockStreamNotifications::new(),
                    |_| Err(invariant("mock must not request a detached stock")),
                    |progress| Ok(match progress {
                        StockStreamProgress::Idle => FeedOutcome::Complete,
                        _ => FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() },
                    }),
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
                    let code = StockId(0);
                    let finished = drive_stock_stream(
                        &crate::compute::ComputeContext::with_workers(workers).unwrap(),
                        BTreeMap::new(),
                        vec![cancel(StockId(0), 1)],
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
                        |progress| Ok(match progress {
                        StockStreamProgress::Idle => FeedOutcome::Complete,
                        _ => FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() },
                    }),
                    )
                    .unwrap();
                    assert_eq!(finished[&code].steps, 1);
                });
            }
        });
    }

    /// Astra 全池活性反例 (a)：同一 2-worker 池上两个协调者同时占槽。
    /// 每协调者各自的在飞上限（workers−1=1）都满足，但两个任务合计排满
    /// 队列时，两个 worker 槽都被阻塞 recv 的协调者占据——没有任何 worker
    /// 可执行任务：纯“每协调者在飞上限”推不出全池活性。夹具在 detached
    /// 回调里以**确定性屏障**（真实同步原语 `Barrier`，round3 (c) 替换原先
    /// 最多 500ms 的轮询握手）会合：两个协调者都到达派发阶段即证明各自已
    /// 占住一个 worker 槽；修复后派发点原子准入在此前提下要么拒留 pending
    /// （各自槽上内联完成）、要么留有空 worker，两种交错都有限时间完成。
    /// 5 秒外部看门狗只让回归红有界并报错；绿路径毫秒级、不经过看门狗。
    /// 【契约内活性门成员（round4 ①(3)）】任务为独立有限的 FiniteShard——
    /// 本测试属于契约内活性绿门（看门狗不参与绿路径，非截止阀）。
    #[test]
    fn two_coordinators_on_one_saturated_pool_still_complete_their_own_stocks() {
        struct FiniteShard;
        impl StockShard for FiniteShard {
            type Round = ();
            const DISPATCH_BOUNDARY: Option<ExecutorBoundary> = None;
            fn apply(&mut self, _: Vec<ValidatedOperation>) -> Result<(), StepFatal> {
                Ok(())
            }
        }
        let pool = std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let shared = crate::compute::ComputeContext::from_pool(pool.clone());
        // 确定性屏障：双方 detached 到达即双双占槽并处于派发阶段（无轮询、无超时）。
        let rendezvous = Arc::new(std::sync::Barrier::new(2));
        let (result_tx, result_rx) = mpsc::channel();
        for lane in 0..2usize {
            let pool = Arc::clone(&pool);
            let shared = shared.clone();
            let rendezvous = Arc::clone(&rendezvous);
            let result_tx = result_tx.clone();
            // 有意不用 scoped thread：修复前的确定性死锁需要外部看门狗判红；
            // 泄漏的阻塞线程随测试进程退出一并终止，不影响其余用例。
            std::thread::spawn(move || {
                let mut handshake = Some(rendezvous);
                let outcome = pool.install(|| {
                    drive_stock_stream(
                        &shared,
                        BTreeMap::new(),
                        vec![cancel(StockId(lane as u32), lane as u64 + 1)],
                        StockStreamNotifications::new(),
                        move |_: &StockId| {
                            // 到达这里即已占住池内 worker 槽并处于派发阶段；
                            // 屏障等待对方 lane 也占住槽，锁死“任务只能排队”的前置。
                            if let Some(rendezvous) = handshake.take() {
                                rendezvous.wait();
                            }
                            Ok(FiniteShard)
                        },
                        |progress| Ok(match progress {
                        StockStreamProgress::Idle => FeedOutcome::Complete,
                        _ => FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() },
                    }),
                    )
                    .map(|books| books.len())
                });
                let _ = result_tx.send((lane, outcome));
            });
        }
        drop(result_tx);
        let mut completed_lanes = BTreeSet::new();
        while completed_lanes.len() < 2 {
            match result_rx.recv_timeout(Duration::from_secs(5)) {
                Ok((lane, Ok(count))) => {
                    assert_eq!(count, 1, "lane {lane} 必须恰好返回自己的 1 本私有簿");
                    completed_lanes.insert(lane);
                }
                Ok((lane, Err(fatal))) => panic!("lane {lane} 的 drive 失败：{fatal:?}"),
                Err(mpsc::RecvTimeoutError::Timeout) => panic!(
                    "同一 2-worker 池上两个协调者互相饿死：5 秒内仍有 lane 未完成（全池活性反例 a）"
                ),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    panic!("结果通道在两条 lane 完成前关闭")
                }
            }
        }
    }

    /// Astra 全池活性反例 (b)：单协调者、三股无环依赖 A/B 等待 C 启动而 C 在 pending。
    /// 2-worker 池、协调者占一槽、在飞上限 1：Stock0 派发占用唯一空闲 worker，
    /// Stock1/Stock2 留 pending。若内联帮助从队首取 Stock1，协调者进入 Stock1 后
    /// 无法再派发 Stock2——Stock0/Stock1 双双阻塞等 Stock2 启动。
    /// 【契约外形状记录（round4 ①(3)）】三股任务间依赖（等待依赖根启动）
    /// 超出调度活性契约的支持范围（任务必须独立）；本测试保留为契约外
    /// 证据（2s 截止保险阀收尾），不再作为契约内活性判据。
    #[test]
    fn inline_help_must_not_strand_the_pending_dependency_root() {
        /// 三股夹具的两种角色：等待依赖根启动 / 依赖根本身（发布两个等待方的启动）。
        enum DependencyShard {
            Waiting {
                started: mpsc::Sender<()>,
                root_started: mpsc::Receiver<()>,
            },
            Root {
                started: [mpsc::Sender<()>; 2],
            },
        }
        impl StockShard for DependencyShard {
            /// 是否在截止前等到依赖根启动（根自身恒为 true）。
            type Round = bool;
            const DISPATCH_BOUNDARY: Option<ExecutorBoundary> = None;
            fn apply(&mut self, _: Vec<ValidatedOperation>) -> Result<bool, StepFatal> {
                match self {
                    Self::Waiting {
                        started,
                        root_started,
                    } => {
                        let _ = started.send(());
                        Ok(root_started.recv_timeout(Duration::from_secs(2)).is_ok())
                    }
                    Self::Root { started } => {
                        let _ = started[0].send(());
                        let _ = started[1].send(());
                        Ok(true)
                    }
                }
            }
        }
        let (root_to_a, wait_root_a) = mpsc::channel();
        let (root_to_b, wait_root_b) = mpsc::channel();
        let (a_started, _wait_a) = mpsc::channel();
        let (b_started, _wait_b) = mpsc::channel();
        let shards = BTreeMap::from([
            (
                StockId(0),
                DependencyShard::Waiting {
                    started: a_started,
                    root_started: wait_root_a,
                },
            ),
            (
                StockId(1),
                DependencyShard::Waiting {
                    started: b_started,
                    root_started: wait_root_b,
                },
            ),
            (
                StockId(2),
                DependencyShard::Root {
                    started: [root_to_a, root_to_b],
                },
            ),
        ]);
        let pool = std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let shared = crate::compute::ComputeContext::from_pool(pool.clone());
        let mut rounds = BTreeMap::new();
        let completed = pool
            .install(|| {
                drive_stock_stream(
                    &shared,
                    shards,
                    vec![
                        cancel(StockId(0), 1),
                        cancel(StockId(1), 2),
                        cancel(StockId(2), 3),
                    ],
                    StockStreamNotifications::new(),
                    |_| Err(invariant("fixture must not create a detached stock")),
                    |progress| {
                        // 契约 v5：return_idle 完成=显式 Complete；空 Vec 只表示
                        // 本轮无就绪操作（非完成）。
                        let idle = matches!(progress, StockStreamProgress::Idle);
                        if let StockStreamProgress::StockCompleted { code, round, .. } = progress {
                            rounds.insert(code.clone(), *round);
                        }
                        Ok(if idle {
                            FeedOutcome::Complete
                        } else {
                            FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() }
                        })
                    },
                )
            })
            .unwrap();
        assert_eq!(completed.len(), 3, "三只股票都必须返回其私有簿");
        assert_eq!(
            rounds,
            BTreeMap::from([(StockId(0), true), (StockId(1), true), (StockId(2), true)]),
            "内联帮助不得把协调者锁进依赖方而搁置 pending 的依赖根：任何一方都不允许靠截止保险阀脱困"
        );
    }

    /// Astra round3 反例 (a)：中位依赖根。三股 [dep0, ROOT1, dep2]，2-worker 池、
    /// 协调者占一槽（派发准入 1）。升序派发先派队首 dep0 占住唯一空闲 worker，
    /// 队尾内联再取 dep2 把协调者锁进依赖方——两条推进链（池从已派发队列、
    /// 内联从剩余 pending 队尾）都被依赖方占住，中位的依赖根 Stock1 永不启动。
    /// 【契约外形状记录（round4 ①(3)）】三股任务间依赖（中位依赖根）超出
    /// 调度活性契约的支持范围（任务必须独立）；本测试保留为契约外证据
    /// （2s 截止保险阀收尾），不再作为契约内活性判据。取尾口径的确定
    /// 顺序（降序派发+队尾内联）本身仍被契约内测试覆盖。
    #[test]
    fn mid_queue_dependency_root_is_not_sealed_off_by_both_eating_chains() {
        /// 三股夹具的两种角色：等待依赖根启动 / 依赖根本身（发布两个等待方的启动）。
        enum MidDependencyShard {
            Waiting { root_started: mpsc::Receiver<()> },
            Root { started: [mpsc::Sender<()>; 2] },
        }
        impl StockShard for MidDependencyShard {
            type Round = bool;
            const DISPATCH_BOUNDARY: Option<ExecutorBoundary> = None;
            fn apply(&mut self, _: Vec<ValidatedOperation>) -> Result<bool, StepFatal> {
                match self {
                    Self::Waiting { root_started } => {
                        Ok(root_started.recv_timeout(Duration::from_secs(2)).is_ok())
                    }
                    Self::Root { started } => {
                        let _ = started[0].send(());
                        let _ = started[1].send(());
                        Ok(true)
                    }
                }
            }
        }
        let (root_to_a, wait_root_a) = mpsc::channel();
        let (root_to_b, wait_root_b) = mpsc::channel();
        // 中位依赖根：BTreeMap 序 [0: dep_a, 1: ROOT, 2: dep_b]。
        let shards = BTreeMap::from([
            (
                StockId(0),
                MidDependencyShard::Waiting {
                    root_started: wait_root_a,
                },
            ),
            (
                StockId(1),
                MidDependencyShard::Root {
                    started: [root_to_a, root_to_b],
                },
            ),
            (
                StockId(2),
                MidDependencyShard::Waiting {
                    root_started: wait_root_b,
                },
            ),
        ]);
        let pool = std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let shared = crate::compute::ComputeContext::from_pool(pool.clone());
        let mut rounds = BTreeMap::new();
        let completed = pool
            .install(|| {
                drive_stock_stream(
                    &shared,
                    shards,
                    vec![
                        cancel(StockId(0), 1),
                        cancel(StockId(1), 2),
                        cancel(StockId(2), 3),
                    ],
                    StockStreamNotifications::new(),
                    |_| Err(invariant("fixture must not create a detached stock")),
                    |progress| {
                        // 契约 v5：return_idle 完成=显式 Complete；空 Vec 只表示
                        // 本轮无就绪操作（非完成）。
                        let idle = matches!(progress, StockStreamProgress::Idle);
                        if let StockStreamProgress::StockCompleted { code, round, .. } = progress {
                            rounds.insert(code.clone(), *round);
                        }
                        Ok(if idle {
                            FeedOutcome::Complete
                        } else {
                            FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() }
                        })
                    },
                )
            })
            .unwrap();
        assert_eq!(completed.len(), 3, "三只股票都必须返回其私有簿");
        assert_eq!(
            rounds,
            BTreeMap::from([(StockId(0), true), (StockId(1), true), (StockId(2), true)]),
            "中位依赖根必须立即可触及：任何一方都不允许靠截止保险阀脱困（round3 反例 a）"
        );
    }

    /// Astra round3 反例 (b)：计数变化窗口交错。池内协调者 X 在派发线程按旧计数
    /// （只含自己）算得准入 1，其 detached 回调用通道握手让 Y 先占住另一槽，
    /// 再放行 X spawn——按旧计数派发的任务排进已被占满的池队列，X 阻塞 recv
    /// 永远等不到它执行（主线程 5s 外部看门狗有界判红）。修复后派发点原子
    /// 预留（reserve-then-spawn）：X 的预留被拒、任务留在 pending，由 X 在
    /// 自己的槽上内联完成。Y 的线程永久占槽（泄漏阻塞线程随测试进程终止）。
    /// 【契约内活性门成员（round4 ①(3)）】任务为独立有限的 FiniteShard——
    /// 本测试属于契约内活性绿门（5s 外部看门狗不参与绿路径）。
    #[test]
    fn spawn_admission_checks_pool_occupancy_at_the_spawn_point() {
        struct FiniteShard;
        impl StockShard for FiniteShard {
            type Round = ();
            const DISPATCH_BOUNDARY: Option<ExecutorBoundary> = None;
            fn apply(&mut self, _: Vec<ValidatedOperation>) -> Result<(), StepFatal> {
                Ok(())
            }
        }
        let pool = std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let shared = crate::compute::ComputeContext::from_pool(pool.clone());
        // 通道握手：X 的 detached（仅首次）先放行 Y 入池占槽，等 Y 登记完成后
        // 才继续——把「按旧计数决策、按新占池现实派发」的窗口钉成确定顺序。
        let (release_y, y_go) = mpsc::channel::<()>();
        let (y_parked_tx, y_parked_rx) = mpsc::channel::<()>();
        let (y_release_tx, y_release_rx) = mpsc::channel::<()>();
        let mut y_handshake = Some((release_y, y_parked_rx));
        let (result_tx, result_rx) = mpsc::channel();
        let y_pool = Arc::clone(&pool);
        let y_shared = shared.clone();
        let lane = std::thread::spawn(move || {
            let outcome = pool.install(|| {
                drive_stock_stream(
                    &shared,
                    BTreeMap::new(),
                    vec![cancel(StockId(0), 1)],
                    StockStreamNotifications::new(),
                    move |_: &StockId| {
                        if let Some((release_y, y_parked_rx)) = y_handshake.take() {
                            release_y.send(()).unwrap();
                            y_parked_rx.recv().unwrap();
                        }
                        Ok(FiniteShard)
                    },
                    |progress| Ok(match progress {
                        StockStreamProgress::Idle => FeedOutcome::Complete,
                        _ => FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() },
                    }),
                )
                .map(|books| books.len())
            });
            let _ = result_tx.send(outcome);
        });
        // Y：等 X 进入派发阶段后入池占槽并永久驻留（模拟另一协调者的阻塞等待）。
        std::thread::spawn(move || {
            y_go.recv().unwrap();
            y_pool.install(move || {
                let _slot = y_shared
                    .try_enter_pool_drive_slot()
                    .expect("Y 在 X 驻留 detached（spawned=0、drives=1）时入口准入必须成功");
                let _ = y_parked_tx.send(());
                // 永久驻留：接收端随测试进程结束丢弃。
                let _ = y_release_rx.recv();
            });
        });
        let count = result_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("派发点原子准入必须让 X 在他人占满池时仍有限时间完成（round3 反例 b）")
            .expect("X 的 drive 不得失败");
        assert_eq!(count, 1, "X 必须恰好返回自己的 1 本私有簿");
        let _ = lane.join();
        let _ = y_release_tx.send(());
    }

    /// Astra round4 ①-必修a：入口计数窗口（确定性红→绿）。旧实现
    /// `enter_pool_drive_slot` 无条件增 drives：X 在派发点预留成功
    /// （drives=1、spawned=1，台账=W 满）后、spawn 入队前存在窗口——
    /// Y 的 install 闭包落在此刻空闲的 worker 上并无条件登记（drives=2），
    /// 联合不变式 drives+spawned≤workers 被打破（2+1>2）。本测试以派发门
    /// （reserve 与 spawn 之间的真实通道钩子）把 X 钉在该窗口内，Y 入池
    /// 驻留（内联执行自有股票、由测试持有驻留），断言台账持续满足
    /// drives+spawned≤workers。旧实现红（3>2，放行后判失败）；修复后
    /// Y 的入口条件准入被拒（drives 保持 1 → 1+1=2≤2）断言绿。
    #[test]
    fn pool_entry_admission_keeps_the_ledger_invariant_during_the_spawn_window() {
        struct FiniteShard;
        impl StockShard for FiniteShard {
            type Round = ();
            const DISPATCH_BOUNDARY: Option<ExecutorBoundary> = None;
            fn apply(&mut self, _: Vec<ValidatedOperation>) -> Result<(), StepFatal> {
                Ok(())
            }
        }
        /// 驻留 shard：报告已在 worker 上开跑，随后由测试持有（测试通道，
        /// 非池任务互等——驻留机制只服务台账断言的定序，不参与活性断言）。
        struct HeldShard {
            running: mpsc::Sender<()>,
            hold: mpsc::Receiver<()>,
        }
        impl StockShard for HeldShard {
            type Round = ();
            const DISPATCH_BOUNDARY: Option<ExecutorBoundary> = None;
            fn apply(&mut self, _: Vec<ValidatedOperation>) -> Result<(), StepFatal> {
                self.running.send(()).unwrap();
                self.hold.recv().unwrap();
                Ok(())
            }
        }
        let pool = std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let shared = crate::compute::ComputeContext::from_pool(pool.clone());
        // 派发门：X 首次成功预留后、spawn 入队前钉住——通知测试并等待放行。
        let (x_parked_tx, x_parked_rx) = mpsc::channel::<()>();
        let (x_release_tx, x_release_rx) = mpsc::channel::<()>();
        let parked = Arc::new(std::sync::Mutex::new(Some((x_parked_tx, x_release_rx))));
        shared.set_dispatch_gate(std::sync::Arc::new(move || {
            if let Some((x_parked_tx, x_release_rx)) = parked.lock().unwrap().take() {
                x_parked_tx.send(()).unwrap();
                x_release_rx.recv().unwrap();
            }
        }));
        // X：池内协调者，1 只股票；派发在预留成功后于门上暂停（S 尚未入队，
        // 另一 worker 此刻空闲——这正是入口计数窗口）。
        let x_pool = Arc::clone(&pool);
        let x_shared = shared.clone();
        let x = std::thread::spawn(move || {
            x_pool.install(|| {
                drive_stock_stream(
                    &x_shared,
                    BTreeMap::new(),
                    vec![cancel(StockId(0), 1)],
                    StockStreamNotifications::new(),
                    |_: &StockId| Ok(FiniteShard),
                    |progress| Ok(match progress {
                        StockStreamProgress::Idle => FeedOutcome::Complete,
                        _ => FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() },
                    }),
                )
            })
            .map(|books| books.len())
        });
        x_parked_rx
            .recv()
            .expect("X 必须到达派发门（预留成功、spawn 未入队）");
        // Y：另一池内协调者，install 闭包落在空闲 worker 上；其股票在内联
        // 路径驻留并被测试持有（Y 的入口登记决策在此之前已完成）。
        let (y_running_tx, y_running_rx) = mpsc::channel::<()>();
        let (y_release_tx, y_release_rx) = mpsc::channel::<()>();
        let y_hold = Arc::new(std::sync::Mutex::new(Some(y_release_rx)));
        let y_pool = Arc::clone(&pool);
        let y_shared = shared.clone();
        let y = std::thread::spawn(move || {
            y_pool.install(|| {
                drive_stock_stream(
                    &y_shared,
                    BTreeMap::new(),
                    vec![cancel(StockId(1), 2)],
                    StockStreamNotifications::new(),
                    move |_: &StockId| {
                        let hold = y_hold
                            .lock()
                            .unwrap()
                            .take()
                            .expect("Y 的 detached 只会被调用一次（单股票）");
                        Ok(HeldShard {
                            running: y_running_tx.clone(),
                            hold,
                        })
                    },
                    |progress| Ok(match progress {
                        StockStreamProgress::Idle => FeedOutcome::Complete,
                        _ => FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() },
                    }),
                )
            })
            .map(|books| books.len())
        });
        y_running_rx
            .recv()
            .expect("Y 必须已在其 worker 上内联驻留（入口决策已完成）");
        // 台账断言：任何交错下联合不变式必须持续成立（round4 ①-必修a；
        // 本测试为 2-worker 池，不变式只对 workers>1 声明，round5-③）。
        let (drives, spawned) = shared.admission_snapshot();
        let workers = shared.workers();
        if drives + spawned > workers {
            // 先放行两侧再判红：阻塞驻留线程随断言失败泄漏的旧模式改为
            // 干净收尾（X/Y 在放行后各自有限完成）。
            x_release_tx.send(()).unwrap();
            y_release_tx.send(()).unwrap();
            let _ = x.join();
            let _ = y.join();
            panic!(
                "入口计数窗口打破联合不变式：drives={drives} + spawned={spawned} > workers={workers}（round4 ①-必修a）"
            );
        }
        x_release_tx.send(()).unwrap();
        y_release_tx.send(()).unwrap();
        assert_eq!(
            x.join().expect("X 线程不得 panic").expect("X drive 不得失败"),
            1,
            "X 必须恰好返回自己的 1 本私有簿"
        );
        assert_eq!(
            y.join().expect("Y 线程不得 panic").expect("Y drive 不得失败"),
            1,
            "Y 必须恰好返回自己的 1 本私有簿"
        );
    }

    /// 契约内活性门（round4 ①(3) 绿门成员）：独立有限任务（不等待任何其他
    /// 池任务信号、零阻塞原语）在「多协调者共享饱和池 + 池外协调者」混合下
    /// 必须真实推进——无截止阀、无超时放行、无看门狗参与绿路径；活性由
    /// 调度活性契约保证（任务独立有限 + 入口/派发两点的台账条件准入，见
    /// .tmp/spine/round5-fix/liveness-contract.md）。回归为死锁时本测试挂起，
    /// 由命令级超时判红。契约外形状（任务互等/跨任务依赖）不在此门内，
    /// 见各契约外记录测试。
    #[test]
    fn independent_finite_tasks_progress_under_saturation_and_off_pool_mix() {
        struct CountingShard {
            executions: Arc<std::sync::atomic::AtomicUsize>,
        }
        impl StockShard for CountingShard {
            type Round = usize;
            const DISPATCH_BOUNDARY: Option<ExecutorBoundary> = None;
            fn apply(&mut self, _: Vec<ValidatedOperation>) -> Result<usize, StepFatal> {
                self.executions.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(1)
            }
        }
        const PER_COORDINATOR: u32 = 3;
        let pool = std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let shared = crate::compute::ComputeContext::from_pool(pool.clone());
        let executions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let total_books = std::thread::scope(|scope| {
            // 两个池内协调者（install 落到池 worker）+ 一个池外协调者（tick
            // 线程形态）：9 只股票（3 协调者 × PER_COORDINATOR=3，round5-④
            // 更正旧注释的「6 只」计数）> 2 worker，必然经历饱和、入口拒绝
            // 与容量等待的任意交错；任务全部独立有限 → 全部完成。
            let handles = (0..3usize)
                .map(|lane| {
                    let pool = Arc::clone(&pool);
                    let shared = shared.clone();
                    let executions = Arc::clone(&executions);
                    scope.spawn(move || {
                    let shards = (0..PER_COORDINATOR)
                        .map(|offset| {
                            let code = StockId(lane as u32 * PER_COORDINATOR + offset);
                            (
                                code,
                                CountingShard {
                                    executions: Arc::clone(&executions),
                                },
                            )
                        })
                        .collect::<BTreeMap<_, _>>();
                    let operations = (0..PER_COORDINATOR)
                        .map(|offset| {
                            cancel(
                                StockId(lane as u32 * PER_COORDINATOR + offset),
                                (lane as u64 * u64::from(PER_COORDINATOR)) + u64::from(offset) + 1,
                            )
                        })
                        .collect::<Vec<_>>();
                    let lane_shards = shards;
                    let drive = move || {
                        drive_stock_stream(
                            &shared,
                            lane_shards,
                            operations,
                            StockStreamNotifications::new(),
                            |_: &StockId| -> Result<CountingShard, StepFatal> {
                                Err(invariant("饱和夹具不得请求 detached 股票"))
                            },
                            |progress| Ok(match progress {
                        StockStreamProgress::Idle => FeedOutcome::Complete,
                        _ => FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() },
                    }),
                        )
                        .map(|books| books.len())
                    };
                    if lane < 2 {
                        pool.install(drive)
                    } else {
                        drive()
                    }
                    })
                })
                .collect::<Vec<_>>();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .expect("协调者线程不得 panic")
                        .expect("独立有限任务下 drive 不得失败")
                })
                .sum::<usize>()
        });
        assert_eq!(
            total_books,
            3 * PER_COORDINATOR as usize,
            "每个协调者必须收回全部私有簿"
        );
        assert_eq!(
            executions.load(std::sync::atomic::Ordering::SeqCst),
            3 * PER_COORDINATOR as usize,
            "每项任务必须恰好执行一次（无阀真实活性）"
        );
    }

    /// Astra round4 ②（round3 P1new 的确定性重写；round5-② 探针锁内化）：
    /// 池外协调者 Z 在派发准入为 0 时不得把股票任务落到池外线程执行
    /// （owner_worker=None 的资源域回归），且必须以真实同步原语等待容量。
    /// 旧夹具以「池内任务等 z_ran 标志或 500ms 超时阀」构造互等形状并靠
    /// 阀放行——round4 裁定：任务互等属契约外形状（不得再用作活性判据），
    /// 且超时阀掩盖慢速与死锁之别。本重写全部确定性编排，无任何超时阀：
    /// - X（单个池内协调者，2 只股票）饱和占满 2-worker 池：1 只派发到
    ///   worker、1 只在自己槽上内联，两任务都「报告已开跑后由测试持有」
    ///   （测试通道 = 控制方，非池任务互等）；
    /// - Z（池外协调者）派发预留被拒后进入容量等待——容量等待探针在台账
    ///   锁内确认满账（1+1=2）之后、`wait_while` 原子入等之前发握手信号
    ///   （round5-②：旧探针在取锁前发信号，测试方无法区分 Z 是已等待还是
    ///   尚未取锁；锁内化后收到信号即断定 Z 已持锁确认满账、即将原子入等，
    ///   且测试方随后的容量释放必然排在 Z 入等之后，唤醒不丢失）；
    /// - 测试方释放 X 的在飞任务容量 → 断言 Z 被唤醒并推进（任务在池内
    ///   worker 执行），随后放行内联侧收尾。
    #[test]
    fn off_pool_coordinator_waits_for_pool_capacity_instead_of_inlining() {
        /// 驻留 shard：报告已开跑后由测试持有（控制方通道；契约内——
        /// 不等待任何其他池任务）。
        struct HeldShard {
            running: mpsc::Sender<()>,
            hold: mpsc::Receiver<()>,
        }
        impl StockShard for HeldShard {
            type Round = ();
            const DISPATCH_BOUNDARY: Option<ExecutorBoundary> = None;
            fn apply(&mut self, _: Vec<ValidatedOperation>) -> Result<(), StepFatal> {
                self.running.send(()).unwrap();
                self.hold.recv().unwrap();
                Ok(())
            }
        }
        struct FiniteShard;
        impl StockShard for FiniteShard {
            type Round = ();
            const DISPATCH_BOUNDARY: Option<ExecutorBoundary> = None;
            fn apply(&mut self, _: Vec<ValidatedOperation>) -> Result<(), StepFatal> {
                Ok(())
            }
        }
        let pool = std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let shared = crate::compute::ComputeContext::from_pool(pool.clone());
        let observations: Arc<std::sync::Mutex<Vec<Option<usize>>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let observer_observations = Arc::clone(&observations);
        shared.set_observer(std::sync::Arc::new(move |observation| {
            if matches!(
                observation.work,
                crate::compute::ComputeWork::StockPrepare | crate::compute::ComputeWork::StockMatch
            ) {
                observer_observations
                    .lock()
                    .unwrap()
                    .push(observation.owner_worker);
            }
        }));
        // X：池内协调者，2 只股票（降序派发：Stock1 入池、Stock0 准入被拒
        // 后在自己槽上内联）——两任务驻留即台账 drives=1、spawned=1 全满。
        let (s1_running_tx, s1_running_rx) = mpsc::channel::<()>();
        let (s1_release_tx, s1_release_rx) = mpsc::channel::<()>();
        let (s0_running_tx, s0_running_rx) = mpsc::channel::<()>();
        let (s0_release_tx, s0_release_rx) = mpsc::channel::<()>();
        let x_s1 = Arc::new(std::sync::Mutex::new(Some((s1_running_tx, s1_release_rx))));
        let x_s0 = Arc::new(std::sync::Mutex::new(Some((s0_running_tx, s0_release_rx))));
        let x_pool = Arc::clone(&pool);
        let x_shared = shared.clone();
        let x = std::thread::spawn(move || {
            let mut x_s1 = Arc::clone(&x_s1);
            let mut x_s0 = Arc::clone(&x_s0);
            x_pool.install(move || {
                drive_stock_stream(
                    &x_shared,
                    BTreeMap::new(),
                    vec![cancel(StockId(0), 1), cancel(StockId(1), 2)],
                    StockStreamNotifications::new(),
                    move |code: &StockId| {
                        if code.0 == 1 {
                            let (running, hold) = x_s1
                                .lock()
                                .unwrap()
                                .take()
                                .expect("Stock1 的 detached 只会被调用一次");
                            Ok(HeldShard { running, hold })
                        } else {
                            let (running, hold) = x_s0
                                .lock()
                                .unwrap()
                                .take()
                                .expect("Stock0 的 detached 只会被调用一次");
                            Ok(HeldShard { running, hold })
                        }
                    },
                    |progress| Ok(match progress {
                        StockStreamProgress::Idle => FeedOutcome::Complete,
                        _ => FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() },
                    }),
                )
            })
            .map(|books| books.len())
        });
        s0_running_rx
            .recv()
            .expect("X 的内联股票必须先报告已开跑（占协调者槽）");
        s1_running_rx
            .recv()
            .expect("X 的派发股票必须报告已开跑（占 worker 槽）");
        // Z：池外线程直接驱动（不 install）。此刻台账必满（测试持有两侧
        // 驻留，未放行任何释放），Z 的容量等待必然真实发生——探针在台账
        // 锁内确认满账后、原子入等前发握手（round5-②；探针回调只发本
        // 通道信号，不取台账锁）。
        let (z_waiting_tx, z_waiting_rx) = mpsc::channel::<()>();
        let z_signal = Arc::new(std::sync::Mutex::new(Some(z_waiting_tx)));
        shared.set_capacity_wait_probe(std::sync::Arc::new(move || {
            if let Some(z_waiting_tx) = z_signal.lock().unwrap().take() {
                z_waiting_tx.send(()).unwrap();
            }
        }));
        let z_shared = shared.clone();
        let z = std::thread::spawn(move || {
            drive_stock_stream(
                &z_shared,
                BTreeMap::new(),
                vec![cancel(StockId(2), 3)],
                StockStreamNotifications::new(),
                |_: &StockId| Ok(FiniteShard),
                |progress| Ok(match progress {
                        StockStreamProgress::Idle => FeedOutcome::Complete,
                        _ => FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() },
                    }),
            )
            .map(|books| books.len())
        });
        z_waiting_rx
            .recv()
            .expect("Z 必须以真实同步原语进入容量等待（锁内确认满账后的握手信号，round5-②）");
        // 测试方释放一方容量（在飞的 Stock1 完成 → spawned 归还并广播）：
        // Z 必须被唤醒、重新派发并推进。
        s1_release_tx.send(()).unwrap();
        // 内联侧随后放行收尾；Z 的推进由 join + 观察断言检验。
        s0_release_tx.send(()).unwrap();
        assert_eq!(
            x.join().expect("池内协调者线程不得 panic").expect("X drive 不得失败"),
            2,
            "池内协调者必须恰好返回自己的 2 本私有簿"
        );
        assert_eq!(
            z.join().expect("池外协调者线程不得 panic").expect("Z drive 不得失败"),
            1,
            "池外协调者必须恰好返回自己的 1 本私有簿"
        );
        let observations = observations.lock().unwrap();
        assert!(
            observations.len() >= 3,
            "三只股票任务的计算观察都必须被记录：{:?}",
            observations
        );
        assert!(
            observations.iter().all(|owner| owner.is_some()),
            "池外协调者的股票任务必须在池内 worker 执行（资源域不得回归到 owner=None）：{:?}",
            observations
        );
    }

    /// 绑定传播探针：记录执行线程身份、两条绑定（计时/扰动）是否与协调者
    /// 捕获一致，以及嵌套隔离（内层临时绑定可覆盖、退出后恢复捕获绑定）。
    struct BindingProbeShard {
        expected_timing: crate::verification_evidence::PhaseTimingBinding,
        expected_executor: executor_perturbation::ExecutorBinding,
        coordinator: std::thread::ThreadId,
        observed: Arc<std::sync::Mutex<Vec<BindingObservation>>>,
    }

    struct BindingObservation {
        on_coordinator_thread: bool,
        timing_same: bool,
        executor_same: bool,
        nested_inner_empty: bool,
        timing_restored_after_nested: bool,
    }

    impl StockShard for BindingProbeShard {
        type Round = ();
        const DISPATCH_BOUNDARY: Option<ExecutorBoundary> = None;
        fn apply(&mut self, _: Vec<ValidatedOperation>) -> Result<(), StepFatal> {
            use crate::verification_evidence::{
                current_phase_timing_binding, with_phase_timing_binding, PhaseTimingBinding,
            };
            let timing = current_phase_timing_binding();
            let executor = executor_perturbation::current_executor_binding();
            let nested_inner_empty = with_phase_timing_binding(PhaseTimingBinding::empty(), || {
                current_phase_timing_binding().is_empty()
            });
            let timing_restored_after_nested =
                current_phase_timing_binding().same_as(&self.expected_timing);
            let on_coordinator_thread = std::thread::current().id() == self.coordinator;
            self.observed.lock().unwrap().push(BindingObservation {
                on_coordinator_thread,
                timing_same: timing.same_as(&self.expected_timing),
                executor_same: executor.same_as(&self.expected_executor),
                nested_inner_empty,
                timing_restored_after_nested,
            });
            Ok(())
        }
    }

    /// P2-①：spawn 路径的绑定必须在派发线程捕获并显式传播到 worker。
    /// Rayon `in_place_scope` 的 spawn 不自动传播 TLS——worker 只读自身 TLS
    /// 时拿到空绑定，与内联路径读协调者绑定的语义分叉。协调者在池外、
    /// 两支股票全部 spawn：断言两条绑定跨线程一致、执行线程不是协调线程、
    /// 嵌套隔离成立。
    #[test]
    fn spawned_stock_tasks_inherit_the_bindings_captured_on_the_dispatch_thread() {
        let timing_probe = crate::verification_evidence::PhaseTimingBinding::test_collector();
        let pool = std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let shared = crate::compute::ComputeContext::from_pool(pool.clone());
        let observed: Arc<std::sync::Mutex<Vec<BindingObservation>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed_for_detached = Arc::clone(&observed);
        let ((), records) = crate::session::pipeline::with_executor_perturbation(
            executor_perturbation::ExecutorPerturbation::default(),
            || {
                crate::verification_evidence::with_phase_timing_binding(timing_probe, || {
                    // 测试观察账本与生产同构：进入 tick 前绑定真实计算上下文，
                    // 否则 enter_phase 记录会在 context_workers 缺失处显式 panic。
                    crate::verification_evidence::begin_authoritative_tick(0, &shared).unwrap();
                    let expected_timing =
                        crate::verification_evidence::current_phase_timing_binding();
                    let expected_executor = executor_perturbation::current_executor_binding();
                    let coordinator = std::thread::current().id();
                    let detached = move |code: &StockId| {
                        Ok(BindingProbeShard {
                            expected_timing: expected_timing.clone(),
                            expected_executor: expected_executor.clone(),
                            coordinator,
                            observed: Arc::clone(&observed_for_detached),
                        })
                    };
                    let completed = drive_stock_stream(
                        &shared,
                        BTreeMap::new(),
                        vec![cancel(StockId(0), 1), cancel(StockId(1), 2)],
                        StockStreamNotifications::new(),
                        detached,
                        |progress| Ok(match progress {
                        StockStreamProgress::Idle => FeedOutcome::Complete,
                        _ => FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() },
                    }),
                    )
                    .unwrap();
                    assert_eq!(completed.len(), 2);
                })
            },
        )
        .unwrap();
        let _ = records;
        let observations = observed.lock().unwrap();
        assert_eq!(observations.len(), 2, "两支股票的探针都必须执行");
        for observation in observations.iter() {
            assert!(
                !observation.on_coordinator_thread,
                "spawn 路径必须在池 worker 线程执行（跨线程传播前提）"
            );
            assert!(
                observation.timing_same,
                "worker 上的股票任务必须看到派发线程捕获的计时绑定（跨线程传播）"
            );
            assert!(
                observation.executor_same,
                "worker 上的股票任务必须看到派发线程捕获的扰动绑定（跨线程传播）"
            );
            assert!(
                observation.nested_inner_empty,
                "执行体内嵌套空绑定时内层必须观察到空绑定（嵌套隔离）"
            );
            assert!(
                observation.timing_restored_after_nested,
                "嵌套绑定退出后必须恢复为捕获的计时绑定（嵌套隔离）"
            );
        }
    }

    /// P2-①：内联帮助与 spawn 共用同一执行体，两条路径的绑定语义必须一致。
    /// 协调者经 `ComputeContext::install` 入池（绑定由 install 在 worker 上
    /// 重建），在飞上限 1：Stock0 派发给 worker，队尾 Stock2/Stock1 由协调者
    /// 内联执行。断言无论哪个线程执行，观察到的绑定都与协调者捕获一致。
    #[test]
    fn inline_help_stock_tasks_use_the_same_bindings_as_spawned_tasks() {
        let timing_probe = crate::verification_evidence::PhaseTimingBinding::test_collector();
        let pool = std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let shared = crate::compute::ComputeContext::from_pool(pool.clone());
        let observed: Arc<std::sync::Mutex<Vec<BindingObservation>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed_for_detached = Arc::clone(&observed);
        let ((), records) = crate::session::pipeline::with_executor_perturbation(
            executor_perturbation::ExecutorPerturbation::default(),
            || {
                crate::verification_evidence::with_phase_timing_binding(timing_probe, || {
                    // 测试观察账本与生产同构：进入 tick 前绑定真实计算上下文，
                    // 否则 enter_phase 记录会在 context_workers 缺失处显式 panic。
                    crate::verification_evidence::begin_authoritative_tick(0, &shared).unwrap();
                    let expected_timing =
                        crate::verification_evidence::current_phase_timing_binding();
                    let expected_executor = executor_perturbation::current_executor_binding();
                    let detached = move |code: &StockId| {
                        // detached 在协调线程的派发阶段执行：此刻的线程即协调线程。
                        Ok(BindingProbeShard {
                            expected_timing: expected_timing.clone(),
                            expected_executor: expected_executor.clone(),
                            coordinator: std::thread::current().id(),
                            observed: Arc::clone(&observed_for_detached),
                        })
                    };
                    let completed = shared
                        .install(|| {
                            drive_stock_stream(
                                &shared,
                                BTreeMap::new(),
                                vec![
                                    cancel(StockId(0), 1),
                                    cancel(StockId(1), 2),
                                    cancel(StockId(2), 3),
                                ],
                                StockStreamNotifications::new(),
                                detached,
                                |progress| Ok(match progress {
                        StockStreamProgress::Idle => FeedOutcome::Complete,
                        _ => FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() },
                    }),
                            )
                        })
                        .unwrap();
                    assert_eq!(completed.len(), 3);
                })
            },
        )
        .unwrap();
        let _ = records;
        let observations = observed.lock().unwrap();
        assert_eq!(observations.len(), 3, "三支股票的探针都必须执行");
        assert!(
            observations
                .iter()
                .any(|observation| observation.on_coordinator_thread),
            "在飞上限 1 的三股夹具必须实际锻炼内联帮助路径"
        );
        for observation in observations.iter() {
            assert!(
                observation.timing_same,
                "内联与 spawn 两路径必须观察到同一计时绑定（绑定语义不分叉）"
            );
            assert!(
                observation.executor_same,
                "内联与 spawn 两路径必须观察到同一扰动绑定（绑定语义不分叉）"
            );
            assert!(observation.nested_inner_empty && observation.timing_restored_after_nested);
        }
    }

    /// P2-① 两段式收尾：完成通知必须严格晚于最后一个绑定克隆的释放。
    /// 以观察账本强引用数为探针：Idle 回调（无在飞任务，持有拓扑与完成回调
    /// 相同）记录的引用数作为干净基线；StockCompleted 回调时若任务执行体的
    /// 绑定克隆尚未释放，引用数会高于基线。
    #[test]
    fn stock_completion_notification_arrives_after_the_last_binding_release() {
        let timing_probe = crate::verification_evidence::PhaseTimingBinding::test_collector();
        let pool = std::sync::Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .unwrap(),
        );
        let shared = crate::compute::ComputeContext::from_pool(pool.clone());
        let counts: Arc<std::sync::Mutex<Vec<(&'static str, usize)>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let counts_for_progress = Arc::clone(&counts);
        let ((), records) = crate::session::pipeline::with_executor_perturbation(
            executor_perturbation::ExecutorPerturbation::default(),
            || {
                crate::verification_evidence::with_phase_timing_binding(timing_probe, || {
                    crate::verification_evidence::begin_authoritative_tick(0, &shared).unwrap();
                    let watched_timing =
                        crate::verification_evidence::current_phase_timing_binding();
                    let completed = drive_stock_stream(
                        &shared,
                        BTreeMap::from([(StockId(0), EmptyShard)]),
                        vec![cancel(StockId(0), 1)],
                        StockStreamNotifications::new(),
                        |_: &StockId| -> Result<EmptyShard, StepFatal> {
                            Err(invariant("fixture must not create a detached stock"))
                        },
                        |progress| {
                            if matches!(
                                progress,
                                StockStreamProgress::StockCompleted { .. }
                                    | StockStreamProgress::Idle
                            ) {
                                counts_for_progress.lock().unwrap().push((
                                    if matches!(
                                        progress,
                                        StockStreamProgress::StockCompleted { .. }
                                    ) {
                                        "completed"
                                    } else {
                                        "idle"
                                    },
                                    watched_timing.strong_count(),
                                ));
                            }
                            Ok(match progress {
                                StockStreamProgress::Idle => FeedOutcome::Complete,
                                _ => FeedOutcome::Operations { operations: Vec::new(), outcomes: Vec::new() },
                            })
                        },
                    )
                    .unwrap();
                    assert_eq!(completed.len(), 1);
                })
            },
        )
        .unwrap();
        let _ = records;
        let counts = counts.lock().unwrap();
        let idle = counts
            .iter()
            .filter(|(tag, _)| *tag == "idle")
            .map(|(_, count)| *count)
            .collect::<Vec<_>>();
        let completed = counts
            .iter()
            .filter(|(tag, _)| *tag == "completed")
            .map(|(_, count)| *count)
            .collect::<Vec<_>>();
        assert_eq!(idle.len(), 1, "单股夹具必须恰好一次 Idle 回调");
        assert_eq!(completed.len(), 1, "单股夹具必须恰好一次完成回调");
        assert_eq!(
            completed[0], idle[0],
            "完成通知到达协调者时，任务执行体的最后一个计时绑定克隆必须已释放（两段式）"
        );
    }
}
