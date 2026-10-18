//! 具名技术指标与个人价格记忆验证。
//!
//! 政策基线：30 分钟/5 日/区间/量能/失衡观测保持不变；
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
use engine::{Money, StockId};

/// 分 → Money。
pub(crate) fn price(cents: i64) -> Money {
    Money::from_cents(cents)
}

/// 测试用股票身份。本夹具不构造目录，按代码字符串升序手工分配稠密 rank
/// （与 `StockCatalog` 的升序规则同构），保证 `Ord` 语义与原 StockCode 一致；
/// 未登记的代码显式失败，不静默编造身份（铁律二）。
pub(crate) fn code(name: &str) -> StockId {
    let rank = match name {
        "000001" => 0,
        "000002" => 1,
        "600001" => 2,
        "600100" => 3,
        "600101" => 4,
        "600102" => 5,
        "600103" => 6,
        "600104" => 7,
        "600105" => 8,
        "600106" => 9,
        "600107" => 10,
        "600108" => 11,
        "600109" => 12,
        "600110" => 13,
        "600111" => 14,
        "600112" => 15,
        other => panic!("测试夹具未登记代码 {other} 的 StockId rank"),
    };
    StockId(rank)
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
