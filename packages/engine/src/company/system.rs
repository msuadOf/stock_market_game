use super::{
    CompanyId, CompanySpec, api::*, capabilities::CompanyCapabilities, config::CompanySystemConfig,
    identity::IssuerRegistry, simple::SimpleFundamentals,
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
    pub fn distributable_profit(
        &self,
        company: &CompanyId,
    ) -> Result<super::dividend::DistributableProfit, CompanySystemError> {
        Ok(self.finance(company)?.distributable_profit()?)
    }

    pub fn define_dividend_legal_facts(
        &mut self,
        company: &CompanyId,
        registered_capital: crate::accounting::AccountingAmount,
        source_evidence: String,
    ) -> Result<(), CompanySystemError> {
        self.finance_mut(company)?.define_dividend_legal_facts(
            super::dividend::DividendLegalFacts {
                registered_capital,
                source_evidence,
            },
        )?;
        self.hash_cache = CompanySystemHashCache::default();
        Ok(())
    }

    pub fn declare_dividend(
        &mut self,
        company: &CompanyId,
        declaration: super::dividend::DividendDeclaration,
    ) -> Result<super::dividend::DividendPlanReceipt, CompanySystemError> {
        let result = self.finance_mut(company)?.declare_dividend(declaration)?;
        self.hash_cache = CompanySystemHashCache::default();
        Ok(result)
    }

    pub fn pay_dividend(
        &mut self,
        company: &CompanyId,
        plan_id: &str,
        payment_id: &str,
        paid_on: CivilDate,
        amount: crate::accounting::AccountingAmount,
    ) -> Result<super::dividend::DividendPaymentReceipt, CompanySystemError> {
        let result = self
            .finance_mut(company)?
            .pay_dividend(plan_id, payment_id, paid_on, amount)?;
        self.hash_cache = CompanySystemHashCache::default();
        Ok(result)
    }

    pub fn dividend_plan_facts(
        &self,
        company: &CompanyId,
    ) -> Result<Vec<super::dividend::DividendPlanFact>, CompanySystemError> {
        Ok(self.finance(company)?.dividend_plan_facts()?)
    }

    pub fn dividend_legal_facts(
        &self,
        company: &CompanyId,
    ) -> Result<Option<super::dividend::DividendLegalFacts>, CompanySystemError> {
        Ok(self.finance(company)?.legal_facts().clone())
    }

    pub fn stock_distribution_facts(
        &self,
        company: &CompanyId,
    ) -> Result<Vec<super::stock_distribution::StockDistributionFinanceFact>, CompanySystemError>
    {
        Ok(self.finance(company)?.stock_distribution_facts()?)
    }

    /// 冻结 Simple 账面送转展示事实；不做借贷过账、不产生现金。
    pub fn declare_stock_distribution(
        &mut self,
        company: &CompanyId,
        declaration: super::stock_distribution::StockDistributionDeclaration,
    ) -> Result<bool, CompanySystemError> {
        let already_declared = self
            .finance_mut(company)?
            .declare_stock_distribution(declaration)?;
        Ok(already_declared)
    }

    /// 真实新股入账后回填账面事实并同步发行人已发行股数；两者在同一候选上变更。
    pub fn record_stock_distribution_credit(
        &mut self,
        company: &CompanyId,
        event_id: &str,
        credited_on: CivilDate,
        new_shares: u64,
    ) -> Result<bool, CompanySystemError> {
        let already_credited = self
            .finance_mut(company)?
            .record_stock_distribution_credit(event_id, credited_on, new_shares)?;
        if !already_credited {
            self.issuers.record_share_issuance(company, new_shares)?;
        }
        Ok(already_credited)
    }

    /// 校验 Session 送转账簿与 Simple 账面事实的跨域对应。
    pub fn validate_stock_distribution_books(
        &self,
        books: &[super::stock_distribution::StockDistributionBook],
    ) -> Result<(), CompanySystemError> {
        use std::collections::{BTreeMap, BTreeSet};
        let mut seen_events = BTreeSet::new();
        for book in books {
            let plan = book.plan();
            if !seen_events.insert(plan.event_id.clone()) {
                return Err(CompanySystemError::Invalid(format!(
                    "送转事件 {} 重复出现账簿",
                    plan.event_id
                )));
            }
            let issuer = self
                .issuers
                .get(&plan.issuer)
                .ok_or_else(|| {
                    CompanySystemError::Invalid(format!(
                        "送转账簿引用未知发行人 {}",
                        plan.issuer.0
                    ))
                })?;
            if issuer.listed_stock.as_ref() != Some(&plan.stock) {
                return Err(CompanySystemError::Invalid(format!(
                    "送转事件 {} 的证券与发行人不匹配",
                    plan.event_id
                )));
            }
            let fact = self
                .stock_distribution_facts(&plan.issuer)?
                .into_iter()
                .find(|fact| fact.event_id == plan.event_id)
                .ok_or_else(|| {
                    CompanySystemError::Invalid(format!(
                        "送转事件 {} 缺少 Simple 声明事实",
                        plan.event_id
                    ))
                })?;
            if fact.approved_on != plan.approved_on
                || fact.approval_reference != plan.approval_reference
                || fact.kind != plan.kind
                || fact.new_shares != plan.approved_total_new_shares
            {
                return Err(CompanySystemError::Invalid(format!(
                    "送转事件 {} 与 Simple 声明的日期、引用、类别或股数不一致",
                    plan.event_id
                )));
            }
            match (book.credited_on(), fact.credited_on) {
                (Some(book_date), Some(fact_date)) if book_date == fact_date => {}
                (None, None) => {}
                _ => {
                    return Err(CompanySystemError::Invalid(format!(
                        "送转事件 {} 的入账事实与 Simple 账面回填不一致",
                        plan.event_id
                    )));
                }
            }
        }
        let declared: BTreeMap<_, _> = self
            .issuers
            .iter()
            .filter_map(|(company, _)| {
                self.stock_distribution_facts(company)
                    .ok()
                    .map(|facts| (company.clone(), facts))
            })
            .collect();
        let mut bound_events = BTreeSet::new();
        for book in books {
            bound_events.insert(book.plan().event_id.clone());
        }
        for (company, facts) in declared {
            for fact in facts {
                if !bound_events.contains(&fact.event_id) {
                    return Err(CompanySystemError::Invalid(format!(
                        "公司 {} 存在未绑定账簿的送转声明 {}",
                        company.0, fact.event_id
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn dividend_payment_facts(
        &self,
        company: &CompanyId,
        plan_id: &str,
    ) -> Result<Option<Vec<super::dividend::DividendPaymentFact>>, CompanySystemError> {
        Ok(self.finance(company)?.dividend_payment_facts(plan_id))
    }

    pub fn validate_cash_dividend_books(
        &self,
        books: &[super::cash_dividend::CashDividendBook],
    ) -> Result<(), CompanySystemError> {
        use std::collections::{BTreeMap, BTreeSet};
        let mut seen = BTreeSet::new();
        if books
            .iter()
            .any(|book| self.issuers.get(&book.plan().issuer).is_none())
        {
            return Err(CompanySystemError::Invalid(
                "现金分红账簿引用未知发行人".into(),
            ));
        }
        for (company, _) in self.issuers.iter() {
            let plans = self.dividend_plan_facts(company)?;
            let plan_ids = plans
                .iter()
                .map(|plan| plan.plan_id.clone())
                .collect::<BTreeSet<_>>();
            let issuer_books = books
                .iter()
                .filter(|book| &book.plan().issuer == company)
                .collect::<Vec<_>>();
            let by_plan = issuer_books
                .iter()
                .map(|book| (book.plan().plan_id.as_str(), *book))
                .collect::<BTreeMap<_, _>>();
            if by_plan.len() != issuer_books.len() {
                return Err(CompanySystemError::Invalid(format!(
                    "公司 {} 存在重复现金分红计划",
                    company.0
                )));
            }
            for plan in plans {
                let book = by_plan.get(plan.plan_id.as_str()).ok_or_else(|| {
                    CompanySystemError::Invalid(format!(
                        "Simple 分红计划 {} 缺少对应现金分红账簿",
                        plan.plan_id
                    ))
                })?;
                let cash_plan = book.plan();
                let cash_limit =
                    crate::accounting::AccountingAmount::from_money(cash_plan.distributable_amount);
                let registered_gross = if book.registration().is_some() {
                    Some(crate::accounting::AccountingAmount::from_money(
                        book.total_gross()
                            .map_err(|error| CompanySystemError::Invalid(error.to_string()))?,
                    ))
                } else {
                    None
                };
                if cash_plan.approved_on != plan.approved_on
                    || plan.total_gross > cash_limit
                    || registered_gross.is_some_and(|gross| gross != plan.total_gross)
                    || plan.registered_capital_source_evidence.trim().is_empty()
                    || cash_plan.issuer != *company
                {
                    return Err(CompanySystemError::Invalid(format!(
                        "现金分红计划 {} 与 Simple 批准金额、日期或法律事实来源不一致",
                        plan.plan_id
                    )));
                }
                let finance_payments = plan
                    .payments
                    .iter()
                    .map(|payment| (payment.payment_id.as_str(), payment))
                    .collect::<BTreeMap<_, _>>();
                if finance_payments.len() != plan.payments.len() {
                    return Err(CompanySystemError::Invalid(format!(
                        "Simple 分红计划 {} 存在重复付款批次",
                        plan.plan_id
                    )));
                }
                for receipt in book.payments() {
                    let successful = receipt
                        .outcomes()
                        .iter()
                        .try_fold(crate::money::Money::ZERO, |total, outcome| match outcome {
                            super::cash_dividend::HolderPaymentOutcome::Paid { amount, .. } => {
                                total.add(*amount)
                            }
                            super::cash_dividend::HolderPaymentOutcome::Failed { .. } => Ok(total),
                        })
                        .map_err(|error| CompanySystemError::Invalid(error.to_string()))?;
                    let Some(payment) = finance_payments.get(receipt.payment_id()) else {
                        if successful.cents() == 0 {
                            continue;
                        }
                        return Err(CompanySystemError::Invalid(format!(
                            "成功现金到账批次 {} 缺少 Simple 付款凭证",
                            receipt.payment_id()
                        )));
                    };
                    if payment.paid_on != receipt.paid_on()
                        || crate::accounting::AccountingAmount::from_money(successful)
                            != payment.amount
                    {
                        return Err(CompanySystemError::Invalid(format!(
                            "付款批次 {} 的 Simple 账簿与持有人到账不一致",
                            receipt.payment_id()
                        )));
                    }
                    seen.insert((
                        company.clone(),
                        plan.plan_id.clone(),
                        payment.payment_id.clone(),
                    ));
                }
                for payment in plan.payments {
                    if !seen.contains(&(
                        company.clone(),
                        plan.plan_id.clone(),
                        payment.payment_id.clone(),
                    )) {
                        return Err(CompanySystemError::Invalid(format!(
                            "Simple 付款批次 {} 缺少匹配的持有人到账回执",
                            payment.payment_id
                        )));
                    }
                }
            }
            if by_plan.keys().any(|plan_id| !plan_ids.contains(*plan_id)) {
                return Err(CompanySystemError::Invalid(format!(
                    "公司 {} 存在未绑定 Simple 声明的现金分红计划",
                    company.0
                )));
            }
        }
        Ok(())
    }

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
                ));
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
