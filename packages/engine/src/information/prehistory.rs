//! 开局已公开集合装配（K4，任务 15）：真实历史排期组装。
//!
//! 调用任务 14 前史生成（`generate_history`）+ 任务 13 报表构建，按各公司
//! 真实历史排期逐期公布——只有公布时点**早于开局日 00:00** 的报告进入已
//! 公开集合（`SeededPrehistory` 标记）；开局未来才公布的报告不提前纳入，
//! 由 live 派发（session/disclosures.rs）在其 18:00 相位公布。处理顺序 =
//! (公布时点, 公司, 种类) 全局确定序 ⇒ `PublicationId` 同 seed 逐位一致。
//!
//! 版本登记策略：季报/半年报走 `snapshot_interim`（不封账的定稿快照）；
//! 年报走 `generate_report_set` + `record()`（外部版本登记——行业账套不
//! 暴露 `&mut Books`，`close_year` 结构性不可达；登记版本同样经勾稽 +
//! 比较项诚实性校验且不可变。封账接线归任务 26，issues 已登记）。

use std::collections::BTreeMap;

use crate::accounting::closing::{verify_comparative_honesty, ClosingEngine};
use crate::accounting::consolidation::{MemberId, ScopeId};
use crate::accounting::reports::validate::prior_year_facts;
use crate::accounting::reports::{
    generate_report_set, IndustryPresentation, ReportKind, ReportRequest, ReportSource,
    ReportVersion, VersionKind,
};
use crate::accounting::{AccountingPeriod, Books, BusinessEventId};
use crate::calendar::{CivilDate, CivilInstant};
use crate::company::operations::{generate_history, CompanyOperations};
use crate::company::{CompanyId, CompanyKind};
use crate::information::schedule::{scheduled_instant, stable_company_offset, ScheduledReportKind};
use crate::information::{
    AccountingPolicyRef, InformationError, PublicLibrary, PublicationOrigin, PublicationRequest,
    APPROVAL_HOUR,
};

/// 共享空重述映射（无更正的生成请求）。
static NO_ADJUSTMENTS: BTreeMap<BusinessEventId, AccountingPeriod> = BTreeMap::new();

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
) -> Result<(), InformationError> {
    let scope = ScopeId::Standalone(member.clone());
    if !closing.versions(&scope, period, kind).is_empty() {
        return Ok(());
    }
    match kind {
        ReportKind::Quarter | ReportKind::HalfYear => {
            closing
                .snapshot_interim(books, member, industry, period, kind)
                .map_err(|err| InformationError::Closing(Box::new(err)))?;
        }
        ReportKind::Monthly | ReportKind::Annual => {
            let version = ReportVersion {
                sequence: 1,
                supersedes: None,
                kind: VersionKind::Original,
            };
            let set = generate_report_set(ReportRequest {
                period,
                kind,
                source: ReportSource::Standalone {
                    id: member.clone(),
                    books,
                    industry,
                },
                version,
                adjustments: &NO_ADJUSTMENTS,
            })
            .map_err(|err| InformationError::ReportNotPublishable(Box::new(err)))?;
            let (has_flows, has_end) = prior_year_facts(books, period);
            verify_comparative_honesty(&set, has_flows, has_end)
                .map_err(|err| InformationError::ReportNotPublishable(Box::new(err)))?;
            closing
                .record(set)
                .map_err(|err| InformationError::Closing(Box::new(err)))?;
        }
    }
    Ok(())
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

/// 开局已公开集合装配：真实历史排期（任务 14 前史 + 任务 13 报表）。
pub fn assemble_seeded_prehistory(
    config: crate::company::operations::CompanyOperationsConfig,
    game_start: CivilDate,
) -> Result<SeededPrehistory, InformationError> {
    let seed = config.seed;
    let ops = generate_history(config, game_start)
        .map_err(|err| InformationError::Operations(Box::new(err)))?;
    let mut closing = ClosingEngine::new();
    let mut library = PublicLibrary::new();
    // 前史首日（generate_history 同式一行孪生：开局年 − 2 的 1 月 1 日）。
    let history_start = CivilDate::from_ymd(game_start.year() - 2, 1, 1)?;
    let start_instant = CivilInstant::new(game_start, 0)?;
    let mut due: Vec<(CivilInstant, CompanyId, ScheduledReportKind, i32)> = Vec::new();
    for id in ops.companies.keys() {
        let offset = stable_company_offset(seed, id);
        for fiscal_year in history_start.year()..=game_start.year() {
            for kind in ScheduledReportKind::ALL {
                let instant = scheduled_instant(kind, fiscal_year, offset)?;
                if instant < start_instant {
                    due.push((instant, id.clone(), kind, fiscal_year));
                }
            }
        }
    }
    due.sort();
    let mut last_published = None;
    for (instant, company_id, kind, fiscal_year) in due {
        let offset = stable_company_offset(seed, &company_id);
        let member = MemberId(company_id.0.clone());
        let (books, industry) = {
            let company = ops.company(&company_id).expect("collected from ops");
            (
                company.books().books(),
                industry_presentation(company.spec().kind),
            )
        };
        let period = kind.landing_period(fiscal_year)?;
        ensure_original_registered(
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
        let approval = CivilInstant::from_hms(instant.date(), APPROVAL_HOUR, 0, 0)?;
        library.publish_closed(
            &closing,
            PublicationRequest {
                company: company_id,
                scope: ScopeId::Standalone(member),
                period,
                kind: kind.report_kind(),
                sequence: 1,
                policy,
                approved_at: approval,
                published_at: instant,
                origin: PublicationOrigin::SeededPrehistory {
                    fiscal_year,
                    kind,
                    offset_days: offset,
                },
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
