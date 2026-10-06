use super::InformationError;
use crate::accounting::AccountingPeriod;
use crate::calendar::{CivilDate, CivilInstant};
use crate::company::CompanyId;

#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum MonthlyReportDelay {
    None,
    Uniform { max_days: u8 },
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum MonthlyReportPreset {
    FirstDayEvening,
    TenthDayEvening,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum MonthlyReportSchedule {
    Preset {
        preset: MonthlyReportPreset,
        delay: MonthlyReportDelay,
    },
    Custom {
        day: u8,
        second_of_day: u32,
        delay: MonthlyReportDelay,
    },
}

impl MonthlyReportSchedule {
    pub fn validate(self) -> Result<(), InformationError> {
        let (day, second, delay) = self.parts();
        if !(1..=28).contains(&day)
            || second >= 86400
            || matches!(
                delay,
                MonthlyReportDelay::Uniform {
                    max_days: 0 | 32..=255
                }
            )
        {
            return Err(InformationError::InconsistentLibrary {
                detail: "月报日期须为次月1–28日、时刻须为当天有效秒数，随机延迟上限须为1–31日"
                    .into(),
            });
        }
        Ok(())
    }

    fn parts(self) -> (u8, u32, MonthlyReportDelay) {
        match self {
            Self::Preset { preset, delay } => (
                match preset {
                    MonthlyReportPreset::FirstDayEvening => 1,
                    MonthlyReportPreset::TenthDayEvening => 10,
                },
                18 * 3600,
                delay,
            ),
            Self::Custom {
                day,
                second_of_day,
                delay,
            } => (day, second_of_day, delay),
        }
    }

    pub fn delay_days(
        self,
        seed: u64,
        company: &CompanyId,
        period: AccountingPeriod,
    ) -> Result<u8, InformationError> {
        self.validate()?;
        let MonthlyReportDelay::Uniform { max_days } = self.parts().2 else {
            return Ok(0);
        };
        let mut hash = seed ^ 0xcbf2_9ce4_8422_2325;
        let year_bytes = period.year().to_le_bytes();
        let month_bytes = [period.month()];
        for byte in b"monthly-report-delay"
            .iter()
            .chain(company.0.as_bytes())
            .chain(year_bytes.iter())
            .chain(month_bytes.iter())
        {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let mut mixed = hash;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        Ok(((mixed ^ (mixed >> 31)) % (u64::from(max_days) + 1)) as u8)
    }

    pub fn instant(
        self,
        period: AccountingPeriod,
        delay_days: u8,
    ) -> Result<CivilInstant, InformationError> {
        self.validate()?;
        let (day, second, delay) = self.parts();
        let maximum = match delay {
            MonthlyReportDelay::None => 0,
            MonthlyReportDelay::Uniform { max_days } => max_days,
        };
        if delay_days > maximum {
            return Err(InformationError::InconsistentLibrary {
                detail: "月报实际随机延迟超出配置".into(),
            });
        }
        let (year, month) = if period.month() == 12 {
            (period.year() + 1, 1)
        } else {
            (period.year(), period.month() + 1)
        };
        let mut date = CivilDate::from_ymd(year, month, day)?;
        for _ in 0..delay_days {
            date = date.next()?;
        }
        Ok(CivilInstant::new(date, second)?)
    }
}
