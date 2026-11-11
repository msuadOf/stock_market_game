//! 公司专户 sender 复用原现金占用、ChildTicket 与 tick undo。
use super::*;
use crate::runtime::protocol::{
    RepurchaseMaterialRestriction, RepurchasePreparedFact, RepurchasePreparedSource,
};

impl GroupState {
    pub(super) fn repurchase_source_snapshot(
        &self,
        id: crate::plans::PlanId,
    ) -> RepurchasePreparedSource {
        let projection = &self.repurchase_projections[&id];
        let tail = self.repurchase_pending_tails.get(&id);
        RepurchasePreparedSource {
            consumed_cursor: projection.book_through_cursor,
            tail_cursor: tail.map_or(projection.book_through_cursor, |tail| tail.cursor),
            facts: tail
                .into_iter()
                .flat_map(|tail| &tail.facts)
                .map(|fact| RepurchasePreparedFact {
                    cursor: fact.cursor,
                    tick: fact.tick,
                    round: fact.round,
                    source: fact.source,
                })
                .collect(),
            child_active: self
                .child_in_flight
                .contains_key(&(id.owner, crate::StockId(u32::from(projection.stock)))),
        }
    }

    pub(super) fn send_issuer_repurchase(
        &mut self,
        desks: &[impl WorkerSender<super::super::protocol::DeskMsg>],
    ) -> Result<u64, RuntimeError> {
        let mut sent = 0;
        let fail = |detail: &str| RuntimeError::ProtocolInvariant {
            detail: detail.into(),
        };
        let Some(snapshot) = self.snapshot.clone() else {
            return Ok(0);
        };
        let calendar = &snapshot.calendar;
        if calendar.phase_before() != crate::TradingPhase::Continuous {
            return Ok(0);
        }
        let day = calendar.day().day_model().current_date();
        let ids: Vec<_> = self.repurchase_projections.keys().copied().collect();
        for id in ids {
            let p = self.repurchase_projections[&id].clone();
            let crate::plans::PlanEntry::IssuerRepurchase(plan) = self
                .plan_book
                .entry(id)
                .map_err(|_| fail("回购 sender 缺少 canonical 计划"))?
            else {
                return Err(fail("回购 sender 错误来源"));
            };
            if plan.status() != crate::plans::RepurchaseExecutionStatus::Funded
                || !p.announced
                || day < p.window_start_on
                || day > p.window_deadline_on
            {
                continue;
            }
            let live = snapshot
                .repurchase_eligibility
                .get(&id)
                .ok_or_else(|| fail("回购缺少本拍真实资格 cut"))?;
            if live.as_of != calendar.observation_instant_before() {
                return Err(fail("回购资格不是本拍同源时点"));
            }
            if !repurchase_eligible(live)? {
                continue;
            }
            if p.eligibility_as_of.date() > day || p.unit_as_of > day {
                return Err(fail("回购资格来自未来"));
            }
            let stock = crate::StockId(u32::from(p.stock));
            let clock = calendar
                .before_projection()
                .stock(stock)
                .map_err(|_| fail("回购缺少本股日历"))?;
            let Some(trading_day) = clock.date_session_ordinal() else {
                continue;
            };
            let source = self.repurchase_source_snapshot(id);
            // 当天真实发送尾流或唯一 book 已记下单日，均禁止第二笔；拒收也不立即重试。
            if p.last_order_day == Some(day)
                || source.child_active
                || source
                    .facts
                    .iter()
                    .any(|fact| fact.source.request_key.requested_trading_day == trading_day)
            {
                continue;
            }
            if !source.facts.is_empty() {
                return Err(fail("前日回购来源尚未在 civil Admin 确认"));
            }
            let root = self
                .root_projection
                .as_ref()
                .ok_or_else(|| fail("回购缺少真实名册"))?;
            let global = root
                .account_ids
                .iter()
                .position(|owner| *owner == id.owner)
                .ok_or_else(|| fail("回购账户不在名册"))?;
            let local = global
                .checked_sub(self.roster_offset)
                .filter(|i| *i < self.accounts.len())
                .ok_or_else(|| fail("回购账户不属于本组"))? as LocalIdx;
            let tick = *root
                .tick_size_cents
                .get(&stock)
                .ok_or_else(|| fail("回购缺少最小价位"))?;
            if tick <= 0 || self.lot != 100 {
                return Err(fail("回购最小价位或 A 股整手配置无效"));
            }
            let exchange = calendar
                .day()
                .day_model()
                .stock(stock)
                .map_err(|_| fail("回购证券交易所缺失"))?
                .exchange();
            if exchange != p.exchange {
                return Err(fail("回购方案与现场交易所不一致"));
            }
            let cap = *root
                .max_order_qty_limit
                .get(&stock)
                .ok_or_else(|| fail("回购缺少单笔上限"))?;
            let view = snapshot
                .stocks
                .get(p.stock as usize)
                .ok_or_else(|| fail("回购缺少真实行情"))?;
            let price = p.price_cap.cents().min(view.max_buy_price).min(
                view.daily_upper_limit
                    .checked_sub(tick)
                    .ok_or_else(|| fail("回购涨停价无法减一档"))?,
            );
            let price = price / tick * tick;
            if price <= 0 || price < view.daily_lower_limit {
                continue;
            }
            let spent = p
                .book_gross
                .cents()
                .checked_add(p.book_fees.cents())
                .and_then(|v| v.checked_add(p.recycled.cents()))
                .ok_or_else(|| fail("回购预算溢出"))?;
            let budget = p
                .funding_amount
                .cents()
                .checked_sub(spent)
                .ok_or_else(|| fail("回购预算不足"))?
                .min(self.accounts[local as usize].cash);
            let remaining = p
                .max_shares
                .checked_sub(p.inventory_shares)
                .ok_or_else(|| fail("回购库存超过上限"))?;
            let mut qty = remaining
                .min(u64::from(cap))
                .min((budget.max(0) / price) as u64) as u32
                / self.lot
                * self.lot;
            // 二分搜索含最低佣金与过户费的最大整手数量，避免每手线性回退。
            let (mut lo, mut hi) = (0, qty / self.lot);
            while lo < hi {
                let mid = lo + (hi - lo).div_ceil(2);
                let purse = buy_purse_limit(
                    &self.config,
                    Money::from_cents(price),
                    mid * self.lot,
                    Money::ZERO,
                )
                .map_err(|_| fail("回购 purse 计算失败"))?
                .cents();
                if purse <= budget {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            qty = lo * self.lot;
            if qty == 0 {
                continue;
            }
            let purse = buy_purse_limit(&self.config, Money::from_cents(price), qty, Money::ZERO)
                .map_err(|_| fail("回购 purse 计算失败"))?
                .cents();
            let request = super::super::RepurchaseRequestKey {
                plan_id: id,
                stock,
                requested_trading_day: trading_day,
            };
            let acct = self.acct_mut(local)?;
            let held = acct
                .buy_purse_held_cents
                .checked_add(purse)
                .ok_or_else(|| fail("回购 held 溢出"))?;
            let active = acct.foreign_active[p.stock as usize]
                .checked_add(1)
                .ok_or_else(|| fail("回购在途计数溢出"))?;
            acct.cash -= purse;
            acct.buy_purse_held_cents = held;
            acct.foreign_active[p.stock as usize] = active;
            self.push_undo(GroupUndo::Cash {
                local,
                delta: -purse,
            });
            self.push_undo(GroupUndo::BuyPurseHeld {
                local,
                delta: purse,
            });
            self.push_undo(GroupUndo::ForeignActive {
                local,
                stock: p.stock,
                delta: 1,
            });
            self.child_in_flight.insert(
                (id.owner, stock),
                ChildTicket {
                    qty,
                    side: Side::Buy,
                    price,
                    order_id: None,
                    remaining: qty,
                    charged_cents: 0,
                    repurchase: Some(RepurchaseChildSource {
                        request_key: request,
                        purse_initial: purse,
                        returned_cumulative: 0,
                        phase: RepurchaseChildPhase::Submitted,
                        last_progress_position: None,
                    }),
                },
            );
            self.push_undo(GroupUndo::ChildInFlight {
                account: id.owner,
                code: stock,
                prev: None,
            });
            desks
                .get(p.stock as usize)
                .ok_or_else(|| fail("回购目标 Desk 缺失"))?
                .send(super::super::protocol::DeskMsg::Ops {
                    group: self.group,
                    tick: self.cur_tick,
                    round: 0,
                    final_flush: false,
                    ops: vec![DeskIn::PlaceIssuerRepurchase {
                        owner: (self.group, local),
                        request,
                        price,
                        qty,
                        purse,
                    }],
                })
                .map_err(|_| RuntimeError::SendFailed {
                    detail: "回购真实委托发送失败".into(),
                })?;
            sent += 1;
        }
        Ok(sent)
    }
}

fn repurchase_eligible(
    live: &super::super::protocol::RepurchaseEligibility,
) -> Result<bool, RuntimeError> {
    if !live.announced
        || live.issuance_in_progress.is_none()
        || live.material_restriction == RepurchaseMaterialRestriction::NotModelled
    {
        return Err(RuntimeError::ProtocolInvariant {
            detail: "回购执行资格无法证明：发行实施或重大事项限制来源未知；请反馈方案与日期".into(),
        });
    }
    Ok(live.issuance_in_progress == Some(false)
        && live.material_restriction == RepurchaseMaterialRestriction::Clear)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restricted_pauses_and_clear_resumes_unknown_is_rejected() {
        let mut live = super::super::super::protocol::RepurchaseEligibility {
            as_of: crate::CivilInstant::from_hms(
                crate::CivilDate::from_iso("2026-01-05").unwrap(),
                10,
                0,
                0,
            )
            .unwrap(),
            issuance_in_progress: Some(false),
            material_restriction: RepurchaseMaterialRestriction::Restricted,
            announced: true,
        };
        assert!(matches!(repurchase_eligible(&live), Ok(false)));
        live.material_restriction = RepurchaseMaterialRestriction::Clear;
        assert!(matches!(repurchase_eligible(&live), Ok(true)));
        live.material_restriction = RepurchaseMaterialRestriction::NotModelled;
        assert!(repurchase_eligible(&live).is_err());
    }
}
