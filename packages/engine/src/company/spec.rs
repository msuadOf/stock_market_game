//! 公司规格（K2）：发行人身份、主营行业、会计类型与固定集团关系。
//!
//! `CompanyId` 与交易域 `StockCode`/`AccountId` 是完全独立的命名空间；发行
//! 股票映射用 `Option<StockCode>`（允许未上市测试实体）。行业科目表与行业
//! 经营逻辑在任务 8–11 落地；本文件只承载规格值类型与规格集合的结构校验
//! （重复 id、重复发行映射、未知母公司、集团环）。

use std::collections::{BTreeMap, BTreeSet};

use crate::account::StockCode;
use crate::company::error::CompanyError;

/// 公司唯一 id newtype（与 `StockCode`/`AccountId` 独立命名空间）。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct CompanyId(pub String);

/// 主营行业 id newtype（虚构行业标识；与证券类别分开建模，不混用）。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct IndustryId(pub String);

/// 公司会计类型（K3 行业模型；行业逻辑在任务 8–11 落地）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum CompanyKind {
    /// 工商（制造/商贸等通用工商主体）。
    Industrial,
    /// 银行（存贷款与三阶段 ECL；任务 9）。
    Bank,
    /// 保险（一般计量模型；任务 10）。
    Insurance,
    /// 地产（开发销售；任务 11）。
    RealEstate,
}

/// 公司规格：身份 + 行业 + 会计类型 + 发行人映射 + 固定集团关系。
///
/// 股本语义（K2）：`issued_shares` 是**已发行普通股总股数**；映射上市股票时
/// 必须与 `total_shares` 精确相等（流通股与总股本不可互换），由
/// [`CompanyRegistry::validate_issuer_mapping`] 强制。
///
/// [`CompanyRegistry::validate_issuer_mapping`]: crate::company::CompanyRegistry::validate_issuer_mapping
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CompanySpec {
    pub id: CompanyId,
    pub name: String,
    pub industry: IndustryId,
    pub kind: CompanyKind,
    /// 发行人映射：`None` = 未上市独立测试实体。
    pub listed_stock: Option<StockCode>,
    /// 已发行普通股总股数（>0；映射股票时与其 total_shares 精确相等）。
    pub issued_shares: u64,
    /// 固定集团母公司（开局后不变；合并报表在任务 12）。
    pub group_parent: Option<CompanyId>,
}

impl CompanySpec {
    /// 单规格校验：非空 id/名称/行业、正股本。
    pub fn validate(&self) -> Result<(), CompanyError> {
        if self.id.0.trim().is_empty() {
            return Err(CompanyError::SpecInvalid {
                detail: "empty company id".to_string(),
            });
        }
        if self.name.trim().is_empty() {
            return Err(CompanyError::SpecInvalid {
                detail: format!("empty name for company {:?}", self.id),
            });
        }
        if self.industry.0.trim().is_empty() {
            return Err(CompanyError::SpecInvalid {
                detail: format!("empty industry for company {:?}", self.id),
            });
        }
        if self.issued_shares == 0 {
            return Err(CompanyError::SpecInvalid {
                detail: format!("company {:?} has zero issued shares", self.id),
            });
        }
        Ok(())
    }

    /// 规格集合校验：id 唯一、发行映射唯一、母公司存在、集团链无环
    /// （任一失败 ⇒ 注册表整体不产生，无部分状态）。
    pub fn validate_set(specs: &[CompanySpec]) -> Result<(), CompanyError> {
        let mut by_id: BTreeMap<&CompanyId, &CompanySpec> = BTreeMap::new();
        let mut by_stock: BTreeMap<&StockCode, &CompanyId> = BTreeMap::new();
        for spec in specs {
            spec.validate()?;
            if by_id.insert(&spec.id, spec).is_some() {
                return Err(CompanyError::DuplicateCompanyId {
                    company: spec.id.clone(),
                });
            }
            if let Some(stock) = &spec.listed_stock {
                if let Some(first) = by_stock.insert(stock, &spec.id) {
                    return Err(CompanyError::DuplicateIssuerStock {
                        stock: stock.clone(),
                        company: spec.id.clone(),
                        first: first.clone(),
                    });
                }
            }
        }
        for spec in specs {
            let mut visited: BTreeSet<&CompanyId> = BTreeSet::new();
            visited.insert(&spec.id);
            let mut current = spec;
            while let Some(parent_id) = &current.group_parent {
                let parent = by_id
                    .get(parent_id)
                    .ok_or(CompanyError::UnknownGroupParent {
                        company: current.id.clone(),
                        parent: parent_id.clone(),
                    })?;
                if !visited.insert(&parent.id) {
                    return Err(CompanyError::GroupCycle {
                        company: parent.id.clone(),
                    });
                }
                current = parent;
            }
        }
        Ok(())
    }
}
