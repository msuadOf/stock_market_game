//! 公司规格（会计与资金边界）：发行人身份、主营行业、会计类型与固定集团关系。
//!
//! `CompanyId` 与交易域 `StockCode`/`AccountId` 是完全独立的命名空间；发行
//! 股票映射用 `Option<StockCode>`（允许未上市测试实体）。行业科目表与行业
//! 经营逻辑由各行业模块提供；本文件只承载规格值类型与规格集合的结构校验
//! （重复 id、重复发行映射、未知母公司、集团环）。

use std::collections::{BTreeMap, BTreeSet};

use crate::account::StockCode;
use crate::company::error::CompanyError;

/// 公司唯一 id newtype（与 `StockId`/`AccountId` 独立命名空间）。
///
/// 数值身份由 [`CompanyCatalog`] 在装配/读档时按字符串代码升序分配，
/// id 升序与代码字典序一致（BTreeMap 迭代序 = 展示序保持）。
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
#[ts(export)]
pub struct CompanyId(pub u32);

impl std::fmt::Display for CompanyId {
    /// 调试/边界文本用；展示代码请经 [`CompanyCatalog::code`] 翻译。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 公司元数据：字符串代码（边界身份，如 `"C-600101"`）与公司名（领域内容）。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct CompanyMeta {
    pub code: String,
    pub name: String,
}

/// 公司目录构造错误：重复代码（代码是字符串身份，重复即装配失败）。
#[derive(Clone, Eq, PartialEq, Debug, thiserror::Error)]
pub enum CompanyCatalogError {
    #[error("duplicate company code: {0}")]
    DuplicateCode(String),
}

/// 公司目录：字符串代码 ↔ 数值 `CompanyId` 的唯一翻译层（沿 `StockCatalog` 模式）。
///
/// 核心运行时只使用 `CompanyId(u32)`；字符串代码只在本目录与配置/显示边界出现。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, Default)]
pub struct CompanyCatalog {
    metas: Vec<CompanyMeta>,
    /// 代码 → id 索引（派生数据，不序列化；反序列化后重建）。
    #[serde(skip)]
    by_code: BTreeMap<String, CompanyId>,
}

impl CompanyCatalog {
    /// 由元数据列表构造：按代码升序排序后顺序分配 id；重复代码显式报错。
    pub fn from_metas(metas: Vec<CompanyMeta>) -> Result<CompanyCatalog, CompanyCatalogError> {
        let mut metas = metas;
        metas.sort_by(|a, b| a.code.cmp(&b.code));
        let mut by_code = BTreeMap::new();
        for (index, meta) in metas.iter().enumerate() {
            let id = CompanyId(index as u32);
            if by_code.insert(meta.code.clone(), id).is_some() {
                return Err(CompanyCatalogError::DuplicateCode(meta.code.clone()));
            }
        }
        Ok(CompanyCatalog { metas, by_code })
    }

    /// 仅按代码列表构造（名称留空）——测试/夹具便捷入口；排序与查重语义同
    /// [`CompanyCatalog::from_metas`]。
    pub fn from_codes(codes: &[&str]) -> Result<CompanyCatalog, CompanyCatalogError> {
        let metas = codes
            .iter()
            .map(|code| CompanyMeta {
                code: (*code).to_string(),
                name: String::new(),
            })
            .collect();
        CompanyCatalog::from_metas(metas)
    }

    /// 目录公司数。
    pub fn len(&self) -> usize {
        self.metas.len()
    }

    /// 目录是否为空。
    pub fn is_empty(&self) -> bool {
        self.metas.is_empty()
    }

    /// 元数据列表（代码升序）。
    pub fn metas(&self) -> &[CompanyMeta] {
        &self.metas
    }

    /// 按 id 取元数据；越界 id 返回 None。
    pub fn meta(&self, id: CompanyId) -> Option<&CompanyMeta> {
        self.metas.get(id.0 as usize)
    }

    /// 按 id 取代码；越界 id 返回 None（调用方必须显式处理）。
    pub fn code(&self, id: CompanyId) -> Option<&str> {
        self.meta(id).map(|meta| meta.code.as_str())
    }

    /// 按 id 取公司名；越界 id 返回 None。
    pub fn name(&self, id: CompanyId) -> Option<&str> {
        self.meta(id).map(|meta| meta.name.as_str())
    }

    /// 字符串代码 → 数值 id；未知代码返回 None（调用方必须显式处理）。
    pub fn id(&self, code: &str) -> Option<CompanyId> {
        self.by_code.get(code).copied()
    }

    /// 按 id 序（= 代码序）迭代全部元数据。
    pub fn iter(&self) -> impl Iterator<Item = (CompanyId, &CompanyMeta)> {
        self.metas
            .iter()
            .enumerate()
            .map(|(index, meta)| (CompanyId(index as u32), meta))
    }

    /// 反序列化后重建代码索引。
    fn rebuild_index(&mut self) {
        self.by_code = self
            .metas
            .iter()
            .enumerate()
            .map(|(index, meta)| (meta.code.clone(), CompanyId(index as u32)))
            .collect();
    }
}

impl<'de> serde::Deserialize<'de> for CompanyCatalog {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Raw {
            metas: Vec<CompanyMeta>,
        }
        let raw = Raw::deserialize(deserializer)?;
        let mut catalog = CompanyCatalog {
            metas: raw.metas,
            by_code: BTreeMap::new(),
        };
        catalog.rebuild_index();
        Ok(catalog)
    }
}

/// 主营行业 id newtype（虚构行业标识；与证券类别分开建模，不混用）。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct IndustryId(pub String);

/// 公司会计类型（四行业模型；行业逻辑由对应经营模块提供）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub enum CompanyKind {
    /// 工商（制造/商贸等通用工商主体）。
    Industrial,
    /// 银行（存贷款与三阶段 ECL；银行经营会计）。
    Bank,
    /// 保险（一般计量模型；保险经营会计）。
    Insurance,
    /// 地产（开发销售；地产经营会计）。
    RealEstate,
}

/// 公司规格：身份 + 行业 + 会计类型 + 发行人映射 + 固定集团关系。
///
/// 股本语义（会计与资金边界）：`issued_shares` 是**已发行普通股总股数**；映射上市股票时
/// 必须与 `total_shares` 精确相等（流通股与总股本不可互换），由
/// [`CompanyRegistry::validate_issuer_mapping`] 强制。
///
/// [`CompanyRegistry::validate_issuer_mapping`]: crate::company::CompanyRegistry::validate_issuer_mapping
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CompanySpec {
    /// 字符串代码（边界身份，如 `"C-600101"`）：配置/存档/显示翻译用，
    /// 核心逻辑一律走数值 `CompanyId`（由 [`CompanyCatalog`] 分配）。
    pub code: String,
    pub name: String,
    pub industry: IndustryId,
    pub kind: CompanyKind,
    /// 发行人映射：`None` = 未上市独立测试实体。
    pub listed_stock: Option<StockCode>,
    /// 已发行普通股总股数（>0；映射股票时与其 total_shares 精确相等）。
    pub issued_shares: u64,
    /// 固定集团母公司代码（开局后不变；母公司必须在同一规格集合内，
    /// 数值 id 由目录在装配时翻译）。
    pub group_parent: Option<String>,
}

impl CompanySpec {
    /// 单规格校验：非空代码/名称/行业、正股本。
    pub fn validate(&self) -> Result<(), CompanyError> {
        if self.code.trim().is_empty() {
            return Err(CompanyError::SpecInvalid {
                detail: "empty company code".to_string(),
            });
        }
        if self.name.trim().is_empty() {
            return Err(CompanyError::SpecInvalid {
                detail: format!("empty name for company {}", self.code),
            });
        }
        if self.industry.0.trim().is_empty() {
            return Err(CompanyError::SpecInvalid {
                detail: format!("empty industry for company {}", self.code),
            });
        }
        if self.issued_shares == 0 {
            return Err(CompanyError::SpecInvalid {
                detail: format!("company {} has zero issued shares", self.code),
            });
        }
        Ok(())
    }

    /// 规格集合校验：代码唯一、发行映射唯一、母公司存在、集团链无环
    /// （任一失败 ⇒ 注册表整体不产生，无部分状态）。
    pub fn validate_set(specs: &[CompanySpec]) -> Result<(), CompanyError> {
        let mut by_code: BTreeMap<&str, &CompanySpec> = BTreeMap::new();
        let mut by_stock: BTreeMap<&StockCode, &str> = BTreeMap::new();
        for spec in specs {
            spec.validate()?;
            if by_code.insert(spec.code.as_str(), spec).is_some() {
                return Err(CompanyError::DuplicateCompanyId {
                    company_code: spec.code.clone(),
                });
            }
            if let Some(stock) = &spec.listed_stock {
                if let Some(first) = by_stock.insert(stock, spec.code.as_str()) {
                    return Err(CompanyError::DuplicateIssuerStock {
                        stock: stock.clone(),
                        company_code: spec.code.clone(),
                        first_code: first.to_string(),
                    });
                }
            }
        }
        for spec in specs {
            let mut visited: BTreeSet<&str> = BTreeSet::new();
            visited.insert(spec.code.as_str());
            let mut current = spec;
            while let Some(parent_code) = &current.group_parent {
                let parent =
                    by_code
                        .get(parent_code.as_str())
                        .ok_or(CompanyError::UnknownGroupParent {
                            company_code: current.code.clone(),
                            parent_code: parent_code.clone(),
                        })?;
                if !visited.insert(parent.code.as_str()) {
                    return Err(CompanyError::GroupCycle {
                        company_code: parent.code.clone(),
                    });
                }
                current = parent;
            }
        }
        Ok(())
    }
}
