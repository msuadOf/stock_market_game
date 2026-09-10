//! 任务 19（company-information-npc-intentions）：具名技术指标与个人价格记忆。
//!
//! 政策基线：计划 K5 行 134–135 —— 原 30 分钟/5 日/区间/量能/失衡观测保持不变；
//! 新增 SMA20/SMA60、RSI14、ATR14 按完整日 K 计算；个人价格记忆保存首次/最近
//! 观察价、已观察高低、时间窗口与来源，本人所见与主动读取公开历史严格区分，
//! 不虚构观察经历。窗口长度一律按完整市场时间（交易日/交易分钟）表达；
//! 技术指标输入是日 K 序列，结构上不存在宿主 tick 概念（tick 密度不变性由
//! tests/observations.rs 的分钟归并测试锁定）。
//!
//! 金样单位约定：注释与断言写「分」，全部为手算精确值。按场景拆分：
//! `gold`（指标独立金样 + 短跌长涨并存）、`memory`（个人价格记忆）、
//! `failures`（类型化拒绝，不用零或虚构走势补位）。
//!
//! QA 入口：`cargo test -p engine --test technical_memory`（happy 与 failure 同命令覆盖）。

mod failures;
mod gold;
mod memory;

use engine::observation::TechnicalDailyInput;
use engine::strategy::TechnicalDailyBar;
use engine::{Money, StockCode};

/// 分 → Money。
pub(crate) fn price(cents: i64) -> Money {
    Money::from_cents(cents)
}

/// 测试用证券代码。
pub(crate) fn code(name: &str) -> StockCode {
    StockCode(name.into())
}

/// 分数组 → 收盘价序列。
pub(crate) fn closes(values: &[i64]) -> Vec<Money> {
    values.iter().map(|value| price(*value)).collect()
}

/// 完整观测输入日 K。
pub(crate) fn bar(day: u32, high: i64, low: i64, close: i64, volume: u64) -> TechnicalDailyInput {
    TechnicalDailyInput {
        trading_day: day,
        high: price(high),
        low: price(low),
        close: price(close),
        volume,
    }
}

/// 高=低=收 的观测输入日 K。
pub(crate) fn flat_bar(day: u32, close: i64, volume: u64) -> TechnicalDailyInput {
    bar(day, close, close, close, volume)
}

/// ATR 内核输入日 K。
pub(crate) fn kbar(high: i64, low: i64, close: i64) -> TechnicalDailyBar {
    TechnicalDailyBar {
        high: price(high),
        low: price(low),
        close: price(close),
    }
}
