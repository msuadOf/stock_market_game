//! 固定集团图（K3，任务 12）：成员规格镜像 + 单层母子公司校验。
//!
//! 输入是 task-7 `CompanySpec`（`group_parent` + `issued_shares`）的镜像：
//! accounting 不得 import company（依赖方向铁律），由调用方（任务 13/26）
//! 从注册表派生。校验顺序（每步先于下一步，全部通过才产出集团）：
//! 重复 id → 未知母公司 → 链上成环 → 根存在且无母公司 → 每个非根成员的
//! 母公司恰为根（多层集团显式不支持）→ 无子公司 `NotApplicable` →
//! 持股数学（正持股、不超发、精确基点、严格 > 5000bp 控制一致性）。

use std::collections::{BTreeMap, BTreeSet};

use crate::accounting::Books;

use super::error::ConsolidationError;

/// 集团成员 id newtype（与 company::CompanyId 平行的会计域命名空间；
/// 由调用方从 CompanyId 派生）。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct MemberId(pub String);

impl std::fmt::Display for MemberId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 报表范围标记（K3：单体与合并分别标记，不双计现金）。
/// `Consolidated` 携带合并母公司 id（集团由根唯一标识）。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum ScopeId {
    /// 单体报表（单一公司）。
    Standalone(MemberId),
    /// 合并报表（固定集团）。
    Consolidated(MemberId),
}

impl std::fmt::Display for ScopeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScopeId::Standalone(id) => write!(f, "standalone({id})"),
            ScopeId::Consolidated(id) => write!(f, "consolidated({id})"),
        }
    }
}

/// 成员规格镜像（task-7 CompanySpec 的会计域投影）。
///
/// `parent_held_shares` 是**集团母公司**持有的该成员股数（固定事实，开局后
/// 不变——不做并购/股权交易）；根成员该字段必须为 0。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct MemberSpec {
    pub id: MemberId,
    pub group_parent: Option<MemberId>,
    pub issued_shares: u64,
    pub parent_held_shares: u64,
}

/// 集团成员：规格镜像 + 权威账套只读引用。
#[derive(Clone, Debug)]
pub struct GroupMember<'a> {
    pub spec: MemberSpec,
    pub books: &'a Books,
}

/// 已校验的子公司持股（基点精确）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct SubsidiaryOwnership {
    pub id: MemberId,
    pub issued_shares: u64,
    pub parent_held_shares: u64,
    /// 母公司持股（基点；= held×10000/issued，整除精确）。
    pub parent_ownership_bp: i64,
    /// 少数股东（基点；= 10000 − parent_ownership_bp）。
    pub minority_bp: i64,
}

/// 已校验的固定集团：根 + 直接子公司（按 id 排序）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ValidatedGroup {
    pub root: MemberId,
    pub subsidiaries: Vec<SubsidiaryOwnership>,
}

/// 基点分母。
const BP_DENOM: i128 = 10_000;
/// 控制所需的最低持股（严格大于；恰好 5000bp = 半数，不构成控制）。
const CONTROL_BP_FLOOR: i128 = 5_000;

/// 校验固定集团图并推导持股比例。输入含全部成员（根 + 子公司）。
pub(crate) fn validate_group(
    root: &MemberId,
    members: &BTreeMap<MemberId, MemberSpec>,
) -> Result<ValidatedGroup, ConsolidationError> {
    if !members.contains_key(root) {
        return Err(ConsolidationError::UnknownRoot { root: root.clone() });
    }
    // 1. 母公司引用存在 + 2. 链上成环（沿 parent 走，带访问集）。
    for spec in members.values() {
        if let Some(parent) = &spec.group_parent {
            if !members.contains_key(parent) {
                return Err(ConsolidationError::UnknownGroupParent {
                    member: spec.id.clone(),
                    parent: parent.clone(),
                });
            }
        }
        let mut visited: BTreeSet<&MemberId> = BTreeSet::new();
        visited.insert(&spec.id);
        let mut current = spec;
        while let Some(parent_id) = &current.group_parent {
            if !visited.insert(parent_id) {
                return Err(ConsolidationError::GroupCycle {
                    member: parent_id.clone(),
                });
            }
            current = &members[parent_id];
        }
    }
    // 3. 根无母公司、无自持股权申报；4. 每个非根成员的母公司恰为根（链
    //    终点必须等于根：终点不是根 = 集团外成员；链长 > 1 = 多层集团）。
    if let Some(parent) = &members[root].group_parent {
        return Err(ConsolidationError::RootHasParent {
            root: root.clone(),
            parent: parent.clone(),
        });
    }
    if members[root].parent_held_shares != 0 {
        return Err(ConsolidationError::RootWithHolding {
            root: root.clone(),
            parent_held_shares: members[root].parent_held_shares,
        });
    }
    let mut subsidiaries = Vec::new();
    for spec in members.values() {
        let Some(declared_parent) = &spec.group_parent else {
            if &spec.id != root {
                return Err(ConsolidationError::MemberOutsideGroup {
                    member: spec.id.clone(),
                    top: spec.id.clone(),
                    root: root.clone(),
                });
            }
            continue;
        };
        if declared_parent != root {
            // 链终点是另一个顶级成员 → 集团外；否则是多层集团。
            let mut top = &members[declared_parent];
            while let Some(next) = &top.group_parent {
                top = &members[next];
            }
            if top.id == *root {
                return Err(ConsolidationError::NestedGroupUnsupported {
                    member: spec.id.clone(),
                    parent: declared_parent.clone(),
                });
            }
            return Err(ConsolidationError::MemberOutsideGroup {
                member: spec.id.clone(),
                top: top.id.clone(),
                root: root.clone(),
            });
        }
        subsidiaries.push(ownership_of(spec)?);
    }
    if subsidiaries.is_empty() {
        return Err(ConsolidationError::NotApplicable {
            company: root.clone(),
        });
    }
    Ok(ValidatedGroup {
        root: root.clone(),
        subsidiaries,
    })
}

/// 由持股股数推导精确基点比例并校验控制一致性。
fn ownership_of(spec: &MemberSpec) -> Result<SubsidiaryOwnership, ConsolidationError> {
    if spec.parent_held_shares == 0 {
        return Err(ConsolidationError::ZeroHolding {
            member: spec.id.clone(),
        });
    }
    if spec.parent_held_shares > spec.issued_shares {
        return Err(ConsolidationError::HoldingBeyondIssued {
            member: spec.id.clone(),
            held: spec.parent_held_shares,
            issued: spec.issued_shares,
        });
    }
    let scaled = i128::from(spec.parent_held_shares) * BP_DENOM;
    let issued = i128::from(spec.issued_shares);
    if scaled % issued != 0 {
        return Err(ConsolidationError::OwnershipNotRepresentable {
            member: spec.id.clone(),
            held: spec.parent_held_shares,
            issued: spec.issued_shares,
        });
    }
    // bp = held×10000/issued 且 0 < held ≤ issued（u64）⇒ bp ∈ (0, 10000]，
    // 恒在 i64 值域内。
    let quotient = scaled / issued;
    if quotient <= CONTROL_BP_FLOOR {
        return Err(ConsolidationError::ControlOwnershipMismatch {
            member: spec.id.clone(),
            ownership_bp: i64::try_from(quotient)
                .expect("basis points are bounded to (0, 10000] by construction"),
        });
    }
    let parent_ownership_bp =
        i64::try_from(quotient).expect("basis points are bounded to (0, 10000] by construction");
    Ok(SubsidiaryOwnership {
        id: spec.id.clone(),
        issued_shares: spec.issued_shares,
        parent_held_shares: spec.parent_held_shares,
        parent_ownership_bp,
        minority_bp: i64::try_from(BP_DENOM).expect("const 10000") - parent_ownership_bp,
    })
}
