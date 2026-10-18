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
use super::protocol::StockPublic;
use super::{GroupIdx, StockIdx};
use std::sync::Arc;

/// 交易部读面分片（owner=desk 线程；观测快照——事实域在 report_log）。
#[derive(Clone, Debug)]
pub struct DeskShard {
    /// 股索引。
    pub stock: StockIdx,
    /// 本收束点公共态（行情/深度/价格历史观察面）。
    pub view: StockPublic,
    /// 本收束点挂单数（观察面计数）。
    pub resting_count: u64,
    /// P2-3：在途单观察面（随 DeskReport 上行——信封/簿镜像读面源）。
    pub resting: Arc<Vec<super::protocol::RestingFact>>,
    /// desk 序号游标（save 投影权威源；空簿保留已耗序号）。
    pub next_serial: u64,
}

/// 组读面分片（P2-2 起 belief 稀疏观察面随报告上行；账户投影/计划
/// 进度为后续接入面）。
#[derive(Clone, Debug)]
pub struct GroupShard {
    /// 组索引。
    pub group: GroupIdx,
    /// 组提交态哈希（验证观察点——与 report_log 一致）。
    pub committed_hash: u64,
    /// belief 稀疏条目（本 tick 生产演进位——P2-2a 语义；全量读面由
    /// 驱动器增量缓存承载）。
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
    /// 全局域快照（committed_tick/day/events_installed）。
    pub global: GlobalState,
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
