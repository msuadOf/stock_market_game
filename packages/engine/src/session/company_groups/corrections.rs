use std::collections::BTreeMap;

use crate::accounting::closing::ClosingEngine;
use crate::accounting::consolidation::{MemberId, ScopeId};
use crate::accounting::reports::{
    generate_report_set, ReportRequest, ReportSource, ReportVersion, VersionKind,
};
use crate::accounting::{AccountingPeriod, BusinessEventId};
use crate::calendar::CivilInstant;
use crate::company::operations::CompanyOperations;
use crate::information::{
    InformationError, PublicLibrary, PublicationId, PublicationOrigin, PublicationRequest,
};

use super::{invalid, request, GroupStructure};

pub(super) fn member_adjustments<'a>(
    group: &GroupStructure,
    operations: &'a CompanyOperations,
    closing: &'a ClosingEngine,
) -> Result<BTreeMap<MemberId, &'a BTreeMap<BusinessEventId, AccountingPeriod>>, InformationError> {
    let mut maps = BTreeMap::new();
    for company_id in
        std::iter::once(&group.root).chain(group.holdings.iter().map(|holding| &holding.company))
    {
        let company = operations
            .company(company_id)
            .ok_or_else(|| invalid(format!("集团成员 {} 不存在", company_id.0)))?;
        let member = MemberId(company_id.0.clone());
        let map = closing.restatement_periods(&ScopeId::Standalone(member.clone()));
        let restatements = company.books().income_tax_restatements();
        if map.map_or(!restatements.is_empty(), |map| map != restatements) {
            return Err(invalid(format!(
                "集团成员 {} 的税务与结账重述身份不一致",
                company_id.0
            )));
        }
        if let Some(map) = map {
            maps.insert(member, map);
        }
    }
    Ok(maps)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn refresh_group_reports_after_correction(
    group: &GroupStructure,
    operations: &CompanyOperations,
    closing: &mut ClosingEngine,
    library: &mut PublicLibrary,
    from_period: AccountingPeriod,
    reason: &str,
    approved_at: CivilInstant,
    published_at: CivilInstant,
) -> Result<Vec<PublicationId>, InformationError> {
    let scope = ScopeId::Consolidated(MemberId(group.root.0.clone()));
    let mut published = BTreeMap::new();
    for report in library.save().reports {
        if report.reports.scope == scope {
            published.insert((report.reports.period, report.reports.kind), report);
        }
    }
    let mut publications = Vec::new();
    for previous in closing
        .latest_reports_for_scope(&scope)
        .into_iter()
        .filter(|report| report.period >= from_period)
    {
        let maps = member_adjustments(group, operations, closing)?;
        let mut candidate = generate_report_set(ReportRequest {
            period: previous.period,
            kind: previous.kind,
            source: ReportSource::ConsolidatedRestated {
                request: request(group, operations, Some(previous.period))?,
                member_adjustments: &maps,
            },
            version: previous.version.clone(),
            adjustments: &BTreeMap::new(),
        })
        .map_err(|error| InformationError::ReportNotPublishable(Box::new(error)))?;
        candidate
            .validate()
            .map_err(|error| InformationError::ReportNotPublishable(Box::new(error)))?;
        if candidate == previous {
            continue;
        }
        let original = published.get(&(previous.period, previous.kind));
        let sequence = previous
            .version
            .sequence
            .checked_add(1)
            .ok_or_else(|| invalid("集团更正报告版本序号耗尽".into()))?;
        candidate.version = ReportVersion {
            sequence,
            supersedes: original.map(|_| previous.version.sequence),
            kind: if original.is_some() {
                VersionKind::Correction {
                    reason: reason.into(),
                }
            } else {
                VersionKind::Original
            },
        };
        closing
            .record(candidate)
            .map_err(|error| InformationError::Closing(Box::new(error)))?;
        if let Some(original) = original {
            if original.published_at > published_at {
                return Err(InformationError::CorrectionPrecedesOriginal {
                    original: original.id,
                    original_at: original.published_at,
                    published_at,
                });
            }
            publications.push(library.publish_closed(
                closing,
                PublicationRequest {
                    company: group.root.clone(),
                    scope: scope.clone(),
                    period: previous.period,
                    kind: previous.kind,
                    sequence,
                    policy: original.policy.clone(),
                    approved_at,
                    published_at,
                    origin: PublicationOrigin::Correction,
                    supersedes: Some(original.id),
                },
            )?);
        }
    }
    Ok(publications)
}
