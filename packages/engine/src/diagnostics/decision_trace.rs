use super::shared_index::SharedDiagnosticIndex;
use serde::ser::{SerializeMap, SerializeSeq};
use std::collections::VecDeque;
use std::sync::Arc;

use crate::{AccountId, OrderId, PlanId, StockId};

pub const MAX_NPC_DECISION_TRACE_RECORDS: usize = 128;

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct NpcDecisionTraceRecord {
    pub account: AccountId,
    #[serde(serialize_with = "crate::diagnostics::serialize_u64_decimal")]
    pub tick: u64,
    pub source_report_ids: Vec<String>,
    pub expectation_method: Option<String>,
    pub plan_ids: Vec<PlanId>,
    pub plan_changes: Vec<String>,
    pub budget_constraints: Vec<String>,
    pub order_ids: Vec<OrderId>,
    pub codes: Vec<StockId>,
}

impl Clone for NpcDecisionTraceRecord {
    fn clone(&self) -> Self {
        #[cfg(test)]
        TRACE_RECORD_CLONES.with(|count| count.set(count.get() + 1));
        Self {
            account: self.account,
            tick: self.tick,
            source_report_ids: self.source_report_ids.clone(),
            expectation_method: self.expectation_method.clone(),
            plan_ids: self.plan_ids.clone(),
            plan_changes: self.plan_changes.clone(),
            budget_constraints: self.budget_constraints.clone(),
            order_ids: self.order_ids.clone(),
            codes: self.codes.clone(),
        }
    }
}

#[cfg(test)]
thread_local! { static TRACE_RING_ARC_CLONES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[cfg(test)]
thread_local! { static TRACE_RECORD_CLONES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[derive(Clone, Default, serde::Serialize)]
pub(crate) struct NpcDecisionTraceCollector {
    #[serde(serialize_with = "serialize_records")]
    records: SharedDiagnosticIndex<Arc<VecDeque<Arc<NpcDecisionTraceRecord>>>>,
}

impl NpcDecisionTraceCollector {
    pub(crate) fn record(&mut self, record: NpcDecisionTraceRecord) {
        let account = record.account;
        let mut ring = match self.records.get(account.0) {
            Some(records) => Arc::clone(records),
            None => Arc::new(VecDeque::new()),
        };
        #[cfg(test)]
        TRACE_RING_ARC_CLONES.with(|count| {
            if Arc::strong_count(&ring) > 1 {
                count.set(count.get() + ring.len());
            }
        });
        let records = Arc::make_mut(&mut ring);
        if records.len() == MAX_NPC_DECISION_TRACE_RECORDS {
            let _ = records.pop_front();
        }
        records.push_back(Arc::new(record));
        self.records.insert(account.0, ring);
    }

    pub(crate) fn records(
        &self,
        account: AccountId,
    ) -> Option<&VecDeque<Arc<NpcDecisionTraceRecord>>> {
        self.records.get(account.0).map(Arc::as_ref)
    }

    pub(crate) fn enrich(
        &mut self,
        account: AccountId,
        tick: u64,
        orders: impl IntoIterator<Item = OrderId>,
        plans: impl IntoIterator<Item = PlanId>,
        changes: impl IntoIterator<Item = String>,
        constraints: impl IntoIterator<Item = String>,
    ) {
        let Some(existing) = self.records.get(account.0) else {
            return;
        };
        if !existing.back().is_some_and(|record| record.tick == tick) {
            return;
        }
        let mut ring = Arc::clone(existing);
        #[cfg(test)]
        TRACE_RING_ARC_CLONES.with(|count| {
            if Arc::strong_count(&ring) > 1 {
                count.set(count.get() + ring.len());
            }
        });
        let records = Arc::make_mut(&mut ring);
        let record = Arc::make_mut(records.back_mut().expect("已检查最后一条 trace"));
        for order in orders {
            if !record.order_ids.contains(&order) {
                record.order_ids.push(order);
            }
        }
        for plan in plans {
            if !record.plan_ids.contains(&plan) {
                record.plan_ids.push(plan);
            }
        }
        record.plan_changes.extend(changes);
        record.budget_constraints.extend(constraints);
        self.records.insert(account.0, ring);
    }
}

fn serialize_records<S: serde::Serializer>(
    records: &SharedDiagnosticIndex<Arc<VecDeque<Arc<NpcDecisionTraceRecord>>>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    struct Ring<'a>(&'a VecDeque<Arc<NpcDecisionTraceRecord>>);
    impl serde::Serialize for Ring<'_> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
            for record in self.0 {
                sequence.serialize_element(record.as_ref())?;
            }
            sequence.end()
        }
    }
    let mut map = serializer.serialize_map(Some(records.len()))?;
    records.visit_sorted(|key, value| map.serialize_entry(&AccountId(key), &Ring(value)))?;
    map.end()
}

#[cfg(test)]
mod tests {
    use super::{
        NpcDecisionTraceCollector, NpcDecisionTraceRecord, MAX_NPC_DECISION_TRACE_RECORDS,
    };
    use crate::AccountId;

    #[test]
    fn retains_the_latest_exactly_128_records() {
        let mut collector = NpcDecisionTraceCollector::default();
        for tick in 0..=MAX_NPC_DECISION_TRACE_RECORDS {
            collector.record(NpcDecisionTraceRecord {
                account: AccountId(1),
                tick: tick as u64,
                source_report_ids: Vec::new(),
                expectation_method: None,
                plan_ids: Vec::new(),
                plan_changes: Vec::new(),
                budget_constraints: Vec::new(),
                order_ids: Vec::new(),
                codes: Vec::new(),
            });
        }

        let records = collector.records(AccountId(1)).unwrap();
        assert_eq!(records.len(), MAX_NPC_DECISION_TRACE_RECORDS);
        assert_eq!(records.front().unwrap().tick, 1);
        assert_eq!(
            records.back().unwrap().tick,
            MAX_NPC_DECISION_TRACE_RECORDS as u64
        );
    }

    #[test]
    fn decision_trace_tick_serializes_without_u64_precision_loss() {
        let record = NpcDecisionTraceRecord {
            account: AccountId(1),
            tick: u64::MAX,
            source_report_ids: Vec::new(),
            expectation_method: None,
            plan_ids: Vec::new(),
            plan_changes: Vec::new(),
            budget_constraints: Vec::new(),
            order_ids: Vec::new(),
            codes: Vec::new(),
        };
        assert_eq!(
            serde_json::to_value(record).unwrap()["tick"],
            u64::MAX.to_string()
        );
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;
    fn record(account: u64, tick: u64) -> NpcDecisionTraceRecord {
        NpcDecisionTraceRecord {
            account: AccountId(account),
            tick,
            source_report_ids: vec![format!("report-{account}-{tick}")],
            expectation_method: Some("真实观察".to_owned()),
            plan_ids: vec![PlanId {
                owner: AccountId(account),
                sequence: 1,
            }],
            plan_changes: vec!["初始变化".into()],
            budget_constraints: vec!["初始约束".into()],
            order_ids: vec![OrderId(1)],
            codes: vec![StockId(0)],
        }
    }
    #[test]
    fn d0b_trace_nonempty_clone_and_single_account_cow_are_bounded() {
        let mut collector = NpcDecisionTraceCollector::default();
        for account in 0..33 {
            for tick in 0..129 {
                collector.record(record(account, tick));
            }
        }
        TRACE_RECORD_CLONES.with(|count| count.set(0));
        TRACE_RING_ARC_CLONES.with(|count| count.set(0));
        super::super::shared_index::counts::reset();
        let checkpoint = collector.clone();
        assert_eq!(TRACE_RECORD_CLONES.with(std::cell::Cell::get), 0);
        assert_eq!(
            super::super::shared_index::counts::get(),
            Default::default()
        );
        for account in 0..33 {
            assert!(std::ptr::eq(
                collector.records(AccountId(account)).unwrap(),
                checkpoint.records(AccountId(account)).unwrap()
            ));
        }
        collector.record(record(17, 129));
        assert_eq!(TRACE_RING_ARC_CLONES.with(std::cell::Cell::get), 128);
        assert_eq!(
            TRACE_RECORD_CLONES.with(std::cell::Cell::get),
            0,
            "push 不得深 clone 旧 Record"
        );
        let old = checkpoint.records(AccountId(17)).unwrap();
        let current = collector.records(AccountId(17)).unwrap();
        assert_eq!((old.len(), current.len()), (128, 128));
        assert_eq!(
            (old.front().unwrap().tick, current.front().unwrap().tick),
            (1, 2)
        );
        for (before, after) in old.iter().skip(1).zip(current.iter()) {
            assert!(Arc::ptr_eq(before, after));
        }
        for account in (0..33).filter(|account| *account != 17) {
            assert!(std::ptr::eq(
                collector.records(AccountId(account)).unwrap(),
                checkpoint.records(AccountId(account)).unwrap()
            ));
        }
        let before_enrich = collector.clone();
        collector.enrich(
            AccountId(17),
            129,
            [OrderId(2), OrderId(2), OrderId(1)],
            [
                PlanId {
                    owner: AccountId(17),
                    sequence: 2,
                },
                PlanId {
                    owner: AccountId(17),
                    sequence: 1,
                },
                PlanId {
                    owner: AccountId(17),
                    sequence: 2,
                },
            ],
            ["变化一".into(), "变化二".into()],
            ["约束一".into(), "约束二".into()],
        );
        assert_eq!(
            TRACE_RECORD_CLONES.with(std::cell::Cell::get),
            1,
            "enrich 只深 clone 最后一条 Record"
        );
        assert_eq!(TRACE_RING_ARC_CLONES.with(std::cell::Cell::get), 256);
        let enriched = collector.records(AccountId(17)).unwrap();
        for (before, after) in before_enrich
            .records(AccountId(17))
            .unwrap()
            .iter()
            .zip(enriched.iter())
            .take(127)
        {
            assert!(Arc::ptr_eq(before, after));
        }
        let last = enriched.back().unwrap();
        assert_eq!(last.order_ids, [OrderId(1), OrderId(2)]);
        assert_eq!(
            last.plan_ids,
            [
                PlanId {
                    owner: AccountId(17),
                    sequence: 1
                },
                PlanId {
                    owner: AccountId(17),
                    sequence: 2
                }
            ]
        );
        assert_eq!(last.plan_changes, ["初始变化", "变化一", "变化二"]);
        assert_eq!(last.budget_constraints, ["初始约束", "约束一", "约束二"]);
        let before = serde_json::to_vec(&collector).unwrap();
        let ring = Arc::clone(collector.records.get(17).unwrap());
        collector.enrich(AccountId(17), 128, [], [], [], []);
        collector.enrich(AccountId(99), 129, [], [], [], []);
        assert_eq!(TRACE_RECORD_CLONES.with(std::cell::Cell::get), 1);
        assert!(Arc::ptr_eq(&ring, collector.records.get(17).unwrap()));
        assert_eq!(serde_json::to_vec(&collector).unwrap(), before);
    }
    #[test]
    fn d0b_trace_checkpoint_restores_evicted_head_and_exact_enrichment() {
        let mut collector = NpcDecisionTraceCollector::default();
        for tick in 0..127 {
            collector.record(record(1, tick));
        }
        assert_eq!(collector.records(AccountId(1)).unwrap().len(), 127);
        collector.record(record(1, 127));
        collector.enrich(
            AccountId(1),
            127,
            [OrderId(2), OrderId(1), OrderId(2)],
            [
                PlanId {
                    owner: AccountId(1),
                    sequence: 2,
                },
                PlanId {
                    owner: AccountId(1),
                    sequence: 1,
                },
            ],
            ["变化一".into(), "变化二".into()],
            ["约束一".into(), "约束二".into()],
        );
        let checkpoint = collector.clone();
        let before = serde_json::to_vec(&collector).unwrap();
        collector.record(record(1, 128));
        collector.record(record(1, 129));
        assert_eq!(
            collector
                .records(AccountId(1))
                .unwrap()
                .front()
                .unwrap()
                .tick,
            2
        );
        collector = checkpoint;
        let restored = collector.records(AccountId(1)).unwrap();
        assert_eq!(restored.len(), 128);
        assert_eq!(restored.front().unwrap().tick, 0);
        assert_eq!(restored.back().unwrap().tick, 127);
        assert_eq!(serde_json::to_vec(&collector).unwrap(), before);
        let last = restored.back().unwrap();
        assert_eq!(last.order_ids, [OrderId(1), OrderId(2)]);
        assert_eq!(
            last.plan_ids,
            [
                PlanId {
                    owner: AccountId(1),
                    sequence: 1
                },
                PlanId {
                    owner: AccountId(1),
                    sequence: 2
                }
            ]
        );
        assert_eq!(last.plan_changes, ["初始变化", "变化一", "变化二"]);
        assert_eq!(last.budget_constraints, ["初始约束", "约束一", "约束二"]);
    }
}
