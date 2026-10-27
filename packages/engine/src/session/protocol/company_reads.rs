//! 三宿主共用的公司只读边界；身份由宿主提供，不属于 wire 请求。

use super::ProtocolSession;
use crate::company::capabilities::CompanyCapabilities;
use crate::company::simple::period::PeriodChangeExplanation;
use crate::company::CompanyErrorClass;
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
    #[error("未划收股息税汇总失败：{source}")]
    DividendTaxOutstanding {
        #[source]
        source: SessionCorporateActionsError,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum CompanyReadFailure {
    Unhealthy,
    UnknownOwner {
        account: AccountId,
    },
    UnknownCompany {
        company: CompanyId,
    },
    Catalog {
        fault: ValidationFault,
    },
    Company {
        classification: CompanyErrorClass,
        reason: String,
    },
    CorporateState {
        reason: String,
    },
    DividendTaxOutstanding {
        reason: String,
    },
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
                reason: error.to_string(),
            },
            Self::Corporate(SessionCorporateActionsError::Invalid(reason)) => CompanyReadFailure::CorporateState { reason: reason.clone() },
            Self::Corporate(SessionCorporateActionsError::RegistryPositionMismatch { .. }) => CompanyReadFailure::CorporateState {
                reason: "公司权益的持仓一致性校验失败，无法生成本人结果；请连同查询类型和当前会话信息反馈，公开错误不包含其他账户持仓".into(),
            },
            Self::DividendTaxOutstanding { .. } => CompanyReadFailure::DividendTaxOutstanding {
                reason: "未划收股息税汇总读取失败，无法生成本人税额结果；完整原因可能涉及其他账户事实，公开错误不包含这些明细。请连同查询类型和当前会话信息反馈".into(),
            },
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
        let game = self.game();
        game.require_healthy()?;
        if !game.state.accounts.contains_key(&owner) {
            return Err(CompanyReadError::UnknownOwner(owner));
        }
        let company = match &query {
            CompanyReadQuery::CompanyPeriodExplanation { company, .. }
            | CompanyReadQuery::CompanyCapabilities { company } => Some(*company),
            _ => None,
        };
        if let Some(company) = company {
            if !game
                .company_catalog_view()?
                .iter()
                .any(|entry| entry.id == company)
            {
                return Err(CompanyReadError::UnknownCompany(company));
            }
        }
        Ok(match query {
            CompanyReadQuery::CompanyPeriodExplanation {
                company,
                period_end,
            } => CompanyReadResult::CompanyPeriodExplanation(
                game.company_period_explanation(&company, period_end)?,
            ),
            CompanyReadQuery::AccountDividendTaxStatus => {
                CompanyReadResult::AccountDividendTaxStatus(
                    game.account_dividend_tax_status(owner)?,
                )
            }
            CompanyReadQuery::DividendTaxOutstandingViews => {
                CompanyReadResult::DividendTaxOutstandingViews(
                    game.dividend_tax_outstanding_views()
                        .map_err(|source| CompanyReadError::DividendTaxOutstanding { source })?
                        .into_iter()
                        .filter(|view| view.account == owner)
                        .collect(),
                )
            }
            CompanyReadQuery::OwnerFlatWithholdingReceipts => {
                CompanyReadResult::OwnerFlatWithholdingReceipts(
                    game.owner_flat_withholding_receipts(owner)?,
                )
            }
            CompanyReadQuery::CompanyCapabilities { company } => {
                CompanyReadResult::CompanyCapabilities(game.company_capabilities(&company, owner)?)
            }
            CompanyReadQuery::OwnerRightsOfferings => {
                CompanyReadResult::OwnerRightsOfferings(game.owner_rights_offerings(owner)?)
            }
        })
    }
}

#[cfg(test)]
mod tests;
