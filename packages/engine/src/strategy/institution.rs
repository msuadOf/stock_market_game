//! 机构 decide 内核（基本面价值策略）。

use super::*;

use super::sizing::a_share_tranche;
use super::value::target_cents;

/// 机构 decide 内核（基本面价值策略）：按可成交报价试探并随折价分档加仓。
/// DriftUp 使用权威标准交易分钟，不维护策略私有时钟。
pub(super) fn decide_inst(
    strategy: &StrategyData,
    market: &MarketView,
    own: &SelfView,
) -> Vec<Intent> {
    let mut out = Vec::new();
    for (code, sv) in &market.stocks {
        let target = match target_cents(
            &strategy.target_policy,
            sv.fundamental_value,
            market.market_minute.saturating_add(1),
        ) {
            Some(t) => t,
            None => continue,
        };
        let buy_price = sv.best_ask.unwrap_or(sv.last_price);
        let buy_quote = buy_price.cents() as f64;
        let sell_price = sv.best_bid.unwrap_or(sv.last_price);
        let sell_quote = sell_price.cents() as f64;
        let low = target * (1.0 - strategy.margin);
        let high = target * (1.0 + strategy.margin);
        if buy_quote < low {
            let discount = (target - buy_quote) / target;
            let divisor = if discount < strategy.margin * 2.0 {
                4
            } else if discount < strategy.margin * 4.0 {
                2
            } else {
                1
            };
            if let Some(qty) = a_share_tranche(strategy.order_size, divisor).and_then(|qty| {
                risk_capped_buy_qty(
                    qty,
                    code,
                    buy_price,
                    market,
                    own,
                    strategy.max_stock_fraction,
                )
            }) {
                out.push(Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price: buy_price,
                    qty,
                });
            }
        } else if sell_quote > high {
            let sellable = own.positions.get(code).map(|p| p.sellable_qty).unwrap_or(0);
            if let Some(qty) = a_share_sell_qty(strategy.order_size, sellable) {
                out.push(Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Sell,
                    price: sell_price,
                    qty,
                });
            }
        }
    }
    out
}
