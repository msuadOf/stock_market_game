//! 权威日 K 的生成、盘中维护与收盘归档。

use super::*;
use crate::experience::AppendOnlyHistory;
use std::collections::VecDeque;
use std::ops::Index;

const TECHNICAL_RECENT_VALID_DAYS: usize = crate::strategy::SMA_LONG_WINDOW;

/// 完整日 K 历史及技术指标的有界缓存。
/// tick candidate clone 共享旧历史；收盘追加只复制最新 history chunk。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct DailyCandleHistory {
    candles: AppendOnlyHistory<DailyCandle>,
    traded_count: usize,
    recent_traded: VecDeque<DailyCandle>,
}

impl DailyCandleHistory {
    pub(super) fn len(&self) -> usize {
        self.candles.len()
    }

    #[cfg(test)]
    pub(super) fn last(&self) -> Option<&DailyCandle> {
        self.candles.last()
    }

    pub(super) fn iter(&self) -> impl ExactSizeIterator<Item = &DailyCandle> {
        self.candles.iter()
    }

    pub(super) fn iter_rev(&self) -> impl ExactSizeIterator<Item = &DailyCandle> {
        self.candles.iter_rev()
    }

    pub(super) fn recent(&self, count: usize) -> Vec<&DailyCandle> {
        let mut recent = self.iter_rev().take(count).collect::<Vec<_>>();
        recent.reverse();
        recent
    }

    pub(super) fn traded_count(&self) -> usize {
        self.traded_count
    }

    pub(super) fn recent_traded(&self) -> &VecDeque<DailyCandle> {
        &self.recent_traded
    }

    pub(super) fn push(&mut self, candle: DailyCandle) {
        if candle.volume > 0 {
            self.traded_count = self
                .traded_count
                .checked_add(1)
                .expect("daily traded sample count overflow");
            self.recent_traded.push_back(candle.clone());
            if self.recent_traded.len() > TECHNICAL_RECENT_VALID_DAYS {
                self.recent_traded.pop_front();
            }
        }
        self.candles.push(candle);
    }
}

impl From<Vec<DailyCandle>> for DailyCandleHistory {
    fn from(candles: Vec<DailyCandle>) -> Self {
        let mut history = Self::default();
        for candle in candles {
            history.push(candle);
        }
        history
    }
}

impl Index<usize> for DailyCandleHistory {
    type Output = DailyCandle;

    fn index(&self, index: usize) -> &Self::Output {
        &self.candles[index]
    }
}

impl serde::Serialize for DailyCandleHistory {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.candles.serialize(serializer)
    }
}

/// 活动日 K 与完整历史的唯一 owner；对外存档仍分别投影两个 map。
#[derive(Clone, Debug, Default)]
pub(super) struct SessionCandleBook {
    histories: BTreeMap<StockCode, DailyCandleHistory>,
    active: BTreeMap<StockCode, DailyCandle>,
}

impl SessionCandleBook {
    pub(super) fn new(
        histories: BTreeMap<StockCode, DailyCandleHistory>,
        active: BTreeMap<StockCode, DailyCandle>,
    ) -> Self {
        Self { histories, active }
    }

    pub(super) fn histories(&self) -> &BTreeMap<StockCode, DailyCandleHistory> {
        &self.histories
    }
    pub(super) fn active(&self) -> &BTreeMap<StockCode, DailyCandle> {
        &self.active
    }

    #[cfg(test)]
    pub(super) fn replace_history(&mut self, code: StockCode, history: DailyCandleHistory) {
        self.histories.insert(code, history);
    }

    pub(super) fn replace_histories(&mut self, histories: BTreeMap<StockCode, DailyCandleHistory>) {
        self.histories = histories;
    }

    pub(super) fn replace_active(&mut self, active: BTreeMap<StockCode, DailyCandle>) {
        self.active = active;
    }

    #[cfg(test)]
    pub(super) fn set_active(&mut self, code: StockCode, candle: DailyCandle) {
        self.active.insert(code, candle);
    }
}

impl SessionCandleBook {
    pub(super) fn record_trade_or_mark(
        &mut self,
        time: i64,
        code: &StockCode,
        price: Money,
        added_volume: u64,
    ) {
        let candle = self.active.entry(code.clone()).or_insert(DailyCandle {
            time,
            open: price,
            high: price,
            low: price,
            close: price,
            volume: 0,
            trade_stats: Some(DailyTradeStats::default()),
        });
        // 开盘前 PriceTick 会用昨收建立零成交占位 K。集合竞价后的第一笔真实成交
        // 才是当日开盘价；此时必须丢弃占位 OHLC，否则跳空实体会错误连接到昨收。
        if candle.volume == 0 && added_volume > 0 {
            *candle = DailyCandle {
                time,
                open: price,
                high: price,
                low: price,
                close: price,
                volume: added_volume,
                trade_stats: Some(DailyTradeStats::from_trade(price, added_volume)),
            };
            return;
        }
        candle.high = candle.high.max(price);
        candle.low = candle.low.min(price);
        candle.close = price;
        candle.volume = candle
            .volume
            .checked_add(added_volume)
            .expect("daily candle volume overflow: engine state invariant violated");
        if added_volume > 0 {
            candle
                .trade_stats
                .as_mut()
                .expect("a real active candle must retain trade statistics")
                .record_trade(price, added_volume);
        } else if candle.trade_stats.is_none() {
            candle.trade_stats = Some(DailyTradeStats::default());
        }
    }

    pub(super) fn commit_active(&mut self) -> BTreeMap<StockCode, DailyCandle> {
        let closed = std::mem::take(&mut self.active);
        for (code, candle) in &closed {
            let history = self.histories.entry(code.clone()).or_default();
            history.push(candle.clone());
        }
        closed
    }
}

impl GameSession {
    pub(super) fn update_active_daily_candle(
        &mut self,
        code: &StockCode,
        price: Money,
        added_volume: u64,
    ) {
        self.state.candle_book.record_trade_or_mark(
            candle_date_time(self.state.civil_clock.current_date()),
            code,
            price,
            added_volume,
        );
    }

    pub(super) fn commit_active_daily_candles(&mut self) -> BTreeMap<StockCode, DailyCandle> {
        self.state.candle_book.commit_active()
    }
}

const PRESET_HISTORY_DAYS: usize = 360;
const SECONDS_PER_DAY: i64 = 86_400;

/// 生成与 session RNG 隔离的确定性历史，避免预置行情改变 NPC 的随机序列。
pub(super) fn generate_preset_daily_candles(
    setup: &SessionSetup,
    seed: u64,
) -> Result<BTreeMap<StockCode, DailyCandleHistory>, SessionError> {
    let calendar = crate::calendar::TradingCalendar::current_default_calendar()?;
    setup
        .stocks
        .iter()
        .map(|stock| {
            let dates = preset_candle_dates(&calendar, setup.start_date, stock.exchange)?;
            let mut rng =
                SplitMix64::new(seed ^ stock_code_hash(&stock.code) ^ 0xD1A1_C4AD_1E50_0360);
            let tick = stock.tick.cents().max(1);
            let anchor = stock.initial_price.cents().max(tick);
            let limit_bps = (stock.limit_pct * 10_000.0).round().max(1.0) as i64;
            let min_price = quantize_cents((anchor / 3).max(tick), tick);
            let max_price = quantize_cents(anchor.saturating_mul(3), tick);
            let mut close = anchor;
            let mut newest_first = Vec::with_capacity(PRESET_HISTORY_DAYS);

            for recency in 0..PRESET_HISTORY_DAYS {
                let body_radius = (close.saturating_mul(limit_bps) / 20_000).max(tick);
                let body_delta = random_signed(&mut rng, body_radius / tick) * tick;
                let open = quantize_cents(
                    close.saturating_sub(body_delta).clamp(min_price, max_price),
                    tick,
                );
                let wick_radius = (close.saturating_mul(limit_bps) / 50_000).max(tick);
                let upper = (rng.next_u64() % ((wick_radius / tick + 1) as u64)) as i64 * tick;
                let lower = (rng.next_u64() % ((wick_radius / tick + 1) as u64)) as i64 * tick;
                let high =
                    quantize_cents(open.max(close).saturating_add(upper).min(max_price), tick);
                let low = quantize_cents(open.min(close).saturating_sub(lower).max(tick), tick);
                let base_volume = u64::from(stock.float_shares).max(100_000) / 1_250;
                let volume = base_volume + rng.next_u64() % (base_volume.saturating_mul(5).max(1));
                newest_first.push(DailyCandle {
                    time: candle_date_time(dates[PRESET_HISTORY_DAYS - 1 - recency]),
                    open: Money::from_cents(open),
                    high: Money::from_cents(high),
                    low: Money::from_cents(low),
                    close: Money::from_cents(close),
                    volume,
                    trade_stats: None,
                });

                let gap_radius = (open.saturating_mul(limit_bps) / 80_000).max(tick);
                let gap_delta = random_signed(&mut rng, gap_radius / tick) * tick;
                close = quantize_cents(
                    open.saturating_sub(gap_delta).clamp(min_price, max_price),
                    tick,
                );
            }
            newest_first.reverse();
            Ok((stock.code.clone(), newest_first.into()))
        })
        .collect()
}

pub(super) fn candle_date_time(date: crate::calendar::CivilDate) -> i64 {
    let epoch =
        crate::calendar::CivilDate::from_ymd(1970, 1, 1).expect("1970-01-01 is a valid civil date");
    date.days_since(epoch) * SECONDS_PER_DAY
}

pub(super) fn preset_candle_dates(
    calendar: &crate::calendar::TradingCalendar,
    start: crate::calendar::CivilDate,
    exchange: StockExchange,
) -> Result<Vec<crate::calendar::CivilDate>, crate::calendar::CalendarError> {
    let mut dates = Vec::with_capacity(PRESET_HISTORY_DAYS);
    let mut before = start;
    for _ in 0..PRESET_HISTORY_DAYS {
        before = calendar.previous_trading_day(session_calendar_exchange(exchange), before)?;
        dates.push(before);
    }
    dates.reverse();
    Ok(dates)
}

impl DailyTradeStats {
    fn from_trade(price: Money, quantity: u64) -> Self {
        let mut stats = Self::default();
        stats.record_trade(price, quantity);
        stats
    }

    fn record_trade(&mut self, price: Money, quantity: u64) {
        let price_cents = u64::try_from(price.cents())
            .expect("trade price must be positive before daily statistics are updated");
        let gross_cents = price_cents
            .checked_mul(quantity)
            .expect("daily trade turnover multiplication overflow: engine invariant violated");
        self.turnover_cents = self
            .turnover_cents
            .checked_add(gross_cents)
            .expect("daily trade turnover overflow: engine invariant violated");
        self.trade_count = self
            .trade_count
            .checked_add(1)
            .expect("daily trade count overflow: engine invariant violated");
    }
}

fn random_signed(rng: &mut SplitMix64, radius: i64) -> i64 {
    let radius = radius.max(1);
    let width = (radius as u64).saturating_mul(2).saturating_add(1);
    (rng.next_u64() % width) as i64 - radius
}

fn quantize_cents(cents: i64, tick: i64) -> i64 {
    ((cents.saturating_add(tick / 2)) / tick).max(1) * tick
}

/// 把 StockCode 哈希为 u64（用于派生每只股票的确定性 RNG 种子）。
pub(super) fn stock_code_hash(code: &StockCode) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    code.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod shared_daily_history_tests {
    use super::*;

    #[test]
    fn candidate_append_shares_old_days_and_preserves_full_json_history() {
        let candle = DailyCandle {
            time: 0,
            open: Money::from_cents(1_000),
            high: Money::from_cents(1_000),
            low: Money::from_cents(1_000),
            close: Money::from_cents(1_000),
            volume: 100,
            trade_stats: None,
        };
        let original: DailyCandleHistory = (0..1_024)
            .map(|day| DailyCandle {
                time: i64::from(day) * SECONDS_PER_DAY,
                ..candle.clone()
            })
            .collect::<Vec<_>>()
            .into();
        let mut candidate = original.clone();
        assert!(std::ptr::eq(
            original.last().unwrap(),
            candidate.last().unwrap()
        ));

        candidate.push(DailyCandle {
            time: 1_024 * SECONDS_PER_DAY,
            ..candle
        });

        assert_eq!(original.len(), 1_024);
        assert_eq!(candidate.len(), 1_025);
        assert_eq!(original.last().unwrap().time, 1_023 * SECONDS_PER_DAY);
        assert_eq!(candidate.last().unwrap().time, 1_024 * SECONDS_PER_DAY);
        assert_eq!(
            serde_json::to_value(&candidate)
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1_025
        );
    }
}

#[cfg(test)]
mod candle_book_tests {
    use super::*;

    #[test]
    fn no_trade_day_is_archived_without_becoming_a_traded_sample() {
        let code = StockCode("600000".into());
        let mut book = SessionCandleBook::default();
        book.record_trade_or_mark(0, &code, Money::from_cents(1_000), 0);
        let closed = book.commit_active();
        assert_eq!(closed[&code].volume, 0);
        assert_eq!(book.histories()[&code].len(), 1);
        assert_eq!(book.histories()[&code].traded_count(), 0);
        assert!(book.histories()[&code].recent_traded().is_empty());
        assert_eq!(closed[&code].trade_stats, Some(DailyTradeStats::default()));
    }

    #[test]
    #[should_panic(expected = "daily candle volume overflow: engine state invariant violated")]
    fn active_candle_volume_overflow_stays_explicit() {
        let code = StockCode("600000".into());
        let mut book = SessionCandleBook::default();
        book.set_active(
            code.clone(),
            DailyCandle {
                time: 0,
                open: Money::from_cents(1_000),
                high: Money::from_cents(1_000),
                low: Money::from_cents(1_000),
                close: Money::from_cents(1_000),
                volume: u64::MAX,
                trade_stats: Some(DailyTradeStats::default()),
            },
        );
        book.record_trade_or_mark(0, &code, Money::from_cents(1_000), 1);
    }

    #[test]
    #[should_panic(expected = "daily trade turnover overflow: engine invariant violated")]
    fn active_candle_trade_statistics_overflow_stays_explicit() {
        let code = StockCode("600000".into());
        let mut book = SessionCandleBook::default();
        book.set_active(
            code.clone(),
            DailyCandle {
                time: 0,
                open: Money::from_cents(1_000),
                high: Money::from_cents(1_000),
                low: Money::from_cents(1_000),
                close: Money::from_cents(1_000),
                volume: 100,
                trade_stats: Some(DailyTradeStats {
                    turnover_cents: u64::MAX,
                    trade_count: 1,
                }),
            },
        );
        book.record_trade_or_mark(0, &code, Money::from_cents(1_000), 1);
    }

    #[test]
    fn first_trade_replaces_mark_and_commit_archives_once() {
        let code = StockCode("600000".into());
        let mut book = SessionCandleBook::default();
        book.record_trade_or_mark(2, &code, Money::from_cents(1_000), 0);
        book.record_trade_or_mark(2, &code, Money::from_cents(1_100), 100);
        book.record_trade_or_mark(2, &code, Money::from_cents(1_050), 50);
        let candle = &book.active()[&code];
        assert_eq!(candle.open, Money::from_cents(1_100));
        assert_eq!(candle.low, Money::from_cents(1_050));
        assert_eq!(candle.high, Money::from_cents(1_100));
        assert_eq!(candle.volume, 150);
        assert_eq!(candle.trade_stats.as_ref().unwrap().trade_count, 2);
        let closed = book.commit_active();
        assert!(book.active().is_empty());
        assert_eq!(book.histories()[&code].last(), Some(&closed[&code]));
        assert!(book.commit_active().is_empty());
        assert_eq!(book.histories()[&code].len(), 1);
    }
}
