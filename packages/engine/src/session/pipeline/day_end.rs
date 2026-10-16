//! 所有交易阶段共用的交易日终迁移；不负责另一套订单或结算编排。

use super::{event_collection::EventFact, EventStableKey, StepFatal};
use crate::plans::PlanEvent;
use crate::{Event, GameSession};

pub(super) struct TradingDayEndTransition<'candidate> {
    candidate: &'candidate mut GameSession,
    trading_day: u64,
}

/// D5 日界安装（runtime step 的 ends_day 拍）：复用同一交易日终迁移
///（T+1 解锁/日终簿扫/candle 提交/day 推进/DayBoundary 事实——与旧管线
/// continuous_tick_finalizer 的 ends_day 段同一函数；world/desk/组的
/// 日终编排不在此，生产由 `RuntimeDriver::day_end` 承载，登记差异）。
pub(in crate::session) fn install_runtime_day_boundary(
    session: &mut GameSession,
) -> Result<Vec<EventFact>, StepFatal> {
    TradingDayEndTransition::new(session).apply()
}

impl<'candidate> TradingDayEndTransition<'candidate> {
    pub(super) fn new(candidate: &'candidate mut GameSession) -> Self {
        let trading_day = u64::from(candidate.state.day);
        Self {
            candidate,
            trading_day,
        }
    }

    pub(super) fn apply(mut self) -> Result<Vec<EventFact>, StepFatal> {
        let session = &mut *self.candidate;
        if session
            .state
            .markets
            .values()
            .any(|market| market.resting_order_count() != 0)
            || session.state.envelope_ledger.iter().next().is_some()
        {
            return Err(invariant("DayEnd retained a live order or envelope"));
        }
        if session.state.setup.t1_enabled {
            session.state.accounts.unlock_t1_positions();
        }
        self.sweep_owned_plans()?;
        let session = &mut *self.candidate;
        // 母单镜像已随执行腿唯一所有权删除（owner8）；日终不再清镜像，只清生命周期簿。
        super::quote_expiry::NpcOrderLifecycleBook::new(&mut session.state.npc_order_lifecycles)
            .clear();
        let closed_daily_candles = session.commit_active_daily_candles();
        session.state.day = session
            .state
            .day
            .checked_add(1)
            .ok_or_else(|| invariant("trading day overflow"))?;
        for history in session.state.market_minute_closes.values_mut() {
            history.clear();
        }
        let event = Event::DayBoundary {
            seq: 0,
            day: session.state.day,
            closed_daily_candles,
        };
        Ok(vec![EventFact {
            key: EventStableKey::for_event(&event, 0),
            event,
        }])
    }

    fn sweep_owned_plans(&mut self) -> Result<(), StepFatal> {
        let trading_day = self.trading_day;
        let session = &mut *self.candidate;
        let mut plans = std::mem::take(&mut session.state.plans);
        let result = (|| {
            for plan_id in plans.active_plan_ids() {
                plans
                    .apply(plan_id, PlanEvent::TradingDayEnded { trading_day })
                    .map_err(|error| {
                        invariant(&format!("日终计划 {plan_id:?} 迁移失败：{error}"))
                    })?;
            }
            Ok(())
        })();
        session.state.plans = plans;
        result
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::day_end".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::pipeline::{
        Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, FeeComponents,
    };
    use crate::{AccountId, Money, Order, OrderId, Position, Side, StockId};

    fn game() -> GameSession {
        let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
        setup.npcs.inst_count = 0;
        GameSession::new(setup, 700).unwrap()
    }

    #[test]
    fn day_end_rejects_each_live_book_mode_before_mutating_candidate_resources() {
        for mode in [
            crate::orderbook::MatchingMode::Auction,
            crate::orderbook::MatchingMode::Continuous,
        ] {
            let mut candidate = game();
            candidate
                .fixture_account_mut(&AccountId(0))
                .unwrap()
                .fixture_insert_position(
                    StockId(0),
                    Position::from_restored_parts(100, 100, 100_000, 0),
                );
            let market = candidate.state.markets.get_mut(&StockId(0)).unwrap();
            market.set_matching_mode(mode).unwrap();
            market
                .place(Order {
                    id: OrderId(1),
                    owner: AccountId(0),
                    side: Side::Buy,
                    price: Money::from_cents(900),
                    qty: 100,
                    original_qty: 100,
                    filled_qty: 0,
                    filled_value: Money::ZERO,
                    seq: 0,
                })
                .unwrap();
            let before = candidate.business_state_hash().unwrap();
            assert!(TradingDayEndTransition::new(&mut candidate)
                .apply()
                .is_err());
            assert_eq!(candidate.business_state_hash().unwrap(), before);
            assert_eq!(
                candidate.state.accounts[&AccountId(0)].positions()[&StockId(0)].t1_locked(),
                100
            );
        }
    }

    #[test]
    fn day_end_rejects_an_orphan_live_envelope_before_t1_unlock() {
        let mut candidate = game();
        candidate
            .fixture_account_mut(&AccountId(0))
            .unwrap()
            .fixture_insert_position(
                StockId(0),
                Position::from_restored_parts(100, 100, 100_000, 0),
            );
        let envelope = Envelope::tick_start_existing(
            EnvelopeKey {
                account: AccountId(0),
                stock: StockId(0),
                order: OrderId(1),
                side: Side::Sell,
            },
            Money::ZERO,
            100,
            EnvelopeAudit {
                limit: Money::from_cents(1_000),
                remaining_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        );
        candidate.state.envelope_ledger = EnvelopeLedger::new(0, [envelope]).unwrap();
        let before = candidate.business_state_hash().unwrap();
        assert!(TradingDayEndTransition::new(&mut candidate)
            .apply()
            .is_err());
        assert_eq!(candidate.business_state_hash().unwrap(), before);
    }

    #[test]
    fn day_end_transition_overflow_keeps_owned_partial_progress_without_touching_authority() {
        let mut authority = game();
        authority.state.day = u32::MAX;
        authority
            .fixture_account_mut(&AccountId(0))
            .unwrap()
            .fixture_insert_position(
                StockId(0),
                Position::from_restored_parts(100, 100, 100_000, 0),
            );
        authority.update_active_daily_candle(&StockId(0), Money::from_cents(1_000), 0);
        let before = authority.business_state_hash().unwrap();
        let history_before = authority.state.candle_book.histories()[&StockId(0)].len();
        let mut candidate = authority.clone_for_tick_shadow().unwrap();
        let error = TradingDayEndTransition::new(&mut candidate)
            .apply()
            .err()
            .expect("day 溢出必须显式失败");
        assert!(
            matches!(error, StepFatal::InvariantViolation { description, .. } if description.contains("overflow"))
        );
        assert_eq!(candidate.state.day, u32::MAX);
        assert_eq!(
            candidate.state.accounts[&AccountId(0)].positions()[&StockId(0)].t1_locked(),
            0
        );
        assert!(candidate.state.candle_book.active().is_empty());
        assert_eq!(
            candidate.state.candle_book.histories()[&StockId(0)].len(),
            history_before + 1
        );
        assert_eq!(authority.business_state_hash().unwrap(), before);
    }

    #[test]
    fn day_end_transition_is_phase_independent_and_publishes_one_closed_candle_boundary() {
        for tick in [0, 2, 3, 7] {
            let mut setup = crate::session::npc_working_quote_tests::quote_setup(3);
            setup.ticks_per_day = 8;
            setup.closing_auction_ticks = 1;
            setup.npcs.inst_count = 0;
            let mut candidate = GameSession::new(setup, 701).unwrap();
            candidate.state.tick = tick;
            candidate.update_active_daily_candle(&StockId(0), Money::from_cents(1_000), 0);
            let facts = TradingDayEndTransition::new(&mut candidate)
                .apply()
                .unwrap();
            assert_eq!(facts.len(), 1);
            assert!(
                matches!(&facts[0].event, Event::DayBoundary { day: 1, closed_daily_candles, .. }
                if closed_daily_candles.len() == 1)
            );
            assert_eq!(candidate.state.day, 1);
            assert!(candidate.state.candle_book.active().is_empty());
        }
    }
}
