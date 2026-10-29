use super::*;
use crate::accounting::reports::{generate_report_set, ReportRequest, ReportSource, VersionKind};

impl SimpleFinanceState {
    pub(super) fn validate_reports(&self) -> Result<(), SimpleFinanceError> {
        let scope = ScopeId::Standalone(self.member_id());
        let adjustments = self
            .closing
            .restatement_periods(&scope)
            .cloned()
            .unwrap_or_default();
        for &(_, end) in &self.recognized_periods {
            let period = AccountingPeriod::of_date(end);
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
                if self.covers(first, end)?
                    && self.closing.versions(&scope, period, kind).is_empty()
                {
                    return Err(SimpleFinanceError::Invalid(format!(
                        "报告 {period} {kind:?} 已生成但 Closing 缺少登记版本"
                    )));
                }
            }
        }
        for (stored_scope, period, kind, versions) in self.closing.report_versions() {
            if stored_scope != &scope {
                return Err(SimpleFinanceError::Invalid(
                    "Simple 报告不得属于其他 Scope".into(),
                ));
            }
            let ((first, last), _) = kind.resolve(period)?;
            let first_date = CivilDate::from_ymd(first.year(), first.month(), 1)?;
            let last_first = CivilDate::from_ymd(last.year(), last.month(), 1)?;
            let end = if last.month() == 12 {
                CivilDate::from_ymd(last.year() + 1, 1, 1)?.prev()?
            } else {
                CivilDate::from_ymd(last.year(), last.month() + 1, 1)?.prev()?
            };
            if last_first > self.as_of || !self.covers(first_date, end)? {
                return Err(SimpleFinanceError::Invalid(format!(
                    "报告 {period} {kind:?} 超出完整生成期间"
                )));
            }
            for (index, report) in versions.iter().enumerate() {
                report.validate()?;
                let sequence = u32::try_from(index + 1)
                    .map_err(|_| SimpleFinanceError::Invalid("报告版本序号耗尽".into()))?;
                let valid_version = match &report.version.kind {
                    VersionKind::Original => report.version.supersedes.is_none(),
                    VersionKind::Correction { reason } => {
                        sequence > 1
                            && report.version.supersedes == Some(sequence - 1)
                            && !reason.trim().is_empty()
                    }
                };
                if report.scope != scope
                    || report.period != period
                    || report.kind != kind
                    || report.version.sequence != sequence
                    || !valid_version
                {
                    return Err(SimpleFinanceError::Invalid(format!(
                        "报告 {period} {kind:?} 的键或版本链不一致"
                    )));
                }
            }
            let latest = versions
                .last()
                .ok_or_else(|| SimpleFinanceError::Invalid("已登记报告缺少版本".into()))?;
            let rebuilt = generate_report_set(ReportRequest {
                period,
                kind,
                source: ReportSource::Standalone {
                    id: self.member_id(),
                    books: &self.books,
                    industry: self.industry(),
                },
                version: latest.version.clone(),
                adjustments: &adjustments,
            })?;
            if latest != &rebuilt {
                return Err(SimpleFinanceError::Invalid(format!(
                    "报告 {period} {kind:?} 与权威 Books 及重述底稿不一致"
                )));
            }
        }
        Ok(())
    }
}
