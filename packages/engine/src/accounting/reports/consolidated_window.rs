//! 合并窗口构造：Σ成员按代码加总 → 固定集团合并 → 工作底稿
//! 抵销折入 + 少数股东拆分事实。
//!
//! 语义决定（登记于 issues 的游戏简化）：
//! - 工作底稿抵销的**流量效应归属合并申报当期**（合并申报不携带期间
//!   属性；重述/跨期不追溯）。
//! - 合并资产负债表权益列：实收资本 = **根成员** 4001；归母留存 = 归母权益
//!   − 根成员实收资本（子公司权益的母公司份额并入留存——固定控制、无并购
//!   计量模型）；少数股东权益由固定集团合并拆分。非根成员权益科目在附注的
//!   合并拆分披露中单独列示。
//! - 上年年末比较项的合并拆分按「成员上年权益 × 少数基点」推导。

use std::collections::{BTreeMap, BTreeSet};

use crate::accounting::amount::AccountingAmount;
use crate::accounting::consolidation::{
    consolidate, ConsolidationOutput, ConsolidationRequest, GroupMember, MemberId,
    MinorityInterest, ScopeId,
};
use crate::accounting::error::AccountingError;
use crate::accounting::journal::{JournalEntry, PostingSide};
use crate::accounting::ledger::{AccountDef, AccountElement, LedgerAccountId};
use crate::accounting::period::AccountingPeriod;
use crate::accounting::Books;

use super::window::{Accumulator, StatementWindows, Window};
use super::ReportError;

/// 上年年末合并拆分。
pub(crate) struct PriorSplit {
    pub minority: AccountingAmount,
    pub parent: AccountingAmount,
    /// 根成员权益科目上年年末净贷方（比较项实收资本行）。
    pub root_capital: AccountingAmount,
}

/// 合并报表所需的固定集团合并输出事实。
pub(crate) struct ConsolidationFacts {
    pub root: MemberId,
    pub minority_equity: AccountingAmount,
    pub equity_to_parent: AccountingAmount,
    pub minority_ni: AccountingAmount,
    pub ni_to_parent: AccountingAmount,
    pub consolidated_ni: AccountingAmount,
    pub window_ni_to_parent: AccountingAmount,
    pub window_minority_ni: AccountingAmount,
    /// 非根成员权益科目期末净贷方贡献（附注合并拆分披露）。
    pub non_root_equity: BTreeMap<LedgerAccountId, AccountingAmount>,
    /// 上年年末合并拆分；无历史 ⇒ None。
    pub prior_split: Option<PriorSplit>,
}

/// 合并窗口（纯函数：只读成员账套 + 申报）。
pub(crate) fn consolidated(
    request: ConsolidationRequest<'_>,
    window: Window,
    prior_window: Window,
) -> Result<StatementWindows, ReportError> {
    let mut bounded = Vec::with_capacity(request.members.len());
    for member in &request.members {
        let mut books = Books::new(member.books.ledger().chart().clone());
        let entries = member
            .books
            .journal()
            .entries()
            .filter(|entry| entry.period() <= window.1)
            .cloned()
            .collect::<Vec<_>>();
        if !entries.is_empty() {
            books.post_batch(entries)?;
        }
        bounded.push((member.spec.clone(), books));
    }
    let request = ConsolidationRequest {
        root: request.root,
        members: bounded
            .iter()
            .map(|(spec, books)| GroupMember {
                spec: spec.clone(),
                books,
            })
            .collect(),
        intercompany_balances: request.intercompany_balances,
        intercompany_sales: request.intercompany_sales,
    };
    let root = request.root.clone();
    let members = request.members.clone();
    let output = consolidate(request)?;
    let mut builder = WindowConsolidationBuilder::new(window, prior_window)?;
    builder.scan_members(&root, &members)?;
    builder.apply_consolidation_output(&output)?;
    builder.finish(output)
}

/// 合并窗口构建期间的成员事实；最终金额仍由 Consolidation 与 Accumulator 计算。
struct WindowConsolidationBuilder {
    window: Window,
    acc: Accumulator,
    defs: BTreeMap<LedgerAccountId, AccountDef>,
    sub_ids: BTreeSet<MemberId>,
    member_prior_equity: BTreeMap<MemberId, AccountingAmount>,
    non_root_equity: BTreeMap<LedgerAccountId, AccountingAmount>,
    root_prior_capital: AccountingAmount,
    keys: crate::accounting::consolidation::AccountKeys,
    member_ytd_income: BTreeMap<MemberId, AccountingAmount>,
    member_window_income: BTreeMap<MemberId, AccountingAmount>,
}

impl WindowConsolidationBuilder {
    fn new(window: Window, prior_window: Window) -> Result<Self, ReportError> {
        Ok(Self {
            window,
            acc: Accumulator::new(window, prior_window)?,
            defs: BTreeMap::new(),
            sub_ids: BTreeSet::new(),
            member_prior_equity: BTreeMap::new(),
            non_root_equity: BTreeMap::new(),
            root_prior_capital: AccountingAmount::ZERO,
            keys: BTreeMap::new(),
            member_ytd_income: BTreeMap::new(),
            member_window_income: BTreeMap::new(),
        })
    }

    fn scan_members(
        &mut self,
        root: &MemberId,
        members: &[GroupMember<'_>],
    ) -> Result<(), ReportError> {
        let books = members
            .iter()
            .map(|member| (member.spec.id.clone(), member.books))
            .collect();
        self.keys = crate::accounting::consolidation::account_keys(&books);
        for member in members {
            if member.spec.group_parent.is_some() {
                self.sub_ids.insert(member.spec.id.clone());
            }
            for (id, def) in member.books.ledger().chart().iter() {
                let key = &self.keys[&(member.spec.id.clone(), id.clone())];
                self.defs.entry(key.clone()).or_insert_with(|| def.clone());
            }
        }
        for member in members {
            let is_sub = self.sub_ids.contains(&member.spec.id);
            let is_root = &member.spec.id == root;
            let mut prior_equity = AccountingAmount::ZERO;
            for entry in member.books.journal().entries() {
                let income = entry_income(entry, member.books)?;
                if entry.period().year() == self.window.1.year() {
                    let total = self
                        .member_ytd_income
                        .entry(member.spec.id.clone())
                        .or_default();
                    *total = total.add(income)?;
                }
                if entry.period() >= self.window.0 && entry.period() <= self.window.1 {
                    let total = self
                        .member_window_income
                        .entry(member.spec.id.clone())
                        .or_default();
                    *total = total.add(income)?;
                }
                let mut mapped = entry.clone();
                for line in &mut mapped.lines {
                    line.account =
                        self.keys[&(member.spec.id.clone(), line.account.clone())].clone();
                }
                self.acc
                    .add_entry(&mapped, entry.period(), false, &self.defs)
                    .map_err(|e| ReportError::Accounting(Box::new(e)))?;
                prior_equity = prior_equity
                    .add(equity_rolling_delta(
                        entry,
                        member.books,
                        self.acc.prior_dec_bound(),
                    )?)
                    .map_err(|e| ReportError::Accounting(Box::new(e)))?;
                if is_sub {
                    credit_of_equity(
                        entry,
                        member.books,
                        Some(&mut self.non_root_equity),
                        self.window.1,
                    )?;
                }
                if is_root {
                    self.root_prior_capital = self
                        .root_prior_capital
                        .add(credit_of_equity(
                            entry,
                            member.books,
                            None,
                            self.acc.prior_dec_bound(),
                        )?)
                        .map_err(|e| ReportError::Accounting(Box::new(e)))?;
                }
            }
            self.member_prior_equity
                .insert(member.spec.id.clone(), prior_equity);
        }
        Ok(())
    }

    fn apply_consolidation_output(
        &mut self,
        output: &ConsolidationOutput,
    ) -> Result<(), ReportError> {
        let mut worksheet = output.worksheet.clone();
        for entry in &mut worksheet {
            for line in &mut entry.lines {
                line.account = self.keys[&(line.member.clone(), line.account.clone())].clone();
                if matches!(
                    self.defs[&line.account].element,
                    AccountElement::Revenue | AccountElement::Expense
                ) {
                    let income = match line.side {
                        PostingSide::Debit => line.amount.neg()?,
                        PostingSide::Credit => line.amount,
                    };
                    let ytd = self
                        .member_ytd_income
                        .entry(line.member.clone())
                        .or_default();
                    *ytd = ytd.add(income)?;
                    let window = self
                        .member_window_income
                        .entry(line.member.clone())
                        .or_default();
                    *window = window.add(income)?;
                }
            }
        }
        self.acc
            .add_current_worksheet(&worksheet)
            .map_err(|e| ReportError::Accounting(Box::new(e)))
    }

    fn finish(self, output: ConsolidationOutput) -> Result<StatementWindows, ReportError> {
        let mut ytd_income = AccountingAmount::ZERO;
        let mut window_income = AccountingAmount::ZERO;
        for income in self.member_ytd_income.values() {
            ytd_income = ytd_income.add(*income)?;
        }
        for income in self.member_window_income.values() {
            window_income = window_income.add(*income)?;
        }
        let mut minority_ytd = AccountingAmount::ZERO;
        let mut minority_window = AccountingAmount::ZERO;
        for interest in &output.minority {
            minority_ytd = minority_ytd.add(apply_minority_bp(
                self.member_ytd_income
                    .get(&interest.subsidiary)
                    .copied()
                    .unwrap_or(AccountingAmount::ZERO),
                interest,
            )?)?;
            minority_window = minority_window.add(apply_minority_bp(
                self.member_window_income
                    .get(&interest.subsidiary)
                    .copied()
                    .unwrap_or(AccountingAmount::ZERO),
                interest,
            )?)?;
        }
        let prior_split = prior_year_split(
            &output,
            &self.member_prior_equity,
            self.root_prior_capital,
            self.acc.has_prior_history(),
        )?;
        let ConsolidationOutput {
            scope,
            minority_equity_total,
            equity_to_parent,
            ..
        } = output;
        let facts = ConsolidationFacts {
            root: match &scope {
                ScopeId::Consolidated(root) => root.clone(),
                ScopeId::Standalone(_) => {
                    return Err(ReportError::InternalWindowInconsistent {
                        detail: "consolidation output scope must be Consolidated".to_string(),
                    });
                }
            },
            minority_equity: minority_equity_total,
            equity_to_parent,
            minority_ni: minority_ytd,
            ni_to_parent: ytd_income.sub(minority_ytd)?,
            consolidated_ni: ytd_income,
            window_ni_to_parent: window_income.sub(minority_window)?,
            window_minority_ni: minority_window,
            non_root_equity: self.non_root_equity,
            prior_split,
        };
        Ok(self.acc.finish(self.defs, Some(facts)))
    }
}

fn entry_income(entry: &JournalEntry, books: &Books) -> Result<AccountingAmount, AccountingError> {
    let mut income = AccountingAmount::ZERO;
    for line in &entry.lines {
        let def = books.ledger().chart().get(&line.account).ok_or_else(|| {
            AccountingError::UnknownAccount {
                event: entry.source,
                account: line.account.clone(),
            }
        })?;
        if matches!(
            def.element,
            AccountElement::Revenue | AccountElement::Expense
        ) {
            income = match line.side {
                PostingSide::Debit => income.sub(line.amount)?,
                PostingSide::Credit => income.add(line.amount)?,
            };
        }
    }
    Ok(income)
}

/// 单成员权益科目净贷方贡献（≤ bound）：按代码累计（`into` 为 Some 时）
/// 并返回合计（根成员上年资本用标量路径）。
fn credit_of_equity(
    entry: &JournalEntry,
    books: &Books,
    mut into: Option<&mut BTreeMap<LedgerAccountId, AccountingAmount>>,
    bound: AccountingPeriod,
) -> Result<AccountingAmount, AccountingError> {
    if entry.period() > bound {
        return Ok(AccountingAmount::ZERO);
    }
    let mut total = AccountingAmount::ZERO;
    for line in &entry.lines {
        if !is_element(entry, books, &line.account, AccountElement::Equity)? {
            continue;
        }
        let credit = match line.side {
            PostingSide::Debit => line.amount.neg()?,
            PostingSide::Credit => line.amount,
        };
        total = total.add(credit)?;
        if let Some(map) = into.as_deref_mut() {
            let slot = map.entry(line.account.clone()).or_default();
            *slot = slot.add(credit)?;
        }
    }
    Ok(total)
}

/// 行科目要素判定（未知科目 = 底座已拒绝的非法输入，此处显式透传）。
fn is_element(
    entry: &JournalEntry,
    books: &Books,
    account: &LedgerAccountId,
    element: AccountElement,
) -> Result<bool, AccountingError> {
    match books.ledger().chart().get(account) {
        Some(def) => Ok(def.element == element),
        None => Err(AccountingError::UnknownAccount {
            event: entry.source,
            account: account.clone(),
        }),
    }
}

/// 单成员「权益滚动」增量（≤ bound）：权益科目贷方差额 + 净利。
fn equity_rolling_delta(
    entry: &JournalEntry,
    books: &Books,
    bound: AccountingPeriod,
) -> Result<AccountingAmount, AccountingError> {
    if entry.period() > bound {
        return Ok(AccountingAmount::ZERO);
    }
    let mut delta = AccountingAmount::ZERO;
    for line in &entry.lines {
        let Some(def) = books.ledger().chart().get(&line.account) else {
            return Err(AccountingError::UnknownAccount {
                event: entry.source,
                account: line.account.clone(),
            });
        };
        let signed = match line.side {
            PostingSide::Debit => line.amount,
            PostingSide::Credit => line.amount.neg()?,
        };
        let contribution = match def.element {
            AccountElement::Equity | AccountElement::Revenue | AccountElement::Expense => {
                signed.neg()?
            }
            AccountElement::Asset | AccountElement::Liability => AccountingAmount::ZERO,
        };
        delta = delta.add(contribution)?;
    }
    Ok(delta)
}

/// 上年年末合并拆分（少数 = Σ子公司上年权益 × 少数基点；归母 = 合计 − 少数）。
fn prior_year_split(
    output: &ConsolidationOutput,
    member_prior_equity: &BTreeMap<MemberId, AccountingAmount>,
    root_prior_capital: AccountingAmount,
    has_history: bool,
) -> Result<Option<PriorSplit>, ReportError> {
    if !has_history {
        return Ok(None);
    }
    let mut total = AccountingAmount::ZERO;
    for equity in member_prior_equity.values() {
        total = total
            .add(*equity)
            .map_err(|e| ReportError::Accounting(Box::new(e)))?;
    }
    let mut minority = AccountingAmount::ZERO;
    for interest in &output.minority {
        let Some(equity) = member_prior_equity.get(&interest.subsidiary) else {
            return Err(ReportError::InternalWindowInconsistent {
                detail: format!(
                    "subsidiary {} missing from member scan",
                    interest.subsidiary
                ),
            });
        };
        minority = minority.add(apply_minority_bp(*equity, interest)?)?;
    }
    Ok(Some(PriorSplit {
        minority,
        parent: total
            .sub(minority)
            .map_err(|e| ReportError::Accounting(Box::new(e)))?,
        root_capital: root_prior_capital,
    }))
}

/// 少数基点应用（bp 构造上 ∈ [0, 10000)，i32 值域恒安全）。
fn apply_minority_bp(
    amount: AccountingAmount,
    interest: &MinorityInterest,
) -> Result<AccountingAmount, ReportError> {
    let bp = i32::try_from(interest.minority_bp)
        .expect("minority bp is bounded to [0, 10000) by group validation");
    amount
        .apply_basis_points(bp)
        .map_err(|e| ReportError::Accounting(Box::new(e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::consolidation::MemberSpec;
    use crate::accounting::journal::{BusinessEventId, BusinessKind, CashFlowClass, JournalLine};
    use crate::accounting::ledger::AccountChart;
    use crate::calendar::CivilDate;

    fn books(capital: i128, date: &str) -> Books {
        let mut books = Books::new(AccountChart::generic_account_chart());
        books
            .post_batch(vec![JournalEntry {
                source: BusinessEventId::new(1),
                date: CivilDate::from_iso(date).unwrap(),
                kind: BusinessKind::OpeningBalance,
                cash_flow: CashFlowClass::Financing,
                lines: vec![
                    JournalLine {
                        account: LedgerAccountId("1001".into()),
                        side: PostingSide::Debit,
                        amount: AccountingAmount::from_cents(capital),
                    },
                    JournalLine {
                        account: LedgerAccountId("4001".into()),
                        side: PostingSide::Credit,
                        amount: AccountingAmount::from_cents(capital),
                    },
                ],
            }])
            .unwrap();
        books
    }

    #[test]
    fn member_scan_keeps_multiple_minority_splits_and_no_history() {
        for date in ["2029-12-01", "2030-01-01"] {
            let root = books(8, date);
            let first = books(10, date);
            let second = books(20, date);
            let member = |id: &str, parent: Option<&str>, held, books| GroupMember {
                spec: MemberSpec {
                    id: MemberId(id.into()),
                    group_parent: parent.map(|id| MemberId(id.into())),
                    issued_shares: 100,
                    parent_held_shares: held,
                },
                books,
            };
            let request = ConsolidationRequest {
                root: MemberId("root".into()),
                members: vec![
                    member("second", Some("root"), 80, &second),
                    member("root", None, 0, &root),
                    member("first", Some("root"), 60, &first),
                ],
                intercompany_balances: vec![],
                intercompany_sales: vec![],
            };
            let current = AccountingPeriod::from_ymd(2030, 1).unwrap();
            let prior = AccountingPeriod::from_ymd(2029, 1).unwrap();
            let windows = consolidated(request, (current, current), (prior, prior)).unwrap();
            let facts = windows.consolidation.unwrap();
            assert_eq!(
                facts.non_root_equity[&LedgerAccountId("4001".into())].cents(),
                30
            );
            assert_eq!(facts.minority_equity.cents(), 8);
            assert_eq!(facts.equity_to_parent.cents(), 30);
            if date.starts_with("2029") {
                let split = facts.prior_split.unwrap();
                assert_eq!(split.root_capital.cents(), 8);
                assert_eq!(split.minority.cents(), 8);
                assert_eq!(split.parent.cents(), 30);
            } else {
                assert!(facts.prior_split.is_none());
                assert!(windows.prior_year_end.is_none());
            }
        }
    }
}
