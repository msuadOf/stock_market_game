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
    pub sellable: BTreeMap<StockCode, u32>,
    pub one_minute_bp: BTreeMap<StockCode, Option<i32>>,
    pub thirty_minute_bp: BTreeMap<StockCode, Option<i32>>,
}

pub(in crate::session) struct PlanChainOperationBatch {
    roots: PlanRootCoordinator,
    operations: VecDeque<PlanChainOperation>,
    candidate_source: PlanChainCandidateSource,
    routes: StockRouteCoordination,
    reports: Vec<PlanExecutionReport>,
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
    pending: BTreeMap<(AccountId, StockCode), (u64, Box<PlanExecutionRoute>)>,
    reconsideration: BTreeMap<(AccountId, StockCode), (CandidateAssessment, MarketView)>,
    retry_market: BTreeMap<(AccountId, StockCode), MarketView>,
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
        resource: &(AccountId, StockCode),
        unfinished: &BTreeSet<(AccountId, StockCode)>,
    ) -> bool {
        self.pending.contains_key(resource) || unfinished.contains(resource)
    }
    fn install_pending(
        &mut self,
        resource: (AccountId, StockCode),
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
        resource: &(AccountId, StockCode),
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
        resource: &(AccountId, StockCode),
        generation: u64,
    ) -> Result<Box<PlanExecutionRoute>, StepFatal> {
        self.pending_for_outcome(resource, generation)?;
        Ok(self
            .pending
            .remove(resource)
            .expect("checked pending route exists")
            .1)
    }
    fn remember_retry(&mut self, resource: (AccountId, StockCode), market: MarketView) {
        self.retry_market.insert(resource, market);
    }
    fn remember_reconsideration(
        &mut self,
        resource: (AccountId, StockCode),
        assessment: CandidateAssessment,
        market: MarketView,
    ) {
        self.reconsideration.insert(resource, (assessment, market));
    }
    fn take_follow_up(
        &mut self,
        (account, code): (AccountId, StockCode),
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

enum AccountSource {
    Empty,
    Accounts {
        remaining: VecDeque<AccountId>,
        observations: DecisionChainObservation,
    },
    InFlight {
        receiver: mpsc::Receiver<PreparedRoot>,
        remaining: usize,
        snapshot: Arc<RootReadContext>,
    },
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
        assessments: BTreeMap<StockCode, CandidateAssessment>,
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
    completion_sender: Option<mpsc::Sender<super::pipeline::TickWorkReady>>,
}

impl PlanRootCoordinator {
    fn empty() -> Self {
        Self {
            source: AccountSource::Empty,
            completion_sender: None,
        }
    }
    fn is_empty(&self) -> bool {
        match &self.source {
            AccountSource::Empty => true,
            AccountSource::Accounts { remaining, .. } => remaining.is_empty(),
            AccountSource::InFlight { remaining, .. } => *remaining == 0,
        }
    }
    fn accounts(accounts: Vec<AccountId>, observations: DecisionChainObservation) -> Self {
        Self {
            source: AccountSource::Accounts {
                remaining: accounts.into(),
                observations,
            },
            completion_sender: None,
        }
    }
    pub(in crate::session) fn set_completion_sender(
        &mut self,
        sender: mpsc::Sender<super::pipeline::TickWorkReady>,
    ) -> Result<(), StepFatal> {
        if matches!(self.source, AccountSource::InFlight { .. }) {
            return Err(invariant(
                "plan root notification must be attached before workers start",
            ));
        }
        self.completion_sender = Some(sender);
        Ok(())
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
        let snapshot = Arc::new(RootReadContext::capture(session)?);
        let observations = Arc::new(observations);
        let inputs = remaining
            .into_iter()
            .map(|account| {
                (
                    account,
                    super::decision_chain::personal_state::PlanPersonalState::take(
                        session, account,
                    ),
                )
            })
            .collect::<Vec<_>>();
        let remaining = inputs.len();
        let (sender, receiver) = mpsc::channel();
        for (account, personal) in inputs {
            let snapshot = Arc::clone(&snapshot);
            let observations = Arc::clone(&observations);
            let sender = sender.clone();
            let completion_sender = self.completion_sender.clone();
            rayon::spawn(move || {
                let result = InstitutionDecisionRoot::new(account, personal)
                    .observe(&snapshot, &observations);
                // 先发布 typed 结果再唤醒 coordinator；receiver 已关闭表示私有 tick 已丢弃。
                if sender.send(result).is_ok() {
                    if let Some(sender) = completion_sender {
                        let _ = sender.send(super::pipeline::TickWorkReady::PlanRoot);
                    }
                }
            });
        }
        self.source = AccountSource::InFlight {
            receiver,
            remaining,
            snapshot,
        };
        Ok(())
    }
    pub(in crate::session) fn prepare_ready_accounts(
        &mut self,
        session: &mut GameSession,
        blocking: bool,
    ) -> Result<Option<PlanChainOperationBatch>, StepFatal> {
        self.start_ready_accounts(session)?;
        let AccountSource::InFlight {
            receiver,
            remaining,
            snapshot,
        } = &mut self.source
        else {
            return Ok(None);
        };
        let result = loop {
            match receiver.try_recv() {
                Ok(result) => break Some(result),
                Err(mpsc::TryRecvError::Empty) if !blocking => break None,
                Err(mpsc::TryRecvError::Empty) => {
                    // tick 可能占据唯一 Rayon worker；等待期间让该 worker 执行 root，避免自锁。
                    if rayon::yield_now().is_none() {
                        std::thread::yield_now();
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(invariant("plan root worker ended without a result"));
                }
            }
        };
        let Some((account, personal, generated, diagnostics)) = result else {
            return Ok(None);
        };
        personal.install(session, account);
        #[cfg(feature = "simulation-diagnostics")]
        session.record_plan_root_diagnostics(account, diagnostics, &snapshot.plans);
        #[cfg(not(feature = "simulation-diagnostics"))]
        let _ = (diagnostics, snapshot);
        *remaining -= 1;
        if *remaining == 0 {
            self.source = AccountSource::Empty;
        }
        Ok(Some(generated))
    }
}

impl PlanChainOperationBatch {
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
        }
    }

    #[cfg(test)]
    pub(in crate::session) fn accounts(
        accounts: Vec<AccountId>,
        market: MarketView,
        paths: BTreeMap<StockCode, crate::observation::PricePathObservation>,
        technical: BTreeMap<StockCode, crate::observation::TechnicalObservation>,
        now: crate::calendar::CivilInstant,
        exposed: BTreeSet<StockCode>,
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

    pub(in crate::session) fn set_completion_sender(
        &mut self,
        sender: mpsc::Sender<super::pipeline::TickWorkReady>,
    ) -> Result<(), StepFatal> {
        self.roots.set_completion_sender(sender)
    }

    pub(in crate::session) fn enumerate_candidate(
        &mut self,
        command: &PlanRouteCommand,
    ) -> Result<PlanChainCandidateBatch, PlanExecutionError> {
        self.candidate_source.enumerate(command)
    }

    /// 汇总已经完成的 root；仅在事务没有其他就绪工作时等待，保持后续执行操作次序。
    pub(in crate::session) fn prepare_ready_accounts(
        &mut self,
        session: &mut GameSession,
        blocking: bool,
    ) -> Result<bool, StepFatal> {
        let Some(mut generated) = self.roots.prepare_ready_accounts(session, blocking)? else {
            return Ok(false);
        };
        self.operations.append(&mut generated.operations);
        Ok(true)
    }

    pub(in crate::session) fn push_account_execution(
        &mut self,
        account: AccountId,
        market: MarketView,
    ) {
        self.operations
            .push_back(PlanChainOperation::AccountExecution { account, market });
    }

    pub(in crate::session) fn push_lifecycle(
        &mut self,
        account: AccountId,
        assessments: BTreeMap<StockCode, CandidateAssessment>,
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
        _code: StockCode,
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
