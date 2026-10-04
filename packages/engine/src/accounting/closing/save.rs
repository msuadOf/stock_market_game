//! ClosingEngine 的 serde 存档形态。
//!
//! JSON 映射键必须为字符串：版本键（Scope × 期间 × 种类）与重述底稿键
//! （Scope）均为组合/枚举键，故平铺为**值列表**，恢复侧重建映射（存档
//! 只存事实、派生结构由恢复重建——`Books` 的 BooksRef/BooksOwned 先例）。
//! 重述底稿随引擎整体存取：save → restore 后继续作用于该 Scope 的所有
//! 后续生成（CAS 28 追溯重述语义，见父模块文档）。

use std::collections::BTreeMap;

use crate::accounting::consolidation::ScopeId;
use crate::accounting::period::AccountingPeriod;
use crate::accounting::reports::{ReportKind, ReportSet};

use super::{ClosingEngine, RestatementRegister, RestatementRows, VersionKey};

/// 存档形态：版本序列与重述底稿均平铺（键组合成为元组值）。
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct EngineSave {
    versions: Vec<(ScopeId, AccountingPeriod, ReportKind, Vec<ReportSet>)>,
    restatements: RestatementRows,
}

impl From<&ClosingEngine> for EngineSave {
    fn from(engine: &ClosingEngine) -> Self {
        EngineSave {
            versions: engine
                .versions
                .iter()
                .map(|((scope, period, kind), sets)| (scope.clone(), *period, *kind, sets.clone()))
                .collect(),
            restatements: engine.restatements.save_rows(),
        }
    }
}

impl From<EngineSave> for ClosingEngine {
    fn from(save: EngineSave) -> Self {
        let mut versions: BTreeMap<VersionKey, Vec<ReportSet>> = BTreeMap::new();
        for (scope, period, kind, sets) in save.versions {
            if !sets.is_empty() {
                versions.insert((scope, period, kind), sets);
            }
        }
        let restatements = RestatementRegister::from_rows(save.restatements);
        ClosingEngine {
            versions,
            restatements,
            hash_projection_cache: Default::default(),
        }
    }
}

impl serde::Serialize for ClosingEngine {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EngineSave::from(self).serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for ClosingEngine {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let save = EngineSave::deserialize(deserializer)?;
        for (_, _, _, reports) in &save.versions {
            for report in reports {
                report
                    .validate_parent_income_source()
                    .map_err(serde::de::Error::custom)?;
            }
        }
        Ok(Self::from(save))
    }
}
