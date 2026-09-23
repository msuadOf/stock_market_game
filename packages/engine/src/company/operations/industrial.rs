//! 工商日常经营流（K4）：生产 → 赊销（到期事件排队）→ 补货 → 费用 →
//! 坏账准备目标化。付款失败（`PaymentFailed`）记为业务状态；其余类型化
//! 错误上抛。产量/需求是整数半偶舍入的数量换算；采购单价随成本乘数缩放
//! （经济参数变化，不是随机改余额）。

use crate::accounting::{AccountingAmount, InventoryItemCode, LedgerAccountId};
use crate::calendar::CivilDate;
use crate::company::counterparty::CounterpartyId;
use crate::company::events::ActiveShock;
use crate::company::industrial::{ExpenseKind, IndustrialBooks, IndustrialError, Settlement};
use crate::company::operations::core::PaymentFailureRecord;
use crate::company::operations::error::OperationsError;
use crate::company::operations::state::{add_days, scale_units, EconomyAggregates};
use crate::company::scheduler::{OperatingScheduler, ScheduledAction, SchedulerRequest};
use crate::company::spec::CompanyId;

/// 工商经营流参数（显式配置；游戏假设 Fixture 标注）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct IndustrialFlowParams {
    pub customer: CounterpartyId,
    pub supplier: CounterpartyId,
    pub raw_item: InventoryItemCode,
    pub finished_item: InventoryItemCode,
    pub raw_account: LedgerAccountId,
    pub finished_account: LedgerAccountId,
    /// 基准日需求（件；实际需求 = 基准 × 需求乘数）。
    pub base_daily_demand_units: i128,
    pub unit_price_excl_vat: AccountingAmount,
    /// 赊销信用期（自然日）。
    pub receivable_credit_days: i64,
    pub raw_replenish_target_units: i128,
    pub raw_unit_cost_excl_vat: AccountingAmount,
    pub daily_production_units: i128,
    pub daily_conversion_cost: AccountingAmount,
    pub daily_admin_expense: AccountingAmount,
    /// 基准坏账率（bp；信用恶化事件在其上叠加）。
    pub bad_debt_base_bp: i32,
    /// 资产减值迹象的一次性计提比例（×账面，bp）。
    pub asset_impairment_fraction_bp: i32,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::company::operations) fn advance_day(
    company: CompanyId,
    books: &mut IndustrialBooks,
    params: &IndustrialFlowParams,
    date: CivilDate,
    aggregates: EconomyAggregates,
    newly: &[ActiveShock],
    scheduler: &mut OperatingScheduler,
    failures: &mut Vec<PaymentFailureRecord>,
    next_flow_seq: &mut i64,
) -> Result<(), OperationsError> {
    let record_failure = |failures: &mut Vec<PaymentFailureRecord>, what: &str, amount| {
        failures.push(PaymentFailureRecord {
            company: company.clone(),
            what: what.to_string(),
            amount,
        });
    };
    let is_payment_failure =
        |error: &IndustrialError| matches!(error, IndustrialError::PaymentFailed { .. });

    // 0. 资产减值迹象（激活当日一次性计提；无资产面则跳过——记录在冲击面）。
    for shock in newly {
        if matches!(
            shock.kind,
            crate::company::events::ShockKind::AssetImpairmentSignal
        ) {
            let target = books
                .assets()
                .iter()
                .next()
                .map(|(code, entry)| (code.clone(), entry.carrying_amount()));
            if let Some((code, carrying)) = target {
                let carrying = carrying?;
                let amount = carrying.apply_basis_points(shock.amplitude_bp)?;
                if amount.is_positive() {
                    books.impair_asset(&code, amount, date)?;
                }
            }
        }
    }

    // 1. 生产（中断窗口内停产）。
    if !aggregates.interrupted {
        let raw_onhand = books.inventory().quantity(&params.raw_item);
        let quantity = params.daily_production_units.min(raw_onhand);
        if quantity > 0 {
            if let Err(error) = books.produce(
                params.raw_item.clone(),
                quantity,
                params.finished_item.clone(),
                quantity,
                params.finished_account.clone(),
                params.daily_conversion_cost,
                date,
            ) {
                if is_payment_failure(&error) {
                    record_failure(
                        failures,
                        "production conversion cost",
                        params.daily_conversion_cost,
                    );
                } else {
                    return Err(error.into());
                }
            }
        }
    }

    // 2. 赊销（实际销售不超过已接受需求与库存）。
    let demand = scale_units(params.base_daily_demand_units, aggregates.demand_mult_bp);
    let finished_onhand = books.inventory().quantity(&params.finished_item);
    let sellable = demand.min(finished_onhand).max(0);
    if sellable > 0 {
        let due_on = add_days(date, params.receivable_credit_days)?;
        let outcome = books.sell_credit(
            &params.customer,
            params.finished_item.clone(),
            sellable,
            params.unit_price_excl_vat,
            due_on,
            date,
        )?;
        scheduler.submit(SchedulerRequest::Due {
            key: format!("MAT:{}:{}", company.0, outcome.receivable.0),
            due_date: due_on,
            action: ScheduledAction::ContractMaturity {
                company: company.clone(),
                reference: format!("AR:{}", outcome.receivable.0),
            },
        })?;
    }

    // 3. 原料补货（目标库存制；单价随成本乘数缩放；现金不足 = 失败记录）。
    let raw_after = books.inventory().quantity(&params.raw_item);
    let shortfall = params.raw_replenish_target_units - raw_after;
    if shortfall > 0 {
        let cost_bp = i32::try_from(aggregates.cost_mult_bp).map_err(|_| {
            OperationsError::ShockParamsInvalid {
                detail: format!("cost multiplier {} overflows i32", aggregates.cost_mult_bp),
            }
        })?;
        let unit_cost = params.raw_unit_cost_excl_vat.apply_basis_points(cost_bp)?;
        if let Err(error) = books.purchase(
            &params.supplier,
            params.raw_item.clone(),
            params.raw_account.clone(),
            shortfall,
            unit_cost,
            Settlement::Cash,
            date,
            date,
        ) {
            if is_payment_failure(&error) {
                let amount = unit_cost.mul_i128(shortfall)?;
                record_failure(failures, "raw replenishment", amount);
            } else {
                return Err(error.into());
            }
        }
    }

    // 4. 日常管理费用。
    if let Err(error) = books.pay_expense(ExpenseKind::Admin, params.daily_admin_expense, date) {
        if is_payment_failure(&error) {
            record_failure(failures, "daily admin expense", params.daily_admin_expense);
        } else {
            return Err(error.into());
        }
    }

    // 5. 坏账准备目标化（基准 + 信用恶化加成，封顶 10000bp；幂等差额计提）。
    let ecl_rate = (params.bad_debt_base_bp + aggregates.credit_risk_add_bp).min(10_000);
    books.update_bad_debt_allowance(ecl_rate, date)?;

    let _ = next_flow_seq; // 工商流的合同 id 由会计事件 id 派生（AR-{event}）
    Ok(())
}
