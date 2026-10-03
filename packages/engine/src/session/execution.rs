//! 会话执行接缝：机构母单物化、NPC 工作单对账计划与订单记录。

mod orders;
pub(in crate::session) mod reconcile_plan;
mod records;

use super::*;
use crate::plans::PlanId;

/// 可恢复的大资金母单：目标与实际成交分离，后续子单不得超过 remaining_qty。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ParentOrderPlan {
    pub(in crate::session::execution) code: StockCode,
    pub(in crate::session::execution) side: Side,
    pub(in crate::session::execution) target_qty: u32,
    pub(in crate::session::execution) filled_qty: u32,
    /// 单次重新报价所允许的最大子单数量。
    pub(in crate::session::execution) child_qty: u32,
    /// 当前在订单簿或集合竞价队列中的子单；只有该订单的真实成交可以推进母单。
    pub(in crate::session::execution) active_child_order_id: Option<OrderId>,
    /// 上述子单尚未成交的数量；与订单簿中的剩余数量严格一致。
    pub(in crate::session::execution) active_child_remaining_qty: Option<u32>,
    /// 显式计划执行接缝的所有者；未接计划的既有母单保持 None 且序列化字节不变。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub(in crate::session::execution) linked_plan_id: Option<PlanId>,
    pub(in crate::session::execution) limit_price: Money,
    #[serde(with = "super::u64_decimal")]
    #[ts(type = "string")]
    pub(in crate::session::execution) expires_market_minute: u64,
}

impl ParentOrderPlan {
    pub fn remaining_qty(&self) -> u32 {
        self.target_qty
            .checked_sub(self.filled_qty)
            .expect("parent-order filled quantity exceeds target")
    }
}

/// 单个母单真实成交后的局部结果；Session 决定 map 清理与 PendingPlanEvent。
pub(in crate::session) struct ParentFillTransition {
    pub(in crate::session) child_complete: bool,
    pub(in crate::session) completed_unlinked: bool,
    pub(in crate::session) linked_plan_id: Option<PlanId>,
}

impl ParentOrderPlan {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::session) fn from_facts(
        code: StockCode,
        side: Side,
        target_qty: u32,
        filled_qty: u32,
        child_qty: u32,
        active: Option<(OrderId, u32)>,
        linked_plan_id: Option<PlanId>,
        limit_price: Money,
        expires_market_minute: u64,
    ) -> Self {
        Self::from_saved_facts(
            code,
            side,
            target_qty,
            filled_qty,
            child_qty,
            active.map(|(id, _)| id),
            active.map(|(_, qty)| qty),
            linked_plan_id,
            limit_price,
            expires_market_minute,
        )
    }

    /// 恢复接缝保留尚未从 live order 重建的剩余数量，不提前改变存档校验顺序。
    #[allow(clippy::too_many_arguments)]
    pub(in crate::session) fn from_saved_facts(
        code: StockCode,
        side: Side,
        target_qty: u32,
        filled_qty: u32,
        child_qty: u32,
        active_child_order_id: Option<OrderId>,
        active_child_remaining_qty: Option<u32>,
        linked_plan_id: Option<PlanId>,
        limit_price: Money,
        expires_market_minute: u64,
    ) -> Self {
        Self {
            code,
            side,
            target_qty,
            filled_qty,
            child_qty,
            active_child_order_id,
            active_child_remaining_qty,
            linked_plan_id,
            limit_price,
            expires_market_minute,
        }
    }

    pub fn code(&self) -> &StockCode {
        &self.code
    }
    pub fn side(&self) -> Side {
        self.side
    }
    pub fn target_qty(&self) -> u32 {
        self.target_qty
    }
    pub fn filled_qty(&self) -> u32 {
        self.filled_qty
    }
    pub fn child_qty(&self) -> u32 {
        self.child_qty
    }
    pub fn active_child_order_id(&self) -> Option<OrderId> {
        self.active_child_order_id
    }
    pub fn active_child_remaining_qty(&self) -> Option<u32> {
        self.active_child_remaining_qty
    }
    pub fn linked_plan_id(&self) -> Option<PlanId> {
        self.linked_plan_id
    }
    pub fn limit_price(&self) -> Money {
        self.limit_price
    }
    pub fn expires_market_minute(&self) -> u64 {
        self.expires_market_minute
    }

    fn replace_active_child(&mut self, active: Option<(OrderId, u32)>) {
        self.replace_child_facts(active.map(|(id, _)| id), active.map(|(_, qty)| qty));
    }

    fn replace_child_facts(&mut self, id: Option<OrderId>, remaining: Option<u32>) {
        self.active_child_order_id = id;
        self.active_child_remaining_qty = remaining;
    }

    pub(in crate::session) fn restore_active_child_remaining_qty(
        &mut self,
        remaining: Option<u32>,
    ) {
        self.replace_child_facts(self.active_child_order_id, remaining);
    }

    #[cfg(test)]
    pub(in crate::session) fn replace_execution_facts_for_test(
        &mut self,
        filled_qty: u32,
        id: Option<OrderId>,
        remaining: Option<u32>,
    ) {
        self.filled_qty = filled_qty;
        self.replace_child_facts(id, remaining);
    }

    pub(in crate::session) fn record_submission(
        &mut self,
        side: Side,
        order_id: OrderId,
        qty: u32,
    ) -> bool {
        if self.side != side {
            return false;
        }
        assert!(
            self.active_child_order_id.is_none(),
            "parent-order accepted a second active child before the first resolved"
        );
        self.replace_active_child(Some((order_id, qty)));
        true
    }

    pub(in crate::session) fn record_fill_after_settlement(
        &mut self,
        side: Side,
        order_id: OrderId,
        qty: u32,
    ) -> Option<ParentFillTransition> {
        if self.side != side || self.active_child_order_id != Some(order_id) {
            return None;
        }
        let remaining_child_qty = self
            .active_child_remaining_qty
            .expect("active parent-order id must carry its remaining quantity")
            .checked_sub(qty)
            .expect("a parent-order fill cannot exceed its active child quantity");
        // 连续路径保留先写 filled_qty 再断言 target 的既有失败时点；竞价 checked 路径另行预校验。
        self.filled_qty = self
            .filled_qty
            .checked_add(qty)
            .expect("a parent-order child cannot fill beyond u32 capacity");
        assert!(
            self.filled_qty <= self.target_qty,
            "a parent-order child filled beyond its target"
        );
        self.replace_active_child(
            (remaining_child_qty != 0).then_some((order_id, remaining_child_qty)),
        );
        Some(self.fill_transition(remaining_child_qty))
    }

    fn fill_transition(&self, remaining_child_qty: u32) -> ParentFillTransition {
        ParentFillTransition {
            child_complete: remaining_child_qty == 0,
            completed_unlinked: self.filled_qty == self.target_qty && self.linked_plan_id.is_none(),
            linked_plan_id: self.linked_plan_id,
        }
    }

    pub(in crate::session) fn clear_matching_child(&mut self, order_id: OrderId) -> bool {
        if self.active_child_order_id != Some(order_id) {
            return false;
        }
        self.replace_active_child(None);
        true
    }

    pub(in crate::session) fn checked_record_submission(
        &mut self,
        side: Side,
        order_id: OrderId,
        qty: u32,
    ) -> Result<bool, &'static str> {
        if self.side != side {
            return Ok(false);
        }
        if self.active_child_order_id.is_some() || self.active_child_remaining_qty.is_some() {
            return Err("linked parent accepted a second active auction child");
        }
        self.replace_active_child(Some((order_id, qty)));
        Ok(true)
    }

    pub(in crate::session) fn checked_record_fill(
        &mut self,
        side: Side,
        order_id: OrderId,
        qty: u32,
    ) -> Result<Option<ParentFillTransition>, &'static str> {
        if self.side != side || self.active_child_order_id != Some(order_id) {
            return Ok(None);
        }
        let child_before = self
            .active_child_remaining_qty
            .ok_or("linked parent active child has no remaining quantity")?;
        let child_after = child_before
            .checked_sub(qty)
            .ok_or("auction fill exceeds linked parent child quantity")?;
        let filled_after = self
            .filled_qty
            .checked_add(qty)
            .ok_or("linked parent auction fill quantity overflow")?;
        if filled_after > self.target_qty {
            return Err("linked parent auction fill exceeds its target quantity");
        }
        self.filled_qty = filled_after;
        self.replace_active_child((child_after != 0).then_some((order_id, child_after)));
        Ok(Some(self.fill_transition(child_after)))
    }

    pub(in crate::session) fn checked_clear_matching_child(
        &mut self,
        order_id: OrderId,
        remaining_qty: u32,
    ) -> Result<bool, &'static str> {
        if self.active_child_order_id != Some(order_id) {
            return Ok(false);
        }
        if self.active_child_remaining_qty != Some(remaining_qty) {
            return Err("auction cancellation disagrees with linked parent child quantity");
        }
        self.replace_active_child(None);
        Ok(true)
    }

    pub(in crate::session) fn revise_same_side_limit(&mut self, side: Side, price: Money) -> bool {
        if self.side != side {
            return false;
        }
        self.limit_price = price;
        true
    }
}

#[cfg(test)]
mod parent_transition_tests {
    use super::*;

    fn parent(linked: bool) -> ParentOrderPlan {
        ParentOrderPlan::from_facts(
            StockCode("600000".into()),
            Side::Buy,
            100,
            0,
            100,
            None,
            linked.then_some(PlanId(7)),
            Money::from_cents(1_000),
            10,
        )
    }

    #[test]
    fn only_matching_settled_child_advances_and_completion_retains_linked_parent() {
        for linked in [false, true] {
            let mut parent = parent(linked);
            assert!(!parent.record_submission(Side::Sell, OrderId(1), 100));
            assert!(parent.record_submission(Side::Buy, OrderId(1), 100));
            assert!(parent
                .record_fill_after_settlement(Side::Sell, OrderId(1), 10)
                .is_none());
            assert!(parent
                .record_fill_after_settlement(Side::Buy, OrderId(2), 10)
                .is_none());
            assert!(!parent.clear_matching_child(OrderId(2)));
            let partial = parent
                .record_fill_after_settlement(Side::Buy, OrderId(1), 40)
                .unwrap();
            assert!(!partial.child_complete);
            assert_eq!(parent.filled_qty(), 40);
            assert_eq!(parent.active_child_remaining_qty(), Some(60));
            let complete = parent
                .record_fill_after_settlement(Side::Buy, OrderId(1), 60)
                .unwrap();
            assert!(complete.child_complete);
            assert_eq!(complete.completed_unlinked, !linked);
            assert_eq!(parent.active_child_order_id(), None);
            assert_eq!(parent.active_child_remaining_qty(), None);
        }
    }

    #[test]
    fn auction_invalid_transitions_preserve_candidate_parent() {
        let mut parent = parent(true);
        parent
            .checked_record_submission(Side::Buy, OrderId(1), 100)
            .unwrap();
        let before = parent.clone();
        assert!(parent
            .checked_record_submission(Side::Buy, OrderId(2), 100)
            .is_err());
        assert!(parent
            .checked_record_fill(Side::Buy, OrderId(1), 101)
            .is_err());
        assert!(parent.checked_clear_matching_child(OrderId(1), 99).is_err());
        assert_eq!(parent, before);
        assert!(parent
            .checked_clear_matching_child(OrderId(1), 100)
            .unwrap());
        assert_eq!(parent.filled_qty(), 0);
        parent.replace_execution_facts_for_test(0, None, Some(100));
        assert!(parent
            .checked_record_submission(Side::Buy, OrderId(2), 100)
            .is_err());
    }

    #[test]
    fn continuous_assert_and_auction_checked_fill_keep_distinct_failure_boundaries() {
        let mut continuous = ParentOrderPlan::from_facts(
            StockCode("600000".into()),
            Side::Buy,
            100,
            0,
            100,
            Some((OrderId(1), 150)),
            None,
            Money::from_cents(1_000),
            10,
        );
        let mut auction = continuous.clone();
        assert!(auction
            .checked_record_fill(Side::Buy, OrderId(1), 101)
            .is_err());
        assert_eq!(auction.filled_qty(), 0);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            continuous.record_fill_after_settlement(Side::Buy, OrderId(1), 101);
        }))
        .is_err());
        assert_eq!(continuous.filled_qty(), 101);
        assert_eq!(continuous.active_child_remaining_qty(), Some(150));
        let mut duplicate = parent(false);
        duplicate.record_submission(Side::Buy, OrderId(1), 100);
        let before = duplicate.clone();
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            duplicate.record_submission(Side::Buy, OrderId(2), 100);
        }))
        .is_err());
        assert_eq!(duplicate, before);
    }

    #[test]
    fn unlinked_parent_reverse_rebuilds_execution_from_the_new_target() {
        let mut session =
            GameSession::new(super::super::npc_working_quote_tests::quote_setup(0), 993).unwrap();
        let account = AccountId(1);
        let code = StockCode("600888".into());
        let empty = WorkingOrderSlices {
            continuous: &[],
            auction: &[],
        };
        session.materialize_parent_order_intents(
            account,
            vec![Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 400,
            }],
            0,
            empty,
        );
        let order_id = OrderId(17);
        session.record_parent_order_submission(account, &code, Side::Buy, order_id, 100);
        // 以结算成功的 typed fill fixture 固定 adapter 的真实身份推进，受理本身不计成交。
        session.record_parent_order_fills(
            &code,
            &[OrderFillSettlement {
                account,
                side: Side::Buy,
                order_id,
                qty: 40,
                #[cfg(feature = "simulation-diagnostics")]
                gross: Money::from_cents(40_000),
            }],
        );
        let old = &session.state.parent_orders[&account][&code];
        assert_eq!(old.filled_qty(), 40);
        assert_eq!(old.active_child_order_id(), Some(order_id));
        assert_eq!(old.active_child_remaining_qty(), Some(60));
        let working = [(
            code.clone(),
            Order {
                id: order_id,
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 60,
                original_qty: 100,
                filled_qty: 40,
                filled_value: Money::from_cents(40_000),
                owner: account,
                seq: 17,
            },
        )];
        let desired = session.materialize_parent_order_intents(
            account,
            vec![Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(980)),
                qty: 800,
            }],
            7,
            WorkingOrderSlices {
                continuous: &working,
                auction: &[],
            },
        );
        let new = &session.state.parent_orders[&account][&code];
        assert_eq!(new.side(), Side::Sell);
        assert_eq!(new.target_qty(), 800);
        assert_eq!(new.filled_qty(), 0);
        assert_eq!(new.child_qty(), 200);
        assert_eq!(new.active_child_order_id(), None);
        assert_eq!(new.active_child_remaining_qty(), None);
        assert_eq!(new.limit_price(), Money::from_cents(980));
        assert_eq!(
            new.expires_market_minute(),
            7 + PARENT_ORDER_HORIZON_MINUTES
        );
        assert_eq!(new.linked_plan_id(), None);
        assert!(matches!(desired.as_slice(), [Intent::PlaceLimit {
            code: intent_code, side: Side::Sell, price: LimitPrice::Fixed(price), qty: 200,
        }] if intent_code == &code && *price == Money::from_cents(980)));
    }

    #[test]
    fn closing_auction_never_duplicates_a_live_continuous_child_and_expiry_stops_intents() {
        let code = StockCode("600000".into());
        let mut parent = parent(false);
        parent.record_submission(Side::Buy, OrderId(1), 100);
        let continuous = [(
            code,
            Order {
                id: OrderId(1),
                side: Side::Buy,
                price: Money::from_cents(1_000),
                qty: 100,
                original_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                owner: AccountId(1),
                seq: 1,
            },
        )];
        let working = WorkingOrderSlices {
            continuous: &continuous,
            auction: &[],
        };
        assert!(matches!(
            parent.desired_child_intent(0, TradingPhase::Continuous, 100, working),
            Some(Intent::PlaceLimit { qty: 100, .. })
        ));
        assert!(parent
            .desired_child_intent(0, TradingPhase::ClosingAuction, 100, working)
            .is_none());
        assert!(parent
            .desired_child_intent(10, TradingPhase::Continuous, 100, working)
            .is_none());
    }

    #[test]
    fn same_side_revision_keeps_target_and_sub_lot_buy_remaining_unsubmitted() {
        let mut parent = parent(false);
        parent.record_submission(Side::Buy, OrderId(1), 100);
        parent.record_fill_after_settlement(Side::Buy, OrderId(1), 50);
        parent.clear_matching_child(OrderId(1));
        assert!(parent.revise_same_side_limit(Side::Buy, Money::from_cents(1_010)));
        assert_eq!(parent.target_qty(), 100);
        assert_eq!(parent.filled_qty(), 50);
        assert!(!parent.revise_same_side_limit(Side::Sell, Money::from_cents(990)));
        assert_eq!(parent.limit_price(), Money::from_cents(1_010));
        assert!(parent
            .desired_child_intent(
                0,
                TradingPhase::Continuous,
                100,
                WorkingOrderSlices {
                    continuous: &[],
                    auction: &[]
                }
            )
            .is_none());
    }
}
