//! 权威日 K 的生成、盘中维护与收盘归档。

use super::*;

impl GameSession {
    pub(super) fn update_active_daily_candle(
        &mut self,
        code: &StockCode,
        price: Money,
        added_volume: u64,
    ) {
        let time = i64::from(self.day) * SECONDS_PER_DAY;
        let candle = self
            .active_daily_candles
            .entry(code.clone())
            .or_insert(DailyCandle {
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

    pub(super) fn commit_active_daily_candles(&mut self) -> BTreeMap<StockCode, DailyCandle> {
        let closed = std::mem::take(&mut self.active_daily_candles);
        for (code, candle) in &closed {
            let history = self.daily_candles.entry(code.clone()).or_default();
            history.push(candle.clone());
            if history.len() > PRESET_HISTORY_DAYS {
                history.drain(..history.len() - PRESET_HISTORY_DAYS);
            }
        }
        closed
    }
}

const PRESET_HISTORY_DAYS: usize = 360;
const SECONDS_PER_DAY: i64 = 86_400;

/// 生成与 session RNG 隔离的确定性历史，避免预置行情改变 NPC 的随机序列。
pub(super) fn generate_preset_daily_candles(
    setup: &SessionSetup,
    seed: u64,
) -> BTreeMap<StockCode, Vec<DailyCandle>> {
    setup
        .stocks
        .iter()
        .map(|stock| {
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
                    time: -((recency as i64) + 1) * SECONDS_PER_DAY,
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
            (stock.code.clone(), newest_first)
        })
        .collect()
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
