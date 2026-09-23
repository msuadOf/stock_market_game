//! 合并窗口构造（K3，任务 13）：Σ成员按代码加总 → 任务 12 合并 → 工作底稿
//! 抵销折入 + 少数股东拆分事实。
//!
//! 语义决定（登记于 issues 的游戏简化）：
//! - 工作底稿抵销的**流量效应归属合并申报当期**（任务 12 申报不携带期间
//!   属性；重述/跨期不追溯）。
//! - 合并资产负债表权益列：实收资本 = **根成员** 4001；归母留存 = 归母权益
//!   − 根成员实收资本（子公司权益的母公司份额并入留存——固定控制、无并购
//!   计量模型）；少数股东权益 = 任务 12 拆分。非根成员权益科目在附注的
//!   合并拆分披露中单独列示。
//! - 上年年末比较项的合并拆分按「成员上年权益 × 少数基点」推导。

use std::collections::{BTreeMap, BTreeSet};

use crate::accounting::amount::AccountingAmount;
use crate::accounting::consolidation::{
    consolidate, ConsolidationOutput, ConsolidationRequest, MemberId, MinorityInterest, ScopeId,
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

/// 合并报表所需的任务 12 输出事实。
pub(crate) struct ConsolidationFacts {
    pub root: MemberId,
    pub minority_equity: AccountingAmount,
    pub equity_to_parent: AccountingAmount,
    pub minority_ni: AccountingAmount,
    pub ni_to_parent: AccountingAmount,
    pub consolidated_ni: AccountingAmount,
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
    let ConsolidationRequest {
        root,
        members,
        intercompany_balances,
        intercompany_sales,
    } = request;
    let mut acc = Accumulator::new(window, prior_window)?;
    let mut defs: BTreeMap<LedgerAccountId, AccountDef> = BTreeMap::new();
    let mut sub_ids: BTreeSet<MemberId> = BTreeSet::new();
    for member in &members {
        if member.spec.group_parent.is_some() {
            sub_ids.insert(member.spec.id.clone());
        }
        for (id, def) in member.books.ledger().chart().iter() {
            defs.entry(id.clone()).or_insert_with(|| def.clone());
        }
    }
    let mut member_prior_equity: BTreeMap<MemberId, AccountingAmount> = BTreeMap::new();
    let mut non_root_equity: BTreeMap<LedgerAccountId, AccountingAmount> = BTreeMap::new();
    let mut root_prior_capital = AccountingAmount::ZERO;
    for member in &members {
        let is_sub = sub_ids.contains(&member.spec.id);
        let is_root = member.spec.id == root;
        let mut prior_equity = AccountingAmount::ZERO;
        for entry in member.books.journal().entries() {
            acc.add_entry(entry, entry.period(), false, &defs)
                .map_err(|e| ReportError::Accounting(Box::new(e)))?;
            prior_equity = prior_equity
                .add(equity_rolling_delta(
                    entry,
                    member.books,
                    acc.prior_dec_bound(),
                )?)
                .map_err(|e| ReportError::Accounting(Box::new(e)))?;
            if is_sub {
                credit_of_equity(entry, member.books, Some(&mut non_root_equity), window.1)?;
            }
            if is_root {
                root_prior_capital = root_prior_capital
                    .add(credit_of_equity(
                        entry,
                        member.books,
                        None,
                        acc.prior_dec_bound(),
                    )?)
                    .map_err(|e| ReportError::Accounting(Box::new(e)))?;
            }
        }
        member_prior_equity.insert(member.spec.id.clone(), prior_equity);
    }
    let output: ConsolidationOutput = consolidate(ConsolidationRequest {
        root,
        members,
        intercompany_balances,
        intercompany_sales,
    })?;
    apply_worksheet(&mut acc, &output)?;
    let prior_split = prior_year_split(
        &output,
        &member_prior_equity,
        root_prior_capital,
        acc.has_prior_history(),
    )?;
    let ConsolidationOutput {
        scope,
        minority_equity_total,
        equity_to_parent,
        net_income_to_minority,
        net_income_to_parent,
        consolidated_net_income,
        ..
    } = output;
    let facts = ConsolidationFacts {
        root: match &scope {
            ScopeId::Consolidated(root) => root.clone(),
            ScopeId::Standalone(_) => {
                return Err(ReportError::InternalWindowInconsistent {
                    detail: "consolidation output scope must be Consolidated".to_string(),
                })
            }
        },
        minority_equity: minority_equity_total,
        equity_to_parent,
        minority_ni: net_income_to_minority,
        ni_to_parent: net_income_to_parent,
        consolidated_ni: consolidated_net_income,
        non_root_equity,
        prior_split,
    };
    Ok(acc.finish(defs, Some(facts)))
}

/// 工作底稿行折入有效期间各桶（无现金——任务 12 红线）。
fn apply_worksheet(acc: &mut Accumulator, output: &ConsolidationOutput) -> Result<(), ReportError> {
    for entry in &output.worksheet {
        for line in &entry.lines {
            let delta = match line.side {
                PostingSide::Debit => line.amount,
                PostingSide::Credit => line
                    .amount
                    .neg()
                    .map_err(|e| ReportError::Accounting(Box::new(e)))?,
            };
            acc.bucket_all(&line.account, delta)
                .map_err(|e| ReportError::Accounting(Box::new(e)))?;
        }
    }
    Ok(())
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
