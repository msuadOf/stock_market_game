use super::*;
use std::collections::BTreeMap;
impl SimpleFinanceState {
    pub(super) fn post_summary(
        &mut self,
        date: CivilDate,
        amounts: &PeriodAmounts,
    ) -> Result<(), SimpleFinanceError> {
        let mut lines = Vec::new();
        use crate::accounting::reports::simple_summary as accounts;
        for (debit, credit, amount) in [
            (accounts::RECEIVABLE, accounts::REVENUE, amounts.revenue),
            (
                accounts::FIXED_EXPENSE,
                accounts::PAYABLE,
                amounts.fixed_expense,
            ),
            (
                accounts::VARIABLE_EXPENSE,
                accounts::PAYABLE,
                amounts.variable_expense,
            ),
        ] {
            if amount.is_positive() {
                lines.push(line(debit, PostingSide::Debit, amount));
                lines.push(line(credit, PostingSide::Credit, amount));
            }
        }
        if !lines.is_empty() {
            let next = self
                .next_event_id
                .checked_add(1)
                .ok_or_else(|| SimpleFinanceError::Invalid("摘要事件序号耗尽".into()))?;
            self.books.post_batch(vec![JournalEntry {
                source: BusinessEventId::new(self.next_event_id),
                date,
                kind: BusinessKind::SimplePeriodSummary,
                cash_flow: CashFlowClass::NonCash,
                lines,
            }])?;
            self.next_event_id = next;
        }
        Ok(())
    }
    pub(super) fn accrue_tax(&mut self, date: CivilDate) -> Result<(), SimpleFinanceError> {
        let preview = crate::company::income_tax::preview_tax_cascade(
            &self.books,
            &self.income_tax_position,
            &self.config.tax_policy.income_tax,
            self.next_event_id,
            date.year(),
            date,
            &BTreeMap::new(),
        )?;
        self.books.post_batch(preview.entries)?;
        self.income_tax_position = preview.position;
        self.next_event_id = preview.next_event_id;
        Ok(())
    }
    pub(super) fn covers(
        &self,
        first: CivilDate,
        last: CivilDate,
    ) -> Result<bool, SimpleFinanceError> {
        let mut expected = first;
        for &(start, end) in &self.recognized_periods {
            if end < first || start > last {
                continue;
            }
            if start != expected || end > last {
                return Ok(false);
            }
            expected = end.next()?;
        }
        Ok(expected == last.next()?)
    }
    pub(super) fn close_generated_period(
        &mut self,
        start: CivilDate,
        end: CivilDate,
    ) -> Result<(), SimpleFinanceError> {
        let mut cursor = start;
        while cursor <= end {
            self.books.close_period(AccountingPeriod::of_date(cursor))?;
            cursor = if cursor.month() == 12 {
                CivilDate::from_ymd(cursor.year() + 1, 1, 1)?
            } else {
                CivilDate::from_ymd(cursor.year(), cursor.month() + 1, 1)?
            };
        }
        let period = AccountingPeriod::of_date(end);
        let scope = ScopeId::Standalone(self.member_id());
        let adjustments = self
            .closing
            .restatement_periods(&scope)
            .cloned()
            .unwrap_or_default();
        for kind in [
            ReportKind::Monthly,
            ReportKind::Quarter,
            ReportKind::HalfYear,
            ReportKind::Annual,
        ] {
            if (kind == ReportKind::Quarter && end.month() % 3 != 0)
                || (kind == ReportKind::HalfYear && end.month() != 6)
                || (kind == ReportKind::Annual && end.month() != 12)
            {
                continue;
            }
            let ((first, _), _) = kind.resolve(period)?;
            let first = CivilDate::from_ymd(first.year(), first.month(), 1)?;
            if !self.covers(first, end)? {
                continue;
            }
            let report = crate::accounting::reports::generate_report_set(
                crate::accounting::reports::ReportRequest {
                    period,
                    kind,
                    source: crate::accounting::reports::ReportSource::Standalone {
                        id: self.member_id(),
                        books: &self.books,
                        industry: self.industry(),
                    },
                    version: crate::accounting::reports::ReportVersion {
                        sequence: 1,
                        supersedes: None,
                        kind: crate::accounting::reports::VersionKind::Original,
                    },
                    adjustments: &adjustments,
                },
            )?;
            self.closing.record(report)?;
        }
        Ok(())
    }
}
