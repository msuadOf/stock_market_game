use super::plan_execution::{PlanExecutionProgress, PlanExecutionRoute, PlanRouteCommand};
use super::*;
use crate::plans::{CandidateAssessment, PlanEvent, PlanId, PlanRevision};
use std::collections::VecDeque;
mod adaptive;
#[cfg(test)]
mod consume;
pub(in crate::session) use adaptive::FrozenPlanChainObservation;
#[cfg(test)]
mod source_tests;

#[derive(Clone, Debug)]
pub(in crate::session) struct PlanChainCandidateBatch {
    pub(in crate::session) owner: AccountId,
    pub(in crate::session) intent: Intent,
    pub(in crate::session) chain_generation_index: u64,
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
                    price: *price,
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
    source: AccountSource,
    operations: VecDeque<PlanChainOperation>,
    candidate_source: PlanChainCandidateSource,
    pending_routes: BTreeMap<(AccountId, StockCode), (u64, Box<PlanExecutionRoute>)>,
    reports: Vec<PlanExecutionReport>,
}

enum AccountSource {
    Empty,
    Accounts {
        remaining: VecDeque<AccountId>,
        observations: ChainObservations,
    },
}

struct ChainObservations {
    market: MarketView,
    paths: BTreeMap<StockCode, crate::observation::PricePathObservation>,
    technical: BTreeMap<StockCode, crate::observation::TechnicalObservation>,
    now: crate::calendar::CivilInstant,
    exposed: BTreeSet<StockCode>,
}

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
        revision: PlanRevision,
    },
    AccountExecution {
        account: AccountId,
        market: MarketView,
    },
    Execute(PlanExecutionRequest),
}

impl PlanChainOperationBatch {
    pub(in crate::session) fn is_empty(&self) -> bool {
        self.operations.is_empty()
            && self.pending_routes.is_empty()
            && match &self.source {
                AccountSource::Empty => true,
                AccountSource::Accounts { remaining, .. } => remaining.is_empty(),
            }
    }

    pub(in crate::session) fn push_quote_plans(&mut self, cursor: QuotePlans) {
        self.operations
            .push_back(PlanChainOperation::QuotePlans(cursor));
    }
    pub(in crate::session) fn empty() -> Self {
        Self {
            source: AccountSource::Empty,
            operations: VecDeque::new(),
            candidate_source: PlanChainCandidateSource::default(),
            pending_routes: BTreeMap::new(),
            reports: Vec::new(),
        }
    }

    pub(in crate::session) fn accounts(
        accounts: Vec<AccountId>,
        market: MarketView,
        paths: BTreeMap<StockCode, crate::observation::PricePathObservation>,
        technical: BTreeMap<StockCode, crate::observation::TechnicalObservation>,
        now: crate::calendar::CivilInstant,
        exposed: BTreeSet<StockCode>,
    ) -> Self {
        if accounts.is_empty() {
            return Self::empty();
        }
        Self {
            source: AccountSource::Accounts {
                remaining: accounts.into(),
                observations: ChainObservations {
                    market,
                    paths,
                    technical,
                    now,
                    exposed,
                },
            },
            operations: VecDeque::new(),
            candidate_source: PlanChainCandidateSource::default(),
            pending_routes: BTreeMap::new(),
            reports: Vec::new(),
        }
    }

    pub(in crate::session) fn enumerate_candidate(
        &mut self,
        command: &PlanRouteCommand,
    ) -> Result<PlanChainCandidateBatch, PlanExecutionError> {
        self.candidate_source.enumerate(command)
    }

    pub(in crate::session) fn prepare_next_account(&mut self, session: &mut GameSession) {
        let AccountSource::Accounts {
            mut remaining,
            observations,
        } = std::mem::replace(&mut self.source, AccountSource::Empty)
        else {
            return;
        };
        let Some(account) = remaining.pop_front() else {
            return;
        };
        let mut plans = std::mem::take(&mut session.plans);
        session.run_chain_for_account(
            account,
            &observations.market,
            &observations.paths,
            &observations.technical,
            observations.now,
            &observations.exposed,
            &mut plans,
            self,
        );
        session.plans = plans;
        self.source = AccountSource::Accounts {
            remaining,
            observations,
        };
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
            revision,
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
