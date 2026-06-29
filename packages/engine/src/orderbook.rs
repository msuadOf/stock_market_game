//! 订单簿撮合引擎：价格-时间优先的限价撮合。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-orderbook-design.md。
//! ADR-0005 §3 撮合驱动价格的核心；纯逻辑，只依赖 Money，与 account/market/strategy 解耦。
//!
//! 当前为 Task 1/2/3：Side/OrderId/OrderError + Order/Trade/MatchResult 数据结构
//! + OrderBook 结构与构造校验（best_bid/best_ask 盘口只读）。place/cancel/撮合在后续 Task 4-7 补齐。

use std::cmp::Reverse;
use std::collections::BTreeMap;

use thiserror::Error;

use crate::money::Money;

/// 买卖方向。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Side {
    /// 买单（愿意买入）。
    Buy,
    /// 卖单（愿意卖出）。
    Sell,
}

/// 订单 id（单调自增）。本模块自带 newtype，不依赖未来 account 模块。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct OrderId(pub u64);

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
        /// 失败原因（如 "negative" / "not a multiple of tick"）。
        reason: String,
    },
    /// 数量非法：qty == 0。
    #[error("invalid qty: {0} (must be > 0)")]
    InvalidQty(u32),
    /// 重复订单 id（防御式，自增分配器正常时不应触发）。
    #[error("duplicate order id: {0:?}")]
    DuplicateOrderId(OrderId),
    /// 撤单时 id 不存在。
    #[error("order not found: {0:?}")]
    OrderNotFound(OrderId),
    /// 构造订单簿时 tick 非法：<= 0（价格最小变动必须为正）。
    #[error("invalid tick: {tick:?} (must be > 0)")]
    InvalidTick {
        /// 被拒的 tick 值。
        tick: Money,
    },
}

/// 账户 id 占位 newtype（待 account 模块统一；本模块不依赖 account）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct AccountId(pub u64);

/// 单笔限价挂单（撮合发生时为不可变快照；簿内以 OrderId/seq 引用）。
///
/// 价格全程定点 [`Money`]（分），绝不存 f64（money 模块铁律）。可序列化供存档/快照。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Order {
    /// 订单唯一 id。
    pub id: OrderId,
    /// 买/卖方向。
    pub side: Side,
    /// 限价（愿意成交的价格）。
    pub price: Money,
    /// 剩余数量（股）。
    pub qty: u32,
    /// 挂单所属账户。
    pub owner: AccountId,
    /// 时间序：同价位排序键（先挂先成交，price-time priority）。
    pub seq: u64,
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
///
/// `#[allow(dead_code)]`：`next_seq`/`next_id`/`tick` 在 Task 3 尚未被读取（best_bid/best_ask
/// 只用 bids/asks），将由 Task 4-7 的 place/cancel 消费；此处显式标注，避免中间态触发
/// clippy `-D warnings`（plan Task 7 clippy 门要求零告警）。
#[allow(dead_code)]
#[derive(Debug)]
pub struct OrderBook {
    /// 买盘：key=(Reverse(price), seq)，value=Order。
    bids: BTreeMap<(Reverse<Money>, u64), Order>,
    /// 卖盘：key=(price, seq)，value=Order。
    asks: BTreeMap<(Money, u64), Order>,
    /// 下一个分配的时间序（同价位 FIFO 排序键）。
    next_seq: u64,
    /// 下一个分配的订单 id。
    next_id: u64,
    /// 价格最小变动单位（必须 > 0）。
    tick: Money,
}

impl OrderBook {
    /// 构造订单簿。tick 必须 > 0（价格最小变动为正才有意义）；否则返回 [`OrderError::InvalidTick`]。
    ///
    /// 防御式（铁律二）：tick 非法时显式 `Err`，绝不静默 fallback 到某默认值。
    pub fn new(tick: Money) -> Result<OrderBook, OrderError> {
        if tick.cents() <= 0 {
            return Err(OrderError::InvalidTick { tick });
        }
        Ok(OrderBook {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            next_seq: 0,
            next_id: 0,
            tick,
        })
    }

    /// 买盘最优价（最高买价）。空簿返回 None。
    ///
    /// 买盘 key 为 `(Reverse(price), seq)`，`first_key_value` 取价最高（Reverse 反转后最小）者。
    pub fn best_bid(&self) -> Option<Money> {
        self.bids.first_key_value().map(|((Reverse(p), _), _)| *p)
    }

    /// 卖盘最优价（最低卖价）。空簿返回 None。
    ///
    /// 卖盘 key 为 `(price, seq)`，`first_key_value` 取价最低者。
    pub fn best_ask(&self) -> Option<Money> {
        self.asks.first_key_value().map(|((p, _), _)| *p)
    }

    /// 撮合新单。先校验数量/价格，再与对手盘逐档撮合；剩余挂入己方簿。
    ///
    /// 本任务（Task 4）只实现「校验 + 无对手盘时直接挂入」（不撮合），为 Task 5 撮合铺路。
    ///
    /// 防御式（铁律二）：非法数量/价格 → 显式 `Err`，绝不静默截断/修正：
    /// - `qty == 0` → [`OrderError::InvalidQty`]。
    /// - `price < 0` 或非 tick 整数倍 → [`OrderError::InvalidPrice`]（reason 区分 negative /
    ///   not a multiple of tick）。价格全程整数分取模，无 f64（money 模块铁律）。
    pub fn place(&mut self, mut order: Order) -> Result<MatchResult, OrderError> {
        // 校验数量：必须 > 0（0 股无意义）。
        if order.qty == 0 {
            return Err(OrderError::InvalidQty(order.qty));
        }
        // 校验价格：非负 + tick 整数倍。价格用整数分取模，无 f64。
        if order.price.cents() < 0 || order.price.cents() % self.tick.cents() != 0 {
            return Err(OrderError::InvalidPrice {
                price: order.price,
                tick: self.tick,
                reason: if order.price.cents() < 0 {
                    "negative".to_string()
                } else {
                    "not a multiple of tick".to_string()
                },
            });
        }

        // 与对手盘逐档撮合，直到无交叉或新单 qty 耗尽。
        // 成交价恒取被动方(maker)价；maker 清零则移出簿，否则原档扣减。
        let mut trades: Vec<Trade> = Vec::new();
        let taker = order.owner;
        let side = order.side;

        loop {
            if order.qty == 0 {
                break;
            }
            // 是否交叉？买单：买价 >= 最优卖价；卖单：卖价 <= 最优买价（统一为「新单价 >= 对手最优价」）。
            let crossed = match side {
                Side::Buy => self
                    .asks
                    .first_key_value()
                    .map(|((ask_p, _), _)| order.price >= *ask_p)
                    .unwrap_or(false),
                Side::Sell => self
                    .bids
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
                    let entry = self.asks.first_entry().expect("crossed => non-empty");
                    let m = entry.get().clone();
                    let p = m.price;
                    (m, p)
                }
                Side::Sell => {
                    let entry = self.bids.first_entry().expect("crossed => non-empty");
                    let m = entry.get().clone();
                    let p = m.price;
                    (m, p)
                }
            };

            let fill_qty = order.qty.min(maker.qty);
            order.qty -= fill_qty;

            // 更新对手档：maker 清零则 pop_first，否则按 (price,seq) 定位原档扣减。
            match side {
                Side::Buy => {
                    if maker.qty == fill_qty {
                        self.asks.pop_first();
                    } else {
                        let key = (maker.price, maker.seq);
                        if let Some(m) = self.asks.get_mut(&key) {
                            m.qty -= fill_qty;
                        }
                    }
                }
                Side::Sell => {
                    if maker.qty == fill_qty {
                        self.bids.pop_first();
                    } else {
                        let key = (Reverse(maker.price), maker.seq);
                        if let Some(m) = self.bids.get_mut(&key) {
                            m.qty -= fill_qty;
                        }
                    }
                }
            }

            trades.push(Trade {
                price: fill_price,
                qty: fill_qty,
                maker: maker.owner,
                taker,
            });
        }

        // 剩余量 > 0 才分配时间序挂入己方簿；否则 resting=None（全成交）。
        let resting = if order.qty > 0 {
            let seq = self.next_seq;
            self.next_seq += 1;
            order.seq = seq;
            self.insert_resting(order.clone());
            Some(order)
        } else {
            None
        };

        Ok(MatchResult { trades, resting })
    }

    /// 将残留订单挂入对应簿（内部辅助，不校验——调用方 place 已校验）。
    ///
    /// 买盘 key = `(Reverse(price), seq)`（价高优先、同价先挂优先）；
    /// 卖盘 key = `(price, seq)`（价低优先、同价先挂优先）。
    fn insert_resting(&mut self, order: Order) {
        match order.side {
            Side::Buy => {
                self.bids.insert((Reverse(order.price), order.seq), order);
            }
            Side::Sell => {
                self.asks.insert((order.price, order.seq), order);
            }
        }
    }

    /// 按 id 撤单，返回被撤订单（供上层回写状态）。id 不存在 → [`OrderError::OrderNotFound`]。
    ///
    /// 防御式（铁律二）：找不到时显式 `Err`，绝不静默返回 `Option::None` 或伪造空订单。
    ///
    /// 簿内以 `(排序键, Order)` 存放且 id 不在排序键里，故需先 `iter().find` 定位键再 `remove`：
    /// 先查买盘、再查卖盘；命中即移除并返回该 Order 快照（含原价/数量/seq，与挂入时一致）。
    /// `expect` 仅在「键刚被 find 命中却在 remove 前消失」时触发——单线程同步路径下不可达，
    /// 若触发即并发/逻辑 bug，应立即暴露而非吞错（铁律三）。
    pub fn cancel(&mut self, id: OrderId) -> Result<Order, OrderError> {
        // 买盘：先找到对应键，再据键移除。
        if let Some(key) = self
            .bids
            .iter()
            .find(|(_, o)| o.id == id)
            .map(|(k, _)| *k)
        {
            return Ok(self
                .bids
                .remove(&key)
                .expect("key just located by find; 单线程同步下 remove 必命中"));
        }
        // 卖盘：同上。
        if let Some(key) = self
            .asks
            .iter()
            .find(|(_, o)| o.id == id)
            .map(|(k, _)| *k)
        {
            return Ok(self
                .asks
                .remove(&key)
                .expect("key just located by find; 单线程同步下 remove 必命中"));
        }
        Err(OrderError::OrderNotFound(id))
    }

    /// 买盘深度：按价高→低，每个价位聚合所有挂单的总数量。
    ///
    /// 返回 `Vec<(Money, u32)>`：元素为 (价位, 该价位累计股数)。空簿返回空 Vec。
    /// 价序天然由 BTreeMap 给出——买盘 key 含 `Reverse(price)`，`values()` 遍历即「价高优先、
    /// 同价先挂优先」，故只需把相邻同价累加。
    pub fn bid_depth(&self) -> Vec<(Money, u32)> {
        self.aggregate(&self.bids)
    }

    /// 卖盘深度：按价低→高，每个价位聚合所有挂单的总数量。
    ///
    /// 返回 `Vec<(Money, u32)>`：元素为 (价位, 该价位累计股数)。空簿返回空 Vec。
    /// 价序天然由 BTreeMap 给出——卖盘 key 为 `(price, seq)`，`values()` 遍历即「价低优先、
    /// 同价先挂优先」，故只需把相邻同价累加。
    pub fn ask_depth(&self) -> Vec<(Money, u32)> {
        self.aggregate(&self.asks)
    }

    /// 将一个盘口的挂单按相邻同价聚合为 (价位, 累计量) 序列。
    ///
    /// 通用泛型 `T: Ord`：买盘 `T = Reverse<Money>`、卖盘 `T = Money`，两者排序键的第一分量类型不同，
    /// 但聚合只关心 `Order.price`（值类型恒为 `Money`），与 `T` 无关——故无需 `_is_bid` 之类的方向参数。
    /// 遍历顺序已由 BTreeMap 的 key 保证为「价优→劣」，相邻同价即合并。
    ///
    /// 数量累加用 `u32` + `+=`：两笔同价挂单之和在游戏尺度下不会溢出 u32（理论上限 ~42 亿股，
    /// 远超合理盘口）；若未来需更强防御可换 checked_add，当前与 Order.qty 同尺度即可。
    fn aggregate<T: Ord>(&self, side: &BTreeMap<(T, u64), Order>) -> Vec<(Money, u32)> {
        let mut out: Vec<(Money, u32)> = Vec::new();
        for o in side.values() {
            // 上一档同价 → 累加到该档；否则新开一档。
            if let Some(last) = out.last_mut() {
                if last.0 == o.price {
                    last.1 += o.qty;
                    continue;
                }
            }
            out.push((o.price, o.qty));
        }
        out
    }
}
