//! 从当前 Session 已安装的资本事实读取批准口径；不执行资本动作或补造来源。

use super::company_capital_restore::{
    validate_company_capital_restore, CapitalRestoreInput, CompanyCapitalRestoreError,
};
use super::company_state::SessionCompanies;
use super::{GameSession, SessionError};
use crate::accounting::AccountingAmount;
use crate::company::{CompanyId, CompanySystem, CompanySystemError};
use crate::{CivilDate, Money, StockCode, StockId};
use std::sync::Arc;

/// 只在同一次受理作用域内存在，不允许从 JSON 或调用方自报资本口径。
pub(super) struct CapitalApprovalContext {
    company: CompanyId,
    stock: StockId,
    code: StockCode,
    as_of: CivilDate,
    receipt_frontier: usize,
    issued_shares: u64,
    registered_capital: AccountingAmount,
    par_value: Money,
    source: Arc<CompanySystem>,
    prepared_revision: u64,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum CapitalApprovalError {
    #[error("资本批准输入失败：{0}")]
    Session(#[from] SessionError),
    #[error("资本批准公司事实失败：{0}")]
    Company(#[from] CompanySystemError),
    #[error("资本批准已安装事实不一致：{0}")]
    Replay(#[from] CompanyCapitalRestoreError),
    #[error("资本批准缺少所选 CompanySystem")]
    MissingSystem,
    #[error("资本批准缺少公司 {company} 的完整在册资本事实")]
    MissingFacts { company: CompanyId },
    #[error("公司 {company} 的资本 {capital:?} 与已发行 {shares} 股不能表示正整数分面值")]
    InvalidPar {
        company: CompanyId,
        capital: AccountingAmount,
        shares: u64,
    },
}

impl CapitalApprovalContext {
    pub(super) fn prepare(
        session: &GameSession,
        company: CompanyId,
    ) -> Result<Self, CapitalApprovalError> {
        let SessionCompanies::CompanySystem(state) = &session.state.companies else {
            return Err(CapitalApprovalError::MissingSystem);
        };
        let initial = crate::company::identity::IssuerRegistry::new(
            super::company_assembly::issuer_specs(&session.state.setup)?,
        )?;
        let day = session.civil_date();
        // 复用完整双向校验读取当前已安装事实；不是生产执行回执。当前日还可能已有
        // 合法批准，不能沿日终 restore 的前一日截止而误拒同日后续提案。
        let replay = validate_company_capital_restore(CapitalRestoreInput {
            setup_stocks: &session.state.setup.stocks,
            setup_issuers: &initial,
            system: &state.system,
            actions: &state.corporate_actions,
            catalog: &session.state.stocks,
            calendar: session.state.civil_clock.calendar(),
            committed_through: day,
            approved_through: day,
            initial_par: session
                .state
                .setup
                .auto_corporate_foundation
                .then_some(session.state.setup.par_value_per_share)
                .flatten(),
        })?;
        let missing = || CapitalApprovalError::MissingFacts { company };
        let facts = replay.companies.get(&company).ok_or_else(missing)?;
        let stock = facts.stock.ok_or_else(missing)?;
        let code = state
            .system
            .issuers()
            .get(&company)
            .and_then(|issuer| issuer.listed_stock.clone())
            .ok_or_else(missing)?;
        if session.state.stocks.id(&code.0) != Some(stock) {
            return Err(missing());
        }
        let capital = facts.registered_capital.ok_or_else(missing)?;
        let shares = facts.issued_shares;
        let invalid = || CapitalApprovalError::InvalidPar {
            company,
            capital,
            shares,
        };
        if shares == 0 || capital.cents() <= 0 || capital.cents() % i128::from(shares) != 0 {
            return Err(invalid());
        }
        let cents = i64::try_from(capital.cents() / i128::from(shares))
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(invalid)?;
        Ok(Self {
            company,
            stock,
            code,
            as_of: day,
            receipt_frontier: facts.receipt_frontier,
            issued_shares: shares,
            registered_capital: capital,
            par_value: Money::from_cents(cents),
            source: Arc::clone(&state.system),
            prepared_revision: session.external_revision(),
        })
    }

    pub(super) fn company(&self) -> CompanyId {
        self.company
    }

    pub(super) fn code(&self) -> &StockCode {
        &self.code
    }

    pub(super) fn as_of(&self) -> CivilDate {
        self.as_of
    }

    pub(super) fn par_value(&self) -> Money {
        self.par_value
    }

    pub(super) fn registered_capital(&self) -> AccountingAmount {
        self.registered_capital
    }

    pub(super) fn matches_source(&self, system: &CompanySystem) -> bool {
        std::ptr::eq(self.source.as_ref(), system)
    }

    pub(super) fn matches_prepared_revision(&self, session: &GameSession) -> bool {
        self.prepared_revision == session.external_revision()
    }

    /// lazy driver 的安装可消费 revision；公司来源与精确名册 frontier 仍须相同。
    pub(super) fn matches_current(&self, session: &GameSession) -> bool {
        let SessionCompanies::CompanySystem(state) = &session.state.companies else {
            return false;
        };
        session.civil_date() == self.as_of
            && Arc::ptr_eq(&state.system, &self.source)
            && session.state.stocks.id(&self.code.0) == Some(self.stock)
            && state.corporate_actions.registries.iter().any(|registry| {
                registry.issuer() == &self.company
                    && registry.stock() == &self.code
                    && registry.receipts().len() == self.receipt_frontier
                    && registry.issued_shares() == self.issued_shares
            })
    }
}
