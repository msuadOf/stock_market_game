//! Group 侧按阶段预演行政资产操作，并为扣税阶段保留边界账户快照。

use super::{apply_operation, invalid};
use crate::runtime::group::{GroupAccount, GroupState};
use crate::runtime::protocol::{
    AdminOperation, AdminStageLayout, PreparedAdminOperationPosition, PreparedGroupAdminStages,
    PreparedTaxStageAccounts,
};
use crate::runtime::LocalIdx;
use std::collections::BTreeMap;
use std::sync::Arc;

/// 在私有 working 上按原始扁平顺序执行，并可选生成扣税阶段边界证据。
pub(super) fn prepare_working(
    state: &GroupState,
    operations: &[AdminOperation],
    layout: Option<&AdminStageLayout>,
    has_repurchase_scope: bool,
) -> Result<
    (
        BTreeMap<LocalIdx, GroupAccount>,
        Option<PreparedGroupAdminStages>,
    ),
    crate::runtime::protocol::RuntimeError,
> {
    let ranges = match layout {
        Some(layout) => validate_layout(layout, operations)?,
        None => vec![(0, operations.len())],
    };
    let mut working = BTreeMap::new();
    let mut operation_indices = Vec::new();
    let mut tax_stages = Vec::new();
    for (stage_index, (start, end)) in ranges.into_iter().enumerate() {
        let stage_index =
            u32::try_from(stage_index).map_err(|_| invalid("行政阶段索引超过 u32"))?;
        let is_tax_stage = layout.is_some_and(|layout| layout.tax_stages.contains(&stage_index));
        let mut before = BTreeMap::<LocalIdx, Arc<GroupAccount>>::new();
        for (operation_index, &operation) in operations.iter().enumerate().take(end).skip(start) {
            if !has_repurchase_scope {
                state.validate_repurchase_funds_operation(operation)?;
            }
            let (local, stock) = operation.coordinates();
            let account = state.accounts.get(local as usize).ok_or_else(|| {
                invalid(format!(
                    "行政 local 越界：group={}, local={local}",
                    state.group
                ))
            })?;
            let candidate = working.entry(local).or_insert_with(|| (**account).clone());
            if matches!(operation, AdminOperation::OpenIssuerRepurchase { .. })
                && has_repurchase_scope
            {
                continue;
            }
            if is_tax_stage {
                before
                    .entry(local)
                    .or_insert_with(|| Arc::new(candidate.clone()));
            }
            apply_operation(candidate, stock, operation).map_err(|error| {
                invalid(format!(
                    "组 {} 行政 local={local}, stock={stock}：{error}",
                    state.group
                ))
            })?;
            if layout.is_some() {
                operation_indices.push(PreparedAdminOperationPosition {
                    stage_index,
                    operation_index,
                });
            }
        }
        if is_tax_stage && start != end {
            let accounts = before
                .into_iter()
                .map(|(local, before)| {
                    let after =
                        Arc::new(working.get(&local).expect("扣税账户已进入 working").clone());
                    (local, before, after)
                })
                .collect();
            tax_stages.push(PreparedTaxStageAccounts {
                stage_index,
                accounts,
            });
        }
    }
    Ok((
        working,
        layout.map(|_| PreparedGroupAdminStages {
            operation_indices,
            tax_stages,
        }),
    ))
}

fn validate_layout(
    layout: &AdminStageLayout,
    operations: &[AdminOperation],
) -> Result<Vec<(usize, usize)>, crate::runtime::protocol::RuntimeError> {
    u32::try_from(layout.counts.len()).map_err(|_| invalid("行政阶段数量超过 u32"))?;
    let mut total = 0usize;
    let mut ranges = Vec::with_capacity(layout.counts.len());
    for &count in &layout.counts {
        let end = total
            .checked_add(count)
            .ok_or_else(|| invalid("行政阶段操作数量溢出"))?;
        ranges.push((total, end));
        total = end;
    }
    if total != operations.len() {
        return Err(invalid(format!(
            "行政阶段操作数与 flat operations 不一致：stages={total}, operations={}",
            operations.len()
        )));
    }
    for &stage_index in &layout.tax_stages {
        let index = usize::try_from(stage_index).map_err(|_| invalid("扣税阶段索引非法"))?;
        if index >= layout.counts.len() {
            return Err(invalid(format!("扣税阶段索引越界：stage={stage_index}")));
        }
    }
    // 逐 stage 校验类型；空扣税阶段合法，但不会生成账户边界快照。
    for (stage_index, &(start, end)) in ranges.iter().enumerate() {
        let stage_index =
            u32::try_from(stage_index).map_err(|_| invalid("行政阶段索引超过 u32"))?;
        let is_tax_stage = layout.tax_stages.contains(&stage_index);
        for (operation_index, operation) in operations.iter().enumerate().take(end).skip(start) {
            let is_tax_operation = matches!(operation, AdminOperation::WithholdingTax { .. });
            if is_tax_stage != is_tax_operation {
                let detail = if is_tax_stage {
                    format!(
                        "扣税阶段包含非 WithholdingTax：stage={stage_index}, operation={operation_index}"
                    )
                } else {
                    format!(
                        "非扣税阶段包含 WithholdingTax：stage={stage_index}, operation={operation_index}"
                    )
                };
                return Err(invalid(detail));
            }
        }
    }
    Ok(ranges)
}
