//! 权威子单事实回写：计划进度、在途身份与执行腿仅由 PlanBook 修改。

use super::*;

impl GameSession {
    /// 消费真实成交事实；逐腿即时更新 owner，后续腿与 continuation 可见已发生的进度。
    pub(in crate::session) fn record_parent_order_fills(
        &mut self,
        code: &StockId,
        fills: &[OrderFillSettlement],
    ) -> Result<(), crate::plans::PlanError> {
        for fill in fills {
            self.state.plans.record_child_fill(
                fill.account,
                code,
                fill.side,
                fill.order_id,
                fill.qty,
                u64::from(self.state.day),
            )?;
            #[cfg(feature = "simulation-diagnostics")]
            {
                let time = self.causal_time();
                self.state.causal.record_continuous_fill(
                    time, fill.order_id, fill.account, code, fill.qty, fill.gross.cents(),
                );
            }
        }
        Ok(())
    }

    /// 与旧镜像一致：计划账户/股票/方向的已接受委托被唯一 owner 采纳为在途子单
    /// （幂等；同批第二张不同子单是类型化错误）。无执行腿时不属于计划。
    pub(in crate::session) fn record_parent_order_submission(
        &mut self,
        account: AccountId,
        code: &StockId,
        side: Side,
        order_id: OrderId,
        qty: u32,
    ) -> Result<(), crate::plans::PlanError> {
        if let Some(plan) = self.state.plans.active_plan(account, code) {
            if plan.active_child_order_id() == Some(order_id) {
                if plan.direction() != side || plan.active_child_remaining_qty() != Some(qty) {
                    return Err(crate::plans::PlanError::InvalidExecutionParameters {
                        plan_id: plan.plan_id(),
                    });
                }
                return Ok(());
            }
        }
        self.state.plans.record_child_submitted(
            account, code, side, order_id, qty, u64::from(self.state.day),
        )?;
        Ok(())
    }

    /// 已确认撤销或到期释放才清除子单引用；拒绝撤单不会调用该入口。
    pub(in crate::session) fn record_parent_order_canceled(
        &mut self,
        account: AccountId,
        code: &StockId,
        order_id: OrderId,
    ) -> Result<(), crate::plans::PlanError> {
        self.state.plans.record_child_released(
            account, code, order_id, u64::from(self.state.day), false,
        )?;
        Ok(())
    }
}
