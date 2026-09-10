//! A 股申报单位与策略层风险预算共用助手。

use super::*;

/// 按 A 股 100 股申报单位把机构计划单拆成试探/加仓档位。
pub(super) fn a_share_tranche(order_size: u32, divisor: u32) -> Option<u32> {
    const LOT: u32 = 100;
    if order_size < LOT {
        return None;
    }
    let raw = (order_size / divisor).max(LOT);
    Some(raw - raw % LOT)
}

/// 卖出量与会话边界使用同一规则：整手之外，可一次性带走全部可卖零股余量。
pub(crate) fn a_share_sell_qty(order_size: u32, sellable: u32) -> Option<u32> {
    const LOT: u32 = 100;
    let requested = order_size.min(sellable);
    let odd_lot = sellable % LOT;
    let board_lot_candidate = requested - requested % LOT;
    let odd_lot_candidate = if requested >= odd_lot && odd_lot > 0 {
        odd_lot + ((requested - odd_lot) / LOT) * LOT
    } else {
        0
    };
    let qty = board_lot_candidate.max(odd_lot_candidate);
    (qty > 0).then_some(qty)
}

/// 在策略层先约束现金与单股暴露，账户层仍保留最终费用及冻结校验。
pub(crate) fn risk_capped_buy_qty(
    desired_qty: u32,
    code: &StockCode,
    price: Money,
    market: &MarketView,
    own: &SelfView,
    max_stock_fraction: f64,
) -> Option<u32> {
    const LOT: u32 = 100;
    const CASH_RESERVE_CENTS: i64 = 1_000;
    if price.cents() <= 0
        || !max_stock_fraction.is_finite()
        || !(0.0..=1.0).contains(&max_stock_fraction)
    {
        return None;
    }
    let cash = i128::from(own.cash.cents().max(0));
    let positions_value = own
        .positions
        .iter()
        .try_fold(0_i128, |total, (stock, position)| {
            let last = market.stocks.get(stock)?.last_price.cents();
            (last > 0).then(|| total + i128::from(last) * i128::from(position.qty))
        })?;
    let equity = cash + positions_value;
    let current_value = own.positions.get(code).map_or(0_i128, |position| {
        i128::from(price.cents()) * i128::from(position.qty)
    });
    // 单股测试局不能因为“分散化”而失去全部买方；股票越少，最低允许权重越高。
    let universe_floor = 1.0 / market.stocks.len() as f64;
    let effective_fraction = max_stock_fraction.max(universe_floor).min(1.0);
    let exposure_limit = (equity as f64 * effective_fraction).floor() as i128;
    let exposure_room = exposure_limit.saturating_sub(current_value);
    let spendable_cash = cash
        .saturating_sub(i128::from(CASH_RESERVE_CENTS))
        .min(cash * 99 / 100);
    let price_cents = i128::from(price.cents());
    let affordable = spendable_cash.max(0) / price_cents;
    let exposure_capacity = exposure_room.max(0) / price_cents;
    let capacity = affordable.min(exposure_capacity).min(i128::from(u32::MAX)) as u32;
    let qty = desired_qty.min(capacity);
    let board_lot_qty = qty - qty % LOT;
    (board_lot_qty >= LOT).then_some(board_lot_qty)
}
