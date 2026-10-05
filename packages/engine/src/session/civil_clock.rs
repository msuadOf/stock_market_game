//! 自然日经营时钟：与市场时钟共同构成权威双时钟。
//!
//! 职责边界：本模块只负责**自然日**语义，不触碰 tick/市场分钟/撮合——
//! - 交易日：[`super::GameSession::step`] 照旧推进交易时间；收盘后由
//!   [`super::GameSession::end_civil_day`] 依次执行当日经营终局窗口、
//!   18:00 披露阶段（hook），然后才前进到次日自然日。
//! - 休市日：无 tick、无成交、无注意力 RNG 消费；当日到期业务恰好一次
//!   派发，再前进。休市起点保持真实起点，不静默挪到开市日。
//!
//! 到期队列按日期恰好一次派发，金额与入账由 CompanyOperations 负责。
//! 派发事实通过 [`CivilDayEndReport`] 返回；DisclosureDispatch 安装无状态的
//! 18:00 相位观察者，GameSession 在同一相位执行有状态披露派发。
//! 所有日结失败先验证并原子拒绝；任何 `Err` 返回时时钟与会话状态保持原样。

use super::{Event, StockExchange};
use crate::calendar::{
    CalendarError, CalendarExchange, CivilDate, CivilDateError, CivilInstant, DayStatus,
    TradingCalendar,
};
use thiserror::Error;

/// 到期业务种类；时钟负责调度，CompanyOperations 负责金额与入账。
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
    /// 按自然日计提的合同利息（ACT/365F，显式游戏假设）。
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

/// 18:00 披露阶段观察者，由 DisclosureDispatch 安装生产相位 hook。
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
#[derive(Clone, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
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
    /// 日结成功后由会话追加的共享公开事件，顺序属于全局 session seq。
    pub events: Vec<Event>,
}

/// 存档中的自然日时钟状态；恢复时按存档携带的
/// [`CalendarPolicySpec`] 重建日历（`from_parts` 重算 digest 并全量校验），
/// 绝不被当前进程的默认政策表覆盖。观察者是进程内 hook，不入档。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct CivilClockSave {
    pub current_date: CivilDate,
    /// 最后一个已完成日结的自然日；`None` = 尚未日结（current == start）。
    pub settled_through: Option<CivilDate>,
    pub next_due_seq: u32,
    /// 待派发到期业务，按 (due_date, id) 升序。
    pub pending_due: Vec<DueBusiness>,
    /// 本局使用的完整日历政策（算法版本/覆盖/回退/事实表）；恢复时经
    /// digest 与形状校验重建——同档永远按同一政策推进。
    #[ts(skip)]
    pub policy: crate::calendar::CalendarPolicySpec,
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
    #[error("civil clock due id sequence exhausted")]
    DueSequenceExhausted,
}

/// 自然日经营时钟：与 tick/市场分钟严格分离的权威自然日推进（冻结日历与双时钟）。
///
/// 持有随存档冻结的会话级交易日历政策与到期业务队列。
/// 不持有任何市场状态——休市推进因此天然不产生 tick/成交/RNG 消费。
#[derive(Clone, Debug)]
pub struct CivilClock {
    calendar: TradingCalendar,
    exchanges: Vec<CalendarExchange>,
    stock_exchanges: std::sync::Arc<std::collections::BTreeMap<crate::StockCode, CalendarExchange>>,
    start_date: CivilDate,
    current_date: CivilDate,
    settled_through: Option<CivilDate>,
    next_due_seq: u32,
    pending: Vec<DueBusiness>,
    disclosure_observers: Vec<DisclosureObserver>,
}

/// 将证券所属交易所映射为交易日历的交易所身份。
pub(super) fn session_calendar_exchange(first: StockExchange) -> CalendarExchange {
    match first {
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

    pub fn new_for_exchanges(
        start_date: CivilDate,
        exchanges: impl IntoIterator<Item = CalendarExchange>,
    ) -> Result<Self, CivilClockError> {
        let calendar = TradingCalendar::current_default_calendar()?;
        calendar.validate_runtime_start(start_date)?;
        let exchanges = Self::canonical_exchanges(exchanges)?;
        Ok(Self {
            calendar,
            exchanges,
            stock_exchanges: std::sync::Arc::new(std::collections::BTreeMap::new()),
            start_date,
            current_date: start_date,
            settled_through: None,
            next_due_seq: 1,
            pending: Vec::new(),
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

    pub fn from_parts_for_exchanges(
        start_date: CivilDate,
        save: &CivilClockSave,
        exchanges: impl IntoIterator<Item = CalendarExchange>,
    ) -> Result<Self, CivilClockError> {
        let exchanges = Self::canonical_exchanges(exchanges)?;
        let calendar = TradingCalendar::from_policy(crate::calendar::CalendarPolicy::from_parts(
            save.policy.clone(),
        )?)?;
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
        let mut ids = std::collections::BTreeSet::new();
        for due in &pending {
            if !ids.insert(due.id) {
                return Err(inconsistent(format!("duplicate due id {}", due.id.value())));
            }
            calendar.day_status(exchanges[0], due.due_date)?;
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
            exchanges,
            stock_exchanges: std::sync::Arc::new(std::collections::BTreeMap::new()),
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

    fn canonical_exchanges(
        exchanges: impl IntoIterator<Item = CalendarExchange>,
    ) -> Result<Vec<CalendarExchange>, CivilClockError> {
        let exchanges = exchanges.into_iter().collect::<std::collections::BTreeSet<_>>();
        if exchanges.is_empty() {
            return Err(CivilClockError::SaveInconsistent {
                detail: "共享自然日时钟必须至少包含一个交易所".into(),
            });
        }
        Ok(exchanges.into_iter().collect())
    }

    pub fn exchange_day_status(
        &self,
        exchange: CalendarExchange,
        date: CivilDate,
    ) -> Result<DayStatus, CalendarError> {
        self.calendar.day_status(exchange, date)
    }

    fn index_stock_exchanges(
        stocks: impl IntoIterator<Item = (crate::StockCode, CalendarExchange)>,
    ) -> Result<std::collections::BTreeMap<crate::StockCode, CalendarExchange>, CivilClockError> {
        let mut indexed = std::collections::BTreeMap::new();
        for (code, exchange) in stocks {
            if indexed.insert(code.clone(), exchange).is_some() {
                return Err(CivilClockError::SaveInconsistent { detail: format!("证券日历重复代码 {}", code.0) });
            }
        }
        Ok(indexed)
    }

    pub(crate) fn new_for_stocks(
        start_date: CivilDate,
        stocks: impl IntoIterator<Item = (crate::StockCode, CalendarExchange)>,
    ) -> Result<Self, CivilClockError> {
        let indexed = Self::index_stock_exchanges(stocks)?;
        let mut clock = Self::new_for_exchanges(start_date, indexed.values().copied())?;
        clock.stock_exchanges = std::sync::Arc::new(indexed);
        Ok(clock)
    }

    pub(crate) fn from_parts_for_stocks(
        start_date: CivilDate,
        save: &CivilClockSave,
        stocks: impl IntoIterator<Item = (crate::StockCode, CalendarExchange)>,
    ) -> Result<Self, CivilClockError> {
        let indexed = Self::index_stock_exchanges(stocks)?;
        let mut clock = Self::from_parts_for_exchanges(start_date, save, indexed.values().copied())?;
        clock.stock_exchanges = std::sync::Arc::new(indexed);
        Ok(clock)
    }

    pub(crate) fn stock_exchange(&self, code: &crate::StockCode) -> Option<CalendarExchange> {
        self.stock_exchanges.get(code).copied()
    }

    pub(crate) fn exchanges(&self) -> &[CalendarExchange] {
        &self.exchanges
    }

    pub(crate) fn calendar(&self) -> &TradingCalendar {
        &self.calendar
    }

    fn shared_day_status(&self, date: CivilDate) -> Result<DayStatus, CalendarError> {
        let mut closed = None;
        for exchange in &self.exchanges {
            match self.calendar.day_status(*exchange, date)? {
                DayStatus::Trading => return Ok(DayStatus::Trading),
                status @ DayStatus::Closed(_) => {
                    if closed.is_none() { closed = Some(status); }
                }
            }
        }
        Ok(closed.expect("自然日时钟的交易所集合已验证非空"))
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

    /// 注册到期业务；due 不得早于当前自然日，且必须落在日历适用区间内。
    /// 同类业务可在同一日期注册多条；CompanyOperations 使用来源 id 去重。
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
        self.calendar.day_status(self.exchanges[0], due_date)?;
        let due = DueBusiness {
            id: DueBusinessId(self.next_due_seq),
            due_date,
            kind,
        };
        let next_due_seq = self
            .next_due_seq
            .checked_add(1)
            .ok_or(CivilClockError::DueSequenceExhausted)?;
        self.pending.push(due.clone());
        self.pending.sort_by_key(|item| (item.due_date, item.id));
        self.next_due_seq = next_due_seq;
        Ok(due)
    }

    /// 添加 18:00 披露观察者；生产 hook 由 DisclosureDispatch 安装。
    pub fn add_disclosure_observer(&mut self, observer: DisclosureObserver) {
        self.disclosure_observers.push(observer);
    }

    pub(super) fn disclosure_observers(&self) -> &[DisclosureObserver] {
        &self.disclosure_observers
    }

    /// 截至 current（含）应已完成的市场会话数（会话同步守卫用）。
    /// 休市日不计会话——周末不欠会话也不多发会话。
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
    /// 校验顺序：目标日期与时钟关系（重复/回拨/跳日）→ 次日不越运行上界 →
    /// 无遗留过去 due。应用段：派发当日到期业务（恰好一次，按注册先后）→
    /// 记录日结、前进次日。18:00 观察者由 `GameSession` 在完整日结成功后调用。
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
        let next_status = self.shared_day_status(next)?;
        self.settled_through = Some(settled_date);
        self.current_date = next;
        Ok(CivilDayEndReport {
            settled_date,
            dispatched_due,
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
            next_due_seq: self.next_due_seq,
            pending_due: self.pending.clone(),
            policy: self.calendar.policy().spec(),
        }
    }
}
