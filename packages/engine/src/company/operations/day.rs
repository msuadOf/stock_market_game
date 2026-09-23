//! 逐自然日推进循环（K4）：到期派发 → 冲击演化/采样 → 行业经营流 → 次日
//! 滚动利息排队。日期严格逐日；任何类型化 `Err` 上抛（经营层内部错误不吞），
//! `PaymentFailed` 在流内捕获为业务状态。

use std::collections::{BTreeMap, BTreeSet};

use crate::calendar::CivilDate;
use crate::company::events::{
    sample_company_shock, sample_industry_shock, sample_market_shock, ActiveShock, ShockKind,
};
use crate::company::operations::config::{FlowParams, IndustryBooks};
use crate::company::operations::core::{
    ActivatedShockRecord, CompanyDayReport, CompanyOperations, ExpiredShockRecord,
    OperatingCompany, PaymentFailureRecord,
};
use crate::company::operations::error::OperationsError;
use crate::company::scheduler::OperatingScheduler;
use crate::company::spec::{CompanyId, IndustryId};

impl CompanyOperations {
    /// 推进一个自然日（权威经营循环）。
    pub fn advance_civil_day(
        &mut self,
        date: CivilDate,
    ) -> Result<CompanyDayReport, OperationsError> {
        if self.next_expected != Some(date) {
            return Err(OperationsError::DateOutOfSequence {
                expected: self.next_expected_date(),
                got: date,
            });
        }
        let entries_before: usize = self
            .companies
            .values()
            .map(|company| company.books.books().journal().entry_count())
            .sum();
        let mut activated = Vec::new();
        let mut expired = Vec::new();
        let mut payment_failures = Vec::new();

        // 1. 冲击到期（恢复钩子用）。
        for (id, company) in &mut self.companies {
            for shock in company.economy.expire_before(date) {
                expired.push(ExpiredShockRecord {
                    company: id.clone(),
                    kind: shock.kind,
                });
            }
        }
        // 2. 冲击采样：市场 → 行业（IndustryId 序）→ 公司（CompanyId 序）。
        if let Some(shock) = sample_market_shock(&mut self.market_rng, date, &self.shock_params)? {
            activated.push(ActivatedShockRecord {
                company: None,
                kind: shock.kind.clone(),
                amplitude_bp: shock.amplitude_bp,
            });
            for company in self.companies.values_mut() {
                company.economy.activate(shock.clone());
            }
        }
        let mut industry_shocks: Vec<(IndustryId, ActiveShock)> = Vec::new();
        for (industry, rng) in &mut self.industry_rngs {
            if let Some(shock) = sample_industry_shock(rng, industry, date, &self.shock_params)? {
                industry_shocks.push((industry.clone(), shock));
            }
        }
        for (industry, shock) in industry_shocks {
            for (id, company) in &mut self.companies {
                if company.spec.industry == industry {
                    activated.push(ActivatedShockRecord {
                        company: Some(id.clone()),
                        kind: shock.kind.clone(),
                        amplitude_bp: shock.amplitude_bp,
                    });
                    company.economy.activate(shock.clone());
                }
            }
        }
        for (id, company) in &mut self.companies {
            if let Some(shock) = sample_company_shock(&mut company.rng, date, &self.shock_params)? {
                activated.push(ActivatedShockRecord {
                    company: Some(id.clone()),
                    kind: shock.kind.clone(),
                    amplitude_bp: shock.amplitude_bp,
                });
                company.economy.activate(shock);
            }
        }

        // 3. 到期派发（(due_date, id) 稳定序）。
        let dispatched_due = self.dispatch_due_on(date)?;

        // 4. 行业经营流（CompanyId 序）。
        let newly: BTreeMap<CompanyId, Vec<ActiveShock>> = self
            .companies
            .iter()
            .map(|(id, company)| {
                (
                    id.clone(),
                    company
                        .economy
                        .active()
                        .iter()
                        .filter(|shock| shock.starts_on == date)
                        .cloned()
                        .collect(),
                )
            })
            .collect();
        let interruption_recovered: BTreeSet<CompanyId> = expired
            .iter()
            .filter(|record| matches!(record.kind, ShockKind::ProductionInterruption))
            .map(|record| record.company.clone())
            .collect();
        {
            let Self {
                scheduler,
                companies,
                ..
            } = self;
            for (id, company) in companies {
                advance_flow_day(
                    company,
                    date,
                    newly.get(id).map(Vec::as_slice).unwrap_or(&[]),
                    interruption_recovered.contains(id),
                    scheduler,
                    &mut payment_failures,
                )?;
            }
        }

        // 5. 次日滚动利息 + 前进。
        let next = date.next()?;
        self.submit_rolling_interest(next)?;
        self.next_expected = Some(next);
        let posted_entries = self
            .companies
            .values()
            .map(|company| company.books.books().journal().entry_count())
            .sum::<usize>()
            .saturating_sub(entries_before);
        Ok(CompanyDayReport {
            date,
            activated,
            expired,
            payment_failures,
            dispatched_due,
            posted_entries,
        })
    }
}

/// 行业经营流分派器（按账套变体路由；参数与流实现一一对应）。
fn advance_flow_day(
    company: &mut OperatingCompany,
    date: CivilDate,
    newly: &[ActiveShock],
    interruption_recovered: bool,
    scheduler: &mut OperatingScheduler,
    failures: &mut Vec<PaymentFailureRecord>,
) -> Result<(), OperationsError> {
    let aggregates = company.economy.aggregates();
    match (&mut company.books, &company.params) {
        (IndustryBooks::Industrial(books), FlowParams::Industrial(params)) => {
            crate::company::operations::industrial::advance_day(
                company.spec.id.clone(),
                books,
                params,
                date,
                aggregates,
                newly,
                scheduler,
                failures,
                &mut company.next_flow_seq,
            )
        }
        (IndustryBooks::Bank(books), FlowParams::Bank(params)) => {
            crate::company::operations::bank::advance_day(
                company.spec.id.clone(),
                books,
                params,
                date,
                aggregates,
                newly,
                scheduler,
                failures,
                &mut company.next_flow_seq,
            )
        }
        (IndustryBooks::Insurance(books), FlowParams::Insurance(params)) => {
            crate::company::operations::insurance::advance_day(
                company.spec.id.clone(),
                books,
                params,
                date,
                aggregates,
                scheduler,
                failures,
                &mut company.next_flow_seq,
            )
        }
        (IndustryBooks::RealEstate(books), FlowParams::RealEstate(params)) => {
            crate::company::operations::real_estate::advance_day(
                company.spec.id.clone(),
                books,
                params,
                date,
                aggregates,
                newly,
                interruption_recovered,
                scheduler,
                failures,
                &mut company.next_flow_seq,
            )
        }
        _ => Err(OperationsError::KindFlowMismatch {
            company: company.spec.id.clone(),
            kind: company.spec.kind,
            flow: company.params.variant_name(),
        }),
    }
}
