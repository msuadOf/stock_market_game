use super::{
    api::*, capabilities::CompanyCapabilities, config::CompanySystemConfig,
    identity::IssuerRegistry, simple::SimpleFundamentals, CompanyId, CompanySpec,
};
use crate::calendar::CivilDate;

#[derive(Debug, thiserror::Error)]
pub enum CompanySystemError {
    #[error("公司系统输入非法：{0}")]
    Invalid(String),
    #[error("公司系统尚不支持：{0}")]
    Unsupported(String),
    #[error(transparent)]
    Accounting(#[from] crate::accounting::AccountingError),
    #[error(transparent)]
    Date(#[from] crate::calendar::CivilDateError),
    #[error(transparent)]
    Finance(#[from] super::simple::SimpleFinanceError),
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "mode", content = "state", deny_unknown_fields)]
pub(crate) enum CompanyImplementation {
    Simple(SimpleFundamentals),
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize)]
pub struct CompanySystem {
    pub(crate) issuers: IssuerRegistry,
    pub(crate) implementation: CompanyImplementation,
    #[serde(skip)]
    pub(crate) hash_cache: CompanySystemHashCache,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub(crate) struct CompanySystemHashProjection {
    serialized_len: usize,
    digest: u64,
}

#[derive(Default, Debug)]
pub(crate) struct CompanySystemHashCache(std::sync::OnceLock<CompanySystemHashProjection>);

impl Clone for CompanySystemHashCache {
    fn clone(&self) -> Self {
        let clone = Self::default();
        if let Some(projection) = self.0.get() {
            let _ = clone.0.set(*projection);
        }
        clone
    }
}

impl PartialEq for CompanySystemHashCache {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}
impl Eq for CompanySystemHashCache {}

impl CompanySystem {
    pub fn create(
        issuers: Vec<CompanySpec>,
        config: CompanySystemConfig,
        start_date: CivilDate,
        seed: u64,
    ) -> Result<Self, CompanySystemError> {
        let issuers = IssuerRegistry::new(issuers)?;
        let implementation = match config {
            CompanySystemConfig::Simple(config) => CompanyImplementation::Simple(
                SimpleFundamentals::create(&issuers, config, start_date, seed)?,
            ),
            CompanySystemConfig::Simulation => {
                return Err(CompanySystemError::Unsupported(
                    "Simulation 在独立分支实现；当前不能创建".into(),
                ))
            }
        };
        Ok(Self {
            issuers,
            implementation,
            hash_cache: CompanySystemHashCache::default(),
        })
    }
    pub fn issuers(&self) -> &IssuerRegistry {
        &self.issuers
    }
    pub fn advanced_through(&self) -> CivilDate {
        match &self.implementation {
            CompanyImplementation::Simple(state) => state.advanced_through,
        }
    }
    pub fn config(&self) -> CompanySystemConfig {
        match &self.implementation {
            CompanyImplementation::Simple(state) => {
                CompanySystemConfig::Simple(state.config.clone())
            }
        }
    }
    pub fn advance_day(
        &mut self,
        date: CivilDate,
    ) -> Result<Vec<SimpleDisclosureCandidate>, CompanySystemError> {
        let result = match &mut self.implementation {
            CompanyImplementation::Simple(state) => state.advance_day(date),
        };
        if result.is_ok() {
            self.hash_cache = CompanySystemHashCache::default();
        }
        result
    }
    pub(crate) fn history(&self) -> &[SimpleDisclosureCandidate] {
        match &self.implementation {
            CompanyImplementation::Simple(state) => &state.history,
        }
    }
    pub(crate) fn finance(
        &self,
        company: &CompanyId,
    ) -> Result<&super::simple::SimpleFinanceState, CompanySystemError> {
        match &self.implementation {
            CompanyImplementation::Simple(state) => state
                .companies
                .get(company)
                .map(|company| &company.finance)
                .ok_or_else(|| CompanySystemError::Invalid(format!("未知公司 {}", company.0))),
        }
    }
    pub(crate) fn finance_mut(
        &mut self,
        company: &CompanyId,
    ) -> Result<&mut super::simple::SimpleFinanceState, CompanySystemError> {
        self.hash_cache = CompanySystemHashCache::default();
        match &mut self.implementation {
            CompanyImplementation::Simple(state) => state
                .companies
                .get_mut(company)
                .map(|company| &mut company.finance)
                .ok_or_else(|| CompanySystemError::Invalid(format!("未知公司 {}", company.0))),
        }
    }
    pub(crate) fn closed_reports(
        &self,
        company: &CompanyId,
    ) -> Result<Vec<crate::accounting::reports::ReportSet>, CompanySystemError> {
        let finance = self.finance(company)?;
        Ok(finance.closing().latest_reports_for_scope(
            &crate::accounting::consolidation::ScopeId::Standalone(finance.member_id()),
        ))
    }
    pub(crate) fn closing_for(
        &self,
        company: &CompanyId,
    ) -> Result<&crate::accounting::closing::ClosingEngine, CompanySystemError> {
        Ok(self.finance(company)?.closing())
    }
    pub(crate) fn accounting_policy_for(
        &self,
        company: &CompanyId,
    ) -> Result<crate::information::AccountingPolicyRef, CompanySystemError> {
        Ok(crate::information::AccountingPolicyRef {
            chart_version: self.finance(company)?.books().ledger().chart().version(),
        })
    }
    pub(crate) fn report_availability(
        &self,
        company: &CompanyId,
        period: crate::accounting::AccountingPeriod,
        kind: crate::accounting::reports::ReportKind,
    ) -> Result<CompanyReportAvailability, CompanySystemError> {
        let finance = self.finance(company)?;
        let scope = crate::accounting::consolidation::ScopeId::Standalone(finance.member_id());
        if !finance.closing().versions(&scope, period, kind).is_empty() {
            return Ok(CompanyReportAvailability::Available);
        }
        let ((first, last), _) = kind
            .resolve(period)
            .map_err(|error| CompanySystemError::Invalid(error.to_string()))?;
        let first_date = CivilDate::from_ymd(first.year(), first.month(), 1)?;
        let end_date = if last.month() == 12 {
            CivilDate::from_ymd(last.year(), 12, 31)?
        } else {
            CivilDate::from_ymd(last.year(), last.month() + 1, 1)?.prev()?
        };
        if first_date <= finance.opening_date() {
            Ok(CompanyReportAvailability::BeforeOpening)
        } else if end_date > finance.as_of() {
            Ok(CompanyReportAvailability::NotYetSettled)
        } else {
            Ok(CompanyReportAvailability::PeriodNotRepresented)
        }
    }
    pub fn capabilities(
        &self,
        company: &CompanyId,
    ) -> Result<CompanyCapabilities, CompanySystemError> {
        self.finance(company)?;
        Ok(CompanyCapabilities {
            revenue: true,
            net_income: true,
            equity: true,
            cash_flow: true,
            full_financial_statements: true,
            cash_settlement: false,
            unsupported_reason:
                "汇总财务已接通；共同股本行为的实际投资者结算仍待接线，不能按模式缺资金拒绝或冒称已支持"
                    .into(),
        })
    }
    pub fn submit_command(&mut self, command: CompanyCommand) -> Result<(), CompanySystemError> {
        let result = match &mut self.implementation {
            CompanyImplementation::Simple(state) => state.submit_command(command),
        };
        if result.is_ok() {
            self.hash_cache = CompanySystemHashCache::default();
        }
        result
    }

    pub(crate) fn hash_projection(&self) -> Result<CompanySystemHashProjection, serde_json::Error> {
        if let Some(projection) = self.hash_cache.0.get() {
            return Ok(*projection);
        }
        let bytes = serde_json::to_vec(self)?;
        let digest = bytes.iter().fold(0xcbf29ce484222325_u64, |digest, byte| {
            (digest ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
        let projection = CompanySystemHashProjection {
            serialized_len: bytes.len(),
            digest,
        };
        let _ = self.hash_cache.0.set(projection);
        Ok(projection)
    }
}
