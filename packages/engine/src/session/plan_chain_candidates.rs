use super::plan_execution::{PlanExecutionProgress, PlanExecutionRoute, PlanRouteCommand};
use super::*;
use crate::plans::{CandidateAssessment, PlanEvent, PlanId, PlanRevision};
use std::collections::VecDeque;
use std::sync::{mpsc, Arc};
mod adaptive;
use super::decision_chain::{DecisionChainObservation, InstitutionDecisionRoot, RootReadContext};
pub(in crate::session) use adaptive::FrozenPlanChainObservation;
#[cfg(test)]
mod source_tests;

#[derive(Clone, Debug)]
pub(in crate::session) struct PlanChainCandidateBatch {
    pub(in crate::session) owner: AccountId,
    pub(in crate::session) intent: Intent,
    pub(in crate::session) chain_generation_index: u64,
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "PlanChainOperationBatch::prepare_ready_accounts".to_owned(),
    }
}

#[derive(Clone, Default)]
struct PlanChainCandidateSource {
    next_generation_index: u64,
}

impl PlanChainCandidateSource {
    fn enumerate(
        &mut self,
        command: &PlanRouteCommand,
    ) -> Result<PlanChainCandidateBatch, PlanExecutionError> {
        let chain_generation_index = self.next_generation_index;
        let next_generation_index = self
            .next_generation_index
            .checked_add(1)
            .ok_or(PlanExecutionError::CommandOrdinalOverflow)?;
        let (owner, intent) = match command {
            PlanRouteCommand::Cancel {
                account,
                code,
                order_id,
                ..
            } => (
                *account,
                Intent::Cancel {
                    code: code.clone(),
                    id: *order_id,
                },
            ),
            PlanRouteCommand::SubmitLimit {
                account,
                code,
                side,
                price,
                qty,
            } => (
                *account,
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: *side,
                    price: LimitPrice::Fixed(*price),
                    qty: *qty,
                },
            ),
        };
        let candidate = PlanChainCandidateBatch {
            owner,
            intent,
            chain_generation_index,
        };
        self.next_generation_index = next_generation_index;
        Ok(candidate)
    }
}

pub(in crate::session) struct QuotePlans {
    pub account: AccountId,
    pub market: MarketView,
    pub plans: VecDeque<PlanId>,
    pub grants: Option<crate::plans::AllocationResult>,
    pub sellable: BTreeMap<StockId, u32>,
    pub one_minute_bp: BTreeMap<StockId, Option<i32>>,
    pub thirty_minute_bp: BTreeMap<StockId, Option<i32>>,
}

pub(in crate::session) struct PlanChainOperationBatch {
    roots: PlanRootCoordinator,
    operations: VecDeque<PlanChainOperation>,
    candidate_source: PlanChainCandidateSource,
    routes: StockRouteCoordination,
    reports: Vec<PlanExecutionReport>,
    existing_plan_ids: BTreeSet<PlanId>,
}

fn route_invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "plan_chain_candidates::adaptive".to_owned(),
    }
}

/// 三类延迟事实独立消费，生命周期复核优先时仍保留报价重试上下文。
#[derive(Default)]
struct StockRouteCoordination {
    pending: BTreeMap<(AccountId, StockId), (u64, Box<PlanExecutionRoute>)>,
    reconsideration: BTreeMap<(AccountId, StockId), (CandidateAssessment, MarketView)>,
    retry_market: BTreeMap<(AccountId, StockId), MarketView>,
}

impl StockRouteCoordination {
    fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
    fn pending_for_account(&self, account: AccountId) -> bool {
        self.pending.keys().any(|(owner, _)| *owner == account)
    }
    fn is_blocked(
        &self,
        resource: &(AccountId, StockId),
        unfinished: &BTreeSet<(AccountId, StockId)>,
    ) -> bool {
        self.pending.contains_key(resource) || unfinished.contains(resource)
    }
    fn install_pending(
        &mut self,
        resource: (AccountId, StockId),
        generation: u64,
        route: Box<PlanExecutionRoute>,
    ) {
        assert!(
            self.pending.insert(resource, (generation, route)).is_none(),
            "stock route installed twice"
        );
    }
    fn pending_for_outcome(
        &self,
        resource: &(AccountId, StockId),
        generation: u64,
    ) -> Result<&PlanExecutionRoute, StepFatal> {
        let (index, route) = self
            .pending
            .get(resource)
            .ok_or_else(|| route_invariant("typed plan outcome has no pending stock route"))?;
        if *index != generation {
            return Err(route_invariant(
                "typed plan outcome names a different command",
            ));
        }
        Ok(route)
    }
    fn take_pending(
        &mut self,
        resource: &(AccountId, StockId),
        generation: u64,
    ) -> Result<Box<PlanExecutionRoute>, StepFatal> {
        self.pending_for_outcome(resource, generation)?;
        Ok(self
            .pending
            .remove(resource)
            .expect("checked pending route exists")
            .1)
    }
    fn remember_retry(&mut self, resource: (AccountId, StockId), market: MarketView) {
        self.retry_market.insert(resource, market);
    }
    fn remember_reconsideration(
        &mut self,
        resource: (AccountId, StockId),
        assessment: CandidateAssessment,
        market: MarketView,
    ) {
        self.reconsideration.insert(resource, (assessment, market));
    }
    fn take_follow_up(
        &mut self,
        (account, code): (AccountId, StockId),
    ) -> Option<PlanChainOperation> {
        if let Some((assessment, market)) = self.reconsideration.remove(&(account, code.clone())) {
            Some(PlanChainOperation::Lifecycle {
                account,
                assessments: BTreeMap::from([(code, assessment)]),
                market,
            })
        } else {
            self.retry_market
                .remove(&(account, code))
                .map(|market| PlanChainOperation::AccountExecution { account, market })
        }
    }
    #[cfg(feature = "simulation-diagnostics")]
    fn pending_cancel_cause(
        &self,
        generation: u64,
    ) -> Option<crate::session::plan_execution::PlanCancelCause> {
        self.pending
            .values()
            .find(|(index, _)| *index == generation)
            .and_then(|(_, route)| match route.command() {
                PlanRouteCommand::Cancel { cause, .. } => Some(*cause),
                PlanRouteCommand::SubmitLimit { .. } => None,
            })
    }
}

/// 缺口② burst 配额化（设计 v4 §3.1②）：root 派发同样收编台账——
/// `pending` 持有未派发 root（跨调用存续），在飞任务逐项 `reserve_spawn_slot`
/// 预留；容量受阻时池内内联推进（4c）、池外以 `RootPoll::PendingCapacity`
/// 上提至 drive 循环层等待（同 stock_stream 原语同位置）。
/// panic 终结政策（v4 处方固定）：root bound 段 catch 仅运输原 panic payload
/// （不恢复不转换，ADR-0017:117）；绑定释放后由 finish 段发布 TaskDied 与
/// 唤醒（两段式天然保证通知不早于临时绑定析构）；协调者收到 TaskDied →
/// 停止派发、排空、不 install（personal state 不恢复进权威），最后重抛原
/// payload；Disconnected 降为冗余断言（无标记的静默消失=invariant 错误）。
/// root panic 注入账户集合（测试/验证专用；p3 验收）。
#[cfg(any(test, feature = "verification-harness"))]
static ROOT_PANIC_ACCOUNTS: std::sync::Mutex<std::collections::BTreeSet<AccountId>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

/// 注入 payload 的堆对象地址登记（Astra 四审：marker Arc 判等仍可被
/// 「clone marker + rebox」伪造；改为登记 panic_any 所装 Box 的元素地址，
/// 测试对重抛对象做**同地址判等**——重建对象地址必不同，真·对象身份）。
#[cfg(any(test, feature = "verification-harness"))]
static ROOT_PANIC_TOKEN_ADDR: std::sync::Mutex<Option<usize>> =
    std::sync::Mutex::new(None);

/// 注入 payload 的对象身份标记（p3 三审③：String 内容判等无法区分 clone/rebox，
/// 测试以 `Arc::ptr_eq` 证明传播的是**原 payload 对象**，非同内容重建）。
#[cfg(any(test, feature = "verification-harness"))]
static ROOT_PANIC_MARKER: std::sync::Mutex<Option<std::sync::Arc<()>>> =
    std::sync::Mutex::new(None);

/// Drop 收满协议的「已见 Empty」探针（测试/验证专用；四审迟到窗口钉死）：
/// Drop 收满循环首次观察到空队列（=迟到 send 窗口的开启点）时在本线程回调。
/// 回调内不得取台账锁（与容量等待探针同纪律）；测试用它在该确定性时点之后
/// 才放行被闩任务的发送，钉住「Empty 之后才 send」交错。
#[cfg(any(test, feature = "verification-harness"))]
static PLAN_ROOT_DROP_EMPTY_PROBE: std::sync::Mutex<
    Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
> = std::sync::Mutex::new(None);

/// 安装/清除 Drop「已见 Empty」探针（None=清除；测试结束必须清除）。
#[cfg(any(test, feature = "verification-harness"))]
pub(in crate::session) fn set_plan_root_drop_empty_probe(
    probe: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
) {
    *PLAN_ROOT_DROP_EMPTY_PROBE
        .lock()
        .expect("Drop Empty 探针锁被 poison") = probe;
}

/// 验证观察点）：1=loop 顶（补派发前）2=try_recv 匹配前 9=TaskDied 分支。
/// Prepared 发布探针（测试/验证专用；六审「F 发布保持锚点」）：finish 段
/// `sender.send(RootMessage::Prepared)` **成功后**以账户回调——F 已执行且
/// 结果已发布的保持位锚点（早于协调者消费）。
#[cfg(any(test, feature = "verification-harness"))]
static PLAN_ROOT_PREPARED_PROBE: std::sync::Mutex<
    Option<std::sync::Arc<dyn Fn(AccountId) + Send + Sync>>,
> = std::sync::Mutex::new(None);

/// 安装/清除 Prepared 发布探针（None=清除）。
#[cfg(any(test, feature = "verification-harness"))]
pub(in crate::session) fn set_plan_root_prepared_probe(
    probe: Option<std::sync::Arc<dyn Fn(AccountId) + Send + Sync>>,
) {
    *PLAN_ROOT_PREPARED_PROBE
        .lock()
        .expect("Prepared 发布探针锁被 poison") = probe;
}

/// panic 注入前等待指定账户的 Prepared 发布（测试/验证专用；六审「F 发布
/// 保持锚点」）：注入点在 panic 前阻塞等待 `wait_for` 账户的 publish flag
/// ——panic 的发生（及其 TaskDied 发送）确定晚于该账户 Prepared 的发送。
#[cfg(any(test, feature = "verification-harness"))]
static ROOT_PANIC_WAIT_PUBLISHED: std::sync::Mutex<Option<AccountId>> =
    std::sync::Mutex::new(None);

/// publish flag/通知（panic 注入点等待侧与 Prepared 探针置位侧共用）。
#[cfg(any(test, feature = "verification-harness"))]
static PLAN_ROOT_PUBLISHED: std::sync::Mutex<std::collections::BTreeSet<AccountId>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());
#[cfg(any(test, feature = "verification-harness"))]
static PLAN_ROOT_PUBLISHED_SIGNAL: std::sync::Condvar = std::sync::Condvar::new();

/// 配置 panic 注入前需等待其 Prepared 发布的账户（None=清除；测试结束
/// 必须清除并清空 publish 集）。
#[cfg(any(test, feature = "verification-harness"))]
pub(in crate::session) fn set_root_panic_wait_published(account: Option<AccountId>) {
    *ROOT_PANIC_WAIT_PUBLISHED
        .lock()
        .expect("panic 等待 publish 配置锁被 poison") = account;
}

/// 清空 publish 保持集（测试收尾）。
#[cfg(any(test, feature = "verification-harness"))]
pub(in crate::session) fn clear_plan_root_published() {
    PLAN_ROOT_PUBLISHED
        .lock()
        .expect("publish 集锁被 poison")
        .clear();
}

/// 账户入口探针（测试/验证专用；六审「账户身份明确化」）：bound 开头以
/// 账户回调——观察点编排不再依赖无账户身份的 ordinal。
#[cfg(any(test, feature = "verification-harness"))]
static PLAN_ROOT_ACCOUNT_ENTRY_PROBE: std::sync::Mutex<
    Option<std::sync::Arc<dyn Fn(AccountId) + Send + Sync>>,
> = std::sync::Mutex::new(None);

/// 安装/清除账户入口探针。
#[cfg(any(test, feature = "verification-harness"))]
pub(in crate::session) fn set_plan_root_account_entry_probe(
    probe: Option<std::sync::Arc<dyn Fn(AccountId) + Send + Sync>>,
) {
    *PLAN_ROOT_ACCOUNT_ENTRY_PROBE
        .lock()
        .expect("账户入口探针锁被 poison") = probe;
}

/// 4b 等待分支探针（测试/验证专用；六审「确定经过 4b」）：协调者在
/// prepare 的 blocking 分支进入「在飞>0 且队列空」的自旋等待时回调
///（首次进入置位语义由测试侧约定；回调在协调者线程，不得取台账锁）。
#[cfg(any(test, feature = "verification-harness"))]
static PLAN_ROOT_4B_SPIN_PROBE: std::sync::Mutex<
    Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
> = std::sync::Mutex::new(None);

/// 安装/清除 4b 探针。
#[cfg(any(test, feature = "verification-harness"))]
pub(in crate::session) fn set_plan_root_4b_probe(
    probe: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
) {
    *PLAN_ROOT_4B_SPIN_PROBE
        .lock()
        .expect("4b 探针锁被 poison") = probe;
}

/// 派发保持账户集（测试/验证专用；六审「真实未派发 pending」）：派发点
/// reserve 成功后若命中保持集，则该项推回 pending 前端并归还预留——
/// 与 reserve 失败同分支（模拟该账户任务暂时不可派发的容量形态；
/// ROOT_PANIC_ACCOUNTS 同款进程级静态口径：串行运行、结束清空）。
#[cfg(any(test, feature = "verification-harness"))]
static ROOT_DISPATCH_HOLD_ACCOUNTS: std::sync::Mutex<std::collections::BTreeSet<AccountId>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

/// 配置/清空派发保持账户集。
#[cfg(any(test, feature = "verification-harness"))]
pub(in crate::session) fn set_root_dispatch_hold_accounts(accounts: &[AccountId]) {
    *ROOT_DISPATCH_HOLD_ACCOUNTS
        .lock()
        .expect("派发保持集锁被 poison") = accounts.iter().copied().collect();
}

/// TaskDied 发布点 TLS 直证记录（测试/验证专用；五审）：finish 段在
/// `sender.send(TaskDied)` **调用现场**记录发布线程的相位计时绑定 TLS
/// 安装态——两段式序（bound 内层绑定作用域退出 → finish 发布）要求该
/// 时刻 TLS 已卸下（实际绑定已释放）。负向 M8（把 finish 移入绑定作用域
/// 内）将记录到 installed=true → 测试红。
#[cfg(any(test, feature = "verification-harness"))]
static PLAN_ROOT_PUBLISH_TLS_STATE: std::sync::Mutex<Option<bool>> =
    std::sync::Mutex::new(None);

/// 读取最近一次 TaskDied 发布点记录的 TLS 安装态（None=尚未发布过）。
#[cfg(any(test, feature = "verification-harness"))]
pub(in crate::session) fn plan_root_publish_tls_state() -> Option<bool> {
    *PLAN_ROOT_PUBLISH_TLS_STATE
        .lock()
        .expect("TaskDied 发布点 TLS 记录锁被 poison")
}

/// TaskDied 接收探针（测试/验证专用；五审时序钉死）：协调者在
/// `prepare_ready_accounts` 的 try_recv **收到 TaskDied 消息的当场**于协调者
/// 线程回调——测试用它门控「确认失败已接收之后才放行迟到发送」（可保持
/// 阶段握手），或在接收当场核验实际绑定已释放（p3 发布顺序直证）。
/// 回调内不得取台账锁；不得阻塞（协调者线程）。
#[cfg(any(test, feature = "verification-harness"))]
static PLAN_ROOT_TASKDIED_RECEIVED_PROBE: std::sync::Mutex<
    Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
> = std::sync::Mutex::new(None);

/// 安装/清除 TaskDied 接收探针（None=清除；测试结束必须清除）。
#[cfg(any(test, feature = "verification-harness"))]
pub(in crate::session) fn set_plan_root_taskdied_probe(
    probe: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
) {
    *PLAN_ROOT_TASKDIED_RECEIVED_PROBE
        .lock()
        .expect("TaskDied 接收探针锁被 poison") = probe;
}

/// 读取已登记的注入 payload 堆地址（对象身份判等的期望侧）。
#[cfg(any(test, feature = "verification-harness"))]
pub(in crate::session) fn root_panic_token_addr() -> Option<usize> {
    *ROOT_PANIC_TOKEN_ADDR
        .lock()
        .expect("root panic 地址登记锁被 poison")
}

/// root panic 注入的对象身份载体（测试/验证专用）：account 承载可读身份，
/// marker 承载对象身份（Arc 指针判等）。
#[cfg(any(test, feature = "verification-harness"))]
pub(in crate::session) struct RootPanicToken {
    pub(in crate::session) account: AccountId,
    #[allow(dead_code)]
    pub(in crate::session) marker: Option<std::sync::Arc<()>>,
}

/// 配置 root panic 注入账户（测试/验证专用）；返回本次注入的 payload 身份
/// 标记，供测试对重抛对象做 `Arc::ptr_eq` 判等。
#[cfg(any(test, feature = "verification-harness"))]
pub(in crate::session) fn set_root_panic_accounts(accounts: &[AccountId]) {
    *ROOT_PANIC_ACCOUNTS
        .lock()
        .expect("root panic 注入锁被 poison") = accounts.iter().copied().collect();
    // 地址登记**不随清空**：每次注入必然覆写；payload 断言只可能发生在本
    // 次注入触发之后（否则无 payload 可断言），跨用例残留值不可达断言路径。
    *ROOT_PANIC_MARKER
        .lock()
        .expect("root panic marker 锁被 poison") = None;
}

/// bound 段绑定释放探针（测试/验证专用；p3 发布顺序钉住）：guard 由 bound
/// 闭包捕获，bound 环境析构（含计时/扰动绑定释放）时回调——两段式序保证
/// finish 段的 TaskDied 发布严格晚于该析构，协调者收到 TaskDied 时探针必已
/// 触发。
#[cfg(any(test, feature = "verification-harness"))]
static ROOT_BOUND_RELEASE_PROBE: std::sync::Mutex<
    Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
> = std::sync::Mutex::new(None);

/// 安装/清除 bound 释放探针（None=清除；测试结束必须清除，防串扰）。
#[cfg(any(test, feature = "verification-harness"))]
pub(in crate::session) fn set_root_bound_release_probe(
    probe: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
) {
    *ROOT_BOUND_RELEASE_PROBE
        .lock()
        .expect("root bound 释放探针锁被 poison") = probe;
}

#[cfg(any(test, feature = "verification-harness"))]
struct RootBoundReleaseGuard(Option<std::sync::Arc<dyn Fn() + Send + Sync>>);

#[cfg(any(test, feature = "verification-harness"))]
impl Drop for RootBoundReleaseGuard {
    fn drop(&mut self) {
        if let Some(probe) = self.0.take() {
            probe();
        }
    }
}

enum AccountSource {
    Empty,
    Accounts {
        remaining: VecDeque<AccountId>,
        observations: DecisionChainObservation,
    },
    InFlight {
        sender: mpsc::Sender<RootMessage>,
        receiver: mpsc::Receiver<RootMessage>,
        /// 在飞已派发数（排空完成=全部已派发任务的终结标记到达；标记到达
        /// ≠ 所有 guard 已析构——容量断言以最终态为准，设计 §3.1④ 三审③）。
        remaining: usize,
        snapshot: Arc<RootReadContext>,
        /// 入口捕获一次，同一 Arc 克隆贯穿全部 wave 不重捕获（设计 §3.1②）。
        observations: Arc<DecisionChainObservation>,
        /// 未派发 root（容量受阻留存；TaskDied 后冻结）。
        pending: VecDeque<(
            AccountId,
            super::decision_chain::personal_state::PlanPersonalState,
        )>,
        compute: std::sync::Arc<crate::compute::ComputeContext>,
        /// 首个 unwind 的原 panic payload；收到后冻结 pending，排空后重抛。
        task_died: Option<Box<dyn std::any::Any + Send>>,
    },
}

/// bound 段的运输结果：观察载荷，或 unwind 的原 panic payload（仅运输）。
enum RootOutcome {
    Prepared(PreparedRoot),
    Died(Box<dyn std::any::Any + Send>),
}

/// spawned root 任务的消息：观察载荷（含 observe 的 Option 语义），或 unwind
/// 的原 panic payload（TaskDied，绑定释放后由 finish 段发布）。
enum RootMessage {
    Prepared(PreparedRoot),
    TaskDied(Box<dyn std::any::Any + Send>),
}

/// `PlanRootCoordinator::prepare_ready_accounts` 的回复：交付载荷（None=非阻塞
/// 暂无），或池外协调者的容量上提信号（drive 循环层 `wait_for_spawn_capacity`
/// 后重入）。
pub(super) enum RootPoll {
    Delivered(Option<PlanChainOperationBatch>),
    PendingCapacity,
}

/// `PlanChainOperationBatch::prepare_ready_accounts` 的包装回复。
pub(in crate::session) enum PrepareReady {
    Progress(bool),
    PendingCapacity,
}

type PreparedRoot = (
    AccountId,
    super::decision_chain::personal_state::PlanPersonalState,
    PlanChainOperationBatch,
    super::decision_chain::PlanRootDiagnostics,
);

enum PlanChainOperation {
    ExecutionRoute(Box<PlanExecutionRoute>),
    QuotePlans(QuotePlans),
    Lifecycle {
        account: AccountId,
        assessments: BTreeMap<StockId, CandidateAssessment>,
        market: MarketView,
    },
    Restructure {
        plan_id: PlanId,
        child_order_id: Option<OrderId>,
        terminating: bool,
        event: PlanEvent,
    },
    AccountExecution {
        account: AccountId,
        market: MarketView,
    },
    Execute(PlanExecutionRequest),
}

/// 管理异步 root 与个人状态返还，不拥有后续执行路由。
struct PlanRootCoordinator {
    source: AccountSource,
}

/// P1-①（增量二十，Astra/Luna 三审最小剩余集）：协调者被外层失败退出提前
/// 销毁（如 `market_tick_transaction` 的 `prepared?` 提前 return 丢弃整条链）
/// 时的**确定性排空协议**，关闭增量十九版本残留的两条失联窗口：
/// ① 已保存的 `task_died` payload——非阻塞路径存入字段后返回、其他 root 尚
///    在飞，随后外层失败销毁协调者（旧 Drop 只扫队列、忽略字段）；
/// ② 排空见 `Empty` 后、receiver 析构前的迟到 send 竞态——send 成功落入
///    队列但不再有人读，send 失败兜底不触发（旧 Drop 一次性排空后即返回）。
/// 协议依据：每个已派发任务恰发一条终结消息（Prepared 或 TaskDied）⇒ 以
/// `remaining` 为目标计数**收满**：池外协调者阻塞 `recv`（在飞任务运行于池
/// worker，本线程不占 worker，等待有限）；池内协调者 `try_recv`+`yield_work`
/// 自旋（参与偷取，自推进在飞任务）。期间 TaskDied 取首份留存、Prepared 随
/// 批丢弃（失败原子性：私有候选与 personal 不进权威，a1 验收）。收满后如有
/// payload 则 `resume_unwind` 原 panic payload（发布已发生 ⇒ 传播义务在持有
/// 者销毁点兑现）；`Disconnected`（本结构仍持有原始 sender，按协议不可能）
/// = 无标记静默消失，invariant 显式报告。
impl Drop for PlanRootCoordinator {
    fn drop(&mut self) {
        let payload = match &mut self.source {
            AccountSource::InFlight {
                receiver,
                remaining,
                compute,
                task_died,
                ..
            } => {
                let target = *remaining;
                let mut payload = task_died.take();
                let mut received = 0usize;
                let mut empty_observed = false;
                let in_pool = compute.is_worker();
                while received < target {
                    // 显式观察队列：**首次见 Empty** = 迟到 send 窗口的开启点
                    //（此后至收满前到达的消息都是「Empty 之后才 send」的迟到者
                    //——四审迟到窗口钉死的协议锚点；探针在该时点回调一次）。
                    let message = match receiver.try_recv() {
                        Ok(message) => message,
                        Err(mpsc::TryRecvError::Empty) => {
                            if !empty_observed {
                                empty_observed = true;
                                #[cfg(any(test, feature = "verification-harness"))]
                                if let Some(probe) = PLAN_ROOT_DROP_EMPTY_PROBE
                                    .lock()
                                    .expect("Drop Empty 探针锁被 poison")
                                    .clone()
                                {
                                    probe();
                                }
                            }
                            if in_pool {
                                // 池内：协作自旋（参与偷取，自推进在飞任务）。
                                crate::compute::yield_work();
                                continue;
                            }
                            // 池外：阻塞等待下一条（在飞任务运行于池 worker，
                            // 本线程不占 worker，等待有限）。
                            match receiver.recv() {
                                Ok(message) => message,
                                Err(_) if !std::thread::panicking() => {
                                    panic!(
                                        "plan root worker ended without a result（Drop 排空期无标记静默消失）"
                                    );
                                }
                                Err(_) => {
                                    eprintln!("plan root coordinator drop: channel disconnected mid-unwind (invariant registered, original failure retained)");
                                    return;
                                }
                            }
                        }
                        Err(mpsc::TryRecvError::Disconnected) if !std::thread::panicking() => {
                            panic!(
                                "plan root worker ended without a result（Drop 排空期无标记静默消失）"
                            );
                        }
                        Err(mpsc::TryRecvError::Disconnected) => {
                            eprintln!("plan root coordinator drop: channel disconnected mid-unwind (invariant registered, original failure retained)");
                            return;
                        }
                    };
                    received += 1;
                    if let RootMessage::TaskDied(died) = message {
                        payload.get_or_insert(died);
                    }
                }
                payload
            }
            _ => None,
        };
        if let Some(payload) = payload {
            // 双重 panic 防护（增量二十）：线程已在 panic 展开中时再重抛会以
            // 「panic in a destructor」abort 并吞掉**原始**失败原因——改为显式
            // stderr 登记（固定字面量；payload 随原始 unwind 一起终结，不静默：
            // 已有进程级失败在途）。非 unwind 路径（外层 Err 提前销毁）照常重抛。
            if std::thread::panicking() {
                eprintln!("plan root coordinator dropped mid-unwind: TaskDied payload not re-raised (double panic would mask the original failure)");
                return;
            }
            std::panic::resume_unwind(payload);
        }
    }
}








impl PlanRootCoordinator {
    fn empty() -> Self {
        Self {
            source: AccountSource::Empty,
        }
    }
    fn is_source_empty(&self) -> bool {
        matches!(self.source, AccountSource::Empty)
    }

    fn is_empty(&self) -> bool {
        match &self.source {
            AccountSource::Empty => true,
            AccountSource::Accounts { remaining, .. } => remaining.is_empty(),
            // Complete 判定扩展（设计 §3.1③）：在飞与 pending 都排空才算空。
            AccountSource::InFlight {
                remaining, pending, ..
            } => *remaining == 0 && pending.is_empty(),
        }
    }
    fn accounts(accounts: Vec<AccountId>, observations: DecisionChainObservation) -> Self {
        Self {
            source: AccountSource::Accounts {
                remaining: accounts.into(),
                observations,
            },
        }
    }
    fn start_ready_accounts(&mut self, session: &mut GameSession) -> Result<(), StepFatal> {
        if !matches!(self.source, AccountSource::Accounts { .. }) {
            return Ok(());
        }
        let AccountSource::Accounts {
            remaining,
            observations,
        } = std::mem::replace(&mut self.source, AccountSource::Empty)
        else {
            return Ok(());
        };
        if remaining.is_empty() {
            return Ok(());
        }
        // snapshot/observations 入口捕获一次，同一 Arc 克隆贯穿全部 wave
        // 不重捕获（设计 §3.1②）；personal 入口 take 一次，pending 持有。
        let snapshot = Arc::new(RootReadContext::capture(session)?);
        let observations = Arc::new(observations);
        let pending: VecDeque<_> = remaining
            .into_iter()
            .map(|account| {
                (
                    account,
                    super::decision_chain::personal_state::PlanPersonalState::take(
                        session, account,
                    ),
                )
            })
            .collect();
        let (sender, receiver) = mpsc::channel();
        let compute = session.compute.clone();
        self.source = AccountSource::InFlight {
            sender,
            receiver,
            remaining: 0,
            snapshot,
            observations,
            pending,
            compute,
            task_died: None,
        };
        self.dispatch_ready_accounts();
        Ok(())
    }

    /// 尽力派发：reserve 成功才 spawn；失败留 pending 绝不 spawn（台账可见）。
    /// TaskDied 已收到时不再派发（pending 冻结，设计 P-b）。
    fn dispatch_ready_accounts(&mut self) {
        let AccountSource::InFlight {
            sender,
            receiver: _,
            remaining,
            snapshot,
            observations,
            pending,
            compute,
            task_died,
        } = &mut self.source
        else {
            return;
        };
        if task_died.is_some() {
            return;
        }
        while let Some((account, personal)) = pending.front() {
            let Some(guard) = compute.reserve_spawn_slot() else {
                break;
            };
            // 派发保持集（六审）：命中账户推回 pending 前端并归还预留——与
            // reserve 失败同分支（容量形态模拟，pending 冻结语义不变）。
            #[cfg(any(test, feature = "verification-harness"))]
            if ROOT_DISPATCH_HOLD_ACCOUNTS
                .lock()
                .expect("派发保持集锁被 poison")
                .contains(account)
            {
                drop(guard);
                break;
            }
            let (account, personal) = pending.pop_front().expect("front 已确认存在");
            #[cfg(any(test, feature = "verification-harness"))]
            compute.run_dispatch_gate();
            // guard 在 bound 前创建，由 finish 闭包 move（发布后 Drop 归还，v4 顺序）。
            let snapshot = Arc::clone(snapshot);
            let observations = Arc::clone(observations);
            let sender = sender.clone();
                let worker_compute = Arc::clone(compute);
            let spawner = Arc::clone(compute);
            // 两段式：观察在继承绑定内完成；typed 结果与完成通知在绑定释放后
            // 发送。panic 政策：bound 内 catch 仅运输原 payload（不恢复不转换）；
            // guard 由 finish 段持有，发布之后 Drop 归还容量并广播（v4 顺序）。
            spawner.spawn_two_stage(
                move || {
                    let timing = crate::verification_evidence::current_phase_timing_binding();
                    #[cfg(any(test, feature = "verification-harness"))]
                    let executor = crate::session::pipeline::current_executor_binding();
                    // guard 由 finish 段持有（增量十八 P2 对齐 v4『先发布、后归还
                    // 容量』）：bound 绑定释放 → finish 发布 TaskDied/结果与唤醒 →
                    // guard 随任务闭包收尾 Drop 归还容量并广播（发布先于归还）。
                    let bound = move || {
                        worker_compute.record(crate::compute::ComputeWork::PlanRoot);
                        // 账户入口探针（六审；七轮锁环修复）：**先取 clone 并
                        // 释放探针锁，再调用回调**——旧形态 `if let Some(p) =
                        // lock().clone() { p() }` 的临时 MutexGuard 存活覆盖
                        // 回调执行（Rust if-let 临时生存期），回调内等待时持有
                        // 全局探针锁，阻塞后续 root 的入口探针调用 ⇒ 互锁
                        //（六轮 inflight 3/5 挂起根因；M10 回退红证钉住）。
                        #[cfg(any(test, feature = "verification-harness"))]
                        {
                            let probe = PLAN_ROOT_ACCOUNT_ENTRY_PROBE
                                .lock()
                                .expect("账户入口探针锁被 poison")
                                .clone();
                            if let Some(probe) = probe {
                                probe(account);
                            }
                        }

                        // 绑定释放探针（p3 发布顺序）：guard 随本闭包捕获，bound
                        // 环境析构时触发——严格早于 finish 段的终结发布。
                        #[cfg(any(test, feature = "verification-harness"))]
                        let _bound_release_probe = RootBoundReleaseGuard(
                            ROOT_BOUND_RELEASE_PROBE
                                .lock()
                                .expect("root bound 释放探针锁被 poison")
                                .clone(),
                        );
                        // 注入点在 catch_unwind 运输区内（p3 验收；区外裸 panic
                        // 无 TaskDied 终结信号）。payload=RootPanicToken（对象
                        // 身份可判等，三审③）。
                        #[cfg(any(test, feature = "verification-harness"))]
                        if ROOT_PANIC_ACCOUNTS
                            .lock()
                            .expect("root panic 注入锁被 poison")
                            .contains(&account)
                        {
                            // panic 前等待（六审「F 发布保持锚点」）：等待指定
                            // 账户的 Prepared 发布 flag 后再 panic——TaskDied 的
                            // 发送确定晚于该账户 Prepared 的发送（FIFO ⇒ 协调者
                            // 先消费该 Prepared）。
                            #[cfg(any(test, feature = "verification-harness"))]
                            {
                                // 七轮锁环修复：先取配置并释放配置锁再等待。
                                let wait_for = *ROOT_PANIC_WAIT_PUBLISHED
                                    .lock()
                                    .expect("panic 等待 publish 配置锁被 poison");
                                if let Some(wait_for) = wait_for {
                                    let mut published = PLAN_ROOT_PUBLISHED
                                        .lock()
                                        .expect("publish 集锁被 poison");
                                    while !published.contains(&wait_for) {
                                        published = match PLAN_ROOT_PUBLISHED_SIGNAL
                                            .wait(published)
                                        {
                                            Ok(guard) => guard,
                                            Err(poisoned) => poisoned.into_inner(),
                                        };
                                    }
                                }
                            }
                            let marker = ROOT_PANIC_MARKER
                                .lock()
                                .expect("root panic marker 锁被 poison")
                                .clone();
                            let token = Box::new(RootPanicToken { account, marker });
                            *ROOT_PANIC_TOKEN_ADDR
                                .lock()
                                .expect("root panic 地址登记锁被 poison") =
                                Some(&*token as *const RootPanicToken as usize);
                            std::panic::panic_any(token);
                        }
                        InstitutionDecisionRoot::new(account, personal)
                            .observe(&snapshot, &observations)
                    };
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                        crate::verification_evidence::with_phase_timing_binding(timing, move || {
                            #[cfg(any(test, feature = "verification-harness"))]
                            {
                                crate::session::pipeline::with_executor_binding(executor, bound)
                            }
                            #[cfg(not(any(test, feature = "verification-harness")))]
                            {
                                bound()
                            }
                        })
                    })) {
                        Ok(payload) => RootOutcome::Prepared(payload),
                        Err(panic_payload) => RootOutcome::Died(panic_payload),
                    }
                },
                move |outcome| {
                    // guard 由本 finish 段持有：先发布（结果/TaskDied+唤醒），
                    // 后归还容量（v4 §3.1②『先发布、后归还 guard』；增量十八 P2）。
                    let _publish_then_release = &guard;
                    match outcome {
                        RootOutcome::Prepared(result) => {
                            // 先发布 typed 结果再唤醒 coordinator；receiver 已关闭
                            // 表示私有 tick 已丢弃。
                            #[cfg(any(test, feature = "verification-harness"))]
                            let published_account = result.0;
                            if sender.send(RootMessage::Prepared(result)).is_ok() {
                                // Prepared 发布探针 + publish 保持集（六审）：send
                                // 成功后置位——「F 已发布」锚点。
                                #[cfg(any(test, feature = "verification-harness"))]
                                {
                                    PLAN_ROOT_PUBLISHED
                                        .lock()
                                        .expect("publish 集锁被 poison")
                                        .insert(published_account);
                                    PLAN_ROOT_PUBLISHED_SIGNAL.notify_all();
                                    // 七轮锁环修复：clone 后释放探针锁再调用。
                                    let probe = PLAN_ROOT_PREPARED_PROBE
                                        .lock()
                                        .expect("Prepared 发布探针锁被 poison")
                                        .clone();
                                    if let Some(probe) = probe {
                                        probe(published_account);
                                    }
                                }
                            }
                        }
                        RootOutcome::Died(panic_payload) => {
                            // 发布点 TLS 直证（五审）：send 调用现场记录发布
                            // 线程的绑定安装态——发布时实际绑定必须已卸下。
                            #[cfg(any(test, feature = "verification-harness"))]
                            {
                                *PLAN_ROOT_PUBLISH_TLS_STATE
                                    .lock()
                                    .expect("TaskDied 发布点 TLS 记录锁被 poison") =
                                    Some(
                                        crate::verification_evidence::phase_timing_binding_installed_on_this_thread(),
                                    );
                            }
                            // 终结通知不早于临时绑定析构（finish 天然晚于 bound
                            // 闭包 drop，含 guard）；TaskDied 同样唤醒 coordinator。
                            // 增量十八 P1-②（Astra 终审）：协调者/receiver 已被
                            // 外层失败退出销毁时，send 失败不得静默吞 panic——
                            // 在任务线程重抛原 payload（运输失败的兜底传播；
                            // guard 由本闭包收尾 Drop 归还容量，发布仍先于归还）。
                            match sender.send(RootMessage::TaskDied(panic_payload)) {
                                Ok(()) => {
                                }
                                Err(mpsc::SendError(RootMessage::TaskDied(payload))) => {
                                    std::panic::resume_unwind(payload);
                                }
                                Err(_) => unreachable!("发送失败必然是 TaskDied 变体"),
                            }
                        }
                    }
                },
            );
            *remaining += 1;
        }
    }

    pub(in crate::session) fn prepare_ready_accounts(
        &mut self,
        session: &mut GameSession,
        blocking: bool,
    ) -> Result<RootPoll, StepFatal> {
        self.start_ready_accounts(session)?;
        loop {
            // R1-c 消费优先（设计 §3.1②/§R1-c；增量二十八重排序）：每轮
            // **先消费现成结果**，再做任何补派发——现成交付不得被派发阻塞
            //（增量十八把补派发放在循环顶与设计顺序不一致，Astra 归因审查
            // 指认后对齐）。补派发点后移至 4c（零在飞+pending 非空）判定前：
            // P1「容量恢复后 pending 必须继续派发至排空」不变——两者并存
            //（P1=不能不派发；R1-c=同轮内先收后补）。首轮派发仍由
            // start_ready_accounts 承担。
            let AccountSource::InFlight {
                receiver,
                remaining,
                snapshot,
                observations,
                pending,
                compute,
                task_died,
                ..
            } = &mut self.source
            else {
                return Ok(RootPoll::Delivered(None));
            };
            match receiver.try_recv() {
                Ok(RootMessage::Prepared(result)) => {
                    *remaining -= 1;
                    if task_died.is_some() {
                        // 排空期间到达的迟到结果不 install（a1 失败原子性：
                        // personal state 随私有候选丢弃，不恢复进权威状态）。
                        if *remaining == 0 && pending.is_empty() {
                            let payload = task_died
                                .take()
                                .expect("TaskDied 分支必然已存 payload");
                            self.source = AccountSource::Empty;
                            std::panic::resume_unwind(payload);
                        }
                        continue;
                    }
                    let (account, personal, generated, diagnostics) = result;
                    #[cfg(test)]
                    session
                        .compute
                        .t1_probes
                        .root_personal_installs
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    personal.install(session, account);
                    #[cfg(feature = "simulation-diagnostics")]
                    session.record_plan_root_diagnostics(account, diagnostics, &snapshot.plans);
                    #[cfg(not(feature = "simulation-diagnostics"))]
                    let _ = (diagnostics, snapshot);
                    if *remaining == 0 && pending.is_empty() {
                        self.source = AccountSource::Empty;
                    }
                    return Ok(RootPoll::Delivered(Some(generated)));
                }
                Ok(RootMessage::TaskDied(payload)) => {
                    // 接收探针：收到 TaskDied 的当场（协调者线程）回调——
                    // 早于存储/冻结/排空处置（五审：迟到发送门控与绑定释放
                    // 接收点直证的锚点）。
                    #[cfg(any(test, feature = "verification-harness"))]
                    {
                        // 七轮锁环修复：clone 后释放探针锁再调用。
                        let probe = PLAN_ROOT_TASKDIED_RECEIVED_PROBE
                            .lock()
                            .expect("TaskDied 接收探针锁被 poison")
                            .clone();
                        if let Some(probe) = probe {
                            probe();
                        }
                    }
                    *remaining -= 1;
                    if task_died.is_none() {
                        *task_died = Some(payload);
                    }
                    // P-b：停止派发（dispatch 的 task_died 守卫）、排空其余在飞。
                    if *remaining == 0 {
                        let payload = task_died
                            .take()
                            .expect("TaskDied 分支必然已存 payload");
                        self.source = AccountSource::Empty;
                        std::panic::resume_unwind(payload);
                    }
                    continue;
                }
                Err(mpsc::TryRecvError::Empty) if !blocking => {
                    return Ok(RootPoll::Delivered(None));
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if *remaining > 0 {
                        // 4b 探针（六审分支锚点；七轮两修）：(a) 仅在
                        // **task_died 已存**（失败后等待迟到结果）时触发——
                        // 普通空队列等待不触发，杜绝「先普通 4b 放行 → 后收
                        // TaskDied」交错；(b) 先取 clone 释放探针锁再调用
                        //（锁环修复，同其他探针）。
                        #[cfg(any(test, feature = "verification-harness"))]
                        if task_died.is_some() {
                            let probe = PLAN_ROOT_4B_SPIN_PROBE
                                .lock()
                                .expect("4b 探针锁被 poison")
                                .clone();
                            if let Some(probe) = probe {
                                probe();
                            }
                        }
                        // 4b：等待已派发 root（等待目标=已入队在飞任务，合法）。
                        crate::compute::yield_work();
                        continue;
                    }
                    // 4c 前补派发（P1：容量恢复后 pending 必须继续派发；
                    // task_died 已存时 dispatch 内部守卫直接冻结）。
                    drop((receiver, remaining, snapshot, observations, pending, compute, task_died));
                    self.dispatch_ready_accounts();
                    let AccountSource::InFlight {
                        receiver,
                        remaining,
                        snapshot,
                        observations,
                        pending,
                        compute,
                        task_died,
                        ..
                    } = &mut self.source
                    else {
                        return Ok(RootPoll::Delivered(None));
                    };
                    // 补派发成功 ⇒ 在飞>0：回到 4b 等待；仍零在飞 ⇒ 继续 4c。
                    if *remaining > 0 {
                        crate::compute::yield_work();
                        continue;
                    }
                    // 4c：在飞==0 且 pending 非空。
                    if task_died.is_some() {
                        let payload = task_died
                            .take()
                            .expect("TaskDied 分支必然已存 payload");
                        self.source = AccountSource::Empty;
                        std::panic::resume_unwind(payload);
                    }
                    if compute.is_worker() {
                        // 池内：内联一项自有 pending root（R1-a 同源：零在飞
                        // 不等结果）。同一 snapshot/observations Arc，不重捕获。
                        let (account, personal) =
                            pending.pop_front().expect("4c 内联项来自非空 pending");
                        let snapshot = Arc::clone(snapshot);
                        let observations = Arc::clone(observations);
                        let root =
                            InstitutionDecisionRoot::new(account, personal)
                                .observe(&snapshot, &observations);
                        let (account, personal, generated, diagnostics) = root;
                        personal.install(session, account);
                        #[cfg(feature = "simulation-diagnostics")]
                        session.record_plan_root_diagnostics(account, diagnostics, &snapshot.plans);
                        #[cfg(not(feature = "simulation-diagnostics"))]
                        let _ = diagnostics;
                        if pending.is_empty() {
                            self.source = AccountSource::Empty;
                        }
                        return Ok(RootPoll::Delivered(Some(generated)));
                    } else {
                        // 池外：容量上提信号（drive 循环层 wait_for_spawn_capacity
                        // 后重入本循环，重入从 4a 起）。
                        return Ok(RootPoll::PendingCapacity);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(invariant(
                        "plan root worker ended without a result（无 TaskDied 标记的静默消失）",
                    ));
                }
            }
        }
    }
}

impl PlanChainOperationBatch {
    /// T5（Sol 终裁·完成边界见证）：root **源**是否排空（AccountSource=Empty）
    /// ——不依赖 operations/routes 状态；capture 返回当场断言用。
    pub(in crate::session) fn roots_source_drained(&self) -> bool {
        self.roots.is_source_empty()
    }

    pub(in crate::session) fn is_empty(&self) -> bool {
        self.operations.is_empty() && !self.routes.has_pending() && self.roots.is_empty()
    }

    pub(in crate::session) fn push_quote_plans(&mut self, cursor: QuotePlans) {
        self.operations
            .push_back(PlanChainOperation::QuotePlans(cursor));
    }
    pub(in crate::session) fn empty() -> Self {
        Self {
            roots: PlanRootCoordinator::empty(),
            operations: VecDeque::new(),
            candidate_source: PlanChainCandidateSource::default(),
            routes: StockRouteCoordination::default(),
            reports: Vec::new(),
            existing_plan_ids: BTreeSet::new(),
        }
    }

    #[cfg(test)]
    pub(in crate::session) fn accounts(
        accounts: Vec<AccountId>,
        market: MarketView,
        paths: BTreeMap<StockId, crate::observation::PricePathObservation>,
        technical: BTreeMap<StockId, crate::observation::TechnicalObservation>,
        now: crate::calendar::CivilInstant,
        exposed: BTreeSet<StockId>,
    ) -> Self {
        Self::from_observation(
            accounts,
            DecisionChainObservation {
                market,
                paths,
                technical,
                now,
                exposed,
            },
        )
    }

    pub(in crate::session) fn from_observation(
        accounts: Vec<AccountId>,
        observation: DecisionChainObservation,
    ) -> Self {
        let mut batch = Self::empty();
        if !accounts.is_empty() {
            batch.roots = PlanRootCoordinator::accounts(accounts, observation);
        }
        batch
    }

    /// T5（Sol T5 节点审·必修 1·root 闭环）：在既定 post-expiry 冻结观察基上
    /// **完成**全部真实 root 计算后交给执行——阻塞排空（R1-c 消费优先/
    /// 4b 协作等待/4c 补派发/TaskDied 排空协议原样复用；personal patch 在
    /// 接收点内联 install）。Progress(false)@blocking ⟺ 全部归并完成；
    /// PendingCapacity=池外协调者容量受阻（真实同步等待后重入，等待目标=
    /// 他人在飞任务释放）。执行期自此无 root 异步回传/唤醒/等待。
    pub(in crate::session) fn drain_ready_accounts_at_capture(
        &mut self,
        session: &mut GameSession,
    ) -> Result<(), StepFatal> {
        loop {
            match self.prepare_ready_accounts(session, true)? {
                PrepareReady::Progress(true) => continue,
                PrepareReady::Progress(false) => return Ok(()),
                PrepareReady::PendingCapacity => {
                    #[cfg(test)]
                    session
                        .compute
                        .t1_probes
                        .root_drain_capacity_waits
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    session.compute.wait_for_spawn_capacity();
                    continue;
                }
            }
        }
    }

    pub(in crate::session) fn enumerate_candidate(
        &mut self,
        command: &PlanRouteCommand,
    ) -> Result<PlanChainCandidateBatch, PlanExecutionError> {
        self.candidate_source.enumerate(command)
    }

    /// 汇总已经完成的 root；仅在事务没有其他就绪工作时等待，保持后续执行操作次序。
    /// 回复三态（设计 §3.1②/③）：Progress(true)=有新操作入队；Progress(false)=
    /// 本轮无交付；PendingCapacity=池外容量上提信号（drive 循环层处理）。
    pub(in crate::session) fn prepare_ready_accounts(
        &mut self,
        session: &mut GameSession,
        blocking: bool,
    ) -> Result<PrepareReady, StepFatal> {
        match self.roots.prepare_ready_accounts(session, blocking)? {
            RootPoll::Delivered(Some(mut generated)) => {
                self.operations.append(&mut generated.operations);
                self.existing_plan_ids
                    .append(&mut generated.existing_plan_ids);
                Ok(PrepareReady::Progress(true))
            }
            RootPoll::Delivered(None) => Ok(PrepareReady::Progress(false)),
            RootPoll::PendingCapacity => Ok(PrepareReady::PendingCapacity),
        }
    }

    pub(in crate::session) fn push_account_execution(
        &mut self,
        account: AccountId,
        market: MarketView,
    ) {
        self.operations
            .push_back(PlanChainOperation::AccountExecution { account, market });
    }

    pub(in crate::session) fn remember_existing_plans(
        &mut self,
        plans: impl IntoIterator<Item = PlanId>,
    ) {
        self.existing_plan_ids.extend(plans);
    }

    pub(in crate::session) fn push_lifecycle(
        &mut self,
        account: AccountId,
        assessments: BTreeMap<StockId, CandidateAssessment>,
        market: MarketView,
    ) {
        self.operations.push_back(PlanChainOperation::Lifecycle {
            account,
            assessments,
            market,
        });
    }

    pub(in crate::session) fn push_restructure(
        &mut self,
        _account: AccountId,
        plan_id: PlanId,
        _code: StockId,
        child_order_id: Option<OrderId>,
        terminating: bool,
        revision: PlanRevision,
    ) {
        self.operations.push_back(PlanChainOperation::Restructure {
            plan_id,
            child_order_id,
            terminating,
            event: PlanEvent::Revised { revision },
        });
    }

    pub(in crate::session) fn push_termination(
        &mut self,
        plan_id: PlanId,
        child_order_id: Option<OrderId>,
        reason: crate::plans::TerminationReason,
        trading_day: u64,
    ) {
        self.operations.push_back(PlanChainOperation::Restructure {
            plan_id,
            child_order_id,
            terminating: true,
            event: PlanEvent::Terminated {
                reason,
                trading_day,
            },
        });
    }

    #[cfg(test)]
    pub(in crate::session) fn push_execution(&mut self, request: PlanExecutionRequest) {
        self.operations
            .push_back(PlanChainOperation::Execute(request));
    }

    #[cfg(test)]
    pub(in crate::session) fn len(&self) -> usize {
        self.operations.len()
    }
}
