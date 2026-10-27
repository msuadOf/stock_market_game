//! 税阶段实际账户投影校验；不计算法定税额，不将 prepare 当作实收。
use crate::runtime::group::GroupAccount;
use crate::runtime::world::{AdminCandidate, AdminOperation, AdminTransactionId};
use crate::runtime::{GroupIdx, LocalIdx, StockIdx};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TaxProjectionError {
    pub group: Option<GroupIdx>,
    pub local: Option<LocalIdx>,
    pub stock: Option<StockIdx>,
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

fn invalid(
    group: Option<GroupIdx>,
    local: Option<LocalIdx>,
    operation_index: Option<usize>,
    detail: impl Into<String>,
) -> TaxProjectionError {
    TaxProjectionError {
        group,
        local,
        stock: None,
        operation_index,
        detail: detail.into(),
    }
}

/// 只供真实 tax-only 整事务；混合事务须由 R4 实际阶段 producer 调用同一 core。
pub(crate) fn inspect_tax_only(
    before: &[(GroupIdx, Vec<GroupAccount>)],
    grouped: &[(GroupIdx, Vec<AdminOperation>)],
    candidate: &AdminCandidate,
) -> Result<PreparedTaxProjection, TaxProjectionError> {
    use std::collections::BTreeMap;
    let mut snapshots = BTreeMap::new();
    for (group, accounts) in before {
        if snapshots.insert(*group, accounts).is_some() {
            return Err(invalid(Some(*group), None, None, "before group 重复"));
        }
    }
    let mut candidates = BTreeMap::new();
    for group in &candidate.groups {
        if candidates.insert(group.group, group).is_some() {
            return Err(invalid(
                Some(group.group),
                None,
                None,
                "candidate group 重复",
            ));
        }
    }
    if grouped.len() != candidates.len() {
        return Err(invalid(
            None,
            None,
            None,
            "原操作与 candidate group 集不一致",
        ));
    }
    let mut groups = Vec::with_capacity(grouped.len());
    for (group_index, (group, operations)) in grouped.iter().enumerate() {
        let source = snapshots
            .get(group)
            .ok_or_else(|| invalid(Some(*group), None, None, "缺少真实 before group"))?;
        let after = candidates
            .get(group)
            .ok_or_else(|| invalid(Some(*group), None, None, "缺少 candidate group"))?;
        let mut accounts = Vec::with_capacity(after.accounts.len());
        for (local, account) in &after.accounts {
            let prior = source
                .get(*local as usize)
                .ok_or_else(|| invalid(Some(*group), Some(*local), None, "before local 越界"))?;
            accounts.push((*local, prior, account.as_ref()));
        }
        groups.push(TaxStageGroup {
            group_index,
            group: *group,
            operations: operations.iter().enumerate().collect(),
            accounts,
        });
    }
    inspect_tax_stage(candidate.id, None, &groups)
}

/// 仅核原 typed 请求与真实阶段账户；调用方须保持 id、索引与 pending 来源关联。
pub(crate) fn inspect_tax_stage(
    id: AdminTransactionId,
    stage_index: Option<u32>,
    groups: &[TaxStageGroup<'_>],
) -> Result<PreparedTaxProjection, TaxProjectionError> {
    use std::collections::{BTreeMap, BTreeSet};
    if groups.is_empty() {
        return Err(invalid(None, None, None, "税阶段不能为空"));
    }
    let mut group_ids = BTreeSet::new();
    let mut prepared = Vec::new();
    for group in groups {
        if !group_ids.insert(group.group) || group.operations.is_empty() {
            return Err(invalid(
                Some(group.group),
                None,
                None,
                "税阶段 group 重复或操作为空",
            ));
        }
        let mut expected = BTreeMap::new();
        for (local, before, after) in &group.accounts {
            if expected
                .insert(*local, ((*before).clone(), *after))
                .is_some()
            {
                return Err(invalid(
                    Some(group.group),
                    Some(*local),
                    None,
                    "阶段 local 重复",
                ));
            }
        }
        let mut touched = BTreeSet::new();
        let mut prior_index = None;
        for (operation_index, operation) in &group.operations {
            if prior_index.is_some_and(|prior| prior >= *operation_index) {
                return Err(invalid(
                    Some(group.group),
                    None,
                    Some(*operation_index),
                    "原 operation_index 必须严格递增",
                ));
            }
            prior_index = Some(*operation_index);
            let AdminOperation::WithholdingTax {
                local,
                stock,
                amount,
            } = **operation
            else {
                return Err(invalid(
                    Some(group.group),
                    None,
                    Some(*operation_index),
                    "税阶段含非 WithholdingTax 操作",
                ));
            };
            let fail = |detail| TaxProjectionError {
                stock: Some(stock),
                ..invalid(
                    Some(group.group),
                    Some(local),
                    Some(*operation_index),
                    detail,
                )
            };
            if amount <= 0 {
                return Err(fail("扣税分额必须为正"));
            }
            let (account, _) = expected
                .get_mut(&local)
                .ok_or_else(|| fail("缺少税阶段 local"))?;
            if stock as usize >= account.shares.len() {
                return Err(fail("stock 越界"));
            }
            account.cash = account
                .cash
                .checked_sub(amount)
                .filter(|cash| *cash >= 0)
                .ok_or_else(|| fail("本人 raw cash 不足或扣款溢出"))?;
            touched.insert(local);
            prepared.push(PreparedTaxOperation {
                group_index: group.group_index,
                group: group.group,
                operation_index: *operation_index,
                local,
                stock,
                amount,
            });
        }
        if touched.len() != expected.len() {
            return Err(invalid(
                Some(group.group),
                None,
                None,
                "阶段账户包含额外 local",
            ));
        }
        for (local, (expected, actual)) in expected {
            if expected != *actual {
                return Err(invalid(
                    Some(group.group),
                    Some(local),
                    None,
                    format!("完整账户不符：expected={expected:?}, actual={actual:?}"),
                ));
            }
        }
    }
    Ok(PreparedTaxProjection {
        id,
        stage_index,
        operations: prepared,
    })
}

#[cfg(test)]
#[path = "admin_tax_projection_tests.rs"]
mod tests;
