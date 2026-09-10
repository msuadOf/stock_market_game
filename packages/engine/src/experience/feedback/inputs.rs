//! 读取输入（任务 22/23 的目标/紧迫度接缝）：失败影响衰减、长期被套与
//! 风险压力。
//!
//! 这些是**派生输入**，不是另一份状态：全部从已登记事实 + 复用的既有字段
//! （`consecutive_failed_buys`/`peak_equity`）现算，读取方必须传入评估时刻
//! 与当前权威成本，评估时刻早于已登记经历时类型化拒绝。

use super::*;

impl RetailExperienceState {
    /// 信心输入：失败影响档 = 当前失败计数 −（距最近一次受挫每满 20 个交易日
    /// 减一档），饱和于零。计数与登记永不因此改动（衰减 ≠ 删除真实亏损）。
    pub fn failure_influence(&self, as_of: &ExperienceMoment) -> Result<u16, ExperienceError> {
        self.feedback.ensure_as_of_reached(as_of)?;
        let tiers = self.feedback.failure_events.last().map_or(0, |event| {
            (as_of.trading_day - event.moment.trading_day) / FAILURE_DECAY_TRADING_DAYS
        });
        Ok(self
            .consecutive_failed_buys
            .saturating_sub(u16::try_from(tiers).unwrap_or(u16::MAX)))
    }

    /// 忍耐输入（长期被套）：持有满 [`LONG_STUCK_TRADING_DAYS`] 个交易日，且
    /// 最近一次本人观察价低于读取方传入的当前权威持仓成本（不复制成本）。
    pub fn is_long_stuck(
        &self,
        code: &StockCode,
        cost: Money,
        as_of: &ExperienceMoment,
    ) -> Result<bool, ExperienceError> {
        require_positive("long-stuck cost", cost)?;
        self.feedback.ensure_as_of_reached(as_of)?;
        let epoch =
            self.feedback
                .stocks
                .get(code)
                .ok_or_else(|| ExperienceError::NoActiveEntry {
                    code: code.0.clone(),
                })?;
        let held_days = as_of.trading_day - epoch.entry_moment.trading_day;
        Ok(held_days >= LONG_STUCK_TRADING_DAYS
            && epoch
                .last_own_observation
                .is_some_and(|obs| obs.price.cents() < cost.cents()))
    }

    /// 风险压力输入：相对本人观察过的账户权益峰值的回撤（复用既有字段，
    /// 不另建账户损益）。负值 = 权益已越过旧峰值（峰值未更新），由消费方
    /// 按阈值解释；无峰值参照时诚实返回 `None`。
    pub fn experience_drawdown_from_peak(&self, equity: Money) -> Option<f64> {
        self.peak_equity.map(|peak| {
            (i128::from(peak.cents()) - i128::from(equity.cents())) as f64 / peak.cents() as f64
        })
    }
}
