//! 附注与列报归类（K3，任务 13）：科目 → 主表行的**单一归类表** + 附注明细。
//!
//! 归类纪律（验收红线）：
//! - **科目无归属 = 类型化拒绝**：科目表内每个科目都必须归入恰好一条主表行
//!   （`UnclassifiedAccount`），杜绝「漏科目」；
//! - **重复分类拒绝**：同一科目映射到两条不同主表行 → `DuplicateClassification`
//!   （同目标重复幂等合法——合并 Scope 跨成员行业表合并的基础）；
//! - **附注明细合计 == 主表行**（勾稽在 [`super::ReportSet::validate`]）。
//!
//! 归类表 = 通用基表（v1 全集）∪ 行业表（v2–v5；代码真源在各行业文件）。
//! 合并 Scope 的归类 = 成员行业表按代码合并（不同目标 = 冲突拒绝）。

use std::collections::BTreeMap;

use crate::accounting::amount::AccountingAmount;
use crate::accounting::ledger::LedgerAccountId;

use super::balance_sheet::BsLine;
use super::income::IncomeLine;
use super::window::StatementWindows;
use super::IndustryPresentation;

/// 归类目标：主表 + 行。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum NoteTarget {
    BalanceSheet(BsLine),
    Income(IncomeLine),
}

/// 单条归类（代码 → 目标）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Assignment {
    pub code: &'static str,
    pub target: NoteTarget,
}

pub(crate) const fn a(code: &'static str, target: NoteTarget) -> Assignment {
    Assignment { code, target }
}

/// 通用基表（v1 全集 16 科目；工业表 = 基表 + 工业扩充）。
pub fn base_assignments() -> Vec<Assignment> {
    use BsLine::*;
    use IncomeLine::*;
    vec![
        a("1001", NoteTarget::BalanceSheet(CashFunds)),
        a("1002", NoteTarget::BalanceSheet(CashFunds)),
        a("1122", NoteTarget::BalanceSheet(Receivables)),
        a("1601", NoteTarget::BalanceSheet(FixedAssets)),
        a("1602", NoteTarget::BalanceSheet(FixedAssets)),
        a("2001", NoteTarget::BalanceSheet(ShortTermBorrowings)),
        a("2202", NoteTarget::BalanceSheet(AccountsPayable)),
        a("2221", NoteTarget::BalanceSheet(TaxesPayable)),
        a("2231", NoteTarget::BalanceSheet(InterestPayable)),
        a("4001", NoteTarget::BalanceSheet(PaidInCapital)),
        a("4103", NoteTarget::BalanceSheet(RetainedEarnings)),
        a("6001", NoteTarget::Income(OperatingRevenue)),
        a("6401", NoteTarget::Income(OperatingCost)),
        a("6602", NoteTarget::Income(AdministrativeExpense)),
        a("6603", NoteTarget::Income(FinanceExpense)),
        a("6801", NoteTarget::Income(IncomeTaxExpense)),
    ]
}

/// 合并两列归类表：同码同目标幂等；同码不同目标 → 类型化拒绝。
pub fn merge_assignments(
    base: Vec<Assignment>,
    industry: Vec<Assignment>,
) -> Result<BTreeMap<LedgerAccountId, NoteTarget>, super::ReportError> {
    merge_into(merge_into(BTreeMap::new(), base)?, industry)
}

fn merge_into(
    mut merged: BTreeMap<LedgerAccountId, NoteTarget>,
    list: Vec<Assignment>,
) -> Result<BTreeMap<LedgerAccountId, NoteTarget>, super::ReportError> {
    for Assignment { code, target } in list {
        let id = LedgerAccountId(code.to_string());
        match merged.get(&id) {
            Some(existing) if *existing != target => {
                return Err(super::ReportError::DuplicateClassification {
                    code: id,
                    first: existing.label(),
                    second: target.label(),
                });
            }
            _ => merged.insert(id, target),
        };
    }
    Ok(merged)
}

/// 按行业取完整归类表（基表 ∪ 行业表；银行/保险/地产表 = 本行业全量表）。
pub fn assignments_for(industry: IndustryPresentation) -> Vec<Assignment> {
    match industry {
        IndustryPresentation::Industrial => {
            merge_lists(base_assignments(), super::industrial::extra_assignments())
        }
        IndustryPresentation::Bank => super::bank::assignments(),
        IndustryPresentation::Insurance => super::insurance::assignments(),
        IndustryPresentation::RealEstate => super::real_estate::assignments(),
    }
}

fn merge_lists(base: Vec<Assignment>, extra: Vec<Assignment>) -> Vec<Assignment> {
    let mut out = base;
    out.extend(extra);
    out
}

/// 已核验冲突与科目表覆盖的列报分类；额外行业映射仍保留。
pub(crate) struct ReportClassification {
    assignments: BTreeMap<LedgerAccountId, NoteTarget>,
}

impl ReportClassification {
    pub(crate) fn from_members(
        members: &[crate::accounting::consolidation::GroupMember<'_>],
    ) -> Result<Self, super::ReportError> {
        let books = members
            .iter()
            .map(|member| (member.spec.id.clone(), member.books))
            .collect();
        let keys = crate::accounting::consolidation::account_keys(&books);
        let mut assignments = BTreeMap::new();
        for member in members {
            let industry = super::industry_of_chart(member.books.ledger().chart())?;
            let source = merge_into(BTreeMap::new(), assignments_for(industry))?;
            for (code, _) in member.books.ledger().chart().iter() {
                let target =
                    source
                        .get(code)
                        .ok_or_else(|| super::ReportError::UnclassifiedAccount {
                            code: code.clone(),
                        })?;
                let key = keys[&(member.spec.id.clone(), code.clone())].clone();
                if let Some(existing) = assignments.insert(key.clone(), target.clone()) {
                    if existing != *target {
                        return Err(super::ReportError::DuplicateClassification {
                            code: key,
                            first: existing.label(),
                            second: target.label(),
                        });
                    }
                }
            }
        }
        Ok(Self { assignments })
    }

    pub(crate) fn from_industries(
        defs: &BTreeMap<LedgerAccountId, crate::accounting::ledger::AccountDef>,
        industries: &[IndustryPresentation],
    ) -> Result<Self, super::ReportError> {
        let mut merged = BTreeMap::new();
        for industry in industries {
            merged = merge_into(merged, assignments_for(*industry))?;
        }
        for code in defs.keys() {
            if !merged.contains_key(code) {
                return Err(super::ReportError::UnclassifiedAccount { code: code.clone() });
            }
        }
        Ok(Self {
            assignments: merged,
        })
    }

    pub(crate) fn target_for(&self, code: &LedgerAccountId) -> Option<&NoteTarget> {
        self.assignments.get(code)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&LedgerAccountId, &NoteTarget)> {
        self.assignments.iter()
    }
}

/// 附注明细行。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct NoteItem {
    pub code: String,
    pub name: String,
    pub target: NoteTarget,
    /// 期初净借方（窗口首期前）。
    pub opening: AccountingAmount,
    /// 窗口净借方运动。
    pub movement: AccountingAmount,
    /// 年初至今净借方运动。
    pub ytd_movement: AccountingAmount,
    /// 期末净借方。
    pub closing: AccountingAmount,
}

/// 附注（五产物之一）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Notes {
    /// 有发生额/余额科目的明细（代码序）。
    pub items: Vec<NoteItem>,
    /// 合并拆分披露：非根成员权益科目（归母/少数拆分行吸收，不参与主表行勾稽）。
    pub consolidation_split_items: Vec<NoteItem>,
}

/// 由窗口读投影构造附注（纯投影；勾稽校验在 ReportSet::validate）。
/// 期初 = 期末 − 运动，溢出为类型化错误——已公布附注绝不以 0 掩盖。
pub(crate) fn build_notes(
    windows: &StatementWindows,
    classification: &ReportClassification,
) -> Result<Notes, super::ReportError> {
    let zero = AccountingAmount::ZERO;
    let mut items = Vec::new();
    for (code, target) in classification.iter() {
        let closing = windows.closing.get(code).copied().unwrap_or(zero);
        let movement = windows.movement.get(code).copied().unwrap_or(zero);
        let ytd = windows.ytd.get(code).copied().unwrap_or(zero);
        if closing.is_zero() && movement.is_zero() && ytd.is_zero() {
            continue;
        }
        let name = windows
            .defs
            .get(code)
            .map(|def| def.name.clone())
            .unwrap_or_else(|| code.0.clone());
        items.push(NoteItem {
            code: code.0.clone(),
            name,
            target: target.clone(),
            opening: closing.sub(movement)?,
            movement,
            ytd_movement: ytd,
            closing,
        });
    }
    let mut split = Vec::new();
    if let Some(facts) = &windows.consolidation {
        for (code, credit) in &facts.non_root_equity {
            if credit.is_zero() {
                continue;
            }
            let target = classification
                .target_for(code)
                .cloned()
                .unwrap_or(NoteTarget::BalanceSheet(BsLine::PaidInCapital));
            let name = windows
                .defs
                .get(code)
                .map(|def| def.name.clone())
                .unwrap_or_else(|| code.0.clone());
            split.push(NoteItem {
                code: code.0.clone(),
                name,
                target,
                opening: zero,
                movement: zero,
                ytd_movement: zero,
                closing: *credit,
            });
        }
    }
    Ok(Notes {
        items,
        consolidation_split_items: split,
    })
}

impl NoteTarget {
    /// 归类标签（错误信息与调试用）。
    pub fn label(&self) -> &'static str {
        match self {
            NoteTarget::BalanceSheet(line) => line.label(),
            NoteTarget::Income(line) => line.label(),
        }
    }
}

/// 资产负债表行标签（250 行天花板把标签表移到本归类模块——标签是列报
/// 元数据，与归类表同源）。
impl BsLine {
    pub fn label(&self) -> &'static str {
        match self {
            BsLine::CashFunds => "货币资金",
            BsLine::Receivables => "应收账款",
            BsLine::InsuranceReceivables => "应收保费",
            BsLine::Inventory => "存货",
            BsLine::DevelopmentInventory => "开发存货",
            BsLine::FixedAssets => "固定资产",
            BsLine::LoansAndAdvances => "贷款及垫款",
            BsLine::DeferredTaxAssets => "递延所得税资产",
            BsLine::ShortTermBorrowings => "短期借款",
            BsLine::AccountsPayable => "应付账款",
            BsLine::ContractLiabilities => "合同负债",
            BsLine::TaxesPayable => "应交税费",
            BsLine::InterestPayable => "应付利息",
            BsLine::CustomerDeposits => "吸收存款",
            BsLine::LongTermBorrowings => "长期借款",
            BsLine::InsuranceContractLiabilities => "保险合同负债",
            BsLine::DeferredTaxLiabilities => "递延所得税负债",
            BsLine::PaidInCapital => "实收资本",
            BsLine::RetainedEarnings => "未分配利润",
            BsLine::MinorityEquity => "少数股东权益",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::ledger::{AccountDef, AccountElement};

    #[test]
    fn classification_keeps_extra_mappings_and_same_target_idempotence() {
        for industry in [
            IndustryPresentation::Industrial,
            IndustryPresentation::Bank,
            IndustryPresentation::Insurance,
            IndustryPresentation::RealEstate,
        ] {
            let classification =
                ReportClassification::from_industries(&BTreeMap::new(), &[industry]).unwrap();
            let repeated =
                ReportClassification::from_industries(&BTreeMap::new(), &[industry, industry])
                    .unwrap();
            assert!(!classification.assignments.is_empty());
            assert_eq!(classification.assignments, repeated.assignments);
            let defs = classification
                .iter()
                .map(|(code, _)| (code.clone(), AccountDef::new("科目", AccountElement::Asset)))
                .collect();
            assert!(ReportClassification::from_industries(&defs, &[industry]).is_ok());
        }
    }

    #[test]
    fn classification_rejects_missing_code_and_conflicting_targets() {
        let code = LedgerAccountId("unknown".into());
        let defs = BTreeMap::from([(
            code.clone(),
            AccountDef::new("未分类", AccountElement::Asset),
        )]);
        assert!(
            matches!(ReportClassification::from_industries(&defs, &[IndustryPresentation::Industrial]), Err(super::super::ReportError::UnclassifiedAccount { code: rejected }) if rejected == code)
        );
        assert!(matches!(
            merge_assignments(
                vec![a("1001", NoteTarget::BalanceSheet(BsLine::CashFunds))],
                vec![a("1001", NoteTarget::BalanceSheet(BsLine::Inventory))]
            ),
            Err(super::super::ReportError::DuplicateClassification { .. })
        ));
    }
}
