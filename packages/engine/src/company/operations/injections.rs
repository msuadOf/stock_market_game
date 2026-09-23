//! 结构化冲击注入与滚动利息排队（`CompanyOperations` 的写入面拆分；
//! 装配/访问在 `core.rs`，逐日循环在 `day.rs`）。

use crate::calendar::CivilDate;
use crate::company::events::{ActiveShock, ShockKind};
use crate::company::operations::core::CompanyOperations;
use crate::company::operations::error::OperationsError;
use crate::company::operations::history::HistoryMeta;
use crate::company::rng::{OperatingRng, RngStream};
use crate::company::spec::{CompanyId, CompanyKind};

impl CompanyOperations {
    pub fn submit_due(
        &mut self,
        request: crate::company::scheduler::SchedulerRequest,
    ) -> Result<crate::company::scheduler::ScheduledDueId, OperationsError> {
        Ok(self.scheduler.submit(request)?)
    }

    /// 注入市场级冲击（只收 `MarketDemandShift`；立即进入活跃集）。
    pub fn apply_market_shock(&mut self, shock: ActiveShock) -> Result<(), OperationsError> {
        if !matches!(shock.kind, ShockKind::MarketDemandShift) {
            return Err(OperationsError::ShockKindMismatch {
                actual: shock.kind,
                slot: "market",
            });
        }
        self.activate_where(shock, |_| true)
    }

    /// 注入行业冲击（只收 `IndustryCostShift`；只作用于被标签行业）。
    pub fn apply_industry_shock(&mut self, shock: ActiveShock) -> Result<(), OperationsError> {
        let ShockKind::IndustryCostShift { industry } = &shock.kind else {
            return Err(OperationsError::ShockKindMismatch {
                actual: shock.kind,
                slot: "industry",
            });
        };
        let industry = industry.clone();
        self.activate_where(shock, move |spec| spec.industry == industry)
    }

    /// 注入单公司冲击（公司级事件种类；`starts_on` 应为下一经营日）。
    pub fn apply_company_shock(
        &mut self,
        company: &CompanyId,
        shock: ActiveShock,
    ) -> Result<(), OperationsError> {
        if matches!(
            shock.kind,
            ShockKind::MarketDemandShift | ShockKind::IndustryCostShift { .. }
        ) {
            return Err(OperationsError::ShockKindMismatch {
                actual: shock.kind,
                slot: "company",
            });
        }
        let target = self
            .companies
            .get_mut(company)
            .ok_or(OperationsError::Company(
                crate::company::CompanyError::SpecInvalid {
                    detail: format!("unknown company {company:?} for shock injection"),
                },
            ))?;
        if shock.expires_on < shock.starts_on {
            return Err(OperationsError::InvalidShockWindow {
                starts_on: shock.starts_on,
                expires_on: shock.expires_on,
            });
        }
        target.economy.activate(shock);
        Ok(())
    }

    pub(crate) fn activate_where(
        &mut self,
        shock: ActiveShock,
        predicate: impl Fn(&crate::company::spec::CompanySpec) -> bool,
    ) -> Result<(), OperationsError> {
        if shock.expires_on < shock.starts_on {
            return Err(OperationsError::InvalidShockWindow {
                starts_on: shock.starts_on,
                expires_on: shock.expires_on,
            });
        }
        for company in self.companies.values_mut() {
            if predicate(&company.spec) {
                company.economy.activate(shock.clone());
            }
        }
        Ok(())
    }

    /// 滚动利息排队（有计息承载面的行业才注册；key 含日期可跨日复用）。
    pub(crate) fn submit_rolling_interest(
        &mut self,
        date: CivilDate,
    ) -> Result<(), OperationsError> {
        let ids: Vec<CompanyId> = self
            .companies
            .iter()
            .filter(|(_, company)| company.spec.kind != CompanyKind::Insurance)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            let key = format!("INT:{}:{}", id.0, date.to_iso());
            self.scheduler
                .submit(crate::company::scheduler::SchedulerRequest::Due {
                    key,
                    due_date: date,
                    action: crate::company::scheduler::ScheduledAction::InterestAccrual {
                        company: id,
                    },
                })?;
        }
        Ok(())
    }

    /// 前史完成：切换到 live RNG 流（init 流仅初始化期间使用——live 冲击
    /// 序列与前史长度无关）并记录前史元数据。
    pub(crate) fn finish_history(&mut self, generated_through: CivilDate) {
        self.market_rng = OperatingRng::derive(self.seed, RngStream::MarketShock, "market");
        self.industry_rngs = self
            .industry_rngs
            .keys()
            .map(|industry| {
                (
                    industry.clone(),
                    OperatingRng::derive(self.seed, RngStream::IndustryShock, &industry.0),
                )
            })
            .collect();
        for (id, company) in &mut self.companies {
            company.rng = OperatingRng::derive(self.seed, RngStream::CompanyOperating, &id.0);
        }
        self.history = Some(HistoryMeta { generated_through });
    }
}
