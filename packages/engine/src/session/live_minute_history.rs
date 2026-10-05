use super::{GameSession, HistorySessionStatus, MinuteBar, SessionError, TradingPhase};
use crate::{AccountId, CivilDate, CivilInstant, DayStatus, StockCode};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct CurrentMinuteHistoryRequest {
    pub code: StockCode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum CurrentMinuteHistoryPhase {
    CallAuction,
    PreOpen,
    Continuous,
    ClosingAuction,
    AfterClose,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct CurrentMinuteHistoryResponse {
    pub code: StockCode,
    pub date: CivilDate,
    pub observed_at: CivilInstant,
    #[ts(type = "true")]
    pub live: bool,
    pub status: HistorySessionStatus,
    pub phase: CurrentMinuteHistoryPhase,
    pub bars: Vec<MinuteBar>,
}

impl GameSession {
    pub fn current_minute_history(
        &self,
        request: &CurrentMinuteHistoryRequest,
    ) -> Result<CurrentMinuteHistoryResponse, SessionError> {
        self.require_healthy()?;
        if !self.state.markets.contains_key(&request.code) {
            return Err(SessionError::UnknownHistoryStock(request.code.clone()));
        }
        let date = self.civil_date();
        let is_open = matches!(self.stock_day_status(&request.code)?, DayStatus::Trading);
        let after_close = is_open
            && self
                .state
                .tick
                .is_multiple_of(self.state.setup.ticks_per_day)
            && self.state.candle_book.histories()[&request.code]
                .iter_rev()
                .next()
                .is_some_and(|candle| candle.time == super::candles::candle_date_time(date));
        let phase = if !is_open {
            CurrentMinuteHistoryPhase::Closed
        } else if after_close {
            CurrentMinuteHistoryPhase::AfterClose
        } else {
            match self.phase() {
                TradingPhase::CallAuction => CurrentMinuteHistoryPhase::CallAuction,
                TradingPhase::PreOpen => CurrentMinuteHistoryPhase::PreOpen,
                TradingPhase::Continuous => CurrentMinuteHistoryPhase::Continuous,
                TradingPhase::ClosingAuction => CurrentMinuteHistoryPhase::ClosingAuction,
            }
        };
        let observed_at = if self.civil_clock().phase() == super::CivilPhase::ClosedDay {
            CivilInstant::new(date, 0)
                .map_err(|error| SessionError::InvalidHistoryRead(error.to_string()))?
        } else {
            self.observation_civil_instant()
        };
        Ok(CurrentMinuteHistoryResponse {
            code: request.code.clone(),
            date,
            observed_at,
            live: true,
            status: if is_open {
                HistorySessionStatus::Trading
            } else {
                HistorySessionStatus::Closed
            },
            phase,
            bars: if is_open {
                self.state
                    .retained_market_history
                    .current_bars(&request.code)
            } else {
                Vec::new()
            },
        })
    }

    pub fn query_current_minute_history_for(
        &mut self,
        account: AccountId,
        request: &CurrentMinuteHistoryRequest,
    ) -> Result<CurrentMinuteHistoryResponse, SessionError> {
        if !self.state.accounts.contains_key(&account) {
            return Err(SessionError::UnknownHistoryAccount(account));
        }
        let response = self.current_minute_history(request)?;
        let minute = self.current_market_minute();
        self.state
            .history_reads
            .get_mut(&account)
            .ok_or(SessionError::UnknownHistoryAccount(account))?
            .record(&request.code, minute)
            .map_err(|error| SessionError::InvalidHistoryRead(error.to_string()))?;
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Intent, LimitPrice, Money, Order, OrderId, Side};

    fn session(date: &str) -> GameSession {
        let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
        setup.start_date = CivilDate::from_iso(date).unwrap();
        setup.ticks_per_day = 3;
        setup.stocks[0].float_shares = 1_000;
        let mut game = GameSession::new(setup, 71).unwrap();
        game.state
            .npc_attention
            .get_mut(&AccountId(1))
            .unwrap()
            .next_attention_candidate_tick = 1_000_000;
        game.state.attention_scheduler = [(1_000_000, AccountId(1))].into_iter().collect();
        game.state.pending_npc = Some(super::super::PendingNpcBatch {
            observed_tick: 0,
            observed_accounts: Vec::new(),
            intents: Vec::new(),
            dependencies: Vec::new(),
        });
        game
    }

    fn request(game: &GameSession) -> CurrentMinuteHistoryRequest {
        CurrentMinuteHistoryRequest {
            code: game.state.setup.stocks[0].code.clone(),
        }
    }

    fn buy(game: &mut GameSession) -> CurrentMinuteHistoryRequest {
        let request = request(game);
        assert!(game.state.accounts[&AccountId(1)].sellable_qty(&request.code) >= 100);
        let id = game.state.next_order_id;
        let placed = game
            .state
            .markets
            .get_mut(&request.code)
            .unwrap()
            .place(Order {
                id: OrderId(id),
                side: Side::Sell,
                price: Money::from_cents(1000),
                qty: 100,
                original_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                owner: AccountId(1),
                seq: id,
            })
            .unwrap();
        assert!(placed.trades.is_empty());
        game.state.next_order_id += 1;
        game.hydrate_or_validate_envelope_ledger().unwrap();
        game.enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: request.code.clone(),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(1000)),
                qty: 100,
            },
        )
        .unwrap();
        let frame = game.step_frame().unwrap();
        assert_eq!(
            frame
                .events
                .iter()
                .filter(|event| matches!(event, crate::Event::Trade { .. }))
                .count(),
            1
        );
        request
    }

    #[test]
    fn live_query_returns_current_real_trade_before_day_end_without_archiving() {
        let mut game = session("2030-01-02");
        let request = buy(&mut game);
        let before = serde_json::to_value(game.save().unwrap()).unwrap();
        let response = game.current_minute_history(&request).unwrap();
        assert_eq!(response.status, HistorySessionStatus::Trading);
        assert_eq!(response.phase, CurrentMinuteHistoryPhase::Continuous);
        assert!(response.live);
        assert_eq!(response.bars.len(), 1);
        assert_eq!(response.bars[0].minute_of_day, 570);
        assert_eq!(response.bars[0].turnover_cents, 100_000);
        assert_eq!(response.bars[0].volume_shares, 100);
        assert!(response
            .bars
            .iter()
            .all(|bar| u32::from(bar.minute_of_day) <= response.observed_at.second_of_day() / 60));
        assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
        assert!(game.save().unwrap().retained_market_history.is_empty());
    }

    #[test]
    fn personal_live_read_records_only_the_actual_reader() {
        let mut game = session("2030-01-02");
        let request = request(&game);
        let before = game.state.history_reads.to_map();
        game.current_minute_history(&request).unwrap();
        assert_eq!(game.state.history_reads.to_map(), before);
        game.query_current_minute_history_for(AccountId(0), &request)
            .unwrap();
        assert_eq!(
            game.state.history_reads[&AccountId(0)].stocks[&request.code].read_count,
            1
        );
        assert!(game.state.history_reads[&AccountId(1)].stocks.is_empty());
        assert!(game
            .query_current_minute_history_for(AccountId(99), &request)
            .is_err());
        assert!(game
            .query_current_minute_history_for(
                AccountId(0),
                &CurrentMinuteHistoryRequest {
                    code: StockCode("000000".into())
                }
            )
            .is_err());
        assert_eq!(
            game.state.history_reads[&AccountId(0)].stocks[&request.code].read_count,
            1
        );
    }

    #[test]
    fn closed_and_untraded_days_do_not_create_fake_minute_bars() {
        let closed = session("2030-01-01");
        let response = closed.current_minute_history(&request(&closed)).unwrap();
        assert_eq!(response.status, HistorySessionStatus::Closed);
        assert_eq!(response.phase, CurrentMinuteHistoryPhase::Closed);
        assert_eq!(response.observed_at.second_of_day(), 0);
        assert!(response.bars.is_empty());
        let open = session("2030-01-02");
        let response = open.current_minute_history(&request(&open)).unwrap();
        assert_eq!(response.status, HistorySessionStatus::Trading);
        assert_eq!(response.phase, CurrentMinuteHistoryPhase::Continuous);
        assert!(response.bars.is_empty());
    }

    #[test]
    fn after_close_keeps_current_facts_until_civil_day_end_not_into_the_next_day() {
        let mut game = session("2030-01-02");
        let request = buy(&mut game);
        game.step().unwrap();
        game.step().unwrap();
        let response = game.current_minute_history(&request).unwrap();
        assert_eq!(response.phase, CurrentMinuteHistoryPhase::AfterClose);
        assert_eq!(response.observed_at.second_of_day(), 54_000);
        assert_eq!(response.bars.len(), 1);
        game.end_civil_day().unwrap();
        let response = game.current_minute_history(&request).unwrap();
        assert_eq!(response.date, CivilDate::from_iso("2030-01-03").unwrap());
        assert!(response.bars.is_empty());
        assert_eq!(game.save().unwrap().retained_market_history.len(), 1);
    }

    #[test]
    fn current_phase_follows_real_auction_and_continuous_transitions() {
        let mut setup = super::super::npc_working_quote_tests::quote_setup(3);
        setup.start_date = CivilDate::from_iso("2030-01-02").unwrap();
        setup.ticks_per_day = 12;
        setup.auction_ticks = 3;
        setup.closing_auction_ticks = 3;
        setup.npcs.inst_count = 0;
        let mut game = GameSession::new(setup, 17).unwrap();
        let request = request(&game);
        for (tick, phase) in [
            (0, CurrentMinuteHistoryPhase::CallAuction),
            (2, CurrentMinuteHistoryPhase::PreOpen),
            (3, CurrentMinuteHistoryPhase::Continuous),
            (9, CurrentMinuteHistoryPhase::ClosingAuction),
            (12, CurrentMinuteHistoryPhase::AfterClose),
        ] {
            while game.tick() < tick {
                game.step().unwrap();
            }
            let response = game.current_minute_history(&request).unwrap();
            assert_eq!(response.phase, phase);
            assert_eq!(response.status, HistorySessionStatus::Trading);
            assert!(
                response.bars.is_empty(),
                "无真实成交时，竞价指示量不能冒充分钟成交"
            );
            assert_eq!(response.observed_at, game.observation_civil_instant());
        }
    }

    #[test]
    fn closed_security_keeps_shared_observation_time_without_opening_its_market() {
        let mut game =
            super::super::exchange_calendar_tests::mixed_session_with_institution(false, 0);
        let closed_request = CurrentMinuteHistoryRequest {
            code: StockCode("600101".into()),
        };
        let open_request = CurrentMinuteHistoryRequest {
            code: StockCode("000101".into()),
        };
        game.step().unwrap();
        let closed = game.current_minute_history(&closed_request).unwrap();
        let open = game.current_minute_history(&open_request).unwrap();
        assert_eq!(closed.status, HistorySessionStatus::Closed);
        assert_eq!(closed.phase, CurrentMinuteHistoryPhase::Closed);
        assert!(closed.bars.is_empty());
        assert_eq!(open.status, HistorySessionStatus::Trading);
        assert_eq!(closed.observed_at, open.observed_at);
        assert!(closed.observed_at.second_of_day() > 0);
    }

    #[test]
    fn current_history_request_rejects_account_injection_and_missing_security() {
        serde_json::from_value::<CurrentMinuteHistoryRequest>(serde_json::json!({"code":"600888"}))
            .unwrap();
        assert!(serde_json::from_value::<CurrentMinuteHistoryRequest>(
            serde_json::json!({"code":"600888","account":"99"})
        )
        .is_err());
        assert!(
            serde_json::from_value::<CurrentMinuteHistoryRequest>(serde_json::json!({})).is_err()
        );
    }
}
