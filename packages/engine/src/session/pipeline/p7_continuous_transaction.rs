//! Detached P7 collection for the continuous P4-P6 transaction candidate.
//!
//! The candidate already owns typed P4 facts. This adapter only projects those facts through
//! the approved producer and collector; it never infers identity from vector position and never
//! mutates the candidate or an authoritative sequence cursor.

use super::{
    p4_p5_p6_transaction::P4P5P6StockOutput,
    p7_events::{collect_events, CollectedEvents},
    p7_p4_producers::adapt_continuous_facts,
    StepFatal,
};
use crate::StockCode;
use std::collections::BTreeMap;

pub(super) fn collect_continuous_transaction_events(
    stocks: &BTreeMap<StockCode, P4P5P6StockOutput>,
    next_seq: u64,
) -> Result<CollectedEvents, StepFatal> {
    let mut facts = Vec::new();
    for stock in stocks.values() {
        facts.extend(adapt_continuous_facts(
            &stock.place_facts,
            &stock.cancel_facts,
            &stock.trades,
        )?);
    }
    collect_events(facts, next_seq)
}
