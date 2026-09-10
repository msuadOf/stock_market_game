//! 披露接线（K4，任务 15）：18:00 观察者 hook + 日终披露派发。
//!
//! 日终顺序（K4）：**finalize 当日业务 →（月/年末封账，如到期）→ 18:00
//! 披露**。本模块承接最后一步：宿主（任务 26）在 `ops_wiring.run_day_end`
//! （经营终局）之后、以同一份 [`CivilDayEndReport`] 调用
//! [`DisclosureDispatch::run_day_end`]。月/年末封账（`close_month`/
//! `close_year`）需要行业账套的 `&mut Books` 面——当前结构性不可达
//!（任务 8–11 只暴露只读 `books()`），本任务以登记簿登记（勾稽 + 诚实性
//! 校验 + 不可变）承载「定稿可公开」，封账接线归任务 26（issues 已登记）。
//!
//! 18:00 观察者是 `fn(CivilInstant)` 裸函数指针（无捕获）：相位时点事实由
//! [`CivilDayEndReport::disclosure_instant`] 权威承载并传入本派发器；
//! [`disclosure_phase_observer`] 是生产相位钩子（宿主侧集成点，任务 28
//! 可在同列表追加自己的观察者），有状态派发在同一相位瞬间由
//! `run_day_end` 执行。观察者保持 panic-free（任务 5 复核 N2）。

use crate::accounting::closing::ClosingEngine;
use crate::calendar::{CivilDate, CivilDateError, CivilInstant};
use crate::company::operations::CompanyOperations;
use crate::company::CompanyId;

use crate::information::{
    ensure_original_registered, industry_presentation, scheduled_instant, stable_company_offset,
    AccountingPolicyRef, AnnouncedEvent, AnnouncementRequest, InformationError, PublicLibrary,
    PublicationId, PublicationOrigin, PublicationRequest, ScheduledReportKind, APPROVAL_HOUR,
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
    pub report: &'a CivilDayEndReport,
    /// 已完成当日 finalize 的经营编排（只读）。
    pub ops: &'a CompanyOperations,
    pub closing: &'a mut ClosingEngine,
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

    /// 安装生产 18:00 相位观察者（替换空生产列表；返回观察者数）。
    pub fn install(&self, clock: &mut CivilClock) -> usize {
        clock.add_disclosure_observer(disclosure_phase_observer);
        1
    }

    /// 18:00 披露派发（恰好一次语义：同日重复调用 = no-op）。
    ///
    /// 1. 临时公告：当日新激活的经济事件（任务 14 目录，`starts_on ==
    ///    settled_date`）按公司 id 序公布——内容只含已确认事件条款；
    /// 2. 定期披露：游标窗口 `(published_through, disclosure_instant]`
    ///    内的全部排期（公司 id × 种类确定序），经登记簿定稿后公布。
    pub fn run_day_end(
        &mut self,
        mut ctx: DayEndDisclosureCtx<'_>,
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
                    if shock.starts_on != settled {
                        continue;
                    }
                    let announcement = ctx.library.publish_announcement(AnnouncementRequest {
                        company: id.clone(),
                        occurred_on: settled,
                        published_at: phase,
                        event: AnnouncedEvent::from_active(shock),
                    })?;
                    out.announcements_published.push(announcement);
                }
            }
            self.announced_through = Some(settled);
        }

        // 2) 定期披露（窗口内的排期；公司 id 序 ⇒ PublicationId 确定）。
        for (id, company) in &ctx.ops.companies {
            let offset = stable_company_offset(ctx.ops.seed, id);
            for fiscal_year in [settled.year() - 1, settled.year()] {
                for kind in ScheduledReportKind::ALL {
                    let instant = scheduled_instant(kind, fiscal_year, offset)?;
                    if self
                        .published_through
                        .is_some_and(|through| instant <= through)
                    {
                        continue;
                    }
                    if instant > phase {
                        continue;
                    }
                    let publication =
                        ctx.publish_scheduled(id, company, instant, fiscal_year, kind, offset)?;
                    out.reports_published.push(publication);
                }
            }
        }
        self.published_through = Some(phase);
        Ok(out)
    }
}

impl DayEndDisclosureCtx<'_> {
    /// 登记并公布一条排期披露（原始版本 sequence 1；已存在则复用）。
    fn publish_scheduled(
        &mut self,
        company_id: &CompanyId,
        company: &crate::company::operations::OperatingCompany,
        instant: CivilInstant,
        fiscal_year: i32,
        kind: ScheduledReportKind,
        offset: u8,
    ) -> Result<PublicationId, DisclosureError> {
        let books = company.books().books();
        let industry = industry_presentation(company.spec().kind);
        let member = crate::accounting::consolidation::MemberId(company_id.0.clone());
        let period = kind.landing_period(fiscal_year)?;
        ensure_original_registered(
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
        let approval = CivilInstant::from_hms(instant.date(), APPROVAL_HOUR, 0, 0)?;
        let publication = self.library.publish_closed(
            self.closing,
            PublicationRequest {
                company: company_id.clone(),
                scope: crate::accounting::consolidation::ScopeId::Standalone(member),
                period,
                kind: kind.report_kind(),
                sequence: 1,
                policy,
                approved_at: approval,
                published_at: instant,
                origin: PublicationOrigin::ScheduledDisclosure {
                    fiscal_year,
                    kind,
                    offset_days: offset,
                },
                supersedes: None,
            },
        )?;
        Ok(publication)
    }
}
