//! 在自然日经营终局后派发 18:00 披露。
//!
//! GameSession 先执行 `ops_wiring.run_day_end`，月末或年末通过行业可变账套
//! 完成 `close_month`/`close_year`，再将同一份 [`CivilDayEndReport`] 传入
//! [`DisclosureDispatch::run_day_end`]。定期报告须经登记簿校验、定稿后公布。
//!
//! 18:00 观察者使用无捕获的 `fn(CivilInstant)`；权威相位瞬间由
//! [`CivilDayEndReport::disclosure_instant`] 承载。生产 hook
//! [`disclosure_phase_observer`] 只观察时点，有状态派发由 `run_day_end` 执行。
//! 观察者不得 panic，以保持日结失败的原子边界。

use crate::accounting::closing::ClosingEngine;
use crate::calendar::{CivilDate, CivilDateError, CivilInstant};
use crate::company::CompanyId;
use crate::company::operations::CompanyOperations;

use crate::information::{
    AccountingPolicyRef, AnnouncedEvent, AnnouncementContent, AnnouncementRequest,
    InformationError, PublicLibrary, PublicationId, PublicationRequest, ScheduledReportKind,
    ensure_original_registered, industry_presentation,
};
use crate::session::civil_clock::{CivilClock, CivilDayEndReport};
use thiserror::Error;

/// 生产 18:00 披露相位观察者（fn 指针；无捕获、无状态、panic-free）。
pub fn disclosure_phase_observer(_instant: CivilInstant) {}

/// 披露接线错误（类型化透传）。
#[derive(Debug, Error)]
pub enum DisclosureError {
    #[error(transparent)]
    Information(#[from] InformationError),
    #[error("civil date error: {0}")]
    Date(#[from] CivilDateError),
}

/// 一次日终披露的结果（确定性顺序：公告先于定期报告；公司 id 序）。
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct DayEndDisclosures {
    pub announcements_published: Vec<PublicationId>,
    pub reports_published: Vec<PublicationId>,
}

/// 日终披露上下文（参数组——closing.rs `StandaloneTarget` 先例）。
pub struct DayEndDisclosureCtx<'a> {
    pub report_frequency: crate::information::ReportFrequency,
    pub groups: &'a [super::company_groups::GroupStructure],
    pub report: &'a CivilDayEndReport,
    /// 已完成当日 finalize 的经营编排（只读）。
    pub ops: &'a CompanyOperations,
    pub closing: &'a mut ClosingEngine,
    pub library: &'a mut PublicLibrary,
}

pub(super) struct SimpleDayEndDisclosureCtx<'a> {
    pub report_frequency: crate::information::ReportFrequency,
    pub report: &'a CivilDayEndReport,
    pub system: &'a crate::company::CompanySystem,
    pub seed: u64,
    pub dividends: &'a [crate::company::cash_dividend::CashDividendBook],
    pub rights_offerings:
        &'a [crate::company::rights_offering::RightsOfferingBook],
    pub issuer_repurchases:
        &'a [crate::company::issuer_repurchase::IssuerRepurchaseBook],
    pub share_splits: &'a [crate::company::share_split::ShareSplitBook],
    pub stock_distributions: &'a [crate::company::stock_distribution::StockDistributionBook],
    pub library: &'a mut PublicLibrary,
}

pub(super) struct SimpleScheduledDisclosureCtx<'a> {
    pub report_frequency: crate::information::ReportFrequency,
    pub through: CivilInstant,
    pub system: &'a crate::company::CompanySystem,
    pub seed: u64,
    pub library: &'a mut PublicLibrary,
}

/// 披露派发器：排期游标 + 公告恰好一次游标（serde 随存档冻结）。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct DisclosureDispatch {
    /// 已派发到的最后一个排期时点（含）；`None` = 尚未派发。
    published_through: Option<CivilInstant>,
    /// 已派发公告的最后一个发生日（含）；`None` = 尚未派发。
    announced_through: Option<CivilDate>,
}

impl DisclosureDispatch {
    pub(super) fn run_simple_day_end(
        &mut self,
        ctx: SimpleDayEndDisclosureCtx<'_>,
    ) -> Result<DayEndDisclosures, DisclosureError> {
        let through = if matches!(
            ctx.report_frequency,
            crate::information::ReportFrequency::Monthly { .. }
        ) {
            CivilInstant::new(ctx.report.settled_date, 86399)?
        } else {
            ctx.report.disclosure_instant
        };
        let reports_published = self.run_simple_scheduled(SimpleScheduledDisclosureCtx {
            report_frequency: ctx.report_frequency,
            through,
            system: ctx.system,
            seed: ctx.seed,
            library: ctx.library,
        })?;
        let mut announcements_published = Vec::new();
        for book in ctx.dividends {
            if book.status() == &crate::company::cash_dividend::CashDividendStatus::Announced
                && book.plan().announced_on == ctx.report.settled_date
            {
                let finance_plan = ctx
                    .system
                    .dividend_plan_facts(&book.plan().issuer)
                    .map_err(|error| InformationError::InconsistentLibrary {
                        detail: format!(
                            "cash dividend {} approved amount unavailable: {error}",
                            book.plan().plan_id
                        ),
                    })?
                    .into_iter()
                    .find(|fact| fact.plan_id == book.plan().plan_id)
                    .ok_or_else(|| InformationError::InconsistentLibrary {
                        detail: format!(
                            "cash dividend {} has no matching approved finance fact",
                            book.plan().plan_id
                        ),
                    })?;
                let total_gross = finance_plan.total_gross.to_money().map_err(|error| {
                    InformationError::InconsistentLibrary {
                        detail: format!(
                            "cash dividend {} approved amount cannot be represented as Money: {error}",
                            book.plan().plan_id
                        ),
                    }
                })?;
                let id = ctx.library.publish_announcement(AnnouncementRequest {
                    company: book.plan().issuer.clone(),
                    occurred_on: ctx.report.settled_date,
                    published_at: ctx.report.disclosure_instant,
                    content: AnnouncementContent::CashDividend(
                        crate::information::CashDividendAnnouncement {
                            plan: book.plan().clone(),
                            total_gross,
                        },
                    ),
                })?;
                announcements_published.push(id);
            }
        }
        // 配股／增发与回购方案公告：复用现金分红公告通道（同日 18:00 相位、
        // 恰好一次语义；NPC 经公开库按既有注意力规则获知）。
        for book in ctx.rights_offerings {
            if book.status() == &crate::company::rights_offering::RightsOfferingStatus::Announced
                && book.plan().announced_on == ctx.report.settled_date
            {
                let id = ctx.library.publish_announcement(AnnouncementRequest {
                    company: book.plan().issuer.clone(),
                    occurred_on: ctx.report.settled_date,
                    published_at: ctx.report.disclosure_instant,
                    content: AnnouncementContent::RightsOffering(
                        crate::information::RightsOfferingAnnouncement {
                            plan: book.plan().clone(),
                        },
                    ),
                })?;
                announcements_published.push(id);
            }
        }
        for book in ctx.issuer_repurchases {
            if book.status()
                == &crate::company::issuer_repurchase::IssuerRepurchaseStatus::Announced
                && book.plan().announced_on == ctx.report.settled_date
            {
                let id = ctx.library.publish_announcement(AnnouncementRequest {
                    company: book.plan().issuer.clone(),
                    occurred_on: ctx.report.settled_date,
                    published_at: ctx.report.disclosure_instant,
                    content: AnnouncementContent::IssuerRepurchase(
                        crate::information::IssuerRepurchaseAnnouncement {
                            plan: book.plan().clone(),
                        },
                    ),
                })?;
                announcements_published.push(id);
            }
        }
        // 拆股／缩股方案公告：复用同一公告通道（同日 18:00 相位、恰好一次）。
        for book in ctx.share_splits {
            if book.status()
                == &crate::company::share_split::ShareSplitStatus::Announced
                && book.plan().announced_on == ctx.report.settled_date
            {
                let id = ctx.library.publish_announcement(AnnouncementRequest {
                    company: book.plan().issuer.clone(),
                    occurred_on: ctx.report.settled_date,
                    published_at: ctx.report.disclosure_instant,
                    content: AnnouncementContent::ShareSplit(book.plan().clone()),
                })?;
                announcements_published.push(id);
            }
        }
        // 送转（股票股利与资本公积转增）方案公告：复用同一公告通道（同日 18:00
        // 相位、恰好一次；`announce` 状态推进发生在日结更早的公司行为阶段，
        // 此处只对 Announced 且当日到期的方案公开发布）。
        for book in ctx.stock_distributions {
            if book.status()
                == &crate::company::stock_distribution::StockDistributionStatus::Announced
                && book.plan().announced_on == ctx.report.settled_date
            {
                let id = ctx.library.publish_announcement(AnnouncementRequest {
                    company: book.plan().issuer.clone(),
                    occurred_on: ctx.report.settled_date,
                    published_at: ctx.report.disclosure_instant,
                    content: AnnouncementContent::StockDistribution(book.plan().clone()),
                })?;
                announcements_published.push(id);
            }
        }
        self.announced_through = Some(ctx.report.settled_date);
        Ok(DayEndDisclosures {
            announcements_published,
            reports_published,
        })
    }

    pub(super) fn run_simple_scheduled(
        &mut self,
        ctx: SimpleScheduledDisclosureCtx<'_>,
    ) -> Result<Vec<PublicationId>, DisclosureError> {
        let publications = crate::information::publish_simple_scheduled(
            ctx.system,
            ctx.report_frequency,
            ctx.seed,
            self.published_through,
            ctx.through,
            ctx.library,
            false,
        )?;
        self.published_through = Some(ctx.through);
        Ok(publications)
    }
    /// 以前史播种的最后公布时点为游标初值（未播种 = `None`，首次派发
    /// 衔接补账窗口）。
    pub fn new(published_through: Option<CivilInstant>) -> Self {
        Self {
            published_through,
            announced_through: None,
        }
    }

    /// 已派发游标（诊断/测试）。
    pub fn published_through(&self) -> Option<CivilInstant> {
        self.published_through
    }

    /// 已派发公告游标（诊断/测试 + 恢复边界校验面）。
    pub fn announced_through(&self) -> Option<CivilDate> {
        self.announced_through
    }

    /// 安装生产 18:00 相位观察者（替换空生产列表；返回观察者数）。
    pub fn install(&self, clock: &mut CivilClock) -> usize {
        clock.add_disclosure_observer(disclosure_phase_observer);
        1
    }

    /// 18:00 披露派发（恰好一次语义：同日重复调用 = no-op）。
    ///
    /// 1. 临时公告：经营目录中 starts_on == settled_date 的当日新激活事件，
    ///    按公司 id 序公布；内容只含已确认事件条款。
    /// 2. 定期披露：游标窗口 `(published_through, disclosure_instant]`
    ///    内的全部排期（公司 id × 种类确定序），经登记簿定稿后公布。
    pub fn run_day_end(
        &mut self,
        ctx: DayEndDisclosureCtx<'_>,
    ) -> Result<DayEndDisclosures, DisclosureError> {
        let mut out = DayEndDisclosures::default();
        let settled = ctx.report.settled_date;
        let phase = ctx.report.disclosure_instant;

        // 1) 临时公告（当日新激活事件；重复派发由游标挡住）。
        if self
            .announced_through
            .is_none_or(|through| settled > through)
        {
            for (id, company) in &ctx.ops.companies {
                for shock in company.economy().active() {
                    if shock.starts_on != settled || !shock.kind.applies_to(company.spec().kind) {
                        continue;
                    }
                    let announcement = ctx.library.publish_announcement(AnnouncementRequest {
                        company: id.clone(),
                        occurred_on: settled,
                        published_at: phase,
                        content: AnnouncementContent::Shock(AnnouncedEvent::from_active(shock)),
                    })?;
                    out.announcements_published.push(announcement);
                }
            }
            for failure in ctx.ops.payment_failures_on(settled) {
                let announcement = ctx.library.publish_announcement(AnnouncementRequest {
                    company: failure.company.clone(),
                    occurred_on: settled,
                    published_at: phase,
                    content: AnnouncementContent::Shock(AnnouncedEvent {
                        kind: crate::company::ShockKind::PaymentFailure {
                            obligation_status: failure.obligation_status,
                            what: failure.what.clone(),
                            amount: failure.amount,
                        },
                        amplitude_bp: 0,
                        starts_on: settled,
                        expires_on: settled,
                    }),
                })?;
                out.announcements_published.push(announcement);
            }
            self.announced_through = Some(settled);
        }

        let through = if matches!(
            ctx.report_frequency,
            crate::information::ReportFrequency::Monthly { .. }
        ) {
            CivilInstant::new(settled, 86399)?
        } else {
            phase
        };
        out.reports_published = self.run_scheduled(ScheduledDisclosureCtx {
            report_frequency: ctx.report_frequency,
            groups: ctx.groups,
            through,
            ops: ctx.ops,
            closing: ctx.closing,
            library: ctx.library,
        })?;

        Ok(out)
    }
    pub(super) fn run_scheduled(
        &mut self,
        mut ctx: ScheduledDisclosureCtx<'_>,
    ) -> Result<Vec<PublicationId>, DisclosureError> {
        let mut due = Vec::new();
        for id in ctx.ops.companies.keys() {
            for year in [ctx.through.date().year() - 1, ctx.through.date().year()] {
                for kind in ctx.report_frequency.scheduled_kinds() {
                    let instant =
                        ctx.report_frequency
                            .scheduled_instant(kind, year, ctx.ops.seed, id)?;
                    if instant <= ctx.through
                        && self
                            .published_through
                            .is_none_or(|through| instant > through)
                    {
                        due.push((instant, id.clone(), year, kind));
                    }
                }
            }
        }
        due.sort();
        let mut publications = Vec::new();
        for (instant, id, year, kind) in due {
            let company = ctx.ops.company(&id).expect("排期来自现存公司");
            publications.push(ctx.publish_scheduled(&id, company, instant, year, kind)?);
            for group in ctx.groups.iter().filter(|group| group.root == id) {
                publications.push(super::company_groups::publish_group_scheduled(
                    group,
                    ctx.ops,
                    ctx.closing,
                    ctx.library,
                    instant,
                    year,
                    kind,
                    ctx.report_frequency,
                )?);
            }
        }
        self.published_through = Some(ctx.through);
        Ok(publications)
    }
}

pub(super) struct ScheduledDisclosureCtx<'a> {
    pub report_frequency: crate::information::ReportFrequency,
    pub groups: &'a [super::company_groups::GroupStructure],
    pub through: CivilInstant,
    pub ops: &'a CompanyOperations,
    pub closing: &'a mut ClosingEngine,
    pub library: &'a mut PublicLibrary,
}

impl ScheduledDisclosureCtx<'_> {
    /// 登记并公布排期原始报告；更正后的私有原始版使用返回的实际 sequence。
    fn publish_scheduled(
        &mut self,
        company_id: &CompanyId,
        company: &crate::company::operations::OperatingCompany,
        instant: CivilInstant,
        fiscal_year: i32,
        kind: ScheduledReportKind,
    ) -> Result<PublicationId, DisclosureError> {
        let books = company.books().books();
        let industry = industry_presentation(company.spec().kind);
        let member = crate::accounting::consolidation::MemberId(company_id.0.clone());
        let period = kind.landing_period(fiscal_year)?;
        let sequence = ensure_original_registered(
            self.closing,
            books,
            &member,
            industry,
            period,
            kind.report_kind(),
        )?;
        let policy = AccountingPolicyRef {
            chart_version: books.ledger().chart().version(),
        };
        let approval = self.report_frequency.approval_instant(kind, instant)?;
        let publication = self.library.publish_closed(
            self.closing,
            PublicationRequest {
                company: company_id.clone(),
                scope: crate::accounting::consolidation::ScopeId::Standalone(member),
                period,
                kind: kind.report_kind(),
                sequence,
                policy,
                approved_at: approval,
                published_at: instant,
                origin: self.report_frequency.publication_origin(
                    kind,
                    fiscal_year,
                    self.ops.seed,
                    company_id,
                    false,
                )?,
                supersedes: None,
            },
        )?;
        Ok(publication)
    }
}
