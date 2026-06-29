//! 可配置的游戏参数集合（GameConfig）—— 纯数据 + 边界校验。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-gameconfig-design.md。
//! 铁律：config 是启动期外部输入，非法 → 显式 `Result` 报错，绝不静默 fallback。
//! 当前为 T1 骨架：仅 `ConfigError` + `GameConfig` 占位，让 lib.rs 导出编译通过。
//! 校验逻辑 / proposed_defaults / commission / stamp_tax 在后续 T2-T6 补齐。

use thiserror::Error;

use crate::money::{Money, MoneyError};

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

    /// 参考游戏提取的提议默认值，待 msuad 确认。
    ///
    /// 这些数值来自参考游戏 `ref/模拟股市.html`（佣金率 0.00025 / 印花税率 0.0005 /
    /// 佣金下限 5 元 / 一手 100 股 / 默认涨跌幅 10% / ST 涨跌幅 5% / 初始资金 100000 元），
    /// 仅由此方法集中返回，**不作为硬编码常量散落代码**（spec §2 第 4 条）。
    ///
    /// 提议值本身恒合法（通过 `new()` 的全部校验），故此路径是受控构造、非业务吞错路径。
    /// 用 `expect`（带说明）而非裸 `unwrap`：若 panic，说明提议值与 `new()` 校验不一致，
    /// 即提议值需要重新校验或 `new()` 约束有误——这是开发者应立即修正的 bug（铁律三）。
    pub fn proposed_defaults() -> GameConfig {
        GameConfig::new(
            0.00025,
            Money::from_cents(500),
            0.0005,
            0.10,
            0.05,
            100,
            Money::from_cents(10_000_000),
        )
        .expect("proposed defaults are valid；若失败说明提议值需重新校验")
    }

    /// 计算成交额的佣金：按 `commission_rate` 缩放后与 `commission_min` 取 `max`。
    ///
    /// 即 `max(amount.apply_rate(commission_rate), commission_min)`：小额成交额的佣金
    /// 不足下限时，回落到 `commission_min`（参考游戏：佣金 5 元下限，spec §3）。
    ///
    /// 透传 [`Money::apply_rate`] 的 `MoneyError`：正常路径下比率已在 `new()` 校验为
    /// 有限非负，运行期不会触发；类型上仍返回 `Result` 以防配置绕过构造（spec §5）。
    ///
    /// `Money` 已 derive `Ord`，可直接比较取较大者。
    pub fn commission(&self, amount: Money) -> Result<Money, MoneyError> {
        let rated = amount.apply_rate(self.commission_rate)?;
        Ok(if rated >= self.commission_min {
            rated
        } else {
            self.commission_min
        })
    }

    /// 计算成交额的印花税：`amount.apply_rate(self.stamp_tax_rate)`，**无 floor**。
    ///
    /// 与 [`GameConfig::commission`] 的关键差异：印花税**没有下限**（参考游戏：佣金有 5 元
    /// 下限、印花税无下限，spec §3）。故这里直接透传 `apply_rate` 的银行家舍入结果，
    /// 不与任何下限取 `max`——小额成交额的印花税就是其真实比率乘积（可能很小但非 0）。
    ///
    /// 透传 [`Money::apply_rate`] 的 `MoneyError`：正常路径下比率已在 `new()` 校验为
    /// 有限非负，运行期不会触发；类型上仍返回 `Result` 以防配置绕过构造（spec §5）。
    pub fn stamp_tax(&self, amount: Money) -> Result<Money, MoneyError> {
        amount.apply_rate(self.stamp_tax_rate)
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
