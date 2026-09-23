use super::*;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CausalError {
    #[error("duplicate or out-of-order fact sequence {0}")]
    Sequence(u64),
    #[error("duplicate submission {0:?}")]
    DuplicateOrder(OrderId),
    #[error("unknown or mismatched order {0:?}")]
    OrderMismatch(OrderId),
    #[error("duplicate or mismatched fill {0:?}")]
    FillMismatch(OrderId),
    #[error("quantity conservation failed for {0:?}")]
    Conservation(OrderId),
    #[error("diagnostic arithmetic overflow")]
    Overflow,
    #[error("time moved backwards at fact {0}")]
    Time(u64),
    #[error("execution does not reconcile with bilateral fills")]
    ExecutionMismatch,
    #[error("diagnostic observation restarted at restore; pre-restore origins are unavailable")]
    RestoredObservation,
    #[error("budget allocation exceeds available cash at fact {0}")]
    Budget(u64),
}

#[derive(Clone, Debug, Serialize)]
pub struct OrderLifecycle {
    pub origin: OrderOrigin,
    pub submitted_at: FactTime,
    pub source_sequence: u64,
    pub filled_qty: u64,
    pub canceled_qty: u64,
    pub aborted_qty: u64,
    pub open_qty: u64,
    pub terminal_reason: Option<Termination>,
    pub lifetime_market_minutes: Option<u64>,
    pub lifetime_civil_seconds: Option<i64>,
    pub censored_reason: Option<&'static str>,
    pub filled_value: i64,
}

#[derive(Debug, Serialize)]
pub struct CausalReport {
    pub seed: u64,
    pub submitted_qty: u64,
    pub filled_qty: u64,
    pub canceled_qty: u64,
    pub aborted_qty: u64,
    pub open_qty: u64,
    pub filled_submitted_ratio: Option<f64>,
    pub ratio_absent_reason: Option<&'static str>,
    pub orders: Vec<OrderLifecycle>,
    pub information_delays: Vec<(u64, i64)>,
    pub direction_persistence: Option<f64>,
    pub direction_absent_reason: Option<&'static str>,
    pub impacts: Vec<ImpactSample>,
    pub recoveries: Vec<RecoverySample>,
    pub observation_end: Option<FactTime>,
}
