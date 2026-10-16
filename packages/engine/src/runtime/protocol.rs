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

use super::{GroupIdx, LocalIdx};
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
    /// 已完成标准交易分钟收盘（绝对分钟, 分）——P1-6 起带绝对键。
    pub minute_closes: Vec<(u64, i64)>,
    /// D3a-2 观察基：已完成日 K（**不过滤**——行为路径口径同
    /// retained_behavior_daily_closes_history；尾部 250 窗）。不含活动 K。
    pub daily_bars: Vec<(i64, i64, i64, i64, u64)>,
    /// 当日活动 K（存在即有；volume>0 时供 current_day_open/技术面样本）。
    pub daily_active: Option<(i64, i64, i64, i64, u64)>,
    /// 已完成日总数（绝对索引基——trading_day = total−kept+offset）。
    pub daily_total_completed: u64,
    /// 历史累计 traded 日数（volume>0——技术面 valid_sample_count 口径）。
    pub daily_total_traded: u64,
    /// 日累计成交量（股）。
    pub volume_today: u64,
    /// 相对历史同期预期量倍率（骨架 1.0，预期量基线随批 5 接入）。
    pub relative_volume: f64,
    /// 前五档买卖失衡 [-1,1]。
    pub order_book_imbalance: f64,
    /// 买盘深度前五档（价高→低，每价聚合总量；TickFrame MarketSnap 映射）。
    pub depth_bids: Vec<(i64, u64)>,
    /// 卖盘深度前五档（价低→高）。
    pub depth_asks: Vec<(i64, u64)>,
}

/// tick 广播快照（批 4：附带全股上一收束点公共态，组端只读；批 8b 起
/// stocks 经 Arc 共享——广播免逐组 clone）。
#[derive(Clone, Debug, PartialEq)]
pub struct TickSnapshot {
    /// 当前 tick。
    pub tick: u64,
    /// 市场分钟数。
    pub market_minute: u32,
    /// 当前自然日（D3a-2c 件2：组线程组装 RootReadContext 的时间面——
    /// 自然日与交易日计数分离，不能从 tick 推导，由协调者自会话同步）。
    pub civil_date: crate::calendar::CivilDate,
    /// 全股公共态（下标=StockIdx）。
    pub stocks: std::sync::Arc<Vec<StockPublic>>,
}

/// 公司域权益（日结 AdminBatch 条目；组端按可逆日志落账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdminRight {
    /// 现金股利入账（除权除息后；单位分）。
    CashDividend {
        /// 组内名册下标。
        local: LocalIdx,
        /// 来源股（诊断面）。
        stock: StockIdx,
        /// 派息金额（分，正）。
        amount: i64,
    },
    /// 送转股到账（可卖——A 股送转股上市日可卖，不入 T+1 冻结；登记简化）。
    ShareSplit {
        /// 组内名册下标。
        local: LocalIdx,
        /// 来源股。
        stock: StockIdx,
        /// 送转股数（正）。
        bonus_qty: u32,
    },
    /// 权益扣税（现金扣减，如送转个税差别化扣缴）。
    WithholdingTax {
        /// 组内名册下标。
        local: LocalIdx,
        /// 来源股。
        stock: StockIdx,
        /// 扣税额（分，正）。
        amount: i64,
    },
}

/// 日结指令（F §6.2 步骤 2/3）：组端 Drain 回流落账 → T+1 解冻 → 权益落账
/// （全部记逆操作，日结失败可整体冲回）→ 回执 GroupDayDone。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DayEndDirective {
    /// 日终 tick（本交易日最后一 tick）。
    pub tick: u64,
    /// 交易日序号。
    pub day: u32,
    /// 本组权益批次（公司域事务产出）。
    pub rights: Vec<AdminRight>,
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
    /// 日结相位（F §6.2）：Drain 回流落账 → T+1 解冻 → AdminBatch 权益落账
    /// （全部进可逆日志）→ GroupDayDone 回执。
    DayEnd(Box<DayEndDirective>),
    /// 关停：上交 `GroupFinal` 后退出。
    Shutdown,
    /// 外部计划注入（翻转批玩家意图路由：协调者/驱动器向组注入本 tick
    /// 计划——消费一次即清）。
    ExternalPlan(Vec<super::decision::PendingOp>),
    /// 观察基读面（D3a-1：price_paths 上行——计划链端口径/测试面）。
    EchoObservations,
    /// 信念个人态读面（D3a-2b：构造投影同源校验/后续驱动面）。
    EchoBelief,
    /// 信息投递（Belief-1b·下：稀疏——仅披露 tick 下行。携带库 Arc
    /// 刷新+per-NPC 注意力**镜像快照**（candidate 域=组权威值，见
    /// 设计注记）+issuer/spec 快照+时刻。组内纯函数推进，上行仅
    /// information 域回写）。
    InformationDelivery {
        /// 观察时刻（披露相位）。
        now: crate::calendar::CivilInstant,
        /// 交易日。
        day: u64,
        /// 库刷新（Arc 共享——稀疏）。
        library: std::sync::Arc<crate::information::PublicLibrary>,
        /// per-NPC（组内下标, AccountId, 注意力镜像快照——AccountId
        /// 供纯函数的获知登记/错误上下文；id_map 在驱动器侧）。
        attentions: Vec<(
            LocalIdx,
            crate::AccountId,
            crate::session::NpcAttentionState,
        )>,
        /// 股→发行人。
        issuer_map: std::collections::BTreeMap<crate::StockId, crate::company::CompanyId>,
        /// 发行人→(spec.kind, issued_shares)。
        company_specs: std::collections::BTreeMap<
            crate::company::CompanyId,
            (crate::company::CompanyKind, u64),
        >,
    },
    /// 计划簿读面（D3a-2c 件5：组内 PlanBook 上行——plans_debug 投影源）。
    EchoPlans,
    /// 受理事实下行（Sol 整改注入通道配套：desk 的 Accepted lifecycle
    /// 经协调者转发组——受理即身份（OrderRef 合成），不等首笔 fill；
    /// 生产通道非测试面）。
    ChildAccepted {
        /// 股索引。
        stock: StockIdx,
        /// 挂单所有者。
        owner: Owner,
        /// desk 本地序号（OrderRef.1——身份合成输入）。
        order_serial: u64,
    },
    /// D3b 件 5：个人态注入通道（cfg(test)）——restructure fixture 的
    /// 组内等价（session 版 personal.install 的下行面；组权威个人态
    /// 覆盖+Personal undo 可逆，Abort 冲回后需重发）。
    #[cfg(test)]
    InstallPersonal {
        /// 组内名册下标。
        local: super::LocalIdx,
        /// 完整个人态快照（六件）。
        snapshot: super::world::BeliefParticipantSnapshot,
    },
    /// D3b 复审注入通道（cfg(test)）：直接向组簿施加计划事件
    ///（Paused/Revised/Terminated 等——restructure fixture 的组内
    /// 等价；PlanEvent 语义由 plan_book.apply 权威承载，可逆日志覆盖）。
    #[cfg(test)]
    ApplyPlanEvents {
        /// (计划 id, 事件) 序列——逐条 apply，失败显式上报。
        events: Vec<(crate::plans::PlanId, crate::plans::PlanEvent)>,
    },
    /// D5 #4 fixture 通道（cfg(test)）：显式推远 next_due（组权威调度面
    /// 的测试控制——"无 due tick"形态构造；效果入可逆日志，Abort 冲回
    /// 后指令失效需重发）。
    #[cfg(test)]
    DeferDueForTest {
        /// 组内名册下标。
        locals: Vec<super::LocalIdx>,
        /// 目标 tick（next_due=max(现值, 目标)——只推远不提前）。
        tick: u64,
    },
    /// 测试控制面（Sol 中点整改注入通道——cfg(test) 语义）：显式强制
    /// 到期指令（账户位+目标 tick；组线程验证目标晚于已转正 tick 后
    /// 前移 next_due——不每 tick 偷用 session 镜像；效果入可逆日志，
    /// Abort 冲回后指令失效需重发）。
    #[cfg(test)]
    ForceDue {
        /// 组内名册下标。
        locals: Vec<super::LocalIdx>,
        /// 目标 tick（组验证 > 已转正 tick）。
        tick: u64,
        /// D5 #2：强制观察（绕 observe 门仍消费 rng 续流+wait 采样——
        /// fixture 的"到期即决策"语义；生产 force 无此面）。
        ensure_observe: bool,
    },
    /// D3b 件 4 fixture 通道（cfg(test)）：复审候选注入——chain_
    /// restructure 测试的 drive_once(人工 assessments) 组内等价
    ///（session 版 collect_plan_lifecycle_actions 直调的输入面；组内
    /// 复审块+开仓循环用注入值替代 observe 产物，一次消费即清）。
    #[cfg(test)]
    InjectReviewAssessments {
        /// 目标账户（组侧经投影名册映射位）。
        account: crate::AccountId,
        /// 注入候选（与 observe 的 lifecycle_assessments 同构）。
        assessments: std::collections::BTreeMap<crate::StockId, crate::plans::CandidateAssessment>,
    },
    /// D3b 件 4 fixture 通道（cfg(test))：组簿测试变换——session 侧
    /// restore_child_remaining_qty/record_child_fill 等手工簿推进的组内
    /// 等价（镜像簿写入组权威不感知；变换经簿快照 undo 可逆）。
    #[cfg(test)]
    MutatePlanBookForTest(Box<dyn FnOnce(&mut crate::plans::PlanBook) + Send>),
    /// D3b 件 4 fixture 通道（cfg(test)）：观察基分钟序列注入——
    /// set_urgency_minutes 的组内等价（组观察基=observation_inputs，
    /// session 侧分钟缓存组内不感知；直改 session tick 会与世界权威
    /// 锁步漂移）。
    #[cfg(test)]
    InjectObservationMinutes {
        /// 股下标。
        stock: u16,
        /// 分钟序列（绝对分钟, 收盘分）。
        minutes: Vec<(u64, i64)>,
    },
    /// D3b 件 4 fixture 通道（cfg(test)）：belief 簿测试变换——session
    /// 侧 observe_equity 等手工 belief 面直改的组内等价（account→local
    /// 组侧经投影名册映射）。
    #[cfg(test)]
    MutateBeliefForTest {
        /// 目标账户。
        account: crate::AccountId,
        /// belief 簿变换（experience/policy 等面的闭包直施）。
        mutate: Box<dyn FnOnce(&mut crate::strategy::BeliefBook) + Send>,
    },
    /// 开仓派生读面（D3a-2c：组线程组装 RootReadContext+DecisionChain
    /// Observation 直调 observe——红证/读面上行；生产驱动接线随件4）。
    EchoOpenDerivation,
    /// 账户快照请求（驱动器纵切/测试面；生产热路径不开）。
    EchoAttention,
    EchoAccounts,
    /// K 线投影请求（驱动器回写 candle_book；轻量）。
    EchoCandles,
    /// D5 #4 retail 链：本拍零售决策 trace 请求（组回 RetailTraces——
    /// 驱动器投影 session.last_retail_decisions 的瞬态读面）。
    EchoRetailTraces,
}

/// D5 envelope 投影：desk 在途单快照（owner/价格/量/费用累计——
/// session 信封账本重建的输入）。
#[derive(Clone, Debug)]
pub struct RestingFact {
    pub owner: Owner,
    pub side: Side,
    pub price: i64,
    pub qty: u32,
    /// 买腿 purse（卖恒 0）。
    pub purse: i64,
    pub filled_value: i64,
    pub charged_commission: i64,
    pub charged_stamp_tax: i64,
    pub charged_transfer_fee: i64,
    pub order_serial: u64,
}

/// 交易部收件消息。
pub enum DeskMsg {
    /// D5 envelope 投影：在途单快照请求（desk 回 DeskResting）。
    EchoResting,
    /// tick 起点（D4：协调者在组 Start 前广播——交易部每 tick 必推进
    /// begin_tick：到期撤销/开盘清算/行情快照不再依赖"本 tick 恰有 Ops
    /// 到达"，相位计时（ExpiryShadow/StockProcessing）每 tick 有 span）。
    Start {
        /// tick 编号。
        tick: u64,
    },
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
    /// K 线投影请求（驱动器回写 candle_book）。
    EchoCandles,
}

/// worker → 协调者的上行消息（全部 worker 共用一个 mpsc）。
/// 手写 Debug：panic payload 不透明，只报告变体名与身份。
pub enum UpMsg {
    /// D5 envelope 投影：在途单快照回执。
    DeskResting {
        stock: StockIdx,
        orders: Vec<RestingFact>,
    },
    /// 组线程本轮报告（每轮一条）。
    GroupReport(GroupReport),
    /// 交易部本轮报告（每轮一条，实体批次已先于此发出）。
    DeskReport(DeskReport),
    /// 日终清簿完成（fee_sink 日结上交 + 行情快照供候选投影）。
    DeskDrained {
        /// 完成清簿的交易部。
        stock: StockIdx,
        /// 费用桶累计（守恒审计：日结上交）。
        fee_sink: i64,
        /// 日终最新价（昨收基准）。
        last_price: i64,
        /// 清簿撤单事实（OrderCanceled 映射输入）。
        drained_lifecycle: Vec<DeskLifecycleFact>,
    },
    /// 组日结完成（权益落账后的账户投影）。
    GroupDayDone {
        /// 组索引。
        group: GroupIdx,
        /// 交易日序号。
        day: u32,
        /// 落账后账户投影（候选组装输入）。
        accounts: Vec<super::group::GroupAccount>,
        /// 落账后态哈希。
        state_hash: u64,
    },
    /// 组线程期末上交。
    GroupFinal(Box<super::group::GroupFinalState>),
    /// 交易部期末上交。
    DeskFinal(Box<super::desk::DeskFinalState>),
    /// 交易部 K 线投影回执（EchoCandles 应答：历史 K+活动 K）。
    DeskCandles {
        /// 股索引。
        stock: StockIdx,
        /// 已完成日 K（time, O, H, L, C, volume）。
        history: Vec<(i64, i64, i64, i64, i64, u64, i64, u64)>,
        /// 活动 K。
        active: Option<(i64, i64, i64, i64, i64, u64, i64, u64)>,
    },
    /// 投递完成回执（Belief-1b·下：组内纯函数推进后的注意力态——
    /// 驱动器**仅回写 information 域**，不触碰 candidate/heap）。
    GroupDeliveryDone {
        /// 组索引。
        group: GroupIdx,
        /// 逐组内下标的更新后注意力态。
        updates: Vec<(LocalIdx, crate::session::NpcAttentionState)>,
    },
    /// 信念个人态回执（EchoBelief 应答——逐组内下标；None=非机构位）。
    GroupBelief {
        /// 组索引。
        group: GroupIdx,
        /// 逐组内下标的信念个人态快照。
        states: Vec<Option<super::world::BeliefParticipantSnapshot>>,
    },
    /// 计划簿回执（D3a-2c 件5：组内 PlanBook 克隆——交付点投影源）。
    GroupPlans {
        /// 组索引。
        group: GroupIdx,
        /// 组内计划簿。
        plans: crate::plans::PlanBook,
    },
    /// 开仓派生读面回执（D3a-2c：组线程直调 observe 的派生——红证/读面。
    /// market/now=组内实际消费的观察输入（红证同输入锚——广播快照为
    /// 本 tick Start 时刻的上一收束点，与驱动器最新公共态差一个 tick）。
    GroupOpenDerivations {
        /// 组索引。
        group: GroupIdx,
        /// 组内消费的市场视图（广播快照转换）。
        market: crate::strategy::MarketView,
        /// 组内消费的观察时刻。
        now: crate::calendar::CivilInstant,
        /// 每 belief 位的派生记录（观察输入+评估输出+个人态演进前后）。
        derivations: Vec<super::world::OpenDerivationRecord>,
    },
    /// 观察基回执（D3a-2：每股链观察——价格路径+技术面）。
    GroupObservations {
        /// 组索引。
        group: GroupIdx,
        /// 每股 (PricePathObservation, TechnicalObservation)。
        observations: Vec<(
            crate::observation::PricePathObservation,
            crate::observation::TechnicalObservation,
        )>,
    },
    /// 组注意力调度回执（P1-10：逐账户 next_due 上行）。
    GroupAttention {
        /// 组索引。
        group: GroupIdx,
        /// 逐账户（组内下标, next_due tick, 注意力 rng 续传态——D5 #2
        /// 起 evaluate 共享流写回 session rng_state 同源）。
        attention: Vec<(LocalIdx, u64, u64)>,
    },
    /// D5 #4 retail 链：本拍零售决策 trace 回执（瞬态——每拍清+装）。
    RetailTraces {
        /// 组索引。
        group: GroupIdx,
        /// 本拍 trace（账户序）。
        traces: Vec<crate::session::RetailDecisionTrace>,
    },
    /// 组账户快照回执（EchoAccounts 应答）。
    GroupEcho {
        /// 组索引。
        group: GroupIdx,
        /// 当前账户全量快照。
        accounts: Vec<super::group::GroupAccount>,
    },
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
            UpMsg::DeskResting { stock, orders } => {
                write!(f, "DeskResting(stock={}, n={})", stock, orders.len())
            }
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
            UpMsg::DeskDrained { stock, .. } => write!(f, "DeskDrained(stock={stock})"),
            UpMsg::GroupFinal(x) => write!(f, "GroupFinal(group={})", x.group),
            UpMsg::GroupDayDone { group, day, .. } => {
                write!(f, "GroupDayDone(group={group}, day={day})")
            }
            UpMsg::GroupPlans { group, .. } => {
                write!(f, "GroupPlans(group={group})")
            }
            UpMsg::GroupOpenDerivations {
                group, derivations, ..
            } => {
                write!(f, "GroupOpenDerivations(group={group}, n={})", derivations.len())
            }
            UpMsg::DeskFinal(x) => write!(f, "DeskFinal(stock={})", x.stock),
            UpMsg::GroupObservations { group, .. } => {
                write!(f, "GroupObservations(group={group})")
            }
            UpMsg::GroupBelief { group, states } => {
                write!(f, "GroupBelief(group={group}, n={})", states.len())
            }
            UpMsg::GroupDeliveryDone { group, updates } => {
                write!(f, "GroupDeliveryDone(group={group}, n={})", updates.len())
            }
            UpMsg::GroupAttention { group, .. } => write!(f, "GroupAttention(group={group})"),
            UpMsg::RetailTraces { group, .. } => write!(f, "RetailTraces(group={group})"),
            UpMsg::GroupEcho { group, .. } => write!(f, "GroupEcho(group={group})"),
            UpMsg::DeskCandles { stock, .. } => write!(f, "DeskCandles(stock={stock})"),
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

/// 双腿成交事实（批 8b 撮合域接线地基）：单笔成交的 maker/taker 双方身份
/// ——`FillFact` 是单腿视角（计划续行），本事实是事件域（TickFrame
/// `Event::Trade` 需要 maker+taker）与守恒审计的输入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeskTradeFact {
    /// 股索引。
    pub stock: StockIdx,
    /// 成交价（分）。
    pub price: i64,
    /// 成交量（股）。
    pub qty: u32,
    /// 卖腿（maker）所有者（ADR-0042 腿方向语义——与到簿先后无关）。
    pub maker: Owner,
    /// 买腿（taker）所有者。
    pub taker: Owner,
    /// taker 方向（成交方向=taker side）。
    pub taker_side: Side,
    /// maker 腿本笔实收费用合计（分；maker 恒为卖方——佣金+印花+过户
    /// 封顶分配实收；Sol 五审同拍公开期末投影的现金推导源）。
    pub maker_fees_cents: i64,
    /// taker 腿本笔实收费用合计（分；taker 恒为买方——佣金+过户边际）。
    pub taker_fees_cents: i64,
    /// maker 腿订单 desk 序号（Sol 六审：执行腿同拍投影按真实订单
    /// 身份推进——OrderRef.1）。
    pub maker_order_serial: u64,
    /// taker 腿订单 desk 序号。
    pub taker_order_serial: u64,
}

/// 集合竞价事实（批 8f：AuctionTick/AuctionCompleted 事件输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeskAuctionFact {
    /// 股索引。
    pub stock: StockIdx,
    /// tick。
    pub tick: u64,
    /// 交易阶段（开盘 CallAuction/收盘 ClosingAuction）。
    pub phase: crate::TradingPhase,
    /// 指示价（竞价窗内虚拟撮合预演；无交叉 None）。
    pub indicative_price: Option<i64>,
    /// 配对量。
    pub matched_volume: u64,
    /// 是否为完成撮合（true=AuctionCompleted；false=竞价窗 AuctionTick）。
    pub completed: bool,
    /// 竞价失衡（买超减卖超，AuctionCompleted 携带）。
    pub imbalance: u64,
}

/// 挂单生命周期事实（批 8e 事件契约：OrderAccepted/OrderCanceled 输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeskLifecycleFact {
    /// 股索引。
    pub stock: StockIdx,
    /// 所有者。
    pub owner: Owner,
    /// 本地序号（OrderRef.1）。
    pub order_serial: u64,
    /// 剩余量。
    pub remaining: u32,
    /// 方向（Accepted 用）。
    pub side: Side,
    /// 限价（Accepted 用）。
    pub price: i64,
    /// 种类。
    pub kind: LifecycleKind,
}

/// 生命周期种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleKind {
    /// 挂簿受理（余量进簿）。
    Accepted,
    /// 撤单终结。
    Cancelled,
    /// 到期终结。
    Expired,
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
    /// 本轮双腿成交事实（TickFrame 事件映射输入；按成交序）。
    pub trades: Vec<DeskTradeFact>,
    /// 本轮生命周期事实（受理/撤单/到期）。
    pub lifecycle: Vec<DeskLifecycleFact>,
    /// 本轮回执明细（IntentRejected 映射输入）。
    pub receipts_out_list: Vec<DeskReceipt>,
    /// 本轮竞价事实（拍卖窗 indicative/完成撮合）。
    pub auction: Vec<DeskAuctionFact>,
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
    /// 日结校验失败（候选组装/对账失败 → Abort 冲回，日历不推进）。
    #[error("日结 {day} 校验失败：{detail}")]
    DayEndValidation {
        /// 失败交易日。
        day: u32,
        /// 失败详情。
        detail: String,
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
