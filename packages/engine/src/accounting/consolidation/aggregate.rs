//! 单体汇总（K3，任务 12）：期间覆盖一致性 + 跨科目表按代码加总。
//!
//! 混合行业集团合法（工业母公司 + 银行子公司等）：各行业科目表（v2–v5）
//! 语义可以不同，合并按**科目代码**加总——代码相同则 T 型合计相加，只存在
//! 于部分成员的代码只由该侧贡献。加总前提：同一代码在两张表中的要素/
//! 现金/备抵标志必须一致（名称是列报事务，允许不同），否则类型化拒绝。
//!
//! 期间覆盖：固定集团要求全部成员的**已记账期间集合**（日记账分录所属
//! 期间）完全一致；不一致（例如子公司缺 2030-01 记账）显式拒绝。

use std::collections::{BTreeMap, BTreeSet};

use crate::accounting::amount::AccountingAmount;
use crate::accounting::error::AccountingError;
use crate::accounting::journal::JournalEntry;
use crate::accounting::ledger::{AccountDef, LedgerAccountId};
use crate::accounting::period::AccountingPeriod;
use crate::accounting::Books;

use super::error::ConsolidationError;
use super::group::MemberId;

/// 合并后单科目余额：科目定义（跨成员语义一致）+ T 型合计（成员合计 +
/// 工作底稿抵销调整）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ConsolidatedBalance {
    pub def: AccountDef,
    pub debit_total: AccountingAmount,
    pub credit_total: AccountingAmount,
}

impl ConsolidatedBalance {
    /// 净借方余额（借 − 贷；贷方余额为负）。
    pub fn net_debit(&self) -> Result<AccountingAmount, AccountingError> {
        self.debit_total.sub(self.credit_total)
    }
}

/// 汇总账本：科目代码 → 合并余额（含后续抵销调整）。
pub(crate) type AggregatedBalances = BTreeMap<LedgerAccountId, ConsolidatedBalance>;

/// 校验期间覆盖一致（基准 = 首个成员的覆盖集合，按 id 序）。
pub(crate) fn check_period_coverage(
    members: &BTreeMap<MemberId, &Books>,
) -> Result<(), ConsolidationError> {
    let mut expected: Option<(MemberId, BTreeSet<AccountingPeriod>)> = None;
    for (id, books) in members {
        let covered: BTreeSet<AccountingPeriod> = books
            .journal()
            .entries()
            .map(JournalEntry::period)
            .collect();
        match &expected {
            None => expected = Some((id.clone(), covered)),
            Some((_, base)) => {
                if &covered != base {
                    return Err(ConsolidationError::PeriodCoverageMismatch {
                        member: id.clone(),
                        expected: base.iter().copied().collect(),
                        actual: covered.iter().copied().collect(),
                    });
                }
            }
        }
    }
    Ok(())
}

/// 单体汇总：按科目代码加总全部成员的 T 型合计。
///
/// 只收录**有发生额**的科目（某成员借/贷合计非零即收录；全员零发生额的
/// 科目不进合并账本——查询语义与 Ledger 读投影一致：无过账即零余额）。
pub(crate) fn aggregate_balances(
    members: &BTreeMap<MemberId, &Books>,
) -> Result<AggregatedBalances, ConsolidationError> {
    let mut merged: AggregatedBalances = BTreeMap::new();
    // code → (首个定义该代码的成员, 定义) —— 语义冲突检测基准。
    let mut def_owners: BTreeMap<LedgerAccountId, (MemberId, AccountDef)> = BTreeMap::new();
    for (id, books) in members {
        let ledger = books.ledger();
        for (code, def) in ledger.chart().iter() {
            let balance = ledger.account_balance(code);
            if balance.debit_total().is_zero() && balance.credit_total().is_zero() {
                continue;
            }
            match def_owners.get(code) {
                None => {
                    def_owners.insert(code.clone(), (id.clone(), def.clone()));
                    merged.insert(
                        code.clone(),
                        ConsolidatedBalance {
                            def: def.clone(),
                            debit_total: balance.debit_total(),
                            credit_total: balance.credit_total(),
                        },
                    );
                }
                Some((owner, base)) => {
                    check_chart_conflict(id, code, def, owner, base)?;
                    // 首见代码时已随 def_owners 一同插入（两映射同键同生命周期）。
                    let entry = merged
                        .get_mut(code)
                        .expect("aggregation inserts the account on first sight");
                    entry.debit_total = entry.debit_total.add(balance.debit_total())?;
                    entry.credit_total = entry.credit_total.add(balance.credit_total())?;
                }
            }
        }
    }
    Ok(merged)
}

/// 同代码跨表语义冲突检测（要素/现金/备抵必须一致；名称允许不同）。
fn check_chart_conflict(
    member: &MemberId,
    code: &LedgerAccountId,
    def: &AccountDef,
    owner: &MemberId,
    base: &AccountDef,
) -> Result<(), ConsolidationError> {
    if def.element == base.element && def.is_cash == base.is_cash && def.is_contra == base.is_contra
    {
        return Ok(());
    }
    let describe =
        |d: &AccountDef| format!("{:?} cash={} contra={}", d.element, d.is_cash, d.is_contra);
    Err(ConsolidationError::ChartConflict {
        code: code.clone(),
        member_a: owner.clone(),
        detail_a: describe(base),
        member_b: member.clone(),
        detail_b: describe(def),
    })
}
