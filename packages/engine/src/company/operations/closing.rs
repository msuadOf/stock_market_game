//! 公司域的自然日关账入口：会话只传日期、集团事实与候选登记簿，不读取账套。

use crate::accounting::closing::{ClosingEngine, ClosingError};
use crate::accounting::AccountingPeriod;
use crate::calendar::{CivilDate, CivilDateError};
use crate::company::groups::GroupStructure;
use crate::company::operations::CompanyOperations;
use crate::information::InformationError;

/// 关账接缝错误；保持各领域的强类型原因，不引入新的运行时 String 载荷。
#[derive(Debug, thiserror::Error)]
pub enum CompanyCloseError {
    #[error(transparent)]
    Date(#[from] CivilDateError),
    #[error(transparent)]
    Closing(Box<ClosingError>),
    #[error(transparent)]
    Information(Box<InformationError>),
    /// 关账调用时点非法，载荷只有错误码和数值上下文。
    #[error("{0}")]
    Fault(crate::error::ValidationFault),
}

/// 定稿材料的发布基础；只返回公开契约的身份/政策，不把账套交给 Session。
pub(crate) struct RegisteredCompanyReport {
    pub scope: crate::accounting::consolidation::ScopeId,
    pub policy: crate::information::AccountingPolicyRef,
}

impl CompanyOperations {
    /// 在候选登记簿内准备单体定稿材料；此入口本身不发布、不更新 NPC 获知。
    pub(crate) fn prepare_report_for_publication(
        &self,
        company: crate::company::CompanyId,
        period: AccountingPeriod,
        kind: crate::accounting::reports::ReportKind,
        closing: &mut ClosingEngine,
    ) -> Result<RegisteredCompanyReport, CompanyCloseError> {
        let source = self.company(&company).ok_or_else(|| CompanyCloseError::Fault(
            crate::error::ValidationFault::new(4002, vec![u64::from(company.0)]),
        ))?;
        let books = source.books().books();
        let member = crate::accounting::consolidation::MemberId(company.0);
        let industry = crate::information::industry_presentation(source.spec().kind);
        crate::information::ensure_original_registered(closing, books, &member, industry, period, kind)
            .map_err(|source| CompanyCloseError::Information(Box::new(source)))?;
        Ok(RegisteredCompanyReport {
            scope: crate::accounting::consolidation::ScopeId::Standalone(member),
            policy: crate::information::AccountingPolicyRef { chart_version: books.ledger().chart().version() },
        })
    }

    /// 当日经营完成后的期间关账；非月末返回 `None`，月末返回实际已关期间。
    /// 入参与修改都属于外层私有 candidate；领域入口不额外复制整个 Session。
    pub fn close_accounting_periods(
        &mut self,
        settled: CivilDate,
        groups: &[GroupStructure],
        closing: &mut ClosingEngine,
    ) -> Result<Option<AccountingPeriod>, CompanyCloseError> {
        let next = settled.next()?;
        if self.next_expected_date() != next {
            let expected = self.next_expected_date();
            let (year, month, day) = settled.ymd();
            let (expect_year, expect_month, expect_day) = expected.ymd();
            return Err(CompanyCloseError::Fault(crate::error::ValidationFault::new(
                4000,
                vec![crate::error::ymd_context(year, month, day),
                    crate::error::ymd_context(expect_year, expect_month, expect_day)],
            )));
        }
        if next.month() == settled.month() {
            return Ok(None);
        }
        let period = AccountingPeriod::of_date(settled);
        let year_end = settled.month() == 12;
        self.invalidate_hash_projection();
        for (id, company) in &mut self.companies {
            let member = crate::accounting::consolidation::MemberId(id.0);
            let industry = crate::information::industry_presentation(company.spec().kind);
            let books = company.books_mut();
            if year_end {
                closing.close_year(books, &member, industry, settled.year())
                    .map_err(|source| CompanyCloseError::Closing(Box::new(source)))?;
            } else {
                closing.close_month(books, &member, industry, period)
                    .map_err(|source| CompanyCloseError::Closing(Box::new(source)))?;
            }
        }
        crate::company::groups::record_group_periods(groups, self, closing, period)
            .map_err(|source| CompanyCloseError::Information(Box::new(source)))?;
        Ok(Some(period))
    }
}
