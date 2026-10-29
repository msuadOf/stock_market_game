//! 有序资产阶段复用唯一行政 Prepare、最终 INFO、inspect 与 Commit 生命周期。

use super::*;
use crate::runtime::admin_tax_projection::{
    inspect_tax_stage, PreparedTaxProjection, TaxStageGroup,
};
use crate::runtime::group::GroupAccount;
use crate::runtime::protocol::{AdminStageLayout, PreparedAdminOperationPosition};
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

/// 只借 World 保留的原请求；组回执不能另报一份 operand 替代它。
#[derive(Debug)]
pub(crate) struct PreparedAdminOperand<'a> {
    pub(crate) group: GroupIdx,
    pub(crate) stage_index: u32,
    pub(crate) operation_index: usize,
    pub(crate) operation: &'a AdminOperation,
}

/// 仅在同次 inspect 借用；税视图来自该 pending 的真实 working。
#[derive(Debug)]
pub(crate) struct PreparedAdministrativeStages<'a> {
    operation_indices: Vec<(GroupIdx, u32, usize)>,
    tax_stages: Vec<PreparedTaxStageAccounts>,
    tax_projections: Vec<PreparedTaxProjection>,
    operands: Vec<PreparedAdminOperand<'a>>,
}

impl<'a> PreparedAdministrativeStages<'a> {
    pub(crate) fn operands(&self) -> &[PreparedAdminOperand<'a>] {
        &self.operands
    }

    pub(crate) fn tax_projections(&self) -> &[PreparedTaxProjection] {
        &self.tax_projections
    }
}

/// 仅核真实组回执的完整索引及税 local 集；损坏回执沿原协议致死路径处理。
pub(super) fn validate_group_stage_evidence(
    id: AdminTransactionId,
    candidate: &AdminGroupCandidate,
    operations: &[AdminOperation],
    layout: Option<&AdminStageLayout>,
) -> Result<(), RuntimeError> {
    let Some(layout) = layout else {
        return if candidate.stages.is_none() {
            Ok(())
        } else {
            Err(invalid("单阶段行政回执含未请求的阶段来源"))
        };
    };
    let prepared = candidate
        .stages
        .as_ref()
        .ok_or_else(|| invalid("组缺少实际阶段来源回执"))?;
    let mut offset = 0usize;
    let mut positions = Vec::new();
    let mut taxes = Vec::new();
    for (stage, &count) in layout.counts.iter().enumerate() {
        let stage = u32::try_from(stage).map_err(|_| invalid("阶段索引超出 u32"))?;
        let end = offset
            .checked_add(count)
            .ok_or_else(|| invalid("阶段操作范围溢出"))?;
        let rows = operations
            .get(offset..end)
            .ok_or_else(|| invalid("阶段范围超出原请求"))?;
        positions.extend(
            (offset..end).map(|operation_index| PreparedAdminOperationPosition {
                stage_index: stage,
                operation_index,
            }),
        );
        if layout.tax_stages.contains(&stage) && !rows.is_empty() {
            let locals: Vec<_> = rows
                .iter()
                .map(|operation| operation.coordinates().0)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            taxes.push((stage, locals));
        }
        offset = end;
    }
    if offset != operations.len()
        || positions != prepared.operation_indices
        || prepared.tax_stages.len() != taxes.len()
    {
        return Err(invalid(
            "组阶段回执与原 flat 请求范围、顺序或税阶段集合不一致",
        ));
    }
    for ((stage, locals), actual) in taxes.iter().zip(&prepared.tax_stages) {
        if *stage != actual.stage_index
            || actual.accounts.iter().map(|row| row.0).collect::<Vec<_>>() != *locals
        {
            return Err(invalid("税阶段 before/after 的阶段或完整 local 集不一致"));
        }
        let stage_start = layout.counts[..*stage as usize].iter().sum::<usize>();
        let stage_end = stage_start + layout.counts[*stage as usize];
        let source = TaxStageGroup {
            group_index: 0,
            group: candidate.group,
            operations: operations[stage_start..stage_end]
                .iter()
                .enumerate()
                .map(|(index, operation)| (stage_start + index, operation))
                .collect(),
            accounts: actual
                .accounts
                .iter()
                .map(|(local, before, after)| (*local, before.as_ref(), after.as_ref()))
                .collect(),
        };
        // 此处仍在协议回执验证路径；来源损坏不能降为可恢复的用户 inspect 拒绝。
        inspect_tax_stage(id, Some(*stage), std::slice::from_ref(&source))
            .map_err(|source| invalid(format!("组税阶段实际账户来源损坏：{source:?}")))?;
    }
    Ok(())
}

fn prepare_stage_source<'a>(
    candidate: &AdminCandidate,
    stages: &'a [AdminOperationStage],
) -> Result<PreparedAdministrativeStages<'a>, RuntimeError> {
    let groups: BTreeMap<_, _> = candidate
        .groups
        .iter()
        .map(|group| (group.group, group))
        .collect();
    let mut offsets = BTreeMap::<GroupIdx, usize>::new();
    let mut evidence = PreparedAdministrativeStages {
        operation_indices: Vec::new(),
        tax_stages: Vec::new(),
        tax_projections: Vec::new(),
        operands: Vec::new(),
    };
    for (stage_index, stage) in stages.iter().enumerate() {
        let stage_index = u32::try_from(stage_index).map_err(|_| invalid("阶段索引超出 u32"))?;
        let mut tax_groups = Vec::new();
        for (group_index, (group, operations)) in stage.operations.iter().enumerate() {
            if operations.is_empty() {
                continue;
            }
            let actual = groups
                .get(group)
                .ok_or_else(|| invalid("原阶段请求缺少参与组候选"))?;
            let prepared = actual
                .stages
                .as_ref()
                .ok_or_else(|| invalid("原阶段请求缺少实际来源"))?;
            let offset = offsets.entry(*group).or_default();
            for (within_stage, operation) in operations.iter().enumerate() {
                let operation_index = offset
                    .checked_add(within_stage)
                    .ok_or_else(|| invalid("组内 flat 索引溢出"))?;
                let position = PreparedAdminOperationPosition {
                    stage_index,
                    operation_index,
                };
                if prepared.operation_indices.get(operation_index) != Some(&position) {
                    return Err(invalid("实际 checked apply 索引与 World 原请求不一致"));
                }
                evidence
                    .operation_indices
                    .push((*group, stage_index, operation_index));
                evidence.operands.push(PreparedAdminOperand {
                    group: *group,
                    stage_index,
                    operation_index,
                    operation,
                });
            }
            if operations
                .iter()
                .all(|operation| matches!(operation, AdminOperation::WithholdingTax { .. }))
            {
                let accounts = prepared
                    .tax_stages
                    .iter()
                    .find(|tax| tax.stage_index == stage_index)
                    .ok_or_else(|| invalid("税阶段缺少真实 working before/after"))?;
                tax_groups.push(PreparedTaxStageGroup {
                    group_index,
                    group: *group,
                    accounts: accounts.accounts.clone(),
                });
            }
            *offset = offset
                .checked_add(operations.len())
                .ok_or_else(|| invalid("组内 flat 索引溢出"))?;
        }
        if !tax_groups.is_empty() {
            let views: Vec<_> = tax_groups
                .iter()
                .map(|group| TaxStageGroup {
                    group_index: group.group_index,
                    group: group.group,
                    operations: evidence
                        .operands
                        .iter()
                        .filter(|operand| {
                            operand.stage_index == stage_index && operand.group == group.group
                        })
                        .map(|operand| (operand.operation_index, operand.operation))
                        .collect(),
                    accounts: group
                        .accounts
                        .iter()
                        .map(|(local, before, after)| (*local, before.as_ref(), after.as_ref()))
                        .collect(),
                })
                .collect();
            evidence.tax_projections.push(
                inspect_tax_stage(candidate.id, Some(stage_index), &views)
                    .map_err(|source| invalid(format!("真实税阶段账户投影不一致：{source:?}")))?,
            );
            evidence.tax_stages.push(PreparedTaxStageAccounts {
                stage_index,
                groups: tax_groups,
            });
        }
    }
    Ok(evidence)
}

fn prepare_stage_requests(
    stages: &[AdminOperationStage],
    n_groups: usize,
    has_information: bool,
) -> Result<
    (
        Vec<(GroupIdx, Vec<AdminOperation>)>,
        BTreeMap<GroupIdx, AdminStageLayout>,
    ),
    RuntimeError,
> {
    let mut grouped = BTreeMap::<GroupIdx, Vec<AdminOperation>>::new();
    let mut counts = BTreeMap::<GroupIdx, Vec<usize>>::new();
    let mut tax_stages = BTreeSet::new();
    for (stage_index, stage) in stages.iter().enumerate() {
        let index = u32::try_from(stage_index).map_err(|_| invalid("阶段索引超出 u32"))?;
        let mut seen = BTreeSet::new();
        let mut tax = false;
        let mut other = false;
        for (group, operations) in &stage.operations {
            if usize::from(*group) >= n_groups || !seen.insert(*group) {
                return Err(invalid("阶段 group 越界或重复"));
            }
            for operation in operations {
                if matches!(operation, AdminOperation::WithholdingTax { .. }) {
                    tax = true;
                } else {
                    other = true;
                }
            }
            grouped
                .entry(*group)
                .or_default()
                .extend_from_slice(operations);
            counts
                .entry(*group)
                .or_insert_with(|| vec![0; stages.len()])[stage_index] = operations.len();
        }
        if tax && other {
            return Err(invalid("税阶段不得混入非 WithholdingTax 操作"));
        }
        if tax {
            tax_stages.insert(index);
        }
    }
    if has_information {
        for group in 0..n_groups as GroupIdx {
            grouped.entry(group).or_default();
            counts.entry(group).or_insert_with(|| vec![0; stages.len()]);
        }
    }
    let layouts = counts
        .into_iter()
        .map(|(group, counts)| {
            (
                group,
                AdminStageLayout {
                    counts,
                    tax_stages: tax_stages.clone(),
                },
            )
        })
        .collect();
    Ok((grouped.into_iter().collect(), layouts))
}

impl World {
    /// 资产阶段与最终 INFO 共用一个 pending/id；成功返回才代表最终 Commit 完成。
    pub(crate) fn with_staged_admin_transaction<P>(
        &mut self,
        stages: Vec<AdminOperationStage>,
        information: Option<crate::runtime::protocol::InformationPreparePayload>,
        inspect: impl FnOnce(
            &AdminCandidate,
            &PreparedAdministrativeStages<'_>,
        ) -> Result<P, RuntimeError>,
    ) -> Result<P, RuntimeError> {
        let (grouped, layouts) =
            prepare_stage_requests(&stages, self.n_groups, information.is_some())?;
        self.with_administrative_inspection(
            grouped,
            information,
            None,
            Some(layouts),
            |candidate| {
                let evidence = prepare_stage_source(candidate, &stages)?;
                inspect(candidate, &evidence)
            },
            |_candidate, _source, value| Ok(value),
        )
        .map(|(value, _ack)| value)
    }

    /// 阶段账务与 INFO 共用一个 pending/id，并在真实 prepared source 上检查阶段证据。
    pub(crate) fn with_prepared_staged_administrative_transaction<P>(
        &mut self,
        stages: Vec<AdminOperationStage>,
        information: Option<crate::runtime::protocol::InformationPreparePayload>,
        inspect: impl FnOnce(
            &AdminCandidate,
            &crate::runtime::world::PreparedAdministrativeSource,
            &PreparedAdministrativeStages<'_>,
        ) -> Result<P, RuntimeError>,
    ) -> Result<(P, crate::runtime::source::PreparedMirrorAck), RuntimeError> {
        let (grouped, layouts) =
            prepare_stage_requests(&stages, self.n_groups, information.is_some())?;
        self.with_administrative_inspection(
            grouped,
            information,
            None,
            Some(layouts),
            |_candidate| Ok(()),
            |candidate, source, ()| {
                let evidence = prepare_stage_source(candidate, &stages)?;
                inspect(candidate, source, &evidence)
            },
        )
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "stage_tests.rs"]
mod tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "stage_information_tests.rs"]
mod information_tests;
