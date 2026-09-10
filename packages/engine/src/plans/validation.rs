//! 计划契约的纯校验规则与类型化错误（K6 / K5a 迟滞）。
//!
//! 全部为 (状态, 事件) 的纯函数：不读时钟、不掷随机数、无副作用。
//! 任何拒绝都以 [`PlanError`] 显式返回，绝不静默 fallback。

use super::revision::PlanRevision;
use super::state::{PlanId, PlanOpen, PlanOpinion, PlanStatus, PlanTarget, TradingPlan};
use super::PlanPolicy;
use crate::account::StockCode;
use crate::orderbook::{AccountId, OrderId, Side};

/// 计划模块操作失败。绝不静默吞掉（铁律二），错误携带完整上下文。
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    #[error("unknown plan id {plan_id:?}")]
    UnknownPlan { plan_id: PlanId },
    #[error("account {account:?} already has a non-terminal plan for {code:?}")]
    DuplicateActivePlan { account: AccountId, code: StockCode },
    #[error("invalid target: {reason}")]
    InvalidTarget { reason: &'static str },
    #[error("invalid confidence {value} bp: must be within 0..=10000")]
    InvalidConfidenceBp { value: u32 },
    #[error("invalid signal score {value} bp: must be within -10000..=10000")]
    InvalidSignalScoreBp { value: i32 },
    #[error("invalid horizon of {value} trading days: must be at least 1")]
    InvalidHorizonDays { value: u32 },
    #[error("invalid policy threshold {field} = {value} bp: must be positive")]
    InvalidPolicyThreshold { field: &'static str, value: i32 },
    #[error("plan {plan_id:?}: fill quantity must be positive")]
    ZeroFillQuantity { plan_id: PlanId },
    #[error(
        "plan {plan_id:?}: fill from order {order_id:?} does not match the linked child {linked_order_id:?}"
    )]
    FillFromUnknownChildOrder {
        plan_id: PlanId,
        order_id: OrderId,
        linked_order_id: Option<OrderId>,
    },
    #[error(
        "plan {plan_id:?}: fill {fill_qty} would exceed target {target_qty} at filled {filled_qty}"
    )]
    FillExceedsTarget {
        plan_id: PlanId,
        filled_qty: u32,
        fill_qty: u32,
        target_qty: u32,
    },
    #[error(
        "plan {plan_id:?}: fill {fill_qty} does not exceed target {target_qty}; use the normal fill path"
    )]
    FillNotExcessive {
        plan_id: PlanId,
        filled_qty: u32,
        fill_qty: u32,
        target_qty: u32,
    },
    #[error("plan {plan_id:?}: excess-fill semantics require a share-count target")]
    FractionTargetHasNoShareExcess { plan_id: PlanId },
    #[error("plan {plan_id:?}: quantity counter overflow")]
    QuantityOverflow { plan_id: PlanId },
    #[error("plan {plan_id:?}: version counter overflow")]
    VersionOverflow { plan_id: PlanId },
    #[error("plan id sequence exhausted")]
    PlanSequenceExhausted,
    #[error(
        "plan {plan_id:?}: reverse revision score {score_bp} bp does not cross the opposite threshold {required_threshold_bp} bp"
    )]
    ReverseRevisionBelowThreshold {
        plan_id: PlanId,
        score_bp: i32,
        required_threshold_bp: i32,
    },
    #[error(
        "plan {plan_id:?}: new target {new_target_qty} is below filled {filled_qty} without a termination rationale"
    )]
    RevisionBelowFilledRequiresRationale {
        plan_id: PlanId,
        filled_qty: u32,
        new_target_qty: u32,
    },
    #[error(
        "plan {plan_id:?}: revision changes nothing (direction/target/urgency/confidence/opinion identical)"
    )]
    UnchangedRevision { plan_id: PlanId },
    #[error(
        "plan {plan_id:?}: event {event} at trading day {event_trading_day} is beyond the last valid day {last_valid_trading_day}"
    )]
    EventBeyondHorizon {
        plan_id: PlanId,
        event: &'static str,
        event_trading_day: u64,
        last_valid_trading_day: u64,
    },
    #[error(
        "plan {plan_id:?}: event {event} at trading day {event_trading_day} precedes the last event day {last_event_trading_day}"
    )]
    EventTimeWentBackwards {
        plan_id: PlanId,
        event: &'static str,
        event_trading_day: u64,
        last_event_trading_day: u64,
    },
    #[error("plan {plan_id:?}: cannot apply {event} from status {from:?}")]
    InvalidTransition {
        plan_id: PlanId,
        from: PlanStatus,
        event: &'static str,
    },
    #[error(
        "plan {plan_id:?}: horizon has not ended (last valid day {last_valid_trading_day}, now {trading_day})"
    )]
    ExpireBeforeHorizonEnd {
        plan_id: PlanId,
        trading_day: u64,
        last_valid_trading_day: u64,
    },
    #[error("plan book state is inconsistent: {detail}")]
    SaveInconsistent { detail: String },
}

/// K5a 迟滞谓词：修订翻转方向时，新方向的综合判断必须越过另一侧门槛。
/// 同向修订不受该门槛约束（是否修订由上游复核条件决定）。
pub fn reverse_crosses_threshold(old: Side, new: Side, score_bp: i32, threshold_bp: i32) -> bool {
    if old == new {
        return true;
    }
    match new {
        Side::Buy => score_bp >= threshold_bp,
        Side::Sell => score_bp <= -threshold_bp,
    }
}

/// 修订应用后的显式去向，由纯规则判定，`revision.rs` 只负责落地。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RevisionOutcome {
    /// 同向修订，目标上调或不变语义外调整，计划保持原状态。
    Forward,
    /// 反向修订跨过门槛：新方向重新累计成交进度，丢弃旧子单引用。
    ReverseRestart,
    /// 修订后目标恰好等于已成交：真实完成。
    CompleteNow,
    /// 修订目标低于已成交且携带终止理由：以该理由终止。
    Terminate(super::state::TerminationReason),
}

pub(crate) fn validate_target(target: PlanTarget) -> Result<(), PlanError> {
    match target {
        PlanTarget::ShareCount(0) => Err(PlanError::InvalidTarget {
            reason: "share-count target must be positive",
        }),
        PlanTarget::PositionFractionBp(bp) if bp > 10_000 => Err(PlanError::InvalidTarget {
            reason: "position fraction must be within 0..=10000 bp",
        }),
        _ => Ok(()),
    }
}

pub(crate) fn validate_confidence(confidence_bp: u32) -> Result<(), PlanError> {
    if confidence_bp > 10_000 {
        return Err(PlanError::InvalidConfidenceBp {
            value: confidence_bp,
        });
    }
    Ok(())
}

pub(crate) fn validate_opinion(opinion: &PlanOpinion) -> Result<(), PlanError> {
    if !(-10_000..=10_000).contains(&opinion.signal_score_bp) {
        return Err(PlanError::InvalidSignalScoreBp {
            value: opinion.signal_score_bp,
        });
    }
    Ok(())
}

pub(crate) fn validate_policy(policy: &PlanPolicy) -> Result<(), PlanError> {
    for (field, value) in [
        (
            "reverse_revision_threshold_bp",
            policy.reverse_revision_threshold_bp,
        ),
        ("review_signal_delta_bp", policy.review_signal_delta_bp),
        ("review_price_change_bp", policy.review_price_change_bp),
    ] {
        if value <= 0 {
            return Err(PlanError::InvalidPolicyThreshold { field, value });
        }
    }
    Ok(())
}

/// 开户请求的字段校验：目标、信心、观点分数、有效期都必须在契约范围内。
pub(crate) fn validate_open(open: &PlanOpen) -> Result<(), PlanError> {
    validate_target(open.target)?;
    validate_confidence(open.confidence_bp)?;
    validate_opinion(&open.opinion)?;
    if open.horizon_trading_days == 0 {
        return Err(PlanError::InvalidHorizonDays {
            value: open.horizon_trading_days,
        });
    }
    Ok(())
}

/// 真实成交不得使累计成交超过份额目标（比例目标无份额上限语义，由换算层负责）。
pub(crate) fn validate_fill_qty(plan: &TradingPlan, qty: u32) -> Result<(), PlanError> {
    if let PlanTarget::ShareCount(target_qty) = plan.target {
        let Some(new_filled) = plan.filled_qty.checked_add(qty) else {
            return Err(PlanError::QuantityOverflow {
                plan_id: plan.plan_id,
            });
        };
        if new_filled > target_qty {
            return Err(PlanError::FillExceedsTarget {
                plan_id: plan.plan_id,
                filled_qty: plan.filled_qty,
                fill_qty: qty,
                target_qty,
            });
        }
    }
    Ok(())
}

/// 修订的完整分类：字段合法 → 拒绝无变化 → 反向门槛 → 份额目标与已成交的关系。
pub(crate) fn classify_revision(
    plan: &TradingPlan,
    revision: &PlanRevision,
    policy: &PlanPolicy,
) -> Result<RevisionOutcome, PlanError> {
    validate_target(revision.target)?;
    validate_confidence(revision.confidence_bp)?;
    validate_opinion(&revision.opinion)?;
    let unchanged = plan.direction == revision.direction
        && plan.target == revision.target
        && plan.urgency == revision.urgency
        && plan.confidence_bp == revision.confidence_bp
        && plan.opinion == revision.opinion;
    if unchanged {
        return Err(PlanError::UnchangedRevision {
            plan_id: plan.plan_id,
        });
    }
    if revision.direction != plan.direction {
        if !reverse_crosses_threshold(
            plan.direction,
            revision.direction,
            revision.opinion.signal_score_bp,
            policy.reverse_revision_threshold_bp,
        ) {
            return Err(PlanError::ReverseRevisionBelowThreshold {
                plan_id: plan.plan_id,
                score_bp: revision.opinion.signal_score_bp,
                required_threshold_bp: policy.reverse_revision_threshold_bp,
            });
        }
        return Ok(RevisionOutcome::ReverseRestart);
    }
    if let PlanTarget::ShareCount(new_target_qty) = revision.target {
        if new_target_qty < plan.filled_qty {
            return match revision.below_filled_rationale {
                Some(reason) => Ok(RevisionOutcome::Terminate(reason)),
                None => {
                    return Err(PlanError::RevisionBelowFilledRequiresRationale {
                        plan_id: plan.plan_id,
                        filled_qty: plan.filled_qty,
                        new_target_qty,
                    });
                }
            };
        }
        if new_target_qty == plan.filled_qty {
            return Ok(RevisionOutcome::CompleteNow);
        }
    }
    Ok(RevisionOutcome::Forward)
}
