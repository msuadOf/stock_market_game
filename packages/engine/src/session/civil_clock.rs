//! 自然日经营时钟：与市场时钟共同构成权威双时钟。
//!
//! 职责边界：本模块只负责**自然日**语义，不触碰 tick/市场分钟/撮合——
//! - 交易日：[`super::GameSession::step`] 照旧推进交易时间；收盘后由
//!   [`super::GameSession::end_civil_day`] 依次执行当日经营终局窗口、
//!   18:00 披露阶段（hook），然后才前进到次日自然日。
//! - 休市日：无 tick、无成交、无注意力 RNG 消费；公司经营仍按自然日推进。
//!   休市起点保持真实起点，不静默挪到开市日。
//!
//! 公司 due 的单一 authority 在 `OperatingScheduler`（`CompanyOperations` 持有）；
//! 本时钟不镜像逐项公司待办、不承载金额与入账，也不派发公司到期业务。
//! DisclosureDispatch 安装无状态的 18:00 相位观察者，GameSession 在同一相位
//! 执行有状态披露派发。所有日结失败先验证并原子拒绝；任何 `Err` 返回时
//! 时钟与会话状态保持原样。

use super::{Event, StockExchange};
use crate::calendar::{
    CalendarError, CalendarExchange, CivilDate, CivilDateError, CivilInstant, DayStatus,
    TradingCalendar,
};
use thiserror::Error;

/// 18:00 披露阶段观察者，由 DisclosureDispatch 安装生产相位 hook。
pub type DisclosureObserver = fn(CivilInstant);

/// 自然日当前稳定阶段。收盘后经营终局窗口与 18:00 披露阶段是
/// [`CivilClock::end_day`] 内部的瞬时窗口，经 [`CivilDayEndReport`] 与
/// 观察者调用可观测，不是可停留状态。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum CivilPhase {
    /// 交易日盘中（tick/市场分钟权威在 `step`）。
    IntradayTrading,
    /// 休市日（无行情；公司经营照常按自然日推进）。
    ClosedDay,
}

/// 一次自然日日结的权威记录（civil 事件面）。
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct CivilDayEndReport {
    /// 刚刚日结的自然日。
    pub settled_date: CivilDate,
    /// 当日 18:00 披露阶段的瞬间（观察者在此时点被调用）。
    pub disclosure_instant: CivilInstant,
    /// 日结后的新自然日及其日历状态。
    pub next_date: CivilDate,
    /// 共享聚合诊断；ClosedReason 取 canonical 第一所的代表原因，不表示逐所原因相同。
    pub next_status: DayStatus,
    /// 日结成功后由会话追加的共享公开事件，顺序属于全局 session seq。
    pub events: Vec<Event>,
}

/// 存档中的自然日时钟状态；恢复时按存档携带的
/// [`CalendarPolicySpec`] 重建日历，并比较存档 `policy_digest` 与重算值，
/// 绝不被当前进程的默认政策表覆盖。观察者是进程内 hook，不入档。
/// 公司到期不在此处镜像（单一 authority 在 `OperatingScheduler`），
/// 携带旧镜像字段（`pending_due`/`next_due_seq`）的 JSON 显式拒绝。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct CivilClockSave {
    pub current_date: CivilDate,
    /// 最后一个已完成日结的自然日；`None` = 尚未日结（current == start）。
    pub settled_through: Option<CivilDate>,
    /// 本局使用的完整日历政策事实（算法版本/覆盖/回退/事实表）。
    #[ts(skip)]
    pub policy: crate::calendar::CalendarPolicySpec,
    /// 日历政策 `CalendarPolicySpec` 的 `content_digest`。新档必须与从
    /// `policy` 重算的值一致；升级前无此字段的旧档仅允许 policy spec
    /// 精确等于升级前默认政策时按冻结模拟政策恢复。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_digest: Option<String>,
}

/// 自然日时钟错误（类型化）。任何变体返回时状态零变更。
#[derive(Debug, Error)]
pub enum CivilClockError {
    /// 透传交易日历错误。
    #[error(transparent)]
    Calendar(#[from] CalendarError),
    /// 日期算法层错误（越过 1900–2199 验证窗；运行区间内正常不可达）。
    #[error("civil date error: {0}")]
    CivilDate(#[from] CivilDateError),
    /// 日结需要前进到运行上界之外（2099-12-31 之后没有下一个自然日）。
    #[error("civil day-end would advance beyond runtime ceiling to {next} (ceiling {ceiling})")]
    BeyondRuntimeCeiling { next: CivilDate, ceiling: CivilDate },
    /// 同一自然日重复日结。
    #[error(
        "duplicate day-end settlement for {date} (already settled through {settled_through:?})"
    )]
    DuplicateDayEnd {
        date: CivilDate,
        settled_through: Option<CivilDate>,
    },
    /// 日时钟回拨（对早于当前、且非恰好重复上一日的日期日结）。
    #[error("out-of-order day-end for {date}: civil clock is at {current}")]
    OutOfOrderDayEnd { date: CivilDate, current: CivilDate },
    /// 跳日：自然日必须逐日日结；被跳过区间的公司经营不允许静默丢失。
    #[error("cannot end {to}: civil days [{from}, {to}) would be skipped")]
    SkippedCivilDays { from: CivilDate, to: CivilDate },
    /// 市场时间与自然日失步（当日会话未跑完就日结，或多跑了会话）。
    #[error(
        "cannot end civil day {date}: market sessions out of sync (completed {completed_sessions}, expected {expected_sessions})"
    )]
    MarketSessionOutOfSync {
        date: CivilDate,
        completed_sessions: u32,
        expected_sessions: u32,
    },
    /// 时钟状态自相矛盾（含存档恢复校验失败）。
    #[error("inconsistent civil clock state: {detail}")]
    SaveInconsistent { detail: String },
}

/// 自然日经营时钟：与 tick/市场分钟严格分离的权威自然日推进（冻结日历与双时钟）。
///
/// 持有随存档冻结的会话级交易日历政策；公司到期业务队列在
/// `OperatingScheduler`（单一 authority），本时钟不镜像。
/// 不持有任何市场状态——休市推进因此天然不产生 tick/成交/RNG 消费。
#[derive(Clone, Debug)]
pub struct CivilClock {
    calendar: TradingCalendar,
    exchanges: Vec<CalendarExchange>,
    start_date: CivilDate,
    current_date: CivilDate,
    settled_through: Option<CivilDate>,
    disclosure_observers: Vec<DisclosureObserver>,
}

/// 将证券上市所映射为冻结日历的交易所维度。
pub(super) fn session_calendar_exchange(exchange: StockExchange) -> CalendarExchange {
    match exchange {
        StockExchange::Shanghai => CalendarExchange::Sse,
        StockExchange::Shenzhen => CalendarExchange::Szse,
    }
}

/// serde 缺省开局日期为 2030-01-01，与当前默认政策的 default_start 一致。
pub(super) fn default_civil_start_date() -> CivilDate {
    CivilDate::from_ymd(2030, 1, 1).expect("2030-01-01 is a statically valid civil date")
}

impl CivilClock {
    /// 用当前默认政策构建，开局日期须在运行区间 2000-01-01..2099-12-31。
    /// 休市开局保持真实起点（phase = `ClosedDay`），不挪到开市日。
    pub fn new(start_date: CivilDate, exchange: CalendarExchange) -> Result<Self, CivilClockError> {
        Self::new_for_exchanges(start_date, [exchange])
    }

    /// 按本局实际配置的非空交易所集合构建；输入排序、去重后决定共享会话。
    pub fn new_for_exchanges(
        start_date: CivilDate,
        exchanges: impl IntoIterator<Item = CalendarExchange>,
    ) -> Result<Self, CivilClockError> {
        let exchanges = Self::canonical_exchanges(exchanges)?;
        let calendar = TradingCalendar::current_default_calendar()?;
        calendar.validate_runtime_start(start_date)?;
        Ok(Self {
            calendar,
            exchanges,
            start_date,
            current_date: start_date,
            settled_through: None,
            disclosure_observers: Vec::new(),
        })
    }

    /// 从存档携带的 policy spec 重建并完整校验日历，
    /// 不回退 current_default_policy 或覆盖冻结政策。
    pub fn from_parts(
        start_date: CivilDate,
        save: &CivilClockSave,
        exchange: CalendarExchange,
    ) -> Result<Self, CivilClockError> {
        Self::from_parts_for_exchanges(start_date, save, [exchange])
    }

    /// 以存档冻结政策与 setup 的完整交易所集合恢复，不增加存档派生字段。
    pub fn from_parts_for_exchanges(
        start_date: CivilDate,
        save: &CivilClockSave,
        exchanges: impl IntoIterator<Item = CalendarExchange>,
    ) -> Result<Self, CivilClockError> {
        let exchanges = Self::canonical_exchanges(exchanges)?;
        let policy = crate::calendar::CalendarPolicy::from_parts(save.policy.clone())?;
        match save.policy_digest.as_deref() {
            Some(stored) => {
                if stored != policy.content_digest() {
                    return Err(CalendarError::DigestMismatch {
                        field: "content_digest",
                        stored: stored.to_string(),
                        computed: policy.content_digest().to_string(),
                    }
                    .into());
                }
            }
            None if !save.policy.official_coverage.is_empty() => {
                return Err(CivilClockError::SaveInconsistent {
                    detail: "missing policy_digest for saved official coverage".into(),
                });
            }
            None => {
                // 升级前旧档没有 policy_digest。只接受与 0f40ffbc 冻结政策完全
                // 相同的内容摘要，避免“缺摘要”掩盖算法版本、边界、回退事实或
                // 来源区间的静默改写。新档由 save() 总会写入摘要。
                if policy.content_digest()
                    != crate::calendar::PRE_2026_LEGACY_CALENDAR_POLICY_DIGEST
                {
                    return Err(CivilClockError::SaveInconsistent {
                        detail: "missing policy_digest is only allowed for the frozen pre-2026 default policy".into(),
                    });
                }
            }
        }
        let calendar = TradingCalendar::from_policy(policy)?;
        calendar.validate_runtime_start(start_date)?;
        let inconsistent = |detail: String| CivilClockError::SaveInconsistent { detail };
        if save.current_date < start_date {
            return Err(inconsistent(format!(
                "current {} precedes start {start_date}",
                save.current_date
            )));
        }
        match save.settled_through {
            None => {
                if save.current_date != start_date {
                    return Err(inconsistent(format!(
                        "nothing settled yet but current {} != start {start_date}",
                        save.current_date
                    )));
                }
            }
            Some(settled) => {
                if settled < start_date || settled.next().ok() != Some(save.current_date) {
                    return Err(inconsistent(format!(
                        "settled_through {settled} is not the civil day before current {}",
                        save.current_date
                    )));
                }
            }
        }
        // 日期相邻不保证 current 仍在政策范围；返回前拒绝越界，避免 phase panic。
        calendar.day_status(exchanges[0], save.current_date)?;
        Ok(Self {
            calendar,
            exchanges,
            start_date,
            current_date: save.current_date,
            settled_through: save.settled_through,
            disclosure_observers: Vec::new(),
        })
    }

    fn canonical_exchanges(
        exchanges: impl IntoIterator<Item = CalendarExchange>,
    ) -> Result<Vec<CalendarExchange>, CivilClockError> {
        let mut exchanges: Vec<_> = exchanges.into_iter().collect();
        exchanges.sort_unstable();
        exchanges.dedup();
        if exchanges.is_empty() {
            return Err(CivilClockError::SaveInconsistent {
                detail: "civil clock requires at least one configured exchange".into(),
            });
        }
        Ok(exchanges)
    }

    /// 任一实际交易所开市即为共享会话；全闭市时按 canonical 顺序取代表性原因。
    /// 该 ClosedReason 只供共享诊断，逐所原因仍须查询本股交易所的冻结日历。
    fn shared_day_status(&self, date: CivilDate) -> Result<DayStatus, CalendarError> {
        let status = self.calendar.day_status(self.exchanges[0], date)?;
        for exchange in &self.exchanges[1..] {
            if self.calendar.day_status(*exchange, date)? == DayStatus::Trading {
                return Ok(DayStatus::Trading);
            }
        }
        Ok(status)
    }

    pub fn start_date(&self) -> CivilDate {
        self.start_date
    }

    pub fn current_date(&self) -> CivilDate {
        self.current_date
    }

    /// 下一 civil 事件所在自然日：当日经营终局与 18:00 披露每天都须完成，
    /// 包括没有公司 due 的休市日，不能因队列为空而跳过自然日日结。
    pub fn next_civil_event_date(&self) -> CivilDate {
        self.current_date
    }

    pub fn settled_through(&self) -> Option<CivilDate> {
        self.settled_through
    }

    /// 当前稳定阶段（交易日盘中 / 休市日）。
    pub fn phase(&self) -> CivilPhase {
        let status = self
            .shared_day_status(self.current_date)
            .expect("clock dates stay within calendar applicability by construction");
        match status {
            DayStatus::Trading => CivilPhase::IntradayTrading,
            DayStatus::Closed(_) => CivilPhase::ClosedDay,
        }
    }

    /// 添加 18:00 披露观察者；生产 hook 由 DisclosureDispatch 安装。
    pub fn add_disclosure_observer(&mut self, observer: DisclosureObserver) {
        self.disclosure_observers.push(observer);
    }

    /// 本局冻结的交易日历；市场入口与逐股历史使用同一本政策。
    pub(super) fn calendar(&self) -> &TradingCalendar {
        &self.calendar
    }

    /// 校验公司 due 日期落在冻结日历的适用范围内（公司 due 单一 authority 后，
    /// 本时钟承担日历上界守卫：越界待办失去恰好一次派发机会，须显式拒绝）。
    pub fn validate_due_horizon(&self, due_date: CivilDate) -> Result<(), CivilClockError> {
        // 第一所仅用于验证统一适用范围；公司事项不会因该所休市而被拒绝。
        self.calendar.day_status(self.exchanges[0], due_date)?;
        Ok(())
    }

    pub(super) fn disclosure_observers(&self) -> &[DisclosureObserver] {
        &self.disclosure_observers
    }

    /// 截至 current（含）应已完成的市场会话数（会话同步守卫用）。
    /// 按交易所集合的 union 计数；同日多所开市只计一次，全闭市不欠会话。
    pub fn completed_trading_sessions_expected(&self) -> Result<u32, CivilClockError> {
        let mut count = 0u32;
        let mut cursor = self.start_date;
        while cursor <= self.current_date {
            if self.shared_day_status(cursor)? == DayStatus::Trading {
                count += 1;
            }
            cursor = cursor.next()?;
        }
        Ok(count)
    }

    /// 自然日日结（冻结日历与双时钟）：**先全量验证，后原子应用**。
    ///
    /// 校验顺序：目标日期与时钟关系（重复/回拨/跳日）→ 次日不越运行上界。
    /// 应用段：记录日结、前进次日。公司到期的恰好一次派发由会话在返回后经
    /// `CompanyOperations::advance_civil_day(settled_date)` 完成（单一 authority：
    /// `OperatingScheduler`）；18:00 观察者由 `GameSession` 在完整日结成功后调用。
    pub fn end_day(&mut self, date: CivilDate) -> Result<CivilDayEndReport, CivilClockError> {
        let report = self.plan_day_end(date)?;
        self.settled_through = Some(report.settled_date);
        self.current_date = report.next_date;
        Ok(report)
    }

    /// 只读规划与 `end_day` 共用全部时钟检查，供 Session 在进入 civil 事务前拒绝确定性越界。
    pub(super) fn plan_day_end(
        &self,
        date: CivilDate,
    ) -> Result<CivilDayEndReport, CivilClockError> {
        if self.settled_through == Some(date) {
            return Err(CivilClockError::DuplicateDayEnd {
                date,
                settled_through: self.settled_through,
            });
        }
        if date < self.current_date {
            return Err(CivilClockError::OutOfOrderDayEnd {
                date,
                current: self.current_date,
            });
        }
        if date > self.current_date {
            return Err(CivilClockError::SkippedCivilDays {
                from: self.current_date,
                to: date,
            });
        }
        let next = self.current_date.next()?;
        let ceiling = self.calendar.policy().runtime_max_end();
        if next > ceiling {
            return Err(CivilClockError::BeyondRuntimeCeiling { next, ceiling });
        }
        let settled_date = self.current_date;
        let disclosure_instant =
            CivilInstant::from_hms(settled_date, 18, 0, 0).expect("18:00:00 is a valid instant");
        let next_status = self.shared_day_status(next)?;
        Ok(CivilDayEndReport {
            settled_date,
            disclosure_instant,
            next_date: next,
            next_status,
            events: Vec::new(),
        })
    }

    /// 导出可存档状态（观察者不入档：进程内 hook，恢复后由宿主重装）。
    pub fn save(&self) -> CivilClockSave {
        CivilClockSave {
            current_date: self.current_date,
            settled_through: self.settled_through,
            policy: self.calendar.policy().spec(),
            policy_digest: Some(self.calendar.policy().content_digest().to_string()),
        }
    }
}
