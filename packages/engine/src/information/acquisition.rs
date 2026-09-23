//! 个人获知登记（K4，任务 16）：`NpcInformationState`——公共曝光与 NPC
//! 本人获取信息分离。
//!
//! 语义（K4 计划行 121–123）：
//! - 逐公司记录**本人已获取**的报告/公告 id 与获知时点；一次获知只记一次
//!   （重复获知是合法幂等输入：保留首次时点，状态字节不变）。
//! - 公共曝光只改变**发现机会**：[`discovery_candidates`] 把公开索引投影为
//!   候选 id 清单（任务 25 注意力接线消费）；候选**不等于阅读**——只有
//!   `record_acquisition` 显式登记才把内容纳入个人信息集。
//! - 守卫顺序固定：属主一致（跨 NPC 注入/合并拒绝——不存在合并 API）→
//!   公开库存在性 + `observed_at >= published_at`（EarlyRead 透传复用任务
//!   15 查询守卫，无前视）→ 重复幂等。
//! - dev/宿主查看走公开库查询面（`&self` 只读），对个人状态零写入。
//!
//! 获知时点是 civil 瞬间（休市日观察语义：与市场分钟无耦合，注意力唤醒
//! 接线归任务 25）。确定性：纯数据 + 校验，无 RNG/时钟/IO。NPC 身份复用
//! 交易 [`AccountId`]，不建平行身份。
//!
//! 存档不变量（恢复边界独力维护）：每公司条目按 id **严格递增**（二分
//! 查找正确性的前提）且跨公司 id 全局唯一（公开库 id 全局唯一 ⇒ 同 id
//! 挂多公司 = 篡改）。状态不校验 id 是否存在于公开库——跨对象校验在读取
//! 时自然发生（[`crate::information::npc_view`] 透传公开库错误）。

use std::collections::{BTreeMap, BTreeSet};

use crate::calendar::CivilInstant;
use crate::company::CompanyId;
use crate::information::{InformationError, PublicLibrary, PublicationId};
use crate::orderbook::AccountId;

/// 个人获知错误（类型化，绝不静默；公开库守卫透传复用，不重复实现）。
#[derive(Debug, thiserror::Error)]
pub enum AcquisitionError {
    /// 公开库查询守卫拒绝（未知 id / 提前读取）。
    #[error("public library rejected the acquisition: {0}")]
    Library(#[from] InformationError),
    /// 个人信息集属主与调用方 NPC 不一致（状态按 NPC 隔离，不可跨人注入）。
    #[error("information state belongs to {state_owner:?}, called for {caller:?}")]
    OwnerMismatch {
        state_owner: AccountId,
        caller: AccountId,
    },
    /// 读取未获知的公布（无前视：公开/候选索引存在 ≠ 本人已读）。
    #[error("npc {npc:?} has not acquired publication {id:?}")]
    NotAcquired { npc: AccountId, id: PublicationId },
    /// 个人获知状态自相矛盾（恢复边界：公司内 id 非严格递增/跨公司重复）。
    #[error("inconsistent acquisition state: {detail}")]
    InconsistentState { detail: String },
}

/// 获知条目种类（登记时由公开库解析定死，读取时不靠猜测）。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum AcquiredKind {
    Report,
    Announcement,
}

/// 一条获知记录（不可变事实：公布 id + 首次获知时点 + 种类）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct AcquisitionRecord {
    pub id: PublicationId,
    pub observed_at: CivilInstant,
    pub kind: AcquiredKind,
}

/// `record_acquisition` 的结果（重复获知幂等，不报错）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum AcquisitionOutcome {
    /// 首次获知：按公布所属公司登记。
    Recorded {
        company: CompanyId,
        observed_at: CivilInstant,
    },
    /// 重复获知：保留首次时点，不追加记录（一次获知只记一次）。
    AlreadyAcquired {
        company: CompanyId,
        first_observed_at: CivilInstant,
    },
}

/// 公开库解析结果（公司归属 + 种类；登记与读取共用）。
struct ResolvedPublication {
    company: CompanyId,
    kind: AcquiredKind,
}

/// 解析公布 id：先按报告查（未知再查公告面）；两条守卫（未知 id /
/// observed_at 早于 published_at）全部透传任务 15 查询守卫，不重复实现。
fn resolve_publication(
    library: &PublicLibrary,
    id: PublicationId,
    as_of: CivilInstant,
) -> Result<ResolvedPublication, InformationError> {
    match library.report(id, as_of) {
        Ok(report) => Ok(ResolvedPublication {
            company: report.company.clone(),
            kind: AcquiredKind::Report,
        }),
        Err(InformationError::UnknownPublication { .. }) => {
            let announcement = library.announcement(id, as_of)?;
            Ok(ResolvedPublication {
                company: announcement.company.clone(),
                kind: AcquiredKind::Announcement,
            })
        }
        Err(early) => Err(early),
    }
}

/// NPC 个人信息集：属主单一，逐公司的已获知公布登记（BTreeMap 公司序；
/// 每公司 Vec 按 id 严格递增——恢复边界校验的不变量）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct NpcInformationState {
    owner: AccountId,
    companies: BTreeMap<CompanyId, Vec<AcquisitionRecord>>,
}

/// 存档 DTO（恢复走 [`NpcInformationState::from_parts`] 校验）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct NpcInformationStateSave {
    pub owner: AccountId,
    pub companies: BTreeMap<CompanyId, Vec<AcquisitionRecord>>,
}

impl NpcInformationState {
    pub fn new(owner: AccountId) -> Self {
        Self {
            owner,
            companies: BTreeMap::new(),
        }
    }

    /// 状态属主（个人信息集按 NPC 隔离）。
    pub fn owner(&self) -> AccountId {
        self.owner
    }

    /// 登记一次获知。守卫顺序固定：属主一致 → 公开库存在 + observed_at ≥
    /// published_at（EarlyRead 透传）→ 重复幂等（保留首次时点，不重记）。
    /// 个人状态只存 id 引用与获知时点，从不克隆报表内容。
    pub fn record_acquisition(
        &mut self,
        npc: AccountId,
        library: &PublicLibrary,
        publication_id: PublicationId,
        observed_at: CivilInstant,
    ) -> Result<AcquisitionOutcome, AcquisitionError> {
        if self.owner != npc {
            return Err(AcquisitionError::OwnerMismatch {
                state_owner: self.owner,
                caller: npc,
            });
        }
        let resolved = resolve_publication(library, publication_id, observed_at)?;
        let records = self.companies.entry(resolved.company.clone()).or_default();
        match records.binary_search_by_key(&publication_id, |record| record.id) {
            Ok(index) => Ok(AcquisitionOutcome::AlreadyAcquired {
                company: resolved.company,
                first_observed_at: records[index].observed_at,
            }),
            Err(position) => {
                records.insert(
                    position,
                    AcquisitionRecord {
                        id: publication_id,
                        observed_at,
                        kind: resolved.kind,
                    },
                );
                Ok(AcquisitionOutcome::Recorded {
                    company: resolved.company,
                    observed_at,
                })
            }
        }
    }

    /// 本人已获知的公布总数（跨公司）。
    pub fn acquired_count(&self) -> usize {
        self.companies.values().map(Vec::len).sum()
    }

    /// 该公司已获知条目（id 序；无则空切片）。
    pub fn records_for_company(&self, company: &CompanyId) -> &[AcquisitionRecord] {
        self.companies.get(company).map_or(&[], Vec::as_slice)
    }

    /// 逐公司迭代（公司序；条目 id 序）——上下文装配/确定性投影用。
    pub fn companies(&self) -> impl Iterator<Item = (&CompanyId, &[AcquisitionRecord])> {
        self.companies
            .iter()
            .map(|(company, records)| (company, records.as_slice()))
    }

    /// 按公布 id 查首次获知时点（id 全局唯一由恢复边界与登记路径保证）。
    pub fn observed_at_of(&self, id: PublicationId) -> Option<CivilInstant> {
        self.record_of(id).map(|record| record.observed_at)
    }

    pub(crate) fn record_of(&self, id: PublicationId) -> Option<&AcquisitionRecord> {
        self.companies.values().find_map(|records| {
            records
                .binary_search_by_key(&id, |record| record.id)
                .ok()
                .map(|index| &records[index])
        })
    }

    /// 恢复边界：公司内 id 严格递增 + 跨公司 id 全局唯一。任何失败 ⇒
    /// 状态不产生（PublicLibrary 先例；二分查找前提不容静默破坏）。
    pub fn from_parts(save: NpcInformationStateSave) -> Result<Self, AcquisitionError> {
        let mut seen = BTreeSet::new();
        for (company, records) in &save.companies {
            for pair in records.windows(2) {
                if pair[0].id >= pair[1].id {
                    return Err(AcquisitionError::InconsistentState {
                        detail: format!(
                            "company {company:?} acquisition ids not strictly increasing"
                        ),
                    });
                }
            }
            for record in records {
                if !seen.insert(record.id) {
                    return Err(AcquisitionError::InconsistentState {
                        detail: format!(
                            "publication {:?} claimed by multiple companies",
                            record.id
                        ),
                    });
                }
            }
        }
        Ok(Self {
            owner: save.owner,
            companies: save.companies,
        })
    }

    /// 导出存档 DTO（公司序 + id 序——BTreeMap/严格递增 Vec，确定性）。
    pub fn save(&self) -> NpcInformationStateSave {
        NpcInformationStateSave {
            owner: self.owner,
            companies: self.companies.clone(),
        }
    }
}

impl serde::Serialize for NpcInformationState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.save().serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for NpcInformationState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let save = NpcInformationStateSave::deserialize(deserializer)?;
        Self::from_parts(save).map_err(serde::de::Error::custom)
    }
}

/// 公共曝光发现面（任务 25 注意力接线消费）：该公司截至 `as_of` 已公布的
/// 全部 id（报告 + 公告，id 序，确定性）。**只列候选，不产生阅读**——是否
/// 真正获知由 `record_acquisition` 显式登记；未登记的候选对
/// [`crate::information::NpcObservationContext`] 不可读（NotAcquired）。
pub fn discovery_candidates(
    library: &PublicLibrary,
    company: &CompanyId,
    as_of: CivilInstant,
) -> Vec<PublicationId> {
    let mut ids: Vec<PublicationId> = library
        .reports_for_company(company, as_of)
        .into_iter()
        .map(|report| report.id)
        .collect();
    ids.extend(
        library
            .announcements_for_company(company, as_of)
            .into_iter()
            .map(|announcement| announcement.id),
    );
    ids.sort_unstable();
    ids
}
