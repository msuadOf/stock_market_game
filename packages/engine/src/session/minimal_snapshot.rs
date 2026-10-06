use super::{AccountId, DailyCandle, Money, PositionSnap, StockCode};
use std::collections::BTreeMap;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct SaveMarketSnap {
    pub last_price: Money,
    pub last_close: Money,
    pub cash_ex_reference_pending_trade: bool,
    pub day_market_activity: bool,
    #[serde(deserialize_with = "crate::company::persistence::required_nullable")]
    #[ts(type = "import(\"../../save/schema/company/ex-reference-price\").ExReferencePrice | null")]
    pub last_cash_ex_reference: Option<crate::company::ex_reference_price::ExReferencePrice>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct SaveAccountSnap {
    pub cash: Money,
    pub positions: BTreeMap<StockCode, PositionSnap>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SaveSnapshot {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub seq: u64,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub tick: u64,
    pub markets: BTreeMap<StockCode, SaveMarketSnap>,
    pub accounts: BTreeMap<AccountId, SaveAccountSnap>,
    pub daily_candles: BTreeMap<StockCode, Vec<DailyCandle>>,
    pub active_daily_candles: BTreeMap<StockCode, DailyCandle>,
}
