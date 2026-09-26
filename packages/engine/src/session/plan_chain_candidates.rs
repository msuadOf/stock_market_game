use super::plan_execution::{PlanExecutionProgress, PlanExecutionRoute, PlanRouteCommand};
use super::*;
use crate::plans::{CandidateAssessment, PlanEvent, PlanId, PlanRevision};
use std::collections::VecDeque;
use std::sync::{mpsc, Arc};
mod adaptive;
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
    completion_sender: Option<mpsc::Sender<super::pipeline::TickWorkReady>>,
    operations: VecDeque<PlanChainOperation>,
    candidate_source: PlanChainCandidateSource,
    pending_routes: BTreeMap<(AccountId, StockCode), (u64, Box<PlanExecutionRoute>)>,
    reports: Vec<PlanExecutionReport>,
    reconsideration: BTreeMap<(AccountId, StockCode), (CandidateAssessment, MarketView)>,
    retry_market: BTreeMap<(AccountId, StockCode), MarketView>,
}

enum AccountSource {
    Empty,
    Accounts {
        remaining: VecDeque<AccountId>,
        observations: ChainObservations,
    },
    InFlight {
        receiver: mpsc::Receiver<PreparedRoot>,
        remaining: usize,
        snapshot: Arc<GameSession>,
    },
}

type PreparedRoot = (
    AccountId,
    super::decision_chain::personal_state::PlanPersonalState,
    PlanChainOperationBatch,
    super::decision_chain::PlanRootDiagnostics,
);

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
                AccountSource::InFlight { remaining, .. } => *remaining == 0,
            }
    }

    pub(in crate::session) fn push_quote_plans(&mut self, cursor: QuotePlans) {
        self.operations
            .push_back(PlanChainOperation::QuotePlans(cursor));
    }
    pub(in crate::session) fn empty() -> Self {
        Self {
            source: AccountSource::Empty,
            completion_sender: None,
            operations: VecDeque::new(),
            candidate_source: PlanChainCandidateSource::default(),
            pending_routes: BTreeMap::new(),
            reports: Vec::new(),
            reconsideration: BTreeMap::new(),
            retry_market: BTreeMap::new(),
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
            completion_sender: None,
            operations: VecDeque::new(),
            candidate_source: PlanChainCandidateSource::default(),
            pending_routes: BTreeMap::new(),
            reports: Vec::new(),
            reconsideration: BTreeMap::new(),
            retry_market: BTreeMap::new(),
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

    pub(in crate::session) fn enumerate_candidate(
        &mut self,
        command: &PlanRouteCommand,
    ) -> Result<PlanChainCandidateBatch, PlanExecutionError> {
        self.candidate_source.enumerate(command)
    }

    /// Start independent roots against one frozen tick view. Each completed root
    /// hands back only its personal state and operations; later trade projections
    /// keep ownership of the live PlanBook and parent orders.
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
        let snapshot = Arc::new(session.clone_for_plan_roots());
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
        for (account, mut personal) in inputs {
            let snapshot = Arc::clone(&snapshot);
            let observations = Arc::clone(&observations);
            let sender = sender.clone();
            let completion_sender = self.completion_sender.clone();
            rayon::spawn(move || {
                let mut generated = Self::empty();
                let diagnostics = snapshot.run_chain_for_account(
                    account,
                    &mut personal,
                    &observations.market,
                    &observations.paths,
                    &observations.technical,
                    observations.now,
                    &observations.exposed,
                    &snapshot.plans,
                    &mut generated,
                );
                // Publish the typed result before waking the coordinator. A
                // dropped receiver means the private tick was already discarded.
                if sender
                    .send((account, personal, generated, diagnostics))
                    .is_ok()
                {
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

    /// A nonblocking poll lets ready NPC/player requests enter while slower plan
    /// roots are still computing. A later blocking poll waits only when the
    /// transaction has no other ready work.
    pub(in crate::session) fn prepare_ready_accounts(
        &mut self,
        session: &mut GameSession,
        blocking: bool,
    ) -> Result<bool, StepFatal> {
        self.start_ready_accounts(session)?;
        let AccountSource::InFlight {
            receiver,
            remaining,
            snapshot,
        } = &mut self.source
        else {
            return Ok(false);
        };
        let result = loop {
            match receiver.try_recv() {
                Ok(result) => break Some(result),
                Err(mpsc::TryRecvError::Empty) if !blocking => break None,
                Err(mpsc::TryRecvError::Empty) => {
                    // The tick may itself occupy the only Rayon worker. Let that
                    // worker run pending roots instead of blocking on the channel.
                    if rayon::yield_now().is_none() {
                        std::thread::yield_now();
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(invariant("plan root worker ended without a result"));
                }
            }
        };
        let Some((account, personal, mut generated, diagnostics)) = result else {
            return Ok(false);
        };
        personal.install(session, account);
        #[cfg(feature = "simulation-diagnostics")]
        session.record_plan_root_diagnostics(account, diagnostics, &snapshot.plans);
        #[cfg(not(feature = "simulation-diagnostics"))]
        let _ = (diagnostics, snapshot);
        self.operations.append(&mut generated.operations);
        *remaining -= 1;
        if *remaining == 0 {
            self.source = AccountSource::Empty;
        }
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
