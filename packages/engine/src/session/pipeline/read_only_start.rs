//! 无报价到期的 Continuous tick：独立准备读取相同不可变起点，结果在原阶段消费。
use super::continuous_matching::IncrementalContinuousStockCoordinator;
use super::{
    continuous_matching::ContinuousStockInput,
    continuous_matching_adapter::prepare_incremental_continuous_inputs,
    stock_stream::continuous_shards, DecisionResourceSnapshot, GameSession, StepFatal,
};
use crate::StockId;
use std::collections::BTreeMap;

/// 终审修正：真删 Clone——深拷对 ReadOnlyStart 编译不可能（零拷由类型系统保证）。
pub(in crate::session) struct ReadOnlyStart {
    /// 捕获的交易制度截点与输入共同移交，不能只凭外层 tick/revision 猜测 shard 模式。
    pub(super) policy: super::phase::PhasePolicy,
    pub(super) hydration: Result<(), StepFatal>,
    pub(super) resources: Result<DecisionResourceSnapshot, StepFatal>,
    /// 捕获错误照旧延迟到原 stock input 消费点传播；成功时输入所有权已移入 stock_shards。
    pub(super) stock_inputs: Result<Vec<ContinuousStockInput>, StepFatal>,
    /// 输入就绪后立刻初始化的逐股 coordinator；仅消费输入本身，可安全提前到准备阶段。
    pub(super) stock_shards:
        Option<Result<BTreeMap<StockId, IncrementalContinuousStockCoordinator>, StepFatal>>,
}

// T3（ADR-0042 §T3）：SpeculativePrepareStash 与投机命中/未中计数器删除
// （commit tail 生产块已退役；非投机快速路径保留——can_prepare/prepare）。
static INLINE_PREPARE_CALLS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// 内联准备调用计数（cfg(test)，验证非投机路径按需现算）。
#[cfg(test)]
pub(in crate::session::pipeline) fn inline_prepare_calls() -> usize {
    INLINE_PREPARE_CALLS.load(std::sync::atomic::Ordering::SeqCst)
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
    #[cfg(test)]
    INLINE_PREPARE_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    prepare_with_observer(session, |_| {})
}

/// 完整 read-only 准备的读视图版本（与 prepare 同构）。Sol 终审·冗余收口：
/// 生产入口只剩 `prepare`（快照路径）；本读视图入口无生产调用方，仅测试
/// 消费——收编为 `#[cfg(test)]`，非测试构建不再编译该退役入口。
#[cfg(test)]
pub(in crate::session::pipeline) fn prepare_on(
    view: &super::npc_split::SessionReadView<'_>,
) -> ReadOnlyStart {
    prepare_on_with_observer(view, |_| {})
}

/// 快速路径资格（读视图）。P0 重叠恢复（throughput-attribution·tree31j 单
/// 变量 +15.9% 在终态树的等价形态）：与决策生产**并行**判定——读集
/// （phase/ledger/生命周期克隆）与决策写集（npc_split 写字段）不相交。
fn fast_path_eligible_on(
    view: &super::npc_split::SessionReadView<'_>,
) -> bool {
    view.phase() == crate::TradingPhase::Continuous
        && view.envelope_ledger.iter().next().is_some()
        && !view
            .npc_order_lifecycles
            .iter()
            .any(|lifecycle| lifecycle.expires_market_minute <= view.current_market_minute())
}

/// 快速路径资格+完整准备的合并入口（并行臂用）：不合格返回 None（慢路径）。
pub(in crate::session::pipeline) fn fast_path_prepare_on(
    view: &super::npc_split::SessionReadView<'_>,
) -> Option<ReadOnlyStart> {
    fast_path_eligible_on(view).then(|| prepare_on_with_observer(view, |_| {}))
}

/// 决策后资格复核（防御性一致）：决策生产理论上不写 `npc_order_lifecycles`
///（生命周期由订单落簿登记），复核用**决策后的原簿**再验一次——不一致时
/// 调用方丢弃并行准备改走慢路径，语义与串行版逐点一致。
pub(in crate::session::pipeline) fn fast_path_still_eligible(
    lifecycles: &[crate::session::NpcOrderLifecycle],
    current_market_minute: u64,
) -> bool {
    !lifecycles
        .iter()
        .any(|lifecycle| lifecycle.expires_market_minute <= current_market_minute)
}

fn prepare_on_with_observer(
    view: &super::npc_split::SessionReadView<'_>,
    observe: impl Fn(StartWork) + Sync,
) -> ReadOnlyStart {
    let (hydration, (resources, (stock_inputs, stock_shards))) = view.compute.join(
        || {
            view.compute
                .record(crate::compute::ComputeWork::ReadOnlyPrepare);
            crate::GameSession::validate_existing_live_projection_with_observer_on(view, || {
                observe(StartWork::ProjectedRow)
            })
        },
        || {
            crate::compute::join(
                || {
                    observe(StartWork::Resources);
                    super::decision_resources::DecisionResourceSnapshot::seal_on(view)
                },
                || {
                    let result = super::continuous_matching_adapter::prepare_incremental_continuous_inputs_on(view);
                    observe(StartWork::CapturedInputs);
                    match result {
                        Ok(inputs) => (
                            Ok(Vec::new()),
                            Some(continuous_shards(
                                inputs,
                                super::continuous_matching_adapter::market_data_priors(view),
                            )),
                        ),
                        Err(error) => (Err(error), None),
                    }
                },
            )
        },
    );
    ReadOnlyStart {
        policy: super::phase::PhasePolicy::capture_on(view),
        hydration,
        resources,
        stock_inputs,
        stock_shards,
    }
}

fn prepare_with_observer(
    session: &GameSession,
    observe: impl Fn(StartWork) + Sync,
) -> ReadOnlyStart {
    // 任务65 #1：与 prepare_on 严格同构——同一读视图、同一轻量
    // hydration 门（完整费用重算只在恢复/水合边界 validate_existing_live_envelope_projection）。
    prepare_on_with_observer(&session.read_view(), observe)
}

#[cfg(test)]
mod tests {
    // T3（ADR-0042 §T3·退役纪律）：SpeculativePrepareStash 专项测试 5 例随
    // 生产形状（commit tail 投机生产 + plan_tick 命中消费）删除而退役；
    // phase gate 用例经增量四十一复核属仍生产形状，恢复为非投机准备测试。
    use super::super::{plan_tick, PhaseInput};

    use super::*;
    use crate::{AccountId, Money, Order, OrderId, Side, StockId};

    /// 增量四十一（Luna 快检恢复）：非投机准备的 phase gate——仍生产的
    /// can_prepare/plan_tick 在非空 live ledger 下，CallAuction/PreOpen/
    /// ClosingAuction 不得构造 Continuous 预计算 shard（原 speculative_*
    /// 命名过度退役——本用例不属 stash 形状）。
    #[test]
    fn non_speculative_prepare_is_refused_in_every_non_continuous_phase() {
        for (tick, auction_ticks, ticks_per_day) in [(0, 3, 8), (2, 3, 8), (7, 3, 8)] {
            let game = phase_gate_fixture(tick, auction_ticks, ticks_per_day);
            let phase = game.phase();
            assert!(matches!(
                phase,
                crate::TradingPhase::CallAuction
                    | crate::TradingPhase::PreOpen
                    | crate::TradingPhase::ClosingAuction
            ));
            assert!(
                game.state.envelope_ledger.iter().next().is_some(),
                "fixture 必须持有 live ledger，否则拒绝与空账本混同"
            );
            assert!(
                !can_prepare(&game),
                "阶段 {phase:?} 不允许把连续 shard 预计算提前到竞价/盘前 tick"
            );
            let mut plan = plan_tick(PhaseInput { session: &game }).unwrap();
            assert!(
                plan.precomputed_stock_inputs.is_none() && plan.precomputed_stock_shards.is_none(),
                "非连续阶段必须走内联准备，不得残留预计算 shard"
            );
        }
    }

    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    fn phase_gate_fixture(tick: u64, auction_ticks: u64, ticks_per_day: u64) -> GameSession {
        let mut setup = crate::session::npc_working_quote_tests::quote_setup(auction_ticks);
        setup.ticks_per_day = ticks_per_day;
        setup.closing_auction_ticks = 1;
        let mut game = GameSession::new(setup, 43).unwrap();
        game.state.tick = tick;
        game.fixture_sync_market_modes_for_test();
        game.fixture_append_auction_orders(
            StockId(0),
            vec![crate::AuctionOrderSnap {
                owner: AccountId(1),
                side: Side::Buy,
                limit: Money::from_cents(900),
                qty: 100,
                order_id: 91,
            }],
        )
        .unwrap();
        game.fixture_sync_market_modes_for_test();
        game.hydrate_or_validate_envelope_ledger().unwrap();
        game
    }

    fn fixture() -> GameSession {
        let mut game =
            GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
        game.state
            .markets
            .get_mut(&StockId(0))
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
        assert!(work.stock_inputs.unwrap().is_empty());
        assert_eq!(
            work.stock_shards.as_ref().unwrap().as_ref().unwrap().len(),
            1
        );
        assert!(
            overlaps.load(Ordering::Acquire),
            "只读 stock 输入不应等待 hydration 计算结束才启动"
        );
        assert_eq!(game.business_state_hash().unwrap(), before);
    }

    #[test]
    fn eligibility_excludes_hydration_writes_and_exactly_due_quotes() {
        let mut game = fixture();
        let code = StockId(0);
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
        let plan = super::super::plan_tick(super::super::PhaseInput {
            session: &game,
        })
        .unwrap();
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
        let code = StockId(0);
        for corrupt_hydration in [true, false] {
            let mut game = fixture();
            game.fixture_account_mut(&AccountId(1))
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
                // 恢复真正财务反例：book字段与资源都不改，不能用remaining_qty替代nominal错误。
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
            let error = super::super::plan_tick(super::super::PhaseInput {
                session: &game,
            })
            .err()
            .expect("原首错必须从对应阶段返回");
            assert_eq!(error, expected);
            assert_eq!(game.business_state_hash().unwrap(), before);
        }
    }

    #[test]
    fn delayed_stock_input_error_stays_after_ingress_failure() {
        let mut game = fixture();
        let code = StockId(0);
        game.state.markets.remove(&code).unwrap();
        // map key 不一致只由 stock adapter 检查；投影与资源仍读取原 map key。
        let mut wrong_market = crate::Market::new(
            StockId(1),
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
        let before = game.business_state_hash().unwrap();
        let mut plan = super::super::plan_tick(super::super::PhaseInput {
            session: &game,
        })
        .unwrap();
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
        // T3 契约重登记（ADR-0042 §9.4 类 7 退役）：旧负例「未就绪 NPC 来源先于
        // stock input 错误」（跨 tick queue missing）随入口切换退役——决策产出
        // 恒在入口完整生产，不存在缺失形状；stock input 错误的延迟传播语义由
        // 下方两段（空决策产出下传播）承载。
        assert_eq!(game.business_state_hash().unwrap(), before);

        crate::session::pipeline::staged_decision::stage(crate::session::pipeline::NpcDecisionHandover::new(game.state.tick, Vec::new(), Vec::new(), Vec::new()));
        let before = game.business_state_hash().unwrap();
        let mut plan = super::super::plan_tick(super::super::PhaseInput {
            session: &game,
        })
        .unwrap();
        let error =
            super::super::market_tick_transaction::apply_tick_shadow_market_transaction(
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
        let mut plan = super::super::plan_tick(super::super::PhaseInput {
            session: &game,
        })
        .unwrap();
        assert!(plan.precomputed_stock_inputs.as_ref().unwrap().is_err());
        let error =
            super::super::market_tick_transaction::apply_tick_shadow_market_transaction(
                &mut plan,
            )
            .err()
            .expect("context 错误仍先于预计算 stock input 错误")
            .into_fatal();
        assert_eq!(error, context_error);
        assert_eq!(game.business_state_hash().unwrap(), before);
    }

    #[test]
    fn read_only_prepare_initializes_stock_shards_before_transaction() {
        let game = fixture();
        let expected = game.state.markets.len();
        let counter =
            &super::super::continuous_matching::incremental_continuous_stock_shadow::FROM_POST_EXPIRY_CALLS;
        // 进程级计数器会被并行测试污染；只要求本测试自身贡献足够增量，
        // 延迟初始化的实现下 prepare 不产生任何增量，仍会触发红灯。
        let before = counter.load(std::sync::atomic::Ordering::SeqCst);
        let prepared = prepare(&game);
        prepared.hydration.unwrap();
        prepared.resources.unwrap();
        prepared.stock_inputs.unwrap();
        let calls = counter.load(std::sync::atomic::Ordering::SeqCst);
        assert!(
            calls.saturating_sub(before) >= expected,
            "read-only 准备应在交易事务前完成每股 coordinator 初始化，而不是延迟到 StockProcessing 相位"
        );
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
            assert!(prepared.stock_inputs.unwrap().is_empty());
            let shards = prepared.stock_shards.unwrap().unwrap();
            let reference = continuous_shards(
                prepare_incremental_continuous_inputs(&game).unwrap(),
                super::super::continuous_matching_adapter::market_data_priors(&game.read_view()),
            )
            .unwrap();
            assert_eq!(shards.len(), reference.len());
            assert_eq!(
                format!("{shards:?}"),
                format!("{reference:?}"),
                "并行初始化的逐股 coordinator 必须与串行参考完全一致"
            );
        }
    }

    #[test]
    fn precomputed_shards_tick_matches_inline_path() {
        let run = |keep_precomputed: bool| -> String {
            let mut game = fixture();
            game.state.tick = 1;
            crate::session::pipeline::staged_decision::stage(crate::session::pipeline::NpcDecisionHandover::new(game.state.tick, Vec::new(), Vec::new(), Vec::new()));
            let before = game.business_state_hash().unwrap();
            let mut plan = super::super::plan_tick(super::super::PhaseInput {
                session: &game,
            })
            .unwrap();
            if !keep_precomputed {
                plan.precomputed_stock_shards = None;
                plan.precomputed_stock_inputs = None;
            }
            let output =
                super::super::market_tick_transaction::apply_tick_shadow_market_transaction(
                    &mut plan,
                )
                .unwrap();
            assert_eq!(game.business_state_hash().unwrap(), before);
            format!(
                "{:?}|{:?}",
                plan.envelope_ledger().unwrap(),
                (output.facts.len(), output.receipts.len())
            )
        };
        assert_eq!(
            run(true),
            run(false),
            "预计算 shards 的 tick 与内联初始化路径必须产出一致的 ledger 与事件"
        );
    }
}
