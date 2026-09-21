//! 双时钟写入方法：把真实成交与本人观察登记进经历反馈事实。
//!
//! 每个方法都是「纯守卫 → legacy 语义原样执行 → 提交段」三段式：任一拒绝
//! 都不动 feedback 状态；legacy 成功后才推进账户时钟并登记生命周期/退出
//! 历史/受挫事件。失败确认严格镜像 legacy 计数增点，不新增任何计数路径。

use super::*;

impl RetailExperienceState {
    /// 开局已分配持仓（真实持仓，非游戏内成交）：登记生命周期与首次本人所见。
    pub fn initialize_holding_dated(
        &mut self,
        code: &StockCode,
        entry_reference_price: Option<Money>,
        current_price: Money,
        moment: ExperienceMoment,
    ) -> Result<(), ExperienceError> {
        require_positive("initial holding price", current_price)?;
        if let Some(reference) = entry_reference_price {
            require_positive("initial entry reference", reference)?;
        }
        self.feedback.ensure_moment_forward(moment)?;
        if self.feedback.stocks.contains_key(code) {
            return Err(ExperienceError::ActiveEntryAlreadyExists {
                code: code.0.clone(),
            });
        }
        self.initialize_holding(
            code,
            entry_reference_price,
            current_price,
            moment.market_minute,
        )?;
        self.feedback.advance_clocks(moment);
        self.feedback.stocks.insert(
            code.clone(),
            HoldingEpoch {
                entry_moment: moment,
                last_own_observation: Some(OwnObservation {
                    price: current_price,
                    moment,
                }),
            },
        );
        Ok(())
    }

    /// 双时钟成交：legacy 心理语义（订单去重/冷静期/盈亏恢复）原样保留，
    /// 同时登记持仓生命周期、退出历史与受挫事件日期。加仓/减仓必须发生在
    /// 已有生命周期之内；从零建仓必须没有活跃生命周期（否则说明退出/入场
    /// 事件丢失，类型化拒绝）。
    #[allow(clippy::too_many_arguments)]
    pub fn record_fill_dated(
        &mut self,
        code: &StockCode,
        side: Side,
        price: Money,
        before_qty: u32,
        after_qty: u32,
        cost_before: Option<Money>,
        order_id: Option<u64>,
        moment: ExperienceMoment,
    ) -> Result<(), ExperienceError> {
        require_positive("fill price", price)?;
        if let Some(cost) = cost_before {
            require_positive("cost before fill", cost)?;
        }
        self.feedback.ensure_moment_forward(moment)?;
        let has_epoch = self.feedback.stocks.contains_key(code);
        match side {
            Side::Buy if before_qty == 0 && has_epoch => {
                // 上一段持仓从未清仓却又从零建仓：退出生命周期已丢失。
                return Err(ExperienceError::ActiveEntryAlreadyExists {
                    code: code.0.clone(),
                });
            }
            Side::Buy if before_qty > 0 && !has_epoch => {
                // 加仓没有入场生命周期：入场事件已丢失。
                return Err(ExperienceError::NoActiveEntry {
                    code: code.0.clone(),
                });
            }
            Side::Sell if !has_epoch => {
                // 减仓/清仓没有入场生命周期：入场事件已丢失。
                return Err(ExperienceError::NoActiveEntry {
                    code: code.0.clone(),
                });
            }
            _ => {}
        }
        let stock_before = self.stocks.get(code);
        let first_fill_of_sell_order = order_id
            .is_none_or(|id| stock_before.is_some_and(|s| s.last_sell_order_id != Some(id)));
        // 失败确认镜像 legacy 增点：亏损卖出首次确认（同一买入回合只计一次）。
        let confirms_failure = side == Side::Sell
            && first_fill_of_sell_order
            && cost_before.is_some_and(|cost| price.cents() < cost.cents())
            && stock_before.is_some_and(|s| !s.adverse_move_recorded);
        let confirmed_buy_order = stock_before.and_then(|s| s.last_buy_order_id);
        let realized_profit =
            side == Side::Sell && cost_before.is_some_and(|cost| price.cents() > cost.cents());

        self.record_fill_with_order(
            code,
            side,
            price,
            before_qty,
            after_qty,
            cost_before,
            moment.market_minute,
            order_id,
        )?;

        // 提交段：legacy 成功后才动 feedback，任一拒绝都不留半截状态。
        self.feedback.advance_clocks(moment);
        if side == Side::Buy && before_qty == 0 {
            self.feedback.stocks.insert(
                code.clone(),
                HoldingEpoch {
                    entry_moment: moment,
                    last_own_observation: None,
                },
            );
        }
        self.feedback
            .stocks
            .get_mut(code)
            .expect("epoch existence is guarded above")
            .last_own_observation = Some(OwnObservation { price, moment });
        if side == Side::Sell && after_qty == 0 {
            let cooldown_until = self.stocks[code]
                .cooldown_until_market_minute
                .expect("legacy sell-to-zero always sets the post-exit cooldown");
            self.feedback.stocks.remove(code);
            self.feedback.exit_records.push(ExitRecord {
                code: code.clone(),
                cooldown_until_market_minute: cooldown_until,
                realized_profit,
                moment,
            });
        }
        if confirms_failure {
            self.feedback.failure_events.push(FailureEventRecord {
                code: code.clone(),
                order_id: confirmed_buy_order,
                moment,
            });
        }
        Ok(())
    }

    /// 双时钟持仓观察：legacy 峰值/不利判定原样保留，同时登记本人所见价与
    /// 受挫确认日期。仅限有持仓生命周期的股票；未持仓观察走关注列表语义。
    pub fn observe_position_dated(
        &mut self,
        code: &StockCode,
        price: Money,
        moment: ExperienceMoment,
    ) -> Result<(), ExperienceError> {
        require_positive("observed position price", price)?;
        self.feedback.ensure_moment_forward(moment)?;
        if !self.feedback.stocks.contains_key(code) {
            return Err(ExperienceError::NoActiveEntry {
                code: code.0.clone(),
            });
        }
        let stock = self.stocks.get(code);
        // 镜像 legacy 不利判定：成交后本人观察价 ≤ 买入价 95%。
        let will_confirm = stock.is_some_and(|s| {
            s.last_buy_price
                .is_some_and(|buy| i128::from(price.cents()) * 100 <= i128::from(buy.cents()) * 95)
                && !s.adverse_move_recorded
        });
        let confirmed_buy_order = stock.and_then(|s| s.last_buy_order_id);

        self.observe_position(code, price, moment.market_minute)?;

        self.feedback.advance_clocks(moment);
        self.feedback
            .stocks
            .get_mut(code)
            .expect("epoch existence is guarded above")
            .last_own_observation = Some(OwnObservation { price, moment });
        if will_confirm {
            self.feedback.failure_events.push(FailureEventRecord {
                code: code.clone(),
                order_id: confirmed_buy_order,
                moment,
            });
        }
        Ok(())
    }
}
