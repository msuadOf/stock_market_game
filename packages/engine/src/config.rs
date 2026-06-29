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
/// 字段全部 `pub`，供在配置层（如启动期从设置文件构造）直接建立；
/// 构造期校验（拒绝非法比率/limit/lot_size/cash）由后续 `new()` 提供（T3）。
///
/// 比率字段（`commission_rate` / `stamp_tax_rate` / `default_limit` / `st_limit`）为 `f64`，
/// 仅作为 [`Money::apply_rate`] 的比率入参使用，绝不参与存储为金额；
/// 金额类字段（`commission_min` / `starting_cash`）为定点 [`Money`]，遵循 money 模块铁律。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GameConfig {
    /// 成交额佣金率（ref 提议: 0.00025）。
    pub commission_rate: f64,
    /// 佣金下限（ref 提议: 5.00 元 = 500 分）。
    pub commission_min: Money,
    /// 印花税率，仅卖（ref 提议: 0.0005）。
    pub stamp_tax_rate: f64,
    /// 默认涨跌幅（ref 提议: 0.10）。
    pub default_limit: f64,
    /// ST 涨跌幅（ref 提议: 0.05）。
    pub st_limit: f64,
    /// 一手股数（ref 提议: 100）。
    pub lot_size: u32,
    /// 初始资金（ref 提议: 100000.00 元 = 10_000_000 分）。
    pub starting_cash: Money,
}

impl GameConfig {
    /// 构造即校验：逐字段校验不变量，任一非法即 `Err`，绝不静默 fallback（铁律二）。
    ///
    /// 仅强制数学/逻辑底线：比率有限且非负、涨跌幅 `0 < limit < 1`、`lot_size >= 1`、
    /// `starting_cash >= 0`。不涉及玩法平衡（见 spec §2 / §5）。
    ///
    /// 字段按结构体声明顺序传入。
    pub fn new(
        commission_rate: f64,
        commission_min: Money,
        stamp_tax_rate: f64,
        default_limit: f64,
        st_limit: f64,
        lot_size: u32,
        starting_cash: Money,
    ) -> Result<GameConfig, ConfigError> {
        // 比率字段：必须有限（拒绝 NaN / ±Inf）且非负。
        if !commission_rate.is_finite() || commission_rate < 0.0 {
            return Err(ConfigError::InvalidRate {
                field: "commission_rate",
                rate: commission_rate,
                reason: rate_reason(commission_rate),
            });
        }
        if !stamp_tax_rate.is_finite() || stamp_tax_rate < 0.0 {
            return Err(ConfigError::InvalidRate {
                field: "stamp_tax_rate",
                rate: stamp_tax_rate,
                reason: rate_reason(stamp_tax_rate),
            });
        }

        // 涨跌幅限制：必须严格落在 (0, 1) 内（<=0 无意义、>=1 等同无限制）。
        if !(default_limit > 0.0 && default_limit < 1.0) {
            return Err(ConfigError::InvalidLimit {
                field: "default_limit",
                limit: default_limit,
            });
        }
        if !(st_limit > 0.0 && st_limit < 1.0) {
            return Err(ConfigError::InvalidLimit {
                field: "st_limit",
                limit: st_limit,
            });
        }

        // 一手股数：必须 >= 1（0 手无法交易）。
        if lot_size == 0 {
            return Err(ConfigError::InvalidLotSize(lot_size));
        }

        // 初始资金：不可为负（游戏允许 0，但绝不允许负债起步）。
        if starting_cash.cents() < 0 {
            return Err(ConfigError::InvalidCash(starting_cash));
        }

        Ok(GameConfig {
            commission_rate,
            commission_min,
            stamp_tax_rate,
            default_limit,
            st_limit,
            lot_size,
            starting_cash,
        })
    }
}

/// 生成比率字段的失败原因描述：区分非有限与负值，便于错误信息可诊断。
fn rate_reason(rate: f64) -> String {
    if rate.is_nan() {
        "NaN".to_string()
    } else if rate.is_infinite() {
        if rate > 0.0 {
            "+Inf".to_string()
        } else {
            "-Inf".to_string()
        }
    } else {
        "negative".to_string()
    }
}
