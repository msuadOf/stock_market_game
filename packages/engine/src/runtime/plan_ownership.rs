//! 计划按稳定 AccountId 分片与完整汇合；不分配身份、不重算游标。

use super::protocol::RuntimeError;
use super::GroupIdx;
use crate::plans::{PlanBook, PlanPolicy};
use crate::AccountId;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
thread_local! {
    static OWNER_MAP_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn owner_map_builds_for_test() -> usize {
    OWNER_MAP_BUILDS.with(std::cell::Cell::get)
}

fn invalid(detail: impl Into<String>) -> RuntimeError {
    RuntimeError::ProtocolInvariant {
        detail: detail.into(),
    }
}

fn owner_groups(
    roster: &[AccountId],
    n_groups: usize,
    per_group: usize,
) -> Result<BTreeMap<AccountId, GroupIdx>, RuntimeError> {
    if n_groups == 0 || per_group == 0 || roster.len() > n_groups.saturating_mul(per_group) {
        return Err(invalid("计划名册与组布局不一致"));
    }
    #[cfg(test)]
    OWNER_MAP_BUILDS.with(|count| count.set(count.get() + 1));
    let mut owners = BTreeMap::new();
    for (index, &owner) in roster.iter().enumerate() {
        if owners
            .insert(owner, (index / per_group) as GroupIdx)
            .is_some()
        {
            return Err(invalid(format!(
                "PlanWriterOwnershipViolation：名册重复 owner={owner:?}"
            )));
        }
    }
    Ok(owners)
}

/// 名册与组布局在 Driver 生命周期内固定；各次汇合只借用已验证映射。
pub(crate) struct OwnerGroups {
    owners: BTreeMap<AccountId, GroupIdx>,
    n_groups: usize,
    per_group: usize,
}

impl OwnerGroups {
    pub(crate) fn new(
        roster: &[AccountId],
        n_groups: usize,
        per_group: usize,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            owners: owner_groups(roster, n_groups, per_group)?,
            n_groups,
            per_group,
        })
    }

    pub(crate) fn matches_layout(&self, n_groups: usize, per_group: usize) -> bool {
        self.n_groups == n_groups && self.per_group == per_group
    }

    /// 已经验证的簿只搬移原部件；无历史的账户不凭空补游标。
    pub(crate) fn split(&self, book: PlanBook) -> Result<Vec<PlanBook>, RuntimeError> {
        let owners = &self.owners;
        let n_groups = self.n_groups;
        let (policy, cursors, plans) = book.into_parts();
        let mut grouped_cursors = vec![Vec::new(); n_groups];
        let mut grouped_plans = vec![Vec::new(); n_groups];
        for cursor in cursors {
            let group = owners.get(&cursor.owner).ok_or_else(|| {
                invalid(format!(
                    "PlanWriterOwnershipViolation：游标 owner={:?} 不在名册",
                    cursor.owner
                ))
            })?;
            grouped_cursors[*group as usize].push(cursor);
        }
        for plan in plans {
            let group = owners.get(&plan.account()).ok_or_else(|| {
                invalid(format!(
                    "PlanWriterOwnershipViolation：计划 owner={:?} 不在名册",
                    plan.account()
                ))
            })?;
            grouped_plans[*group as usize].push(plan);
        }
        grouped_cursors
            .into_iter()
            .zip(grouped_plans)
            .map(|(cursors, plans)| {
                PlanBook::from_parts(policy, cursors, plans)
                    .map_err(|error| invalid(format!("计划分片非法：{error}")))
            })
            .collect()
    }

    /// 对显式组集合与每个 owner 的唯一 writer 验证后，才构造完整新簿。
    pub(crate) fn merge(
        &self,
        shards: Vec<(GroupIdx, PlanBook)>,
        policy: PlanPolicy,
    ) -> Result<PlanBook, RuntimeError> {
        let owners = &self.owners;
        let n_groups = self.n_groups;
        if shards.len() != n_groups {
            return Err(invalid(format!(
                "计划组应答不完整：expected={n_groups}, actual={}",
                shards.len()
            )));
        }
        let mut groups = BTreeSet::new();
        let mut cursors = Vec::new();
        let mut plans = Vec::new();
        for (group, book) in shards {
            if group as usize >= n_groups || !groups.insert(group) {
                return Err(invalid(format!("计划组应答重复或越界：group={group}")));
            }
            if *book.policy() != policy {
                return Err(invalid(format!("PlanPolicyMismatch：group={group}")));
            }
            let (_, part_cursors, part_plans) = book.into_parts();
            for cursor in &part_cursors {
                if owners.get(&cursor.owner) != Some(&group) {
                    return Err(invalid(format!(
                        "PlanWriterOwnershipViolation：group={group}, owner={:?}",
                        cursor.owner
                    )));
                }
            }
            for plan in &part_plans {
                if owners.get(&plan.account()) != Some(&group) {
                    return Err(invalid(format!(
                        "PlanWriterOwnershipViolation：group={group}, plan={:?}",
                        plan.plan_id()
                    )));
                }
            }
            cursors.extend(part_cursors);
            plans.extend(part_plans);
        }
        PlanBook::from_parts(policy, cursors, plans)
            .map_err(|error| invalid(format!("计划完整汇合非法：{error}")))
    }
}

/// 独立入口自行验证名册，不依赖 RuntimeDriver 的固定布局。
pub(crate) fn split(
    book: PlanBook,
    roster: &[AccountId],
    n_groups: usize,
    per_group: usize,
) -> Result<Vec<PlanBook>, RuntimeError> {
    OwnerGroups::new(roster, n_groups, per_group)?.split(book)
}

/// 独立 World 构造与候选验证保留完整布局检查。
pub(crate) fn merge(
    shards: Vec<(GroupIdx, PlanBook)>,
    roster: &[AccountId],
    n_groups: usize,
    per_group: usize,
    policy: PlanPolicy,
) -> Result<PlanBook, RuntimeError> {
    OwnerGroups::new(roster, n_groups, per_group)?.merge(shards, policy)
}
