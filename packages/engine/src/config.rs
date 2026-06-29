//! 可配置的游戏参数集合（GameConfig）—— 纯数据 + 边界校验。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-gameconfig-design.md。
//! 铁律：config 是启动期外部输入，非法 → 显式 `Result` 报错，绝不静默 fallback。
//! 当前为 T1 骨架：仅 `ConfigError` + `GameConfig` 占位，让 lib.rs 导出编译通过。
//! 校验逻辑 / proposed_defaults / commission / stamp_tax 在后续 T2-T6 补齐。

use thiserror::Error;

use crate::money::Money;

/// config 校验失败。绝不静默吞掉（铁律二），错误携带字段名 / 实际值 / 原因。
#[derive(Debug, Error)]
pub enum ConfigError {
    /// 比率非有限（NaN/Inf）或为负：commission_rate / stamp_tax_rate。
    #[error("invalid rate field {field:?}: value {rate} ({reason})")]
    InvalidRate {
        field: &'static str,
        rate: f64,
        reason: String,
    },
    /// 涨跌幅非法：<= 0 或 >= 1（无意义）。default_limit / st_limit。
    #[error("invalid limit field {field:?}: value {limit} (must be in (0,1))")]
    InvalidLimit { field: &'static str, limit: f64 },
    /// lot_size == 0。
    #[error("invalid lot_size: {0} (must be >= 1)")]
    InvalidLotSize(u32),
    /// starting_cash 为负。
    #[error("invalid starting_cash: {0:?} (must be >= 0)")]
    InvalidCash(Money),
}

/// 可配置的游戏参数集合。纯数据 + 边界校验，可序列化。
///
/// 占位骨架：字段与方法在后续 task 补齐。
#[derive(Clone, Debug)]
pub struct GameConfig;
