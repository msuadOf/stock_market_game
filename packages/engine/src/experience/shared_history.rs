//! 兼容重导出：AppendOnlyHistory 已提升为引擎顶层共享结构（公司 journal
//! 批次等域复用同一实现；架构项 #13）。保留旧路径避免 experience 消费方
//! 大面积改动。
pub use crate::shared_history::AppendOnlyHistory;
