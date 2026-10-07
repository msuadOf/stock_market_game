//! 资产负债表生成器：由窗口读投影推导主表行。
//!
//! 列示口径（CAS 30（2026）§25/§27/§16 已核验，docs/company-accounting.md §2.1）：
//! 行值 = 该行归类科目净借方（余额/运动）按借贷正常方向折算的正数口径；
//! 备抵科目（1602/1603/1231/1303/1542/进项税）随净额自然冲减所属行。
//! 期初 = 窗口首期前余额；期末 = 报告期末余额；上年年末为类型化比较项
//! （缺历史 `Unavailable(reason)`，绝不填零）。
//!
//! 合并权益列：实收资本 = 根成员 4001；归母留存 = 归母权益 − 根成员实收
//! 资本（固定控制、无并购计量——子公司权益母公司份额并入留存，登记于
//! issues 的游戏简化）；少数股东权益由固定集团合并拆分。

use std::collections::BTreeMap;

use crate::accounting::amount::AccountingAmount;
use crate::accounting::ledger::LedgerAccountId;

use super::notes::{NoteTarget, ReportClassification};
use super::window::{net_income_of, StatementWindows};
use super::{Comparative, ReportError, UnavailableReason};

/// 资产负债表列示行。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum BsLine {
    CashFunds,
    Receivables,
    SimpleDividendSettlementAdjustment,
    SimpleIssuerFundingAdjustment,
    InsuranceReceivables,
    Inventory,
    DevelopmentInventory,
    FixedAssets,
    LoansAndAdvances,
    DeferredTaxAssets,
    CurrentTaxAssets,
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
    CapitalReserve,
    StatutoryReserve,
    RetainedEarnings,
    MinorityEquity,
}

impl BsLine {
    /// 列示顺序（资产 → 负债 → 权益）。
    pub const ALL: &'static [BsLine] = &[
        BsLine::CashFunds,
        BsLine::Receivables,
        BsLine::SimpleDividendSettlementAdjustment,
        BsLine::SimpleIssuerFundingAdjustment,
        BsLine::InsuranceReceivables,
        BsLine::Inventory,
        BsLine::DevelopmentInventory,
        BsLine::FixedAssets,
        BsLine::LoansAndAdvances,
        BsLine::DeferredTaxAssets,
        BsLine::CurrentTaxAssets,
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
        BsLine::CapitalReserve,
        BsLine::StatutoryReserve,
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
                | CapitalReserve
                | StatutoryReserve
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
                | SimpleDividendSettlementAdjustment
                | SimpleIssuerFundingAdjustment
                | InsuranceReceivables
                | Inventory
                | DevelopmentInventory
                | FixedAssets
                | LoansAndAdvances
                | DeferredTaxAssets
                | CurrentTaxAssets
        )
    }

    /// 权益区行（实收资本 + 派生行）。
    pub fn is_equity(&self) -> bool {
        matches!(
            self,
            BsLine::PaidInCapital
                | BsLine::CapitalReserve
                | BsLine::StatutoryReserve
                | BsLine::RetainedEarnings
                | BsLine::MinorityEquity
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

fn has_line(
    classification: &ReportClassification,
    line: BsLine,
    map: &BTreeMap<LedgerAccountId, AccountingAmount>,
) -> bool {
    classification
        .iter()
        .any(|(code, target)| matches!(effective_target(code, target, map.get(code).copied().unwrap_or(AccountingAmount::ZERO)), NoteTarget::BalanceSheet(candidate) if candidate == line))
}

pub(crate) fn effective_target(
    code: &LedgerAccountId,
    target: &NoteTarget,
    value: AccountingAmount,
) -> NoteTarget {
    if super::consolidated_window::is_current_tax_key(code)
        && value.is_positive()
        && *target == NoteTarget::BalanceSheet(BsLine::TaxesPayable)
    {
        NoteTarget::BalanceSheet(BsLine::CurrentTaxAssets)
    } else {
        target.clone()
    }
}

/// 行值 = 归类科目净借方按正常方向折算（备抵自然冲减）。
pub(crate) fn signed_sum(
    map: &BTreeMap<LedgerAccountId, AccountingAmount>,
    classification: &ReportClassification,
    line: BsLine,
) -> Result<AccountingAmount, ReportError> {
    let mut total = AccountingAmount::ZERO;
    for (code, target) in classification.iter() {
        let value = map.get(code).copied().unwrap_or(AccountingAmount::ZERO);
        if matches!(effective_target(code, target, value), NoteTarget::BalanceSheet(candidate) if candidate == line)
        {
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
    classification: &ReportClassification,
) -> Result<BalanceSheet, ReportError> {
    let facts = windows.consolidation.as_ref();
    let mut asset_lines = Vec::new();
    let mut total_assets = AccountingAmount::ZERO;
    let mut liability_lines = Vec::new();
    let mut total_liabilities = AccountingAmount::ZERO;
    for line in BsLine::ALL {
        if !has_line(classification, *line, &windows.closing) {
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
    let equity = EquityPresentation::from_closing(windows, classification)?;
    let equity_lines = equity.lines();
    let total_equity = equity.total_equity()?;
    let equity_to_parent = equity.equity_to_parent(total_equity)?;
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
///
/// 权益区行统一由 [`EquityPresentation::from_prior`] 的追加段提供（实收资本、
/// 资本公积、法定公积金、未分配利润、少数股东权益），主循环跳过全部权益行——
/// 否则法定公积金等行会被主循环与追加段各列一次（2026-10-08 N3 批修复的
/// 既有重复列示）。
fn prior_lines(
    prior: &BTreeMap<LedgerAccountId, AccountingAmount>,
    classification: &ReportClassification,
    windows: &StatementWindows,
    facts: Option<&super::consolidated_window::ConsolidationFacts>,
) -> Result<Vec<(BsLine, AccountingAmount)>, ReportError> {
    let mut lines = Vec::new();
    for line in BsLine::ALL {
        if !has_line(classification, *line, prior) || line.is_equity() {
            continue;
        }
        lines.push((*line, signed_sum(prior, classification, *line)?));
    }
    if let Some(equity) = EquityPresentation::from_prior(prior, classification, windows, facts)? {
        lines.extend(equity.lines());
    }
    Ok(lines)
}

/// 本次列报的权益行值；比较期路径不额外计算原先未校验的总额。
struct EquityPresentation {
    paid_in: AccountingAmount,
    capital_reserve: AccountingAmount,
    /// 科目表是否实际归类了资本公积科目（仅 Simple 账套；四行业基础科目表无
    /// 该科目时不列零值行，既有列报不变）。
    show_capital_reserve: bool,
    statutory_reserve: AccountingAmount,
    retained: AccountingAmount,
    minority: Option<AccountingAmount>,
}

impl EquityPresentation {
    fn from_closing(
        windows: &StatementWindows,
        classification: &ReportClassification,
    ) -> Result<Self, ReportError> {
        let facts = windows.consolidation.as_ref();
        // 实收资本：合并口径 = Σ成员 − 非根成员贡献（根成员 4001）。
        let mut paid_in = signed_sum(&windows.closing, classification, BsLine::PaidInCapital)?;
        let capital_reserve =
            signed_sum(&windows.closing, classification, BsLine::CapitalReserve)?;
        let show_capital_reserve =
            has_line(classification, BsLine::CapitalReserve, &windows.closing);
        let statutory_reserve = signed_sum(&windows.closing, classification, BsLine::StatutoryReserve)?;
        if let Some(facts) = facts {
            for (code, credit) in &facts.non_root_equity {
                if matches!(
                    classification.target_for(code),
                    Some(NoteTarget::BalanceSheet(BsLine::PaidInCapital))
                ) {
                    paid_in = paid_in.sub(*credit)?;
                }
            }
        }
        let retained = match facts {
            Some(facts) => facts
                .equity_to_parent
                .sub(paid_in)?
                .sub(capital_reserve)?
                .sub(statutory_reserve)?,
            None => net_income_of(&windows.closing, &windows.defs)?.add(signed_sum(
                &windows.closing,
                classification,
                BsLine::RetainedEarnings,
            )?)?,
        };
        let minority = facts.map(|facts| facts.minority_equity);
        Ok(Self {
            paid_in,
            capital_reserve,
            show_capital_reserve,
            statutory_reserve,
            retained,
            minority,
        })
    }

    fn from_prior(
        prior: &BTreeMap<LedgerAccountId, AccountingAmount>,
        classification: &ReportClassification,
        windows: &StatementWindows,
        facts: Option<&super::consolidated_window::ConsolidationFacts>,
    ) -> Result<Option<Self>, ReportError> {
        let show_capital_reserve = has_line(classification, BsLine::CapitalReserve, prior);
        let (paid_in, capital_reserve, statutory_reserve, retained, minority) = match facts {
            None => {
                let paid_in = signed_sum(prior, classification, BsLine::PaidInCapital)?;
                let capital_reserve = signed_sum(prior, classification, BsLine::CapitalReserve)?;
                let statutory_reserve = signed_sum(prior, classification, BsLine::StatutoryReserve)?;
                let retained = net_income_of(prior, &windows.defs)?.add(signed_sum(
                    prior,
                    classification,
                    BsLine::RetainedEarnings,
                )?)?;
                (paid_in, capital_reserve, statutory_reserve, retained, None)
            }
            Some(facts) => match &facts.prior_split {
                None => return Ok(None),
                Some(split) => {
                    let capital_reserve = signed_sum(prior, classification, BsLine::CapitalReserve)?;
                    let statutory_reserve = signed_sum(prior, classification, BsLine::StatutoryReserve)?;
                    (split.root_capital, capital_reserve, statutory_reserve,
                        split.parent.sub(split.root_capital)?.sub(capital_reserve)?.sub(statutory_reserve)?, Some(split.minority))
                }
            },
        };
        Ok(Some(Self {
            paid_in,
            capital_reserve,
            show_capital_reserve,
            statutory_reserve,
            retained,
            minority,
        }))
    }

    fn lines(&self) -> Vec<(BsLine, AccountingAmount)> {
        let mut lines = vec![(BsLine::PaidInCapital, self.paid_in)];
        if self.show_capital_reserve {
            lines.push((BsLine::CapitalReserve, self.capital_reserve));
        }
        lines.push((BsLine::StatutoryReserve, self.statutory_reserve));
        lines.push((BsLine::RetainedEarnings, self.retained));
        if let Some(minority) = self.minority {
            lines.push((BsLine::MinorityEquity, minority));
        }
        lines
    }

    fn total_equity(&self) -> Result<AccountingAmount, ReportError> {
        let mut total = self
            .paid_in
            .add(self.capital_reserve)?
            .add(self.statutory_reserve)?
            .add(self.retained)?;
        if let Some(minority) = self.minority {
            total = total.add(minority)?;
        }
        Ok(total)
    }

    fn equity_to_parent(&self, total: AccountingAmount) -> Result<AccountingAmount, ReportError> {
        Ok(match self.minority {
            Some(minority) => total.sub(minority)?,
            None => total,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::consolidated_window::{ConsolidationFacts, PriorSplit};
    use super::super::window::Accumulator;
    use super::super::IndustryPresentation;
    use super::*;
    use crate::accounting::consolidation::MemberId;
    use crate::accounting::ledger::AccountChart;
    use crate::accounting::period::AccountingPeriod;

    fn windows() -> StatementWindows {
        let current = AccountingPeriod::from_ymd(2030, 1).unwrap();
        let prior = AccountingPeriod::from_ymd(2029, 1).unwrap();
        let defs = AccountChart::generic_account_chart()
            .iter()
            .map(|(code, def)| (code.clone(), def.clone()))
            .collect();
        Accumulator::new((current, current), (prior, prior))
            .unwrap()
            .finish(defs, None)
    }

    fn cents(value: i128) -> AccountingAmount {
        AccountingAmount::from_cents(value)
    }

    fn facts() -> ConsolidationFacts {
        ConsolidationFacts {
            root: MemberId("root".into()),
            minority_equity: cents(5),
            equity_to_parent: cents(30),
            minority_ni: cents(0),
            ni_to_parent: cents(0),
            consolidated_ni: cents(0),
            window_ni_to_parent: cents(0),
            window_minority_ni: cents(0),
            non_root_equity: BTreeMap::from([(LedgerAccountId("4001".into()), cents(40))]),
            prior_split: None,
        }
    }

    #[test]
    fn equity_presentation_keeps_standalone_negative_retained() {
        let mut windows = windows();
        windows.closing = BTreeMap::from([
            (LedgerAccountId("4001".into()), cents(-10)),
            (LedgerAccountId("4103".into()), cents(2)),
            (LedgerAccountId("6602".into()), cents(5)),
        ]);
        let classification = ReportClassification::from_industries(
            &windows.defs,
            &[IndustryPresentation::Industrial],
        )
        .unwrap();
        let sheet = generate(&windows, &classification).unwrap();
        assert_eq!(
            sheet.equity_lines,
            vec![
                (BsLine::PaidInCapital, cents(10)),
                (BsLine::RetainedEarnings, cents(-7))
            ]
        );
        assert_eq!(sheet.total_equity, cents(3));
        let prior = prior_lines(&windows.closing, &classification, &windows, None).unwrap();
        assert!(prior.contains(&(BsLine::RetainedEarnings, cents(-7))));
    }

    #[test]
    fn equity_presentation_keeps_consolidated_split_and_missing_prior_equity() {
        let mut windows = windows();
        windows
            .closing
            .insert(LedgerAccountId("4001".into()), cents(-50));
        windows.consolidation = Some(facts());
        let classification = ReportClassification::from_industries(
            &windows.defs,
            &[IndustryPresentation::Industrial],
        )
        .unwrap();
        let sheet = generate(&windows, &classification).unwrap();
        assert_eq!(
            sheet.equity_lines,
            vec![
                (BsLine::PaidInCapital, cents(10)),
                (BsLine::RetainedEarnings, cents(20)),
                (BsLine::MinorityEquity, cents(5))
            ]
        );
        assert_eq!(sheet.total_equity, cents(35));
        assert_eq!(sheet.equity_to_parent, cents(30));
        let prior = BTreeMap::from([(LedgerAccountId("1001".into()), cents(7))]);
        let lines = prior_lines(
            &prior,
            &classification,
            &windows,
            windows.consolidation.as_ref(),
        )
        .unwrap();
        assert!(lines.contains(&(BsLine::CashFunds, cents(7))));
        assert!(!lines.iter().any(|(line, _)| line.is_equity()));
        windows.consolidation.as_mut().unwrap().prior_split = Some(PriorSplit {
            minority: cents(4),
            parent: cents(12),
            root_capital: cents(10),
        });
        let lines = prior_lines(
            &prior,
            &classification,
            &windows,
            windows.consolidation.as_ref(),
        )
        .unwrap();
        assert!(lines.contains(&(BsLine::RetainedEarnings, cents(2))));
        assert!(lines.contains(&(BsLine::MinorityEquity, cents(4))));
    }
}
