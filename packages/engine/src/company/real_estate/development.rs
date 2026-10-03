//! 开发成本与项目生命周期处理器（K3 地产）：开发投入（现金）、暂停/复工
//! （资本化窗口状态）、完工（资本化终止）。
//!
//! 分录（开发投入）：Dr 1541 / Cr 1002（经营活动，同购地归类）。
//! 暂停/复工/完工是**纯子账状态转移，不过账**（无资金/成本运动的日期事实，
//! 随存档序列化；对资本化窗口的影响见 projects.rs 模块文档）。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::counterparty::{CounterpartyId, FlowDirection};
use crate::company::real_estate::projects::ProjectId;
use crate::company::real_estate::{chart, RealEstateBooks, RealEstateError};

impl RealEstateBooks {
    /// 开发成本发生（现金支付）：校验（项目存在/未完工/未暂停、对手方、
    /// 正金额）→ 原子过账 → 成本入项目。**首笔开发投入开启资本化窗口**。
    pub fn incur_development(
        &mut self,
        project: &ProjectId,
        contractor: &CounterpartyId,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, RealEstateError> {
        self.ensure_counterparty(contractor)?;
        let state = self
            .project(project)
            .ok_or_else(|| RealEstateError::UnknownProject {
                project: project.clone(),
            })?;
        state.validate_development(project, amount)?;

        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::DevelopmentCostIncurred,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::DEV_INVENTORY, PostingSide::Debit, amount),
                    super::line(chart::acct::CASH, PostingSide::Credit, amount),
                ],
            }],
        )?;
        self.projects_mut()
            .get_mut(project)
            .expect("validated above")
            .add_development(amount, date);
        self.record_flow(
            date,
            contractor,
            FlowDirection::Outbound,
            amount,
            "development cost",
        )?;
        Ok(event)
    }

    /// 暂停开发（中断开始；资本化窗口影响由政策阈值决定，见 projects.rs）。
    pub fn suspend_development(
        &mut self,
        project: &ProjectId,
        date: CivilDate,
    ) -> Result<(), RealEstateError> {
        let state = self
            .project(project)
            .ok_or_else(|| RealEstateError::UnknownProject {
                project: project.clone(),
            })?;
        state.validate_suspend(project)?;
        self.projects_mut()
            .get_mut(project)
            .expect("validated above")
            .begin_interruption(date);
        Ok(())
    }

    /// 复工（闭合中断区间；区间长度与政策阈值比较在计提分类时进行）。
    pub fn resume_development(
        &mut self,
        project: &ProjectId,
        date: CivilDate,
    ) -> Result<(), RealEstateError> {
        let state = self
            .project(project)
            .ok_or_else(|| RealEstateError::UnknownProject {
                project: project.clone(),
            })?;
        let gap = state.preview_resume(project, date)?;
        self.projects_mut()
            .get_mut(project)
            .expect("validated above")
            .close_interruption(gap);
        Ok(())
    }

    /// 完工（达到可交付状态）：资本化窗口自此终止——此后利息必须费用化。
    pub fn complete_project(
        &mut self,
        project: &ProjectId,
        date: CivilDate,
    ) -> Result<(), RealEstateError> {
        let state = self
            .project(project)
            .ok_or_else(|| RealEstateError::UnknownProject {
                project: project.clone(),
            })?;
        state.validate_complete(project)?;
        self.projects_mut()
            .get_mut(project)
            .expect("validated above")
            .mark_completed(date);
        Ok(())
    }
}
