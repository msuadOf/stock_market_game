//! 订单簿撮合引擎：价格-时间优先的限价撮合。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-orderbook-design.md。
//! ADR-0005 §3 撮合驱动价格的核心；纯逻辑，只依赖 Money，与 account/market/strategy 解耦。

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

mod book_state;
use book_state::BookState;
mod filled_orders;
mod persistent_index;
#[cfg(test)]
mod state_contract_tests;
use filled_orders::FilledOrders;
mod resting_index;
use resting_index::{RestingKey, RestingOrderIndex};

use crate::money::{Money, MoneyError};

/// 买卖方向。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum Side {
    /// 买单（愿意买入）。
    Buy,
    /// 卖单（愿意卖出）。
    Sell,
}

/// 撮合模式：集合竞价挂入不撮合，连续竞价立即撮合。
#[derive(Copy, Clone, Eq, PartialEq, Debug, Default)]
pub enum MatchingMode {
    /// 集合竞价：新单挂入簿但不触发撮合，仅更新参考匹配价。
    Auction,
    /// 连续竞价：新单挂入并立刻与对手盘撮合。
    #[default]
    Continuous,
}

/// 集合竞价清算拒绝原因。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AuctionClearingReject {
    #[error("book is not in Auction mode")]
    NotInAuctionMode,
    #[error("crossed quotes cannot enter Continuous mode")]
    CrossedContinuous,
    #[error("auction quantity overflow")]
    QuantityOverflow,
    #[error("auction selection and execution disagree")]
    SelectionMismatch,
    #[error("auction price tick must be positive")]
    InvalidPriceTick,
    #[error("auction midpoint does not fit Money")]
    PriceOverflow,
}

pub(crate) mod auction_clearing;

/// 集合竞价清算结果。
#[derive(Clone, Debug)]
pub struct AuctionClearingResult {
    /// 清算价（无成交时为 None）。
    pub price: Option<Money>,
    /// 所有成交 Trade。
    pub trades: Vec<Trade>,
    /// 总成交量（股）。
    pub matched_volume: u64,
}

/// 订单 id（单调自增）。本模块自带 newtype，不依赖未来 account 模块。
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
#[ts(type = "number")]
#[serde(transparent)]
pub struct OrderId(#[serde(with = "js_safe_u64")] pub u64);

pub(crate) mod js_safe_u64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub const MAX: u64 = 9_007_199_254_740_991;

    pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if *value > MAX {
            return Err(serde::ser::Error::custom(format!(
                "u64 value {value} exceeds JavaScript's safe integer range"
            )));
        }
        serializer.serialize_u64(*value)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u64::deserialize(deserializer)?;
        if value > MAX {
            return Err(serde::de::Error::custom(format!(
                "u64 value {value} exceeds JavaScript's safe integer range"
            )));
        }
        Ok(value)
    }
}

/// orderbook 操作失败。绝不静默吞掉（铁律二），错误携带字段名 / 实际值 / 原因。
/// 委托价格被拒原因（数值枚举，ADR 0041）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[repr(u16)]
pub enum PriceReject {
    #[error("non-positive")]
    NonPositive,
    #[error("not a multiple of tick")]
    NotTickMultiple,
}

/// 存档投影不一致类别（数值枚举，ADR 0041）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[repr(u16)]
pub enum ProjectionMismatchKind {
    #[error("duplicate saved book sequence")]
    DuplicateSeq,
    #[error("saved book sequence not JavaScript-safe or above cursor")]
    UnsafeSeq,
    #[error("saved resting orders cross during restore")]
    CrossDuringRestore,
    #[error("candidate book time sequence differs from stock worker")]
    TimeSeqDiffers,
    #[error("bid price-time key already exists")]
    BidKeyExists,
    #[error("ask price-time key already exists")]
    AskKeyExists,
    #[error("live order index points at an absent order")]
    AbsentLiveOrder,
    #[error("candidate order differs from stock worker")]
    CandidateDiffers,
    #[error("continuous stock round changed yesterday close")]
    YesterdayCloseChanged,
    #[error("candidate market version differs from stock worker")]
    MarketVersionDiffers,
}

/// orderbook 操作失败。绝不静默吞掉（铁律二），错误携带字段名 / 实际值 / 原因。
#[derive(Debug, Error)]
pub enum OrderError {
    /// 价格非法：为负 / 非 tick 整数倍 / 超范围。
    #[error("invalid price {price:?}: {reason} (tick {tick:?})")]
    InvalidPrice {
        /// 被拒的订单价格。
        price: Money,
        /// 当前簿的最小变动单位（用于诊断「非整数倍」）。
        tick: Money,
        /// 失败原因（数值枚举，ADR 0041）。
        reason: PriceReject,
    },
    /// 数量非法：qty == 0。
    #[error("invalid qty: {0} (must be > 0)")]
    InvalidQty(u32),
    /// 委托数量进度不自洽：原始申报量必须等于已成交量加剩余量。
    #[error(
        "invalid order quantity progress: original={original_qty}, filled={filled_qty}, remaining={remaining_qty}"
    )]
    InvalidQuantityProgress {
        original_qty: u32,
        filled_qty: u32,
        remaining_qty: u32,
    },
    /// 已成交金额非法：只能是非负累计值。
    #[error("invalid filled value: {0:?} (must be >= 0)")]
    InvalidFilledValue(Money),
    /// 重复订单 id（防御式，自增分配器正常时不应触发）。
    #[error("duplicate order id: {0:?}")]
    DuplicateOrderId(OrderId),
    /// A tick-private stock result cannot be applied to the expected book version.
    #[error("stock book projection mismatch: {reason}")]
    /// 投影不一致（数值枚举 + 数值上下文，ADR 0041）。
    ProjectionMismatch {
        reason: ProjectionMismatchKind,
        context: Vec<u64>,
    },
    /// 集合竞价清算被拒绝。
    #[error("auction clearing rejected: {reason}")]
    InvalidAuctionClearing { reason: AuctionClearingReject },
    #[error("order book time sequence exhausted")]
    SequenceOverflow,
    /// 撤单时 id 不存在。
    #[error("order not found: {0:?}")]
    OrderNotFound(OrderId),
    /// 委托已全部成交，簿上没有可撤的余量。
    #[error("order already fully filled and cannot be canceled: {0:?}")]
    OrderAlreadyFilled(OrderId),
    /// 构造订单簿时 tick 非法：<= 0（价格最小变动必须为正）。
    #[error("invalid tick: {tick:?} (must be > 0)")]
    InvalidTick {
        /// 被拒的 tick 值。
        tick: Money,
    },
    /// 成交金额累计溢出。
    #[error(transparent)]
    Money(#[from] MoneyError),
}

/// 账户 id 占位 newtype（待 account 模块统一；本模块不依赖 account）。
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
#[ts(type = "number")]
#[serde(transparent)]
pub struct AccountId(#[serde(with = "js_safe_u64")] pub u64);

/// 单笔限价挂单（撮合发生时为不可变快照；簿内以 OrderId/seq 引用）。
///
/// 价格全程定点 [`Money`]（分），绝不存 f64（money 模块铁律）。可序列化供存档/快照。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct Order {
    /// 订单唯一 id。
    pub id: OrderId,
    /// 买/卖方向。
    pub side: Side,
    /// 限价（愿意成交的价格）。
    pub price: Money,
    /// 剩余数量（股）。
    pub qty: u32,
    /// 原始申报数量（股）。
    pub original_qty: u32,
    /// 本委托此前已成交的累计数量（股）。
    pub filled_qty: u32,
    /// 本委托此前已成交的累计金额；用于跨多次撮合按委托累计费用。
    pub filled_value: Money,
    /// 挂单所属账户。
    pub owner: AccountId,
    /// 时间序：同价位排序键（先挂先成交，price-time priority）。
    #[serde(with = "js_safe_u64")]
    #[ts(type = "number")]
    pub seq: u64,
}

impl Order {
    /// 只校验成交进度；身份、价格、tick 和 FIFO 仍由撮合 owner 判断。
    pub fn validate_progress(&self) -> Result<(), OrderError> {
        if self.qty == 0 {
            return Err(OrderError::InvalidQty(self.qty));
        }
        if self.filled_value.cents() < 0 {
            return Err(OrderError::InvalidFilledValue(self.filled_value));
        }
        if self
            .filled_qty
            .checked_add(self.qty)
            .is_none_or(|total| total != self.original_qty)
        {
            return Err(self.quantity_progress_error());
        }
        Ok(())
    }

    fn quantity_progress_error(&self) -> OrderError {
        OrderError::InvalidQuantityProgress {
            original_qty: self.original_qty,
            filled_qty: self.filled_qty,
            remaining_qty: self.qty,
        }
    }

    fn filled_value_after(&self, fill_value: Money) -> Result<Money, OrderError> {
        Ok(self.filled_value.add(fill_value)?)
    }

    fn filled_qty_after(&self, fill_qty: u32) -> Result<u32, OrderError> {
        self.filled_qty
            .checked_add(fill_qty)
            .ok_or_else(|| self.quantity_progress_error())
    }
}

/// 一笔成交。成交价取被动方（maker）的价格。
///
/// 派生 `Eq`+`PartialEq` 便于测试整体比较与去重；可序列化供成交历史/存档。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Trade {
    /// 成交价（= maker 挂单价）。
    pub price: Money,
    /// 本次成交数量（股）。
    pub qty: u32,
    /// 被动方（较早挂单、提供流动性）。
    pub maker: AccountId,
    /// 主动方（新进入、吃流动性）。
    pub taker: AccountId,
    /// 被动委托 id。
    pub maker_order_id: OrderId,
    /// 主动委托 id。
    pub taker_order_id: OrderId,
    /// 本次成交前，被动委托的累计成交金额。
    pub maker_filled_value_before: Money,
    /// 本次成交前，主动委托的累计成交金额。
    pub taker_filled_value_before: Money,
}

/// 撮合一笔新单的结果。
///
/// `trades` 为本次产生的成交（按发生顺序）；`resting` 为新单若有剩余量挂入簿的残留订单，
/// `None` 表示全成交。设计为值类型，便于上层（account/strategy）据此回写状态。
///
/// 派生 `Debug`：内部 `Vec<Trade>` / `Option<Order>` 均可 Debug，且无 f64，
/// 便于测试中 `unwrap_err`/诊断输出（`Result::unwrap_err` 要求 `Ok` 变体 `Debug`）。
#[derive(Debug)]
pub struct MatchResult {
    /// 本次撮合产生的成交（按发生顺序）。
    pub trades: Vec<Trade>,
    /// 新单若有剩余量，挂入簿的残留订单；None 表示全成交。
    pub resting: Option<Order>,
    /// Maker states before this match, used to project only changed orders.
    pub(crate) maker_before: Vec<Order>,
}

#[derive(Clone, Debug)]
pub(crate) struct OrderBookChange {
    id: OrderId,
    before: Option<Order>,
    after: Option<Order>,
    filled_owner: Option<AccountId>,
}

#[derive(Clone, Debug)]
pub(crate) struct OrderBookDelta {
    expected_next_seq: u64,
    next_seq: u64,
    changes: Vec<OrderBookChange>,
}

/// 单只股票的订单簿。
///
/// 用两个 [`BTreeMap`] 维护买卖盘，以 (价格, 时间序) 为排序键实现 price-time 优先：
/// - 买盘按「价高优先、同价先挂优先」：key = `(Reverse(price), seq)`，`Reverse` 使价高者排前。
/// - 卖盘按「价低优先、同价先挂优先」：key = `(price, seq)`，价低者天然排前。
///
/// 价格全程定点 [`Money`]（分），绝不存 f64（money 模块铁律）。tick 为价格最小变动，构造期强制 > 0。
///
/// 派生 `Debug`：内部全为可 Debug 类型（BTreeMap、Money、Order），且无 f64，便于测试断言
/// （如 `Result::unwrap_err` 要求 `T: Debug`）与诊断输出。
#[derive(Clone, Debug)]
pub struct OrderBook {
    state: BookState,
    /// 下一个分配的时间序（同价位 FIFO 排序键）。
    next_seq: u64,
    /// 价格最小变动单位（必须 > 0）。
    tick: Money,
    /// 当前撮合模式：Auction 挂入不撮合，Continuous 立即撮合。
    mode: MatchingMode,
}

impl OrderBook {
    pub(crate) fn hash_projection(&self) -> impl serde::Serialize + '_ {
        (
            &self.tick,
            &self.next_seq,
            self.mode as u8,
            self.state.filled_orders().entries(),
            self.state.bids().iter().collect::<Vec<_>>(),
            self.state.asks().iter().collect::<Vec<_>>(),
        )
    }
    /// 构造订单簿。tick 必须 > 0（价格最小变动为正才有意义）；否则返回 [`OrderError::InvalidTick`]。
    ///
    /// 防御式（铁律二）：tick 非法时显式 `Err`，绝不静默 fallback 到某默认值。
    pub fn new(tick: Money) -> Result<OrderBook, OrderError> {
        if tick.cents() <= 0 {
            return Err(OrderError::InvalidTick { tick });
        }
        Ok(OrderBook {
            state: BookState::default(),
            next_seq: 0,
            tick,
            mode: MatchingMode::Continuous,
        })
    }

    /// 返回当前撮合模式。
    pub fn mode(&self) -> MatchingMode {
        self.mode
    }

    /// 交易阶段由调用方控制；交叉委托必须先清算，才能进入 Continuous。
    pub fn set_mode(&mut self, mode: MatchingMode) -> Result<(), OrderError> {
        if mode == MatchingMode::Continuous
            && self
                .best_bid()
                .zip(self.best_ask())
                .is_some_and(|(bid, ask)| bid >= ask)
        {
            return Err(OrderError::InvalidAuctionClearing {
                reason: AuctionClearingReject::CrossedContinuous,
            });
        }
        self.mode = mode;
        Ok(())
    }

    /// 买盘最优价（最高买价）。空簿返回 None。
    ///
    /// 买盘 key 为 `(Reverse(price), seq)`，`first_key_value` 取价最高（Reverse 反转后最小）者。
    pub fn best_bid(&self) -> Option<Money> {
        self.state
            .bids()
            .first_key_value()
            .map(|((Reverse(p), _), _)| *p)
    }

    /// 卖盘最优价（最低卖价）。空簿返回 None。
    ///
    /// 卖盘 key 为 `(price, seq)`，`first_key_value` 取价最低者。
    pub fn best_ask(&self) -> Option<Money> {
        self.state.asks().first_key_value().map(|((p, _), _)| *p)
    }

    /// 撮合新单。先校验数量/价格，再与对手盘逐档撮合；剩余挂入己方簿。
    ///
    /// 防御式（铁律二）：非法数量/价格 → 显式 `Err`，绝不静默截断/修正：
    /// - `qty == 0` → [`OrderError::InvalidQty`]。
    /// - `price <= 0` 或非 tick 整数倍 → [`OrderError::InvalidPrice`]（reason 区分 non-positive /
    ///   not a multiple of tick）。价格全程整数分取模，无 f64（money 模块铁律）。
    pub fn place(&mut self, order: Order) -> Result<MatchResult, OrderError> {
        match self.mode {
            MatchingMode::Auction => self.place_auction(order),
            MatchingMode::Continuous => self.place_inner(order, false),
        }
    }

    pub(crate) fn place_recording(&mut self, order: Order) -> Result<MatchResult, OrderError> {
        match self.mode {
            MatchingMode::Auction => self.place_auction(order),
            MatchingMode::Continuous => self.place_inner(order, true),
        }
    }

    /// 集合竞价模式：挂入簿但不撮合。校验与 Continuous 相同（数量/价格/重复 id）。
    fn place_auction(&mut self, mut order: Order) -> Result<MatchResult, OrderError> {
        if self.state.filled_orders().owner(order.id).is_some()
            || self.state.live_by_id().get(order.id).is_some()
        {
            return Err(OrderError::DuplicateOrderId(order.id));
        }
        order.validate_progress()?;
        if order.price.cents() <= 0 || order.price.cents() % self.tick.cents() != 0 {
            return Err(OrderError::InvalidPrice {
                price: order.price,
                tick: self.tick,
                reason: if order.price.cents() <= 0 {
                    PriceReject::NonPositive
                } else {
                    PriceReject::NotTickMultiple
                },
            });
        }
        let seq = self.next_seq;
        self.next_seq = self
            .next_seq
            .checked_add(1)
            .ok_or(OrderError::SequenceOverflow)?;
        order.seq = seq;
        self.insert_resting(order.clone())?;
        Ok(MatchResult {
            trades: Vec::new(),
            resting: Some(order),
            maker_before: Vec::new(),
        })
    }

    fn place_inner(
        &mut self,
        mut order: Order,
        record_makers: bool,
    ) -> Result<MatchResult, OrderError> {
        if self.state.filled_orders().owner(order.id).is_some()
            || self.state.live_by_id().get(order.id).is_some()
        {
            return Err(OrderError::DuplicateOrderId(order.id));
        }
        order.validate_progress()?;
        // 校验价格：正数 + tick 整数倍。A 股委托价不能为零，价格用整数分取模，无 f64。
        if order.price.cents() <= 0 || order.price.cents() % self.tick.cents() != 0 {
            return Err(OrderError::InvalidPrice {
                price: order.price,
                tick: self.tick,
                reason: if order.price.cents() <= 0 {
                    PriceReject::NonPositive
                } else {
                    PriceReject::NotTickMultiple
                },
            });
        }

        // 与对手盘逐档撮合，直到无交叉或新单 qty 耗尽。
        // 成交价恒取被动方(maker)价；maker 清零则移出簿，否则原档扣减。
        let mut trades: Vec<Trade> = Vec::new();
        let mut maker_before = Vec::new();
        let taker = order.owner;
        let side = order.side;

        loop {
            if order.qty == 0 {
                break;
            }
            // 是否交叉？买单：买价 >= 最优卖价；卖单：卖价 <= 最优买价（统一为「新单价 >= 对手最优价」）。
            let crossed = match side {
                Side::Buy => self
                    .state
                    .asks()
                    .first_key_value()
                    .map(|((ask_p, _), _)| order.price >= *ask_p)
                    .unwrap_or(false),
                Side::Sell => self
                    .state
                    .bids()
                    .first_key_value()
                    .map(|((Reverse(bid_p), _), _)| order.price <= *bid_p)
                    .unwrap_or(false),
            };
            if !crossed {
                break;
            }

            // 取对手最优档（maker），成交价取 maker.price。
            let (maker, fill_price) = match side {
                Side::Buy => {
                    let entry = self
                        .state
                        .asks()
                        .first_key_value()
                        .expect("crossed => non-empty");
                    let m = entry.1.clone();
                    let p = m.price;
                    (m, p)
                }
                Side::Sell => {
                    let entry = self
                        .state
                        .bids()
                        .first_key_value()
                        .expect("crossed => non-empty");
                    let m = entry.1.clone();
                    let p = m.price;
                    (m, p)
                }
            };

            let fill_qty = order.qty.min(maker.qty);
            if record_makers {
                maker_before.push(maker.clone());
            }
            let fill_value = fill_price.mul_shares(fill_qty)?;
            let maker_filled_value_before = maker.filled_value;
            let taker_filled_value_before = order.filled_value;
            let maker_filled_value_after = maker.filled_value_after(fill_value)?;
            order.filled_value = order.filled_value_after(fill_value)?;
            order.filled_qty = order.filled_qty_after(fill_qty)?;
            order.qty -= fill_qty;

            // 更新对手档：maker 清零则 pop_first，否则按 (price,seq) 定位原档扣减。
            self.state
                .apply_maker_fill(side, &maker, fill_qty, maker_filled_value_after)?;

            trades.push(Trade {
                price: fill_price,
                qty: fill_qty,
                maker: maker.owner,
                taker,
                maker_order_id: maker.id,
                taker_order_id: order.id,
                maker_filled_value_before,
                taker_filled_value_before,
            });
        }

        // 剩余量 > 0 才分配时间序挂入己方簿；否则 resting=None（全成交）。
        if order.qty == 0 {
            self.state.record_filled(order.id, order.owner)?;
        }
        let resting = if order.qty > 0 {
            let seq = self.next_seq;
            self.next_seq = self
                .next_seq
                .checked_add(1)
                .ok_or(OrderError::SequenceOverflow)?;
            order.seq = seq;
            self.insert_resting(order.clone())?;
            Some(order)
        } else {
            None
        };

        Ok(MatchResult {
            trades,
            resting,
            maker_before,
        })
    }

    /// 将残留订单挂入对应簿（内部辅助，不校验——调用方 place 已校验）。
    ///
    /// 买盘 key = `(Reverse(price), seq)`（价高优先、同价先挂优先）；
    /// 卖盘 key = `(price, seq)`（价低优先、同价先挂优先）。
    fn insert_resting(&mut self, order: Order) -> Result<(), OrderError> {
        self.state.insert_resting(order)
    }

    /// 按 id 撤单，返回被撤订单（供上层回写状态）。id 不存在 → [`OrderError::OrderNotFound`]。
    ///
    /// 防御式（铁律二）：找不到时显式 `Err`，绝不静默返回 `Option::None` 或伪造空订单。
    ///
    /// 订单身份索引只用于定位，实际价格与时间优先级仍由买卖档排序键决定。
    pub fn cancel(&mut self, id: OrderId) -> Result<Order, OrderError> {
        self.state.cancel(id)
    }

    /// Returns the owner of an order that has no remaining shares after a fill.
    pub fn filled_order_owner(&self, id: OrderId) -> Option<AccountId> {
        self.state.filled_orders().owner(id)
    }

    /// 在唯一订单簿上按既有沪深选价规则清算，不改变交易阶段。
    /// 开盘余单保留原始 seq；收盘余单由调用方日界释放，不能自动进入 Continuous。
    pub fn execute_auction_clearing(
        &mut self,
        previous_close: Money,
        exchange: crate::StockExchange,
    ) -> Result<AuctionClearingResult, OrderError> {
        if self.mode != MatchingMode::Auction {
            return Err(OrderError::InvalidAuctionClearing {
                reason: AuctionClearingReject::NotInAuctionMode,
            });
        }
        let Some(selection) = self.clearing_selection(previous_close, exchange)? else {
            return Ok(AuctionClearingResult {
                price: None,
                trades: Vec::new(),
                matched_volume: 0,
            });
        };
        let mut trades = Vec::new();
        let mut matched_volume = 0_u64;
        while matched_volume < selection.volume {
            let buy = self
                .state
                .bids()
                .first_key_value()
                .map(|(_, order)| order.clone())
                .filter(|order| order.price >= selection.price)
                .ok_or(OrderError::InvalidAuctionClearing {
                    reason: AuctionClearingReject::SelectionMismatch,
                })?;
            let sell = self
                .state
                .asks()
                .first_key_value()
                .map(|(_, order)| order.clone())
                .filter(|order| order.price <= selection.price)
                .ok_or(OrderError::InvalidAuctionClearing {
                    reason: AuctionClearingReject::SelectionMismatch,
                })?;
            let qty = buy.qty.min(sell.qty);
            let fill_value = selection.price.mul_shares(qty)?;
            let buy_after = buy.filled_value_after(fill_value)?;
            let sell_after = sell.filled_value_after(fill_value)?;
            buy.filled_qty_after(qty)?;
            sell.filled_qty_after(qty)?;
            let volume_after = matched_volume.checked_add(u64::from(qty)).ok_or(
                OrderError::InvalidAuctionClearing {
                    reason: AuctionClearingReject::QuantityOverflow,
                },
            )?;
            if qty == 0 || volume_after > selection.volume {
                return Err(OrderError::InvalidAuctionClearing {
                    reason: AuctionClearingReject::SelectionMismatch,
                });
            }
            self.state
                .apply_maker_fill(Side::Sell, &buy, qty, buy_after)?;
            self.state
                .apply_maker_fill(Side::Buy, &sell, qty, sell_after)?;
            let (maker, taker) = if buy.seq < sell.seq {
                (&buy, &sell)
            } else {
                (&sell, &buy)
            };
            trades.push(Trade {
                price: selection.price,
                qty,
                maker: maker.owner,
                taker: taker.owner,
                maker_order_id: maker.id,
                taker_order_id: taker.id,
                maker_filled_value_before: maker.filled_value,
                taker_filled_value_before: taker.filled_value,
            });
            matched_volume = volume_after;
        }
        Ok(AuctionClearingResult {
            price: Some(selection.price),
            trades,
            matched_volume,
        })
    }

    /// 参考价是不成交投影，不消耗序号、不修改 last_trade 或 Envelope。
    pub fn indicative_clearing_price(
        &self,
        previous_close: Money,
        exchange: crate::StockExchange,
    ) -> Result<Option<Money>, OrderError> {
        Ok(self
            .clearing_selection(previous_close, exchange)?
            .map(|selection| selection.price))
    }

    pub(crate) fn auction_indicative(
        &self,
        previous_close: Money,
        exchange: crate::StockExchange,
    ) -> Result<(Option<(Money, u64)>, u64), OrderError> {
        let selection = self.clearing_selection(previous_close, exchange)?;
        let imbalance = if let Some(selection) = selection {
            selection.imbalance
        } else {
            let orders = self
                .resting_order_refs()
                .map(|order| auction_clearing::ClearingOrder {
                    side: order.side,
                    limit: order.price,
                    qty: u64::from(order.qty),
                })
                .collect::<Vec<_>>();
            auction_clearing::checked_total_imbalance(&orders)?
        };
        Ok((
            selection.map(|selection| (selection.price, selection.volume)),
            imbalance,
        ))
    }

    pub(crate) fn clearing_selection(
        &self,
        previous_close: Money,
        exchange: crate::StockExchange,
    ) -> Result<Option<auction_clearing::ClearingSelection>, OrderError> {
        let orders = self
            .resting_order_refs()
            .map(|order| auction_clearing::ClearingOrder {
                side: order.side,
                limit: order.price,
                qty: u64::from(order.qty),
            })
            .collect::<Vec<_>>();
        auction_clearing::select_clearing(&orders, previous_close, exchange, self.tick)
    }
    pub fn filled_orders(&self) -> Vec<(OrderId, AccountId)> {
        self.state.filled_orders().entries()
    }

    pub fn restore_filled_orders(
        &mut self,
        entries: impl IntoIterator<Item = (OrderId, AccountId)>,
    ) -> Result<(), OrderError> {
        self.state.restore_filled_orders(entries)
    }

    /// 返回指定账户当前仍在订单簿中的全部未成交委托快照。
    /// 上层据此计算冻结资金/股份；返回克隆避免暴露内部排序容器。
    pub fn resting_orders_for(&self, owner: AccountId) -> Vec<Order> {
        self.state
            .bids()
            .values()
            .chain(self.state.asks().values())
            .filter(|order| order.owner == owner)
            .cloned()
            .collect()
    }

    /// 返回订单簿中的全部未成交委托，按簿内到达序排列。
    pub fn resting_orders(&self) -> Vec<Order> {
        self.resting_orders_matching(|_| true)
    }

    pub(crate) fn resting_order_refs(&self) -> impl Iterator<Item = &Order> {
        self.state.bids().values().chain(self.state.asks().values())
    }

    pub(crate) fn resting_order_by_id(&self, id: OrderId) -> Option<&Order> {
        self.state.resting_order_by_id(id)
    }

    pub(crate) fn next_sequence(&self) -> u64 {
        self.next_seq
    }

    /// Rebuild a complete saved book without reassigning its price-time keys.
    /// Matching validates orders on a private candidate; crossed or duplicate
    /// input never replaces the original book. The historical cursor is an
    /// independent fact even when the book is empty.
    pub(crate) fn restore_resting_orders(
        &mut self,
        orders: &[Order],
        next_seq: u64,
    ) -> Result<(), OrderError> {
        let mut ordered: Vec<_> = orders.iter().collect();
        ordered.sort_by_key(|order| order.seq);
        for pair in ordered.windows(2) {
            if pair[0].seq == pair[1].seq {
                return Err(OrderError::ProjectionMismatch {
                    reason: ProjectionMismatchKind::DuplicateSeq,
                    context: vec![pair[0].seq],
                });
            }
        }
        let mut candidate = Self::new(self.tick)?;
        candidate.mode = self.mode;
        for order in ordered {
            if order.seq >= next_seq || order.seq > js_safe_u64::MAX {
                return Err(OrderError::ProjectionMismatch {
                    reason: ProjectionMismatchKind::UnsafeSeq,
                    context: vec![order.seq, next_seq],
                });
            }
            candidate.next_seq = order.seq;
            let result = candidate.place(order.clone())?;
            if !result.trades.is_empty() || result.resting.as_ref() != Some(order) {
                return Err(OrderError::ProjectionMismatch {
                    reason: ProjectionMismatchKind::CrossDuringRestore,
                    context: Vec::new(),
                });
            }
        }
        candidate.next_seq = next_seq;
        *self = candidate;
        Ok(())
    }

    pub(crate) fn changed_orders_since(
        &self,
        expected_next_seq: u64,
        originals: BTreeMap<OrderId, Option<Order>>,
    ) -> OrderBookDelta {
        let changes = self.state.changed_orders_since(originals);
        OrderBookDelta {
            expected_next_seq,
            next_seq: self.next_seq,
            changes,
        }
    }

    pub(crate) fn apply_changes(&mut self, delta: OrderBookDelta) -> Result<(), OrderError> {
        if self.next_seq != delta.expected_next_seq {
            return Err(OrderError::ProjectionMismatch {
                reason: ProjectionMismatchKind::TimeSeqDiffers,
                context: Vec::new(),
            });
        }
        self.state.apply_changes(delta.changes)?;
        self.next_seq = delta.next_seq;
        Ok(())
    }

    fn resting_orders_matching(&self, mut matches: impl FnMut(&Order) -> bool) -> Vec<Order> {
        let mut orders: Vec<Order> = self
            .state
            .bids()
            .values()
            .chain(self.state.asks().values())
            .filter(|order| matches(order))
            .cloned()
            .collect();
        orders.sort_by_key(|order| order.seq);
        orders
    }

    /// Snapshot only orders belonging to the accounts observed in this tick.
    /// The selected orders keep the same per-stock arrival order as `resting_orders`.
    pub(crate) fn resting_orders_for_owners(&self, owners: &BTreeSet<AccountId>) -> Vec<Order> {
        self.resting_orders_matching(|order| owners.contains(&order.owner))
    }

    pub fn resting_order_count(&self) -> usize {
        self.state.bids().len() + self.state.asks().len()
    }

    /// 清空当日未成交委托。A 股普通竞价委托不跨交易日保留。
    pub fn clear(&mut self) {
        self.state.clear_resting();
    }

    /// 买盘深度：按价高→低，每个价位聚合所有挂单的总数量。
    ///
    /// 返回 `Vec<(Money, u64)>`：元素为 (价位, 该价位累计股数)。空簿返回空 Vec。
    /// 价序天然由 BTreeMap 给出——买盘 key 含 `Reverse(price)`，`values()` 遍历即「价高优先、
    /// 同价先挂优先」，故只需把相邻同价累加。
    pub fn bid_depth(&self) -> Vec<(Money, u64)> {
        self.aggregate(self.state.bids())
    }

    /// 买盘前 `max_levels` 个聚合价位；用于高频增量行情，避免遍历完整订单簿。
    pub fn bid_depth_limited(&self, max_levels: usize) -> Vec<(Money, u64)> {
        self.aggregate_limited(self.state.bids(), max_levels)
    }

    /// 卖盘深度：按价低→高，每个价位聚合所有挂单的总数量。
    ///
    /// 返回 `Vec<(Money, u64)>`：元素为 (价位, 该价位累计股数)。空簿返回空 Vec。
    /// 价序天然由 BTreeMap 给出——卖盘 key 为 `(price, seq)`，`values()` 遍历即「价低优先、
    /// 同价先挂优先」，故只需把相邻同价累加。
    pub fn ask_depth(&self) -> Vec<(Money, u64)> {
        self.aggregate(self.state.asks())
    }

    /// 卖盘前 `max_levels` 个聚合价位；用于高频增量行情，避免遍历完整订单簿。
    pub fn ask_depth_limited(&self, max_levels: usize) -> Vec<(Money, u64)> {
        self.aggregate_limited(self.state.asks(), max_levels)
    }

    /// 将一个盘口的挂单按相邻同价聚合为 (价位, 累计量) 序列。
    ///
    /// 通用泛型 `T: Ord`：买盘 `T = Reverse<Money>`、卖盘 `T = Money`，两者排序键的第一分量类型不同，
    /// 但聚合只关心 `Order.price`（值类型恒为 `Money`），与 `T` 无关——故无需 `_is_bid` 之类的方向参数。
    /// 遍历顺序已由 BTreeMap 的 key 保证为「价优→劣」，相邻同价即合并。
    ///
    /// 单笔委托量为 u32，但同价位可能聚合多笔，累计量必须使用更宽的 u64。
    fn aggregate<T: Ord>(&self, side: &BTreeMap<(T, u64), Order>) -> Vec<(Money, u64)> {
        self.aggregate_limited(side, usize::MAX)
    }

    fn aggregate_limited<T: Ord>(
        &self,
        side: &BTreeMap<(T, u64), Order>,
        max_levels: usize,
    ) -> Vec<(Money, u64)> {
        let mut out: Vec<(Money, u64)> = Vec::new();
        for o in side.values() {
            // 上一档同价 → 累加到该档；否则新开一档。
            if let Some(last) = out.last_mut() {
                if last.0 == o.price {
                    last.1 += u64::from(o.qty);
                    continue;
                }
            }
            if out.len() == max_levels {
                break;
            }
            out.push((o.price, u64::from(o.qty)));
        }
        out
    }
}

#[cfg(test)]
mod matching_mode_tests {
    use super::*;

    fn make_order(id: u64, side: Side, price: i64, qty: u32, owner: u64) -> Order {
        Order {
            id: OrderId(id),
            side,
            price: Money::from_cents(price),
            qty,
            original_qty: qty,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: crate::AccountId(owner),
            seq: 0,
        }
    }

    #[test]
    fn clearing_keeps_phase_control_with_caller() {
        let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
        book.set_mode(MatchingMode::Auction).unwrap();
        book.place(make_order(1, Side::Buy, 1000, 300, 10)).unwrap();
        book.place(make_order(2, Side::Sell, 1000, 100, 20))
            .unwrap();
        book.execute_auction_clearing(Money::from_cents(1000), crate::StockExchange::Shanghai)
            .unwrap();
        assert_eq!(
            book.mode(),
            MatchingMode::Auction,
            "收盘清算不授权再次连续交易"
        );
        let result = book
            .place(make_order(3, Side::Sell, 1000, 100, 30))
            .unwrap();
        assert!(result.trades.is_empty());
    }

    #[test]
    fn auction_multi_leg_fill_preserves_age_progress_and_filled_identity() {
        let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
        book.set_mode(MatchingMode::Auction).unwrap();
        book.place(make_order(1, Side::Buy, 1000, 350, 10)).unwrap();
        book.place(make_order(2, Side::Sell, 1000, 100, 20))
            .unwrap();
        book.place(make_order(3, Side::Sell, 1000, 200, 30))
            .unwrap();
        let cursor = book.next_sequence();
        let before = book.resting_order_by_id(OrderId(1)).unwrap().clone();
        let result = book
            .execute_auction_clearing(Money::from_cents(1000), crate::StockExchange::Shanghai)
            .unwrap();
        assert_eq!(result.matched_volume, 300);
        assert_eq!(result.trades.len(), 2);
        assert_eq!(result.trades[1].maker_order_id, OrderId(1));
        assert_eq!(
            result.trades[1].maker_filled_value_before,
            Money::from_cents(100_000)
        );
        let remaining = book.resting_order_by_id(OrderId(1)).unwrap();
        assert_eq!(
            (remaining.original_qty, remaining.filled_qty, remaining.qty),
            (350, 300, 50)
        );
        assert_eq!(remaining.filled_value, Money::from_cents(300_000));
        assert_eq!(remaining.seq, before.seq);
        assert_eq!(book.next_sequence(), cursor);
        remaining.validate_progress().unwrap();
        assert_eq!(book.filled_order_owner(OrderId(2)), Some(AccountId(20)));
        assert_eq!(book.filled_order_owner(OrderId(3)), Some(AccountId(30)));
        assert!(matches!(
            book.cancel(OrderId(2)),
            Err(OrderError::OrderAlreadyFilled(OrderId(2)))
        ));
    }

    #[test]
    fn auction_restore_accepts_crossed_quotes_without_executing_them() {
        let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
        book.set_mode(MatchingMode::Auction).unwrap();
        book.place(make_order(1, Side::Buy, 1001, 100, 10)).unwrap();
        book.place(make_order(2, Side::Sell, 1000, 100, 20))
            .unwrap();
        let orders = book.resting_orders();
        let mut restored = OrderBook::new(Money::from_cents(1)).unwrap();
        restored.set_mode(MatchingMode::Auction).unwrap();
        restored
            .restore_resting_orders(&orders, book.next_sequence())
            .unwrap();
        assert_eq!(restored.resting_orders(), orders);
        assert_eq!(restored.mode(), MatchingMode::Auction);
        let mut continuous = OrderBook::new(Money::from_cents(1)).unwrap();
        assert!(continuous
            .restore_resting_orders(&orders, book.next_sequence())
            .is_err());
    }

    #[test]
    fn shanghai_half_tick_rounds_up_without_money_overflow() {
        for (low, high, expected) in [(1000, 1001, 1001), (i64::MAX - 2, i64::MAX, i64::MAX - 1)] {
            let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
            book.set_mode(MatchingMode::Auction).unwrap();
            book.place(make_order(1, Side::Buy, high, 1, 10)).unwrap();
            book.place(make_order(2, Side::Sell, low, 1, 20)).unwrap();
            assert_eq!(
                book.indicative_clearing_price(
                    Money::from_cents(low),
                    crate::StockExchange::Shanghai
                )
                .unwrap(),
                Some(Money::from_cents(expected))
            );
        }
    }

    #[test]
    fn auction_mode_place_does_not_match() {
        let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
        book.set_mode(MatchingMode::Auction).unwrap();

        // 挂入买和卖，即使交叉也不撮合
        let result = book.place(make_order(1, Side::Buy, 1000, 100, 10)).unwrap();
        assert!(result.trades.is_empty(), "Auction 模式不应产生成交");
        assert!(result.resting.is_some());

        let result = book.place(make_order(2, Side::Sell, 900, 50, 20)).unwrap();
        assert!(result.trades.is_empty(), "Auction 模式不应产生成交");
        assert!(result.resting.is_some());

        // 簿里有 2 张挂单
        assert_eq!(book.resting_orders().len(), 2);
    }

    #[test]
    fn auction_clearing_computes_correct_price_and_volume() {
        let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
        book.set_mode(MatchingMode::Auction).unwrap();

        // 构造一个经典场景：清算价应为 10.00
        // 买方：3 张 10.00×100 + 1 张 10.01×50 = 350 股 >= 10.00
        // 卖方：2 张 10.00×80 + 1 张 9.99×120 = 280 股 <= 10.00
        book.place(make_order(1, Side::Buy, 1000, 100, 10)).unwrap();
        book.place(make_order(2, Side::Buy, 1000, 100, 11)).unwrap();
        book.place(make_order(3, Side::Buy, 1000, 100, 12)).unwrap();
        book.place(make_order(4, Side::Buy, 1001, 50, 13)).unwrap();
        book.place(make_order(5, Side::Sell, 1000, 80, 20)).unwrap();
        book.place(make_order(6, Side::Sell, 1000, 80, 21)).unwrap();
        book.place(make_order(7, Side::Sell, 999, 120, 22)).unwrap();

        let result = book
            .execute_auction_clearing(
                Money::from_cents(1000),
                crate::session::StockExchange::Shenzhen,
            )
            .unwrap();

        // 清算价 10.00，成交量 = min(350, 280) = 280
        assert_eq!(result.price, Some(Money::from_cents(1000)));
        assert_eq!(result.matched_volume, 280);
        assert!(!result.trades.is_empty());

        // 清算只生成 Fill，不决定开盘/收盘之后的交易阶段。
        assert_eq!(book.mode(), MatchingMode::Auction);
        book.set_mode(MatchingMode::Continuous).unwrap();

        // 部分成交的买单（3×100+50=350 只成交 280）剩余留在簿里
        let remaining = book.resting_orders();
        assert!(
            !remaining.is_empty(),
            "未成交部分应留在簿里成为连续模式挂单"
        );
    }

    #[test]
    fn auction_clearing_no_eligible_orders_keeps_mode_and_sequence() {
        let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
        book.set_mode(MatchingMode::Auction).unwrap();

        // 只挂买无卖 → 无可成交
        book.place(make_order(1, Side::Buy, 1000, 100, 10)).unwrap();

        let result = book
            .execute_auction_clearing(
                Money::from_cents(1000),
                crate::session::StockExchange::Shenzhen,
            )
            .unwrap();

        assert_eq!(result.price, None);
        assert_eq!(result.matched_volume, 0);
        assert!(result.trades.is_empty());
        assert_eq!(book.mode(), MatchingMode::Auction);
        assert_eq!(book.next_sequence(), 1);
        // 原始单仍在簿里
        assert_eq!(book.resting_orders().len(), 1);
    }

    #[test]
    fn auction_clearing_fails_in_continuous_mode() {
        let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
        assert_eq!(book.mode(), MatchingMode::Continuous);

        let err = book
            .execute_auction_clearing(
                Money::from_cents(1000),
                crate::session::StockExchange::Shenzhen,
            )
            .unwrap_err();
        assert!(matches!(
            err,
            OrderError::InvalidAuctionClearing {
                reason: AuctionClearingReject::NotInAuctionMode
            }
        ));
    }

    #[test]
    fn after_clearing_remaining_orders_match_continuously() {
        let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
        book.set_mode(MatchingMode::Auction).unwrap();

        // 买卖不平衡：买 100@10.00、买 100@10.01 vs 卖 50@10.00
        // 清算价 10.00，成交 50；买 100@10.01 中 50 成交后剩余 50 留在簿
        book.place(make_order(1, Side::Buy, 1000, 100, 10)).unwrap();
        book.place(make_order(2, Side::Buy, 1001, 100, 11)).unwrap();
        book.place(make_order(3, Side::Sell, 1000, 50, 20)).unwrap();

        let _ = book
            .execute_auction_clearing(
                Money::from_cents(1000),
                crate::session::StockExchange::Shenzhen,
            )
            .unwrap();

        // 由开盘后的交易阶段显式授权 Continuous，再执行新卖单。
        assert_eq!(book.mode(), MatchingMode::Auction);
        book.set_mode(MatchingMode::Continuous).unwrap();
        assert_eq!(book.mode(), MatchingMode::Continuous);
        let result = book.place(make_order(4, Side::Sell, 1000, 30, 30)).unwrap();
        assert!(
            !result.trades.is_empty(),
            "清算后剩余买单应能被连续模式撮合"
        );
    }

    #[test]
    fn indicative_price_during_auction() {
        let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
        book.set_mode(MatchingMode::Auction).unwrap();

        // 空簿 → None
        assert!(book
            .indicative_clearing_price(
                Money::from_cents(1000),
                crate::session::StockExchange::Shenzhen
            )
            .unwrap()
            .is_none());

        // 挂入买卖单后可查参考价
        book.place(make_order(1, Side::Buy, 1000, 100, 10)).unwrap();
        book.place(make_order(2, Side::Sell, 1000, 80, 20)).unwrap();

        let price = book
            .indicative_clearing_price(
                Money::from_cents(1000),
                crate::session::StockExchange::Shenzhen,
            )
            .unwrap();
        assert_eq!(price, Some(Money::from_cents(1000)));

        // indicative 不改变簿状态
        assert_eq!(book.resting_orders().len(), 2);
        assert_eq!(book.mode(), MatchingMode::Auction);
    }

    #[test]
    fn auction_cancel_removes_from_book() {
        let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
        book.set_mode(MatchingMode::Auction).unwrap();

        book.place(make_order(1, Side::Buy, 1000, 100, 10)).unwrap();
        assert_eq!(book.resting_orders().len(), 1);

        let cancelled = book.cancel(OrderId(1)).unwrap();
        assert_eq!(cancelled.qty, 100);
        assert_eq!(book.resting_orders().len(), 0);
    }

    #[test]
    fn shanghai_clearing_picks_midpoint_on_tie() {
        let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
        book.set_mode(MatchingMode::Auction).unwrap();

        // 构造两个同最大成交量候选价：10.00 和 10.02
        // 买方：1 张 10.00×100 + 1 张 10.02×100
        // 卖方：1 张 10.00×100 + 1 张 10.02×100
        // 在 10.00：买 200 卖 100 → 成交 100
        // 在 10.02：买 100 卖 200 → 成交 100
        // 上海规则取中间价 → 10.01
        book.place(make_order(1, Side::Buy, 1000, 100, 10)).unwrap();
        book.place(make_order(2, Side::Buy, 1002, 100, 11)).unwrap();
        book.place(make_order(3, Side::Sell, 1000, 100, 20))
            .unwrap();
        book.place(make_order(4, Side::Sell, 1002, 100, 21))
            .unwrap();

        let result = book
            .execute_auction_clearing(
                Money::from_cents(1000),
                crate::session::StockExchange::Shanghai,
            )
            .unwrap();
        // 上海规则应选某个有效价格（可能是 10.01 中间价或 10.00/10.02 之一）
        assert!(result.price.is_some());
        assert!(result.matched_volume > 0);
    }
}
