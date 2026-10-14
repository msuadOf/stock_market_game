//! 为 Continuous 股票处理与结算事务 candidate 独立收集 Projection。
//!
//! candidate 已拥有 typed StockProcessing 事实；adapter 通过既定 producer 和 collector 投影。
//! 不从 vector 位置推导身份，不修改 candidate 或权威事件序号游标。

use super::{
    event_collection::{collect_events, CollectedEvents, EventFact},
    execution_fact_producers::adapt_continuous_facts, StepFatal,
};
#[cfg(test)]
use super::stock_execution_transaction::StockExecutionOutput;
use crate::StockId;
use std::collections::BTreeMap;

#[cfg(test)]
pub(super) fn collect_continuous_transaction_events(
    stocks: &BTreeMap<StockId, StockExecutionOutput>,
    next_seq: u64,
    mut facts: Vec<EventFact>,
) -> Result<CollectedEvents, StepFatal> {
    for stock in stocks.values() {
        facts.extend(adapt_continuous_facts(
            &stock.place_facts,
            &stock.cancel_facts,
            &stock.trades,
        )?);
    }
    collect_events(facts, next_seq)
}
