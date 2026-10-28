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

mod prepared_stop;
pub(crate) use prepared_stop::StopReport;

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
use std::time::Duration;
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
    /// D3a-2：种子日 K（会话预置历史——desk 为链观察日 K 权威）。
    pub initial_daily_bars: Vec<(i64, i64, i64, i64, u64)>,
    /// D5 restore 等价：种子 tick 价格历史（restore/接管场景 desk 起步
    /// 恢复会话历史——save 校验 fault2010 的长度面）。
    pub initial_recent_prices: Vec<i64>,
    /// D3a-2：种子外累计 traded 日数（0=全新）。
    pub initial_traded_days: u64,
    /// D5 restore 链最后一层：日内行情聚合种子（None=构造零点）。
    pub initial_market: Option<super::desk::DeskMarketSeed>,
    /// D5 attention：收盘竞价公开截点种子（restore 链——market 的
    /// auction_public_cutoff_sequence 投影；0=全新世界）。
    pub initial_public_cutoff: u64,
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
            initial_daily_bars: Vec::new(),
            initial_recent_prices: Vec::new(),
            initial_traded_days: 0,
            initial_market: None,
            initial_public_cutoff: 0,
        }
    }
}

/// 信息投递载荷（Belief-1b·下：驱动器组装——库 Arc+镜像快照+映射）。
pub struct GroupInformationPayload {
    pub now: crate::calendar::CivilInstant,
    pub day: u64,
    pub library: std::sync::Arc<crate::information::PublicLibrary>,
    pub attentions: Vec<Vec<(
        super::LocalIdx,
        crate::AccountId,
        crate::session::NpcAttentionState,
    )>>,
}

/// 一次完整个人读面的真实槽；None 是明确缺乏能力，不代表缺少真实名册成员。
pub(crate) struct PersonalFullSnapshot {
    pub source: super::source::WorldMirrorSource,
    pub groups: Vec<(GroupIdx, Vec<(super::LocalIdx, crate::AccountId, Option<BeliefParticipantSnapshot>)>)>,
}

/// 信念个人态快照（D3a-2b Belief-1：watchlist/价格记忆/信息态/信念书
/// ——crate 纯类型直载；构造期自 session 投影进组，读面上行同源校验。
/// D3a-2c 件2 增补 attention/history_reads（observe 直调的完整个人态承载
/// ——组内权威，构造期投影）。
#[derive(Clone, Debug, PartialEq)]
pub struct BeliefParticipantSnapshot {
    pub watchlist: crate::experience::PersonalWatchlist,
    pub price_memory: crate::experience::PersonalPriceMemory,
    pub information: crate::information::NpcInformationState,
    pub belief: crate::strategy::BeliefBook,
    /// 注意力状态（D3a-2c：observe 的 discovery RNG 流承载——组内权威）。
    pub attention: crate::session::NpcAttentionState,
    /// 公共历史读账（observe 的 record_public_history_read 承载）。
    pub history_reads: crate::experience::PersonalHistoryReadLedger,
    /// D5 #4 retail 链：零售经验行（retail 位 Some——观察/决策/经验反馈
    /// 的组内权威载体；inst/玩家位 None）。
    pub retail_experience: Option<crate::RetailExperienceState>,
}

/// 开仓派生记录（D3a-2c：组线程 observe 直调的输入/输出——红证同输入
/// 比对载体）。
#[derive(Clone, Debug, PartialEq)]
pub struct OpenDerivationRecord {
    /// 账户。
    pub account: crate::AccountId,
    /// observe 输入个人态（红证同输入锚）。
    pub personal_in: crate::session::PlanPersonalState,
    /// observe 输出个人态（演进后）。
    pub personal_out: crate::session::PlanPersonalState,
    /// Lifecycle 携带的候选评估（账户序单元素）。
    pub assessments: Vec<(
        crate::StockId,
        crate::plans::CandidateAssessment,
    )>,
    /// D3a-2c 件4：组内 Lifecycle 开仓消费产物（PlanOpen——红证锚；
    /// 已落组内 PlanBook）。
    pub opens: Vec<crate::plans::PlanOpen>,
}

/// 决策链根组装投影（D3a-2c 件2：构造期自 session 一次性投影——catalog
/// 克隆、初始 CompanyReadFacts 共享、名册 AccountId、相位配置）。
pub struct RootProjection {
    /// 股票目录（id↔代码翻译层）。
    pub catalog: crate::stock::StockCatalog,
    /// 构造静点安装的选定模型公开事实；后续消费经组当前引用。
    pub initial_company_facts: Arc<crate::company::CompanyReadFacts>,
    /// 组内名册的 AccountId（下标=LocalIdx；红证/session 映射）。
    pub account_ids: Vec<crate::AccountId>,
    /// 开盘集合竞价窗 tick 数（CivilInstant 秒映射输入）。
    pub auction_ticks: u64,
    /// 收盘集合竞价窗 tick 数。
    pub closing_auction_ticks: u64,
    /// D3a-2c 件4：信念链参数/机构风格（下标=LocalIdx；None=非信念机构）。
    pub chain_params: Vec<Option<(crate::strategy::BeliefChainParams, Option<crate::strategy::InstitutionStyle>)>>,
    /// D5 #4 retail 链：会话种子（决策 rng 的 npc_seed_base 同源输入——
    /// retail 决策流与旧管线 A 类任务同式派生）。
    pub session_seed: u64,
    /// D3a-2c 件4：紧迫度策略（组内 initial_urgency 输入——同 session 对象克隆）。
    pub urgency_policy: crate::plans::UrgencyPolicy,
    /// D3a-3 件4b：每股限价单笔上限（category.max_order_qty(false)——
    /// 预算请求与报价数量输入）。
    pub max_order_qty_limit: std::collections::BTreeMap<crate::StockId, u32>,
    /// D3a-3 复核收口：每股最小报价单位（tick——报价取整口径，setup 权威）。
    pub tick_size_cents: std::collections::BTreeMap<crate::StockId, i64>,
}

/// 世界构造参数。
pub struct WorldSpec {
    /// 按组移交的完整计划簿；空 Vec 仅表示无历史的新世界。
    pub initial_plan_books: Vec<crate::plans::PlanBook>,
    /// 实际 AccountBook 派生的不可变专户名册；空集合不授予资金角色。
    pub admin_treasury_bindings: Vec<super::protocol::AdminTreasuryBinding>,
    /// 组数 G。
    pub n_groups: usize,
    /// 每组账户数。
    pub accounts_per_group: usize,
    /// 每账户初始现金（i64 分）。
    pub initial_cash: i64,
    /// 初始持仓 `[group][local][stock]`（缺省零）。
    pub initial_shares: Vec<Vec<Vec<u32>>>,
    /// D3a-2c 件1：初始成本基 invested `[group][local][stock]`（i64 分；
    /// 构造投影自 session positions——开仓派生 cost_price 消费）。
    pub initial_invested: Vec<Vec<Vec<i64>>>,
    /// D3a-2c 件1：初始成本基 recovered `[group][local][stock]`（i64 分）。
    pub initial_recovered: Vec<Vec<Vec<i64>>>,
    /// D3a-3：初始曾持有位 `[group][local][stock]`（session positions 的
    /// 零量条目面投影——清仓后观察面 held 集成员）。
    pub initial_ever_held: Vec<Vec<Vec<bool>>>,
    /// D5 restore 链最后一层：初始 T+1 当日冻结 `[group][local][stock]`
    /// （session positions t1_locked 投影；缺省零）。
    pub initial_frozen_today: Vec<Vec<Vec<u32>>>,
    /// D5 restore 链最后一层：初始在途买占用 `[group][local]`（i64 分
    /// 聚合额——restore 形态的 ledger 买种子 purse 投影；缺省零）。
    pub initial_purse_held: Vec<Vec<i64>>,
    /// Sol 终审四审预留族：初始在途卖余量镜像 `[group][local][stock]`
    /// （desk 卖种子的组端 pending_sell_qty 投影——公开持仓=组 shares+
    /// 在途卖余量口径的加回面；缺省零）。
    pub initial_pending_sell: Vec<Vec<Vec<u32>>>,
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
    /// Sol 终审四审 A2（restore 重复日结）：恢复形态下上一拍若为日结
    /// 拍（session.tick%tpd==0——session 日终迁移已跑），构造期即视为
    /// 该交易日已日结——否则首拍 run_ticks 的自动日结判定会重复跑
    /// 一次日结（desk 二次归档使日 K 多一条 fault2064）。
    pub initial_last_day_end_tick: Option<u64>,
    /// 内联决策装配（Some=decide_data 路径；None=测试脚本路径）。
    pub decision: Option<DecisionSetup>,
    /// 公司域权益产出（日结 AdminBatch 注入；None=无权益）。
    pub rights_computer: Option<Box<dyn Fn(u32, GroupIdx) -> Vec<AdminRight> + Send>>,
    /// 按账户现金重建（存档候选往返；None=用 initial_cash 均一初始化）。
    pub initial_cash_override: Option<Vec<Vec<i64>>>,
    /// 计划链工厂（批 6：跨股多步计划经 Continue 轮排空）。
    pub plan_factory: Option<Box<dyn Fn(GroupIdx) -> Vec<Box<dyn PlanChain>> + Send>>,
    /// D3a-2b：信念个人态种子（组 0 按下标承接——None=非机构位；
    /// 构造期一次性投影，日结披露驱动随 Belief-1b 下沉）。
    pub belief_seeds: Vec<Option<super::world::BeliefParticipantSnapshot>>,
    /// Belief-1b·下：公开库初始 Arc（缺省空库——投递消息稀疏刷新）。
    pub library: Option<std::sync::Arc<crate::information::PublicLibrary>>,
    /// D5 adopt：构造期直入 desk 初始簿的会话旧簿在途单（按股 idx）。
    pub initial_desk_orders: Vec<Vec<super::desk::DeskRestingSeed>>,
    /// 每股 desk 序号游标种子（恢复链——空簿也保留已耗序号；Sol 四轮
    /// 修复：save 投影持久化真实 next_serial，恢复侧贯通到新 Desk）。
    pub initial_desk_next_serial: Vec<u64>,
    /// D3a-2c 件2：决策链根组装投影（None=无会话来源——observe 直调不可
    /// 用，读面上行空）。
    pub root_projection: Option<std::sync::Arc<RootProjection>>,
    /// D3a-2c 件2：起始自然日（快照 civil_date 基线——驱动器每步同步）。
    pub civil_start_date: crate::calendar::CivilDate,
    /// raw Calendar 会话计数的起日；与当前 civil_start_date 分开显式提供。
    pub calendar_start_date: crate::calendar::CivilDate,
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
    #[cfg(feature = "simulation-diagnostics")]
    pub decision_traces: Vec<super::diagnostics::GroupDecisionTrace>,
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
    /// 本 tick 双腿成交事实（事件映射输入；驱动器/测试面）。Continue
    /// 链下含**全轮**成交（早轮+终轮，每条恰一次——事实不丢）。
    pub desk_trades: Vec<super::protocol::DeskTradeFact>,
    /// 默认 minimal 生命周期；driver 在普通 record clone 前取走。
    pub retail: Vec<super::retail_lifecycle::Fact>,
    /// 未交付的诊断 sideband；驱动器取走后记录中只保留空 Vec。
    #[cfg(feature = "simulation-diagnostics")]
    pub desk_causal: Vec<super::diagnostics::DeskCausalFact>,
    /// Q2 批 4（D3 退役）：unsettled_trades 字段已删除——Seal 落账
    ///（批 2）使"终轮未落账尾巴"结构性不存在（合同 5 由类型承载：
    /// 复活需显式重新引入字段并过全部行为合同）。
    /// 本 tick 生命周期事实（受理/撤单/到期；事件映射输入）。
    pub desk_lifecycle: Vec<super::protocol::DeskLifecycleFact>,
    /// 本轮回执（拒收/不可撤；IntentRejected 映射输入）。
    pub desk_receipts: Vec<DeskReceipt>,
    /// 组端业务拒绝（资金/可卖/零股/竞价×市价；同上映射输入）。
    pub group_rejections: Vec<DeskReceipt>,
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
    /// B2：期末 Drain 应答捎带的当日分钟K桶（下标=股——观察面与日结
    /// 统一；跨日形态 live 桶已并入应答）。
    pub drained_minute_bars: Vec<Option<Vec<super::protocol::MinuteBarFact>>>,
}

struct DayEndProgress {
    desks_sent: usize,
    groups_sent: usize,
    desks_reported: Vec<bool>,
    groups_reported: Vec<bool>,
}

/// 只记录本 authority 真正成功提交的最后市场拍及当时的发布身份。
#[derive(Clone, Debug)]
pub(crate) struct CommittedCalendarTick {
    pub calendar: Arc<super::MarketTickContext>,
    pub source: super::source::WorldMirrorSource,
}

/// 世界（协调者视角持有全部通道与线程句柄）。
pub struct World {
    calendar_source: Arc<crate::session::market_calendar_context::MarketCalendarSource>,
    calendar_bindings: Vec<(crate::StockId, crate::calendar::CalendarExchange)>,
    tick_layout: crate::session::exchange_calendar_model::SessionTickLayout,
    pending_calendar_tick: Option<Arc<super::MarketTickContext>>,
    last_committed_calendar: Option<CommittedCalendarTick>,
    admin_next_id: u64,
    authority_epoch: u64,
    non_tick_revision: u64,
    pending_mirror_domains: super::source::MirrorDomains,
    mirror_day_end_stage: super::source::MirrorDayEndStage,
    /// 仅身份元数据，用于核对信息候选回执的组内 local/AccountId；不保存个人态。
    information_roster: Vec<Vec<crate::AccountId>>,
    personal_presence: Option<Vec<(GroupIdx, Vec<(crate::AccountId, super::personal_full_validation::PersonalPresence)>)>>,
    #[cfg(test)]
    admin_send_fault_for_test: Option<(super::protocol::AdminPhase, GroupIdx)>,
    /// 全局域权威（已收敛态）。
    pub global: GlobalState,
    /// 已收敛 tick 记录。
    pub report_log: Vec<TickRecord>,
    group_txs: Vec<Sender<GroupIn>>,
    desk_txs: Vec<Sender<DeskMsg>>,
    up_rx: Receiver<UpMsg>,
    handles: Vec<(WorkerId, crate::thread_facade::JoinHandle<Result<(), RuntimeError>>)>,
    abort: Arc<AtomicBool>,
    /// Sol 终审三审：永久关停标记（sever/Drop/显式 shutdown 置位）——
    /// worker_recv 的退出判定源（与 abort 正交：abort=tick 级回滚语义，
    /// 可重试 Abort 窗口内 worker 存活）。
    worker_shutdown: Arc<AtomicBool>,
    /// Sol 五审再复核 P1-2（四审 A1 指定回归的确定性 seam）：正常关停
    /// 序列在组 Shutdown 广播**前**的可控停顿（cfg(test)——复现四审
    /// 竞态窗"协调者停顿>50ms 时空闲 worker 提前退出"）。None=不停顿
    /// （生产语义零变化）。
    #[cfg(test)]
    pub shutdown_stall_for_test: Option<Duration>,
    /// Q2 存活契约测试注入（one-shot）：下一次 Seal 收尾在**广播后**
    /// 消费 consume_acks 条真实 SealAck 再合成收集失败（Abort 语义
    ///——worker 全存活，同 tick 重试成功）。Sol 复核 P1-3：注入点自
    /// 广播前迁移到广播后（真实路径——组已收到 Seal，Abort 与 Seal 同
    /// 队列竞态、迟到 Ack 落入重试收束窗都被注入面覆盖）。
    #[cfg(test)]
    seal_ack_fail_after_acks: Option<usize>,
    #[cfg(test)]
    day_end_fail_after_drain_reports: Option<usize>,
    #[cfg(test)]
    day_end_fail_before_group: Option<usize>,
    #[cfg(test)]
    start_cleanup_message_for_test: Option<UpMsg>,
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
    /// 成功日结的广播基撤销记录；下一真实 Start 关闭窗口，不参与 PublishedTick。
    day_end_prior_public_views: Option<Vec<StockPublic>>,
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
    /// Sol 六审段 1.2：日结 Drain 的收盘竞价清算事实（末拍事件源）。
    pub drained_auction: Vec<super::protocol::DeskAuctionFact>,
    /// 日界批：日结 Drain 捎带的收盘竞价成交（原随下一拍 DeskReport
    /// 迟到——receipt civil_date 取组装时点成次日；驱动器在日结段
    /// 消费入事件流+个人确认，取走即清）。
    pub drained_day_trades: Vec<super::protocol::DeskTradeFact>,
    pub(crate) drained_retail: Vec<super::retail_lifecycle::Fact>,
    /// 当前自然日（D3a-2c：快照 civil_date 来源——驱动器自会话同步，
    /// 自然日权威在会话 civil clock）。
    civil_date: crate::calendar::CivilDate,
    /// Q1 发布槽（方案 A）：install_tick 单点 publish；读者 load() 得
    /// 一致读点。初值=spawn 时的 v0。
    publication: std::sync::Arc<super::publication::ViewPublication>,
    /// Q2 A 口径归因诊断：seal_round 累计 wall（含广播+收集等待——
    /// 组端 drain/落账/分片构建都反映在等待里）。瞬态诊断面，不入
    /// 存档/hash；探针读数用。
    pub seal_ns_accumulated: u128,
    /// Seal 地板分解诊断（同上瞬态面）：广播→首条 Ack 的累计墙
    /// （最快组的完整链路=send+唤醒+组端处理+回传）与首末 Ack 间隔
    /// 累计（组间长尾方差）。地板归因用（换向链固有 vs 组端不均）。
    pub seal_first_ack_ns: u128,
    /// 首末 Ack 间隔累计（组间 spread）。
    pub seal_ack_spread_ns: u128,
    /// B-1' 验证批诊断：最近一次 seal_round 最晚 Ack 的组号（每组计数
    /// ——最晚组身份分布是"唤醒级联 vs 组端处理"假设的直接证据：
    /// 最晚组集中在低 handle 闲组=唤醒假设成立；集中在活跃组=组端
    /// 处理主导，假设推翻回设计台）。瞬态诊断面，不入存档/hash。
    pub seal_last_ack_groups: Vec<u32>,
    /// B2 保留历史链（日结冻结的不可变快照——ADR-0034 全部保留；发布
    /// 面共享外壳见 `retained_index`）。
    retained_days: Vec<std::sync::Arc<super::publication::RetainedDaySnapshot>>,
    /// B2：保留链发布外壳（非冻结 tick `Arc::clone` 复用——避免逐拍
    /// 重建 Vec）。
    retained_index: std::sync::Arc<super::publication::RetainedIndex>,
    /// B2：日结 Drain 应答捎带的当日分钟K桶（驱动器编排冻结的消费面；
    /// 每日结一次，取走即清）。
    pending_day_bars: Vec<Option<Vec<super::protocol::MinuteBarFact>>>,
    /// 日界批：日结 Drain 应答捎带的当日终 K（含收盘段——驱动器
    /// install 面补真值的消费面；与 pending_day_bars 同生命周期）。
    pending_day_candles: Vec<Option<super::protocol::DeskDayCandleFinal>>,
    /// B2：发布代次（每次 publish 递增——区分同 tick 的保留链推进）。
    publish_generation: u64,
}

impl World {
    pub(crate) fn mirror_source(&self) -> super::source::WorldMirrorSource {
        let published = self.publication.load();
        super::source::WorldMirrorSource {
            authority_epoch: self.authority_epoch,
            tick: self.global.committed_tick,
            day: self.global.day,
            generation: published.generation,
            day_end_stage: self.mirror_day_end_stage,
            non_tick_revision: self.non_tick_revision,
            civil_date: self.civil_date,
        }
    }

    pub(crate) fn pending_mirror_domains(&self) -> super::source::MirrorDomains {
        self.pending_mirror_domains
    }

    pub(crate) fn prepare_mirror_ack(&self, source: super::source::WorldMirrorSource, domains: super::source::MirrorDomains) -> Result<super::source::PreparedMirrorAck, RuntimeError> {
        if source != self.mirror_source() || domains != self.pending_mirror_domains {
            return Err(RuntimeError::ProtocolInvariant { detail: format!("镜像 candidate 已过期：prepared={source:?} current={:?}", self.mirror_source()) });
        }
        Ok(super::source::PreparedMirrorAck { domains })
    }

    /// 调用者持有同一独占静点，source 已在唯一 swap 前验证，期间不再执行 callback。
    pub(crate) fn ack_prepared_mirror(&mut self, ack: super::source::PreparedMirrorAck) {
        if ack.domains.accounts_full { self.pending_mirror_domains.accounts_full = false; }
        if ack.domains.personal_full { self.pending_mirror_domains.personal_full = false; }
        if ack.domains.attention_full { self.pending_mirror_domains.attention_full = false; }
        if ack.domains.plans_pending { self.pending_mirror_domains.plans_pending = false; }
    }

    /// Driver 只在构造期接收已按实际 strategy/participant/attention 验证的能力名册。
    pub(crate) fn install_personal_presence(&mut self, authority_epoch: u64, roster: Vec<(GroupIdx, Vec<(crate::AccountId, super::personal_full_validation::PersonalPresence)>)>) {
        assert!(self.personal_presence.is_none() && self.report_log.is_empty(), "完整个人能力名册只能初始安装一次");
        assert_eq!(roster.len(), self.n_groups, "Driver 已验证的能力名册必须覆盖全部组");
        for (expected, (group, rows)) in self.information_roster.iter().zip(&roster) {
            assert_eq!(&rows.iter().map(|(id, _)| *id).collect::<Vec<_>>(), expected, "能力名册与真实 World 名册必须同源");
            assert!(usize::from(*group) < self.n_groups, "能力名册组号必须来自已验证拓扑");
        }
        self.personal_presence = Some(roster);
        self.authority_epoch = authority_epoch;
    }

    /// 完整只读回执不消费 dirty/pending，不执行任何个人计算。
    pub(crate) fn collect_personal_full(&mut self) -> Result<PersonalFullSnapshot, RuntimeError> {
        match self.collect_personal_full_inner() {
            Ok(snapshot) => Ok(snapshot),
            Err(error) => Err(self.stop_after_fatal_failure(
                self.global.committed_tick, 0, "collect_personal_full", error, true, None,
            )),
        }
    }

    /// prepared World 的失败由拥有者统一回收，不进入常态的 panic 重抛路径。
    pub(crate) fn collect_personal_full_for_prepared(&mut self) -> Result<PersonalFullSnapshot, RuntimeError> {
        self.collect_personal_full_inner()
    }

    fn collect_personal_full_inner(&mut self) -> Result<PersonalFullSnapshot, RuntimeError> {
        if let Some(reason) = &self.dead_reason {
            return Err(RuntimeError::ProtocolInvariant { detail: format!("Full 读取拒绝已终止 World：{reason}") });
        }
        let expected = self.personal_presence.as_ref().ok_or_else(|| RuntimeError::ProtocolInvariant {
            detail: "完整个人态缺构造期实际能力名册；不能从缺 seed 推断合法 None".to_owned(),
        })?;
        if self.group_txs.len() != self.n_groups {
            return Err(RuntimeError::ProtocolInvariant { detail: "Full 读取通道与组数不一致".to_owned() });
        }
        let source = self.mirror_source();
        for tx in &self.group_txs {
            tx.send(GroupIn::EchoPersonalFull { source }).map_err(|_| RuntimeError::SendFailed {
                detail: "EchoPersonalFull 广播失败".to_owned(),
            })?;
        }
        let mut groups = Vec::with_capacity(self.n_groups);
        for _ in 0..self.n_groups {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::GroupPersonalFull { group, source: actual, slots } => {
                    if actual != source {
                        return Err(RuntimeError::ProtocolInvariant { detail: format!("Full 个人回执来源错误：group={group} expected={source:?} actual={actual:?}") });
                    }
                    groups.push((group, slots));
                }
                UpMsg::Failed { who, error } => return Err(RuntimeError::WorkerFailed { who, error: Box::new(error) }),
                other => return Err(prepared_stop::read_failure("Full 个人读取", other)),
            }
        }
        super::personal_full_validation::validate_personal_full(expected, &groups)
            .map_err(|error| RuntimeError::ProtocolInvariant { detail: format!("Full 个人态完整性错误：{error:?}") })?;
        if source != self.mirror_source() {
            return Err(RuntimeError::ProtocolInvariant { detail: "Full 读取期间 World 静点发生变化".to_owned() });
        }
        groups.sort_by_key(|(group, _)| *group);
        Ok(PersonalFullSnapshot { source, groups })
    }

    /// 构造世界并启动全部 worker 线程。
    pub fn spawn(
        spec: WorldSpec,
        calendar: crate::calendar::TradingCalendar,
    ) -> Result<Self, RuntimeError> {
        let bindings = spec
            .stocks
            .iter()
            .enumerate()
            .map(|(index, stock)| {
                let index = u32::try_from(index).map_err(|_| RuntimeError::ProtocolInvariant {
                    detail: "raw StockIdx 超过 u32".to_owned(),
                })?;
                let exchange = match stock.exchange {
                    crate::StockExchange::Shanghai => crate::calendar::CalendarExchange::Sse,
                    crate::StockExchange::Shenzhen => crate::calendar::CalendarExchange::Szse,
                };
                Ok((crate::StockId(index), exchange))
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        let source = crate::session::market_calendar_context::MarketCalendarSource::new(
            calendar,
            spec.calendar_start_date,
            bindings.clone(),
        )
        .map_err(|error| RuntimeError::ProtocolInvariant {
            detail: error.to_string(),
        })?;
        Self::spawn_with_calendar_source(spec, source, bindings)
    }

    pub(crate) fn spawn_with_calendar_source(
        spec: WorldSpec,
        calendar_source: Arc<crate::session::market_calendar_context::MarketCalendarSource>,
        calendar_bindings: Vec<(crate::StockId, crate::calendar::CalendarExchange)>,
    ) -> Result<Self, RuntimeError> {
        let mut actual_bindings = std::collections::BTreeMap::new();
        for (index, stock) in spec.stocks.iter().enumerate() {
            let id = crate::StockId(u32::try_from(index).map_err(|_| {
                RuntimeError::ProtocolInvariant {
                    detail: "runtime dense StockId 越界".to_owned(),
                }
            })?);
            let exchange = match stock.exchange {
                crate::StockExchange::Shanghai => crate::calendar::CalendarExchange::Sse,
                crate::StockExchange::Shenzhen => crate::calendar::CalendarExchange::Szse,
            };
            actual_bindings.insert(id, exchange);
        }
        if calendar_bindings.len() != actual_bindings.len()
            || calendar_bindings
                .iter()
                .any(|(stock, exchange)| actual_bindings.get(stock) != Some(exchange))
        {
            return Err(RuntimeError::ProtocolInvariant {
                detail: "source bindings 与 runtime 实际 dense StockId/exchange 不一致".to_owned(),
            });
        }
        let initial_calendar_day = calendar_source
            .day(spec.civil_start_date)
            .and_then(|day| {
                day.validate_for(&calendar_source, &calendar_bindings, spec.civil_start_date)?;
                Ok(day)
            })
            .map_err(|error| RuntimeError::ProtocolInvariant {
                detail: error.to_string(),
            })?;
        let tick_layout = crate::session::exchange_calendar_model::SessionTickLayout {
            ticks_per_day: spec.ticks_per_day,
            auction_ticks: spec.auction_ticks,
            closing_auction_ticks: spec.closing_auction_ticks,
        };
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
        admin_treasury_binding::validate(&spec)?;
        let information_roster = (0..spec.n_groups).map(|group| {
            spec.root_projection.as_ref().map(|root| {
                let start = group * spec.accounts_per_group;
                root.account_ids.iter().skip(start).take(spec.accounts_per_group).copied().collect()
            }).unwrap_or_default()
        }).collect();
        let WorldSpec {
            initial_plan_books,
            admin_treasury_bindings,
            n_groups,
            accounts_per_group,
            initial_cash,
            initial_shares,
            initial_invested,
            initial_recovered,
            initial_ever_held,
            initial_frozen_today,
            initial_purse_held,
            initial_pending_sell,
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
            initial_last_day_end_tick,
            decision,
            rights_computer,
            initial_cash_override,
            plan_factory,
            belief_seeds,
            library,
            initial_desk_orders,
            initial_desk_next_serial,
            root_projection,
            civil_start_date,
            calendar_start_date: _,
        } = spec;
        // 可失败的行情计算须在启动 worker 前完成，防止构造失败遗留后台线程。
        // 初始广播是首个 Start 的观察基；只有该拍处于连续竞价时才叠加价格笼子
        //（集合竞价/盘前不接受连续竞价笼子约束）。相位规则与 desk 单一同源。
        let cage_on = config.price_cage_enabled;
        let first_tick = initial_committed_tick.saturating_add(1);
        let cage_applies_first_tick = super::desk::phase_at_tick(
            ticks_per_day,
            auction_ticks,
            closing_auction_ticks,
            first_tick,
        ) == super::desk::DeskPhase::Continuous;
        let public_views = Self::initial_views(&stocks, cage_on, cage_applies_first_tick)?;
        // 任何线程启动前校验完整组集合、唯一账户 writer 与所有 child 引用。
        let roster = root_projection.as_ref().map(|root| root.account_ids.as_slice()).unwrap_or(&[]);
        let books = if initial_plan_books.is_empty() {
            vec![crate::plans::PlanBook::default(); n_groups]
        } else { initial_plan_books };
        let policy = books.first().map(|book| *book.policy()).unwrap_or_default();
        super::plan_ownership::merge(
            books.iter().cloned().enumerate().map(|(group, book)| (group as GroupIdx, book)).collect(),
            roster, n_groups, accounts_per_group.max(1), policy,
        )?;
        let prepared_plans = super::group::prepare_plan_seeds(
            books, roster, accounts_per_group.max(1), &initial_desk_orders,
        )?;
        let mut prepared_plans = prepared_plans.into_iter();
        let library_seed: std::sync::Arc<crate::information::PublicLibrary> =
            library.unwrap_or_default();
        let mut stock_seed_bars: Vec<Vec<(i64, i64, i64, i64, u64)>> = stocks
            .iter()
            .map(|spec| spec.initial_daily_bars.clone())
            .collect();
        let mut stock_seed_traded: Vec<u64> =
            stocks.iter().map(|spec| spec.initial_traded_days).collect();
        let n_stocks = stocks.len();
        let abort = Arc::new(AtomicBool::new(false));
        let worker_shutdown = Arc::new(AtomicBool::new(false));
        let capture_slot: std::sync::Arc<
            std::sync::Mutex<crate::verification_evidence::PhaseTimingBinding>,
        > = std::sync::Arc::new(std::sync::Mutex::new(
            crate::verification_evidence::PhaseTimingBinding::empty(),
        ));
        // 等待点账（R4a 阶段 1，设计 §10.1）：本构造体建立 1+G+N 条
        // unbounded channel + deferred_drop 一条 bounded(64)。业务层
        // 等待点全部经 watchdog 三函数（相位分档）；unbounded send 业务
        // 层零等待——原语层可达等待点 (l)(m)(n)（分配锁/SyncWaker/链表
        // 发布）逃逸外包宿主，锁序图见 §10.1.3（capture_slot 只进 (l)
        // 锁叶，临界区 lock→clone→释放——由 wait_ledger_tests 静态钉住
        // 「持锁不 send」）。
        let (up_tx, up_rx) = channel::<UpMsg>();
        let mut belief_seeds_clone = belief_seeds;
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
                // D3a-2c 件1：构造期成本基投影（session positions 累计器直取）。
                if let Some(invested) = initial_invested
                    .get(g as usize)
                    .and_then(|by_group| by_group.get(l))
                {
                    for (slot, &v) in acct.invested_cents.iter_mut().zip(invested) {
                        *slot = v;
                    }
                }
                if let Some(recovered) = initial_recovered
                    .get(g as usize)
                    .and_then(|by_group| by_group.get(l))
                {
                    for (slot, &v) in acct.recovered_cents.iter_mut().zip(recovered) {
                        *slot = v;
                    }
                }
                if let Some(ever) = initial_ever_held
                    .get(g as usize)
                    .and_then(|by_group| by_group.get(l))
                {
                    for (slot, &v) in acct.ever_held.iter_mut().zip(ever) {
                        *slot = v;
                    }
                }
                // D5 restore 链最后一层：T+1 当日冻结投影（save 前买入的
                /// 当日冻结面——组侧冻结权威自此起步）。
                if let Some(frozen) = initial_frozen_today
                    .get(g as usize)
                    .and_then(|by_group| by_group.get(l))
                {
                    for (slot, &v) in acct.frozen_today.iter_mut().zip(frozen) {
                        *slot = v;
                    }
                }
                // D5 restore 链最后一层：在途买占用投影（ledger 买种子
                /// purse——组现金已同步扣减，公开现金=组现金+占用口径
                /// 因此成立）。
                if let Some(&purse) = initial_purse_held
                    .get(g as usize)
                    .and_then(|by_group| by_group.get(l))
                {
                    acct.buy_purse_held_cents = purse;
                }
                // Sol 终审四审预留族：在途卖余量镜像投影（desk 卖种子
                /// 的组端 pending_sell_qty——组 shares 已同步扣减，公开
                /// 持仓=组 shares+余量口径因此成立；可卖=组 shares−冻结
                /// 不含在途）。
                if let Some(pending) = initial_pending_sell
                    .get(g as usize)
                    .and_then(|by_group| by_group.get(l))
                {
                    for (slot, &qty) in acct.pending_sell_qty.iter_mut().zip(pending) {
                        *slot = qty;
                    }
                }
                accounts.push(acct);
            }
            let group_decision = decision.clone().map(|mut d| {
                // 翻转批组参数化：每组取全局名册的第 g 段 profiles（组内
                // local 与账户段对齐；单组 n_groups=1 时为全量，行为不变）。
                let skip = (g as usize) * accounts_per_group;
                if skip > 0 {
                    let skip = skip.min(d.profiles.len());
                    d.profiles.drain(..skip);
                }
                d.profiles.truncate(accounts_per_group);
                // 组因子混入种子：防跨组同种子同输出。
                for (i, p) in d.profiles.iter_mut().enumerate() {
                    p.seed ^= (g as u64)
                        .wrapping_mul(0x9E3779B97F4A7C15)
                        .wrapping_add(i as u64);
                }
                (d.profiles, d.attention_gap_ticks)
            });
            let group_plans = plan_factory.as_ref().map(|f| f(g)).unwrap_or_default();
            let belief_states = {
                // 按组取段（flat 全局序 → 组内 per_group 段；尾组余位
                // 补 None 空槽）。
                let take = belief_seeds_clone
                    .drain(..accounts_per_group.min(belief_seeds_clone.len()))
                    .collect::<Vec<_>>();
                let mut seeded = take;
                seeded.resize(accounts_per_group, None);
                seeded
            };
            let group_root = root_projection
                .as_ref()
                .map(std::sync::Arc::clone);
            let mut state = super::group::GroupState::new_with_initial_tick(
                g,
                accounts,
                belief_states,
                std::sync::Arc::clone(&library_seed),
                chunk,
                config.lot_size,
                Arc::clone(&config),
                ticks_per_day,
                initial_committed_tick,
                spec.auction_ticks,
                spec.closing_auction_ticks,
                group_decision,
                group_plans,
                group_root,
                script_for(g),
            );
            // 翻转批组参数化：本组全局名 roster 偏移。
            state.bind_calendar(Arc::clone(&calendar_source), calendar_bindings.clone());
            state.roster_offset = g as usize * accounts_per_group;
            state.admin_treasury_bindings = admin_treasury_bindings.iter()
                .filter(|binding| binding.owner.0 == g).copied().collect();
            state.install_plan_seed(prepared_plans.next().expect("全部组已预检"));
            let rx = group_rxs.remove(0);
            let handle = crate::thread_facade::Builder::new()
                .name(format!("rt-group-{g}"))
                .spawn({
                    let desk_txs = desk_txs.clone();
                    let up_tx = up_tx.clone();
                    let abort = Arc::clone(&abort);
                    let shutdown = Arc::clone(&worker_shutdown);
                    let watchdog = watchdog;
                    let capture = std::sync::Arc::clone(&capture_slot);
                    move || group_main(state, desk_txs, rx, up_tx, abort, shutdown, watchdog, capture)
                })
                .map_err(|e| RuntimeError::SendFailed {
                    detail: format!("spawn rt-group-{g} 失败：{e}"),
                })?;
            handles.push((WorkerId::Group(g), handle));
        }
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
                initial_daily_bars: stock_seed_bars.remove(0),
                initial_traded_days: stock_seed_traded.remove(0),
                initial_orders: initial_desk_orders.get(s as usize).cloned().unwrap_or_default(),
                initial_recent_prices: stock_spec.initial_recent_prices,
                initial_market: stock_spec.initial_market,
                initial_public_cutoff: stock_spec.initial_public_cutoff,
                // Sol 四轮修复：恢复链序号游标贯通（空簿保留已耗序号）。
                initial_next_serial: initial_desk_next_serial
                    .get(s as usize)
                    .copied()
                    .unwrap_or(0),
            };
            let handle = crate::thread_facade::Builder::new()
                .name(format!("rt-desk-{s}"))
                .spawn({
                    let group_txs = group_txs.clone();
                    let up_tx = up_tx.clone();
                    let abort = Arc::clone(&abort);
                    let shutdown = Arc::clone(&worker_shutdown);
                    let watchdog = watchdog;
                    let config = Arc::clone(&config);
                    let capture = std::sync::Arc::clone(&capture_slot);
                    move || {
                        desk_main(
                            setup, n_groups, config, rx, group_txs, up_tx, abort, shutdown,
                            watchdog, capture,
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
        // Q1 发布槽 v0：初始公共态即第一个一致读点（tick=initial）。
        for tx in &desk_txs {
            tx.send(DeskMsg::BindCalendar {
                day: Arc::clone(&initial_calendar_day),
            })
            .map_err(|_| RuntimeError::SendFailed {
                detail: "交易部 Calendar owner 绑定失败".to_owned(),
            })?;
        }
        let global_v0 = GlobalState {
            committed_tick: initial_committed_tick,
            day: (initial_committed_tick / ticks_per_day) as u32,
            events_installed: 0,
        };
        let publication = std::sync::Arc::new(
            super::publication::ViewPublication::new(std::sync::Arc::new(
                super::publication::PublishedTick {
                    tick: initial_committed_tick,
                    generation: 0,
                    retained: std::sync::Arc::new(super::publication::RetainedIndex::default()),
                    global: global_v0,
                    desks: public_views
                        .iter()
                        .enumerate()
                        .map(|(s, view)| super::publication::DeskShard {
                            stock: s as super::StockIdx,
                            view: std::sync::Arc::new(view.clone()),
                            resting_count: 0,
                            // P2-3：v0 空 observation 面；游标=初始种子
                            ///（空簿保留已耗序号）。
                            resting: std::sync::Arc::new(Vec::new()),
                            next_serial: initial_desk_next_serial
                                .get(s)
                                .copied()
                                .unwrap_or(0),
                        })
                        .collect(),
                    groups: Vec::new(),
                    events: super::publication::EventSegment::default(),
                    hashes: super::publication::TickHashes::default(),
                },
            )),
        );
        Ok(World {
            calendar_source,
            calendar_bindings,
            tick_layout,
            pending_calendar_tick: None,
            last_committed_calendar: None,
            admin_next_id: 0,
            authority_epoch: 0,
            non_tick_revision: 0,
            pending_mirror_domains: super::source::MirrorDomains::default(),
            mirror_day_end_stage: if initial_last_day_end_tick.is_some() {
                super::source::MirrorDayEndStage::DayEndCommitted
            } else {
                super::source::MirrorDayEndStage::Tick
            },
            information_roster,
            personal_presence: None,
            #[cfg(test)]
            admin_send_fault_for_test: None,
            global: global_v0,
            report_log: Vec::new(),
            group_txs,
            desk_txs,
            up_rx,
            handles,
            abort,
            worker_shutdown,
            #[cfg(test)]
            shutdown_stall_for_test: None,
            #[cfg(test)]
            seal_ack_fail_after_acks: None,
            #[cfg(test)]
            day_end_fail_after_drain_reports: None,
            #[cfg(test)]
            day_end_fail_before_group: None,
            #[cfg(test)]
            start_cleanup_message_for_test: None,
            watchdog,
            validator: no_op_validator(),
            rounds_cap,
            n_groups,
            n_stocks,
            dead_finals: None,
            dead_reason: None,
            public_views,
            day_end_prior_public_views: None,
            rights_computer,
            ticks_per_day,
            day_end_validator: Box::new(|_| Ok(())),
            last_day_end_tick: spec.initial_last_day_end_tick,
            capture_slot,
            last_day_end_candidate: None,
            drained_lifecycle: Vec::new(),
            drained_auction: Vec::new(),
            drained_day_trades: Vec::new(),
            drained_retail: Vec::new(),
            civil_date: civil_start_date,
            publication,
            seal_ns_accumulated: 0,
            seal_first_ack_ns: 0,
            seal_ack_spread_ns: 0,
            seal_last_ack_groups: Vec::new(),
            retained_days: Vec::new(),
            retained_index: std::sync::Arc::new(super::publication::RetainedIndex::default()),
            pending_day_bars: vec![None; n_stocks],
            pending_day_candles: vec![None; n_stocks],
            publish_generation: 0,
        })
    }

    /// Q1 发布槽句柄（读者面：load() 得一致读点；句柄可长期持有）。
    pub fn publication(&self) -> std::sync::Arc<super::publication::ViewPublication> {
        std::sync::Arc::clone(&self.publication)
    }

    /// B2：取走日结 Drain 捎带的当日分钟K桶（取走即清——每日结一次）。
    /// 返回 `Vec<Option<Vec<MinuteBarFact>>>`（下标=股；None=本日结未
    /// 收集到该股应答——结构性 bug 由调用方显式失败）。
    pub fn take_pending_day_bars(
        &mut self,
    ) -> Vec<Option<Vec<super::protocol::MinuteBarFact>>> {
        let taken = std::mem::take(&mut self.pending_day_bars);
        // 取走即按股数重建空槽（mem::take 的空 Vec 会使下次日结收集失效）。
        self.pending_day_bars = vec![None; self.n_stocks];
        taken
    }

    /// 日界批：取走日结 Drain 捎带的当日终 K（取走即清——每日结一次；
    /// 与 `take_pending_day_bars` 同式）。
    pub fn take_pending_day_candles(
        &mut self,
    ) -> Vec<Option<super::protocol::DeskDayCandleFinal>> {
        let taken = std::mem::take(&mut self.pending_day_candles);
        self.pending_day_candles = vec![None; self.n_stocks];
        taken
    }

    /// 日界批：取走日结 Drain 捎带的收盘竞价成交（取走即清——驱动器
    /// 日结段消费入事件流+个人确认）。
    pub fn take_drained_day_trades(&mut self) -> Vec<super::protocol::DeskTradeFact> {
        std::mem::take(&mut self.drained_day_trades)
    }

    /// B2：保留链只读（发布/查询面同源句柄）。
    pub fn retained_days(&self) -> &[std::sync::Arc<super::publication::RetainedDaySnapshot>] {
        &self.retained_days
    }

    /// B2：冻结一日保留快照——push 链 + **原子重发布**（同 tick、
    /// generation+1：history/日期随发布原子可见；"日结全成功后原子
    /// 发布"的合同入口）。调用方（驱动器编排）负责快照内容校验。
    pub fn freeze_retained_day(
        &mut self,
        snapshot: std::sync::Arc<super::publication::RetainedDaySnapshot>,
    ) {
        if self.last_day_end_tick == Some(self.global.committed_tick) {
            self.mirror_day_end_stage = super::source::MirrorDayEndStage::RetainedPublished;
            // retained 已向读者发布，之后任何失败都不得再撤销市场日结。
            self.day_end_prior_public_views = None;
        }
        self.retained_days.push(std::sync::Arc::clone(&snapshot));
        self.retained_index = std::sync::Arc::new(super::publication::RetainedIndex {
            days: self.retained_days.clone(),
        });
        self.publish_generation += 1;
        let generation = self.publish_generation;
        self.publication.publish(std::sync::Arc::new(
            super::publication::PublishedTick {
                tick: self.global.committed_tick,
                generation,
                retained: std::sync::Arc::clone(&self.retained_index),
                global: self.global,
                desks: self.current_desk_shards_for_republish(),
                groups: Vec::new(),
                events: super::publication::EventSegment::default(),
                hashes: super::publication::TickHashes::default(),
            },
        ));
        let _ = snapshot;
    }

    /// 重发布用分片重组（B2 冻结重发——复用最近发布版本的 desk 分片
    /// 句柄，避免重拉）。从当前发布版本读取（其 tick 与 committed 一致
    /// 的不变量由调用时点保证：日结冻结发生在 install_tick 之后）。
    fn current_desk_shards_for_republish(&self) -> Vec<super::publication::DeskShard> {
        self.publication.load().desks.clone()
    }

    /// 注入 PreCommit 校验器（测试/批 3+ 接线点）。
    pub fn set_validator(&mut self, v: Validator) {
        self.validator = v;
    }

    /// Q2 存活契约测试注入（one-shot）：下一次 Seal 收尾在广播后消费
    /// consume_acks 条真实 SealAck 再合成收集失败一次（Abort 语义
    ///——worker 全存活；对照六审 300ms 延迟重试模式；Sol 复核 P1-3
    /// 注入点走真实路径）。
    #[cfg(test)]
    pub fn set_seal_ack_failure_once_for_test(&mut self, consume_acks: usize) {
        self.seal_ack_fail_after_acks = Some(consume_acks);
    }

    #[cfg(test)]
    pub fn swap_desk_sender_for_test(&mut self, stock: usize, sender: Sender<DeskMsg>) -> Sender<DeskMsg> {
        std::mem::replace(self.desk_txs.get_mut(stock).expect("测试股票索引合法"), sender)
    }

    #[cfg(test)]
    pub fn swap_group_sender_for_test(&mut self, group: usize, sender: Sender<GroupIn>) -> Sender<GroupIn> {
        std::mem::replace(self.group_txs.get_mut(group).expect("测试组索引合法"), sender)
    }

    #[cfg(test)]
    pub fn live_workers_for_test(&self) -> usize {
        self.handles.len()
    }

    #[cfg(test)]
    pub fn set_start_cleanup_message_for_test(&mut self, message: UpMsg) {
        self.start_cleanup_message_for_test = Some(message);
    }

    #[cfg(test)]
    pub fn fail_day_end_after_drain_reports_for_test(&mut self, count: usize) {
        self.day_end_fail_after_drain_reports = Some(count);
    }

    #[cfg(test)]
    pub fn fail_day_end_before_group_for_test(&mut self, group: usize) {
        self.day_end_fail_before_group = Some(group);
    }

    #[cfg(test)]
    pub fn group_ticks_for_test(&mut self) -> Vec<(GroupIdx, u64, u64)> {
        for tx in &self.group_txs {
            tx.send(GroupIn::EchoTickForTest).expect("测试组必须存活");
        }
        let mut ticks = Vec::new();
        while ticks.len() < self.n_groups {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog).expect("测试游标回声可达") {
                UpMsg::GroupTickForTest { group, tick, committed_tick } => {
                    ticks.push((group, tick, committed_tick));
                }
                other => panic!("日结撤销返回后仍残留非游标消息：{other:?}"),
            }
        }
        ticks.sort_by_key(|entry| entry.0);
        ticks
    }

    /// Seal 攻顶批 1 旋钮桩（cfg(test)）：组 0 分片构建的突变判别——
    /// ①篡改首条目 cash+1（分片内容源判别）；②丢末条目（dirty 集完整
    /// 性判别）。批 1 桩：组端未消费（无效果→旋钮测试红=注入面红先行）。
    #[cfg(test)]
    pub fn seal_knobs_for_test(&mut self, tamper: bool, skip_last: bool) {
        if let Some(tx) = self.group_txs.first() {
            let _ = tx.send(GroupIn::SetSealKnobsForTest { tamper, skip_last });
        }
    }

    /// 初始公共态是首个 Start 的决策观察基；连续竞价按配置叠加买卖笼子。
    fn initial_views(
        specs: &[DeskStockSpec],
        cage_on: bool,
        cage_applies_first_tick: bool,
    ) -> Result<Vec<StockPublic>, RuntimeError> {
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
                // 空簿时笼子参考价为最新价；买卖向公开边界均须与 desk 受理同口径。
                let min_sell = if cage_on && cage_applies_first_tick {
                    let pct = super::desk::bound_price(Money::from_cents(last), 9_800, false)
                        .map_err(|error| RuntimeError::ProtocolInvariant {
                            detail: format!("初始卖向笼子下界计算失败：{error}"),
                        })?
                        .cents();
                    down.cents().max(pct.min(last - 10).max(1))
                } else {
                    down.cents()
                };
                let max_buy = if cage_on && cage_applies_first_tick {
                    let pct = super::desk::bound_price(Money::from_cents(last), 10_200, true)
                        .map_err(|error| RuntimeError::ProtocolInvariant {
                            detail: format!("初始买向笼子上界计算失败：{error}"),
                        })?
                        .cents();
                    let ten_ticks = last.checked_add(10).ok_or_else(|| RuntimeError::ProtocolInvariant {
                        detail: format!("初始买向十个最小价位上界溢出：reference={last}"),
                    })?;
                    up.cents().min(pct.max(ten_ticks))
                } else {
                    up.cents()
                };
                Ok(StockPublic {
                    last_price: last,
                    best_bid: None,
                    best_ask: None,
                    depth_bids: Vec::new(),
                    depth_asks: Vec::new(),
                    max_buy_price: max_buy,
                    daily_upper_limit: up.cents(),
                    daily_lower_limit: down.cents(),
                    min_sell_price: min_sell,
                    recent_prices: vec![last],
                    minute_closes: Vec::new(),
                    minute_bars: std::sync::Arc::new(Vec::new()),
                daily_bars: Vec::new(),
                daily_active: None,
                daily_active_turnover_cents: 0,
                daily_active_trade_count: 0,
                daily_total_completed: 0,
                daily_total_traded: 0,
                    volume_today: 0,
                    relative_volume: 1.0,
                    order_book_imbalance: 0.0,
                    public_cutoff_serial: 0,
                })
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

    /// 测试控制面（Sol 整改注入通道）：显式强制到期指令下行（组 0——
    /// 惯例单组；组验证目标 tick 后前移 next_due）。
    #[cfg(test)]
    pub fn force_due_for_test(&mut self, locals: Vec<super::LocalIdx>, tick: u64) {
        self.force_due_group_for_test(0, locals, tick);
    }

    /// 按真实组内坐标发送到期指令；AccountId 路由由 driver 完成。
    #[cfg(test)]
    pub fn force_due_group_for_test(&mut self, group: GroupIdx, locals: Vec<super::LocalIdx>, tick: u64) {
        self.group_txs.get(group as usize).expect("注意力账户组存在")
            .send(GroupIn::ForceDue { locals, tick, ensure_observe: true }).expect("到期注入通道连通");
    }

    /// 保留明确使用组 0 局部坐标的底层 fixture 入口。
    #[cfg(test)]
    pub fn defer_due_for_test(&mut self, locals: Vec<super::LocalIdx>, tick: u64) {
        self.defer_due_group_for_test(0, locals, tick);
    }

    /// 按真实组内坐标推远 next_due；不把全局下标当 local。
    #[cfg(test)]
    pub fn defer_due_group_for_test(&mut self, group: GroupIdx, locals: Vec<super::LocalIdx>, tick: u64) {
        let next_revision = self.non_tick_revision.checked_add(1)
            .expect("DeferDueForTest non_tick_revision 已耗尽，拒绝发送");
        self.group_txs.get(group as usize).expect("注意力账户组存在")
            .send(GroupIn::DeferDueForTest { locals, tick }).expect("推远到期注入通道连通");
        self.non_tick_revision = next_revision;
        self.pending_mirror_domains.attention_full = true;
    }

    /// step_frame 接轨前置约束① fixture 通道：下行组 0 强制续行标记
    /// （cfg(test)——下一次轮上报 continuation=true，一次消费即清）。
    #[cfg(test)]
    pub fn force_continuation_for_test(&mut self) {
        if let Some(tx) = self.group_txs.first() {
            let _ = tx.send(GroupIn::ForceContinuationForTest);
        }
    }

    /// D3b 件 5：个人态注入下行组 0（cfg(test)——组权威覆盖+可逆）。
    #[cfg(test)]
    pub fn install_personal_for_test(
        &mut self,
        local: super::LocalIdx,
        snapshot: super::world::BeliefParticipantSnapshot,
    ) {
        if let Some(tx) = self.group_txs.first() {
            let _ = tx.send(GroupIn::InstallPersonal { local, snapshot });
        }
    }

    /// D3b 复审注入通道（cfg(test)）：计划事件定向下行所属组（组簿
    /// apply 权威+可逆——Abort 冲回后指令失效需重发）。
    #[cfg(test)]
    pub fn apply_plan_events_for_test(
        &mut self,
        group: GroupIdx,
        events: Vec<(crate::plans::PlanId, crate::plans::PlanEvent)>,
    ) {
        self.group_txs.get(group as usize).expect("计划事件组存在")
            .send(GroupIn::ApplyPlanEvents { events }).expect("计划事件通道连通");
    }

    /// D3b 件 4 fixture 通道（cfg(test)）：复审候选定向注入所属组（组内
    /// run_chain_decision 的复审块+开仓循环用注入值替代 observe 产物
    /// ——一次消费即清；chain_restructure 测试的 drive_once 等价面）。
    #[cfg(test)]
    pub fn inject_review_assessments_for_test(
        &mut self,
        group: GroupIdx,
        account: crate::AccountId,
        assessments: std::collections::BTreeMap<crate::StockId, crate::plans::CandidateAssessment>,
    ) {
        self.group_txs.get(group as usize).expect("复审账户组存在")
            .send(GroupIn::InjectReviewAssessments { account, assessments }).expect("复审通道连通");
    }

    /// D3b 件 4 fixture 通道（cfg(test)）：组簿测试变换定向下行所属组（手工
    /// 簿推进的组内等价——restore/record 等 API 以闭包直施组权威簿）。
    #[cfg(test)]
    pub fn mutate_plan_book_for_test(
        &mut self,
        group: GroupIdx,
        owner: crate::AccountId,
        mutate: Box<dyn FnOnce(&mut crate::plans::PlanBook) + Send>,
    ) {
        self.group_txs.get(group as usize).expect("计划账户组存在")
            .send(GroupIn::MutatePlanBookForTest { owner, mutate }).expect("计划变换通道连通");
    }

    /// D3b 件 4 fixture 通道（cfg(test)）：观察基分钟序列注入下行组 0
    ///（set_urgency_minutes 的组内等价——组观察基=observation_inputs）。
    #[cfg(test)]
    pub fn inject_observation_minutes_for_test(
        &mut self,
        stock: u16,
        minutes: Vec<(u64, i64)>,
    ) {
        if let Some(tx) = self.group_txs.first() {
            let _ = tx.send(GroupIn::InjectObservationMinutes { stock, minutes });
        }
    }

    /// D3b 件 4 fixture 通道（cfg(test)）：belief 簿测试变换下行组 0
    ///（observe_equity 等手工 belief 面直改的组内等价）。
    #[cfg(test)]
    pub fn mutate_belief_for_test(
        &mut self,
        account: crate::AccountId,
        mutate: Box<dyn FnOnce(&mut crate::strategy::BeliefBook) + Send>,
    ) {
        if let Some(tx) = self.group_txs.first() {
            let _ = tx.send(GroupIn::MutateBeliefForTest { account, mutate });
        }
    }

    /// 外部计划广播（D2 玩家意图路由：组 0 消费——玩家惯例位）。
    pub fn broadcast_external_plan(
        &mut self,
        ops: Vec<super::decision::PendingOp>,
        cancels: Vec<(super::LocalIdx, super::StockIdx, u64)>,
    ) {
        if let Some(tx) = self.group_txs.first() {
            let _ = tx.send(GroupIn::ExternalPlan { ops, cancels });
        }
    }

    /// Sol 终审修复 1c 同族：按组定向发送外部计划（多组名册——ops 的
    /// local 已是**组内**下标；空桶跳过，各组只收自己账户的操作。
    /// 单组（n_groups=1）与 broadcast_external_plan 等价）。
    pub fn send_external_plan_grouped(
        &mut self,
        ops_by_group: Vec<Vec<super::decision::PendingOp>>,
        cancels_by_group: Vec<Vec<(super::LocalIdx, super::StockIdx, u64)>>,
    ) {
        for (g, tx) in self.group_txs.iter().enumerate() {
            let ops = ops_by_group.get(g).cloned().unwrap_or_default();
            let cancels = cancels_by_group.get(g).cloned().unwrap_or_default();
            if ops.is_empty() && cancels.is_empty() {
                continue;
            }
            let _ = tx.send(GroupIn::ExternalPlan { ops, cancels });
        }
    }

    /// 计划簿收集（D3a-2c 件5：全量读面——EchoPlansFull 广播→收齐）。
    /// EveryTick 生产安装与诊断完整读取共用本面，不受 dirty 门控影响。
    pub fn collect_plans(
        &mut self,
    ) -> Result<Vec<(GroupIdx, crate::plans::PlanBook)>, RuntimeError> {
        self.collect_plans_with(|| GroupIn::EchoPlansFull, "EchoPlansFull")
    }

    /// 保留 dirty 诊断读面，未变时回空；不用于完整镜像汇合。
    pub fn collect_plans_echo(
        &mut self,
    ) -> Result<Vec<(GroupIdx, crate::plans::PlanBook)>, RuntimeError> {
        self.collect_plans_with(|| GroupIn::EchoPlans, "EchoPlans")
    }

    /// 计划簿收集共用体（请求消息由调用面构造——读面全量/生产门控；
    /// GroupIn 非 Clone，逐组以构造闭包产新值）。
    fn collect_plans_with(
        &mut self,
        make_request: impl Fn() -> GroupIn,
        request_name: &'static str,
    ) -> Result<Vec<(GroupIdx, crate::plans::PlanBook)>, RuntimeError> {
        if self.group_txs.len() != self.n_groups {
            return Err(RuntimeError::Disconnected {
                who: None,
                stage: format!(
                    "collect_plans 通道面与组数不一致（txs={} n_groups={}——世界已断连）",
                    self.group_txs.len(),
                    self.n_groups
                ),
            });
        }
        for tx in &self.group_txs {
            tx.send(make_request()).map_err(|_| {
                RuntimeError::SendFailed {
                    detail: format!("{request_name} 广播失败"),
                }
            })?;
        }
        let mut out = Vec::with_capacity(self.n_groups);
        let mut left = self.n_groups;
        let mut seen_groups = std::collections::BTreeSet::new();
        while left > 0 {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::GroupPlans { group, plans } => {
                    if group as usize >= self.n_groups || !seen_groups.insert(group) {
                        return Err(RuntimeError::ProtocolInvariant {
                            detail: format!("计划组应答重复或越界：group={group}"),
                        });
                    }
                    out.push((group, plans));
                    left -= 1;
                }
                other => return Err(prepared_stop::read_failure("collect_plans", other)),
            }
        }
        out.sort_by_key(|(g, _)| *g);
        Ok(out)
    }

    /// 自然日同步（D3a-2c：驱动器每步/日结后自会话 civil clock 同步——
    /// 快照 civil_date 的唯一来源）。
    pub fn set_civil_date(&mut self, date: crate::calendar::CivilDate) {
        self.civil_date = date;
    }

    /// 开仓派生读面收集（D3a-2c：EchoOpenDerivation 广播→收齐）。
    pub fn collect_open_derivations(
        &mut self,
    ) -> Result<
        Vec<(
            GroupIdx,
            crate::strategy::MarketView,
            crate::calendar::CivilInstant,
            Vec<super::world::OpenDerivationRecord>,
        )>,
        RuntimeError,
    > {
        if self.group_txs.len() != self.n_groups {
            return Err(RuntimeError::Disconnected {
                who: None,
                stage: format!(
                    "collect_open_derivations 通道面与组数不一致（txs={} n_groups={}——世界已断连）",
                    self.group_txs.len(),
                    self.n_groups
                ),
            });
        }
        for tx in &self.group_txs {
            tx.send(GroupIn::EchoOpenDerivation).map_err(|_| {
                RuntimeError::SendFailed { detail: "EchoOpenDerivation 广播失败".to_owned() }
            })?;
        }
        let mut out = Vec::with_capacity(self.n_groups);
        let mut left = self.n_groups;
        while left > 0 {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::GroupOpenDerivations {
                    group,
                    market,
                    now,
                    derivations,
                } => {
                    out.push((group, market, now, derivations));
                    left -= 1;
                }
                other => {
                    return Err(RuntimeError::Disconnected {
                        who: None,
                        stage: format!(
                            "collect_open_derivations 期望 GroupOpenDerivations，收到 {other:?}"
                        ),
                    });
                }
            }
        }
        out.sort_by_key(|(g, _, _, _)| *g);
        Ok(out)
    }

    /// 观察基收集（D3a-1：EchoObservations 广播→收齐——计划链端读面）。
    pub fn collect_group_observations(
        &mut self,
    ) -> Result<
        Vec<(
            GroupIdx,
            Vec<(
                crate::observation::PricePathObservation,
                crate::observation::TechnicalObservation,
            )>,
        )>,
        RuntimeError,
    > {
        if self.group_txs.len() != self.n_groups {
            return Err(RuntimeError::Disconnected {
                who: None,
                stage: format!(
                    "collect_group_observations 通道面与组数不一致（txs={} n_groups={}——世界已断连）",
                    self.group_txs.len(),
                    self.n_groups
                ),
            });
        }
        for tx in &self.group_txs {
            tx.send(GroupIn::EchoObservations).map_err(|_| {
                RuntimeError::SendFailed { detail: "EchoObservations 广播失败".to_owned() }
            })?;
        }
        let mut out = Vec::with_capacity(self.n_groups);
        let mut left = self.n_groups;
        while left > 0 {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::GroupObservations { group, observations } => {
                    out.push((group, observations));
                    left -= 1;
                }
                other => {
                    return Err(RuntimeError::Disconnected {
                        who: None,
                        stage: format!(
                            "collect_group_observations 期望 GroupObservations，收到 {other:?}"
                        ),
                    });
                }
            }
        }
        out.sort_by_key(|(g, _)| *g);
        Ok(out)
    }

    /// 信息投递编排（Belief-1b·下：稀疏——由驱动器在披露版本变化时
    /// 调用；广播→收齐 GroupDeliveryDone）。
    pub fn deliver_information(
        &mut self,
        payload: GroupInformationPayload,
    ) -> Result<Vec<(GroupIdx, Vec<(super::LocalIdx, crate::session::NpcAttentionState)>)>, RuntimeError> {
        if self.group_txs.len() != self.n_groups {
            return Err(RuntimeError::Disconnected {
                who: None,
                stage: "deliver_information 通道面与组数不一致（世界已断连）".to_owned(),
            });
        }
        if payload.attentions.len() != self.n_groups {
            return Err(RuntimeError::ProtocolInvariant {
                detail: "信息投递 attention 分组数与真实组数不同".to_owned(),
            });
        }
        // 所有分组先校验；禁止广播后才发现后组身份错误而留下部分个人写入。
        for (group, rows) in payload.attentions.iter().enumerate() {
            let roster = &self.information_roster[group];
            let mut seen = std::collections::BTreeSet::new();
            for (local, account, _) in rows {
                if roster.get(*local as usize) != Some(account) || !seen.insert(*local) {
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: format!("信息投递身份不匹配或重复：group={group} local={local} account={account:?}"),
                    });
                }
            }
        }
        for (tx, attentions) in self.group_txs.iter().zip(&payload.attentions) {
            tx.send(GroupIn::InformationDelivery {
                now: payload.now,
                day: payload.day,
                library: std::sync::Arc::clone(&payload.library),
                attentions: attentions.clone(),
            })
            .map_err(|_| RuntimeError::SendFailed {
                detail: "InformationDelivery 广播失败".to_owned(),
            })?;
        }
        let mut out = Vec::with_capacity(self.n_groups);
        let mut left = self.n_groups;
        let mut seen_groups = std::collections::BTreeSet::new();
        let expected_locals: Vec<std::collections::BTreeSet<_>> = payload.attentions.iter()
            .map(|rows| rows.iter().map(|(local, _, _)| *local).collect()).collect();
        while left > 0 {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::GroupDeliveryDone { group, updates } => {
                    let sent = expected_locals.get(usize::from(group)).ok_or_else(|| {
                        RuntimeError::ProtocolInvariant {
                            detail: format!("信息投递回执未知组：group={group}"),
                        }
                    })?;
                    if !seen_groups.insert(group) {
                        return Err(RuntimeError::ProtocolInvariant {
                            detail: format!("信息投递回执重复组：group={group}"),
                        });
                    }
                    let mut seen_locals = std::collections::BTreeSet::new();
                    for (local, _) in &updates {
                        if !sent.contains(local)
                            || !seen_locals.insert(*local)
                        {
                            return Err(RuntimeError::ProtocolInvariant {
                                detail: format!("信息投递回执 local 未请求或重复：group={group} local={local}"),
                            });
                        }
                    }
                    out.push((group, updates));
                    left -= 1;
                }
                UpMsg::Failed { who, error } => {
                    return Err(RuntimeError::WorkerFailed { who, error: Box::new(error) });
                }
                other => {
                    return Err(RuntimeError::Disconnected {
                        who: None,
                        stage: format!("deliver_information 期望 GroupDeliveryDone，收到 {other:?}"),
                    });
                }
            }
        }
        out.sort_by_key(|(g, _)| *g);
        Ok(out)
    }

    /// 信念个人态收集（D3a-2b：EchoBelief 广播→收齐）。
    /// D5 #4 retail 链：本拍零售决策 trace 收集（全组——瞬态读面）。
    pub fn collect_retail_traces(
        &mut self,
    ) -> Result<Vec<(GroupIdx, Vec<crate::session::RetailDecisionTrace>)>, RuntimeError> {
        if self.group_txs.len() != self.n_groups {
            return Err(RuntimeError::Disconnected {
                who: None,
                stage: "collect_retail_traces 通道面与组数不一致（世界已断连）".to_owned(),
            });
        }
        for tx in &self.group_txs {
            tx.send(GroupIn::EchoRetailTraces).map_err(|_| {
                RuntimeError::SendFailed { detail: "EchoRetailTraces 广播失败".to_owned() }
            })?;
        }
        let mut out = Vec::with_capacity(self.n_groups);
        let mut left = self.n_groups;
        while left > 0 {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::RetailTraces { group, traces } => {
                    out.push((group, traces));
                    left -= 1;
                }
                other => {
                    return Err(RuntimeError::Disconnected {
                        who: None,
                        stage: format!("collect_retail_traces 期望 RetailTraces，收到 {other:?}"),
                    });
                }
            }
        }
        out.sort_by_key(|(g, _)| *g);
        Ok(out)
    }

    pub fn collect_group_belief(
        &mut self,
    ) -> Result<Vec<(GroupIdx, Vec<(super::LocalIdx, BeliefParticipantSnapshot)>)>, RuntimeError>
    {
        if self.group_txs.len() != self.n_groups {
            return Err(RuntimeError::Disconnected {
                who: None,
                stage: "collect_group_belief 通道面与组数不一致（世界已断连）".to_owned(),
            });
        }
        for tx in &self.group_txs {
            tx.send(GroupIn::EchoBelief).map_err(|_| {
                RuntimeError::SendFailed { detail: "EchoBelief 广播失败".to_owned() }
            })?;
        }
        let mut out = Vec::with_capacity(self.n_groups);
        let mut left = self.n_groups;
        while left > 0 {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::GroupBelief { group, states } => {
                    // P2-2a：稀疏条目（dirty 且在册）——全量语义由驱动器
                    // 增量缓存承载（下标合法性由组端构造保证，驱动器侧
                    // get 越界走防御性跳过+缓存 None 槽位）。
                    out.push((group, states));
                    left -= 1;
                }
                other => {
                    return Err(RuntimeError::Disconnected {
                        who: None,
                        stage: format!("collect_group_belief 期望 GroupBelief，收到 {other:?}"),
                    });
                }
            }
        }
        out.sort_by_key(|(g, _)| *g);
        Ok(out)
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

    /// P2-3e：组注意力稀疏上行（生产安装段——只回本 tick 演进位；全量
    /// 读面 collect_group_attention 不变，构造恢复/Abort 测试锚）。
    pub fn collect_group_attention_sparse(
        &mut self,
    ) -> Result<Vec<(GroupIdx, Vec<(super::LocalIdx, u64, u64)>)>, RuntimeError> {
        if self.group_txs.len() != self.n_groups {
            return Err(RuntimeError::Disconnected {
                who: None,
                stage: format!(
                    "collect_group_attention_sparse 通道面与组数不一致（txs={} n_groups={}——世界已断连）",
                    self.group_txs.len(),
                    self.n_groups
                ),
            });
        }
        for tx in &self.group_txs {
            tx.send(GroupIn::EchoAttentionSparse).map_err(|_| {
                RuntimeError::SendFailed { detail: "EchoAttentionSparse 广播失败".to_owned() }
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
                        stage: format!(
                            "collect_group_attention_sparse 期望 GroupAttention，收到 {other:?}"
                        ),
                    });
                }
            }
        }
        out.sort_by_key(|(g, _)| *g);
        Ok(out)
    }

    /// 组注意力调度收集（P1-10：逐账户 next_due 上行——驱动器写回
    /// 会话 npc_attention 的真实状态源）。
    pub fn collect_group_attention(
        &mut self,
    ) -> Result<Vec<(GroupIdx, Vec<(super::LocalIdx, u64, u64)>)>, RuntimeError> {
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
                other => return Err(prepared_stop::read_failure("collect_group_attention", other)),
            }
        }
        out.sort_by_key(|(g, _)| *g);
        Ok(out)
    }

    /// D5 envelope 投影：在途单快照收集（向全部交易部发 EchoResting 并
    /// 收齐——desk 权威的费用累计/余量）。
    pub fn collect_desk_resting(
        &mut self,
    ) -> Result<(Vec<Vec<super::protocol::RestingFact>>, Vec<u64>), RuntimeError> {
        for tx in &self.desk_txs {
            tx.send(DeskMsg::EchoResting).map_err(|_| {
                RuntimeError::SendFailed {
                    detail: "EchoResting 广播失败".to_owned(),
                }
            })?;
        }
        let mut out: Vec<Option<Vec<super::protocol::RestingFact>>> =
            vec![None; self.desk_txs.len()];
        // Sol 四轮修复：每股 desk 真实 next_serial（空簿也保留已耗序号）。
        let mut cursors = vec![0u64; self.desk_txs.len()];
        let mut left = self.desk_txs.len();
        while left > 0 {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::DeskResting {
                    stock,
                    orders,
                    next_serial,
                } => {
                    cursors[stock as usize] = next_serial;
                    if out[stock as usize].replace(orders).is_some() {
                        return Err(RuntimeError::Disconnected {
                            who: None,
                            stage: format!(
                                "collect_desk_resting 收到股 {stock} 重复快照"
                            ),
                        });
                    }
                    left -= 1;
                }
                other => return Err(prepared_stop::read_failure("collect_desk_resting", other)),
            }
        }
        Ok((
            out.into_iter().map(|slot| slot.unwrap_or_default()).collect(),
            cursors,
        ))
    }

    /// 组账户快照（全量读面：调试纵切/日结 force_full——向全部组发
    /// EchoAccounts 并收齐。生产每拍回写走 snapshot_accounts_sparse）。
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
                other => return Err(prepared_stop::read_failure("snapshot_accounts", other)),
            }
        }
        out.sort_by_key(|(g, _)| *g);
        Ok(out)
    }

    /// P2-4a：组账户稀疏回声收集（生产回写段——按驱动器 dirty 集向各
    /// 组请求指名账户并收齐；回执集与请求集逐位核对，缺项/错序/多项
    /// =RuntimeError 显式失败，不静默丢账户）。请求集升序去重由调用方
    /// 保证（dirty 扫描天然有序）。
    pub fn snapshot_accounts_sparse(
        &mut self,
        wanted_per_group: &[Vec<super::LocalIdx>],
    ) -> Result<Vec<Vec<(super::LocalIdx, GroupAccount)>>, RuntimeError> {
        if self.group_txs.len() != self.n_groups {
            return Err(RuntimeError::Disconnected {
                who: None,
                stage: format!(
                    "snapshot_accounts_sparse 通道面与组数不一致（txs={} n_groups={}——世界已断连）",
                    self.group_txs.len(), self.n_groups
                ),
            });
        }
        if wanted_per_group.len() != self.group_txs.len() {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "snapshot_accounts_sparse 请求面与组数不一致（wanted={} txs={}）",
                    wanted_per_group.len(),
                    self.group_txs.len()
                ),
            });
        }
        for (g, tx) in self.group_txs.iter().enumerate() {
            tx.send(GroupIn::EchoAccountsSparse {
                wanted: wanted_per_group[g].clone(),
            })
            .map_err(|_| RuntimeError::SendFailed {
                detail: format!("EchoAccountsSparse 组 {g} 发送失败"),
            })?;
        }
        let mut out: Vec<Option<Vec<(super::LocalIdx, GroupAccount)>>> =
            vec![None; self.group_txs.len()];
        let mut left = self.group_txs.len();
        while left > 0 {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::GroupEchoSparse { group, accounts } => {
                    let wanted = &wanted_per_group[group as usize];
                    validate_sparse_echo_reply(group, wanted, &accounts)?;
                    if out[group as usize].replace(accounts).is_some() {
                        return Err(RuntimeError::Disconnected {
                            who: None,
                            stage: format!("snapshot_accounts_sparse 收到组 {group} 重复回执"),
                        });
                    }
                    left -= 1;
                }
                other => {
                    return Err(RuntimeError::Disconnected {
                        who: None,
                        stage: format!(
                            "snapshot_accounts_sparse 期望 GroupEchoSparse，收到 {other:?}"
                        ),
                    });
                }
            }
        }
        Ok(out
            .into_iter()
            .map(|slot| slot.expect("left==0 保证每组已回执"))
            .collect())
    }

    /// P2-4a：组数读面（驱动器稀疏回声请求面构建——dirty 全局位 →
    /// (组, 组内下标) 分桶）。
    pub fn n_groups(&self) -> usize {
        self.n_groups
    }

    /// 读取既有失败生命周期，调用方不得把 terminal 失败作为业务拒绝重试。
    pub(crate) fn is_terminal(&self) -> bool {
        self.dead_reason.is_some()
    }

    pub(crate) fn stage_calendar_tick(
        &mut self,
        calendar: Arc<super::MarketTickContext>,
    ) -> Result<(), RuntimeError> {
        self.validate_calendar_tick(&calendar)?;
        self.pending_calendar_tick = Some(calendar);
        Ok(())
    }

    /// None 明确表示本 authority 尚未成功提交市场拍；不得由 pending 或 restore 补造。
    pub(crate) fn committed_calendar_tick(&self) -> Result<Option<CommittedCalendarTick>, RuntimeError> {
        if let Some(reason) = &self.dead_reason {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("world 已死亡，不可读取已提交 Calendar：{reason}"),
            });
        }
        Ok(self.last_committed_calendar.clone())
    }

    pub(crate) fn staged_calendar_tick(&self) -> Option<Arc<super::MarketTickContext>> {
        self.pending_calendar_tick.as_ref().map(Arc::clone)
    }

    pub(crate) fn take_or_stage_calendar_tick(
        &mut self,
        calendar: Arc<super::MarketTickContext>,
    ) -> Result<Arc<super::MarketTickContext>, RuntimeError> {
        if let Some(staged) = &self.pending_calendar_tick {
            self.validate_calendar_tick(staged)?;
            return Ok(Arc::clone(staged));
        }
        self.stage_calendar_tick(Arc::clone(&calendar))?;
        Ok(calendar)
    }

    fn validate_calendar_tick(
        &self,
        calendar: &super::MarketTickContext,
    ) -> Result<(), RuntimeError> {
        calendar
            .validate_for(
                &self.calendar_source,
                &self.calendar_bindings,
                self.civil_date,
                self.tick_layout,
                crate::session::exchange_calendar_model::SharedClockInput {
                    tick: self.global.committed_tick,
                    completed_shared_sessions: calendar.before_input().completed_shared_sessions,
                    progress: calendar.before_input().progress,
                },
            )
            .map_err(|error| RuntimeError::ProtocolInvariant {
                detail: error.to_string(),
            })
    }

    pub(crate) fn prepare_raw_calendar_tick(
        &self,
    ) -> Result<Arc<super::MarketTickContext>, RuntimeError> {
        use crate::session::exchange_calendar_model::{SharedClockInput, SharedDayProgress};
        let fail = |error: crate::session::market_calendar_context::MarketCalendarContextError| {
            RuntimeError::ProtocolInvariant {
                detail: error.to_string(),
            }
        };
        let day = self.calendar_source.day(self.civil_date).map_err(fail)?;
        let base = day
            .day_model()
            .shared_sessions_before_date()
            .checked_mul(self.ticks_per_day)
            .ok_or_else(|| RuntimeError::ProtocolInvariant {
                detail: "raw Calendar tick base 溢出".to_owned(),
            })?;
        let completed_ticks = self
            .global
            .committed_tick
            .checked_sub(base)
            .ok_or_else(|| RuntimeError::ProtocolInvariant {
                detail: "raw Calendar tick 早于当日 base".to_owned(),
            })?;
        let progress = if day.day_model().shared_trading_on_date() {
            SharedDayProgress::Trading { completed_ticks }
        } else {
            SharedDayProgress::ClosedDay
        };
        let completed_shared_sessions = day.day_model().shared_sessions_before_date();
        day.tick(
            self.tick_layout,
            SharedClockInput {
                tick: self.global.committed_tick,
                completed_shared_sessions,
                progress,
            },
        )
        .map_err(fail)
    }

    /// 运行 n 个 tick（协调者逻辑在本调用线程执行）。
    /// panic 运输：worker panic ⇒ 本函数以原 payload `resume_unwind`（不吞不改写）。
    pub fn run_ticks(&mut self, n: u64) -> Result<u64, RuntimeError> {
        if let Some(reason) = &self.dead_reason {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("world 已死亡，不可继续跑 tick：{reason}"),
            });
        }
        // 整段 preflight 必须先于任何 Start 或日结 undo 退役。
        let day = match &self.pending_calendar_tick {
            Some(calendar) => Arc::clone(calendar.day()),
            None => self.calendar_source.day(self.civil_date).map_err(|error| {
                RuntimeError::ProtocolInvariant {
                    detail: error.to_string(),
                }
            })?,
        };
        day.validate_for(
            &self.calendar_source,
            &self.calendar_bindings,
            self.civil_date,
        )
        .map_err(|error| RuntimeError::ProtocolInvariant {
            detail: error.to_string(),
        })?;
        let base = day
            .day_model()
            .shared_sessions_before_date()
            .checked_mul(self.ticks_per_day)
            .ok_or_else(|| RuntimeError::ProtocolInvariant {
                detail: "raw tick base 溢出".to_owned(),
            })?;
        let elapsed = self
            .global
            .committed_tick
            .checked_sub(base)
            .ok_or_else(|| RuntimeError::ProtocolInvariant {
                detail: "raw tick 早于当前 civil date".to_owned(),
            })?;
        let remaining = if day.day_model().shared_trading_on_date() {
            self.ticks_per_day.checked_sub(elapsed).ok_or_else(|| {
                RuntimeError::ProtocolInvariant {
                    detail: "raw tick 已越过当前 civil date".to_owned(),
                }
            })?
        } else {
            if elapsed != 0 {
                return Err(RuntimeError::ProtocolInvariant {
                    detail: "closed raw date 的 tick 与 Calendar 不一致".to_owned(),
                });
            }
            0
        };
        if n > remaining {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!("TickRangeCrossesCivilDay: n={n}, remaining={remaining}"),
            });
        }
        for _ in 0..n {
            let calendar = match self.pending_calendar_tick.take() {
                Some(calendar) => calendar,
                None => self.prepare_raw_calendar_tick()?,
            };
            self.validate_calendar_tick(&calendar)?;
            let tick = calendar.after_input().tick;
            // 首个 Start 即可能使参与者退役日结 undo；发送失败也不能重新打开窗口。
            self.day_end_prior_public_views = None;
            // D4：tick 起点先达交易部（FIFO 保证先于同 tick Ops）——
            // 每 tick 必推进 begin_tick（到期/清算/快照不再依赖 Ops 到达）。
            for tx in &self.desk_txs {
                if tx
                    .send(DeskMsg::Start {
                        tick,
                        calendar: Arc::clone(&calendar),
                    })
                    .is_err()
                {
                    let error = RuntimeError::SendFailed {
                        detail: format!("DeskMsg::Start(tick={tick}) 广播失败（部已退出）"),
                    };
                    return Err(self.stop_after_start_failure(tick, error));
                }
            }
            if let Err(error) = broadcast_start(
                &self.group_txs,
                tick,
                Arc::clone(&calendar),
                &self.public_views,
            ) {
                return Err(self.stop_after_start_failure(tick, error));
            }
            let mut rounds = 0u32;
            // Sol 三轮修复 1（Continue 丢早轮事实）：跨轮事实累积器——
            // Continue 链上每一轮的成交/生命周期/回执/组端拒绝/竞价
            // 事实都必须进入最终 TickRecord（恰一次；此前早轮 evidence
            // 在 Continue 分支被丢弃，轮 0 的 IntentRejected/Cancelled
            // 静默消失）。
            let mut acc_trades = Vec::new();
            let mut acc_retail = Vec::new();
            #[cfg(feature = "simulation-diagnostics")]
            let mut acc_causal = Vec::new();
            #[cfg(feature = "simulation-diagnostics")]
            let mut acc_decision_traces = Vec::new();
            let mut acc_lifecycle = Vec::new();
            let mut acc_receipts = Vec::new();
            let mut acc_group_rejections = Vec::new();
            let mut acc_auction = Vec::new();
            let mut acc_entities_out = 0u64;
            loop {
                let mut evidence = self.converge_or_die(tick, rounds)?;
                if let Err(detail) = (self.validator)(&evidence) {
                    self.abort_tick(tick);
                    return Err(RuntimeError::TickValidation {
                        tick,
                        round: rounds,
                        detail,
                    });
                }
                // Sol 二审修复集 3：受理事实**逐轮**下行组（先于 Continue——
                // 身份在续行轮落账/再报价前就位；此前只在最终收束轮转发，
                // 而最终轮 evidence 已不含早轮受理——续行轮受理永不传达，
                // 无 fill 即永不 adopt，后续 Closed 释放走未受理路径）。
                // P1-A 性能修复（受理按组批发）：原逐笔×全组广播使每轮
                // 消息量=Accepted×组数（组侧 90% 条目被 owner 过滤直接
                // 丢弃），每条消息一次通道唤醒——协调者单线程满负荷成为
                // wall 关键路径（1385 万次上下文切换/3000 tick，见
                // .tmp/perf-p0-diff-profile.md）。改为按 owner 组分桶、
                // 每组每轮**至多一条**批消息；entries 保持 desk_lifecycle
                // 原序，组侧逐条走原 ChildAccepted 消费路径——时序裁决
                //（先于 Continue）与内容不变。
                let mut batches: Vec<Vec<(super::StockIdx, super::Owner, u64)>> =
                    vec![Vec::new(); self.group_txs.len()];
                for fact in &evidence.desk_lifecycle {
                    if !matches!(fact.kind, super::protocol::LifecycleKind::Accepted) {
                        continue;
                    }
                    let bucket = fact.owner.0 as usize;
                    if let Some(slot) = batches.get_mut(bucket) {
                        slot.push((fact.stock, fact.owner, fact.order_serial));
                    }
                }
                for (tx, entries) in self.group_txs.iter().zip(batches) {
                    if entries.is_empty() {
                        continue;
                    }
                    let _ = tx.send(GroupIn::AcceptedBatch { entries });
                }
                // 累积本轮事实（终轮合并进 TickRecord——早轮不丢）。
                acc_trades.extend(evidence.desk_trades.iter().copied());
                acc_retail.append(&mut evidence.retail);
                #[cfg(feature = "simulation-diagnostics")]
                acc_causal.append(&mut evidence.desk_causal);
                #[cfg(feature = "simulation-diagnostics")]
                acc_decision_traces.append(&mut evidence.decision_traces);
                acc_lifecycle.extend(evidence.desk_lifecycle.iter().copied());
                acc_receipts.extend(evidence.desk_receipts.iter().copied());
                acc_group_rejections.extend(evidence.group_rejections.iter().copied());
                acc_auction.extend(evidence.desk_auction.iter().copied());
                acc_entities_out += evidence.entities_out;
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
                    // 终轮：以累积事实替换终轮单轮快照（恰一次进
                    // TickRecord——含全部 Continue 轮）。Q2 Seal 落地后
                    // 终轮成交已随 Seal 落账（S2）——unsettled 尾巴恒空
                    //（合同 5；D3 退役的类型级表达随批 4）。

                    evidence.desk_trades = acc_trades;
                    evidence.retail = acc_retail;
                    #[cfg(feature = "simulation-diagnostics")]
                    {
                        evidence.desk_causal = acc_causal;
                        evidence.decision_traces = acc_decision_traces;
                    }
                    evidence.desk_lifecycle = acc_lifecycle;
                    evidence.desk_receipts = acc_receipts;
                    evidence.group_rejections = acc_group_rejections;
                    evidence.desk_auction = acc_auction;
                    evidence.entities_out = acc_entities_out;
                    // Q2 Seal 收尾确认：组 drain+落账（S4/S2）→SealAck
                    // 收齐 → install_tick（发布=提交=Abort 窗口关闭）。
                    let seal_shards = self.seal_round(tick, rounds)?;
                    self.install_tick(tick, rounds, evidence, seal_shards);
                    self.last_committed_calendar = Some(CommittedCalendarTick {
                        calendar: Arc::clone(&calendar),
                        source: self.mirror_source(),
                    });
                    break;
                }
            }
        }
        Ok(self.global.committed_tick)
    }

    /// Q2 Seal 收尾确认（S1-S4）：广播 Seal → 组 drain 本拍全部批次并
    /// 落账（S2 落账点唯一化）→ 收齐 G 个 SealAck。**发布=提交=Abort
    /// 窗口关闭**（S1）——本函数返回 Ok 后 install_tick 才发布。
    ///
    /// 失败语义（协调者守卫 1——存活契约）：
    /// - **收集失败/超时/断连**：`abort_tick`（tick 级 Abort——幸存
    ///   worker 回滚存活，同 tick 可重试；不触碰 worker_shutdown 永久
    ///   关停语义）。
    /// - **组端 Seal 处理失败**（Failed 上行=数据损坏类）：组 worker 已
    ///   退出——撤退收齐 Final + 标记死亡（与 converge_or_die 的
    ///   Failed 分支同式）。
    fn seal_round(
        &mut self,
        tick: u64,
        round_final: u32,
    ) -> Result<Vec<super::publication::GroupShardInput>, RuntimeError> {
        let seal_started = crate::clock::Instant::now();
        // 存活契约测试注入（one-shot，Sol 复核 P1-3 真实路径）：广播
        // **之后**在收集循环内消费 consume_acks 条真实 Ack 再合成收集
        // 失败（组已处理/正在处理 Seal——Abort 与 Seal 同队列竞态、迟到
        // Ack 落入重试收束窗均为注入面覆盖的真实形态）。
        #[cfg(test)]
        let fail_after_acks = self.seal_ack_fail_after_acks.take();
        #[cfg(not(test))]
        let fail_after_acks: Option<usize> = None;
        for tx in &self.group_txs {
            if tx.send(GroupIn::Seal { tick, round_final }).is_err() {
                self.abort_tick(tick);
                return Err(RuntimeError::SendFailed {
                    detail: format!("Seal(tick={tick}) 组广播失败（worker 已退出？）"),
                });
            }
        }
        let mut left = self.n_groups;
        // 复核建议 ④：计数仅供 cfg(test) 注入阈值消费——非测试构建
        // 不落地（免死写）。
        #[cfg(test)]
        let mut acks_seen = 0usize;
        let mut acked_groups = vec![false; self.n_groups];
        let mut seal_accounts = Vec::with_capacity(self.n_groups);
        while left > 0 {
            #[cfg(test)]
            if fail_after_acks.is_some_and(|n| acks_seen >= n) {
                self.abort_tick(tick);
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!(
                        "【测试注入】SealAck 收集失败（广播后已收 {acks_seen} Ack——Abort 语义，worker 存活可重试）"
                    ),
                });
            }
            match recv_deadline(&self.up_rx, Phase::Seal, &self.watchdog) {
                Ok(UpMsg::SealAck { group, tick: t, accounts_sparse, belief_sparse, .. }) => {
                    if t != tick {
                        self.abort_tick(tick);
                        return Err(RuntimeError::ProtocolInvariant {
                            detail: format!("SealAck 错位：期望 tick={tick}，收到组 {group} tick={t}"),
                        });
                    }
                    let Some(already) = acked_groups.get_mut(usize::from(group)) else {
                        self.abort_tick(tick);
                        return Err(RuntimeError::ProtocolInvariant {
                            detail: format!(
                                "SealAck 组号越界：{group}（组数 {}）",
                                self.n_groups
                            ),
                        });
                    };
                    // P1-3（Sol 复核）：同组重复 Ack=上一失败尝试的迟到
                    /// 竞态产物（本组本拍已 Ack——重跑确定性下值同源）；
                    /// 跳过不计（缺组由真 Ack 补齐）。
                    if std::mem::replace(already, true) {
                        continue;
                    }
                    // 地板分解：首条 Ack 记链路墙；末条记 spread 与身份。
                    if left == self.n_groups {
                        self.seal_first_ack_ns += seal_started.elapsed().as_nanos();
                    }
                    if left == 1 {
                        self.seal_ack_spread_ns += seal_started.elapsed().as_nanos();
                        // B-1' 验证批：最晚 Ack 组身份（唤醒假设直接证据）。
                        self.seal_last_ack_groups.push(u32::from(group));
                    }
                    // P1-2（Sol 复核）：账户+belief 双分片随 SealAck 上行
                    ///（belief 在 apply_deferred 后构建——含清仓经历行）。
                    seal_accounts.push(super::publication::GroupShardInput {
                        group,
                        accounts: accounts_sparse,
                        belief: belief_sparse,
                    });
                    left -= 1;
                    #[cfg(test)]
                    {
                        acks_seen += 1;
                    }
                }
                Ok(UpMsg::Failed { who, error }) => {
                    self.abort_tick(tick);
                    self.send_worker_shutdown();
                    return Err(self.stop_after_fatal_failure(
                        tick, round_final, "Seal", RuntimeError::WorkerFailed { who, error: Box::new(error) }, false, None,
                    ));
                }
                Ok(other) => {
                    self.abort_tick(tick);
                    return Err(RuntimeError::Disconnected {
                        who: None,
                        stage: format!(
                            "Seal(tick={tick}) 期望 SealAck，收到 {other:?}（worker 已提前退出？）"
                        ),
                    });
                }
                Err(e) => {
                    self.abort_tick(tick);
                    return Err(e);
                }
            }
        }
        self.seal_ns_accumulated += seal_started.elapsed().as_nanos();
        Ok(seal_accounts)
    }

    /// Start 部分发送后参与者可能分处两拍，不能对全员广播任一拍的 Abort。
    /// 永久停止并 join 后再耗尽上行：Final 只作诊断，不安装半拍或重开日结窗口。
    fn stop_after_start_failure(&mut self, tick: u64, error: RuntimeError) -> RuntimeError {
        self.stop_after_fatal_failure(tick, 0, "Start", error, true, None)
    }

    /// 失败退出统一先 join 再收上行，允许失败轮已发出的报告到达；不安装这些报告。
    /// Start 部分发送时不能统一 Abort；其他失败已先排队 Abort/Shutdown，须让其被消费。
    fn stop_after_fatal_failure(
        &mut self,
        tick: u64,
        round: u32,
        stage: &str,
        error: RuntimeError,
        stop_immediately: bool,
        original_panic: Option<(WorkerId, Box<dyn std::any::Any + Send>)>,
    ) -> RuntimeError {
        self.dead_reason = Some(format!("{stage}(tick={tick}) 失败，world 已终止且不可重试：{error}"));
        if stop_immediately {
            self.worker_shutdown.store(true, Ordering::SeqCst);
        }
        let expected_failure = match &error {
            RuntimeError::WorkerFailed { who, error } => Some((*who, error.as_ref())),
            _ => None,
        };
        let mut errors = Vec::new();
        let mut panics: Vec<(WorkerId, Box<dyn std::any::Any + Send>)> = original_panic.into_iter().collect();
        let mut worker_results = Vec::new();
        // 上行通道无界，worker 不等待协调者读 Final；先 join 可保证异常收集也不漏线程。
        for (who, handle) in std::mem::take(&mut self.handles) {
            let result = match handle.join() {
                Ok(result) => result,
                Err(payload) => {
                    // wasm join 的 io::Error 保留诊断载荷，在此统一进入失败收集。
                    #[cfg(target_family = "wasm")]
                    let payload = Box::new(payload) as Box<dyn std::any::Any + Send>;
                    panics.push((who, payload));
                    Err(RuntimeError::ProtocolInvariant {
                        detail: format!("worker {who} 未经 TaskDied 运输的线程 panic"),
                    })
                }
            };
            if let Err(failure) = &result {
                if expected_failure != Some((who, failure)) {
                    errors.push(format!("join {who}：{failure}"));
                }
            }
            worker_results.push((who, result));
        }
        let mut group_finals: Vec<Option<GroupFinalState>> = (0..self.n_groups).map(|_| None).collect();
        let mut desk_finals: Vec<Option<DeskFinalState>> = (0..self.n_stocks).map(|_| None).collect();
        let mut group_reports = vec![false; self.n_groups];
        let mut desk_reports = vec![false; self.n_stocks];
        let mut failed_count = 0;
        #[cfg(test)]
        let injected = self.start_cleanup_message_for_test.take();
        #[cfg(not(test))]
        let injected: Option<UpMsg> = None;
        for message in injected.into_iter().chain(self.up_rx.try_iter()) {
            match message {
                UpMsg::GroupFinal(final_state) => {
                    let group = usize::from(final_state.group);
                    match group_finals.get_mut(group) {
                        Some(slot) if slot.is_none() => *slot = Some(*final_state),
                        _ => errors.push(format!("GroupFinal 身份越界或重复：组 {group}")),
                    }
                }
                UpMsg::DeskFinal(final_state) => {
                    let stock = usize::from(final_state.stock);
                    match desk_finals.get_mut(stock) {
                        Some(slot) if slot.is_none() => *slot = Some(*final_state),
                        _ => errors.push(format!("DeskFinal 身份越界或重复：股 {stock}")),
                    }
                }
                UpMsg::GroupReport(report) => match group_reports.get_mut(usize::from(report.group)) {
                    Some(seen) if !*seen && report.tick == tick && report.round == round => *seen = true,
                    _ => errors.push(format!("{stage} 清理收到错位或重复 GroupReport：{report:?}")),
                },
                UpMsg::DeskReport(report) => match desk_reports.get_mut(usize::from(report.stock)) {
                    Some(seen) if !*seen && report.tick == tick && report.round == round => *seen = true,
                    _ => errors.push(format!("{stage} 清理收到错位或重复 DeskReport：{report:?}")),
                },
                UpMsg::Failed { who, error } => {
                    failed_count += 1;
                    if expected_failure != Some((who, &error)) {
                        errors.push(format!("worker {who} 显式失败：{error}"));
                    }
                }
                UpMsg::TaskDied { who, payload } => panics.push((who, payload)),
                UpMsg::SealAck { group, tick: ack_tick, .. } if stage == "Seal" && ack_tick == tick && usize::from(group) < self.n_groups => {}
                other => errors.push(format!("{stage} 清理收到非期望消息：{other:?}")),
            }
        }
        let joined_count = worker_results.len();
        let missing_groups: Vec<_> = group_finals
            .iter()
            .enumerate()
            .filter_map(|(i, f)| f.is_none().then_some(i))
            .collect();
        let missing_desks: Vec<_> = desk_finals
            .iter()
            .enumerate()
            .filter_map(|(i, f)| f.is_none().then_some(i))
            .collect();
        if !missing_groups.is_empty() || !missing_desks.is_empty() {
            errors.push(format!(
                "Final 缺口：组 {missing_groups:?}，交易部 {missing_desks:?}"
            ));
        } else {
            self.dead_finals = Some(ShutdownReport {
                group_finals: group_finals
                    .into_iter()
                    .map(|f| f.expect("已核对 Final 完整性"))
                    .collect(),
                desk_finals: desk_finals
                    .into_iter()
                    .map(|f| f.expect("已核对 Final 完整性"))
                    .collect(),
                worker_results,
                drained_minute_bars: std::mem::take(&mut self.pending_day_bars),
            });
        }
        let detail = format!(
                "{stage}(tick={tick}, round={round}) 失败，world 已终止且不可重试：{error}；已 join {} 个真实 worker；在途 GroupReport={}、DeskReport={}、Failed={}、TaskDied/线程panic={}；清理异常={errors:?}",
                joined_count,
                group_reports.iter().filter(|&&seen| seen).count(),
                desk_reports.iter().filter(|&&seen| seen).count(),
                failed_count,
                panics.len(),
            );
        self.dead_reason = Some(detail.clone());
        if !panics.is_empty() {
            // 多个 panic 只能恢复一个；全部身份与次生清理错误先显式登记。
            eprintln!(
                "{detail}；panic workers={:?}",
                panics.iter().map(|(who, _)| *who).collect::<Vec<_>>()
            );
            let (_, payload) = panics.into_iter().next().expect("已检查非空");
            std::panic::resume_unwind(payload);
        }
        if errors.is_empty() {
            match error {
                RuntimeError::SendFailed { .. } => RuntimeError::SendFailed { detail },
                other => other,
            }
        } else {
            RuntimeError::ProtocolInvariant { detail }
        }
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
                self.abort_tick(tick);
                self.send_worker_shutdown();
                let error = RuntimeError::ProtocolInvariant {
                    detail: format!("worker {who} panic（原 payload 待恢复）"),
                };
                self.stop_after_fatal_failure(tick, round, "收束", error, false, Some((who, payload)));
                unreachable!("原 panic 必须在完整清理后恢复")
            }
            RoundOutcome::Failed(error @ RuntimeError::WorkerFailed { .. }) => {
                self.abort_tick(tick);
                self.send_worker_shutdown();
                Err(self.stop_after_fatal_failure(tick, round, "收束", error, false, None))
            }
            RoundOutcome::Failed(e) => {
                self.abort_tick(tick);
                Err(e)
            }
        }
    }

    /// 终态失败的正常下行仍可达，Abort 排在 Shutdown 前，确保幸存者先回滚。
    fn send_worker_shutdown(&self) {
        for tx in &self.group_txs {
            let _ = tx.send(GroupIn::Shutdown);
        }
        for tx in &self.desk_txs {
            let _ = tx.send(DeskMsg::Shutdown);
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
    fn install_tick(
        &mut self,
        tick: u64,
        rounds: u32,
        evidence: TickEvidence,
        seal_shards: Vec<super::publication::GroupShardInput>,
    ) {
        // tick 成功转正：清除可能残留的全局中止标记（上一次 Abort 已回滚收敛）。
        self.abort.store(false, Ordering::SeqCst);
        // Q1 分片组装（发布=提交=Abort 窗口关闭——C2）：公共态 move 进
        // 广播基（public_views）的同时 clone 一份进 DeskShard（P1 v1
        // 口径：clone 税由 J1 定标，可见再 Arc 化）；resting 计数自观察面。
        let resting_of = |stock: super::StockIdx| {
            evidence
                .desk_resting
                .iter()
                .find(|(s, _)| *s == stock)
                .map(|(_, n)| *n)
                .unwrap_or(0)
        };
        // P2-3：在途单观察面+游标进分片（随报告上行——读面源）。facts
        // 按股索引 move 重排（免逐股 clone）。
        let mut resting_facts = evidence.desk_resting_facts;
        let serial_cursors = evidence.desk_serial_cursors;
        let mut desk_shards: Vec<super::publication::DeskShard> =
            Vec::with_capacity(self.public_views.len());
        for (stock, view) in evidence.desk_views {
            if let Some(slot) = self.public_views.get_mut(stock as usize) {
                // 刀 2b：resting 条目已 Arc（desk 报告直通——免整 Vec
                /// 重建包装）；view Arc 直通分片；广播基保持值拷贝（组
                /// 决策输入 &[StockPublic] 切片形态——P1 的"可见再 Arc
                /// 化"面：发布分片与收束收集双 clone 退役）。
                let facts_for_stock = match resting_facts.get_mut(stock as usize) {
                    Some(buffer) => std::sync::Arc::clone(buffer),
                    None => std::sync::Arc::new(Vec::new()),
                };
                desk_shards.push(super::publication::DeskShard {
                    stock,
                    view: std::sync::Arc::clone(&view),
                    resting_count: resting_of(stock),
                    resting: facts_for_stock,
                    next_serial: serial_cursors.get(stock as usize).copied().unwrap_or(0),
                });
                *slot = (*view).clone();
            }
        }
        // 收束到达序不定——发布分片按股序稳定排列（读者按下标=股消费）。
        desk_shards.sort_by_key(|shard| shard.stock);
        self.global.committed_tick = tick;
        self.mirror_day_end_stage = super::source::MirrorDayEndStage::Tick;
        // Sol 终审四审 A2：day 按 World 自身的 ticks_per_day 推导（此前
        // 硬编码 240——非 240 配置（如 tpd=1 的存档契约 fixture）下组
        // 侧日历超前，session 日终计划迁移的 TradingDayEnded 落后于
        // plan 事件 day，precedes 校验显红（save_contract 38 败同根因）。
        self.global.day = ((tick - 1) / self.ticks_per_day) as u32;
        self.global.events_installed += evidence.entities_out;
        let group_hashes = evidence.group_hashes;
        let desk_hashes = evidence.desk_hashes;
        let record = TickRecord {
            #[cfg(feature = "simulation-diagnostics")]
            decision_traces: evidence.decision_traces,
            tick,
            rounds,
            group_hashes: group_hashes.clone(),
            desk_hashes: desk_hashes.clone(),
            desk_resting: evidence.desk_resting,
            desk_trades: evidence.desk_trades,
            retail: evidence.retail,
            #[cfg(feature = "simulation-diagnostics")]
            desk_causal: evidence.desk_causal,
            desk_lifecycle: evidence.desk_lifecycle,
            desk_receipts: evidence.desk_receipts,
            group_rejections: evidence.group_rejections,
            desk_auction: evidence.desk_auction,
        };
        self.report_log.push(record);
        // Q1 单点发布（唯一 publish 调用点）：组装不可变版本并原子交换。
        // P1-2（Sol 复核）：groups 分片=SealAck 双分片（账户+belief——
        /// 均为 Seal 落账后构建，含清仓经历行）；events 占位（事件迁移
        /// 时填充）。
        let group_shards: Vec<super::publication::GroupShard> = seal_shards
            .into_iter()
            .map(|input| {
                super::publication::GroupShard {
                    committed_hash: group_hashes
                        .iter()
                        .find(|(g, _)| *g == input.group)
                        .map(|(_, h)| *h)
                        .unwrap_or(0),
                    group: input.group,
                    belief: input.belief,
                    accounts: input.accounts,
                }
            })
            .collect();
        self.publish_generation += 1;
        let generation = self.publish_generation;
        self.publication.publish(std::sync::Arc::new(
            super::publication::PublishedTick {
                tick,
                generation,
                retained: std::sync::Arc::clone(&self.retained_index),
                global: self.global,
                desks: desk_shards,
                groups: group_shards,
                events: super::publication::EventSegment::default(),
                hashes: super::publication::TickHashes {
                    groups: group_hashes,
                    desks: desk_hashes,
                },
            },
        ));
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
        let day = self.calendar_source.day(self.civil_date).map_err(|error| RuntimeError::ProtocolInvariant {
            detail: error.to_string(),
        })?;
        self.run_day_end_with_calendar(super::protocol::DayEndCalendar::ManualDayEnd(day))
    }

    /// 共用日结核心；所有来源与纯时钟投影校验必须先于 Drain 或参与者消息。
    pub(crate) fn run_day_end_with_calendar(
        &mut self,
        calendar: super::protocol::DayEndCalendar,
    ) -> Result<(), RuntimeError> {
        use crate::session::exchange_calendar_model::{calendar_clock_projection, SharedDayProgress};
        let fail = |detail: String| RuntimeError::ProtocolInvariant { detail };
        if let Some(reason) = &self.dead_reason {
            return Err(fail(format!("world 已死亡，不可继续日结：{reason}")));
        }
        let tick = self.global.committed_tick;
        if tick == 0 {
            return Err(fail("DayEnd 缺少真实已提交 tick".to_owned()));
        }
        if self.last_day_end_tick == Some(tick) {
            return Err(fail(format!("交易日已日结（收尾 tick={tick}）——不得重复日结")));
        }
        let day = calendar.day();
        day.validate_for(&self.calendar_source, &self.calendar_bindings, self.civil_date)
            .map_err(|error| fail(error.to_string()))?;
        let sessions = tick.checked_div(self.ticks_per_day)
            .ok_or_else(|| fail("DayEnd ticks_per_day 无效".to_owned()))?;
        let input = crate::session::calendar_clock_input(day, self.tick_layout, tick, sessions)
            .map_err(|error| fail(error.to_string()))?;
        calendar_clock_projection(day.day_model(), self.tick_layout, input)
            .map_err(|error| fail(error.to_string()))?;

        if let super::protocol::DayEndCalendar::MarketTick(context) = &calendar {
            let committed = self.committed_calendar_tick()?
                .ok_or_else(|| fail("市场 DayEnd 缺少本 authority 已提交原始 TickArc".to_owned()))?;
            context.validate_for(&self.calendar_source, &self.calendar_bindings, self.civil_date,
                self.tick_layout, context.before_input()).map_err(|error| fail(error.to_string()))?;
            if !Arc::ptr_eq(context, &committed.calendar)
                || committed.source.authority_epoch != self.authority_epoch
                || committed.source.tick != tick || committed.source.civil_date != self.civil_date
                || context.after_input().tick != tick
                || !matches!(context.after_input().progress, SharedDayProgress::TradingCompleted) {
                return Err(fail("市场 DayEnd 原始已提交 Calendar 身份或末拍进度不一致".to_owned()));
            }
        }
        self.run_day_end(tick, &calendar)
    }

    /// 只撤 F0 的未发布日结参与者；普通 tick 和 retained 发布后的静点均不可撤。
    pub(crate) fn abort_unpublished_day_end(&mut self) {
        if self.dead_reason.is_none()
            && self.mirror_day_end_stage == super::source::MirrorDayEndStage::DayEndCommitted
            && self.day_end_prior_public_views.is_some()
        {
            self.abort_day_end();
        }
    }

    /// 撤销一次**已成功**的日结（Sol 二审修复集 2）：广播 Abort——组按
    /// 日结隔离日志冲回（解冻/权益/Drain 落账），部冲回清簿撤单，回到
    /// 上一交易日收敛态；日结候选一并作废。仅在"日结成功但编排后续
    /// 步骤（如 civil 推进）失败"时调用——此时组/部可逆日志仍存活
    /// （下一 Start 前的 Abort 窗口内），冲回后可重跑日结。
    pub fn abort_day_end(&mut self) {
        // 无待裁决日结（含下一 Start 已关闭窗口）时无事可撤，不触碰当前 tick。
        let Some(prior) = self.day_end_prior_public_views.take() else {
            return;
        };
        self.public_views = prior;
        let tick = self.global.committed_tick;
        let mut progress = DayEndProgress {
            desks_sent: self.n_stocks,
            groups_sent: self.n_groups,
            desks_reported: vec![true; self.n_stocks],
            groups_reported: vec![true; self.n_groups],
        };
        if let Err(error) = self.rollback_day_end_participants(tick, &mut progress) {
            self.dead_reason = Some(format!("日结撤销未完整确认：{error}"));
            panic!("日结撤销失败，world 已停止：{error}");
        }
        // Sol 六审收官复核阻断 4：日结 Drain 期间上行的事实缓冲随冲回
        /// 清空——Abort 重跑会重新 Drain 产生同名事实，残留使缓冲
        /// 双载（竞价事实 2→4 条实测）。
        self.drained_lifecycle.clear();
        self.drained_auction.clear();
        // 日界批：drain 成交/终 K 同步作废（回滚对称——重跑重新收集）。
        self.drained_day_trades.clear();
        self.drained_retail.clear();
        for slot in &mut self.pending_day_candles {
            *slot = None;
        }
        // B2：日结 Drain 捎带的桶同步作废（重跑日结重新收集——回滚对称）。
        for slot in &mut self.pending_day_bars {
            *slot = None;
        }
        self.last_day_end_candidate = None;
        self.last_day_end_tick = None;
        self.mirror_day_end_stage = super::source::MirrorDayEndStage::Tick;
    }

    fn run_day_end(&mut self, day_final_tick: u64, calendar: &super::protocol::DayEndCalendar) -> Result<(), RuntimeError> {
        // Sol 收官再复核阻断 4：日结序列任一失败路径（Drain/组翻/校验）
        /// 都必须清空 Drain 期间上行的事实缓冲——重跑会重新 Drain 产生
        /// 同名事实，残留使缓冲双载（此前仅 abort_day_end 清，run_day_end
        /// 内部失败绕过）。
        let mut progress = DayEndProgress {
            desks_sent: 0,
            groups_sent: 0,
            desks_reported: vec![false; self.n_stocks],
            groups_reported: vec![false; self.n_groups],
        };
        let mut result = self.run_day_end_inner(day_final_tick, calendar, &mut progress);
        if let Err(error) = &result {
            if let Err(rollback_error) = self.rollback_day_end_participants(day_final_tick, &mut progress) {
                let detail = format!("日结失败且撤销未完整确认：{error}；{rollback_error}");
                self.dead_reason = Some(detail.clone());
                result = Err(RuntimeError::ProtocolInvariant { detail });
            } else if matches!(error, RuntimeError::SendFailed { .. } | RuntimeError::WorkerFailed { .. }) {
                self.dead_reason = Some(format!("日结参与者断连，不可继续运行：{error}"));
            }
            self.drained_lifecycle.clear();
            self.drained_auction.clear();
            self.drained_day_trades.clear();
            self.drained_retail.clear();
            // 复核建议 S1：终 K 暂存对称清理（良性缺口——下次成功 Drain
            /// 全槽覆写、take 仅在成功后发生；此处补齐与 abort_day_end 同式）。
            for slot in &mut self.pending_day_candles {
                *slot = None;
            }
            for slot in &mut self.pending_day_bars {
                *slot = None;
            }
        }
        result
    }

    fn rollback_day_end_participants(
        &mut self,
        tick: u64,
        progress: &mut DayEndProgress,
    ) -> Result<(), RuntimeError> {
        if progress.desks_sent == 0 {
            return Ok(());
        }
        self.abort.store(true, Ordering::SeqCst);
        let desks: Vec<_> = (0..progress.desks_sent)
            .map(|stock| WorkerId::Desk(stock as StockIdx))
            .collect();
        for stock in 0..progress.desks_sent {
            self.desk_txs[stock].send(DeskMsg::AbortDayEnd { tick }).map_err(|_| RuntimeError::SendFailed {
                detail: format!("交易部 {stock} 日结撤销发送失败"),
            })?;
        }
        self.wait_day_end_aborted(tick, &desks, progress)?;
        // desk 的真实确认晚于全部 Drain Batch，保证组撤销后不会再收到旧日结资源。
        let groups: Vec<_> = (0..self.n_groups)
            .map(|group| WorkerId::Group(group as GroupIdx))
            .collect();
        for (group, tx) in self.group_txs.iter().enumerate() {
            tx.send(GroupIn::AbortDayEnd { tick }).map_err(|_| RuntimeError::SendFailed {
                detail: format!("组 {group} 日结撤销发送失败"),
            })?;
        }
        self.wait_day_end_aborted(tick, &groups, progress)?;
        self.abort.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn wait_day_end_aborted(
        &mut self,
        tick: u64,
        expected: &[WorkerId],
        progress: &mut DayEndProgress,
    ) -> Result<(), RuntimeError> {
        let mut received = vec![false; expected.len()];
        let mut left = expected.len();
        while left > 0 {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::DayEndAborted { who, tick: actual } => {
                    let index = expected.iter().position(|worker| *worker == who)
                        .ok_or_else(|| RuntimeError::ProtocolInvariant {
                            detail: format!("日结撤销收到非本阶段参与者确认：{who}"),
                        })?;
                    if actual != tick || std::mem::replace(&mut received[index], true) {
                        return Err(RuntimeError::ProtocolInvariant {
                            detail: format!("日结撤销确认错位或重复：{who} tick={actual}，期望={tick}"),
                        });
                    }
                    left -= 1;
                }
                UpMsg::DeskDrained { stock, .. } => {
                    let index = usize::from(stock);
                    if index >= progress.desks_sent || progress.desks_reported[index] {
                        return Err(RuntimeError::ProtocolInvariant {
                            detail: format!("日结撤销收到未知或重复 DeskDrained：stock={stock}"),
                        });
                    }
                    progress.desks_reported[index] = true;
                }
                UpMsg::GroupDayDone { group, day, .. } => {
                    let index = usize::from(group);
                    if index >= progress.groups_sent
                        || progress.groups_reported[index]
                        || u64::from(day) != self.calendar_source.day(self.civil_date)
                            .map_err(|error| RuntimeError::ProtocolInvariant { detail: error.to_string() })?
                            .day_model().shared_sessions_before_date()
                    {
                        return Err(RuntimeError::ProtocolInvariant {
                            detail: format!("日结撤销收到未知、错位或重复 GroupDayDone：group={group}, day={day}"),
                        });
                    }
                    progress.groups_reported[index] = true;
                }
                UpMsg::Failed { who, error } => return Err(RuntimeError::WorkerFailed { who, error: Box::new(error) }),
                other => return Err(RuntimeError::ProtocolInvariant {
                    detail: format!("日结撤销期望 actor 完成确认，收到 {other:?}"),
                }),
            }
        }
        Ok(())
    }

    fn run_day_end_inner(&mut self, day_final_tick: u64, calendar: &super::protocol::DayEndCalendar, progress: &mut DayEndProgress) -> Result<(), RuntimeError> {
        let day = u32::try_from(calendar.day().day_model().shared_sessions_before_date())
            .map_err(|_| RuntimeError::ProtocolInvariant { detail: "DayEnd 共享 day 溢出".to_owned() })?;
        // ① Drain。
        for tx in &self.desk_txs {
            tx.send(DeskMsg::Drain)
                .map_err(|_| RuntimeError::SendFailed {
                    detail: "DayEnd Drain 广播失败".to_owned(),
                })?;
            progress.desks_sent += 1;
        }
        let mut desks: Vec<(StockIdx, i64, i64)> = Vec::with_capacity(self.n_stocks);
        let mut drained_views = vec![None; self.n_stocks];
        let mut drained = 0usize;
        let mut retail_fragments = Vec::new();
        while drained < self.n_stocks {
            #[cfg(test)]
            if self.day_end_fail_after_drain_reports == Some(drained) {
                self.day_end_fail_after_drain_reports = None;
                return Err(RuntimeError::ProtocolInvariant {
                    detail: format!("【测试注入】收到 {drained} 个真实 DeskDrained 后收集失败"),
                });
            }
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::DeskDrained {
                    stock,
                    fee_sink,
                    last_price,
                    drained_view,
                    drained_lifecycle,
                    drained_auction,
                    drained_minute_bars,
                    drained_trades,
                    drained_retail,
                    drained_day_candle,
                } => {
                    let slot = drained_views.get_mut(stock as usize).ok_or_else(|| {
                        RuntimeError::ProtocolInvariant {
                            detail: format!(
                                "日结 DeskDrained 股索引越界：stock={stock}, n_stocks={}",
                                self.n_stocks
                            ),
                        }
                    })?;
                    if slot.replace(drained_view).is_some() {
                        return Err(RuntimeError::ProtocolInvariant {
                            detail: format!("日结 DeskDrained 重复股索引：stock={stock}"),
                        });
                    }
                    progress.desks_reported[usize::from(stock)] = true;
                    desks.push((stock, fee_sink, last_price));
                    // B2：当日桶暂存（驱动器编排冻结的消费面）。
                    if let Some(slot) = self.pending_day_bars.get_mut(stock as usize) {
                        *slot = Some(drained_minute_bars);
                    }
                    // 日界批：当日终 K 暂存（install 面补真值的消费面）。
                    if let Some(slot) = self.pending_day_candles.get_mut(stock as usize) {
                        *slot = drained_day_candle;
                    }
                    self.drained_lifecycle.extend(drained_lifecycle);
                    // Sol 六审段 1.2：日结收盘竞价事实（末拍清算事件源）。
                    self.drained_auction.extend(drained_auction);
                    // 日界批：收盘竞价成交（事件流+receipt 消费面）。
                    let context = super::retail_lifecycle::BatchContext::Drain {
                        ended_date: self.civil_date, final_tick: self.global.committed_tick, stock,
                    };
                    let facts = super::retail_lifecycle::resolve_report(context, drained_retail, &drained_trades)?;
                    if !facts.is_empty() { retail_fragments.push((stock, facts)); }
                    self.drained_day_trades.extend(drained_trades);
                    drained += 1;
                }
                UpMsg::Failed { who, error } => {
                    return Err(RuntimeError::WorkerFailed {
                        who,
                        error: Box::new(error),
                    });
                }
                other => {
                    return Err(RuntimeError::Disconnected {
                        who: None,
                        stage: format!("DayEnd Drain 期望 DeskDrained，收到 {other:?}"),
                    });
                }
            }
        }
        retail_fragments.sort_unstable_by_key(|(stock, _)| *stock);
        self.drained_retail.extend(retail_fragments.into_iter().flat_map(|(_, facts)| facts));
        // ② 组日翻转 + 权益落账（可逆）。
        for g in 0..self.n_groups as GroupIdx {
            #[cfg(test)]
            if self.day_end_fail_before_group == Some(g as usize) {
                self.day_end_fail_before_group = None;
                let (tx, rx) = std::sync::mpsc::channel();
                drop(rx);
                tx.send(GroupIn::DayEnd(Box::new(DayEndDirective {
                    tick: day_final_tick,
                    civil_date: self.civil_date,
                    calendar: calendar.clone(),
                    day,
                    rights: Vec::new(),
                }))).map_err(|_| RuntimeError::SendFailed {
                    detail: format!("【测试注入】第 {g} 组真实断连 Sender 无法发送 DayEnd"),
                })?;
            }
            let rights = self
                .rights_computer
                .as_ref()
                .map(|f| f(day, g))
                .unwrap_or_default();
            self.group_txs[g as usize]
                .send(GroupIn::DayEnd(Box::new(DayEndDirective {
                    tick: day_final_tick,
                    civil_date: self.civil_date,
                    calendar: calendar.clone(),
                    day,
                    rights,
                })))
                .map_err(|_| RuntimeError::SendFailed {
                    detail: "DayEnd 广播失败".to_owned(),
                })?;
            progress.groups_sent += 1;
        }
        let mut groups: Vec<(GroupIdx, Vec<GroupAccount>)> = Vec::with_capacity(self.n_groups);
        let mut group_hashes: Vec<(GroupIdx, u64)> = Vec::with_capacity(self.n_groups);
        let mut done = 0usize;
        while done < self.n_groups {
            match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                UpMsg::GroupDayDone {
                    group,
                    day: actual_day,
                    accounts,
                    state_hash,
                } => {
                    let index = usize::from(group);
                    if actual_day != day || index >= self.n_groups || std::mem::replace(&mut progress.groups_reported[index], true) {
                        return Err(RuntimeError::ProtocolInvariant {
                            detail: format!("日结收到未知、错位或重复组结果：group={group}, day={actual_day}"),
                        });
                    }
                    groups.push((group, accounts));
                    group_hashes.push((group, state_hash));
                    done += 1;
                }
                UpMsg::Failed { who, error } => {
                    return Err(RuntimeError::WorkerFailed {
                        who,
                        error: Box::new(error),
                    });
                }
                other => {
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
            return Err(RuntimeError::DayEndValidation { day, detail });
        }
        let next_views = drained_views
            .into_iter()
            .enumerate()
            .map(|(stock, view)| {
                view.map(|view| (*view).clone())
                    .ok_or_else(|| RuntimeError::ProtocolInvariant {
                        detail: format!("日结 DeskDrained 缺失股票：stock={stock}"),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        // 所有 desk 完整、唯一且校验通过后一次安装；旧基准保留到下一 Start。
        // PublishedTick 仍是上一成功 tick 的完整读点，不混装日结账户/事实/行情。
        self.day_end_prior_public_views = Some(std::mem::replace(&mut self.public_views, next_views));
        // 成功：候选不可变交出；下一 Start 隐式提交（Abort 窗口关闭）。
        self.last_day_end_candidate = Some(candidate);
        self.last_day_end_tick = Some(day_final_tick);
        self.mirror_day_end_stage = super::source::MirrorDayEndStage::DayEndCommitted;
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
        // Sol 终审四审 A1：正常关停**不置**永久标记——Drain/Shutdown/
        /// Final 是完整消息协议（组在 Shutdown 消息处 apply_staging 后
        /// 上交 Final）；标记驱退会旁路 Final 上交（空闲 worker 50ms
        /// 提前退出，特定交错漏合入回流实体）。标记仅用于通道不可达
        /// 场景（sever/Drop）。消息协议失败时 Drop 兜底仍回收。
        if let Some(report) = self.dead_finals.take() {
            // 已在失败撤退路径收齐 Final：只补 join。
            self.join_workers(report)
        } else {
            if let Some(reason) = &self.dead_reason {
                if self.handles.is_empty() {
                    return Err(RuntimeError::ProtocolInvariant {
                        detail: format!("world 已终止且 Final 不完整，不能再次 Drain：{reason}"),
                    });
                }
            }
            // 成功日结（含恢复）已完成清簿与完整结果收集；无新成功 tick 时复用。
            // AbortDayEnd 清标，新 tick 成功推进 committed_tick，二者均恢复真实 Drain。
            if self.last_day_end_tick != Some(self.global.committed_tick) {
                for tx in &self.desk_txs {
                    tx.send(DeskMsg::Drain)
                        .map_err(|_| RuntimeError::SendFailed {
                            detail: "Drain 广播失败".to_owned(),
                        })?;
                }
                let mut drained = 0usize;
                while drained < self.n_stocks {
                    match recv_deadline(&self.up_rx, Phase::Drain, &self.watchdog)? {
                        UpMsg::DeskDrained {
                            stock,
                            drained_minute_bars,
                            ..
                        } => {
                            // B2：期末 Drain 应答同样收集桶（观察面与日结统一；
                            /// DeskFinal 的 live 桶在跨日形态已并入应答）。
                            if let Some(slot) = self.pending_day_bars.get_mut(stock as usize) {
                                *slot = Some(drained_minute_bars);
                            }
                            drained += 1;
                        }
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
            }
            // Sol 五审再复核 P1-2：组 Shutdown 广播前的受控停顿（仅测试
            /// 装配；复现四审竞态窗——若重引"关停前置永久标记"，空闲组
            /// 在此窗内被标记驱退提前退出、Shutdown 消息不被处理）。
            #[cfg(test)]
            if let Some(stall) = self.shutdown_stall_for_test {
                std::thread::sleep(stall);
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
        let drained_minute_bars = std::mem::take(&mut self.pending_day_bars);
        Ok(ShutdownReport {
            drained_minute_bars,
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
        // Sol 终审三审：置永久关停（worker_recv 分片窗口内观察退出）——
        /// sever 后下行断连互等（组↔部互持 clone），消息不可达只能走标记。
        self.worker_shutdown.store(true, Ordering::SeqCst);
        self.group_txs.clear();
        self.desk_txs.clear();
    }
}

impl Drop for World {
    /// D0 兜底关停：显式 shutdown 之外的丢弃路径（如 session 析构带走
    /// 常驻 driver）也必须回收组/部线程——发 Shutdown（失败=已退出）并
    /// join（结果忽略：worker 错误本经上行通道报告，Drop 期无处报告，
    /// 此处只保证线程回收）。
    /// Sol 终审二审阻断 2：**先置全局中止标记再 join**——sever 后下行
    /// sender 已全部 drop（Shutdown 不可达），worker 的 recv 断连分支
    /// 本可立即退出，但处于超时等待中的 worker（正常档 5s/测试注入
    /// 档更长）须凭 abort 标记在超时点干净退出（worker_recv 的
    /// Timeout+abort 分支），否则 Drop 的 join 被拖到看门狗满档——
    /// sol3_install_candles 的 10 秒硬上限实测击穿。断连检测语义
    /// 不变（P1 时代的通道面一致性守卫——send 侧显式报错不受影响）。
    fn drop(&mut self) {
        self.worker_shutdown.store(true, Ordering::SeqCst);
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

/// P2-4a：稀疏账户回声回执校验（自由函数——收集端逐位核对）。
/// 回执必须与请求集**逐位一致**（同长、同序、同下标）：缺项=镜像静默
/// 陈旧（铁律二禁止），多项/重复/乱序=协议漂移，一律 RuntimeError。
pub(in crate::runtime) fn validate_sparse_echo_reply(
    group: GroupIdx,
    requested: &[super::LocalIdx],
    reply: &[(super::LocalIdx, GroupAccount)],
) -> Result<(), RuntimeError> {
    if requested.len() != reply.len() {
        let missing: Vec<super::LocalIdx> = requested
            .iter()
            .filter(|i| !reply.iter().any(|(r, _)| r == *i))
            .copied()
            .collect();
        return Err(RuntimeError::ProtocolInvariant {
            detail: format!(
                "组 {group} 稀疏回声回执与请求集不一致：请求 {} 项，回执 {} 项，缺项 {missing:?}",
                requested.len(),
                reply.len()
            ),
        });
    }
    for (slot, (want, (got, _))) in requested.iter().zip(reply.iter()).enumerate() {
        if want != got {
            return Err(RuntimeError::ProtocolInvariant {
                detail: format!(
                    "组 {group} 稀疏回声回执序错位：第 {slot} 位请求 {want} 回执 {got}"
                ),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "group_roster_consumer_tests.rs"]
mod group_roster_consumer_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod failed_cleanup_tests {
    use super::*;
    use crate::runtime::decision::PendingOp;
    use crate::runtime::group::{PlaceSpec, TickPlan};
    use crate::runtime::protocol::RoutedPrice;
    use crate::runtime::skeleton_tests::basic_spec;
    use crate::Side;

    #[test]
    fn failed_desk_keeps_original_error_and_joins_with_in_flight_group_report() {
        let mut spec = basic_spec(2, 1, 1, Box::new(|group| {
            Box::new(move |tick, _| {
                let mut plan = TickPlan::default();
                if tick == 1 && group == 0 {
                    plan.places.push(PlaceSpec {
                        local: 0, stock: 0, side: Side::Buy, price: 900, qty: 100,
                    });
                }
                plan
            })
        }));
        let mut config = GameConfig::proposed_defaults();
        config.price_cage_enabled = false;
        spec.config = Arc::new(config);
        let mut world = World::spawn(
            spec,
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .expect("构造两组真实线程");
        world.run_ticks(1).expect("先发布真实扣款 tick1");
        let before = world.snapshot_accounts().expect("已发布账户");
        let publication = world.publication();
        let published = publication.load();
        // 模拟旧行情导致的错误预留：受理价 1100 分，组端只预留 1000 分×100股。
        world.broadcast_external_plan(vec![PendingOp {
            local: 0, stock: 0, side: Side::Buy,
            routed: RoutedPrice::Fixed(1100), purse_reference: 1000, qty: 100,
        }], vec![]);
        // 只置换真实上行的交错：先交付交易部 Failed，再交付同轮真实 GroupReport。
        // 不合成错误或报告，避免依赖 OS 调度偶然命中历史撤退窗口。
        let (relay_tx, relay_rx) = channel();
        let original_rx = std::mem::replace(&mut world.up_rx, relay_rx);
        let relay = std::thread::spawn(move || {
            let mut pending = Vec::new();
            let failure = loop {
                let message = original_rx.recv_timeout(Duration::from_secs(2)).expect("真实交易部失败应有限到达");
                if matches!(&message, UpMsg::Failed { who: WorkerId::Desk(0), .. }) {
                    break message;
                }
                pending.push(message);
            };
            while !pending.iter().any(|m| matches!(m, UpMsg::GroupReport(_))) {
                pending.push(original_rx.recv_timeout(Duration::from_secs(2)).expect("真实组报告应有限到达"));
            }
            relay_tx.send(failure).expect("失败接收端存活");
            for message in pending {
                if relay_tx.send(message).is_err() { return; }
            }
            while let Ok(message) = original_rx.recv() {
                if relay_tx.send(message).is_err() { return; }
            }
        });
        let error = world.run_ticks(1).expect_err("低于精确需求必须显式失败");
        let live_workers = world.handles.len();
        let same_publication = Arc::ptr_eq(&published, &publication.load());
        let committed_tick = world.global.committed_tick;
        let dead = world.run_ticks(1).expect_err("死亡世界不能重试");
        let shutdown = world.shutdown();
        relay.join().expect("上行置换线程退出");
        assert!(matches!(&error, RuntimeError::WorkerFailed { who: WorkerId::Desk(0), error }
            if matches!(error.as_ref(), RuntimeError::ProtocolInvariant { detail }
                if detail.contains("purse=100501, required=110501"))), "保留原交易部错误：{error:?}");
        assert_eq!(live_workers, 0, "失败返回前 join 全部真实线程");
        assert!(same_publication, "失败不发布半拍");
        assert_eq!(committed_tick, 1, "已发布 tick1 不得被撤销");
        assert!(matches!(dead, RuntimeError::ProtocolInvariant { detail } if detail.contains("world 已死亡")));
        let finals = shutdown.expect("失败撤退应缓存完整 Final");
        assert_eq!(finals.group_finals.len(), 2);
        assert_eq!(finals.desk_finals.len(), 1);
        assert_eq!(finals.worker_results.len(), 3);
        assert_eq!(finals.group_finals.iter().map(|g| (g.group, g.accounts.clone())).collect::<Vec<_>>(), before,
            "只回滚失败 tick2，保留 tick1 的真实扣款");
        assert_eq!(finals.desk_finals[0].resting.len(), 1, "tick1 已发布挂单仍在诊断 Final");
    }
}

mod admin_transaction;

pub use super::protocol::{AdminCandidate, AdminOperation, AdminTransactionId, AdminTreasuryBinding, InformationObservation, InformationPreparePayload, PreparedInformationResult, InformationPrepareError};

mod admin_treasury_binding;
