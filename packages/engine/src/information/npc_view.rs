//! NPC 观察上下文（K4/K5，任务 16）：策略可读的「本人已知公开信息 +
//! 可见行情」引用面。
//!
//! 构造输入**只有**四样：NPC 身份、本人获知状态（属主校验）、公开库
//! 引用、可见行情快照引用。行情快照是泛型参数——生产侧由 session 装配
//! 真实可见行情（任务 26 接线），engine 信息域不依赖 session；**类型面上
//! 即不可达 CompanyState/Books/总账**（本类型不引用那些类型，构造面也无
//! 从传入——测试以最小构造钉死）。
//!
//! 内容全部按**引用**暴露（公布 id + 借用的报告/公告访问器）——个人状态
//! 从不克隆报表。历史版本按获知时点钉死：更正 = 新公布 id，未获知的新
//! 版本对上下文 [`AcquisitionError::NotAcquired`]，直到新的获知事件登记。

use crate::calendar::CivilInstant;
use crate::company::CompanyId;
use crate::information::{
    AcquiredKind, AcquisitionError, Announcement, NpcInformationState, PublicLibrary,
    PublicationId, PublishedReport,
};
use crate::orderbook::AccountId;

/// 上下文中的已获知条目（公布 id 引用 + 首次获知时点 + 归属公司）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct AcquiredEntry {
    pub company: CompanyId,
    pub id: PublicationId,
    pub observed_at: CivilInstant,
}

/// NPC 观察上下文：只携带引用（`&state`/`&library`/`&market`），零克隆。
pub struct NpcObservationContext<'a, Market> {
    npc: AccountId,
    state: &'a NpcInformationState,
    library: &'a PublicLibrary,
    market: &'a Market,
}

impl<'a, Market> NpcObservationContext<'a, Market> {
    /// 构造（属主校验：获知状态属主 ≠ npc ⇒ [`AcquisitionError::OwnerMismatch`]
    /// ——他人信息集不可注入；不存在跨 NPC 合并 API）。
    pub fn new(
        npc: AccountId,
        state: &'a NpcInformationState,
        library: &'a PublicLibrary,
        market: &'a Market,
    ) -> Result<Self, AcquisitionError> {
        if state.owner() != npc {
            return Err(AcquisitionError::OwnerMismatch {
                state_owner: state.owner(),
                caller: npc,
            });
        }
        Ok(Self {
            npc,
            state,
            library,
            market,
        })
    }

    /// 本上下文属主。
    pub fn npc(&self) -> AccountId {
        self.npc
    }

    /// 可见行情快照引用（session 侧装配，只读；不是 CompanyState/总账）。
    pub fn market(&self) -> &'a Market {
        self.market
    }

    /// 本人已获知的报告条目（公司序 + id 序，确定性）。
    pub fn acquired_reports(&self) -> Vec<AcquiredEntry> {
        self.entries_of_kind(AcquiredKind::Report)
    }

    /// 本人已获知的公告条目（公司序 + id 序，确定性）。
    pub fn acquired_announcements(&self) -> Vec<AcquiredEntry> {
        self.entries_of_kind(AcquiredKind::Announcement)
    }

    /// 按获知时点读取已获知报告（as_of = 首次获知时点 ⇒ 钉死获知版本；
    /// 未获知 id ⇒ NotAcquired；篡改状态里的库外 id 透传公开库错误）。
    pub fn report(&self, id: PublicationId) -> Result<&'a PublishedReport, AcquisitionError> {
        let observed_at = self.require_acquired(id)?;
        Ok(self.library.report(id, observed_at)?)
    }

    /// 按获知时点读取已获知公告（语义同 [`Self::report`]）。
    pub fn announcement(&self, id: PublicationId) -> Result<&'a Announcement, AcquisitionError> {
        let observed_at = self.require_acquired(id)?;
        Ok(self.library.announcement(id, observed_at)?)
    }

    fn require_acquired(&self, id: PublicationId) -> Result<CivilInstant, AcquisitionError> {
        self.state
            .observed_at_of(id)
            .ok_or(AcquisitionError::NotAcquired { npc: self.npc, id })
    }

    fn entries_of_kind(&self, kind: AcquiredKind) -> Vec<AcquiredEntry> {
        let mut entries = Vec::new();
        for (company, records) in self.state.companies() {
            for record in records {
                if record.kind == kind {
                    entries.push(AcquiredEntry {
                        company: company.clone(),
                        id: record.id,
                        observed_at: record.observed_at,
                    });
                }
            }
        }
        entries
    }
}
