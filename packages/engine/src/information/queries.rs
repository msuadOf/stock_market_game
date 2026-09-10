//! 公开库查询面（K4，任务 15）：按 id / 公司 / 期间读取已公开版本。
//!
//! 读取语义：`as_of` 是查询方的当前 civil 时点——早于公布时点的读取 =
//! [`InformationError::EarlyRead`]（NPC/宿主不可读未公开信息，K4 明文）；
//! 公司面查询按 `as_of` 过滤后返回，不报错（「该时点可见什么」语义）。

use std::collections::BTreeSet;

use crate::accounting::reports::ReportKind;
use crate::accounting::AccountingPeriod;
use crate::calendar::CivilInstant;
use crate::company::CompanyId;
use crate::information::{
    Announcement, InformationError, PublicLibrary, PublicationId, PublishedReport,
};

impl PublicLibrary {
    /// 按 id 读取报告（查询时点早于公布时点 = `EarlyRead`）。
    pub fn report(
        &self,
        id: PublicationId,
        as_of: CivilInstant,
    ) -> Result<&PublishedReport, InformationError> {
        let report = self
            .reports
            .get(&id)
            .ok_or(InformationError::UnknownPublication { id })?;
        if as_of < report.published_at {
            return Err(InformationError::EarlyRead {
                id,
                published_at: report.published_at,
                as_of,
            });
        }
        Ok(report)
    }

    /// 按 id 读取公告（提前读取同样拒绝）。
    pub fn announcement(
        &self,
        id: PublicationId,
        as_of: CivilInstant,
    ) -> Result<&Announcement, InformationError> {
        let announcement = self
            .announcements
            .get(&id)
            .ok_or(InformationError::UnknownPublication { id })?;
        if as_of < announcement.published_at {
            return Err(InformationError::EarlyRead {
                id,
                published_at: announcement.published_at,
                as_of,
            });
        }
        Ok(announcement)
    }

    /// 该公司该期间该种类的最新已公布版本（as_of 过滤，无则 `None`；
    /// 更正后新版本 id 更大 ⇒ 天然「最新」）。
    pub fn latest_report(
        &self,
        company: &CompanyId,
        kind: ReportKind,
        period: AccountingPeriod,
        as_of: CivilInstant,
    ) -> Option<&PublishedReport> {
        self.reports_for_company(company, as_of)
            .into_iter()
            .filter(|report| report.reports.kind == kind && report.reports.period == period)
            .max_by_key(|report| report.id)
    }

    /// 该公司截至 as_of 的全部已公布报告（公布时点序）。
    pub fn reports_for_company(
        &self,
        company: &CompanyId,
        as_of: CivilInstant,
    ) -> Vec<&PublishedReport> {
        self.ids_for_company(company)
            .iter()
            .filter_map(|id| self.reports.get(id))
            .filter(|report| report.published_at <= as_of)
            .collect()
    }

    /// 该公司截至 as_of 的全部已公布公告。
    pub fn announcements_for_company(
        &self,
        company: &CompanyId,
        as_of: CivilInstant,
    ) -> Vec<&Announcement> {
        self.ids_for_company(company)
            .iter()
            .filter_map(|id| self.announcements.get(id))
            .filter(|announcement| announcement.published_at <= as_of)
            .collect()
    }

    /// 全部公布 id（id 序；确定性断言/遍历用）。
    pub fn publication_ids(&self) -> Vec<PublicationId> {
        self.reports.keys().copied().collect()
    }

    pub fn report_count(&self) -> usize {
        self.reports.len()
    }

    pub fn announcement_count(&self) -> usize {
        self.announcements.len()
    }

    pub(super) fn ids_for_company(&self, company: &CompanyId) -> &BTreeSet<PublicationId> {
        static EMPTY: BTreeSet<PublicationId> = BTreeSet::new();
        self.by_company.get(company).unwrap_or(&EMPTY)
    }
}
