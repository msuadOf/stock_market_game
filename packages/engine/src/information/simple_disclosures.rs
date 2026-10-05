use super::{InformationError, PublicLibrary, PublicationId, PublicationRequest, ReportFrequency, ScheduledReportKind};
use crate::calendar::CivilInstant;
use crate::company::CompanySystem;

pub fn publish_simple_scheduled(
    system: &CompanySystem,
    frequency: ReportFrequency,
    seed: u64,
    after: Option<CivilInstant>,
    through: CivilInstant,
    library: &mut PublicLibrary,
    seeded: bool,
) -> Result<Vec<PublicationId>, InformationError> {
    if after.is_some_and(|after| after > through) {
        return Err(InformationError::InconsistentLibrary { detail: "Simple 披露区间发生时间逆序".into() });
    }
    let system_error = |error: crate::company::CompanySystemError| InformationError::InconsistentLibrary {
        detail: format!("Simple 公开材料查询失败：{error}"),
    };
    let mut pending = Vec::new();
    for (company, _) in system.issuers().iter() {
        for report in system.closed_reports(company).map_err(system_error)? {
            let scheduled_kind = match (report.kind, report.period.month()) {
                (crate::accounting::reports::ReportKind::Annual, 12) => ScheduledReportKind::Annual,
                (crate::accounting::reports::ReportKind::HalfYear, 6) => ScheduledReportKind::HalfYear,
                (crate::accounting::reports::ReportKind::Quarter, 3) => ScheduledReportKind::Q1,
                (crate::accounting::reports::ReportKind::Quarter, 9) => ScheduledReportKind::Q3,
                (crate::accounting::reports::ReportKind::Monthly, month) if matches!(frequency, ReportFrequency::Monthly { .. }) => ScheduledReportKind::Monthly { month },
                _ => continue,
            };
            let published_at = frequency.scheduled_instant(scheduled_kind, report.period.year(), seed, company)?;
            if published_at > through || after.is_some_and(|after| published_at <= after) { continue; }
            if library.reports_for_company(company, through).iter().any(|published| {
                published.reports.scope == report.scope && published.reports.period == report.period && published.reports.kind == report.kind
            }) { continue; }
            pending.push((published_at, company.clone(), scheduled_kind, report));
        }
    }
    pending.sort_by(|left, right| (left.0, &left.1, left.2).cmp(&(right.0, &right.1, right.2)));
    if pending.is_empty() { return Ok(Vec::new()); }
    let mut candidate = library.clone();
    let mut ids = Vec::with_capacity(pending.len());
    for (published_at, company, kind, report) in pending {
        let request = PublicationRequest {
            company: company.clone(), scope: report.scope, period: report.period, kind: report.kind,
            sequence: report.version.sequence,
            policy: system.accounting_policy_for(&company).map_err(system_error)?,
            approved_at: frequency.approval_instant(kind, published_at)?, published_at,
            origin: frequency.publication_origin(kind, report.period.year(), seed, &company, seeded)?,
            supersedes: None,
        };
        ids.push(candidate.publish_simple_closed(system.closing_for(&company).map_err(system_error)?, request)?);
    }
    *library = candidate;
    Ok(ids)
}
