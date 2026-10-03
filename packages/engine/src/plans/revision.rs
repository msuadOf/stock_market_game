//! 计划事件转移：观察/子单接受/真实成交/版本化修订（含反向门槛）、
//! 暂停/恢复、到期与日终。全部为纯状态转移；决策（何时复核、何时暂停、
//! 报价策略）分别在 allocation、candidates 与 urgency 中实现。

use super::state::{
    PauseReason, PlanOpinion, PlanStatus, PlanTarget, ResumeReason, TerminationReason, TradingPlan,
    Urgency,
};
use super::validation::{classify_revision, validate_fill_qty, PlanError, RevisionOutcome};
use super::PlanPolicy;
use crate::orderbook::{OrderId, Side};

/// 修订原因（个人判断复核触发的显式数据）。
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
    /// 事件守卫：非终态、事件时间不回拨；`allow_beyond_horizon = false` 时还要求
    /// 未越过有效期。到期与日终必须传 `true` 豁免有效期上限——显式到期扫描和
    /// 错过日终的补账恰好发生在有效期过后，否则这两类事件永远不可达。
    pub(super) fn ensure_event_allowed(
        &self,
        event: &'static str,
        trading_day: u64,
        allow_beyond_horizon: bool,
    ) -> Result<(), PlanError> {
        if self.is_terminal() {
            return Err(PlanError::InvalidTransition {
                plan_id: self.plan_id,
                from: self.status,
                event,
            });
        }
        if !allow_beyond_horizon && trading_day > self.last_valid_trading_day() {
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
        self.ensure_event_allowed("observe", trading_day, false)?;
        self.last_event_trading_day = trading_day;
        Ok(())
    }

    /// 委托被权威路由接受：只建立子单引用，绝不推进成交进度。
    pub(super) fn record_child_order_accepted(
        &mut self,
        order_id: OrderId,
        trading_day: u64,
    ) -> Result<(), PlanError> {
        self.ensure_event_allowed("child-order-accepted", trading_day, false)?;
        self.active_child_order_id = Some(order_id);
        self.last_event_trading_day = trading_day;
        Ok(())
    }

    /// orderbook 撤销未成交余量成功后，累计真实成交数量保持不变。
    pub(super) fn record_child_order_canceled(
        &mut self,
        order_id: OrderId,
        trading_day: u64,
    ) -> Result<(), PlanError> {
        self.ensure_event_allowed("child-order-canceled", trading_day, false)?;
        self.require_linked_child(order_id)?;
        self.active_child_order_id = None;
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
        child_complete: bool,
        trading_day: u64,
    ) -> Result<(), PlanError> {
        self.ensure_event_allowed("fill", trading_day, false)?;
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
        if child_complete || self.is_terminal() {
            self.active_child_order_id = None;
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
        self.ensure_event_allowed("excess-fill", trading_day, false)?;
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
        self.active_child_order_id = None;
        self.last_event_trading_day = trading_day;
        Ok(())
    }
}

impl TradingPlan {
    /// 应用一次修订：先由纯规则分类，再落地版本与状态。
    pub(super) fn apply_revision(
        &mut self,
        revision: &PlanRevision,
        policy: &PlanPolicy,
    ) -> Result<(), PlanError> {
        self.ensure_event_allowed("revision", revision.trading_day, false)?;
        match classify_revision(self, revision, policy)? {
            RevisionOutcome::Forward => {}
            RevisionOutcome::ReverseRestart => {
                // 反向修订开新进度：旧腿的真实成交已在账户里，不在此重复记账。
                self.filled_qty = 0;
                self.active_child_order_id = None;
                if self.direction == Side::Buy && revision.direction == Side::Sell {
                    self.status = PlanStatus::Active;
                }
            }
            RevisionOutcome::CompleteNow => {
                self.status = PlanStatus::Completed;
            }
            RevisionOutcome::Terminate(reason) => {
                self.status = PlanStatus::Terminated { reason };
            }
        }
        self.direction = revision.direction;
        self.target = revision.target;
        self.opinion = revision.opinion;
        self.confidence_bp = revision.confidence_bp;
        self.urgency = revision.urgency;
        self.version = self
            .version
            .checked_add(1)
            .ok_or(PlanError::VersionOverflow {
                plan_id: self.plan_id,
            })?;
        self.last_revision = Some(RevisionRecord {
            version: self.version,
            reason: revision.reason,
            trading_day: revision.trading_day,
        });
        self.review.last_review_signal_score_bp = revision.opinion.signal_score_bp;
        self.review.last_review_trading_day = revision.trading_day;
        self.last_event_trading_day = revision.trading_day;
        Ok(())
    }

    /// 暂停：仅 Active 可暂停，且必须携带显式原因。
    pub(super) fn pause(&mut self, reason: PauseReason, trading_day: u64) -> Result<(), PlanError> {
        self.ensure_event_allowed("pause", trading_day, false)?;
        let upgrades_risk_pressure = matches!(self.status, PlanStatus::Paused { reason: previous } if previous != PauseReason::RiskPressure)
            && reason == PauseReason::RiskPressure;
        if self.status != PlanStatus::Active && !upgrades_risk_pressure {
            return Err(PlanError::InvalidTransition {
                plan_id: self.plan_id,
                from: self.status,
                event: "pause",
            });
        }
        self.status = PlanStatus::Paused { reason };
        self.last_event_trading_day = trading_day;
        Ok(())
    }

    /// 恢复：仅 Paused 可恢复，且必须携带显式原因。
    pub(super) fn resume(
        &mut self,
        reason: ResumeReason,
        trading_day: u64,
    ) -> Result<(), PlanError> {
        self.ensure_event_allowed("resume", trading_day, false)?;
        if !matches!(self.status, PlanStatus::Paused { .. }) {
            return Err(PlanError::InvalidTransition {
                plan_id: self.plan_id,
                from: self.status,
                event: "resume",
            });
        }
        self.status = PlanStatus::Active;
        self.last_resume = Some(reason);
        self.last_event_trading_day = trading_day;
        Ok(())
    }

    /// 显式到期终止：只接受真正越过有效期末日之后的调用（守卫对到期事件
    /// 豁免有效期上限，因此该成功路径可达）。
    pub(super) fn expire(&mut self, trading_day: u64) -> Result<(), PlanError> {
        self.ensure_event_allowed("horizon-expiry", trading_day, true)?;
        if trading_day <= self.last_valid_trading_day() {
            return Err(PlanError::ExpireBeforeHorizonEnd {
                plan_id: self.plan_id,
                trading_day,
                last_valid_trading_day: self.last_valid_trading_day(),
            });
        }
        self.status = PlanStatus::Terminated {
            reason: TerminationReason::HorizonExpired,
        };
        self.active_child_order_id = None;
        self.last_event_trading_day = trading_day;
        Ok(())
    }

    /// 日终：仅结束子单生命周期（清除子单引用）；计划本身保留，状态除显式转移外不变。
    /// 覆盖最后一个有效交易日的日终执行到期终止；日终在有效期过后才送达（错过补账）
    /// 同样直接补终止，不把计划搁浅在 Active。
    pub(super) fn end_of_trading_day(&mut self, trading_day: u64) -> Result<(), PlanError> {
        self.ensure_event_allowed("day-end", trading_day, true)?;
        self.active_child_order_id = None;
        if trading_day >= self.last_valid_trading_day() {
            self.status = PlanStatus::Terminated {
                reason: TerminationReason::HorizonExpired,
            };
        }
        self.last_event_trading_day = trading_day;
        Ok(())
    }
}
