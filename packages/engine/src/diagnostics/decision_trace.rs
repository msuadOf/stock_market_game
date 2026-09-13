use std::collections::{BTreeMap, VecDeque};

use crate::{AccountId, OrderId, PlanId, StockCode};

pub const MAX_NPC_DECISION_TRACE_RECORDS: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct NpcDecisionTraceRecord {
    pub account: AccountId,
    pub tick: u64,
    pub source_report_ids: Vec<String>,
    pub expectation_method: Option<String>,
    pub plan_ids: Vec<PlanId>,
    pub budget_constraints: Vec<String>,
    pub order_ids: Vec<OrderId>,
    pub codes: Vec<StockCode>,
}

#[derive(Default)]
pub(crate) struct NpcDecisionTraceCollector {
    records: BTreeMap<AccountId, VecDeque<NpcDecisionTraceRecord>>,
}

impl NpcDecisionTraceCollector {
    pub(crate) fn record(&mut self, record: NpcDecisionTraceRecord) {
        let records = self.records.entry(record.account).or_default();
        records.push_back(record);
        if records.len() > MAX_NPC_DECISION_TRACE_RECORDS {
            let _ = records.pop_front();
        }
    }

    pub(crate) fn records(&self, account: AccountId) -> Option<&VecDeque<NpcDecisionTraceRecord>> {
        self.records.get(&account)
    }
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
}
