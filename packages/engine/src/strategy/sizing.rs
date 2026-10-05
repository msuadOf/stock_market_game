//! A 股申报单位与真实现金负担能力共用助手。

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

/// 将策略选择的数量缩到现金能够支付的整手数，包括当前会话的真实买入费用。
/// 账户层仍负责冻结、同账户请求之间的资金竞争和最终受理。
pub(crate) fn affordable_buy_qty(
    desired_qty: u32,
    price: Money,
    cash: Money,
    config: &crate::config::GameConfig,
) -> Option<u32> {
    use crate::config::{buy_order_reservation, A_SHARE_BOARD_LOT};
    use crate::money::MoneyError;

    if price.cents() <= 0 || cash.cents() <= 0 {
        return None;
    }
    assert!(
        config.commission_rate.is_finite()
            && config.commission_rate >= 0.0
            && config.commission_min >= Money::ZERO,
        "buy sizing requires a validated nonnegative commission configuration"
    );
    let gross_capacity = (cash.cents() / price.cents()).min(i64::from(desired_qty)) as u32;
    let mut lower_lots = 0;
    let mut upper_lots = gross_capacity / A_SHARE_BOARD_LOT;
    while lower_lots < upper_lots {
        let candidate_lots = lower_lots + (upper_lots - lower_lots).div_ceil(2);
        let candidate_qty = candidate_lots * A_SHARE_BOARD_LOT;
        let affordable = match buy_order_reservation(config, price, candidate_qty, Money::ZERO) {
            Ok(required) => required <= cash,
            // 上界已保证成交额不超过现金。非负费用之和超出 Money 范围时，
            // 该数量必定买不起；继续找更小的整手数，不把它当成零费用。
            Err(MoneyError::Overflow { .. }) => false,
            Err(error) => panic!("buy sizing cannot calculate order fees: {error}"),
        };
        if affordable {
            lower_lots = candidate_lots;
        } else {
            upper_lots = candidate_lots - 1;
        }
    }
    (lower_lots > 0).then_some(lower_lots * A_SHARE_BOARD_LOT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn market(stock_count: u32, price: Money) -> MarketView {
        MarketView {
            stocks: (0..stock_count)
                .map(|index| {
                    (
                        StockCode(format!("600{index:03}")),
                        StockView {
                            is_trading: true,
                            best_bid: Some(price),
                            best_ask: Some(price),
                            last_price: price,
                            max_buy_price: Money::from_cents(price.cents() * 11 / 10),
                            daily_upper_limit: Money::from_cents(price.cents() * 11 / 10),
                            min_sell_price: Money::from_cents(price.cents() * 9 / 10),
                            recent_prices: vec![price],
                            recent_market_minute_prices: vec![price],
                            relative_volume: 1.0,
                            order_book_imbalance: 0.0,
                        },
                    )
                })
                .collect(),
            tick: 0,
            market_minute: 0,
        }
    }

    #[test]
    fn buy_uses_cash_up_to_actual_fees_without_an_extra_reserve() {
        let price = Money::from_cents(1_000);
        // 100 股 × 10 元，佣金 5 元，过户费 0.01 元。
        let mut own = SelfView {
            cash: Money::from_cents(100_501),
            positions: BTreeMap::new(),
        };
        let config = crate::GameConfig::proposed_defaults();
        assert_eq!(
            affordable_buy_qty(100, price, own.cash, &config),
            Some(100),
            "足额支付成交额及真实费用即可买入，不应额外保留 1% 或 10 元"
        );

        own.cash = Money::from_cents(100_500);
        assert_eq!(
            affordable_buy_qty(100, price, own.cash, &config),
            None,
            "取消固定留底仍必须覆盖交易费用"
        );
    }

    #[test]
    fn funded_buy_is_independent_of_existing_concentration_and_market_stock_count() {
        let price = Money::from_cents(1_000);
        let code = StockCode("600000".to_owned());
        let own = SelfView {
            cash: Money::from_cents(200_000),
            positions: [(
                code.clone(),
                PositionView {
                    qty: 800,
                    sellable_qty: 800,
                    cost_price: Some(price),
                },
            )]
            .into(),
        };
        for stock_count in [1, 2, 5] {
            let mut strategy = MomentumStrategy::new(2, 0.01, 100).unwrap();
            let mut market = market(stock_count, price);
            market
                .stocks
                .get_mut(&code)
                .unwrap()
                .recent_market_minute_prices = vec![Money::from_cents(900), price];
            let intents = strategy.decide(
                &market,
                &own,
                &mut crate::SplitMix64::new(7),
                &crate::GameConfig::proposed_defaults(),
            );
            assert_eq!(
                intents.len(),
                1,
                "策略已选择加仓时，80% 的已有单股持仓与市场股票数 {stock_count} 不得阻止有资金支持的买单"
            );
            assert!(
                matches!(&intents[0], Intent::PlaceLimit { code: actual, side: Side::Buy, qty: 100, .. } if actual == &code)
            );
        }
    }

    #[test]
    fn buy_quantity_pays_configured_fees_and_selects_the_largest_funded_board_lot() {
        let mut config = crate::GameConfig::proposed_defaults();
        config.commission_rate = 0.10;
        config.commission_min = Money::ZERO;
        let price = Money::from_cents(1_000);
        let cash = Money::from_cents(220_002);
        assert_eq!(affordable_buy_qty(350, price, cash, &config), Some(200));
        assert_eq!(
            affordable_buy_qty(350, price, Money::from_cents(cash.cents() - 1), &config),
            Some(100)
        );
        assert_eq!(affordable_buy_qty(150, price, cash, &config), Some(100));
    }

    #[test]
    fn buy_can_spend_all_cash_when_the_order_has_no_fees() {
        let mut config = crate::GameConfig::proposed_defaults();
        config.commission_rate = 0.0;
        config.commission_min = Money::ZERO;
        // 成交额 1 元，过户费按现行 Money 舍入为零。
        assert_eq!(
            affordable_buy_qty(100, Money::from_cents(1), Money::from_cents(100), &config),
            Some(100)
        );
        assert_eq!(
            affordable_buy_qty(100, Money::from_cents(1), Money::ZERO, &config),
            None
        );
    }

    #[test]
    fn fee_overflow_cannot_turn_into_a_free_buy() {
        let mut config = crate::GameConfig::proposed_defaults();
        config.commission_rate = f64::MAX;
        config.validate().unwrap();
        assert_eq!(
            affordable_buy_qty(
                100,
                Money::from_cents(100),
                Money::from_cents(10_000),
                &config
            ),
            None
        );
    }
}
