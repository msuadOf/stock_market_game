//! 单只股票的市场状态层（ADR-0005 §3）：包装 OrderBook + 价格/涨跌停/隐藏公允价 V。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-market-design.md。
//! 只做单股状态：涨跌停拒单、last_price 记录、V 演化、日终重置。
//! 不碰 account 结算、不碰全 tick 编排（session/simulator）。

use crate::account::StockCode;
use crate::money::{Money, MoneyError};
use crate::orderbook::{Order, MatchResult, OrderBook, OrderError};
use crate::strategy::Rng;
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
    /// V 演化参数非法（volatility/mean_reversion 负或非有限；V 跨零）。
    #[error("invalid v params: {reason}")]
    InvalidVParams { reason: String },
}

/// V（隐藏公允价）演化参数。均值回复几何随机游走。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct VParams {
    /// 长期均值（V 向其回复）。
    pub long_run_mean: Money,
    /// 回复速度 α（≥0）。
    pub mean_reversion: f64,
    /// 随机扰动幅度 σ（≥0）。
    pub volatility: f64,
}

/// 单只股票的市场状态。
///
/// 包装 [`OrderBook`]，叠加涨跌停边界、最新价/昨收价记录、隐藏公允价 V。
/// 价格与 V 全程 [`Money`]（i64 分），绝不存 f64（money 模块铁律）；
/// `limit_pct` 是涨跌停百分比（构造期校验 ∈ (0,1)），非权威价格状态。
///
/// 非显式派生 `Debug`：内部 `OrderBook` 已实现 `Debug`，编译器可自动派生；
/// 此处保持裸结构体以匹配既有模块风格，需要时上层按只读访问器取值。
pub struct Market {
    /// 股票代码。
    code: StockCode,
    /// 委托其撮合的订单簿。
    book: OrderBook,
    /// 最新成交价（成交驱动更新；首日 = initial_price）。
    last_price: Money,
    /// 昨日收盘价（涨跌停基准；日终重置为 last_price）。
    last_close: Money,
    /// 隐藏公允价 V（演化更新；首日 = v_initial）。
    fundamental_value: Money,
    /// 涨跌停百分比（如 0.10 = ±10%），构造期校验 ∈ (0,1)。
    limit_pct: f64,
}

impl Market {
    /// 构造。校验 `limit_pct`∈(0,1)、`initial_price`>0、`v_initial`>0。
    /// `last_close = last_price = initial_price`（首日无昨收，以开盘价为准）。
    ///
    /// 防御式（铁律二）：任一参数非法 → 显式 [`MarketError::InvalidVParams`]，
    /// 绝不静默 clamp 到默认值或存非正 V。tick 非法透传 [`OrderBook::new`] 的
    /// [`MarketError::OrderBook`]。
    pub fn new(
        code: StockCode,
        initial_price: Money,
        limit_pct: f64,
        v_initial: Money,
        tick: Money,
    ) -> Result<Market, MarketError> {
        if !(limit_pct > 0.0 && limit_pct < 1.0) {
            return Err(MarketError::InvalidVParams {
                reason: format!("limit_pct {limit_pct} not in (0,1)"),
            });
        }
        if initial_price.cents() <= 0 {
            return Err(MarketError::InvalidVParams {
                reason: format!("initial_price {:?} must be > 0", initial_price),
            });
        }
        if v_initial.cents() <= 0 {
            return Err(MarketError::InvalidVParams {
                reason: format!("v_initial {:?} must be > 0", v_initial),
            });
        }
        let book = OrderBook::new(tick)?;
        Ok(Market {
            code,
            book,
            last_price: initial_price,
            last_close: initial_price,
            fundamental_value: v_initial,
            limit_pct,
        })
    }

    /// 涨停价 = last_close × (1 + limit_pct)。
    ///
    /// `apply_rate` 返回 `Result`（比率/金额溢出 → [`MoneyError`]）；但 `limit_pct`
    /// 已校验有限、`last_close` 为有限 Money，此式恒成功——故 `expect` 带说明标注不变量，
    /// 仅在 V/limit 演化逻辑 bug 时 panic（非业务路径吞错）。
    pub fn up_stop(&self) -> Money {
        self.last_close
            .apply_rate(1.0 + self.limit_pct)
            .expect("up_stop: limit_pct finite & last_close finite => 不可溢出")
    }

    /// 跌停价 = last_close × (1 - limit_pct)。
    ///
    /// 理由同 [`Self::up_stop`]：恒成功，`expect` 标注不变量。
    pub fn down_stop(&self) -> Money {
        self.last_close
            .apply_rate(1.0 - self.limit_pct)
            .expect("down_stop: limit_pct finite & last_close finite => 不可溢出")
    }

    /// 最新成交价（只读）。
    pub fn last_price(&self) -> Money {
        self.last_price
    }

    /// 昨日收盘价（只读）。
    pub fn last_close(&self) -> Money {
        self.last_close
    }

    /// 隐藏公允价 V（只读）。
    pub fn fundamental_value(&self) -> Money {
        self.fundamental_value
    }

    /// 股票代码（只读引用）。
    pub fn code(&self) -> &StockCode {
        &self.code
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
        let up = self.up_stop();
        let down = self.down_stop();
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

    /// V 几何均值回复一步（种子化）。
    ///
    /// `drift = α·gap + σ·z`，其中 `gap=(mean−V)/V`、`z = next_f64*2−1 ∈ [−1,1]`；
    /// `V_new = round_half_to_even(V_cents × (1+drift))`。
    ///
    /// 防御式（铁律二）：参数非法（α/σ 非有限或 <0、mean≤0、当前 V≤0）→
    /// [`MarketError::InvalidVParams`]；`multiplier=1+drift ≤ 0`（V 将跨零）或
    /// `V_new ≤ 0` → 同样 `Err`，且 **V 不变**（绝不存非正 V，绝不静默 clamp）。
    ///
    /// f64 仅在本方法「乘 drift」边界出现，立即经 [`round_half_to_even_f_to_i64`]
    /// 落回整数分——权威 V 始终是 `Money`（i64 分），无 f64 存储状态。
    pub fn evolve_v(&mut self, params: &VParams, rng: &mut dyn Rng) -> Result<(), MarketError> {
        // 校验参数：α/σ 必须有限且 ≥0；mean 与当前 V 必须 >0。
        if !params.mean_reversion.is_finite() || params.mean_reversion < 0.0 {
            return Err(MarketError::InvalidVParams {
                reason: format!("mean_reversion {} invalid", params.mean_reversion),
            });
        }
        if !params.volatility.is_finite() || params.volatility < 0.0 {
            return Err(MarketError::InvalidVParams {
                reason: format!("volatility {} invalid", params.volatility),
            });
        }
        if params.long_run_mean.cents() <= 0 {
            return Err(MarketError::InvalidVParams {
                reason: "long_run_mean must be > 0".to_string(),
            });
        }
        let v_cents = self.fundamental_value.cents();
        if v_cents <= 0 {
            return Err(MarketError::InvalidVParams {
                reason: "current V must be > 0".to_string(),
            });
        }
        // 边界 f64 计算：仅在此刻引入，立即落回整数分。
        let mean_cents = params.long_run_mean.cents() as f64;
        let v_f = v_cents as f64;
        let z = rng.next_f64() * 2.0 - 1.0; // [-1, 1]
        let gap = (mean_cents - v_f) / v_f;
        let drift = params.mean_reversion * gap + params.volatility * z;
        let multiplier = 1.0 + drift;
        if multiplier <= 0.0 {
            return Err(MarketError::InvalidVParams {
                reason: format!(
                    "multiplier {} <= 0 (V would cross zero)",
                    multiplier
                ),
            });
        }
        let new_v = round_half_to_even_f_to_i64(v_f * multiplier);
        if new_v <= 0 {
            return Err(MarketError::InvalidVParams {
                reason: format!("evolved V {} <= 0", new_v),
            });
        }
        self.fundamental_value = Money::from_cents(new_v);
        Ok(())
    }
}

/// f64 → i64 银行家舍入（round-half-to-even）。用于 V 演化的 `V_cents × multiplier`。
///
/// 取 `floor` 与小数部分 `diff` 比较：<0.5 向下、>0.5 向上、恰为 0.5 取偶。
/// NaN 等异常走 `None` 分支返回 0（理论不可达——multiplier 已校验 >0 且有限、v_f 有限），
/// 随后调用方 `new_v ≤ 0` 触发 `Err`，安全兜底而非静默吞错。
fn round_half_to_even_f_to_i64(x: f64) -> i64 {
    let floor = x.floor();
    let diff = x - floor;
    match diff.partial_cmp(&0.5) {
        Some(std::cmp::Ordering::Less) => floor as i64,
        Some(std::cmp::Ordering::Greater) => (floor + 1.0) as i64,
        Some(std::cmp::Ordering::Equal) => {
            if (floor as i64) % 2 == 0 {
                floor as i64
            } else {
                (floor + 1.0) as i64
            }
        }
        None => 0_i64,
    }
}
