//! 公开库查询面：按 id / 公司 / 期间读取已公开版本。
//!
//! 读取语义：`as_of` 是查询方的当前 civil 时点——早于公布时点的读取 =
//! [`InformationError::EarlyRead`]（NPC/宿主不可读未公开信息）；
//! 公司面查询按 `as_of` 过滤后返回，不报错（「该时点可见什么」语义）。

use std::collections::BTreeSet;

use crate::accounting::reports::ReportKind;
use crate::accounting::AccountingPeriod;
use crate::calendar::CivilInstant;
use crate::company::{
    CompanyId, PublicReportPage, PublicReportQuery, PublicReportSummary,
    DEFAULT_PUBLIC_REPORT_PAGE_SIZE, MAX_PUBLIC_REPORT_PAGE_SIZE,
};
use crate::information::{
    Announcement, InformationError, PublicLibrary, PublicationId, PublishedReport,
};

impl PublicLibrary {
    /// Host-safe report page ordered only by immutable publication ID.
    ///
    /// The opaque cursor is the decimal ID of the last report returned. It must refer to
    /// a visible report for the requested company; malformed, stale, or cross-company
    /// cursors are explicit errors rather than a silently reset page.
    pub fn query_public_reports(
        &self,
        query: &PublicReportQuery,
        as_of: CivilInstant,
    ) -> Result<PublicReportPage, InformationError> {
        let page_size = query.page_size.unwrap_or(DEFAULT_PUBLIC_REPORT_PAGE_SIZE);
        if page_size == 0 || page_size > MAX_PUBLIC_REPORT_PAGE_SIZE {
            return Err(InformationError::InvalidPublicReportPageSize {
                page_size,
                max: MAX_PUBLIC_REPORT_PAGE_SIZE,
            });
        }
        let company = CompanyId(query.company_id);
        let cursor = query.cursor;
        if let Some(cursor) = cursor {
            let id = PublicationId::new(cursor);
            let valid = self
                .reports
                .get(&id)
                .is_some_and(|report| report.company == company && report.published_at <= as_of);
            if !valid {
                return Err(InformationError::InvalidPublicReportCursor { cursor, company });
            }
        }
        // 分页先截取再生成摘要：take(page_size + 1) 既能检出 has_next，又避免为整页之外的报告构建 summary。
        let mut reports = self
            .reports_for_company(&company, as_of)
            .into_iter()
            .filter(|report| cursor.is_none_or(|id| report.id.value() > id))
            .take(usize::from(page_size) + 1)
            .map(PublicReportSummary::from)
            .collect::<Vec<_>>();
        let has_next = reports.len() > usize::from(page_size);
        if has_next {
            reports.truncate(usize::from(page_size));
        }
        let next_cursor = if has_next {
            reports.last().map(|report| report.id.clone())
        } else {
            None
        };
        Ok(PublicReportPage {
            reports,
            next_cursor,
        })
    }

    /// Host-safe report-by-ID access. The published-at guard prevents future disclosure.
    pub fn public_report_by_id(
        &self,
        id: String,
        as_of: CivilInstant,
    ) -> Result<PublicReportSummary, InformationError> {
        let value = id
            .parse::<u32>()
            .map_err(|_| InformationError::InvalidPublicReportCursor {
                cursor: 0,
                company: crate::company::CompanyId(u32::MAX),
            })?;
        self.report(PublicationId::new(value), as_of)
            .map(PublicReportSummary::from)
    }

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

    pub fn all_publication_ids(&self) -> Result<Vec<PublicationId>, InformationError> {
        let mut ids = BTreeSet::new();
        for id in self.reports.keys().chain(self.announcements.keys()) {
            if !ids.insert(*id) {
                return Err(InformationError::DuplicatePublicationId { id: *id });
            }
        }
        Ok(ids.into_iter().collect())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::consolidation::MemberId;
    use crate::accounting::reports::{
        generate_report_set, IndustryPresentation, ReportKind, ReportRequest, ReportSet,
        ReportSource, ReportVersion, VersionKind,
    };
    use crate::accounting::{
        AccountingAmount, Books, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry,
        JournalLine, LedgerAccountId, PostingSide,
    };
    use crate::calendar::CivilDate;
    use crate::company::CompanyId;
    use crate::information::{
        AccountingPolicyRef, PublicationOrigin, PublicLibrarySave, ScheduledReportKind,
    };
    use std::collections::BTreeMap;

    /// 最小合法年报 `ReportSet`（两行开局分录 → `generate_report_set`）。
    ///
    /// 分页边界与基准只关心条目数量与 id 序，报告内容允许同构——
    /// 这样库装配成本与条目数线性相关，且全部走生产校验路径（`from_parts`）。
    fn minimal_annual_set() -> ReportSet {
        let mut books = Books::new(crate::company::industrial::industrial_account_chart());
        books
            .post_batch(vec![JournalEntry {
                source: BusinessEventId::new(1),
                date: CivilDate::from_ymd(2029, 12, 31).expect("fixture 日期合法"),
                kind: BusinessKind::OpeningBalance,
                cash_flow: CashFlowClass::Financing,
                lines: vec![
                    JournalLine {
                        account: LedgerAccountId("1002".into()),
                        side: PostingSide::Debit,
                        amount: AccountingAmount::from_cents(10_000),
                    },
                    JournalLine {
                        account: LedgerAccountId("4001".into()),
                        side: PostingSide::Credit,
                        amount: AccountingAmount::from_cents(10_000),
                    },
                ],
            }])
            .expect("开局分录必须可过账");
        generate_report_set(ReportRequest {
            period: AccountingPeriod::from_ymd(2029, 12).expect("年末期间合法"),
            kind: ReportKind::Annual,
            source: ReportSource::Standalone {
                id: MemberId(0),
                books: &books,
                industry: IndustryPresentation::Industrial,
            },
            version: ReportVersion {
                sequence: 1,
                supersedes: None,
                kind: VersionKind::Original,
            },
            adjustments: &BTreeMap::new(),
        })
        .expect("最小年报集合必须可生成")
    }

    /// 同构报告：仅 id 不同，其余字段与合法年报一致（前史播种来源）。
    fn report_with_id(id: u32, set: &ReportSet) -> PublishedReport {
        let published_at =
            crate::information::scheduled_instant(ScheduledReportKind::Annual, 2029, 0)
                .expect("年报排期时点合法");
        PublishedReport {
            id: PublicationId::new(id),
            company: CompanyId(0),
            policy: AccountingPolicyRef { chart_version: 2 },
            approved_at: CivilInstant::from_hms(published_at.date(), 8, 0, 0)
                .expect("批准时点合法"),
            published_at,
            origin: PublicationOrigin::SeededPrehistory {
                fiscal_year: 2029,
                kind: ScheduledReportKind::Annual,
                offset_days: 0,
            },
            supersedes: None,
            reports: set.clone(),
        }
    }

    /// 装配 count 条同构报告库：首条走生产恢复路径（`from_parts` 全量校验，
    /// 证明夹具形状合法），其余在模块内直填内部索引。
    ///
    /// 直填原因：`from_parts` 对每条报告做形状校验 + 内容摘要全量序列化
    /// （实测 ~26ms/条，与被测的查询路径无关）；`query_public_reports`
    /// 只读 `reports`/`by_company`，不触碰 `content_digest`。
    fn library_with_reports(count: u32) -> PublicLibrary {
        assert!(count >= 1, "库至少含一条报告");
        let set = minimal_annual_set();
        let mut library = PublicLibrary::from_parts(PublicLibrarySave {
            next_seq: 1,
            reports: vec![report_with_id(0, &set)],
            announcements: Vec::new(),
        })
        .expect("首条同构报告必须能通过生产恢复校验");
        for id in 1..count {
            let report = report_with_id(id, &set);
            library
                .by_company
                .entry(report.company.clone())
                .or_default()
                .insert(report.id);
            library.reports.insert(report.id, report);
        }
        library.next_seq = count;
        library
    }

    /// 查询时点：全部报告均已公布（次日上午）。
    fn as_of_after_publication() -> CivilInstant {
        let published_at =
            crate::information::scheduled_instant(ScheduledReportKind::Annual, 2029, 0)
                .expect("年报排期时点合法");
        CivilInstant::new(published_at.date().next().expect("次日合法"), 0)
            .expect("次日零点合法")
    }

    fn page(library: &PublicLibrary, cursor: Option<u32>, page_size: u16) -> PublicReportPage {
        library
            .query_public_reports(
                &PublicReportQuery {
                    company_id: 0,
                    cursor,
                    page_size: Some(page_size),
                },
                as_of_after_publication(),
            )
            .expect("分页查询必须成功")
    }

    /// 恰满一页：has_next 判定不能把「正好取完」误报为还有下一页。
    #[test]
    fn page_size_equal_to_total_returns_full_page_without_next_cursor() {
        let library = library_with_reports(7);
        let full = page(&library, None, 7);
        assert_eq!(full.reports.len(), 7);
        assert_eq!(full.next_cursor, None);
    }

    /// 恰好多一条：首页带游标，尾页恰好一条且无游标（先截取后计数的边界）。
    #[test]
    fn one_beyond_page_size_walks_to_single_item_tail() {
        let library = library_with_reports(7);
        let first = page(&library, None, 6);
        assert_eq!(first.reports.len(), 6);
        let cursor = first
            .next_cursor
            .clone()
            .expect("余量 1 条 ⇒ 必须给出下一页游标")
            .parse::<u32>()
            .expect("游标是十进制 id");
        let tail = page(&library, Some(cursor), 6);
        assert_eq!(tail.reports.len(), 1);
        assert_eq!(tail.next_cursor, None);
        assert!(
            tail.reports[0]
                .id
                .parse::<u32>()
                .expect("报告 id 是十进制")
                > cursor
        );
    }

    /// 跳过一条后恰满一页：游标过滤与页大小合成下 has_next 仍须精确。
    #[test]
    fn cursor_leaving_exact_page_has_no_next_cursor() {
        let library = library_with_reports(7);
        let remaining = page(&library, Some(0), 6);
        assert_eq!(remaining.reports.len(), 6);
        assert_eq!(remaining.next_cursor, None);
    }

    /// 信息查询路径基准：同构大库上分页漫游 + 首页重复查询。
    ///
    /// 显式运行：`cargo test -p engine --release --lib information::queries -- --ignored --nocapture`。
    /// 输出三行 `bench:` 结果（库装配 / 全库漫游 / 首页二十次），供优化前后对照。
    #[test]
    #[ignore = "性能基准：显式 --release --ignored 运行，不入常规回归"]
    fn bench_public_report_pagination() {
        let total = 4_000u32;
        let started = std::time::Instant::now();
        let library = library_with_reports(total);
        let setup_elapsed = started.elapsed();
        let as_of = as_of_after_publication();

        // 场景 1：宿主翻完整历史（游标漫游全部页）。
        let mut cursor = None;
        let mut pages = 0usize;
        let mut summaries = 0usize;
        let started = std::time::Instant::now();
        loop {
            let result = page(&library, cursor, 100);
            summaries += result.reports.len();
            pages += 1;
            let Some(next) = result
                .next_cursor
                .as_deref()
                .and_then(|id| id.parse::<u32>().ok())
            else {
                break;
            };
            cursor = Some(next);
        }
        let walk_elapsed = started.elapsed();
        assert_eq!(pages, usize::try_from(total / 100).expect("页数有限"));
        assert_eq!(summaries, total as usize);

        // 场景 2：面板刷新只打首页（cursor=None）。
        let started = std::time::Instant::now();
        for _ in 0..20 {
            let first = page(&library, None, 100);
            assert_eq!(first.reports.len(), 100);
            assert!(first.next_cursor.is_some());
        }
        let first_page_elapsed = started.elapsed();

        println!("bench: setup total={total} elapsed={setup_elapsed:?}");
        println!("bench: full_walk pages={pages} summaries={summaries} elapsed={walk_elapsed:?}");
        println!("bench: first_page x20 elapsed={first_page_elapsed:?}");
    }
}
