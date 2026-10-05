//! 五产物勾稽校验：`ReportSet::validate` —— 公布前置守卫。
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
        if p.year() == prior_year
            || (entry.kind == crate::accounting::BusinessKind::OpeningBalance
                && p.year() < prior_year)
        {
            flows = true;
        }
        if p.year() <= prior_year {
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
    pub(crate) fn validate_parent_income_source(&self) -> Result<(), ReportError> {
        if matches!(self.scope, ScopeId::Consolidated(_))
            && self.income.net_income_to_parent.is_none()
        {
            return Err(ReportError::MissingParentIncome {
                scope: self.scope.clone(),
                period: self.period,
            });
        }
        Ok(())
    }

    /// 结构与勾稽校验（生成器产物必须通过；手工构造物同规则拒绝）。
    pub fn validate(&self) -> Result<(), ReportError> {
        self.validate_parent_income_source()?;
        let consolidated = matches!(self.scope, ScopeId::Consolidated(_));
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

#[cfg(test)]
mod prior_year_coverage_tests {
    use super::*;
    use crate::accounting::consolidation::MemberId;
    use crate::accounting::reports::{
        generate_report_set, IndustryPresentation, ReportKind, ReportRequest, ReportSource,
        ReportVersion, VersionKind,
    };
    use crate::accounting::{
        AccountingPeriod, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, JournalLine,
        LedgerAccountId, PostingSide,
    };
    use crate::calendar::CivilDate;
    use std::collections::BTreeMap;

    fn books(opened_on: &str) -> Books {
        let mut books = Books::new(crate::company::industrial::industrial_account_chart());
        books
            .post_batch(vec![JournalEntry {
                source: BusinessEventId::new(1),
                date: CivilDate::from_iso(opened_on).unwrap(),
                kind: BusinessKind::OpeningBalance,
                cash_flow: CashFlowClass::Financing,
                lines: vec![
                    JournalLine {
                        account: LedgerAccountId("1002".into()),
                        side: PostingSide::Debit,
                        amount: AccountingAmount::from_cents(10000),
                    },
                    JournalLine {
                        account: LedgerAccountId("4001".into()),
                        side: PostingSide::Credit,
                        amount: AccountingAmount::from_cents(10000),
                    },
                ],
            }])
            .unwrap();
        books
    }

    fn add_late_expense(books: &mut Books) -> BTreeMap<BusinessEventId, AccountingPeriod> {
        let source = BusinessEventId::new(2);
        books
            .post_batch(vec![JournalEntry {
                source,
                date: CivilDate::from_iso("2031-05-01").unwrap(),
                kind: BusinessKind::CashExpense,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    JournalLine {
                        account: LedgerAccountId("6602".into()),
                        side: PostingSide::Debit,
                        amount: AccountingAmount::from_cents(400),
                    },
                    JournalLine {
                        account: LedgerAccountId("1002".into()),
                        side: PostingSide::Credit,
                        amount: AccountingAmount::from_cents(400),
                    },
                ],
            }])
            .unwrap();
        BTreeMap::from([(source, AccountingPeriod::from_ymd(2030, 12).unwrap())])
    }

    #[test]
    fn january_opening_without_december_entries_still_has_real_prior_year_end_balance() {
        let mut books = books("2030-01-15");
        let member = MemberId("PARTIAL-FIRST-YEAR".into());
        let mut closing = crate::accounting::closing::ClosingEngine::new();
        let (_, handle) = closing
            .close_year(&mut books, &member, IndustryPresentation::Industrial, 2031)
            .unwrap();
        let report = closing
            .version(
                &ScopeId::Standalone(member),
                handle.period,
                ReportKind::Annual,
                handle.sequence,
            )
            .unwrap();
        assert!(matches!(
            report.balance_sheet.prior_year_end,
            Comparative::Available(_)
        ));
        let facts = prior_year_facts(&books, handle.period);
        assert_eq!(facts, (true, true));
    }

    #[test]
    fn earlier_real_opening_keeps_empty_year_coverage_when_late_expense_restates_amounts() {
        let mut books = books("2029-12-31");
        let adjustments = add_late_expense(&mut books);
        let period = AccountingPeriod::from_ymd(2031, 12).unwrap();
        let report = generate_report_set(ReportRequest {
            period,
            kind: ReportKind::Annual,
            source: ReportSource::Standalone {
                id: MemberId("REAL-CONTINUOUS-BOOKS".into()),
                books: &books,
                industry: IndustryPresentation::Industrial,
            },
            version: ReportVersion {
                sequence: 1,
                supersedes: None,
                kind: VersionKind::Original,
            },
            adjustments: &adjustments,
        })
        .unwrap();
        let (flows, end) = prior_year_facts(&books, period);
        assert_eq!((flows, end), (true, true));
        verify_comparative_honesty(&report, flows, end).unwrap();
        assert!(matches!(
            report.income.prior_year,
            Comparative::Available(_)
        ));
        assert_eq!(
            books
                .journal()
                .posted_date(BusinessEventId::new(2))
                .unwrap(),
            CivilDate::from_iso("2031-05-01").unwrap()
        );
        assert_eq!(
            books.ledger().cash_total().unwrap(),
            AccountingAmount::from_cents(9600)
        );
    }

    #[test]
    fn mapping_current_year_entries_to_prior_year_cannot_manufacture_history_coverage() {
        let mut books = books("2031-01-15");
        let adjustments = add_late_expense(&mut books);
        assert_eq!(
            adjustments[&BusinessEventId::new(2)],
            AccountingPeriod::from_ymd(2030, 12).unwrap()
        );
        let period = AccountingPeriod::from_ymd(2031, 12).unwrap();
        assert_eq!(prior_year_facts(&books, period), (false, false));
        assert!(books
            .journal()
            .entries()
            .all(|entry| entry.date.year() == 2031));
        assert_eq!(
            books.ledger().cash_total().unwrap(),
            AccountingAmount::from_cents(9600)
        );
    }
}
