//! 单只股票的市场状态层（ADR-0005 §3）：包装 OrderBook + 价格/涨跌停边界。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-market-design.md。
//! 只做单股状态：涨跌停拒单、last_price 记录、日终重置。
//! 不碰 account 结算、不碰全 tick 编排（session/simulator）。

use crate::account::StockCode;
use crate::money::{Money, MoneyError};
use crate::orderbook::{AccountId, MatchResult, Order, OrderBook, OrderError, OrderId, Side};
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
    /// 昨日收盘价（涨跌停基准；日终重置为 last_price）。
    last_close: Money,
    /// 涨跌停比例（基点，10%=1000），避免边界价格使用浮点运算。
    limit_bps: u32,
    /// 申报价格最小变动单位。
    tick: Money,
}

impl Market {
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
        .unwrap_or(self.last_price)
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
        let denominator = i128::from(10_000_u32) * i128::from(self.tick.cents());
        let numerator = i128::from(reference.cents()) * i128::from(ratio_bps);
        let rounded_ticks = (numerator + denominator / 2) / denominator;
        let rounded_cents = rounded_ticks
            .checked_mul(i128::from(self.tick.cents()))
            .and_then(|value| i64::try_from(value).ok())
            .ok_or_else(|| MoneyError::Overflow {
                op: "positive_half_up_price_bound",
                operand: format!("{} * {ratio_bps} / 10000", reference.cents()),
            })?;
        let mut bound = Money::from_cents(rounded_cents.max(self.tick.cents()));
        if upward && bound <= reference {
            bound = reference.add(self.tick)?;
        } else if !upward && bound >= reference {
            bound = reference.sub(self.tick)?.max(self.tick);
        }
        Ok(bound)
    }

    /// 最新成交价（只读）。
    pub fn last_price(&self) -> Money {
        self.last_price
    }

    /// 昨日收盘价（只读）。
    pub fn last_close(&self) -> Money {
        self.last_close
    }

    /// 股票代码（只读引用）。
    pub fn code(&self) -> &StockCode {
        &self.code
    }

    // ── 存档恢复用 setter（仅 restore 调用）──

    /// 设置最新成交价（存档恢复用）。
    pub fn set_last_price(&mut self, price: Money) {
        self.last_price = price;
    }

    /// 设置昨收价（存档恢复用）。
    pub fn set_last_close(&mut self, price: Money) {
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
        let result = self.book.place(order)?;
        // 末笔成交价成为新的 last_price（成交驱动）。
        if let Some(last) = result.trades.last() {
            self.last_price = last.price;
        }
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

    pub fn resting_order_count(&self) -> usize {
        self.book.resting_order_count()
    }

    pub fn resting_order_count_for(&self, owner: AccountId) -> usize {
        self.book.resting_order_count_for(owner)
    }

    /// 撤销一笔连续竞价委托；返回原委托供上层校验所有权和释放冻结量。
    pub fn cancel(&mut self, id: OrderId) -> Result<Order, MarketError> {
        Ok(self.book.cancel(id)?)
    }

    /// 日终：`last_close = last_price`，为次日重置涨跌停基准。
    ///
    /// 涨跌停价以 `last_close` 为基准（见 [`Self::up_stop`] / [`Self::down_stop`]），
    /// 日终把昨收对齐到当日最新成交价，使次日 ±`limit_pct` 区间跟随当日收盘。
    pub fn end_of_day(&mut self) {
        self.last_close = self.last_price;
        self.book.clear();
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
