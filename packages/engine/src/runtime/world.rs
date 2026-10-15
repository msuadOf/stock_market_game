//! 世界拓扑（批 2 骨架，非 wasm）：G 个组线程 + N 个交易部线程 + 协调者
//! （协调者逻辑在调用线程上运行，不单独建线程）。
//!
//! 通道拓扑（F §4.2，全程 std::sync::mpsc）：
//! - 组↔交易部全互联直发（G×N 下行 Ops + N×G 上行实体批次）——数据面不经协调者；
//! - 每组一条协调者下行（Start/Continue/Abort/Shutdown）；
//! - 每交易部一条协调者下行（Abort/Drain/Shutdown）；
//! - 单一上行收束 channel（全部 worker 共用）。
//!
//! 代码本体零线程数管理概念：线程数 = G+N(+调用线程)，由世界构造参数决定。

use super::coordinator::{
    broadcast_continue, broadcast_start, converge_round, no_op_validator, GlobalState,
    RoundOutcome, TickEvidence, Validator,
};
use super::decision::DecisionSetup;
use super::desk::{desk_main, DeskFinalState, DeskSetup};
use super::group::{group_main, GroupAccount, GroupFinalState, GroupScript};
use super::plans::PlanChain;
use super::protocol::StockPublic;
use super::protocol::{AdminRight, DayEndDirective};
use super::protocol::{DeskMsg, GroupIn, RuntimeError, UpMsg, WorkerId};
use super::watchdog::{recv_deadline, Phase, WatchdogConfig};
use super::DeskReceipt;
use super::{GroupIdx, StockIdx};
use crate::config::GameConfig;
use crate::money::Money;
use crate::StockExchange;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// 每股交易部规格（类别差异显式建模：主板/创业板）。
#[derive(Clone, Debug)]
pub struct DeskStockSpec {
    /// 上市交易所（集合竞价选价规则差异）。
    pub exchange: StockExchange,
    /// 初始价（分）。
    pub initial_price: i64,
    /// 涨跌幅（基点：主板 1000=10% / 创业板 2000=20%）。
    pub limit_bps: u32,
    /// 限价单笔上限（主板 1_000_000 / 创业板 300_000）。
    pub max_order_qty_limit: u32,
    /// 市价单笔上限（主板 1_000_000 / 创业板 150_000）。
    pub max_order_qty_market: u32,
}

impl DeskStockSpec {
    /// 主板缺省（10%、100 万股上限）。
    pub fn main_board(initial_price: i64) -> Self {
        DeskStockSpec {
            exchange: StockExchange::Shanghai,
            initial_price,
            limit_bps: 1000,
            max_order_qty_limit: 1_000_000,
            max_order_qty_market: 1_000_000,
        }
    }
}

/// 世界构造参数。
pub struct WorldSpec {
    /// 组数 G。
    pub n_groups: usize,
    /// 每组账户数。
    pub accounts_per_group: usize,
    /// 每账户初始现金（i64 分）。
    pub initial_cash: i64,
    /// 初始持仓 `[group][local][stock]`（缺省零）。
    pub initial_shares: Vec<Vec<Vec<u32>>>,
    /// 组决策脚本工厂。
    pub script_for: Box<dyn Fn(GroupIdx) -> GroupScript>,
    /// 组端分段 flush 粒度（op 数）。
    pub chunk: usize,
    /// 看门狗阈值。
    pub watchdog: WatchdogConfig,
    /// 同 tick 续行轮数上限（异常形状显式登记）。
    pub rounds_cap: u32,
    /// 每股交易部规格（下标=StockIdx）。
    pub stocks: Vec<DeskStockSpec>,
    /// 费用/整手/价格笼子配置（引擎 GameConfig 同口径）。
    pub config: Arc<GameConfig>,
    /// 每交易日 tick 数。
    pub ticks_per_day: u64,
    /// 开盘集合竞价窗口 tick 数（0=全天连续，测试形态）。
    pub auction_ticks: u64,
    /// 收盘集合竞价窗口 tick 数。
    pub closing_auction_ticks: u64,
    /// 挂单 TTL（tick）。
    pub order_ttl_ticks: u64,
    /// tick 收盘价历史容量。
    pub history_len: usize,
    /// D0：世界起始已转正 tick（恢复/接管对齐——组/部/全局域从此起步，
    /// 相位按绝对 tick 推导；0=全新世界）。
    pub initial_committed_tick: u64,
    /// 内联决策装配（Some=decide_data 路径；None=测试脚本路径）。
    pub decision: Option<DecisionSetup>,
    /// 公司域权益产出（日结 AdminBatch 注入；None=无权益）。
    pub rights_computer: Option<Box<dyn Fn(u32, GroupIdx) -> Vec<AdminRight> + Send>>,
    /// 按账户现金重建（存档候选往返；None=用 initial_cash 均一初始化）。
    pub initial_cash_override: Option<Vec<Vec<i64>>>,
    /// 计划链工厂（批 6：跨股多步计划经 Continue 轮排空）。
    pub plan_factory: Option<Box<dyn Fn(GroupIdx) -> Vec<Box<dyn PlanChain>> + Send>>,
}

/// 日结候选投影（F §6.2 步骤 4：组投影 + 交易部清簿态 + 全局域 + 费用上交；
/// 全部可失败校验成功后才提交日结——候选即"不可变存档输入"，宿主写盘失败
/// ≠ 市场回滚）。
#[derive(Clone, Debug)]
pub struct DayEndCandidate {
    /// 交易日序号。
    pub day: u32,
    /// 日终 tick。
    pub tick: u64,
    /// 各组权益落账后账户投影。
    pub groups: Vec<(GroupIdx, Vec<GroupAccount>)>,
    /// 各组落账后态哈希。
    pub group_hashes: Vec<(GroupIdx, u64)>,
    /// 各交易部日终态（fee_sink 上交、昨收基准）。
    pub desks: Vec<(StockIdx, i64, i64)>,
    /// 全局域快照。
    pub global: GlobalState,
}

/// 日结候选校验器（注入点：对账/守恒/哈希等全部可失败校验）。
pub type DayEndValidator = Box<dyn FnMut(&DayEndCandidate) -> Result<(), String> + Send>;

/// 每个已收敛 tick 的记录（测试与审计面）。
#[derive(Clone, Debug)]
pub struct TickRecord {
    /// tick 编号。
    pub tick: u64,
    /// 实际轮数。
    pub rounds: u32,
    /// 各组提交态哈希。
    pub group_hashes: Vec<(GroupIdx, u64)>,
    /// 各交易部提交态哈希。
    pub desk_hashes: Vec<(StockIdx, u64)>,
    /// 各交易部本收束点挂单数（观察面）。
    pub desk_resting: Vec<(StockIdx, u64)>,
    /// 本 tick 双腿成交事实（事件映射输入；驱动器/测试面）。
    pub desk_trades: Vec<super::protocol::DeskTradeFact>,
    /// 本 tick 生命周期事实（受理/撤单/到期；事件映射输入）。
    pub desk_lifecycle: Vec<super::protocol::DeskLifecycleFact>,
    /// 本轮回执（拒收/不可撤；IntentRejected 映射输入）。
    pub desk_receipts: Vec<DeskReceipt>,
    /// 本轮竞价事实（AuctionTick/Completed 输入）。
    pub desk_auction: Vec<super::protocol::DeskAuctionFact>,
}

/// 关停报告（期末对账面）。
#[derive(Clone, Debug)]
pub struct ShutdownReport {
    /// 组期末状态（下标=组）。
    pub group_finals: Vec<GroupFinalState>,
    /// 交易部期末状态（下标=股）。
    pub desk_finals: Vec<DeskFinalState>,
    /// worker 线程返回值（Err=线程主循环显式失败，已通过 Failed 上抛过）。
    pub worker_results: Vec<(WorkerId, Result<(), RuntimeError>)>,
}

/// 世界（协调者视角持有全部通道与线程句柄）。
pub struct World {
    /// 全局域权威（已收敛态）。
    pub global: GlobalState,
    /// 已收敛 tick 记录。
    pub report_log: Vec<TickRecord>,
    group_txs: Vec<Sender<GroupIn>>,
    desk_txs: Vec<Sender<DeskMsg>>,
    up_rx: Receiver<UpMsg>,
    handles: Vec<(WorkerId, std::thread::JoinHandle<Result<(), RuntimeError>>)>,
    abort: Arc<AtomicBool>,
    watchdog: WatchdogConfig,
    validator: Validator,
    rounds_cap: u32,
    n_groups: usize,
    n_stocks: usize,
    /// 已死亡 worker（Failed 路径收齐的 Final；world 不可再跑 tick）。
    dead_finals: Option<ShutdownReport>,
    dead_reason: Option<String>,
    /// 全股上一收束点公共态（决策观察基；DeskReport 安装更新）。
    public_views: Vec<StockPublic>,
    /// 公司域权益产出（日结注入）。
    rights_computer: Option<Box<dyn Fn(u32, GroupIdx) -> Vec<AdminRight> + Send>>,
    /// 每交易日 tick 数（日界判定）。
    ticks_per_day: u64,
    /// 日结候选校验器（F §6.2 步骤 4：候选组装与全部可失败校验成功后才提交）。
    day_end_validator: DayEndValidator,
    /// 最近一次已成功日结的收尾 tick（D0：手动/自动日结显式互斥——
    /// 同一交易日只日结一次；abort_day_end 回滚时清空）。
    last_day_end_tick: Option<u64>,
    /// D4 相位计时捕获槽：驱动器在 world 运行期挂当前捕获绑定，组/部
    /// worker 每 tick 读取并在线程本地安装（无捕获时恒 None——零开销）。
    capture_slot: std::sync::Arc<std::sync::Mutex<crate::verification_evidence::PhaseTimingBinding>>,
    /// 最近一次成功日结的不可变候选（宿主写盘失败 ≠ 市场回滚：显式报错保留）。
    pub last_day_end_candidate: Option<DayEndCandidate>,
    /// 日终清簿撤单事实（驱动器事件拼装读取）。
    pub drained_lifecycle: Vec<super::protocol::DeskLifecycleFact>,
}

impl World {
    /// 构造世界并启动全部 worker 线程。
    pub fn spawn(spec: WorldSpec) -> Result<Self, RuntimeError> {
        if spec.n_groups == 0 || spec.n_groups > GroupIdx::MAX as usize {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("组数需 1~{}，收到 {}", GroupIdx::MAX, spec.n_groups),
            });
        }
        let n_stocks = spec.stocks.len();
        if n_stocks == 0 || n_stocks > StockIdx::MAX as usize {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("股数需 1~{}，收到 {n_stocks}", StockIdx::MAX),
            });
        }
        if spec.ticks_per_day == 0 || spec.closing_auction_ticks >= spec.ticks_per_day {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "日 tick 配置非法：ticks_per_day={}, closing={}",
                    spec.ticks_per_day, spec.closing_auction_ticks
                ),
            });
        }
        if spec.chunk == 0 {
            return Err(RuntimeError::ProtocolInvariant {
                detail: "chunk 需为正（分段 flush 粒度）".to_owned(),
            });
        }
        let WorldSpec {
            n_groups,
            accounts_per_group,
            initial_cash,
            initial_shares,
            script_for,
            chunk,
            watchdog,
            rounds_cap,
            stocks,
            config,
            ticks_per_day,
            auction_ticks,
            closing_auction_ticks,
            order_ttl_ticks,
            history_len,
            initial_committed_tick,
            decision,
            rights_computer,
            initial_cash_override,
            plan_factory,
        } = spec;
        let n_stocks = stocks.len();
        let abort = Arc::new(AtomicBool::new(false));
        let capture_slot: std::sync::Arc<
            std::sync::Mutex<crate::verification_evidence::PhaseTimingBinding>,
        > = std::sync::Arc::new(std::sync::Mutex::new(
            crate::verification_evidence::PhaseTimingBinding::empty(),
        ));
        let (up_tx, up_rx) = channel::<UpMsg>();
        let mut group_rxs = Vec::with_capacity(n_groups);
        let mut group_txs = Vec::with_capacity(n_groups);
        for _ in 0..n_groups {
            let (tx, rx) = channel::<GroupIn>();
            group_txs.push(tx);
            group_rxs.push(rx);
        }
        let mut desk_rxs = Vec::with_capacity(n_stocks);
        let mut desk_txs = Vec::with_capacity(n_stocks);
        for _ in 0..n_stocks {
            let (tx, rx) = channel::<DeskMsg>();
            desk_txs.push(tx);
            desk_rxs.push(rx);
        }
        let mut handles = Vec::with_capacity(n_groups + n_stocks);
        for g in 0..n_groups as GroupIdx {
            let mut accounts = Vec::with_capacity(accounts_per_group);
            for l in 0..accounts_per_group {
                let cash = initial_cash_override
                    .as_ref()
                    .and_then(|by_group| by_group.get(g as usize))
                    .and_then(|by_local| by_local.get(l))
                    .copied()
                    .unwrap_or(initial_cash);
                let mut acct = GroupAccount::new(cash, n_stocks);
                if let Some(shares) = initial_shares
                    .get(g as usize)
                    .and_then(|by_group| by_group.get(l))
                {
                    acct.shares.copy_from_slice(shares);
                }
                accounts.push(acct);
            }
            let group_decision = decision.clone().map(|mut d| {
                // 组因子混入种子：防跨组同种子同输出。
                for (i, p) in d.profiles.iter_mut().enumerate() {
                    p.seed ^= (g as u64)
                        .wrapping_mul(0x9E3779B97F4A7C15)
                        .wrapping_add(i as u64);
                }
                (d.profiles, d.attention_gap_ticks)
            });
            let group_plans = plan_factory.as_ref().map(|f| f(g)).unwrap_or_default();
            let state = super::group::GroupState::new_with_initial_tick(
                g,
                accounts,
                chunk,
                config.lot_size,
                Arc::clone(&config),
                ticks_per_day,
                initial_committed_tick,
                group_decision,
                group_plans,
                script_for(g),
            );
            let rx = group_rxs.remove(0);
            let handle = std::thread::Builder::new()
                .name(format!("rt-group-{g}"))
                .spawn({
                    let desk_txs = desk_txs.clone();
                    let up_tx = up_tx.clone();
                    let abort = Arc::clone(&abort);
                    let watchdog = watchdog;
                    let capture = std::sync::Arc::clone(&capture_slot);
                    move || group_main(state, desk_txs, rx, up_tx, abort, watchdog, capture)
                })
                .map_err(|e| RuntimeError::SendFailed {
                    detail: format!("spawn rt-group-{g} 失败：{e}"),
                })?;
            handles.push((WorkerId::Group(g), handle));
        }
        let stocks_specs: Vec<DeskStockSpec> = stocks.clone();
        let cage_on = config.price_cage_enabled;
        for (s, stock_spec) in stocks.into_iter().enumerate() {
            let s = s as StockIdx;
            let rx = desk_rxs.remove(0);
            let setup = DeskSetup {
                stock: s,
                exchange: stock_spec.exchange,
                initial_price: stock_spec.initial_price,
                limit_bps: stock_spec.limit_bps,
                max_order_qty_limit: stock_spec.max_order_qty_limit,
                max_order_qty_market: stock_spec.max_order_qty_market,
                ticks_per_day,
                auction_ticks,
                closing_auction_ticks,
                order_ttl_ticks,
                history_len,
                initial_tick: initial_committed_tick,
            };
            let handle = std::thread::Builder::new()
                .name(format!("rt-desk-{s}"))
                .spawn({
                    let group_txs = group_txs.clone();
                    let up_tx = up_tx.clone();
                    let abort = Arc::clone(&abort);
                    let watchdog = watchdog;
                    let config = Arc::clone(&config);
                    let capture = std::sync::Arc::clone(&capture_slot);
                    move || {
                        desk_main(
                            setup, n_groups, config, rx, group_txs, up_tx, abort, watchdog,
                            capture,
                        )
                    }
                })
                .map_err(|e| RuntimeError::SendFailed {
                    detail: format!("spawn rt-desk-{s} 失败：{e}"),
                })?;
            handles.push((WorkerId::Desk(s), handle));
        }
        // 世界不持上行发送端：全部 worker 退出后 recv 才会断连（显式失败面）。
        drop(up_tx);
        Ok(World {
            global: GlobalState {
                committed_tick: initial_committed_tick,
                day: (initial_committed_tick / ticks_per_day) as u32,
                events_installed: 0,
            },
            report_log: Vec::new(),
            group_txs,
            desk_txs,
            up_rx,
            handles,
            abort,
            watchdog,
            validator: no_op_validator(),
            rounds_cap,
            n_groups,
            n_stocks,
            dead_finals: None,
            dead_reason: None,
            public_views: Self::initial_views(&stocks_specs, cage_on),
            rights_computer,
            ticks_per_day,
            day_end_validator: Box::new(|_| Ok(())),
            last_day_end_tick: None,
            capture_slot,
            last_day_end_candidate: None,
            drained_lifecycle: Vec::new(),
        })
    }

    /// 注入 PreCommit 校验器（测试/批 3+ 接线点）。
    pub fn set_validator(&mut self, v: Validator) {
        self.validator = v;
    }

    /// 初始公共态（tick1 决策观察基；涨跌停按构造规格推导，无盘口）。
    fn initial_views(specs: &[DeskStockSpec], cage_on: bool) -> Vec<StockPublic> {
        let _ = cage_on;
        specs
            .iter()
            .map(|s| {
                let last = s.initial_price.max(1);
                let up =
                    super::desk::bound_price(Money::from_cents(last), 10_000 + s.limit_bps, true)
                        .unwrap_or(Money::from_cents(last));
                let down =
                    super::desk::bound_price(Money::from_cents(last), 10_000 - s.limit_bps, false)
                        .unwrap_or(Money::from_cents(1));
                StockPublic {
                    last_price: last,
                    best_bid: None,
                    best_ask: None,
                    depth_bids: Vec::new(),
                    depth_asks: Vec::new(),
                    max_buy_price: up.cents(),
                    daily_upper_limit: up.cents(),
                    min_sell_price: down.cents(),
                    recent_prices: vec![last],
                    minute_closes: Vec::new(),
                    volume_today: 0,
                    relative_volume: 1.0,
                    order_book_imbalance: 0.0,
                }
            })
            .collect()
    }

    /// K 线投影收集（驱动器 candle_book 回写）。Sol 二审修复集 3：发送
    /// 失败与异常消息显式传播（此前 `let _ = send` + `_ => break` 吞错
    /// 并返回部分数据——部分 K 安装=静默语义丢失）。
    pub fn take_candle_projection(
        &mut self,
    ) -> Result<
        Vec<(
            super::StockIdx,
            Vec<(i64, i64, i64, i64, i64, u64, i64, u64)>,
            Option<(i64, i64, i64, i64, i64, u64, i64, u64)>,
        )>,
        RuntimeError,
    > {
        // Sol 三审阻断 2：通道面一致性守卫——sever/死亡后 txs 清空而
        // 期望响应数不变，收集会空等 watchdog（30s）才失败。断连形态
        // 立即显式报错（防御式：不静默等待）。
        if self.desk_txs.len() != self.n_stocks {
            return Err(RuntimeError::Disconnected {
                who: None,
                stage: format!(
                    "take_candle_projection 通道面与部数不一致（txs={} n_stocks={}——世界已断连）",
                    self.desk_txs.len(),
                    self.n_stocks
                ),
            });
        }
        for tx in &self.desk_txs {
            tx.send(DeskMsg::EchoCandles).map_err(|_| RuntimeError::SendFailed {
                detail: "EchoCandles 广播失败".to_owned(),
            })?;
        }
        let mut out = Vec::with_capacity(self.n_stocks);
        let mut left = self.n_stocks;
        while left > 0 {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::DeskCandles {
                    stock,
                    history,
                    active,
                } => {
                    out.push((stock, history, active));
                    left -= 1;
                }
                other => {
                    return Err(RuntimeError::Disconnected {
                        who: None,
                        stage: format!("take_candle_projection 期望 DeskCandles，收到 {other:?}"),
                    });
                }
            }
        }
        out.sort_by_key(|(s, _, _)| *s);
        Ok(out)
    }

    /// 全局日历读取（驱动器对账面）。
    pub fn global_day(&self) -> u32 {
        self.global.day
    }

    /// 决策观察基读取（测试面）。
    pub fn public_views(&self) -> &[StockPublic] {
        &self.public_views
    }

    /// 外部计划广播（D2 玩家意图路由：组 0 消费——玩家惯例位）。
    pub fn broadcast_external_plan(&mut self, ops: Vec<super::decision::PendingOp>) {
        if let Some(tx) = self.group_txs.first() {
            let _ = tx.send(GroupIn::ExternalPlan(ops));
        }
    }

    /// D4：挂当前相位计时捕获（world 运行期组/部 worker 读取安装）。
    pub fn set_tick_capture(
        &mut self,
        binding: crate::verification_evidence::PhaseTimingBinding,
    ) {
        *self.capture_slot.lock().expect("capture 槽锁被 poison") = binding;
    }

    /// D4：摘除捕获（world 运行结束必调——槽内克隆持有 Arc，不清则
    /// take_capture 的 try_unwrap 失败=CaptureStillBound）。
    pub fn clear_tick_capture(&mut self) {
        *self.capture_slot.lock().expect("capture 槽锁被 poison") =
            crate::verification_evidence::PhaseTimingBinding::empty();
    }

    /// 组注意力调度收集（P1-10：逐账户 next_due 上行——驱动器写回
    /// 会话 npc_attention 的真实状态源）。
    pub fn collect_group_attention(
        &mut self,
    ) -> Result<Vec<(GroupIdx, Vec<(super::LocalIdx, u64)>)>, RuntimeError> {
        if self.group_txs.len() != self.n_groups {
            return Err(RuntimeError::Disconnected {
                who: None,
                stage: format!(
                    "collect_group_attention 通道面与组数不一致（txs={} n_groups={}——世界已断连）",
                    self.group_txs.len(),
                    self.n_groups
                ),
            });
        }
        for tx in &self.group_txs {
            tx.send(GroupIn::EchoAttention).map_err(|_| {
                RuntimeError::SendFailed { detail: "EchoAttention 广播失败".to_owned() }
            })?;
        }
        let mut out = Vec::with_capacity(self.n_groups);
        let mut left = self.n_groups;
        while left > 0 {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::GroupAttention { group, attention } => {
                    out.push((group, attention));
                    left -= 1;
                }
                other => {
                    return Err(RuntimeError::Disconnected {
                        who: None,
                        stage: format!("collect_group_attention 期望 GroupAttention，收到 {other:?}"),
                    });
                }
            }
        }
        out.sort_by_key(|(g, _)| *g);
        Ok(out)
    }

    /// 组账户快照（驱动器纵切/测试面：向全部组发 EchoAccounts 并收齐）。
    /// 生产热路径不调用（账户上行是调试口径开销）。
    pub fn snapshot_accounts(
        &mut self,
    ) -> Result<Vec<(GroupIdx, Vec<GroupAccount>)>, RuntimeError> {
        if self.group_txs.len() != self.n_groups {
            return Err(RuntimeError::Disconnected {
                who: None,
                stage: format!(
                    "snapshot_accounts 通道面与组数不一致（txs={} n_groups={}——世界已断连）",
                    self.group_txs.len(),
                    self.n_groups
                ),
            });
        }
        for tx in &self.group_txs {
            tx.send(GroupIn::EchoAccounts)
                .map_err(|_| RuntimeError::SendFailed {
                    detail: "EchoAccounts 广播失败".to_owned(),
                })?;
        }
        let mut out: Vec<(GroupIdx, Vec<GroupAccount>)> = Vec::with_capacity(self.n_groups);
        let mut left = self.n_groups;
        while left > 0 {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::GroupEcho { group, accounts } => {
                    out.push((group, accounts));
                    left -= 1;
                }
                other => {
                    return Err(RuntimeError::Disconnected {
                        who: None,
                        stage: format!("snapshot_accounts 期望 GroupEcho，收到 {other:?}"),
                    });
                }
            }
        }
        out.sort_by_key(|(g, _)| *g);
        Ok(out)
    }

    /// 运行 n 个 tick（协调者逻辑在本调用线程执行）。
    /// panic 运输：worker panic ⇒ 本函数以原 payload `resume_unwind`（不吞不改写）。
    pub fn run_ticks(&mut self, n: u64) -> Result<u64, RuntimeError> {
        if let Some(reason) = &self.dead_reason {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("world 已死亡，不可继续跑 tick：{reason}"),
            });
        }
        for _ in 0..n {
            let tick = self.global.committed_tick + 1;
            // 日界：上一交易日末 tick 之后、新日首 tick 之前跑 DayEnd 序列
            // （F §6.2：Drain → 组日翻转/权益 → 候选校验 → 本 Start 即隐式提交）。
            if tick > 1
                && (tick - 1) % self.ticks_per_day == 0
                && self.last_day_end_tick != Some(tick - 1)
            {
                // D0 互斥：该交易日尚未日结才自动跑（手动已日结则跳过）。
                self.run_day_end(tick - 1)?;
            }
            // D4：tick 起点先达交易部（FIFO 保证先于同 tick Ops）——
            // 每 tick 必推进 begin_tick（到期/清算/快照不再依赖 Ops 到达）。
            for tx in &self.desk_txs {
                tx.send(DeskMsg::Start { tick })
                    .map_err(|_| RuntimeError::SendFailed {
                        detail: format!("DeskMsg::Start(tick={tick}) 广播失败（部已退出）"),
                    })?;
            }
            broadcast_start(&self.group_txs, tick, &self.public_views)?;
            let mut rounds = 0u32;
            loop {
                let evidence = self.converge_or_die(tick, rounds)?;
                if let Err(detail) = (self.validator)(&evidence) {
                    self.abort_tick(tick);
                    return Err(RuntimeError::TickValidation {
                        tick,
                        round: rounds,
                        detail,
                    });
                }
                if evidence.continuation_any {
                    let next = rounds + 1;
                    if next > self.rounds_cap {
                        self.abort_tick(tick);
                        return Err(RuntimeError::RoundsExceeded {
                            tick,
                            rounds_cap: self.rounds_cap,
                        });
                    }
                    broadcast_continue(&self.group_txs, tick, next)?;
                    rounds = next;
                } else {
                    self.install_tick(tick, rounds, evidence);
                    break;
                }
            }
        }
        Ok(self.global.committed_tick)
    }

    /// 收束一轮；TaskDied ⇒ 撤退 + 收齐 Final + join + `resume_unwind` 原 payload
    /// （不吞、不改写）。收集失败不丢原 payload：stderr 登记后仍 resume。
    fn converge_or_die(&mut self, tick: u64, round: u32) -> Result<TickEvidence, RuntimeError> {
        let outcome = converge_round(
            &self.up_rx,
            self.n_groups,
            self.n_stocks,
            tick,
            round,
            &self.watchdog,
        );
        match outcome {
            RoundOutcome::Converged(ev) => Ok(ev),
            RoundOutcome::TaskDied { who, payload } => {
                // 全体撤退：Abort（幸存者回滚）+ Shutdown（含已死者，发送失败忽略）。
                self.abort.store(true, Ordering::SeqCst);
                for tx in &self.group_txs {
                    let _ = tx.send(GroupIn::Abort { tick });
                    let _ = tx.send(GroupIn::Shutdown);
                }
                for tx in &self.desk_txs {
                    let _ = tx.send(DeskMsg::Abort { tick });
                    let _ = tx.send(DeskMsg::Shutdown);
                }
                match self.collect_finals() {
                    Ok(report) => {
                        let joined = self.join_workers(report).expect("join 不可失败");
                        self.dead_finals = Some(joined);
                        self.dead_reason = Some(format!("worker {who} panic（payload 已 resume）"));
                    }
                    Err(e) => {
                        // 撤退对账失败是第二重失败：显式登记到 stderr，但原 panic
                        // payload 优先（不吞不改写）。
                        eprintln!("panic 撤退路径 Final 收集失败（worker {who}）：{e}");
                    }
                }
                std::panic::resume_unwind(payload);
            }
            RoundOutcome::Failed(RuntimeError::WorkerFailed { who, error }) => {
                // worker 显式失败：撤退（幸存者回滚存活）+ 收齐 Final + 标记死亡。
                self.abort_tick(tick);
                for tx in &self.group_txs {
                    let _ = tx.send(GroupIn::Shutdown);
                }
                for tx in &self.desk_txs {
                    let _ = tx.send(DeskMsg::Shutdown);
                }
                let report =
                    self.collect_finals()
                        .map_err(|e| RuntimeError::ProtocolInvariant {
                            detail: format!("失败撤退路径 Final 收集失败：{e}"),
                        })?;
                self.dead_finals = Some(report);
                self.dead_reason = Some(format!("worker {who} 显式失败"));
                Err(RuntimeError::WorkerFailed { who, error })
            }
            RoundOutcome::Failed(e) => {
                self.abort_tick(tick);
                Err(e)
            }
        }
    }

    /// 广播 Abort（tick 判死；幸存 worker 回滚并存活，可重跑或关停）。
    fn abort_tick(&mut self, tick: u64) {
        self.abort.store(true, Ordering::SeqCst);
        for tx in &self.group_txs {
            let _ = tx.send(GroupIn::Abort { tick });
        }
        for tx in &self.desk_txs {
            let _ = tx.send(DeskMsg::Abort { tick });
        }
    }

    /// 单点安装（唯一安装点语义保持：成功路径才触碰全局域权威）。
    fn install_tick(&mut self, tick: u64, rounds: u32, evidence: TickEvidence) {
        // tick 成功转正：清除可能残留的全局中止标记（上一次 Abort 已回滚收敛）。
        self.abort.store(false, Ordering::SeqCst);
        for (stock, view) in evidence.desk_views {
            if let Some(slot) = self.public_views.get_mut(stock as usize) {
                *slot = view;
            }
        }
        self.global.committed_tick = tick;
        self.global.day = (tick / 240) as u32;
        self.global.events_installed += evidence.entities_out;
        self.report_log.push(TickRecord {
            tick,
            rounds,
            group_hashes: evidence.group_hashes,
            desk_hashes: evidence.desk_hashes,
            desk_resting: evidence.desk_resting,
            desk_trades: evidence.desk_trades,
            desk_lifecycle: evidence.desk_lifecycle,
            desk_receipts: evidence.desk_receipts,
            desk_auction: evidence.desk_auction,
        });
    }

    /// 日结序列（F §6.2）：
    /// ① Drain 交易部（强撤+实体回流+fee_sink 上交）→ 收 DeskDrained×N；
    /// ② 组 DayEnd（Drain 回流落账 → T+1 解冻 → AdminBatch 权益可逆落账）
    ///    → 收 GroupDayDone×G（实体批次先于 DayEnd 入队——FIFO 保证回流入投影）；
    /// ③ 组装日结候选跑全部可失败校验——失败 ⇒ Abort 广播（组/部按可逆日志
    ///    整体冲回上一交易日收敛态，日历不推进）；成功 ⇒ 保存不可变候选，
    ///    下一 Start 即隐式提交（可逆日志退役、Abort 窗口关闭）。
    /// 日结公开入口（驱动器编排：tick 参数由调用方给定）。
    pub fn run_day_end_public(&mut self) -> Result<(), RuntimeError> {
        let day_final_tick = self.global.committed_tick;
        // D0 互斥：同一交易日重复日结显式拒绝（自动路径已日结会跳过）。
        if self.last_day_end_tick == Some(day_final_tick) {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("交易日已日结（收尾 tick={day_final_tick}）——不得重复日结"),
            });
        }
        self.run_day_end(day_final_tick)
    }

    /// 撤销一次**已成功**的日结（Sol 二审修复集 2）：广播 Abort——组按
    /// 日结隔离日志冲回（解冻/权益/Drain 落账），部冲回清簿撤单，回到
    /// 上一交易日收敛态；日结候选一并作废。仅在"日结成功但编排后续
    /// 步骤（如 civil 推进）失败"时调用——此时组/部可逆日志仍存活
    /// （下一 Start 前的 Abort 窗口内），冲回后可重跑日结。
    pub fn abort_day_end(&mut self) {
        let tick = self.global.committed_tick;
        self.abort_tick(tick);
        self.last_day_end_candidate = None;
        self.last_day_end_tick = None;
    }

    fn run_day_end(&mut self, day_final_tick: u64) -> Result<(), RuntimeError> {
        let day = ((day_final_tick - 1) / self.ticks_per_day) as u32;
        // ① Drain。
        for tx in &self.desk_txs {
            tx.send(DeskMsg::Drain)
                .map_err(|_| RuntimeError::SendFailed {
                    detail: "DayEnd Drain 广播失败".to_owned(),
                })?;
        }
        let mut desks: Vec<(StockIdx, i64, i64)> = Vec::with_capacity(self.n_stocks);
        let mut drained = 0usize;
        while drained < self.n_stocks {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::DeskDrained {
                    stock,
                    fee_sink,
                    last_price,
                    drained_lifecycle,
                } => {
                    desks.push((stock, fee_sink, last_price));
                    self.drained_lifecycle.extend(drained_lifecycle);
                    drained += 1;
                }
                UpMsg::Failed { who, error } => {
                    self.abort_tick(day_final_tick);
                    return Err(RuntimeError::WorkerFailed {
                        who,
                        error: Box::new(error),
                    });
                }
                other => {
                    self.abort_tick(day_final_tick);
                    return Err(RuntimeError::Disconnected {
                        who: None,
                        stage: format!("DayEnd Drain 期望 DeskDrained，收到 {other:?}"),
                    });
                }
            }
        }
        // ② 组日翻转 + 权益落账（可逆）。
        for g in 0..self.n_groups as GroupIdx {
            let rights = self
                .rights_computer
                .as_ref()
                .map(|f| f(day, g))
                .unwrap_or_default();
            self.group_txs[g as usize]
                .send(GroupIn::DayEnd(Box::new(DayEndDirective {
                    tick: day_final_tick,
                    day,
                    rights,
                })))
                .map_err(|_| RuntimeError::SendFailed {
                    detail: "DayEnd 广播失败".to_owned(),
                })?;
        }
        let mut groups: Vec<(GroupIdx, Vec<GroupAccount>)> = Vec::with_capacity(self.n_groups);
        let mut group_hashes: Vec<(GroupIdx, u64)> = Vec::with_capacity(self.n_groups);
        let mut done = 0usize;
        while done < self.n_groups {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::GroupDayDone {
                    group,
                    accounts,
                    state_hash,
                    ..
                } => {
                    groups.push((group, accounts));
                    group_hashes.push((group, state_hash));
                    done += 1;
                }
                UpMsg::Failed { who, error } => {
                    self.abort_tick(day_final_tick);
                    return Err(RuntimeError::WorkerFailed {
                        who,
                        error: Box::new(error),
                    });
                }
                other => {
                    self.abort_tick(day_final_tick);
                    return Err(RuntimeError::Disconnected {
                        who: None,
                        stage: format!("DayEnd 期望 GroupDayDone，收到 {other:?}"),
                    });
                }
            }
        }
        // ③ 候选组装 + 全部可失败校验。
        let candidate = DayEndCandidate {
            day,
            tick: day_final_tick,
            groups,
            group_hashes,
            desks,
            global: self.global,
        };
        if let Err(detail) = (self.day_end_validator)(&candidate) {
            self.abort_tick(day_final_tick);
            return Err(RuntimeError::DayEndValidation { day, detail });
        }
        // 成功：候选不可变交出；下一 Start 隐式提交（Abort 窗口关闭）。
        self.last_day_end_candidate = Some(candidate);
        self.last_day_end_tick = Some(day_final_tick);
        Ok(())
    }

    /// 注入日结候选校验器（测试/存档接线点）。
    pub fn set_day_end_validator(&mut self, v: DayEndValidator) {
        self.day_end_validator = v;
    }

    /// 关停序列（对齐原型 orun.rs:373-453）：
    /// Drain 交易部（强撤+实体回流）→ 收 DeskDrained → Shutdown 组（实体批次
    /// 先于 Shutdown 入队——FIFO 保证期末名册含全部回流）→ 收 GroupFinal →
    /// Shutdown 交易部 → 收 DeskFinal → join。
    pub fn shutdown(mut self) -> Result<ShutdownReport, RuntimeError> {
        if let Some(report) = self.dead_finals.take() {
            // 已在失败撤退路径收齐 Final：只补 join。
            self.join_workers(report)
        } else {
            for tx in &self.desk_txs {
                tx.send(DeskMsg::Drain)
                    .map_err(|_| RuntimeError::SendFailed {
                        detail: "Drain 广播失败".to_owned(),
                    })?;
            }
            let mut drained = 0usize;
            while drained < self.n_stocks {
                match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                    UpMsg::DeskDrained { .. } => drained += 1,
                    UpMsg::Failed { who, error } => {
                        return Err(RuntimeError::WorkerFailed {
                            who,
                            error: Box::new(error),
                        });
                    }
                    other => {
                        return Err(RuntimeError::Disconnected {
                            who: None,
                            stage: format!("Drain 期望 DeskDrained，收到 {other:?}（实现 bug）"),
                        });
                    }
                }
            }
            for tx in &self.group_txs {
                tx.send(GroupIn::Shutdown)
                    .map_err(|_| RuntimeError::SendFailed {
                        detail: "组 Shutdown 广播失败".to_owned(),
                    })?;
            }
            // 交易部在组 Final 前发 Shutdown：组的 desk_txs 克隆随组退出释放，
            // 交易部收件循环随后退出并上交 Final。
            for tx in &self.desk_txs {
                tx.send(DeskMsg::Shutdown)
                    .map_err(|_| RuntimeError::SendFailed {
                        detail: "交易部 Shutdown 广播失败".to_owned(),
                    })?;
            }
            let report = self.collect_finals()?;
            self.join_workers(report)
        }
    }

    /// 收齐 G+N Final（worker 退出路径必发）。缺口（worker 未发即消亡）由
    /// 看门狗/断连显式暴露，不静默。
    fn collect_finals(&mut self) -> Result<ShutdownReport, RuntimeError> {
        let mut group_finals: Vec<Option<GroupFinalState>> =
            (0..self.n_groups).map(|_| None).collect();
        let mut desk_finals: Vec<Option<DeskFinalState>> =
            (0..self.n_stocks).map(|_| None).collect();
        let mut left = self.n_groups + self.n_stocks;
        while left > 0 {
            let msg = recv_deadline(&self.up_rx, Phase::FinalCollection, &self.watchdog)?;
            match msg {
                UpMsg::GroupFinal(f) => {
                    let idx = f.group as usize;
                    group_finals[idx] = Some(*f);
                    left -= 1;
                }
                UpMsg::DeskFinal(f) => {
                    let idx = f.stock as usize;
                    desk_finals[idx] = Some(*f);
                    left -= 1;
                }
                UpMsg::Failed { who, error } => {
                    // Failed 与 Final 都会到（worker 先 Failed 后 Final）；记录不中断。
                    let _ = (who, error);
                }
                other => {
                    return Err(RuntimeError::Disconnected {
                        who: None,
                        stage: format!(
                            "Final 收集期望 GroupFinal/DeskFinal，收到 {other:?}（实现 bug）"
                        ),
                    });
                }
            }
        }
        let missing_groups: Vec<usize> = group_finals
            .iter()
            .enumerate()
            .filter(|(_, f)| f.is_none())
            .map(|(i, _)| i)
            .collect();
        if !missing_groups.is_empty() {
            return Err(RuntimeError::Disconnected {
                who: None,
                stage: format!(
                    "GroupFinal 缺口：组 {missing_groups:?} 未上交（worker 未发 Final 即消亡）"
                ),
            });
        }
        let missing_desks: Vec<usize> = desk_finals
            .iter()
            .enumerate()
            .filter(|(_, f)| f.is_none())
            .map(|(i, _)| i)
            .collect();
        if !missing_desks.is_empty() {
            return Err(RuntimeError::Disconnected {
                who: None,
                stage: format!(
                    "DeskFinal 缺口：交易部 {missing_desks:?} 未上交（worker 未发 Final 即消亡）"
                ),
            });
        }
        Ok(ShutdownReport {
            group_finals: group_finals
                .into_iter()
                .map(|f| f.expect("已核验非空"))
                .collect(),
            desk_finals: desk_finals
                .into_iter()
                .map(|f| f.expect("已核验非空"))
                .collect(),
            worker_results: Vec::new(),
        })
    }

    /// join 全部 worker（消费句柄）。
    fn join_workers(&mut self, mut report: ShutdownReport) -> Result<ShutdownReport, RuntimeError> {
        for (who, handle) in std::mem::take(&mut self.handles) {
            match handle.join() {
                Ok(r) => report.worker_results.push((who, r)),
                Err(_) => {
                    report.worker_results.push((
                        who,
                        Err(RuntimeError::ProtocolInvariant {
                            detail: "worker 线程 panic 退出（payload 应已由 TaskDied 运输）"
                                .to_owned(),
                        }),
                    ));
                }
            }
        }
        Ok(report)
    }

    /// 切断协调者下行通道（断连显式失败测试面）：worker 见 Disconnected 退出。
    pub fn sever(&mut self) {
        self.group_txs.clear();
        self.desk_txs.clear();
    }
}

impl Drop for World {
    /// D0 兜底关停：显式 shutdown 之外的丢弃路径（如 session 析构带走
    /// 常驻 driver）也必须回收组/部线程——发 Shutdown（失败=已退出）并
    /// join（结果忽略：worker 错误本经上行通道报告，Drop 期无处报告，
    /// 此处只保证线程回收）。
    fn drop(&mut self) {
        for tx in &self.group_txs {
            let _ = tx.send(GroupIn::Shutdown);
        }
        for tx in &self.desk_txs {
            let _ = tx.send(DeskMsg::Shutdown);
        }
        for (_, handle) in std::mem::take(&mut self.handles) {
            let _ = handle.join();
        }
    }
}
