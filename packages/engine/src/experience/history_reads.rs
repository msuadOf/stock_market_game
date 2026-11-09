use std::collections::BTreeMap;
use std::sync::Arc;

use crate::StockId;

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PersonalHistoryReadLedger {
    pub stocks: Arc<BTreeMap<StockId, StockHistoryRead>>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct StockHistoryRead {
    #[serde(with = "super::u64_decimal")]
    #[ts(type = "string")]
    pub last_read_market_minute: u64,
    pub read_count: u32,
}

impl PersonalHistoryReadLedger {
    pub fn record(&mut self, code: &StockId, market_minute: u64) -> Result<(), HistoryReadError> {
        if let Some(entry) = self.stocks.get(code) {
            if market_minute < entry.last_read_market_minute {
                return Err(HistoryReadError::TimeWentBackwards {
                    attempted: market_minute,
                    last: entry.last_read_market_minute,
                });
            }
        }
        let stocks = Arc::make_mut(&mut self.stocks);
        match stocks.get_mut(code) {
            Some(entry) => {
                entry.read_count = entry
                    .read_count
                    .checked_add(1)
                    .expect("read count is bounded by accepted history requests per session");
                entry.last_read_market_minute = market_minute;
            }
            None => {
                stocks.insert(
                    *code,
                    StockHistoryRead {
                        last_read_market_minute: market_minute,
                        read_count: 1,
                    },
                );
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum HistoryReadError {
    #[error("history read time cannot go backwards: attempted {attempted} after last read {last}")]
    TimeWentBackwards { attempted: u64, last: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(rank: u32) -> StockId {
        StockId(rank)
    }

    #[test]
    fn history_reads_record_only_access_facts_without_price_observations() {
        let mut ledger = PersonalHistoryReadLedger::default();
        ledger.record(&code(0), 12).unwrap();
        ledger.record(&code(0), 12).unwrap();
        ledger.record(&code(1), 9).unwrap();

        assert_eq!(ledger.stocks[&code(0)].read_count, 2);
        assert_eq!(ledger.stocks[&code(0)].last_read_market_minute, 12);
        assert_eq!(ledger.stocks[&code(1)].read_count, 1);
    }

    #[test]
    fn 日终快照共享历史并在写入时隔离() {
        let mut live = PersonalHistoryReadLedger::default();
        live.record(&code(0), 12).unwrap();
        let saved = live.clone();
        assert!(std::sync::Arc::ptr_eq(&live.stocks, &saved.stocks));
        assert!(live.record(&code(0), 11).is_err());
        assert!(std::sync::Arc::ptr_eq(&live.stocks, &saved.stocks));
        live.record(&code(0), 13).unwrap();
        assert!(!std::sync::Arc::ptr_eq(&live.stocks, &saved.stocks));
        assert_eq!(saved.stocks[&code(0)].last_read_market_minute, 12);
        assert_eq!(live.stocks[&code(0)].last_read_market_minute, 13);
        let json = serde_json::to_value(&saved).unwrap();
        assert_eq!(json["stocks"]["0"]["last_read_market_minute"], "12");
        assert_eq!(
            serde_json::from_value::<PersonalHistoryReadLedger>(json).unwrap(),
            saved
        );
    }

    #[test]
    fn rejected_history_read_does_not_mutate_ledger() {
        let mut ledger = PersonalHistoryReadLedger::default();
        ledger.record(&code(0), 12).unwrap();
        let before = ledger.clone();

        assert_eq!(
            ledger.record(&code(0), 11),
            Err(HistoryReadError::TimeWentBackwards {
                attempted: 11,
                last: 12,
            })
        );
        assert_eq!(ledger, before);
    }
}
