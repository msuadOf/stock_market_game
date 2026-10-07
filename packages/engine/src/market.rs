//! 单只股票的市场状态层（ADR-0005 §3）：包装 OrderBook + 价格/涨跌停边界。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-market-design.md。
//! 只做单股状态：涨跌停拒单、last_price 记录、日终重置。
//! 不碰 account 结算、不碰全 tick 编排（session/simulator）。

use crate::account::StockCode;
use crate::company::ex_reference_price::ExReferencePrice;
use crate::money::{Money, MoneyError};
use crate::orderbook::{
    AccountId, MatchResult, Order, OrderBook, OrderBookDelta, OrderError, OrderId, Side,
};
use crate::strategy::LimitPrice;
use std::collections::BTreeMap;
use thiserror::Error;

/// market 操作失败。绝不静默吞掉（铁律二）。
#[derive(Debug, Error)]
pub enum MarketError {
    /// 下单价超出涨跌停范围 [down_stop, up_stop]。
    #[error("limit exceeded for {code:?}: price {price:?} not in [{down:?}, {up:?}]")]
    LimitExceeded {
        code: StockCode,
        price: Money,
        down: Money,
        up: Money,
    },
    /// 透传 orderbook 错误（非法价格/数量/tick）。
    #[error(transparent)]
    OrderBook(#[from] OrderError),
    /// 透传 money 错误（涨跌停价/apply_rate 溢出）。
    #[error(transparent)]
    Money(#[from] MoneyError),
    /// 市场构造参数非法（limit_pct 不在 (0,1)、initial_price 非正等）。
    #[error("invalid market params: {reason}")]
    InvalidParams { reason: String },
    /// 权威价格状态必须为正，否则无法计算交易边界。
    #[error("invalid price state: {field}={value:?} (must be positive)")]
    InvalidPriceState { field: &'static str, value: Money },
    /// 准备日与除息日不符，或参考价无法用于当前市场。
    #[error("invalid ex-date reference for {code:?}: {reason}")]
    InvalidExDateReference { code: StockCode, reason: String },
    /// 同一除息日收到不同市场锚，或事件日期倒退。
    #[error("cash ex-reference conflicts for {code:?}: {reason}")]
    CashExReferenceConflict { code: StockCode, reason: String },
}

/// 涨跌停/价格笼子边界价的整数推导核心（单位：分）。
///
/// 语义与 [`Market`] 的 `price_bound` 完全一致，提取为共享函数供两处复用：
/// （1）`Market::price_bound` 自身；（2）公司行为偏好封顶的跌停链迭代
/// （`company::simple::preferences`，见 2026-10-07 修复轮 2：闭式线性下界在
/// 低价域被证伪，跌停敞口余量必须按本核心逐日迭代推导）。
/// 推导：`reference × ratio_bps / 10000` 按正数四舍五入（half-up）取至最小
/// 价位；**涨/跌幅度不足一个价位（舍入结果未离开昨收）时强制至少移动一个
/// 价位**；最低不低于一个价位。
/// 返回 `None` 表示输入非法（reference/tick 非正）或推导溢出，由调用方
/// 显式映射为既有错误（铁律二：不静默吞错）。
pub(crate) fn price_bound_cents(
    reference_cents: i64,
    ratio_bps: u32,
    tick_cents: i64,
    upward: bool,
) -> Option<i64> {
    if reference_cents <= 0 || tick_cents <= 0 {
        return None;
    }
    let denominator = i128::from(10_000_u32) * i128::from(tick_cents);
    let numerator = i128::from(reference_cents) * i128::from(ratio_bps);
    let rounded_ticks = (numerator + denominator / 2) / denominator;
    let rounded_cents = i64::try_from(rounded_ticks.checked_mul(i128::from(tick_cents))?).ok()?;
    let mut bound = rounded_cents.max(tick_cents);
    if upward && bound <= reference_cents {
        bound = reference_cents.checked_add(tick_cents)?;
    } else if !upward && bound >= reference_cents {
        bound = (reference_cents - tick_cents).max(tick_cents);
    }
    Some(bound)
}

/// 单只股票的市场状态。
///
/// 包装 [`OrderBook`]，叠加涨跌停边界与最新价/昨收价记录。价格全程
/// [`Money`]（i64 分），绝不存 f64（money 模块铁律）；`limit_pct` 仅在
/// 构造期转换为整数基点，非权威价格状态。
///
/// 非显式派生 `Debug`：内部 `OrderBook` 已实现 `Debug`，编译器可自动派生；
/// 此处保持裸结构体以匹配既有模块风格，需要时上层按只读访问器取值。
#[derive(Clone, Debug)]
pub struct Market {
    /// 股票代码。
    code: StockCode,
    /// 委托其撮合的订单簿。
    book: OrderBook,
    /// 最新成交价（成交驱动更新；首日 = initial_price）。
    last_price: Money,
    /// 昨日收盘/交易所除息参考价（涨跌停基准）。
    last_close: Money,
    /// 除息日无成交前，限价笼子的行情回退应使用交易所除息前收。
    cash_ex_reference_pending_trade: bool,
    /// 最近已应用的市场除息事实；与 pending 独立，供幂等重放和日期顺序校验。
    last_cash_ex_reference: Option<ExReferencePrice>,
    /// 本交易日是否已有受理委托或成交；用于拒绝开市后迟到的锚安装。
    day_market_activity: bool,
    /// 涨跌停比例（基点，10%=1000），避免边界价格使用浮点运算。
    limit_bps: u32,
    /// 申报价格最小变动单位。
    tick: Money,
}

/// The changed part of one stock book after a tick-private worker round.
/// Order identities locate rows; they do not assign trading priority.
#[derive(Clone, Debug)]
pub(crate) struct MarketDelta {
    code: StockCode,
    before_last_price: Money,
    before_last_close: Money,
    after_last_price: Money,
    last_close: Money,
    before_cash_ex_reference_pending_trade: bool,
    cash_ex_reference_pending_trade: bool,
    before_last_cash_ex_reference: Option<ExReferencePrice>,
    last_cash_ex_reference: Option<ExReferencePrice>,
    before_day_market_activity: bool,
    day_market_activity: bool,
    book: OrderBookDelta,
}

impl Market {
    pub(crate) fn hash_projection(&self) -> impl serde::Serialize + '_ {
        (
            &self.code,
            self.last_price,
            self.last_close,
            self.cash_ex_reference_pending_trade,
            self.last_cash_ex_reference
                .map(|reference| (reference.ex_date.to_string(), reference.reference_price)),
            self.day_market_activity,
            self.limit_bps,
            self.tick,
            self.book.hash_projection(),
        )
    }
    /// 构造。校验 `limit_pct`∈(0,1)、`initial_price`>0。
    /// `last_close = last_price = initial_price`（首日无昨收，以开盘价为准）。
    ///
    /// 防御式（铁律二）：任一参数非法 → 显式 [`MarketError::InvalidParams`]，
    /// 绝不静默 clamp 到默认值。tick 非法透传 [`OrderBook::new`] 的
    /// [`MarketError::OrderBook`]。
    pub fn new(
        code: StockCode,
        initial_price: Money,
        limit_pct: f64,
        tick: Money,
    ) -> Result<Market, MarketError> {
        if !(limit_pct > 0.0 && limit_pct < 1.0) {
            return Err(MarketError::InvalidParams {
                reason: format!("limit_pct {limit_pct} not in (0,1)"),
            });
        }
        let scaled_limit = limit_pct * 10_000.0;
        let rounded_limit = scaled_limit.round();
        if (scaled_limit - rounded_limit).abs() > 1e-9 {
            return Err(MarketError::InvalidParams {
                reason: format!("limit_pct {limit_pct} must be an exact basis-point rate"),
            });
        }
        if initial_price.cents() <= 0 {
            return Err(MarketError::InvalidParams {
                reason: format!("initial_price {:?} must be > 0", initial_price),
            });
        }
        let book = OrderBook::new(tick)?;
        Ok(Market {
            code,
            book,
            last_price: initial_price,
            last_close: initial_price,
            cash_ex_reference_pending_trade: false,
            last_cash_ex_reference: None,
            day_market_activity: false,
            limit_bps: rounded_limit as u32,
            tick,
        })
    }

    /// 涨停价：昨收按涨幅计算后以正数四舍五入取至最小价位；不足一价位时至少上移一档。
    pub fn up_stop(&self) -> Result<Money, MarketError> {
        self.price_bound(self.last_close, 10_000 + self.limit_bps, true)
    }

    /// 跌停价：昨收按跌幅计算后以正数四舍五入取至最小价位；最低不小于一个价位。
    pub fn down_stop(&self) -> Result<Money, MarketError> {
        self.price_bound(self.last_close, 10_000 - self.limit_bps, false)
    }

    /// 连续竞价限价申报的基准价。
    ///
    /// 依据上交所《交易规则（2026 年修订）》3.3.14：买入依次取卖一、买一、最新成交、
    /// 前收；卖出依次取买一、卖一、最新成交、前收。本模型的 `last_price` 在当日无成交时
    /// 等于前收，因此覆盖最后两级回退。规则原文：
    /// <https://www.sse.com.cn/lawandrules/sselawsrules2025/trade/universal/c/c_20260331_10808286.shtml>
    pub fn continuous_limit_reference(&self, side: Side) -> Money {
        match side {
            Side::Buy => self.best_ask().or_else(|| self.best_bid()),
            Side::Sell => self.best_bid().or_else(|| self.best_ask()),
        }
        .unwrap_or(if self.cash_ex_reference_pending_trade {
            self.last_close
        } else {
            self.last_price
        })
    }

    /// 连续竞价价格笼子的买入上限或卖出下限。
    ///
    /// 买入上限取“参考价的 102%”与“参考价加十个最小价位”中的较高者；
    /// 卖出下限取“参考价的 98%”与“参考价减十个最小价位”中的较低者。
    pub fn continuous_limit_bound(&self, side: Side) -> Result<Money, MarketError> {
        let reference = self.continuous_limit_reference(side);
        let ten_ticks = self.tick.mul_shares(10)?;
        match side {
            Side::Buy => Ok(self
                .price_bound(reference, 10_200, true)?
                .max(reference.add(ten_ticks)?)),
            Side::Sell => Ok(self
                .price_bound(reference, 9_800, false)?
                .min(reference.sub(ten_ticks)?.max(self.tick))),
        }
    }

    /// 当前限价委托可使用的买入最高价或卖出最低价。
    ///
    /// 日涨跌停始终生效；调用方仅在连续竞价且配置开启时传入 `apply_price_cage`。
    pub fn limit_order_price_bound(
        &self,
        side: Side,
        apply_price_cage: bool,
    ) -> Result<Money, MarketError> {
        let daily_bound = match side {
            Side::Buy => self.up_stop()?,
            Side::Sell => self.down_stop()?,
        };
        if !apply_price_cage {
            return Ok(daily_bound);
        }
        let cage_bound = self.continuous_limit_bound(side)?;
        Ok(match side {
            Side::Buy => daily_bound.min(cage_bound),
            Side::Sell => daily_bound.max(cage_bound),
        })
    }

    /// 受理时把符号限价解析成当前权威价格；Fixed 保持原值，由后续委托校验处理。
    pub fn resolve_limit_price(
        &self,
        side: Side,
        price: LimitPrice,
        apply_price_cage: bool,
    ) -> Result<Money, MarketError> {
        match price {
            LimitPrice::Fixed(price) => Ok(price),
            LimitPrice::Highest => match side {
                Side::Buy => self.limit_order_price_bound(Side::Buy, apply_price_cage),
                Side::Sell => self.up_stop(),
            },
            LimitPrice::Lowest => match side {
                Side::Buy => self.down_stop(),
                Side::Sell => self.limit_order_price_bound(Side::Sell, apply_price_cage),
            },
        }
    }

    fn price_bound(
        &self,
        reference: Money,
        ratio_bps: u32,
        upward: bool,
    ) -> Result<Money, MarketError> {
        if reference.cents() <= 0 {
            return Err(MarketError::InvalidPriceState {
                field: "reference",
                value: reference,
            });
        }
        let bound_cents =
            price_bound_cents(reference.cents(), ratio_bps, self.tick.cents(), upward).ok_or_else(
                || MoneyError::Overflow {
                    op: "positive_half_up_price_bound",
                    operand: format!("{} * {ratio_bps} / 10000", reference.cents()),
                },
            )?;
        Ok(Money::from_cents(bound_cents))
    }

    /// 最新成交价（只读）。
    pub fn last_price(&self) -> Money {
        self.last_price
    }

    /// 昨日收盘价（只读）。
    pub fn last_close(&self) -> Money {
        self.last_close
    }

    /// 涨跌幅限制（整数基点，10% = 1000）。构造期由 `limit_pct` 转换的权威
    /// 整数值；公司行为偏好提案的跌停余量封顶按该整数推导（只读）。
    pub const fn limit_bps(&self) -> u32 {
        self.limit_bps
    }

    /// 申报价格最小变动单位（只读）。公司行为偏好的跌停链迭代必须与
    /// `price_bound` 使用同一价位取整，防止两侧整数口径漂移。
    pub const fn tick(&self) -> Money {
        self.tick
    }

    /// 在调用方确认该证券除息日首次开市前安装已核验的除息参考价。
    /// 仅更新行情前收/涨跌停锚，不覆盖真实最新成交，不生成行情或成交。
    pub fn prepare_ex_date_reference(
        &mut self,
        prepared_date: crate::calendar::CivilDate,
        ex_reference: ExReferencePrice,
    ) -> Result<(), MarketError> {
        if prepared_date != ex_reference.ex_date {
            return Err(MarketError::InvalidExDateReference {
                code: self.code.clone(),
                reason: format!(
                    "prepared date {prepared_date} differs from ex-date {}",
                    ex_reference.ex_date
                ),
            });
        }
        if let Some(previous) = self.last_cash_ex_reference {
            if previous == ex_reference {
                return Ok(());
            }
            if previous.ex_date >= ex_reference.ex_date {
                return Err(MarketError::CashExReferenceConflict {
                    code: self.code.clone(),
                    reason: format!(
                        "new ex-date {} does not follow installed ex-date {}",
                        ex_reference.ex_date, previous.ex_date
                    ),
                });
            }
        }
        if ex_reference.reference_price <= Money::ZERO {
            return Err(MarketError::InvalidPriceState {
                field: "ex_reference_price",
                value: ex_reference.reference_price,
            });
        }
        if ex_reference.reference_price.cents() % self.tick.cents() != 0 {
            return Err(MarketError::InvalidExDateReference {
                code: self.code.clone(),
                reason: format!(
                    "reference price {:?} is not aligned to tick {:?}",
                    ex_reference.reference_price, self.tick
                ),
            });
        }
        if self.day_market_activity {
            return Err(MarketError::InvalidExDateReference {
                code: self.code.clone(),
                reason: "market activity has already started for this trading day".to_owned(),
            });
        }
        self.last_close = ex_reference.reference_price;
        self.cash_ex_reference_pending_trade = true;
        self.last_cash_ex_reference = Some(ex_reference);
        Ok(())
    }

    /// 除息参考价是否仍是当日无成交前收锚，供严格存档恢复。
    pub const fn cash_ex_reference_pending_trade(&self) -> bool {
        self.cash_ex_reference_pending_trade
    }

    /// 最近已应用的除息事实，包含事件日期和税前参考价。
    pub const fn last_cash_ex_reference(&self) -> Option<ExReferencePrice> {
        self.last_cash_ex_reference
    }

    /// 本股当交易日是否已有受理委托或成交，供严格存档恢复。
    pub const fn day_market_activity(&self) -> bool {
        self.day_market_activity
    }

    pub(crate) fn mark_day_market_activity(&mut self) {
        self.day_market_activity = true;
    }

    pub(crate) fn validate_restored_facts(
        code: &StockCode,
        snapshot_date: crate::calendar::CivilDate,
        last_price: Money,
        last_close: Money,
        cash_ex_reference_pending_trade: bool,
        last_cash_ex_reference: Option<ExReferencePrice>,
        tick: Money,
    ) -> Result<(), MarketError> {
        if tick <= Money::ZERO {
            return Err(MarketError::InvalidExDateReference {
                code: code.clone(),
                reason: format!("stored market tick must be positive, got {tick:?}"),
            });
        }
        for (field, value) in [("last_price", last_price), ("last_close", last_close)] {
            if value <= Money::ZERO {
                return Err(MarketError::InvalidPriceState { field, value });
            }
        }
        if let Some(reference) = last_cash_ex_reference {
            if reference.ex_date > snapshot_date {
                return Err(MarketError::InvalidExDateReference {
                    code: code.clone(),
                    reason: format!(
                        "stored ex-date {} is after snapshot date {snapshot_date}",
                        reference.ex_date
                    ),
                });
            }
            if reference.reference_price <= Money::ZERO
                || reference.reference_price.cents() % tick.cents() != 0
            {
                return Err(MarketError::InvalidExDateReference {
                    code: code.clone(),
                    reason: format!(
                        "stored ex-reference is not positive and tick aligned: {reference:?}"
                    ),
                });
            }
        }
        if cash_ex_reference_pending_trade {
            let reference =
                last_cash_ex_reference.ok_or_else(|| MarketError::InvalidExDateReference {
                    code: code.clone(),
                    reason: "pending ex-reference has no applied reference fact".to_owned(),
                })?;
            if last_close != reference.reference_price {
                return Err(MarketError::InvalidExDateReference {
                    code: code.clone(),
                    reason: "pending ex-reference does not match the market previous close"
                        .to_owned(),
                });
            }
        }
        Ok(())
    }

    /// 股票代码（只读引用）。
    pub fn code(&self) -> &StockCode {
        &self.code
    }

    // ── 经过上层校验的恢复事实与竞价结果写入口 ──

    /// 完整 save slot 校验后恢复市场价格与除息运行事实。
    pub(crate) fn restore_prices(
        &mut self,
        last_price: Money,
        last_close: Money,
        cash_ex_reference_pending_trade: bool,
        day_market_activity: bool,
        last_cash_ex_reference: Option<ExReferencePrice>,
    ) {
        self.last_price = last_price;
        self.last_close = last_close;
        self.cash_ex_reference_pending_trade = cash_ex_reference_pending_trade;
        self.day_market_activity = day_market_activity;
        self.last_cash_ex_reference = last_cash_ex_reference;
    }

    /// 竞价 caller 在完成撮合后应用其 clearing price，不重新改变输入接受集。
    pub(crate) fn apply_auction_price(&mut self, price: Money) {
        self.last_price = price;
        self.cash_ex_reference_pending_trade = false;
        self.day_market_activity = true;
    }

    #[cfg(test)]
    pub(crate) fn fixture_set_last_price(&mut self, price: Money) {
        self.last_price = price;
    }

    #[cfg(test)]
    pub(crate) fn fixture_set_last_close(&mut self, price: Money) {
        self.last_close = price;
    }

    /// 下单：涨跌停校验（超限拒单，book 不变）→ 委托 book 撮合 → 末笔成交更新 last_price。
    ///
    /// 防御式（铁律二）：价格超 `[down_stop, up_stop]` → 显式 [`MarketError::LimitExceeded`]，
    /// **不静默 clamp 价格**，且 book 不被改动；`book.place` 的非法价格/数量错误经 `#[from]` 透传。
    ///
    /// 闭区间 `[down_stop, up_stop]`：边界价合法（涨停价买、跌停价卖接受）。
    /// `Money` 已 `derive(Ord)`，可直接比较。
    /// 末笔成交价成为新的 last_price（成交驱动；无成交则 last_price 不变）。
    pub fn place(&mut self, order: Order) -> Result<MatchResult, MarketError> {
        self.place_inner(order, false)
    }

    pub(crate) fn place_recording(&mut self, order: Order) -> Result<MatchResult, MarketError> {
        self.place_inner(order, true)
    }

    fn place_inner(
        &mut self,
        order: Order,
        record_makers: bool,
    ) -> Result<MatchResult, MarketError> {
        let up = self.up_stop()?;
        let down = self.down_stop()?;
        if order.price < down || order.price > up {
            return Err(MarketError::LimitExceeded {
                code: self.code.clone(),
                price: order.price,
                down,
                up,
            });
        }
        let result = if record_makers {
            self.book.place_recording(order)?
        } else {
            self.book.place(order)?
        };
        // 末笔成交价成为新的 last_price（成交驱动）。
        if let Some(last) = result.trades.last() {
            self.last_price = last.price;
            self.cash_ex_reference_pending_trade = false;
        }
        self.day_market_activity = true;
        Ok(result)
    }

    /// 买盘最优价（透传 book）。空簿返回 None。
    pub fn best_bid(&self) -> Option<Money> {
        self.book.best_bid()
    }

    /// 卖盘最优价（透传 book）。空簿返回 None。
    pub fn best_ask(&self) -> Option<Money> {
        self.book.best_ask()
    }

    /// 订单簿只读引用（上层按需取盘口深度等）。
    pub fn book(&self) -> &OrderBook {
        &self.book
    }

    /// 返回账户在本股票上的全部未成交委托，用于上层冻结资产核算。
    pub fn resting_orders_for(&self, owner: AccountId) -> Vec<Order> {
        self.book.resting_orders_for(owner)
    }

    /// 返回该市场全部未成交委托，供无损存档与恢复。
    pub fn resting_orders(&self) -> Vec<Order> {
        self.book.resting_orders()
    }

    pub(crate) fn resting_order_refs(&self) -> impl Iterator<Item = &Order> {
        self.book.resting_order_refs()
    }

    pub(crate) fn resting_order_by_id(&self, id: OrderId) -> Option<&Order> {
        self.book.resting_order_by_id(id)
    }

    pub(crate) fn book_next_sequence(&self) -> u64 {
        self.book.next_sequence()
    }

    pub(crate) fn restore_resting_orders(
        &mut self,
        orders: &[Order],
        next_seq: u64,
    ) -> Result<(), MarketError> {
        self.book.restore_resting_orders(orders, next_seq)?;
        Ok(())
    }

    pub(crate) fn changed_orders_since(
        &self,
        before_last_price: Money,
        before_last_close: Money,
        before_cash_ex_reference_pending_trade: bool,
        before_last_cash_ex_reference: Option<ExReferencePrice>,
        before_day_market_activity: bool,
        before_next_seq: u64,
        originals: BTreeMap<OrderId, Option<Order>>,
    ) -> Result<MarketDelta, MarketError> {
        if self.last_close != before_last_close {
            return Err(MarketError::OrderBook(OrderError::ProjectionMismatch {
                reason: "continuous stock round changed yesterday's close".to_owned(),
            }));
        }
        Ok(MarketDelta {
            code: self.code.clone(),
            before_last_price,
            before_last_close,
            after_last_price: self.last_price,
            last_close: self.last_close,
            before_cash_ex_reference_pending_trade,
            cash_ex_reference_pending_trade: self.cash_ex_reference_pending_trade,
            before_last_cash_ex_reference,
            day_market_activity: self.day_market_activity,
            before_day_market_activity,
            last_cash_ex_reference: self.last_cash_ex_reference,
            book: self.book.changed_orders_since(before_next_seq, originals),
        })
    }

    pub(crate) fn apply_changed_orders(&mut self, delta: MarketDelta) -> Result<(), MarketError> {
        if self.code != delta.code
            || self.last_price != delta.before_last_price
            || self.last_close != delta.before_last_close
            || self.last_close != delta.last_close
            || self.cash_ex_reference_pending_trade != delta.before_cash_ex_reference_pending_trade
            || self.last_cash_ex_reference != delta.before_last_cash_ex_reference
            || self.day_market_activity != delta.before_day_market_activity
        {
            return Err(MarketError::OrderBook(OrderError::ProjectionMismatch {
                reason: "candidate market version differs from stock worker".to_owned(),
            }));
        }
        self.book.apply_changes(delta.book)?;
        self.last_price = delta.after_last_price;
        self.cash_ex_reference_pending_trade = delta.cash_ex_reference_pending_trade;
        self.day_market_activity = delta.day_market_activity;
        self.last_cash_ex_reference = delta.last_cash_ex_reference;
        Ok(())
    }

    pub fn filled_order_owner(&self, id: OrderId) -> Option<AccountId> {
        self.book.filled_order_owner(id)
    }

    pub fn filled_orders(&self) -> Vec<(OrderId, AccountId)> {
        self.book.filled_orders()
    }

    pub(crate) fn restore_filled_orders(
        &mut self,
        entries: impl IntoIterator<Item = (OrderId, AccountId)>,
    ) -> Result<(), MarketError> {
        self.book.restore_filled_orders(entries)?;
        Ok(())
    }

    pub(crate) fn record_filled_order(
        &mut self,
        id: OrderId,
        owner: AccountId,
    ) -> Result<(), MarketError> {
        self.book.restore_filled_orders([(id, owner)])?;
        Ok(())
    }

    pub(crate) fn resting_orders_for_owners(
        &self,
        owners: &std::collections::BTreeSet<AccountId>,
    ) -> Vec<Order> {
        self.book.resting_orders_for_owners(owners)
    }

    pub fn resting_order_count(&self) -> usize {
        self.book.resting_order_count()
    }

    /// 撤销一笔连续竞价委托；返回原委托供上层校验所有权和释放冻结量。
    pub fn cancel(&mut self, id: OrderId) -> Result<Order, MarketError> {
        Ok(self.book.cancel(id)?)
    }

    /// 日终：有真实成交时以 `last_price` 重置涨跌停基准；除息日尚无成交时保留除息参考价。
    ///
    /// 涨跌停价以 `last_close` 为基准（见 [`Self::up_stop`] / [`Self::down_stop`]），
    /// 日终通常把昨收对齐到当日最新成交价；除息后无成交期间保留交易所参考价。
    pub fn end_of_day(&mut self) {
        if !self.cash_ex_reference_pending_trade {
            self.last_close = self.last_price;
        }
        self.book.clear();
        self.day_market_activity = false;
    }

    /// 卖盘深度（透传 book）：按价低→高，每价位聚合总数量。空簿返回空 Vec。
    pub fn ask_depth(&self) -> Vec<(Money, u64)> {
        self.book.ask_depth()
    }

    pub fn ask_depth_limited(&self, max_levels: usize) -> Vec<(Money, u64)> {
        self.book.ask_depth_limited(max_levels)
    }

    /// 买盘深度（透传 book）：按价高→低，每价位聚合总数量。空簿返回空 Vec。
    pub fn bid_depth(&self) -> Vec<(Money, u64)> {
        self.book.bid_depth()
    }

    pub fn bid_depth_limited(&self, max_levels: usize) -> Vec<(Money, u64)> {
        self.book.bid_depth_limited(max_levels)
    }
}

#[cfg(test)]
mod price_limit_state_tests {
    use super::*;
    use crate::LimitPrice;

    fn mk_market() -> Market {
        // last_close=last_price=10.00，limit=0.10，tick=0.01
        Market::new(
            StockCode("600101".to_string()),
            Money::from_cents(1000),
            0.10,
            Money::from_cents(1),
        )
        .unwrap()
    }

    fn sell(id: u64, price_cents: i64, qty: u32) -> Order {
        Order {
            id: OrderId(id),
            side: Side::Sell,
            price: Money::from_cents(price_cents),
            qty,
            original_qty: qty,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: AccountId(2),
            seq: 0,
        }
    }

    #[test]
    fn symbolic_limit_prices_resolve_at_the_current_authoritative_boundary() {
        let mut market = mk_market();
        for (side, price, caged, uncaged) in [
            (Side::Buy, LimitPrice::Highest, 1020, 1100),
            (Side::Sell, LimitPrice::Lowest, 980, 900),
            (Side::Sell, LimitPrice::Highest, 1100, 1100),
            (Side::Buy, LimitPrice::Lowest, 900, 900),
        ] {
            assert_eq!(
                market.resolve_limit_price(side, price, true).unwrap(),
                Money::from_cents(caged)
            );
            assert_eq!(
                market.resolve_limit_price(side, price, false).unwrap(),
                Money::from_cents(uncaged)
            );
        }
        assert_eq!(
            market
                .resolve_limit_price(Side::Buy, LimitPrice::Fixed(Money::from_cents(1_007)), true)
                .unwrap(),
            Money::from_cents(1_007)
        );
        market.place(sell(1, 1_050, 100)).unwrap();
        assert_eq!(
            market
                .resolve_limit_price(Side::Buy, LimitPrice::Highest, true)
                .unwrap(),
            Money::from_cents(1_071)
        );
        market.fixture_set_last_close(Money::from_cents(i64::MAX));
        assert!(market
            .resolve_limit_price(Side::Buy, LimitPrice::Highest, true)
            .is_err());
        assert_eq!(
            market
                .resolve_limit_price(Side::Buy, LimitPrice::Fixed(Money::from_cents(1_007)), true)
                .unwrap(),
            Money::from_cents(1_007)
        );
    }

    #[test]
    fn price_limit_overflow_is_an_explicit_error_not_a_panic() {
        let mut market = mk_market();
        market.fixture_set_last_close(Money::from_cents(i64::MAX));
        assert!(market.up_stop().is_err());
    }

    #[test]
    fn legal_limit_order_prices_keep_the_wider_ten_tick_cage_and_propagate_errors() {
        let mut market = Market::new(
            StockCode("LOW_PRICE".to_string()),
            Money::from_cents(285),
            0.10,
            Money::from_cents(1),
        )
        .unwrap();
        assert_eq!(
            market.limit_order_price_bound(Side::Buy, true).unwrap(),
            Money::from_cents(295)
        );
        assert_eq!(
            market.limit_order_price_bound(Side::Sell, true).unwrap(),
            Money::from_cents(275)
        );
        market.fixture_set_last_close(Money::from_cents(i64::MAX));
        assert!(market.limit_order_price_bound(Side::Buy, true).is_err());
        assert!(market.limit_order_price_bound(Side::Buy, false).is_err());
    }

    #[test]
    fn ex_date_reference_changes_quotes_and_limits_without_rewriting_last_trade() {
        let mut market = mk_market();
        let ex_date = crate::calendar::CivilDate::from_ymd(2026, 10, 7).unwrap();
        let reference = crate::company::ex_reference_price::ExReferencePrice {
            ex_date,
            reference_price: Money::from_cents(900),
        };

        market
            .prepare_ex_date_reference(ex_date, reference)
            .unwrap();

        assert_eq!(market.last_price(), Money::from_cents(1000));
        assert_eq!(market.last_close(), Money::from_cents(900));
        assert_eq!(market.up_stop().unwrap(), Money::from_cents(990));
        assert_eq!(market.down_stop().unwrap(), Money::from_cents(810));
        assert_eq!(
            market.continuous_limit_reference(Side::Buy),
            Money::from_cents(900)
        );
        assert_eq!(
            market.continuous_limit_bound(Side::Buy).unwrap(),
            Money::from_cents(918)
        );
        assert_eq!(
            market.limit_order_price_bound(Side::Buy, true).unwrap(),
            Money::from_cents(918)
        );
        assert!(matches!(
            market.place(sell(8, 1000, 100)),
            Err(MarketError::LimitExceeded { .. })
        ));
        market.end_of_day();
        assert_eq!(market.last_price(), Money::from_cents(1000));
        assert_eq!(market.last_close(), Money::from_cents(900));
    }

    #[test]
    fn invalid_or_late_ex_date_reference_leaves_market_unchanged() {
        let mut market = mk_market();
        let ex_date = crate::calendar::CivilDate::from_ymd(2026, 10, 7).unwrap();
        let invalid = crate::company::ex_reference_price::ExReferencePrice {
            ex_date,
            reference_price: Money::ZERO,
        };
        assert!(market.prepare_ex_date_reference(ex_date, invalid).is_err());
        assert_eq!(market.last_close(), Money::from_cents(1000));
        assert!(!market.cash_ex_reference_pending_trade());
        assert_eq!(market.last_cash_ex_reference(), None);

        let mismatched_date = crate::calendar::CivilDate::from_ymd(2026, 10, 8).unwrap();
        let valid = crate::company::ex_reference_price::ExReferencePrice {
            ex_date,
            reference_price: Money::from_cents(900),
        };
        assert!(market
            .prepare_ex_date_reference(mismatched_date, valid)
            .is_err());
        assert_eq!(market.last_close(), Money::from_cents(1000));
        assert_eq!(market.last_cash_ex_reference(), None);

        market.place(sell(1, 1050, 100)).unwrap();
        assert!(market.prepare_ex_date_reference(ex_date, valid).is_err());
        assert_eq!(market.last_close(), Money::from_cents(1000));
        assert_eq!(market.last_cash_ex_reference(), None);
    }

    #[test]
    fn pending_ex_reference_is_restored_explicitly_not_inferred_from_price_difference() {
        let mut market = mk_market();
        market.restore_prices(
            Money::from_cents(1000),
            Money::from_cents(900),
            false,
            false,
            None,
        );
        assert_eq!(
            market.continuous_limit_reference(Side::Buy),
            Money::from_cents(1000)
        );

        market.restore_prices(
            Money::from_cents(1000),
            Money::from_cents(900),
            true,
            false,
            None,
        );
        assert_eq!(
            market.continuous_limit_reference(Side::Buy),
            Money::from_cents(900)
        );
    }

    #[test]
    fn ex_reference_is_idempotent_conflict_checked_and_advances_by_explicit_date() {
        let mut market = mk_market();
        let first_date = crate::calendar::CivilDate::from_ymd(2026, 10, 7).unwrap();
        let first = crate::company::ex_reference_price::ExReferencePrice {
            ex_date: first_date,
            reference_price: Money::from_cents(900),
        };
        market.prepare_ex_date_reference(first_date, first).unwrap();
        market.end_of_day();
        market.prepare_ex_date_reference(first_date, first).unwrap();
        assert_eq!(market.last_close(), Money::from_cents(900));
        assert_eq!(market.last_cash_ex_reference(), Some(first));

        let conflict = crate::company::ex_reference_price::ExReferencePrice {
            reference_price: Money::from_cents(890),
            ..first
        };
        assert!(matches!(
            market.prepare_ex_date_reference(first_date, conflict),
            Err(MarketError::CashExReferenceConflict { .. })
        ));
        assert_eq!(market.last_close(), Money::from_cents(900));

        let second_date = crate::calendar::CivilDate::from_ymd(2026, 10, 8).unwrap();
        let second = crate::company::ex_reference_price::ExReferencePrice {
            ex_date: second_date,
            reference_price: Money::from_cents(800),
        };
        market
            .prepare_ex_date_reference(second_date, second)
            .unwrap();
        assert_eq!(market.last_close(), Money::from_cents(800));
        assert_eq!(market.last_price(), Money::from_cents(1000));
    }

    #[test]
    fn restored_pending_reference_requires_a_matching_applied_reference_fact() {
        let code = StockCode("600101".to_owned());
        let ex_date = crate::calendar::CivilDate::from_ymd(2026, 10, 7).unwrap();
        let reference = crate::company::ex_reference_price::ExReferencePrice {
            ex_date,
            reference_price: Money::from_cents(900),
        };
        assert!(Market::validate_restored_facts(
            &code,
            ex_date,
            Money::from_cents(1000),
            Money::from_cents(900),
            true,
            None,
            Money::from_cents(1),
        )
        .is_err());
        assert!(Market::validate_restored_facts(
            &code,
            ex_date,
            Money::from_cents(1000),
            Money::from_cents(890),
            true,
            Some(reference),
            Money::from_cents(1),
        )
        .is_err());
        assert!(Market::validate_restored_facts(
            &code,
            ex_date,
            Money::from_cents(1000),
            Money::from_cents(900),
            true,
            Some(reference),
            Money::from_cents(1),
        )
        .is_ok());
        let earlier_date = crate::calendar::CivilDate::from_ymd(2026, 10, 6).unwrap();
        assert!(Market::validate_restored_facts(
            &code,
            earlier_date,
            Money::from_cents(1000),
            Money::from_cents(900),
            true,
            Some(reference),
            Money::from_cents(1),
        )
        .is_err());
    }
}
