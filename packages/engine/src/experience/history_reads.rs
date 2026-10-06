use std::collections::BTreeMap;

use crate::StockId;

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PersonalHistoryReadLedger {
    pub stocks: BTreeMap<StockId, StockHistoryRead>,
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
        match self.stocks.get_mut(code) {
            Some(entry) => {
                if market_minute < entry.last_read_market_minute {
                    return Err(HistoryReadError::TimeWentBackwards {
                        attempted: market_minute,
                        last: entry.last_read_market_minute,
                    });
                }
                entry.read_count = entry
                    .read_count
                    .checked_add(1)
                    .expect("read count is bounded by accepted history requests per session");
                entry.last_read_market_minute = market_minute;
            }
            None => {
                self.stocks.insert(
                    code.clone(),
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
