//! 税阶段实际账户投影校验；不计算法定税额，不将 prepare 当作实收。
use crate::runtime::group::GroupAccount;
use crate::runtime::world::{AdminCandidate, AdminOperation, AdminTransactionId};
use crate::runtime::{GroupIdx, LocalIdx, StockIdx};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TaxProjectionError {
    pub group: Option<GroupIdx>,
    pub local: Option<LocalIdx>,
    pub operation_index: Option<usize>,
    pub detail: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PreparedTaxOperation {
    pub group_index: usize,
    pub group: GroupIdx,
    pub operation_index: usize,
    pub local: LocalIdx,
    pub stock: StockIdx,
    pub amount: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PreparedTaxProjection {
    pub id: AdminTransactionId,
    pub stage_index: Option<u32>,
    pub operations: Vec<PreparedTaxOperation>,
}
/// 外层来源必须为同 pending 实际 apply 前后的投影；此类型本身不是来源证明。
pub(crate) struct TaxStageGroup<'a> {
    pub group_index: usize,
    pub group: GroupIdx,
    pub operations: Vec<(usize, &'a AdminOperation)>,
    pub accounts: Vec<(LocalIdx, &'a GroupAccount, &'a GroupAccount)>,
}

pub(crate) fn inspect_tax_only(
    _before: &[(GroupIdx, Vec<GroupAccount>)],
    _grouped: &[(GroupIdx, Vec<AdminOperation>)],
    _candidate: &AdminCandidate,
) -> Result<PreparedTaxProjection, TaxProjectionError> {
    Err(TaxProjectionError { group: None, local: None, operation_index: None, detail: "税投影尚未实现".into() })
}

#[cfg(test)]
#[path = "admin_tax_projection_tests.rs"]
mod tests;
