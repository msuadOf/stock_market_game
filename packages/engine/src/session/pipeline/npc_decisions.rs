//! 运行纯 NPC 决策源。
use super::{DecisionAccountInput, DecisionSnapshot, DecisionSnapshotError, IntentCandidateKey};
use crate::account::StoredStrategy;
use crate::strategy::{Intent, StrategyDecision, StrategyState, StrategyStateError};
use crate::{AccountId, SplitMix64};
use std::collections::BTreeSet;
use std::sync::{mpsc, Arc};

/// NPC raw intent 携带身份；候选 composition 必须保留该身份。
#[derive(Clone, Debug, serde::Serialize)]
pub(in crate::session) struct NpcDecisionIntent {
    key: IntentCandidateKey,
    intent: Intent,
}

impl NpcDecisionIntent {
    pub(in crate::session) const fn key(&self) -> &IntentCandidateKey {
        &self.key
    }

    pub(in crate::session) const fn intent(&self) -> &Intent {
        &self.intent
    }
}

/// The prospective, non-routing decision result for one NPC account.
#[derive(Clone, Debug)]
pub(in crate::session) struct NpcDecisionAccountOutput {
    account: AccountId,
    #[cfg(test)]
    strategy_state: Option<StrategyState>,
    strategy: Option<NpcStrategyUpdate>,
    reviewed_stocks: BTreeSet<crate::StockId>,
    position_decision: Option<crate::behavior::PositionDecision>,
    execution_urgency: crate::plans::urgency::risk::RiskUrgencyAssessment,
    updates_working_quotes: bool,
    uses_parent_order_execution: bool,
}

#[derive(Clone, Debug)]
pub(in crate::session) enum NpcStrategyUpdate {
    Unchanged,
    Replace(StoredStrategy),
}

impl NpcDecisionAccountOutput {
    pub(in crate::session) const fn account(&self) -> AccountId {
        self.account
    }

    #[cfg(test)]
    pub(in crate::session) const fn strategy_state(&self) -> Option<&StrategyState> {
        self.strategy_state.as_ref()
    }

    pub(in crate::session) fn take_strategy(&mut self) -> Option<NpcStrategyUpdate> {
        #[cfg(test)]
        self.strategy_state.take()?;
        self.strategy.take()
    }

    pub(in crate::session) const fn reviewed_stocks(&self) -> &BTreeSet<crate::StockId> {
        &self.reviewed_stocks
    }

    pub(in crate::session) const fn position_decision(
        &self,
    ) -> Option<&crate::behavior::PositionDecision> {
        self.position_decision.as_ref()
    }

    pub(in crate::session) fn execution_urgency(
        &self,
    ) -> &crate::plans::urgency::risk::RiskUrgencyAssessment {
        &self.execution_urgency
    }

    pub(in crate::session) const fn updates_working_quotes(&self) -> bool {
        self.updates_working_quotes
    }

    pub(in crate::session) const fn uses_parent_order_execution(&self) -> bool {
        self.uses_parent_order_execution
    }
}

/// 纯 NPC 决策结果只含 prospective strategy states 与 raw intents。
///
/// 不持有事件、router、订单簿、envelope 或会话句柄。
#[derive(Clone, Debug)]
pub(in crate::session) struct NpcDecisionSourceOutput {
    account_ids: Vec<AccountId>,
    accounts: Vec<NpcDecisionAccountOutput>,
    intents: Vec<NpcDecisionIntent>,
}

impl NpcDecisionSourceOutput {
    pub(in crate::session) fn accounts(&self) -> &[AccountId] {
        &self.account_ids
    }

    pub(in crate::session) fn intents(&self) -> &[NpcDecisionIntent] {
        &self.intents
    }

    pub(in crate::session) fn account_outputs(&self) -> &[NpcDecisionAccountOutput] {
        &self.accounts
    }

    pub(in crate::session) fn projection_parts(
        &mut self,
    ) -> (&mut [NpcDecisionAccountOutput], &[NpcDecisionIntent]) {
        (&mut self.accounts, &self.intents)
    }

    #[cfg(test)]
    pub(in crate::session) fn strategy_state(
        &self,
        account: AccountId,
    ) -> Result<&StrategyState, NpcDecisionSourceError> {
        self.accounts
            .iter()
            .find(|output| output.account == account)
            .and_then(NpcDecisionAccountOutput::strategy_state)
            .ok_or(NpcDecisionSourceError::MissingOutputState(account))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(in crate::session) enum NpcDecisionSourceError {
    #[error("DecisionAndContinuation NPC 账户 {account:?} 的 risk urgency 失败：{source}")]
    RiskUrgency {
        account: AccountId,
        source: crate::plans::UrgencyError,
    },
    #[error("DecisionAndContinuation NPC 来源快照错误：{0}")]
    Snapshot(DecisionSnapshotError),
    #[error("DecisionAndContinuation NPC 来源无法恢复账户 {account:?} 的 strategy：{source}")]
    StrategyHydration {
        account: AccountId,
        source: StrategyStateError,
    },
    #[error("DecisionAndContinuation NPC 来源账户 {account:?} 的 intent ordinal 溢出")]
    IntentOrdinalOverflow { account: AccountId },
    #[error("DecisionAndContinuation NPC 来源账户 {0:?} 缺少输出 strategy state")]
    #[cfg(test)]
    MissingOutputState(AccountId),
}

pub(in crate::session) fn npc_rng_seed(base: u64, tick: u64, account: AccountId) -> u64 {
    base ^ tick.wrapping_mul(0x9E3779B97F4A7C15) ^ account.0.wrapping_mul(0x6A09E667F3BCC908)
}

pub(in crate::session) struct NpcAccountDecision {
    #[cfg(test)]
    pub(in crate::session) account: AccountId,
    pub(in crate::session) output: NpcDecisionSourceOutput,
}

#[derive(Debug)]
pub(in crate::session) enum NpcDecisionStreamError<E> {
    Decision(NpcDecisionSourceError),
    Consumer(E),
}

pub(in crate::session) fn decide_npc_account(
    snapshot: &DecisionSnapshot,
    config: &crate::GameConfig,
    account: AccountId,
) -> Result<NpcDecisionSourceOutput, NpcDecisionSourceError> {
    let input = snapshot
        .account(account)
        .map_err(NpcDecisionSourceError::Snapshot)?;
    decide_npc_account_from_shared(&snapshot.shared_inputs(), input, config, account)
}

/// T1·两阶段（ADR-0042 §T1）：decide 核心——共享输入（借用视图）+ 单账户
/// 密封输入。账户任务（queue 链）与单账户快照路径共用同一实现；RNG 由
/// `npc_seed_base/tick/account` 派生，跨路径一致。
pub(in crate::session) fn decide_npc_account_from_shared(
    shared: &super::decision_snapshot::DecisionSharedInputs<'_>,
    input: &DecisionAccountInput,
    config: &crate::GameConfig,
    account: AccountId,
) -> Result<NpcDecisionSourceOutput, NpcDecisionSourceError> {
    let mut strategy = input
        .strategy_state()
        .clone()
        .into_strategy()
        .map_err(|source| NpcDecisionSourceError::StrategyHydration { account, source })?;
    let npc_seed = npc_rng_seed(shared.npc_seed_base, shared.tick, account);
    let mut rng = SplitMix64::new(npc_seed);
    let dated_experience = input.failure_influence().map(|influence| {
        let mut experience = input
            .retail_experience()
            .expect("dated failure influence requires retail experience")
            .clone();
        experience.consecutive_failed_buys = influence;
        experience
    });
    let experience = dated_experience
        .as_ref()
        .or_else(|| input.retail_experience());
    let mut decision = strategy.decide_with_experience(
        shared.market,
        input.self_view(),
        (input.kind() == crate::AccountKind::Retail)
            .then(|| shared.behavior_market)
            .flatten(),
        (input.kind() == crate::AccountKind::Retail)
            .then(|| input.account_risk())
            .flatten(),
        experience,
        shared.market_minute,
        &mut rng,
        config,
    );
    if let Some(analysis) = input.retail_analysis() {
        let StrategyState::ZiNoise(retail) = input.strategy_state() else {
            panic!("retail analysis requires the retail execution strategy for {account:?}");
        };
        retail.apply_personal_analysis(
            &mut decision,
            analysis,
            shared.market,
            input.self_view(),
            input
                .account_risk()
                .expect("retail analysis requires account risk"),
            experience.expect("retail analysis requires personal experience"),
            shared.market_minute,
            &mut rng,
            config,
        );
    }
    let execution_urgency = crate::plans::urgency::risk::assess_personal_risk_urgency(
        decision.position_decision.as_ref(),
        input.account_risk(),
        shared.urgency_policy,
    )
    .map_err(|source| NpcDecisionSourceError::RiskUrgency { account, source })?;
    let strategy_state = StrategyState::from_strategy(strategy.as_ref())
        .map_err(|source| NpcDecisionSourceError::StrategyHydration { account, source })?;
    let unchanged = &strategy_state == input.strategy_state();
    let StrategyDecision {
        intents: local_intents,
        reviewed_stocks,
        position_decision,
    } = decision;
    let updates_working_quotes = !local_intents.is_empty()
        || !reviewed_stocks.is_empty()
        || strategy.updates_working_quotes_on_empty_decision();
    let uses_parent_order_execution = strategy.uses_parent_order_execution();
    let mut intents = Vec::with_capacity(local_intents.len());
    for (npc_local_index, intent) in local_intents.into_iter().enumerate() {
        let npc_local_index = u64::try_from(npc_local_index)
            .map_err(|_| NpcDecisionSourceError::IntentOrdinalOverflow { account })?;
        intents.push(NpcDecisionIntent {
            key: IntentCandidateKey::npc(account, npc_local_index),
            intent,
        });
    }
    let account_output = NpcDecisionAccountOutput {
        account,
        #[cfg(test)]
        strategy_state: Some(strategy_state.clone()),
        strategy: Some(if unchanged {
            NpcStrategyUpdate::Unchanged
        } else {
            NpcStrategyUpdate::Replace(StoredStrategy::production_validated(
                strategy,
                strategy_state,
            ))
        }),
        reviewed_stocks,
        position_decision,
        execution_urgency,
        updates_working_quotes,
        uses_parent_order_execution,
    };
    Ok(NpcDecisionSourceOutput {
        account_ids: vec![account],
        accounts: vec![account_output],
        intents,
    })
}

/// 测试/验证钩子（缺口② §3.1④）：spawned 任务在 send 成功之后、guard Drop
/// 之前调用——用于确定性钉住「send 已发生、Drop 未发生」窗口（d2）与
/// spawn/inline 分计探针（d3/d4）。阻塞回调会钉住该任务的 guard 持有期。
#[cfg(any(test, feature = "verification-harness"))]
static DECIDE_POST_SEND_HOLD: std::sync::Mutex<
    Option<std::sync::Arc<dyn Fn(AccountId) + Send + Sync>>,
> = std::sync::Mutex::new(None);

/// 测试/验证钩子：命中账户的 decide 任务在计算前 panic（p1/p2 原 payload
/// 重抛验收；payload 为 &'static str 便于 downcast 判等）。
#[cfg(any(test, feature = "verification-harness"))]
static DECIDE_PANIC_ACCOUNTS: std::sync::Mutex<std::collections::BTreeSet<AccountId>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

/// 安装 send 后 guard 前的 hold 钩子（仅测试/验证构建）。
#[cfg(any(test, feature = "verification-harness"))]
pub(in crate::session) fn set_decide_post_send_hold(
    hold: Option<std::sync::Arc<dyn Fn(AccountId) + Send + Sync>>,
) {
    *DECIDE_POST_SEND_HOLD.lock().expect("hold 钩子锁被 poison") = hold;
}

/// 配置 decide panic 注入账户集合（仅测试/验证构建）。
#[cfg(any(test, feature = "verification-harness"))]
pub(in crate::session) fn set_decide_panic_accounts(accounts: &[AccountId]) {
    *DECIDE_PANIC_ACCOUNTS
        .lock()
        .expect("panic 注入锁被 poison") = accounts.iter().copied().collect();
}

pub(in crate::session) fn stream_npc_decisions<E>(
    snapshot: Arc<DecisionSnapshot>,
    config: &crate::GameConfig,
    compute: &std::sync::Arc<crate::compute::ComputeContext>,
    receive: impl FnMut(NpcAccountDecision) -> Result<(), E> + Send,
) -> Result<(), NpcDecisionStreamError<E>>
where
    E: Send,
{
    stream_npc_decisions_with(snapshot, config, compute, |_| {}, receive)
}

/// decide 任务经会话上下文进入其池；消费与投影在调用线程（不新建 consumer 线程）。
/// 缺口② burst 配额化（设计 v4 §3.1①）：burst 协调者按位置分类——
/// 池内经 `try_enter_pool_drive_slot` 登记 drives（A 类）；被拒降 C 类全内联；
/// 池外（日界链 tick 线程）走池外路径。逐项任务 `reserve_spawn_slot` 预留后
/// 无条件 spawn，`SpawnSlotGuard` 随任务终结（成功或 unwind）析构归还容量。
/// R1-a：在飞==0 且 pending 非空时绝不等待结果——池内内联执行一项自有 pending，
/// 池外 `wait_for_spawn_capacity`（顶层编排合法）。R1-c：先取现成结果再尽力派发
///（循环顶 (d0) 非阻塞收尽 → (a) 尽力派发；确定性见证=
/// `npc_decisions_quota_tests::r1c_decide_consumes_ready_result_before_dispatching_more`）。
/// panic 终结协议（P-a/P-b/P-c，v4 处方固定）：spawned 任务 unwind 时 catch 仅
/// 运输原 panic payload（不恢复不转换，ADR-0017:117 边界内），绑定释放后发送
/// TaskDied；协调者收到即停止派发（pending 冻结）、排空其余在飞、最后重抛原
/// payload。原始 sender 持有至排空完成（Disconnected 仅作冗余断言）。
pub(super) fn stream_npc_decisions_with<E, B, R>(
    snapshot: Arc<DecisionSnapshot>,
    config: &crate::GameConfig,
    compute: &std::sync::Arc<crate::compute::ComputeContext>,
    before_decide: B,
    receive: R,
) -> Result<(), NpcDecisionStreamError<E>>
where
    E: Send,
    B: Fn(AccountId) + Sync + Send,
    R: FnMut(NpcAccountDecision) -> Result<(), E> + Send,
{
    let mut receive = receive;
    stream_npc_account_tasks_with(
        Arc::new(snapshot.due_npc_ids().to_vec()),
        compute,
        before_decide,
        {
            let snapshot = snapshot.clone();
            move |account| decide_npc_account(&snapshot, config, account)
        },
        move |account, result, errors: &mut Vec<(AccountId, NpcDecisionStreamError<_>)>| {
            match result {
                Ok(output) => {
                    if let Err(error) = receive(NpcAccountDecision {
                        #[cfg(test)]
                        account,
                        output,
                    }) {
                        errors.push((account, NpcDecisionStreamError::Consumer(error)));
                    }
                }
                Err(error) => errors.push((account, NpcDecisionStreamError::Decision(error))),
            }
        },
    )
}

/// T1·两阶段审计（ADR-0042 §T1）：账户任务流骨架泛化——按**到期集合**派发，
/// per-account 工作由 `work` 闭包给出（decide-only 或完整账户计算
/// attention→观察→分析→decide→投影），quota/TaskDied/R1-x/测试探针协议
/// 骨架与 decide 流完全同源；消费经 `merge` 回调（协调线程纯归并，无
/// install 往返）。决策/消费错误聚合语义与旧 stream_npc_decisions_with
/// 完全一致（到达序首错）。
pub(super) fn stream_npc_account_tasks_with<T, FE, E, B, W, M>(
    due_accounts: Arc<Vec<AccountId>>,
    compute: &std::sync::Arc<crate::compute::ComputeContext>,
    before_task: B,
    work: W,
    mut merge: M,
) -> Result<(), NpcDecisionStreamError<E>>
where
    T: Send,
    FE: Send,
    E: Send,
    B: Fn(AccountId) + Sync + Send,
    W: Fn(AccountId) -> Result<T, FE> + Sync + Send,
    M: FnMut(AccountId, Result<T, FE>, &mut Vec<(AccountId, NpcDecisionStreamError<E>)>) + Send,
{
    #[cfg(target_arch = "wasm32")]
    {
        let mut errors = Vec::new();
        for account in due_accounts.iter().copied() {
            before_task(account);
            record_stream_error_generic(account, work(account), &mut merge, &mut errors);
        }
        return select_stream_error(errors);
    }

    /// spawned decide 任务的消息：成功载荷，或 unwind 的原 panic payload（TaskDied）。
    enum DecideMessage<T, FE> {
        Payload(AccountId, Result<T, FE>),
        TaskDied(Box<dyn std::any::Any + Send>),
    }

    /// 协调者位置：池内（登记 drive 成功，A 类）或池外（非 worker 线程顶层编排）。
    // drive 槽必须在函数级持有（match 臂内绑定会在臂结束即 Drop——A 类登记
    // 形同虚设的缺陷由 d2 确定性台账探针暴露并修复；函数级 `_drive_slot`
    // 存续到 return，池外协调者不占 drive 槽（设计 §2.2/§4.3）。
    let (pooled, _drive_slot) = if compute.is_worker() {
        match compute.try_enter_pool_drive_slot() {
            Some(slot) => (true, Some(slot)),
            // C 类被拒：全内联退化（逐账户恰一次、共用同一 Arc、错误聚合、资源域
            // 四项语义维持——设计 §1.2）。
            None => {
                let mut errors = Vec::new();
                for account in due_accounts.iter().copied() {
                    before_task(account);
                    record_stream_error_generic(account, work(account), &mut merge, &mut errors);
                }
                return select_stream_error(errors);
            }
        }
    } else {
        (false, None)
    };

    compute.in_place_scope(|scope| {
        // 方向A+B·任务粗化→宽度封顶（宽度税治理）：到期账户按 chunk 连续
        // 区间打包派发——chunk 内串行执行、逐账户 send/merge；粒度对齐
        // active_width（方向B：实际参与宽度≤池宽=部署上限）。
        // 契约保持（ADR-0042 §T1/§9.4）：每账户恰一次计算（chunk 区间不相交）、
        // 逐账户 payload 消息与归并到达序 receipt（ADR-0032）、panic 运输原
        // payload（TaskDied 后本 chunk 余下账户不再计算——与旧形状 pending
        // 冻结等价：整 tick 失败）、消费错误冻结纪律逐账户判定。
        let chunk_size = crate::compute::parallel_grain(due_accounts.len(), compute.active_width());
        let due_len = due_accounts.len();
        let pending: std::collections::VecDeque<std::ops::Range<usize>> = (0..due_len)
            .step_by(chunk_size)
            .map(|start| start..(start + chunk_size).min(due_len))
            .collect();
        let (sender, receiver) = mpsc::channel::<DecideMessage<T, FE>>();
        // 原始 sender 以 Option 承载：TaskDied 排空路径需要在排空前提前 Drop
        //（见下方排空块），正常路径在流末 Drop（P-c）。
        let mut origin_sender = Some(sender);
        let mut pending = pending;
        let mut in_flight = 0usize;
        let mut errors = Vec::new();
        let mut task_died: Option<Box<dyn std::any::Any + Send>> = None;
        // 消费错误路径（设计 §3.1①）：merge 出错 → 停止派发（含 R1-a 内联）→
        // 排空在飞 → 聚合错误返回；与 panic 路径合流于 P-b。
        let mut consumer_failed = false;
        let mut merge = merge;
        let _ = &merge;

        loop {
            // (d0) R1-c 消费优先：先**非阻塞**收尽现成结果，再尽力派发。
            // 现成=try_recv 立即可得（区别于 (d) 的合法阻塞等待 R1-b）；
            // 处理纪律与 (d) 完全同一（payload→merge / TaskDied→冻结派发）。
            // 断连（Disconnected）不在此时报告——留给 (d) 的阻塞路径按
            // 「NPC worker 未交回结果」原语义显式失败。
            while in_flight > 0 {
                let message = match receiver.try_recv() {
                    Ok(message) => message,
                    Err(mpsc::TryRecvError::Empty) | Err(mpsc::TryRecvError::Disconnected) => {
                        break
                    }
                };
                in_flight -= 1;
                match message {
                    DecideMessage::Payload(account, result) => {
                        // 工作量探针：spawned 任务的载荷消息计 1（cfg(test)，按本上下文隔离）。
                        #[cfg(test)]
                        compute.t1_probes.stream_payloads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let before = errors.len();
                        record_stream_error_generic(account, result, &mut merge, &mut errors);
                        if errors.len() > before
                            && matches!(errors.last(), Some((_, NpcDecisionStreamError::Consumer(_))))
                        {
                            consumer_failed = true;
                        }
                    }
                    DecideMessage::TaskDied(payload) => {
                        // P-b：立即停止派发（pending 冻结），排空其余在飞，最后重抛。
                        if task_died.is_none() {
                            task_died = Some(payload);
                        }
                    }
                }
            }
            if task_died.is_some() {
                // P-b 排空（chunk 化口径修正）：in_flight 按账户消息预记，
                // panic 中断的 chunk 余下账户不再发消息 ⇒ 计数上界不可靠，
                // 不能作为排空完成判据。改为：先 Drop 协调者自己的原始
                // sender（派发已冻结，不再需要），再阻塞收至 Disconnected
                //——全部 spawned 任务终结（sender 克隆全 Drop）且缓冲消息
                // 先于断连可见，即排空完成。payload 仍按到达序归并。
                drop(origin_sender.take().expect("TaskDied 排空只发生一次"));
                loop {
                    match receiver.recv() {
                        Ok(DecideMessage::Payload(account, result)) => {
                            #[cfg(test)]
                            compute.t1_probes.stream_payloads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            let before = errors.len();
                            record_stream_error_generic(account, result, &mut merge, &mut errors);
                            if errors.len() > before
                                && matches!(errors.last(), Some((_, NpcDecisionStreamError::Consumer(_))))
                            {
                                consumer_failed = true;
                            }
                        }
                        Ok(DecideMessage::TaskDied(_)) => {
                            // 首例已登记；后续终结信号仅确认排空进度。
                        }
                        Err(mpsc::RecvError) => break,
                    }
                }
                break;
            }
            if task_died.is_none() && !consumer_failed {
                // (a) 尽力派发：reserve 成功才 spawn；失败留 pending 绝不 spawn。
                // 派发门（测试探针）在 reserve 后、spawn 前调用（与股票派发同位）。
                while let Some(range) = pending.front().cloned() {
                    let Some(guard) = compute.reserve_spawn_slot() else {
                        break;
                    };
                    pending.pop_front();
                    #[cfg(any(test, feature = "verification-harness"))]
                    compute.run_dispatch_gate();
                    let sender = origin_sender.as_ref().expect("派发阶段原始 sender 必须仍在").clone();
                    let before_task = &before_task;
                    let work = &work;
                    let timing = crate::verification_evidence::current_phase_timing_binding();
                    #[cfg(any(test, feature = "verification-harness"))]
                    let executor = crate::session::pipeline::current_executor_binding();
                    // in_flight 按**账户消息**计（每账户恰一条 Payload/TaskDied），
                    // 不按 chunk 计——(d0)/(d) 的递减与终止判定都以消息为口径。
                    let chunk_accounts = range.end - range.start;
                    let due_accounts = due_accounts.clone();
                    scope.spawn(move |_| {
                        // 工作量探针：spawned 任务（chunk）计 1（cfg(test)，按本
                        // 上下文隔离）——任务数=min(due,workers) 不随池宽放大。
                        #[cfg(test)]
                        compute.t1_probes.stream_spawned.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        // 两段式：决策在继承绑定内完成；结果发送移到绑定释放后。
                        // P-a：unwind 路径 catch 仅运输原 panic payload（不恢复不转换），
                        // 绑定释放（catch 返回即 with_phase_timing_binding 作用域退出）
                        // 之后发送 TaskDied；guard 随任务结束析构归还容量并广播。
                        // chunk 内逐账户串行；任一账户 panic ⇒ TaskDied 后终止本
                        // chunk 余下账户（与旧形状的 pending 冻结等价：整 tick 失败）。
                        for account in due_accounts[range].iter().copied() {
                            // 绑定句柄随迭代克隆（原单账户闭包按值捕获）。
                            let timing = timing.clone();
                            #[cfg(any(test, feature = "verification-harness"))]
                            let executor = executor.clone();
                            let bound = move || {
                                // 注入点必须在 catch_unwind 运输区内：裸 panic（区外）
                                // 不产生 TaskDied 终结信号，协调者将无限排空（p1 红）。
                                #[cfg(any(test, feature = "verification-harness"))]
                                if DECIDE_PANIC_ACCOUNTS
                                    .lock()
                                    .expect("panic 注入锁被 poison")
                                    .contains(&account)
                                {
                                    panic!("decide quota test panic {account:?}");
                                }
                                before_task(account);
                                (account, work(account))
                            };
                            let outcome = std::panic::catch_unwind(
                                std::panic::AssertUnwindSafe(move || {
                                    crate::verification_evidence::with_phase_timing_binding(
                                        timing,
                                        move || {
                                            #[cfg(any(test, feature = "verification-harness"))]
                                            {
                                                crate::session::pipeline::with_executor_binding(
                                                    executor, bound,
                                                )
                                            }
                                            #[cfg(not(any(test, feature = "verification-harness")))]
                                            {
                                                bound()
                                            }
                                        },
                                    )
                                }),
                            );
                            match outcome {
                                Ok((account, result)) => {
                                    let _ = sender.send(DecideMessage::Payload(account, result));
                                    // d2 钉住窗口：send 已发生、guard（容量）未归还。
                                    #[cfg(any(test, feature = "verification-harness"))]
                                    if let Some(hold) = DECIDE_POST_SEND_HOLD
                                        .lock()
                                        .expect("hold 钩子锁被 poison")
                                        .clone()
                                    {
                                        hold(account);
                                    }
                                }
                                Err(panic_payload) => {
                                    let _ = sender.send(DecideMessage::TaskDied(panic_payload));
                                    break;
                                }
                            }
                        }
                        drop(guard);
                    });
                    in_flight += chunk_accounts;
                }
            }
            // (b) 终止：pending 空 && 在飞 0（或 panic 排空完毕，pending 冻结）。
            if in_flight == 0 && (pending.is_empty() || task_died.is_some() || consumer_failed) {
                break;
            }
            if in_flight == 0 {
                // (c) R1-a 零在飞推进：绝不等待结果。
                if pooled {
                    let range = pending.pop_front().expect("R1-a 内联项来自非空 pending");
                    // 零在飞 ⇒ 通道内无未消费现成结果（(d0) 已收尽），整 chunk
                    // 内联不违背 R1-c 消费优先；逐账户判定冻结纪律。
                    for account in due_accounts[range].iter().copied() {
                        // 工作量探针：R1-a 内联执行逐账户计 1（cfg(test)，按本
                        // 上下文隔离）——payloads + inline == due 恰一次交付不变式。
                        #[cfg(test)]
                        compute.t1_probes.stream_inline.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        before_task(account);
                        // 增量十八 P2（Astra 终审）：内联消费与 spawned 消费同一失败
                        // 纪律——merge 出错即冻结（consumer_failed），不得继续内联/派发。
                        let before = errors.len();
                        record_stream_error_generic(account, work(account), &mut merge, &mut errors);
                        if errors.len() > before
                            && matches!(errors.last(), Some((_, NpcDecisionStreamError::Consumer(_))))
                        {
                            consumer_failed = true;
                            break;
                        }
                    }
                } else {
                    compute.wait_for_spawn_capacity();
                }
                continue;
            }
            // (d) 在飞消费一个结果（R1-b：等待合法，不依赖容量恢复）。
            let message = if compute.is_worker() {
                loop {
                    match receiver.try_recv() {
                        Ok(message) => break message,
                        Err(mpsc::TryRecvError::Empty) => crate::compute::yield_work(),
                        Err(mpsc::TryRecvError::Disconnected) => {
                            panic!("NPC worker 未交回结果")
                        }
                    }
                }
            } else {
                receiver.recv().expect("所有 NPC worker 必须交回结果")
            };
            in_flight -= 1;
            match message {
                DecideMessage::Payload(account, result) => {
                    // 工作量探针：spawned 任务的载荷消息计 1（cfg(test)，按本上下文隔离）。
                    #[cfg(test)]
                    compute.t1_probes.stream_payloads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let before = errors.len();
                    record_stream_error_generic(account, result, &mut merge, &mut errors);
                    if errors.len() > before
                        && matches!(errors.last(), Some((_, NpcDecisionStreamError::Consumer(_))))
                    {
                        consumer_failed = true;
                    }
                }
                DecideMessage::TaskDied(payload) => {
                    // P-b：立即停止派发（pending 冻结），排空其余在飞，最后重抛。
                    if task_died.is_none() {
                        task_died = Some(payload);
                    }
                }
            }
        }
        // P-c：原始 sender 持有至排空完成再 Drop（TaskDied 路径已在排空前
        // Drop——见上方排空块；正常路径在此 Drop）；此后 Disconnected 可作
        // 冗余断言。
        drop(origin_sender);
        if let Some(payload) = task_died {
            std::panic::resume_unwind(payload);
        }
        select_stream_error(errors)
    })
}

fn stream_npc_decisions_inline<E>(
    snapshot: Arc<DecisionSnapshot>,
    config: &crate::GameConfig,
    before_decide: impl Fn(AccountId),
    mut receive: impl FnMut(NpcAccountDecision) -> Result<(), E>,
) -> Result<(), NpcDecisionStreamError<E>> {
    let mut errors = Vec::new();
    for account in snapshot.due_npc_ids().iter().copied() {
        before_decide(account);
        match decide_npc_account(&snapshot, config, account) {
            Ok(output) => {
                if let Err(error) = receive(NpcAccountDecision {
                    #[cfg(test)]
                    account,
                    output,
                }) {
                    errors.push((account, NpcDecisionStreamError::Consumer(error)));
                }
            }
            Err(error) => errors.push((account, NpcDecisionStreamError::Decision(error))),
        }
    }
    select_stream_error(errors)
}

/// 泛化错误记账（T1）：merge 回调返回 Err ⇒ Consumer 错误；任务结果 Err ⇒
/// 由 merge 侧自行转换（decide 适配器把决策错误并入同一聚合——见
/// decide_merge_adapter）。到达序首错语义与旧 record_stream_result 一致。
fn record_stream_error_generic<T, FE, E, M>(
    account: AccountId,
    result: Result<T, FE>,
    merge: &mut M,
    errors: &mut Vec<(AccountId, NpcDecisionStreamError<E>)>,
) where
    M: FnMut(AccountId, Result<T, FE>, &mut Vec<(AccountId, NpcDecisionStreamError<E>)>),
{
    merge(account, result, errors);
}

fn select_stream_error<E>(
    errors: Vec<(AccountId, NpcDecisionStreamError<E>)>,
) -> Result<(), NpcDecisionStreamError<E>> {
    // 用户已授权放宽：不强求固定赢家/固定首错。任一真实错误均可报告，
    // 到达序即报告序（单线程下保持迭代序，多线程下由调度决定）。
    match errors.into_iter().next() {
        Some((_, error)) => Err(error),
        None => Ok(()),
    }
}

/// 测试适配器：并行评估密封的 NPC 策略，且不修改权威状态。
///
/// 每账户在同一并行任务中恢复策略并计算决策；结果按账户规范顺序聚合，
/// 因此错误选择稳定，且局部结果不会提前逃逸。每账户 RNG 不依赖调度顺序。
#[cfg(test)]
pub(in crate::session) fn run_npc_decisions(
    snapshot: Arc<DecisionSnapshot>,
    config: &crate::GameConfig,
) -> Result<NpcDecisionSourceOutput, NpcDecisionSourceError> {
    let mut outputs = Vec::with_capacity(snapshot.due_npc_ids().len());
    let compute =
        crate::compute::ComputeContext::shared_default().expect("测试适配器默认上下文初始化失败");
    stream_npc_decisions(snapshot, config, &compute, |result| {
        outputs.push(result.output);
        Ok::<_, std::convert::Infallible>(())
    })
    .map_err(|error| match error {
        NpcDecisionStreamError::Decision(error) => error,
        NpcDecisionStreamError::Consumer(never) => match never {},
    })?;
    outputs.sort_by_key(|output| output.accounts[0].account());
    let mut accounts = Vec::with_capacity(outputs.len());
    let mut intents = Vec::new();
    for mut output in outputs {
        accounts.append(&mut output.accounts);
        intents.append(&mut output.intents);
    }
    let account_ids = accounts
        .iter()
        .map(NpcDecisionAccountOutput::account)
        .collect();
    Ok(NpcDecisionSourceOutput {
        account_ids,
        accounts,
        intents,
    })
}
