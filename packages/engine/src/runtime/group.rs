//! NPC 组线程骨架：持有组内账户（现金/持仓/在途登记），决策脚本注入（批 4 换
//! `decide_data`），实体扣除 → 分段 flush → 收实体落账。
//!
//! 失败原子性（F §5.2 组账户行）：
//! - **可逆日志**：本 tick 内每次落账（实体应用）与每次决策期扣除、每次
//!   `foreign_active` 增减都记逆操作；Abort 逆序回放恢复起始值。
//! - **转正点**：收到下一 `Start`（隐式：上一 tick 已成功）——落账 staging 后丢弃
//!   全部本 tick 日志。`Continue` 只触发落账（进可逆日志），**不是**转正点。
//! - **Abort**：逆序回放 + 清 staging + 回到 `committed_tick`（失败后可继续重跑）。

use super::decision::{decide_account, NpcProfile, PendingOp};
use super::fees::buy_purse_limit;
use super::plans::PlanChain;
use super::protocol::TickSnapshot;
use super::protocol::{
    AdminRight, DayEndDirective, DeskBatch, DeskIn, GroupIn, GroupReport, RoutedPrice,
    RuntimeError, UpMsg, WorkerId,
};
use super::watchdog::{worker_recv, WatchdogConfig};
use super::{Entity, FillFact, GroupIdx, LocalIdx, OrderRef, StockIdx};
use crate::config::GameConfig;
use crate::money::Money;
use crate::orderbook::Side;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

/// 组内账户：现金聚合额 + 每股持仓聚合额 + T+1 冻结 + 每股在途挂单登记
/// + 每股成本基（invested/recovered 累计器）。
/// 不变量（期末审计断言）：`shares == sellable + frozen_today`（sellable 派生）。
/// 成本基口径（D3a-2c 件1，对齐 `Account::apply_buy_settlement` /
/// `apply_sell_settlement`）：invested/recovered 只累计**成交额**
/// （`price × qty`，佣金/过户费/印花税不入账）；清仓（shares 归零）后
/// 成本基清零（对齐 session 清仓删除条目——下次买入从零新建）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupAccount {
    /// 现金聚合额（i64 分）。
    pub cash: i64,
    /// 每股持仓聚合额（下标=StockIdx）。
    pub shares: Vec<u32>,
    /// 每股当日买入冻结（T+1：日界解冻，非每 tick——批 5 DayEnd 接线）。
    pub frozen_today: Vec<u32>,
    /// 每股在途挂单计数（下标=StockIdx；严格所有权口径的诊断面）。
    pub foreign_active: Vec<u16>,
    /// 每股累计买入成交额（i64 分，仅成交额——开仓派生 cost_price 消费）。
    pub invested_cents: Vec<i64>,
    /// 每股累计卖出成交额（i64 分，仅成交额）。
    pub recovered_cents: Vec<i64>,
    /// 每股"曾持有"位（D3a-3 收口：session positions 含清仓后零量条目
    /// ——观察面 held 集成员（sample_discovery_stock 持仓优先采样）；
    /// 组侧 qty=0 无条目概念，以独立位承载。清仓置位、再持有复位）。
    pub ever_held: Vec<bool>,
}

impl GroupAccount {
    /// 构造：给定现金与股数（持仓/冻结/在途/成本基清零）。
    pub fn new(cash: i64, n_stocks: usize) -> Self {
        GroupAccount {
            cash,
            shares: vec![0; n_stocks],
            frozen_today: vec![0; n_stocks],
            foreign_active: vec![0; n_stocks],
            invested_cents: vec![0; n_stocks],
            recovered_cents: vec![0; n_stocks],
            ever_held: vec![false; n_stocks],
        }
    }

    /// 可卖（T+1 口径）：持仓 − 当日冻结。
    pub fn sellable(&self, stock: StockIdx) -> u32 {
        self.shares[stock as usize] - self.frozen_today[stock as usize]
    }

    /// 日界解冻：当日买入进入可卖（DayEnd 相位调用；批 5 接线）。
    pub fn unfreeze_t1(&mut self) {
        for f in &mut self.frozen_today {
            *f = 0;
        }
    }
}

/// D3a-2 观察基输入（每股）：desk 权威分钟收盘+日 K 条（尾部 250——
/// desk 侧已按行为窗口截断，组侧不二次截断）。
#[derive(Clone, Default)]
struct ObservationInputs {
    minutes: Vec<(u64, i64)>,
    /// 已完成日（尾部 250，不过滤零成交日）。
    daily_bars: Vec<(i64, i64, i64, i64, u64)>,
    /// 当日活动 K。
    daily_active: Option<(i64, i64, i64, i64, u64)>,
    /// 已完成日总数（绝对索引基）。
    daily_total_completed: u64,
    /// 历史累计 traded 日数。
    daily_total_traded: u64,
}

impl ObservationInputs {
    /// 链观察构建（与 session 读面同源纯函数——字段不得分叉）。
    fn build_observations(
        &self,
    ) -> (
        crate::observation::PricePathObservation,
        crate::observation::TechnicalObservation,
    ) {
        let minute_closes: Vec<crate::observation::MarketMinuteClose> = self
            .minutes
            .iter()
            .map(|&(minute, c)| crate::observation::MarketMinuteClose {
                absolute_trading_minute: minute,
                close: crate::money::Money::from_cents(c.max(1)),
            })
            .collect();
        // 行为路径口径：已完成日**不过滤**（含零成交日）——绝对索引
        // trading_day = total_completed − kept + offset（同
        // retained_behavior_daily_closes_history）。
        let kept = self.daily_bars.len();
        let start = self.daily_total_completed.saturating_sub(kept as u64);
        let completed_days: Vec<crate::observation::CompletedDayClose> = self
            .daily_bars
            .iter()
            .enumerate()
            .map(|(offset, bar)| crate::observation::CompletedDayClose {
                trading_day: u32::try_from(start + offset as u64)
                    .expect("行为日索引超 u32"),
                close: crate::money::Money::from_cents(bar.3.max(1)),
            })
            .collect();
        let current_day_open = self
            .daily_active
            .filter(|bar| bar.4 > 0)
            .map(|bar| crate::money::Money::from_cents(bar.0.max(1)));
        let price_path = crate::observation::build_price_path_observation(
            &minute_closes,
            &completed_days,
            current_day_open,
        )
        .unwrap_or_else(|e| panic!("组内价格路径观察构建失败：{e:?}"));
        // 技术面口径：traded（volume>0）**含活动 K**，窗 SMA_LONG_WINDOW
        // （60）滚动；valid_sample_count=历史累计 traded 总数（同
        // session traded_count()）。
        let mut bars: Vec<crate::strategy::TechnicalDailyBar> = Vec::new();
        let mut all: Vec<&(i64, i64, i64, i64, u64)> =
            self.daily_bars.iter().collect();
        if let Some(active) = &self.daily_active {
            all.push(active);
        }
        for bar in all.into_iter().filter(|bar| bar.4 > 0) {
            bars.push(crate::strategy::TechnicalDailyBar {
                high: crate::money::Money::from_cents(bar.1.max(1)),
                low: crate::money::Money::from_cents(bar.2.max(1)),
                close: crate::money::Money::from_cents(bar.3.max(1)),
            });
        }
        let technical_window = crate::strategy::SMA_LONG_WINDOW;
        if bars.len() > technical_window {
            bars.drain(..bars.len() - technical_window);
        }
        let technical = crate::observation::build_technical_observation_from_recent_trades(
            &bars,
            self.daily_total_traded as usize,
        );
                (price_path, technical)
    }
}

/// 计划子单在途票（Sol 整改 2：挂出量+受理身份——adopt 后身份在）。
#[derive(Clone, Debug)]
struct ChildTicket {
    /// 挂出量（完整子单数量——PendingOp.qty，adopt 的数量输入）。
    qty: u32,
    /// 受理身份（fill 首见时自 FillFact.order 确定性合成；None=挂出
    /// 未受理——desk 尚未成交/拒收）。
    order_id: Option<crate::OrderId>,
}

/// FillFact.order 的确定性身份合成（Sol 整改 2：不另造计数器——
/// OrderRef=(股, desk 本地序号) → OrderId 高位编码股索引）。
fn order_id_of(order: &OrderRef) -> crate::OrderId {
    crate::OrderId((u64::from(order.0) << 48) | (order.1 & 0x0000_FFFF_FFFF_FFFF))
}

/// 组端可逆日志条目：记录“已施加的增量”，回滚时逆序施加 `-delta`。
/// 覆盖：决策期扣除（purse/股票）、实体落账、`foreign_active` 增减。
#[derive(Clone, Debug)]
enum GroupUndo {
    /// 现金变动（扣除为负、落账为正）。
    Cash { local: LocalIdx, delta: i64 },
    /// 持仓变动（卖出扣除为负、回流为正）。
    Shares {
        local: LocalIdx,
        stock: StockIdx,
        delta: i32,
    },
    /// 在途登记变动（下单 +1、Closed 落账 −1）。
    ForeignActive {
        local: LocalIdx,
        stock: StockIdx,
        delta: i16,
    },
    /// T+1 冻结变动（BoughtShares 落账 +qty）。
    Frozen {
        local: LocalIdx,
        stock: StockIdx,
        delta: i32,
    },
    /// attention 重排（decide 后 next_due 前移；`prev`=重排前值）。
    /// Sol 二审修复集 5：不记则失败 tick Abort 后 NPC 已被排到未来，
    /// 重跑同 tick 时少一次决策。
    NextDue {
        local: LocalIdx,
        prev: u64,
    },
    /// 成本基变动（D3a-2c 件1：FillFact 落账的 invested/recovered 增量，
    /// 含清仓清零的负增量；回滚时逆序减回）。
    CostBasis {
        local: LocalIdx,
        stock: StockIdx,
        d_invested: i64,
        d_recovered: i64,
    },
    /// 个人态写回前快照（D3a-3：observe 演进的组态写回——Abort 恢复；
    /// 与 session 版"个人候选私有、失败不 install"语义对齐）。
    Personal {
        local: LocalIdx,
        prev: Box<super::world::BeliefParticipantSnapshot>,
    },
    /// 计划簿创建前快照（D3a-3：组内开仓 create 的可逆化——Abort 恢复；
    /// tick 内首个创建时保存一次）。
    PlanBook {
        prev: Box<crate::plans::PlanBook>,
    },
    /// 曾持有位回滚（D3a-3 收口：清仓置位/再持有复位的逆操作）。
    EverHeld {
        local: LocalIdx,
        stock: StockIdx,
        prev: bool,
    },
    /// 计划子单在途票回滚（Sol 整改 2：prev=变更前值——Abort 逆序恢复；
    /// 覆盖挂出置入/身份合成/全成与终结清除）。
    ChildInFlight {
        account: crate::AccountId,
        code: crate::StockId,
        prev: Option<ChildTicket>,
    },
}

/// 决策脚本注入的下单规格（组端换算为 `DeskIn`， purse=对价，费用上限随批 3 补）。
#[derive(Clone, Copy, Debug)]
pub struct PlaceSpec {
    /// 组内名册下标。
    pub local: LocalIdx,
    /// 目标股。
    pub stock: StockIdx,
    /// 方向。
    pub side: Side,
    /// 限价（i64 分；骨架只支持 Fixed）。
    pub price: i64,
    /// 数量。
    pub qty: u32,
}

/// 决策脚本注入的对齐撤单规格。
#[derive(Clone, Copy, Debug)]
pub struct CancelSpec {
    /// 组内名册下标。
    pub local: LocalIdx,
    /// 目标股。
    pub stock: StockIdx,
}

/// 一轮决策计划（骨架档：脚本产出；批 4 换 decide_data + 计划续行）。
#[derive(Clone, Debug, Default)]
pub struct TickPlan {
    /// 本轮下单（同 NPC 的 cancel 恒先于 place——FIFO 对齐语义由脚本顺序保证）。
    pub places: Vec<PlaceSpec>,
    /// 本轮对齐撤单。
    pub cancels: Vec<CancelSpec>,
    /// 本组是否还有续行需求（驱动协调者 Continue）。
    pub continuation: bool,
    /// 注入 panic（panic 运输测试）：处理期以 `panic_any` 抛出。
    pub panic_payload: Option<String>,
    /// 注入停顿（看门狗测试）。
    pub stall: Option<Duration>,
}

/// 组决策脚本：`(tick, round) -> TickPlan`。
pub type GroupScript = Box<dyn Fn(u64, u32) -> TickPlan + Send>;

/// 组线程观察日志（协议时序断言用：实体批次先于下一 Start）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observed {
    /// 收到实体批次（携带产出 tick）。
    Batch(u64),
    /// 收到 Start（携带 tick）。
    Start(u64),
    /// 收到 Continue（携带 tick/round）。
    Continue(u64, u32),
    /// 收到 Abort（携带 tick）。
    Abort(u64),
    /// 收到 DayEnd（携带交易日）。
    DayEnd(u32),
    /// 收到 Shutdown。
    Shutdown,
}

/// 组线程期末上交状态。
#[derive(Clone, Debug)]
pub struct GroupFinalState {
    /// 组索引。
    pub group: GroupIdx,
    /// 期末账户（含 Drain 回流落账）。
    pub accounts: Vec<GroupAccount>,
    /// 累计跳过单量。
    pub skipped_orders: u64,
    /// 累计到期决策次数。
    pub decide_calls_total: u64,
    /// 累计 Cancel 意图跳过数。
    pub cancel_intents_total: u64,
    /// 当前态哈希（Abort 回滚后应等于 `committed_hash`）。
    pub state_hash: u64,
    /// 提交态哈希（最后一次 Start 转正后）。
    pub committed_hash: u64,
    /// 消息观察日志。
    pub observed: Vec<Observed>,
}

/// 组线程私有状态。
pub struct GroupState {
    /// 组索引。
    pub group: GroupIdx,
    /// 组内账户（下标=LocalIdx）。
    pub accounts: Vec<GroupAccount>,
    /// 可逆日志（本 tick 内累积；Start 转正时清空）。
    undo: Vec<GroupUndo>,
    /// 实体批次 staging（tick N 内到达，Start/Continue 时落账）。
    staging: Vec<DeskBatch>,
    /// 当前正在执行的 tick（Abort 后回退到 committed_tick）。
    cur_tick: u64,
    /// 当前轮次。
    cur_round: u32,
    /// 已转正 tick。
    committed_tick: u64,
    /// 提交态指纹（O(1) 诊断：fnv(tick 递进)；批 8b 哈希惰性化——热路径
    /// 报告不再做全量账户哈希，回滚完整性判据改字段断言，全量 `state_hash`
    /// 留 Final/日结冷路径）。
    committed_hash: u64,
    /// 累计跳过单量。
    pub skipped_orders: u64,
    /// 累计到期 NPC 决策次数（attention due）。
    pub decide_calls_total: u64,
    /// 累计被登记跳过的 Cancel 意图数。
    pub cancel_intents_total: u64,
    /// 分段 flush 粒度（op 数）。
    chunk: usize,
    /// 一手股数（买入整手/卖出零股一次性校验）。
    lot: u32,
    /// 费用/整手配置（purse 最坏情况口径）。
    config: Arc<GameConfig>,
    /// 决策装配（Some=内联 decide_data 路径；None=测试脚本路径）。
    decision: Option<(Vec<NpcProfile>, u32)>,
    /// 每交易日 tick 数（T+1 日界解冻判定）。
    ticks_per_day: u64,
    /// 最近一次广播快照（决策观察基；Continue 轮复用）。
    snapshot: Option<std::sync::Arc<TickSnapshot>>,
    /// 活跃计划链（Continue 轮驱动；跨股步骤产物走实体扣除路径）。
    plans: Vec<Box<dyn PlanChain>>,
    /// 本 tick 已见成交事实（计划续行的结果输入；Start 落账后清空）。
    plan_fills: Vec<FillFact>,
    /// 消息观察日志。
    pub observed: Vec<Observed>,
    /// 外部注入计划缓冲（玩家意图路由；run_round 消费一次即清）。
    external_pending: Vec<super::decision::PendingOp>,
    /// D3a-2b 信念个人态（逐组内下标；None=非机构位）——构造期投影，
    /// Belief-1b 起承接披露驱动的组内自演进。
    belief_states: Vec<Option<super::world::BeliefParticipantSnapshot>>,
    /// 公开库（Belief-1b·下：Arc 共享——投递消息稀疏刷新）。
    library: std::sync::Arc<crate::information::PublicLibrary>,
    /// D3a-2 观察基：快照来源的每股（分钟收盘，日 K 条）——链观察的
    /// 纯输入（与 session 读面同源：desk 权威 minute_closes/daily_bars；
    /// 观察对象每 tick 由纯构建器现算，不累积不缓存——口径不可能漂移）。
    observation_inputs: Vec<ObservationInputs>,
    /// D3a-2c 件2：决策链根组装投影（None=无会话来源——observe 直调
    /// 不可用，读面上行空）。
    root_projection: Option<std::sync::Arc<super::world::RootProjection>>,
    /// D3a-3 复核收口+Sol 整改 2：计划子单在途票（挂出量+受理身份）——
    /// 身份=FillFact.order 的确定性合成（不另造计数器）；部分成交推进、
    /// 全成/终结（Closed/到期/日终清簿/拒收）清除。
    child_in_flight:
        std::collections::BTreeMap<(crate::AccountId, crate::StockId), ChildTicket>,
    /// Sol 整改 1：簿快照已保存标志（本 tick 内首个簿变更前保存一次；
    /// 转正/回滚清 undo 时重置）。
    plan_undo_saved: bool,
    /// D3a-2c 件4：组内计划簿（开仓产物落地——crate::plans 纯类型直载；
    /// 组装时起始空，runtime 模式 session plans 恒空天然一致）。
    plan_book: crate::plans::PlanBook,
    /// 决策脚本。
    script: GroupScript,
}

impl GroupState {
    /// 构造组状态（全新世界：tick 0 起步）。
    pub fn new(
        group: GroupIdx,
        accounts: Vec<GroupAccount>,
        chunk: usize,
        lot: u32,
        config: Arc<GameConfig>,
        ticks_per_day: u64,
        decision: Option<(Vec<NpcProfile>, u32)>,
        plans: Vec<Box<dyn PlanChain>>,
        script: GroupScript,
    ) -> Self {
        Self::new_with_initial_tick(
            group,
            accounts,
            Vec::new(),
            std::sync::Arc::new(crate::information::PublicLibrary::default()),
            chunk,
            lot,
            config,
            ticks_per_day,
            0,
            decision,
            plans,
            None,
            script,
        )
    }

    /// 构造组状态（D0：从 initial_tick 起步——恢复/接管对齐；Start 的
    /// tick 连续性校验按绝对 tick 推导）。
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_initial_tick(
        group: GroupIdx,
        accounts: Vec<GroupAccount>,
        belief_states_seed: Vec<Option<super::world::BeliefParticipantSnapshot>>,
        library_seed: std::sync::Arc<crate::information::PublicLibrary>,
        chunk: usize,
        lot: u32,
        config: Arc<GameConfig>,
        ticks_per_day: u64,
        initial_tick: u64,
        decision: Option<(Vec<NpcProfile>, u32)>,
        plans: Vec<Box<dyn PlanChain>>,
        root_projection: Option<std::sync::Arc<super::world::RootProjection>>,
        script: GroupScript,
    ) -> Self {
        let n_stocks = accounts.first().map(|a| a.shares.len()).unwrap_or(0);
        GroupState {
            group,
            accounts,
            undo: Vec::new(),
            staging: Vec::new(),
            cur_tick: initial_tick,
            cur_round: 0,
            committed_tick: initial_tick,
            committed_hash: 0,
            skipped_orders: 0,
            decide_calls_total: 0,
            cancel_intents_total: 0,
            chunk,
            lot,
            config,
            ticks_per_day,
            decision,
            snapshot: None,
            plans,
            plan_fills: Vec::new(),
            observed: Vec::new(),
            external_pending: Vec::new(),
            observation_inputs: vec![ObservationInputs::default(); n_stocks],
            belief_states: belief_states_seed,
            library: library_seed,
            root_projection,
            child_in_flight: Default::default(),
            plan_undo_saved: false,
            plan_book: crate::plans::PlanBook::default(),
            script,
        }
    }

    /// 当前态哈希（FNV-1a，按名册与股序确定序遍历）。仅域内容（账户），不含
    /// 协议位置字段（tick/轮）——跨 tick 可比（Abort 回滚等价判据）。
    pub fn state_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = fnv(h, self.group as u64);
        for a in &self.accounts {
            h = fnv(h, a.cash as u64);
            for &s in &a.shares {
                h = fnv(h, s as u64);
            }
            for &f in &a.frozen_today {
                h = fnv(h, f as u64);
            }
            for &f in &a.foreign_active {
                h = fnv(h, f as u64);
            }
        }
        // Sol 二审修复集 5：attention 调度态（next_due）入全量哈希——
        // Abort 回滚等价判据覆盖"决策已重排但未回放"的漂移。
        if let Some((profiles, _)) = &self.decision {
            for p in profiles {
                h = fnv(h, p.next_due);
            }
        }
        // Sol 整改 1：计划域新状态入哈希（簿数值面+在途+身份计数——
        // Abort 回滚等价判据覆盖计划/身份漂移）。
        for plan_id in self.plan_book.plan_ids() {
            if let Ok(plan) = self.plan_book.plan(plan_id) {
                h = fnv(h, plan_id.0);
                h = fnv(h, u64::from(plan.filled_qty()));
                h = fnv(
                    h,
                    match plan.status() {
                        crate::plans::PlanStatus::Active => 0,
                        crate::plans::PlanStatus::Paused { .. } => 1,
                        crate::plans::PlanStatus::Completed => 2,
                        crate::plans::PlanStatus::Terminated { .. } => 3,
                    },
                );
                h = fnv(h, plan.active_child_order_id().map_or(0, |o| o.0));
            }
        }
        for ((account, code), ticket) in &self.child_in_flight {
            h = fnv(h, u64::from(account.0));
            h = fnv(h, u64::from(code.0));
            h = fnv(h, u64::from(ticket.qty));
            h = fnv(h, ticket.order_id.map_or(0, |o| o.0));
        }
        h
    }

    /// 逆序回放可逆日志（Abort/panic 回滚）：恢复到本 tick 起始（上一收敛态）。
    fn rollback(&mut self) {
        for entry in self.undo.drain(..).rev() {
            match entry {
                GroupUndo::Cash { local, delta } => {
                    self.accounts[local as usize].cash -= delta;
                }
                GroupUndo::Shares {
                    local,
                    stock,
                    delta,
                } => {
                    let slot = &mut self.accounts[local as usize].shares[stock as usize];
                    *slot = (*slot as i64 - delta as i64) as u32;
                }
                GroupUndo::ForeignActive {
                    local,
                    stock,
                    delta,
                } => {
                    let slot = &mut self.accounts[local as usize].foreign_active[stock as usize];
                    *slot = (*slot as i32 - delta as i32) as u16;
                }
                GroupUndo::Frozen {
                    local,
                    stock,
                    delta,
                } => {
                    let slot = &mut self.accounts[local as usize].frozen_today[stock as usize];
                    *slot = (*slot as i64 - delta as i64) as u32;
                }
                GroupUndo::NextDue { local, prev } => {
                    // Sol 二审修复集 5：回放恢复重排前 due——重跑同 tick
                    // 时该 NPC 仍到期（决策次数不少一次）。
                    if let Some((profiles, _)) = &mut self.decision {
                        if let Some(p) = profiles.get_mut(local as usize) {
                            p.next_due = prev;
                        }
                    }
                }
                GroupUndo::CostBasis {
                    local,
                    stock,
                    d_invested,
                    d_recovered,
                } => {
                    let acct = &mut self.accounts[local as usize];
                    acct.invested_cents[stock as usize] -= d_invested;
                    acct.recovered_cents[stock as usize] -= d_recovered;
                }
                GroupUndo::Personal { local, prev } => {
                    if let Some(slot) = self.belief_states.get_mut(local as usize) {
                        *slot = Some(*prev);
                    }
                }
                GroupUndo::PlanBook { prev } => {
                    self.plan_book = *prev;
                }
                GroupUndo::EverHeld { local, stock, prev } => {
                    if let Some(acct) = self.accounts.get_mut(local as usize) {
                        acct.ever_held[stock as usize] = prev;
                    }
                }
                GroupUndo::ChildInFlight { account, code, prev } => {
                    // Sol 整改 2：恢复变更前票值（None=原本不在途）。
                    match prev {
                        Some(ticket) => {
                            self.child_in_flight.insert((account, code), ticket);
                        }
                        None => {
                            self.child_in_flight.remove(&(account, code));
                        }
                    }
                }
            }
        }
        self.staging.clear();
        self.plan_fills.clear();
        self.plan_undo_saved = false;
        self.cur_tick = self.committed_tick;
        self.cur_round = 0;
    }

    fn acct_mut(&mut self, local: LocalIdx) -> Result<&mut GroupAccount, RuntimeError> {
        let group = self.group;
        self.accounts
            .get_mut(local as usize)
            .ok_or_else(|| RuntimeError::ProtocolInvariant {
                detail: format!("组 {group} 收到越界 local={local}"),
            })
    }

    /// 落账 staging 中属于 `(expected_tick, expected_round)` 的批次（记逆操作）。
    fn apply_staging(
        &mut self,
        expected_tick: u64,
        expected_round: u32,
    ) -> Result<(), RuntimeError> {
        let batches = std::mem::take(&mut self.staging);
        for batch in batches {
            batch
                .verify_tick_round(expected_tick, expected_round)
                .map_err(|m| RuntimeError::ProtocolInvariant {
                    detail: format!("组 {} staging 批次防御核对失败：{m}", self.group),
                })?;
            let stock = batch.stock as usize;
            if stock >= self.accounts.first().map(|a| a.shares.len()).unwrap_or(0) {
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!("组 {} staging 批次股索引越界：{stock}", self.group),
                });
            }
            // 成交事实进计划结果缓冲（信息回流：续行由 FillFact 驱动，F §4.3）。
            self.plan_fills.extend(batch.fills.iter().copied());
            // D3a-2c 件1：成本基先行（FillFact 逐笔记 invested/recovered，
            // 仅成交额——对齐 Account::apply_buy/apply_sell 口径）。
            // D3a-3：计划成交推进（执行腿——同股同方向唯一活跃计划）。
            let mut touched: Vec<(LocalIdx, StockIdx)> = Vec::new();
            for fill in &batch.fills {
                self.apply_fill_cost_basis(stock as StockIdx, fill)?;
                self.apply_fill_plan_progress(stock as StockIdx, fill)?;
                if !touched.contains(&(fill.owner.1, stock as StockIdx)) {
                    touched.push((fill.owner.1, stock as StockIdx));
                }
            }
            // Sol 整改 2：拒收回执=失败释放（未受理票清除——desk 拒收
            // 的挂单从未成交，票 order_id=None 走未受理释放路径）。
            for receipt in &batch.receipts {
                let rejected = matches!(
                    receipt.kind,
                    crate::runtime::ReceiptKind::PriceOutOfBand
                        | crate::runtime::ReceiptKind::NotRoundLot
                        | crate::runtime::ReceiptKind::ExceedsOrderCap
                        | crate::runtime::ReceiptKind::PhaseRejected
                        | crate::runtime::ReceiptKind::AuctionLimitOrderRequired
                );
                if rejected {
                    self.release_child_ticket(stock as StockIdx, receipt.owner.1)?;
                }
            }
            for entity in batch.entities {
                self.apply_entity(stock as StockIdx, entity)?;
            }
            // 清仓清零（实体落账后按期末 shares 判定：对齐 session 清仓删除
            // 条目——下次买入从零新建，避免再持有后 cost_price 漂移）。
            self.clear_flat_cost_basis(&touched);
        }
        Ok(())
    }

    /// 单笔成交的成本基落账（买腿 invested += price×qty、卖腿 recovered
    /// += price×qty；费用不入账）。记逆操作（Abort 冲回）。
    fn apply_fill_cost_basis(
        &mut self,
        stock: StockIdx,
        fill: &FillFact,
    ) -> Result<(), RuntimeError> {
        let group = self.group;
        if fill.owner.0 != group {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "组 {group} 批次收到外组成交事实 owner={:?}",
                    fill.owner
                ),
            });
        }
        let local = fill.owner.1;
        let amount = Money::from_cents(fill.price)
            .mul_shares(fill.qty)
            .map_err(|e| RuntimeError::ProtocolInvariant {
                detail: format!(
                    "组 {group} 成本基金额溢出：local={local}, stock={stock}, price={}, qty={}：{e:?}",
                    fill.price, fill.qty
                ),
            })?
            .cents();
        let (d_invested, d_recovered) = match fill.side {
            Side::Buy => (amount, 0),
            Side::Sell => (0, amount),
        };
        let acct = self.acct_mut(local)?;
        let slot = &mut acct.invested_cents[stock as usize];
        *slot = slot.checked_add(d_invested).ok_or_else(|| {
            RuntimeError::ProtocolInvariant {
                detail: format!(
                    "组 {group} invested 溢出：local={local}, stock={stock}, delta={d_invested}"
                ),
            }
        })?;
        let acct = self.acct_mut(local)?;
        let slot = &mut acct.recovered_cents[stock as usize];
        *slot = slot.checked_add(d_recovered).ok_or_else(|| {
            RuntimeError::ProtocolInvariant {
                detail: format!(
                    "组 {group} recovered 溢出：local={local}, stock={stock}, delta={d_recovered}"
                ),
            }
        })?;
        self.undo.push(GroupUndo::CostBasis {
            local,
            stock,
            d_invested,
            d_recovered,
        });
        Ok(())
    }

    /// Sol 整改 2：成交事实的计划推进。身份=FillFact.order 确定性合成
    ///（首见 adopt，数量=**挂出量**——完整子单数量）；部分成交推进
    /// remaining；全成清在途票（下一轮可再报价——加仓）。
    fn apply_fill_plan_progress(
        &mut self,
        stock: StockIdx,
        fill: &FillFact,
    ) -> Result<(), RuntimeError> {
        let group = self.group;
        let Some(&account) = self
            .root_projection
            .as_ref()
            .and_then(|projection| projection.account_ids.get(fill.owner.1 as usize))
        else {
            return Ok(());
        };
        let code = crate::StockId(u32::from(stock));
        // 在途票（本 (账户, 股) 唯一）——无票=非计划链子单（retail/注入）。
        let Some(mut ticket) = self.child_in_flight.get(&(account, code)).cloned() else {
            return Ok(());
        };
        if self
            .plan_book
            .active_plan(account, &code)
            .is_none_or(|plan| plan.direction() != fill.side)
        {
            return Ok(());
        }
        let day = self.cur_tick / self.ticks_per_day;
        self.save_plan_book_undo();
        // 首见 adopt（身份=FillFact.order 合成；数量=挂出量——完整）。
        if ticket.order_id.is_none() {
            let order_id = order_id_of(&fill.order);
            let child_qty = ticket.qty;
            self.plan_book
                .record_child_submitted(account, &code, fill.side, order_id, child_qty, day)
                .unwrap_or_else(|error| {
                    panic!("组内子单身份登记失败 {account:?} {code:?}: {error}")
                });
            ticket.order_id = Some(order_id);
            self.undo.push(GroupUndo::ChildInFlight {
                account,
                code: code.clone(),
                prev: Some(ChildTicket {
                    qty: ticket.qty,
                    order_id: None,
                }),
            });
            self.child_in_flight.insert((account, code.clone()), ticket.clone());
        }
        let Some(order_id) = ticket.order_id else {
            return Ok(());
        };
        let remaining = {
            let Some(plan) = self.plan_book.active_plan(account, &code) else {
                return Ok(());
            };
            plan.active_child_remaining_qty()
                .unwrap_or_else(|| plan.execution().expect("在途腿存在").child_qty())
        };
        let pushed = self
            .plan_book
            .record_child_fill(account, &code, fill.side, order_id, fill.qty, day)
            .unwrap_or_else(|error| {
                panic!("组 {group} 计划成交推进失败 {account:?} {code:?}: {error}")
            });
        if !pushed {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "组 {group} 成交事实未被计划消费：account={account:?}, code={code:?}, order={order_id:?}, remaining={remaining}"
                ),
            });
        }
        // 部分成交推进（票保留）或全成清除（下一轮可再报价）。
        if fill.qty >= remaining {
            if self.child_in_flight.remove(&(account, code)).is_some() {
                self.undo.push(GroupUndo::ChildInFlight {
                    account,
                    code,
                    prev: Some(ticket),
                });
            }
        }
        Ok(())
    }

    /// Sol 整改 2：子单终结（Closed 实体——到期、日终清簿、撤销与
    /// 失败释放的统一信号）。已受理（有身份）→record_child_released；
    /// 未受理→票清除（执行腿留待下次挂出覆盖——install 对 order=None
    /// 可重入）。
    fn release_child_ticket(
        &mut self,
        stock: StockIdx,
        local: LocalIdx,
    ) -> Result<(), RuntimeError> {
        let Some(&account) = self
            .root_projection
            .as_ref()
            .and_then(|projection| projection.account_ids.get(local as usize))
        else {
            return Ok(());
        };
        let code = crate::StockId(u32::from(stock));
        let Some(ticket) = self.child_in_flight.remove(&(account, code)) else {
            return Ok(());
        };
        self.undo.push(GroupUndo::ChildInFlight {
            account,
            code: code.clone(),
            prev: Some(ticket.clone()),
        });
        if let Some(order_id) = ticket.order_id {
            let day = self.cur_tick / self.ticks_per_day;
            self.save_plan_book_undo();
            let released = self
                .plan_book
                .record_child_released(account, &code, order_id, day)
                .unwrap_or_else(|error| {
                    panic!("组内子单释放失败 {account:?} {code:?}: {error}")
                });
            if !released {
                // 已受理身份与计划不符=实现漂移，显式失败（铁律二）。
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!(
                        "组 {} 子单释放未被计划消费：account={account:?}, code={code:?}, order={order_id:?}",
                        self.group
                    ),
                });
            }
        }
        Ok(())
    }

    /// 清仓清零+曾持有位（期末 touched 槽位的终态维护）：
    /// - 成本基清零：shares==0 且**该股无在途挂单**——invested/recovered
    ///   归零。foreign_active 守卫（复核发现 2 收口）：跨轮余量退款（轮 N
    ///   卖单全成 shares=0，轮 N+1 RefundedShares 回升）不得永久丢失成本
    ///   基——在途未终结即不清零，退款落账（shares 回升）后自然跳过。
    ///   登记残余边界：同 tick 卖光+同股新买单在途推迟清零，买单 Closed
    ///   时 shares>0（成交）则旧值残留（session 语义=删条目后新建零基）
    ///   ——罕见形态，D3a-3 注入路由统一时一并收口。
    /// - 曾持有位（D3a-3 收口）：shares==0 → true（对齐 session 清仓保留
    ///   零量条目——观察面 held 集成员）；shares>0 → false（再持有）。
    fn clear_flat_cost_basis(&mut self, touched: &[(LocalIdx, StockIdx)]) {
        for &(local, stock) in touched {
            let s = stock as usize;
            let acct = &mut self.accounts[local as usize];
            if acct.shares[s] == 0
                && acct.foreign_active[s] == 0
                && (acct.invested_cents[s] != 0 || acct.recovered_cents[s] != 0)
            {
                let (d_invested, d_recovered) = (acct.invested_cents[s], acct.recovered_cents[s]);
                acct.invested_cents[s] = 0;
                acct.recovered_cents[s] = 0;
                self.undo.push(GroupUndo::CostBasis {
                    local,
                    stock,
                    d_invested: -d_invested,
                    d_recovered: -d_recovered,
                });
            }
            let prev_ever = acct.ever_held[s];
            let now_ever = acct.shares[s] == 0;
            if prev_ever != now_ever {
                acct.ever_held[s] = now_ever;
                self.undo.push(GroupUndo::EverHeld {
                    local,
                    stock,
                    prev: prev_ever,
                });
            }
        }
    }

    /// 落账单条实体（记逆操作）。
    fn apply_entity(&mut self, stock: StockIdx, e: Entity) -> Result<(), RuntimeError> {
        let group = self.group;
        match e {
            Entity::CashProceeds { local, amount } | Entity::CashRefund { local, amount } => {
                let acct = self.acct_mut(local)?;
                acct.cash = acct.cash.checked_add(amount).ok_or_else(|| {
                    RuntimeError::ProtocolInvariant {
                        detail: format!("组 {group} 现金落账溢出：local={local}, amount={amount}"),
                    }
                })?;
                self.undo.push(GroupUndo::Cash {
                    local,
                    delta: amount,
                });
            }
            Entity::BoughtShares { local, qty } => {
                // T+1：当日买入冻结，日界解冻（下一交易日可卖）。
                let acct = self.acct_mut(local)?;
                let slot = &mut acct.shares[stock as usize];
                *slot = slot
                    .checked_add(qty)
                    .ok_or_else(|| RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "组 {group} 持仓落账溢出：local={local}, stock={stock}, qty={qty}"
                        ),
                    })?;
                acct.frozen_today[stock as usize] += qty;
                self.undo.push(GroupUndo::Shares {
                    local,
                    stock,
                    delta: qty as i32,
                });
                self.undo.push(GroupUndo::Frozen {
                    local,
                    stock,
                    delta: qty as i32,
                });
            }
            Entity::RefundedShares { local, qty } => {
                // 卖单余股回流：恢复可卖（不新增冻结）；曾持有位复位
                // （再次实际持有——session 条目 qty>0）。
                let acct = self.acct_mut(local)?;
                let slot = &mut acct.shares[stock as usize];
                *slot = slot
                    .checked_add(qty)
                    .ok_or_else(|| RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "组 {group} 持仓落账溢出：local={local}, stock={stock}, qty={qty}"
                        ),
                    })?;
                if qty > 0 {
                    let prev_ever = acct.ever_held[stock as usize];
                    if prev_ever {
                        acct.ever_held[stock as usize] = false;
                        self.undo.push(GroupUndo::EverHeld {
                            local,
                            stock,
                            prev: prev_ever,
                        });
                    }
                }
                self.undo.push(GroupUndo::Shares {
                    local,
                    stock,
                    delta: qty as i32,
                });
            }
            Entity::Closed { local } => {
                // Sol 整改 2：子单终结信号（撤销/到期/日终清簿——组侧
                // 在途票与执行腿释放）。
                self.release_child_ticket(stock, local)?;
                let acct = self.acct_mut(local)?;
                let slot = &mut acct.foreign_active[stock as usize];
                if *slot == 0 {
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "组 {} Closed 落账使在途登记为负：local={local}, stock={stock}",
                            self.group
                        ),
                    });
                }
                *slot -= 1;
                self.undo.push(GroupUndo::ForeignActive {
                    local,
                    stock,
                    delta: -1,
                });
            }
        }
        Ok(())
    }

    /// 处理 `Start{tick}`：收到 `Start(N)` 即证明 tick N−1 已收敛——落账 staging →
    /// 转正（清可逆日志、`committed_tick = N−1`）→ 本轮决策。
    fn handle_start(
        &mut self,
        tick: u64,
        desk_txs: &[Sender<super::protocol::DeskMsg>],
        up_tx: &Sender<UpMsg>,
    ) -> Result<(), RuntimeError> {
        if tick != self.cur_tick + 1 {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "组 {} Start 错序：tick={tick}, 上一执行 tick={}",
                    self.group, self.cur_tick
                ),
            });
        }
        // D3a-2：观察基——快照即权威（desk 的 minute_closes/daily_bars
        /// 每 tick 整值刷新，无累积无窗口截断——与 session 读面同源）。
        if let Some(snapshot) = &self.snapshot {
            for (inputs, stock) in self
                .observation_inputs
                .iter_mut()
                .zip(snapshot.stocks.iter())
            {
                inputs.minutes = stock.minute_closes.clone();
                inputs.daily_bars = stock.daily_bars.clone();
                inputs.daily_active = stock.daily_active;
                inputs.daily_total_completed = stock.daily_total_completed;
                inputs.daily_total_traded = stock.daily_total_traded;
            }
        }
        // 先落账上一 tick 最后一轮的实体批次（tick/round=上一执行位）。
        self.apply_staging(self.cur_tick, self.cur_round)?;
        // 转正点：丢弃上一 tick 可逆日志；committed 推进到 N−1（旧 cur_tick）。
        self.undo.clear();
        self.plan_undo_saved = false;
        self.committed_tick = self.cur_tick;
        self.cur_tick = tick;
        self.cur_round = 0;
        // T+1 日界解冻移至 DayEnd 相位（F §6.2，批 5）：Start 不再承担。
        self.committed_hash = (0xcbf2_9ce4_8422_2325u64 ^ tick).wrapping_mul(0x100000001b3);
        self.run_round(tick, 0, desk_txs, up_tx)
    }

    /// 日结相位（F §6.2 步骤 2/3）：Drain 回流落账 → T+1 解冻 → AdminBatch
    /// 权益落账——全部进可逆日志（日结校验失败 ⇒ Abort 整体冲回上一交易日
    /// 收敛态）；成功则由下一 `Start` 隐式转正（可逆日志退役=窗口关闭）。
    fn handle_day_end(&mut self, directive: &DayEndDirective) -> Result<(), RuntimeError> {
        if directive.tick != self.cur_tick {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "组 {} DayEnd tick 错配：{} != {}",
                    self.group, directive.tick, self.cur_tick
                ),
            });
        }
        // ⓪ 隐式转正上一 tick（日终 tick 已收敛——DayEnd 只在收束后发生）：
        // 隔离日结可逆日志，Abort 只冲回日结变更（回到上一交易日收敛态）。
        self.undo.clear();
        self.committed_tick = self.cur_tick;
        // ① Drain 回流落账（清空在途）。
        self.apply_staging(self.cur_tick, self.cur_round)?;
        // ② T+1 日界解冻（当日买入进入可卖）。
        for local in 0..self.accounts.len() as LocalIdx {
            let frozen = self.accounts[local as usize].frozen_today.clone();
            for (stock, &f) in frozen.iter().enumerate() {
                if f > 0 {
                    self.accounts[local as usize].frozen_today[stock] = 0;
                    self.undo.push(GroupUndo::Frozen {
                        local,
                        stock: stock as StockIdx,
                        delta: -(f as i32),
                    });
                }
            }
        }
        // ③ AdminBatch 权益落账（可逆）。
        for right in &directive.rights {
            self.apply_right(*right)?;
        }
        Ok(())
    }

    /// 单条权益落账（记逆操作）。
    fn apply_right(&mut self, right: AdminRight) -> Result<(), RuntimeError> {
        let group = self.group;
        match right {
            AdminRight::CashDividend {
                local,
                stock,
                amount,
            } => {
                let acct = self.acct_mut(local)?;
                acct.cash = acct.cash.checked_add(amount).ok_or_else(|| {
                    RuntimeError::ProtocolInvariant {
                        detail: format!("股利入账溢出：local={local}, stock={stock}"),
                    }
                })?;
                self.undo.push(GroupUndo::Cash {
                    local,
                    delta: amount,
                });
            }
            AdminRight::ShareSplit {
                local,
                stock,
                bonus_qty,
            } => {
                let acct = self.acct_mut(local)?;
                let slot = &mut acct.shares[stock as usize];
                *slot =
                    slot.checked_add(bonus_qty)
                        .ok_or_else(|| RuntimeError::ProtocolInvariant {
                            detail: format!("送转股溢出：local={local}, stock={stock}"),
                        })?;
                self.undo.push(GroupUndo::Shares {
                    local,
                    stock,
                    delta: bonus_qty as i32,
                });
            }
            AdminRight::WithholdingTax {
                local,
                stock,
                amount,
            } => {
                let acct = self.acct_mut(local)?;
                if acct.cash < amount {
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "组 {group} 扣税现金不足：local={local}, stock={stock}, cash={}, tax={amount}",
                            acct.cash
                        ),
                    });
                }
                acct.cash -= amount;
                self.undo.push(GroupUndo::Cash {
                    local,
                    delta: -amount,
                });
            }
        }
        Ok(())
    }

    /// 处理 `Continue{tick, round}`：落账本轮实体（进可逆日志，不转正）→ 续行决策。
    fn handle_continue(
        &mut self,
        tick: u64,
        round: u32,
        desk_txs: &[Sender<super::protocol::DeskMsg>],
        up_tx: &Sender<UpMsg>,
    ) -> Result<(), RuntimeError> {
        if tick != self.cur_tick || round != self.cur_round + 1 {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "组 {} Continue 错序：tick={tick}, round={round}, 当前=({}, {})",
                    self.group, self.cur_tick, self.cur_round
                ),
            });
        }
        // 上一轮实体批次落账（tick/round=上一执行位）。
        self.apply_staging(self.cur_tick, self.cur_round)?;
        self.cur_round = round;
        self.run_round(tick, round, desk_txs, up_tx)
    }

    /// D3a-3：单 belief 位的决策链一轮（observe 直调 → 组态写回 →
    /// Lifecycle 开仓消费 → 首子单报价）。`write_back=false` 为读面模式
    /// （EchoOpenDerivation——纯观察：组态/簿不演进、无 undo、幂等）；
    /// 生产路径（run_round due 分流）`write_back=true`（个人态与簿创建
    /// 均入可逆日志——Abort 恢复，对齐 session 版"个人候选私有、失败
    /// 不 install"的失败原子性）。
    fn run_chain_decision(
        &mut self,
        local: LocalIdx,
        tick: u64,
        write_back: bool,
    ) -> Result<
        (
            super::world::OpenDerivationRecord,
            Vec<super::decision::PendingOp>,
        ),
        RuntimeError,
    > {
        let group = self.group;
        let (projection, snapshot) = match (
            self.root_projection.clone(),
            self.snapshot.clone(),
        ) {
            (Some(p), Some(s)) => (p, s),
            _ => {
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!(
                        "组 {group} belief 位 {local} 决策链缺投影/快照（无会话来源或未跑 tick）"
                    ),
                })
            }
        };
        let Some(&account) = projection.account_ids.get(local as usize) else {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("组 {group} belief 位 {local} 无名册 AccountId"),
            });
        };
        let Some(snap) = self.belief_states.get(local as usize).cloned().flatten() else {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("组 {group} 位 {local} 缺信念个人态"),
            });
        };
        let context = build_root_read_context(
            &self.accounts,
            &self.belief_states,
            &projection,
            &self.library,
            &self.plan_book,
            snapshot.tick,
            self.ticks_per_day,
            &snapshot,
        );
        let observation = build_chain_observation(
            &self.observation_inputs,
            &snapshot,
            self.ticks_per_day,
            projection.auction_ticks,
            projection.closing_auction_ticks,
        );
        let personal = crate::session::PlanPersonalState {
            attention: snap.attention.clone(),
            watchlist: snap.watchlist.clone(),
            price_memory: snap.price_memory.clone(),
            history_reads: snap.history_reads.clone(),
            information: snap.information.clone(),
            belief: snap.belief.clone(),
        };
        let personal_in = personal.clone();
        let (_account, out, operations, _diagnostics) =
            crate::session::InstitutionDecisionRoot::new(account, personal)
                .observe(&context, &observation);
        let assessments = operations
            .lifecycle_assessments()
            .into_iter()
            .find(|(owner, _)| *owner == account)
            .map(|(_, by_code)| by_code.into_iter().collect::<Vec<_>>())
            .unwrap_or_default();
        // Lifecycle 开仓消费（共享 open_position_action——active 分流：已
        // 活跃的股不重复 Create；Active 复审=D3b 登记暂缓）。
        let (params, style) = projection
            .chain_params
            .get(local as usize)
            .and_then(|slot| *slot)
            .unwrap_or_else(|| panic!("组 {group} belief 位 {local} 缺链参数投影"));
        let mut opens = Vec::new();
        // Sol 整改 1：读面（write_back=false）不消费开仓——纯观察（此前
        // 仅跳过 undo 快照仍 create，第二次读取因 active 分流产生不同
        // 结果——违反只读/幂等承诺，红证 solmid_echo_* 显红）。
        let consume_open = write_back;
        for (code, assessment) in &assessments {
            if !consume_open || self.plan_book.active_plan(account, code).is_some() {
                continue;
            }
            let Some(group_account) = self.accounts.get(local as usize) else {
                continue;
            };
            let held_qty = group_account.shares[code.0 as usize];
            let price = observation
                .market
                .stocks
                .get(code)
                .map(|view| view.last_price)
                .unwrap_or_else(|| panic!("组内开仓消费缺股 {code:?} 市场视图"));
            let score = match assessment {
                crate::plans::CandidateAssessment::Scored { score, .. } => Some(*score),
                crate::plans::CandidateAssessment::InsufficientInformation { .. } => None,
            };
            let weights = out.belief.analysis().weights();
            let fundamental_ready = weights.fundamental_bp() == 0
                || out.belief.entry(code).is_some_and(|entry| {
                    matches!(
                        entry.valuation,
                        crate::strategy::ValuationOutcome::Available { .. }
                    )
                });
            let open = crate::session::open_position_action(
                account,
                code,
                price,
                held_qty,
                equity_cents(group_account, &observation.market),
                score,
                fundamental_ready,
                &out.belief,
                self.plan_book.policy().reverse_revision_threshold_bp,
                &params,
                style,
                phase_is_continuous(
                    snapshot.tick,
                    self.ticks_per_day,
                    projection.auction_ticks,
                    projection.closing_auction_ticks,
                ),
                snapshot.tick / self.ticks_per_day,
                self.config.lot_size,
                projection.total_shares.get(code).copied().unwrap_or(0),
                |direction, confidence_bp, horizon| {
                    initial_group_urgency(
                        direction,
                        confidence_bp,
                        horizon,
                        style,
                        &projection.urgency_policy,
                    )
                },
            );
            if let Some(open) = open {
                self.save_plan_book_undo();
                self.plan_book
                    .create(open.clone())
                    .unwrap_or_else(|error| {
                        panic!("组内计划创建失败 {account:?} {code:?}: {error}")
                    });
                opens.push(open);
            }
        }
        // 首子单报价（D3a-3 件4b——生产路径；读面不算子单）。
        let child_ops = if write_back {
            self.chain_quote_children(local, account, &projection, &out, &snapshot)?
        } else {
            Vec::new()
        };
        // 组态写回（生产路径：个人态六件入可逆日志后演进）。
        if write_back {
            self.undo.push(GroupUndo::Personal {
                local,
                prev: Box::new(snap),
            });
            if let Some(slot) = self.belief_states.get_mut(local as usize) {
                *slot = Some(super::world::BeliefParticipantSnapshot {
                    watchlist: out.watchlist.clone(),
                    price_memory: out.price_memory.clone(),
                    information: out.information.clone(),
                    belief: out.belief.clone(),
                    attention: out.attention.clone(),
                    history_reads: out.history_reads.clone(),
                });
            }
        }
        let _ = tick;
        Ok((
            super::world::OpenDerivationRecord {
                account,
                personal_in,
                personal_out: out,
                assessments,
                opens,
            },
            child_ops,
        ))
    }

    /// Sol 整改 1：簿变更前的可逆快照（本 tick 首次变更保存；后续复用）。
    fn save_plan_book_undo(&mut self) {
        if !self.plan_undo_saved {
            self.undo.push(GroupUndo::PlanBook {
                prev: Box::new(self.plan_book.clone()),
            });
            self.plan_undo_saved = true;
        }
    }

    /// D3a-3 件4b：账户级首子单报价（预算+报价 → SubmitLimit →
    /// PendingOp 并入 run_round 扣除路径）。既有在途子单的计划跳过
    /// （remaining 校验/成交推进=D3a-3 后半）。
    ///
    /// 登记差异（D3b 统一收口）：首子单的 urgency 输入=创建时 initial
    /// （session 版报价前重估——含风险观察/behavior 的完整面提取随 D3b
    /// urgency 域）；pause=Unavailable（无暂停判定）。frozen_cash=0
    /// （组内所有权模型：现金已扣在途——保守预算）。
    fn chain_quote_children(
        &mut self,
        local: LocalIdx,
        account: crate::AccountId,
        projection: &std::sync::Arc<super::world::RootProjection>,
        personal: &crate::session::PlanPersonalState,
        snapshot: &std::sync::Arc<TickSnapshot>,
    ) -> Result<Vec<super::decision::PendingOp>, RuntimeError> {
        let group = self.group;
        let mut ops = Vec::new();
        let plan_ids = self.plan_book.active_plan_ids_for_account(account);
        if plan_ids.is_empty() {
            return Ok(ops);
        }
        let Some(group_account) = self.accounts.get(local as usize) else {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("组 {group} belief 位 {local} 账户缺失"),
            });
        };
        let (params, _style) = projection
            .chain_params
            .get(local as usize)
            .and_then(|slot| *slot)
            .unwrap_or_else(|| panic!("组 {group} belief 位 {local} 缺链参数投影"));
        let cash = crate::money::Money::from_cents(group_account.cash);
        let existing: std::collections::BTreeSet<crate::plans::PlanId> =
            plan_ids.iter().copied().collect();
        // 软预算请求（与 session prepare_classified_plan_quotes 同源纯
        // 函数：买入按涨停预留、卖出按可卖量收敛）。
        let mut requests = Vec::new();
        for &plan_id in &plan_ids {
            let plan = self
                .plan_book
                .plan(plan_id)
                .unwrap_or_else(|error| panic!("组内计划读取失败 {plan_id:?}: {error}"));
            if !matches!(plan.status(), crate::plans::PlanStatus::Active)
                || self
                    .child_in_flight
                    .contains_key(&(account, plan.code().clone()))
            {
                continue;
            }
            let Some(remaining) = plan.remaining_share_qty() else {
                continue;
            };
            let code = plan.code();
            let stock_idx = code.0 as usize;
            let view = snapshot
                .stocks
                .get(stock_idx)
                .unwrap_or_else(|| panic!("组内报价缺股 {code:?} 公共态"));
            let max_order_qty = projection
                .max_order_qty_limit
                .get(code)
                .copied()
                .unwrap_or(0);
            let request = match plan.direction() {
                crate::orderbook::Side::Buy => {
                    // 涨停价预留（session 用 market.up_stop()——公共态
                    // daily_upper_limit 同值）。
                    let reservation_price = crate::money::Money::from_cents(
                        view.daily_upper_limit.max(1),
                    );
                    crate::session::buy_allocation_request(
                        &self.config,
                        plan_id,
                        code.clone(),
                        plan.confidence_bp(),
                        reservation_price,
                        remaining,
                        params.order_size,
                        self.config.lot_size,
                        max_order_qty,
                    )
                }
                crate::orderbook::Side::Sell => {
                    let sellable = group_account.sellable(stock_idx as StockIdx);
                    let qty = crate::session::next_routable_sell_qty(
                        remaining,
                        sellable,
                        self.config.lot_size,
                        max_order_qty,
                    );
                    qty.map(|qty| crate::plans::AllocationRequest {
                        plan_id,
                        code: code.clone(),
                        side: crate::orderbook::Side::Sell,
                        class: crate::plans::AllocationClass::RiskReduction,
                        confidence_bp: plan.confidence_bp(),
                        requested_cash: crate::money::Money::ZERO,
                        fee_reserve: crate::money::Money::ZERO,
                        requested_sell_qty: qty,
                        sellable_qty: sellable,
                        experience: crate::plans::AllocationExperience::default(),
                    })
                }
            };
            if let Some(mut request) = request {
                // 分类（预算优先级）：卖出=减仓优先；买入=既有计划/新机会。
                request.class = if request.side == crate::orderbook::Side::Sell {
                    crate::plans::AllocationClass::RiskReduction
                } else if existing.contains(&request.plan_id) {
                    crate::plans::AllocationClass::ExistingPlan
                } else {
                    crate::plans::AllocationClass::NewOpportunity
                };
                requests.push(request);
            }
        }
        let grants = if requests.is_empty() {
            None
        } else {
            Some(
                crate::plans::allocate_child_quote_budgets(
                    &crate::plans::AllocationFunds {
                        cash,
                        frozen_cash: crate::money::Money::ZERO,
                    },
                    &requests,
                    self.config.commission_min,
                )
                .unwrap_or_else(|error| panic!("组内预算分配失败 {account:?}: {error}")),
            )
        };
        // 逐计划报价（共享 decide_plan_quote——与 session 版同一函数）。
        let mut submits: Vec<(
            crate::plans::PlanId,
            crate::StockId,
            StockIdx,
            crate::orderbook::Side,
            crate::money::Money,
            u32,
        )> = Vec::new();
        for &plan_id in &plan_ids {
            let plan = self
                .plan_book
                .plan(plan_id)
                .unwrap_or_else(|error| panic!("组内计划读取失败 {plan_id:?}: {error}"));
            if !matches!(plan.status(), crate::plans::PlanStatus::Active)
                || self
                    .child_in_flight
                    .contains_key(&(account, plan.code().clone()))
            {
                continue;
            }
            let Some(remaining) = plan.remaining_share_qty() else {
                continue;
            };
            let Some(grant) = grants
                .as_ref()
                .and_then(|result| result.grants.iter().find(|g| g.plan_id == plan_id))
                .cloned()
            else {
                continue;
            };
            let code = plan.code();
            let stock_idx = code.0 as usize;
            let view = snapshot
                .stocks
                .get(stock_idx)
                .unwrap_or_else(|| panic!("组内报价缺股 {code:?} 公共态"));
            let per_share = personal.belief.entry(code).and_then(|entry| {
                match &entry.valuation {
                    crate::strategy::ValuationOutcome::Available { per_share, .. } => {
                        Some(*per_share)
                    }
                    crate::strategy::ValuationOutcome::Unavailable { .. } => None,
                }
            });
            let max_order_qty = projection
                .max_order_qty_limit
                .get(code)
                .copied()
                .unwrap_or(0);
            let decision = crate::session::decide_plan_quote(crate::session::PlanQuoteInputs {
                account,
                plan,
                price: crate::money::Money::from_cents(view.last_price),
                best_bid: view.best_bid.map(crate::money::Money::from_cents),
                best_ask: view.best_ask.map(crate::money::Money::from_cents),
                auction_book_fallback: !phase_is_continuous(
                    snapshot.tick,
                    self.ticks_per_day,
                    projection.auction_ticks,
                    projection.closing_auction_ticks,
                ),
                band_up: crate::money::Money::from_cents(view.daily_upper_limit.max(1)),
                band_down: crate::money::Money::from_cents(view.min_sell_price.max(1)),
                legal_bound: match plan.direction() {
                    crate::orderbook::Side::Buy => {
                        crate::money::Money::from_cents(view.max_buy_price.max(1))
                    }
                    crate::orderbook::Side::Sell => {
                        crate::money::Money::from_cents(view.min_sell_price.max(1))
                    }
                },
                // 价格笼子：与 session 同判（连续相位 ∧ config 开启）——
                // decide_quote 的笼子分支（chase 限制）需要信号（复核
                // 发现收口：legal_bound 已含交集但信号不得丢）。
                apply_price_cage: phase_is_continuous(
                    snapshot.tick,
                    self.ticks_per_day,
                    projection.auction_ticks,
                    projection.closing_auction_ticks,
                ) && self.config.price_cage_enabled,
                per_share: per_share.as_ref(),
                urgency: plan.urgency(),
                pause: crate::plans::PauseAssessment::Unavailable {
                    reason: "group first-child quote keeps the initial urgency (D3b
 收口:报价前重估含风险观察/behavior)"
                        .to_owned(),
                },
                remaining,
                order_size: params.order_size,
                lot: self.config.lot_size,
                max_order_qty,
                sellable: group_account.sellable(stock_idx as StockIdx),
                active_child: None,
                cancellable_now: phase_is_continuous(
                    snapshot.tick,
                    self.ticks_per_day,
                    projection.auction_ticks,
                    projection.closing_auction_ticks,
                ),
                tick_size: crate::money::Money::from_cents(
                    projection.tick_size_cents.get(code).copied().unwrap_or(1),
                ),
                allocation_cash: grant.allocated_cash,
                config: &self.config,
            });
            let Some(decision) = decision else {
                continue;
            };
            if let crate::plans::QuoteAction::Submit { price, qty } = decision.action {
                if qty == 0 {
                    continue;
                }
                submits.push((
                    plan_id,
                    code.clone(),
                    stock_idx as StockIdx,
                    plan.direction(),
                    price,
                    qty,
                ));
            }
        }
        // 执行腿登记（借用外——D3a-3：挂出即登记，execution 腿在=后续轮
        // 报价跳过——同计划不重复挂子单；remaining 校验/成交推进见
        // apply_staging 的 fills 段）。expires 近似=当日市场分钟上限
        //（挂单 TTL 域登记——撤销/到期释放的腿清理随撤销路径补）。
        for (plan_id, code, stock, side, price, qty) in submits {
            let prev = self.child_in_flight.insert(
                (account, code.clone()),
                ChildTicket { qty, order_id: None },
            );
            // 防御：同键重复置入=上游跳过条件失效（显式失败，铁律二）。
            assert!(
                prev.is_none(),
                "组内子单在途票重复置入：{account:?} {code:?}"
            );
            self.undo.push(GroupUndo::ChildInFlight {
                account,
                code: code.clone(),
                prev: None,
            });
            // Sol 整改 1：簿快照覆盖全部簿变更（install/record 亦然）。
            self.save_plan_book_undo();
            self.plan_book
                .install_execution(
                    plan_id,
                    qty,
                    price,
                    u64::from(crate::observation::GAME_INTRADAY_MINUTES_PER_DAY),
                )
                .unwrap_or_else(|error| {
                    panic!("组内执行腿登记失败 {account:?} {code:?}: {error}")
                });
            ops.push(super::decision::PendingOp {
                local,
                stock,
                side,
                routed: RoutedPrice::Fixed(price.cents()),
                purse_reference: price.cents(),
                qty,
            });
        }
        Ok(ops)
    }

    /// 执行一轮：脚本计划 → 实体扣除（记逆操作）→ 分段 flush → GroupReport。
    fn run_round(
        &mut self,
        tick: u64,
        round: u32,
        desk_txs: &[Sender<super::protocol::DeskMsg>],
        up_tx: &Sender<UpMsg>,
    ) -> Result<(), RuntimeError> {
        // 决策来源：内联 decide_data（批 4）或测试脚本。
        enum Pending {
            Op(PendingOp),
            CancelAll { local: LocalIdx, stock: StockIdx },
        }
        // D4 相位计时：决策段（decide_data 内联/测试脚本产计划）。
        crate::verification_evidence::enter_phase(
            crate::session::pipeline::TickPhase::DecisionAndContinuation,
        );
        let mut pendings: Vec<Pending> = Vec::new();
        // D3a-3：profiles 借用期内的 next_due 重排待办（undo 入账延后到
        // 借用释放——Sol 二审修复集 5 语义不变）。
        let mut pending_next_due: Vec<(LocalIdx, u64)> = Vec::new();
        let mut continuation = false;
        let mut due_npcs = 0u64;
        let mut decide_calls = 0u64;
        let mut cancel_intents_skipped = 0u64;
        let mut panic_payload: Option<String> = None;
        let mut stall: Option<Duration> = None;
        if let Some((profiles, gap)) = &mut self.decision {
            // Continue 轮跳过普通 due 筛选（F §4.3）——只推进计划链；
            // 测试脚本路径不受限（脚本按轮定义行为，是续行注入面）。
            if round > 0 {
                let _ = (profiles, gap);
            } else {
                let snapshot =
                    self.snapshot
                        .clone()
                        .ok_or_else(|| RuntimeError::ProtocolInvariant {
                            detail: format!(
                                "组 {} 决策缺快照（Start 未达即 Continue？）",
                                self.group
                            ),
                        })?;
                let group_mix = (self.group as u64).wrapping_mul(0x9E3779B9);
                // D3a-3：belief 位的决策链轮（observe 直调——方向 A 深移植）
                // 与 retail 的 decide_data 轮分离：due 判定+重排在 profiles
                // 借用内完成（Sol 二审修复集 5 的 undo 语义），observe 链
                // 在借用外执行（组态/簿的演进借用互斥）。
                let mut chain_due: Vec<LocalIdx> = Vec::new();
                {
                    for local in 0..profiles.len() as LocalIdx {
                        let chain_local = self.root_projection.is_some()
                            && self
                                .belief_states
                                .get(local as usize)
                                .is_some_and(|slot| slot.is_some());
                        if chain_local {
                            if tick < profiles[local as usize].next_due {
                                continue;
                            }
                            let prev_due = profiles[local as usize].next_due;
                            super::decision::reschedule_due(
                                &mut profiles[local as usize],
                                group_mix,
                                tick,
                                local,
                                *gap,
                            );
                            due_npcs += 1;
                            decide_calls += 1;
                            chain_due.push(local);
                            // undo 与计数在借用内不便（&mut self 冲突）——
                            // 转为待办：observe 轮统一入账。
                            pending_next_due.push((local, prev_due));
                            continue;
                        }
                        let account = &self.accounts[local as usize];
                        let prev_due = profiles[local as usize].next_due;
                        let (ops, decided, cancels) = decide_account(
                            &mut profiles[local as usize],
                            group_mix,
                            tick,
                            local,
                            *gap,
                            &snapshot,
                            account,
                            &self.config,
                        );
                        if decided {
                            due_npcs += 1;
                            decide_calls += 1;
                            pending_next_due.push((local, prev_due));
                        }
                        cancel_intents_skipped += cancels as u64;
                        self.cancel_intents_total += cancels as u64;
                        for op in ops {
                            pendings.push(Pending::Op(op));
                        }
                    }
                }
                // retail 的 undo 入账 + belief 位 observe 链（借用已释放）。
                for (local, prev_due) in std::mem::take(&mut pending_next_due) {
                    self.decide_calls_total += 1;
                    self.undo.push(GroupUndo::NextDue { local, prev: prev_due });
                }
                for local in chain_due {
                    let (_record, child_ops) = self.run_chain_decision(local, tick, true)?;
                    for op in child_ops {
                        pendings.push(Pending::Op(op));
                    }
                }
            }
        } else {
            let plan = (self.script)(tick, round);
            panic_payload = plan.panic_payload;
            stall = plan.stall;
            continuation = plan.continuation;
            for c in &plan.cancels {
                pendings.push(Pending::CancelAll {
                    local: c.local,
                    stock: c.stock,
                });
            }
            for p in &plan.places {
                pendings.push(Pending::Op(super::decision::PendingOp {
                    local: p.local,
                    stock: p.stock,
                    side: p.side,
                    routed: RoutedPrice::Fixed(p.price),
                    purse_reference: p.price,
                    qty: p.qty,
                }));
            }
        }
        // 外部注入计划（玩家意图路由，D2）：并入扣除路径（一次消费即清）。
        let external = std::mem::take(&mut self.external_pending);
        for op in external {
            pendings.push(Pending::Op(op));
        }
        // 计划链推进（批 6）：轮 0=根命令（空结果输入）；轮 k=按本 tick 已见
        // FillFact 推进（Continue 轮先落账再续行——staging 已在 handle_continue
        // 应用，plan_fills 随落账收集）。跨股产物并入同一扣除路径。
        let mut plans_pending = false;
        let fills = std::mem::take(&mut self.plan_fills);
        for plan in self.plans.iter_mut() {
            let step = plan.next_step(&fills);
            for op in step.ops {
                pendings.push(Pending::Op(op));
            }
            plans_pending |= step.has_more_steps;
        }
        continuation |= plans_pending;
        if let Some(payload) = panic_payload {
            std::panic::panic_any(payload);
        }
        if let Some(stall) = stall {
            std::thread::sleep(stall);
        }
        let group = self.group;
        let n_stocks = self.accounts.first().map(|a| a.shares.len()).unwrap_or(0);
        let mut buffers: Vec<Vec<DeskIn>> = vec![Vec::new(); n_stocks];
        let mut buffered = 0usize;
        let mut ops_sent = 0u64;
        // D4 相位计时：账户校验段（purse/持仓扣除+在途登记）。
        crate::verification_evidence::enter_phase(
            crate::session::pipeline::TickPhase::AccountValidation,
        );
        // 对齐撤单先于下单（同批次 FIFO；脚本顺序保证）。
        for c in pendings.iter().filter_map(|p| match p {
            Pending::CancelAll { local, stock } => Some((*local, *stock)),
            _ => None,
        }) {
            let active = self.accounts[c.0 as usize].foreign_active[c.1 as usize];
            if active > 0 {
                buffers[c.1 as usize].push(DeskIn::CancelAll {
                    owner: (group, c.0),
                });
                buffered += 1;
            }
        }
        for p in pendings.iter().filter_map(|p| match p {
            Pending::Op(op) => Some(*op),
            _ => None,
        }) {
            let owner = (group, p.local);
            match p.side {
                Side::Buy => {
                    // purse = 最坏情况对价 + 佣金上限 + 过户费上限（F §1.5；
                    // `fees.rs buy_purse_limit`，与现引擎预留同口径。符号价/市价
                    // 的最坏情况 = 日涨停（`purse_reference`，F §1.5）。
                    let purse = buy_purse_limit(
                        &self.config,
                        Money::from_cents(p.purse_reference.max(1)),
                        p.qty,
                        Money::ZERO,
                    )
                    .map_err(|e| RuntimeError::ProtocolInvariant {
                        detail: format!("组 {group} purse 计算失败：{e:?}"),
                    })?
                    .cents();
                    let acct = self.acct_mut(p.local)?;
                    if acct.cash < purse {
                        // 严格所有权：在途退款未到账 ⇒ 显式 skip 计数（不静默、不部分执行）。
                        self.skipped_orders += 1;
                        continue;
                    }
                    acct.cash -= purse;
                    acct.foreign_active[p.stock as usize] += 1;
                    self.undo.push(GroupUndo::Cash {
                        local: p.local,
                        delta: -purse,
                    });
                    self.undo.push(GroupUndo::ForeignActive {
                        local: p.local,
                        stock: p.stock,
                        delta: 1,
                    });
                    buffers[p.stock as usize].push(DeskIn::Place {
                        owner,
                        side: p.side,
                        price: p.routed,
                        qty: p.qty,
                        purse,
                    });
                    buffered += 1;
                }
                Side::Sell => {
                    let lot = self.lot;
                    let acct = self.acct_mut(p.local)?;
                    let sellable = acct.sellable(p.stock);
                    if p.qty > sellable {
                        // 含 T+1：当日买入不可卖。
                        self.skipped_orders += 1;
                        continue;
                    }
                    // 零股须一次性卖出：非整手数量只允许恰好等于全部可卖零头。
                    if p.qty % lot != 0 && p.qty != sellable {
                        self.skipped_orders += 1;
                        continue;
                    }
                    // D3a-3 收口：卖出扣除后持仓归零=所有权已转移（挂单在
                    // 途）——session 条目保留（曾持有），组侧置曾持有位
                    // （观察面 held 集等价；undo 冲回——入账延后到借用
                    // 释放）。
                    let mut ever_flip: Option<bool> = None;
                    {
                        let slot = &mut acct.shares[p.stock as usize];
                        *slot -= p.qty;
                        let now_ever = *slot == 0;
                        let prev_ever = acct.ever_held[p.stock as usize];
                        if prev_ever != now_ever {
                            acct.ever_held[p.stock as usize] = now_ever;
                            ever_flip = Some(prev_ever);
                        }
                    }
                    acct.foreign_active[p.stock as usize] += 1;
                    self.undo.push(GroupUndo::Shares {
                        local: p.local,
                        stock: p.stock,
                        delta: -(p.qty as i32),
                    });
                    self.undo.push(GroupUndo::ForeignActive {
                        local: p.local,
                        stock: p.stock,
                        delta: 1,
                    });
                    if let Some(prev) = ever_flip {
                        self.undo.push(GroupUndo::EverHeld {
                            local: p.local,
                            stock: p.stock,
                            prev,
                        });
                    }
                    buffers[p.stock as usize].push(DeskIn::Place {
                        owner,
                        side: p.side,
                        price: p.routed,
                        qty: p.qty,
                        purse: 0,
                    });
                    buffered += 1;
                }
            }
            if buffered >= self.chunk {
                ops_sent += flush_buffers(&mut buffers, desk_txs, tick, round, group, false)?;
                buffered = 0;
            }
        }
        // 名册末尾：每股发 final_flush 空批作为本组本轮提交标记（可空）。
        ops_sent += flush_buffers(&mut buffers, desk_txs, tick, round, group, true)?;
        let report = GroupReport {
            group,
            tick,
            round,
            ops_sent,
            skipped: self.skipped_orders,
            continuation,
            committed_hash: self.committed_hash,
            due_npcs,
            decide_calls,
            cancel_intents_skipped,
        };
        up_tx
            .send(UpMsg::GroupReport(report))
            .map_err(|_| RuntimeError::SendFailed {
                detail: format!("组 {group} GroupReport 发送失败"),
            })
    }

    /// 期末上交。
    fn final_state(&self) -> GroupFinalState {
        GroupFinalState {
            group: self.group,
            accounts: self.accounts.clone(),
            skipped_orders: self.skipped_orders,
            decide_calls_total: self.decide_calls_total,
            cancel_intents_total: self.cancel_intents_total,
            state_hash: self.state_hash(),
            committed_hash: self.committed_hash,
            observed: self.observed.clone(),
        }
    }
}

/// 组账户权益（现金+持仓×最新价——与 observe_personal 的 equity 口径同式：
/// 溢出/缺视图 panic，不静默）。
fn equity_cents(
    account: &GroupAccount,
    market: &crate::strategy::MarketView,
) -> crate::money::Money {
    let mut equity = crate::money::Money::from_cents(account.cash);
    for (stock, &qty) in account.shares.iter().enumerate() {
        if qty == 0 {
            continue;
        }
        let price = market
            .stocks
            .get(&crate::StockId(stock as u32))
            .map(|view| view.last_price)
            .unwrap_or_else(|| {
                panic!("组内权益计算缺股 {stock:?} 市场视图");
            });
        equity = equity
            .add(price.mul_shares(qty).unwrap_or_else(|error| {
                panic!("组内权益持仓估值失败 {stock:?}: {error:?}");
            }))
            .unwrap_or_else(|error| panic!("组内权益累加失败：{error:?}"));
    }
    equity
}

/// 相位判定（与 session `phase()` 同式：申报窗=竞价窗前 2/3、盘前=竞价窗
/// 余量、收盘窗=尾部——Continuous ⟺ 连续段内）。
fn phase_is_continuous(
    tick: u64,
    ticks_per_day: u64,
    auction_ticks: u64,
    closing_ticks: u64,
) -> bool {
    let day_tick = tick % ticks_per_day;
    let closing_start = ticks_per_day.saturating_sub(closing_ticks);
    day_tick >= auction_ticks && day_tick < closing_start
}

/// 交易日序号（tick/tpd——与 RootReadContext 组装同式）。
fn context_day(tick: u64, ticks_per_day: u64) -> u64 {
    tick / ticks_per_day
}

/// 整手（世界配置——与 session setup.config.lot_size 同源投影）。
fn self_lot(config: &std::sync::Arc<crate::config::GameConfig>) -> u32 {
    config.lot_size
}

/// 组内初始紧迫度（与 session `initial_plan_urgency` 同一 `assess_urgency`
/// 输入面——风格/策略为构造期投影）。
fn initial_group_urgency(
    direction: crate::orderbook::Side,
    confidence_bp: u32,
    horizon_trading_days: u32,
    style: Option<crate::strategy::InstitutionStyle>,
    policy: &crate::plans::UrgencyPolicy,
) -> crate::plans::Urgency {
    use crate::plans::{assess_urgency, PatienceStyle, UrgencyInputs};
    let inputs = UrgencyInputs {
        side: direction,
        return_30min_bp: None,
        return_1min_bp: None,
        risk_pressure_pause: false,
        adverse_selection_pause: false,
        risk_reduction_active: false,
        account_drawdown_bp: None,
        remaining_trading_days: horizon_trading_days.saturating_sub(1),
        confidence_bp,
        style: match style {
            Some(crate::strategy::InstitutionStyle::DeepValue) => PatienceStyle::DeepValue,
            _ => PatienceStyle::Other,
        },
    };
    assess_urgency(&inputs, policy)
        .unwrap_or_else(|error| panic!("组内初始紧迫度失败：{error:?}"))
        .urgency
}

/// D3a-2c 件2：组线程组装 RootReadContext（与 `RootReadContext::capture`
/// 同一字段面——投影来源同源：组账户→AccountBook 保真投影（现金/持仓/
/// T+1 冻结/成本基）、catalog/registry/operations 构造期 Arc、library=组内
/// 共享、plans=组内空簿（runtime 模式 session plans 恒空）、时间面=快照
/// tick 推导 + 快照 civil_date）。任何组装分叉在红证（组内派生==session
/// 版派生）显红。
#[allow(clippy::too_many_arguments)]
fn build_root_read_context(
    accounts: &[GroupAccount],
    belief_states: &[Option<super::world::BeliefParticipantSnapshot>],
    projection: &std::sync::Arc<super::world::RootProjection>,
    library: &std::sync::Arc<crate::information::PublicLibrary>,
    plans: &crate::plans::PlanBook,
    tick: u64,
    ticks_per_day: u64,
    snapshot: &TickSnapshot,
) -> crate::session::RootReadContext {
    let mut book = crate::session::AccountBook::default();
    for (local, slot) in belief_states.iter().enumerate() {
        if slot.is_none() {
            continue;
        }
        let Some(&account_id) = projection.account_ids.get(local) else {
            continue;
        };
        let group_account = &accounts[local];
        let mut account = crate::account::Account::new(
            account_id,
            crate::account::AccountKind::Inst,
            crate::money::Money::from_cents(group_account.cash),
        );
        let mut positions = std::collections::BTreeMap::new();
        for (stock, &qty) in group_account.shares.iter().enumerate() {
            // 零持仓仅在曾持有时建条目（D3a-3 收口：对齐 session 清仓保留
            // 零量条目——held 集成员参与发现采样；qty=0 时派生面
            // （cost_price=None）等价，成本基数值无消费者）。
            if qty == 0 && !group_account.ever_held[stock] {
                continue;
            }
            positions.insert(
                crate::StockId(stock as u32),
                crate::account::Position::from_restored_parts(
                    qty,
                    group_account.frozen_today[stock],
                    group_account.invested_cents[stock],
                    group_account.recovered_cents[stock],
                ),
            );
        }
        account.fixture_set_positions(positions);
        book.insert(account_id, account);
    }
    let day = u32::try_from(tick / ticks_per_day).expect("交易日序号超 u32");
    let day_tick = tick % ticks_per_day;
    let continuous = ticks_per_day
        .saturating_sub(projection.auction_ticks)
        .saturating_sub(projection.closing_auction_ticks);
    // 市场分钟：与 session `current_market_minute` 同式（交易日×240 +
    // 连续段完成分钟——completed_market_minute_count 为共享纯函数）。
    let market_minute = u64::from(day) * u64::from(crate::observation::GAME_INTRADAY_MINUTES_PER_DAY)
        + u64::from(
            crate::observation::completed_market_minute_count(
                day_tick
                    .saturating_sub(projection.auction_ticks)
                    .min(continuous),
                continuous,
            )
            .expect("相位配置映射市场分钟（与 session 同式）"),
        );
    crate::session::RootReadContext::from_parts(
        book,
        projection.catalog.clone(),
        std::sync::Arc::clone(&projection.company_registry),
        std::sync::Arc::clone(&projection.operations),
        std::sync::Arc::clone(library),
        // 组内计划簿（D3a-2c 件4 起承载开仓产物——与 session 投影同源；
        // 此前恒空簿的起点形态在件4 落地后退役）。
        plans.clone(),
        snapshot.civil_date,
        market_minute,
        day,
    )
}

/// D3a-2c 件2：组线程组装 DecisionChainObservation（market=广播快照视图、
/// paths/technical=组内链观察纯构建器（D3a-2 同源）、now=共享秒映射、
/// exposed=空集起步——公司披露曝光面登记差异，随披露载荷补）。
fn build_chain_observation(
    inputs: &[ObservationInputs],
    snapshot: &TickSnapshot,
    ticks_per_day: u64,
    auction_ticks: u64,
    closing_auction_ticks: u64,
) -> crate::session::DecisionChainObservation {
    let market = super::decision::build_market_view(snapshot);
    let mut paths = std::collections::BTreeMap::new();
    let mut technical = std::collections::BTreeMap::new();
    for (stock, inputs) in inputs.iter().enumerate() {
        let (path, tech) = inputs.build_observations();
        paths.insert(crate::StockId(stock as u32), path);
        technical.insert(crate::StockId(stock as u32), tech);
    }
    let day_tick = snapshot.tick % ticks_per_day;
    let second = crate::session::intraday_observation_second(
        day_tick,
        ticks_per_day,
        auction_ticks,
        closing_auction_ticks,
    );
    let now = crate::calendar::CivilInstant::new(snapshot.civil_date, second)
        .expect("快照自然日与日内秒构成合法 CivilInstant");
    crate::session::DecisionChainObservation {
        market,
        paths,
        technical,
        now,
        exposed: std::collections::BTreeSet::new(),
    }
}

/// 分段 flush：非空缓冲直发目标交易部；`final` 时每股必发（空批=提交标记）。
fn flush_buffers(
    buffers: &mut [Vec<DeskIn>],
    desk_txs: &[Sender<super::protocol::DeskMsg>],
    tick: u64,
    round: u32,
    group: GroupIdx,
    final_flush: bool,
) -> Result<u64, RuntimeError> {
    let mut sent = 0u64;
    for (stock, buf) in buffers.iter_mut().enumerate() {
        if buf.is_empty() && !final_flush {
            continue;
        }
        let ops = std::mem::take(buf);
        sent += ops.len() as u64;
        desk_txs[stock]
            .send(super::protocol::DeskMsg::Ops {
                tick,
                round,
                group,
                ops,
                final_flush,
            })
            .map_err(|_| RuntimeError::SendFailed {
                detail: format!("组 {group} → 交易部 {stock} Ops 发送失败"),
            })?;
    }
    Ok(sent)
}

/// FNV-1a 混合（数值直传）。
fn fnv(state: u64, value: u64) -> u64 {
    let mut h = state;
    for b in value.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 组线程主循环（骨架）。panic 运输：处理体 `catch_unwind` 捕获 → 回滚自身
/// overlay → `TaskDied`（原 payload）→ Final 上交 → 退出。
pub fn group_main(
    state: GroupState,
    desk_txs: Vec<Sender<super::protocol::DeskMsg>>,
    rx: Receiver<GroupIn>,
    up_tx: Sender<UpMsg>,
    abort: Arc<AtomicBool>,
    watchdog: WatchdogConfig,
    capture: Arc<std::sync::Mutex<crate::verification_evidence::PhaseTimingBinding>>,
) -> Result<(), RuntimeError> {
    /// D4：每条消息处理期安装当前捕获绑定（无捕获=None 零开销；完成
    /// 信号在作用域退出（绑定释放）后发出——沿用旧协议保证）。
    fn with_capture<R>(
        capture: &Arc<std::sync::Mutex<crate::verification_evidence::PhaseTimingBinding>>,
        work: impl FnOnce() -> R,
    ) -> R {
        let binding = capture
            .lock()
            .expect("capture 槽锁被 poison")
            .clone();
        crate::verification_evidence::with_phase_timing_binding(binding, work)
    }
    let who = WorkerId::Group(state.group);
    let mut state = state;
    let mut result = Ok(());
    loop {
        let msg = match worker_recv(&rx, &watchdog, &abort, who) {
            Ok(Some(m)) => m,
            Ok(None) => break,
            Err(e) => {
                abort.store(true, Ordering::SeqCst);
                let _ = up_tx.send(UpMsg::Failed {
                    who,
                    error: e.clone(),
                });
                result = Err(e);
                break;
            }
        };
        match msg {
            GroupIn::Batch(batch) => {
                state.observed.push(Observed::Batch(batch.tick));
                state.staging.push(batch);
            }
            GroupIn::Start { tick, snapshot } => {
                state.observed.push(Observed::Start(tick));
                if snapshot.tick != tick {
                    abort.store(true, Ordering::SeqCst);
                    let e = RuntimeError::ProtocolInvariant {
                        detail: format!("组 {} Start 快照 tick 不一致", state.group),
                    };
                    let _ = up_tx.send(UpMsg::Failed {
                        who,
                        error: e.clone(),
                    });
                    result = Err(e);
                    break;
                }
                state.snapshot = Some(snapshot);
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    with_capture(&capture, || state.handle_start(tick, &desk_txs, &up_tx))
                }));
                match outcome {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        abort.store(true, Ordering::SeqCst);
                        let _ = up_tx.send(UpMsg::Failed {
                            who,
                            error: e.clone(),
                        });
                        result = Err(e);
                        break;
                    }
                    Err(payload) => {
                        // 回滚自身 overlay（可逆日志逆序回放），上交已收敛态后退出。
                        state.rollback();
                        abort.store(true, Ordering::SeqCst);
                        let _ = up_tx.send(UpMsg::TaskDied { who, payload });
                        break;
                    }
                }
            }
            GroupIn::Continue { tick, round } => {
                state.observed.push(Observed::Continue(tick, round));
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    with_capture(&capture, || {
                        state.handle_continue(tick, round, &desk_txs, &up_tx)
                    })
                }));
                match outcome {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        abort.store(true, Ordering::SeqCst);
                        let _ = up_tx.send(UpMsg::Failed {
                            who,
                            error: e.clone(),
                        });
                        result = Err(e);
                        break;
                    }
                    Err(payload) => {
                        state.rollback();
                        abort.store(true, Ordering::SeqCst);
                        let _ = up_tx.send(UpMsg::TaskDied { who, payload });
                        break;
                    }
                }
            }
            GroupIn::Abort { tick } => {
                state.observed.push(Observed::Abort(tick));
                state.rollback();
            }
            GroupIn::DayEnd(directive) => {
                state.observed.push(Observed::DayEnd(directive.day));
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    state.handle_day_end(&directive)
                }));
                match outcome {
                    Ok(Ok(())) => {
                        let done = UpMsg::GroupDayDone {
                            group: state.group,
                            day: directive.day,
                            accounts: state.accounts.clone(),
                            state_hash: state.state_hash(),
                        };
                        if up_tx.send(done).is_err() {
                            abort.store(true, Ordering::SeqCst);
                            result = Err(RuntimeError::SendFailed {
                                detail: format!("组 {who} GroupDayDone 发送失败"),
                            });
                            break;
                        }
                    }
                    Ok(Err(e)) => {
                        abort.store(true, Ordering::SeqCst);
                        let _ = up_tx.send(UpMsg::Failed {
                            who,
                            error: e.clone(),
                        });
                        result = Err(e);
                        break;
                    }
                    Err(payload) => {
                        state.rollback();
                        abort.store(true, Ordering::SeqCst);
                        let _ = up_tx.send(UpMsg::TaskDied { who, payload });
                        break;
                    }
                }
            }
            GroupIn::ExternalPlan(ops) => {
                // 玩家意图路由（D2 批）：外部计划存缓冲，run_round 消费。
                state.external_pending.extend(ops);
            }
            GroupIn::InformationDelivery {
                now,
                day,
                library,
                attentions,
                issuer_map,
                company_specs,
            } => {
                // Belief-1b·下：组内纯函数推进（与 session 版同输入同
                // 演进）。codes=当前持仓∪watchlist（组账户无"曾持有"
                // 零量条目面——差异登记）。快照缺席（未跑 tick 即投递）
                // 如实上行空更新。
                state.library = library;
                let mut updates = Vec::new();
                if let Some(snapshot) = state.snapshot.clone() {
                    let market = super::decision::build_market_view(&snapshot);
                    for (local, account_id, attention) in attentions {
                        let li = local as usize;
                        let Some(snap) = state.belief_states.get_mut(li).and_then(|s| s.as_mut()) else {
                            continue;
                        };
                        let mut codes: std::collections::BTreeSet<crate::StockId> =
                            state.accounts[li]
                                .shares
                                .iter()
                                .enumerate()
                                .filter(|(_, &q)| q > 0)
                                .map(|(idx, _)| crate::StockId(idx as u32))
                                .collect();
                        codes.extend(snap.watchlist.stocks.keys().copied());
                        let issuer_of =
                            |code: crate::StockId| issuer_map.get(&code).copied();
                        let spec_of =
                            |company: &crate::company::CompanyId| company_specs.get(company).copied();
                        match crate::session::notices::deliver_information_for_npc(
                            account_id,
                            now,
                            day,
                            &attention,
                            &snap.information,
                            &snap.belief,
                            codes,
                            &issuer_of,
                            &spec_of,
                            &state.library,
                            &market,
                        ) {
                            Ok(Some((_, information, belief, attention))) => {
                                snap.information = information;
                                snap.belief = belief;
                                updates.push((local, attention));
                            }
                            Ok(None) => {}
                            Err(e) => {
                                abort.store(true, Ordering::SeqCst);
                                let detail = format!("组内信息投递失败：{e:?}");
                                let _ = up_tx.send(UpMsg::Failed {
                                    who,
                                    error: RuntimeError::ProtocolInvariant {
                                        detail: detail.clone(),
                                    },
                                });
                                result = Err(RuntimeError::ProtocolInvariant { detail });
                                break;
                            }
                        }
                    }
                }
                if up_tx
                    .send(UpMsg::GroupDeliveryDone {
                        group: state.group,
                        updates,
                    })
                    .is_err()
                {
                    result = Err(RuntimeError::SendFailed {
                        detail: "GroupDeliveryDone 上行失败".to_owned(),
                    });
                    break;
                }
            }
            GroupIn::EchoBelief => {
                let states = state.belief_states.clone();
                if up_tx
                    .send(UpMsg::GroupBelief {
                        group: state.group,
                        states,
                    })
                    .is_err()
                {
                    result = Err(RuntimeError::SendFailed {
                        detail: "GroupBelief 上行失败".to_owned(),
                    });
                    break;
                }
            }
            GroupIn::EchoPlans => {
                // D3a-2c 件5：组内计划簿上行（驱动器交付点回写 session
                // plans_debug 投影源——D4 模式）。
                let plans = state.plan_book.clone();
                if up_tx
                    .send(UpMsg::GroupPlans {
                        group: state.group,
                        plans,
                    })
                    .is_err()
                {
                    result = Err(RuntimeError::SendFailed {
                        detail: "GroupPlans 上行失败".to_owned(),
                    });
                    break;
                }
            }
            GroupIn::EchoOpenDerivation => {
                // D3a-2c/3 读面：observe 直调（纯观察——组态/簿不演进、
                // 无 undo、幂等；market/now 为组内实际消费值——红证同输入锚）。
                let mut derivations = Vec::new();
                let mut derivation_market = None;
                let mut derivation_now = None;
                let mut projection_missing = false;
                if let Some(projection) = state.root_projection.clone() {
                    if let Some(snapshot) = state.snapshot.clone() {
                        derivation_market = Some(super::decision::build_market_view(&snapshot));
                        derivation_now = Some(crate::calendar::CivilInstant::new(
                            snapshot.civil_date,
                            crate::session::intraday_observation_second(
                                snapshot.tick % state.ticks_per_day,
                                state.ticks_per_day,
                                projection.auction_ticks,
                                projection.closing_auction_ticks,
                            ),
                        )
                        .expect("快照自然日与日内秒构成合法 CivilInstant"));
                        for local in 0..state.belief_states.len() as LocalIdx {
                            if state.belief_states[local as usize].is_none() {
                                continue;
                            }
                            match state.run_chain_decision(local, snapshot.tick, false) {
                                Ok((record, _)) => derivations.push(record),
                                Err(e) => {
                                    abort.store(true, Ordering::SeqCst);
                                    let _ = up_tx.send(UpMsg::Failed { who, error: e.clone() });
                                    result = Err(e);
                                    break;
                                }
                            }
                        }
                    } else {
                        projection_missing = true;
                    }
                } else {
                    projection_missing = true;
                }
                let _ = projection_missing;
                let (derivation_market, derivation_now) = (
                    derivation_market.unwrap_or_else(|| crate::strategy::MarketView {
                        stocks: Default::default(),
                        tick: 0,
                        market_minute: 0,
                    }),
                    derivation_now.unwrap_or_else(|| {
                        crate::calendar::CivilInstant::new(
                            crate::calendar::CivilDate::from_ymd(1970, 1, 1).expect("纪元日合法"),
                            0,
                        )
                        .expect("纪元零点合法")
                    }),
                );
                if up_tx
                    .send(UpMsg::GroupOpenDerivations {
                        group: state.group,
                        market: derivation_market,
                        now: derivation_now,
                        derivations,
                    })
                    .is_err()
                {
                    result = Err(RuntimeError::SendFailed {
                        detail: "GroupOpenDerivations 上行失败".to_owned(),
                    });
                    break;
                }
            }
            GroupIn::EchoObservations => {
                // D3a-2：每股链观察（价格路径+技术面）——纯构建器与
                // session 读面同源（build_price_path_observation /
                /// build_technical_observation_from_recent_trades）。
                let observations: Vec<(
                    crate::observation::PricePathObservation,
                    crate::observation::TechnicalObservation,
                )> = state
                    .observation_inputs
                    .iter()
                    .map(|inputs| inputs.build_observations())
                    .collect();
                if up_tx
                    .send(UpMsg::GroupObservations {
                        group: state.group,
                        observations,
                    })
                    .is_err()
                {
                    result = Err(RuntimeError::SendFailed {
                        detail: "GroupObservations 上行失败".to_owned(),
                    });
                    break;
                }
            }
            GroupIn::EchoAttention => {
                let attention = match &state.decision {
                    Some((profiles, _)) => profiles
                        .iter()
                        .enumerate()
                        .map(|(i, p)| (i as LocalIdx, p.next_due))
                        .collect(),
                    None => Vec::new(),
                };
                let msg = UpMsg::GroupAttention {
                    group: state.group,
                    attention,
                };
                if up_tx.send(msg).is_err() {
                    abort.store(true, Ordering::SeqCst);
                    result = Err(RuntimeError::SendFailed {
                        detail: format!("组 {who} GroupAttention 发送失败"),
                    });
                    break;
                }
            }
            GroupIn::EchoAccounts => {
                let echo = UpMsg::GroupEcho {
                    group: state.group,
                    accounts: state.accounts.clone(),
                };
                if up_tx.send(echo).is_err() {
                    abort.store(true, Ordering::SeqCst);
                    result = Err(RuntimeError::SendFailed {
                        detail: format!("组 {who} GroupEcho 发送失败"),
                    });
                    break;
                }
            }
            GroupIn::EchoCandles => {
                // K 线投影面向交易部（协调者直发 desk 通道）；组收到=
                // 路由错误，显式失败。
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!("组 {} 收到 EchoCandles（路由错误）", state.group),
                });
            }
            GroupIn::Shutdown => {
                state.observed.push(Observed::Shutdown);
                // Drain 实体批次已先于 Shutdown 入队（FIFO）：期末名册须含全部回流。
                let (cur_tick, cur_round) = (state.cur_tick, state.cur_round);
                if let Err(e) = state.apply_staging(cur_tick, cur_round) {
                    abort.store(true, Ordering::SeqCst);
                    let _ = up_tx.send(UpMsg::Failed {
                        who,
                        error: e.clone(),
                    });
                    result = Err(e);
                    break;
                }
                break;
            }
        }
    }
    let final_state = state.final_state();
    let send = up_tx.send(UpMsg::GroupFinal(Box::new(final_state)));
    if result.is_ok() {
        if let Err(_e) = send {
            result = Err(RuntimeError::SendFailed {
                detail: format!("组 {} GroupFinal 发送失败", who),
            });
        }
    }
    result
}
