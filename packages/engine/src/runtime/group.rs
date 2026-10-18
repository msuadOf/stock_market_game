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
    crate::OrderId(desk_order_id(order.0, order.1))
}

/// desk 域 OrderId 合成（Sol 六审 v6 判 B 必修·方案 A：压缩 stock 位宽）
/// `(stock<<32)|(serial 低 32 位)`——域容量 stock<2^21 × serial<2^32，
/// 最大 id=2^53−2 恰内含 js_safe wire 域（此前的 `(stock<<48)|serial`
/// 第 33 股起超域）。serial 低 32 位取的是**恒等值**（desk 取号路径
/// 的 next_serial 均 checked<2^32——超界显式失败，不存在先增长后
/// 截断的路径）；drive/decision_chain 的合成与掩码同式复用本函数。
pub(crate) fn desk_order_id(stock: super::StockIdx, serial: u64) -> u64 {
    (u64::from(stock) << 32) | (serial & 0xFFFF_FFFF)
}

/// desk 域 OrderId 的 serial 分量提取（与 desk_order_id 同式反解）。
pub(crate) fn desk_order_serial(id: u64) -> u64 {
    id & 0xFFFF_FFFF
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

/// eager 分域延迟载荷（单批）：即时相（F1/F2/F3 delta 族）到达即应用后，
/// F4/F5/F6 序敏感族按**原批内序**可重放的延迟动作清单。重放时点=下一
/// Continue/Seal 消息点（与原 apply_staging 消费点一致——续行决策可见
/// 推进后的计划簿/票）。
struct DeferredBatch {
    /// 批次股索引（延迟动作的目标股）。
    stock: StockIdx,
    /// 成交事实原序（计划推进 F5/F6 重放+经历转移输入）。
    fills: Vec<FillFact>,
    /// 卖腿 fill 前仓基快照（经历转移的 cost_before 输入——即时相
    /// fill 循环前捕获，时点等价原落账点：批到达与原落账消息点之间
    /// 组线程串行无其他账户写）。
    pre_exit_basis: std::collections::BTreeMap<LocalIdx, (i64, i64, u32)>,
    /// fill 循环后、批尾前的延迟动作（原批内序：receipts 拒收释放与
    /// entities 内的 Closed 释放/RefundedShares 复位按原序交错）。
    actions: Vec<DeferredAction>,
    /// 期末清仓判定触点（曾持有翻转 F4+经历转移 F5+记忆裁剪 F5——
    /// 批尾三连按原序重放）。
    touched: Vec<(LocalIdx, StockIdx)>,
}

/// 延迟域单步动作（原批内序交错保持——同键票据/曾持有位的序敏感面）。
enum DeferredAction {
    /// 票释放（F6）：拒收回执与 Closed 实体的统一终结信号。
    TicketRelease {
        /// 挂单所有者组内下标。
        local: LocalIdx,
        /// 终结单的 desk 序号（身份关联）。
        order_serial: u64,
    },
    /// RefundedShares 的曾持有复位（F4：qty>0 的再持有动作）。
    EverReset {
        /// 入账账户组内下标。
        local: LocalIdx,
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
    /// Seal 地板分解诊断（瞬态面，不入存档/hash）：组端 Seal 处理
    /// （drain+延迟相+分片构建+Ack）累计墙——A 口径长尾归因用。
    pub seal_handle_ns: u128,
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
    /// eager 分域延迟载荷（F4/F5/F6 序敏感族——即时相暂存，延迟相
    /// （Continue/Seal 消息点）按原批内序重放；Abort 随 rollback 清空。
    staging_deferred: Vec<DeferredBatch>,
    /// 账户分片脏集（SealAck accounts_sparse 的即时维护——push_undo
    /// 账户域族顺带插入，Seal 临界路径免 O(undo_len) 重扫；Start/回滚/
    /// 日结转正随日志退役清空）。
    account_dirty_set: std::collections::BTreeSet<LocalIdx>,
    /// SealAck 发送点的 undo 日志长度基线（Q2 eager 分域守卫）：日结
    /// Drain 批次的即时记录在 DayEnd 消息**前**到达应用（FIFO）——日结
    /// 隐式转正只退役上一 tick 记录（前段 drain），必须保留 Seal 后
    /// 追加的即时记录（Abort 仍可冲回）。消费/回滚后置 None。
    seal_acked_len: Option<usize>,
    /// Seal 地板分解诊断（瞬态面）：组端 Seal 处理累计墙。
    pub(crate) seal_handle_ns: u128,
    /// SealAck 后到达并即时应用的实体批次数（Start 防御专用）：Seal
    /// 后到达的 Batch=锁步破坏（协调者收齐 DeskReport 才发 Seal——
    /// 合法例外只有日结 Drain 序列，由 DayEnd 消化清零）。与 undo 基线
    /// 分离：AcceptedBatch（受理身份）的 F5/F6 记录跨 SealAck 迟到是
    /// 合法协议形态，不计数不拦截。
    batches_after_seal: usize,
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
    /// 开盘集合竞价 tick 数（组端相位门禁——Sol 复审修复 3；与
    /// DeskSetup::phase_at 的 entry 同式消费）。
    auction_ticks: u64,
    /// 收盘集合竞价 tick 数（同上）。
    closing_auction_ticks: u64,
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
    /// 组端业务拒绝回执（本轮累积；run_round 起点清零、报告排空——
    /// Sol 复审修复 2：资金/可卖/零股 skip 与竞价×市价门禁的玩家
    /// 逐请求可见结果）。
    round_rejections: Vec<super::DeskReceipt>,
    /// D5 终程：外部定向撤单缓冲（玩家 Cancel 意图路由——run_round 直转
    /// DeskIn::Cancel；desk 权威判定撤单窗/存在性）。
    external_cancels: Vec<(LocalIdx, StockIdx, u64)>,
    /// D3a-2b 信念个人态（逐组内下标；None=非机构位）——构造期投影，
    /// Belief-1b 起承接披露驱动的组内自演进。
    belief_states: Vec<Option<super::world::BeliefParticipantSnapshot>>,
    /// P2-1a 个人态 dirty 追踪：本 tick 生产演进面（retail/chain 写回、
    /// 信息投递、测试注入）触碰过的 local——EchoBelief 应答随附掩码，
    /// 驱动器只对 dirty 条目重装 session 镜像（省全名册深拷贝安装，
    /// B 口径 ≈58% CPU 的大头，见 .tmp/perf-p2-p0-breakdown.md）。
    /// 等价不变式：非 dirty ⇒ 组端值==session 镜像值（构造期同源投影
    /// +两侧只经 dirty 通道同步）；Start 随 undo 一并清（新 tick 起）。
    belief_dirty: Vec<bool>,
    /// P2-2a：跨 Start 存活的注入待上行位（InstallPersonal 在 Start 间
    /// 窗口注入——belief_dirty 会被下一 Start 清，注入态须独立承载到
    /// 下一次 EchoBelief 上行消费；对齐 pending_force_due 的"效果归
    /// 下一拍日志域"模式）。
    belief_pending_dirty: Vec<LocalIdx>,
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
    pending_force_due: Vec<(LocalIdx, u64, bool)>,
    /// D5 #4 retail 链：本拍零售决策 trace（RetailDecisionTrace 的组内
    /// 缓冲——驱动器段投影 session.last_retail_decisions；Start 清空
    /// 对齐旧管线「本 tick 最新」瞬态语义）。
    retail_traces: Vec<crate::session::RetailDecisionTrace>,
    /// 翻转批组参数化：本组在全局名 roster 的偏移（RootProjection 的
    /// per-local 向量按全局序——组内 local 访问需加偏移；单组=0）。
    pub(crate) roster_offset: usize,
    /// P2-3e：本 tick 注意力演进位（evaluate 消费 RNG/重排 next_due 或
    /// ForceDue 改排的位——稀疏上行源）。Start 清（上一 tick 的演进已在
    /// Drain 期上行消费；未消费即失效）。
    attention_evolved: Vec<LocalIdx>,
    /// D3b 件 4 fixture 通道：注入复审候选（账户键——run_chain_decision
    /// 消费一次即清；chain_restructure 测试的 drive_once 等价面）。
    #[cfg(test)]
    injected_assessments:
        Vec<(crate::AccountId, std::collections::BTreeMap<crate::StockId, crate::plans::CandidateAssessment>)>,
    /// D3b 件 4 fixture 通道：观察基分钟覆盖（股 idx 键——Start 整值
    /// 刷新后套用；set_urgency_minutes 的组内等价面，持续到显式改写）。
    #[cfg(test)]
    observation_overrides: std::collections::BTreeMap<u16, Vec<(u64, i64)>>,
    /// step_frame 接轨前置约束① fixture 通道（cfg(test)）：下一次轮
    /// 上报强制 `continuation=true`（一次消费即清——驱动 Continue 轮
    /// 的确定性触发）。
    #[cfg(test)]
    force_continuation: bool,
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
            0,
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
        auction_ticks: u64,
        closing_auction_ticks: u64,
        decision: Option<(Vec<NpcProfile>, u32)>,
        plans: Vec<Box<dyn PlanChain>>,
        root_projection: Option<std::sync::Arc<crate::runtime::world::RootProjection>>,
        script: GroupScript,
    ) -> Self {
        let n_stocks = accounts.first().map(|a| a.shares.len()).unwrap_or(0);
        // P2-1a：名册长度先于 seed move 记录（dirty 掩码初始化用）。
        let belief_roster = belief_states_seed.len();
        GroupState {
            group,
            accounts,
            undo: Vec::new(),
            staging_deferred: Vec::new(),
            account_dirty_set: Default::default(),
            seal_acked_len: None,
            batches_after_seal: 0,
            seal_handle_ns: 0,
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
            auction_ticks,
            closing_auction_ticks,
            decision,
            snapshot: None,
            plans,
            plan_fills: Vec::new(),
            observed: Vec::new(),
            external_pending: Vec::new(),
            round_rejections: Vec::new(),
            external_cancels: Vec::new(),
            observation_inputs: vec![ObservationInputs::default(); n_stocks],
            belief_states: belief_states_seed,
            // P2-1a：dirty 掩码随名册长度初始化（构造期全 false——组端
            // 初值=会话投影同源，无需回装）。
            belief_dirty: vec![false; belief_roster],
            belief_pending_dirty: Vec::new(),
            library: library_seed,
            root_projection,
            child_in_flight: Default::default(),
            plan_undo_saved: false,
            pending_force_due: Vec::new(),
            retail_traces: Vec::new(),
            roster_offset: 0,
            attention_evolved: Vec::new(),
            #[cfg(test)]
            #[cfg(test)]
            injected_assessments: Vec::new(),
            #[cfg(test)]
            observation_overrides: std::collections::BTreeMap::new(),
            #[cfg(test)]
            force_continuation: false,
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
        self.staging_deferred.clear();
        self.account_dirty_set.clear();
        self.plan_fills.clear();
        self.plan_undo_saved = false;
        // eager 分域：Seal 收敛点基线作废（Abort 后重跑不再持有基线——
        // Start 防御以新基线重建）。
        self.seal_acked_len = None;
        self.batches_after_seal = 0;
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

    /// undo 入账统一入口：账户域族（F1/F2/F3 即时域+F4 曾持有）顺带登记
    /// `account_dirty_set`（SealAck 分片集合的即时维护——Seal 临界路径免
    /// O(undo_len) 重扫）；非账户域族（NextDue/Personal/PlanBook/
    /// ChildInFlight）不进分片。集合生命周期与 undo 日志同步（Start/回滚/
    /// 日结转正清空；Seal take 消费——与原 Seal 期全日志扫描的族集合
    /// 逐族一致，等价替换）。
    fn push_undo(&mut self, entry: GroupUndo) {
        match &entry {
            GroupUndo::Cash { local, .. }
            | GroupUndo::Shares { local, .. }
            | GroupUndo::ForeignActive { local, .. }
            | GroupUndo::Frozen { local, .. }
            | GroupUndo::CostBasis { local, .. }
            | GroupUndo::EverHeld { local, .. }
            | GroupUndo::PendingSell { local, .. }
            | GroupUndo::BuyPurseHeld { local, .. } => {
                self.account_dirty_set.insert(*local);
            }
            GroupUndo::NextDue { .. }
            | GroupUndo::Personal { .. }
            | GroupUndo::PlanBook { .. }
            | GroupUndo::ChildInFlight { .. } => {}
        }
        self.undo.push(entry);
    }

    /// eager 分域即时相（Q2 批 2）：Batch 消息**到达即**应用 F1/F2/F3
    /// delta 族（Cash/Shares/Frozen/ForeignActive/CostBasis/PendingSell/
    /// BuyPurseHeld——回放算子 `-=` 对全应用封闭，与决策段记录任意交错
    /// 无害，合同 delta_undo_shuffled_replay_equivalence 守卫），并把
    /// F4/F5/F6 序敏感族按原批内序暂存 `staging_deferred`（延迟相在
    /// Continue/Seal 消息点重放）。语义等价锚：同一落账逻辑/undo 日志/
    /// 防御核对；原"落账点=消息点"拆为"即时域=到达点、延迟域=消息点"，
    /// 两时点间组线程串行无其他账户写（判定读值等价）。
    fn apply_batch_immediate(
        &mut self,
        batch: DeskBatch,
        expected_tick: u64,
        expected_round: u32,
    ) -> Result<(), RuntimeError> {
        batch
            .verify_tick_round(expected_tick, expected_round)
            .map_err(|m| RuntimeError::ProtocolInvariant {
                detail: format!("组 {} 批次防御核对失败：{m}", self.group),
            })?;
        let stock_idx = batch.stock as usize;
        if stock_idx >= self
            .accounts
            .first()
            .map(|a| a.shares.len())
            .unwrap_or(0)
        {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("组 {} 批次股索引越界：{stock_idx}", self.group),
            });
        }
        let stock = batch.stock;
        // 成交事实即时进计划结果缓冲（信息回流：续行由 FillFact 驱动，
        // F §4.3——Continue 轮"先见 fills 再续行"语义不因分域改变）。
        self.plan_fills.extend(batch.fills.iter().copied());
        // Sol 终审四审经历族：卖腿 fill 前成本基/面快照（清仓转移的
        // cost_before 输入——fill 循环内 apply_fill_cost_basis 先行
        // 推进 invested/recovered，期末值不可反推）。捕获时点=即时相
        // fill 循环前（等价原落账点——见函数 doc）。
        let mut pre_exit_basis: std::collections::BTreeMap<LocalIdx, (i64, i64, u32)> =
            std::collections::BTreeMap::new();
        for fill in &batch.fills {
            if fill.side != Side::Sell || fill.owner.0 != self.group {
                continue;
            }
            let local = fill.owner.1;
            if pre_exit_basis.contains_key(&local) {
                continue;
            }
            if let Some(acct) = self.accounts.get(local as usize) {
                pre_exit_basis.insert(
                    local,
                    (
                        acct.invested_cents.get(stock_idx).copied().unwrap_or(0),
                        acct.recovered_cents.get(stock_idx).copied().unwrap_or(0),
                        acct.shares.get(stock_idx).copied().unwrap_or(0).saturating_add(
                            acct.pending_sell_qty.get(stock_idx).copied().unwrap_or(0),
                        ),
                    ),
                );
            }
        }
        let mut deferred = DeferredBatch {
            stock,
            fills: batch.fills.clone(),
            pre_exit_basis,
            actions: Vec::new(),
            touched: Vec::new(),
        };
        // fill 循环即时域：成本基（F2）+在途镜像（F2）。计划推进
        // （F5/F6——apply_fill_plan_progress）延迟重放（fills 已暂存）。
        let mut touched: Vec<(LocalIdx, StockIdx)> = Vec::new();
        for fill in &batch.fills {
            self.apply_fill_cost_basis(stock, fill)?;
            if !touched.contains(&(fill.owner.1, stock)) {
                touched.push((fill.owner.1, stock));
            }
            // Sol 五审：在途镜像统一按 fill 推进（不限计划链票——
            // 外部/retail 单同口径）：卖腿余量减；买腿占用减实付
            //（gross+佣金+过户=desk buyer_charge 同式）。
            let gross = i64::from(fill.price) * i64::from(fill.qty);
            match fill.side {
                Side::Sell => {
                    let acct = &mut self.accounts[fill.owner.1 as usize];
                    let before = acct.pending_sell_qty[stock_idx];
                    let after = before.saturating_sub(fill.qty);
                    acct.pending_sell_qty[stock_idx] = after;
                    if before != after {
                        self.push_undo(GroupUndo::PendingSell {
                            local: fill.owner.1,
                            stock,
                            delta: -i64::from(before - after),
                        });
                    }
                }
                Side::Buy => {
                    let charge = gross + fill.commission + fill.transfer_fee;
                    let acct = &mut self.accounts[fill.owner.1 as usize];
                    acct.buy_purse_held_cents -= charge;
                    self.push_undo(GroupUndo::BuyPurseHeld {
                        local: fill.owner.1,
                        delta: -charge,
                    });
                }
            }
        }
        // Sol 整改 2：拒收回执=失败释放（未受理票清除——desk 拒收的挂单
        // 从未成交，票 order_id=None 走未受理释放路径）。票域（F6）延迟
        // 重放——原批内序（receipts 在 entities 前）由 actions 追加序保持。
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
                deferred.actions.push(DeferredAction::TicketRelease {
                    local: receipt.owner.1,
                    order_serial: receipt.order.1,
                });
            }
        }
        // entities：即时域落账（F1/F2/F3）；Closed 票释放（F6）与
        // RefundedShares 曾持有复位（F4）按原 entities 序进 actions。
        for entity in batch.entities {
            self.apply_entity(stock, entity, &mut deferred)?;
        }
        // 清仓成本基清零（F2——实体落账后按期末 shares/在途判定：对齐
        // session 清仓删除条目——下次买入从零新建，避免再持有后
        // cost_price 漂移）。曾持有翻转（F4）随 touched 延迟到批尾重放。
        self.clear_flat_cost_basis(&touched);
        deferred.touched = touched;
        self.staging_deferred.push(deferred);
        Ok(())
    }

    /// eager 分域延迟相：暂存的 F4/F5/F6 延迟动作按原批内序重放。
    /// 调用点=Continue/Seal/Shutdown/DayEnd 消息点（与原 apply_staging
    /// 消费点一致——续行决策可见推进后的计划簿/票；Seal 相位到达时
    /// 全部批次的延迟项已在等待窗暂存齐，S4 锁步不变量承载）。
    fn apply_deferred(&mut self) -> Result<(), RuntimeError> {
        for deferred in std::mem::take(&mut self.staging_deferred) {
            self.replay_deferred_batch(deferred)?;
        }
        Ok(())
    }

    /// 单批延迟重放（原 apply_staging 批内序的 F4/F5/F6 部分）：
    /// fills 计划推进（F5/F6）→ actions 票释放/曾持有复位（F6/F4）→
    /// 批尾三连：曾持有翻转（F4）→ 经历转移（F5）→ 记忆裁剪（F5）。
    /// 延迟相不写账户资源域（F1/F2/F3 已在即时相定型）——判定读值
    /// =即时相末值，与原批尾时点等价（延迟域不触碰资源域字段）。
    fn replay_deferred_batch(&mut self, deferred: DeferredBatch) -> Result<(), RuntimeError> {
        let stock = deferred.stock;
        for fill in &deferred.fills {
            self.apply_fill_plan_progress(stock, fill)?;
        }
        for action in deferred.actions {
            match action {
                DeferredAction::TicketRelease {
                    local,
                    order_serial,
                } => {
                    self.release_child_ticket(stock, local, order_serial)?;
                }
                DeferredAction::EverReset { local } => {
                    // RefundedShares 复位（原 apply_entity 批内位置）：qty>0
                    // 的再持有=曾持有位复位；读当前值翻转（即时相不写
                    /// ever_held——延迟相内按原序重放等价原批内判定）。
                    let acct = &mut self.accounts[local as usize];
                    if acct.ever_held[stock as usize] {
                        acct.ever_held[stock as usize] = false;
                        self.push_undo(GroupUndo::EverHeld {
                            local,
                            stock,
                            prev: true,
                        });
                    }
                }
            }
        }
        // 批尾曾持有翻转（原 clear_flat_cost_basis 批内位置）：期末
        // shares==0 → true（对齐 session 清仓保留零量条目）；>0 → false。
        let touched = deferred.touched;
        for &(local, stock) in &touched {
            let acct = &mut self.accounts[local as usize];
            let prev_ever = acct.ever_held[stock as usize];
            let now_ever = acct.shares[stock as usize] == 0;
            if prev_ever != now_ever {
                acct.ever_held[stock as usize] = now_ever;
                self.push_undo(GroupUndo::EverHeld {
                    local,
                    stock,
                    prev: prev_ever,
                });
            }
        }
        // Sol 终审四审经历族（fault2051）：清仓 retail 经历转移（F5——
        /// 期末 face=即时相末值，读值时点等价原批尾）。
        self.apply_full_exit_experience(stock, &deferred.fills, &deferred.pre_exit_basis)?;
        // Sol D3b 件 1：fill 触达账户的个人记忆裁剪（F5——对齐 session
        /// candidate_commit 的逐账户 prune_plan_personal_memory——
        /// completed_sell 2069 残余收口；清仓后售出股脱离保护集）。
        for (local, _) in &touched {
            self.prune_personal_memory_after_completion(*local);
        }
        Ok(())
    }

    /// Sol 终审四审经历族（fault2051）：清仓边界的 retail 经历转移。
    /// 对齐旧管线 record_fill 的 Sell 且 after_qty==0 分支：清
    /// entry_reference_price/peak_price_since_entry/last_buy_price/
    /// last_buy_order_id/adverse_move_recorded，设退出冷静期
    /// （cooldown_until_market_minute）与 last_sell_order_id/
    /// last_trade_market_minute。覆盖面=**清仓边界**（期末
    /// face=组 shares+在途卖余量 归零的最后一条卖腿）；加仓/部分
    /// 卖出的经历迁移仍按 D5 登记近似承载（首买 init 走回写段）。
    /// failed_buys 逆境计数按 fill 前仓均成本（pre_exit_basis——与旧
    /// 管线 cost_before 同式，银行家舍入同口径）判定。
    fn apply_full_exit_experience(
        &mut self,
        stock: StockIdx,
        fills: &[FillFact],
        pre_exit_basis: &std::collections::BTreeMap<LocalIdx, (i64, i64, u32)>,
    ) -> Result<(), RuntimeError> {
        let group = self.group;
        let idx = stock as usize;
        let minute = market_minute_at(
            self.cur_tick,
            self.ticks_per_day,
            self.auction_ticks,
            self.closing_auction_ticks,
        );
        // 倒序取每账户**最后**一条卖腿；期末 face 归零=清仓边界。
        let mut seen: std::collections::BTreeSet<LocalIdx> = Default::default();
        for fill in fills.iter().rev() {
            if fill.owner.0 != group || fill.side != Side::Sell || !seen.insert(fill.owner.1) {
                continue;
            }
            let local = fill.owner.1;
            let face = {
                let acct = self
                    .accounts
                    .get(local as usize)
                    .ok_or_else(|| RuntimeError::ProtocolInvariant {
                        detail: format!("组 {group} 经历转移账户越界：local={local}"),
                    })?;
                acct.shares.get(idx).copied().unwrap_or(0).saturating_add(
                    acct.pending_sell_qty.get(idx).copied().unwrap_or(0),
                )
            };
            if face != 0 {
                continue;
            }
            // 槽位在席且持 retail 经历行（inst/hot/玩家位无行——原样跳过）。
            let Some(mut snapshot) = self
                .belief_states
                .get_mut(local as usize)
                .and_then(Option::take)
            else {
                continue;
            };
            // 拍前值（undo 用——变异前克隆）。
            let prev = snapshot.clone();
            let Some(experience) = snapshot.retail_experience.as_mut() else {
                if let Some(slot) = self.belief_states.get_mut(local as usize) {
                    *slot = Some(snapshot);
                }
                continue;
            };
            // cost_before=fill 前仓均成本（银行家舍入同 Position::cost_price；
            /// 非正=不可用 None——逆境计数不因本腿调整）。
            let cost_before = pre_exit_basis
                .get(&local)
                .and_then(|&(invested, recovered, face_pre)| {
                    (face_pre > 0).then(|| {
                        crate::account::Position::from_restored_parts(
                            face_pre, 0, invested, recovered,
                        )
                        .cost_price()
                        .filter(|price| price.cents() > 0)
                    })
                })
                .flatten();
            let code = crate::StockId(u32::from(stock));
            let result = experience.record_fill_with_order(
                &code,
                Side::Sell,
                Money::from_cents(fill.price),
                fill.qty,
                0,
                cost_before,
                minute,
                Some(order_id_of(&fill.order).0),
            );
            if let Err(error) = result {
                // 还槽后显式失败（组不变量破坏不静默）。
                if let Some(slot) = self.belief_states.get_mut(local as usize) {
                    *slot = Some(snapshot);
                }
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!(
                        "组 {group} 清仓经历转移失败：local={local}, code={code:?}：{error:?}"
                    ),
                });
            }
            // 拍前值入可逆日志（Personal）+还槽+dirty（经验行随 Belief
            /// 上行通道同拍回写 session 镜像）。
            self.push_undo(GroupUndo::Personal {
                local,
                prev: Box::new(prev),
            });
            if let Some(slot) = self.belief_states.get_mut(local as usize) {
                *slot = Some(snapshot);
            }
            if let Some(flag) = self.belief_dirty.get_mut(local as usize) {
                *flag = true;
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
        self.push_undo(GroupUndo::CostBasis {
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
        // Sol 终审四审 A2：日基准=(tick−1)/tpd（首拍属 day 0——与
        /// session.day 同基；此前 tick/tpd 使组内日历超前 1，plan 事件
        /// day 超前触发 session 日终迁移的 precedes 校验拒绝）。
        let day = self.cur_tick.saturating_sub(1) / self.ticks_per_day;
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
            self.push_undo(GroupUndo::ChildInFlight {
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
            self.push_undo(GroupUndo::ChildInFlight {
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
                self.push_undo(GroupUndo::ChildInFlight {
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
            .and_then(|projection| {
                projection.account_ids.get(self.roster_offset + local as usize)
            })
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
        self.push_undo(GroupUndo::Personal {
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
            .and_then(|projection| {
                projection.account_ids.get(self.roster_offset + local as usize)
            })
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
        // Sol 六审收官复核阻断 2：desk 侧 lifecycle/facts 的 order_serial
        /// 已是 wire 编码 id（(stock<<32)|serial）——不再二次合成。
        let order_id = crate::OrderId(order_serial);
        let day = self.cur_tick.saturating_sub(1) / self.ticks_per_day;
        self.save_plan_book_undo();
        self.plan_book
            .record_child_submitted(account, &code, side, order_id, qty, day)
            .unwrap_or_else(|error| {
                panic!("组内受理身份登记失败 {account:?} {code:?}: {error}")
            });
        ticket.order_id = Some(order_id);
        self.push_undo(GroupUndo::ChildInFlight {
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
            .and_then(|projection| {
                projection.account_ids.get(self.roster_offset + local as usize)
            })
        else {
            return Ok(());
        };
        let code = crate::StockId(u32::from(stock));
        if let Some(known) = self
            .child_in_flight
            .get(&(account, code))
            .and_then(|ticket| ticket.order_id)
        {
            if known != crate::OrderId(order_serial) {
                // 终结的是他单（票不归属本单）——保留本票，仅落资源实体。
                return Ok(());
            }
        }
        let Some(ticket) = self.child_in_flight.remove(&(account, code)) else {
            return Ok(());
        };
        self.push_undo(GroupUndo::ChildInFlight {
            account,
            code: code.clone(),
            prev: Some(ticket.clone()),
        });
        // 在途卖余量镜像改由 RefundedShares 实体驱动（apply_staging 统一
        // 段——Sol 五审：票域减法与实体退款双计）。
        if let Some(order_id) = ticket.order_id {
            let day = self.cur_tick.saturating_sub(1) / self.ticks_per_day;
            self.save_plan_book_undo();
            // 日终清簿的子单释放=错过日终的补账（revision.rs 守卫注释
            /// 的豁免面——horizon 外仍须可达）。
            let released = self
                .plan_book
                .record_child_released(account, &code, order_id, day, true)
                .unwrap_or_else(|error| {
                    panic!("组内子单释放失败 {account:?} {code:?}: {error}")
                });
            if !released {
                // 迟到 Closed 的合法面：多轮推进后计划 active_child 可能
                /// 已换新单（旧票身份不再匹配——非漂移；无害跳过）。
                return Ok(());
            }
        }
        Ok(())
    }

    /// 清仓成本基清零（即时相批尾——F2 delta 族）：期末 touched 槽位的
    /// 终态维护。成本基清零：shares==0 且**该股无在途挂单**——
    /// invested/recovered 归零。foreign_active 守卫（复核发现 2 收口）：
    /// 跨轮余量退款（轮 N 卖单全成 shares=0，轮 N+1 RefundedShares 回升）
    /// 不得永久丢失成本基——在途未终结即不清零，退款落账（shares 回升）
    /// 后自然跳过。登记残余边界：同 tick 卖光+同股新买单在途推迟清零，
    /// 买单 Closed 时 shares>0（成交）则旧值残留（session 语义=删条目后
    /// 新建零基）——罕见形态，D3a-3 注入路由统一时一并收口。
    /// 曾持有翻转（F4 序敏感）已移延迟相批尾重放（replay_deferred_batch）。
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
                self.push_undo(GroupUndo::CostBasis {
                    local,
                    stock,
                    d_invested: -d_invested,
                    d_recovered: -d_recovered,
                });
            }
        }
    }

    /// 落账单条实体（记逆操作）。eager 分域：资源域（F1/F2/F3 delta 族）
    /// 即时落账；序敏感域延迟——Closed 的票释放（F6）与 RefundedShares
    /// 的曾持有复位（F4）按原 entities 序追加进 `deferred.actions`。
    fn apply_entity(
        &mut self,
        stock: StockIdx,
        e: Entity,
        deferred: &mut DeferredBatch,
    ) -> Result<(), RuntimeError> {
        let group = self.group;
        match e {
            Entity::CashProceeds { local, amount } => {
                let acct = self.acct_mut(local)?;
                acct.cash = acct.cash.checked_add(amount).ok_or_else(|| {
                    RuntimeError::ProtocolInvariant {
                        detail: format!("组 {group} 现金落账溢出：local={local}, amount={amount}"),
                    }
                })?;
                self.push_undo(GroupUndo::Cash {
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
                self.push_undo(GroupUndo::Cash {
                    local,
                    delta: amount,
                });
                self.push_undo(GroupUndo::BuyPurseHeld {
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
                self.push_undo(GroupUndo::Shares {
                    local,
                    stock,
                    delta: qty as i32,
                });
                self.push_undo(GroupUndo::Frozen {
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
                        self.push_undo(GroupUndo::PendingSell {
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
                    // F4 序敏感（曾持有复位）——延迟相按原 entities 序重放
                    ///（replay_deferred_batch 的 EverReset：读当前值翻转，
                    /// 即时相不写 ever_held，读值时点等价原批内判定）。
                    deferred.actions.push(DeferredAction::EverReset { local });
                }
                self.push_undo(GroupUndo::Shares {
                    local,
                    stock,
                    delta: qty as i32,
                });
            }
            Entity::Closed { local, order_serial } => {
                // Sol 整改 2：子单终结信号（撤销/到期/日终清簿——组侧
                // 在途票与执行腿释放；order_serial=订单身份关联，二审修复 3）。
                // 票域（F6）延迟重放——原批内位置由 actions 追加序保持；
                // F3 在途计数减（delta 族）即时。跨族序变化无害（票据与
                // 计数器互不相写）。
                deferred.actions.push(DeferredAction::TicketRelease {
                    local,
                    order_serial,
                });
                let acct = self.acct_mut(local)?;
                let slot = &mut acct.foreign_active[stock as usize];
                if *slot == 0 {
                    // D5 adopt 例外：foreign_active==0 且无票的 Closed=构造期
                    // 直入的旧簿在途单（无组端登记——计数器从未加过）；
                    // 真负数形态（登记后多减）仍由下面的减法下溢防御显式
                    // 拦截（i16 saturating 不静默）。计数登记诊断面。
                    self.skipped_orders += 1;
                    return Ok(());
                }
                *slot -= 1;
                self.push_undo(GroupUndo::ForeignActive {
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
            #[cfg(test)]
            let overrides = self.observation_overrides.clone();
            for (stock_idx, (inputs, stock)) in self
                .observation_inputs
                .iter_mut()
                .zip(snapshot.stocks.iter())
                .enumerate()
            {
                // D3b 件 4 fixture 通道：权威刷新后套用分钟覆盖槽。
                #[cfg(test)]
                {
                    inputs.minutes = stock.minute_closes.clone();
                    if let Some(minutes) = overrides.get(&(stock_idx as u16)) {
                        inputs.minutes = minutes.clone();
                    }
                }
                #[cfg(not(test))]
                {
                    inputs.minutes = stock.minute_closes.clone();
                }
                inputs.daily_bars = stock.daily_bars.clone();
                inputs.daily_active = stock.daily_active;
                inputs.daily_total_completed = stock.daily_total_completed;
                inputs.daily_total_traded = stock.daily_total_traded;
            }
        }
        // Q2 Seal 后防御核对（协调者守卫 2，eager 分域版）：SealAck 后
        // 延迟载荷必须恒空且无基线外 undo 记录——非空=锁步破坏（批次
        // 绕过 Seal 到达或 Seal drain 缺漏），显式失败上报不静默落账。
        // 两条覆盖面：①staging_deferred 非空=Seal 后批次到达（有延迟
        // 项的形态）；②undo.len() 超 SealAck 基线=Seal 后纯即时批次
        // （无延迟项的形态——即时相已应用，日志越界即证据）。
        if !self.staging_deferred.is_empty() {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "组 {} Start 时延迟载荷非空（{} 批）——Seal 落账后 Start 不应再有残余批次（锁步破坏，显式上报）",
                    self.group,
                    self.staging_deferred.len()
                ),
            });
        }
        if self.batches_after_seal > 0 {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "组 {} Start 时发现 Seal 后到达的实体批次（{} 批）——锁步破坏（合法例外只有日结 Drain 序列，显式上报）",
                    self.group,
                    self.batches_after_seal
                ),
            });
        }
        // 防御核对保留（恒空批次过 verify——历史接口兼容；上一 tick 的
        // 尾巴已由 Seal 落账）。eager 分域后延迟相已在 Seal 消费——此处
        // 恒空，防御性调用保持（apply_deferred 幂等）。
        self.apply_deferred()?;
        // 转正点：丢弃上一 tick 可逆日志；committed 推进到 N−1（旧 cur_tick）。
        self.undo.clear();
        self.account_dirty_set.clear();
        self.seal_acked_len = None;
        self.batches_after_seal = 0;
        // P2-1a：dirty 掩码随新 tick 清（上一 tick 的 dirty 已在安装期
        /// 上行消费；新 tick 的演进面重新标记）。
        self.belief_dirty.iter_mut().for_each(|f| *f = false);
        self.attention_evolved.clear();
        self.plan_undo_saved = false;
        // Sol 二审修复集 4：Start 间窗口收到的 ForceDue 在此应用——效果
        // 归本 tick 日志域（Abort 冲回强制前值、指令失效需重发）。
        // push_undo 封装（&mut self）与 decision 借用互斥——undo 条目
        // 先攒后入（同拍序不变）。
        let mut force_due_undo: Vec<GroupUndo> = Vec::new();
        let mut force_due_evolved: Vec<LocalIdx> = Vec::new();
        for (local, target, ensure_observe) in std::mem::take(&mut self.pending_force_due) {
            if let Some((profiles, _)) = &mut self.decision {
                if let Some(p) = profiles.get_mut(local as usize) {
                    if p.next_due > target {
                        force_due_undo.push(GroupUndo::NextDue {
                            local,
                            prev: p.next_due,
                        });
                        p.next_due = target;
                        // P2-3e：ForceDue 改排=演进位（稀疏上行须携带）。
                        force_due_evolved.push(local);
                    }
                    // D5 #2：强制观察登记（evaluate 门绕行——仍续流）。
                    if ensure_observe {
                        p.attention_base = 1.0;
                    }
                }
            }
        }
        for entry in force_due_undo {
            self.push_undo(entry);
        }
        self.attention_evolved.extend(force_due_evolved);
        self.committed_tick = self.cur_tick;
        self.cur_tick = tick;
        self.cur_round = 0;
        // D5 #4 retail 链：trace 瞬态语义（旧管线每 tick 入口清——
        /// 本 tick 生产写入存活到提交，无新产出 tick 返回后读数为空）。
        self.retail_traces.clear();
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
        // eager 分域：日结 Drain 批次在 DayEnd 消息**前**到达（FIFO——
        // desk 回流直发组通道），即时相已应用并 push 了 F1/F2/F3 记录；
        // 隐式转正只退役上一 tick 记录（前段 drain），保留 Seal 后追加的
        // Drain 即时记录（日结 Abort 仍可冲回——回到 Drain 前的上一交易
        // 日收敛态）。无基线（未经 Seal 的异常路径）退化为整清（原状）。
        if let Some(baseline) = self.seal_acked_len {
            let kept = self.undo.len().saturating_sub(baseline);
            self.undo.drain(..baseline);
            debug_assert_eq!(self.undo.len(), kept);
        } else {
            self.undo.clear();
        }
        self.seal_acked_len = None;
        self.batches_after_seal = 0;
        self.account_dirty_set.clear();
        // P2-1a：日结转正同步清 dirty（日结后安装期消费的是日结相位
        /// 的标记——留待下一 Start 前的正常安装轮完成）。
        self.belief_dirty.iter_mut().for_each(|f| *f = false);
        self.attention_evolved.clear();
        self.plan_undo_saved = false;
        self.committed_tick = self.cur_tick;
        // ① Drain 回流落账（清空在途——即时相已在到达时应用，此处
        /// 补延迟相 F4/F5/F6 重放；与原 apply_staging 全族落账等价）。
        self.apply_deferred()?;
        // ② T+1 日界解冻（当日买入进入可卖）。
        for local in 0..self.accounts.len() as LocalIdx {
            let frozen = self.accounts[local as usize].frozen_today.clone();
            for (stock, &f) in frozen.iter().enumerate() {
                if f > 0 {
                    self.accounts[local as usize].frozen_today[stock] = 0;
                    self.push_undo(GroupUndo::Frozen {
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
        // 组内 PlanBook 是权威；session 镜像的日终迁移不能替代此处扫尾。
        // Drain 已释放子单，最后有效日结束时终止计划，其余计划跨日保留。
        self.save_plan_book_undo();
        for plan_id in self.plan_book.active_plan_ids() {
            self.plan_book
                .apply(
                    plan_id,
                    crate::plans::PlanEvent::TradingDayEnded {
                        trading_day: u64::from(directive.day),
                    },
                )
                .map_err(|error| RuntimeError::ProtocolInvariant {
                    detail: format!("组 {} 日终计划 {plan_id:?} 迁移失败：{error}", self.group),
                })?;
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
                self.push_undo(GroupUndo::Cash {
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
                self.push_undo(GroupUndo::Shares {
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
                self.push_undo(GroupUndo::Cash {
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
        // 上一轮实体批次延迟相落账（即时相已随 Batch 到达应用；此处重放
        // F4/F5/F6——续行决策可见推进后的计划簿/票，与原 apply_staging
        // 消费点语义一致）。
        self.apply_deferred()?;
        self.cur_round = round;
        self.run_round(tick, round, desk_txs, up_tx)
    }

    /// D5 #4 retail 链：零售位决策一轮（旧管线 A 类账户任务的组内化——
    /// 观察（belief entry/watchlist/information/经验反馈）+ behavior 决策
    /// （decide_retail_position_with_experience+个人分析应用）+ trace 收集
    /// + 经验行演进）。决策流=npc_seed_base 同式派生（与旧管线 A 类一致）；
    /// 个人态六件+经验行入可逆日志后写回组权威。
    ///
    /// P2-3a：`chain`/`behavior` 为本轮共享公共观察（调用方 run_round 每
    /// 轮构造一次——纯 snapshot 函数，与旧管线 T1"捕获一次共享只读"同式；
    /// 此前每位各自 build_chain_observation+等权聚合+paths 克隆，是组线程
    /// 拷贝放大主源）。等价论证同 P1-B：同轮各 NPC 的输入逐位相同。
    #[allow(clippy::too_many_arguments)]
    fn run_retail_decision(
        &mut self,
        local: LocalIdx,
        tick: u64,
        snapshot: &super::protocol::TickSnapshot,
        market: &crate::strategy::MarketView,
        chain: &crate::session::DecisionChainObservation,
        behavior: &crate::behavior::BehaviorMarketObservation,
    ) -> Result<Vec<super::decision::PendingOp>, RuntimeError> {
        let group = self.group;
        let projection = match self.root_projection.clone() {
            Some(p) => p,
            None => {
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!(
                        "组 {group} retail 位 {local} 决策缺投影（无会话来源或未跑 tick）"
                    ),
                })
            }
        };
        // P2-3d：槽 take 化——原「输入 clone + 决策尾 undo clone」双深拷贝
        /// 归一（journal 先记拍前值再消费；错误/panic 路径 Abort 重放
        /// GroupUndo::Personal 无条件恢复 Some(prev)，槽空窗仅函数内——
        /// 组线程单线程，run_round 期无并发读面）。journal 相对序不变
        ///（本函数此前唯一 undo 条目=Personal，前后无其他条目交错）。
        let mut snap = match self
            .belief_states
            .get_mut(local as usize)
            .and_then(Option::take)
        {
            Some(s) => {
                self.push_undo(GroupUndo::Personal {
                    local,
                    prev: Box::new(s.clone()),
                });
                s
            }
            None => {
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!(
                        "组 {group} retail 位 {local} 决策缺个人态（无会话来源或未跑 tick）"
                    ),
                })
            }
        };
        let Some(&account) = projection
            .account_ids
            .get(self.roster_offset + local as usize)
        else {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("组 {group} retail 位 {local} 无名册 AccountId"),
            });
        };
        let Some((profiles, _)) = &self.decision else {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("组 {group} retail 位 {local} 缺决策档案"),
            });
        };
        let profile = &profiles[local as usize];
        let Some(retail_style) = profile.retail_style else {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("组 {group} retail 位 {local} 缺零售风格投影"),
            });
        };
        // P2-3d：经验行自 snap 取走（journal 已持拍前副本——无需 clone）。
        let mut experience = snap
            .retail_experience
            .take()
            .ok_or_else(|| RuntimeError::ProtocolInvariant {
                detail: format!("组 {group} retail 位 {local} 缺经验行投影"),
            })?;
        // 组账户 → 零售分析 ctx（旧管线 view 依赖面的组内等价——
        /// qty 口径=在手+在途卖（回写同式）；零量条目按曾持有位保留
        /// （旧管线 positions 的清仓残留键语义）。
        let acct = &self.accounts[local as usize];
        let mut positions: std::collections::BTreeMap<crate::StockId, crate::Position> =
            std::collections::BTreeMap::new();
        for idx in 0..acct.shares.len() {
            let qty = acct.shares[idx].saturating_add(
                acct.pending_sell_qty.get(idx).copied().unwrap_or(0),
            );
            let invested = acct.invested_cents.get(idx).copied().unwrap_or(0);
            let recovered = acct.recovered_cents.get(idx).copied().unwrap_or(0);
            if qty > 0 {
                positions.insert(
                    crate::StockId(idx as u32),
                    crate::Position::from_restored_parts(
                        qty,
                        acct.frozen_today.get(idx).copied().unwrap_or(0),
                        invested,
                        recovered,
                    ),
                );
            } else if *acct.ever_held.get(idx).unwrap_or(&false) {
                positions.insert(
                    crate::StockId(idx as u32),
                    crate::Position::from_restored_parts(0, 0, invested, recovered),
                );
            }
        }
        let held: std::collections::BTreeSet<crate::StockId> = positions.keys().copied().collect();
        let issuer_of: std::collections::BTreeMap<
            crate::StockId,
            Option<crate::company::CompanyId>,
        > = projection
            .stock_codes
            .iter()
            .map(|(code, id)| {
                (
                    *id,
                    projection.company_registry.issuer_of(code),
                )
            })
            .collect();
        let minute = u64::from(snapshot.market_minute);
        let day = tick.saturating_sub(1) / self.ticks_per_day.max(1);
        let retail = crate::session::RetailStrategyInstance {
            data: profile.strategy.clone(),
            style: retail_style,
        };
        let ctx = crate::session::RetailAnalysisCtx {
            account,
            held: held.clone(),
            cash: crate::Money::from_cents(acct.cash + acct.buy_purse_held_cents),
            positions,
            minute,
            day,
            civil_date: snapshot.civil_date,
            seed: projection.session_seed,
            tick,
            library: &self.library,
            issuer_of,
            operations: &projection.operations,
            retail,
        };
        // 观察（core——session 版同核）：belief entry/watchlist/information
        /// 演进+候选评估。P2-3a：公共观察=本轮共享（调用方一次构造）。
        let observation = chain;
        // 经验行 capture 演进（旧管线 decision_snapshot_capture 的
        /// capture_from_row 等价——equity 观测/持仓 dated 演进/首建；
        /// 观察 core 与决策消费演进后的行）。
        {
            let moment = crate::experience::ExperienceMoment {
                civil_date: snapshot.civil_date,
                market_minute: minute,
                trading_day: day,
            };
            let mut equity = ctx.cash;
            for (code, position) in &ctx.positions {
                let Some(stock) = observation.market.stocks.get(code) else {
                    continue;
                };
                let market_value = stock
                    .last_price
                    .mul_shares(position.qty())
                    .map_err(|e| RuntimeError::ProtocolInvariant {
                        detail: format!("组 {group} retail {account:?} 市值计算失败：{e}"),
                    })?;
                equity = equity
                    .add(market_value)
                    .map_err(|e| RuntimeError::ProtocolInvariant {
                        detail: format!("组 {group} retail {account:?} 权益聚合失败：{e}"),
                    })?;
                let cost_price = position.cost_price().filter(|p| p.cents() > 0);
                if !experience.feedback.stocks.contains_key(code) {
                    experience
                        .initialize_holding_dated(code, cost_price, stock.last_price, moment)
                        .map_err(|e| RuntimeError::ProtocolInvariant {
                            detail: format!("组 {group} retail {account:?} 首建经验失败：{e}"),
                        })?;
                }
                experience
                    .observe_position_dated(code, stock.last_price, moment)
                    .map_err(|e| RuntimeError::ProtocolInvariant {
                        detail: format!("组 {group} retail {account:?} 持仓经验失败：{e}"),
                    })?;
            }
            if equity.cents() > 0 {
                experience
                    .observe_equity(equity)
                    .map_err(|e| RuntimeError::ProtocolInvariant {
                        detail: format!("组 {group} retail {account:?} 权益经验失败：{e}"),
                    })?;
            }
        }
        // P2-3a：等权市场聚合为公共量——本轮共享（原每位各聚合一次）。
        let thirty_minute_market = behavior.thirty_minute_market;
        // P2-3d：六件自 snap 取走（journal 已持拍前副本——构建零 clone）。
        let mut personal = crate::session::PlanPersonalState {
            attention: snap.attention,
            watchlist: snap.watchlist,
            price_memory: snap.price_memory,
            history_reads: snap.history_reads,
            information: snap.information,
            belief: snap.belief,
        };
        let assessments = crate::GameSession::observe_retail_analysis_core(
            &ctx,
            &self.plan_book,
            &experience,
            &mut personal,
            observation,
            &thirty_minute_market,
        )
        .map_err(|e| RuntimeError::ProtocolInvariant {
            detail: format!("组 {group} retail {account:?} 观察失败：{e:?}"),
        })?;
        // 决策（旧管线 decide_with_experience+apply_personal_analysis 等价）：
        /// behavior+risk+dated 经验输入；rng=npc_seed_base 同式派生。
        // own 自 ctx.positions 同源组装（决策内核要求 own.positions ⊆
        /// risk.positions——build_self_view 不过滤零量股，多股/零仓形态
        /// 集合漂移即 panic）。
        let own = crate::strategy::SelfView {
            cash: ctx.cash,
            positions: ctx
                .positions
                .iter()
                .map(|(code, position)| {
                    (
                        *code,
                        crate::strategy::PositionView {
                            qty: position.qty(),
                            sellable_qty: position.sellable(),
                            cost_price: position.cost_price(),
                        },
                    )
                })
                .collect(),
        };
        // P2-3a：behavior 观察面=本轮共享引用（原每位 paths BTreeMap 克隆
        /// 一次——下游 decide_* 直接消费共享引用）。
        let mut risk_inputs = std::collections::BTreeMap::new();
        for (code, position) in &ctx.positions {
            let stock = observation
                .market
                .stocks
                .get(code)
                .ok_or_else(|| RuntimeError::ProtocolInvariant {
                    detail: format!("组 {group} retail {account:?} 持仓 {code:?} 无行情"),
                })?;
            risk_inputs.insert(
                *code,
                crate::observation::RiskPositionInput {
                    qty: position.qty(),
                    cost_price: position.cost_price(),
                    last_price: stock.last_price,
                    peak_price_since_entry: experience
                        .stocks
                        .get(code)
                        .and_then(|row| row.peak_price_since_entry),
                },
            );
        }
        let risk = crate::observation::build_account_risk_observation(
            ctx.cash,
            &risk_inputs,
            experience.reference_equity,
            experience.peak_equity,
        )
        .map_err(|e| RuntimeError::ProtocolInvariant {
            detail: format!("组 {group} retail {account:?} 风险观察失败：{e}"),
        })?;
        let mut dated = experience.clone();
        dated.consecutive_failed_buys = dated
            .failure_influence(&crate::experience::ExperienceMoment {
                civil_date: snapshot.civil_date,
                market_minute: minute,
                trading_day: day,
            })
            .map_err(|e| RuntimeError::ProtocolInvariant {
                detail: format!("组 {group} retail {account:?} 经验时刻失败：{e}"),
            })?;
        let npc_seed = crate::session::npc_rng_seed(
            projection.session_seed,
            tick,
            account,
        );
        let mut rng = crate::session::SplitMix64::new(npc_seed);
        let data = profile.strategy.clone();
        // 决策组装（ZiNoiseStrategy::decide_with_experience 等价——
        /// PositionDecision → intents/reviewed/trace 三面）。
        let original = crate::behavior::decide_retail_position_with_experience(
            &data,
            retail_style,
            market,
            &own,
            &behavior,
            &risk,
            &dated,
            minute,
            &mut rng,
        );
        let position = if data.arrival_rate == 0.0 {
            original
        } else {
            // 个人分析应用（apply_personal_analysis 等价——分析驱动目标
            /// 仓位修订）。
            crate::behavior::apply_personal_analysis(
                original.clone(),
                &assessments,
                &own,
                market,
                &risk,
                &dated,
                minute,
                f64::from(data.position_step_bp) / 10_000.0,
                &mut rng,
            )
        };
        let intents = crate::strategy::retail_position_decision_to_intents(
            &data,
            &position,
            market,
            &own,
            &self.config,
        );
        let reviewed_stocks: std::collections::BTreeSet<crate::StockId> =
            position.code.iter().cloned().collect();
        let urgency = crate::plans::urgency::risk::assess_personal_risk_urgency(
            Some(&position),
            Some(&risk),
            &projection.urgency_policy,
        )
        .map_err(|e| RuntimeError::ProtocolInvariant {
            detail: format!("组 {group} retail {account:?} 紧迫度评估失败：{e}"),
        })?;
        // 经验行演进（旧管线归并面：reviewed 股 observe_stock+未持仓修剪）。
        for code in &reviewed_stocks {
            experience.observe_stock(code, minute);
        }
        experience.prune_watchlist(&held);
        // trace 收集（本拍缓冲——驱动器段投影 session）。
        self.retail_traces.push(crate::session::RetailDecisionTrace {
            account,
            decision: position,
            execution_urgency: urgency,
        });
        // 意图 → PendingOp。
        let mut ops = Vec::with_capacity(intents.len());
        for intent in intents {
            if let Some(mut op) = super::decision::intent_to_pending(intent, market) {
                op.local = local;
                ops.push(op);
            }
        }
        // 组态写回（个人态六件+经验行）。P2-3d：undo 已在 take 时记录；
        /// 写回字段自 personal move（personal 此后无消费——六件 clone 归零）。
        if let Some(slot) = self.belief_states.get_mut(local as usize) {
            *slot = Some(super::world::BeliefParticipantSnapshot {
                watchlist: personal.watchlist,
                price_memory: personal.price_memory,
                information: personal.information,
                belief: personal.belief,
                attention: personal.attention,
                history_reads: personal.history_reads,
                retail_experience: Some(experience),
            });
        }
        // P2-1a：retail 位生产写回=dirty（EchoBelief 掩码+驱动器重装面）。
        if let Some(flag) = self.belief_dirty.get_mut(local as usize) {
            *flag = true;
        }
        Ok(ops)
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
        let Some(&account) = projection
            .account_ids
            .get(self.roster_offset + local as usize)
        else {
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
        let mut assessments_map = operations
            .lifecycle_assessments()
            .into_iter()
            .find(|(owner, _)| *owner == account)
            .map(|(_, by_code)| by_code)
            .unwrap_or_default();
        // D3b 件 4 fixture 通道：注入候选替代 observe 产物（复审块+开仓
        // 循环同源——chain_restructure 测试的 drive_once 等价面；一次
        // 消费即清，注入不跨拍）。
        #[cfg(test)]
        {
            let injected: Vec<_> = self
                .injected_assessments
                .iter()
                .position(|(owner, _)| *owner == account)
                .map(|idx| self.injected_assessments.remove(idx))
                .into_iter()
                .collect();
            if let Some((_, injected_map)) = injected.into_iter().next() {
                assessments_map = injected_map;
            }
        }
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
        let trading_day = snapshot.tick.saturating_sub(1) / self.ticks_per_day;
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
                    // 复审时点面：observe 输出（assessments/behavior/acquired
                    /// 同源——belief_states 是 observe 前旧态）。
                    belief: &out.belief,
                    information: &out.information,
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
                        account,
                        trading_day,
                        &projection,
                        &out.information,
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
                // Sol 终审四审 A2：开仓复审 trading_day 同 (tick−1)/tpd 基准。
                snapshot.tick.saturating_sub(1) / self.ticks_per_day,
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
                let acquired = group_review_acquired(&projection, &out.information, code);
                self.plan_book
                    .record_review(plan_id, trading_day, price, acquired)
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
                    &out.belief,
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
            self.push_undo(GroupUndo::Personal {
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
                    // 经验行非 observe 产物——保留组内现值（retail 链
                    /// 的决策段演进）。
                    retail_experience: slot.as_ref().and_then(|s| s.retail_experience.clone()),
                });
            }
            // P2-1a：chain 位生产写回=dirty（EchoBelief 掩码+驱动器重装面）。
            if let Some(flag) = self.belief_dirty.get_mut(local as usize) {
                *flag = true;
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
        account: crate::AccountId,
        trading_day: u64,
        projection: &std::sync::Arc<super::world::RootProjection>,
        information: &crate::information::NpcInformationState,
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
            // 复审事实：price=快照行情；acquired=issuer 记录计数
            //（group_review_acquired——session plan_review_facts 同式）。
            let price = market
                .stocks
                .get(&code)
                .map(|v| v.last_price)
                .unwrap_or_else(|| panic!("组内复审缺股 {code:?} 行情"));
            let idx = code.0 as usize;
            let acquired = group_review_acquired(projection, information, &code);
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
                .record_review(plan_id, trading_day, price, acquired)
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
                // release 路径同源；簿侧腿引用同拍释放——session 执行链
                /// 的撤单即清语义，Closed 落账的 release 幂等跳过）。
                if let Some(order_id) = child_order_id {
                    cancels.push((
                        local,
                        code.0 as StockIdx,
                        desk_order_serial(order_id.0),
                    ));
                    // 幂等（Ok(false)=Closed 落账先行清理）容忍；Err（非法
                    /// 迁移）显式失败——铁律二。
                    let released = self
                        .plan_book
                        .record_child_released(account, &code, order_id, trading_day, true);
                    if let Err(error) = released {
                        panic!("组内复审终止腿释放失败 {plan_id:?}: {error:?}");
                    }
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
                // 先撤在途子单再 apply 修订——生命周期约束，决策核注记；
                /// 同向 Forward 修订不清腿，簿侧腿引用随撤单同拍释放——
                /// session 执行链语义，Closed 落账的 release 幂等跳过）。
                if let Some(order_id) = child_order_id {
                    cancels.push((
                        local,
                        code.0 as StockIdx,
                        desk_order_serial(order_id.0),
                    ));
                    // 幂等（Ok(false)=Closed 落账先行清理）容忍；Err（非法
                    /// 迁移）显式失败——铁律二。
                    let released = self
                        .plan_book
                        .record_child_released(account, &code, order_id, trading_day, true);
                    if let Err(error) = released {
                        panic!("组内复审重构腿释放失败 {plan_id:?}: {error:?}");
                    }
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
            self.push_undo(GroupUndo::PlanBook {
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
                            let serial = desk_order_serial(order_id.0);
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
                    let serial = desk_order_serial(order_id.0);
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
                    trading_day: snapshot.tick.saturating_sub(1) / self.ticks_per_day,
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
            self.push_undo(GroupUndo::ChildInFlight {
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
                // P1-B（性能修复）：本轮共享公共视图与 attention signal——
                /// 此前每 due NPC 各自 build_market_view（重建全市场
                /// BTreeMap+复制价格/分钟历史）只为算一个对全组相同的
                /// 公共 signal，observe 通过后 decide 再建一次（构造次数
                /// ≈3.4× 放大，见 .tmp/perf-p0-diff-profile.md）。两者均为
                /// snapshot 的纯函数且不消费 RNG——共享后决策序列/RNG 流
                /// 逐位不变（等价守卫=同种子终态逐字段对比）。
                let shared_market = super::decision::build_market_view(&snapshot);
                let shared_signal = crate::session::market_attention_signal(&shared_market);
                // D3a-3：belief 位的决策链轮（observe 直调——方向 A 深移植）
                // 与 retail 的 decide_data 轮分离：due 判定+重排在 profiles
                // 借用内完成（Sol 二审修复集 5 的 undo 语义），observe 链
                // 在借用外执行（组态/簿的演进借用互斥）。
                let mut chain_due: Vec<LocalIdx> = Vec::new();
                // D5 #4 retail 链：retail 位（个人态在位+零售风格）的
                /// 决策延后到借用释放（组态写回与决策档案借用互斥）。
                let mut retail_due: Vec<LocalIdx> = Vec::new();
                {
                    for local in 0..profiles.len() as LocalIdx {
                        let belief_seeded = self
                            .belief_states
                            .get(local as usize)
                            .is_some_and(|slot| slot.is_some());
                        // D5 #4：链位判定加链参数面——retail 位个人态在位
                        /// 但无机构链参数，走 retail 链（观察+behavior 决策）。
                        let chain_params_seeded = self
                            .root_projection
                            .as_ref()
                            .is_some_and(|p| {
                                p.chain_params.get(self.roster_offset + local as usize).is_some_and(|c| c.is_some())
                            });
                        let chain_local = self.root_projection.is_some()
                            && belief_seeded
                            && chain_params_seeded;
                        let retail_local = self.root_projection.is_some()
                            && belief_seeded
                            && !chain_params_seeded
                            && profiles[local as usize].retail_style.is_some();
                        if chain_local || retail_local {
                            if tick < profiles[local as usize].next_due {
                                continue;
                            }
                            // D5 #2：signal-based evaluate 门（旧管线
                            /// evaluate_candidate_with_signal 同式：共享续流+
                            /// observe 门+wait 采样；observe=false 只重排不
                            /// 决策（链轮跳过——决策频率语义与旧管线对齐，
                            /// 此前均匀 gap 近似使 epoch 双轨显红）。
                            let (observes, prev_due) =
                                super::decision::evaluate_attention(
                                    &mut profiles[local as usize],
                                    shared_signal,
                                    tick,
                                );
                            due_npcs += 1;
                            pending_next_due.push((local, prev_due));
                            // P2-3e：evaluate 消费过 RNG/重排 next_due——
                            /// 稀疏上行演进位标记（observe=false 也演进）。
                            self.attention_evolved.push(local);
                            if observes {
                                decide_calls += 1;
                                if chain_local {
                                    chain_due.push(local);
                                } else {
                                    retail_due.push(local);
                                }
                            }
                            continue;
                        }
                        // D5 #2：due 外判（decide_account 内判已随纯决策化
                        /// 移除——调用方控制契约）。
                        if tick < profiles[local as usize].next_due {
                            continue;
                        }
                        let account = &self.accounts[local as usize];
                        // D5 #2：retail 同式 evaluate 门（observe=false 只
                        /// 重排不决策）。
                        let (observes, prev_due) = super::decision::evaluate_attention(
                            &mut profiles[local as usize],
                            shared_signal,
                            tick,
                        );
                        due_npcs += 1;
                        pending_next_due.push((local, prev_due));
                        // P2-3e：同上——稀疏上行演进位标记。
                        self.attention_evolved.push(local);
                        if observes {
                            decide_calls += 1;
                            let (ops, cancels) = decide_account(
                                &mut profiles[local as usize],
                                group_mix,
                                tick,
                                local,
                                &shared_market,
                                account,
                                &self.config,
                            );
                            cancel_intents_skipped += cancels as u64;
                            self.cancel_intents_total += cancels as u64;
                            for op in ops {
                                pendings.push(Pending::Op(op));
                            }
                        }
                    }
                }
                // retail 的 undo 入账 + belief 位 observe 链（借用已释放）。
                for (local, prev_due) in std::mem::take(&mut pending_next_due) {
                    self.decide_calls_total += 1;
                    self.push_undo(GroupUndo::NextDue { local, prev: prev_due });
                }
                for local in chain_due {
                    let (_record, child_ops, cancels) =
                        self.run_chain_decision(local, tick, true)?;
                    for op in child_ops {
                        pendings.push(Pending::Op(op));
                    }
                    chain_cancels.extend(cancels);
                }
                // D5 #4 retail 链：观察+决策+trace+经验行（组内权威）。
                // P1-B：复用本轮共享公共视图（原每轮零售位再建一次——
                /// 同 snapshot 纯函数，逐位等价）。
                // P2-3a：链观察/等权聚合/behavior 价径面同样上移每轮一次
                ///（原每位 build_chain_observation+聚合+paths 克隆——组线程
                /// 拷贝放大主源；同轮输入逐位相同 ⇒ 等价论证同 P1-B）。
                if !retail_due.is_empty() {
                    let projection = self.root_projection.as_ref().ok_or_else(|| {
                        RuntimeError::ProtocolInvariant {
                            detail: format!(
                                "组 {} 零售公共观察缺投影（retail 位分类前置已要求投影在位——结构性不可达）",
                                self.group
                            ),
                        }
                    })?;
                    let chain = build_chain_observation(
                        &self.observation_inputs,
                        &snapshot,
                        self.ticks_per_day,
                        projection.auction_ticks,
                        projection.closing_auction_ticks,
                        &self.library,
                        &projection.company_registry,
                        &projection.stock_codes,
                    );
                    let returns: std::collections::BTreeMap<crate::StockId, Option<f64>> = chain
                        .paths
                        .iter()
                        .map(|(stock, path)| (*stock, path.thirty_minute.return_ratio))
                        .collect();
                    let thirty_minute_market =
                        crate::observation::build_equal_weight_market_observation(&returns)
                            .map_err(|e| RuntimeError::ProtocolInvariant {
                                detail: format!(
                                    "组 {} 零售公共观察市场聚合失败：{e}",
                                    self.group
                                ),
                            })?;
                    let behavior = crate::behavior::BehaviorMarketObservation {
                        price_paths: chain.paths.clone(),
                        thirty_minute_market,
                    };
                    for local in retail_due {
                        let ops = self.run_retail_decision(
                            local,
                            tick,
                            &snapshot,
                            &shared_market,
                            &chain,
                            &behavior,
                        )?;
                        for op in ops {
                            pendings.push(Pending::Op(op));
                        }
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
        // D5 终程：外部定向撤单延迟到下单 flush 之后（同批 Place+Cancel
        /// 的 desk 到达序——Cancel 须晚于 Place 才能命中）。
        let external_cancels = std::mem::take(&mut self.external_cancels);
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
        // step_frame 接轨前置约束① fixture 通道：一次性强制续行标记
        ///（消费即清——仅驱动 Continue 轮的触发面，不改决策/落账语义）。
        #[cfg(test)]
        if self.force_continuation {
            self.force_continuation = false;
            continuation = true;
        }
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
        let mut external_cancel_buffer: Vec<(LocalIdx, super::StockIdx, u64)> =
            external_cancels;
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
            // Sol 复审修复 3（校验顺序）：竞价相位×市价门禁**先于**资源
            /// 校验——保证相位显式回执可达（资源不足不误吞相位拒收；
            /// 对齐旧管线相位门禁在市场层入口的次序）。相位基准
            /// phase_at(tick−1)（与 desk 受理同窗——D5 off-by-one 口径）。
            if matches!(p.routed, super::protocol::RoutedPrice::Market) {
                let day_tick = (tick.saturating_sub(1)) % self.ticks_per_day;
                let entry = self.auction_ticks - self.auction_ticks / 3;
                let closing_start =
                    self.ticks_per_day.saturating_sub(self.closing_auction_ticks);
                let in_auction = day_tick < entry
                    || (day_tick >= closing_start && self.closing_auction_ticks > 0);
                if in_auction {
                    self.skipped_orders += 1;
                    self.round_rejections.push(super::DeskReceipt {
                        order: (p.stock, 0),
                        owner,
                        kind: super::ReceiptKind::AuctionLimitOrderRequired,
                    });
                    continue;
                }
            }
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
                        // 严格所有权：在途退款未到账 ⇒ skip（不部分执行）。
                        // Sol 复审修复 2：计数不替代玩家回执——显式
                        // InsufficientCash 进本轮回执流。
                        self.skipped_orders += 1;
                        self.round_rejections.push(super::DeskReceipt {
                            order: (p.stock, 0),
                            owner,
                            kind: super::ReceiptKind::InsufficientCash,
                        });
                        continue;
                    }
                    acct.cash -= purse;
                    acct.foreign_active[p.stock as usize] += 1;
                    // Sol 五审：在途买占用镜像（全部买单——公开现金口径
                    // 的加回面；成交/退款由 apply_staging 实体驱动减）。
                    acct.buy_purse_held_cents += purse;
                    self.push_undo(GroupUndo::Cash {
                        local: p.local,
                        delta: -purse,
                    });
                    self.push_undo(GroupUndo::BuyPurseHeld {
                        local: p.local,
                        delta: purse,
                    });
                    self.push_undo(GroupUndo::ForeignActive {
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
                        // 含 T+1：当日买入不可卖。Sol 复审修复 2：显式
                        // InsufficientShares 回执（计数不替代玩家结果）。
                        self.skipped_orders += 1;
                        self.round_rejections.push(super::DeskReceipt {
                            order: (p.stock, 0),
                            owner,
                            kind: super::ReceiptKind::InsufficientShares,
                        });
                        continue;
                    }
                    // 零股须一次性卖出：非整手数量只允许恰好等于全部可卖零头。
                    if p.qty % lot != 0 && p.qty != sellable {
                        self.skipped_orders += 1;
                        self.round_rejections.push(super::DeskReceipt {
                            order: (p.stock, 0),
                            owner,
                            kind: super::ReceiptKind::NotRoundLot,
                        });
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
                    self.push_undo(GroupUndo::Shares {
                        local: p.local,
                        stock: p.stock,
                        delta: -(p.qty as i32),
                    });
                    self.push_undo(GroupUndo::ForeignActive {
                        local: p.local,
                        stock: p.stock,
                        delta: 1,
                    });
                    self.push_undo(GroupUndo::PendingSell {
                        local: p.local,
                        stock: p.stock,
                        delta: i64::from(p.qty),
                    });
                    if let Some(prev) = ever_flip {
                        self.push_undo(GroupUndo::EverHeld {
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
        // 外部定向撤单在下单 flush 之后（同批 Place+Cancel 的 desk 到达
        /// 序——Cancel 晚于 Place 才能命中簿内单）。
        for (local, stock, serial) in std::mem::take(&mut external_cancel_buffer) {
            buffers[stock as usize].push(DeskIn::Cancel {
                owner: (group, local),
                order: (stock, serial),
            });
        }
        // 名册末尾：每股发 final_flush 空批作为本组本轮提交标记（可空）。
        ops_sent += flush_buffers(&mut buffers, desk_txs, tick, round, group, true)?;
        // P2-2（视图内生战役）：belief 稀疏分片随报告上行（替代驱动器每拍
        // EchoBelief 独立往返）。组装口径与 EchoBelief 应答逐字同源
        //（dirty ∪ 跨 Start pending——P2-2a 语义）。跨轮保留机制：dirty
        // 在 Continue 轮间不清（清点在 Start）→ 终轮分片天然含全部轮的
        // 演进位；pending 在首轮构建时并入 dirty（消费一次、随 dirty 跨
        // 轮到终轮——跨 Start 注入窗口语义保持）。
        let pending_belief = std::mem::take(&mut self.belief_pending_dirty);
        for local in pending_belief {
            if let Some(flag) = self.belief_dirty.get_mut(local as usize) {
                *flag = true;
            }
        }
        let belief_sparse: Vec<(_, _)> = self
            .belief_states
            .iter()
            .zip(self.belief_dirty.iter())
            .enumerate()
            .filter_map(|(i, (slot, dirty))| {
                (*dirty)
                    .then(|| slot.as_ref().map(|s| (i as super::LocalIdx, s.clone())))
                    .flatten()
            })
            .collect();
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
            // 组端业务拒绝（Sol 复审修复 2）：本轮排空上行——Abort 重跑
            /// 时由校验路径重新产生（run_round 起点清零）。
            rejections: std::mem::take(&mut self.round_rejections),
            belief_sparse: std::sync::Arc::new(belief_sparse),
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
            seal_handle_ns: self.seal_handle_ns,
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
    /// 复审时点 belief（observe 输出——复审的 assessments 与行为面同源；
    /// belief_states 是 observe 前旧态，session 版复审定在 observe 后原地
    /// 演进的 belief 上，时点必须对齐）。
    belief: &'a crate::strategy::BeliefBook,
    /// 复审时点 information（同上——acquired 计数与 observe 后状态一致）。
    information: &'a crate::information::NpcInformationState,
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
            self.belief,
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
        // acquired 数：issuer 记录计数（group_review_acquired——session
        // plan_review_facts 同式：股→发行人→information 记录；复审时点
        /// 的 observe 输出面）。
        let acquired = group_review_acquired(
            self.state.root_projection.as_ref().expect("复审位投影在"),
            self.information,
            code,
        );
        (price, acquired)
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
        // 复审时点面（observe 输出——与宿主其余面同源；profile 冻结故
        /// 与 belief_states 旧态值恒同，时点统一消分叉面）。
        match self.belief.profile() {
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
    belief: &crate::strategy::BeliefBook,
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

/// 复审 acquired 数（与 session `plan_review_facts` 同式：股→发行人→
/// information 记录计数；issuer 查询经投影 stock_codes+registry）。此前
/// 恒 0 的近似使 quiet 判定的 acquired 比对永不成立（组记录 0 vs session
/// 真实计数）——D3b 件 4 fixture 适配红证 institution_small_signal。
fn group_review_acquired(
    projection: &std::sync::Arc<super::world::RootProjection>,
    information: &crate::information::NpcInformationState,
    code: &crate::StockId,
) -> u32 {
    projection
        .stock_codes
        .iter()
        .find(|(_, id)| id == code)
        .and_then(|(code_str, _)| projection.company_registry.issuer_of(code_str))
        .map(|company| information.records_for_company(&company).len())
        .map(|len| u32::try_from(len).expect("publication ids fit in u32"))
        .unwrap_or(0)
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

/// 交易日序号（(tick−1)/tpd——首拍属 day 0，与 session.day 同基；
/// Sol 终审四审 A2：此前 tick/tpd 使组内观察/经历时刻的 trading_day
/// 超前 1，与复审 as_of 错位触发 AsOfBeforeLatestEvent）。
fn context_day(tick: u64, ticks_per_day: u64) -> u64 {
    tick.saturating_sub(1) / ticks_per_day
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
    let day = u32::try_from(tick.saturating_sub(1) / ticks_per_day).expect("交易日序号超 u32");
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
    // Sol 终审四审 A2：day=(tick−1)/tpd；day_tick=(tick−1)%tpd（与 day
    /// 同受理窗——跨日 minute 单调；曾试对齐 session 的 tick%tpd 异基
    /// 式，institution 观察 clock（roots.rs 要求单调）跨日显红
    /// "attempted 0 after last 236"——组内观察 clock 以同窗单调为先，
    /// session 的异基式不适配组内，登记差异）。
    let day = u32::try_from(tick.saturating_sub(1) / ticks_per_day).expect("交易日序号超 u32");
    let day_tick = tick.saturating_sub(1) % ticks_per_day;
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
    shutdown: Arc<AtomicBool>,
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
        // Sol 终审三审：退出判定用 shutdown（永久关停——sever/Drop）；
        // abort 只管本 tick 回滚语义（本 worker 失败时置位通知对端）。
        let msg = match worker_recv(&rx, &watchdog, &shutdown, who) {
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
                // eager 分域即时相：到达即应用 F1/F2/F3（等待窗=组空闲
                // recv——与协调者收 DeskReport 的 Seal 屏障重叠，回收串行
                // 税）；F4/F5/F6 暂存延迟相（Continue/Seal 消息点重放）。
                // 失败显式上报（与 Seal 期处理同形态——不静默）。
                state.observed.push(Observed::Batch(batch.tick));
                // SealAck 后到达=锁步破坏面（计数供 Start 防御；日结 Drain
                /// 序列由 DayEnd 消化清零——批次本身照常即时应用，防御
                /// 不改变落账行为只改变上报时点）。
                if state.seal_acked_len.is_some() {
                    state.batches_after_seal += 1;
                }
                let (cur_tick, cur_round) = (state.cur_tick, state.cur_round);
                if let Err(e) = state.apply_batch_immediate(batch, cur_tick, cur_round) {
                    abort.store(true, Ordering::SeqCst);
                    let _ = up_tx.send(UpMsg::Failed {
                        who,
                        error: e.clone(),
                    });
                    result = Err(e);
                    break;
                }
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
            GroupIn::ExternalPlan { ops, cancels } => {
                // 玩家意图路由（D2 批）：外部计划存缓冲，run_round 消费。
                state.external_pending.extend(ops);
                state.external_cancels.extend(cancels);
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
                                // P2-1a：投递演进=dirty（EchoBelief 掩码+
                                /// 驱动器重装面）。
                                if let Some(flag) = state.belief_dirty.get_mut(li) {
                                    *flag = true;
                                }
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
                // P2-2a：稀疏上行——只 clone 本 tick dirty 且在册的条目
                //（省全名册深拷贝；驱动器增量缓存承载全量读面语义）。
                let pending = std::mem::take(&mut state.belief_pending_dirty);
                let states: Vec<(_, _)> = state
                    .belief_states
                    .iter()
                    .zip(state.belief_dirty.iter())
                    .enumerate()
                    .filter_map(|(i, (slot, dirty))| {
                        (*dirty || pending.contains(&(i as super::LocalIdx)))
                            .then(|| slot.as_ref().map(|s| (i as super::LocalIdx, s.clone())))
                            .flatten()
                    })
                    .collect();
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
            GroupIn::AcceptedBatch { entries } => {
                // P1-A 受理按组批发：逐条走与 ChildAccepted 完全相同的
                // 消费路径（Observed 记录+adopt——发送侧已按 owner 分桶，
                // 过滤判定保留为防御不变式）。批内顺序=该轮 desk_lifecycle
                // 原序；任一条失败即整轮失败（不静默跳过）。
                for (stock, owner, order_serial) in entries {
                    if owner.0 == state.group {
                        state
                            .observed
                            .push(Observed::ChildAccepted(stock, owner, order_serial));
                        if let Err(e) = state.adopt_child_on_accepted(stock, owner.1, order_serial)
                        {
                            abort.store(true, Ordering::SeqCst);
                            let _ = up_tx.send(UpMsg::Failed { who, error: e.clone() });
                            result = Err(e);
                            break;
                        }
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
                        // P2-2a：注入=跨 Start 待上行（Start 会清本 tick
                        /// 的 belief_dirty——注入态由 pending 承载到上行消费）。
                        state.belief_pending_dirty.push(local);
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
            GroupIn::InjectReviewAssessments {
                account,
                assessments,
            } => {
                // D3b 件 4 fixture 通道：存入待办（run_chain_decision 按
                // 账户匹配消费一次即清——不复读）。
                state.injected_assessments.push((account, assessments));
            }
            #[cfg(test)]
            GroupIn::MutatePlanBookForTest(mutate) => {
                // D3b 件 4 fixture 通道：组簿测试变换（簿快照 undo 可逆
                /// ——Abort 冲回后指令失效需重发）。
                state.save_plan_book_undo();
                mutate(&mut state.plan_book);
            }
            #[cfg(test)]
            GroupIn::InjectObservationMinutes { stock, minutes } => {
                // D3b 件 4 fixture 通道：观察基分钟覆盖槽登记（Start 的
                // 整值刷新会覆盖直写——覆盖槽在刷新后套用，持续到改写）。
                state.observation_overrides.insert(stock, minutes);
            }
            #[cfg(test)]
            GroupIn::MutateBeliefForTest { account, mutate } => {
                // D3b 件 4 fixture 通道：belief 簿测试变换（account→local
                /// 经投影名册映射——目标位缺失=显式失败）。
                let local = state
                    .root_projection
                    .as_ref()
                    .and_then(|p| {
                        p.account_ids
                            .iter()
                            .position(|id| *id == account)
                            .map(|idx| idx as LocalIdx)
                    })
                    .unwrap_or_else(|| {
                        panic!("MutateBeliefForTest 目标账户 {account:?} 不在组名册")
                    });
                match state.belief_states.get_mut(local as usize) {
                    Some(Some(snapshot)) => {
                        let prev = snapshot.clone();
                        state.undo.push(GroupUndo::Personal {
                            local,
                            prev: Box::new(prev),
                        });
                        mutate(&mut snapshot.belief);
                    }
                    _ => panic!("MutateBeliefForTest 目标位 {local} 无 belief 态"),
                }
            }
            #[cfg(test)]
            GroupIn::ForceContinuationForTest => {
                // step_frame 接轨前置约束① fixture 通道：下一次轮上报
                // 强制 continuation=true（一次消费即清——Start 间窗口
                // 注入则由下一次 run_round 消费，指令本身无状态残留）。
                state.force_continuation = true;
            }
            #[cfg(test)]
            GroupIn::DeferDueForTest { locals, tick } => {
                // D5 #4 fixture 通道（cfg(test)）：推远 next_due——"无 due
                /// tick"形态构造（组权威调度面的测试控制；效果入可逆日志）。
                for local in locals {
                    if let Some((profiles, _)) = &mut state.decision {
                        if let Some(p) = profiles.get_mut(local as usize) {
                            let prev = p.next_due;
                            if prev < tick {
                                state.undo.push(GroupUndo::NextDue {
                                    local,
                                    prev,
                                });
                                p.next_due = tick;
                            }
                        }
                    }
                }
            }
            #[cfg(test)]
            GroupIn::ForceDue {
                locals,
                tick,
                ensure_observe,
            } => {
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
                    state
                        .pending_force_due
                        .push((local, tick, ensure_observe));
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
                // D5 #2：next_due+注意力 rng 续传态（evaluate 共享流——
                /// session rng_state 同源写回）。D5 restore 链最后一层：
                /// 构造期组因子混入在回传时逆运算——session 面存纯值
                ///（restore 再构造混入恰一次，不叠加）。
                let attention = match &state.decision {
                    Some((profiles, _)) => profiles
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let mixed = (state.group as u64)
                                .wrapping_mul(0x9E3779B97F4A7C15)
                                .wrapping_add(i as u64);
                            (i as LocalIdx, p.next_due, p.seed ^ mixed)
                        })
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
            GroupIn::EchoAttentionSparse => {
                // P2-3e：生产安装段稀疏上行——只回本 tick 演进位
                ///（evaluate 消费 RNG/重排 next_due，或 ForceDue 改排的位）。
                /// 非演进位 session 镜像==组权威（构造期同源+只经本通道
                /// 同步——不变式同 belief P2-2a），全量语义由驱动器缓存/
                /// session 镜像承载。全量读面用 EchoAttention（构造恢复/
                /// Abort 测试锚）不变。
                let evolved = std::mem::take(&mut state.attention_evolved);
                let attention = match &state.decision {
                    Some((profiles, _)) => evolved
                        .into_iter()
                        .map(|i: LocalIdx| {
                            let mixed = (state.group as u64)
                                .wrapping_mul(0x9E3779B97F4A7C15)
                                .wrapping_add(u64::from(i));
                            (i, profiles[i as usize].next_due, profiles[i as usize].seed ^ mixed)
                        })
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
                        detail: format!("组 {who} GroupAttention(稀疏) 发送失败"),
                    });
                    break;
                }
            }
            GroupIn::EchoRetailTraces => {
                // D5 #4 retail 链：本拍 trace 上行（瞬态读面——Start 清、
                /// 生产写入存活到提交）。
                let msg = UpMsg::RetailTraces {
                    group: state.group,
                    traces: state.retail_traces.clone(),
                };
                if up_tx.send(msg).is_err() {
                    abort.store(true, Ordering::SeqCst);
                    result = Err(RuntimeError::SendFailed {
                        detail: format!("组 {who} RetailTraces 发送失败"),
                    });
                    break;
                }
            }
            GroupIn::EchoAccounts => {
                // 在途买占用为账户级维护字段（Sol 五审：run_round 挂出/
                /// apply_staging 成交与退款驱动——全部买单，不限计划链票）。
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
            GroupIn::Seal {
                tick,
                round_final,
            } => {
                // Q2 Seal 收尾确认（S1-S4）：drain 本拍全部批次——S4 锁步
                /// 不变量：协调者收齐全部 DeskReport（desk 批次 FIFO 先于
                /// DeskReport 入组收件箱）后才发 Seal ⇒ 组处理到 Seal 时
                /// 全部批次已在收件箱，逐条消费进 staging 后一次落账
                ///（S2：落账点从下一 Start 前移到 Seal——apply_staging
                /// 唯一化）。落账进 undo 日志（Abort 可冲回——S3：undo
                /// 跨发布无害，Start 清点不变）。期末哈希随 SealAck 上行。
                if tick != state.cur_tick || round_final != state.cur_round {
                    // 铁律二收口（seal_failed_injection_tests 发现的缺口）：
                    /// 协议错位退出必须 Failed 上行+abort 置位（对齐其他
                    /// 失败分支——不静默只 break 会使协调者退化为
                    /// watchdog/断连超时发现）。
                    abort.store(true, Ordering::SeqCst);
                    let e = RuntimeError::ProtocolInvariant {
                        detail: format!(
                            "组 {} Seal 错位：期望 ({},{})，收到 ({tick},{round_final})",
                            state.group, state.cur_tick, state.cur_round
                        ),
                    };
                    let _ = up_tx.send(UpMsg::Failed {
                        who,
                        error: e.clone(),
                    });
                    result = Err(e);
                    break;
                }
                // Seal 地板分解诊断：组端处理段（drain→Ack 发送前）计时。
                let seal_handle_started = std::time::Instant::now();
                // 收件箱顺序 drain：Seal 前入队的全部数据面消息（S4：
                /// desk 批次 FIFO 先于其 DeskReport ⇒ 先于 Seal 入队）。
                /// 数据面=Batch（实体批次）与 AcceptedBatch（受理身份）；
                /// 其他消息（外部意图/回声请求/Abort 等）属协议错位——
                /// Seal 相位不应出现，显式失败不静默。
                loop {
                    match rx.try_recv() {
                        Ok(GroupIn::Batch(batch)) => {
                            // eager 分域即时相（Seal drain 批次与等待窗批次
                            /// 同路径——S4 FIFO 锁步语义不变）。
                            state.observed.push(Observed::Batch(batch.tick));
                            if let Err(e) =
                                state.apply_batch_immediate(batch, tick, round_final)
                            {
                                abort.store(true, Ordering::SeqCst);
                                let _ = up_tx.send(UpMsg::Failed { who, error: e.clone() });
                                result = Err(e);
                                break;
                            }
                        }
                        Ok(GroupIn::AcceptedBatch { entries }) => {
                            for (stock, owner, order_serial) in entries {
                                if owner.0 == state.group {
                                    state
                                        .observed
                                        .push(Observed::ChildAccepted(stock, owner, order_serial));
                                    if let Err(e) =
                                        state.adopt_child_on_accepted(stock, owner.1, order_serial)
                                    {
                                        abort.store(true, Ordering::SeqCst);
                                        let _ = up_tx.send(UpMsg::Failed { who, error: e.clone() });
                                        result = Err(e);
                                        break;
                                    }
                                }
                            }
                            if result.is_err() {
                                break;
                            }
                        }
                        Ok(other) => {
                            let _ = other;
                            // 铁律二收口：同上——协议错位显式上报（不静默）。
                            abort.store(true, Ordering::SeqCst);
                            let e = RuntimeError::ProtocolInvariant {
                                detail: format!(
                                    "组 {} Seal 期收到非数据面消息（协议错位）",
                                    state.group
                                ),
                            };
                            let _ = up_tx.send(UpMsg::Failed {
                                who,
                                error: e.clone(),
                            });
                            result = Err(e);
                            break;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => break,
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            result = Err(RuntimeError::Disconnected {
                                who: None,
                                stage: format!("组 {} Seal drain 收件箱断连", state.group),
                            });
                            break;
                        }
                    }
                }
                if result.is_err() {
                    break;
                }
                // 终轮延迟相（S2 落账点唯一化——eager 分域版）：即时相
                /// 已随 Batch 到达/Seal drain 应用；此处重放 F4/F5/F6
                /// 延迟域（早轮批次的延迟项已随各自 handle_continue
                /// 消费，只剩终轮）。
                if let Err(e) = state.apply_deferred() {
                    abort.store(true, Ordering::SeqCst);
                    let _ = up_tx.send(UpMsg::Failed { who, error: e.clone() });
                    result = Err(e);
                    break;
                }
                // Q2：final_hash 用 O(1) 指纹口径（设计稿 §3.2——与
                /// committed_hash 同族合成，**非** state_hash 全名册遍历：
                /// B 口径 20000×5 股×8 域的 O(N) 哈希 ≈2ms/tick 串行屏障
                /// 实测打回）。指纹含落账事实量（undo 长度+延迟载荷余量）
                /// 供未来 Abort 对账观察点扩展。
                let final_hash = (0xcbf2_9ce4_8422_2325u64 ^ tick ^ ((round_final as u64) << 32))
                    .wrapping_mul(0x100000001b3)
                    ^ (state.undo.len() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
                // Q2 批 3+eager 分域：账户稀疏分片随 SealAck 上行——
                /// account_dirty_set 由 push_undo 即时维护（决策扣除+
                /// 全部轮即时域落账的写点；等待窗/Seal drain 消化），Seal
                /// 临界路径免原 O(undo_len) 全日志重扫。集合族与原扫描
                /// 口径逐族一致（8 个账户域族），等价替换。
                let accounts_sparse: Vec<(_, _)> = std::mem::take(&mut state.account_dirty_set)
                    .into_iter()
                    .filter_map(|local| {
                        state
                            .accounts
                            .get(local as usize)
                            .map(|acct| (local, acct.clone()))
                    })
                    .collect();
                if up_tx
                    .send(UpMsg::SealAck {
                        group: state.group,
                        tick,
                        final_hash,
                        accounts_sparse: std::sync::Arc::new(accounts_sparse),
                    })
                    .is_err()
                {
                    abort.store(true, Ordering::SeqCst);
                    result = Err(RuntimeError::SendFailed {
                        detail: format!("组 {who} SealAck 发送失败"),
                    });
                    break;
                }
                state.seal_handle_ns += seal_handle_started.elapsed().as_nanos();
                // eager 分域守卫：记录 SealAck 发送点的日志长度基线。
                // ①日结 Drain 批次的即时记录（Seal 后、DayEnd 前到达）在
                //   日结隐式转正时保留（drain 前段=上一 tick 记录）；
                // ②Start 防御以基线判"Seal 后批次绕锁步"（超基线即证据）。
                state.seal_acked_len = Some(state.undo.len());
                state.batches_after_seal = 0;
            }
            GroupIn::EchoAccountsSparse { wanted } => {
                // P2-4a：生产回写段稀疏上行——只回驱动器 dirty 集指名的
                /// 账户（P2-1b 已把**写回**稀疏化，但全量回声仍是每拍
                /// 全名册 20000×8 堆 Vec 克隆+主线程 drop 的双热点）。
                /// 全量语义由驱动器 dirty 写回承载（等价构造：取回的恰是
                /// 原来要写的同一批值）；全量读面用 EchoAccounts 不变。
                /// 回执集与请求集的精确一致由收集端校验（缺项
                /// =RuntimeError 不静默）。
                let mut accounts: Vec<(LocalIdx, GroupAccount)> =
                    Vec::with_capacity(wanted.len());
                for &local in &wanted {
                    let Some(account) = state.accounts.get(local as usize) else {
                        result = Err(RuntimeError::ProtocolInvariant {
                            detail: format!(
                                "组 {} 稀疏回声请求下标越界：local={local}，名册 {}",
                                state.group,
                                state.accounts.len()
                            ),
                        });
                        break;
                    };
                    accounts.push((local, account.clone()));
                }
                if result.is_err() {
                    abort.store(true, Ordering::SeqCst);
                    break;
                }
                let msg = UpMsg::GroupEchoSparse {
                    group: state.group,
                    accounts,
                };
                if up_tx.send(msg).is_err() {
                    abort.store(true, Ordering::SeqCst);
                    result = Err(RuntimeError::SendFailed {
                        detail: format!("组 {who} GroupEchoSparse 发送失败"),
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
                // Drain 实体批次已先于 Shutdown 入队（FIFO）且即时相已
                /// 落账（eager 分域）：期末名册须含全部回流——延迟相补齐
                /// F4/F5/F6 后上交 Final。
                if let Err(e) = state.apply_deferred() {
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
// ═══ Sol 五审件 3（四审 A1 指定回归）：正常关停 >50ms 空闲窗 × 在途资源 ═══

#[cfg(not(target_family = "wasm"))]
mod v5_shutdown_idle_window_tests {
    use super::*;

    /// 四审 A1 关停竞态的确定性回归：正常关停（Drain→Shutdown→Final
    /// 消息协议）期间 **不置** worker_shutdown 永久标记——空闲 worker
    /// 的 50ms 分片轮询必须持续等待 Shutdown 消息（协议退出），且
    /// Shutdown 处理 apply_staging 把在途回流实体落账。此前标记驱退
    /// 会使空闲 worker 在 >50ms 窗内提前退出上交 Final，漏合入退款。
    /// 构造：真实组线程通道；Batch（CashRefund 500）入 staging 未落账；
    /// 闲置 120ms（跨 50ms 分片轮询片界——竞态窗）；发 Shutdown；
    /// 断言 Final 经协议上交且退款已落账（旧缺陷形态=提前 Final 且
    /// 现金停留在 10_000_000）。
    #[test]
    fn v5_shutdown_over_50ms_idle_window_applies_staged_refunds() {
        let (group_tx, group_rx) = std::sync::mpsc::channel::<GroupIn>();
        let (desk_tx, _desk_rx) = std::sync::mpsc::channel::<crate::runtime::protocol::DeskMsg>();
        let (up_tx, up_rx) = std::sync::mpsc::channel::<UpMsg>();
        let abort = Arc::new(AtomicBool::new(false));
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let capture = Arc::new(std::sync::Mutex::new(
            crate::verification_evidence::current_phase_timing_binding(),
        ));
        let mut state = GroupState::new(
            0,
            vec![GroupAccount::new(10_000_000, 1)],
            8,
            100,
            Arc::new(crate::config::GameConfig::proposed_defaults()),
            240,
            None,
            Vec::new(),
            Box::new(|_t, _r| TickPlan::default()),
        );
        state.cur_tick = 1;
        state.cur_round = 0;
        let handle = {
            let abort = Arc::clone(&abort);
            let shutdown_flag = Arc::clone(&shutdown_flag);
            let capture = Arc::clone(&capture);
            std::thread::spawn(move || {
                group_main(
                    state,
                    vec![desk_tx],
                    group_rx,
                    up_tx,
                    abort,
                    shutdown_flag,
                    crate::runtime::watchdog::WatchdogConfig {
                        normal: std::time::Duration::from_secs(30),
                        long: std::time::Duration::from_secs(120),
                    },
                    capture,
                )
            })
        };
        // 在途资源：退款实体入 staging（未落账——正常关停的典型在途面）。
        group_tx
            .send(GroupIn::Batch(DeskBatch {
                stock: 0,
                tick: 1,
                round: 0,
                entities: vec![Entity::CashRefund {
                    local: 0,
                    amount: 500,
                }],
                fills: Vec::new(),
                receipts: Vec::new(),
            }))
            .expect("Batch 入队");
        // >50ms 空闲窗：worker_recv 分片轮询跨片等待（四审竞态窗——
        /// 旧缺陷形态在此窗内提前退出）。
        std::thread::sleep(std::time::Duration::from_millis(120));
        // 正常关停消息（协议路径——不置永久标记）。
        group_tx
            .send(GroupIn::Shutdown)
            .expect("Shutdown 发送");
        let outcome = handle.join().expect("组线程退出");
        outcome.expect("正常关停协议成功（无失败）");
        // Final 必经协议上交；期末账户含在途回流落账。
        let final_state = match up_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(UpMsg::GroupFinal(f)) => *f,
            other => panic!("期望 GroupFinal，收到 {other:?}"),
        };
        assert_eq!(
            final_state.accounts[0].cash,
            10_000_500,
            "在途退款实体在 Shutdown 处 apply_staging 落账（>50ms 空闲窗不丢回流）"
        );
        assert!(
            final_state
                .observed
                .iter()
                .any(|o| matches!(o, Observed::Shutdown)),
            "Shutdown 消息被协议处理（非标记驱退）"
        );
        // 永久标记全程未置位（正常关停不旁路协议）。
        assert!(
            !shutdown_flag.load(Ordering::SeqCst),
            "正常关停不置 worker_shutdown 永久标记"
        );
        assert!(!abort.load(Ordering::SeqCst), "无 Abort");
    }

    /// World 级正常关停协议回归（与上行循环级测试互补）：真实世界跑一拍
    /// 后正常 shutdown——全部组的期末观察日志必须含 Shutdown（消息协议
    /// 路径；重引"关停前置永久标记"的缺陷形态下，空闲组在标记驱退中
    /// 提前退出、Shutdown 消息不被处理，observed 缺项即显红）。
    #[test]
    fn v5_world_shutdown_reaches_all_groups_via_message_protocol() {
        let spec = crate::runtime::skeleton_tests::basic_spec(
            2,
            1,
            1,
            Box::new(|_g| Box::new(|_t, _r| TickPlan::default())),
        );
        let mut world = crate::runtime::world::World::spawn(spec).expect("世界构造");
        world.run_ticks(1).expect("首拍");
        let report = world.shutdown().expect("正常关停成功");
        assert_eq!(report.group_finals.len(), 2, "两组期末态齐");
        for final_state in &report.group_finals {
            assert!(
                final_state
                    .observed
                    .iter()
                    .any(|o| matches!(o, Observed::Shutdown)),
                "组 {} 的 Shutdown 经消息协议处理（非标记驱退提前退出）",
                final_state.group
            );
        }
        for (who, result) in &report.worker_results {
            assert!(result.is_ok(), "worker {who:?} 正常退出：{result:?}");
        }
    }

}

/// Sol 五审再复核 P1-2（确定性 seam 落地后的 World 级回归）：
/// 组 Shutdown 广播前注入 120ms 停顿（World::shutdown_stall_
/// for_test——四审竞态窗"协调者停顿>50ms"的真实复现）。若重引
/// "关停前置永久标记"缺陷，空闲组在停顿窗内被标记驱退提前退出、
/// Shutdown 消息不被处理 → observed 缺项显红（此前无 seam 时该
/// 缺陷被 32/32 漏检——再复核实证）。
#[cfg(not(target_family = "wasm"))]
#[test]
fn v5_world_shutdown_stall_window_keeps_message_protocol() {
    let spec = crate::runtime::skeleton_tests::basic_spec(
        2,
        1,
        1,
        Box::new(|_g| Box::new(|_t, _r| TickPlan::default())),
    );
    let mut world = crate::runtime::world::World::spawn(spec).expect("世界构造");
    world.run_ticks(1).expect("首拍");
    world.shutdown_stall_for_test = Some(std::time::Duration::from_millis(120));
    let report = world.shutdown().expect("正常关停成功（停顿窗不破坏协议）");
    assert_eq!(report.group_finals.len(), 2, "两组期末态齐");
    for final_state in &report.group_finals {
        assert!(
            final_state
                .observed
                .iter()
                .any(|o| matches!(o, Observed::Shutdown)),
            "组 {} 的 Shutdown 经消息协议处理（停顿窗内不被标记驱退）",
            final_state.group
        );
    }
    for (who, result) in &report.worker_results {
        assert!(result.is_ok(), "worker {who:?} 正常退出：{result:?}");
    }
}

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
            session_seed: 1,
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
        state.apply_batch_immediate(batch(
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
        ), 1, 0);
        state
            .apply_deferred()
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
        state.apply_batch_immediate(
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
            1,
            0,
        );
        // Sol 六审勘正：外来同键单成交=合法形态（意图路由/测试注入）——
        /// 跳过不误挂（Ok 且余量不动），不再显式失败（原 Err 形态实测会
        /// 杀死合法外来单的拍——sol7th_foreign 场景红证）。
        state
            .apply_deferred()
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
        state.apply_batch_immediate(batch(
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
        ), 1, 0);
        state
            .apply_deferred()
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
        state.apply_batch_immediate(batch(
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
        ), 1, 0);
        state
            .apply_deferred()
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
            .apply_batch_immediate(
                batch(
                    vec![Entity::Closed {
                        local: 0,
                        order_serial: 88,
                    }],
                    Vec::new(),
                ),
                1,
                0,
            )
            .expect("他单终结批次即时相落账");
        state
            .apply_deferred()
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
            .apply_batch_immediate(
                batch(
                    vec![Entity::Closed {
                        local: 0,
                        order_serial: 5,
                    }],
                    Vec::new(),
                ),
                1,
                0,
            )
            .expect("本单终结批次即时相落账");
        state
            .apply_deferred()
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
            .apply_batch_immediate(
                batch(
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
                ),
                1,
                0,
            )
            .expect("即时相落账");
        state
            .apply_deferred()
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
        state.apply_batch_immediate(DeskBatch {
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
        }, 1, 0);
        state
            .apply_deferred()
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
                session_seed: 1,
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
            daily_active_turnover_cents: 0,
            daily_active_trade_count: 0,
            last_price: last,
            best_bid: None,
            best_ask: None,
            max_buy_price: last * 11 / 10,
            daily_upper_limit: last * 11 / 10,
            daily_lower_limit: last * 9 / 10,
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
                public_cutoff_serial: 0,
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

// ═══ eager-apply undo 分域合同（第 20 任批 1——红先行）═══
//
// 设计稿：agents/view-campaign/plans/eager-undo-domains-design.md §3。
// 分域：F1（Cash/Shares/Frozen）/F2（CostBasis/PendingSell/BuyPurseHeld）/
// F3（ForeignActive）delta 逆减族=可交换——Batch 到达（等待窗）即应用；
// F4（EverHeld）/F5（Personal/PlanBook）/F6（ChildInFlight）序敏感族
// 延迟到 Continue/Seal 消息点重放（首版 eager 撤回的交错教训根因）。
// 红证形态：批 1 提交含现状语义桩（apply_batch_immediate=staging.push）
// ——合同 2 以"应用后状态必须变化"红；合同 3 是数学性质守卫（现状即绿，
// 防未来把序敏感族混进即时域时数学基础被无声破坏）。批 2 实现后合同 2
// 转绿；突变旋钮红证（延迟项即时化→sol4 红/即时相漏 undo push→合同 2
// 红）随批 2 实证归档 evidence。
#[cfg(all(test, not(target_arch = "wasm32")))]
mod eager_domain_tests {
    use super::*;

    /// 分域合同标准夹具：2 账户 × 3 股、空决策脚本；预置在途/成本基/
    /// 曾持有素材（即时域各族的合法应用前提——如 foreign_active>0 才有
    /// Closed 的 F3 减、pending_sell>0 才有卖腿 fill 的 F2 减）。
    fn domain_harness() -> GroupState {
        let mut a0 = GroupAccount::new(10_000_000, 3);
        a0.shares[0] = 500;
        a0.pending_sell_qty[0] = 200;
        a0.foreign_active[0] = 1;
        let mut a1 = GroupAccount::new(5_000_000, 3);
        a1.shares[2] = 80;
        a1.buy_purse_held_cents = 60_000;
        a1.ever_held[0] = true;
        GroupState::new(
            0,
            vec![a0, a1],
            8,
            100,
            Arc::new(crate::config::GameConfig::proposed_defaults()),
            240,
            None,
            Vec::new(),
            Box::new(|_t, _r| TickPlan::default()),
        )
    }

    /// 卖腿成交事实（owner=组 0 位 0；经历转移/成本基的 Sell 路径素材）。
    fn sell_fill(qty: u32) -> FillFact {
        FillFact {
            order: (0, 7),
            owner: (0, 0),
            side: Side::Sell,
            price: 1000,
            qty,
            commission: 5,
            stamp_tax: 1,
            transfer_fee: 1,
        }
    }

    /// 合同 2：等待窗即时相部分应用后 Abort——delta 可交换性保证全冲回
    /// 恒等拍前态（端到端验证分域数学基础；批 1 桩形态=红：等待窗不应用
    /// ⇒ 状态无变化 ⇒ 第一断言红）。两个到达序（b1→b2 与 b2→b1）都必须
    /// 精确恢复——任意批到达序与决策段记录交错无害的判别面。
    #[test]
    fn eager_partial_apply_abort_rolls_back() {
        // 批 1：F1（CashRefund 现金+占用/BoughtShares 持仓+冻结）+
        // F2（RefundedShares 卖余量+持仓、sell fill 成本基/卖余量）+
        // F3（Closed 在途减）+F4/F5/F6 延迟载荷（RefundedShares 曾持有
        /// 复位/拒收释放——延迟相重放，Abort 不得触碰）。
        let b1 = DeskBatch {
            stock: 0,
            tick: 1,
            round: 0,
            entities: vec![
                Entity::CashRefund {
                    local: 1,
                    amount: 7_000,
                },
                Entity::BoughtShares { local: 0, qty: 100 },
                Entity::RefundedShares { local: 1, qty: 30 },
                Entity::Closed {
                    local: 0,
                    order_serial: 1,
                },
            ],
            fills: vec![sell_fill(50)],
            receipts: vec![super::super::DeskReceipt {
                order: (0, 9),
                owner: (0, 0),
                kind: super::super::ReceiptKind::PriceOutOfBand,
            }],
        };
        // 批 2：买腿 fill（owner=组 0 位 1——BuyPurseHeld F2 减实付）+
        /// CashProceeds（F1 加）。
        let b2 = DeskBatch {
            stock: 1,
            tick: 1,
            round: 0,
            entities: vec![Entity::CashProceeds {
                local: 0,
                amount: 99_400,
            }],
            fills: vec![FillFact {
                order: (1, 3),
                owner: (0, 1),
                side: Side::Buy,
                price: 1000,
                qty: 30,
                commission: 0,
                stamp_tax: 0,
                transfer_fee: 0,
            }],
            receipts: Vec::new(),
        };
        for order in [0u8, 1] {
            let mut state = domain_harness();
            state.cur_tick = 1;
            state.cur_round = 0;
            let hash_before = state.state_hash();
            let (first, second) = if order == 0 {
                (b1.clone(), b2.clone())
            } else {
                (b2.clone(), b1.clone())
            };
            state
                .apply_batch_immediate(first, 1, 0)
                .expect("即时相批 1 防御通过");
            state
                .apply_batch_immediate(second, 1, 0)
                .expect("即时相批 2 防御通过");
            assert_ne!(
                state.state_hash(),
                hash_before,
                "等待窗即时相必须已落账（到达序 {order}）——分域批 1 红证锚"
            );
            assert!(
                !state.staging_deferred.is_empty(),
                "延迟载荷（F4/F5/F6）须暂存 staging_deferred（到达序 {order}）"
            );
            // Abort：逆序全回放——部分应用的即时域精确冲回拍前态。
            state.rollback();
            assert_eq!(
                state.state_hash(),
                hash_before,
                "Abort 全冲回=拍前态（delta 可交换性，到达序 {order}）"
            );
            assert!(
                state.staging_deferred.is_empty(),
                "Abort 清空延迟载荷（到达序 {order}）"
            );
            // state_hash 不覆盖的成本基/曾持有位：字段级补充断言。
            assert_eq!(state.accounts[0].invested_cents, vec![0; 3]);
            assert_eq!(state.accounts[0].recovered_cents, vec![0; 3]);
            assert!(
                state.accounts[1].ever_held[0],
                "RefundedShares 的曾持有复位属延迟域——即时相/Abort 均不得触碰"
            );
        }
    }

    /// 合同 3（数学性质守卫）：F1/F2/F3 记录集按两个不同交错序"应用+记录"，
    /// rollback（逆序回放）都必须恢复初始态——delta 逆减族对全应用封闭，
    /// 日志序任意交错无害。这是"批次 delta 与决策段 delta 交错"的分域
    /// 基础（设计稿 §1）；未来若把序敏感族混进即时域，此守卫先红。
    #[test]
    fn delta_undo_shuffled_replay_equivalence() {
        /// 与 rollback 逆算子对称的正向应用（测试专用——只覆盖即时域三族）。
        fn apply_forward(state: &mut GroupState, entry: &GroupUndo) {
            match entry {
                GroupUndo::Cash { local, delta } => {
                    state.accounts[*local as usize].cash += delta;
                }
                GroupUndo::Shares { local, stock, delta } => {
                    let slot = &mut state.accounts[*local as usize].shares[*stock as usize];
                    *slot = (*slot as i64 + *delta as i64) as u32;
                }
                GroupUndo::ForeignActive { local, stock, delta } => {
                    let slot = &mut state.accounts[*local as usize].foreign_active[*stock as usize];
                    *slot = (*slot as i32 + *delta as i32) as u16;
                }
                GroupUndo::Frozen { local, stock, delta } => {
                    let slot = &mut state.accounts[*local as usize].frozen_today[*stock as usize];
                    *slot = (*slot as i64 + *delta as i64) as u32;
                }
                GroupUndo::CostBasis {
                    local,
                    stock,
                    d_invested,
                    d_recovered,
                } => {
                    let acct = &mut state.accounts[*local as usize];
                    acct.invested_cents[*stock as usize] += d_invested;
                    acct.recovered_cents[*stock as usize] += d_recovered;
                }
                GroupUndo::PendingSell { local, stock, delta } => {
                    let slot = &mut state.accounts[*local as usize].pending_sell_qty[*stock as usize];
                    *slot = (*slot as i64 + delta) as u32;
                }
                GroupUndo::BuyPurseHeld { local, delta } => {
                    state.accounts[*local as usize].buy_purse_held_cents += delta;
                }
                other => panic!("合同 3 只覆盖即时域三族（F1/F2/F3），收到 {other:?}"),
            }
        }
        // 记录集：全族覆盖+同键多条（同账户同股的重复 delta 是交错判别面）。
        // 取值保证任意应用前缀不触底（pending/占用初始垫高）。
        let entries: Vec<GroupUndo> = vec![
            GroupUndo::Cash { local: 0, delta: -50_000 },
            GroupUndo::Cash { local: 1, delta: 7_000 },
            GroupUndo::Shares { local: 0, stock: 0, delta: 100 },
            GroupUndo::Shares { local: 0, stock: 0, delta: -40 },
            GroupUndo::Shares { local: 1, stock: 2, delta: 30 },
            GroupUndo::ForeignActive { local: 0, stock: 0, delta: 1 },
            GroupUndo::ForeignActive { local: 0, stock: 0, delta: -1 },
            GroupUndo::Frozen { local: 1, stock: 1, delta: 55 },
            GroupUndo::CostBasis { local: 0, stock: 0, d_invested: 100_000, d_recovered: 0 },
            GroupUndo::CostBasis { local: 0, stock: 0, d_invested: 0, d_recovered: 60_000 },
            GroupUndo::PendingSell { local: 0, stock: 0, delta: 100 },
            GroupUndo::PendingSell { local: 0, stock: 0, delta: -40 },
            GroupUndo::BuyPurseHeld { local: 1, delta: -7_000 },
            GroupUndo::BuyPurseHeld { local: 0, delta: 50_000 },
        ];
        // 固定交错置换（决策段/批次记录交错的形态模拟——非逆序非原序）。
        let interleave: [usize; 14] = [7, 0, 13, 3, 8, 1, 9, 4, 10, 2, 11, 5, 12, 6];
        for (label, permutation) in [("原序", None), ("交错序", Some(&interleave[..]))] {
            let mut state = domain_harness();
            state.cur_tick = 1;
            let hash_before = state.state_hash();
            let sequence: Vec<usize> = match permutation {
                None => (0..entries.len()).collect(),
                Some(perm) => perm.to_vec(),
            };
            for i in sequence {
                let entry = entries[i].clone();
                apply_forward(&mut state, &entry);
                state.undo.push(entry);
            }
            assert_ne!(
                state.state_hash(),
                hash_before,
                "夹具须产生状态变更（{label}）"
            );
            state.rollback();
            assert_eq!(
                state.state_hash(),
                hash_before,
                "{label}日志逆序回放=初始态（delta 族可交换性守卫）"
            );
            // 成本基（state_hash 不覆盖域）字段级补充。
            assert_eq!(state.accounts[0].invested_cents, vec![0; 3], "{label} invested 冲回");
            assert_eq!(state.accounts[0].recovered_cents, vec![0; 3], "{label} recovered 冲回");
        }
    }
}

// ═══ 组端 Seal Failed 路径注入（登记项收口——seal_tests 承载的组内
// 直连形态，私有字段可见性归本模块）═══
#[cfg(all(test, not(target_arch = "wasm32")))]
mod seal_failed_injection_tests {
    use super::*;
// ═══ 组端 Seal Failed 路径注入（第 20 任登记项收口）═══
//
// 协调者侧 SealAck 收集失败已有合同（seal_ack_failure_aborts_then_
// retry_survives）；组端 Seal 相位自身的两条显式失败防御此前无注入
// 测试：①Seal tick/round 错位（5183 形态防御）；②Seal drain 期收到
// 非数据面消息（协议错位——Batch/AcceptedBatch 之外的消息属 Seal
// 相位不可达形态，显式失败不静默）。组线程直连形态（v5 同款真实
// 通道）承载。

/// 防御 ①：Seal tick 错位——组在（tick=1, round=0）收到 Seal{tick=2}。
#[test]
fn seal_mismatched_tick_fails_explicitly() {
    
    use super::GroupIn;
        use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    let (group_tx, group_rx) = std::sync::mpsc::channel::<GroupIn>();
    let (_desk_tx, _desk_rx) = std::sync::mpsc::channel::<crate::runtime::protocol::DeskMsg>();
    let (up_tx, up_rx) = std::sync::mpsc::channel::<UpMsg>();
    let abort = Arc::new(AtomicBool::new(false));
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let capture = Arc::new(Mutex::new(
        crate::verification_evidence::current_phase_timing_binding(),
    ));
    let mut state = GroupState::new(
        0,
        vec![GroupAccount::new(1_000_000, 1)],
        8,
        100,
        Arc::new(crate::config::GameConfig::proposed_defaults()),
        240,
        None,
        Vec::new(),
        Box::new(|_t, _r| TickPlan::default()),
    );
    state.cur_tick = 1;
    state.cur_round = 0;
    let handle = {
        let (abort, shutdown_flag, capture) = (Arc::clone(&abort), Arc::clone(&shutdown_flag), Arc::clone(&capture));
        std::thread::spawn(move || {
            group_main(
                state,
                vec![_desk_tx],
                group_rx,
                up_tx,
                abort,
                shutdown_flag,
                crate::runtime::watchdog::WatchdogConfig {
                    normal: std::time::Duration::from_secs(30),
                    long: std::time::Duration::from_secs(120),
                },
                capture,
            )
        })
    };
    // 错位 Seal（期望 (1,0)，发 (2,0)）——显式失败（ProtocolInvariant）。
    group_tx
        .send(GroupIn::Seal {
            tick: 2,
            round_final: 0,
        })
        .expect("错位 Seal 发送");
    let outcome = handle.join().expect("组线程退出");
    let err = outcome.expect_err("Seal 错位必须显式失败");
    assert!(
        matches!(err, RuntimeError::ProtocolInvariant { .. }),
        "错误面可辨识（ProtocolInvariant）：{err:?}"
    );
    // 线程退出后 Failed 上行到达（break 前已发送；单消费者按序收）。
    let mut saw_failed = false;
    while let Ok(msg) = up_rx.try_recv() {
        if matches!(msg, UpMsg::Failed { .. }) {
            saw_failed = true;
        }
    }
    assert!(saw_failed, "组端退出前 Failed 上行（不静默）");
    assert!(abort.load(Ordering::SeqCst), "abort 标记置位（通知对端）");
}

/// 防御 ②：Seal drain 期非数据面消息（Abort 属协议错位形态——Seal
/// 相位不可达；显式失败不静默吞）。
#[test]
fn seal_drain_foreign_message_fails_explicitly() {
    
    use super::GroupIn;
        use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    let (group_tx, group_rx) = std::sync::mpsc::channel::<GroupIn>();
    let (_desk_tx, _desk_rx) = std::sync::mpsc::channel::<crate::runtime::protocol::DeskMsg>();
    let (up_tx, up_rx) = std::sync::mpsc::channel::<UpMsg>();
    let abort = Arc::new(AtomicBool::new(false));
    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let capture = Arc::new(Mutex::new(
        crate::verification_evidence::current_phase_timing_binding(),
    ));
    let mut state = GroupState::new(
        0,
        vec![GroupAccount::new(1_000_000, 1)],
        8,
        100,
        Arc::new(crate::config::GameConfig::proposed_defaults()),
        240,
        None,
        Vec::new(),
        Box::new(|_t, _r| TickPlan::default()),
    );
    state.cur_tick = 1;
    state.cur_round = 0;
    let handle = {
        let (abort, shutdown_flag, capture) = (Arc::clone(&abort), Arc::clone(&shutdown_flag), Arc::clone(&capture));
        std::thread::spawn(move || {
            group_main(
                state,
                vec![_desk_tx],
                group_rx,
                up_tx,
                abort,
                shutdown_flag,
                crate::runtime::watchdog::WatchdogConfig {
                    normal: std::time::Duration::from_secs(30),
                    long: std::time::Duration::from_secs(120),
                },
                capture,
            )
        })
    };
    // 先入队一条非数据面消息（Abort——drain 期将被消费到），再发 Seal：
    // drain 循环收件箱顺序消费——Abort 先于 Seal 处理到达。
    group_tx
        .send(GroupIn::Abort { tick: 1 })
        .expect("协议错位消息入队");
    group_tx
        .send(GroupIn::Seal {
            tick: 1,
            round_final: 0,
        })
        .expect("Seal 发送");
    let outcome = handle.join().expect("组线程退出");
    let err = outcome.expect_err("Seal drain 期非数据面消息必须显式失败");
    assert!(
        matches!(err, RuntimeError::ProtocolInvariant { .. }),
        "错误面可辨识（协议错位）：{err:?}"
    );
    let mut saw_failed = false;
    while let Ok(msg) = up_rx.try_recv() {
        if matches!(msg, UpMsg::Failed { .. }) {
            saw_failed = true;
        }
    }
    assert!(saw_failed, "组端退出前 Failed 上行（不静默）");
    assert!(abort.load(Ordering::SeqCst), "abort 标记置位");
}

}
