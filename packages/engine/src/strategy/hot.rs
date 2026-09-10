//! 游资 decide 内核（动量/反转）。

use super::*;

/// 游资 decide 内核（动量策略）。趋势窗口以完整交易分钟计，不受宿主 tick 密度影响。
pub(super) fn decide_hot(
    strategy: &StrategyData,
    market: &MarketView,
    own: &SelfView,
) -> Vec<Intent> {
    let mut out = Vec::new();
    for (code, sv) in &market.stocks {
        let p = &sv.recent_market_minute_prices;
        if p.len() < 2 {
            continue;
        }
        let start = p.len().saturating_sub(strategy.lookback);
        let first = p[start].cents() as f64;
        let last = p.last().unwrap().cents() as f64;
        if first <= 0.0 {
            continue;
        }
        let change = (last - first) / first;
        if change > strategy.trend_threshold
            && sv.relative_volume >= strategy.volume_confirmation
            && sv.order_book_imbalance > -0.50
        {
            let price = sv.best_ask.unwrap_or(sv.last_price);
            if let Some(qty) = risk_capped_buy_qty(
                strategy.order_size,
                code,
                price,
                market,
                own,
                strategy.max_stock_fraction,
            ) {
                out.push(Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price,
                    qty,
                });
            }
        } else if change < -strategy.trend_threshold {
            let sellable = own
                .positions
                .get(code)
                .map(|pp| pp.sellable_qty)
                .unwrap_or(0);
            if let Some(qty) = a_share_sell_qty(strategy.order_size, sellable) {
                out.push(Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Sell,
                    price: sv.best_bid.unwrap_or(sv.last_price),
                    qty,
                });
            }
        }
    }
    out
}

/// 反转型游资：放量急跌时承接，放量急涨且已有可卖库存时兑现。
pub(super) fn decide_hot_reversal(
    strategy: &StrategyData,
    market: &MarketView,
    own: &SelfView,
) -> Vec<Intent> {
    let mut out = Vec::new();
    for (code, sv) in &market.stocks {
        let prices = &sv.recent_market_minute_prices;
        if prices.len() < 2 || sv.relative_volume < strategy.volume_confirmation {
            continue;
        }
        let start = prices.len().saturating_sub(strategy.lookback);
        let first = prices[start].cents() as f64;
        let last = prices.last().expect("non-empty price window").cents() as f64;
        if first <= 0.0 {
            continue;
        }
        let change = (last - first) / first;
        if change < -strategy.trend_threshold && sv.order_book_imbalance < 0.50 {
            let price = sv.best_bid.unwrap_or(sv.last_price);
            if let Some(qty) = risk_capped_buy_qty(
                strategy.order_size,
                code,
                price,
                market,
                own,
                strategy.max_stock_fraction,
            ) {
                out.push(Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price,
                    qty,
                });
            }
        } else if change > strategy.trend_threshold {
            let sellable = own
                .positions
                .get(code)
                .map(|position| position.sellable_qty)
                .unwrap_or(0);
            if let Some(qty) = a_share_sell_qty(strategy.order_size, sellable) {
                out.push(Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Sell,
                    price: sv.best_ask.unwrap_or(sv.last_price),
                    qty,
                });
            }
        }
    }
    out
}
