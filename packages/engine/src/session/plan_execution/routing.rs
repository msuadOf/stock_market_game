//! 计划工作单读取与执行腿安装接缝。
//!
//! 母单执行子状态唯一保存在 `PlanBook`/`TradingPlan`（ADR-0015 2026-09-10 修订）；
//! 本文件只提供会话侧的读视图（真实订单簿/竞价队列）与安装入口。

use super::*;

#[derive(Clone, Copy)]
pub(in crate::session) struct WorkingPlanOrder {
    pub(super) id: OrderId,
    pub(super) side: Side,
    pub(super) price: Money,
    pub(super) qty: u32,
}

impl GameSession {
    pub(in crate::session) fn plan_working_orders(
        &self,
        account: AccountId,
        code: &StockId,
    ) -> Vec<WorkingPlanOrder> {
        self.state
            .markets
            .get(code)
            .into_iter()
            .flat_map(|market| {
                let mut orders = market.resting_orders_for(account);
                orders.sort_by_key(|order| order.seq);
                orders
            })
            .map(|order| WorkingPlanOrder {
                id: order.id,
                side: order.side,
                price: order.price,
                qty: order.qty,
            })
            .collect()
    }

    /// 为一张新提交的子单安装执行腿（目标股数与已成交进度来自计划本体）。
    /// 采纳已有工作单时同时登记在途子单身份与剩余数量。
    pub(in crate::session) fn install_plan_execution(
        &self,
        plans: &mut PlanBook,
        plan: &crate::plans::TradingPlan,
        child: NewChildSpec,
        adopted: Option<(OrderId, u32)>,
    ) -> Result<(), PlanExecutionError> {
        if !matches!(plan.target(), PlanTarget::ShareCount(_)) {
            return Err(PlanExecutionError::UnconvertedFractionTarget {
                plan_id: plan.plan_id(),
            });
        }
        let stock_day = self.stock_calendar_day(*plan.code())?.ok_or_else(|| {
            StepFatal::InvariantViolation {
                location: "plan_execution::install_plan_execution".to_owned(),
                description: "closed 证券不可建立执行腿".to_owned(),
            }
        })?;
        let day_end = stock_day
            .checked_add(1)
            .and_then(|day| {
                day.checked_mul(u64::from(crate::session::GAME_INTRADAY_MINUTES_PER_DAY))
            })
            .ok_or_else(|| StepFatal::InvariantViolation {
                location: "plan_execution::install_plan_execution".to_owned(),
                description: "本股执行腿到期分钟溢出".to_owned(),
            })?;
        plans
            .install_execution(plan.plan_id(), child.qty, child.price, day_end)
            .map_err(PlanExecutionError::Plan)?;
        if let Some((order_id, qty)) = adopted {
            plans
                .record_child_submitted(
                    plan.account(),
                    plan.code(),
                    plan.direction(),
                    order_id,
                    qty,
                    stock_day,
                )
                .map_err(PlanExecutionError::Plan)?;
        }
        Ok(())
    }
}
