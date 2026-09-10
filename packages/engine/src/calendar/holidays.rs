//! 交易日解析（K1 §3.3）：周末规则 → 官方覆盖 → 模拟回退；前史查询与序数。
//!
//! 规则优先级：周六/周日休市（调休补班周末**不是**交易日）→ 当年存在官方
//! 覆盖条目则按其闭市区间休市（真实公告永远覆盖模拟结果）→ 无覆盖年份按
//! §3.3 模拟规则（元旦 1-1；劳动 5-1..5-2；国庆 10-1..10-3；春节除夕至正月初三；
//! 清明/端午/中秋当日，农历日期来自政策内嵌事实表）。
//!
//! 1998–1999 是**初始化专用**段：日历查询可触达（前史 K 线/财务），但
//! `validate_runtime_start` 拒绝其作为运行时开局——两道独立查询门。

use super::date::CivilDate;
use super::policy::{CalendarExchange, CalendarPolicy, YearCoverageLabel};
use super::{CalendarError, ClosedReason, DayStatus, HolidayKind, TradingDayOrdinal};

/// 由一份完整 `CalendarPolicy` 构建的交易日历。同政策 → 同结果（确定性）；
/// 恢复存档用 `from_policy`（自洽校验后完全按存档政策服务，不读新默认表）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct TradingCalendar {
    policy: CalendarPolicy,
}

impl TradingCalendar {
    /// 从（可能是恢复出的）政策构建：先全量校验政策自洽性。
    pub fn from_policy(policy: CalendarPolicy) -> Result<Self, CalendarError> {
        policy.validate()?;
        Ok(Self { policy })
    }

    /// 当前发布默认政策 v1。
    pub fn default_v1() -> Result<Self, CalendarError> {
        Self::from_policy(CalendarPolicy::default_v1()?)
    }

    pub fn policy(&self) -> &CalendarPolicy {
        &self.policy
    }

    /// 运行时开局门：只接受 [runtime_min_start, runtime_max_end]。
    /// 1998–1999（初始化专用段）在此门被拒——不开放为开局选择。
    pub fn validate_runtime_start(&self, date: CivilDate) -> Result<(), CalendarError> {
        let min = self.policy.runtime_min_start();
        let max = self.policy.runtime_max_end();
        if date < min || date > max {
            return Err(CalendarError::RuntimeStartOutOfRange { date, min, max });
        }
        Ok(())
    }

    /// 日历查询门：覆盖区间 [init_only_min_start, runtime_max_end]（含前史段）。
    fn ensure_within_applicability(&self, date: CivilDate) -> Result<(), CalendarError> {
        let floor = self.policy.init_only_min_start();
        let ceiling = self.policy.runtime_max_end();
        if date < floor {
            return Err(CalendarError::BeforeInitFloor { date, floor });
        }
        if date > ceiling {
            return Err(CalendarError::AfterRuntimeCeiling { date, ceiling });
        }
        Ok(())
    }

    /// 单日状态（周末 → 官方覆盖 → 模拟回退）。
    pub fn day_status(
        &self,
        exchange: CalendarExchange,
        date: CivilDate,
    ) -> Result<DayStatus, CalendarError> {
        self.ensure_within_applicability(date)?;
        Ok(self.day_status_inner(exchange, date))
    }

    fn day_status_inner(&self, exchange: CalendarExchange, date: CivilDate) -> DayStatus {
        if date.weekday().is_weekend() {
            return DayStatus::Closed(ClosedReason::Weekend);
        }
        if let Some(entry) = self
            .policy
            .official_coverage()
            .iter()
            .find(|e| e.exchange == exchange && e.year == date.year())
        {
            if entry.covers(date) {
                return DayStatus::Closed(ClosedReason::OfficialHoliday {
                    citation_id: entry.source_citation_id.clone(),
                });
            }
        }
        if let Some(kind) = self.simulated_holiday_kind(date) {
            return DayStatus::Closed(ClosedReason::SimulatedHoliday(kind));
        }
        DayStatus::Trading
    }

    /// §3.3 模拟假日规则。事实表年份缺口在政策装配时已拒绝，此处不再出现。
    fn simulated_holiday_kind(&self, date: CivilDate) -> Option<HolidayKind> {
        let facts = &self.policy.simulated_fallback().lunar_facts;
        let fact = facts.fact_for_year(date.year()).ok()?;
        let cny = fact.lunar_new_year;
        // 春节除夕至正月初三 = 正月初一前 1 后 2 共 4 天（春节不早于 1-21，
        // 两端不会越出日期算法窗）。
        let cny_eve = cny.prev().ok()?;
        let chusi = cny.next().ok()?.next().ok()?;
        if date >= cny_eve && date <= chusi {
            return Some(HolidayKind::SpringFestival);
        }
        match (date.month(), date.day()) {
            (1, 1) => Some(HolidayKind::NewYearDay),
            (5, 1..=2) => Some(HolidayKind::LabourDay),
            (10, 1..=3) => Some(HolidayKind::NationalDay),
            _ if date == fact.qingming => Some(HolidayKind::Qingming),
            _ if date == fact.dragon_boat => Some(HolidayKind::DragonBoat),
            _ if date == fact.mid_autumn => Some(HolidayKind::MidAutumn),
            _ => None,
        }
    }

    pub fn is_trading_day(
        &self,
        exchange: CalendarExchange,
        date: CivilDate,
    ) -> Result<bool, CalendarError> {
        Ok(matches!(
            self.day_status(exchange, date)?,
            DayStatus::Trading
        ))
    }

    fn is_trading_unchecked(&self, exchange: CalendarExchange, date: CivilDate) -> bool {
        matches!(self.day_status_inner(exchange, date), DayStatus::Trading)
    }

    /// `after` 之后（不含）的第一个交易日；覆盖区间内不存在则类型化越界错误。
    pub fn next_trading_day(
        &self,
        exchange: CalendarExchange,
        after: CivilDate,
    ) -> Result<CivilDate, CalendarError> {
        self.ensure_within_applicability(after)?;
        let ceiling = self.policy.runtime_max_end();
        let mut cur = after;
        loop {
            cur = cur.next()?;
            if cur > ceiling {
                return Err(CalendarError::AfterRuntimeCeiling { date: cur, ceiling });
            }
            if self.is_trading_unchecked(exchange, cur) {
                return Ok(cur);
            }
        }
    }

    /// `before` 之前（不含）的第一个交易日；早于初始化下界则类型化错误。
    pub fn previous_trading_day(
        &self,
        exchange: CalendarExchange,
        before: CivilDate,
    ) -> Result<CivilDate, CalendarError> {
        self.ensure_within_applicability(before)?;
        let floor = self.policy.init_only_min_start();
        let mut cur = before;
        loop {
            cur = cur.prev()?;
            if cur < floor {
                return Err(CalendarError::BeforeInitFloor { date: cur, floor });
            }
            if self.is_trading_unchecked(exchange, cur) {
                return Ok(cur);
            }
        }
    }

    /// `date` 之前（不含）第 `n` 个交易日（n ≥ 1）；前史耗尽则类型化错误。
    /// 供 2000-01-01 最早开局的 360 交易日 K 线前史定位。
    pub fn trading_days_before(
        &self,
        exchange: CalendarExchange,
        date: CivilDate,
        n: u32,
    ) -> Result<CivilDate, CalendarError> {
        self.ensure_within_applicability(date)?;
        if n == 0 {
            return Err(CalendarError::InvalidCount {
                what: "trading day count",
                count: 0,
            });
        }
        let floor = self.policy.init_only_min_start();
        let mut cur = date;
        let mut remaining = n;
        loop {
            cur = cur.prev()?;
            if cur < floor {
                return Err(CalendarError::PrehistoryExhausted {
                    needed: n,
                    available: n - remaining,
                    date,
                    floor,
                });
            }
            if self.is_trading_unchecked(exchange, cur) {
                remaining -= 1;
                if remaining == 0 {
                    return Ok(cur);
                }
            }
        }
    }

    /// 交易日序数（0 = 初始化下界起第一个交易日）；非交易日为类型化错误。
    pub fn trading_day_ordinal(
        &self,
        exchange: CalendarExchange,
        date: CivilDate,
    ) -> Result<TradingDayOrdinal, CalendarError> {
        self.ensure_within_applicability(date)?;
        if !self.is_trading_unchecked(exchange, date) {
            let DayStatus::Closed(reason) = self.day_status_inner(exchange, date) else {
                unreachable!("checked is_trading_unchecked above");
            };
            return Err(CalendarError::NotATradingDay {
                exchange,
                date,
                reason,
            });
        }
        let floor = self.policy.init_only_min_start();
        let mut cur = floor;
        let mut ordinal = 0u32;
        while cur < date {
            if self.is_trading_unchecked(exchange, cur) {
                ordinal += 1;
            }
            cur = cur.next()?;
        }
        Ok(TradingDayOrdinal::from_u32(ordinal))
    }

    /// 序数 → 日期（序数越界为类型化错误，不循环）。
    pub fn date_of_ordinal(
        &self,
        exchange: CalendarExchange,
        ordinal: TradingDayOrdinal,
    ) -> Result<CivilDate, CalendarError> {
        let floor = self.policy.init_only_min_start();
        let ceiling = self.policy.runtime_max_end();
        let mut cur = floor;
        let mut remaining = ordinal.value();
        loop {
            if cur > ceiling {
                return Err(CalendarError::AfterRuntimeCeiling { date: cur, ceiling });
            }
            if self.is_trading_unchecked(exchange, cur) {
                if remaining == 0 {
                    return Ok(cur);
                }
                remaining -= 1;
            }
            cur = cur.next()?;
        }
    }

    /// 年份覆盖标签（委托政策；存档冻结语义同源）。
    pub fn year_label(
        &self,
        exchange: CalendarExchange,
        year: i32,
    ) -> Result<YearCoverageLabel, CalendarError> {
        self.policy.year_label(exchange, year)
    }
}
