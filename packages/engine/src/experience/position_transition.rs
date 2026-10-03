//! 同股 legacy 与可选 active epoch 的短暂转换借用；不拥有账户或历史账本。

use std::collections::BTreeMap;

use super::{
    ExperienceError, ExperienceMoment, HoldingEpoch, OwnObservation, RetailStockExperience,
    POST_EXIT_COOLDOWN_MINUTES,
};
use crate::{Money, Side, StockCode};

pub(super) struct PositionExperienceTransition<'a> {
    legacy: &'a mut RetailStockExperience,
    epoch: Option<&'a mut HoldingEpoch>,
}

impl<'a> PositionExperienceTransition<'a> {
    pub(super) fn new(
        legacy: &'a mut RetailStockExperience,
        epoch: Option<&'a mut HoldingEpoch>,
    ) -> Self {
        Self { legacy, epoch }
    }

    // caller 完成原守卫后才能建立借用；缺 legacy 行仍沿用原 entry.or_default 接受集。
    pub(super) fn from_maps(
        stocks: &'a mut BTreeMap<StockCode, RetailStockExperience>,
        epochs: &'a mut BTreeMap<StockCode, HoldingEpoch>,
        code: &StockCode,
    ) -> Self {
        Self::new(
            stocks.entry(code.clone()).or_default(),
            epochs.get_mut(code),
        )
    }

    pub(super) fn reset_initial_holding(
        &mut self,
        reference: Option<Money>,
        price: Money,
        moment: ExperienceMoment,
    ) -> HoldingEpoch {
        *self.legacy = RetailStockExperience {
            entry_reference_price: reference,
            peak_price_since_entry: Some(price),
            last_observed_market_minute: moment.market_minute,
            ..RetailStockExperience::default()
        };
        HoldingEpoch {
            entry_moment: moment,
            last_own_observation: Some(OwnObservation { price, moment }),
            institutional_fees_paid: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn record_retail_fill(
        &mut self,
        side: Side,
        price: Money,
        before_qty: u32,
        after_qty: u32,
        cost_before: Option<Money>,
        market_minute: u64,
        order_id: Option<u64>,
        failed_buys: &mut u16,
    ) -> Result<(), ExperienceError> {
        let stock = &mut *self.legacy;
        stock.last_trade_market_minute = market_minute;
        stock.last_observed_market_minute = market_minute;
        match side {
            Side::Buy => {
                stock.last_buy_price = Some(price);
                if order_id.is_none_or(|id| stock.last_buy_order_id != Some(id)) {
                    stock.adverse_move_recorded = false;
                }
                stock.last_buy_order_id = order_id;
                stock.cooldown_until_market_minute = None;
                if before_qty == 0 {
                    stock.entry_reference_price = Some(price);
                    stock.peak_price_since_entry = Some(price);
                } else {
                    stock.peak_price_since_entry = Some(
                        stock
                            .peak_price_since_entry
                            .map_or(price, |peak| peak.max(price)),
                    );
                }
            }
            Side::Sell => {
                let has_buy_experience = stock.last_buy_price.is_some();
                if has_buy_experience
                    && order_id.is_none_or(|id| stock.last_sell_order_id != Some(id))
                {
                    if let Some(cost) = cost_before {
                        if price > cost {
                            *failed_buys = (*failed_buys).saturating_sub(1);
                        } else if price < cost && !stock.adverse_move_recorded {
                            *failed_buys = (*failed_buys)
                                .checked_add(1)
                                .ok_or(ExperienceError::CounterOverflow)?;
                            stock.adverse_move_recorded = true;
                        }
                    }
                }
                stock.last_sell_order_id = order_id;
                if after_qty == 0 {
                    stock.entry_reference_price = None;
                    stock.peak_price_since_entry = None;
                    stock.last_buy_price = None;
                    stock.last_buy_order_id = None;
                    stock.adverse_move_recorded = false;
                    stock.cooldown_until_market_minute = Some(
                        market_minute
                            .checked_add(POST_EXIT_COOLDOWN_MINUTES)
                            .ok_or(ExperienceError::MarketMinuteOverflow {
                                minute: market_minute,
                                increment: POST_EXIT_COOLDOWN_MINUTES,
                            })?,
                    );
                }
            }
        }
        Ok(())
    }

    pub(super) fn observe_retail_position(
        &mut self,
        price: Money,
        market_minute: u64,
        failed_buys: &mut u16,
    ) -> Result<(), ExperienceError> {
        let stock = &mut *self.legacy;
        stock.last_observed_market_minute = market_minute;
        if stock.entry_reference_price.is_some() {
            stock.peak_price_since_entry = Some(
                stock
                    .peak_price_since_entry
                    .map_or(price, |peak| peak.max(price)),
            );
        }
        let adverse = stock.last_buy_price.is_some_and(|buy_price| {
            i128::from(price.cents()) * 100 <= i128::from(buy_price.cents()) * 95
        });
        if adverse && !stock.adverse_move_recorded {
            *failed_buys = (*failed_buys)
                .checked_add(1)
                .ok_or(ExperienceError::CounterOverflow)?;
            stock.adverse_move_recorded = true;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn record_institutional_fill(
        &mut self,
        side: Side,
        price: Money,
        before_qty: u32,
        after_qty: u32,
        order_id: u64,
        market_minute: u64,
        starts_new_buy_order: bool,
    ) {
        let stock = &mut *self.legacy;
        stock.last_trade_market_minute = market_minute;
        stock.last_observed_market_minute = market_minute;
        match side {
            Side::Buy => {
                stock.last_buy_price = Some(price);
                stock.last_buy_order_id = Some(order_id);
                if starts_new_buy_order {
                    stock.adverse_move_recorded = false;
                }
                if before_qty == 0 {
                    stock.entry_reference_price = Some(price);
                    stock.peak_price_since_entry = Some(price);
                } else {
                    stock.peak_price_since_entry = Some(
                        stock
                            .peak_price_since_entry
                            .map_or(price, |peak| peak.max(price)),
                    );
                }
            }
            Side::Sell => {
                stock.last_sell_order_id = Some(order_id);
                if after_qty == 0 {
                    stock.entry_reference_price = None;
                    stock.peak_price_since_entry = None;
                    stock.last_buy_price = None;
                    stock.last_buy_order_id = None;
                    stock.adverse_move_recorded = false;
                }
            }
        }
    }

    pub(super) fn observe_institutional_position(
        &mut self,
        price: Money,
        moment: ExperienceMoment,
        confirms_failure: bool,
    ) {
        let stock = &mut *self.legacy;
        stock.last_observed_market_minute = moment.market_minute;
        stock.peak_price_since_entry = Some(
            stock
                .peak_price_since_entry
                .map_or(price, |peak| peak.max(price)),
        );
        if confirms_failure {
            stock.adverse_move_recorded = true;
        }
    }

    pub(super) fn record_own_observation(&mut self, price: Money, moment: ExperienceMoment) {
        let moment = ExperienceMoment {
            market_minute: self.legacy.last_observed_market_minute,
            ..moment
        };
        self.epoch
            .as_deref_mut()
            .expect("epoch existence is guarded above")
            .last_own_observation = Some(OwnObservation { price, moment });
    }

    pub(super) fn record_institutional_observation(
        &mut self,
        price: Money,
        moment: ExperienceMoment,
        total_fees: Money,
    ) {
        let moment = ExperienceMoment {
            market_minute: self.legacy.last_observed_market_minute,
            ..moment
        };
        if let Some(epoch) = self.epoch.as_deref_mut() {
            epoch.institutional_fees_paid = Some(total_fees);
            epoch.last_own_observation = Some(OwnObservation { price, moment });
        }
    }

    pub(super) fn initialize_institutional_fees(&mut self) {
        self.epoch
            .as_deref_mut()
            .expect("institutional holding was initialized above")
            .institutional_fees_paid = Some(Money::ZERO);
    }

    pub(super) fn cooldown_until(&self) -> u64 {
        self.legacy
            .cooldown_until_market_minute
            .expect("legacy sell-to-zero always sets the post-exit cooldown")
    }
}
