//! runtime 内复用的纯类型别名；均与原容器、tuple 和回调类型完全相同。

use std::sync::Arc;

use super::{GroupIdx, LocalIdx};

/// K 线记录（time、open、high、low、close、volume、turnover、trade_count）。
/// 价格与 turnover 使用分，volume 使用股；字段顺序与原协议保持一致。
pub type CandleProjection = (i64, i64, i64, i64, i64, u64, i64, u64);

/// 单个组账户分片中的名册位置与账户共享句柄。
pub(crate) type GroupAccountShardEntry = (LocalIdx, Arc<super::group::GroupAccount>);
/// 单个组的账户分片；Arc 层级保留发布快照的共享所有权。
pub(crate) type GroupAccountShard = Arc<Vec<GroupAccountShardEntry>>;
/// 全组账户分片集合。
pub(crate) type GroupAccountShards = Vec<GroupAccountShard>;

/// Session 到 runtime 的账户矩阵投影，顺序为现金、持仓、投入、回收、冻结、曾持有、账户身份。
pub type SessionAccountProjection = (
    Vec<i64>,
    Vec<Vec<u32>>,
    Vec<Vec<i64>>,
    Vec<Vec<i64>>,
    Vec<Vec<u32>>,
    Vec<Vec<bool>>,
    Vec<crate::AccountId>,
);

/// 组权益计算器。
pub type RightsComputer = Box<dyn Fn(u32, GroupIdx) -> Vec<super::protocol::AdminRight> + Send>;
/// 计划链工厂。
pub type PlanFactory = Box<dyn Fn(GroupIdx) -> Vec<Box<dyn super::plans::PlanChain>> + Send>;

/// 单只股票的完整 K 线投影。
pub type StockCandleProjection = (
    super::StockIdx,
    Vec<CandleProjection>,
    Option<CandleProjection>,
);
/// 深度档位及其价格、数量。
pub type DepthLevels = Vec<(i64, u64)>;
/// 单个组内账户的注意力调度计数。
pub type LocalAttentionCount = (LocalIdx, u64, u64);
/// 按组收集的注意力调度计数。
pub type GroupAttentionCounts = Vec<(GroupIdx, Vec<LocalAttentionCount>)>;
/// 按组收集的完整个人信念快照。
pub type GroupBeliefSnapshots = Vec<(
    GroupIdx,
    Vec<(LocalIdx, super::world::BeliefParticipantSnapshot)>,
)>;
/// 按组收集的可缺失个人信念快照。
pub type GroupOptionalBeliefSnapshots = Vec<(
    GroupIdx,
    Vec<Option<super::world::BeliefParticipantSnapshot>>,
)>;
/// 单条开仓观察与技术分析事实。
pub type ObservationPair = (
    crate::observation::PricePathObservation,
    crate::observation::TechnicalObservation,
);
/// 按组收集的开仓观察事实。
pub type GroupObservations = Vec<(GroupIdx, Vec<ObservationPair>)>;
/// 按组收集的开仓派生事实。
pub type GroupOpenDerivations = Vec<(
    GroupIdx,
    crate::strategy::MarketView,
    crate::calendar::CivilInstant,
    Vec<super::world::OpenDerivationRecord>,
)>;
/// 单个组的个人完整读面。
pub(crate) type PersonalFullGroup = (
    GroupIdx,
    Vec<(
        LocalIdx,
        crate::AccountId,
        Option<super::world::BeliefParticipantSnapshot>,
    )>,
);
/// 按组组织的个人完整读面。
pub(crate) type PersonalFullGroups = Vec<PersonalFullGroup>;

/// 保存当前在途事实与账户身份映射的共享快照。
pub type RestingSessionSnapshot = (
    Vec<Arc<Vec<super::protocol::RestingFact>>>,
    Arc<Vec<crate::AccountId>>,
);
/// 外部意图按组组织的操作和撤单队列。
pub(crate) type RoutedExternalIntents = (
    Vec<Vec<super::decision::PendingOp>>,
    Vec<Vec<(LocalIdx, super::StockIdx, u64)>>,
);
/// 日历来源及其交易所映射。
pub(crate) type CalendarOwner = (
    Arc<crate::session::market_calendar_context::MarketCalendarSource>,
    Vec<(crate::StockId, crate::calendar::CalendarExchange)>,
);
/// 行情日线种子（OHLC 与成交量）。
pub(crate) type DailyBarSeed = (i64, i64, i64, i64, u64);
/// 按组收集的已交付注意力状态。
pub type GroupAttentionUpdates = Vec<(
    GroupIdx,
    Vec<(
        LocalIdx,
        crate::session::NpcAttentionState,
        super::world::BeliefParticipantSnapshot,
    )>,
)>;
/// 个人完整读面中的账户身份与 presence。
pub(crate) type PersonalPresenceEntry = (
    crate::AccountId,
    super::personal_full_validation::PersonalPresence,
);
/// 按组组织的个人 presence。
pub(crate) type PersonalPresenceGroup = (GroupIdx, Vec<PersonalPresenceEntry>);
/// 按组组织的个人 presence 集合。
pub(crate) type GroupPersonalPresence = Vec<PersonalPresenceGroup>;
/// 管理权交接捕获及其公共资料、公司事实和个人计数。
pub(crate) type InformationHandoverCapture = (
    super::information_handover::GroupInformationHandover,
    Arc<crate::information::PublicLibrary>,
    Arc<crate::company::CompanyReadFacts>,
    Vec<(crate::AccountId, u64, u64)>,
);
/// 分组行政阶段请求及其原始阶段布局。
pub(crate) type PreparedAdminStageRequests = (
    Vec<(GroupIdx, Vec<super::protocol::AdminOperation>)>,
    std::collections::BTreeMap<GroupIdx, super::protocol::AdminStageLayout>,
);
/// 用户观察的公开行情深度视图。
pub type MarketDepthLevels = (i64, DepthLevels, DepthLevels);
/// 模拟诊断中 Added、Removed、Changed 三类计数。
#[cfg(test)]
pub(crate) type RestingDeltaFaceCounts = (usize, usize, usize);

/// 单组计划操作与定向撤单队列。
pub(crate) type GroupPlanOperations = (
    Vec<(crate::PlanId, super::decision::PendingOp)>,
    Vec<(LocalIdx, super::StockIdx, u64)>,
);

/// 单次开仓派生及其计划操作、定向撤单结果。
pub(crate) type OpenDerivationPlanResult = (
    super::world::OpenDerivationRecord,
    Vec<(crate::PlanId, super::decision::PendingOp)>,
    Vec<(LocalIdx, super::StockIdx, u64)>,
);

/// 单条信息投递更新：`belief_cache` 全局索引、账户 id、注意力状态与共享运行时个人视图。
pub(crate) type DeliveryUpdate = (
    usize,
    crate::AccountId,
    crate::session::NpcAttentionState,
    crate::session::SharedRuntimePersonal,
);
/// 一次信息投递的**跨组**更新批次与本拍信息检查下界（各组下界取最小；`None`=无业务下界）。
pub(crate) type DeliveryBatch = (Vec<DeliveryUpdate>, Option<crate::calendar::CivilInstant>);
