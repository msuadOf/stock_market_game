use super::{AccountId, DailyCandle, Money, PositionSnap, StockId};
use std::collections::BTreeMap;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct SaveMarketSnap {
    pub last_price: Money,
    pub last_close: Money,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct SaveAccountSnap {
    pub cash: Money,
    pub positions: BTreeMap<StockId, PositionSnap>,
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
    pub markets: BTreeMap<StockId, SaveMarketSnap>,
    pub accounts: BTreeMap<AccountId, SaveAccountSnap>,
    pub daily_candles: BTreeMap<StockId, Vec<DailyCandle>>,
    pub active_daily_candles: BTreeMap<StockId, DailyCandle>,
}
