//! 只读所选公司源，准备一个 PublicLibrary／DisclosureDispatch 候选。
//!
//! 此模块不安装候选、不推进公司或账户，也不安排 NPC 观察。publication 的
//! published_at 是材料公布时点；visible_through 仅保留调用方给出的真实扫描上界。

use super::DisclosureDispatch;
use crate::accounting::closing::ClosingEngine;
use crate::accounting::consolidation::ScopeId;
use crate::accounting::reports::{ReportKind, ReportSet, ReportVersion, VersionKind};
use crate::accounting::AccountingPeriod;
use crate::accounting::BusinessEventId;
use crate::calendar::{CivilDate, CivilInstant};
use crate::company::operations::CompanyOperations;
use crate::company::{CompanyId, CompanySpec, CompanySystem};
use crate::information::{
    AccountingPolicyRef, AnnouncedEvent, Announcement, AnnouncementContent, AnnouncementRequest,
    CashDividendAnnouncement, InformationError, IssuerRepurchaseAnnouncement, PublicLibrary,
    PublicationId, PublicationRequest, PublicationSource, PublishedReport, ReportFrequency,
    RightsOfferingAnnouncement, ScheduledReportKind,
};
use crate::session::corporate_actions::SessionCorporateActions;
use std::collections::{BTreeMap, BTreeSet};

/// selected source 的短借用；不保存另一份公司状态，也不执行 fallback。
#[derive(Clone, Copy)]
pub(in crate::session) enum DisclosureSourceRef<'a> {
    Simple(&'a CompanySystem),
    Legacy {
        operations: &'a CompanyOperations,
        closing: &'a ClosingEngine,
    },
    PreparedSimple(&'a crate::company::PreparedSimpleDisclosureCorrection<'a>),
    #[expect(
        dead_code,
        reason = "保留正式 Legacy owner 更正证明的 selected/proof 适配；现有生产 builder 未接入且仓内无构造"
    )]
    PreparedLegacy(&'a crate::company::operations::PreparedLegacyDisclosureCorrection<'a>),
}

pub(in crate::session) struct DisclosurePrepareCtx<'a> {
    pub source: DisclosureSourceRef<'a>,
    pub previous_dispatch: &'a DisclosureDispatch,
    pub previous_library: &'a PublicLibrary,
    pub seed: u64,
    pub report_frequency: ReportFrequency,
    pub through: CivilInstant,
    /// 前史只能扫描至开局前日末秒；None 表示调用方真实 live 窗口。
    pub prehistory_opening: Option<CivilDate>,
    /// 只有完整 civil 公告阶段才传入；盘中调用必须为 None。
    pub settled_announcements: Option<CivilDate>,
    /// 已批准公司行为账簿；现金金额还必须与所选源真实 finance fact 相符。
    pub corporate_actions: Option<&'a SessionCorporateActions>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session) enum DisclosurePublicationKind {
    Announcement,
    Report {
        scope: ScopeId,
        period: AccountingPeriod,
        kind: ReportKind,
        version: ReportVersion,
        supersedes: Option<PublicationId>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session) struct DisclosurePublication {
    pub id: PublicationId,
    pub company: CompanyId,
    pub published_at: CivilInstant,
    pub kind: DisclosurePublicationKind,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::session) struct DisclosureCandidate {
    pub dispatch: DisclosureDispatch,
    pub library: PublicLibrary,
    pub publications: Vec<DisclosurePublication>,
    pub visible_through: CivilInstant,
}

/// 披露纯候选的明确失败原因；未来确认不能让外层取得可安装的未来金融结果。
#[derive(Debug, thiserror::Error)]
pub(in crate::session) enum DisclosurePrepareError {
    #[error(transparent)]
    Information(#[from] InformationError),
    #[error(transparent)]
    Company(#[from] crate::company::CompanySystemError),
    #[error(transparent)]
    CashDividend(#[from] crate::company::cash_dividend::CashDividendError),
    #[error("披露所选源没有公司 {company}")]
    UnknownCompany { company: CompanyId },
    #[error("公司 {company} 披露来源错误：期望 {expected:?}，实际 {actual:?}")]
    SourceMismatch {
        company: CompanyId,
        expected: PublicationSource,
        actual: PublicationSource,
    },
    #[error("公司 {company} 披露 policy 错误：期望 chart {expected}，实际 {actual}")]
    PolicyMismatch {
        company: CompanyId,
        expected: u32,
        actual: u32,
    },
    #[error("公司 {company} 尚无可用于 {kind} 公告的已批准财务事实接口")]
    UnsupportedAnnouncement {
        company: CompanyId,
        kind: &'static str,
    },
    #[error("披露扫描窗口逆序：{after:?} > {through:?}")]
    ReversedWindow {
        after: CivilInstant,
        through: CivilInstant,
    },
    #[error("更正包的真实 base 与 previous_library 不一致")]
    CorrectionBaseMismatch,
    #[error("前史上界 {through:?} 不是开局日 {opening} 的前日 23:59:59")]
    InvalidPrehistoryWindow {
        opening: CivilDate,
        through: CivilInstant,
    },
    #[error("完整更正包的公布时间 {published_at:?} 不在本次 ({after:?}, {through:?}] 窗口")]
    CorrectionOutsideWindow {
        published_at: CivilInstant,
        after: Option<CivilInstant>,
        through: CivilInstant,
    },
}

/// 盘中月报的轻量预过滤：不克隆公开库，只检查是否存在到期且尚未公开的
/// Monthly Original。完整候选仍由 [`prepare_disclosures`] 生成。
pub(in crate::session) fn has_due_monthly_report(
    source: DisclosureSourceRef<'_>,
    frequency: ReportFrequency,
    library: &PublicLibrary,
    seed: u64,
    after: Option<CivilInstant>,
    through: CivilInstant,
) -> Result<bool, DisclosurePrepareError> {
    if !matches!(frequency, ReportFrequency::Monthly { .. }) {
        return Ok(false);
    }
    let source = source.selected();
    let mut latest =
        BTreeMap::<ReportKey, (&crate::accounting::reports::ReportSet, CompanyId)>::new();
    for (company, _closing, set) in source.registered_reports()? {
        if !matches!(set.version.kind, VersionKind::Original) {
            continue;
        }
        if scheduled_kind(set.kind, set.period, frequency).is_none() {
            continue;
        }
        let key = report_key(company, set);
        if latest
            .get(&key)
            .is_none_or(|(previous, _)| previous.version.sequence < set.version.sequence)
        {
            latest.insert(key, (set, company));
        }
    }
    for (key, (set, company)) in latest {
        let Some(ScheduledReportKind::Monthly { month }) =
            scheduled_kind(set.kind, set.period, frequency)
        else {
            continue;
        };
        let code = source.spec(company)?.code.as_str();
        let published_at = frequency.scheduled_instant(
            ScheduledReportKind::Monthly { month },
            set.period.year(),
            seed,
            code,
        )?;
        if !in_window(after, through, published_at) {
            continue;
        }
        if !library.has_report_identity(key.0, &key.1, key.2, key.3) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(in crate::session) fn prepare_disclosures(
    ctx: DisclosurePrepareCtx<'_>,
) -> Result<DisclosureCandidate, DisclosurePrepareError> {
    let after = ctx.previous_dispatch.published_through();
    if let Some(after) = after {
        if after > ctx.through {
            return Err(DisclosurePrepareError::ReversedWindow {
                after,
                through: ctx.through,
            });
        }
    }
    if let Some(opening) = ctx.prehistory_opening {
        let endpoint = CivilInstant::new(opening.prev().map_err(InformationError::from)?, 86399)
            .map_err(InformationError::from)?;
        if ctx.through != endpoint
            || ctx.settled_announcements.is_some()
            || ctx.source.proof().is_some()
        {
            return Err(DisclosurePrepareError::InvalidPrehistoryWindow {
                opening,
                through: ctx.through,
            });
        }
    }
    if ctx
        .previous_library
        .latest_published_instant()
        .is_some_and(|instant| instant > ctx.through)
    {
        return Err(invalid("previous_library 含有晚于真实 through 的材料"));
    }
    let source = ctx.source.selected();
    validate_previous_library(source, ctx.previous_library)?;
    let mut pending = Vec::new();
    let published_keys = ctx
        .previous_library
        .save()
        .reports
        .iter()
        .map(|report| report_key(report.company, &report.reports))
        .collect::<BTreeSet<_>>();
    let mut seen_originals = BTreeMap::<ReportIdentity, &ReportSet>::new();
    let mut latest_originals = BTreeMap::<ReportKey, (&ClosingEngine, &ReportSet)>::new();
    for (company, closing, set) in source.registered_reports()? {
        if !matches!(set.version.kind, VersionKind::Original) {
            continue;
        }
        if let Some(previous) = seen_originals.insert(report_identity(company, set), set) {
            if previous != set {
                return Err(invalid("同一 Original identity 的完整 ReportSet 不一致"));
            }
        }
        if closing.version(&set.scope, set.period, set.kind, set.version.sequence) != Some(set) {
            return Err(invalid("Original 定稿与同源登记序号查询不一致"));
        }
        let key = report_key(company, set);
        if latest_originals
            .get(&key)
            .is_none_or(|(_, previous)| previous.version.sequence < set.version.sequence)
        {
            latest_originals.insert(key, (closing, set));
        }
    }
    for (key, (closing, set)) in latest_originals {
        // 已公开的完整版本先经同源校验，私有 Original 重建不得另发或重标旧材料。
        if published_keys.contains(&key) {
            continue;
        }
        let company = key.0;
        let Some(kind) = scheduled_kind(set.kind, set.period, ctx.report_frequency) else {
            continue;
        };
        let code = source.spec(company)?.code.as_str();
        let published_at =
            ctx.report_frequency
                .scheduled_instant(kind, set.period.year(), ctx.seed, code)?;
        if !in_window(after, ctx.through, published_at) {
            continue;
        }
        let request = PublicationRequest {
            company,
            scope: set.scope.clone(),
            period: set.period,
            kind: set.kind,
            sequence: set.version.sequence,
            policy: source.policy(company)?,
            approved_at: ctx.report_frequency.approval_instant(kind, published_at)?,
            published_at,
            origin: ctx.report_frequency.publication_origin(
                kind,
                set.period.year(),
                ctx.seed,
                code,
                ctx.prehistory_opening.is_some(),
            )?,
            supersedes: None,
        };
        pending.push(Pending::Report(ReportMaterial {
            closing,
            set,
            source: source.source_kind(),
            request,
            operation_sources: &[],
        }));
    }
    if let Some(proof) = ctx.source.proof() {
        if proof.base() != ctx.previous_library {
            return Err(DisclosurePrepareError::CorrectionBaseMismatch);
        }
        for operation in proof.operations() {
            for (request, approved_set, expected_source) in operation.reports() {
                if !in_window(after, ctx.through, request.published_at) {
                    return Err(DisclosurePrepareError::CorrectionOutsideWindow {
                        published_at: request.published_at,
                        after,
                        through: ctx.through,
                    });
                }
                let closing = source.closing(request.company)?;
                let actual = closing
                    .version(
                        &request.scope,
                        request.period,
                        request.kind,
                        request.sequence,
                    )
                    .ok_or_else(|| invalid("更正 proof 缺少同源定稿版本"))?;
                if actual != approved_set || *expected_source != source.source_kind() {
                    return Err(invalid(
                        "更正 proof 的完整 ReportSet 或 source 与金融候选不一致",
                    ));
                }
                pending.push(Pending::Report(ReportMaterial {
                    closing,
                    set: actual,
                    source: *expected_source,
                    request: request.clone(),
                    operation_sources: operation.operation_sources(),
                }));
            }
        }
    }
    let mut announced_through = ctx.previous_dispatch.announced_through();
    if let Some(settled) = ctx.settled_announcements {
        let second = match ctx.report_frequency {
            ReportFrequency::Monthly { .. } => 86399,
            ReportFrequency::Quarterly => 18 * 3600,
        };
        let endpoint = CivilInstant::new(settled, second).map_err(InformationError::from)?;
        if ctx.through != endpoint || announced_through.is_some_and(|previous| previous > settled) {
            return Err(invalid(
                "完整 civil 公告阶段必须使用真实频率 endpoint，日期不得回退",
            ));
        }
        if announced_through.is_none_or(|previous| previous < settled) {
            collect_announcements(source, ctx.corporate_actions, settled, &mut pending)?;
            announced_through = Some(settled);
        }
    }
    pending.sort_by_cached_key(Pending::order_key);
    let mut library = ctx.previous_library.clone();
    let mut publications = Vec::new();
    let mut reports = BTreeMap::<ReportIdentity, ReportMaterial<'_>>::new();
    let mut announcements =
        BTreeMap::<(CompanyId, AnnouncementIdentity), AnnouncementRequest>::new();
    for material in pending {
        match material {
            Pending::Report(material) => {
                let identity = report_identity(material.request.company, material.set);
                if let Some(previous) = reports.get(&identity) {
                    if !same_material(previous, &material) {
                        return Err(invalid("同一报告 identity 的 payload／更正操作身份不一致"));
                    }
                    continue;
                }
                let old = library
                    .reports_for_company(&material.request.company, ctx.through)
                    .into_iter()
                    .find(|report| report_identity(report.company, &report.reports) == identity);
                if let Some(old) = old {
                    if !same_published(old, &material) {
                        return Err(invalid("既有报告 identity 被不同 payload 占用"));
                    }
                } else {
                    if library.version() == u32::MAX {
                        return Err(invalid("PublicationId 已耗尽"));
                    }
                    let id = match material.source {
                        PublicationSource::SimpleGenerated => library
                            .publish_simple_closed(material.closing, material.request.clone())?,
                        PublicationSource::SimulationAccounting => {
                            library.publish_closed(material.closing, material.request.clone())?
                        }
                    };
                    publications.push(DisclosurePublication {
                        id,
                        company: material.request.company,
                        published_at: material.request.published_at,
                        kind: DisclosurePublicationKind::Report {
                            scope: material.set.scope.clone(),
                            period: material.set.period,
                            kind: material.set.kind,
                            version: material.set.version.clone(),
                            supersedes: material.request.supersedes,
                        },
                    });
                }
                reports.insert(identity, material);
            }
            Pending::Announcement(request) => {
                let identity = (
                    request.company,
                    announcement_identity(&request.content, request.occurred_on),
                );
                if let Some(previous) = announcements.get(&identity) {
                    if !same_announcement_request(previous, &request) {
                        return Err(invalid("同一公告 identity 的 payload 不一致"));
                    }
                    continue;
                }
                let old = library.save().announcements.into_iter().find(|old| {
                    old.company == request.company
                        && announcement_identity(&old.content, old.occurred_on) == identity.1
                });
                if let Some(old) = old {
                    if !same_announcement(&old, &request) {
                        return Err(invalid("既有公告 identity 被不同 payload 占用"));
                    }
                } else {
                    if library.version() == u32::MAX {
                        return Err(invalid("PublicationId 已耗尽"));
                    }
                    let id = library.publish_announcement(request.clone())?;
                    publications.push(DisclosurePublication {
                        id,
                        company: request.company,
                        published_at: request.published_at,
                        kind: DisclosurePublicationKind::Announcement,
                    });
                }
                announcements.insert(identity, request);
            }
        }
    }
    Ok(DisclosureCandidate {
        dispatch: DisclosureDispatch {
            published_through: Some(ctx.through),
            announced_through,
        },
        library,
        publications,
        visible_through: ctx.through,
    })
}

fn invalid(detail: &str) -> DisclosurePrepareError {
    InformationError::InconsistentLibrary {
        detail: detail.into(),
    }
    .into()
}
fn in_window(after: Option<CivilInstant>, through: CivilInstant, instant: CivilInstant) -> bool {
    instant <= through && after.is_none_or(|after| instant > after)
}
fn scheduled_kind(
    kind: ReportKind,
    period: AccountingPeriod,
    frequency: ReportFrequency,
) -> Option<ScheduledReportKind> {
    match (kind, period.month()) {
        (ReportKind::Annual, 12) => Some(ScheduledReportKind::Annual),
        (ReportKind::HalfYear, 6) => Some(ScheduledReportKind::HalfYear),
        (ReportKind::Quarter, 3) => Some(ScheduledReportKind::Q1),
        (ReportKind::Quarter, 9) => Some(ScheduledReportKind::Q3),
        (ReportKind::Monthly, month) if matches!(frequency, ReportFrequency::Monthly { .. }) => {
            Some(ScheduledReportKind::Monthly { month })
        }
        _ => None,
    }
}

impl<'a> DisclosureSourceRef<'a> {
    fn selected(self) -> Self {
        match self {
            Self::PreparedSimple(prepared) => Self::Simple(prepared.source()),
            Self::PreparedLegacy(prepared) => {
                let (operations, closing) = prepared.source();
                Self::Legacy {
                    operations,
                    closing,
                }
            }
            selected => selected,
        }
    }
    fn proof(
        self,
    ) -> Option<&'a crate::company::report_correction::ConfirmedDisclosureCorrections<'a>> {
        match self {
            Self::PreparedSimple(prepared) => Some(prepared.proof()),
            Self::PreparedLegacy(prepared) => Some(prepared.proof()),
            _ => None,
        }
    }
    fn source_kind(self) -> PublicationSource {
        match self.selected() {
            Self::Simple(_) => PublicationSource::SimpleGenerated,
            Self::Legacy { .. } => PublicationSource::SimulationAccounting,
            _ => unreachable!("已提取唯一 source"),
        }
    }
    fn spec(self, company: CompanyId) -> Result<&'a CompanySpec, DisclosurePrepareError> {
        match self.selected() {
            Self::Simple(system) => system
                .issuers()
                .get(&company)
                .ok_or(DisclosurePrepareError::UnknownCompany { company }),
            Self::Legacy { operations, .. } => operations
                .company(&company)
                .map(|company| company.spec())
                .ok_or(DisclosurePrepareError::UnknownCompany { company }),
            _ => unreachable!("已提取唯一 source"),
        }
    }
    fn closing(self, company: CompanyId) -> Result<&'a ClosingEngine, DisclosurePrepareError> {
        self.spec(company)?;
        match self.selected() {
            Self::Simple(system) => Ok(system.closing_for(&company)?),
            Self::Legacy { closing, .. } => Ok(closing),
            _ => unreachable!("已提取唯一 source"),
        }
    }
    fn policy(self, company: CompanyId) -> Result<AccountingPolicyRef, DisclosurePrepareError> {
        match self.selected() {
            Self::Simple(system) => Ok(system.accounting_policy_for(&company)?),
            Self::Legacy { operations, .. } => Ok(AccountingPolicyRef {
                chart_version: operations
                    .company(&company)
                    .ok_or(DisclosurePrepareError::UnknownCompany { company })?
                    .books()
                    .books()
                    .ledger()
                    .chart()
                    .version(),
            }),
            _ => unreachable!("已提取唯一 source"),
        }
    }
    fn registered_reports(
        self,
    ) -> Result<Vec<(CompanyId, &'a ClosingEngine, &'a ReportSet)>, DisclosurePrepareError> {
        let mut result = Vec::new();
        match self.selected() {
            Self::Simple(system) => {
                for (company, _) in system.issuers().iter() {
                    let closing = system.closing_for(company)?;
                    for (scope, period, kind, versions) in closing.report_versions() {
                        if *scope
                            != ScopeId::Standalone(crate::accounting::consolidation::MemberId(
                                company.0,
                            ))
                        {
                            return Err(invalid("Simple 定稿含有其他 scope"));
                        }
                        for set in versions {
                            if set.scope != *scope || set.period != period || set.kind != kind {
                                return Err(invalid("定稿登记键与完整报表不一致"));
                            }
                            result.push((*company, closing, set));
                        }
                    }
                }
            }
            Self::Legacy { closing, .. } => {
                for (scope, period, kind, versions) in closing.report_versions() {
                    let company = match scope {
                        ScopeId::Standalone(member) | ScopeId::Consolidated(member) => {
                            CompanyId(member.0)
                        }
                    };
                    self.spec(company)?;
                    for set in versions {
                        if set.scope != *scope || set.period != period || set.kind != kind {
                            return Err(invalid("定稿登记键与完整报表不一致"));
                        }
                        result.push((company, closing, set));
                    }
                }
            }
            _ => unreachable!("已提取唯一 source"),
        }
        Ok(result)
    }
}

fn validate_previous_library(
    source: DisclosureSourceRef<'_>,
    library: &PublicLibrary,
) -> Result<(), DisclosurePrepareError> {
    for report in library.save().reports {
        if report.source != source.source_kind() {
            return Err(DisclosurePrepareError::SourceMismatch {
                company: report.company,
                expected: source.source_kind(),
                actual: report.source,
            });
        }
        let policy = source.policy(report.company)?;
        if report.policy != policy {
            return Err(DisclosurePrepareError::PolicyMismatch {
                company: report.company,
                expected: policy.chart_version,
                actual: report.policy.chart_version,
            });
        }
        if source.closing(report.company)?.version(
            &report.reports.scope,
            report.reports.period,
            report.reports.kind,
            report.reports.version.sequence,
        ) != Some(&report.reports)
        {
            return Err(invalid("公开库的完整报告没有同源不可变定稿版本"));
        }
    }
    Ok(())
}

struct ReportMaterial<'a> {
    closing: &'a ClosingEngine,
    set: &'a ReportSet,
    source: PublicationSource,
    request: PublicationRequest,
    operation_sources: &'a [BusinessEventId],
}

enum Pending<'a> {
    Report(ReportMaterial<'a>),
    Announcement(AnnouncementRequest),
}

type ReportKey = (CompanyId, ScopeId, AccountingPeriod, ReportKind);
type ReportIdentity = (CompanyId, ScopeId, AccountingPeriod, ReportKind, u32);

fn report_key(company: CompanyId, set: &ReportSet) -> ReportKey {
    (company, set.scope.clone(), set.period, set.kind)
}

fn report_identity(company: CompanyId, set: &ReportSet) -> ReportIdentity {
    (
        company,
        set.scope.clone(),
        set.period,
        set.kind,
        set.version.sequence,
    )
}

#[derive(Eq, PartialEq, Ord, PartialOrd)]
struct ReportOrder {
    scope: (u8, u32),
    period: AccountingPeriod,
    kind: u8,
    version_kind: u8,
    sequence: u32,
    private_predecessor: Option<u32>,
    public_predecessor: Option<PublicationId>,
    reason: String,
    operation_sources: Vec<BusinessEventId>,
}

#[derive(Eq, PartialEq, Ord, PartialOrd)]
enum ShockIdentity {
    MarketDemandShift,
    IndustryCostShift(String),
    CompanyDemandShift,
    ContractWon,
    ContractCancelled,
    CreditDeterioration,
    ProductionInterruption,
    AssetImpairmentSignal,
    PaymentFailure {
        what: String,
        amount: i128,
        obligation_status: u8,
    },
}

#[derive(Eq, PartialEq, Ord, PartialOrd)]
enum AnnouncementIdentity {
    Shock {
        kind: ShockIdentity,
        amplitude_bp: i32,
        starts_on: CivilDate,
        expires_on: CivilDate,
        occurred_on: CivilDate,
    },
    CashDividend(String, CivilDate),
    RightsOffering(String, CivilDate),
    IssuerRepurchase(String, CivilDate),
    ShareSplit(String, CivilDate),
    StockDistribution(String, CivilDate),
}

#[derive(Eq, PartialEq, Ord, PartialOrd)]
struct PublicationOrder {
    published_at: CivilInstant,
    category: u8,
    company: CompanyId,
    announcement: Option<AnnouncementIdentity>,
    report: Option<ReportOrder>,
}

impl Pending<'_> {
    fn order_key(&self) -> PublicationOrder {
        match self {
            Self::Announcement(request) => PublicationOrder {
                published_at: request.published_at,
                category: 0,
                company: request.company,
                announcement: Some(announcement_identity(&request.content, request.occurred_on)),
                report: None,
            },
            Self::Report(material) => {
                let (version_kind, reason) = match &material.set.version.kind {
                    VersionKind::Original => (0, String::new()),
                    VersionKind::Correction { reason } => (1, reason.clone()),
                };
                let scope = match &material.set.scope {
                    ScopeId::Standalone(member) => (0, member.0),
                    ScopeId::Consolidated(member) => (1, member.0),
                };
                let kind = match material.set.kind {
                    ReportKind::Monthly => 0,
                    ReportKind::Quarter => 1,
                    ReportKind::HalfYear => 2,
                    ReportKind::Annual => 3,
                };
                PublicationOrder {
                    published_at: material.request.published_at,
                    category: 1,
                    company: material.request.company,
                    announcement: None,
                    report: Some(ReportOrder {
                        scope,
                        period: material.set.period,
                        kind,
                        version_kind,
                        sequence: material.set.version.sequence,
                        private_predecessor: material.set.version.supersedes,
                        public_predecessor: material.request.supersedes,
                        reason,
                        operation_sources: material.operation_sources.to_vec(),
                    }),
                }
            }
        }
    }
}

fn announcement_identity(
    content: &AnnouncementContent,
    occurred_on: CivilDate,
) -> AnnouncementIdentity {
    match content {
        AnnouncementContent::CashDividend(value) => {
            AnnouncementIdentity::CashDividend(value.plan.plan_id.clone(), occurred_on)
        }
        AnnouncementContent::RightsOffering(value) => {
            AnnouncementIdentity::RightsOffering(value.plan.event_id.clone(), occurred_on)
        }
        AnnouncementContent::IssuerRepurchase(value) => {
            AnnouncementIdentity::IssuerRepurchase(value.plan.event_id.clone(), occurred_on)
        }
        AnnouncementContent::ShareSplit(value) => {
            AnnouncementIdentity::ShareSplit(value.event_id.clone(), occurred_on)
        }
        AnnouncementContent::StockDistribution(value) => {
            AnnouncementIdentity::StockDistribution(value.event_id.clone(), occurred_on)
        }
        AnnouncementContent::Shock(event) => {
            use crate::company::events::PaymentObligationStatus;
            use crate::company::ShockKind;
            let kind = match &event.kind {
                ShockKind::MarketDemandShift => ShockIdentity::MarketDemandShift,
                ShockKind::IndustryCostShift { industry } => {
                    ShockIdentity::IndustryCostShift(industry.0.clone())
                }
                ShockKind::CompanyDemandShift => ShockIdentity::CompanyDemandShift,
                ShockKind::ContractWon => ShockIdentity::ContractWon,
                ShockKind::ContractCancelled => ShockIdentity::ContractCancelled,
                ShockKind::CreditDeterioration => ShockIdentity::CreditDeterioration,
                ShockKind::ProductionInterruption => ShockIdentity::ProductionInterruption,
                ShockKind::AssetImpairmentSignal => ShockIdentity::AssetImpairmentSignal,
                ShockKind::PaymentFailure {
                    what,
                    amount,
                    obligation_status,
                } => ShockIdentity::PaymentFailure {
                    what: what.clone(),
                    amount: amount.cents(),
                    obligation_status: match obligation_status {
                        PaymentObligationStatus::UncommittedExpense => 0,
                        PaymentObligationStatus::ContractualPayable => 1,
                        PaymentObligationStatus::ContractualOverdue => 2,
                        PaymentObligationStatus::StatutoryPaymentFailure => 3,
                    },
                },
            };
            AnnouncementIdentity::Shock {
                kind,
                amplitude_bp: event.amplitude_bp,
                starts_on: event.starts_on,
                expires_on: event.expires_on,
                occurred_on,
            }
        }
    }
}

fn same_request(left: &PublicationRequest, right: &PublicationRequest) -> bool {
    left.company == right.company
        && left.scope == right.scope
        && left.period == right.period
        && left.kind == right.kind
        && left.sequence == right.sequence
        && left.policy == right.policy
        && left.approved_at == right.approved_at
        && left.published_at == right.published_at
        && left.origin == right.origin
        && left.supersedes == right.supersedes
}
fn same_material(left: &ReportMaterial<'_>, right: &ReportMaterial<'_>) -> bool {
    left.set == right.set
        && left.source == right.source
        && same_request(&left.request, &right.request)
        && left.operation_sources == right.operation_sources
}
fn same_published(left: &PublishedReport, right: &ReportMaterial<'_>) -> bool {
    left.reports == *right.set
        && left.source == right.source
        && left.company == right.request.company
        && left.policy == right.request.policy
        && left.approved_at == right.request.approved_at
        && left.published_at == right.request.published_at
        && left.origin == right.request.origin
        && left.supersedes == right.request.supersedes
}
fn same_announcement_request(left: &AnnouncementRequest, right: &AnnouncementRequest) -> bool {
    left.company == right.company
        && left.occurred_on == right.occurred_on
        && left.published_at == right.published_at
        && left.content == right.content
}
fn same_announcement(left: &Announcement, right: &AnnouncementRequest) -> bool {
    left.company == right.company
        && left.occurred_on == right.occurred_on
        && left.published_at == right.published_at
        && left.content == right.content
}

fn collect_announcements<'a>(
    source: DisclosureSourceRef<'a>,
    actions: Option<&SessionCorporateActions>,
    settled: CivilDate,
    pending: &mut Vec<Pending<'a>>,
) -> Result<(), DisclosurePrepareError> {
    let published_at = CivilInstant::from_hms(settled, 18, 0, 0).map_err(InformationError::from)?;
    if let DisclosureSourceRef::Legacy { operations, .. } = source {
        for (company, operating) in &operations.companies {
            for shock in operating.economy().active() {
                if shock.starts_on == settled && shock.kind.applies_to(operating.spec().kind) {
                    pending.push(Pending::Announcement(AnnouncementRequest {
                        company: *company,
                        occurred_on: settled,
                        published_at,
                        content: AnnouncementContent::Shock(AnnouncedEvent::from_active(shock)),
                    }));
                }
            }
        }
        for failure in operations.payment_failures_on(settled) {
            source.spec(failure.company)?;
            pending.push(Pending::Announcement(AnnouncementRequest {
                company: failure.company,
                occurred_on: settled,
                published_at,
                content: AnnouncementContent::Shock(AnnouncedEvent {
                    kind: crate::company::ShockKind::PaymentFailure {
                        what: failure.what.clone(),
                        amount: failure.amount,
                        obligation_status: failure.obligation_status,
                    },
                    amplitude_bp: 0,
                    starts_on: settled,
                    expires_on: settled,
                }),
            }));
        }
    }
    let Some(actions) = actions else {
        return Ok(());
    };
    let mut append = |company: CompanyId,
                      stock: &crate::account::StockCode,
                      content: AnnouncementContent|
     -> Result<(), DisclosurePrepareError> {
        if source.spec(company)?.listed_stock.as_ref() != Some(stock) {
            return Err(invalid("公告的 issuer／stock 与所选源映射不一致"));
        }
        pending.push(Pending::Announcement(AnnouncementRequest {
            company,
            occurred_on: settled,
            published_at,
            content,
        }));
        Ok(())
    };
    for book in &actions.dividends {
        let plan = book.plan();
        if plan.announced_on != settled {
            continue;
        }
        let DisclosureSourceRef::Simple(system) = source else {
            return Err(DisclosurePrepareError::UnsupportedAnnouncement {
                company: plan.issuer,
                kind: "CashDividend",
            });
        };
        let facts = system.dividend_plan_facts(&plan.issuer)?;
        let fact = facts
            .iter()
            .find(|fact| fact.plan_id == plan.plan_id)
            .ok_or_else(|| invalid("现金分红公告没有同源批准财务事实"))?;
        let total_gross = fact
            .total_gross
            .to_money()
            .map_err(InformationError::from)?;
        if fact.approved_on != plan.approved_on || total_gross > plan.distributable_amount {
            return Err(invalid("现金分红公告批准日／授权上限与同源财务事实不一致"));
        }
        // 未登记时真实 Book／finance fact 承载已批准方案；不以公告日名册倒推批准日资格。
        if book.registration().is_some() && book.total_gross()? != total_gross {
            return Err(invalid("现金分红已冻结登记总额与同源批准财务事实不一致"));
        }
        append(
            plan.issuer,
            &plan.stock,
            AnnouncementContent::CashDividend(CashDividendAnnouncement {
                plan: plan.clone(),
                total_gross,
            }),
        )?;
    }
    for book in &actions.rights_offerings {
        let plan = book.plan();
        if plan.announced_on == settled {
            append(
                plan.issuer,
                &plan.stock,
                AnnouncementContent::RightsOffering(RightsOfferingAnnouncement {
                    plan: plan.clone(),
                }),
            )?;
        }
    }
    for book in &actions.issuer_repurchases {
        let plan = book.plan();
        if plan.announced_on == settled {
            append(
                plan.issuer,
                &plan.stock,
                AnnouncementContent::IssuerRepurchase(IssuerRepurchaseAnnouncement {
                    plan: plan.clone(),
                }),
            )?;
        }
    }
    for book in &actions.share_splits {
        let plan = book.plan();
        if plan.announced_on == settled {
            append(
                plan.issuer,
                &plan.stock,
                AnnouncementContent::ShareSplit(plan.clone()),
            )?;
        }
    }
    for book in &actions.stock_distributions {
        let plan = book.plan();
        if plan.announced_on == settled {
            append(
                plan.issuer,
                &plan.stock,
                AnnouncementContent::StockDistribution(plan.clone()),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "disclosure_candidate_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "disclosure_batch_candidate_tests.rs"]
mod batch_tests;
