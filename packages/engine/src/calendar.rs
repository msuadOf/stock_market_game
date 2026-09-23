//! 真实公历与冻结交易日历（K1，任务 4）。
//!
//! 分层：`date`（纯公历算法：CivilDate / CivilInstant / Weekday）→
//! `data`（1998–2099 离线农历/节气事实表，香港天文台官方来源，构建期固化）→
//! `policy`（CalendarPolicy：算法版本/适用范围/沪深官方覆盖/模拟回退 + digest，
//! 随存档冻结；再分 coverage/validation 子模块）→ `holidays`（交易日解析与
//! 前史查询）。本文件承载模块级词汇（TradingDayOrdinal / HolidayKind /
//! ClosedReason / DayStatus）、统一错误与内容摘要函数。
//!
//! 政策与法源基线：docs/simulation-calendar.md；冻结值 fixture：
//! tests/fixtures/company-model/policy-sources.json `calendar` 节。
//! 无第三方运行时依赖；无网络；越界一律类型化错误，不钳位、不改状态。

mod date;
mod holidays;
mod policy;

pub mod data;

pub use data::{embedded_lunar_facts, LunarYearFact, LunarYearFacts};
pub use date::{
    CivilDate, CivilDateError, CivilInstant, Weekday, CIVIL_YEAR_MAX, CIVIL_YEAR_MIN,
    SECONDS_PER_DAY,
};
pub use holidays::TradingCalendar;
pub use policy::{
    CalendarExchange, CalendarPolicy, CalendarPolicySpec, DateRange, OfficialCoverageEntry,
    SimulatedFallbackRuleset, YearCoverageLabel, DAY_COUNT_BASIS_ACT_365F,
};

/// 交易日序数 newtype：由 `TradingCalendar` 在特定政策下赋予/解析
/// （0 = 初始化下界起第一个交易日）。与 tick 计数、市场分钟严格分离（K1）。
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    Default,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
#[serde(transparent)]
#[ts(type = "number")]
pub struct TradingDayOrdinal(u32);

impl TradingDayOrdinal {
    pub fn from_u32(value: u32) -> Self {
        Self(value)
    }

    pub fn value(self) -> u32 {
        self.0
    }
}

/// 模拟假日类别（§3.3 游戏假设；判定规则在 holidays.rs）。
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
pub enum HolidayKind {
    NewYearDay,
    LabourDay,
    NationalDay,
    SpringFestival,
    Qingming,
    DragonBoat,
    MidAutumn,
}

/// 休市原因。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum ClosedReason {
    /// 周末（含政府调休补班周末——K1 不模拟补班）。
    Weekend,
    /// 官方通知闭市（附出处标识）。
    OfficialHoliday { citation_id: String },
    /// 模拟回退假日（游戏假设）。
    SimulatedHoliday(HolidayKind),
}

/// 单日状态。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum DayStatus {
    Trading,
    Closed(ClosedReason),
}

/// 日历层统一错误（政策校验 + 事实表 + 查询门）。绝不静默吞掉（铁律二）。
#[derive(Debug, thiserror::Error)]
pub enum CalendarError {
    /// 日期算法层错误（非法日期/解析失败/时间分量）。
    #[error("civil date error: {0}")]
    Civil(#[from] CivilDateError),

    /// 查询早于初始化专用下界（1998-01-01）。
    #[error("date {date} precedes init-only floor {floor}")]
    BeforeInitFloor { date: CivilDate, floor: CivilDate },

    /// 查询晚于运行上界（2099-12-31）；不循环年份。
    #[error("date {date} exceeds runtime ceiling {ceiling}")]
    AfterRuntimeCeiling { date: CivilDate, ceiling: CivilDate },

    /// 运行时开局越界：1998–1999 仅供初始化前史查询，不是合法开局。
    #[error("runtime start {date} outside [{min}, {max}]; 1998–1999 is init-only prehistory")]
    RuntimeStartOutOfRange {
        date: CivilDate,
        min: CivilDate,
        max: CivilDate,
    },

    /// 非交易日上执行了仅交易日可用的查询。
    #[error("{exchange:?} {date} is not a trading day: {reason:?}")]
    NotATradingDay {
        exchange: CalendarExchange,
        date: CivilDate,
        reason: ClosedReason,
    },

    /// 前史交易日耗尽（360 日 K 线前史超出 1998-01-01 起可用天数）。
    #[error("prehistory exhausted: need {needed} trading days before {date}, only {available} available since {floor}")]
    PrehistoryExhausted {
        needed: u32,
        available: u32,
        date: CivilDate,
        floor: CivilDate,
    },

    /// 事实表缺该年份（不用邻年顶替）。
    #[error("lunar fact table has no year {year}")]
    MissingLunarFact { year: i32 },

    /// 官方覆盖重复/重叠/乱序。
    #[error("conflicting official coverage: {detail}")]
    ConflictingOfficialCoverage { detail: String },

    /// digest 校验失败（存档被篡改或表内容与摘要不符）。
    #[error("digest mismatch on {field}: stored {stored}, computed {computed}")]
    DigestMismatch {
        field: &'static str,
        stored: String,
        computed: String,
    },

    /// 政策内部不一致（顺序/覆盖缺口/出处缺失等）。
    #[error("invalid calendar policy: {reason}")]
    PolicyInvalid { reason: String },

    /// 计数参数非法（如 n = 0）。
    #[error("invalid count {count}: {what} must be >= 1")]
    InvalidCount { what: &'static str, count: u32 },
}

/// FNV-1a 64 位十六进制摘要（16 字符）。分部以 0x1f 分隔，`["a","b"]` 与
/// `["ab"]` 摘要不同。仅用于内容绑定（防篡改/防换表），非密码学用途。
pub(crate) fn fnv1a64_hex(parts: &[&str]) -> String {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for part in parts {
        for byte in part.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(PRIME);
        }
        hash ^= 0x1f;
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")
}
