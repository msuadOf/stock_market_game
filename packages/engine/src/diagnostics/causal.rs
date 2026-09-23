use crate::company::CompanyId;
use crate::plans::PlanId;
use crate::{AccountId, CivilInstant, OrderId, Side, StockCode};
use serde::Serialize;

mod aggregate;
mod microstructure;
mod report;
pub use microstructure::{ImpactSample, RecoverySample};
pub use report::{CausalError, CausalReport, OrderLifecycle};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Termination {
    Voluntary,
    Reprice,
    Expired,
    DayEnd,
    MarketRemainder,
    Aborted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct FactTime {
    pub phase: crate::TradingPhase,
    pub market_minute: u64,
    pub civil: CivilInstant,
}

#[derive(Clone, Debug, Serialize)]
pub struct OrderOrigin {
    pub order: OrderId,
    pub account: AccountId,
    pub code: StockCode,
    pub company: Option<CompanyId>,
    pub plan: Option<PlanId>,
    pub decision: Option<u64>,
    pub side: Side,
    pub qty: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Quote {
    pub code: StockCode,
    pub bid_cents: Option<i64>,
    pub ask_cents: Option<i64>,
    pub bid_depth: u64,
    pub ask_depth: u64,
}

impl Quote {
    pub fn midpoint(&self) -> Option<f64> {
        match (self.bid_cents, self.ask_cents) {
            (Some(bid), Some(ask)) if bid > 0 && ask > bid => Some((bid as f64 + ask as f64) / 2.0),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub enum CausalFactKind {
    Submitted(OrderOrigin),
    Filled {
        order: OrderId,
        account: AccountId,
        code: StockCode,
        qty: u32,
        value_before: i64,
        gross: i64,
    },
    Terminated {
        order: OrderId,
        account: AccountId,
        code: StockCode,
        qty: u32,
        reason: Termination,
    },
    Quote(Quote),
    Execution {
        code: StockCode,
        maker: OrderId,
        taker: OrderId,
        side: Option<Side>,
        qty: u32,
        price_cents: i64,
        before: Quote,
    },
    ObservationRestart,
    Acquisition {
        account: AccountId,
        company: CompanyId,
        publication: u64,
        published: CivilInstant,
        acquired: CivilInstant,
    },
    Decision {
        account: AccountId,
    },
    Budget {
        account: AccountId,
        available_cents: i64,
        allocated_cents: Vec<i64>,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct CausalFact {
    pub sequence: u64,
    pub time: FactTime,
    pub kind: CausalFactKind,
}

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct CausalCollector {
    pub facts: Vec<CausalFact>,
    pub decision: std::collections::BTreeMap<AccountId, u64>,
    pub termination: Option<Termination>,
    pub filled_values: std::collections::BTreeMap<OrderId, i64>,
}

impl CausalCollector {
    pub fn record(&mut self, time: FactTime, kind: CausalFactKind) {
        self.facts.push(CausalFact {
            sequence: self.facts.len() as u64,
            time,
            kind,
        });
    }
}
