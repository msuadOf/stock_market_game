//! 自然日经营时钟（K1，任务 5）：权威推进的双时钟之一。
//!
//! 职责边界：本模块只负责**自然日**语义，不触碰 tick/市场分钟/撮合——
//! - 交易日：[`super::GameSession::step`] 照旧推进交易时间；收盘后由
//!   [`super::GameSession::end_civil_day`] 依次执行当日经营终局窗口、
//!   18:00 披露阶段（hook），然后才前进到次日自然日。
//! - 休市日：无 tick、无成交、无注意力 RNG 消费；当日到期业务恰好一次
//!   派发，再前进。休市起点保持真实起点，不静默挪到开市日。
//!
//! 钩子接缝（诚实声明）：利息/到期真实业务由任务 14 实现，本模块只提供
//! "带种类的到期队列 + 按日期恰好一次派发"的调度面，派发结果经
//! [`CivilDayEndReport`] 返回；18:00 披露阶段是 fn 指针观察者列表，生产
//! 路径为空，任务 15 填充。所有日结失败**先验证、原子拒绝**：任何 `Err`
//! 返回时时钟与会话状态保持原样。

use super::StockExchange;
use crate::calendar::{
    CalendarError, CalendarExchange, CivilDate, CivilDateError, CivilInstant, DayStatus,
    TradingCalendar,
};
use thiserror::Error;

/// 到期业务种类。任务 5 只承载时钟侧调度语义；金额与入账由任务 14 实现。
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
pub enum DueKind {
    /// 按自然日计提的合同利息（ACT/365F，K2 游戏假设）。
    InterestAccrual,
    /// 合同到期收付/回调。
    ContractMaturity,
}

/// 到期业务 id：时钟注册时分配，单调递增不复用。
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
#[serde(transparent)]
#[ts(type = "number")]
pub struct DueBusinessId(u32);

impl DueBusinessId {
    pub fn value(self) -> u32 {
        self.0
    }
}

/// 一条已注册的到期业务（随存档持久化）。
#[derive(
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
pub struct DueBusiness {
    pub id: DueBusinessId,
    pub due_date: CivilDate,
    pub kind: DueKind,
}

/// 18:00 披露阶段观察者（hook）。生产路径为空列表；任务 15 接入真实披露。
pub type DisclosureObserver = fn(CivilInstant);

/// 自然日当前稳定阶段。收盘后经营终局窗口与 18:00 披露阶段是
/// [`CivilClock::end_day`] 内部的瞬时窗口，经 [`CivilDayEndReport`] 与
/// 观察者调用可观测，不是可停留状态。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum CivilPhase {
    /// 交易日盘中（tick/市场分钟权威在 `step`）。
    IntradayTrading,
    /// 休市日（无行情；到期业务照常处理）。
    ClosedDay,
}

/// 一次自然日日结的权威记录（civil 事件面）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct CivilDayEndReport {
    /// 刚刚日结的自然日。
    pub settled_date: CivilDate,
    /// 该日恰好一次派发的到期业务（按注册先后）。
    pub dispatched_due: Vec<DueBusiness>,
    /// 当日 18:00 披露阶段的瞬间（观察者在此时点被调用）。
    pub disclosure_instant: CivilInstant,
    /// 日结后的新自然日及其日历状态。
    pub next_date: CivilDate,
    pub next_status: DayStatus,
}

/// 存档中的自然日时钟状态。日历政策本体由任务 27 冻结进档；当前按
/// default_v1 重建。观察者是进程内 hook，不入档。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct CivilClockSave {
    pub current_date: CivilDate,
    /// 最后一个已完成日结的自然日；`None` = 尚未日结（current == start）。
    pub settled_through: Option<CivilDate>,
    pub next_due_seq: u32,
    /// 待派发到期业务，按 (due_date, id) 升序。
    pub pending_due: Vec<DueBusiness>,
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
    /// 跳日：自然日必须逐日日结；被跳过区间的到期业务不允许静默丢失。
    #[error(
        "cannot end {to}: civil days [{from}, {to}) would be skipped with {} unprocessed due item(s)",
        unprocessed.len()
    )]
    SkippedCivilDays {
        from: CivilDate,
        to: CivilDate,
        unprocessed: Vec<DueBusiness>,
    },
    /// 到期业务注册在过去（当日之前的 due 无法再恰好一次派发）。
    #[error("cannot register {kind:?} due on {due_date}: civil clock is already at {current}")]
    DueRegistrationInPast {
        kind: DueKind,
        due_date: CivilDate,
        current: CivilDate,
    },
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

/// 自然日经营时钟：与 tick/市场分钟严格分离的权威自然日推进（K1）。
///
/// 持有会话级交易日历（政策 v1；任务 27 起随存档冻结）与到期业务队列，
/// 不持有任何市场状态——休市推进因此天然不产生 tick/成交/RNG 消费。
#[derive(Debug)]
pub struct CivilClock {
    calendar: TradingCalendar,
    exchange: CalendarExchange,
    start_date: CivilDate,
    current_date: CivilDate,
    settled_through: Option<CivilDate>,
    next_due_seq: u32,
    pending: Vec<DueBusiness>,
    disclosure_observers: Vec<DisclosureObserver>,
}

/// 会话级日历参考所：A 股会话共用一套休市节奏，政策 v1 下沪深同轨。
/// 取 setup 首只股票的上市所，保证确定性（K1 任务 5 会话接线）。
pub(super) fn session_calendar_exchange(first: StockExchange) -> CalendarExchange {
    match first {
        StockExchange::Shanghai => CalendarExchange::Sse,
        StockExchange::Shenzhen => CalendarExchange::Szse,
    }
}

/// K1 默认开局日期 = 政策 v1 `default_start`（2030-01-01）。serde 缺省入口。
pub(super) fn default_civil_start_date() -> CivilDate {
    CivilDate::from_ymd(2030, 1, 1).expect("2030-01-01 is a statically valid civil date")
}

impl CivilClock {
    /// 以政策 v1 构建；开局日期必须落在运行区间 2000-01-01..2099-12-31。
    /// 休市开局保持真实起点（phase = `ClosedDay`），不挪到开市日。
    pub fn new(start_date: CivilDate, exchange: CalendarExchange) -> Result<Self, CivilClockError> {
        let calendar = TradingCalendar::default_v1()?;
        calendar.validate_runtime_start(start_date)?;
        Ok(Self {
            calendar,
            exchange,
            start_date,
            current_date: start_date,
            settled_through: None,
            next_due_seq: 1,
            pending: Vec::new(),
            disclosure_observers: Vec::new(),
        })
    }

    /// 从存档重建（内部自洽全量校验；政策按 default_v1 重建，任务 27 冻结）。
    pub fn from_parts(
        start_date: CivilDate,
        save: &CivilClockSave,
        exchange: CalendarExchange,
    ) -> Result<Self, CivilClockError> {
        let calendar = TradingCalendar::default_v1()?;
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
        let mut pending = save.pending_due.clone();
        for due in &pending {
            if due.id.value() >= save.next_due_seq {
                return Err(inconsistent(format!(
                    "due {due:?} id is not below next_due_seq {}",
                    save.next_due_seq
                )));
            }
            if due.due_date < save.current_date {
                return Err(inconsistent(format!(
                    "due {due:?} precedes current {} and can never fire exactly once",
                    save.current_date
                )));
            }
        }
        pending.sort_by_key(|due| (due.due_date, due.id));
        Ok(Self {
            calendar,
            exchange,
            start_date,
            current_date: save.current_date,
            settled_through: save.settled_through,
            next_due_seq: save.next_due_seq,
            pending,
            disclosure_observers: Vec::new(),
        })
    }

    pub fn start_date(&self) -> CivilDate {
        self.start_date
    }

    pub fn current_date(&self) -> CivilDate {
        self.current_date
    }

    pub fn settled_through(&self) -> Option<CivilDate> {
        self.settled_through
    }

    pub fn pending_due(&self) -> &[DueBusiness] {
        &self.pending
    }

    /// 当前稳定阶段（交易日盘中 / 休市日）。
    pub fn phase(&self) -> CivilPhase {
        let status = self
            .calendar
            .day_status(self.exchange, self.current_date)
            .expect("clock dates stay within calendar applicability by construction");
        match status {
            DayStatus::Trading => CivilPhase::IntradayTrading,
            DayStatus::Closed(_) => CivilPhase::ClosedDay,
        }
    }

    /// 注册到期业务；due 不得早于当前自然日，且必须落在日历适用区间内。
    /// 同类业务可在同一日期注册多条（业务侧去重由任务 14 的来源 id 承担）。
    pub fn register_due(
        &mut self,
        due_date: CivilDate,
        kind: DueKind,
    ) -> Result<DueBusiness, CivilClockError> {
        if due_date < self.current_date {
            return Err(CivilClockError::DueRegistrationInPast {
                kind,
                due_date,
                current: self.current_date,
            });
        }
        self.calendar.day_status(self.exchange, due_date)?;
        let due = DueBusiness {
            id: DueBusinessId(self.next_due_seq),
            due_date,
            kind,
        };
        self.next_due_seq += 1;
        self.pending.push(due.clone());
        self.pending.sort_by_key(|item| (item.due_date, item.id));
        Ok(due)
    }

    /// 安装 18:00 披露观察者（hook；任务 15 前生产路径为空）。
    pub fn add_disclosure_observer(&mut self, observer: DisclosureObserver) {
        self.disclosure_observers.push(observer);
    }

    /// 截至 current（含）应已完成的市场会话数（会话同步守卫用）。
    /// 休市日不计会话——周末不欠会话也不多发会话。
    pub fn completed_trading_sessions_expected(&self) -> Result<u32, CivilClockError> {
        let mut count = 0u32;
        let mut cursor = self.start_date;
        while cursor <= self.current_date {
            if self.calendar.is_trading_day(self.exchange, cursor)? {
                count += 1;
            }
            cursor = cursor.next()?;
        }
        Ok(count)
    }

    /// 自然日日结（K1）：**先全量验证，后原子应用**。
    ///
    /// 校验顺序：目标日期与时钟关系（重复/回拨/跳日）→ 次日不越运行上界 →
    /// 无遗留过去 due。应用段：派发当日到期业务（恰好一次，按注册先后）→
    /// 18:00 披露观察者 → 记录日结、前进次日。任何 `Err` 返回时状态原样。
    pub fn end_day(&mut self, date: CivilDate) -> Result<CivilDayEndReport, CivilClockError> {
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
            let unprocessed: Vec<DueBusiness> = self
                .pending
                .iter()
                .filter(|due| due.due_date < date)
                .cloned()
                .collect();
            return Err(CivilClockError::SkippedCivilDays {
                from: self.current_date,
                to: date,
                unprocessed,
            });
        }
        let next = self.current_date.next()?;
        let ceiling = self.calendar.policy().runtime_max_end();
        if next > ceiling {
            return Err(CivilClockError::BeyondRuntimeCeiling { next, ceiling });
        }
        if self
            .pending
            .iter()
            .any(|due| due.due_date < self.current_date)
        {
            let stale: Vec<DueBusiness> = self
                .pending
                .iter()
                .filter(|due| due.due_date < self.current_date)
                .cloned()
                .collect();
            return Err(CivilClockError::SaveInconsistent {
                detail: format!(
                    "{} due item(s) precede the current civil date {stale:?}",
                    stale.len()
                ),
            });
        }
        let settled_date = self.current_date;
        let disclosure_instant =
            CivilInstant::from_hms(settled_date, 18, 0, 0).expect("18:00:00 is a valid instant");
        let mut dispatched_due: Vec<DueBusiness> = self
            .pending
            .extract_if(.., |due| due.due_date == settled_date)
            .collect();
        dispatched_due.sort_by_key(|due| due.id);
        for observer in &self.disclosure_observers {
            observer(disclosure_instant);
        }
        let next_status = self.calendar.day_status(self.exchange, next)?;
        self.settled_through = Some(settled_date);
        self.current_date = next;
        Ok(CivilDayEndReport {
            settled_date,
            dispatched_due,
            disclosure_instant,
            next_date: next,
            next_status,
        })
    }

    /// 导出可存档状态（观察者不入档：进程内 hook，恢复后由宿主重装）。
    pub fn save(&self) -> CivilClockSave {
        CivilClockSave {
            current_date: self.current_date,
            settled_through: self.settled_through,
            next_due_seq: self.next_due_seq,
            pending_due: self.pending.clone(),
        }
    }
}
