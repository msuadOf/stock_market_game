//! 交易日盘中月报：在私有 tick candidate 内把已到期月报真实公开并送达本人。
//!
//! 只处理 Simple 公司源的 Monthly 排期。窗口上界是调用方给出的真实市场观察
//! 时点；休市日、收盘后的月报仍由自然日日结处理，不能借盘中入口制造市场拍。

use super::disclosures::candidate::{
    has_due_monthly_report, prepare_disclosures, DisclosurePrepareCtx, DisclosurePublication,
    DisclosurePublicationKind, DisclosureSourceRef,
};
use super::*;
use std::sync::Arc;

impl GameSession {
    /// 在私有 candidate 上准备到期月报；无新公开时返回 `None`，不推进游标。
    pub(super) fn prepare_intraday_disclosure_candidate(
        &self,
        now: CivilInstant,
    ) -> Result<Option<disclosures::candidate::DisclosureCandidate>, SessionError> {
        let frequency = self.state.setup.report_frequency;
        if !matches!(
            frequency,
            Some(crate::information::ReportFrequency::Monthly { .. })
        ) {
            return Ok(None);
        }
        let SessionCompanies::CompanySystem(companies) = &self.state.companies else {
            return Ok(None);
        };
        let frequency = frequency.expect("Monthly 频率刚经匹配");
        let source = DisclosureSourceRef::Simple(&companies.system);
        if !has_due_monthly_report(
            source,
            frequency,
            &self.state.library,
            self.state.seed,
            self.state.disclosures.published_through(),
            now,
        )
        .map_err(|error| {
            SessionError::Step(StepFatal::InvariantViolation {
                location: "GameSession::prepare_intraday_disclosure_candidate".into(),
                description: format!("盘中月报预过滤失败：{error}"),
            })
        })? {
            return Ok(None);
        }
        let candidate = prepare_disclosures(DisclosurePrepareCtx {
            source,
            previous_dispatch: &self.state.disclosures,
            previous_library: &self.state.library,
            seed: self.state.seed,
            report_frequency: frequency,
            through: now,
            prehistory_opening: None,
            settled_announcements: None,
            corporate_actions: Some(&companies.corporate_actions),
        })
        .map_err(|error| {
            SessionError::Step(StepFatal::InvariantViolation {
                location: "GameSession::prepare_intraday_disclosure_candidate".into(),
                description: format!("盘中月报候选准备失败：{error}"),
            })
        })?;
        Ok((!candidate.publications.is_empty()).then_some(candidate))
    }

    /// 本局生命周期内是否已发生过真实公开（区别于前史播种游标）。
    pub(crate) fn has_session_lifetime_disclosure(&self) -> bool {
        self.state
            .library
            .has_publication_since(self.state.civil_clock.start_date())
    }

    /// 串行 fallback：本局已有真实公开后，按非即时 cadence 到期补投本人。
    pub(super) fn deliver_due_information(
        &mut self,
        now: CivilInstant,
    ) -> Result<bool, SessionError> {
        if !self.has_session_lifetime_disclosure() {
            self.information_check_floor = Some(Self::no_information_check_instant());
            return Ok(false);
        }
        if self
            .information_check_floor
            .is_some_and(|floor| now < floor)
        {
            return Ok(false);
        }
        if !self.has_due_non_immediate_information_check(now) {
            self.information_check_floor = self.scan_information_check_floor();
            return Ok(false);
        }
        let anchors = self
            .prepare_information_anchors()
            .map_err(SessionError::Step)?;
        self.deliver_public_information_with_anchors(now, &anchors)?;
        self.information_check_floor = self.scan_information_check_floor();
        Ok(true)
    }

    /// 把盘中候选安装到私有会话并生成真实公开事件；先按公布时点送达本人。
    pub(super) fn publish_intraday_reports(
        &mut self,
        now: CivilInstant,
    ) -> Result<Vec<Event>, SessionError> {
        let Some(candidate) = self.prepare_intraday_disclosure_candidate(now)? else {
            return Ok(Vec::new());
        };
        self.state.library = Arc::new(candidate.library);
        self.state.disclosures = candidate.dispatch;
        // 新 library 版本真正安装时无条件失效旧 floor；首次 pub 前可能
        // 已因“无 baseline”写入远期哨兵，不能挡住本局真实 cadence。
        self.information_check_floor = None;
        let mut publications = candidate.publications;
        publications.sort_by_key(|publication| (publication.published_at, publication.id));
        let anchors = self
            .prepare_information_anchors()
            .map_err(SessionError::Step)?;
        let mut events = Vec::with_capacity(publications.len());
        let mut delivered_at = None;
        for DisclosurePublication {
            id,
            company,
            published_at,
            kind,
        } in publications
        {
            let DisclosurePublicationKind::Report {
                version,
                period: _,
                scope: _,
                kind: _,
                supersedes: _,
            } = kind
            else {
                return Err(SessionError::Step(StepFatal::InvariantViolation {
                    location: "GameSession::publish_intraday_reports".into(),
                    description: "盘中月报候选不得包含公告".into(),
                }));
            };
            if delivered_at != Some(published_at) {
                self.deliver_public_information_with_anchors(published_at, &anchors)?;
                delivered_at = Some(published_at);
            }
            events.push(Event::CompanyDisclosurePublished {
                seq: self.next_seq(),
                publication_id: id,
                company,
                published_at,
                kind: CompanyDisclosureKind::Report {
                    report_revision: version.sequence,
                },
            });
        }
        Ok(events)
    }
}
