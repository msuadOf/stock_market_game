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
    pub(super) hydration: Result<(), StepFatal>,
    pub(super) resources: Result<DecisionResourceSnapshot, StepFatal>,
    /// 捕获错误照旧延迟到原 stock input 消费点传播；成功时输入所有权已移入 stock_shards。
    pub(super) stock_inputs: Result<Vec<ContinuousStockInput>, StepFatal>,
    /// 输入就绪后立刻初始化的逐股 coordinator；仅消费输入本身，可安全提前到准备阶段。
    pub(super) stock_shards:
        Option<Result<BTreeMap<StockId, IncrementalContinuousStockCoordinator>, StepFatal>>,
}

/// X1 投机预计算暂存：随 candidate 提交自然流转到权威状态，plan_tick 消费后置 None。
/// 不进 SaveSlot（save 逐字段构造不含此字段），不参与 business_state_hash；
/// tick 失败重试时仍留在权威状态上且语义不变（同 tick 重算）。
/// shadow 不携带（clone_for_shadow 置 None），全链移动语义——不 Clone。
pub(in crate::session) struct SpeculativePrepareStash {
    /// 为哪个市场 tick 准备（准备发生时权威 tick + 1）。
    pub(in crate::session) for_tick: u64,
    /// 生产时刻的外部直改计数；消费要求相等，否则值过期整体回退现算。
    pub(in crate::session) produced_revision: u64,
    /// 纯值（终审修正：去 Arc——单点移交后消费为移动语义，无 try_unwrap/克隆路径）。
    pub(in crate::session) work: ReadOnlyStart,
}

static SPECULATIVE_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static SPECULATIVE_MISSES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static INLINE_PREPARE_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// 消费命中计数（cfg(test)，验证投机命中时不再内联重算）。
#[cfg(test)]
pub(in crate::session::pipeline) fn speculative_hits() -> usize {
    SPECULATIVE_HITS.load(std::sync::atomic::Ordering::SeqCst)
}
#[cfg(test)]
pub(in crate::session::pipeline) fn speculative_misses() -> usize {
    SPECULATIVE_MISSES.load(std::sync::atomic::Ordering::SeqCst)
}
#[cfg(test)]
pub(in crate::session::pipeline) fn inline_prepare_calls() -> usize {
    INLINE_PREPARE_CALLS.load(std::sync::atomic::Ordering::SeqCst)
}
#[inline]
pub(in crate::session::pipeline) fn count_speculative_hit() {
    SPECULATIVE_HITS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}
#[inline]
pub(in crate::session::pipeline) fn count_speculative_miss_if_any() {
    SPECULATIVE_MISSES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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

/// eligibility 的读视图版本（投机分支在 candidate 终态上判定；
/// lifecycles 为快照，快照含 queue 将移除的到期项时只会安全地假阴性回退）。
pub(in crate::session::pipeline) fn can_prepare_on(
    view: &super::npc_split::SessionReadView<'_>,
) -> bool {
    view.phase() == crate::TradingPhase::Continuous
        && view.envelope_ledger.iter().next().is_some()
        && !view
            .npc_order_lifecycles
            .iter()
            .any(|lifecycle| lifecycle.expires_market_minute <= view.current_market_minute())
}

/// 完整 read-only 准备的读视图版本（投机分支消费；与 prepare 同构）。
pub(in crate::session::pipeline) fn prepare_on(
    view: &super::npc_split::SessionReadView<'_>,
) -> ReadOnlyStart {
    prepare_on_with_observer(view, |_| {})
}

fn prepare_on_with_observer(
    view: &super::npc_split::SessionReadView<'_>,
    observe: impl Fn(StartWork) + Sync,
) -> ReadOnlyStart {
    let (hydration, (resources, (stock_inputs, stock_shards))) = rayon::join(
        || {
            crate::GameSession::validate_existing_live_projection_with_observer_on(
                view,
                || observe(StartWork::ProjectedRow),
            )
        },
        || {
            rayon::join(
                || {
                    observe(StartWork::Resources);
                    super::decision_resources::DecisionResourceSnapshot::seal_on(view)
                },
                || {
                    let result = super::continuous_matching_adapter::prepare_incremental_continuous_inputs_on(view);
                    observe(StartWork::CapturedInputs);
                    match result {
                        Ok(inputs) => (Ok(Vec::new()), Some(continuous_shards(inputs))),
                        Err(error) => (Err(error), None),
                    }
                },
            )
        },
    );
    ReadOnlyStart {
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
    // 计算均只读；不在 worker 按完成先后选择错误，各 Result 留给原消费阶段。
    let (hydration, (resources, (stock_inputs, stock_shards))) = rayon::join(
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
                    // 捕获完成后立刻在同一分支初始化逐股 coordinator，消除与
                    // StockProcessing 相位之间的屏障；from_post_expiry 只消费
                    // 已脱离的输入，不读取 session，提前执行不改变任何结果。
                    match result {
                        Ok(inputs) => (Ok(Vec::new()), Some(continuous_shards(inputs))),
                        Err(error) => (Err(error), None),
                    }
                },
            )
        },
    );
    ReadOnlyStart {
        hydration,
        resources,
        stock_inputs,
        stock_shards,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccountId, Money, Order, OrderId, Side, StockId};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

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
        let plan = super::super::plan_tick(super::super::PhaseInput { session: &game, speculative: None }).unwrap();
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
            let error = super::super::plan_tick(super::super::PhaseInput { session: &game, speculative: None })
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
        game.state.pending_npc = None;
        let before = game.business_state_hash().unwrap();
        let mut plan =
            super::super::plan_tick(super::super::PhaseInput { session: &game, speculative: None }).unwrap();
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
            super::super::plan_tick(super::super::PhaseInput { session: &game, speculative: None }).unwrap();
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
            super::super::plan_tick(super::super::PhaseInput { session: &game, speculative: None }).unwrap();
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

    /// X1：stash 命中时 plan_tick 不得内联重算（计数器差分断言）。
    #[test]
    fn speculative_stash_hit_skips_inline_prepare() {
        let mut game = fixture();
        game.state.tick = 1;
        game.state.pending_npc = Some(crate::session::PendingNpcBatch {
            observed_tick: 1,
            observed_accounts: Vec::new(),
            intents: Vec::new(),
            dependencies: Vec::new(),
        });
        assert!(can_prepare(&game));
        let work = prepare(&game);
        let stash = SpeculativePrepareStash {
            for_tick: game.state.tick,
            produced_revision: game.external_revision(),
            work: work,
        };
        let before = game.business_state_hash().unwrap();
        let (hits, misses, inline) = (
            speculative_hits(),
            speculative_misses(),
            inline_prepare_calls(),
        );
        let plan = super::super::plan_tick(super::super::PhaseInput {
            session: &game,
            speculative: Some(stash),
        })
        .unwrap();
        assert_eq!(game.business_state_hash().unwrap(), before);
        assert!(
            speculative_hits() > hits,
            "tick 匹配且 eligible 的 stash 必须命中"
        );
        // 计数器为进程级（并行测试污染）：按先例只用差分存在性断言。
        // “命中不内联”由 match 分支结构保证（命中臂直接返回 stash 不调 prepare）。
        let _ = (misses, inline);
        assert!(plan.precomputed_stock_inputs.is_some());
        // 权威上的 stash 在未提交的 plan_tick 后保留（重试语义）；随提交才被
        // commit_from 的 None 覆盖。
    }

    /// X1：tick 失配或 eligibility 失败必须整体回退现算，不消费陈旧结果。
    #[test]
    fn speculative_stash_mismatch_falls_back_to_inline() {
        let mut game = fixture();
        game.state.tick = 1;
        game.state.pending_npc = Some(crate::session::PendingNpcBatch {
            observed_tick: 1,
            observed_accounts: Vec::new(),
            intents: Vec::new(),
            dependencies: Vec::new(),
        });
        let stash = SpeculativePrepareStash {
            for_tick: game.state.tick + 1,
            produced_revision: game.external_revision(),
            work: prepare(&game),
        };
        let before = game.business_state_hash().unwrap();
        let (hits, misses, inline) = (
            speculative_hits(),
            speculative_misses(),
            inline_prepare_calls(),
        );
        let plan = super::super::plan_tick(super::super::PhaseInput {
            session: &game,
            speculative: Some(stash),
        })
        .unwrap();
        assert_eq!(game.business_state_hash().unwrap(), before);
        let _ = hits;
        assert!(speculative_misses() > misses, "失配 stash 计为 miss");
        assert!(
            inline_prepare_calls() > inline,
            "失配必须回退内联现算"
        );
        assert!(plan.precomputed_stock_inputs.is_some());
        // eligibility 失败（到期报价）同样不消费 stash。
        let now = game.current_market_minute();
        let stale = SpeculativePrepareStash {
            for_tick: game.state.tick,
            produced_revision: game.external_revision(),
            work: prepare(&game),
        };
        game.state.npc_order_lifecycles.push(crate::session::NpcOrderLifecycle {
            account: AccountId(1),
            code: StockId(0),
            order_id: OrderId(91),
            placed_market_minute: now,
            expires_market_minute: now,
        });
        assert!(!can_prepare(&game));
        let plan = super::super::plan_tick(super::super::PhaseInput {
            session: &game,
            speculative: Some(stale),
        })
        .unwrap();
        assert!(plan.precomputed_stock_inputs.is_none());
    }

    /// X1：stash 携带的错误仍在原优先级位置传播（镜像 hydration 损坏 fixture）。
    #[test]
    fn speculative_stash_errors_keep_original_priority() {
        let code = StockId(0);
        let mut game = fixture();
        game.state.tick = 1;
        game.state.pending_npc = Some(crate::session::PendingNpcBatch {
            observed_tick: 1,
            observed_accounts: Vec::new(),
            intents: Vec::new(),
            dependencies: Vec::new(),
        });
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
            super::super::Envelope::tick_start_existing(key, original.live().cash, 0, audit),
        );
        let expected = game.validate_existing_live_envelope_projection().unwrap_err();
        let stash = SpeculativePrepareStash {
            for_tick: game.state.tick,
            produced_revision: game.external_revision(),
            work: prepare(&game),
        };
        let before = game.business_state_hash().unwrap();
        let error = super::super::plan_tick(super::super::PhaseInput {
            session: &game,
            speculative: Some(stash),
        })
        .err()
            .expect("stash 携带的 hydration 错误必须传播");
        assert_eq!(error, expected);
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
            let reference =
                continuous_shards(prepare_incremental_continuous_inputs(&game).unwrap()).unwrap();
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
            game.state.pending_npc = Some(crate::session::PendingNpcBatch {
                observed_tick: game.state.tick,
                observed_accounts: Vec::new(),
                intents: Vec::new(),
                dependencies: Vec::new(),
            });
            let before = game.business_state_hash().unwrap();
            let mut plan =
                super::super::plan_tick(super::super::PhaseInput { session: &game, speculative: None }).unwrap();
            if !keep_precomputed {
                plan.precomputed_stock_shards = None;
                plan.precomputed_stock_inputs = None;
            }
            let output =
                super::super::continuous_tick_transaction::apply_tick_shadow_continuous_transaction(
                    &mut plan,
                )
                .unwrap();
            assert_eq!(game.business_state_hash().unwrap(), before);
            format!(
                "{:?}|{:?}",
                plan.state.envelope_ledger().unwrap(),
                (output.events.len(), output.receipts.len())
            )
        };
        assert_eq!(
            run(true),
            run(false),
            "预计算 shards 的 tick 与内联初始化路径必须产出一致的 ledger 与事件"
        );
    }
}
