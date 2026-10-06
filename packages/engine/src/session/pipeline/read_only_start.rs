//! 无报价到期的 Continuous tick：独立准备读取相同不可变起点，结果在原阶段消费。
use super::{
    continuous_matching::ContinuousStockInput,
    continuous_matching_adapter::prepare_incremental_continuous_inputs, DecisionResourceSnapshot,
    GameSession, StepFatal,
};

pub(super) struct ReadOnlyStart {
    pub(super) hydration: Result<(), StepFatal>,
    pub(super) resources: Result<DecisionResourceSnapshot, StepFatal>,
    pub(super) stock_inputs: Result<Vec<ContinuousStockInput>, StepFatal>,
}

#[derive(Clone, Copy)]
pub(super) enum StartWork {
    ProjectedRow,
    Resources,
    CapturedInputs,
}

pub(super) fn can_prepare(session: &GameSession) -> bool {
    session.phase() == crate::TradingPhase::Continuous
        && session.state.envelope_ledger.iter().next().is_some()
        && !session
            .state
            .npc_order_lifecycles
            .iter()
            .any(|lifecycle| lifecycle.expires_market_minute <= session.current_market_minute())
}

pub(super) fn prepare(session: &GameSession) -> ReadOnlyStart {
    prepare_with_observer(session, |_| {})
}

fn prepare_with_observer(
    session: &GameSession,
    observe: impl Fn(StartWork) + Sync,
) -> ReadOnlyStart {
    // 计算均只读；不在 worker 按完成先后选择错误，各 Result 留给原消费阶段。
    let (hydration, (resources, stock_inputs)) = rayon::join(
        || {
            session.validate_existing_live_projection_with_observer(|| {
                observe(StartWork::ProjectedRow)
            })
        },
        || {
            rayon::join(
                || {
                    observe(StartWork::Resources);
                    DecisionResourceSnapshot::seal(session)
                },
                || {
                    let result = prepare_incremental_continuous_inputs(session);
                    observe(StartWork::CapturedInputs);
                    result
                },
            )
        },
    );
    ReadOnlyStart {
        hydration,
        resources,
        stock_inputs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccountId, Money, Order, OrderId, Side, StockCode};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    fn fixture() -> GameSession {
        let mut game =
            GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
        game.state
            .markets
            .get_mut(&StockCode("600888".to_owned()))
            .unwrap()
            .place(Order {
                id: OrderId(91),
                owner: AccountId(1),
                side: Side::Buy,
                price: Money::from_cents(900),
                qty: 100,
                original_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                seq: 1,
            })
            .unwrap();
        game.hydrate_or_validate_envelope_ledger().unwrap();
        game
    }

    #[test]
    fn stock_input_work_starts_before_hydration_work_finishes() {
        let game = fixture();
        assert!(can_prepare(&game));
        let started = AtomicBool::new(false);
        let overlaps = AtomicBool::new(false);
        let before = game.business_state_hash().unwrap();
        let work = rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap()
            .install(|| {
                prepare_with_observer(&game, |stage| match stage {
                    StartWork::ProjectedRow => {
                        let until = Instant::now() + Duration::from_millis(200);
                        while !started.load(Ordering::Acquire) && Instant::now() < until {
                            std::thread::yield_now();
                        }
                        overlaps.store(started.load(Ordering::Acquire), Ordering::Release);
                    }
                    // stock input 的真实捕获与逐行数学已完成，才释放正在投影内部等待的行。
                    StartWork::CapturedInputs => started.store(true, Ordering::Release),
                    StartWork::Resources => {}
                })
            });
        work.hydration.unwrap();
        work.resources.unwrap();
        assert_eq!(work.stock_inputs.unwrap()[0].envelopes.len(), 1);
        assert!(
            overlaps.load(Ordering::Acquire),
            "只读 stock 输入不应等待 hydration 计算结束才启动"
        );
        assert_eq!(game.business_state_hash().unwrap(), before);
    }

    #[test]
    fn eligibility_excludes_hydration_writes_and_exactly_due_quotes() {
        let mut game = fixture();
        let code = StockCode("600888".to_owned());
        let now = game.current_market_minute();
        game.state
            .npc_order_lifecycles
            .push(crate::session::NpcOrderLifecycle {
                account: AccountId(1),
                code,
                order_id: OrderId(91),
                placed_market_minute: now,
                expires_market_minute: now + 1,
            });
        assert!(can_prepare(&game));
        game.state.npc_order_lifecycles[0].expires_market_minute = now;
        assert!(!can_prepare(&game));
        let before = game.business_state_hash().unwrap();
        let plan = super::super::plan_tick(super::super::PhaseInput { session: &game }).unwrap();
        assert_eq!(plan.expiry().releases.len(), 1);
        assert!(plan.precomputed_stock_inputs.is_none());
        assert_eq!(game.business_state_hash().unwrap(), before);
        let empty =
            GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
        assert!(!can_prepare(&empty));
        let auction =
            GameSession::new(crate::session::npc_working_quote_tests::quote_setup(3), 42).unwrap();
        assert!(!can_prepare(&auction));
    }

    #[test]
    fn hydration_then_resource_failures_keep_original_priority_and_authority() {
        let code = StockCode("600888".to_owned());
        for corrupt_hydration in [true, false] {
            let mut game = fixture();
            game.state
                .accounts
                .get_mut(&AccountId(1))
                .unwrap()
                .fixture_set_cash(Money::ZERO);
            if corrupt_hydration {
                let key = super::super::EnvelopeKey {
                    account: AccountId(1),
                    stock: code.clone(),
                    order: OrderId(91),
                    side: Side::Buy,
                };
                let original = game.state.envelope_ledger.get(&key).unwrap().clone();
                let mut audit = original.audit();
                audit.nominal.commission = Money::from_cents(1);
                audit.charged.commission = Money::from_cents(1);
                game.state.envelope_ledger.envelopes.insert(
                    key.clone(),
                    super::super::Envelope::tick_start_existing(
                        key,
                        original.live().cash,
                        0,
                        audit,
                    ),
                );
            } else {
                // hydration 与 Seal 都不消费 category；稍后的 context 必须仍排在资源错误之后。
                game.state.setup.stocks.clear();
            }
            let expected = match game.validate_existing_live_envelope_projection() {
                Err(error) => error,
                Ok(()) => DecisionResourceSnapshot::seal(&game).unwrap_err(),
            };
            let before = game.business_state_hash().unwrap();
            let error = super::super::plan_tick(super::super::PhaseInput { session: &game })
                .err()
                .expect("原首错必须从对应阶段返回");
            assert_eq!(error, expected);
            assert_eq!(game.business_state_hash().unwrap(), before);
        }
    }

    #[test]
    fn delayed_stock_input_error_stays_after_ingress_failure() {
        let mut game = fixture();
        let code = StockCode("600888".to_owned());
        game.state.markets.remove(&code).unwrap();
        // map key 不一致只由 stock adapter 检查；投影与资源仍读取原 map key。
        let mut wrong_market = crate::Market::new(
            StockCode("600889".to_owned()),
            Money::from_cents(1000),
            0.1,
            Money::from_cents(1),
        )
        .unwrap();
        wrong_market
            .place(Order {
                id: OrderId(91),
                owner: AccountId(1),
                side: Side::Buy,
                price: Money::from_cents(900),
                qty: 100,
                original_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                seq: 1,
            })
            .unwrap();
        game.state.markets.insert(code, wrong_market);
        game.state.tick = 1;
        game.state.pending_npc = None;
        let before = game.business_state_hash().unwrap();
        let mut plan =
            super::super::plan_tick(super::super::PhaseInput { session: &game }).unwrap();
        assert!(plan.precomputed_stock_inputs.as_ref().unwrap().is_err());
        let input_error = plan
            .precomputed_stock_inputs
            .as_ref()
            .unwrap()
            .as_ref()
            .err()
            .unwrap()
            .clone();
        assert!(plan.expiry().releases.is_empty());
        assert!(super::super::quote_expiry::plan_expiry(&mut plan).is_err());
        let error =
            super::super::continuous_tick_transaction::apply_tick_shadow_continuous_transaction(
                &mut plan,
            )
            .err()
            .expect("未就绪 NPC 来源仍先于 stock input 错误")
            .into_fatal();
        assert!(
            matches!(error, StepFatal::InvariantViolation { description, .. }
            if description == "next-tick NPC queue is missing from a committed market version")
        );
        assert_eq!(game.business_state_hash().unwrap(), before);

        game.state.pending_npc = Some(crate::session::PendingNpcBatch {
            observed_tick: game.state.tick,
            observed_accounts: Vec::new(),
            intents: Vec::new(),
            dependencies: Vec::new(),
        });
        let before = game.business_state_hash().unwrap();
        let mut plan =
            super::super::plan_tick(super::super::PhaseInput { session: &game }).unwrap();
        let error =
            super::super::continuous_tick_transaction::apply_tick_shadow_continuous_transaction(
                &mut plan,
            )
            .err()
            .expect("入口有效时仍须传播已准备的 stock input 错误")
            .into_fatal();
        assert_eq!(error, input_error);
        assert_eq!(game.business_state_hash().unwrap(), before);

        game.state
            .markets
            .values_mut()
            .next()
            .unwrap()
            .fixture_set_last_close(Money::ZERO);
        let context_error =
            super::super::account_validation_context::build_account_validation_context(&game)
                .err()
                .expect("fixture 必须同时触发 context 与 stock-input 错误");
        let before = game.business_state_hash().unwrap();
        let mut plan =
            super::super::plan_tick(super::super::PhaseInput { session: &game }).unwrap();
        assert!(plan.precomputed_stock_inputs.as_ref().unwrap().is_err());
        let error =
            super::super::continuous_tick_transaction::apply_tick_shadow_continuous_transaction(
                &mut plan,
            )
            .err()
            .expect("context 错误仍先于预计算 stock input 错误")
            .into_fatal();
        assert_eq!(error, context_error);
        assert_eq!(game.business_state_hash().unwrap(), before);
    }

    #[test]
    fn one_and_four_workers_preserve_resources_and_complete_stock_inputs() {
        let game = fixture();
        for threads in [1, 4] {
            let prepared = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap()
                .install(|| prepare(&game));
            prepared.hydration.unwrap();
            assert_eq!(
                prepared.resources.unwrap(),
                DecisionResourceSnapshot::seal(&game).unwrap()
            );
            let stocks = prepared.stock_inputs.unwrap();
            let original = prepare_incremental_continuous_inputs(&game).unwrap();
            assert_eq!(stocks.len(), original.len());
            for (stock, reference) in stocks.iter().zip(&original) {
                assert_eq!(stock.envelopes, reference.envelopes);
                assert_eq!(
                    stock.market.resting_orders(),
                    reference.market.resting_orders()
                );
                assert_eq!(
                    serde_json::to_vec(&stock.market.hash_projection()).unwrap(),
                    serde_json::to_vec(&reference.market.hash_projection()).unwrap(),
                    "Market 全部权威字段与 OrderBook 索引/游标/已成交身份必须一致"
                );
                assert_eq!(stock.phase, reference.phase);
                assert_eq!(
                    serde_json::to_vec(&stock.config).unwrap(),
                    serde_json::to_vec(&reference.config).unwrap()
                );
                assert!(stock.operations.is_empty());
            }
        }
    }
}
