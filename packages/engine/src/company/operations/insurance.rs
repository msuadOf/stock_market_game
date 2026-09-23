//! 保险日常经营流（K4）：新合同组建立（需求乘数作用于新单量）+ 保费收讫、
//! 责任单元释放、确定性赔案日程（发生 + 支付；支付失败 = 未付赔款面，公司
//! 存活）。成本字段不适用（无商品成本面——跨行业适用面矩阵）。

use crate::accounting::AccountingAmount;
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::counterparty::CounterpartyId;
use crate::company::insurance::{ClaimId, InsuranceBooks, InsuranceError, InsuranceProductKind};
use crate::company::operations::core::PaymentFailureRecord;
use crate::company::operations::error::OperationsError;
use crate::company::operations::state::{add_days, scale_units, EconomyAggregates};
use crate::company::scheduler::OperatingScheduler;
use crate::company::spec::CompanyId;

/// 保险经营流参数（显式配置；贴现/风险调整为 Fixture 游戏假设）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct InsuranceFlowParams {
    pub policyholder: CounterpartyId,
    /// 每日新组基数（实际 = 基数 × 需求乘数）。
    pub daily_groups_base: i128,
    pub premium: AccountingAmount,
    pub expected_claims: AccountingAmount,
    pub risk_adjustment: AccountingAmount,
    pub coverage_days: i64,
    /// 每隔 N 个保障日发生一次赔案（确定性日程）。
    pub claim_every_days: i64,
    pub claim_size: AccountingAmount,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::company::operations) fn advance_day(
    company: CompanyId,
    books: &mut InsuranceBooks,
    params: &InsuranceFlowParams,
    date: CivilDate,
    aggregates: EconomyAggregates,
    _scheduler: &mut OperatingScheduler,
    failures: &mut Vec<PaymentFailureRecord>,
    next_flow_seq: &mut i64,
) -> Result<(), OperationsError> {
    // 1. 新合同组（保费收讫同日；保费是负债不是收入——处理器红线）。
    let groups_today = scale_units(params.daily_groups_base, aggregates.demand_mult_bp);
    for _ in 0..groups_today {
        let seq = *next_flow_seq;
        *next_flow_seq += 1;
        let group = ContractId(format!("GRP-{seq}"));
        let end = add_days(date, params.coverage_days)?;
        books.establish_group(
            InsuranceProductKind::TermProtection,
            group.clone(),
            &params.policyholder,
            params.premium,
            params.expected_claims,
            params.risk_adjustment,
            date,
            end,
        )?;
        books.collect_premium(&group, params.premium, date)?;
    }

    // 2. 存续组：责任单元释放（每天 1 单元）+ 确定性赔案日程。
    let groups: Vec<ContractId> = books.groups().map(|(id, _)| id.clone()).collect();
    for group in groups {
        let Some(state) = books.group(&group) else {
            continue;
        };
        let remaining = state.units_remaining();
        let elapsed = date.days_since(state.coverage_start());
        if remaining > 0 {
            books.release_service(&group, 1, date)?;
        }
        if elapsed > 0 && params.claim_every_days >= 1 && elapsed % params.claim_every_days == 0 {
            let claim = ClaimId(format!("CLM-{}-{elapsed}", group.0));
            books.record_claim(&group, claim.clone(), params.claim_size, date)?;
            if let Err(error) = books.pay_claim(&group, &claim, params.claim_size, date) {
                if matches!(error, InsuranceError::PaymentFailed { .. }) {
                    failures.push(PaymentFailureRecord {
                        company: company.clone(),
                        what: format!("claim payment {}", claim.0),
                        amount: params.claim_size,
                    });
                } else {
                    return Err(error.into());
                }
            }
        }
    }
    // 保险流当前无到期承载面（赔案日程由保障日推导，不排队）。
    Ok(())
}
