//! 资产负债表生成器（K3，任务 13）：由窗口读投影推导主表行。
//!
//! 列示口径（CAS 30（2026）§25/§27/§16 已核验，docs/company-accounting.md §2.1）：
//! 行值 = 该行归类科目净借方（余额/运动）按借贷正常方向折算的正数口径；
//! 备抵科目（1602/1603/1231/1303/1542/进项税）随净额自然冲减所属行。
//! 期初 = 窗口首期前余额；期末 = 报告期末余额；上年年末为类型化比较项
//! （缺历史 `Unavailable(reason)`，绝不填零）。
//!
//! 合并权益列：实收资本 = 根成员 4001；归母留存 = 归母权益 − 根成员实收
//! 资本（固定控制、无并购计量——子公司权益母公司份额并入留存，登记于
//! issues 的游戏简化）；少数股东权益 = 任务 12 拆分。

use std::collections::BTreeMap;

use crate::accounting::amount::AccountingAmount;
use crate::accounting::ledger::LedgerAccountId;

use super::notes::NoteTarget;
use super::window::{net_income_of, StatementWindows};
use super::{Comparative, ReportError, UnavailableReason};

/// 资产负债表列示行。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum BsLine {
    CashFunds,
    Receivables,
    InsuranceReceivables,
    Inventory,
    DevelopmentInventory,
    FixedAssets,
    LoansAndAdvances,
    DeferredTaxAssets,
    ShortTermBorrowings,
    AccountsPayable,
    ContractLiabilities,
    TaxesPayable,
    InterestPayable,
    CustomerDeposits,
    LongTermBorrowings,
    InsuranceContractLiabilities,
    DeferredTaxLiabilities,
    PaidInCapital,
    RetainedEarnings,
    MinorityEquity,
}

impl BsLine {
    /// 列示顺序（资产 → 负债 → 权益）。
    pub const ALL: &'static [BsLine] = &[
        BsLine::CashFunds,
        BsLine::Receivables,
        BsLine::InsuranceReceivables,
        BsLine::Inventory,
        BsLine::DevelopmentInventory,
        BsLine::FixedAssets,
        BsLine::LoansAndAdvances,
        BsLine::DeferredTaxAssets,
        BsLine::ShortTermBorrowings,
        BsLine::AccountsPayable,
        BsLine::ContractLiabilities,
        BsLine::TaxesPayable,
        BsLine::InterestPayable,
        BsLine::CustomerDeposits,
        BsLine::LongTermBorrowings,
        BsLine::InsuranceContractLiabilities,
        BsLine::DeferredTaxLiabilities,
        BsLine::PaidInCapital,
        BsLine::RetainedEarnings,
        BsLine::MinorityEquity,
    ];

    /// 行的借贷正常方向（资产借方正常；负债/权益贷方正常）。
    pub fn credit_positive(&self) -> bool {
        use BsLine::*;
        matches!(
            self,
            ShortTermBorrowings
                | AccountsPayable
                | ContractLiabilities
                | TaxesPayable
                | InterestPayable
                | CustomerDeposits
                | LongTermBorrowings
                | InsuranceContractLiabilities
                | DeferredTaxLiabilities
                | PaidInCapital
                | RetainedEarnings
                | MinorityEquity
        )
    }

    /// 资产区行（合计方向）。
    pub fn is_asset(&self) -> bool {
        use BsLine::*;
        matches!(
            self,
            CashFunds
                | Receivables
                | InsuranceReceivables
                | Inventory
                | DevelopmentInventory
                | FixedAssets
                | LoansAndAdvances
                | DeferredTaxAssets
        )
    }

    /// 权益区行（实收资本 + 派生行）。
    pub fn is_equity(&self) -> bool {
        matches!(
            self,
            BsLine::PaidInCapital | BsLine::RetainedEarnings | BsLine::MinorityEquity
        )
    }

    /// 派生行（无科目明细参与勾稽；由报表恒等式校验）。
    pub fn is_derived(&self) -> bool {
        matches!(self, BsLine::RetainedEarnings | BsLine::MinorityEquity)
    }
}

/// 资产负债表（期末 + 上年年末比较项）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct BalanceSheet {
    pub asset_lines: Vec<(BsLine, AccountingAmount)>,
    pub total_assets: AccountingAmount,
    pub liability_lines: Vec<(BsLine, AccountingAmount)>,
    pub total_liabilities: AccountingAmount,
    pub equity_lines: Vec<(BsLine, AccountingAmount)>,
    pub total_equity: AccountingAmount,
    pub equity_to_parent: AccountingAmount,
    pub liabilities_and_equity: AccountingAmount,
    pub closing_cash: AccountingAmount,
    pub prior_year_end: Comparative<Vec<(BsLine, AccountingAmount)>>,
}

type Classification = BTreeMap<LedgerAccountId, NoteTarget>;

fn has_line(classification: &Classification, line: BsLine) -> bool {
    classification
        .values()
        .any(|target| matches!(target, NoteTarget::BalanceSheet(l) if *l == line))
}

/// 行值 = 归类科目净借方按正常方向折算（备抵自然冲减）。
pub(crate) fn signed_sum(
    map: &BTreeMap<LedgerAccountId, AccountingAmount>,
    classification: &Classification,
    line: BsLine,
) -> Result<AccountingAmount, ReportError> {
    let mut total = AccountingAmount::ZERO;
    for (code, target) in classification {
        if matches!(target, NoteTarget::BalanceSheet(l) if *l == line) {
            let value = map.get(code).copied().unwrap_or(AccountingAmount::ZERO);
            let signed = if line.credit_positive() {
                value.neg()?
            } else {
                value
            };
            total = total.add(signed)?;
        }
    }
    Ok(total)
}

/// 生成资产负债表（期末 + 比较项）。
pub(crate) fn generate(
    windows: &StatementWindows,
    classification: &Classification,
) -> Result<BalanceSheet, ReportError> {
    let facts = windows.consolidation.as_ref();
    let mut asset_lines = Vec::new();
    let mut total_assets = AccountingAmount::ZERO;
    let mut liability_lines = Vec::new();
    let mut total_liabilities = AccountingAmount::ZERO;
    for line in BsLine::ALL {
        if !has_line(classification, *line) {
            continue;
        }
        let value = signed_sum(&windows.closing, classification, *line)?;
        if line.is_asset() {
            total_assets = total_assets.add(value)?;
            asset_lines.push((*line, value));
        } else if !line.is_equity() {
            total_liabilities = total_liabilities.add(value)?;
            liability_lines.push((*line, value));
        }
    }
    // 实收资本：合并口径 = Σ成员 − 非根成员贡献（根成员 4001）。
    let mut paid_in = signed_sum(&windows.closing, classification, BsLine::PaidInCapital)?;
    if let Some(facts) = facts {
        for (code, credit) in &facts.non_root_equity {
            if matches!(
                classification.get(code),
                Some(NoteTarget::BalanceSheet(BsLine::PaidInCapital))
            ) {
                paid_in = paid_in.sub(*credit)?;
            }
        }
    }
    let retained = match facts {
        Some(facts) => facts.equity_to_parent.sub(paid_in)?,
        None => net_income_of(&windows.closing, &windows.defs)?.add(signed_sum(
            &windows.closing,
            classification,
            BsLine::RetainedEarnings,
        )?)?,
    };
    let minority = facts.map(|facts| facts.minority_equity);
    let mut equity_lines = vec![
        (BsLine::PaidInCapital, paid_in),
        (BsLine::RetainedEarnings, retained),
    ];
    let mut total_equity = paid_in.add(retained)?;
    if let Some(minority) = minority {
        equity_lines.push((BsLine::MinorityEquity, minority));
        total_equity = total_equity.add(minority)?;
    }
    let equity_to_parent = match minority {
        Some(minority) => total_equity.sub(minority)?,
        None => total_equity,
    };
    let mut closing_cash = AccountingAmount::ZERO;
    for (code, def) in &windows.defs {
        if def.is_cash {
            closing_cash = closing_cash.add(
                windows
                    .closing
                    .get(code)
                    .copied()
                    .unwrap_or(AccountingAmount::ZERO),
            )?;
        }
    }
    let prior_year_end = match &windows.prior_year_end {
        None => Comparative::Unavailable {
            reason: UnavailableReason::NoPriorYearHistory,
        },
        Some(prior) => Comparative::Available(prior_lines(prior, classification, windows, facts)?),
    };
    Ok(BalanceSheet {
        liabilities_and_equity: total_liabilities.add(total_equity)?,
        total_equity,
        equity_to_parent,
        closing_cash,
        asset_lines,
        total_assets,
        liability_lines,
        total_liabilities,
        equity_lines,
        prior_year_end,
    })
}

/// 上年年末比较项行。
fn prior_lines(
    prior: &BTreeMap<LedgerAccountId, AccountingAmount>,
    classification: &Classification,
    windows: &StatementWindows,
    facts: Option<&super::consolidated_window::ConsolidationFacts>,
) -> Result<Vec<(BsLine, AccountingAmount)>, ReportError> {
    let mut lines = Vec::new();
    for line in BsLine::ALL {
        if !has_line(classification, *line) || line.is_derived() {
            continue;
        }
        lines.push((*line, signed_sum(prior, classification, *line)?));
    }
    let (paid_in, retained, minority) = match facts {
        None => {
            let paid_in = signed_sum(prior, classification, BsLine::PaidInCapital)?;
            let retained = net_income_of(prior, &windows.defs)?.add(signed_sum(
                prior,
                classification,
                BsLine::RetainedEarnings,
            )?)?;
            (paid_in, retained, None)
        }
        Some(facts) => match &facts.prior_split {
            None => return Ok(lines),
            Some(split) => (
                split.root_capital,
                split.parent.sub(split.root_capital)?,
                Some(split.minority),
            ),
        },
    };
    lines.push((BsLine::PaidInCapital, paid_in));
    lines.push((BsLine::RetainedEarnings, retained));
    if let Some(minority) = minority {
        lines.push((BsLine::MinorityEquity, minority));
    }
    Ok(lines)
}
