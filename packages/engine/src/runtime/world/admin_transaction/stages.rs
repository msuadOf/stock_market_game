//! 同一行政事务的有序资产阶段；当前 placeholder 仅用于先取得真实行为红。

use super::*;
use crate::runtime::admin_tax_projection::PreparedTaxProjection;
use crate::runtime::group::GroupAccount;
use crate::runtime::LocalIdx;
use std::sync::Arc;

/// 阶段只携原 typed 请求，不携“已执行”或付款成功标志。
#[derive(Clone, Debug)]
pub(crate) struct AdminOperationStage {
    pub(crate) operations: Vec<(GroupIdx, Vec<AdminOperation>)>,
}

#[derive(Debug)]
struct PreparedTaxStageGroup {
    group_index: usize,
    group: GroupIdx,
    accounts: Vec<(LocalIdx, Arc<GroupAccount>, Arc<GroupAccount>)>,
}

#[derive(Debug)]
struct PreparedTaxStageAccounts {
    stage_index: u32,
    groups: Vec<PreparedTaxStageGroup>,
}

/// 仅在同次 inspect 借用；真实生产实现须从该 pending 的 working 构造。
#[derive(Debug)]
pub(crate) struct PreparedAdministrativeStages {
    operation_indices: Vec<(GroupIdx, u32, usize)>,
    tax_stages: Vec<PreparedTaxStageAccounts>,
    tax_projections: Vec<PreparedTaxProjection>,
}

impl World {
    /// 保留一个 pending/id/Commit；不能循环调用旧整事务入口冒阶段。
    pub(crate) fn with_staged_admin_transaction<P>(
        &mut self,
        stages: Vec<AdminOperationStage>,
        information: Option<crate::runtime::protocol::InformationPreparePayload>,
        inspect: impl FnOnce(&AdminCandidate, &PreparedAdministrativeStages) -> Result<P, RuntimeError>,
    ) -> Result<P, RuntimeError> {
        let _ = (stages, information, inspect);
        Err(invalid("R4 ordered stages 的同 pending 实际来源尚未接通"))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "stage_tests.rs"]
mod tests;
