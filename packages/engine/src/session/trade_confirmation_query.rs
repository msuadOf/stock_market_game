use super::{GameSession, PersonalTradeConfirmation, SessionError};
use crate::{AccountId, CivilDate, Side, StockCode};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(transparent)]
#[ts(export)]
pub struct TradeHistoryReceiptCursor(
    #[serde(with = "super::canonical_u64_decimal")]
    #[ts(type = "string")]
    pub u64,
);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PersonalTradeHistoryRequest {
    pub date_from: CivilDate,
    pub date_to: CivilDate,
    #[serde(deserialize_with = "required_nullable")]
    pub code: Option<StockCode>,
    #[serde(deserialize_with = "required_nullable")]
    pub side: Option<Side>,
    #[serde(deserialize_with = "required_nullable")]
    pub before_receipt: Option<TradeHistoryReceiptCursor>,
    #[serde(deserialize_with = "required_nullable")]
    pub as_of_receipt: Option<TradeHistoryReceiptCursor>,
    pub page_size: u32,
}

fn required_nullable<'de, Value: Deserialize<'de>, Deserializer: serde::Deserializer<'de>>(
    deserializer: Deserializer,
) -> Result<Option<Value>, Deserializer::Error> {
    Option::<Value>::deserialize(deserializer)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PersonalTradeHistoryPage {
    pub request: PersonalTradeHistoryRequest,
    pub confirmations: Vec<PersonalTradeConfirmation>,
    pub next_cursor: Option<TradeHistoryReceiptCursor>,
    pub as_of_receipt: TradeHistoryReceiptCursor,
    pub start_date: CivilDate,
    pub current_date: CivilDate,
    pub settled_through: Option<CivilDate>,
}

impl GameSession {
    pub fn query_personal_trade_history(
        &self,
        account: AccountId,
        request: PersonalTradeHistoryRequest,
    ) -> Result<PersonalTradeHistoryPage, SessionError> {
        self.require_healthy()?;
        if !self.state.accounts.contains_key(&account) {
            return Err(SessionError::UnknownHistoryAccount(account));
        }
        if let Some(code) = &request.code {
            if !self.state.markets.contains_key(code) {
                return Err(SessionError::UnknownHistoryStock(code.clone()));
            }
        }
        let as_of_receipt = request.as_of_receipt.unwrap_or(TradeHistoryReceiptCursor(self.state.next_receipt_base));
        if request.date_from > request.date_to
            || !(1..=100).contains(&request.page_size)
            || as_of_receipt.0 > self.state.next_receipt_base
            || request.before_receipt.is_some_and(|before| before.0 > as_of_receipt.0)
        {
            return Err(SessionError::InvalidTradeHistoryQuery(
                "交割单日期范围、page_size或receipt排他游标不合法".into(),
            ));
        }
        let mut rows = self.state.personal_trade_confirmations.get(&account)
            .into_iter()
            .flat_map(|history| history.iter_rev())
            .filter(|row| row.receipt_id < as_of_receipt.0
                && request.before_receipt.is_none_or(|before| row.receipt_id < before.0)
                && row.civil_date >= request.date_from
                && row.civil_date <= request.date_to
                && request.code.as_ref().is_none_or(|code| row.code == *code)
                && request.side.is_none_or(|side| row.side == side));
        let confirmations: Vec<_> = rows.by_ref().take(request.page_size as usize).cloned().collect();
        let next_cursor = if rows.next().is_some() {
            confirmations.last().map(|row| TradeHistoryReceiptCursor(row.receipt_id))
        } else {
            None
        };
        Ok(PersonalTradeHistoryPage {
            request,
            confirmations,
            next_cursor,
            as_of_receipt,
            start_date: self.state.setup.start_date,
            current_date: self.civil_date(),
            settled_through: self.civil_clock().settled_through(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::experience::AppendOnlyHistory;
    use crate::Money;

    fn date(value: &str) -> CivilDate { CivilDate::from_iso(value).unwrap() }
    fn request() -> PersonalTradeHistoryRequest {
        PersonalTradeHistoryRequest { date_from: date("2030-01-02"), date_to: date("2030-01-04"), code: None, side: None, before_receipt: None, as_of_receipt: None, page_size: 2 }
    }
    fn fixture() -> GameSession {
        let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
        setup.start_date = date("2030-01-02");
        setup.npcs.inst_count = 0;
        let mut game = GameSession::new(setup, 31).unwrap();
        let code = game.state.setup.stocks[0].code.clone();
        let mut history = AppendOnlyHistory::default();
        for (receipt, when, side) in [(1, "2030-01-02", Side::Buy), (4, "2030-01-03", Side::Sell), (7, "2030-01-03", Side::Buy), (9, "2030-01-04", Side::Sell)] {
            history.push(PersonalTradeConfirmation { receipt_id: receipt, civil_date: date(when), code: code.clone(), side, price: Money::from_cents(1000), quantity_shares: 100, gross: Money::from_cents(100000), actual_fees: super::super::pipeline::FeeComponents::ZERO });
        }
        game.state.personal_trade_confirmations.insert(AccountId(0), history);
        game.state.next_receipt_base = 10;
        let mut clock = super::super::CivilClock::new(
            game.state.setup.start_date, crate::calendar::CalendarExchange::Sse,
        ).unwrap().save();
        clock.current_date = date("2030-01-05");
        clock.settled_through = Some(date("2030-01-04"));
        game.state.civil_clock = super::super::CivilClock::from_parts(
            game.state.setup.start_date, &clock, crate::calendar::CalendarExchange::Sse,
        ).unwrap();
        game
    }

    #[test]
    fn date_range_and_receipt_cursor_return_complete_stable_owned_pages() {
        let game = fixture();
        let first = game.query_personal_trade_history(AccountId(0), request()).unwrap();
        assert_eq!(first.confirmations.iter().map(|row| row.receipt_id).collect::<Vec<_>>(), [9, 7]);
        assert_eq!(first.next_cursor, Some(TradeHistoryReceiptCursor(7)));
        let second = game.query_personal_trade_history(AccountId(0), PersonalTradeHistoryRequest { before_receipt: first.next_cursor, as_of_receipt: Some(first.as_of_receipt), ..request() }).unwrap();
        assert_eq!(second.confirmations.iter().map(|row| row.receipt_id).collect::<Vec<_>>(), [4, 1]);
        assert_eq!(second.next_cursor, None);
    }

    #[test]
    fn filters_are_inclusive_and_new_fills_do_not_enter_existing_receipt_window() {
        let mut game = fixture();
        let first = game.query_personal_trade_history(AccountId(0), request()).unwrap();
        let mut added = game.state.personal_trade_confirmations[&AccountId(0)].last().unwrap().clone();
        added.receipt_id = 12;
        game.state.personal_trade_confirmations.get_mut(&AccountId(0)).unwrap().push(added);
        game.state.next_receipt_base = 13;
        let page = game.query_personal_trade_history(AccountId(0), PersonalTradeHistoryRequest { date_from: date("2030-01-03"), date_to: date("2030-01-03"), side: Some(Side::Buy), as_of_receipt: Some(first.as_of_receipt), ..request() }).unwrap();
        assert_eq!(page.confirmations.iter().map(|row| row.receipt_id).collect::<Vec<_>>(), [7]);
        assert_eq!(page.as_of_receipt, first.as_of_receipt);
        assert_eq!(game.personal_trade_confirmations(AccountId(0)).len(), 5);
    }

    #[test]
    fn invalid_date_range_page_size_and_future_receipt_are_explicit_errors() {
        let game = fixture();
        for invalid in [PersonalTradeHistoryRequest { date_from: date("2030-01-05"), ..request() }, PersonalTradeHistoryRequest { page_size: 0, ..request() }, PersonalTradeHistoryRequest { page_size: 101, ..request() }, PersonalTradeHistoryRequest { as_of_receipt: Some(TradeHistoryReceiptCursor(11)), ..request() }, PersonalTradeHistoryRequest { before_receipt: Some(TradeHistoryReceiptCursor(11)), ..request() }] {
            assert!(matches!(game.query_personal_trade_history(AccountId(0), invalid), Err(SessionError::InvalidTradeHistoryQuery(_))));
        }
        assert!(matches!(game.query_personal_trade_history(AccountId(1), request()), Err(SessionError::UnknownHistoryAccount(_))));
    }

    #[test]
    fn security_filter_empty_history_and_exact_full_page_keep_truthful_boundaries() {
        let mut game = fixture();
        let code = game.state.setup.stocks[0].code.clone();
        let page = game.query_personal_trade_history(AccountId(0), PersonalTradeHistoryRequest {
            code: Some(code), page_size: 4, ..request()
        }).unwrap();
        assert_eq!(page.confirmations.len(), 4);
        assert_eq!(page.next_cursor, None);
        assert_eq!(page.current_date, date("2030-01-05"));
        assert_eq!(page.settled_through, Some(date("2030-01-04")));
        assert!(matches!(game.query_personal_trade_history(AccountId(0), PersonalTradeHistoryRequest {
            code: Some(StockCode("999999".into())), ..request()
        }), Err(SessionError::UnknownHistoryStock(_))));
        let empty = game.query_personal_trade_history(AccountId(0), PersonalTradeHistoryRequest {
            before_receipt: Some(TradeHistoryReceiptCursor(0)), ..request()
        }).unwrap();
        assert!(empty.confirmations.is_empty());
        assert_eq!(empty.next_cursor, None);
        game.state.personal_trade_confirmations.remove(&AccountId(0));
        let empty = game.query_personal_trade_history(AccountId(0), request()).unwrap();
        assert!(empty.confirmations.is_empty());
        assert_eq!(empty.next_cursor, None);
        assert_eq!(empty.as_of_receipt, TradeHistoryReceiptCursor(10));
    }

    #[test]
    fn date_history_query_does_not_record_market_history_reads_or_mutate_history() {
        let game = fixture();
        let before = serde_json::to_value(game.state.history_reads.to_map()).unwrap();
        let history = game.personal_trade_confirmations(AccountId(0));
        let page = game.query_personal_trade_history(AccountId(0), PersonalTradeHistoryRequest { date_from: date("2029-12-31"), date_to: date("2029-12-31"), ..request() }).unwrap();
        assert!(page.confirmations.is_empty());
        assert_eq!(page.start_date, date("2030-01-02"));
        assert_eq!(serde_json::to_value(game.state.history_reads.to_map()).unwrap(), before);
        assert_eq!(game.personal_trade_confirmations(AccountId(0)), history);
    }

    #[test]
    fn query_wire_requires_nullable_fields_and_canonical_complete_u64_receipts() {
        let complete = serde_json::to_value(request()).unwrap();
        assert!(serde_json::from_value::<PersonalTradeHistoryRequest>(complete.clone()).is_ok());
        for field in ["code", "side", "before_receipt", "as_of_receipt"] {
            let mut missing = complete.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<PersonalTradeHistoryRequest>(missing).is_err());
        }
        for invalid in ["01", "+1", "18446744073709551616"] {
            let mut altered = complete.clone();
            altered["before_receipt"] = serde_json::json!(invalid);
            assert!(serde_json::from_value::<PersonalTradeHistoryRequest>(altered).is_err());
        }
        let mut maximum = complete;
        maximum["before_receipt"] = serde_json::json!("18446744073709551615");
        assert!(serde_json::from_value::<PersonalTradeHistoryRequest>(maximum).is_ok());
    }
}
