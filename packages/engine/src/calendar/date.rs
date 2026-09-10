//! 纯公历日期算术（K1）：`CivilDate` / `CivilInstant` / `TradingDayOrdinal`。
//!
//! 本文件只做格里高利历的**数学事实**：闰年（4/100/400 规则）、星期、日计数、
//! ISO 解析/格式化。交易所休市语义在 `policy.rs` / `holidays.rs`。类型刻意与
//! `Money`、tick 计数、市场分钟分离（K1：金额/日期/交易分钟不混用）。
//!
//! 年限 1900–2199 是**算法验证窗**：覆盖运行区间 1998–2099 及两侧世纪边界
//! （2000 闰 / 2100 不闰），窗外年份显式拒绝。运行边界（2000-01-01 至
//! 2099-12-31）与初始化专用下界（1998-01-01）不是日期算法事实，由
//! `CalendarPolicy` 强制（见 policy.rs）。

use thiserror::Error;

/// 算法验证窗下界（含）。
pub const CIVIL_YEAR_MIN: i32 = 1900;
/// 算法验证窗上界（含）。
pub const CIVIL_YEAR_MAX: i32 = 2199;
/// 一自然日的秒数上界（`CivilInstant::second_of_day` 合法域为 0..86_400）。
pub const SECONDS_PER_DAY: u32 = 86_400;

/// 星期。0 基枚举顺序仅用于序列化稳定，比较语义以变体名为准。
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
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    /// 是否周末（周六/周日）。调休形成的补班周末在 K1 下**不**改此判定
    /// （不模拟补班，见 docs/simulation-calendar.md §3.3）。
    pub fn is_weekend(self) -> bool {
        matches!(self, Weekday::Saturday | Weekday::Sunday)
    }
}

/// 公历日期构造/解析失败。绝不静默钳位（铁律二）。
#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum CivilDateError {
    /// 年份超出算法验证窗 1900–2199（例：9999）。
    #[error("year {year} outside civil algorithm window 1900..=2199")]
    YearOutOfRange { year: i32 },
    /// 月份不在 1..=12（例：13、0）。
    #[error("month {month} outside 1..=12")]
    MonthOutOfRange { month: u8 },
    /// 日超出该月实际天数（含 2100-02-29：整百年非 400 倍数不闰）。
    #[error("day {day} invalid for {year}-{month:02}: month has {days_in_month} days")]
    DayOutOfRange {
        year: i32,
        month: u8,
        day: u8,
        days_in_month: u8,
    },
    /// ISO 字符串形状非法（非 `YYYY-MM-DD`、非数字、长度不符等）。
    #[error("parse failed: input {input:?}: {reason}")]
    ParseFailed { input: String, reason: String },
    /// 日内时间分量非法（`CivilInstant` 的秒数或时分秒分量）。
    #[error("time component {value} invalid: {reason}")]
    TimeComponent { value: u32, reason: &'static str },
}

/// ISO `YYYY-MM-DD` 公历日期 newtype。内部不变量：年 ∈ 1900..=2199、
/// 月 ∈ 1..=12、日 ≤ 当月实际天数——唯一构造入口 `from_ymd`/`from_iso` 保证。
/// 刻意不实现 `Default`（0-0-0 不是合法日期）。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, ts_rs::TS)]
#[ts(type = "string")]
pub struct CivilDate {
    year: i32,
    month: u8,
    day: u8,
}

impl CivilDate {
    /// 由年/月/日构造，完整校验（闰月、月天数、年份窗）。非法输入一律 `Err`。
    pub fn from_ymd(year: i32, month: u8, day: u8) -> Result<Self, CivilDateError> {
        if !(CIVIL_YEAR_MIN..=CIVIL_YEAR_MAX).contains(&year) {
            return Err(CivilDateError::YearOutOfRange { year });
        }
        if !(1..=12).contains(&month) {
            return Err(CivilDateError::MonthOutOfRange { month });
        }
        let days_in_month = days_in_month(year, month);
        if day < 1 || day > days_in_month {
            return Err(CivilDateError::DayOutOfRange {
                year,
                month,
                day,
                days_in_month,
            });
        }
        Ok(Self { year, month, day })
    }

    /// 严格解析 `YYYY-MM-DD`：4 位年、2 位月/日、恰好两个连字符，其余拒收。
    pub fn from_iso(input: &str) -> Result<Self, CivilDateError> {
        let malformed = |reason: &str| CivilDateError::ParseFailed {
            input: input.to_string(),
            reason: reason.to_string(),
        };
        let bytes = input.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return Err(malformed("expected strict YYYY-MM-DD shape"));
        }
        let mut groups = [0u32; 3];
        let mut group = 0usize;
        let mut acc = 0u32;
        for (idx, byte) in bytes.iter().enumerate() {
            if idx == 4 || idx == 7 {
                groups[group] = acc;
                group += 1;
                acc = 0;
                continue;
            }
            if !byte.is_ascii_digit() {
                return Err(malformed("non-digit component"));
            }
            acc = acc * 10 + u32::from(byte - b'0');
        }
        groups[2] = acc;
        let year = i32::try_from(groups[0]).map_err(|_| malformed("year overflow"))?;
        let month = u8::try_from(groups[1]).map_err(|_| malformed("month overflow"))?;
        let day = u8::try_from(groups[2]).map_err(|_| malformed("day overflow"))?;
        Self::from_ymd(year, month, day)
    }

    /// 格式化为 ISO `YYYY-MM-DD`。
    pub fn to_iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    pub fn year(self) -> i32 {
        self.year
    }

    pub fn month(self) -> u8 {
        self.month
    }

    pub fn day(self) -> u8 {
        self.day
    }

    /// 闰年（4/100/400 规则）：2000 闰、2100 不闰、2032 闰、1998 不闰。
    pub fn is_leap_year(self) -> bool {
        is_leap_year(self.year)
    }

    /// 星期（1970-01-01 为周四的既定公历事实推得）。
    pub fn weekday(self) -> Weekday {
        WEEKDAYS_BY_OFFSET
            [(days_from_civil(self.year, self.month, self.day) + 4).rem_euclid(7) as usize]
    }

    /// 年内序数（1 基）：1-01-01 为 1，闰年 12-31 为 366。
    pub fn day_of_year(self) -> u16 {
        let leap_after_february = u16::from(is_leap_year(self.year) && self.month > 2);
        CUMULATIVE_DAYS[usize::from(self.month) - 1] + u16::from(self.day) + leap_after_february
    }

    /// `self - other` 的自然日数（有符号；self 早于 other 为负）。
    pub fn days_since(self, other: CivilDate) -> i64 {
        days_from_civil(self.year, self.month, self.day)
            - days_from_civil(other.year, other.month, other.day)
    }

    /// 下一自然日；越过 2199-12-31 返回 `YearOutOfRange`。
    pub fn next(self) -> Result<Self, CivilDateError> {
        if self.day < days_in_month(self.year, self.month) {
            Ok(Self {
                day: self.day + 1,
                ..self
            })
        } else if self.month < 12 {
            Self::from_ymd(self.year, self.month + 1, 1)
        } else {
            Self::from_ymd(self.year + 1, 1, 1)
        }
    }

    /// 上一自然日；早于 1900-01-01 返回 `YearOutOfRange`。
    pub fn prev(self) -> Result<Self, CivilDateError> {
        if self.day > 1 {
            Ok(Self {
                day: self.day - 1,
                ..self
            })
        } else if self.month > 1 {
            let month = self.month - 1;
            Self::from_ymd(self.year, month, days_in_month(self.year, month))
        } else {
            Self::from_ymd(self.year - 1, 12, 31)
        }
    }
}

/// Display 即 ISO 形（与 serde 序列化一致）。
impl std::fmt::Display for CivilDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// 序列化为 ISO 字符串（与 `from_iso` 互逆；存档人可读）。
impl serde::Serialize for CivilDate {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_iso())
    }
}

/// 反序列化经 `from_iso` 全量校验——存档中的非法日期在恢复边界显式失败。
impl<'de> serde::Deserialize<'de> for CivilDate {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_iso(&text).map_err(serde::de::Error::custom)
    }
}

/// Asia/Shanghai 语义的日内时刻：日期 + 当日秒（0..86_400）。
///
/// 规则时间与模拟时间分离（K1 §4）的最小承载单元；时区不引入 tz 数据库，
/// 中国大陆自 1991 年起无夏令时，全年统一 UTC+8，此处直接存"本地日 + 日内秒"。
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
pub struct CivilInstant {
    date: CivilDate,
    second_of_day: u32,
}

impl CivilInstant {
    /// 构造并校验当日秒域。
    pub fn new(date: CivilDate, second_of_day: u32) -> Result<Self, CivilDateError> {
        if second_of_day >= SECONDS_PER_DAY {
            return Err(CivilDateError::TimeComponent {
                value: second_of_day,
                reason: "second_of_day < 86400",
            });
        }
        Ok(Self {
            date,
            second_of_day,
        })
    }

    /// 时/分/秒构造（各分量域校验后折算为当日秒）。
    pub fn from_hms(
        date: CivilDate,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> Result<Self, CivilDateError> {
        for (value, limit, reason) in [
            (hour, 24, "hour must be < 24"),
            (minute, 60, "minute must be < 60"),
            (second, 60, "second must be < 60"),
        ] {
            if value >= limit {
                return Err(CivilDateError::TimeComponent { value, reason });
            }
        }
        Self::new(date, hour * 3600 + minute * 60 + second)
    }

    pub fn date(self) -> CivilDate {
        self.date
    }

    pub fn second_of_day(self) -> u32 {
        self.second_of_day
    }
}

/// 各月前的累计天数（平年；`day_of_year` 对闰年 3 月起 +1）。
const CUMULATIVE_DAYS: [u16; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];

/// 星期查表：下标 = (自 1970-01-01 天数 + 4) mod 7，0 = Sunday（1970-01-01 周四）。
const WEEKDAYS_BY_OFFSET: [Weekday; 7] = [
    Weekday::Sunday,
    Weekday::Monday,
    Weekday::Tuesday,
    Weekday::Wednesday, // 1970-01-01 周四锚点
    Weekday::Thursday,
    Weekday::Friday,
    Weekday::Saturday,
];

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        // from_ymd 已挡住非法月份；此处仅供已验证日期的 next/prev 复用。
        _ => unreachable!("month validated by from_ymd"),
    }
}

/// 自 1970-01-00 起的天数（Howard Hinnant 算法，纯整数、无浮点）。
fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (i64::from(month) + 9) % 12; // 3 月为岁首
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
