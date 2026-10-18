use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{DailyCandle, GameSession, SessionError};
use crate::{AccountId, StockId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct HistoricalStockData {
    pub code: StockId,
    pub daily_candles: Vec<DailyCandle>,
    pub active_daily_candle: Option<DailyCandle>,
}

impl GameSession {
    pub fn query_stock_history(
        &mut self,
        account: AccountId,
        code: &StockId,
    ) -> Result<HistoricalStockData, SessionError> {
        if !self.state.accounts.contains_key(&account) {
            return Err(SessionError::UnknownHistoryAccount(account));
        }
        let history = self
            .state
            .candle_book
            .histories()
            .get(code)
            .ok_or_else(|| SessionError::UnknownHistoryStock(code.clone()))?;
        let result = HistoricalStockData {
            code: code.clone(),
            daily_candles: history.iter().cloned().collect(),
            active_daily_candle: self.state.candle_book.active().get(code).cloned(),
        };
        let market_minute = self.current_market_minute();
        self.state
            .history_reads
            .get_mut(&account)
            .ok_or(SessionError::UnknownHistoryAccount(account))?
            .record(code, market_minute)
            // 1260：个人历史读记录失败（数值原因由下层枚举承载，此处统一码）。
            .map_err(|_| {
                SessionError::InvalidHistoryRead(crate::error::ValidationFault::bare(1260))
            })?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn session() -> GameSession {
        GameSession::new(super::super::npc_working_quote_tests::quote_setup(0), 41).unwrap()
    }

    #[test]
    fn player_history_request_records_only_requested_account_without_personal_price_memory() {
        let mut game = session();
        let code = game.stock_id("600888").unwrap();
        let before = game.snapshot();

        let result = game.query_stock_history(AccountId(0), &code).unwrap();

        assert_eq!(result.code, code);
        assert_eq!(result.daily_candles, before.daily_candles[&result.code]);
        assert_eq!(
            result.active_daily_candle,
            before.active_daily_candles.get(&result.code).cloned()
        );
        assert_eq!(
            game.state.history_reads[&AccountId(0)].stocks[&result.code].read_count,
            1
        );
        assert!(game.state.history_reads[&AccountId(1)].stocks.is_empty());
        assert_eq!(
            serde_json::to_value(game.snapshot()).unwrap(),
            serde_json::to_value(before).unwrap()
        );
    }

    #[test]
    fn invalid_history_requests_do_not_record_reads() {
        let mut game = session();
        let code = game.stock_id("600888").unwrap();
        let before = game.state.history_reads.to_map();

        assert!(matches!(
            game.query_stock_history(AccountId(999), &code),
            Err(SessionError::UnknownHistoryAccount(AccountId(999)))
        ));
        assert!(matches!(
            game.query_stock_history(AccountId(0), &StockId(9)),
            Err(SessionError::UnknownHistoryStock(_))
        ));
        assert_eq!(game.state.history_reads.to_map(), before);
    }

    #[test]
    fn history_reads_survive_civil_day_save_restore_without_mutating_snapshot_or_price_cache() {
        let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
        setup.start_date = crate::CivilDate::from_iso("2030-01-07").unwrap();
        setup.ticks_per_day = 3;
        let mut game = GameSession::new(setup, 41).unwrap();
        let code = game.stock_id("600888").unwrap();
        let snapshot_before = serde_json::to_value(game.snapshot()).unwrap();
        let price_memories_before = game
            .state
            .belief_participants
            .iter()
            .map(|(account, participant)| (*account, participant.price_memory().clone()))
            .collect::<BTreeMap<_, _>>();

        game.query_stock_history(AccountId(0), &code).unwrap();

        assert_eq!(
            serde_json::to_value(game.snapshot()).unwrap(),
            snapshot_before
        );
        assert_eq!(
            game.state
                .belief_participants
                .iter()
                .map(|(account, participant)| (*account, participant.price_memory().clone()))
                .collect::<BTreeMap<_, _>>(),
            price_memories_before
        );
        let expected_reads = game.state.history_reads.to_map();
        for _ in 0..game.state.setup.ticks_per_day {
            game.step().unwrap();
        }
        game.end_civil_day().unwrap();
        let save = game.save().unwrap();
        let restored = GameSession::restore(&save).unwrap();

        assert_eq!(restored.state.history_reads.to_map(), expected_reads);
        assert_eq!(
            restored.state.history_reads[&AccountId(0)].stocks[&code].read_count,
            1
        );
    }
}
