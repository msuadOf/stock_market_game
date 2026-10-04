use std::collections::{BTreeMap, BTreeSet};

use crate::accounting::closing::ClosingEngine;
use crate::accounting::consolidation::{
    consolidate, ConsolidationRequest, GroupMember, IntercompanyBalance, MemberId, MemberSpec,
    ScopeId,
};
use crate::accounting::reports::{
    generate_report_set, ReportKind, ReportRequest, ReportSource, ReportVersion, VersionKind,
};
use crate::accounting::{AccountingAmount, AccountingPeriod, LedgerAccountId};
use crate::calendar::CivilInstant;
use crate::company::operations::CompanyOperations;
use crate::company::CompanyId;
use crate::information::{
    AccountingPolicyRef, InformationError, PublicLibrary, PublicationId, PublicationOrigin,
    PublicationRequest, ScheduledReportKind, APPROVAL_HOUR,
};

mod sales;
#[cfg(test)]
mod tests;

/// 固定开局的母公司持股事实；股数单位为股，不随证券交易变化。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct GroupHolding {
    pub company: CompanyId,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub parent_held_shares: u64,
}

/// 单层固定集团；成员关系与股本从 CompanySpec 读取，不另存第二份关系。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct GroupStructure {
    pub root: CompanyId,
    pub holdings: Vec<GroupHolding>,
}

fn invalid(detail: String) -> InformationError {
    InformationError::ReportNotPublishable(Box::new(
        crate::accounting::reports::ReportError::InternalWindowInconsistent { detail },
    ))
}

fn request<'a>(
    group: &GroupStructure,
    ops: &'a CompanyOperations,
    bound: Option<AccountingPeriod>,
) -> Result<ConsolidationRequest<'a>, InformationError> {
    let mut holdings = BTreeMap::new();
    for holding in &group.holdings {
        if holdings
            .insert(holding.company.clone(), holding.parent_held_shares)
            .is_some()
        {
            return Err(invalid(format!(
                "group {} duplicate holding {}",
                group.root.0, holding.company.0
            )));
        }
    }
    let ids = std::iter::once(&group.root)
        .chain(holdings.keys())
        .collect::<BTreeSet<_>>();
    let mut members = Vec::new();
    let mut balances = Vec::new();
    for id in ids {
        let company = ops
            .company(id)
            .ok_or_else(|| invalid(format!("group {} unknown member {}", group.root.0, id.0)))?;
        let spec = company.spec();
        members.push(GroupMember {
            spec: MemberSpec {
                id: MemberId(id.0.clone()),
                group_parent: spec.group_parent.as_ref().map(|id| MemberId(id.0.clone())),
                issued_shares: spec.issued_shares,
                parent_held_shares: holdings.get(id).copied().unwrap_or(0),
            },
            books: company.books().books(),
        });
        if let Some(industrial) = company.books().as_industrial() {
            if industrial.books().journal().entries().any(|entry| {
                entry.kind == crate::accounting::BusinessKind::OpeningBalance
                    && entry
                        .lines
                        .iter()
                        .any(|line| ["1122", "2202"].contains(&line.account.0.as_str()))
            }) {
                return Err(invalid(format!("group member {} opening trade balances require counterparty identities and are unsupported", id.0)));
            }
            industrial
                .validate_trade_counterparty_events()
                .map_err(|error| invalid(error.to_string()))?;
            let entries = industrial
                .books()
                .journal()
                .entries()
                .map(|entry| (entry.source, entry))
                .collect::<BTreeMap<_, _>>();
            for account in ["1122", "2202"] {
                let mut amounts: BTreeMap<CompanyId, AccountingAmount> = BTreeMap::new();
                for identity in industrial
                    .trade_counterparty_events()
                    .iter()
                    .filter(|identity| identity.account.0 == account)
                {
                    let entry = entries[&identity.event];
                    if bound.is_some_and(|bound| entry.period() > bound) {
                        continue;
                    }
                    let counterparty = CompanyId(identity.counterparty.0.clone());
                    if counterparty != *id
                        && (counterparty == group.root || holdings.contains_key(&counterparty))
                    {
                        let amount = amounts.entry(counterparty).or_default();
                        for line in entry
                            .lines
                            .iter()
                            .filter(|line| line.account == identity.account)
                        {
                            let increases = matches!(
                                (account, line.side),
                                ("1122", crate::accounting::PostingSide::Debit)
                                    | ("2202", crate::accounting::PostingSide::Credit)
                            );
                            *amount = if increases {
                                amount.add(line.amount)
                            } else {
                                amount.sub(line.amount)
                            }
                            .map_err(|error| invalid(error.to_string()))?;
                        }
                    }
                }
                for (counterparty, amount) in amounts {
                    if amount.is_zero() {
                        continue;
                    }
                    if !amount.is_positive() {
                        return Err(invalid(format!(
                            "negative internal trade balance for {} against {} account {}: {}",
                            id.0, counterparty.0, account, amount
                        )));
                    }
                    balances.push(IntercompanyBalance {
                        member: MemberId(id.0.clone()),
                        counterparty: MemberId(counterparty.0),
                        account: LedgerAccountId(account.into()),
                        amount,
                    });
                }
            }
        }
    }
    for company in ops.companies.values() {
        if company.spec().group_parent.as_ref() == Some(&group.root)
            && !holdings.contains_key(&company.spec().id)
        {
            return Err(invalid(format!(
                "group {} missing subsidiary holding {}",
                group.root.0,
                company.spec().id.0
            )));
        }
    }
    Ok(ConsolidationRequest {
        root: MemberId(group.root.0.clone()),
        members,
        intercompany_balances: balances,
        intercompany_sales: sales::derive_sales(group, ops, bound)?,
    })
}

pub(crate) fn validate_groups(
    groups: &[GroupStructure],
    ops: &CompanyOperations,
) -> Result<(), InformationError> {
    let mut roots = BTreeSet::new();
    let mut included = BTreeSet::new();
    for group in groups {
        if !roots.insert(&group.root) {
            return Err(invalid(format!("duplicate group root {}", group.root.0)));
        }
        let request = request(group, ops, None)?;
        for member in &request.members {
            if !included.insert(member.spec.id.clone()) {
                return Err(invalid(format!(
                    "member {} appears in multiple groups",
                    member.spec.id.0
                )));
            }
        }
        consolidate(request).map_err(|error| invalid(error.to_string()))?;
    }
    for company in ops.companies.values() {
        if company.spec().group_parent.is_some()
            && !included.contains(&MemberId(company.spec().id.0.clone()))
        {
            return Err(invalid(format!(
                "subsidiary {} has no configured group",
                company.spec().id.0
            )));
        }
    }
    Ok(())
}

pub(crate) fn ensure_group_report(
    group: &GroupStructure,
    ops: &CompanyOperations,
    closing: &mut ClosingEngine,
    period: AccountingPeriod,
    kind: ReportKind,
) -> Result<(), InformationError> {
    let scope = ScopeId::Consolidated(MemberId(group.root.0.clone()));
    if !closing.versions(&scope, period, kind).is_empty() {
        return Ok(());
    }
    let set = generate_report_set(ReportRequest {
        period,
        kind,
        source: ReportSource::Consolidated {
            request: request(group, ops, Some(period))?,
        },
        version: ReportVersion {
            sequence: 1,
            supersedes: None,
            kind: VersionKind::Original,
        },
        adjustments: &BTreeMap::new(),
    })
    .map_err(|error| InformationError::ReportNotPublishable(Box::new(error)))?;
    closing
        .record(set)
        .map_err(|error| InformationError::Closing(Box::new(error)))?;
    Ok(())
}

pub(crate) fn record_group_periods(
    groups: &[GroupStructure],
    ops: &CompanyOperations,
    closing: &mut ClosingEngine,
    period: AccountingPeriod,
) -> Result<(), InformationError> {
    for group in groups {
        ensure_group_report(group, ops, closing, period, ReportKind::Monthly)?;
        match period.month() {
            3 | 9 => ensure_group_report(group, ops, closing, period, ReportKind::Quarter)?,
            6 => ensure_group_report(group, ops, closing, period, ReportKind::HalfYear)?,
            12 => ensure_group_report(group, ops, closing, period, ReportKind::Annual)?,
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn seed_groups(
    groups: &[GroupStructure],
    prehistory: &mut crate::information::SeededPrehistory,
    start: crate::calendar::CivilDate,
) -> Result<(), InformationError> {
    validate_groups(groups, &prehistory.ops)?;
    let start_instant = CivilInstant::new(start, 0)?;
    let mut due = Vec::new();
    for group in groups {
        let offset = crate::information::stable_company_offset(prehistory.ops.seed, &group.root);
        for fiscal_year in start.year() - 2..=start.year() {
            for kind in ScheduledReportKind::ALL {
                let instant = crate::information::scheduled_instant(kind, fiscal_year, offset)?;
                if instant < start_instant {
                    due.push((instant, group.root.clone(), fiscal_year, kind, offset));
                }
            }
        }
    }
    due.sort_by_key(|row| (row.0, row.1.clone(), row.2));
    for (instant, root_id, fiscal_year, kind, offset) in due {
        let group = groups
            .iter()
            .find(|group| group.root == root_id)
            .expect("seed schedule comes from configured groups");
        let period = kind.landing_period(fiscal_year)?;
        ensure_group_report(
            group,
            &prehistory.ops,
            &mut prehistory.closing,
            period,
            kind.report_kind(),
        )?;
        let root = prehistory
            .ops
            .company(&root_id)
            .expect("group validation checked root");
        prehistory.library.publish_closed(
            &prehistory.closing,
            PublicationRequest {
                company: root_id.clone(),
                scope: ScopeId::Consolidated(MemberId(root_id.0)),
                period,
                kind: kind.report_kind(),
                sequence: 1,
                policy: AccountingPolicyRef {
                    chart_version: root.books().books().ledger().chart().version(),
                },
                approved_at: CivilInstant::from_hms(instant.date(), APPROVAL_HOUR, 0, 0)?,
                published_at: instant,
                origin: PublicationOrigin::SeededPrehistory {
                    fiscal_year,
                    kind,
                    offset_days: offset,
                },
                supersedes: None,
            },
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn publish_group_scheduled(
    group: &GroupStructure,
    ops: &CompanyOperations,
    closing: &mut ClosingEngine,
    library: &mut PublicLibrary,
    instant: CivilInstant,
    fiscal_year: i32,
    kind: ScheduledReportKind,
    offset: u8,
) -> Result<PublicationId, InformationError> {
    let period = kind.landing_period(fiscal_year)?;
    ensure_group_report(group, ops, closing, period, kind.report_kind())?;
    let root = ops
        .company(&group.root)
        .ok_or_else(|| invalid(format!("unknown group root {}", group.root.0)))?;
    library.publish_closed(
        closing,
        PublicationRequest {
            company: group.root.clone(),
            scope: ScopeId::Consolidated(MemberId(group.root.0.clone())),
            period,
            kind: kind.report_kind(),
            sequence: 1,
            policy: AccountingPolicyRef {
                chart_version: root.books().books().ledger().chart().version(),
            },
            approved_at: CivilInstant::from_hms(instant.date(), APPROVAL_HOUR, 0, 0)?,
            published_at: instant,
            origin: PublicationOrigin::ScheduledDisclosure {
                fiscal_year,
                kind,
                offset_days: offset,
            },
            supersedes: None,
        },
    )
}
