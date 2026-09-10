//! 合并抵销逻辑（K3，任务 12）：内部往来的申报配对校验与工作底稿分录
//! 生成 + 抵销应用到汇总余额（销售抵销在 [`super::sale`]；值类型在
//! [`super::worksheet`]）。
//!
//! 铁律：
//! - 抵销分录属于合并工作底稿，**绝不回记任何成员账套**——集团现金逐分
//!   不变（`集团现金 == Σ 成员现金`），由「申报科目不得为现金」+ 全部
//!   抵销科目（应收/应付/收入/成本/存货）天然非现金共同保证。
//! - 往来抵销要求对手方两侧余额**精确相等**（一资产一负债）：不符 →
//!   类型化错误列出两侧数值，绝不用差额 plug 平账。

use std::collections::BTreeMap;

use crate::accounting::journal::PostingSide;
use crate::accounting::ledger::{AccountDef, AccountElement, LedgerAccountId};
use crate::accounting::Books;

use super::aggregate::AggregatedBalances;
use super::error::{ConsolidationError, DeclaredSide};
use super::group::{MemberId, ValidatedGroup};
use super::sale::eliminate_sale;
use super::worksheet::{
    IntercompanyBalance, IntercompanySale, WorksheetEntry, WorksheetLine, WorksheetReason,
};
use PostingSide::{Credit, Debit};

/// 校验申报并生成工作底稿分录（申报顺序 = 输出顺序，确定性）。
///
/// 往来抵销按**成员对**去重：一对「资产侧申报 + 负债侧申报」只产生一条
/// 抵销分录（两侧各申报一次是正常输入形态，不是重复抵销）。
pub(crate) fn build_worksheet(
    group: &ValidatedGroup,
    members: &BTreeMap<MemberId, &Books>,
    balances: &[IntercompanyBalance],
    sales: &[IntercompanySale],
) -> Result<Vec<WorksheetEntry>, ConsolidationError> {
    let mut worksheet = Vec::new();
    let mut handled = vec![false; balances.len()];
    for (index, decl) in balances.iter().enumerate() {
        if handled[index] {
            continue;
        }
        precheck_balance(group, members, decl)?;
        // 对侧申报 = 相同成员对、方向相反（成员对上的首条；同对多条申报
        // 由要素配对与金额精确检查共同约束，确定性按输入顺序）。
        let Some(mirror_index) = balances.iter().position(|other| {
            other.member == decl.counterparty && other.counterparty == decl.member
        }) else {
            return Err(ConsolidationError::CounterpartyMismatch {
                side_a: declared_side(decl),
                side_b: None,
            });
        };
        handled[index] = true;
        handled[mirror_index] = true;
        worksheet.push(balance_entry(members, decl, &balances[mirror_index])?);
    }
    for sale in sales {
        worksheet.push(eliminate_sale(group, members, sale)?);
    }
    Ok(worksheet)
}

/// 单笔申报的基本校验（成员存在、非自指、科目存在、非现金）——先于配对。
fn precheck_balance(
    group: &ValidatedGroup,
    members: &BTreeMap<MemberId, &Books>,
    decl: &IntercompanyBalance,
) -> Result<(), ConsolidationError> {
    ensure_member(group, &decl.member)?;
    ensure_member(group, &decl.counterparty)?;
    if decl.member == decl.counterparty {
        return Err(ConsolidationError::IntercompanySelfReference {
            member: decl.member.clone(),
        });
    }
    let def = member_def(members, &decl.member, &decl.account)?;
    if def.is_cash {
        return Err(ConsolidationError::IntercompanyTouchesCash {
            member: decl.member.clone(),
            account: decl.account.clone(),
        });
    }
    Ok(())
}

/// 配对成功后的往来抵销分录：Dr 负债侧 / Cr 资产侧（两侧金额精确相等——
/// 不等即拒绝，绝不 plug）。
fn balance_entry(
    members: &BTreeMap<MemberId, &Books>,
    decl: &IntercompanyBalance,
    mirror: &IntercompanyBalance,
) -> Result<WorksheetEntry, ConsolidationError> {
    let def = member_def(members, &decl.member, &decl.account)?;
    let mirror_def = member_def(members, &mirror.member, &mirror.account)?;
    if mirror_def.is_cash {
        return Err(ConsolidationError::IntercompanyTouchesCash {
            member: mirror.member.clone(),
            account: mirror.account.clone(),
        });
    }
    if mirror.amount != decl.amount {
        return Err(ConsolidationError::CounterpartyMismatch {
            side_a: declared_side(decl),
            side_b: Some(Box::new(declared_side(mirror))),
        });
    }
    let (asset_side, liability_side) = match (def.element, mirror_def.element) {
        (AccountElement::Asset, AccountElement::Liability) => (decl, mirror),
        (AccountElement::Liability, AccountElement::Asset) => (mirror, decl),
        (a, b) => {
            return Err(ConsolidationError::IntercompanyPairShape {
                member_a: decl.member.clone(),
                account_a: decl.account.clone(),
                element_a: a,
                member_b: mirror.member.clone(),
                account_b: mirror.account.clone(),
                element_b: b,
            });
        }
    };
    Ok(WorksheetEntry {
        reason: WorksheetReason::IntercompanyBalance,
        lines: vec![
            WorksheetLine {
                member: liability_side.member.clone(),
                account: liability_side.account.clone(),
                side: Debit,
                amount: decl.amount,
            },
            WorksheetLine {
                member: asset_side.member.clone(),
                account: asset_side.account.clone(),
                side: Credit,
                amount: decl.amount,
            },
        ],
    })
}

/// 把工作底稿分录应用到汇总余额（T 型合计增量）。
pub(crate) fn apply_worksheet(
    balances: &mut AggregatedBalances,
    worksheet: &[WorksheetEntry],
) -> Result<(), ConsolidationError> {
    for entry in worksheet {
        for line in &entry.lines {
            let Some(account) = balances.get_mut(&line.account) else {
                // 申报校验已保证科目存在于成员科目表；无发生额科目不可能
                // 出现在抵销行（有余额才有往来）。到达这里 = 内部不一致。
                return Err(ConsolidationError::UnknownIntercompanyAccount {
                    member: line.member.clone(),
                    account: line.account.clone(),
                });
            };
            match line.side {
                Debit => account.debit_total = account.debit_total.add(line.amount)?,
                Credit => account.credit_total = account.credit_total.add(line.amount)?,
            }
        }
    }
    Ok(())
}

/// 成员必须在集团内（根或直接子公司）。
pub(super) fn ensure_member(
    group: &ValidatedGroup,
    id: &MemberId,
) -> Result<(), ConsolidationError> {
    let known = id == &group.root || group.subsidiaries.iter().any(|sub| &sub.id == id);
    if known {
        Ok(())
    } else {
        Err(ConsolidationError::UnknownIntercompanyMember { member: id.clone() })
    }
}

/// 成员科目表中的科目定义（不存在 → 类型化拒绝）。
pub(super) fn member_def(
    members: &BTreeMap<MemberId, &Books>,
    id: &MemberId,
    account: &LedgerAccountId,
) -> Result<AccountDef, ConsolidationError> {
    members[id]
        .ledger()
        .chart()
        .get(account)
        .cloned()
        .ok_or_else(|| ConsolidationError::UnknownIntercompanyAccount {
            member: id.clone(),
            account: account.clone(),
        })
}

/// 构造申报侧错误上下文。
fn declared_side(decl: &IntercompanyBalance) -> DeclaredSide {
    DeclaredSide {
        member: decl.member.clone(),
        account: decl.account.clone(),
        amount: decl.amount,
    }
}
