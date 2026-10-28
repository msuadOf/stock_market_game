use crate::company::CompanyId;
use crate::plans::PlanId;
use crate::{AccountId, CivilInstant, OrderId, Side, StockId};
use serde::Serialize;
use std::sync::OnceLock;

use super::shared_index::SharedDiagnosticIndex;
use crate::shared_history::AppendOnlyHistory;
use serde::ser::SerializeMap;

mod aggregate;
mod decimal;
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
    #[serde(serialize_with = "crate::diagnostics::serialize_u64_decimal")]
    pub market_minute: u64,
    pub civil: CivilInstant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OrderOrigin {
    pub order: OrderId,
    pub account: AccountId,
    pub code: StockId,
    pub company: Option<CompanyId>,
    pub plan: Option<PlanId>,
    #[serde(serialize_with = "decimal::optional")]
    pub decision: Option<u64>,
    pub side: Side,
    pub qty: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Quote {
    pub code: StockId,
    #[serde(serialize_with = "crate::money::cents_decimal::optional")]
    pub bid_cents: Option<i64>,
    #[serde(serialize_with = "crate::money::cents_decimal::optional")]
    pub ask_cents: Option<i64>,
    #[serde(serialize_with = "crate::diagnostics::serialize_u64_decimal")]
    pub bid_depth: u64,
    #[serde(serialize_with = "crate::diagnostics::serialize_u64_decimal")]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum CausalFactKind {
    Submitted(OrderOrigin),
    Filled {
        order: OrderId,
        account: AccountId,
        code: StockId,
        qty: u32,
        #[serde(serialize_with = "crate::money::cents_decimal::serialize")]
        value_before: i64,
        #[serde(serialize_with = "crate::money::cents_decimal::serialize")]
        gross: i64,
    },
    Terminated {
        order: OrderId,
        account: AccountId,
        code: StockId,
        qty: u32,
        reason: Termination,
    },
    Quote(Quote),
    Execution {
        code: StockId,
        /// 卖腿订单（ADR-0042 腿方向语义）。
        maker: OrderId,
        /// 买腿订单。
        taker: OrderId,
        side: Option<Side>,
        qty: u32,
        #[serde(serialize_with = "crate::money::cents_decimal::serialize")]
        price_cents: i64,
        before: Quote,
    },
    ObservationRestart,
    Acquisition {
        account: AccountId,
        company: CompanyId,
        #[serde(serialize_with = "crate::diagnostics::serialize_u64_decimal")]
        publication: u64,
        published: CivilInstant,
        acquired: CivilInstant,
    },
    Decision {
        account: AccountId,
    },
    Budget {
        account: AccountId,
        #[serde(serialize_with = "crate::money::cents_decimal::serialize")]
        available_cents: i64,
        #[serde(serialize_with = "crate::money::cents_decimal::list")]
        allocated_cents: Vec<i64>,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct CausalFact {
    #[serde(serialize_with = "crate::diagnostics::serialize_u64_decimal")]
    pub sequence: u64,
    pub time: FactTime,
    pub kind: CausalFactKind,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct CausalCollector {
    facts: AppendOnlyHistory<CausalFact>,
    /// 仅显式全量查询构建；checkpoint 不携带这份副本。
    #[serde(skip)]
    flat_facts: OnceLock<Vec<CausalFact>>,
    #[serde(serialize_with = "serialize_decisions")]
    decision: SharedDiagnosticIndex<u64>,
    pub termination: Option<Termination>,
    #[serde(serialize_with = "serialize_filled_values")]
    filled_values: SharedDiagnosticIndex<i64>,
}

impl Clone for CausalCollector {
    fn clone(&self) -> Self {
        Self {
            facts: self.facts.clone(),
            flat_facts: OnceLock::new(),
            decision: self.decision.clone(),
            termination: self.termination,
            filled_values: self.filled_values.clone(),
        }
    }
}

impl CausalCollector {
    pub(crate) fn fact_count(&self) -> usize {
        self.facts.len()
    }

    pub(crate) fn facts(&self) -> &[CausalFact] {
        self.flat_facts
            .get_or_init(|| self.facts.iter().cloned().collect())
    }

    pub(crate) fn decision_for(&self, account: AccountId) -> Option<u64> {
        self.decision.get(account.0).copied()
    }

    pub(crate) fn record_plan_root(
        &mut self,
        time: FactTime,
        account: AccountId,
        facts: impl IntoIterator<Item = CausalFactKind>,
    ) {
        self.decision.insert(account.0, self.facts.len() as u64);
        for fact in facts {
            self.record(time, fact);
        }
    }

    pub(crate) fn record_continuous_fill(
        &mut self,
        time: FactTime,
        order: OrderId,
        account: AccountId,
        code: &StockId,
        qty: u32,
        gross: i64,
    ) {
        let value_before = match self.filled_values.get(order.0) {
            Some(value) => *value,
            None => {
                self.filled_values.insert(order.0, 0);
                0
            }
        };
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
            self.filled_values.insert(order.0, value);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_auction_fill(
        &mut self,
        time: FactTime,
        order: OrderId,
        account: AccountId,
        code: &StockId,
        qty: u32,
        value_before: i64,
        value_after: i64,
    ) -> Result<(), &'static str> {
        let gross = value_after
            .checked_sub(value_before)
            .ok_or("auction causal fill value regressed")?;
        let tracked = match self.filled_values.get(order.0) {
            Some(value) => *value,
            None => {
                self.filled_values.insert(order.0, 0);
                0
            }
        };
        if tracked != value_before {
            return Err("auction causal fill value disagrees with the receipt chain");
        }
        self.filled_values.insert(order.0, value_after);
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
        // 回收显式全量查询的副本可能为 O(history)；普通 append 不构建副本。
        self.flat_facts.take();
        self.facts.push(CausalFact {
            sequence: self.facts.len() as u64,
            time,
            kind,
        });
    }
}

fn serialize_decisions<S: serde::Serializer>(
    values: &SharedDiagnosticIndex<u64>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(values.len()))?;
    values.visit_sorted(|key, value| map.serialize_entry(&AccountId(key), value))?;
    map.end()
}

fn serialize_filled_values<S: serde::Serializer>(
    values: &SharedDiagnosticIndex<i64>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(values.len()))?;
    values.visit_sorted(|key, value| {
        map.serialize_entry(&OrderId(key), &crate::Money::from_cents(*value))
    })?;
    map.end()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collector_checkpoint_shares_three_chunks_and_isolates_later_appends() {
        let mut collector = CausalCollector::default();
        for _ in 0..101 {
            collector.record(time(), CausalFactKind::ObservationRestart);
        }
        let checkpoint = collector.clone();
        for index in [0, 31, 32, 63, 64, 95, 100] {
            assert!(
                std::ptr::eq(&collector.facts[index], &checkpoint.facts[index]),
                "checkpoint 应共享已有事实，不能复制历史：index={index}"
            );
        }
        for _ in 0..67 {
            collector.record(time(), CausalFactKind::ObservationRestart);
        }
        assert_eq!(checkpoint.facts().len(), 101);
        assert_eq!(collector.facts().len(), 168);
        for index in [0, 31, 32, 63, 64, 95] {
            assert!(std::ptr::eq(
                &collector.facts[index],
                &checkpoint.facts[index]
            ));
        }
        collector = checkpoint;
        collector.record(time(), CausalFactKind::ObservationRestart);
        assert_eq!(collector.facts().len(), 102);
        assert_eq!(collector.facts()[101].sequence, 101);
    }

    #[test]
    fn collector_full_query_cache_is_lazy_not_cloned_and_preserves_serialization() {
        let mut collector = CausalCollector::default();
        for _ in 0..101 {
            collector.record(time(), CausalFactKind::ObservationRestart);
        }
        assert!(collector.flat_facts.get().is_none());
        let before = serde_json::to_value(&collector).unwrap();
        assert!(
            collector.flat_facts.get().is_none(),
            "serde 直接流式读取共享历史"
        );
        let first = collector.facts().as_ptr();
        assert_eq!(collector.facts().as_ptr(), first);
        let checkpoint = collector.clone();
        assert!(checkpoint.flat_facts.get().is_none());
        assert_eq!(before, serde_json::to_value(&checkpoint).unwrap());
        assert_eq!(before, serde_json::to_value(&collector).unwrap());
        collector.record(time(), CausalFactKind::ObservationRestart);
        assert!(collector.flat_facts.get().is_none());
        assert_eq!(collector.facts().len(), 102);
        assert_eq!(checkpoint.facts().len(), 101);
        assert_eq!(before, serde_json::to_value(&checkpoint).unwrap());
    }

    #[test]
    fn collector_filled_values_serialize_as_exact_decimal_cents() {
        let mut collector = CausalCollector::default();
        let code = StockId(1);
        collector.record_continuous_fill(time(), OrderId(1), AccountId(1), &code, 100, i64::MAX);
        let value = serde_json::to_value(collector).unwrap();
        assert_eq!(value["filled_values"]["1"], i64::MAX.to_string());
        assert_eq!(
            value["facts"][0]["kind"]["Filled"]["gross"],
            i64::MAX.to_string()
        );
    }

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
        let code = StockId(1);
        collector.record_continuous_fill(time(), order, AccountId(1), &code, 100, i64::MAX);
        collector.record_continuous_fill(time(), order, AccountId(1), &code, 100, 1);
        assert_eq!(*collector.filled_values.get(order.0).unwrap(), i64::MAX);
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
        let code = StockId(1);
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
        assert!(collector.filled_values.get(order.0).is_none());
        assert!(collector.facts().is_empty());
    }

    #[test]
    fn collector_auction_chain_failure_keeps_entry_zero_without_appending() {
        let mut collector = CausalCollector::default();
        let order = OrderId(1);
        let code = StockId(1);
        assert_eq!(
            collector.record_auction_fill(time(), order, AccountId(1), &code, 100, 1, 2),
            Err("auction causal fill value disagrees with the receipt chain")
        );
        assert_eq!(*collector.filled_values.get(order.0).unwrap(), 0);
        assert!(collector.facts().is_empty());
        collector
            .record_auction_fill(time(), order, AccountId(1), &code, 100, 0, 10)
            .unwrap();
        assert_eq!(*collector.filled_values.get(order.0).unwrap(), 10);
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

#[cfg(test)]
mod storage_tests {
    use super::*;
    fn time() -> FactTime {
        FactTime {
            phase: crate::TradingPhase::Continuous,
            market_minute: 1,
            civil: CivilInstant::new(crate::CivilDate::from_ymd(2030, 1, 1).unwrap(), 1).unwrap(),
        }
    }
    #[test]
    fn d0b_nonempty_causal_clone_shares_both_maps_and_preserves_old_values() {
        let mut collector = CausalCollector::default();
        let mut keys = vec![0, u32::MAX as u64, u64::MAX];
        keys.extend((1..255).map(|key| key << 32));
        for &key in &keys {
            collector.record_plan_root(
                time(),
                AccountId(key),
                [CausalFactKind::Decision {
                    account: AccountId(key),
                }],
            );
            collector.record_continuous_fill(
                time(),
                OrderId(key),
                AccountId(key),
                &StockId(0),
                100,
                99,
            );
        }
        super::super::shared_index::counts::reset();
        let checkpoint = collector.clone();
        assert_eq!(
            super::super::shared_index::counts::get(),
            Default::default()
        );
        for &key in &keys {
            assert!(std::ptr::eq(
                collector.decision.get(key).unwrap(),
                checkpoint.decision.get(key).unwrap()
            ));
            assert!(std::ptr::eq(
                collector.filled_values.get(key).unwrap(),
                checkpoint.filled_values.get(key).unwrap()
            ));
        }
        collector.record_plan_root(
            time(),
            AccountId(1 << 32),
            [CausalFactKind::Decision {
                account: AccountId(1 << 32),
            }],
        );
        collector.record_continuous_fill(
            time(),
            OrderId(1 << 32),
            AccountId(1 << 32),
            &StockId(0),
            100,
            11,
        );
        assert_eq!(checkpoint.decision_for(AccountId(1 << 32)), Some(6));
        assert_eq!(collector.decision_for(AccountId(1 << 32)), Some(514));
        assert_eq!(*checkpoint.filled_values.get(1 << 32).unwrap(), 99);
        assert_eq!(*collector.filled_values.get(1 << 32).unwrap(), 110);
        for &key in keys.iter().filter(|key| **key != 1 << 32) {
            assert!(std::ptr::eq(
                collector.decision.get(key).unwrap(),
                checkpoint.decision.get(key).unwrap()
            ));
            assert!(std::ptr::eq(
                collector.filled_values.get(key).unwrap(),
                checkpoint.filled_values.get(key).unwrap()
            ));
        }
        assert!(
            serde_json::to_vec(&collector).is_err(),
            "完整 u64 内部存储不能放宽 OrderId wire 范围"
        );
        assert_eq!(checkpoint.fact_count(), 514);
        assert_eq!(collector.fact_count(), 516);
    }
    #[test]
    fn d0b_typed_serializers_keep_numeric_key_order_and_decimal_money() {
        let mut collector = CausalCollector::default();
        let mut decisions = std::collections::BTreeMap::new();
        let mut filled = std::collections::BTreeMap::new();
        for (key, value) in [(1u64 << 40, i64::MAX), (10, i64::MIN), (2, -1), (0, 0)] {
            collector.decision.insert(key, key);
            decisions.insert(AccountId(key), key);
            collector.filled_values.insert(key, value);
            filled.insert(OrderId(key), value);
        }
        #[derive(serde::Serialize)]
        struct Legacy<'a> {
            facts: &'a AppendOnlyHistory<CausalFact>,
            decision: std::collections::BTreeMap<AccountId, u64>,
            termination: Option<Termination>,
            #[serde(serialize_with = "crate::money::cents_decimal::map")]
            filled_values: std::collections::BTreeMap<OrderId, i64>,
        }
        let expected = Legacy {
            facts: &collector.facts,
            decision: decisions,
            termination: None,
            filled_values: filled,
        };
        assert_eq!(
            serde_json::to_vec(&collector).unwrap(),
            serde_json::to_vec(&expected).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&collector).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        collector.filled_values.insert(u64::MAX, 1);
        assert_eq!(*collector.filled_values.get(u64::MAX).unwrap(), 1);
        assert!(serde_json::to_vec(&collector).is_err());
        let mut legacy_invalid = std::collections::BTreeMap::new();
        legacy_invalid.insert(OrderId(u64::MAX), 1);
        let invalid = Legacy {
            facts: &collector.facts,
            decision: Default::default(),
            termination: None,
            filled_values: legacy_invalid,
        };
        assert!(serde_json::to_vec(&invalid).is_err());
    }
}
