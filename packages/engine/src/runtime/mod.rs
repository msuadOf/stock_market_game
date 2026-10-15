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

mod entities;
mod facts;
mod protocol;

pub mod bridge;
pub mod coordinator;
pub mod decision;
pub mod desk;
pub mod drive;
pub mod event_bridge;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod bridge_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod drive_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod day_end_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod decision_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod equivalence_tests;
pub mod fees;
pub mod group;
pub mod plans;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod plans_tests;
pub mod watchdog;
#[cfg(not(target_arch = "wasm32"))]
pub mod world;

pub use entities::Entity;
pub use facts::{DeskReceipt, FillFact, FillFactError, ReceiptKind};
pub use protocol::{DeskBatch, DeskBatchMismatch, DeskIn, RoutedPrice};

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

/// 生产入口翻转开关（翻转批·实测后回退为 opt-in）：ENGINE_STEP_RUNTIME=1
/// 显式走 runtime，**缺省 0=旧管线**。实测（翻转批）：step() 全量翻转
/// 使 106 个 session 侧测试失败——全局域（envelope/attention/candidate
/// 投影）演进仍锚定旧管线，runtime step 未承载 ⇒ 硬翻=静默语义丢失
/// （铁律二）。翻转的最后一步=全局域迁移完成后再改缺省。
pub fn step_runtime_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(
        || std::env::var("ENGINE_STEP_RUNTIME").map(|v| v == "1").unwrap_or(false),
    )
}
