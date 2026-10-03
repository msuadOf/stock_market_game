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
    facts: Vec<CausalFact>,
    decision: std::collections::BTreeMap<AccountId, u64>,
    pub termination: Option<Termination>,
    filled_values: std::collections::BTreeMap<OrderId, i64>,
}

impl CausalCollector {
    pub(crate) fn facts(&self) -> &[CausalFact] {
        &self.facts
    }

    pub(crate) fn decision_for(&self, account: AccountId) -> Option<u64> {
        self.decision.get(&account).copied()
    }

    pub(crate) fn record_plan_root(
        &mut self,
        time: FactTime,
        account: AccountId,
        facts: impl IntoIterator<Item = CausalFactKind>,
    ) {
        self.decision.insert(account, self.facts.len() as u64);
        for fact in facts {
            self.record(time, fact);
        }
    }

    pub(crate) fn record_continuous_fill(
        &mut self,
        time: FactTime,
        order: OrderId,
        account: AccountId,
        code: &StockCode,
        qty: u32,
        gross: i64,
    ) {
        let value_before = *self.filled_values.entry(order).or_insert(0);
        self.record(
            time,
            CausalFactKind::Filled {
                order,
                account,
                code: code.clone(),
                qty,
                value_before,
                gross,
            },
        );
        if let Some(value) = value_before.checked_add(gross) {
            self.filled_values.insert(order, value);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_auction_fill(
        &mut self,
        time: FactTime,
        order: OrderId,
        account: AccountId,
        code: &StockCode,
        qty: u32,
        value_before: i64,
        value_after: i64,
    ) -> Result<(), &'static str> {
        let gross = value_after
            .checked_sub(value_before)
            .ok_or("auction causal fill value regressed")?;
        let tracked = self.filled_values.entry(order).or_insert(0);
        if *tracked != value_before {
            return Err("auction causal fill value disagrees with the receipt chain");
        }
        *tracked = value_after;
        self.record(
            time,
            CausalFactKind::Filled {
                order,
                account,
                code: code.clone(),
                qty,
                value_before,
                gross,
            },
        );
        Ok(())
    }

    pub fn record(&mut self, time: FactTime, kind: CausalFactKind) {
        self.facts.push(CausalFact {
            sequence: self.facts.len() as u64,
            time,
            kind,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time() -> FactTime {
        FactTime {
            phase: crate::TradingPhase::Continuous,
            market_minute: 1,
            civil: CivilInstant::new(crate::CivilDate::from_ymd(2030, 1, 1).unwrap(), 1).unwrap(),
        }
    }

    #[test]
    fn collector_decision_batch_keeps_empty_and_non_decision_acceptance() {
        let mut collector = CausalCollector::default();
        collector.record_plan_root(time(), AccountId(1), []);
        assert_eq!(collector.decision_for(AccountId(1)), Some(0));
        assert!(collector.facts().is_empty());
        collector.record_plan_root(time(), AccountId(1), [CausalFactKind::ObservationRestart]);
        assert_eq!(collector.decision_for(AccountId(1)), Some(0));
        assert!(matches!(
            collector.facts()[0].kind,
            CausalFactKind::ObservationRestart
        ));
        collector.record_plan_root(
            time(),
            AccountId(2),
            [CausalFactKind::Decision {
                account: AccountId(2),
            }],
        );
        assert_eq!(collector.decision_for(AccountId(2)), Some(1));
        assert_eq!(collector.facts()[1].sequence, 1);
    }

    #[test]
    fn collector_continuous_overflow_appends_without_advancing_value() {
        let mut collector = CausalCollector::default();
        let order = OrderId(1);
        let code = StockCode("600101".to_owned());
        collector.record_continuous_fill(time(), order, AccountId(1), &code, 100, i64::MAX);
        collector.record_continuous_fill(time(), order, AccountId(1), &code, 100, 1);
        assert_eq!(collector.filled_values[&order], i64::MAX);
        assert_eq!(collector.facts().len(), 2);
        assert!(matches!(
            collector.facts()[1].kind,
            CausalFactKind::Filled {
                value_before: i64::MAX,
                gross: 1,
                ..
            }
        ));
    }

    #[test]
    fn collector_auction_arithmetic_failure_precedes_zero_entry_and_chain_check() {
        let mut collector = CausalCollector::default();
        let order = OrderId(1);
        let code = StockCode("600101".to_owned());
        assert_eq!(
            collector.record_auction_fill(
                time(),
                order,
                AccountId(1),
                &code,
                100,
                i64::MIN,
                i64::MAX
            ),
            Err("auction causal fill value regressed")
        );
        assert!(!collector.filled_values.contains_key(&order));
        assert!(collector.facts().is_empty());
    }

    #[test]
    fn collector_auction_chain_failure_keeps_entry_zero_without_appending() {
        let mut collector = CausalCollector::default();
        let order = OrderId(1);
        let code = StockCode("600101".to_owned());
        assert_eq!(
            collector.record_auction_fill(time(), order, AccountId(1), &code, 100, 1, 2),
            Err("auction causal fill value disagrees with the receipt chain")
        );
        assert_eq!(collector.filled_values[&order], 0);
        assert!(collector.facts().is_empty());
        collector
            .record_auction_fill(time(), order, AccountId(1), &code, 100, 0, 10)
            .unwrap();
        assert_eq!(collector.filled_values[&order], 10);
        assert_eq!(collector.facts().len(), 1);
        assert!(matches!(
            collector.facts()[0].kind,
            CausalFactKind::Filled {
                value_before: 0,
                gross: 10,
                ..
            }
        ));
    }
}
