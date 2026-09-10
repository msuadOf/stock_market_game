//! 现金流量表生成器（K3，任务 13）：直接法三分类（CAS 31 现行口径，存在性
//! 经 CAS 30 第三条核验）+ 间接法调节（净利润 → 经营活动现金）。
//!
//! **间接法恒等式（无 plug，逐行由总账运动推导）**：
//! `经营现金 = 净利润 + Σ(非现金非损益科目 × −运动) − 投资现金 − 筹资现金`
//! ——由复式簿记逐笔平衡 + 分录现金流分类代数推出（模块文档钉死）。
//! 重述版本的调整分录现金属实际收付期间，经显式「重述现金调整」行配平，
//! **绝不双计现金**。现金期初/期末 = 实际期间口径（勾稽基准）。

use crate::accounting::amount::AccountingAmount;
use crate::accounting::ledger::AccountElement;

use super::window::net_income_of;
use super::window::StatementWindows;
use super::ReportError;

/// 间接法调节行。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct IndirectLine {
    pub label: String,
    pub amount: AccountingAmount,
}

/// 现金流量表（直接法三分类 + 间接法调节）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CashFlowStatement {
    pub operating: AccountingAmount,
    pub investing: AccountingAmount,
    pub financing: AccountingAmount,
    pub net_change: AccountingAmount,
    pub opening_cash: AccountingAmount,
    pub closing_cash: AccountingAmount,
    pub indirect: Vec<IndirectLine>,
}

/// 生成现金流量表（窗口实际期间口径）。
pub(crate) fn generate(windows: &StatementWindows) -> Result<CashFlowStatement, ReportError> {
    let operating = windows.cash.operating;
    let investing = windows.cash.investing;
    let financing = windows.cash.financing;
    let net_change = operating.add(investing)?.add(financing)?;
    let closing_cash = windows.cash_closing_actual;
    let opening_cash = closing_cash.sub(net_change)?;

    let mut indirect = vec![IndirectLine {
        label: "净利润".to_string(),
        amount: net_income_of(&windows.movement, &windows.defs)?,
    }];
    // 非现金非损益科目的窗口运动（代码序确定；零运动不产生行）。
    for (code, def) in &windows.defs {
        if def.is_cash {
            continue;
        }
        if matches!(
            def.element,
            AccountElement::Revenue | AccountElement::Expense
        ) {
            continue;
        }
        let movement = windows
            .movement
            .get(code)
            .copied()
            .unwrap_or(AccountingAmount::ZERO);
        if movement.is_zero() {
            continue;
        }
        indirect.push(IndirectLine {
            label: format!("{}变动", def.name),
            amount: movement.neg()?,
        });
    }
    if !windows.restated_cash_correction.is_zero() {
        indirect.push(IndirectLine {
            label: "重述调整对应现金流量（按实际收付期间列报）".to_string(),
            amount: windows.restated_cash_correction,
        });
    }
    indirect.push(IndirectLine {
        label: "投资活动现金流量净额".to_string(),
        amount: investing.neg()?,
    });
    indirect.push(IndirectLine {
        label: "筹资活动现金流量净额".to_string(),
        amount: financing.neg()?,
    });
    Ok(CashFlowStatement {
        operating,
        investing,
        financing,
        net_change,
        opening_cash,
        closing_cash,
        indirect,
    })
}
