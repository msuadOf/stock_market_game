//! Q1 版化发布协议（方案 A·RwLock 先行版）——跨线程一致读点。
//!
//! 设计依据：`agents/std-thread-migration/plans/view-design-publication.md`
//! （方案 A 推荐：不可变快照+原子交换、单点组装；终裁 C1/C2 约束）。
//!
//! 语义合同（本模块的承诺边界）：
//! - **发布点唯一**：`install_tick` 末尾（全局提交裁决后）是唯一 publish
//!   调用点——失败 tick 的 candidate 分片从不进槽（C2：发布=提交=Abort
//!   窗口关闭）。
//! - **一致性构造性保证**：一个 `Arc<PublishedTick>` 内含全部分片句柄+
//!   全局域——读者一次 `load()` clone 即得同拍一致读点，不存在"账户 N 拍
//!   +盘口 N+1 拍"的撕裂窗口（J4 判据的违约计数恒 0 是结构性质）。
//! - **生命周期**：`Arc` 独立计数——读者持旧版本任意久，旧版本随最后
//!   读者释放；发布槽只持 1 代 current（无隐藏多代，J3 判据）。
//! - **零 String**：全数值标识（engine 无字符串纪律）。
//!
//! P1 边界（如实登记，P2 扩充）：
//! - `groups` 分片为占位——Echo 链拆迁（D1）时由组线程收尾构建随
//!   `GroupReport` 上行填充（账户投影/计划进度/个人态观察面）。
//! - `events` 段为占位——事件投影迁移（R4 语义不变）时填充；当前事件
//!   组装仍在驱动器 step 内（report_log 事实投影）。
//! - 存储槽为 `std::sync::RwLock<Arc<PublishedTick>>`（零新依赖先行版）；
//!   J2 数据若显示读侧争用，升级 `arc-swap` 走 ADR。

use super::coordinator::GlobalState;
use super::protocol::{MinuteBarFact, StockPublic};
use super::{GroupIdx, StockIdx};
use crate::calendar::CivilDate;
use std::collections::BTreeMap;
use std::sync::Arc;

/// 一日分钟K历史快照（B2：日结冻结的不可变块——保留链节点）。
///
/// 语义合同（对照 18725d07 `RetainedHistoryDay`/`SharedHistoryDay`）：
/// - `securities` 覆盖**全部**在册证券（开市与休市日都留条目——休市
///   `trading=false` 且 bars 恒空）；
/// - bars 为封闭块共享（`Arc`——冻结后不可变；发布/查询零复制）；
/// - ADR-0034：保留链**全部保留**（五日窗只是显示语义，非存储窗）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RetainedDaySnapshot {
    /// 自然日。
    pub date: CivilDate,
    /// 每证券当日分钟会话（键=股索引）。
    pub securities: BTreeMap<u32, RetainedMinuteSession>,
}

/// 单证券一日分钟会话（开市/休市状态+bars）。
///
/// serde 手写（`Arc` 委托内值——旧 `SharedHistoryDay` 同式：存档序列化
/// 按值，恢复重建 `Arc`；`Arc` 是进程内共享形态而非 wire 形态）。
#[derive(Clone, Debug, PartialEq, Eq, ts_rs::TS)]
#[ts(export)]
pub struct RetainedMinuteSession {
    /// 开市=true（Trading）/休市=false（Closed）——旧 `HistorySessionStatus` 二值。
    pub trading: bool,
    /// 当日分钟K桶（零成交开市日为空——NoTrades 语义；休市恒空）。
    pub bars: Arc<Vec<MinuteBarFact>>,
}

impl serde::Serialize for RetainedMinuteSession {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("RetainedMinuteSession", 2)?;
        state.serialize_field("trading", &self.trading)?;
        state.serialize_field("bars", self.bars.as_ref())?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for RetainedMinuteSession {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Wire {
            trading: bool,
            bars: Vec<MinuteBarFact>,
        }
        let wire = Wire::deserialize(deserializer)?;
        Ok(Self {
            trading: wire.trading,
            bars: Arc::new(wire.bars),
        })
    }
}

/// 保留链节点的进程内共享形态（serde 按值委托内值——旧 `SharedHistoryDay`
/// 同式；存档序列化与恢复重建 `Arc`，`Arc` 不是 wire 形态）。
#[derive(Clone, Debug, PartialEq, Eq, ts_rs::TS)]
#[ts(export, as = "RetainedDaySnapshot")]
pub struct SharedRetainedDay(pub Arc<RetainedDaySnapshot>);

impl serde::Serialize for SharedRetainedDay {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_ref().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for SharedRetainedDay {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(Arc::new(RetainedDaySnapshot::deserialize(
            deserializer,
        )?)))
    }
}

/// 保留链索引（发布槽共享外壳——非冻结 tick 发布时 `Arc::clone` 复用，
/// 不逐拍复制全历史）。
#[derive(Clone, Debug, Default)]
pub struct RetainedIndex {
    /// 已冻结自然日（按日序严格递增，全部保留）。
    pub days: Vec<Arc<RetainedDaySnapshot>>,
}

/// 交易部读面分片（owner=desk 线程；观测快照——事实域在 report_log）。
#[derive(Clone, Debug)]
pub struct DeskShard {
    /// 股索引。
    pub stock: StockIdx,
    /// 本收束点公共态（行情/深度/价格历史观察面）。
    /// 刀 2b：Arc 共享（desk 报告条目直通——安装免深拷贝；读者
    /// 字段访问经 Deref 不变）。
    pub view: std::sync::Arc<StockPublic>,
    /// 本收束点挂单数（观察面计数）。
    pub resting_count: u64,
    /// P2-3：在途单观察面（随 DeskReport 上行——信封/簿镜像读面源）。
    pub resting: Arc<Vec<super::protocol::RestingFact>>,
    /// desk 序号游标（save 投影权威源；空簿保留已耗序号）。
    pub next_serial: u64,
}

/// P1-2（Sol 复核阻断）：install_tick 的组分片输入——SealAck 双分片
///（账户+belief 均在 Seal 的 apply_deferred 后构建，同拍一致上行）。
pub struct GroupShardInput {
    /// 组索引。
    pub group: GroupIdx,
    /// 账户稀疏分片（Seal 落账后 undo 集口径）。
    pub accounts:
        std::sync::Arc<Vec<(super::LocalIdx, std::sync::Arc<super::group::GroupAccount>)>>,
    /// belief 稀疏分片（Seal 落账后 dirty 集——含清仓经历转移行）。
    pub belief:
        std::sync::Arc<Vec<(super::LocalIdx, super::world::BeliefParticipantSnapshot)>>,
}

/// 组读面分片（P1-2 起 belief 稀疏观察面随 SealAck 上行；账户投影/
/// 计划进度为后续接入面）。
#[derive(Clone, Debug)]
pub struct GroupShard {
    /// 组索引。
    pub group: GroupIdx,
    /// 组提交态哈希（验证观察点——与 report_log 一致）。
    pub committed_hash: u64,
    /// belief 稀疏条目（本 tick 生产演进位——P2-2a 语义；P1-2 起随
    /// SealAck 上行=含 Seal 延迟相演进；全量读面由驱动器增量缓存承载）。
    pub belief: std::sync::Arc<
        Vec<(
            super::LocalIdx,
            super::world::BeliefParticipantSnapshot,
        )>,
    >,
    /// Q2 批 3：账户稀疏条目（SealAck 上行——Seal 落账后 undo 集口径；
    /// 替代驱动器 EchoAccountsSparse 往返；全量读面走 EchoAccounts）。
    pub accounts:
        std::sync::Arc<Vec<(super::LocalIdx, std::sync::Arc<super::group::GroupAccount>)>>,
}

/// 本拍事件段（P2 填充——R4 事件身份/seq/交付缓冲语义不变）。
#[derive(Clone, Copy, Debug, Default)]
pub struct EventSegment {
    /// 段首 seq（含）。
    pub seq_from: u64,
    /// 段尾 seq（含；空段 seq_from > seq_to）。
    pub seq_to: u64,
}

/// 提交态哈希族（验证观察点——与 report_log 逐位一致；Abort 候选的
/// 哈希永不进入发布面）。
#[derive(Clone, Debug, Default)]
pub struct TickHashes {
    /// 各组提交态哈希。
    pub groups: Vec<(GroupIdx, u64)>,
    /// 各交易部提交态哈希。
    pub desks: Vec<(StockIdx, u64)>,
}

/// 一个已提交 tick 的不可变发布版本。
#[derive(Clone, Debug)]
pub struct PublishedTick {
    /// 版本身份=tick 编号（锁步不变量保证全局唯一递增）。
    pub tick: u64,
    /// 发布代次（B2：每次 publish 递增——区分**同 tick** 的保留链/civil
    /// 推进（日结冻结重发）；tick 语义不变）。
    pub generation: u64,
    /// 全局域快照（committed_tick/day/events_installed）。
    pub global: GlobalState,
    /// 保留历史链（B2：日结冻结后原子换新；平日 Arc 复用零复制）。
    pub retained: Arc<RetainedIndex>,
    /// 各交易部读面分片（下标=股）。
    pub desks: Vec<DeskShard>,
    /// 各组读面分片（P2 填充；P1 恒空）。
    pub groups: Vec<GroupShard>,
    /// 本拍事件段（P2 填充；P1 恒空段）。
    pub events: EventSegment,
    /// 提交态哈希族。
    pub hashes: TickHashes,
}

/// 发布槽：一写多读（写=install_tick 单点；读者 load 一次 clone）。
pub struct ViewPublication {
    current: std::sync::RwLock<Arc<PublishedTick>>,
}

impl ViewPublication {
    /// 构造（初值=spawn 时的 v0 版本）。
    pub fn new(initial: Arc<PublishedTick>) -> Self {
        Self {
            current: std::sync::RwLock::new(initial),
        }
    }

    /// 读者入口：一次 clone 得一致读点（持有期任意长，无 use-after-free）。
    pub fn load(&self) -> Arc<PublishedTick> {
        self.current
            .read()
            .expect("发布槽读锁 poison（写侧 panic——world 已死，读面保留诊断）")
            .clone()
    }

    /// 唯一写点（install_tick 内调用；写锁临界区=一次指针赋值）。
    pub fn publish(&self, v: Arc<PublishedTick>) {
        *self
            .current
            .write()
            .expect("发布槽写锁 poison（前次写侧 panic）") = v;
    }

    /// 当前已发布 tick 号（诊断面）。
    pub fn current_tick(&self) -> u64 {
        self.load().tick
    }
}
