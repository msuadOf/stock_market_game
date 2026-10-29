//! std-thread 所有权转移模型的运行时原语（批 1：类型先行，不接线）。
//!
//! 模型一句话：NPC 分组线程持有账户，交易部线程一股一个持有订单簿；下单时现金/股票
//! 实体从组账户**实际扣出**随消息转移给交易部（所有权转移，非 escrow 预留），成交时
//! 实体互换、终结时原路退回。数据面（订单/实体）组↔交易部直发，不经协调者。
//!
//! 本模块当前只落地协议**类型**（身份标识 / DeskIn / Entity / FillFact / DeskReceipt /
//! DeskBatch），主循环（组线程、交易部、协调者）由后续批次接入。设计蓝本：迁移设计
//! F 收敛架构 §1 实体类型表 + 原型 `packages/std-thread-proto/src/oworker.rs`。
//!
//! 零 String 纪律：身份全部数值化（组内名册下标、股数值索引、复合挂单号），字符串只
//! 留边界层。

/// 组索引（NPC 组线程编号；世界构造期确定，运行期不变）。
pub type GroupIdx = u16;

/// 组内名册下标（组线程私有，坐标即账户）。
pub type LocalIdx = u32;

/// 资源所有者身份：(组索引, 组内名册下标)。实体回流时原样回带，组端零查找落账。
pub type Owner = (GroupIdx, LocalIdx);

/// 股票数值索引（世界构造期建立 `StockId` ↔ `StockCode` 边界映射）。
pub type StockIdx = u16;

/// 挂单标识：`(股索引, 交易部本地单调序号)` 复合，全局唯一。替代全局
/// `next_order_id` 计数器；恢复时按各股已消耗序号重建。存档/回执/定向撤单均用该
/// 复合标识。
pub type OrderRef = (StockIdx, u64);

pub(crate) mod admin_tax_projection;

#[cfg(any(target_arch = "wasm32", test))]
mod wasm_step_override;

#[cfg(target_arch = "wasm32")]
static STEP_RUNTIME_OVERRIDE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// 在 WASM 首次读取运行时开关前设置一次显式 override。
#[cfg(target_arch = "wasm32")]
pub fn set_step_runtime_override(enabled: bool) -> Result<(), &'static str> {
    wasm_step_override::set_step_runtime_override(&STEP_RUNTIME_OVERRIDE, enabled)
}

mod entities;
mod facts;
mod protocol;
#[cfg(test)]
pub(crate) use protocol::LifecycleKind;
pub(crate) use protocol::{RepurchasePreparePayload, RepurchaseProjectionInput, RuntimeError};
mod information_clock_context;
pub(crate) mod information_handover;
pub use information_clock_context::InformationClockContext;
mod repurchase_transport;
pub use repurchase_transport::{
    RepurchaseRequestKey, RepurchaseReturnKind, RepurchaseSourceFact, RepurchaseSourceKind,
};

pub mod bridge;
pub mod coordinator;
pub mod decision;
pub mod desk;
#[cfg(feature = "simulation-diagnostics")]
pub mod diagnostics;
pub mod drive;
pub(crate) mod driver_sync;
pub mod event_bridge;
pub mod retail_lifecycle;

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod bridge_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) use bridge_tests::setup_fixture as administrative_mirror_setup_for_test;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod calendar_cancel_all_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod calendar_chain_caller_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod calendar_day_end_authority_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod calendar_drain_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod calendar_group_day_end_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod calendar_misroute_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod calendar_postroll_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod calendar_projection_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod calendar_public_restore_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod calendar_raw_range_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod calendar_reopen_lifecycle_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod candle_retirement_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod day_end_public_views_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod day_end_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod decision_tests;
/// 刀 2a desk 投影轮间缓存合同（协调者刀单裁决②，2026-10-19）。
#[cfg(all(test, not(target_arch = "wasm32")))]
mod desk_projection_cache_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod drive_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod equivalence_tests;
pub mod fees;
pub mod group;
/// P0 预存缺陷红证：多组名册偏移（组端坐标 vs 全局投影坐标）。
#[cfg(all(test, not(target_arch = "wasm32")))]
mod group_roster_offset_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod inline_account_tests;
/// B2 件 2：desk 分钟K观察面语义测试（对照 18725d07 MinuteBar 基线）。
#[cfg(all(test, not(target_arch = "wasm32")))]
mod minute_bars_tests;
pub(crate) mod personal_full_validation;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod plan_identity_tests;
mod plan_ownership;
pub mod plans;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod plans_tests;
/// Q1 版化发布协议（方案 A·RwLock 先行版）：Arc<PublishedTick> 单点发布。
pub mod publication;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod publication_tests;
/// B2 件 4：保留历史快照链测试（18725d07 RetainedHistoryDay 基线）。
#[cfg(all(test, not(target_arch = "wasm32")))]
mod retained_history_tests;
/// Q2 Seal 收尾确认相位——行为合同测试（批 1 红先行）。
#[cfg(all(test, not(target_arch = "wasm32")))]
mod seal_tests;
pub(crate) mod source;
/// P0 分段画像探针（视图内生战役）：driver 线程 step 各段 wall 计时。
/// 默认零开销（探针显式 enable；生产恒关闭）。
pub mod step_profile;
/// B2 件 3：DeskTradeFact 双腿费用三分项测试（旧收据 actual_fees 基线）。
#[cfg(all(test, not(target_arch = "wasm32")))]
mod trade_fee_split_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod treasury_autonomy_tests;
/// 等待点分层账静态锚（wasm 线程抽象层 R4a 阶段 1——设计 §10.1/§11.1）。
#[cfg(all(test, not(target_arch = "wasm32")))]
mod wait_ledger_tests;
#[cfg(target_family = "wasm")]
mod wasm_wait;
pub mod watchdog;
/// B-1' 组线程等待分级自旋合同（协调者批 2026-10-19）。
#[cfg(all(test, not(target_arch = "wasm32")))]
mod watchdog_spin_tests;
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
pub mod watchdog_verification;
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
pub mod watchdog_wait_verification;
// D5 wasm 方向 A：world 不再按 wasm 门控（session/bridge/drive 的类型
// 引用面在 wasm 下编译必需）；wasm 宿主没有 OS 线程。串行 fallback 是缺省值，
// 宿主可在首次读取前显式设置 step runtime override。
pub mod world;

pub use entities::Entity;
pub use facts::{DeskReceipt, FillFact, FillFactError, ReceiptKind};
pub use protocol::{DeskBatch, DeskBatchMismatch, DeskIn, DeskTradeFact, RoutedPrice};
pub use protocol::{
    MinuteBarFact, MinuteBarPhase, RestingDelta, RestingDeltaEntry, RestingFact, StockPublic,
};
// B2 单一事实源纯函数/常量（desk 与串行安装共用——serial_b2_surfaces 经此路径引用）。
pub use protocol::{
    continuous_minute_of_day, minute_bar_apply, CLOSING_AUCTION_BAR_MINUTE, OPEN_AUCTION_BAR_MINUTE,
};

/// WASM 默认串行冒烟的编译级断言定义；不能据此认定默认行为已实测。
/// 首次设置前 `step_runtime_enabled` 缺省为 false；本片未实跑 WASM 测试。
#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_serial_smoke {
    #[test]
    fn runtime_step_disabled_on_wasm() {
        assert!(
            !crate::runtime::step_runtime_enabled(),
            "wasm 未设置 override 时缺省走串行管线"
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod desk_semantics_tests;
#[cfg(test)]
mod entities_tests;
#[cfg(test)]
mod facts_tests;
#[cfg(test)]
mod protocol_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod skeleton_tests;

/// 查询 step 管线开关。native 缺省启用 runtime，可用 `ENGINE_STEP_RUNTIME=0`
/// 临时回旧管线；WASM 缺省仍为串行，只有宿主在首次读取前显式设置
/// `step runtime override` 才进入经 `thread_facade` 创建 worker 的 `World` 路径。
/// 此处的编译级冒烟断言定义不表示本片已实跑 WASM cfg、ABI、`World` 或 Browser。
pub fn step_runtime_enabled() -> bool {
    // wasm 缺省使用串行管线；宿主可在首次读取前显式设置一次 override。
    #[cfg(target_arch = "wasm32")]
    {
        wasm_step_override::step_runtime_override_enabled(&STEP_RUNTIME_OVERRIDE)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // native 缺省启用 runtime（std-thread 生产缺省）；显式
        // ENGINE_STEP_RUNTIME=0 临时回旧管线。OnceLock 首读缓存——同进程不可中途切换。
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| {
            std::env::var("ENGINE_STEP_RUNTIME")
                .map(|v| v != "0")
                .unwrap_or(true)
        })
    }
}

// 公开消息只暴露 opaque context；构造与读取仍由 engine 内部受控。
pub use crate::session::market_calendar_context::{MarketDayContext, MarketTickContext};

#[cfg(test)]
mod calendar_test_support;

/// Session 镜像汇合时机；不改变 World 每拍提交。
pub mod sync_policy;

/// 公开镜像来源与运行进度分别标识。
pub mod mirror_origin;
