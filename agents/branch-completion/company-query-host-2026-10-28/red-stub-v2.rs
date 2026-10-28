//! 三宿主共用的公司只读边界；身份由宿主提供，不属于 wire 请求。

use super::ProtocolSession;
use crate::company::capabilities::CompanyCapabilities;
use crate::company::error::CompanyErrorClass;
use crate::company::simple::period::PeriodChangeExplanation;
use crate::company::{CompanyId, CompanySystemError};
use crate::session::{
    AccountDividendTaxStatusView, CompanyCatalogViewError, DividendTaxOutstandingView,
    FlatWithholdingReceipt, OwnerRightsOfferingView, SessionCorporateActionsError, StepFatal,
};
use crate::{AccountId, CivilDate, ValidationFault};

/// CompanyId 来自本局 company_catalog；请求不能选择账户。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
pub enum CompanyReadQuery {
    CompanyPeriodExplanation {
        company: CompanyId,
        period_end: CivilDate,
    },
    AccountDividendTaxStatus,
    DividendTaxOutstandingViews,
    OwnerFlatWithholdingReceipts,
    CompanyCapabilities {
        company: CompanyId,
    },
    OwnerRightsOfferings,
}

/// 仅包装已有领域视图，不重算金额、股数或能力。
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum CompanyReadResult {
    CompanyPeriodExplanation(PeriodChangeExplanation),
    AccountDividendTaxStatus(AccountDividendTaxStatusView),
    DividendTaxOutstandingViews(Vec<DividendTaxOutstandingView>),
    OwnerFlatWithholdingReceipts(Vec<FlatWithholdingReceipt>),
    CompanyCapabilities(CompanyCapabilities),
    OwnerRightsOfferings(Vec<OwnerRightsOfferingView>),
}

/// 保留原始错误源供 Rust 诊断；wire 只交付分类与既有数值 fault。
#[derive(Debug, thiserror::Error)]
pub enum CompanyReadError {
    #[error(transparent)]
    Unhealthy(#[from] StepFatal),
    #[error("公司查询账户不存在：{0:?}")]
    UnknownOwner(AccountId),
    #[error("公司不在本局目录：{0:?}")]
    UnknownCompany(CompanyId),
    #[error(transparent)]
    Catalog(#[from] CompanyCatalogViewError),
    #[error(transparent)]
    Company(#[from] CompanySystemError),
    #[error(transparent)]
    Corporate(#[from] SessionCorporateActionsError),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum CompanyReadFailure {
    Unhealthy,
    UnknownOwner { account: AccountId },
    UnknownCompany { company: CompanyId },
    Catalog { fault: ValidationFault },
    Company { classification: CompanyErrorClass },
    CorporateState,
}

impl CompanyReadError {
    pub fn public_failure(&self) -> CompanyReadFailure {
        match self {
            Self::Unhealthy(_) => CompanyReadFailure::Unhealthy,
            Self::UnknownOwner(account) => CompanyReadFailure::UnknownOwner { account: *account },
            Self::UnknownCompany(company) => {
                CompanyReadFailure::UnknownCompany { company: *company }
            }
            Self::Catalog(error) => CompanyReadFailure::Catalog {
                fault: crate::session::company_catalog_fault(error),
            },
            Self::Company(error) => CompanyReadFailure::Company {
                classification: error.classification(),
            },
            Self::Corporate(_) => CompanyReadFailure::CorporateState,
        }
    }
}

impl ProtocolSession {
    /// 宿主已认证账户只作为内部参数，六项 query 均不创建或推进 World。
    pub fn company_read_query(
        &self,
        owner: AccountId,
        query: CompanyReadQuery,
    ) -> Result<CompanyReadResult, CompanyReadError> {
        let _ = (owner, query);
        Err(CompanySystemError::Unsupported("公司只读注册尚未实现".into()).into())
    }
}

#[cfg(test)]
mod tests;
