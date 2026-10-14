//! ADR-0042 §10：plan chain continuation 的窄写域视图（增量一百落地）。
//!
//! 终审判定（§10 引言）：同股 continuation 的计算此前全部经过持有
//! `&mut GameSession` 的共享协调者（`ReadyStockStream`→
//! `AdaptivePlanChainCoordinator`），不能宣称窄交接完成。本类型把 §10.1
//! 的写域清单落成**源码级 API 契约**：
//!
//! - continuation 计算入口（`advance_after_typed_outcomes`/
//!   `project_execution_round`/`next_ready_batch` 及其内部
//!   `yield/resume/install` 族）签名只接受本视图，不再接受
//!   `&mut GameSession`——「零全量借用」由类型系统见证（§10.2 验收钩子；
//!   见 `pipeline::plan_continuation_domain_tests` 的函数项类型钉）。
//! - 视图按 §10.1 逐域暴露白名单方法：本股 market（delta 跨越应用）、
//!   计划簿（`plans` 读 + resume/materialize/parents 写）、报价续行
//!   （quote 生成族）、收据/信封（母单回写与 NPC 生命周期簿）。
//!   **不在白名单上的会话能力（账户授权、跨股依赖序、收据汇合）没有
//!   对应入口**——按 §10.2 契约保留在协调者（`validate_available_ready`）。
//! - 构造点=驱动环 seam（`ReadyStockStream`/`ReadyIngress` 以 `scoped`
//!   短暂开域），域存活期即 continuation 写域窗口。
//!
//! 如实登记（§10.2 剩余缝，均不因本视图存在而宣称完成）：
//! - 缝①：本股 `apply_changed_orders` 随 delta 生产内联进 owner 收尾——
//!   本视图先以 `apply_market_delta` 收窄跨越点，内联迁移留后续批。
//! - 缝②：计划簿按账户分片——全簿 `std::mem::take` 往返仍存在（因
//!   `record_parent_order_*` 等 session 方法内部写 `state.plans`，簿必须
//!   留在 session 内；take/put 收敛在本视图方法内部、点位不多于迁移前）。
//! - 缝③：协调者多股容器随 ①② 同批退役，不单飞。

use super::plan_chain_candidates::{
    FrozenPlanChainObservation, PlanChainOperationBatch, QuotePlans,
};
use super::plan_execution::{
    PlanExecutionError, PlanExecutionProgress, PlanExecutionRequest, PlanExecutionRoute,
    PlanRouteOutcome, WorkingPlanOrder,
};
use super::StepFatal;
use crate::plans::{CandidateAssessment, PlanBook, PlanId};
use crate::session::decision_chain::PlanLifecycleAction;
use crate::session::OrderFillSettlement;
use crate::strategy::MarketView;
use crate::{AccountId, GameSession, Money, Order, OrderId, Side, StockId};
use std::collections::{BTreeMap, BTreeSet};

/// continuation 计算的窄写域视图：持有会话，但只暴露 §10.1 白名单操作。
pub(in crate::session) struct PlanContinuationDomain<'a> {
    session: &'a mut GameSession,
}

impl<'a> PlanContinuationDomain<'a> {
    /// 驱动环 seam 的开域入口：`continuation` 运行期间计划簿等写域只能
    /// 经视图触达；错误路径同样在视图丢弃后返回（无额外清理义务）。
    pub(in crate::session) fn scoped<T>(
        session: &'a mut GameSession,
        continuation: impl FnOnce(&mut PlanContinuationDomain<'_>) -> Result<T, StepFatal>,
    ) -> Result<T, StepFatal> {
        let mut domain = Self { session };
        continuation(&mut domain)
    }

    /// 直接开域（测试与驱动环内部使用；生产路径优先 `scoped`）。
    pub(in crate::session) fn new(session: &'a mut GameSession) -> Self {
        Self { session }
    }

    /// §10.1 域「本股 market」：owner 生产的 delta 在此跨越应用到权威簿。
    /// 错误文案与位置沿用迁移前 `pipeline::adaptive_plan_chain` 口径。
    pub(in crate::session) fn apply_market_delta(
        &mut self,
        code: &StockId,
        delta: crate::market::MarketDelta,
    ) -> Result<(), StepFatal> {
        let market = self
            .session
            .state
            .markets
            .get_mut(code)
            .ok_or_else(|| invariant("plan projection returned an unknown stock"))?;
        market
            .apply_changed_orders(delta)
            .map_err(|error| invariant(&error.to_string()))
    }

    /// §10.1 域「计划簿」：只读视图（resume/materialize 等写操作见下方
    /// 窄方法；按账户分片=缝②后续批）。
    pub(in crate::session) fn plans(&self) -> &PlanBook {
        &self.session.state.plans
    }

    /// §10.1 域「计划簿」：可变视图（Restructure 事件的 plan 读与 apply 写）。
    pub(in crate::session) fn plans_mut(&mut self) -> &mut PlanBook {
        &mut self.session.state.plans
    }

    /// §10.1 域「计划簿」：`route.resume`（按账户独占写）。
    pub(in crate::session) fn resume_route(
        &mut self,
        route: PlanExecutionRoute,
        outcome: PlanRouteOutcome,
    ) -> Result<PlanExecutionProgress, PlanExecutionError> {
        self.with_plan_book(|session, plans| route.resume(session, plans, outcome))
    }

    /// §10.1 域「计划簿」：受理的 Submit 母单安装（按账户独占写）。
    pub(in crate::session) fn install_accepted_route(
        &mut self,
        route: &PlanExecutionRoute,
        outcome: &PlanRouteOutcome,
    ) -> Result<(), PlanExecutionError> {
        self.with_plan_book(|session, plans| {
            route.install_accepted_submit_parent(session, plans, outcome)
        })
    }

    /// §10.1 域「报价续行」：progress 物化（读工作单/写计划簿）。
    pub(in crate::session) fn materialize_plan_progress(
        &mut self,
        progress: PlanExecutionProgress,
    ) -> Result<PlanExecutionProgress, PlanExecutionError> {
        self.with_plan_book(|session, plans| session.materialize_plan_progress(plans, progress))
    }

    /// §10.1 域「报价续行」：Execute 操作的执行观察。
    pub(in crate::session) fn prepare_plan_observation(
        &self,
        request: PlanExecutionRequest,
    ) -> Result<PlanExecutionProgress, PlanExecutionError> {
        self.session
            .prepare_plan_observation(&self.session.state.plans, request)
    }

    /// §10.1 域「报价续行」：下一计划报价（读计划簿与在途路由）。
    pub(in crate::session) fn generate_next_plan_quote(
        &self,
        cursor: &mut QuotePlans,
        unfinished_routes: &BTreeSet<(AccountId, StockId)>,
    ) -> Option<PlanExecutionRequest> {
        self.session
            .generate_next_plan_quote(cursor, &self.session.state.plans, unfinished_routes)
    }

    /// §10.1 域「报价续行」：生命周期动作收集（读侧）。
    pub(in crate::session) fn collect_plan_lifecycle_actions(
        &self,
        account: AccountId,
        assessments: &BTreeMap<StockId, CandidateAssessment>,
        market: &MarketView,
    ) -> Vec<PlanLifecycleAction> {
        self.session.collect_plan_lifecycle_actions(
            account,
            assessments,
            market,
            &self.session.state.plans,
        )
    }

    /// §10.1 域「计划簿」：生命周期动作应用（写侧）。
    pub(in crate::session) fn apply_plan_lifecycle_actions(
        &mut self,
        actions: Vec<PlanLifecycleAction>,
        market: &MarketView,
        operations: &mut PlanChainOperationBatch,
    ) {
        self.with_plan_book(|session, plans| {
            super::decision_chain::apply_plan_lifecycle_actions(
                actions, session, market, plans, operations,
            )
        })
    }

    /// §10.1 域「计划簿」：账户计划个人记忆修剪（读计划簿写个人状态）。
    pub(in crate::session) fn prune_plan_personal_memory(&mut self, account: AccountId) {
        self.with_plan_book(|session, plans| session.prune_plan_personal_memory(account, plans))
    }

    /// §10.1 域「报价续行」：账户分类报价游标构建。
    pub(in crate::session) fn prepare_classified_plan_quotes(
        &self,
        account: AccountId,
        market: &MarketView,
        existing_plan_ids: &BTreeSet<PlanId>,
    ) -> Option<QuotePlans> {
        self.session.prepare_classified_plan_quotes_for_account(
            account,
            market,
            &self.session.state.plans,
            existing_plan_ids,
        )
    }

    /// §10.1 域「报价续行」：计划工作单视图（读）。
    pub(in crate::session) fn plan_working_orders(
        &self,
        account: AccountId,
        code: &StockId,
    ) -> Vec<WorkingPlanOrder> {
        self.session.plan_working_orders(account, code)
    }

    /// §10.1 域「收据/信封」（tick 级共享）：母单受理回写。
    pub(in crate::session) fn record_parent_order_submission(
        &mut self,
        account: AccountId,
        code: &StockId,
        side: Side,
        order_id: OrderId,
        qty: u32,
    ) -> Result<(), crate::plans::PlanError> {
        self.session
            .record_parent_order_submission(account, code, side, order_id, qty)
    }

    /// §10.1 域「收据/信封」：母单撤销/释放回写。
    pub(in crate::session) fn record_parent_order_canceled(
        &mut self,
        account: AccountId,
        code: &StockId,
        order_id: OrderId,
    ) -> Result<(), crate::plans::PlanError> {
        self.session
            .record_parent_order_canceled(account, code, order_id)
    }

    /// §10.1 域「收据/信封」：母单成交回写。
    pub(in crate::session) fn record_parent_order_fills(
        &mut self,
        code: &StockId,
        fills: &[OrderFillSettlement],
    ) -> Result<(), crate::plans::PlanError> {
        self.session.record_parent_order_fills(code, fills)
    }

    /// §10.1 域「收据/信封」：受理时点报价登记（NPC 生命周期簿）。
    pub(in crate::session) fn register_npc_order_lifecycle_at_quote(
        &mut self,
        account: AccountId,
        code: &StockId,
        order: &Order,
        last_price: Money,
        best_bid: Option<Money>,
        best_ask: Option<Money>,
    ) {
        self.session.register_npc_order_lifecycle_at_quote(
            account, code, order, last_price, best_bid, best_ask,
        )
    }

    /// §10.1 域「收据/信封」：生命周期簿移除。
    pub(in crate::session) fn remove_npc_order_lifecycle(
        &mut self,
        account: AccountId,
        code: &StockId,
        order_id: OrderId,
    ) {
        self.session
            .remove_npc_order_lifecycle(account, code, order_id)
    }

    /// 冻结观察容器交换（yield 期报价/生命周期观察重用同一组容器）：
    /// action 只拿到本视图——观察期间 continuation 仍不接触 `&mut GameSession`。
    pub(in crate::session) fn observe_frozen<T>(
        &mut self,
        observation: &mut FrozenPlanChainObservation,
        action: impl FnOnce(&mut Self) -> T,
    ) -> T {
        observation.swap_session_containers(self.session);
        let result = action(self);
        observation.swap_session_containers(self.session);
        result
    }

    /// 计划簿暂借执行（迁移前 `std::mem::take` 点位的等价收拢；错误路径
    /// 同样归还）。缝②（按账户分片消除全簿往返）落地前的过渡载体。
    fn with_plan_book<T>(
        &mut self,
        action: impl FnOnce(&mut GameSession, &mut PlanBook) -> T,
    ) -> T {
        let mut plans = std::mem::take(&mut self.session.state.plans);
        let result = action(self.session, &mut plans);
        self.session.state.plans = plans;
        result
    }

    /// diagnostics（simulation-diagnostics）：NPC 决策轨迹增补——报价约束面
    /// （AccountExecution 观察位）。
    #[cfg(feature = "simulation-diagnostics")]
    pub(in crate::session) fn enrich_npc_decision_trace_constraints(
        &mut self,
        account: AccountId,
        constraints: impl IntoIterator<Item = String>,
    ) {
        let tick = self.session.state.tick;
        self.session
            .state
            .npc_decision_traces
            .enrich(account, tick, [], [], [], constraints);
    }

    /// diagnostics（simulation-diagnostics）：NPC 决策轨迹增补——trace 单面
    /// （resume 观察位）。
    #[cfg(feature = "simulation-diagnostics")]
    pub(in crate::session) fn enrich_npc_decision_trace_order(
        &mut self,
        account: AccountId,
        trace_order: Option<OrderId>,
    ) {
        let tick = self.session.state.tick;
        self.session
            .state
            .npc_decision_traces
            .enrich(account, tick, trace_order, [], [], []);
    }

    /// diagnostics（simulation-diagnostics）：计划轨迹变更登记。
    #[cfg(feature = "simulation-diagnostics")]
    pub(in crate::session) fn record_plan_trace_changes(
        &mut self,
        account: AccountId,
        before: &BTreeMap<PlanId, String>,
    ) {
        self.with_plan_book(|session, plans| {
            session.record_plan_trace_changes(account, before, plans)
        })
    }

    /// diagnostics（simulation-diagnostics）：因果事实记录。
    #[cfg(feature = "simulation-diagnostics")]
    pub(in crate::session) fn causal_record(
        &mut self,
        kind: crate::diagnostics::causal::CausalFactKind,
    ) {
        self.session.causal_record(kind);
    }

    /// diagnostics（simulation-diagnostics）：带报价的提交事实。
    #[cfg(feature = "simulation-diagnostics")]
    pub(in crate::session) fn causal_submitted_with_quote(
        &mut self,
        account: AccountId,
        order_id: OrderId,
        code: &StockId,
        side: Side,
        qty: u32,
        quote: crate::diagnostics::causal::Quote,
    ) {
        self.session
            .causal_submitted_with_quote(account, order_id, code, side, qty, quote);
    }

    /// diagnostics（simulation-diagnostics）：终结事实。
    #[cfg(feature = "simulation-diagnostics")]
    pub(in crate::session) fn causal_terminated(
        &mut self,
        order: (AccountId, OrderId, u32),
        code: &StockId,
        reason: crate::diagnostics::causal::Termination,
    ) {
        self.session.causal_terminated(order, code, reason);
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        // 与迁移前 `pipeline::adaptive_plan_chain` 口径保持一致（错误等价）。
        location: "pipeline::adaptive_plan_chain".to_owned(),
    }
}
