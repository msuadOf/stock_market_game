use super::*;
use std::collections::BTreeMap;
impl SimpleFinanceState {
    /// 公司行为汇总分录入口（N3 批 2026-10-08）：行为财务事实过账进权威
    /// `Books`，下期报表自然反映。分录是一次性事实：
    /// - 日期必须晚于最后结算日（`as_of`），禁止向已封账期间回写——已登记
    ///   报告按原窗口冻结，行为分录落在封账期间会破坏「报告与权威账簿
    ///   一致」不变量，因此在过账前显式拒绝；
    /// - 全部为 `NonCash`（Simple 不追踪真实公司资金链，账面/真实分离）；
    /// - 消费并推进 `next_event_id`，与既有摘要/分红分录共享同一来源序列。
    pub(super) fn post_capital_action(
        &mut self,
        date: CivilDate,
        kind: BusinessKind,
        lines: Vec<JournalLine>,
    ) -> Result<(), SimpleFinanceError> {
        if lines.is_empty() {
            return Ok(());
        }
        if date <= self.as_of {
            return Err(SimpleFinanceError::Invalid(
                "公司行为分录日期必须晚于最后结算日，不得向已封账期间回写".into(),
            ));
        }
        let next = self
            .next_event_id
            .checked_add(1)
            .ok_or_else(|| SimpleFinanceError::Invalid("行为事件序号耗尽".into()))?;
        self.books.post_batch(vec![JournalEntry {
            source: BusinessEventId::new(self.next_event_id),
            date,
            kind,
            cash_flow: CashFlowClass::NonCash,
            lines,
        }])?;
        self.next_event_id = next;
        Ok(())
    }
    /// 逐项结算分录（2026-10-10 分录级派生批）：收入／固定费用／变动费用
    /// 各生成一笔独立 `BusinessKind` 的两行分录（同一批次原子提交、来源
    /// 连续递增），税前利润、净利润、权益、ROE 与账面展示字段全部由这些
    /// 分录事实经唯一账簿派生。零金额项目不落零金额凭证。
    pub(super) fn post_summary(
        &mut self,
        date: CivilDate,
        amounts: &PeriodAmounts,
    ) -> Result<(), SimpleFinanceError> {
        use crate::accounting::reports::simple_summary as accounts;
        let items: [(BusinessKind, &str, &str, AccountingAmount); 3] = [
            (
                BusinessKind::SimpleRevenueAccrual,
                accounts::RECEIVABLE,
                accounts::REVENUE,
                amounts.revenue,
            ),
            (
                BusinessKind::SimpleFixedExpenseAccrual,
                accounts::FIXED_EXPENSE,
                accounts::PAYABLE,
                amounts.fixed_expense,
            ),
            (
                BusinessKind::SimpleVariableExpenseAccrual,
                accounts::VARIABLE_EXPENSE,
                accounts::PAYABLE,
                amounts.variable_expense,
            ),
        ];
        let mut entries = Vec::new();
        let mut next_id = self.next_event_id;
        for (kind, debit, credit, amount) in items {
            if !amount.is_positive() {
                continue;
            }
            let source = BusinessEventId::new(next_id);
            next_id = next_id
                .checked_add(1)
                .ok_or_else(|| SimpleFinanceError::Invalid("结算分录事件序号耗尽".into()))?;
            entries.push(JournalEntry {
                source,
                date,
                kind,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    line(debit, PostingSide::Debit, amount),
                    line(credit, PostingSide::Credit, amount),
                ],
            });
        }
        if !entries.is_empty() {
            self.books.post_batch(entries)?;
            self.next_event_id = next_id;
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
