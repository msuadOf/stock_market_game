//! 经营编排装配输入：公司（规格 + 行业账套 + 流参数）与全局（seed + 冲击参数）。

use crate::accounting::Books;
use crate::company::bank::BankBooks;
use crate::company::events::ShockParams;
use crate::company::industrial::IndustrialBooks;
use crate::company::insurance::InsuranceBooks;
use crate::company::operations::{
    BankFlowParams, IndustrialFlowParams, InsuranceFlowParams, RealEstateFlowParams,
};
use crate::company::real_estate::RealEstateBooks;
use crate::company::spec::{CompanyKind, CompanySpec};

/// 行业账套（任务 8–11 的独立引擎组合；不经过 `Company` 注册表壳——会话
/// 接线归任务 26）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum IndustryBooks {
    Industrial(IndustrialBooks),
    Bank(BankBooks),
    Insurance(InsuranceBooks),
    RealEstate(RealEstateBooks),
}

impl IndustryBooks {
    /// 权威账套（任何行业的过账事实都在这里；确定性金样比较锚点）。
    pub fn books(&self) -> &Books {
        match self {
            IndustryBooks::Industrial(inner) => inner.books(),
            IndustryBooks::Bank(inner) => inner.books(),
            IndustryBooks::Insurance(inner) => inner.books(),
            IndustryBooks::RealEstate(inner) => inner.books(),
        }
    }

    pub fn kind(&self) -> CompanyKind {
        match self {
            IndustryBooks::Industrial(_) => CompanyKind::Industrial,
            IndustryBooks::Bank(_) => CompanyKind::Bank,
            IndustryBooks::Insurance(_) => CompanyKind::Insurance,
            IndustryBooks::RealEstate(_) => CompanyKind::RealEstate,
        }
    }

    pub fn as_industrial(&self) -> Option<&IndustrialBooks> {
        match self {
            IndustryBooks::Industrial(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn as_bank(&self) -> Option<&BankBooks> {
        match self {
            IndustryBooks::Bank(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn as_insurance(&self) -> Option<&InsuranceBooks> {
        match self {
            IndustryBooks::Insurance(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn as_real_estate(&self) -> Option<&RealEstateBooks> {
        match self {
            IndustryBooks::RealEstate(inner) => Some(inner),
            _ => None,
        }
    }
}

/// 行业经营流参数（与 `IndustryBooks` 变体一一对应；装配时校验匹配）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum FlowParams {
    Industrial(IndustrialFlowParams),
    Bank(BankFlowParams),
    Insurance(InsuranceFlowParams),
    RealEstate(RealEstateFlowParams),
}

impl FlowParams {
    pub fn variant_name(&self) -> &'static str {
        match self {
            FlowParams::Industrial(_) => "Industrial",
            FlowParams::Bank(_) => "Bank",
            FlowParams::Insurance(_) => "Insurance",
            FlowParams::RealEstate(_) => "RealEstate",
        }
    }
}

/// 单公司装配输入。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct OperatingCompanyConfig {
    pub spec: CompanySpec,
    pub books: IndustryBooks,
    pub flow: FlowParams,
}

/// 全局装配输入。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CompanyOperationsConfig {
    /// 公司域 RNG 总种子（派生市场/行业/公司/前史流；绝不与策略/会话种子
    /// 共享——K4）。
    pub seed: u64,
    pub shock_params: ShockParams,
    pub companies: Vec<OperatingCompanyConfig>,
}
