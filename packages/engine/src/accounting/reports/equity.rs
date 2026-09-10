//! 所有者权益变动表生成器（K3，任务 13）：期初 → 净利 → 其他综合收益 →
//! 所有者投入 → 对所有者分配 → 期末（CAS 30（2026）§60/§61 已核验：
//! 综合收益与所有者资本交易分别列示；向所有者分配必须列示）。
//!
//! 游戏红线（K3 guardrail #4）：**对所有者分配行恒为 0**——公司域不存在
//! 股东分配路径（结构保证，非 clamp）；OCI 同理恒 0（无 OCI 科目）。
//! 合并 Scope 追加少数股东权益列（拆分来自任务 12 输出）。

use crate::accounting::amount::AccountingAmount;

use super::window::net_income_of;
use super::window::StatementWindows;
use super::ReportError;

/// 所有者权益变动表。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct EquityStatement {
    pub opening_parent: AccountingAmount,
    pub net_income: AccountingAmount,
    pub other_comprehensive: AccountingAmount,
    pub capital_contributions: AccountingAmount,
    pub distributions: AccountingAmount,
    pub closing_parent: AccountingAmount,
    pub opening_minority: Option<AccountingAmount>,
    pub minority_net_income: Option<AccountingAmount>,
    pub closing_minority: Option<AccountingAmount>,
}

/// 单体期初/期末权益滚动 = 权益科目贷余 + 累计净利。
fn equity_rolling(
    windows: &StatementWindows,
    map: &std::collections::BTreeMap<crate::accounting::ledger::LedgerAccountId, AccountingAmount>,
) -> Result<AccountingAmount, ReportError> {
    let mut total = net_income_of(map, &windows.defs)?;
    for (code, def) in &windows.defs {
        if matches!(
            def.element,
            crate::accounting::ledger::AccountElement::Equity
        ) {
            let value = map.get(code).copied().unwrap_or(AccountingAmount::ZERO);
            total = total.sub(value)?;
        }
    }
    Ok(total)
}

/// 生成所有者权益变动表。
pub(crate) fn generate(windows: &StatementWindows) -> Result<EquityStatement, ReportError> {
    let mut opening_map = std::collections::BTreeMap::new();
    for (code, closing) in &windows.closing {
        let movement = windows
            .movement
            .get(code)
            .copied()
            .unwrap_or(AccountingAmount::ZERO);
        opening_map.insert(code.clone(), closing.sub(movement)?);
    }
    match windows.consolidation.as_ref() {
        None => {
            let mut capital = AccountingAmount::ZERO;
            for (code, def) in &windows.defs {
                if matches!(
                    def.element,
                    crate::accounting::ledger::AccountElement::Equity
                ) {
                    let movement = windows
                        .movement
                        .get(code)
                        .copied()
                        .unwrap_or(AccountingAmount::ZERO);
                    capital = capital.sub(movement)?;
                }
            }
            Ok(EquityStatement {
                opening_parent: equity_rolling(windows, &opening_map)?,
                net_income: net_income_of(&windows.movement, &windows.defs)?,
                other_comprehensive: AccountingAmount::ZERO,
                capital_contributions: capital,
                distributions: AccountingAmount::ZERO,
                closing_parent: equity_rolling(windows, &windows.closing)?,
                opening_minority: None,
                minority_net_income: None,
                closing_minority: None,
            })
        }
        Some(facts) => Ok(EquityStatement {
            opening_parent: facts.equity_to_parent.sub(facts.ni_to_parent)?,
            net_income: facts.ni_to_parent,
            other_comprehensive: AccountingAmount::ZERO,
            capital_contributions: AccountingAmount::ZERO,
            distributions: AccountingAmount::ZERO,
            closing_parent: facts.equity_to_parent,
            opening_minority: Some(facts.minority_equity.sub(facts.minority_ni)?),
            minority_net_income: Some(facts.minority_ni),
            closing_minority: Some(facts.minority_equity),
        }),
    }
}
