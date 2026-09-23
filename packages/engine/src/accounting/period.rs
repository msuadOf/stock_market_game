//! 会计期间与封账状态（K2）：`AccountingPeriod`（年/月）+ `PeriodStates`
//! （Open/Closed 状态 + 已封期间入账守卫）。结账机制本体（试算/结转/快照）
//! 属任务 13；本模块只提供期间类型、状态与守卫。日历事实复用
//! `crate::calendar::CivilDate`（K1：金额/日期/交易分钟不混用）。

use std::collections::BTreeSet;
use std::fmt;

use crate::accounting::error::AccountingError;
use crate::calendar::{CivilDate, CIVIL_YEAR_MAX, CIVIL_YEAR_MIN};

/// 自然月度会计期间（会计年度 = 自然年，K4）。serde = `YYYY-MM` 字符串。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct AccountingPeriod {
    year: i32,
    month: u8,
}

impl AccountingPeriod {
    /// 构造并校验：月 ∈ 1..=12、年 ∈ 1900..=2199（与 CivilDate 算法窗一致）。
    pub fn from_ymd(year: i32, month: u8) -> Result<Self, AccountingError> {
        if !(CIVIL_YEAR_MIN..=CIVIL_YEAR_MAX).contains(&year) {
            return Err(AccountingError::InvalidPeriod {
                year,
                month,
                reason: "year outside civil algorithm window 1900..=2199",
            });
        }
        if !(1..=12).contains(&month) {
            return Err(AccountingError::InvalidPeriod {
                year,
                month,
                reason: "month must be 1..=12",
            });
        }
        Ok(Self { year, month })
    }

    /// 由（已验证的）`CivilDate` 取期间：日期合法 ⇒ 期间合法，不可失败。
    pub fn of_date(date: CivilDate) -> Self {
        Self {
            year: date.year(),
            month: date.month(),
        }
    }

    /// 严格解析 `YYYY-MM`（4 位年、2 位月、恰好一个连字符）。
    pub fn from_iso(input: &str) -> Result<Self, AccountingError> {
        let bad = |reason: &str| AccountingError::PeriodParse {
            input: input.to_string(),
            reason: reason.to_string(),
        };
        let bytes = input.as_bytes();
        if bytes.len() != 7 || bytes[4] != b'-' {
            return Err(bad("expected strict YYYY-MM shape"));
        }
        if !bytes[..4].iter().all(u8::is_ascii_digit) || !bytes[5..].iter().all(u8::is_ascii_digit)
        {
            return Err(bad("non-digit component"));
        }
        let year = input[..4]
            .parse::<i32>()
            .map_err(|_| bad("year overflow"))?;
        let month = input[5..]
            .parse::<u8>()
            .map_err(|_| bad("month overflow"))?;
        Self::from_ymd(year, month).map_err(|_| bad("component out of range"))
    }

    pub fn year(self) -> i32 {
        self.year
    }

    pub fn month(self) -> u8 {
        self.month
    }

    /// `YYYY-MM`（Display 与 serde 同形）。
    pub fn to_iso(self) -> String {
        format!("{:04}-{:02}", self.year, self.month)
    }
}

impl fmt::Display for AccountingPeriod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_iso())
    }
}

impl fmt::Debug for AccountingPeriod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AccountingPeriod({})", self.to_iso())
    }
}

impl serde::Serialize for AccountingPeriod {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_iso())
    }
}

impl<'de> serde::Deserialize<'de> for AccountingPeriod {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_iso(&text).map_err(serde::de::Error::custom)
    }
}

/// 期间封账状态。`Closed` 仅表示「拒绝新入账」；结账产物在任务 13。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum PeriodStatus {
    Open,
    Closed,
}

/// 已封期间集合：缺省即 Open。随存档保存（K7 恢复后状态一致）。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PeriodStates {
    closed: BTreeSet<AccountingPeriod>,
}

impl PeriodStates {
    pub fn new() -> Self {
        Self::default()
    }

    /// 封账（幂等性拒绝：重复封账 → `PeriodAlreadyClosed`）。
    pub fn close(&mut self, period: AccountingPeriod) -> Result<(), AccountingError> {
        if self.closed.contains(&period) {
            return Err(AccountingError::PeriodAlreadyClosed { period });
        }
        self.closed.insert(period);
        Ok(())
    }

    pub fn status(&self, period: AccountingPeriod) -> PeriodStatus {
        if self.closed.contains(&period) {
            PeriodStatus::Closed
        } else {
            PeriodStatus::Open
        }
    }

    pub fn is_closed(&self, period: AccountingPeriod) -> bool {
        self.closed.contains(&period)
    }

    /// 已封期间升序枚举（恢复重放用）。
    pub fn closed_periods(&self) -> impl Iterator<Item = AccountingPeriod> + '_ {
        self.closed.iter().copied()
    }
}
