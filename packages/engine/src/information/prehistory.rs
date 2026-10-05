//! 开局已公开集合装配：按历史披露排期组装。
//!
//! 调用公司经营前史生成（`generate_history`）与报表构建，按各公司
//! 真实历史排期逐期公布——只有公布时点**早于开局日 00:00** 的报告进入已
//! 公开集合（`SeededPrehistory` 标记）；开局未来才公布的报告不提前纳入，
//! 由 live 派发（session/disclosures.rs）在其 18:00 相位公布。处理顺序 =
//! (公布时点, 公司, 种类) 全局确定序 ⇒ `PublicationId` 同 seed 逐位一致。
//!
//! 版本登记由真实窗口的 `generate_report_set` + `record()` 完成，并校验勾稽与
//! 比较项诚实性。季报／半年报不补假封账，月报必须已有封月版本；历史更正改变
//! 尚未公开原始报告时追加私有 Original 版本，不改写旧登记版，公开使用实际序号。

use crate::accounting::closing::{verify_comparative_honesty, ClosingEngine};
use crate::accounting::consolidation::{MemberId, ScopeId};
use crate::accounting::reports::validate::prior_year_facts;
use crate::accounting::reports::{
    generate_report_set, IndustryPresentation, ReportKind, ReportRequest, ReportSource,
    ReportVersion, VersionKind,
};
use crate::accounting::{AccountingPeriod, Books};
use crate::calendar::{CivilDate, CivilInstant};
use crate::company::operations::{generate_history, CompanyOperations};
use crate::company::{CompanyId, CompanyKind};
use crate::information::schedule::ScheduledReportKind;
use crate::information::{
    AccountingPolicyRef, InformationError, PublicLibrary, PublicationRequest,
};

/// 公司会计类型 → 行业列报口径（等价于按科目表版本推导的映射）。
pub fn industry_presentation(kind: CompanyKind) -> IndustryPresentation {
    match kind {
        CompanyKind::Industrial => IndustryPresentation::Industrial,
        CompanyKind::Bank => IndustryPresentation::Bank,
        CompanyKind::Insurance => IndustryPresentation::Insurance,
        CompanyKind::RealEstate => IndustryPresentation::RealEstate,
    }
}

/// 确保结账登记簿中存在该 (scope, 期间, 种类) 的原始版本（sequence 1）。
pub fn ensure_original_registered(
    closing: &mut ClosingEngine,
    books: &Books,
    member: &MemberId,
    industry: IndustryPresentation,
    period: AccountingPeriod,
    kind: ReportKind,
) -> Result<u32, InformationError> {
    let scope = ScopeId::Standalone(member.clone());
    let existing = closing.versions(&scope, period, kind).last().cloned();
    if kind == ReportKind::Monthly
        && (existing.is_none()
            || books.journal().period_status(period) != crate::accounting::PeriodStatus::Closed)
    {
        return Err(InformationError::InconsistentLibrary {
            detail: format!("monthly report for {scope:?} {period:?} has not been closed"),
        });
    }
    let adjustments = closing
        .restatement_periods(&scope)
        .cloned()
        .unwrap_or_default();
    let version = existing
        .as_ref()
        .map(|set| set.version.clone())
        .unwrap_or(ReportVersion {
            sequence: 1,
            supersedes: None,
            kind: VersionKind::Original,
        });
    let mut set = generate_report_set(ReportRequest {
        period,
        kind,
        source: ReportSource::Standalone {
            id: member.clone(),
            books,
            industry,
        },
        version,
        adjustments: &adjustments,
    })
    .map_err(|err| InformationError::ReportNotPublishable(Box::new(err)))?;
    if let Some(previous) = &existing {
        if set == *previous {
            return Ok(previous.version.sequence);
        }
        set.version = ReportVersion {
            sequence: previous.version.sequence.checked_add(1).ok_or_else(|| {
                InformationError::InconsistentLibrary {
                    detail: format!(
                        "original report version space exhausted for {scope:?} {period:?}"
                    ),
                }
            })?,
            supersedes: None,
            kind: VersionKind::Original,
        };
    }
    set.validate()
        .map_err(|err| InformationError::ReportNotPublishable(Box::new(err)))?;
    let (has_flows, has_end) = prior_year_facts(books, period);
    verify_comparative_honesty(&set, has_flows, has_end)
        .map_err(|err| InformationError::ReportNotPublishable(Box::new(err)))?;
    let handle = closing
        .record(set)
        .map_err(|err| InformationError::Closing(Box::new(err)))?;
    Ok(handle.sequence)
}

/// 开局前史装配结果：推进到开局的经营编排 + 版本登记簿 + 已播种公开库。
pub struct SeededPrehistory {
    pub ops: CompanyOperations,
    pub closing: ClosingEngine,
    pub library: PublicLibrary,
    last_published: Option<CivilInstant>,
}

impl SeededPrehistory {
    /// 已播种的最后公布时点（live 派发游标初值）。
    pub fn last_published_instant(&self) -> Option<CivilInstant> {
        self.last_published
    }
}

/// 开局已公开集合装配：经营前史与报表按历史披露排期组合。
pub fn assemble_seeded_prehistory(
    config: crate::company::operations::CompanyOperationsConfig,
    game_start: CivilDate,
    report_frequency: crate::information::ReportFrequency,
) -> Result<SeededPrehistory, InformationError> {
    let seed = config.seed;
    let mut ops = generate_history(config, game_start)
        .map_err(|err| InformationError::Operations(Box::new(err)))?;
    let mut closing = ClosingEngine::new();
    let mut library = PublicLibrary::new();
    if matches!(
        report_frequency,
        crate::information::ReportFrequency::Monthly { .. }
    ) {
        for company in ops.companies.values_mut() {
            let member = MemberId(company.spec().id.0.clone());
            let industry = industry_presentation(company.spec().kind);
            for year in game_start.year() - 2..=game_start.year() {
                for month in 1..=12 {
                    let period = AccountingPeriod::from_ymd(year, month)?;
                    if super::publication::period_end_date(period)? >= game_start {
                        continue;
                    }
                    closing
                        .close_month(company.books_mut(), &member, industry, period)
                        .map_err(|error| InformationError::Closing(Box::new(error)))?;
                }
            }
        }
    }
    // 前史首日（generate_history 同式一行孪生：开局年 − 2 的 1 月 1 日）。
    let history_start = CivilDate::from_ymd(game_start.year() - 2, 1, 1)?;
    let start_instant = CivilInstant::new(game_start, 0)?;
    let mut due: Vec<(CivilInstant, CompanyId, ScheduledReportKind, i32)> = Vec::new();
    for id in ops.companies.keys() {
        for fiscal_year in history_start.year()..=game_start.year() {
            for kind in report_frequency.scheduled_kinds() {
                let instant = report_frequency.scheduled_instant(kind, fiscal_year, seed, id)?;
                if instant < start_instant {
                    due.push((instant, id.clone(), kind, fiscal_year));
                }
            }
        }
    }
    due.sort();
    let mut last_published = None;
    for (instant, company_id, kind, fiscal_year) in due {
        let member = MemberId(company_id.0.clone());
        let (books, industry) = {
            let company = ops.company(&company_id).expect("collected from ops");
            (
                company.books().books(),
                industry_presentation(company.spec().kind),
            )
        };
        let period = kind.landing_period(fiscal_year)?;
        let sequence = ensure_original_registered(
            &mut closing,
            books,
            &member,
            industry,
            period,
            kind.report_kind(),
        )?;
        let policy = AccountingPolicyRef {
            chart_version: books.ledger().chart().version(),
        };
        let approval = report_frequency.approval_instant(kind, instant)?;
        let origin =
            report_frequency.publication_origin(kind, fiscal_year, seed, &company_id, true)?;
        library.publish_closed(
            &closing,
            PublicationRequest {
                company: company_id,
                scope: ScopeId::Standalone(member),
                period,
                kind: kind.report_kind(),
                sequence,
                policy,
                approved_at: approval,
                published_at: instant,
                origin,
                supersedes: None,
            },
        )?;
        last_published = Some(instant);
    }
    // generate_history 已把经营推进到开局（next_expected = game_start），
    // 此处不再前进；宿主从开局日起逐日 advance。
    Ok(SeededPrehistory {
        ops,
        closing,
        library,
        last_published,
    })
}
