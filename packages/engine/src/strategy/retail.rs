//! 散户 decide 内核（噪音到达 + 追涨、下跌抄底与亏损止损）。

use super::*;

/// 散户 decide 内核（噪音到达 + 追涨、下跌抄底与亏损止损）。
///
/// 关键修正（修复「只有一只股票有成交」）：散户**从全部股票中均匀随机选一只**下单，
/// 而非恒取 `first_key_value()`（那会让全部散户 NPC 的订单集中在字典序最小的那只股票——
/// BTreeMap<StockCode> 按 string 排序，首键固定——其余股票毫无散户流动性、无人撮合、价格不动）。
/// 随机选股经注入 RNG，保持确定性（同种子同输出，铁律三）。
pub(super) fn decide_retail(
    strategy: &StrategyData,
    market: &MarketView,
    own: &SelfView,
    rng: &mut dyn Rng,
    config: &GameConfig,
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
        let context = RetailDecisionContext::from_selected_stock(strategy, code.clone(), sv, own);
        match context.classify_chase(strategy) {
            RetailSignal::TakeProfit | RetailSignal::StopLoss => {
                if let Some(qty) =
                    a_share_sell_qty(strategy.order_size_mean, context.sellable_qty())
                {
                    return vec![Intent::PlaceLimit {
                        code: context.code,
                        side: Side::Sell,
                        price: LimitPrice::Lowest,
                        qty,
                    }];
                }
                // 当日买入后下跌仍受 A 股 T+1 约束，不能用止损信号绕过可卖数量。
                return Vec::new();
            }
            RetailSignal::ChaseBuy | RetailSignal::DipBuy => {
                let Some(qty) = affordable_buy_qty(
                    strategy.order_size_mean,
                    context.stock_view.daily_upper_limit,
                    own.cash,
                    config,
                ) else {
                    return Vec::new();
                };
                return vec![Intent::PlaceLimit {
                    code: context.code,
                    side: Side::Buy,
                    price: LimitPrice::Highest,
                    qty,
                }];
            }
            RetailSignal::Wait => return Vec::new(),
            RetailSignal::Noise => {}
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
        Side::Buy => LimitPrice::Highest,
        Side::Sell => LimitPrice::Lowest,
    };
    let qty = match side {
        Side::Buy => affordable_buy_qty(
            strategy.order_size_mean,
            sv.daily_upper_limit,
            own.cash,
            config,
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

/// 单次追势判断中同一股票的行情与本人持仓事实，不进入存档。
struct RetailDecisionContext<'a> {
    code: StockCode,
    stock_view: &'a StockView,
    minute_change: f64,
    position: Option<&'a PositionView>,
    cost_return: Option<f64>,
    effective_stop_threshold: f64,
    effective_dip_threshold: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RetailSignal {
    TakeProfit,
    ChaseBuy,
    StopLoss,
    DipBuy,
    Wait,
    Noise,
}

impl<'a> RetailDecisionContext<'a> {
    fn from_selected_stock(
        strategy: &StrategyData,
        code: StockCode,
        stock_view: &'a StockView,
        own: &'a SelfView,
    ) -> Self {
        let position = own.positions.get(&code);
        let cost_return = position
            .and_then(|position| position.cost_price)
            .filter(|cost| cost.cents() > 0)
            .map(|cost| {
                (stock_view.last_price.cents() - cost.cents()) as f64 / cost.cents() as f64
            });
        // 缺成本时 loss=0 与合法零 stop_loss_threshold 的 >= 比较均保留现行语义。
        let loss = cost_return.map_or(0.0, |value| (-value).max(0.0));
        let effective_stop_threshold =
            strategy.stop_loss_threshold * (1.0 + 0.35 * stock_view.order_book_imbalance);
        let effective_dip_threshold = strategy.dip_threshold
            * (1.0 - 0.25 * stock_view.order_book_imbalance)
            * (1.0 + 0.50 * (loss / strategy.stop_loss_threshold.max(f64::EPSILON)).min(1.0));
        Self {
            code,
            stock_view,
            minute_change: market_minute_price_change(stock_view).unwrap_or(0.0),
            position,
            cost_return,
            effective_stop_threshold,
            effective_dip_threshold,
        }
    }

    /// chase 的 RNG 门由调用方先抽样；这里只分类纯信号，不组装委托或读取随机数。
    fn classify_chase(&self, strategy: &StrategyData) -> RetailSignal {
        if self.minute_change > 0.0 {
            if self
                .cost_return
                .is_some_and(|value| value >= strategy.take_profit_threshold)
            {
                RetailSignal::TakeProfit
            } else if self.stock_view.relative_volume < strategy.volume_confirmation {
                RetailSignal::Wait
            } else {
                RetailSignal::ChaseBuy
            }
        } else if self.minute_change < 0.0 {
            let loss = self.cost_return.map_or(0.0, |value| (-value).max(0.0));
            if loss >= self.effective_stop_threshold {
                RetailSignal::StopLoss
            } else if -self.minute_change >= self.effective_dip_threshold {
                RetailSignal::DipBuy
            } else {
                RetailSignal::Wait
            }
        } else {
            RetailSignal::Noise
        }
    }

    fn sellable_qty(&self) -> u32 {
        self.position.map_or(0, |position| position.sellable_qty)
    }
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

#[cfg(test)]
mod decision_context_tests {
    use super::*;
    use std::collections::VecDeque;

    struct ScriptedRng {
        floats: VecDeque<f64>,
        indices: VecDeque<u32>,
    }

    impl Rng for ScriptedRng {
        fn next_f64(&mut self) -> f64 {
            self.floats.pop_front().expect("预设 f64 抽样不能增加")
        }

        fn next_range_u32(&mut self, lo: u32, hi: u32) -> u32 {
            let value = self.indices.pop_front().expect("预设选股抽样不能增加");
            assert!((lo..hi).contains(&value));
            value
        }
    }

    fn stock(last: i64, history: &[i64], imbalance: f64) -> StockView {
        StockView {
            best_bid: Some(Money::from_cents(last - 1)),
            best_ask: Some(Money::from_cents(last + 1)),
            last_price: Money::from_cents(last),
            max_buy_price: Money::from_cents(1100),
            daily_upper_limit: Money::from_cents(1100),
            min_sell_price: Money::from_cents(900),
            recent_prices: vec![Money::from_cents(1000), Money::from_cents(900)],
            recent_market_minute_prices: history.iter().copied().map(Money::from_cents).collect(),
            relative_volume: 1.0,
            order_book_imbalance: imbalance,
        }
    }

    fn own(code: &StockCode, cost: Option<i64>, sellable: u32) -> SelfView {
        SelfView {
            cash: Money::from_cents(1_000_000),
            positions: [(
                code.clone(),
                PositionView {
                    qty: 100,
                    sellable_qty: sellable,
                    cost_price: cost.map(Money::from_cents),
                },
            )]
            .into(),
        }
    }

    fn assert_decision(
        strategy: &StrategyData,
        stock: StockView,
        cost: Option<i64>,
        sellable: u32,
        floats: &[f64],
        expected: Option<(Side, u32)>,
    ) {
        let code = StockCode("600101".to_owned());
        let market = MarketView {
            stocks: [(code.clone(), stock)].into(),
            tick: 0,
            market_minute: 0,
        };
        let mut rng = ScriptedRng {
            floats: floats.iter().copied().collect(),
            indices: [0].into(),
        };
        let intents = decide_retail(
            strategy,
            &market,
            &own(&code, cost, sellable),
            &mut rng,
            &GameConfig::proposed_defaults(),
        );
        match expected {
            None => assert!(intents.is_empty()),
            Some((side, qty)) => assert!(matches!(intents.as_slice(),
                [Intent::PlaceLimit { code: actual_code, side: actual_side, price, qty: actual_qty }]
                if actual_code == &code && *actual_side == side && *actual_qty == qty
                    && *price == if side == Side::Buy { LimitPrice::Highest } else { LimitPrice::Lowest }
            )),
        }
        assert!(rng.floats.is_empty());
        assert!(rng.indices.is_empty());
    }

    #[test]
    fn chase_profit_takes_only_sellable_shares_and_invalid_cost_keeps_buy_signal() {
        let strategy = StrategyData::retail(1.0, 100, 1.0);
        for (cost, sellable, expected) in [
            (Some(1000), 50, Some((Side::Sell, 50))),
            (Some(1000), 0, None),
            (None, 100, Some((Side::Buy, 100))),
            (Some(0), 100, Some((Side::Buy, 100))),
            (Some(-1), 100, Some((Side::Buy, 100))),
        ] {
            assert_decision(
                &strategy,
                stock(1100, &[1000, 1100], 0.0),
                cost,
                sellable,
                &[0.0, 0.0],
                expected,
            );
        }
    }

    #[test]
    fn falling_chase_preserves_stop_dip_and_zero_stop_boundaries() {
        let strategy = StrategyData::retail(1.0, 100, 1.0);
        for (last, cost, sellable, imbalance, expected) in [
            (950, Some(1000), 50, 0.0, Some((Side::Sell, 50))),
            (950, Some(1000), 0, 0.0, None),
            (951, Some(1000), 100, 0.0, Some((Side::Buy, 100))),
            (980, None, 100, 0.0, Some((Side::Buy, 100))),
            (981, None, 100, 0.0, None),
            (980, None, 100, -1.0, None),
            (975, None, 100, -1.0, Some((Side::Buy, 100))),
        ] {
            assert_decision(
                &strategy,
                stock(last, &[1000, last], imbalance),
                cost,
                sellable,
                &[0.0, 0.0],
                expected,
            );
        }
        let mut zero_stop = strategy;
        zero_stop.stop_loss_threshold = 0.0;
        assert_decision(
            &zero_stop,
            stock(980, &[1000, 980], 0.0),
            None,
            50,
            &[0.0, 0.0],
            Some((Side::Sell, 50)),
        );
    }

    #[test]
    fn incomplete_minute_history_keeps_noise_branch_and_draw_order() {
        for history in [&[][..], &[1000][..], &[0, 900][..]] {
            assert_decision(
                &StrategyData::retail(1.0, 100, 1.0),
                stock(900, history, 0.0),
                Some(1000),
                0,
                &[0.0, 0.0, 0.25],
                Some((Side::Buy, 100)),
            );
        }
    }

    #[test]
    fn noise_sell_reselects_its_own_stock_view_and_position() {
        let first = StockCode("600101".to_owned());
        let second = StockCode("600102".to_owned());
        let market = MarketView {
            stocks: [
                (first, stock(1000, &[], 0.0)),
                (second.clone(), stock(900, &[], 0.0)),
            ]
            .into(),
            tick: 0,
            market_minute: 0,
        };
        let mut rng = ScriptedRng {
            floats: [0.0, 0.0, 0.75].into(),
            indices: [0, 0].into(),
        };
        let intents = decide_retail(
            &StrategyData::retail(1.0, 100, 0.0),
            &market,
            &own(&second, None, 50),
            &mut rng,
            &GameConfig::proposed_defaults(),
        );
        assert!(matches!(intents.as_slice(),
            [Intent::PlaceLimit { code, side: Side::Sell, price: LimitPrice::Lowest, qty: 50 }] if code == &second
        ));
        assert!(rng.floats.is_empty());
        assert!(rng.indices.is_empty());
    }
}
