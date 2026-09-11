//! 策略观察视图接缝：市场视图与散户行为/风险观测构建（W1-Task 3 抽出）。

use super::*;

impl GameSession {
    /// 构建公共市场视图（所有决策者同构输入；不存在按账户身份授予的隐藏信息）。
    ///
    /// 遍历所有 markets，每股取 best_bid/best_ask/last_price，并把 tick 级 `price_history`
    /// 与已完成的标准交易分钟收盘分别拷入视图。游资趋势只读取后者，因此宿主 tick
    /// 密度不会改变其观察时间跨度。产 owned [`MarketView`]（不持 `&self` 借用），便于
    /// 随后安全地 `self.accounts.get_mut`。
    pub(super) fn build_market_view(&self) -> MarketView {
        let mut stocks = BTreeMap::new();
        for (code, m) in &self.markets {
            let hist: Vec<Money> = self
                .price_history
                .get(code)
                .map(|d| d.iter().copied().collect())
                .expect("every market must have a price-history queue");
            let completed_minute_prices: Vec<Money> = self
                .market_minute_closes
                .get(code)
                .expect("every market must have canonical minute history")
                .iter()
                .map(|sample| sample.close)
                .collect();
            let historical = self
                .daily_candles
                .get(code)
                .expect("every market must have authoritative daily candles");
            let sample_count = historical.len().min(20);
            let average_daily_volume = if sample_count == 0 {
                0.0
            } else {
                historical
                    .iter()
                    .rev()
                    .take(sample_count)
                    .map(|candle| candle.volume as f64)
                    .sum::<f64>()
                    / sample_count as f64
            };
            let current_volume = self
                .active_daily_candles
                .get(code)
                .map_or(0, |candle| candle.volume) as f64;
            let elapsed_fraction = intraday_expected_volume_fraction(
                self.tick % self.setup.ticks_per_day + 1,
                self.setup.ticks_per_day,
                self.setup.auction_ticks,
            );
            let expected_volume = average_daily_volume * elapsed_fraction;
            let relative_volume = if expected_volume > 0.0 {
                current_volume / expected_volume
            } else {
                0.0
            };
            let bid_volume: u64 = m
                .bid_depth()
                .into_iter()
                .take(5)
                .map(|(_, quantity)| quantity)
                .sum();
            let ask_volume: u64 = m
                .ask_depth()
                .into_iter()
                .take(5)
                .map(|(_, quantity)| quantity)
                .sum();
            let depth_total = bid_volume.saturating_add(ask_volume);
            let order_book_imbalance = if depth_total == 0 {
                0.0
            } else {
                (bid_volume as f64 - ask_volume as f64) / depth_total as f64
            };
            stocks.insert(
                code.clone(),
                StockView {
                    best_bid: m.best_bid(),
                    best_ask: m.best_ask(),
                    last_price: m.last_price(),
                    recent_prices: hist,
                    recent_market_minute_prices: completed_minute_prices,
                    relative_volume,
                    order_book_imbalance,
                },
            );
        }
        MarketView {
            stocks,
            tick: self.tick,
            market_minute: self.current_market_minute(),
        }
    }

    /// 按当前权威分钟序列与已完成日 K 构造每股公共价格路径观测。
    ///
    /// 该函数不消耗 RNG、不改变会话，也不读取宿主墙钟或 UI/Publisher 状态。
    pub fn market_price_path_observations(
        &self,
    ) -> Result<BTreeMap<StockCode, PricePathObservation>, ObservationError> {
        self.markets
            .keys()
            .map(|code| {
                let minutes = self
                    .market_minute_closes
                    .get(code)
                    .expect("every market must have canonical minute history");
                let retained_daily = self
                    .daily_candles
                    .get(code)
                    .expect("every market must have authoritative daily candles");
                // 行为窗口最长 250 个已完成交易日；长局不能在每个观察 tick
                // 重复制和重校验数千日历史。绝对交易日序号仍被保留。
                let daily = retained_behavior_daily_closes(retained_daily);
                let current_day_open = self
                    .active_daily_candles
                    .get(code)
                    .filter(|candle| candle.volume > 0)
                    .map(|candle| candle.open);
                build_price_path_observation(minutes, &daily, current_day_open)
                    .map(|observation| (code.clone(), observation))
            })
            .collect()
    }

    pub(super) fn behavior_market_observation(&self) -> BehaviorMarketObservation {
        let price_paths = self
            .market_price_path_observations()
            .expect("validated authoritative histories must produce behavior observations");
        let thirty_minute_returns = price_paths
            .iter()
            .map(|(code, path)| (code.clone(), path.thirty_minute.return_ratio))
            .collect();
        let thirty_minute_market = build_equal_weight_market_observation(&thirty_minute_returns)
            .expect("validated market returns must produce equal-weight market observation");
        BehaviorMarketObservation {
            price_paths,
            thirty_minute_market,
        }
    }

    pub(super) fn account_risk_observations_for(
        &self,
        ids: &[AccountId],
    ) -> BTreeMap<AccountId, AccountRiskObservation> {
        ids.iter()
            .filter_map(|id| {
                let account = self
                    .accounts
                    .get(id)
                    .expect("risk observations may only be built for existing accounts");
                if account.kind != AccountKind::Retail {
                    return None;
                }
                let positions = account
                    .positions
                    .iter()
                    .map(|(code, position)| {
                        let last_price = self
                            .markets
                            .get(code)
                            .unwrap_or_else(|| {
                                panic!("account {} holds unknown stock {}", id.0, code.0)
                            })
                            .last_price();
                        (
                            code.clone(),
                            RiskPositionInput {
                                qty: position.qty,
                                // 已实现盈利可能使剩余持仓的净成本降到零或以下；此时成本
                                // 收益率没有合法正分母，按 ADR-0011 显式记为不可用。
                                cost_price: position.cost_price().filter(|price| price.cents() > 0),
                                last_price,
                                peak_price_since_entry: self
                                    .retail_experience
                                    .get(id)
                                    .and_then(|experience| experience.stocks.get(code))
                                    .and_then(|stock| stock.peak_price_since_entry),
                            },
                        )
                    })
                    .collect();
                let experience = self.retail_experience.get(id).unwrap_or_else(|| {
                    panic!("retail account {} is missing experience state", id.0)
                });
                let risk = build_account_risk_observation(
                    account.cash,
                    &positions,
                    experience.reference_equity,
                    experience.peak_equity,
                )
                .unwrap_or_else(|error| {
                    panic!("account {} risk observation failed: {error}", id.0)
                });
                Some((*id, risk))
            })
            .collect()
    }
}
