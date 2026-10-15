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
    /// 每股在途卖单余量（Sol 整改 3：child_in_flight 卖票的镜像——
    /// 回写/读面口径"在手+在途"对齐旧管线；挂出加、成交/终结减）。
    pub pending_sell_qty: Vec<u32>,
    /// 在途买单 desk 占用累计（分；账户级——**全部**在途买单，不限计划
    /// 链票：挂出 +purse、买腿成交 −实付、CashRefund −退款）。Sol 五审：
    /// 公开现金=组现金+此值（挂出不减、成交实付减、终结不变——旧管线
    /// reservation 观同口径；价优差额自动保留至退款）。
    pub buy_purse_held_cents: i64,
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
            pending_sell_qty: vec![0; n_stocks],
            buy_purse_held_cents: 0,
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

/// 计划子单在途票（Sol 整改 2/3：挂出量+方向+挂出限价+受理身份；
/// Sol 二审修复集 2：实时未成交余量+累计成交额——组观察/开仓/session
/// 投影三面统一消费**真实余量**，不再按原挂出量）。
#[derive(Clone, Debug)]
struct ChildTicket {
    /// 挂出量（完整子单数量——PendingOp.qty，adopt 的数量输入）。
    qty: u32,
    /// 挂出方向（Sol 整改 3：在途资产读面——卖单量加回持仓、买单
    /// purse 加回现金）。
    side: Side,
    /// 挂出限价（i64 分——买单 purse 重算的输入）。
    price: i64,
    /// 受理身份（fill 首见时自 FillFact.order 确定性合成；None=挂出
    /// 未受理——desk 尚未成交/拒收）。
    order_id: Option<crate::OrderId>,
    /// 实时未成交余量（Sol 二审修复集 2：挂出=qty、fill 递减——在途
    /// 持仓/ purse 读面与 session 镜像同口径）。
    remaining: u32,
    /// desk 实际扣减累计（Sol 四审修复 1：买腿 Σ(price×qty+佣金+过户费)
    /// ——live purse=挂出全额 purse−此值，价优差额不漏计；与 desk 逐笔
    /// buyer_charge 扣减同口径）。
    charged_cents: i64,
}

/// Sol 整改 3：含在途的权益（现金含在途买 purse；持仓含在途卖余量
/// ×最新价——对齐旧管线权益观察口径）。
fn equity_cents_with_pending(
    group_account: &GroupAccount,
    child_in_flight: &std::collections::BTreeMap<
        (crate::AccountId, crate::StockId),
        ChildTicket,
    >,
    account_ids: &[crate::AccountId],
    local: LocalIdx,
    config: &std::sync::Arc<crate::config::GameConfig>,
    market: &crate::strategy::MarketView,
) -> crate::money::Money {
    let mut equity = crate::money::Money::from_cents(cash_with_pending_at(
        group_account,
        child_in_flight,
        account_ids,
        local,
        config,
    ));
    for (stock, &qty_raw) in group_account.shares.iter().enumerate() {
        let qty = held_qty_with_pending_at(
            &group_account.shares,
            child_in_flight,
            account_ids,
            local,
            stock,
        );
        if qty == 0 && qty_raw == 0 {
            continue;
        }
        let price = market
            .stocks
            .get(&crate::StockId(stock as u32))
            .map(|view| view.last_price)
            .unwrap_or_else(|| panic!("组内权益计算缺股 {stock:?} 市场视图"));
        equity = equity
            .add(price.mul_shares(qty).unwrap_or_else(|error| {
                panic!("组内权益持仓估值失败 {stock:?}: {error:?}");
            }))
            .unwrap_or_else(|error| panic!("组内权益累加失败：{error:?}"));
    }
    equity
}

/// Sol 整改 3：含在途的持仓读面（旧管线口径=在手+在途卖单余量——组
/// shares 已扣在途；本 (账户,股) 唯一在途票为卖时加回其量）。
/// Sol 二审修复集 2：加回**实时未成交余量**（此前按原挂出量——挂 200
/// 成 100 后组加回 200、session 镜像只见 100，两口径分叉）。
fn held_qty_with_pending_at(
    shares: &[u32],
    child_in_flight: &std::collections::BTreeMap<
        (crate::AccountId, crate::StockId),
        ChildTicket,
    >,
    account_ids: &[crate::AccountId],
    local: LocalIdx,
    stock: usize,
) -> u32 {
    let mut qty = shares.get(stock).copied().unwrap_or(0);
    if let Some(&account_id) = account_ids.get(local as usize) {
        if let Some(ticket) = child_in_flight.get(&(account_id, crate::StockId(stock as u32))) {
            if matches!(ticket.side, Side::Sell) {
                qty = qty.saturating_add(ticket.remaining);
            }
        }
    }
    qty
}

/// Sol 整改 3：含在途的现金读面（在途买 purse 加回——挂单期间权益
/// 观察不缩水）。Sol 四审修复 1：**desk 实际占用**口径——加回=挂出全额
/// purse−票上累计 desk 扣减（Σ buyer_charge：成交额+边际佣金+过户费）。
/// 三层现金语义：`GroupAccount.cash`=可分配预算（严格所有权，在途已扣
/// ——报价预算消费）；本读面=总现金观察（权益观察消费）；价优部分成交
/// 的差额仍在 desk（此前按余量重算 buy_purse_limit 下界——漏计价优
/// 差额，三审反例：限价 1000 挂 200、100 股以 900 成交，desk 余额
/// 110001 vs 下界 100001）。
fn cash_with_pending_at(
    group_account: &GroupAccount,
    child_in_flight: &std::collections::BTreeMap<
        (crate::AccountId, crate::StockId),
        ChildTicket,
    >,
    account_ids: &[crate::AccountId],
    local: LocalIdx,
    config: &std::sync::Arc<crate::config::GameConfig>,
) -> i64 {
    let mut cash = group_account.cash;
    if let Some(&account_id) = account_ids.get(local as usize) {
        for ((owner, _code), ticket) in child_in_flight {
            if *owner == account_id && matches!(ticket.side, Side::Buy) {
                if let Ok(purse) = super::fees::buy_purse_limit(
                    config,
                    Money::from_cents(ticket.price.max(1)),
                    ticket.qty,
                    Money::ZERO,
                ) {
                    // desk 占用扣减（票上累计 buyer_charge；超挂出全额=
                    /// desk 扣减溢出——票/簿推进失同步，显式失败）。
                    let occupancy = purse
                        .cents()
                        .checked_sub(ticket.charged_cents)
                        .unwrap_or_else(|| {
                            panic!(
                                "在途 purse 占用下穿：price={} qty={} charged={}",
                                ticket.price, ticket.qty, ticket.charged_cents
                            )
                        });
                    cash = cash.saturating_add(occupancy);
                }
            }
        }
    }
    cash
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
    /// 在途卖余量镜像回滚（Sol 整改 3：挂出加/成交终结减的逆增量）。
    PendingSell {
        local: LocalIdx,
        stock: StockIdx,
        delta: i64,
    },
    /// 在途买占用累计回滚（Sol 五审：挂出加/买腿实付减/退款减的逆增量）。
    BuyPurseHeld {
        local: LocalIdx,
        delta: i64,
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
    /// 收到受理事实下行（Sol 二审修复集 3：股，所有者，desk 序号——
    /// 续行轮受理完整传递的时序锚）。
    ChildAccepted(StockIdx, (GroupIdx, LocalIdx), u64),
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
    root_projection: Option<std::sync::Arc<crate::runtime::world::RootProjection>>,
    /// D3a-3 复核收口+Sol 整改 2：计划子单在途票（挂出量+受理身份）——
    /// 身份=FillFact.order 的确定性合成（不另造计数器）；部分成交推进、
    /// 全成/终结（Closed/到期/日终清簿/拒收）清除。
    child_in_flight:
        std::collections::BTreeMap<(crate::AccountId, crate::StockId), ChildTicket>,
    /// Sol 整改 1：簿快照已保存标志（本 tick 内首个簿变更前保存一次；
    /// 转正/回滚清 undo 时重置）。
    plan_undo_saved: bool,
    /// Sol 二审修复集 4：Start 间窗口收到的 ForceDue 待办——延迟到下一
    /// Start 应用（指令效果归下一 tick 的日志域：tick Abort 冲回强制前
    /// 值、指令失效需重发——代码承诺语义；此前收到即入账，undo 被
    /// Start 的 undo.clear() 清除）。仅 cfg(test) 注入通道写入。
    pending_force_due: Vec<(LocalIdx, u64)>,
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
        root_projection: Option<std::sync::Arc<crate::runtime::world::RootProjection>>,
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
            pending_force_due: Vec::new(),
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
            // Sol 五审：在途镜像入哈希（Abort 回滚等价判据覆盖镜像漂移）。
            for &p in &a.pending_sell_qty {
                h = fnv(h, p as u64);
            }
            h = fnv(h, a.buy_purse_held_cents as u64);
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
            h = fnv(h, ticket.price as u64);
            h = fnv(h, matches!(ticket.side, Side::Buy) as u64);
            h = fnv(h, ticket.order_id.map_or(0, |o| o.0));
            // Sol 二审修复集 2：实时余量/累计成交额入哈希（Abort 回滚等价
            // 判据覆盖票推进漂移）。
            h = fnv(h, u64::from(ticket.remaining));
            h = fnv(h, ticket.charged_cents as u64);
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
                GroupUndo::PendingSell { local, stock, delta } => {
                    let acct = &mut self.accounts[local as usize];
                    let slot = &mut acct.pending_sell_qty[stock as usize];
                    *slot = (*slot as i64 - delta).max(0) as u32;
                }
                GroupUndo::BuyPurseHeld { local, delta } => {
                    self.accounts[local as usize].buy_purse_held_cents -= delta;
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
        // Sol 二审修复集 4：未消费的 ForceDue 待办一并作废（指令失效
        /// 需重发）；已入日志域的由上方 undo 逆序回放冲回。
        self.pending_force_due.clear();
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
                // Sol 五审：在途镜像统一按 fill 推进（不限计划链票——
                // 外部/retail 单同口径）：卖腿余量减；买腿占用减实付
                //（gross+佣金+过户=desk buyer_charge 同式）。
                let gross = i64::from(fill.price) * i64::from(fill.qty);
                match fill.side {
                    Side::Sell => {
                        let acct = &mut self.accounts[fill.owner.1 as usize];
                        let before = acct.pending_sell_qty[stock as usize];
                        let after = before.saturating_sub(fill.qty);
                        acct.pending_sell_qty[stock as usize] = after;
                        if before != after {
                            self.undo.push(GroupUndo::PendingSell {
                                local: fill.owner.1,
                                stock: stock as StockIdx,
                                delta: -i64::from(before - after),
                            });
                        }
                    }
                    Side::Buy => {
                        let charge = gross + fill.commission + fill.transfer_fee;
                        let acct = &mut self.accounts[fill.owner.1 as usize];
                        acct.buy_purse_held_cents -= charge;
                        self.undo.push(GroupUndo::BuyPurseHeld {
                            local: fill.owner.1,
                            delta: -charge,
                        });
                    }
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
                    self.release_child_ticket(
                        stock as StockIdx,
                        receipt.owner.1,
                        receipt.order.1,
                    )?;
                }
            }
            for entity in batch.entities {
                self.apply_entity(stock as StockIdx, entity)?;
            }
            // 清仓清零（实体落账后按期末 shares 判定：对齐 session 清仓删除
            // 条目——下次买入从零新建，避免再持有后 cost_price 漂移）。
            self.clear_flat_cost_basis(&touched);
            // Sol D3b 件 1：fill 触达账户的个人记忆裁剪（对齐 session
            /// candidate_commit 的逐账户 prune_plan_personal_memory——
            /// completed_sell 2069 残余收口；清仓后售出股脱离保护集）。
            for (local, _) in &touched {
                self.prune_personal_memory_after_completion(*local);
            }
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
                    side: ticket.side,
                    price: ticket.price,
                    order_id: None,
                    remaining: ticket.remaining,
                    charged_cents: ticket.charged_cents,
                }),
            });
            self.child_in_flight.insert((account, code.clone()), ticket.clone());
        }
        let Some(order_id) = ticket.order_id else {
            return Ok(());
        };
        // Sol 二审修复集 3（六审勘正）：票已有身份时成交**同单才推进**——
        // 此前未验证即以票身份推进外来单的成交量（同账户同股他单的 fill
        // 静默吃掉本票执行腿余量）。身份不符=外来同键单的成交（驱动器
        /// 意图路由/测试注入的合法形态——ticket 只承载本单事实）：跳过
        /// 不误挂（与 release 的身份关联语义一致；原"显式失败"形态会
        /// 杀死合法外来单的拍——sol7th_foreign 红证实测）。
        if order_id_of(&fill.order) != order_id {
            return Ok(());
        }
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
        // Sol 二审修复集 2：票的实时余量随 fill 推进（组读面三面统一）；
        // Sol 四审修复 1：买腿同步累计 desk 实际扣减。余量下穿/扣减溢出
        /// =票/簿推进失同步，显式失败（铁律二）。
        {
            let prev_ticket = ticket.clone();
            ticket.remaining = ticket.remaining.checked_sub(fill.qty).unwrap_or_else(|| {
                panic!(
                    "组 {group} 在途票余量下穿：account={account:?}, code={code:?}, remaining={}, fill_qty={}",
                    ticket.remaining, fill.qty
                )
            });
            let gross = crate::money::Money::from_cents(fill.price)
                .mul_shares(fill.qty)
                .unwrap_or_else(|error| {
                    panic!("组 {group} 在途票成交额溢出：{error:?}")
                })
                .cents();
            // Sol 四审修复 1：desk 实际扣减累计（买腿 buyer_charge=成交额
            // +边际佣金+过户费；卖腿收方不占 purse——StampTax 由净得扣）。
            if matches!(fill.side, Side::Buy) {
                let charge = gross
                    .checked_add(fill.commission)
                    .and_then(|v| v.checked_add(fill.transfer_fee))
                    .unwrap_or_else(|| {
                        panic!("组 {group} 在途票 desk 扣减溢出：account={account:?}, code={code:?}")
                    });
                ticket.charged_cents = ticket
                    .charged_cents
                    .checked_add(charge)
                    .unwrap_or_else(|| {
                        panic!("组 {group} 在途票 desk 扣减累计溢出：account={account:?}, code={code:?}")
                    });
            }
            self.undo.push(GroupUndo::ChildInFlight {
                account,
                code: code.clone(),
                prev: Some(prev_ticket),
            });
            self.child_in_flight.insert((account, code.clone()), ticket.clone());
        }
        // 在途卖余量镜像已上移至 apply_staging 统一 fill 段（Sol 五审：
        // 外部/retail 卖单同口径——票域减法会造成外部单双计/漏计）。
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

    /// D3b 件 1：计划完成后的个人记忆裁剪（session 版语义的组内等价——
    /// 保护集外的 watchlist/price_memory 按 MAX_UNHELD_WATCHLIST_STOCKS
    /// 驱逐；Personal undo 可逆）。
    fn prune_personal_memory_after_completion(&mut self, local: LocalIdx) {
        let Some(&account) = self
            .root_projection
            .as_ref()
            .and_then(|projection| projection.account_ids.get(local as usize))
        else {
            return;
        };
        let Some(mut snap) = self.belief_states.get(local as usize).cloned().flatten() else {
            return;
        };
        // 保护集：在手∪在途卖（session positions 键面等价——旧管线清仓
        /// 删条目后售出股不在保护集；在途卖单期间条目保留=保护）∪活跃
        /// 计划股。**不含**曾持有（ever_held 是组侧观察面位——清仓后
        /// session 条目已删，误入保护集会使售出股永不被裁剪）。
        let mut protected: std::collections::BTreeSet<crate::StockId> = self.accounts
            [local as usize]
            .shares
            .iter()
            .enumerate()
            .filter(|(idx, &qty)| {
                qty > 0 || self.accounts[local as usize].pending_sell_qty[*idx] > 0
            })
            .map(|(idx, _)| crate::StockId(idx as u32))
            .collect();
        protected.extend(self.plan_book.active_codes(account).copied());
        // 与 session 相同的裁剪原语（共享类型 pub 方法）。
        snap.watchlist.prune(&protected);
        snap.price_memory.prune(&protected);
        // 写回+undo（Personal 快照可逆）。
        let prev = self
            .belief_states
            .get(local as usize)
            .cloned()
            .flatten()
            .expect("上方已核在位");
        self.undo.push(GroupUndo::Personal {
            local,
            prev: Box::new(prev),
        });
        if let Some(slot) = self.belief_states.get_mut(local as usize) {
            *slot = Some(snap);
        }
    }

    /// Sol 整改受理通道：受理事实的票身份登记（Accepted lifecycle 下行
    /// ——OrderRef 合成+挂出量 adopt；fill 首见时票已有身份直接推进）。
    fn adopt_child_on_accepted(
        &mut self,
        stock: StockIdx,
        local: LocalIdx,
        order_serial: u64,
    ) -> Result<(), RuntimeError> {
        let Some(&account) = self
            .root_projection
            .as_ref()
            .and_then(|projection| projection.account_ids.get(local as usize))
        else {
            return Ok(());
        };
        let code = crate::StockId(u32::from(stock));
        let Some(mut ticket) = self.child_in_flight.get(&(account, code)).cloned() else {
            return Ok(());
        };
        if ticket.order_id.is_some() {
            return Ok(());
        }
        let side = ticket.side;
        let qty = ticket.qty;
        let order_id = order_id_of(&(stock, order_serial));
        let day = self.cur_tick / self.ticks_per_day;
        self.save_plan_book_undo();
        self.plan_book
            .record_child_submitted(account, &code, side, order_id, qty, day)
            .unwrap_or_else(|error| {
                panic!("组内受理身份登记失败 {account:?} {code:?}: {error}")
            });
        ticket.order_id = Some(order_id);
        self.undo.push(GroupUndo::ChildInFlight {
            account,
            code: code.clone(),
            prev: Some(ChildTicket {
                qty,
                side,
                price: ticket.price,
                order_id: None,
                remaining: ticket.remaining,
                charged_cents: ticket.charged_cents,
            }),
        });
        self.child_in_flight.insert((account, code), ticket);
        Ok(())
    }

    /// Sol 整改 2：子单终结（Closed 实体——到期、日终清簿、撤销与
    /// 失败释放的统一信号）。已受理（有身份）→record_child_released；
    /// 未受理→票清除（执行腿留待下次挂出覆盖——install 对 order=None
    /// 可重入）。Sol 二审修复集 3：按订单身份关联释放——终结单号与票
    /// 已受理身份不符时**不释放**（同账户同股他单的终结不得放错票；
    /// 资源实体/在途计数照常落账）。
    fn release_child_ticket(
        &mut self,
        stock: StockIdx,
        local: LocalIdx,
        order_serial: u64,
    ) -> Result<(), RuntimeError> {
        let Some(&account) = self
            .root_projection
            .as_ref()
            .and_then(|projection| projection.account_ids.get(local as usize))
        else {
            return Ok(());
        };
        let code = crate::StockId(u32::from(stock));
        if let Some(known) = self
            .child_in_flight
            .get(&(account, code))
            .and_then(|ticket| ticket.order_id)
        {
            if known != order_id_of(&(stock, order_serial)) {
                // 终结的是他单（票不归属本单）——保留本票，仅落资源实体。
                return Ok(());
            }
        }
        let Some(ticket) = self.child_in_flight.remove(&(account, code)) else {
            return Ok(());
        };
        self.undo.push(GroupUndo::ChildInFlight {
            account,
            code: code.clone(),
            prev: Some(ticket.clone()),
        });
        // 在途卖余量镜像改由 RefundedShares 实体驱动（apply_staging 统一
        // 段——Sol 五审：票域减法与实体退款双计）。
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
            Entity::CashProceeds { local, amount } => {
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
            Entity::CashRefund { local, amount } => {
                // 买单 purse 余额退款：现金入账 + 在途买占用同步减
                ///（Sol 五审：实体驱动——全额终结后占用归零；卖腿现金走
                /// CashProceeds）。
                let acct = self.acct_mut(local)?;
                acct.cash = acct.cash.checked_add(amount).ok_or_else(|| {
                    RuntimeError::ProtocolInvariant {
                        detail: format!("组 {group} 现金落账溢出：local={local}, amount={amount}"),
                    }
                })?;
                acct.buy_purse_held_cents -= amount;
                self.undo.push(GroupUndo::Cash {
                    local,
                    delta: amount,
                });
                self.undo.push(GroupUndo::BuyPurseHeld {
                    local,
                    delta: -amount,
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
                // （再次实际持有——session 条目 qty>0）。Sol 五审：在途卖
                // 余量镜像同步减（退款量=未成交余量——实体驱动，票/外部
                // 单统一）。
                {
                    let acct = self.acct_mut(local)?;
                    let before = acct.pending_sell_qty[stock as usize];
                    let after = before.saturating_sub(qty);
                    acct.pending_sell_qty[stock as usize] = after;
                    if before != after {
                        self.undo.push(GroupUndo::PendingSell {
                            local,
                            stock: stock as StockIdx,
                            delta: -i64::from(before - after),
                        });
                    }
                }
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
            Entity::Closed { local, order_serial } => {
                // Sol 整改 2：子单终结信号（撤销/到期/日终清簿——组侧
                // 在途票与执行腿释放；order_serial=订单身份关联，二审修复 3）。
                self.release_child_ticket(stock, local, order_serial)?;
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
        // Sol 二审修复集 4：Start 间窗口收到的 ForceDue 在此应用——效果
        // 归本 tick 日志域（Abort 冲回强制前值、指令失效需重发）。
        for (local, target) in std::mem::take(&mut self.pending_force_due) {
            if let Some((profiles, _)) = &mut self.decision {
                if let Some(p) = profiles.get_mut(local as usize) {
                    if p.next_due > target {
                        self.undo.push(GroupUndo::NextDue {
                            local,
                            prev: p.next_due,
                        });
                        p.next_due = target;
                    }
                }
            }
        }
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
        // Sol 二审修复集 4：快照标志随日志一并复位——此前保留 true 时，
        // 日结相位的簿变更（Drain Closed→release_child_ticket）不再保存
        // 新快照，Abort 恢复在途票但不恢复执行腿（票/簿分裂）。
        self.undo.clear();
        self.plan_undo_saved = false;
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
            Vec<(LocalIdx, StockIdx, u64)>,
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
            &self.child_in_flight,
            &self.config,
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
            &self.library,
            &projection.company_registry,
            &projection.stock_codes,
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
        let assessments_map = operations
            .lifecycle_assessments()
            .into_iter()
            .find(|(owner, _)| *owner == account)
            .map(|(_, by_code)| by_code)
            .unwrap_or_default();
        let assessments: Vec<_> = assessments_map.iter().map(|(k, v)| (*k, v.clone())).collect();
        let (params, style) = projection
            .chain_params
            .get(local as usize)
            .and_then(|slot| *slot)
            .unwrap_or_else(|| panic!("组 {group} belief 位 {local} 缺链参数投影"));
        // D3b 件 4：复审消费（session Lifecycle operation 等价——observe
        // 之后、开仓循环之前；record_review/quiet 门控在 apply_lifecycle_
        // action 内——每动作前记录簿内基线）。读面（write_back=false）不
        // 消费复审（纯观察——EchoOpenDerivation 同式幂等承诺）。
        let mut review_cancels: Vec<(LocalIdx, StockIdx, u64)> = Vec::new();
        // 复审/开仓后复评共用输入（交易日/分钟收盘预转换/复审时刻——
        // 组装一次；分钟转换与 build_observations 同式 c.max(1)，时刻与
        // RootReadContext 共用 market_minute_at——字段不得分叉）。
        let trading_day = snapshot.tick / self.ticks_per_day;
        let minutes_by_code: std::collections::BTreeMap<
            crate::StockId,
            Vec<crate::observation::MarketMinuteClose>,
        > = self
            .observation_inputs
            .iter()
            .enumerate()
            .filter(|(idx, _)| assessments_map.contains_key(&crate::StockId(*idx as u32)))
            .map(|(idx, inputs)| {
                (
                    crate::StockId(idx as u32),
                    inputs
                        .minutes
                        .iter()
                        .map(|&(minute, c)| crate::observation::MarketMinuteClose {
                            absolute_trading_minute: minute,
                            close: crate::money::Money::from_cents(c.max(1)),
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        let moment = crate::experience::ExperienceMoment {
            civil_date: snapshot.civil_date,
            market_minute: market_minute_at(
                snapshot.tick,
                self.ticks_per_day,
                projection.auction_ticks,
                projection.closing_auction_ticks,
            ),
            trading_day,
        };
        if write_back {
            let group_account = self
                .accounts
                .get(local as usize)
                .unwrap_or_else(|| panic!("组 {group} 复审位 {local} 账户缺失"));
            let held_by_code: std::collections::BTreeMap<crate::StockId, u32> = group_account
                .shares
                .iter()
                .enumerate()
                .map(|(idx, &qty)| (crate::StockId(idx as u32), qty))
                .collect();
            let equity = equity_cents_with_pending(
                group_account,
                &self.child_in_flight,
                &projection.account_ids,
                local,
                &self.config,
                &observation.market,
            );
            // equity<=0 不复审（session PlanLifecycleReview::capture 同式
            // 早退——权益耗尽的账户无修订面）。
            if equity.cents() > 0 {
                let review = crate::session::PlanLifecycleReview {
                    account,
                    market: &observation.market,
                    plans: &self.plan_book,
                    held_by_code,
                    equity,
                    belief: &out.belief,
                    chain_params: params,
                    trading_day,
                    lot: self.config.lot_size,
                };
                let host = GroupReviewHost {
                    state: &*self,
                    local,
                    account,
                    market: observation.market.clone(),
                    // 开仓后复评共用同一份预转换（host 拥有副本——分离
                    /// 字段借用下的生命周期独立）。
                    minutes_by_code: minutes_by_code.clone(),
                    moment,
                };
                let actions = review.collect(&host, &assessments_map);
                for action in actions {
                    self.apply_lifecycle_action(
                        action,
                        &observation.market,
                        local,
                        trading_day,
                        &mut review_cancels,
                    );
                }
            }
        }
        let mut opens: Vec<crate::plans::PlanOpen> = Vec::new();
        // Sol 二审修复集 1：existing=**本轮开仓前**计划集合——采集点必须
        // 在开仓循环之前（此前在循环之后采集：同轮新开的计划全被归入
        // existing → 预算分类全成 ExistingPlan，NewOpportunity 不可达）。
        let existing_before: Vec<crate::plans::PlanId> =
            self.plan_book.active_plan_ids_for_account(account);
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
            // Sol 整改 3：开仓消费的持仓/权益读面含在途（挂单期间目标
            // 量与权益观察不因所有权转移缩水——qty 口径差收口）。
            let held_qty = held_qty_with_pending_at(
                &group_account.shares,
                &self.child_in_flight,
                &projection.account_ids,
                local,
                code.0 as usize,
            );
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
                equity_cents_with_pending(
                    group_account,
                    &self.child_in_flight,
                    &projection.account_ids,
                    local,
                    &self.config,
                    &observation.market,
                ),
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
                let plan_id = self
                    .plan_book
                    .create(open.clone())
                    .unwrap_or_else(|error| panic!("组内计划创建失败 {account:?} {code:?}: {error}"));
                // D3b 件 4：新建计划复审基线+报价前 urgency 复评（session
                /// Create 动作同式：record_review/record_resource_review 后
                /// 按 PauseAssessment 判停——组内此前缺失，quiet 首拍无基线）。
                let price = observation
                    .market
                    .stocks
                    .get(code)
                    .map(|v| v.last_price)
                    .unwrap_or_else(|| panic!("组内新计划复审缺股 {code:?} 行情"));
                let idx = code.0 as usize;
                let resources = {
                    let acct = &self.accounts[local as usize];
                    crate::plans::ReviewResources {
                        cash: crate::money::Money::from_cents(acct.cash),
                        frozen_cash: crate::money::Money::ZERO,
                        held_qty: acct.shares.get(idx).copied().unwrap_or(0),
                        t1_locked: acct.frozen_today.get(idx).copied().unwrap_or(0),
                    }
                };
                self.plan_book
                    .record_review(plan_id, trading_day, price, 0)
                    .unwrap_or_else(|e| panic!("组内新计划复审事实记录失败 {plan_id:?}: {e:?}"));
                self.plan_book
                    .record_resource_review(plan_id, resources)
                    .unwrap_or_else(|e| panic!("组内新计划复审资源记录失败 {plan_id:?}: {e:?}"));
                // 报价前 urgency 复评（session Create 分支同式：分钟路径
                /// return + 组内 urgency 面共享函数——判停即 Paused）。
                let minutes = minutes_by_code.get(code).cloned().unwrap_or_default();
                let path = crate::observation::build_price_path_observation(&minutes, &[], None)
                    .unwrap_or_else(|e| panic!("组内新计划价格路径失败 {plan_id:?}: {e:?}"));
                let plan = self
                    .plan_book
                    .plan(plan_id)
                    .unwrap_or_else(|e| panic!("组内新计划读取失败 {plan_id:?}: {e:?}"))
                    .clone();
                let (assessment, _) = group_urgency_at_view(
                    &self.accounts,
                    &self.belief_states,
                    &self.child_in_flight,
                    &projection,
                    &self.config,
                    local,
                    &observation.market,
                    &moment,
                    &plan,
                    crate::session::ratio_to_bp(path.thirty_minute.return_ratio),
                    crate::session::ratio_to_bp(path.one_minute.return_ratio),
                );
                if let crate::plans::PauseAssessment::PauseAndRequestCancel(reason) =
                    assessment.pause
                {
                    self.save_plan_book_undo();
                    self.plan_book
                        .apply(
                            plan_id,
                            crate::plans::PlanEvent::Paused { reason, trading_day },
                        )
                        .unwrap_or_else(|e| panic!("组内新计划判停失败 {plan_id:?}: {e:?}"));
                }
                opens.push(open);
            }
        }
        // 首子单报价（D3a-3 件4b——生产路径；读面不算子单）。existing=
        // 本轮开仓前计划集合（Sol 整改 3b：预算新旧分类——采集点在开仓
        // 循环之前，见上方 Sol 二审修复集 1 注记）。
        let (child_ops, chain_cancels) = if write_back {
            self.chain_quote_children(
                local,
                account,
                &projection,
                &out,
                &snapshot,
                &existing_before,
            )?
        } else {
            (Vec::new(), Vec::new())
        };
        // D3b 件 4：复审撤单（Terminate/Restructure 的在途子单）在前、
        // 报价撤单（件 3 前置分支/QuoteAction::Cancel）在后——复审先于
        // 报价执行的同序（session Lifecycle operation → QuotePlans）。
        let mut cancels_out = review_cancels;
        cancels_out.extend(chain_cancels);
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
            cancels_out,
        ))
    }

    /// D3b 件 4：复审动作施加（session `apply_plan_lifecycle_actions` 的
    /// 组内等价）。既有计划动作 apply 前先 record_review/record_resource_
    /// review——quiet 门控的簿内基线（session 同式：决策核比对 last_review_*
    /// 与本次事实；组内缺失则每拍修订/计划身份漂移，第 9 任定位的共同
    /// 根因）。Terminate/Restructure 的在途子单定向撤单并入 cancels
    /// （session 版 push_termination/push_restructure 执行链的路由面——
    /// 否则子单搁置市场成交到日终）。Create 由开仓循环承载（复审不重复
    /// 创建；session Create 分支的 record+报价前 urgency 复评组内缺——
    /// 首拍复审非 quiet 由 unchanged 吸收，登记差异）。
    fn apply_lifecycle_action(
        &mut self,
        action: crate::session::PlanLifecycleAction,
        market: &crate::strategy::MarketView,
        local: LocalIdx,
        trading_day: u64,
        cancels: &mut Vec<(LocalIdx, StockIdx, u64)>,
    ) {
        use crate::session::PlanLifecycleAction as A;
        // 既有计划动作先记复审事实（session apply_plan_lifecycle_actions
        // 同式——每个动作独立记录，同一计划的连续动作各自刷新基线）。
        let existing = match &action {
            A::ExecutionState { plan_id, .. }
            | A::Terminate { plan_id, .. }
            | A::Observe { plan_id, .. }
            | A::Revise { plan_id, .. }
            | A::Restructure { plan_id, .. } => Some(*plan_id),
            A::Create { .. } => None,
        };
        if let Some(plan_id) = existing {
            let code = {
                let plan = self.plan_book.plan(plan_id).unwrap_or_else(|e| {
                    panic!("组内复审动作计划读取失败 {plan_id:?}: {e:?}")
                });
                plan.code().clone()
            };
            // 复审事实：price=快照行情；acquired=0（belief information 的
            // 公司记录计数近似——与 GroupReviewHost.review_facts 同式登记）。
            let price = market
                .stocks
                .get(&code)
                .map(|v| v.last_price)
                .unwrap_or_else(|| panic!("组内复审缺股 {code:?} 行情"));
            let idx = code.0 as usize;
            let resources = {
                let acct = &self.accounts[local as usize];
                crate::plans::ReviewResources {
                    cash: crate::money::Money::from_cents(acct.cash),
                    // 组内所有权模型：现金已扣在途（chain_quote_children
                    /// frozen=ZERO 同式——保守预算，登记差异）。
                    frozen_cash: crate::money::Money::ZERO,
                    held_qty: acct.shares.get(idx).copied().unwrap_or(0),
                    t1_locked: acct.frozen_today.get(idx).copied().unwrap_or(0),
                }
            };
            self.save_plan_book_undo();
            self.plan_book
                .record_review(plan_id, trading_day, price, 0)
                .unwrap_or_else(|e| panic!("组内复审事实记录失败 {plan_id:?}: {e:?}"));
            self.plan_book
                .record_resource_review(plan_id, resources)
                .unwrap_or_else(|e| panic!("组内复审资源记录失败 {plan_id:?}: {e:?}"));
        }
        match action {
            A::ExecutionState { plan_id, event } => {
                self.save_plan_book_undo();
                self.plan_book
                    .apply(plan_id, event)
                    .unwrap_or_else(|e| panic!("组内复审执行态失败 {plan_id:?}: {e:?}"));
            }
            A::Revise { plan_id, revision } => {
                self.save_plan_book_undo();
                self.plan_book
                    .apply(plan_id, crate::plans::PlanEvent::Revised { revision })
                    .unwrap_or_else(|e| panic!("组内复审修订失败 {plan_id:?}: {e:?}"));
            }
            A::Observe { plan_id, trading_day } => {
                self.save_plan_book_undo();
                self.plan_book
                    .apply(
                        plan_id,
                        crate::plans::PlanEvent::ObservedNoChange { trading_day },
                    )
                    .unwrap_or_else(|e| panic!("组内复审观察失败 {plan_id:?}: {e:?}"));
            }
            A::Terminate {
                plan_id,
                code,
                child_order_id,
                reason,
                trading_day,
                ..
            } => {
                // 在途子单先真实撤销（票留待 Closed 实体释放——件 3 的
                // release 路径同源）。
                if let Some(order_id) = child_order_id {
                    cancels.push((
                        local,
                        code.0 as StockIdx,
                        order_id.0 & 0x0000_FFFF_FFFF_FFFF,
                    ));
                }
                self.save_plan_book_undo();
                self.plan_book
                    .apply(
                        plan_id,
                        crate::plans::PlanEvent::Terminated { reason, trading_day },
                    )
                    .unwrap_or_else(|e| panic!("组内复审终止失败 {plan_id:?}: {e:?}"));
            }
            A::Restructure {
                plan_id,
                code,
                child_order_id,
                revision,
                ..
            } => {
                // 重构同撤（session push_restructure：终止/反向/超额时
                // 先撤在途子单再 apply 修订——生命周期约束，决策核注记）。
                if let Some(order_id) = child_order_id {
                    cancels.push((
                        local,
                        code.0 as StockIdx,
                        order_id.0 & 0x0000_FFFF_FFFF_FFFF,
                    ));
                }
                self.save_plan_book_undo();
                self.plan_book
                    .apply(plan_id, crate::plans::PlanEvent::Revised { revision })
                    .unwrap_or_else(|e| panic!("组内复审重构失败 {plan_id:?}: {e:?}"));
            }
            A::Create { .. } => {
                // 开仓由开仓循环承载（无计划分支）——复审不重复 Create。
            }
        }
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
        projection: &std::sync::Arc<crate::runtime::world::RootProjection>,
        personal: &crate::session::PlanPersonalState,
        snapshot: &std::sync::Arc<TickSnapshot>,
        existing_before: &[crate::plans::PlanId],
    ) -> Result<(Vec<super::decision::PendingOp>, Vec<(LocalIdx, StockIdx, u64)>), RuntimeError>
    {
        let group = self.group;
        let mut ops = Vec::new();
        // D3b 件 3：定向撤单目标（local, stock, desk serial）——
        // QuoteAction::Cancel 的路由产物（票留待 Closed 实体释放——
        /// identity 匹配路径已有）。
        let mut cancels: Vec<(LocalIdx, StockIdx, u64)> = Vec::new();
        // D3b 件 3：撤买终止目标（循环外 apply——借用隔离）。
        let mut withdrawals: Vec<crate::plans::PlanId> = Vec::new();
        let plan_ids = self.plan_book.active_plan_ids_for_account(account);
        if plan_ids.is_empty() {
            return Ok((ops, cancels));
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
        // Sol 整改 3：预算现金=可用（已扣在途 purse——所有权口径；frozen
        /// 单独为 0 防重复扣，裁决登记）。Sol 整改 3b：existing=本轮开仓
        /// 前集合（此前从全部 active 构造——新计划全成 ExistingPlan，
        /// NewOpportunity 不可达）。
        let cash = crate::money::Money::from_cents(group_account.cash);
        let existing: std::collections::BTreeSet<crate::plans::PlanId> =
            existing_before.iter().copied().collect();
        // 软预算请求（与 session prepare_classified_plan_quotes 同源纯
        // 函数：买入按涨停预留、卖出按可卖量收敛）。
        let mut requests = Vec::new();
        for &plan_id in &plan_ids {
            let plan = self
                .plan_book
                .plan(plan_id)
                .unwrap_or_else(|error| panic!("组内计划读取失败 {plan_id:?}: {error}"));
            // D3b 件 3：Paused 计划放行到报价决策（共享 decide_plan_quote
            /// 的暂停分支产出 Cancel——定向撤单路由）；terminal 跳过。
            /// 在途票跳过仅对 Active（防重复挂单）——Paused+在途恰是
            /// Cancel 的触发形态，不得跳过。
            if matches!(
                plan.status(),
                crate::plans::PlanStatus::Completed | crate::plans::PlanStatus::Terminated { .. }
            ) || (matches!(plan.status(), crate::plans::PlanStatus::Active)
                && self
                    .child_in_flight
                    .contains_key(&(account, plan.code().clone())))
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
            // D3b 件 3：Paused 计划放行到报价决策（共享 decide_plan_quote
            /// 的暂停分支产出 Cancel——定向撤单路由）；terminal 跳过。
            /// 在途票跳过仅对 Active（防重复挂单）——Paused+在途恰是
            /// Cancel 的触发形态，不得跳过。
            if matches!(
                plan.status(),
                crate::plans::PlanStatus::Completed | crate::plans::PlanStatus::Terminated { .. }
            ) || (matches!(plan.status(), crate::plans::PlanStatus::Active)
                && self
                    .child_in_flight
                    .contains_key(&(account, plan.code().clone())))
            {
                continue;
            }
            let Some(remaining) = plan.remaining_share_qty() else {
                continue;
            };
            // D3b 件 3：暂停/撤买前置分支（session generate_next_plan_quote
            /// 头段的组内等价——共享 decide_plan_quote 不含该面）：Paused
            /// 或（买方向且非反手信号 ≤0）→ Cancel/Keep/Wait + 撤买终止。
            /// 位置在预算查找**之前**（撤单不是下单——不依赖软预算分配；
            /// Sell 计划 sellable 不足时无 grant，暂停撤单不得被预算挡住，
            /// session 头段同序）。
            let code = plan.code();
            {
                let cancellable_now = phase_is_continuous(
                    snapshot.tick,
                    self.ticks_per_day,
                    projection.auction_ticks,
                    projection.closing_auction_ticks,
                );
                let reversing_to_sell = plan.direction() == Side::Buy
                    && plan.opinion().signal_score_bp
                        <= -(plan_book_policy_reverse_threshold(self) as i32);
                let withdraw = plan.direction() == Side::Buy
                    && plan.opinion().signal_score_bp <= 0
                    && !reversing_to_sell;
                let paused = matches!(plan.status(), crate::plans::PlanStatus::Paused { .. });
                if paused || withdraw {
                    match plan.active_child_order_id() {
                        Some(order_id) if cancellable_now => {
                            let serial = order_id.0 & 0x0000_FFFF_FFFF_FFFF;
                            cancels.push((local, code.0 as StockIdx, serial));
                        }
                        _ => {}
                    }
                    if withdraw {
                        // 撤买终止（session 的 Terminate 动作组内等价）——
                        /// 借用冲突，收集后循环外 apply。
                        withdrawals.push(plan_id);
                    }
                    continue;
                }
            }
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
            match decision.action {
                crate::plans::QuoteAction::Submit { price, qty } => {
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
                crate::plans::QuoteAction::Cancel { order_id } => {
                    // D3b 件 3：定向撤单路由（OrderRef 合成逆——serial=
                    // 低 48 位；票保留至 Closed 释放）。
                    let serial = order_id.0 & 0x0000_FFFF_FFFF_FFFF;
                    cancels.push((local, stock_idx as StockIdx, serial));
                }
                _ => {}
            }
        }
        // D3b 件 3：撤买终止（session Terminate 动作的组内等价——
        // 撤信号 ≤0 的买计划终止；子单 Cancel 已入 cancels）。
        for plan_id in withdrawals {
            self.save_plan_book_undo();
            let _ = self.plan_book.apply(
                plan_id,
                crate::plans::PlanEvent::Terminated {
                    reason: crate::plans::TerminationReason::Cancelled,
                    trading_day: snapshot.tick / self.ticks_per_day,
                },
            );
        }
        // 执行腿登记（借用外——D3a-3：挂出即登记，execution 腿在=后续轮
        // 报价跳过——同计划不重复挂子单；remaining 校验/成交推进见
        // apply_staging 的 fills 段）。expires 近似=当日市场分钟上限
        //（挂单 TTL 域登记——撤销/到期释放的腿清理随撤销路径补）。
        for (plan_id, code, stock, side, price, qty) in submits {
            let prev = self.child_in_flight.insert(
                (account, code.clone()),
                ChildTicket {
                    qty,
                    side,
                    price: price.cents(),
                    order_id: None,
                    // Sol 二审修复集 2/四审修复 1：实时余量/desk 扣减起步值。
                    remaining: qty,
                    charged_cents: 0,
                },
            );
            // 在途卖余量镜像由 run_round 挂出臂统一维护（Sol 五审：
            // 计划链子单与外部/retail 单同口径——此处再加会双计）。
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
        Ok((ops, cancels))
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
        // D3b 件 3：belief 链定向撤单目标（决策块内收集，缓冲创建后下发）。
        let mut chain_cancels: Vec<(LocalIdx, StockIdx, u64)> = Vec::new();
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
                    let (_record, child_ops, cancels) =
                        self.run_chain_decision(local, tick, true)?;
                    for op in child_ops {
                        pendings.push(Pending::Op(op));
                    }
                    chain_cancels.extend(cancels);
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
        // D3b 件 3：定向撤单路由（DeskIn::Cancel{owner, order}——与本轮
        /// 下单共用分段 flush 缓冲；票保留至 Closed 实体释放——identity
        /// 匹配的 release 路径已有）。
        for (local, stock, serial) in chain_cancels {
            buffers[stock as usize].push(DeskIn::Cancel {
                owner: (group, local),
                order: (stock, serial),
            });
        }
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
                    // Sol 五审：在途买占用镜像（全部买单——公开现金口径
                    // 的加回面；成交/退款由 apply_staging 实体驱动减）。
                    acct.buy_purse_held_cents += purse;
                    self.undo.push(GroupUndo::Cash {
                        local: p.local,
                        delta: -purse,
                    });
                    self.undo.push(GroupUndo::BuyPurseHeld {
                        local: p.local,
                        delta: purse,
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
                    // Sol 五审：在途卖余量镜像（全部卖单——公开持仓
                    // "在手+在途"口径；成交/退款实体驱动减）。
                    acct.pending_sell_qty[p.stock as usize] += p.qty;
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
                    self.undo.push(GroupUndo::PendingSell {
                        local: p.local,
                        stock: p.stock,
                        delta: i64::from(p.qty),
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

/// D3b 件 4：组线程复审宿主（PlanReviewHost 的组内实现——输入全部
/// 组权威：账户（成本基/T+1/在途）、belief 簿（experience/policy）、
/// 快照行情、观察基分钟序列、投影策略面）。接线于 run_chain_decision
/// （observe 之后、开仓循环之前）；quiet 门控校准=apply_lifecycle_action
/// 内的 record_review（session 每动作前同式记录，比对 last_review_* 基线）。
struct GroupReviewHost<'a> {
    state: &'a GroupState,
    local: LocalIdx,
    account: crate::AccountId,
    market: crate::strategy::MarketView,
    /// 复审时刻的分钟收盘（observation_inputs 预转换——run_chain_decision
    /// 组装时映射复审候选股；价格路径观察输入）。
    minutes_by_code: std::collections::BTreeMap<crate::StockId, Vec<crate::observation::MarketMinuteClose>>,
    /// 复审时刻（ExperienceMoment 组装——快照 civil+市场分钟）。
    moment: crate::experience::ExperienceMoment,
}

impl<'a> crate::session::PlanReviewHost for GroupReviewHost<'a> {
    fn urgency_at_view(
        &self,
        plan: &crate::plans::TradingPlan,
        return_30min_bp: Option<i32>,
        return_1min_bp: Option<i32>,
    ) -> (
        crate::plans::UrgencyAssessment,
        crate::plans::urgency::risk::RiskUrgencyAssessment,
    ) {
        group_urgency_at_view(
            &self.state.accounts,
            &self.state.belief_states,
            &self.state.child_in_flight,
            self.state.root_projection.as_ref().expect("复审位投影在"),
            &self.state.config,
            self.local,
            &self.market,
            &self.moment,
            plan,
            return_30min_bp,
            return_1min_bp,
        )
    }
    fn review_facts(&self, _account: crate::AccountId, code: &crate::StockId) -> (Money, u32) {
        let price = self
            .market
            .stocks
            .get(code)
            .map(|v| v.last_price)
            .unwrap_or_else(|| panic!("组内复审缺股 {code:?} 行情"));
        // acquired 数：belief 个人态的 information 记录计数（issuer 映射
        // 经 Belief-1b 载荷——组内 issuer_map 快照在 world 侧；复审面用
        // belief.information 的公司记录全集近似（ issuer 单公司一股——
        /// 主键面等价）。
        let information = &self
            .state
            .belief_states
            .get(self.local as usize)
            .and_then(|s| s.as_ref())
            .expect("复审位 belief 态在")
            .information;
        let _ = information;
        (price, 0)
    }
    fn review_resources(
        &self,
        _account: crate::AccountId,
        code: &crate::StockId,
    ) -> crate::plans::ReviewResources {
        let acct = &self.state.accounts[self.local as usize];
        let idx = code.0 as usize;
        crate::plans::ReviewResources {
            cash: Money::from_cents(acct.cash),
            frozen_cash: crate::money::Money::ZERO,
            held_qty: acct.shares.get(idx).copied().unwrap_or(0),
            t1_locked: acct.frozen_today.get(idx).copied().unwrap_or(0),
        }
    }
    fn child_cancellable_now(&self) -> bool {
        let projection = self.state.root_projection.as_ref().expect("复审位投影在");
        let tick = self.state.cur_tick;
        phase_is_continuous(
            tick,
            self.state.ticks_per_day,
            projection.auction_ticks,
            projection.closing_auction_ticks,
        )
    }
    fn initial_urgency(&self, direction: Side, confidence_bp: u32, horizon: u32) -> crate::plans::Urgency {
        let style = self.belief_style();
        initial_group_urgency(
            direction,
            confidence_bp,
            horizon,
            style,
            &self
                .state
                .root_projection
                .as_ref()
                .expect("复审位投影在")
                .urgency_policy,
        )
    }
    fn minute_closes(&self, code: &crate::StockId) -> &[crate::observation::MarketMinuteClose] {
        // 组内等价：observation_inputs 的分钟序列预转换（与 build_
        /// observations 同式 c.max(1)——字段不得分叉）；非候选股兜底
        /// 空切片（session 版 get().unwrap_or(&[]) 同式）。
        self.minutes_by_code
            .get(code)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
    fn belief_style(&self) -> Option<crate::strategy::InstitutionStyle> {
        match self
            .state
            .belief_states
            .get(self.local as usize)
            .and_then(|s| s.as_ref())
            .expect("复审位 belief 态在")
            .belief
            .profile()
        {
            crate::strategy::StrategyProfile::Institution(style) => Some(*style),
            _ => None,
        }
    }
    fn phase_continuous(&self) -> bool {
        self.child_cancellable_now()
    }
    fn urgency_policy(&self) -> &crate::plans::UrgencyPolicy {
        &self
            .state
            .root_projection
            .as_ref()
            .expect("复审位投影在")
            .urgency_policy
    }
    fn institution_behavior(
        &self,
        belief: &crate::strategy::BeliefBook,
        code: &crate::StockId,
        paused: Option<crate::plans::PauseReason>,
    ) -> crate::session::InstitutionBehavior {
        group_institution_behavior(
            &self.state.accounts,
            self.local,
            &self.market,
            &self.moment,
            belief,
            code,
            paused,
        )
    }
    fn max_holdable_shares(&self, code: &crate::StockId) -> u64 {
        self.state
            .root_projection
            .as_ref()
            .and_then(|p| p.total_shares.get(code).copied())
            .unwrap_or(0)
            .min(u64::from(u32::MAX))
    }
}

/// 机构行为评估（组内输入组装：policy/experience/成本基/现价/时刻——
/// 复审宿主与开仓后复评共用；与 session `institution_behavior` 同式：
/// cost_price 自组账目推导（D3a-2c 件1 投影对账背书）、account_risk 面
/// 同式 belief 权威。参数=组账户切片（开仓循环在簿可变借用旁调用）。
fn group_institution_behavior(
    accounts: &[GroupAccount],
    local: LocalIdx,
    market: &crate::strategy::MarketView,
    moment: &crate::experience::ExperienceMoment,
    belief: &crate::strategy::BeliefBook,
    code: &crate::StockId,
    paused: Option<crate::plans::PauseReason>,
) -> crate::session::InstitutionBehavior {
    let policy = belief
        .institution_policy()
        .expect("机构 belief 簿有冻结行为策略");
    let experience = belief.experience();
    let acct = &accounts[local as usize];
    let idx = code.0 as usize;
    let cost = crate::account::Position::from_restored_parts(
        acct.shares.get(idx).copied().unwrap_or(0),
        acct.frozen_today.get(idx).copied().unwrap_or(0),
        acct.invested_cents.get(idx).copied().unwrap_or(0),
        acct.recovered_cents.get(idx).copied().unwrap_or(0),
    )
    .cost_price()
    .filter(|c| c.cents() > 0);
    let price = market
        .stocks
        .get(code)
        .map(|v| v.last_price)
        .unwrap_or_else(|| panic!("组内行为评估缺股 {code:?} 行情"));
    crate::session::assess_institution_behavior(
        policy,
        experience,
        code,
        cost,
        price,
        *moment,
        paused,
        if belief.institution_account_risk_paused() {
            Some(true)
        } else {
            experience.peak_equity.map(|_| false)
        },
    )
    .expect("组内机构行为评估失败")
}

/// 组内报价/复审期紧迫度+风险评估（session `plan_execution_urgency_at_
/// view` 的组内等价——字段级显式参数：开仓后复评在簿可变借用旁调用，
/// 经宿主委托或直接传字段）。
#[allow(clippy::too_many_arguments)]
fn group_urgency_at_view(
    accounts: &[GroupAccount],
    belief_states: &[Option<super::world::BeliefParticipantSnapshot>],
    child_in_flight: &std::collections::BTreeMap<
        (crate::AccountId, crate::StockId),
        ChildTicket,
    >,
    projection: &std::sync::Arc<super::world::RootProjection>,
    config: &std::sync::Arc<crate::config::GameConfig>,
    local: LocalIdx,
    market: &crate::strategy::MarketView,
    moment: &crate::experience::ExperienceMoment,
    plan: &crate::plans::TradingPlan,
    return_30min_bp: Option<i32>,
    return_1min_bp: Option<i32>,
) -> (
    crate::plans::UrgencyAssessment,
    crate::plans::urgency::risk::RiskUrgencyAssessment,
) {
    use crate::plans::urgency::risk::{
        assess_existing_reduction_urgency, RiskUrgencyAssessment,
    };
    use crate::plans::UrgencyInputs;
    let acct = &accounts[local as usize];
    let belief = &belief_states
        .get(local as usize)
        .and_then(|s| s.as_ref())
        .expect("复审位 belief 态在")
        .belief;
    let experience = belief.experience();
    let risk = match experience.peak_equity {
        None => RiskUrgencyAssessment::Unavailable {
            reason: "no own-observed institution equity peak".to_owned(),
        },
        Some(peak) => {
            // 权益观察（含在途口径——组读面）。
            let equity = equity_cents_with_pending(
                acct,
                child_in_flight,
                &projection.account_ids,
                local,
                config,
                market,
            );
            let mut positions: std::collections::BTreeMap<
                crate::StockId,
                crate::observation::RiskPositionInput,
            > = std::collections::BTreeMap::new();
            for (idx, &qty) in acct.shares.iter().enumerate() {
                // 零量条目不入风险观察（RiskPositionInput 要求 qty>0
                /// ——组侧曾持有/清仓面在 shares 中为 0）。
                if qty == 0 {
                    continue;
                }
                let price = market
                    .stocks
                    .get(&crate::StockId(idx as u32))
                    .map(|v| v.last_price)
                    .unwrap_or(crate::money::Money::from_cents(1));
                let code = crate::StockId(idx as u32);
                positions.insert(
                    code,
                    crate::observation::RiskPositionInput {
                        qty,
                        cost_price: crate::account::Position::from_restored_parts(
                            qty,
                            acct.frozen_today[idx],
                            acct.invested_cents[idx],
                            acct.recovered_cents[idx],
                        )
                        .cost_price()
                        .filter(|c| c.cents() > 0),
                        last_price: price,
                        peak_price_since_entry: experience
                            .stocks
                            .get(&code)
                            .and_then(|s| s.peak_price_since_entry)
                            .map(|observed| observed.max(price)),
                    },
                );
            }
            let observed = crate::observation::build_account_risk_observation(
                crate::money::Money::from_cents(cash_with_pending_at(
                    acct,
                    child_in_flight,
                    &projection.account_ids,
                    local,
                    config,
                )),
                &positions,
                experience.reference_equity,
                Some(peak.max(equity)),
            )
            .expect("组内机构风险观察构建失败");
            let reducing = plan.direction() == Side::Sell
                && plan.remaining_share_qty().is_some_and(|qty| qty > 0)
                && acct.shares.get(plan.code().0 as usize).is_some_and(|q| *q > 0);
            assess_existing_reduction_urgency(
                reducing,
                &observed,
                &projection.urgency_policy,
            )
            .expect("组内计划风险紧迫度失败")
        }
    };
    let (risk_reduction_active, account_drawdown_bp) = match &risk {
        RiskUrgencyAssessment::Assessed {
            risk_reduction_active,
            account_drawdown_bp,
            ..
        } => (*risk_reduction_active, *account_drawdown_bp),
        RiskUrgencyAssessment::Unavailable { .. } => (false, None),
    };
    let behavior = group_institution_behavior(
        accounts,
        local,
        market,
        moment,
        belief,
        plan.code(),
        match plan.status() {
            crate::plans::PlanStatus::Paused { reason } => Some(reason),
            _ => None,
        },
    );
    let style = match belief.profile() {
        crate::strategy::StrategyProfile::Institution(style) => Some(style),
        _ => None,
    };
    let inputs = UrgencyInputs {
        side: plan.direction(),
        return_30min_bp,
        return_1min_bp,
        risk_pressure_pause: plan.direction() == Side::Buy
            && behavior.risk_pressure_pause == Some(true),
        adverse_selection_pause: plan.direction() == Side::Buy && behavior.adverse_selection_pause,
        risk_reduction_active,
        account_drawdown_bp,
        remaining_trading_days: u32::try_from(
            plan.last_valid_trading_day().saturating_sub(moment.trading_day),
        )
        .expect("live plan remaining days fit its u32 horizon"),
        confidence_bp: plan.confidence_bp(),
        style: match style {
            Some(crate::strategy::InstitutionStyle::DeepValue) => {
                crate::plans::PatienceStyle::DeepValue
            }
            _ => crate::plans::PatienceStyle::Other,
        },
    };
    let urgency =
        crate::plans::assess_urgency(&inputs, &projection.urgency_policy)
            .expect("组内计划紧迫度评估失败");
    (urgency, risk)
}

/// 组簿反手阈值（D3b 件 3 辅助——plan_book.policy() 的只读便捷）。
fn plan_book_policy_reverse_threshold(state: &GroupState) -> i32 {
    state.plan_book.policy().reverse_revision_threshold_bp
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
    projection: &std::sync::Arc<crate::runtime::world::RootProjection>,
    library: &std::sync::Arc<crate::information::PublicLibrary>,
    plans: &crate::plans::PlanBook,
    child_in_flight: &std::collections::BTreeMap<
        (crate::AccountId, crate::StockId),
        ChildTicket,
    >,
    config: &std::sync::Arc<crate::config::GameConfig>,
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
        // Sol 整改 3：含在途现金（买 purse 加回——挂单期间权益观察不
        /// 缩水，对齐旧管线 escrow 口径）。
        let mut account = crate::account::Account::new(
            account_id,
            crate::account::AccountKind::Inst,
            crate::money::Money::from_cents(cash_with_pending_at(
                group_account,
                child_in_flight,
                &projection.account_ids,
                local as LocalIdx,
                config,
            )),
        );
        let mut positions = std::collections::BTreeMap::new();
        for (stock, &qty_raw) in group_account.shares.iter().enumerate() {
            // Sol 整改 3：持仓含在途卖单余量（旧管线口径=在手+在途——
            /// 组 shares 已扣；qty 口径差收口）。零持仓仅在曾持有或在途
            /// 卖时建条目（held 集成员参与发现采样）。
            let qty = held_qty_with_pending_at(
                &group_account.shares,
                child_in_flight,
                &projection.account_ids,
                local as LocalIdx,
                stock,
            );
            if qty == 0 && qty_raw == 0 && !group_account.ever_held[stock] {
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
    let market_minute = market_minute_at(tick, ticks_per_day, projection.auction_ticks, projection.closing_auction_ticks);
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

/// 市场分钟（与 session `current_market_minute` 同式：交易日×240 +
/// 连续段完成分钟——completed_market_minute_count 为共享纯函数；复审
/// 时刻 ExperienceMoment 与 RootReadContext 共用此式，字段不得分叉）。
fn market_minute_at(tick: u64, ticks_per_day: u64, auction_ticks: u64, closing_ticks: u64) -> u64 {
    let day = u32::try_from(tick / ticks_per_day).expect("交易日序号超 u32");
    let day_tick = tick % ticks_per_day;
    let continuous = ticks_per_day
        .saturating_sub(auction_ticks)
        .saturating_sub(closing_ticks);
    u64::from(day) * u64::from(crate::observation::GAME_INTRADAY_MINUTES_PER_DAY)
        + u64::from(
            crate::observation::completed_market_minute_count(
                day_tick.saturating_sub(auction_ticks).min(continuous),
                continuous,
            )
            .expect("相位配置映射市场分钟（与 session 同式）"),
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
    library: &std::sync::Arc<crate::information::PublicLibrary>,
    company_registry: &std::sync::Arc<crate::company::CompanyRegistry>,
    stock_codes: &[(crate::account::StockCode, crate::StockId)],
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
    // Sol 整改 5：曝光面收口（此前空集——生产发现路径丢披露曝光权重；
    /// 与 session 版同一共享函数 exposed_stocks_shared）。
    let exposed =
        crate::session::exposed_stocks_shared(library, company_registry, stock_codes, now);
    crate::session::DecisionChainObservation {
        market,
        paths,
        technical,
        now,
        exposed,
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
            GroupIn::ChildAccepted {
                stock,
                owner,
                order_serial,
            } => {
                // Sol 整改受理通道：受理即身份（OrderRef 合成——不等首笔
                // fill；adopt 数量=挂出量）。非本组/非计划链单（无票）跳过。
                if owner.0 == state.group {
                    state
                        .observed
                        .push(Observed::ChildAccepted(stock, owner, order_serial));
                    if let Err(e) = state.adopt_child_on_accepted(stock, owner.1, order_serial) {
                        abort.store(true, Ordering::SeqCst);
                        let _ = up_tx.send(UpMsg::Failed { who, error: e.clone() });
                        result = Err(e);
                        break;
                    }
                }
            }
            #[cfg(test)]
            GroupIn::InstallPersonal { local, snapshot } => {
                // D3b 件 5：个人态注入——组权威覆盖（Personal undo 可逆；
                /// Abort 冲回后需重发）。缺位/越界=注入面漂移，显式失败。
                let prev = state.belief_states.get(local as usize).cloned().flatten();
                match prev {
                    Some(prev) => {
                        state.undo.push(GroupUndo::Personal {
                            local,
                            prev: Box::new(prev),
                        });
                        if let Some(slot) = state.belief_states.get_mut(local as usize) {
                            *slot = Some(snapshot);
                        }
                    }
                    None => {
                        abort.store(true, Ordering::SeqCst);
                        let e = RuntimeError::ProtocolInvariant {
                            detail: format!(
                                "组 {} 个人态注入目标位 {local} 缺信念态（非机构位或越界）",
                                state.group
                            ),
                        };
                        let _ = up_tx.send(UpMsg::Failed { who, error: e.clone() });
                        result = Err(e);
                        break;
                    }
                }
            }
            #[cfg(test)]
            GroupIn::ApplyPlanEvents { events } => {
                // D3b 复审注入通道：逐条 apply 到组簿（权威语义由
                // plan_book.apply 承载；簿快照 undo 可逆——Abort 冲回后
                // 指令失效需重发）。
                for (plan_id, event) in events {
                    state.save_plan_book_undo();
                    let applied = state.plan_book.apply(plan_id, event);
                    if let Err(error) = applied {
                        abort.store(true, Ordering::SeqCst);
                        let e = RuntimeError::ProtocolInvariant {
                            detail: format!(
                                "组 {} 注入计划事件失败 {plan_id:?}：{error:?}",
                                state.group
                            ),
                        };
                        let _ = up_tx.send(UpMsg::Failed { who, error: e.clone() });
                        result = Err(e);
                        break;
                    }
                }
            }
            #[cfg(test)]
            GroupIn::ForceDue { locals, tick } => {
                // Sol 整改注入通道（cfg(test) 语义）：显式强制到期——组
                // 验证目标 tick 晚于已转正 tick 后登记待办；效果延迟到
                // 下一 Start 应用（Sol 二审修复集 4：归下一 tick 的日志
                // 域——Abort 冲回强制前值，指令失效需重发）。
                if tick <= state.committed_tick {
                    abort.store(true, Ordering::SeqCst);
                    let e = RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "组 {} ForceDue 目标 tick {tick} 不晚于已转正 {}（过期指令拒绝）",
                            state.group, state.committed_tick
                        ),
                    };
                    let _ = up_tx.send(UpMsg::Failed { who, error: e.clone() });
                    result = Err(e);
                    break;
                }
                for local in locals {
                    state.pending_force_due.push((local, tick));
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
                                Ok((record, _, _)) => derivations.push(record),
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
                // 在途买占用为账户级维护字段（Sol 五审：run_round 挂出/
                // apply_staging 成交与退款驱动——全部买单，不限计划链票）。
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

/// Sol 二审修复集 3 红证（受理身份关联）：组内最小决策链态直调
/// apply_staging——外来单的成交/终结事实不得触碰本票执行腿。
#[cfg(test)]
mod sol_recheck3_tests {
    use super::*;

    /// 最小组态：单账户单股，手工置入计划+执行腿+已受理在途票
    /// （身份=order_id_of((0, 5))——serial 5 为本票，他单用其他 serial）。
    fn chain_harness(side: Side) -> GroupState {
        chain_harness_target(side, 200)
    }

    /// 同 chain_harness，但计划目标/执行腿/子单量可变（terminal fixture
    /// 用小目标 100——全额成交即 Completed，不依赖巨量成交）。
    fn chain_harness_target(side: Side, target: u32) -> GroupState {
        let mut state = GroupState::new(
            0,
            vec![GroupAccount::new(10_000_000, 1)],
            8,
            100,
            std::sync::Arc::new(crate::config::GameConfig::proposed_defaults()),
            240,
            None,
            Vec::new(),
            Box::new(|_t, _r| TickPlan::default()),
        );
        state.cur_tick = 1;
        state.cur_round = 0;
        state.root_projection = Some(std::sync::Arc::new(crate::runtime::world::RootProjection {
            catalog: crate::stock::StockCatalog::from_metas(vec![crate::stock::StockMeta::new(
                "600000",
            )])
            .expect("目录构造"),
            company_registry: std::sync::Arc::new(
                crate::company::CompanyRegistry::new(Vec::new()).expect("空注册表"),
            ),
            operations: std::sync::Arc::new(
                crate::company::operations::CompanyOperations::new(
                    crate::company::operations::CompanyOperationsConfig {
                        seed: 1,
                        shock_params: crate::company::events::ShockParams::current_default_parameters(),
                        catalog: crate::company::CompanyCatalog::from_codes(&[])
                            .expect("空公司目录"),
                        companies: Vec::new(),
                    },
                    crate::calendar::CivilDate::from_ymd(2026, 1, 5).expect("日期合法"),
                )
                .expect("空经营"),
            ),
            account_ids: vec![crate::AccountId(9)],
            auction_ticks: 0,
            closing_auction_ticks: 3,
            chain_params: vec![None],
            urgency_policy: crate::plans::UrgencyPolicy::default(),
            total_shares: Default::default(),
            max_order_qty_limit: Default::default(),
            tick_size_cents: Default::default(),
            stock_codes: Vec::new(),
        }));
        let account = crate::AccountId(9);
        let code = crate::StockId(0);
        let order_id = order_id_of(&(0, 5));
        let pid = state
            .plan_book
            .create(crate::plans::PlanOpen {
                account,
                code: code.clone(),
                direction: side,
                target: crate::plans::PlanTarget::ShareCount(target),
                opinion: crate::plans::PlanOpinion {
                    signal_score_bp: 3_000,
                    source: crate::plans::OpinionSource::Blended,
                },
                confidence_bp: 8_000,
                urgency: crate::plans::Urgency::Normal,
                horizon_trading_days: 5,
                created_trading_day: 0,
            })
            .expect("计划创建");
        state
            .plan_book
            .install_execution(pid, target, crate::money::Money::from_cents(1_000), 240)
            .expect("执行腿登记");
        state
            .plan_book
            .record_child_submitted(account, &code, side, order_id, target, 0)
            .expect("子单身份登记");
        state.child_in_flight.insert(
            (account, code),
            ChildTicket {
                qty: target,
                side,
                price: 1_000,
                order_id: Some(order_id),
                remaining: target,
                charged_cents: 0,
            },
        );
        state
    }

    // Sol 三审修复 3：terminal 记录小型确定性 fixture——target=100 的买
    // 计划经**全额成交**（filled==target）到 Completed（不依赖巨量成交或
    // D3b 复审终止路径）；腿清空、在途票释放（EchoPlans 的组簿内容面）。
    #[test]
    fn sol4th_full_fill_completes_plan_to_terminal() {
        let mut state = chain_harness_target(Side::Buy, 100);
        state.accounts[0].cash = 1_000_000;
        state.staging.push(batch(
            Vec::new(),
            vec![FillFact {
                order: (0, 5),
                owner: (0, 0),
                side: Side::Buy,
                price: 900,
                qty: 100,
                commission: 500,
                stamp_tax: 0,
                transfer_fee: 1,
            }],
        ));
        state
            .apply_staging(1, 0)
            .expect("全额成交正常推进");
        // terminal 记录不再出现在 active 面——按 id 直查（记录保留在簿）。
        let plan = state
            .plan_book
            .plan_ids()
            .find_map(|pid| {
                state
                    .plan_book
                    .plan(pid)
                    .ok()
                    .filter(|p| p.account() == crate::AccountId(9))
            })
            .expect("terminal 计划记录仍在簿");
        assert!(
            matches!(plan.status(), crate::plans::PlanStatus::Completed),
            "filled==target ⇒ Completed（实际 {:?}）",
            plan.status()
        );
        assert!(
            plan.execution().is_none(),
            "terminal 后执行腿清空（无悬挂在途）"
        );
        assert!(
            plan.active_child_order_id().is_none(),
            "terminal 后在途子单身份清空"
        );
        assert!(
            !state
                .child_in_flight
                .contains_key(&(crate::AccountId(9), crate::StockId(0))),
            "全成票释放（下一轮可再报价）"
        );
    }

    fn batch(entities: Vec<Entity>, fills: Vec<FillFact>) -> DeskBatch {
        DeskBatch {
            stock: 0,
            tick: 1,
            round: 0,
            entities,
            fills,
            receipts: Vec::new(),
        }
    }

    // 3b：票已有身份（serial 5）时，外来单（serial 77）的成交事实必须
    // 显式失败——此前以票身份推进外来单的成交量（执行腿被静默吃掉）。
    #[test]
    fn sol3_fill_with_foreign_order_fails_instead_of_eating_the_leg() {
        let mut state = chain_harness(Side::Buy);
        state.staging.push(
            batch(
                Vec::new(),
                vec![FillFact {
                    order: (0, 77),
                    owner: (0, 0),
                    side: Side::Buy,
                    price: 1_000,
                    qty: 50,
                    commission: 0,
                    stamp_tax: 0,
                    transfer_fee: 0,
                }],
            ),
        );
        // Sol 六审勘正：外来同键单成交=合法形态（意图路由/测试注入）——
        /// 跳过不误挂（Ok 且余量不动），不再显式失败（原 Err 形态实测会
        /// 杀死合法外来单的拍——sol7th_foreign 场景红证）。
        state
            .apply_staging(1, 0)
            .expect("外来单成交跳过（不构成失败）");
        let plan = state
            .plan_book
            .active_plan(crate::AccountId(9), &crate::StockId(0))
            .expect("计划仍在簿");
        assert_eq!(
            plan.active_child_remaining_qty(),
            Some(200),
            "执行腿余量不被外来单吃掉（跳过——原缺陷形态为静默吃掉 50）"
        );
    }

    // 2：挂 200 成 100 后——组持仓读面加回**真实未成交余量**（100），
    // 不再按原挂出量（200）——与 session 镜像（pending_sell_qty）同口径。
    #[test]
    fn sol2_held_read_face_tracks_remaining_after_partial_fill() {
        let mut state = chain_harness(Side::Sell);
        // 卖出扣除后（挂单在途）：在手 0；挂出时镜像加 200。
        state.accounts[0].shares[0] = 0;
        state.accounts[0].pending_sell_qty[0] = 200;
        state.staging.push(batch(
            Vec::new(),
            vec![FillFact {
                order: (0, 5),
                owner: (0, 0),
                side: Side::Sell,
                price: 1_000,
                qty: 100,
                commission: 0,
                stamp_tax: 50,
                transfer_fee: 1,
            }],
        ));
        state
            .apply_staging(1, 0)
            .expect("本单部分成交正常推进");
        // session 镜像口径（fill 推进减 fill.qty）。
        assert_eq!(state.accounts[0].pending_sell_qty[0], 100, "镜像减成交部分");
        // 组读面必须同口径：在手(0)+在途余量(100)=100——原挂出量口径=200。
        let held = held_qty_with_pending_at(
            &state.accounts[0].shares,
            &state.child_in_flight,
            &[crate::AccountId(9)],
            0,
            0,
        );
        assert_eq!(
            held, 100,
            "组持仓读面=在手+真实未成交余量（与 session 镜像一致）"
        );
        // 票余量与镜像一致（三面统一）。
        let ticket = state
            .child_in_flight
            .get(&(crate::AccountId(9), crate::StockId(0)))
            .expect("部分成交票保留");
        assert_eq!(ticket.remaining, 100, "票实时余量=挂出−成交");
        // Sol 四审修复 1：卖腿不占 purse（charged 仅买腿累计）。
        assert_eq!(ticket.charged_cents, 0, "卖腿无 desk purse 扣减累计");
    }

    // Sol 四审修复 1 红证：live purse 按 **desk 实际占用** 维护——价优
    // 部分成交后 desk 仍持有价优差额（三审反例：限价 1000 挂 200、100 股
    // 以 900 成交：purse0=200502、desk 扣减=90000+500+1=90501、desk 余额
    // =110001；重算下界=100001，漏计 10000）。预期全部由字面 desk 扣减
    // 语义推导（总现金−Σcharge 与 加回值= purse0−Σcharge），不复用实现
    // 的费用 helper。
    #[test]
    fn sol4th_buy_purse_tracks_desk_occupancy_with_price_improvement() {
        let mut state = chain_harness(Side::Buy);
        // 挂出扣款：总现金 1_000_000，purse0=200000 对价+500 最低佣金
        // +2 过户费=200502（desk 余额语义独立推导）。
        const PURSE0: i64 = 200_000 + 500 + 2;
        const CHARGE: i64 = 90_000 + 500 + 1;
        state.accounts[0].cash = 1_000_000 - PURSE0;
        state.staging.push(batch(
            Vec::new(),
            vec![FillFact {
                order: (0, 5),
                owner: (0, 0),
                side: Side::Buy,
                price: 900,
                qty: 100,
                commission: 500,
                stamp_tax: 0,
                transfer_fee: 1,
            }],
        ));
        state
            .apply_staging(1, 0)
            .expect("本单部分成交正常推进");
        let face = cash_with_pending_at(
            &state.accounts[0],
            &state.child_in_flight,
            &[crate::AccountId(9)],
            0,
            &state.config,
        );
        assert_eq!(
            face,
            1_000_000 - CHARGE,
            "总现金读面=总现金−desk 实际扣减（价优差额仍在 desk）"
        );
        assert_eq!(
            face - state.accounts[0].cash,
            PURSE0 - CHARGE,
            "在途 purse 加回=desk 余额 110001（价优差额不漏计——重算下界=100001 显红）"
        );
        // Abort 验证：票占用随 undo 冲回——读面恢复挂出全额。
        state.rollback();
        let face_after = cash_with_pending_at(
            &state.accounts[0],
            &state.child_in_flight,
            &[crate::AccountId(9)],
            0,
            &state.config,
        );
        assert_eq!(
            face_after - state.accounts[0].cash,
            PURSE0,
            "Abort 冲回 charged——在途 purse 恢复挂出全额"
        );
    }

    // 3c：Closed 按订单身份关联释放——他单（serial 88）的终结不释放本票
    // （serial 5）；本单的 Closed 正常释放（票+执行腿）。
    #[test]
    fn sol3_closed_with_foreign_order_keeps_the_ticket() {
        let mut state = chain_harness(Side::Sell);
        state.accounts[0].foreign_active[0] = 1;
        state
            .staging
            .push(batch(vec![Entity::Closed {
                local: 0,
                order_serial: 88,
            }], Vec::new()));
        state
            .apply_staging(1, 0)
            .expect("他单终结不构成失败（资源实体照常落账）");
        let key = (crate::AccountId(9), crate::StockId(0));
        assert!(
            state.child_in_flight.contains_key(&key),
            "他单的 Closed 不得释放本票（订单身份关联）"
        );
        let plan = state
            .plan_book
            .active_plan(crate::AccountId(9), &crate::StockId(0))
            .expect("计划仍在簿");
        assert!(
            plan.active_child_order_id().is_some(),
            "他单的 Closed 不得释放执行腿"
        );
        // 对照：本单（serial 5）的 Closed 正常释放。
        state.accounts[0].foreign_active[0] = 1;
        state
            .staging
            .push(batch(vec![Entity::Closed {
                local: 0,
                order_serial: 5,
            }], Vec::new()));
        state
            .apply_staging(1, 0)
            .expect("本单终结正常释放");
        assert!(
            !state.child_in_flight.contains_key(&key),
            "本单的 Closed 释放在途票"
        );
    }
// Sol 三审修复 4：未受理票（order_id=None）的终结关联边界——同
    // (账户,股) 唯一挂出守卫下，未受理票的 Closed/拒收回执即本票的终结
    //（desk 对被拒单的 Closed 与回执共用同一 serial；票尚无受理身份可比
    // 对，(账户,股) 释放是唯一可判定关联）。固化现状语义：释放+卖量镜像
    // 回收；若实测发现错误语义即修（本测试为守卫锚）。
    #[test]
    fn sol4th_unaccepted_ticket_releases_on_closed_and_rejection() {
        // Closed 路径（未受理票）。
        let mut state = chain_harness(Side::Sell);
        let key = (crate::AccountId(9), crate::StockId(0));
        state
            .child_in_flight
            .get_mut(&key)
            .expect("票在")
            .order_id = None;
        state.accounts[0].foreign_active[0] = 1;
        state.accounts[0].pending_sell_qty[0] = 200;
        // desk 真实形态：卖单终结 Closed 与余股回流 RefundedShares 成对
        //（镜像回收由 RefundedShares 实体驱动——Sol 五审统一口径）。
        state
            .staging
            .push(batch(
                vec![
                    Entity::RefundedShares {
                        local: 0,
                        qty: 200,
                    },
                    Entity::Closed {
                        local: 0,
                        order_serial: 42,
                    },
                ],
                Vec::new(),
            ));
        state
            .apply_staging(1, 0)
            .expect("未受理票的 Closed 正常释放");
        assert!(
            !state.child_in_flight.contains_key(&key),
            "未受理票的 Closed 释放在途票（唯一挂出守卫下即本票终结）"
        );
        assert_eq!(
            state.accounts[0].pending_sell_qty[0], 0,
            "卖量镜像随释放回收（余量=挂出全额）"
        );
        // 拒收回执路径（未受理票——被拒单从未受理，serial 无身份可比）。
        let mut state = chain_harness(Side::Sell);
        state
            .child_in_flight
            .get_mut(&key)
            .expect("票在")
            .order_id = None;
        state.accounts[0].foreign_active[0] = 1;
        state.accounts[0].pending_sell_qty[0] = 200;
        // desk 拒收形态：RefundedShares（卖单原路退回）+ Closed 与回执
        // 同批（reject() 三件套）。
        state.staging.push(DeskBatch {
            stock: 0,
            tick: 1,
            round: 0,
            entities: vec![
                Entity::RefundedShares {
                    local: 0,
                    qty: 200,
                },
                Entity::Closed {
                    local: 0,
                    order_serial: 43,
                },
            ],
            fills: Vec::new(),
            receipts: vec![crate::runtime::DeskReceipt {
                order: (0, 43),
                owner: (0, 0),
                kind: crate::runtime::ReceiptKind::PriceOutOfBand,
            }],
        });
        state
            .apply_staging(1, 0)
            .expect("未受理票的拒收回执正常释放");
        assert!(
            !state.child_in_flight.contains_key(&key),
            "拒收回执释放在途票（被拒单即本票）"
        );
        assert_eq!(
            state.accounts[0].pending_sell_qty[0], 0,
            "卖量镜像随拒收释放回收"
        );
    }
}


#[cfg(test)]
mod sol_recheck1_tests {
    use super::*;

    /// 双买计划预算竞争 fixture：两股同价 1000（涨停 1100）、各余 100 股
    /// （order_size=100）、现金恰够一条腿（约 11.1 万）——预算分类的
    /// 优先级决定谁挂出子单（NewOpportunity=2 让位 ExistingPlan=1，
    /// 平手按信心倒序）。
    fn competing_plans_state() -> GroupState {
        let mut state = GroupState::new(
            0,
            vec![GroupAccount::new(120_000, 2)],
            8,
            100,
            std::sync::Arc::new(crate::config::GameConfig::proposed_defaults()),
            240,
            None,
            Vec::new(),
            Box::new(|_t, _r| TickPlan::default()),
        );
        state.cur_tick = 1;
        state.cur_round = 0;
        let account = crate::AccountId(9);
        let chain_params = crate::strategy::BeliefChainParams {
            position_step_bp: 1_000,
            margin: 0.05,
            order_size: 100,
            daily_plan_review: false,
        };
        state.root_projection = Some(std::sync::Arc::new(
            crate::runtime::world::RootProjection {
                catalog: crate::stock::StockCatalog::from_metas(vec![
                    crate::stock::StockMeta::new("600000"),
                    crate::stock::StockMeta::new("600001"),
                ])
                .expect("目录构造"),
                company_registry: std::sync::Arc::new(
                    crate::company::CompanyRegistry::new(Vec::new()).expect("空注册表"),
                ),
                operations: std::sync::Arc::new(
                    crate::company::operations::CompanyOperations::new(
                        crate::company::operations::CompanyOperationsConfig {
                            seed: 1,
                            shock_params:
                                crate::company::events::ShockParams::current_default_parameters(),
                            catalog: crate::company::CompanyCatalog::from_codes(&[])
                                .expect("空公司目录"),
                            companies: Vec::new(),
                        },
                        crate::calendar::CivilDate::from_ymd(2026, 1, 5).expect("日期合法"),
                    )
                    .expect("空经营"),
                ),
                account_ids: vec![account],
                auction_ticks: 2,
                closing_auction_ticks: 3,
                chain_params: vec![Some((chain_params, None))],
                urgency_policy: crate::plans::UrgencyPolicy::default(),
                total_shares: Default::default(),
                max_order_qty_limit: [
                    (crate::StockId(0), 1_000_000),
                    (crate::StockId(1), 1_000_000),
                ]
                .into_iter()
                .collect(),
                tick_size_cents: [
                    (crate::StockId(0), 1),
                    (crate::StockId(1), 1),
                ]
                .into_iter()
                .collect(),
                stock_codes: Vec::new(),
            },
        ));
        // 计划 A（股 0，信心 3000）与计划 B（股 1，信心 9000）。
        for (stock, confidence) in [(0u32, 3_000u32), (1, 9_000)] {
            state
                .plan_book
                .create(crate::plans::PlanOpen {
                    account,
                    code: crate::StockId(stock),
                    direction: Side::Buy,
                    target: crate::plans::PlanTarget::ShareCount(100),
                    opinion: crate::plans::PlanOpinion {
                        signal_score_bp: 5_000,
                        source: crate::plans::OpinionSource::Blended,
                    },
                    confidence_bp: confidence,
                    urgency: crate::plans::Urgency::Normal,
                    horizon_trading_days: 5,
                    created_trading_day: 0,
                })
                .expect("计划创建");
        }
        state
    }

    fn snapshot_arc() -> std::sync::Arc<TickSnapshot> {
        let stock = |last: i64| super::super::protocol::StockPublic {
            last_price: last,
            best_bid: None,
            best_ask: None,
            max_buy_price: last * 11 / 10,
            daily_upper_limit: last * 11 / 10,
            min_sell_price: last * 9 / 10,
            recent_prices: Vec::new(),
            minute_closes: Vec::new(),
            daily_bars: Vec::new(),
            daily_active: None,
            daily_total_completed: 0,
            daily_total_traded: 0,
            volume_today: 0,
            relative_volume: 1.0,
            order_book_imbalance: 0.0,
            depth_bids: Vec::new(),
            depth_asks: Vec::new(),
        };
        std::sync::Arc::new(TickSnapshot {
            // tick 0 < auction_ticks=2 ⇒ 竞价相位（空簿回退 last 价——
            // decide_plan_quote 可 Submit，无需估值条目）。
            tick: 0,
            market_minute: 0,
            civil_date: crate::calendar::CivilDate::from_ymd(2026, 1, 5).expect("日期合法"),
            stocks: std::sync::Arc::new(vec![stock(1_000), stock(1_000)]),
        })
    }

    fn personal(account: crate::AccountId) -> crate::session::PlanPersonalState {
        let mut rng = crate::session::SplitMix64::new(7);
        let profile =
            crate::strategy::StrategyProfile::Institution(crate::strategy::InstitutionStyle::Balanced);
        let analysis =
            crate::strategy::derive_analysis_profile(&profile, account, &mut rng).unwrap();
        crate::session::PlanPersonalState {
            attention: crate::session::NpcAttentionState {
                information_cadence: crate::session::notices::NpcInformationCadence::Immediate,
                next_information_check: crate::calendar::CivilInstant::new(
                    crate::calendar::CivilDate::from_ymd(2026, 1, 5).expect("日期合法"),
                    0,
                )
                .expect("午夜有效"),
                base_probability: 1.0,
                next_attention_candidate_tick: 0,
                rng_state: 1,
            },
            watchlist: crate::experience::PersonalWatchlist::new(),
            price_memory: crate::experience::PersonalPriceMemory::default(),
            history_reads: crate::experience::PersonalHistoryReadLedger::default(),
            information: crate::information::NpcInformationState::new(account),
            belief: crate::strategy::BeliefBook::new(account, profile, analysis, &mut rng),
        }
    }

    // Sol 二审修复集 1：existing_before 成员在现金受限时优先获预算——
    // 同轮新开（不在集合内）=NewOpportunity 让位；若同轮新开被错误归入
    // existing（修复前调用点的实际行为——采集点在开仓循环之后），平手
    // 按信心倒序，高信心新计划抢走既有计划的预算。固化"分类→优先级→
    // 资金→挂出股"的可观察链（调用点采集时点见 run_chain_decision 注记）。
    #[test]
    fn sol1_quote_classification_prefers_pre_existing_under_budget_constraint() {
        let account = crate::AccountId(9);
        let snapshot = snapshot_arc();
        // 正确输入：existing=[A]（B=同轮新开不在集合内）。
        let mut correct = competing_plans_state();
        let projection = correct.root_projection.clone().expect("投影在");
        let a = correct
            .plan_book
            .active_plan(account, &crate::StockId(0))
            .expect("计划 A")
            .plan_id();
        let (ops, _) = correct
            .chain_quote_children(0, account, &projection, &personal(account), &snapshot, &[a])
            .expect("报价");
        assert!(
            !ops.is_empty() && ops.iter().all(|op| op.stock == 0),
            "现金受限时既有计划（ExistingPlan 优先级 1）获预算挂出；NewOpportunity 让位（实际挂出 {:?}）",
            ops.iter().map(|op| op.stock).collect::<Vec<_>>()
        );
        // 错误输入（修复前行为）：existing=[A,B]——同轮新开 B 同类平手，
        // 信心 9000>3000 抢先获预算 → 股 1 挂出、股 0 无子单。
        let mut buggy = competing_plans_state();
        let b = buggy
            .plan_book
            .active_plan(account, &crate::StockId(1))
            .expect("计划 B")
            .plan_id();
        let (ops, _) = buggy
            .chain_quote_children(0, account, &projection, &personal(account), &snapshot, &[a, b])
            .expect("报价");
        assert!(
            !ops.is_empty() && ops.iter().all(|op| op.stock == 1),
            "同轮新开被归入 existing 时信心序抢占预算（NewOpportunity 不可达的旧形态——股 1 挂出 {:?}）",
            ops.iter().map(|op| op.stock).collect::<Vec<_>>()
        );
    }
}
