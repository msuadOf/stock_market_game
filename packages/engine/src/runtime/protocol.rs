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

// ═══════════════════════════════════════════════════════════════════
// 批 2：线程拓扑消息面（协调者 / 组线程 / 交易部线程）
// ═══════════════════════════════════════════════════════════════════

use super::GroupIdx;
use std::time::Duration;

/// worker 身份（错误上报与 Final 对账用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkerId {
    /// NPC 组线程。
    Group(GroupIdx),
    /// 交易部线程。
    Desk(StockIdx),
}

impl std::fmt::Display for WorkerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkerId::Group(g) => write!(f, "group-{g}"),
            WorkerId::Desk(s) => write!(f, "desk-{s}"),
        }
    }
}

/// 单股上一收束点公共态（协调者广播快照元素；字段对齐 `strategy::StockView`）。
#[derive(Clone, Debug, PartialEq)]
pub struct StockPublic {
    /// 最新价（分）。
    pub last_price: i64,
    /// 买一（分；空簿 None）。
    pub best_bid: Option<i64>,
    /// 卖一（分；空簿 None）。
    pub best_ask: Option<i64>,
    /// 本观察版本允许的最高买入限价（涨停 ∧ 笼子交集）。
    pub max_buy_price: i64,
    /// 本交易日的涨停价（符号价 purse 最坏情况基准）。
    pub daily_upper_limit: i64,
    /// 本观察版本允许的最低卖出限价。
    pub min_sell_price: i64,
    /// 最近 N 个 tick 收盘价（分）。
    pub recent_prices: Vec<i64>,
    /// 当日已完成标准交易分钟收盘（分）。
    pub minute_closes: Vec<i64>,
    /// 日累计成交量（股）。
    pub volume_today: u64,
    /// 相对历史同期预期量倍率（骨架 1.0，预期量基线随批 5 接入）。
    pub relative_volume: f64,
    /// 前五档买卖失衡 [-1,1]。
    pub order_book_imbalance: f64,
}

/// tick 广播快照（批 4：附带全股上一收束点公共态，组端只读）。
#[derive(Clone, Debug, PartialEq)]
pub struct TickSnapshot {
    /// 当前 tick。
    pub tick: u64,
    /// 市场分钟数。
    pub market_minute: u32,
    /// 全股公共态（下标=StockIdx）。
    pub stocks: Vec<StockPublic>,
}

/// 组线程收件消息（coordinator 的控制消息与交易部的实体批次共用一个 mpsc；
/// FIFO + 收束因果序保证批次先于下一 Start 被处理）。
pub enum GroupIn {
    /// tick 开始：隐式转正上一 tick（落账 staging 后丢弃可逆日志）+ 本轮决策。
    Start {
        /// tick 编号。
        tick: u64,
        /// 广播快照。
        snapshot: std::sync::Arc<TickSnapshot>,
    },
    /// 同 tick 续行轮：组先落账本轮实体（记逆操作，Abort 可冲回）再续行决策。
    Continue {
        /// tick 编号。
        tick: u64,
        /// 轮次。
        round: u32,
    },
    /// 交易部实体批次（进 staging，Start/Continue 时落账）。
    Batch(DeskBatch),
    /// 撤销 working overlay，回到上一收敛态（可逆日志逆序回放 + 清 staging）。
    Abort {
        /// 被撤销的 tick。
        tick: u64,
    },
    /// 关停：上交 `GroupFinal` 后退出。
    Shutdown,
}

/// 交易部收件消息。
pub enum DeskMsg {
    /// 一组本轮的一批操作。`final_flush` = 该组本轮最后一批（可空——提交完成标记）；
    /// 交易部收齐全部组的 final_flush 即本轮撮合输入完毕（先 flush 实体批次再上报）。
    Ops {
        /// tick 编号。
        tick: u64,
        /// 轮次。
        round: u32,
        /// 发方组索引。
        group: GroupIdx,
        /// 操作批次。
        ops: Vec<DeskIn>,
        /// 本组本轮提交标记。
        final_flush: bool,
    },
    /// 撤销 working overlay（撤销日志逆序回放 + 清实体出口缓冲）。
    Abort {
        /// 被撤销的 tick。
        tick: u64,
    },
    /// 日终清簿：强撤全部挂单、实体回流、上交清簿后状态。
    Drain,
    /// 关停：上交 `DeskFinal` 后退出。
    Shutdown,
}

/// worker → 协调者的上行消息（全部 worker 共用一个 mpsc）。
/// 手写 Debug：panic payload 不透明，只报告变体名与身份。
pub enum UpMsg {
    /// 组线程本轮报告（每轮一条）。
    GroupReport(GroupReport),
    /// 交易部本轮报告（每轮一条，实体批次已先于此发出）。
    DeskReport(DeskReport),
    /// 日终清簿完成。
    DeskDrained {
        /// 完成清簿的交易部。
        stock: StockIdx,
    },
    /// 组线程期末上交。
    GroupFinal(Box<super::group::GroupFinalState>),
    /// 交易部期末上交。
    DeskFinal(Box<super::desk::DeskFinalState>),
    /// worker 显式失败上抛（铁律二：不静默；尽力发送，通道已断由看门狗兜底）。
    Failed {
        /// 失败 worker。
        who: WorkerId,
        /// 失败详情。
        error: RuntimeError,
    },
    /// worker panic 运输：payload 原样上抛，协调者收齐 Final 后 `resume_unwind`。
    TaskDied {
        /// panic 的 worker（已回滚自身 overlay 并上交 Final 后退出）。
        who: WorkerId,
        /// 原 panic payload（不吞、不改写）。
        payload: Box<dyn std::any::Any + Send>,
    },
}

impl std::fmt::Debug for UpMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpMsg::GroupReport(r) => write!(
                f,
                "GroupReport(group={}, tick={}, round={})",
                r.group, r.tick, r.round
            ),
            UpMsg::DeskReport(r) => write!(
                f,
                "DeskReport(stock={}, tick={}, round={})",
                r.stock, r.tick, r.round
            ),
            UpMsg::DeskDrained { stock } => write!(f, "DeskDrained(stock={stock})"),
            UpMsg::GroupFinal(x) => write!(f, "GroupFinal(group={})", x.group),
            UpMsg::DeskFinal(x) => write!(f, "DeskFinal(stock={})", x.stock),
            UpMsg::Failed { who, .. } => write!(f, "Failed({who}, ..)"),
            UpMsg::TaskDied { who, .. } => write!(f, "TaskDied({who}, ..)"),
        }
    }
}

/// 组线程本轮报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupReport {
    /// 组索引。
    pub group: GroupIdx,
    /// tick 编号。
    pub tick: u64,
    /// 轮次。
    pub round: u32,
    /// 本轮发出的操作数（含对齐撤单）。
    pub ops_sent: u64,
    /// 严格所有权下被跳过的单量（资金/可卖不足，显式计数不静默）。
    pub skipped: u64,
    /// 本组是否还有续行需求（计划续行；协调者据此决定 Continue）。
    pub continuation: bool,
    /// 本轮到期 NPC 数（attention 简化 due 检查）。
    pub due_npcs: u64,
    /// 本轮实际调用 decide_data 次数。
    pub decide_calls: u64,
    /// 被登记跳过的 Cancel 意图数（OrderRef 跟踪随批 6 接线，不静默）。
    pub cancel_intents_skipped: u64,
    /// 提交态哈希（Start 转正后立即计算；回滚后 Final 哈希应与之相等）。
    pub committed_hash: u64,
}

/// 交易部本轮报告。
#[derive(Clone, Debug, PartialEq)]
pub struct DeskReport {
    /// 股索引。
    pub stock: StockIdx,
    /// tick 编号。
    pub tick: u64,
    /// 轮次。
    pub round: u32,
    /// 挂单数（本收束点）。
    pub resting_count: u64,
    /// 本轮实体流出条数。
    pub entities_out: u64,
    /// 本轮回执数。
    pub receipts_out: u64,
    /// 提交态哈希（隐式转正后立即计算；回滚后 Final 哈希应与之相等）。
    pub committed_hash: u64,
    /// 本轮成交事实数（双边计数）。
    pub fills_out: u64,
    /// 最新价（分）。
    pub last_price: i64,
    /// 日内最高（分）。
    pub day_high: i64,
    /// 日内最低（分）。
    pub day_low: i64,
    /// 日累计成交量（股）。
    pub volume_today: u64,
    /// 日累计成交笔数。
    pub trade_count: u64,
    /// 费用桶累计（双边实收费用，守恒审计输入）。
    pub fee_sink: i64,
    /// 本收束点公共态（协调者安装进下一 tick 广播快照）。
    pub view: StockPublic,
}

/// 运行时显式失败（不静默吞错）。panic 走 `TaskDied` 运输，不进本枚举。
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum RuntimeError {
    /// 看门狗超时（携带相位上下文——相位感知，不静默放大全局阈值）。
    #[error("看门狗超时：phase={phase:?}, waited={waited:?}：{detail}")]
    Watchdog {
        /// 超时相位。
        phase: super::watchdog::Phase,
        /// 等待时长。
        waited: Duration,
        /// 上下文详情。
        detail: String,
    },
    /// 通道断连（worker 过早退出或收束期收到非期望消息）。
    #[error("通道断连：who={who:?}, stage={stage}")]
    Disconnected {
        /// 断连关联 worker（None=无法定位）。
        who: Option<WorkerId>,
        /// 发生阶段。
        stage: String,
    },
    /// worker 显式失败上抛。
    #[error("worker {who} 显式失败：{error}")]
    WorkerFailed {
        /// 失败 worker。
        who: WorkerId,
        /// 失败详情。
        error: Box<RuntimeError>,
    },
    /// tick 收束前校验失败（PreCommit 注入点）→ Abort。
    #[error("tick {tick} 第 {round} 轮校验失败：{detail}")]
    TickValidation {
        /// 失败 tick。
        tick: u64,
        /// 失败轮次。
        round: u32,
        /// 失败详情。
        detail: String,
    },
    /// 同 tick 续行轮数超上限（计划链异常形状，显式登记不静默）。
    #[error("tick {tick} 续行轮数超上限 {rounds_cap}（计划链异常形状）")]
    RoundsExceeded {
        /// 失败 tick。
        tick: u64,
        /// 轮数上限。
        rounds_cap: u32,
    },
    /// 消息发送失败（对端已退出）。
    #[error("发送失败：{detail}")]
    SendFailed {
        /// 失败详情。
        detail: String,
    },
    /// 协议不变量破坏（tick/轮次错序、防御字段错配等结构性 bug）。
    #[error("协议不变量破坏：{detail}")]
    ProtocolInvariant {
        /// 破坏详情。
        detail: String,
    },
}

impl DeskBatch {
    /// 组端防御核对（组无法预知发方交易部，只核对 tick/轮归属）：批次必须属于
    /// `(tick, round)`，错配即显式失败。
    pub fn verify_tick_round(&self, tick: u64, round: u32) -> Result<(), DeskBatchMismatch> {
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
