//! 散户 decide 内核（噪音到达 + 追涨、下跌抄底与亏损止损）。

use super::*;

/// 散户 decide 内核（噪音到达 + 追涨、下跌抄底与亏损止损）。
///
/// 关键修正（修复「只有一只股票有成交」）：散户**从全部股票中均匀随机选一只**下单，
/// 而非恒取 `first_key_value()`（旧实现会让全部散户 NPC 的订单集中在字典序最小的那只股票——
/// BTreeMap<StockCode> 按 string 排序，首键固定——其余股票毫无散户流动性、无人撮合、价格不动）。
/// 随机选股经注入 RNG，保持确定性（同种子同输出，铁律三）。
pub(super) fn decide_retail(
    strategy: &StrategyData,
    market: &MarketView,
    own: &SelfView,
    rng: &mut dyn Rng,
) -> Vec<Intent> {
    if market.stocks.is_empty() {
        return Vec::new();
    }
    // 从全部股票中均匀随机选一只（注入 RNG，确定性）。BTreeMap 无随机访问 → 先按 key 取索引。
    let n = market.stocks.len();
    let idx = rng.next_range_u32(0, n as u32) as usize;
    let (code, sv) = match market.stocks.keys().nth(idx) {
        Some(c) => {
            let v = market
                .stocks
                .get(c)
                .expect("key 刚从同一 BTreeMap 取出，必存在（防御式：不可达则显式 panic）");
            (c.clone(), v)
        }
        None => return Vec::new(),
    };
    let change = market_minute_price_change(sv).unwrap_or(0.0);
    let volume_activity = 0.5
        + 0.5 * sv.relative_volume.clamp(0.0, 1.0)
        + 0.25 * sv.order_book_imbalance.abs().clamp(0.0, 1.0);
    let price_activity = if strategy.dip_threshold > 0.0 {
        (change.abs() / strategy.dip_threshold).min(1.0)
    } else {
        0.0
    };
    let effective_arrival = (strategy.arrival_rate * (volume_activity + price_activity)).min(1.0);
    if rng.next_f64() >= effective_arrival {
        return Vec::new();
    }
    if rng.next_f64() < strategy.chase_prob {
        let position = own.positions.get(&code);
        let pnl = position
            .and_then(|p| p.cost_price)
            .filter(|cost| cost.cents() > 0)
            .map(|cost| (sv.last_price.cents() - cost.cents()) as f64 / cost.cents() as f64);
        if change > 0.0 {
            if pnl.is_some_and(|value| value >= strategy.take_profit_threshold) {
                let sellable = position.map_or(0, |p| p.sellable_qty);
                if let Some(qty) = a_share_sell_qty(strategy.order_size_mean, sellable) {
                    return vec![Intent::PlaceLimit {
                        code,
                        side: Side::Sell,
                        price: sv.best_bid.unwrap_or(sv.last_price),
                        qty,
                    }];
                }
                return Vec::new();
            }
            if sv.relative_volume < strategy.volume_confirmation {
                return Vec::new();
            }
            let price = sv.best_ask.unwrap_or(sv.last_price);
            let Some(qty) = risk_capped_buy_qty(
                strategy.order_size_mean,
                &code,
                price,
                market,
                own,
                strategy.max_stock_fraction,
            ) else {
                return Vec::new();
            };
            return vec![Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price,
                qty,
            }];
        } else if change < 0.0 {
            let loss = pnl.map_or(0.0, |value| (-value).max(0.0));
            let effective_stop_threshold =
                strategy.stop_loss_threshold * (1.0 + 0.35 * sv.order_book_imbalance);
            if loss >= effective_stop_threshold {
                let sellable = position.map(|p| p.sellable_qty).unwrap_or(0);
                if let Some(qty) = a_share_sell_qty(strategy.order_size_mean, sellable) {
                    return vec![Intent::PlaceLimit {
                        code,
                        side: Side::Sell,
                        price: sv.best_bid.unwrap_or(sv.last_price),
                        qty,
                    }];
                }
                // 当日抄底后继续下跌：受 A 股 T+1 约束，只能等待下一交易日再止损。
                return Vec::new();
            }
            let effective_dip_threshold = strategy.dip_threshold
                * (1.0 - 0.25 * sv.order_book_imbalance)
                * (1.0 + 0.50 * (loss / strategy.stop_loss_threshold.max(f64::EPSILON)).min(1.0));
            if -change >= effective_dip_threshold {
                // 不知道内在价值的散户把足够大的跌幅当作“变便宜”，以小单主动吃卖一试探。
                let price = sv.best_ask.unwrap_or(sv.last_price);
                let Some(qty) = risk_capped_buy_qty(
                    strategy.order_size_mean,
                    &code,
                    price,
                    market,
                    own,
                    strategy.max_stock_fraction,
                ) else {
                    return Vec::new();
                };
                return vec![Intent::PlaceLimit {
                    code,
                    side: Side::Buy,
                    price,
                    qty,
                }];
            }
            return Vec::new();
        }
    }
    let side = if rng.next_f64() < 0.5 {
        Side::Buy
    } else {
        Side::Sell
    };
    let (code, sv) = if side == Side::Sell {
        let sellable_codes: Vec<&StockCode> = market
            .stocks
            .keys()
            .filter(|candidate| {
                own.positions
                    .get(*candidate)
                    .is_some_and(|position| position.sellable_qty > 0)
            })
            .collect();
        if sellable_codes.is_empty() {
            return Vec::new();
        }
        let selected =
            sellable_codes[rng.next_range_u32(0, sellable_codes.len() as u32) as usize].clone();
        let selected_view = market
            .stocks
            .get(&selected)
            .expect("sellable stock must exist in the same market view");
        (selected, selected_view)
    } else {
        (code, sv)
    };
    let price = match side {
        Side::Buy => {
            Money::from_cents(sv.best_bid.unwrap_or(sv.last_price).cents() + strategy.tick_cents)
        }
        Side::Sell => Money::from_cents(
            (sv.best_ask.unwrap_or(sv.last_price).cents() - strategy.tick_cents).max(0),
        ),
    };
    let qty = match side {
        Side::Buy => risk_capped_buy_qty(
            strategy.order_size_mean,
            &code,
            price,
            market,
            own,
            strategy.max_stock_fraction,
        ),
        Side::Sell => own
            .positions
            .get(&code)
            .and_then(|position| a_share_sell_qty(strategy.order_size_mean, position.sellable_qty)),
    };
    qty.map_or_else(Vec::new, |qty| {
        vec![Intent::PlaceLimit {
            code,
            side,
            price,
            qty,
        }]
    })
}

/// 完整交易分钟收盘序列的相对变化（至少 2 个点，首价必须为正）。
///
/// 散户的追涨、抄底和止损不把宿主 tick 密度当作行情经历；tick 级 `recent_prices`
/// 仅可表达盘口/注意力等即时观测。
fn market_minute_price_change(sv: &StockView) -> Option<f64> {
    let p = &sv.recent_market_minute_prices;
    let first = p.first()?.cents();
    let last = p.last()?.cents();
    (p.len() >= 2 && first > 0).then(|| (last - first) as f64 / first as f64)
}
