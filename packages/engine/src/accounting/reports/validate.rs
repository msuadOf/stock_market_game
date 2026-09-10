//! 五产物勾稽校验（K3，任务 13）：`ReportSet::validate` —— 公布前置守卫。
//!
//! 校验项（验收红线，任一失败 = 类型化拒绝）：
//! 1. 资产负债表恒等式：资产 = 负债 + 权益；
//! 2. 权益变动表勾稽：期初 + 净利 + OCI + 投入 − 分配 = 期末（归母/少数）；
//! 3. 现金流量表勾稽：期初 + 三类净额 = 期末；间接法合计 = 经营现金；
//! 4. 附注勾稽：明细合计 = 主表行（BS 行期末口径；利润表行累计口径）。
//!    派生行（未分配利润/少数股东权益）与计算行由恒等式覆盖，不参与
//!    明细勾稽；合并 Scope 的实收资本行取根成员口径，跳过明细勾稽
//!    （非根成员权益在合并拆分披露中单独列示）。

use crate::accounting::amount::AccountingAmount;
use crate::accounting::Books;

use super::balance_sheet::BsLine;
use super::income::IncomeLine;
use super::notes::{NoteTarget, Notes};
use super::{Comparative, ReportError, ReportSet, ScopeId};

/// 附注勾稽辅助：附注明细按资产负债表行折算的合计（期末口径）。
pub(crate) fn bs_notes_total(notes: &Notes, line: BsLine) -> Result<AccountingAmount, ReportError> {
    let mut total = AccountingAmount::ZERO;
    for item in &notes.items {
        if matches!(item.target, NoteTarget::BalanceSheet(l) if l == line) {
            let signed = if line.credit_positive() {
                item.closing.neg()?
            } else {
                item.closing
            };
            total = total.add(signed)?;
        }
    }
    Ok(total)
}

/// 附注勾稽辅助：附注明细按利润表行折算的合计（累计运动口径）。
pub(crate) fn income_notes_total(
    notes: &Notes,
    line: IncomeLine,
) -> Result<AccountingAmount, ReportError> {
    let mut total = AccountingAmount::ZERO;
    for item in &notes.items {
        if matches!(item.target, NoteTarget::Income(l) if l == line) {
            let signed = if line.credit_positive() {
                item.ytd_movement.neg()?
            } else {
                item.ytd_movement
            };
            total = total.add(signed)?;
        }
    }
    Ok(total)
}

/// 账套是否含上年流量 / 上年年末余额（比较项诚实性基准；宽松口径：任一
/// 上年分录即视为有流量——生成器按比较窗口精确判定 Unavailable）。
pub(crate) fn prior_year_facts(
    books: &Books,
    period: crate::accounting::AccountingPeriod,
) -> (bool, bool) {
    let prior_year = period.year() - 1;
    let mut flows = false;
    let mut end = false;
    for entry in books.journal().entries() {
        let p = entry.period();
        if p.year() == prior_year {
            flows = true;
        }
        if p.year() < prior_year || (p.year() == prior_year && p.month() == 12) {
            end = true;
        }
        if flows && end {
            return (true, true);
        }
    }
    (flows, end)
}

/// 比较项诚实性：缺历史却呈 `Available`（捏造 0/值）→ 类型化拒绝；
/// 合法 `Unavailable(reason)` 可公布（缺原因在类型层不可表示）。
pub fn verify_comparative_honesty(
    set: &ReportSet,
    has_prior_year_flows: bool,
    has_prior_year_end: bool,
) -> Result<(), ReportError> {
    if !has_prior_year_flows && matches!(set.income.prior_year, Comparative::Available(_)) {
        return Err(ReportError::ComparativeFabricated {
            location: "income.prior_year",
        });
    }
    if !has_prior_year_end && matches!(set.balance_sheet.prior_year_end, Comparative::Available(_))
    {
        return Err(ReportError::ComparativeFabricated {
            location: "balance_sheet.prior_year_end",
        });
    }
    Ok(())
}

fn equity_cross_foot(
    opening: AccountingAmount,
    changes: AccountingAmount,
    closing: AccountingAmount,
) -> Result<(), ReportError> {
    if opening.add(changes)? != closing {
        return Err(ReportError::EquityCrossFootMismatch {
            opening_plus_changes: opening.add(changes)?,
            closing,
        });
    }
    Ok(())
}

impl ReportSet {
    /// 结构与勾稽校验（生成器产物必须通过；手工构造物同规则拒绝）。
    pub fn validate(&self) -> Result<(), ReportError> {
        let bs = &self.balance_sheet;
        if bs.total_assets != bs.liabilities_and_equity {
            return Err(ReportError::BalanceSheetNotBalanced {
                assets: bs.total_assets,
                liabilities_equity: bs.liabilities_and_equity,
            });
        }
        let eq = &self.equity;
        let parent_changes = eq
            .net_income
            .add(eq.other_comprehensive)?
            .add(eq.capital_contributions)?
            .sub(eq.distributions)?;
        equity_cross_foot(eq.opening_parent, parent_changes, eq.closing_parent)?;
        if let (Some(opening), Some(ni), Some(closing)) = (
            eq.opening_minority,
            eq.minority_net_income,
            eq.closing_minority,
        ) {
            equity_cross_foot(opening, ni, closing)?;
        }
        let cf = &self.cash_flow;
        let opening_plus = cf
            .opening_cash
            .add(cf.operating)?
            .add(cf.investing)?
            .add(cf.financing)?;
        if opening_plus != cf.closing_cash {
            return Err(ReportError::CashFlowCrossFootMismatch {
                opening_plus_changes: opening_plus,
                closing: cf.closing_cash,
            });
        }
        let mut indirect_total = AccountingAmount::ZERO;
        for line in &cf.indirect {
            indirect_total = indirect_total.add(line.amount)?;
        }
        if indirect_total != cf.operating {
            return Err(ReportError::IndirectReconciliationMismatch {
                direct: cf.operating,
                indirect: indirect_total,
            });
        }
        let consolidated = matches!(self.scope, ScopeId::Consolidated(_));
        let check_bs_line = |line: BsLine, value: AccountingAmount| -> Result<(), ReportError> {
            if line.is_derived() || (consolidated && line == BsLine::PaidInCapital) {
                return Ok(());
            }
            let total = bs_notes_total(&self.notes, line)?;
            if total != value {
                return Err(ReportError::NotesCrossFootMismatch {
                    line: line.label(),
                    statement_total: value,
                    notes_total: total,
                });
            }
            Ok(())
        };
        for (line, value) in bs
            .asset_lines
            .iter()
            .chain(bs.liability_lines.iter())
            .chain(bs.equity_lines.iter())
        {
            check_bs_line(*line, *value)?;
        }
        for line in IncomeLine::ALL {
            if line.is_computed() {
                continue;
            }
            let Some(value) = self.income.cumulative.line_amount(*line) else {
                continue;
            };
            let total = income_notes_total(&self.notes, *line)?;
            if total != value {
                return Err(ReportError::NotesCrossFootMismatch {
                    line: line.label(),
                    statement_total: value,
                    notes_total: total,
                });
            }
        }
        Ok(())
    }
}
