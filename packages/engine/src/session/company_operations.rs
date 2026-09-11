//! 会话接缝（任务 14）：调度器 ↔ 自然日时钟到期队列。
//!
//! `CompanyOperationsClockWiring` 把经营调度器的待办镜像为 [`CivilClock`] 的
//! `DueKind` 到期项（安装时 + 每次日终后增量同步）；`run_day_end` 在
//! `GameSession::end_civil_day` 返回报告后推进当日经营并重新同步。**披露
//! 装配不在这里**（任务 15 经 18:00 观察者接缝）；新局接线归任务 26——本
//! 接缝是纯函数性粘合，不改变既有会话行为。

use std::collections::BTreeSet;

use crate::company::operations::{CompanyDayReport, CompanyOperations, OperationsError};
use crate::company::scheduler::ScheduledAction;
use crate::session::civil_clock::{CivilClock, CivilClockError, CivilDayEndReport, DueKind};
use thiserror::Error;

/// 接缝错误（时钟侧 + 经营侧各自类型化透传；经营侧装箱——错误枚举大）。
#[derive(Debug, Error)]
pub enum CompanyOperationsSeamError {
    #[error(transparent)]
    Clock(#[from] CivilClockError),
    #[error(transparent)]
    Operations(Box<OperationsError>),
}

impl From<OperationsError> for CompanyOperationsSeamError {
    fn from(source: OperationsError) -> Self {
        Self::Operations(Box::new(source))
    }
}

/// 时钟镜像状态：已镜像的调度事件 id 集合（serde 持久化，恢复后增量续同步）。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct CompanyOperationsClockWiring {
    mirrored: BTreeSet<u64>,
}

/// 调度动作 → 时钟到期种类（InterestAccrual/ContractMaturity，任务 5 的
/// DueKind 钩子语义）。
pub fn due_kind_of(action: &ScheduledAction) -> DueKind {
    match action {
        ScheduledAction::InterestAccrual { .. } => DueKind::InterestAccrual,
        ScheduledAction::ContractMaturity { .. } => DueKind::ContractMaturity,
    }
}

impl CompanyOperationsClockWiring {
    pub fn new() -> Self {
        Self::default()
    }

    /// 安装：把当前全部待办镜像到时钟（开局/恢复后各调用一次）。
    pub fn install(
        &mut self,
        clock: &mut CivilClock,
        ops: &CompanyOperations,
    ) -> Result<usize, CompanyOperationsSeamError> {
        self.sync(clock, ops)
    }

    /// 增量同步：为尚未镜像的待办注册时钟 due。
    pub fn sync(
        &mut self,
        clock: &mut CivilClock,
        ops: &CompanyOperations,
    ) -> Result<usize, CompanyOperationsSeamError> {
        let mut registered = 0;
        for due in ops.scheduler().pending() {
            if self.mirrored.insert(due.id.value()) {
                clock.register_due(due.due_date, due_kind_of(&due.action))?;
                registered += 1;
            }
        }
        Ok(registered)
    }

    /// 日终回调：时钟已派发（恰好一次）→ 推进当日经营 → 重新同步。
    /// 在 `GameSession::end_civil_day` 成功返回后调用（任务 26 接宿主循环）。
    pub fn run_day_end(
        &mut self,
        report: &CivilDayEndReport,
        clock: &mut CivilClock,
        ops: &mut CompanyOperations,
    ) -> Result<CompanyDayReport, CompanyOperationsSeamError> {
        let day_report = ops.advance_civil_day(report.settled_date)?;
        self.sync(clock, ops)?;
        Ok(day_report)
    }

    /// 恢复路径专用：把当前全部待办视为已镜像（恢复的时钟已含这些 due 的
    /// 原注册——重装会造成重复注册；重放调度器的 due id 与原序一致）。
    pub fn adopt_all_pending(&mut self, ops: &CompanyOperations) {
        self.mirrored = ops
            .scheduler()
            .pending()
            .iter()
            .map(|due| due.id.value())
            .collect();
    }

    /// 已镜像数量（诊断/测试）。
    pub fn mirrored_count(&self) -> usize {
        self.mirrored.len()
    }

    /// 裁剪不再待办的镜像 id（task-14 复核 F-O3：`mirrored` 只增不减会让
    /// 长局存档线性膨胀；派发/到期的 due 已离开调度器，镜像记录无增量同步
    /// 价值，按当前待办集合收缩）。
    pub fn prune_dispatched(&mut self, ops: &CompanyOperations) {
        let pending: BTreeSet<u64> = ops
            .scheduler()
            .pending()
            .iter()
            .map(|due| due.id.value())
            .collect();
        self.mirrored.retain(|id| pending.contains(id));
    }
}
