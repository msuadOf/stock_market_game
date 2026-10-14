//! 组线程 → 交易部线程的操作（`DeskIn`）与交易部 → 组线程的实体批次（`DeskBatch`）。
//!
//! 时序不变量（正确性基石，继承原型 `oworker.rs:16-19`）：交易部在 `DeskDone` 之前
//! flush 全部实体批次，协调者收齐全部 `DeskDone` 之后才广播下一 `Start` ⇒ 组线程
//! 收件箱中 tick N 的全部实体先于 `Start(N+1)` 入队（mpsc FIFO + happens-before）。
//!
//! 同一批次（及同一 channel 先后批次）FIFO：同 NPC 的 `CancelAll`/`Cancel` 恒先于其
//! `Place`（对齐撤单语义）。

use super::{DeskReceipt, Entity, FillFact, OrderRef, Owner, StockIdx};
use crate::orderbook::Side;

/// 价格符号：组端看不到目标股活簿，符号在**交易部受理时**解析。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutedPrice {
    /// 限价（i64 分）。
    Fixed(i64),
    /// 本方最高可接受价（买单=涨停保护上限档；按 side 在交易部解析）。
    Highest,
    /// 本方最低可接受价（卖单=跌停保护下限档；按 side 在交易部解析）。
    Lowest,
    /// 市价=涨跌停保护逐档即成；未成交余量不进簿（原路退回）。
    Market,
}

/// 组线程 → 交易部线程的操作。资源在组端已实体化扣出，所有权随消息转移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeskIn {
    /// 下单。买单 `purse` = 现金实体（最坏情况对价 + 佣金上限 + 过户费上限，已从组
    /// 账户实际扣除）；卖单 `purse` = 0、`qty` 股已从持仓实际扣除。
    Place {
        /// 下单账户身份。
        owner: Owner,
        /// 买卖方向。
        side: Side,
        /// 价格符号（交易部受理时解析为具体分价）。
        price: RoutedPrice,
        /// 申报数量（买入须整手；卖出零股须一次性——组端校验、交易部防御复核）。
        qty: u32,
        /// 随单现金实体（i64 分）。卖单恒为 0。
        purse: i64,
    },
    /// 对齐撤单：撤该 owner 在本交易部的全部挂单（实体原路回流）。
    CancelAll {
        /// 撤单账户身份。
        owner: Owner,
    },
    /// 定向撤单（引擎语义：部分成交撤余量；已全成回显式业务失败回执）。
    Cancel {
        /// 撤单账户身份。
        owner: Owner,
        /// 目标挂单复合标识（StockIdx 前缀必须指向本交易部）。
        order: OrderRef,
    },
}

/// 交易部 → 组线程的一个上行批次：资源实体 + 事实流（成交/回执）。
/// 每交易部每 tick 每组至多一个非空批次（聚合 Vec，不逐条发消息）。
#[derive(Clone, Debug, Default)]
pub struct DeskBatch {
    /// 发方交易部的股索引（组端 primary/secondary 判定与在途登记用）。
    pub stock: StockIdx,
    /// 防御字段：产出该批次的 tick。组端核对属当前 tick，错配=显式失败。
    pub tick: u64,
    /// 防御字段：产出该批次的续行轮（普通 tick 首轮为 0）。错配=显式失败。
    pub round: u32,
    /// 资源实体（所有权转移）。
    pub entities: Vec<Entity>,
    /// 成交事实（信息回流）。
    pub fills: Vec<FillFact>,
    /// 受理/撤单回执（信息回流）。
    pub receipts: Vec<DeskReceipt>,
}

/// `DeskBatch` 防御字段错配（显式失败载体，不静默落账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DeskBatchMismatch {
    /// 批次来自非目标交易部（stock 错配）。
    #[error("DeskBatch.stock 错配：expected={expected}, actual={actual}（批次来自非目标交易部）")]
    Stock {
        /// 期望的股索引。
        expected: StockIdx,
        /// 批次实际携带的股索引。
        actual: StockIdx,
    },
    /// 迟到/超前批次（tick 错配）。
    #[error(
        "DeskBatch.tick 错配：expected={expected}, actual={actual}（迟到/超前批次，不静默落账）"
    )]
    Tick {
        /// 期望的当前 tick。
        expected: u64,
        /// 批次实际携带的 tick。
        actual: u64,
    },
    /// 非当前续行轮（round 错配）。
    #[error("DeskBatch.round 错配：expected={expected}, actual={actual}（非当前续行轮）")]
    Round {
        /// 期望的当前轮次。
        expected: u32,
        /// 批次实际携带的轮次。
        actual: u32,
    },
}

impl DeskBatch {
    /// 组端防御核对：批次必须属于当前 `(stock, tick, round)`，任一错配即显式失败。
    /// 供组线程在落账 staging 前调用；返回 `Ok(())` 不代表实体语义已校验（那属于
    /// 组端落账与期末审计）。
    pub fn verify(&self, stock: StockIdx, tick: u64, round: u32) -> Result<(), DeskBatchMismatch> {
        if self.stock != stock {
            return Err(DeskBatchMismatch::Stock {
                expected: stock,
                actual: self.stock,
            });
        }
        if self.tick != tick {
            return Err(DeskBatchMismatch::Tick {
                expected: tick,
                actual: self.tick,
            });
        }
        if self.round != round {
            return Err(DeskBatchMismatch::Round {
                expected: round,
                actual: self.round,
            });
        }
        Ok(())
    }
}
