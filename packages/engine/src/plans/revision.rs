//! 计划事件转移（K6）：观察/子单接受/真实成交/版本化修订（含 K5a 反向门槛）、
//! 暂停/恢复、到期与日终。全部为纯状态转移；决策（何时复核、何时暂停、
//! 报价策略）属任务 22/23。

use super::state::{
    PauseReason, PlanOpinion, PlanStatus, PlanTarget, ResumeReason, TerminationReason, TradingPlan,
    Urgency,
};
use super::validation::{classify_revision, validate_fill_qty, PlanError, RevisionOutcome};
use super::PlanPolicy;
use crate::orderbook::{OrderId, Side};

/// 修订原因（K5a 复核触发的显式数据）。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum RevisionReason {
    /// 综合判断变化达到复核阈值。
    SignalShift,
    /// 价格相对上次判断变化达到复核阈值。
    PriceMove,
    /// 可用现金 / 库存 / T+1 约束变化。
    ConstraintsChanged,
    /// 风险分支触发。
    RiskTriggered,
    /// 期限届满复核。
    HorizonReview,
    /// 新事实（新披露、新经历）。
    NewInformation,
}

/// 一次已落地修订的记录：版本 + 原因 + 交易日。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct RevisionRecord {
    pub version: u32,
    pub reason: RevisionReason,
    pub trading_day: u64,
}

/// 修订请求：新方向/目标/观点/紧迫度 + 显式原因。
/// `below_filled_rationale`：同向修订目标低于已成交时必须携带的终止理由。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PlanRevision {
    pub reason: RevisionReason,
    pub trading_day: u64,
    pub direction: Side,
    pub target: PlanTarget,
    pub opinion: PlanOpinion,
    pub confidence_bp: u32,
    pub urgency: Urgency,
    pub below_filled_rationale: Option<TerminationReason>,
}

impl TradingPlan {
    /// 事件三重守卫：非终态、未越过有效期、事件时间不回拨。
    pub(super) fn ensure_event_allowed(
        &self,
        event: &'static str,
        trading_day: u64,
    ) -> Result<(), PlanError> {
        if self.is_terminal() {
            return Err(PlanError::InvalidTransition {
                plan_id: self.plan_id,
                from: self.status,
                event,
            });
        }
        if trading_day > self.last_valid_trading_day() {
            return Err(PlanError::EventBeyondHorizon {
                plan_id: self.plan_id,
                event,
                event_trading_day: trading_day,
                last_valid_trading_day: self.last_valid_trading_day(),
            });
        }
        if trading_day < self.last_event_trading_day {
            return Err(PlanError::EventTimeWentBackwards {
                plan_id: self.plan_id,
                event,
                event_trading_day: trading_day,
                last_event_trading_day: self.last_event_trading_day,
            });
        }
        Ok(())
    }

    /// 平静观察：信息/风险/约束无变化 → 方向、目标与累计进度全部保持。
    pub(super) fn observe_no_change(&mut self, trading_day: u64) -> Result<(), PlanError> {
        self.ensure_event_allowed("observe", trading_day)?;
        self.last_event_trading_day = trading_day;
        Ok(())
    }

    /// 委托被权威路由接受：只建立子单引用，绝不推进成交进度。
    pub(super) fn record_child_order_accepted(
        &mut self,
        order_id: OrderId,
        trading_day: u64,
    ) -> Result<(), PlanError> {
        self.ensure_event_allowed("child-order-accepted", trading_day)?;
        self.active_child_order_id = Some(order_id);
        self.last_event_trading_day = trading_day;
        Ok(())
    }

    fn require_linked_child(&self, order_id: OrderId) -> Result<(), PlanError> {
        match self.active_child_order_id {
            Some(linked) if linked == order_id => Ok(()),
            linked => Err(PlanError::FillFromUnknownChildOrder {
                plan_id: self.plan_id,
                order_id,
                linked_order_id: linked,
            }),
        }
    }

    /// 真实成交：只有此路径推进 `filled_qty`；到达份额目标即真实完成。
    pub(super) fn record_real_fill(
        &mut self,
        order_id: OrderId,
        qty: u32,
        trading_day: u64,
    ) -> Result<(), PlanError> {
        self.ensure_event_allowed("fill", trading_day)?;
        if qty == 0 {
            return Err(PlanError::ZeroFillQuantity {
                plan_id: self.plan_id,
            });
        }
        self.require_linked_child(order_id)?;
        validate_fill_qty(self, qty)?;
        let new_filled = self
            .filled_qty
            .checked_add(qty)
            .ok_or(PlanError::QuantityOverflow {
                plan_id: self.plan_id,
            })?;
        self.filled_qty = new_filled;
        self.last_event_trading_day = trading_day;
        if let PlanTarget::ShareCount(target_qty) = self.target {
            if new_filled == target_qty {
                self.status = PlanStatus::Completed;
            }
        }
        Ok(())
    }

    /// 超目标真实成交（如实入账，拒绝伪装）：累计成交如实更新，计划以显式原因终止。
    pub(super) fn record_excess_fill(
        &mut self,
        order_id: OrderId,
        qty: u32,
        trading_day: u64,
    ) -> Result<(), PlanError> {
        self.ensure_event_allowed("excess-fill", trading_day)?;
        if qty == 0 {
            return Err(PlanError::ZeroFillQuantity {
                plan_id: self.plan_id,
            });
        }
        self.require_linked_child(order_id)?;
        let target_qty = match self.target {
            PlanTarget::ShareCount(target_qty) => target_qty,
            PlanTarget::PositionFractionBp(_) => {
                return Err(PlanError::FractionTargetHasNoShareExcess {
                    plan_id: self.plan_id,
                })
            }
        };
        let new_filled = self
            .filled_qty
            .checked_add(qty)
            .ok_or(PlanError::QuantityOverflow {
                plan_id: self.plan_id,
            })?;
        if new_filled <= target_qty {
            return Err(PlanError::FillNotExcessive {
                plan_id: self.plan_id,
                filled_qty: self.filled_qty,
                fill_qty: qty,
                target_qty,
            });
        }
        self.filled_qty = new_filled;
        self.status = PlanStatus::Terminated {
            reason: TerminationReason::FilledBeyondTarget,
        };
        self.last_event_trading_day = trading_day;
        Ok(())
    }
}

/// 应用一次修订：先由纯规则分类，再落地版本与状态。
pub(super) fn apply_revision(
    plan: &mut TradingPlan,
    revision: &PlanRevision,
    policy: &PlanPolicy,
) -> Result<(), PlanError> {
    plan.ensure_event_allowed("revision", revision.trading_day)?;
    match classify_revision(plan, revision, policy)? {
        RevisionOutcome::Forward => {}
        RevisionOutcome::ReverseRestart => {
            // 反向修订开新进度：旧腿的真实成交已在账户里，不在此重复记账。
            plan.filled_qty = 0;
            plan.active_child_order_id = None;
        }
        RevisionOutcome::CompleteNow => {
            plan.status = PlanStatus::Completed;
        }
        RevisionOutcome::Terminate(reason) => {
            plan.status = PlanStatus::Terminated { reason };
        }
    }
    plan.direction = revision.direction;
    plan.target = revision.target;
    plan.opinion = revision.opinion;
    plan.confidence_bp = revision.confidence_bp;
    plan.urgency = revision.urgency;
    plan.version = plan
        .version
        .checked_add(1)
        .ok_or(PlanError::VersionOverflow {
            plan_id: plan.plan_id,
        })?;
    plan.last_revision = Some(RevisionRecord {
        version: plan.version,
        reason: revision.reason,
        trading_day: revision.trading_day,
    });
    plan.review.last_review_signal_score_bp = revision.opinion.signal_score_bp;
    plan.review.last_review_trading_day = revision.trading_day;
    plan.last_event_trading_day = revision.trading_day;
    Ok(())
}

/// 暂停：仅 Active 可暂停，且必须携带显式原因。
pub(super) fn pause(
    plan: &mut TradingPlan,
    reason: PauseReason,
    trading_day: u64,
) -> Result<(), PlanError> {
    plan.ensure_event_allowed("pause", trading_day)?;
    if plan.status != PlanStatus::Active {
        return Err(PlanError::InvalidTransition {
            plan_id: plan.plan_id,
            from: plan.status,
            event: "pause",
        });
    }
    plan.status = PlanStatus::Paused { reason };
    plan.last_event_trading_day = trading_day;
    Ok(())
}

/// 恢复：仅 Paused 可恢复，且必须携带显式原因。
pub(super) fn resume(
    plan: &mut TradingPlan,
    reason: ResumeReason,
    trading_day: u64,
) -> Result<(), PlanError> {
    plan.ensure_event_allowed("resume", trading_day)?;
    if !matches!(plan.status, PlanStatus::Paused { .. }) {
        return Err(PlanError::InvalidTransition {
            plan_id: plan.plan_id,
            from: plan.status,
            event: "resume",
        });
    }
    plan.status = PlanStatus::Active;
    plan.last_resume = Some(reason);
    plan.last_event_trading_day = trading_day;
    Ok(())
}

/// 显式到期终止：只接受真正越过有效期末日之后的调用。
pub(super) fn expire(plan: &mut TradingPlan, trading_day: u64) -> Result<(), PlanError> {
    plan.ensure_event_allowed("horizon-expiry", trading_day)?;
    if trading_day <= plan.last_valid_trading_day() {
        return Err(PlanError::ExpireBeforeHorizonEnd {
            plan_id: plan.plan_id,
            trading_day,
            last_valid_trading_day: plan.last_valid_trading_day(),
        });
    }
    plan.status = PlanStatus::Terminated {
        reason: TerminationReason::HorizonExpired,
    };
    plan.active_child_order_id = None;
    plan.last_event_trading_day = trading_day;
    Ok(())
}

/// 日终：仅结束子单生命周期（清除子单引用）；计划本身保留，状态除显式转移外不变。
/// 覆盖最后一个有效交易日的日终同时执行到期终止。
pub(super) fn end_of_trading_day(
    plan: &mut TradingPlan,
    trading_day: u64,
) -> Result<(), PlanError> {
    plan.ensure_event_allowed("day-end", trading_day)?;
    plan.active_child_order_id = None;
    if trading_day >= plan.last_valid_trading_day() {
        plan.status = PlanStatus::Terminated {
            reason: TerminationReason::HorizonExpired,
        };
    }
    plan.last_event_trading_day = trading_day;
    Ok(())
}
