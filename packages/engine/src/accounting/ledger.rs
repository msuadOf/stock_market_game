//! 总账（K2）：T 型科目余额 + 按科目/期间的增量索引 + 现金流（期间×类别）索引。
//!
//! Ledger 是**派生投影**：只由 Journal 的已提交分录增量构建，不进入存档
//! （恢复时由 `Books` 重放重建）——来源事实与派生 report 边界不可混用。
//! 所有合计使用 checked `AccountingAmount` 运算；查询是纯读投影。

use std::collections::BTreeMap;

use crate::accounting::amount::AccountingAmount;
use crate::accounting::error::AccountingError;
use crate::accounting::journal::{CashFlowClass, JournalEntry, JournalLine, PostingSide};
use crate::accounting::period::AccountingPeriod;

mod chart;

pub use chart::{AccountChart, AccountDef, AccountElement, LedgerAccountId};

/// 单科目 T 型合计（借方总额 / 贷方总额；净额 = 借 − 贷）。
#[derive(Clone, Copy, Eq, PartialEq, Debug, Default)]
pub struct AccountBalance {
    debit_total: AccountingAmount,
    credit_total: AccountingAmount,
}

impl AccountBalance {
    pub fn debit_total(&self) -> AccountingAmount {
        self.debit_total
    }

    pub fn credit_total(&self) -> AccountingAmount {
        self.credit_total
    }

    /// 净借方余额（借 − 贷；贷方余额为负）。
    pub fn net_debit(&self) -> Result<AccountingAmount, AccountingError> {
        self.debit_total.sub(self.credit_total)
    }
}

/// 试算平衡摘要（全部科目借/贷总额；必须相等——入账时已逐笔保证）。
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub struct TrialBalanceSummary {
    pub total_debits: AccountingAmount,
    pub total_credits: AccountingAmount,
}

/// 总账：科目表 + 增量索引。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Ledger {
    chart: AccountChart,
    /// 科目 → 全期间 T 型合计。
    account_totals: BTreeMap<LedgerAccountId, AccountBalance>,
    /// （科目, 期间）→ T 型合计（增量维护，不重放历史）。
    period_balances: BTreeMap<(LedgerAccountId, AccountingPeriod), AccountBalance>,
    /// （期间, 现金流类别）→ 现金净变动（NonCash 分录经验证不含现金行，
    /// 故该键不会出现——非现金标识与现金行互斥）。
    cash_flows: BTreeMap<(AccountingPeriod, CashFlowClass), AccountingAmount>,
}

impl Ledger {
    pub fn new(chart: AccountChart) -> Self {
        Self {
            chart,
            account_totals: BTreeMap::new(),
            period_balances: BTreeMap::new(),
            cash_flows: BTreeMap::new(),
        }
    }

    pub fn chart(&self) -> &AccountChart {
        &self.chart
    }

    /// 增量计入一笔**已通过全部验证**的分录（仅在 Books 验证路径与恢复重放中
    /// 调用；未知科目/非现金触碰现金在这里再次类型化拒绝，错误不静默）。
    pub(crate) fn apply_entry(&mut self, entry: &JournalEntry) -> Result<(), AccountingError> {
        let period = entry.period();
        for line in &entry.lines {
            let def =
                self.chart
                    .get(&line.account)
                    .ok_or_else(|| AccountingError::UnknownAccount {
                        event: entry.source,
                        account: line.account.clone(),
                    })?;
            let totals = self.account_totals.entry(line.account.clone()).or_default();
            apply_side(totals, line.side, line.amount)?;
            let period_key = (line.account.clone(), period);
            let period_totals = self.period_balances.entry(period_key).or_default();
            apply_side(period_totals, line.side, line.amount)?;
            if def.is_cash {
                self.apply_cash_flow(entry, line, period)?;
            }
        }
        Ok(())
    }

    fn apply_cash_flow(
        &mut self,
        entry: &JournalEntry,
        line: &JournalLine,
        period: AccountingPeriod,
    ) -> Result<(), AccountingError> {
        if entry.cash_flow == CashFlowClass::NonCash {
            return Err(AccountingError::NonCashTouchesCash {
                event: entry.source,
                account: line.account.clone(),
            });
        }
        let key = (period, entry.cash_flow);
        let current = self
            .cash_flows
            .get(&key)
            .copied()
            .unwrap_or(AccountingAmount::ZERO);
        let updated = match line.side {
            PostingSide::Debit => current.add(line.amount)?,
            PostingSide::Credit => current.sub(line.amount)?,
        };
        self.cash_flows.insert(key, updated);
        Ok(())
    }

    /// 科目全期间 T 型合计（未入账科目为零合计）。
    pub fn account_balance(&self, id: &LedgerAccountId) -> AccountBalance {
        self.account_totals.get(id).copied().unwrap_or_default()
    }

    /// 科目某期间 T 型合计。
    pub fn account_period_balance(
        &self,
        id: &LedgerAccountId,
        period: AccountingPeriod,
    ) -> AccountBalance {
        self.period_balances
            .get(&(id.clone(), period))
            .copied()
            .unwrap_or_default()
    }

    /// 科目净借方余额（借 − 贷；未入账科目为 0，查询是读投影）。
    pub fn account_net_debit(
        &self,
        id: &LedgerAccountId,
    ) -> Result<AccountingAmount, AccountingError> {
        self.account_balance(id).net_debit()
    }

    /// 现金类科目合计（负现金由过账守卫拒绝，此处恒 ≥ 0）。
    pub fn cash_total(&self) -> Result<AccountingAmount, AccountingError> {
        self.element_net_debit_sum(|def| def.is_cash)
    }

    /// 负债合计（贷方正常余额合计）。
    pub fn liabilities_total(&self) -> Result<AccountingAmount, AccountingError> {
        self.element_net_debit_sum(|def| def.element == AccountElement::Liability)?
            .neg()
    }

    /// 净利 = 收入（贷余）− 费用（借余）；结账前的滚动口径（结账在任务 13）。
    pub fn net_income(&self) -> Result<AccountingAmount, AccountingError> {
        let revenues = self
            .element_net_debit_sum(|def| def.element == AccountElement::Revenue)?
            .neg()?;
        let expenses = self.element_net_debit_sum(|def| def.element == AccountElement::Expense)?;
        revenues.sub(expenses)
    }

    /// 权益滚动 = 权益科目（贷余）+ 净利。合法状态可为负（亏损主体），
    /// 不 clamp、不报错。
    pub fn equity_rolling(&self) -> Result<AccountingAmount, AccountingError> {
        let equity_accounts = self
            .element_net_debit_sum(|def| def.element == AccountElement::Equity)?
            .neg()?;
        equity_accounts.add(self.net_income()?)
    }

    /// 指定（要素正常口径）科目的净借方合计：备抵科目以带符号净额自然冲减。
    fn element_net_debit_sum(
        &self,
        predicate: impl Fn(&AccountDef) -> bool,
    ) -> Result<AccountingAmount, AccountingError> {
        let mut total = AccountingAmount::ZERO;
        for (id, def) in self.chart.iter() {
            if predicate(def) {
                total = total.add(self.account_balance(id).net_debit()?)?;
            }
        }
        Ok(total)
    }

    /// 某期间某现金流类别的现金净变动（无流量为 0）。
    pub fn cash_flow_for_period(
        &self,
        period: AccountingPeriod,
        class: CashFlowClass,
    ) -> AccountingAmount {
        self.cash_flows
            .get(&(period, class))
            .copied()
            .unwrap_or(AccountingAmount::ZERO)
    }

    /// 全期间某现金流类别合计。
    pub fn cash_flow_total(
        &self,
        class: CashFlowClass,
    ) -> Result<AccountingAmount, AccountingError> {
        let mut total = AccountingAmount::ZERO;
        for ((_, key_class), amount) in &self.cash_flows {
            if *key_class == class {
                total = total.add(*amount)?;
            }
        }
        Ok(total)
    }

    /// 试算平衡摘要（全部科目借/贷总额）。
    pub fn trial_balance(&self) -> Result<TrialBalanceSummary, AccountingError> {
        let mut total_debits = AccountingAmount::ZERO;
        let mut total_credits = AccountingAmount::ZERO;
        for balance in self.account_totals.values() {
            total_debits = total_debits.add(balance.debit_total())?;
            total_credits = total_credits.add(balance.credit_total())?;
        }
        Ok(TrialBalanceSummary {
            total_debits,
            total_credits,
        })
    }
}

/// 按方向累加一行到 T 型合计。
fn apply_side(
    balance: &mut AccountBalance,
    side: PostingSide,
    amount: AccountingAmount,
) -> Result<(), AccountingError> {
    match side {
        PostingSide::Debit => balance.debit_total = balance.debit_total.add(amount)?,
        PostingSide::Credit => balance.credit_total = balance.credit_total.add(amount)?,
    }
    Ok(())
}
