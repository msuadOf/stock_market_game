//! B4e 最小只读查询：投影公司 spine 的既有事实，不触发日终或结算。

use super::corporate_actions::{
    taxpayer_identity_of_kind, AccountDividendTaxStatusView, AccountStockDividendTaxStatus,
    DividendTaxOutstandingView, DividendTaxStatus, FlatWithholdingReceipt,
};
use super::{GameSession, SessionCorporateActions, SessionCorporateActionsError};
use crate::company::cash_dividend_tax::CashDividendTaxMode;
use crate::company::simple::period::PeriodChangeExplanation;
use crate::company::{CompanyId, CompanySystemError};
use crate::{AccountId, CivilDate};

impl GameSession {
    /// 读取公司已结算期间的变化解释，不生成新期间；领域错误原样透传。
    pub fn company_period_explanation(
        &self,
        company: &CompanyId,
        period_end: CivilDate,
    ) -> Result<PeriodChangeExplanation, CompanySystemError> {
        self.company_system()
            .ok_or_else(|| {
                CompanySystemError::Unsupported(format!(
                    "当前会话未配置公司系统，不能查询公司 {} 在 {period_end} 的期间解释",
                    company.0
                ))
            })?
            .period_change_explanation(company, period_end)
    }

    /// 汇总账户身份与各完整名册证券的税账状态，不自动开账。
    /// 无名册时 stocks 为空；无公司线或未知账户显式报错。
    pub fn account_dividend_tax_status(
        &self,
        account: AccountId,
    ) -> Result<AccountDividendTaxStatusView, SessionCorporateActionsError> {
        let (actions, mode) = self.company_tax_query_context()?;
        let identity = taxpayer_identity_of_kind(
            self.state
                .accounts
                .get(&account)
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid(format!(
                        "查询股息税状态的账户 {account:?} 不存在"
                    ))
                })?
                .kind(),
        );
        let stocks = actions
            .registries
            .iter()
            .map(|registry| AccountStockDividendTaxStatus {
                stock: registry.stock().clone(),
                status: if mode == CashDividendTaxMode::FlatWithholding {
                    DividendTaxStatus::FlatWithholding
                } else if actions
                    .dividend_tax_books
                    .iter()
                    .any(|book| book.account() == account && book.stock() == registry.stock())
                {
                    DividendTaxStatus::IndividualPublicMarket
                } else {
                    DividendTaxStatus::TreatmentNotConfigured
                },
            })
            .collect();
        Ok(AccountDividendTaxStatusView {
            mode,
            identity,
            stocks,
        })
    }

    /// 汇总所有已配置个人税账的未划收税额，不触发划收。
    /// 这是完整引擎诊断面，宿主交付账户数据时须另做访问权限过滤。
    pub fn dividend_tax_outstanding_views(
        &self,
    ) -> Result<Vec<DividendTaxOutstandingView>, SessionCorporateActionsError> {
        let (actions, _) = self.company_tax_query_context()?;
        actions.dividend_tax_outstanding_views()
    }

    /// 读取本人 FlatWithholding 冻结回执；非 Flat 模式显式拒绝。
    pub fn owner_flat_withholding_receipts(
        &self,
        account: AccountId,
    ) -> Result<Vec<FlatWithholdingReceipt>, SessionCorporateActionsError> {
        let (actions, mode) = self.company_tax_query_context()?;
        if mode != CashDividendTaxMode::FlatWithholding {
            return Err(SessionCorporateActionsError::Invalid(
                "当前会话现金分红税务模式非 FlatWithholding（简税），无付款日代扣回执可查".into(),
            ));
        }
        if !self.state.accounts.contains_key(&account) {
            return Err(SessionCorporateActionsError::Invalid(format!(
                "查询简税代扣回执的账户 {account:?} 不存在"
            )));
        }
        Ok(actions
            .flat_withholding_receipts
            .iter()
            .filter(|receipt| receipt.account == account)
            .cloned()
            .collect())
    }

    fn company_tax_query_context(
        &self,
    ) -> Result<(&SessionCorporateActions, CashDividendTaxMode), SessionCorporateActionsError> {
        let actions = self.corporate_actions().ok_or_else(|| {
            SessionCorporateActionsError::Invalid(
                "当前会话未配置公司行为状态，不能查询现金分红税务事实".into(),
            )
        })?;
        let mode = self.state.setup.dividend_tax_mode.ok_or_else(|| {
            SessionCorporateActionsError::Invalid(
                "当前会话公司行为状态缺少现金分红税务模式，不能解释税务事实".into(),
            )
        })?;
        Ok((actions, mode))
    }
}
