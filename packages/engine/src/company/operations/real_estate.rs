//! 地产日常经营流（K4）：购地（首日）→ 开发投入（成本乘数缩放；中断暂停/
//! 恢复）→ 完工 → 预售签约收款（需求乘数作用于预售节奏；完工后不现售）→
//! 交付到期排队（完工 + lag）。

use crate::accounting::AccountingAmount;
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::counterparty::CounterpartyId;
use crate::company::events::{ActiveShock, ShockKind};
use crate::company::operations::core::PaymentFailureRecord;
use crate::company::operations::error::OperationsError;
use crate::company::operations::state::{add_days, scale_units, EconomyAggregates};
use crate::company::real_estate::{ProjectId, RealEstateBooks, RealEstateError};
use crate::company::scheduler::{OperatingScheduler, ScheduledAction, SchedulerRequest};
use crate::company::spec::CompanyId;

/// 地产经营流参数（显式配置；资本化政策在账套配置侧，版本化游戏假设）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct RealEstateFlowParams {
    pub land_seller: CounterpartyId,
    pub contractor: CounterpartyId,
    pub buyer: CounterpartyId,
    pub project: ProjectId,
    pub total_units: i128,
    pub land_cost: AccountingAmount,
    /// 自首次开发投入起算的开发日数（自然日；中断暂停支出、不延长时钟——
    /// 登记简化）。
    pub development_days: i64,
    pub daily_development_spend: AccountingAmount,
    /// 开工后第 N 日起可预售。
    pub presale_open_day: i64,
    /// 每日预售基数（实际 = 基数 × 需求乘数，受可售套数约束）。
    pub presale_units_per_day: i128,
    pub unit_price: AccountingAmount,
    /// 完工后第 N 日交付已售合同。
    pub delivery_lag_days: i64,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::company::operations) fn advance_day(
    company: CompanyId,
    books: &mut RealEstateBooks,
    params: &RealEstateFlowParams,
    date: CivilDate,
    aggregates: EconomyAggregates,
    newly: &[ActiveShock],
    interruption_recovered: bool,
    scheduler: &mut OperatingScheduler,
    failures: &mut Vec<PaymentFailureRecord>,
    next_flow_seq: &mut i64,
) -> Result<(), OperationsError> {
    // 0. 首个经营日购地（项目建立）。现金不足 = 失败记录，公司继续（无项目面）。
    if books.projects().is_empty() {
        if let Err(error) = books.acquire_land(
            params.project.clone(),
            &params.land_seller,
            params.total_units,
            params.land_cost,
            date,
        ) {
            if matches!(error, RealEstateError::PaymentFailed { .. }) {
                failures.push(PaymentFailureRecord {
                    company: company.clone(),
                    what: "land acquisition".to_string(),
                    amount: params.land_cost,
                });
            } else {
                return Err(error.into());
            }
        }
    }

    let (dev_started_on, mut suspended, mut completed) = match books.project(&params.project) {
        Some(state) => (
            state.dev_started_on(),
            state.interrupted_on().is_some(),
            state.completed_on().is_some(),
        ),
        None => return Ok(()), // 购地失败后无项目面（失败已记录）
    };

    // 1. 中断/恢复（生产中断冲击 → 暂停开发；到期 → 复工）。尚未开工时
    //    不调用 suspend（处理器要求已开工），中断由 `interrupted_now` 直接
    //    封锁首笔投入。
    let interrupted_now = newly
        .iter()
        .any(|shock| matches!(shock.kind, ShockKind::ProductionInterruption));
    if interrupted_now && dev_started_on.is_some() && !suspended && !completed {
        books.suspend_development(&params.project, date)?;
        suspended = true;
    }
    if interruption_recovered && suspended && !completed {
        books.resume_development(&params.project, date)?;
        suspended = false;
    }

    // 2. 开发投入（成本乘数缩放）与完工判定。首笔投入开启项目时钟
    //    （`dev_started_on` 由首笔 `incur_development` 落地）。中断窗口内
    //    （`aggregates.interrupted`，含开工前/复工前）一律不投入。
    let mut just_completed = false;
    if !completed && !suspended && !interrupted_now && !aggregates.interrupted {
        let elapsed = books
            .project(&params.project)
            .and_then(|state| state.dev_started_on())
            .map(|started| date.days_since(started));
        let development_due =
            elapsed.is_none() || elapsed.is_some_and(|days| days < params.development_days);
        if development_due {
            let cost_bp = i32::try_from(aggregates.cost_mult_bp).map_err(|_| {
                OperationsError::ShockParamsInvalid {
                    detail: format!("cost multiplier {} overflows i32", aggregates.cost_mult_bp),
                }
            })?;
            let spend = params.daily_development_spend.apply_basis_points(cost_bp)?;
            if let Err(error) =
                books.incur_development(&params.project, &params.contractor, spend, date)
            {
                if matches!(error, RealEstateError::PaymentFailed { .. }) {
                    failures.push(PaymentFailureRecord {
                        company: company.clone(),
                        what: "development spend".to_string(),
                        amount: spend,
                    });
                } else {
                    return Err(error.into());
                }
            }
        } else {
            books.complete_project(&params.project, date)?;
            completed = true;
            just_completed = true;
        }
    }

    // 3. 预售签约 + 全额收款（完工后停止——现售不在本模型，登记简化）。
    if let Some(started) = books
        .project(&params.project)
        .and_then(|state| state.dev_started_on())
    {
        let elapsed = date.days_since(started);
        if !completed && !suspended && elapsed >= params.presale_open_day {
            // 薄适配器：`available_units` 是 real_estate 内部可见性（接口缺口
            // 已登记 issues）；用公开只读面按同公式推导——
            // 可售 = 总套数 − 未交付合同已占套数。
            let available = {
                let total = books
                    .project(&params.project)
                    .map(|state| state.total_units())
                    .unwrap_or(0);
                let reserved: i128 = books
                    .presales()
                    .values()
                    .filter(|presale| !presale.delivered() && presale.project() == &params.project)
                    .map(|presale| presale.units())
                    .sum();
                total - reserved
            };
            let wanted = scale_units(params.presale_units_per_day, aggregates.demand_mult_bp);
            let units = wanted.min(available).max(0);
            if units > 0 {
                let seq = *next_flow_seq;
                *next_flow_seq += 1;
                let contract = ContractId(format!("PS-{seq}"));
                let price_total = params.unit_price.mul_i128(units)?;
                books.sign_presale(
                    contract.clone(),
                    &params.project,
                    &params.buyer,
                    units,
                    price_total,
                    date,
                )?;
                books.collect_presale(&contract, price_total, date)?;
            }
        }
    }

    // 4. 完工当日为全部已售未交付合同排交付到期（完工 + lag）。
    if just_completed {
        let delivery_day = add_days(date, params.delivery_lag_days)?;
        let contracts: Vec<ContractId> = books
            .presales()
            .iter()
            .filter(|(_, presale)| !presale.delivered())
            .map(|(id, _)| id.clone())
            .collect();
        for contract in contracts {
            scheduler.submit(SchedulerRequest::Due {
                key: format!("MAT:{}:{}", company.0, contract.0),
                due_date: delivery_day,
                action: ScheduledAction::ContractMaturity {
                    company: company.clone(),
                    reference: format!("DL:{}", contract.0),
                },
            })?;
        }
    }
    Ok(())
}
