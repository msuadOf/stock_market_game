//! 母单物化：机构方向性限价目标 → 可恢复母单计划与至多一张在途子单。

use super::*;

impl GameSession {
    /// 把机构的方向性限价目标转化为一个可恢复的母单及其至多一张子单。
    ///
    /// 一张未完全成交的子单会原样保留，避免每 tick 撤掉再报；真正的 fill 才会减少
    /// `filled_qty`。策略给出反向目标时替换旧母单，截止时间到达时停止续发，后续通用
    /// 工作单对齐会撤掉其剩余子单。
    pub(in crate::session) fn materialize_parent_order_intents(
        &mut self,
        account: AccountId,
        intents: Vec<Intent>,
        market_minute: u64,
        working: WorkingOrderSlices<'_>,
    ) -> Vec<Intent> {
        let lot_size = self.setup.config.lot_size;
        let mut requested: BTreeMap<StockCode, (Side, Money, u32)> = BTreeMap::new();
        let mut passthrough = Vec::new();
        for intent in intents {
            match intent {
                Intent::PlaceLimit {
                    code,
                    side,
                    price,
                    qty,
                } => {
                    requested.insert(code, (side, price, qty));
                }
                other => passthrough.push(other),
            }
        }

        let existing_codes: Vec<StockCode> = self
            .parent_orders
            .get(&account)
            .map(|plans| plans.keys().cloned().collect())
            .unwrap_or_default();
        let mut desired = passthrough;
        for code in existing_codes {
            let request = requested.remove(&code);
            let mut remove = false;
            {
                let plan = self
                    .parent_orders
                    .get_mut(&account)
                    .and_then(|plans| plans.get_mut(&code))
                    .expect("parent-order code was collected from its owning map");
                if plan.expires_market_minute <= market_minute {
                    remove = true;
                } else if let Some((side, price, _target_qty)) = request {
                    if side != plan.side {
                        remove = true;
                        requested.insert(code.clone(), (side, price, _target_qty));
                    } else {
                        // 同向再判断只修订报价上限；原目标数量保持，避免随每次观察重置执行进度。
                        plan.limit_price = price;
                    }
                }
            }
            if remove {
                self.parent_orders
                    .get_mut(&account)
                    .expect("parent-order owner was collected from its map")
                    .remove(&code);
                continue;
            }
            self.append_parent_child_or_working_intent(
                account,
                &code,
                market_minute,
                working,
                &mut desired,
            );
        }

        for (code, (side, price, target_qty)) in requested {
            // 不吞掉策略产生的非法数量；让权威预校验给出可见拒绝事件。
            if target_qty == 0 || !target_qty.is_multiple_of(lot_size) {
                desired.push(Intent::PlaceLimit {
                    code,
                    side,
                    price,
                    qty: target_qty,
                });
                continue;
            }
            let quarter = target_qty / 4;
            let child_qty = (quarter / lot_size)
                .max(1)
                .saturating_mul(lot_size)
                .min(target_qty);
            // 兼容母单机制启用前已存在的同向工作单：把它认领为首张子单，保留排队优先级，
            // 而不是为了开始执行而撤单重报。
            let active_child = working
                .continuous
                .iter()
                .find(|(working_code, order)| {
                    working_code == &code
                        && order.side == side
                        && order.price == price
                        && order.qty <= target_qty
                })
                .map(|(_, order)| (order.id, order.qty))
                .or_else(|| {
                    working
                        .auction
                        .iter()
                        .find(|(working_code, order)| {
                            working_code == &code
                                && order.side == side
                                && order.limit == price
                                && order.qty <= target_qty
                        })
                        .map(|(_, order)| (OrderId(order.arrival_seq), order.qty))
                });
            let expires_market_minute = market_minute
                .checked_add(PARENT_ORDER_HORIZON_MINUTES)
                .expect("market minute plus fixed parent-order horizon fits u64");
            self.parent_orders.entry(account).or_default().insert(
                code.clone(),
                ParentOrderPlan {
                    code: code.clone(),
                    side,
                    target_qty,
                    filled_qty: 0,
                    child_qty,
                    active_child_order_id: active_child.map(|(id, _)| id),
                    active_child_remaining_qty: active_child.map(|(_, qty)| qty),
                    limit_price: price,
                    expires_market_minute,
                },
            );
            if let Some((order_id, _)) = active_child {
                // 被母单认领的旧连续报价保留排队优先级，但从普通 NPC 撤单寿命中移出；
                // 此后只由母单执行状态决定它何时修订或撤销。
                self.remove_npc_order_lifecycle(account, &code, order_id);
            }
            self.append_parent_child_or_working_intent(
                account,
                &code,
                market_minute,
                working,
                &mut desired,
            );
        }
        if self
            .parent_orders
            .get(&account)
            .is_some_and(BTreeMap::is_empty)
        {
            self.parent_orders.remove(&account);
        }
        desired
    }

    pub(in crate::session) fn append_parent_child_or_working_intent(
        &self,
        account: AccountId,
        code: &StockCode,
        market_minute: u64,
        working: WorkingOrderSlices<'_>,
        desired: &mut Vec<Intent>,
    ) {
        let plan = self
            .parent_orders
            .get(&account)
            .and_then(|plans| plans.get(code))
            .expect("parent-order execution requires an active plan");
        if plan.expires_market_minute <= market_minute || plan.filled_qty >= plan.target_qty {
            return;
        }
        if let Some((_, order)) = working.continuous.iter().find(|(working_code, order)| {
            working_code == code
                && order.side == plan.side
                && plan.active_child_order_id == Some(order.id)
                && plan.active_child_remaining_qty == Some(order.qty)
        }) {
            // 收盘集合竞价期间，连续竞价簿里的旧单不能被复制为另一张竞价子单。
            // 它会在日终统一失效，母单仅保留未完成目标供下一交易日重新判断。
            if self.phase() == TradingPhase::ClosingAuction {
                return;
            }
            desired.push(Intent::PlaceLimit {
                code: code.clone(),
                side: order.side,
                price: plan.limit_price,
                qty: order.qty,
            });
            return;
        }
        if let Some((_, order)) = working.auction.iter().find(|(working_code, order)| {
            working_code == code
                && order.side == plan.side
                && plan.active_child_order_id == Some(OrderId(order.arrival_seq))
                && plan.active_child_remaining_qty == Some(order.qty)
        }) {
            desired.push(Intent::PlaceLimit {
                code: code.clone(),
                side: order.side,
                price: plan.limit_price,
                qty: order.qty,
            });
            return;
        }
        let remaining = plan.remaining_qty();
        // A 股买入申报必须整手。子单被零股卖单部分成交后，可能只剩不足一手的目标；
        // 该残余只能保留为未完成目标，不能为了“完成母单”伪造一张非法买单或超额买入。
        if plan.side == Side::Buy && remaining < self.setup.config.lot_size {
            return;
        }
        let minutes_left = plan
            .expires_market_minute
            .saturating_sub(market_minute)
            .max(1);
        let pace = u32::try_from(
            u64::from(remaining)
                .div_ceil(minutes_left)
                .min(u64::from(u32::MAX)),
        )
        .expect("clamped parent-order pace fits u32");
        let lot_size = self.setup.config.lot_size;
        let paced_lot_qty = (pace / lot_size).max(1).saturating_mul(lot_size);
        let qty = plan.child_qty.max(paced_lot_qty).min(remaining);
        desired.push(Intent::PlaceLimit {
            code: code.clone(),
            side: plan.side,
            price: plan.limit_price,
            qty,
        });
    }
}
