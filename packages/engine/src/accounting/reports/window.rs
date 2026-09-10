//! 报表窗口底座（K3，任务 13）：把「分录 + 重述映射」折算成各报表所需的
//! 窗口化余额/运动读投影。**报表是分录的纯函数**——本结构不持有任何独立
//! 状态，同输入 ⇒ 同输出（重建性质）。
//!
//! 两套口径（更正语义，见 closing.rs）：
//! - **有效期间口径**（effective period = 重述映射的目标期间，缺省 = 实际
//!   过账期间）：资产负债表/利润表/权益表/附注的窗口。调整分录按其目标
//!   历史期间进入重述版本。
//! - **实际期间口径**：现金流量表（现金属于实际收付期间，重述绝不双计
//!   现金）。间接法以显式「重述现金调整」行配平（`restated_cash_correction`）。
//!
//! 合并窗口构造见 [`super::consolidated_window`]（Σ成员按代码 + 工作底稿
//! 抵销归属申报当期——登记于 issues 的游戏简化）。

use std::collections::BTreeMap;

use crate::accounting::amount::AccountingAmount;
use crate::accounting::error::AccountingError;
use crate::accounting::journal::{BusinessEventId, CashFlowClass, JournalEntry, PostingSide};
use crate::accounting::ledger::{AccountDef, LedgerAccountId};
use crate::accounting::period::AccountingPeriod;
use crate::accounting::Books;

use super::ReportError;

/// 窗口化读投影（报表生成唯一数据源；crate 内可见）。
pub(crate) struct StatementWindows {
    pub defs: BTreeMap<LedgerAccountId, AccountDef>,
    /// 有效期间口径：期末净借方余额。
    pub closing: BTreeMap<LedgerAccountId, AccountingAmount>,
    /// 报告窗口净借方运动（kind 决定窗口）。
    pub movement: BTreeMap<LedgerAccountId, AccountingAmount>,
    /// 当季运动（季度首月..=期末月）。
    pub quarter: BTreeMap<LedgerAccountId, AccountingAmount>,
    /// 年初至今运动。
    pub ytd: BTreeMap<LedgerAccountId, AccountingAmount>,
    /// 上年同期窗口运动（无历史流量 ⇒ None）。
    pub prior_year: Option<BTreeMap<LedgerAccountId, AccountingAmount>>,
    /// 上年年末净借方余额（无历史 ⇒ None）。
    pub prior_year_end: Option<BTreeMap<LedgerAccountId, AccountingAmount>>,
    /// 实际期间口径：窗口现金流量分类净额。
    pub cash: CashWindowTotals,
    /// 实际期间口径期末现金（现金流量表勾稽基准）。
    pub cash_closing_actual: AccountingAmount,
    /// 重述现金调整（间接法配平行；无重述 ⇒ 0）。
    pub restated_cash_correction: AccountingAmount,
    /// 合并专属事实（单体 ⇒ None）。
    pub consolidation: Option<super::consolidated_window::ConsolidationFacts>,
}

/// 窗口现金流量分类净额（实际期间口径）。
pub(crate) struct CashWindowTotals {
    pub operating: AccountingAmount,
    pub investing: AccountingAmount,
    pub financing: AccountingAmount,
}

/// 报告窗口形状（首期, 末期）。
pub(crate) type Window = (AccountingPeriod, AccountingPeriod);

/// 单体窗口：遍历日记账一次，逐行折算各桶。
pub(crate) fn standalone(
    books: &Books,
    window: Window,
    prior_window: Window,
    adjustments: &BTreeMap<BusinessEventId, AccountingPeriod>,
) -> Result<StatementWindows, ReportError> {
    let defs: BTreeMap<LedgerAccountId, AccountDef> = books
        .ledger()
        .chart()
        .iter()
        .map(|(id, def)| (id.clone(), def.clone()))
        .collect();
    let mut acc = Accumulator::new(window, prior_window)?;
    for entry in books.journal().entries() {
        let mapped = adjustments.contains_key(&entry.source);
        let effective = if mapped {
            adjustments[&entry.source]
        } else {
            entry.period()
        };
        acc.add_entry(entry, effective, mapped, &defs)
            .map_err(|e| ReportError::Accounting(Box::new(e)))?;
    }
    Ok(acc.finish(defs, None))
}

/// 季度首月（1/4/7/10）。
pub(crate) fn quarter_first(p: AccountingPeriod) -> AccountingPeriod {
    let first = p.month() - ((p.month() - 1) % 3);
    AccountingPeriod::from_ymd(p.year(), first).expect("quarter start is always valid")
}

/// 累计净利（窗口口径）：收入贷余 − 费用借余（BS 留存/CF 间接法/权益表共用）。
pub(crate) fn net_income_of(
    map: &BTreeMap<LedgerAccountId, AccountingAmount>,
    defs: &BTreeMap<LedgerAccountId, AccountDef>,
) -> Result<AccountingAmount, ReportError> {
    let mut total = AccountingAmount::ZERO;
    for (code, def) in defs {
        let Some(value) = map.get(code) else { continue };
        if matches!(
            def.element,
            crate::accounting::ledger::AccountElement::Revenue
                | crate::accounting::ledger::AccountElement::Expense
        ) {
            total = total.sub(*value)?;
        }
    }
    Ok(total)
}

/// 逐桶累加一行。
fn bump(
    map: &mut BTreeMap<LedgerAccountId, AccountingAmount>,
    account: &LedgerAccountId,
    delta: AccountingAmount,
) -> Result<(), AccountingError> {
    let slot = map.entry(account.clone()).or_default();
    *slot = slot.add(delta)?;
    Ok(())
}

/// 逐桶累加器（单体/合并共用）。
pub(crate) struct Accumulator {
    window: Window,
    prior_window: Window,
    quarter_start: AccountingPeriod,
    ytd_start: AccountingPeriod,
    prior_dec: AccountingPeriod,
    closing: BTreeMap<LedgerAccountId, AccountingAmount>,
    movement: BTreeMap<LedgerAccountId, AccountingAmount>,
    quarter: BTreeMap<LedgerAccountId, AccountingAmount>,
    ytd: BTreeMap<LedgerAccountId, AccountingAmount>,
    prior_year: BTreeMap<LedgerAccountId, AccountingAmount>,
    prior_year_end: BTreeMap<LedgerAccountId, AccountingAmount>,
    cash: CashWindowTotals,
    cash_closing_actual: AccountingAmount,
    restated_cash_correction: AccountingAmount,
}

impl Accumulator {
    pub(crate) fn new(window: Window, prior_window: Window) -> Result<Self, ReportError> {
        let last = window.1;
        let ytd_start = AccountingPeriod::from_ymd(last.year(), 1)
            .map_err(|e| ReportError::Accounting(Box::new(e)))?;
        let prior_dec = AccountingPeriod::from_ymd(last.year() - 1, 12).map_err(|_| {
            ReportError::InvalidReportKind {
                period: last,
                reason: "prior year precedes the supported civil window",
            }
        })?;
        Ok(Self {
            window,
            prior_window,
            quarter_start: quarter_first(last),
            ytd_start,
            prior_dec,
            closing: BTreeMap::new(),
            movement: BTreeMap::new(),
            quarter: BTreeMap::new(),
            ytd: BTreeMap::new(),
            prior_year: BTreeMap::new(),
            prior_year_end: BTreeMap::new(),
            cash: CashWindowTotals {
                operating: AccountingAmount::ZERO,
                investing: AccountingAmount::ZERO,
                financing: AccountingAmount::ZERO,
            },
            cash_closing_actual: AccountingAmount::ZERO,
            restated_cash_correction: AccountingAmount::ZERO,
        })
    }

    pub(crate) fn prior_dec_bound(&self) -> AccountingPeriod {
        self.prior_dec
    }

    /// 计入一笔分录（`mapped` = 来源属重述映射 ⇒ 有效期间 = 目标历史期间）。
    pub(crate) fn add_entry(
        &mut self,
        entry: &JournalEntry,
        effective: AccountingPeriod,
        mapped: bool,
        defs: &BTreeMap<LedgerAccountId, AccountDef>,
    ) -> Result<(), AccountingError> {
        let (first, last) = self.window;
        let in_win = first <= effective && effective <= last;
        let mut net_cash = AccountingAmount::ZERO;
        for line in &entry.lines {
            let delta = match line.side {
                PostingSide::Debit => line.amount,
                PostingSide::Credit => line.amount.neg()?,
            };
            if effective <= last {
                bump(&mut self.closing, &line.account, delta)?;
            }
            if in_win {
                bump(&mut self.movement, &line.account, delta)?;
            }
            // 当季/年初至今有各自的窗口（季度首月/年初 ..= 末期），独立于
            // 报告窗口（月报的累计栏横跨全年）。
            if self.quarter_start <= effective && effective <= last {
                bump(&mut self.quarter, &line.account, delta)?;
            }
            if self.ytd_start <= effective && effective <= last {
                bump(&mut self.ytd, &line.account, delta)?;
            }
            if self.prior_window.0 <= effective && effective <= self.prior_window.1 {
                bump(&mut self.prior_year, &line.account, delta)?;
            }
            if effective <= self.prior_dec {
                bump(&mut self.prior_year_end, &line.account, delta)?;
            }
            let is_cash = defs.get(&line.account).is_some_and(|def| def.is_cash);
            if is_cash {
                net_cash = net_cash.add(delta)?;
            }
        }
        let actual = entry.period();
        if entry.cash_flow != CashFlowClass::NonCash && first <= actual && actual <= last {
            match entry.cash_flow {
                CashFlowClass::Operating => {
                    self.cash.operating = self.cash.operating.add(net_cash)?
                }
                CashFlowClass::Investing => {
                    self.cash.investing = self.cash.investing.add(net_cash)?
                }
                CashFlowClass::Financing => {
                    self.cash.financing = self.cash.financing.add(net_cash)?
                }
                CashFlowClass::NonCash => {}
            }
        }
        if entry.cash_flow != CashFlowClass::NonCash && actual <= last {
            self.cash_closing_actual = self.cash_closing_actual.add(net_cash)?;
        }
        // 重述现金调整：映射进窗口的调整分录，其现金运动属实际期间（不双计）。
        if mapped && in_win && entry.cash_flow != CashFlowClass::NonCash {
            self.restated_cash_correction = self.restated_cash_correction.sub(net_cash)?;
        }
        Ok(())
    }

    /// 工作底稿行折入有效期间各桶（closing/movement/quarter/ytd；无现金）。
    pub(crate) fn bucket_all(
        &mut self,
        account: &LedgerAccountId,
        delta: AccountingAmount,
    ) -> Result<(), AccountingError> {
        bump(&mut self.closing, account, delta)?;
        bump(&mut self.movement, account, delta)?;
        bump(&mut self.quarter, account, delta)?;
        bump(&mut self.ytd, account, delta)?;
        Ok(())
    }

    /// 上年年末窗口是否有任何历史（决定比较项可用性）。
    pub(crate) fn has_prior_history(&self) -> bool {
        !self.prior_year_end.is_empty()
    }

    pub(crate) fn finish(
        self,
        defs: BTreeMap<LedgerAccountId, AccountDef>,
        consolidation: Option<super::consolidated_window::ConsolidationFacts>,
    ) -> StatementWindows {
        StatementWindows {
            defs,
            closing: self.closing,
            movement: self.movement,
            quarter: self.quarter,
            ytd: self.ytd,
            prior_year: if self.prior_year.is_empty() {
                None
            } else {
                Some(self.prior_year)
            },
            prior_year_end: if self.prior_year_end.is_empty() {
                None
            } else {
                Some(self.prior_year_end)
            },
            cash: self.cash,
            cash_closing_actual: self.cash_closing_actual,
            restated_cash_correction: self.restated_cash_correction,
            consolidation,
        }
    }
}
