//! Transactional PlanBook synchronization from authoritative order facts.

use super::*;
use crate::plans::PlanEvent;

impl GameSession {
    pub(super) fn merge_external_plan_book_handoff(
        &self,
        external: &PlanBook,
    ) -> Result<PlanBook, PlanExecutionError> {
        let session_plan_count = self.plans.plan_ids().count();
        if session_plan_count == 0 {
            return Ok(external.clone());
        }
        let external_plan_count = external.plan_ids().count();
        let preserves_session_history = self.plans.policy() == external.policy()
            && self.plans.plan_ids().all(|plan_id| {
                self.plans
                    .plan(plan_id)
                    .ok()
                    .zip(external.plan(plan_id).ok())
                    .is_some_and(|(session, provided)| {
                        external_plan_is_current_or_stale(provided, session)
                    })
            });
        let merged = preserves_session_history
            .then(|| self.plans.with_appended_plans_from(external))
            .flatten();
        merged.ok_or(PlanExecutionError::PlanBookOwnershipConflict {
            session_plan_count,
            external_plan_count,
        })
    }

    pub(in crate::session) fn has_pending_plan_event_capacity(&self, required: usize) -> bool {
        required <= crate::session::MAX_SAVED_PLAN_EVENTS - self.pending_plan_events.len()
    }

    pub(in crate::session) fn push_pending_plan_event(
        &mut self,
        event: PendingPlanEvent,
        events: &mut Vec<Event>,
    ) -> bool {
        if self.pending_plan_events.len() >= crate::session::MAX_SAVED_PLAN_EVENTS {
            self.report_pending_plan_event_capacity(events);
            return false;
        }
        self.pending_plan_events.push(event);
        true
    }

    pub(in crate::session) fn report_pending_plan_event_capacity(
        &mut self,
        events: &mut Vec<Event>,
    ) {
        if events.iter().any(|event| {
            matches!(
                event,
                Event::ResourceLimit {
                    resource: RuntimeResource::PendingPlanEvents,
                    ..
                }
            )
        }) {
            return;
        }
        events.push(Event::ResourceLimit {
            seq: self.next_seq(),
            resource: RuntimeResource::PendingPlanEvents,
            limit: crate::session::MAX_SAVED_PLAN_EVENTS as u32,
        });
    }

    /// Applies accepted/fill/day-end facts captured by real routing since the previous observation.
    ///
    /// This old public handoff is retained only for callers outside the tick path.
    pub fn synchronize_plan_execution(
        &mut self,
        plans: &mut PlanBook,
    ) -> Result<(), PlanExecutionError> {
        // Save schema v2 makes the session copy authoritative. The legacy public adapter still
        // accepts an external PlanBook, so treat it as an explicit handoff: it may seed an empty
        // session or append new plans, but it may not rewrite/drop history already owned by the
        // session. Successful synchronization writes the same candidate to both owners.
        let mut candidate = self.merge_external_plan_book_handoff(plans)?;
        self.synchronize_owned_plan_execution(&mut candidate)?;
        self.plans = candidate.clone();
        *plans = candidate;
        Ok(())
    }

    /// The tick already owns this PlanBook inside its private session shadow.
    /// Touch only plans named by pending facts; the caller restores the book to
    /// the session before the complete tick is committed.
    pub(in crate::session) fn synchronize_owned_plan_execution(
        &mut self,
        plans: &mut PlanBook,
    ) -> Result<(), PlanExecutionError> {
        let pending = &self.pending_plan_events;
        let mut events = Vec::with_capacity(pending.len());
        for event in pending {
            let (plan_id, plan_event) = match *event {
                PendingPlanEvent::Accepted {
                    plan_id,
                    order_id,
                    trading_day,
                } => (
                    plan_id,
                    PlanEvent::ChildOrderAccepted {
                        order_id,
                        trading_day,
                    },
                ),
                PendingPlanEvent::Filled {
                    plan_id,
                    order_id,
                    qty,
                    trading_day,
                } => (
                    plan_id,
                    PlanEvent::ChildOrderFilled {
                        order_id,
                        qty,
                        trading_day,
                    },
                ),
                PendingPlanEvent::DayEnded {
                    plan_id,
                    trading_day,
                } => (plan_id, PlanEvent::TradingDayEnded { trading_day }),
            };
            events.push((plan_id, plan_event));
        }
        let outcomes = plans.apply_active_events_atomically(&events)?;
        let mut completed_fills = Vec::new();
        for ((plan_id, plan_event), outcome) in events.iter().zip(outcomes) {
            if let Some(status) = outcome {
                if matches!(plan_event, PlanEvent::ChildOrderFilled { .. })
                    && matches!(
                        status,
                        crate::plans::PlanStatus::Completed
                            | crate::plans::PlanStatus::Terminated { .. }
                    )
                {
                    completed_fills.push(*plan_id);
                }
            }
        }
        // A day-end fact after a completing fill in this sealed batch has no
        // further plan transition. Consume it now: saved pending facts may only
        // target live plans, and replaying it every tick would grow without bound.
        self.pending_plan_events.clear();
        for plan_id in completed_fills {
            if plans
                .plan(plan_id)
                .map(|p| p.is_terminal())
                .unwrap_or(false)
            {
                self.remove_linked_parent(plan_id);
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(in crate::session) fn record_plan_execution_day_end(&mut self, events: &mut Vec<Event>) {
        let trading_day = u64::from(self.day);
        let plan_ids: Vec<PlanId> = self
            .parent_orders
            .values()
            .flat_map(|plans| plans.values())
            .filter(|plan| plan.filled_qty < plan.target_qty)
            .filter_map(|plan| plan.linked_plan_id)
            .collect();
        if plan_ids.len() > crate::session::MAX_SAVED_PLAN_EVENTS - self.pending_plan_events.len() {
            self.report_pending_plan_event_capacity(events);
            return;
        }
        for plan_id in plan_ids {
            let appended = self.push_pending_plan_event(
                PendingPlanEvent::DayEnded {
                    plan_id,
                    trading_day,
                },
                events,
            );
            if !appended {
                self.report_pending_plan_event_capacity(events);
                return;
            }
        }
    }
}

fn external_plan_is_current_or_stale(
    external: &crate::plans::TradingPlan,
    session: &crate::plans::TradingPlan,
) -> bool {
    if external == session {
        return true;
    }
    let same_origin = external.plan_id == session.plan_id
        && external.account == session.account
        && external.code == session.code
        && external.created_trading_day == session.created_trading_day
        && external.horizon_trading_days == session.horizon_trading_days;
    if !same_origin || external.version > session.version {
        return false;
    }
    if external.version < session.version {
        return true;
    }

    // Within one plan revision, only execution/lifecycle facts may advance the session-owned
    // copy between public adapter calls. Decision semantics must remain byte-for-byte equal.
    let same_revision = external.direction == session.direction
        && external.target == session.target
        && external.opinion == session.opinion
        && external.confidence_bp == session.confidence_bp
        && external.urgency == session.urgency
        && external.last_revision == session.last_revision
        && external.review == session.review;
    let execution_did_not_go_backwards = external.filled_qty <= session.filled_qty
        && external.last_event_trading_day <= session.last_event_trading_day
        && (!external.is_terminal() || session.is_terminal());
    same_revision && execution_did_not_go_backwards
}
