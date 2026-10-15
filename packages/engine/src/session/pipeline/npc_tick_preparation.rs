//! 从同一密封决策快照与资源快照准备 NPC intent candidates。
//!
//! 后续账户校验、股票处理、结算、最终事件与权威提交由完整 tick coordinator 负责。

#[cfg(test)]
use super::decision_snapshot_capture::CapturedDecisionSnapshot;
#[cfg(test)]
use super::npc_decisions::run_npc_decisions;
#[cfg(test)]
use super::npc_state_projection::project_npc_state;
#[cfg(test)]
use super::DecisionSnapshot;
use super::{
    candidate_composition::CandidateCompositionError,
    decision_snapshot_capture::DecisionSnapshotCaptureError,
    npc_decisions::{NpcDecisionSourceError, NpcDecisionSourceOutput, NpcDecisionStreamError},
    npc_state_projection::{
        NpcDecisionProjectionError, NpcDecisionProjectionOutput, NpcReconciliationDecision,
    },
    IntentCandidate, IntentCandidateBatch, IntentCandidateError, IntentCandidateKey, StepFatal,
};
#[cfg(test)]
use super::decision_snapshot_capture::capture_decision_snapshot;
#[cfg(test)]
use super::npc_decisions::stream_npc_decisions_with;
#[cfg(test)]
use super::npc_state_projection::project_npc_account;
#[cfg(test)]
use crate::compute::join;
#[cfg(test)]
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
use crate::{AccountId, AccountKind, GameSession, Intent, StockId};
use std::collections::BTreeMap;
#[cfg(test)]
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub(super) enum NpcDecisionPreparationError {
    #[error("NPC decision snapshot capture failed: {0}")]
    Snapshot(#[from] DecisionSnapshotCaptureError),
    #[error("NPC DecisionAndContinuation 来源失败：{0}")]
    Source(#[from] NpcDecisionSourceError),
    #[error("NPC DecisionAndContinuation 投影失败：{0}")]
    Projection(#[from] NpcDecisionProjectionError),
    #[error("NPC DecisionAndContinuation 候选 composition 失败：{0}")]
    Composition(#[from] CandidateCompositionError),
    #[error("plan root capture failed: {0}")]
    Roots(#[from] StepFatal),
    #[error("NPC projected intent for account {account:?} has no sealed source identity")]
    UnkeyedProjectedIntent { account: AccountId },
    #[error("NPC projected source key {key:?} does not exist in the sealed raw source")]
    UnknownProjectedSource { key: IntentCandidateKey },
}

/// 为三来源 orchestrator 提供仅含 NPC 的 DecisionAndContinuation 输出。
///
/// candidate 身份按账户先覆盖 reconciliation commands，再覆盖 residual intents。
/// raw strategy 身份保留在投影中用于来源追踪；此边界尚未读取或追加玩家与计划链候选。
#[cfg(test)]
pub(super) struct PreparedNpcDecisionSource {
    pub(super) snapshot: Arc<DecisionSnapshot>,
    pub(super) projection: NpcDecisionProjectionOutput,
    pub(super) candidates: IntentCandidateBatch,
    pub(super) roots: PlanChainOperationBatch,
}

/// Test adapter for inspecting NPC projection and plan roots before next-tick queueing.
#[cfg(test)]
pub(super) fn prepare_npc_decisions(
    prospective: &mut GameSession,
    roots_override: Option<PlanChainOperationBatch>,
) -> Result<PreparedNpcDecisionSource, NpcDecisionPreparationError> {
    let (captured, source, projection, roots) =
        prepare_npc_projection(prospective, roots_override)?;
    let candidates = projected_candidates(&source, &projection)?;
    Ok(PreparedNpcDecisionSource {
        snapshot: captured.snapshot,
        projection,
        candidates,
        roots,
    })
}

#[cfg(test)]
fn prepare_npc_projection(
    prospective: &mut GameSession,
    roots_override: Option<PlanChainOperationBatch>,
) -> Result<
    (
        CapturedDecisionSnapshot,
        NpcDecisionSourceOutput,
        NpcDecisionProjectionOutput,
        PlanChainOperationBatch,
    ),
    NpcDecisionPreparationError,
> {
    let captured = capture_decision_snapshot(prospective)?;
    let config = prospective.state.setup.config.clone();
    let (source, roots) = match roots_override {
        Some(roots) => (
            run_npc_decisions(captured.snapshot.clone(), &config),
            Ok(roots),
        ),
        None => join(
            || run_npc_decisions(captured.snapshot.clone(), &config),
            || {
                prospective.capture_decision_chain_roots(
                    captured.snapshot.due_npc_ids(),
                    &captured.snapshot,
                )
            },
        ),
    };
    let mut source = source?;
    let projection = project_npc_state(prospective, &captured, &mut source)?;
    let roots = roots?;
    Ok((captured, source, projection, roots))
}

#[cfg(test)]
mod t1_account_task_tests {
    use super::*;

    /// Sol 二审修复集 5 红证：queue 的全名册到期在 runtime 真实生效——
    /// 此前 force_all_due_via_runtime 传 u64::MAX，被组端 `next_due > MAX`
    /// 守卫钳成 no-op（queue 语义没真承载）。首步后（驱动器在位→分流）
    /// queue+step：全部 retail 到期决策（订单受理或 attention 重排）。
    /// 种子扫描保证：所选种子下**无任何账户**在下一 tick 自然到期
    /// （不 force 则零决策——断言可判定）。
    #[test]
    fn sol5_queue_npc_all_due_takes_effect_in_runtime() {
        if !crate::runtime::step_runtime_enabled() {
            return;
        }
        for seed in 311..360u64 {
            let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
            setup.npcs.retail_count = 5;
            setup.strategy_params.retail.arrival_rate = 1.0;
            let mut session = crate::GameSession::new(setup, seed).unwrap();
            session.step().expect("首步（驱动器在位+镜像同步）");
            let next_tick = session.state.tick + 1;
            let dues: Vec<u64> = (1..=5)
                .map(|i| {
                    session
                        .state
                        .npc_attention
                        .get(&crate::AccountId(i))
                        .unwrap()
                        .next_attention_candidate_tick
                })
                .collect();
            if dues.iter().any(|&due| due <= next_tick) {
                continue; // 有账户自然到期——换种子（断言不可判定）。
            }
            super::queue_npc_for_next_tick(&mut session).unwrap();
            let events = session.step().expect("第二步");
            let decided_by_order = events.iter().any(|event| matches!(
                event,
                crate::Event::OrderAccepted { .. }
            ));
            let dues_after: Vec<u64> = (1..=5)
                .map(|i| {
                    session
                        .state
                        .npc_attention
                        .get(&crate::AccountId(i))
                        .unwrap()
                        .next_attention_candidate_tick
                })
                .collect();
            let decided_by_attention = dues_after.iter().zip(dues.iter()).any(|(a, b)| a != b);
            assert!(
                decided_by_order || decided_by_attention,
                "queue 的全名册到期须在 runtime 生效（种子 {seed}：{}→{dues_after:?}，MAX no-op 时零决策）",
                dues[0]
            );
            return;
        }
        panic!("未找到无自然到期的种子（35 个种子全命中——fixture 失效）");
    }

    /// 到期零售 fixture：base_probability 与策略 base_observation_probability
    /// 同步（save 校验 fault2210 与 due_retail 同款）；可选注入买入失衡分析
    /// 使决策必然产生同股 intents（顺序断言非空前提）。
    fn due_session(retail_count: u32, seed: u64, observing: bool) -> crate::GameSession {
        let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
        setup.npcs.retail_count = retail_count;
        let mut session = crate::GameSession::new(setup, seed).unwrap();
        session.rebind_compute_for_verification(
            crate::compute::ComputeContext::with_workers(4).unwrap(),
        );
        let tick = session.state.tick;
        for account in 1..=retail_count {
            let account = crate::AccountId(u64::try_from(account).unwrap());
            crate::session::npc_working_quote_tests::force_attention_candidate(
                &mut session,
                account,
                tick,
            );
            // force 固定 base_probability=1.0；全拒绝 fixture 需要极小概率
            //（仍为合法 (0,1] 值）——判定结果由 fixture 末尾断言自证。
            let probability = if observing { 1.0 } else { 1e-12 };
            session
                .state
                .npc_attention
                .get_mut(&account)
                .unwrap()
                .base_probability = probability;
            // 与 due_retail 同款策略：arrival_rate=1.0 ⇒ 每次观察产生交易
            // intents（顺序断言的非空前提）。
            let mut strategy = crate::strategy::StrategyState::ZiNoise(
                crate::strategy::ZiNoiseStrategy::new(1.0, 100, 0.5).unwrap(),
            );
            strategy.set_base_observation_probability(probability);
            session
                .fixture_account_mut(&account)
                .unwrap()
                .set_strategy(strategy.into_strategy().unwrap());
            super::super::npc_state_projection_tests::use_buy_imbalance_analysis(
                &mut session,
                account,
            );
        }
        session
    }

    /// T1 验收（两阶段审计「不存在逐 NPC 结果回调→再 install 做重投影的往返」）：
    /// queue 链账户任务流消费窗口内不得发起任何 `compute.install` 往返——
    /// 协调线程纯归并、账户任务纯计算 ⇒ 计数恒 0；旧形态（逐结果 install
    /// 投影）计数>0 可红（旧形态红证绑定增量三十二/三十三冻结目录的
    /// red-probe-vs-twophase30.patch 与 t1fix2-red-on-t30-rebuild.log）。
    #[test]
    fn npc_queue_stream_does_no_install_roundtrip_on_coordinator() {
        let mut session = due_session(2, 7, true);
        super::queue_npc_for_next_tick(&mut session).unwrap();
        assert_eq!(
            session
                .compute_context()
                .take_last_install_roundtrip_probe_count(),
            0,
            "T1：queue 链账户任务流消费窗口内不得发起 compute.install 往返（协调线程纯归并）"
        );
        assert!(
            super::staged_decision::take_any()
                .is_some_and(|batch| !batch.observed_accounts().is_empty()),
            "fixture 应产生到期 NPC（探针计数前提）"
        );
    }

    /// T1 验收（审计 §五「任务内覆盖本账户 attention 判定、个人观察/分析、
    /// 策略决策、quote/plan 个人投影」）：queue 链各计算阶段必须在**账户任务**
    /// 内执行——阶段计数=到期/受理账户数，协调线程不承担任何阶段批次。
    #[test]
    fn npc_queue_account_tasks_cover_attention_observation_analysis_decide_projection() {
        let mut session = due_session(3, 11, true);
        let _ = session.compute_context().take_queue_stage_counts();
        super::queue_npc_for_next_tick(&mut session).unwrap();
        let (attention, observation, analysis, decide, projection, captures) = session
            .compute_context()
            .take_queue_stage_counts();
        assert_eq!(captures, 1, "共享输入捕获每 tick 恰一次（不随池宽放大）");
        assert_eq!(attention, 3, "attention 判定应在每个账户任务内恰好一次");
        assert_eq!(observation, 3, "个人观察应在受理账户任务内完成");
        assert_eq!(analysis, 3, "个人分析（计划/经验前置）应在受理零售任务内完成");
        assert_eq!(decide, 3, "策略决策应在受理账户任务内完成");
        assert_eq!(projection, 3, "quote/plan 个人投影应在受理账户任务内完成");
        let batch = super::staged_decision::take_any().expect("queue 后 staged 批必在");
        assert_eq!(batch.observed_accounts().len(), 3);
    }

    /// T1 验收（审计 §五「1/2/4/16/128 worker 使用同一计算语义；任务数、
    /// 消息数和扫描次数必须可说明，不能由配置池宽无依据地放大业务工作量」）：
    /// 到期数固定 ⇒ 六档宽度下任务数=到期数、消息数=派发任务数、捕获=1、
    /// **五阶段计数（含 attention 判定）**逐一相等。
    /// 顺序语义断言（替代字节级 save 全等——合法接收顺序差异不得被抹平）：
    /// ① 批内 intents 的 owner 序 == 归并到达序（ADR-0032 真实接收事实）；
    /// ② 每账户 account_ordinal 沿批内顺序严格递增（账户内 FIFO 保序）；
    /// ③ 同股 stock_ordinal 沿批内顺序严格递增（同股 FIFO=登记序）；
    /// ④ 跨宽度 (owner,intent) 多重集相等、每账户 ordinal 序列相等
    ///    （同一冻结输入 ⇒ 同账户决策输出与局部序不变；跨账户到达序
    ///    允许随调度变化——并发策略下非确定即合法）。
    /// 本用例区分**测试线程**（cargo --test-threads 1 串行）与**计算池
    /// worker**（ComputeContext::with_workers 显式宽度，rebind 后由 queue 消费）。
    #[test]
    fn npc_queue_task_and_message_counts_do_not_scale_with_pool_width() {
        let mut per_width_sets = Vec::new();
        for workers in [1usize, 2, 4, 8, 16, 128] {
            let mut session = due_session(4, 23, true);
            session.rebind_compute_for_verification(
                crate::compute::ComputeContext::with_workers(workers).unwrap(),
            );
            let compute = session.compute_context().clone();
            let _ = compute.take_stream_workload_counts();
            let _ = compute.take_queue_stage_counts();
            let _ = compute.take_last_install_roundtrip_probe_count();
            super::queue_npc_for_next_tick(&mut session).unwrap();
            let (spawned, payloads, inline) = compute.take_stream_workload_counts();
            // 方向A·任务粗化（宽度税治理）：spawned=派发的 chunk 任务数=
            // min(due,workers)（消息数=逐账户 payload，保持=due）；inline=
            // 协调线程内联执行的账户数。不变式：payloads+inline==due（每账户
            // 恰一次交付），任务数不随池宽放大（上限=min(due,workers)）。
            assert_eq!(
                spawned, (4usize.min(workers)) as u64,
                "chunk 任务数=min(due,workers)（workers={workers}；spawned={spawned}）"
            );
            assert_eq!(
                payloads + inline, 4,
                "每账户恰一次交付（workers={workers}；payloads={payloads} inline={inline}）"
            );
            let (attention, observation, analysis, decide, projection, captures) =
                compute.take_queue_stage_counts();
            assert_eq!(
                (attention, observation, analysis, decide, projection, captures),
                (4, 4, 4, 4, 4, 1),
                "五阶段计数（含 attention）与捕获不随池宽放大（workers={workers}）"
            );
            let arrival = {
                let mut order = compute
                    .t1_probes
                    .queue_arrival_order
                    .lock()
                    .expect("到达序探针锁被 poison");
                std::mem::take(&mut *order)
            };
            let batch = super::staged_decision::take_any()
                .expect("queue 后 staged 批必在（每宽度循环各自配平）");
            assert!(
                !batch.intents().is_empty(),
                "fixture 前提自证：买入失衡分析应使各账户产生同股 intents（workers={workers}）"
            );
            // ① 真实接收序：登记序=到达序（到达记录仅在 patch 含 intents 时追加）。
            let owners: Vec<crate::AccountId> =
                batch.intents().iter().map(|received| received.owner).collect();
            let arrival_with_intents = arrival;
            assert_eq!(
                owners, arrival_with_intents,
                "ingress 登记序必须等于账户任务结果的归并到达序（ADR-0032，workers={workers}）"
            );
            // ② 每账户 account_ordinal 沿批内顺序严格递增；收集**完整序列**
            //    并跨宽度直接比较 Vec（增量三十五·Luna 快检：不压扁为末值；
            //    当前 fixture 每账户单 intent ⇒ 序列长 1，多 intent 一般序由
            //    ②递增断言+单元用例多 intent 保序实证承载）。
            let mut sequences_per_account: std::collections::BTreeMap<
                crate::AccountId,
                Vec<u64>,
            > = std::collections::BTreeMap::new();
            for received in batch.intents() {
                if let Some(last) = sequences_per_account
                    .get(&received.owner)
                    .and_then(|sequence| sequence.last())
                {
                    assert!(
                        received.account_ordinal > *last,
                        "账户内 ordinal 必须严格递增（workers={workers}）"
                    );
                }
                sequences_per_account
                    .entry(received.owner)
                    .or_default()
                    .push(received.account_ordinal);
            }
            // ③ 同股 FIFO：同股 stock_ordinal 沿批内顺序严格递增。
            let mut last_per_stock: std::collections::BTreeMap<crate::StockId, u64> =
                std::collections::BTreeMap::new();
            for received in batch.intents() {
                let (crate::Intent::PlaceLimit { code, .. } | crate::Intent::PlaceMarket { code, .. }) =
                    &received.intent
                else {
                    continue;
                };
                if let Some(last) = last_per_stock.get(code) {
                    assert!(
                        received.stock_ordinal > *last,
                        "同股 stock_ordinal 必须沿登记序严格递增（FIFO，workers={workers}）"
                    );
                }
                last_per_stock.insert(code.clone(), received.stock_ordinal);
            }
            // ④ 跨宽度顺序语义不变量（非字节全等；Intent 无 PartialEq，用
            //    Debug 形态比较多重集）。
            let mut set: Vec<String> = batch
                .intents
                .iter()
                .map(|received| format!("{:?}:{:?}", received.owner, received.intent))
                .collect();
            set.sort();
            per_width_sets.push((set, sequences_per_account));
        }
        for (set, ordinals) in &per_width_sets[1..] {
            assert_eq!(
                &per_width_sets[0].0, set,
                "同一冻结输入下各宽度 (owner,intent) 集合必须一致"
            );
            assert_eq!(
                &per_width_sets[0].1, ordinals,
                "同一冻结输入下每账户完整 ordinal 序列（Vec 直比）必须一致（账户内序不随池宽变化）"
            );
        }
    }

    /// ADR-0032 顺序契约（增量三十三·Astra 阻塞项）：NPC 结果的 ingress
    /// receipt 必须来自**真实接收事实**（结果到达消费方的顺序），不由 due
    /// 序/key/生产阶段推导。本用例以到达序探针记录的逆序构造（B 先于 A 到
    /// 达）直接驱动登记助手：断言登记序=B,A（而非 due 序 A,B）且同股
    /// stock_ordinal 沿 B→A 递增——同股 FIFO 依据真实登记顺序。
    #[test]
    fn npc_patch_intents_register_in_real_arrival_order_not_due_order() {
        let mut session = due_session(2, 37, true);
        let (_view, mut writes) = super::super::npc_split::npc_split(&mut session);
        let a = crate::AccountId(1);
        let b = crate::AccountId(2);
        let mut patch_a = crate::session::pipeline::npc_state_projection::NpcAccountPatch {
            account: a,
            observes: true,
            next_attention_candidate_tick: 1,
            attention: None,
            personal: None,
            strategy: None,
            retail_trace: None,
            experience_row: None,
            ordered_intents: vec![crate::Intent::Cancel {
                code: crate::StockId(0),
                id: crate::OrderId(11),
            }],
            local_dependencies: vec![],
        };
        // 多 intent patch（增量三十四·Luna）：账户内两笔（撤单+买入）按序
        // 登记，局部依赖随全局偏移平移——多 intent 有序性实证。
        let mut patch_b = crate::session::pipeline::npc_state_projection::NpcAccountPatch {
            account: b,
            observes: true,
            next_attention_candidate_tick: 1,
            attention: None,
            personal: None,
            strategy: None,
            retail_trace: None,
            experience_row: None,
            ordered_intents: vec![
                crate::Intent::Cancel {
                    code: crate::StockId(0),
                    id: crate::OrderId(22),
                },
                crate::Intent::PlaceLimit {
                    code: crate::StockId(0),
                    side: crate::Side::Buy,
                    price: crate::LimitPrice::Fixed(crate::Money::from_cents(1_000)),
                    qty: 100,
                },
            ],
            local_dependencies: vec![(0, 1)],
        };
        let mut intents = Vec::new();
        let mut dependencies = Vec::new();
        // 到达序 B → A（逆 due 序）。
        super::receive_npc_patch_intents_in_arrival_order(
            &mut writes,
            &mut patch_b,
            &mut intents,
            &mut dependencies,
        )
        .unwrap();
        super::receive_npc_patch_intents_in_arrival_order(
            &mut writes,
            &mut patch_a,
            &mut intents,
            &mut dependencies,
        )
        .unwrap();
        let owners: Vec<crate::AccountId> = intents.iter().map(|r| r.owner).collect();
        assert_eq!(
            owners,
            vec![b, b, a],
            "登记序必须等于真实到达序（B 先到、账户内两笔保序，A 后到）"
        );
        assert!(
            intents[0].stock_ordinal < intents[1].stock_ordinal,
            "同股 FIFO：B 先登记 ⇒ stock_ordinal 严格在前"
        );
        assert_eq!(
            intents[0].account_ordinal, intents[2].account_ordinal,
            "各自账户域首序（账户 ordinal 域互相独立，此处同为各账户首个）"
        );
        assert!(
            intents[1].account_ordinal > intents[0].account_ordinal,
            "B 账户内第二笔 ordinal 严格递增（多 intent 保序）"
        );
        assert!(
            intents[1].stock_ordinal > intents[0].stock_ordinal,
            "B 账户内同股两笔 stock_ordinal 递增（多 intent FIFO）"
        );
        assert_eq!(
            dependencies, vec![(0usize, 1usize)],
            "局部依赖 (0,1) 随 B 的登记偏移平移为全局 (0,1)"
        );
    }

    /// ADR-0032（增量三十三）：既有玩家 receipt 保持——队列登记 NPC intents
    /// 只追加新接收事实，不得改写或越过已登记的玩家同股 receipt（跨来源
    /// 同股 FIFO：先登记的玩家买入保持在前）。
    #[test]
    fn npc_queue_registration_appends_after_existing_player_receipts() {
        let mut session = due_session(2, 41, true);
        let code = crate::StockId(0);
        let player = crate::AccountId(0);
        // 预登记一个更早的玩家接收事实（同股、同侧）。
        let player_receipt = {
            let (_view, mut writes) = super::super::npc_split::npc_split(&mut session);
            super::super::npc_split::receive_private_intent_on(
                writes.ingress,
                &mut writes.ingress_receipt_cursors,
                player,
                crate::Intent::Cancel { code: code.clone(), id: crate::OrderId(7) },
            )
            .unwrap()
        };
        super::queue_npc_for_next_tick(&mut session).unwrap();
        let batch = super::staged_decision::take_any().expect("queue 后 staged 批必在");
        assert!(
            batch
                .intents
                .iter()
                .all(|received| received.stock_ordinal > player_receipt.stock_ordinal),
            "NPC 同股登记必须全部晚于既有玩家 receipt（跨来源 FIFO=登记序）"
        );
    }

    /// 增量三十三·错误边界（Astra）：attention 全拒绝的 due 零售存在时，
    /// 共享捕获（behavior_market/分析观察，按 due 超集捕获）失败**不得**使
    /// queue 失败——错误只在首个受理零售账户消费时上浮；全拒绝 ⇒ 成功安装
    /// 空观察批次（与旧「按受理集捕获」路径的错误边界一致）。
    #[test]
    fn npc_queue_all_rejected_due_retail_tolerates_shared_capture_failure() {
        let mut session = due_session(2, 43, false);
        // 确定性注入共享捕获失败：末尾追加 0 收盘分钟 ⇒ 价格路径观察守卫
        // market_minute_close_non_positive（“market minute close must be
        // positive, got 0 cents”）在 behavior 捕获链路失败——与 observing
        // 对照用例同一实际错误（非 InvalidMarketReturn；增量三十五口径统一）。
        {
            let closes = session.state.market_minute_closes.get_mut(&crate::StockId(0)).unwrap();
            let next_minute = closes
                .last()
                .map(|close| close.absolute_trading_minute + 1)
                .unwrap_or(0);
            closes.push(crate::MarketMinuteClose {
                absolute_trading_minute: next_minute,
                close: crate::Money::from_cents(0),
            });
        }
        super::queue_npc_for_next_tick(&mut session).unwrap();
        let batch = super::staged_decision::take_any().expect("queue 后 staged 批必在");
        assert!(
            batch.observed_accounts().is_empty() && batch.intents().is_empty(),
            "全拒绝 fixture 应安装空观察批次（attention 前提自证）"
        );
    }

    /// 增量三十三·错误边界对照：存在受理零售时，同一共享捕获失败必须上浮
    /// 为 queue 失败（错误不因延迟消费而被吞掉——防御式编程纪律）。
    #[test]
    fn npc_queue_observing_retail_surfaces_shared_capture_failure() {
        let mut session = due_session(2, 43, true);
        {
            let closes = session.state.market_minute_closes.get_mut(&crate::StockId(0)).unwrap();
            let next_minute = closes
                .last()
                .map(|close| close.absolute_trading_minute + 1)
                .unwrap_or(0);
            closes.push(crate::MarketMinuteClose {
                absolute_trading_minute: next_minute,
                close: crate::Money::from_cents(0),
            });
        }
        let result = super::queue_npc_for_next_tick(&mut session);
        // 「同错误上抛」明确到实际错误类型：价格路径观察守卫
        // market_minute_close_non_positive（Display=“market minute close
        // must be positive, got N cents”）经 Observation 包装上抛。
        assert!(
            matches!(result, Err(super::StepFatal::InvariantViolation { ref description, .. })
                if description.contains("market minute close must be positive")),
            "受理零售存在时共享捕获失败必须按原错误正文上抛：{result:?}"
        );
        assert!(
            !super::staged_decision::has_any(),
            "失败路径不得安装部分批次"
        );
    }

    /// 增量三十四·错误边界（Astra 混合形态）：due **Retail 全拒绝＋非
    /// Retail（Inst）受理**＋共享捕获失败 ⇒ queue 必须成功——behavior_market
    /// 为零售专用共享输入，其捕获错误只能在**受理零售**任务上浮；非零售
    /// 不消费该输入（decide 对非零售恒传 None）。旧路径仅在受理零售存在
    /// 时构建，该形态下旧路径不失败，新路径不得新增失败。
    #[test]
    fn npc_queue_all_rejected_retail_with_observing_inst_tolerates_capture_failure() {
        let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
        setup.npcs.retail_count = 2;
        setup.npcs.inst_count = 1;
        let mut session = crate::GameSession::new(setup, 47).unwrap();
        session.rebind_compute_for_verification(
            crate::compute::ComputeContext::with_workers(4).unwrap(),
        );
        let tick = session.state.tick;
        // 零售 1/2：极小概率（全拒绝，fixture 末尾断言自证）。
        for account in [crate::AccountId(1), crate::AccountId(2)] {
            crate::session::npc_working_quote_tests::force_attention_candidate(
                &mut session,
                account,
                tick,
            );
            session
                .state
                .npc_attention
                .get_mut(&account)
                .unwrap()
                .base_probability = 1e-12;
        }
        // 机构 3：必然受理。
        crate::session::npc_working_quote_tests::force_attention_candidate(
            &mut session,
            crate::AccountId(3),
            tick,
        );
        // 确定性注入共享捕获失败：末尾 0 收盘分钟 ⇒ 价格路径观察守卫
        // 「market minute close must be positive」失败（实际错误类型，
        /// 见 observing 对照用例断言）。
        {
            let closes = session.state.market_minute_closes.get_mut(&crate::StockId(0)).unwrap();
            let next_minute = closes
                .last()
                .map(|close| close.absolute_trading_minute + 1)
                .unwrap_or(0);
            closes.push(crate::MarketMinuteClose {
                absolute_trading_minute: next_minute,
                close: crate::Money::from_cents(0),
            });
        }
        super::queue_npc_for_next_tick(&mut session).unwrap();
        let batch = super::staged_decision::take_any().expect("queue 后 staged 批必在");
        assert_eq!(
            batch.observed_accounts(),
            vec![crate::AccountId(3)],
            "受理集恰为机构账户（零售全拒绝前提自证）"
        );
    }

    /// T1 验收（审计 §五「任一账户错误…均无权威部分安装」）：账户任务内
    /// 失败（零售缺经验行 ⇒ 个人观察 MissingAccount）必须以 InvariantViolation
    /// 上抛且不安装任何 pending 批次；权威层由调用方丢弃 candidate 承载
    /// （V0 原子性测试族）。
    #[test]
    fn npc_queue_account_task_failure_installs_no_partial_batch() {
        let mut session = due_session(2, 5, true);
        assert!(
            session
                .state
                .retail_experience
                .remove(&crate::AccountId(1))
                .is_some(),
            "fixture 应先存在该账户经验行"
        );
        let result = super::queue_npc_for_next_tick(&mut session);
        assert!(
            matches!(result, Err(super::StepFatal::InvariantViolation { ref description, .. })
                if description.contains("受理了不存在的账户")),
            "账户任务失败必须以 InvariantViolation 上抛：{result:?}"
        );
        assert!(
            !super::staged_decision::has_any(),
            "失败路径不得安装任何部分批次"
        );
    }
}

/// T3 批四退役登记（A1 §7 删除清单）：`queue_empty_npc_at_day_end` 空占位
/// 生产随跨 tick 载体删除——原 day_end_tests 三用例（unconsumed 守卫/非日界
/// 拒绝/u64 tick 域）随函数退役；「日终不带跨日批次」语义由
/// day_end_npc_tests 的 pending None（现为结构性不存在）与 epoch 对齐
/// fixture 场景 3/4 的日终断言承载。


/// 在完整 tick candidate 的 CommitTick 之前调用。
/// 日内决策基于即将提交的状态，其订单进入下一市场 tick；
/// 日界首个 tick 则在 ExpiryShadow 前的隔离 shadow 上基于已提交日结版本准备。
/// T3 批四·测试探针（cfg(test)）：旧「queue 下一 tick」语义——produce 后把
/// 产物放入 staged NpcDecisionHandover 槽（`pending_npc` 字段已随批四删除；
/// civil/session.rs 探针对拍等直接使用）。
#[cfg(test)]
pub(in crate::session) fn queue_npc_for_next_tick(
    session: &mut GameSession,
) -> Result<(), StepFatal> {
    // Sol 整改注入通道（cfg(test) 分流）：runtime 驱动器在位时决策权威
    /// 在组——不生产/暂存旧管线产物（take 个人态的副作用同样跳过）；
    /// 测试意图（下一 tick 全部 NPC 决策）经显式 ForceDue 指令承载
    ///（全部 belief 位+retail 位到期）。Sol 二审修复集 5：首步前（驱动器
    /// 不在位）走旧 produce/stage——个人态经构造期投影接管（staged 批
    /// 由旧管线语义持有，runtime 消费面=构造期 attention/挂单迁移）。
    #[cfg(test)]
    if session.runtime_driver.is_some() {
        session.force_all_due_via_runtime();
        return Ok(());
    }
    if staged_decision::has_any() {
        return Err(invariant(
            "next-tick NPC queue still contains an unconsumed batch",
        ));
    }
    let output = produce_decision_output(session)?;
    staged_decision::stage(output);
    Ok(())
}

/// T1·两阶段（ADR-0042 §T1·审计 §五）：queue 链账户任务化——协调线程只做
/// 三件事：scheduler 发现 due 集合、共享输入捕获一次、确定性归并安装；
/// attention 判定、个人观察/分析、策略决策、quote/plan 个人投影与经验
/// 终值全部在**账户任务**内完成（`run_npc_account_task`），任务不等待其他
/// 任务，协调线程纯归并（install 往返探针恒 0，见 t1_account_task_tests）。
/// 个人状态（attention/个人计划记忆/经验行）在捕获时 take、任务内独占
/// 演化；失败时调用方丢弃整个 tick candidate（模块既有契约）。
/// T3（ADR-0042 §9.2）·生产入口：`(a→a+1)` 调用决策阶段入口的 D(a) 生产
/// （pop 视界=已提交 tick——调用者须在 shadow tick 推进前调用）。产出 tick 内
/// `NpcDecisionHandover`，**不写回 session**（不触碰跨 tick `pending_npc`）。
pub(in crate::session) fn produce_decision_output(
    session: &mut GameSession,
) -> Result<NpcDecisionHandover, StepFatal> {
    // T3 批四·测试兼容缝（cfg(test)）：旧 fixture 经 staged NpcDecisionHandover
    // 注入或经 `queue_npc_for_next_tick`（cfg(test) 适配器）排队的批次，在
    // 消费 tick 入口转交为 tick 内 NpcDecisionHandover 同 tick 消费——保留旧
    // 「排队→下一 step 消费」的测试语义；生产路径（非测试构建）不触达本缝。
    #[cfg(test)]
    if let Some(injected) = staged_decision::take_any() {
        if injected.observed_tick() != session.state.tick {
            return Err(invariant(
                "decision output epoch does not match the consuming tick",
            ));
        }
        return Ok(injected);
    }
    let (view, mut writes) = super::npc_split::npc_split(session);
    produce_decision_output_on(&view, &mut writes)
}

/// T3 批四·测试适配器（cfg(test)）：旧直接操作 `pending_npc` 的 fixture 改经
/// staged NpcDecisionHandover 槽（字段已删除；语义=排队→下一 step 消费）。
#[cfg(test)]
pub(in crate::session::pipeline) fn queue_npc_on_for_test(
    view: &super::npc_split::SessionReadView<'_>,
    writes: &mut super::npc_split::NpcWriteFields<'_>,
) -> Result<(), StepFatal> {
    if staged_decision::has_any() {
        return Err(invariant(
            "next-tick NPC queue still contains an unconsumed batch",
        ));
    }
    let output = produce_decision_output_on(view, writes)?;
    staged_decision::stage(output);
    Ok(())
}

/// P0 重叠恢复入口（plan_tick 专用）：与只读快速路径准备共用同一 npc_split
/// 并行执行；语义与 [`produce_decision_output`] 完全一致（含 cfg(test)
/// staged 兼容缝的 epoch 校验）。
pub(in crate::session) fn produce_decision_output_at_entry(
    view: &super::npc_split::SessionReadView<'_>,
    writes: &mut super::npc_split::NpcWriteFields<'_>,
) -> Result<NpcDecisionHandover, StepFatal> {
    #[cfg(test)]
    if let Some(injected) = staged_decision::take_any() {
        if injected.observed_tick() != view.tick {
            return Err(invariant(
                "decision output epoch does not match the consuming tick",
            ));
        }
        return Ok(injected);
    }
    produce_decision_output_on(view, writes)
}

pub(in crate::session) fn produce_decision_output_on(
    view: &super::npc_split::SessionReadView<'_>,
    writes: &mut super::npc_split::NpcWriteFields<'_>,
) -> Result<NpcDecisionHandover, StepFatal> {
    // T3·调用见证（cfg(test)，Astra 四发现裁定·发现 1）：本生产调用的 attempt
    // 序号——同一 attempt 内的全部 evaluate 记录共用，作为「逐 attempt/epoch/
    // account」分组键；非测试构建零开销。
    #[cfg(test)]
    let attempt = view
        .compute
        .t1_probes
        .queue_attempt_seq
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        + 1;
    // 决策阶段生产的计算计量（原 tail 块的 CandidatePrepare 记账随 T3 前移
    // 到入口；在 compute.parallel 内记录，保持池上下文采样语义不变）。
    view.compute
        .parallel(|| view.compute.record(crate::compute::ComputeWork::CandidatePrepare));
    if writes
        .attention_scheduler
        .next_scheduled_tick()
        .is_none_or(|scheduled_tick| scheduled_tick > view.tick)
    {
        return Ok(NpcDecisionHandover::empty(view.tick));
    }
    // ===== 协调线程：scheduler 只负责发现 due 集合（pop_due 原语义）=====
    let (due, popped) = {
        let attention = &*writes.npc_attention;
        writes.attention_scheduler.pop_due(view.tick, |id| {
            attention
                .get(&id)
                .map(|state| state.next_attention_candidate_tick)
        })
    };
    // 方向B·宽度封顶（宽度税治理）：本 tick 实际并行需求 = 到期账户数 +
    // 股票数（owner 任务）——收缩实际参与宽度（恒 ≤ 池宽；「显式 N=上限」
    // 的部署合同语义不变，无可见工作的线程走空闲协议入睡）。多会话共享
    // 上下文时后写者生效（与准入台账同域，登记于方向B台账）。
    view.compute
        .set_active_width(due.len() + view.setup.stocks.len());
    if let Some(account) = popped
        .iter()
        .map(|(_, account)| *account)
        .find(|account| !writes.npc_attention.contains_key(account))
    {
        return Err(invariant(
            &super::decision_snapshot_capture::DecisionSnapshotCaptureError::MissingAttention(
                account,
            )
            .to_string(),
        ));
    }
    // ===== 协调线程：共享输入捕获一次（任务数/消息数/扫描不随池宽放大）=====
    #[cfg(test)]
    view.compute
        .t1_probes
        .queue_stage_captures
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let capture_err = |error: super::decision_snapshot_capture::DecisionSnapshotCaptureError| {
        invariant(&error.to_string())
    };
    super::decision_snapshot_capture::validate_market_view_inputs_on(view).map_err(capture_err)?;
    let market = crate::session::views::build_market_view_on(view);
    let attention_signal = crate::session::attention::market_attention_signal(&market);
    let market_minute = view.current_market_minute();
    let moment = crate::experience::ExperienceMoment {
        civil_date: view.civil_date(),
        market_minute,
        trading_day: u64::from(view.day),
    };
    let any_due_retail = due
        .iter()
        .any(|account| {
            view.accounts
                .get(account)
                .is_some_and(|entry| entry.kind() == AccountKind::Retail)
        });
    // behavior_market/分析观察按「due 集含零售」捕获（超集条件，比旧
    // 「受理集含零售」多覆盖未受理零售——共享捕获一次，登记为工作量差）。
    let behavior_market: Result<
        Option<crate::behavior::BehaviorMarketObservation>,
        String,
    > = if any_due_retail {
        super::decision_snapshot_capture::build_behavior_market_checked(view)
            .map(Some)
            .map_err(|error: super::decision_snapshot_capture::DecisionSnapshotCaptureError| {
                error.to_string()
            })
    } else {
        Ok(None)
    };
    let (working_continuous, working_auction) = if due.is_empty() {
        (BTreeMap::new(), BTreeMap::new())
    } else {
        crate::session::self_views::working_orders_for_accounts_on(
            view,
            &due.iter().copied().collect(),
        )
    };
    // 共享捕获错误延迟消费（增量三十三·错误边界）：due 超集上构建，错误
    // 存为字符串、仅在受理零售任务内上浮——全拒绝 ⇒ 不上浮（旧路径该情形
    // 根本不构建，错误边界一致）。
    let analysis = if any_due_retail {
        Some((|| {
            let observation =
                crate::session::decision_chain::DecisionChainObservation::capture_for_roots_on(
                    view,
                    market.clone(),
                )
                .map_err(|error| {
                    super::decision_snapshot_capture::DecisionSnapshotCaptureError::RetailAnalysis(
                        error,
                    )
                    .to_string()
                })?;
            let returns = observation
                .paths
                .iter()
                .map(|(stock, path)| (stock.clone(), path.thirty_minute.return_ratio))
                .collect();
            let thirty_minute_market = crate::observation::build_equal_weight_market_observation(
                &returns,
            )
            .map_err(|error| {
                super::decision_snapshot_capture::DecisionSnapshotCaptureError::Observation {
                    location: "decision_chain::retail_analysis_market",
                    account: None,
                    source: error,
                }
                .to_string()
            })?;
            Ok(NpcSharedAnalysisInputs {
                observation,
                thirty_minute_market,
            })
        })())
    } else {
        None
    };
    let urgency_policy = view.urgency_policy.clone();
    urgency_policy.validate().map_err(|error| {
        capture_err(super::decision_snapshot_capture::DecisionSnapshotCaptureError::UrgencyPolicy(
            error,
        ))
    })?;
    // ===== 协调线程：个人状态 take（每账户恰一次，O(due)；失败时随候选丢弃）=====
    let mut taken = BTreeMap::new();
    for account in &due {
        let kind = view
            .accounts
            .get(account)
            .map(|entry| entry.kind())
            .ok_or_else(|| {
                capture_err(
                    super::decision_snapshot_capture::DecisionSnapshotCaptureError::MissingAccount(
                        *account,
                    ),
                )
            })?;
        if kind == AccountKind::Player {
            return Err(capture_err(
                super::decision_snapshot_capture::DecisionSnapshotCaptureError::PlayerAccount(
                    *account,
                ),
            ));
        }
        let state = if kind == AccountKind::Retail {
            NpcTakenAccountState {
                personal: Some(
                    crate::session::decision_chain::personal_state::PlanPersonalState::take_on(
                        writes, *account,
                    ),
                ),
                attention: None,
                experience: writes.retail_experience.remove(account),
            }
        } else {
            NpcTakenAccountState {
                personal: None,
                attention: writes.npc_attention.remove(account),
                experience: None,
            }
        };
        taken.insert(*account, state);
    }
    // ===== 账户任务流（归并纯收集；安装与批次组装在流后确定性进行）=====
    let ctx = std::sync::Arc::new(NpcAccountTaskContext {
        tick: view.tick,
        npc_seed_base: view.seed,
        phase: view.phase(),
        market_minute,
        market,
        behavior_market,
        urgency_policy,
        attention_signal,
        moment,
        analysis,
        projection: super::npc_state_projection::NpcAccountProjectionContext {
            working_continuous,
            working_auction,
            cancelable_auction_window: view.tick % view.setup.ticks_per_day
                < view.setup.auction_ticks / 3,
        },
        taken: std::sync::Mutex::new(taken),
        #[cfg(test)]
        attempt,
    });
    // plans 与写组隔离（账户任务只读借用；mem::take 后在所有非 panic 出口回装）。
    let plans = std::mem::take(writes.plans);
    let config = view.setup.config.clone();
    let mut patches = BTreeMap::new();
    let mut intents = Vec::new();
    let mut dependencies = Vec::new();
    #[cfg(test)]
    view.compute.begin_install_roundtrip_probe();
    let stream_error = super::npc_decisions::stream_npc_account_tasks_with(
        std::sync::Arc::new(due.clone()),
        &view.compute,
        |_| {},
        |account| run_npc_account_task(view, &ctx, &plans, &config, account),
        |account,
         result,
         errors: &mut Vec<(
            crate::AccountId,
            super::npc_decisions::NpcDecisionStreamError<String>,
         )>| {
            match result {
                Ok(mut patch) => {
                    if patch.account != account {
                        errors.push((
                            account,
                            super::npc_decisions::NpcDecisionStreamError::Consumer(
                                super::npc_state_projection::NpcDecisionProjectionError::AccountOrderMismatch
                                    .to_string(),
                            ),
                        ));
                        return;
                    }
                    // ADR-0032：结果到达消费方即登记 ingress receipt（真实
                    // 接收序）；账户内 ordered_intents 保序。个人状态安装另
                    // 按 due 弹出序在流后确定性进行。
                    let before_intents = intents.len();
                    if let Err(error) = receive_npc_patch_intents_in_arrival_order(
                        writes,
                        &mut patch,
                        &mut intents,
                        &mut dependencies,
                    ) {
                        errors.push((
                            account,
                            super::npc_decisions::NpcDecisionStreamError::Consumer(
                                error.to_string(),
                            ),
                        ));
                        return;
                    }
                    #[cfg(test)]
                    if intents.len() > before_intents {
                        view.compute
                            .t1_probes
                            .queue_arrival_order
                            .lock()
                            .expect("到达序探针锁被 poison")
                            .push(account);
                    }
                    patches.insert(account, patch);
                }
                Err(description) => errors.push((
                    account,
                    super::npc_decisions::NpcDecisionStreamError::Consumer(description),
                )),
            }
        },
    );
    #[cfg(test)]
    view.compute.end_install_roundtrip_probe();
    *writes.plans = plans;
    if let Err(error) = stream_error {
        let description = match error {
            NpcDecisionStreamError::Decision(error) => error.to_string(),
            NpcDecisionStreamError::Consumer(error) => error,
        };
        return Err(invariant(&description));
    }
    // ===== 确定性状态安装（due 弹出序=旧批处理安装序；ingress 已在归并
    // 到达序登记——见上；此处仅个人状态落位，无逐账户计算）=====
    let mut observed_accounts = Vec::with_capacity(due.len());
    for account in &due {
        let Some(mut patch) = patches.remove(account) else {
            return Err(invariant(&format!(
                "queue 账户 {account:?} 缺少任务结果（流协议破坏）"
            )));
        };
        super::npc_state_projection::install_npc_account_patch(writes, &mut patch)
            .map_err(|error| invariant(&error.to_string()))?;
        if patch.observes {
            observed_accounts.push(*account);
        }
    }
    Ok(NpcDecisionHandover::new(
        view.tick,
        observed_accounts,
        intents,
        dependencies,
    ))
}

/// T1·两阶段（ADR-0042 §T1）：queue 链账户任务的共享输入与个人状态托管。
struct NpcAccountTaskContext {
    /// 决策共享半边（与 DecisionSnapshot 相同字段；MarketView 捕获一次共享）。
    tick: u64,
    npc_seed_base: u64,
    phase: crate::TradingPhase,
    market_minute: u64,
    market: crate::strategy::MarketView,
    /// due 集含零售才构建；构建错误**延迟消费**（增量三十三·错误边界）：
    /// 仅在首个受理零售账户的任务内上浮——attention 全拒绝时错误不上浮，
    /// 与旧「按受理集捕获」路径的错误边界一致。
    behavior_market: Result<
        Option<crate::behavior::BehaviorMarketObservation>,
        String,
    >,
    urgency_policy: crate::plans::UrgencyPolicy,
    /// 共享 attention 信号（由市场视图派生一次）。
    attention_signal: f64,
    moment: crate::experience::ExperienceMoment,
    /// 共享分析输入（due 集含零售才捕获）；错误延迟消费（同上）。
    analysis: Option<Result<NpcSharedAnalysisInputs, String>>,
    /// 投影上下文（工作单切片；捕获一次）。
    projection: super::npc_state_projection::NpcAccountProjectionContext,
    /// 个人状态托管（捕获时 take；每账户由其任务取走恰一次）。
    taken: std::sync::Mutex<BTreeMap<AccountId, NpcTakenAccountState>>,
    /// T3·调用见证（cfg(test)）：本生产调用的 attempt 序号（见 queue_npc_on 入口）。
    #[cfg(test)]
    attempt: u64,
}

impl NpcAccountTaskContext {
    /// 共享决策输入视图；behavior_market 由调用方先解析（Err=延迟的共享
    /// 捕获错误，受理零售任务上浮，见字段注释）。
    fn shared_inputs_with<'a>(
        &'a self,
        behavior_market: Option<&'a crate::behavior::BehaviorMarketObservation>,
    ) -> super::decision_snapshot::DecisionSharedInputs<'a> {
        super::decision_snapshot::DecisionSharedInputs {
            tick: self.tick,
            npc_seed_base: self.npc_seed_base,
            phase: self.phase,
            market_minute: self.market_minute,
            market: &self.market,
            behavior_market,
            urgency_policy: &self.urgency_policy,
        }
    }
}

struct NpcSharedAnalysisInputs {
    observation: crate::session::decision_chain::DecisionChainObservation,
    thirty_minute_market: crate::observation::EqualWeightMarketObservation,
}

struct NpcTakenAccountState {
    /// 机构/游资 attention 状态（take 自写组）。
    attention: Option<crate::session::attention::NpcAttentionState>,
    /// 零售个人计划状态（take_on：内含 attention/watchlist/价格记忆/信息/信念）。
    personal: Option<crate::session::decision_chain::personal_state::PlanPersonalState>,
    /// 零售经验行（take 自写组；观察/复核/修剪在任务内演化出终值）。
    experience: Option<crate::RetailExperienceState>,
}

/// T1·两阶段（ADR-0042 §T1·审计 §五）：单账户**完整计算**——attention 判定
/// →（受理时）个人观察 → 个人分析 → 策略决策 → quote/plan 个人投影。
/// 纯计算、不借用会话写组、不等待其他任务；错误字符串与旧批处理捕获
/// 路径逐类一致（DecisionSnapshotCaptureError/DecisionSnapshotError 文案）。
/// T1·ADR-0032（增量三十三·Astra 阻塞项）：账户任务结果的 ingress 登记
/// 按**真实到达序**（归并回调内，结果到达消费方时）进行——receipt 来自
/// 实际接收事实，不由 due 序/key/固定生产阶段推导；账户内 ordered_intents
/// 保序。个人状态安装另按 due 弹出序确定性进行（见 queue_npc_on 尾段）。
fn receive_npc_patch_intents_in_arrival_order(
    writes: &mut super::npc_split::NpcWriteFields<'_>,
    patch: &mut super::npc_state_projection::NpcAccountPatch,
    intents: &mut Vec<crate::ReceiptBearingIntent>,
    dependencies: &mut Vec<(usize, usize)>,
) -> Result<(), crate::SessionError> {
    let offset = intents.len();
    for intent in std::mem::take(&mut patch.ordered_intents) {
        intents.push(super::npc_split::receive_private_intent_on(
            writes.ingress,
            writes.ingress_receipt_cursors,
            patch.account,
            intent,
        )?);
    }
    dependencies.extend(
        patch
            .local_dependencies
            .iter()
            .map(|(before, after)| (offset + before, offset + after)),
    );
    Ok(())
}

fn run_npc_account_task(
    view: &super::npc_split::SessionReadView<'_>,
    ctx: &NpcAccountTaskContext,
    plans: &crate::plans::PlanBook,
    config: &crate::GameConfig,
    account: AccountId,
) -> Result<super::npc_state_projection::NpcAccountPatch, String> {
    let capture_err = |error: super::decision_snapshot_capture::DecisionSnapshotCaptureError| {
        error.to_string()
    };
    let taken = ctx
        .taken
        .lock()
        .expect("queue 账户状态托管锁被 poison")
        .remove(&account)
        .ok_or_else(|| format!("queue 账户任务缺少托管个人状态 {account:?}（调度不变量破坏）"))?;
    let entry = view
        .accounts
        .get(&account)
        .ok_or_else(|| capture_err(
            super::decision_snapshot_capture::DecisionSnapshotCaptureError::MissingAccount(
                account,
            ),
        ))?;
    let kind = entry.kind();
    if kind == AccountKind::Player {
        return Err(capture_err(
            super::decision_snapshot_capture::DecisionSnapshotCaptureError::PlayerAccount(
                account,
            ),
        ));
    }
    // —— attention 判定（任务内推进个人 RNG 流；概率有效性先行）——
    #[cfg(test)]
    view.compute
        .t1_probes
        .queue_stage_attention
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let observes;
    let next_attention_candidate_tick;
    let (mut personal, mut attention);
    match taken.personal {
        Some(mut state) => {
            validate_attention_probability(state.attention(), account)?;
            // T3·调用见证（cfg(test)，Astra 四发现裁定·发现 1）：记录 evaluate
            // 实际调用与边界前后 rng——恰一次评估的证据；受理判定后个人分析
            // 继续消费同一流（rng_after 仅到 evaluate 边界）。
            #[cfg(test)]
            let rng_before = state.attention().rng_state;
            let accepted = state
                .attention_mut()
                .evaluate_candidate_with_signal(kind, ctx.attention_signal, ctx.tick);
            #[cfg(test)]
            view.compute
                .t1_probes
                .queue_attention_trace
                .lock()
                .expect("attention 调用见证锁被 poison")
                .push(crate::compute::AttentionCallRecord {
                    attempt: ctx.attempt,
                    tick: ctx.tick,
                    account,
                    accepted,
                    rng_before,
                    rng_after: state.attention().rng_state,
                });
            next_attention_candidate_tick = state.attention().next_attention_candidate_tick;
            observes = accepted;
            personal = Some(state);
            attention = None;
        }
        None => {
            let mut state = taken.attention.ok_or_else(|| {
                capture_err(
                    super::decision_snapshot_capture::DecisionSnapshotCaptureError::MissingAttention(
                        account,
                    ),
                )
            })?;
            validate_attention_probability(&state, account)?;
            #[cfg(test)]
            let rng_before = state.rng_state;
            let accepted =
                state.evaluate_candidate_with_signal(kind, ctx.attention_signal, ctx.tick);
            #[cfg(test)]
            view.compute
                .t1_probes
                .queue_attention_trace
                .lock()
                .expect("attention 调用见证锁被 poison")
                .push(crate::compute::AttentionCallRecord {
                    attempt: ctx.attempt,
                    tick: ctx.tick,
                    account,
                    accepted,
                    rng_before,
                    rng_after: state.rng_state,
                });
            next_attention_candidate_tick = state.next_attention_candidate_tick;
            observes = accepted;
            attention = Some(state);
            personal = None;
        }
    }
    if !observes {
        // 未受理：个人经验行原样带回安装（捕获时已 take，不得丢失）。
        return Ok(crate::session::pipeline::npc_state_projection::NpcAccountPatch {
            account,
            observes: false,
            next_attention_candidate_tick,
            attention,
            personal,
            strategy: None,
            retail_trace: None,
            experience_row: taken.experience,
            ordered_intents: Vec::new(),
            local_dependencies: Vec::new(),
        });
    }
    // —— 个人观察（任务内；经验行权益/持仓观察 + SelfView + 风险观察）——
    #[cfg(test)]
    view.compute
        .t1_probes
        .queue_stage_observation
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    view.compute.record(crate::compute::ComputeWork::Observe);
    let observed = super::decision_snapshot_capture::capture_npc_account_input_on(
        view,
        account,
        kind,
        taken.experience,
        ctx.market_minute,
        ctx.phase,
        &ctx.moment,
        &ctx.projection.working_continuous,
        &ctx.projection.working_auction,
    )
    .map_err(capture_err)?;
    let mut experience_row = observed.experience_row;
    // —— 个人分析（零售；任务内——计划/经验前置计算）——
    let input = if kind == AccountKind::Retail {
        #[cfg(test)]
        view.compute
            .t1_probes
            .queue_stage_analysis
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        view.compute.record(crate::compute::ComputeWork::Analyse);
        let analysis = match ctx.analysis.as_ref() {
            None => {
                return Err("queue 缺少共享分析输入（due 集含零售则必须捕获）".to_owned())
            }
            Some(Err(description)) => return Err(description.clone()),
            Some(Ok(inputs)) => inputs,
        };
        let mut state = personal
            .take()
            .expect("零售到期账户必有托管个人计划状态");
        let row = experience_row.as_ref().ok_or_else(|| {
            capture_err(
                super::decision_snapshot_capture::DecisionSnapshotCaptureError::MissingAccount(
                    account,
                ),
            )
        })?;
        let assessments = crate::session::GameSession::observe_retail_analysis_on(
            view,
            plans,
            row,
            account,
            &mut state,
            &analysis.observation,
            &analysis.thirty_minute_market,
        )
        .map_err(|error| {
            capture_err(
                super::decision_snapshot_capture::DecisionSnapshotCaptureError::RetailAnalysis(
                    error,
                ),
            )
        })?;
        personal = Some(state);
        observed.input.with_retail_analysis(Some(assessments))
    } else {
        observed.input
    };
    // 共享 behavior_market 解析（增量三十四·Astra：零售专用输入）——捕获
    // 错误只在**受理零售**任务上浮；非零售不消费 behavior_market（decide
    // 对非零售恒传 None）。「Retail 全拒绝＋非 Retail 受理」时共享捕获错误
    // 不再影响非零售受理，与旧「受理集含零售才构建」错误边界一致。
    let behavior_market = match &ctx.behavior_market {
        Err(description) if kind == AccountKind::Retail => return Err(description.clone()),
        Err(_) => None,
        Ok(market) => market.as_ref(),
    };
    // 输入校验（镜像 DecisionSnapshot::new 的单账户规则，错误文案一致）。
    validate_npc_account_input(&input, account, kind, behavior_market, &ctx.market)?;
    // —— 策略决策（任务内；RNG=seed/tick/account 派生，与快照路径同核）——
    #[cfg(test)]
    view.compute
        .t1_probes
        .queue_stage_decide
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    view.compute.record(crate::compute::ComputeWork::Decide);
    let shared = ctx.shared_inputs_with(behavior_market);
    let source = super::npc_decisions::decide_npc_account_from_shared(&shared, &input, config, account)
        .map_err(|error| error.to_string())?;
    // —— quote/plan 个人投影 + 经验终值（任务内）——
    #[cfg(test)]
    view.compute
        .t1_probes
        .queue_stage_projection
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    view.compute.record(crate::compute::ComputeWork::Project);
    let decision = super::npc_state_projection::plan_npc_account_decision(
        &shared,
        account,
        kind,
        source,
        &ctx.projection,
        view,
        &mut experience_row,
    )?;
    Ok(crate::session::pipeline::npc_state_projection::NpcAccountPatch {
        account,
        observes: true,
        next_attention_candidate_tick,
        attention,
        personal,
        strategy: decision.strategy,
        retail_trace: decision.retail_trace,
        experience_row: decision.experience_row,
        ordered_intents: decision.ordered_intents,
        local_dependencies: decision.local_dependencies,
    })
}

/// attention 基础概率有效性（与旧批处理捕获同文案）。
fn validate_attention_probability(
    attention: &crate::session::attention::NpcAttentionState,
    account: AccountId,
) -> Result<(), String> {
    if !attention.base_probability.is_finite()
        || attention.base_probability <= 0.0
        || attention.base_probability > 1.0
    {
        return Err(
            super::decision_snapshot_capture::DecisionSnapshotCaptureError::InvalidAttentionProbability {
                account,
                probability: attention.base_probability,
            }
            .to_string(),
        );
    }
    Ok(())
}

/// 单账户输入校验（镜像 DecisionSnapshot::new 的逐账户规则；错误文案一致）。
fn validate_npc_account_input(
    input: &super::DecisionAccountInput,
    account: AccountId,
    kind: AccountKind,
    behavior_market: Option<&crate::behavior::BehaviorMarketObservation>,
    market: &crate::strategy::MarketView,
) -> Result<(), String> {
    use super::DecisionSnapshotError;
    if kind != AccountKind::Retail
        && (input.retail_analysis().is_some() || input.failure_influence().is_some())
    {
        return Err(DecisionSnapshotError::UnexpectedRetailAnalysis(account).to_string());
    }
    if let Some(analysis) = input.retail_analysis() {
        if !matches!(input.strategy_state(), crate::strategy::StrategyState::ZiNoise(_)) {
            return Err(
                DecisionSnapshotError::InvalidRetailAnalysisStrategy(account).to_string(),
            );
        }
        if let Some(code) = analysis.keys().find(|code| !market.stocks.contains_key(*code)) {
            return Err(DecisionSnapshotError::UnknownRetailAnalysisStock {
                account,
                code: code.clone(),
            }
            .to_string());
        }
    }
    if kind == AccountKind::Retail {
        if behavior_market.is_none() {
            return Err(
                DecisionSnapshotError::MissingRetailBehaviorMarket(account).to_string(),
            );
        }
        if input.account_risk().is_none() {
            return Err(DecisionSnapshotError::MissingRetailAccountRisk(account).to_string());
        }
        if input.retail_experience().is_none() {
            return Err(DecisionSnapshotError::MissingRetailExperience(account).to_string());
        }
    }
    Ok(())
}

/// T3（ADR-0042 §9.1/ADR-0032 R3）：tick 内决策产出边界 **NpcDecisionHandover**。
///
/// 承载普通 intents（已携带 receipt）、依赖边、observed accounts 与 epoch
/// （`observed_tick`）；个人 patch 随 tick 私有 candidate 演化、由 Commit 一次
/// 发布（不在此结构重复承载）；计划 root 维持 post-expiry 捕获口径（本结构
/// 不含）。决策阶段完整构造后移交执行阶段，**不写回 session** 等待下一调用。
///
/// 交接三检查（Astra 步 4 放行条件；替代 `take_ready_npc_batch` 跨 tick
/// invariant 的 tick 内形态）：
/// 1. **产物完整**：`into_batch` 校验 observed_accounts 严格递增、intents
///    owner 全部落在观察集合内、依赖表合法（镜像 fault2068–2070 语义）；
/// 2. **epoch 匹配**：`observed_tick ==` 消费 tick，否则 InvariantViolation；
/// 3. **恰一次消费**：[`take_decision_output`] 从 `Option` 槽位 take——缺失
///    或重复消费均报错。
#[derive(Clone, Debug)]
pub(in crate::session) struct NpcDecisionHandover {
    observed_tick: u64,
    observed_accounts: Vec<AccountId>,
    intents: Vec<crate::ReceiptBearingIntent>,
    dependencies: Vec<(usize, usize)>,
}

impl NpcDecisionHandover {
    /// 空批构造（无 due 账户）：完整产出，仅承载 epoch 与空载荷。
    pub(in crate::session) fn empty(observed_tick: u64) -> Self {
        Self {
            observed_tick,
            observed_accounts: Vec::new(),
            intents: Vec::new(),
            dependencies: Vec::new(),
        }
    }

    /// 生产构造（决策阶段归并完毕后一次性成批）。
    pub(in crate::session) fn new(
        observed_tick: u64,
        observed_accounts: Vec<AccountId>,
        intents: Vec<crate::ReceiptBearingIntent>,
        dependencies: Vec<(usize, usize)>,
    ) -> Self {
        Self {
            observed_tick,
            observed_accounts,
            intents,
            dependencies,
        }
    }

    /// 解构（cfg(test)：测试适配器装回跨 tick pending 用）。
    #[cfg(test)]
    pub(in crate::session::pipeline) fn into_parts(
        self,
    ) -> (
        u64,
        Vec<AccountId>,
        Vec<crate::ReceiptBearingIntent>,
        Vec<(usize, usize)>,
    ) {
        (
            self.observed_tick,
            self.observed_accounts,
            self.intents,
            self.dependencies,
        )
    }

    pub(in crate::session) fn observed_tick(&self) -> u64 {
        self.observed_tick
    }

    /// 批内容只读访问（cfg(test)：staged 注入/检查 fixture 用）。
    #[cfg(test)]
    pub(in crate::session) fn observed_accounts(&self) -> &[AccountId] {
        &self.observed_accounts
    }

    /// 批内容只读访问（cfg(test)：同上）。
    #[cfg(test)]
    pub(in crate::session) fn intents(&self) -> &[crate::ReceiptBearingIntent] {
        &self.intents
    }

    /// 批内容只读访问（cfg(test)：同上）。
    #[cfg(test)]
    pub(in crate::session) fn dependencies(&self) -> &[(usize, usize)] {
        &self.dependencies
    }

    /// 交接消费：三检查通过后转换为执行相位输入（与
    /// `take_ready_npc_batch` 的转换同构：key=账户局部序、ingress_order 携带、
    /// 依赖转 predecessors）。
    pub(in crate::session) fn into_batch(
        self,
        consuming_tick: u64,
    ) -> Result<(IntentCandidateBatch, Vec<AccountId>), StepFatal> {
        // 检查二（epoch 匹配）。
        if self.observed_tick != consuming_tick {
            return Err(invariant(
                "decision output epoch does not match the consuming tick",
            ));
        }
        // 检查一（产物完整）：observed_accounts 严格递增（镜像 fault2069）。
        if self
            .observed_accounts
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(invariant(
                "decision output observed accounts must be strictly increasing",
            ));
        }
        // 检查一（产物完整·边界修正）：owner⊆observed 不属于交接契约——那是
        // 旧跨 tick 存档校验 fault2070 的规则（批四随字段删除退役）；运行时
        // 交接只要求 observed_accounts 严格递增+依赖表合法。既有生产/注入
        // fixture 中「无观察记录但携带 intents」的形状合法（receive 事实由
        // receipt 承载，不由观察集合推导——ADR-0032 决策第 1 条）。
        // 检查一（产物完整）：依赖表合法（镜像 fault2068）。
        self.validate_dependencies().map_err(|error| invariant(&error))?;
        let ready = self;
        let mut local = BTreeMap::<AccountId, u64>::new();
        let mut candidates = Vec::with_capacity(ready.intents.len());
        for received in ready.intents {
            let account = received.owner;
            let index = local.entry(account).or_default();
            candidates.push(
                IntentCandidate::new(
                    IntentCandidateKey::npc(account, *index),
                    account,
                    received.intent,
                )
                .with_ingress_order(received.account_ordinal, received.stock_ordinal),
            );
            *index = index
                .checked_add(1)
                .ok_or_else(|| invariant("queued NPC local sequence overflow"))?;
        }
        Ok((
            IntentCandidateBatch::new(with_dependencies(candidates, ready.dependencies))
                .map_err(|error| invariant(&error.to_string()))?,
            ready.observed_accounts,
        ))
    }

    /// 依赖表校验（镜像 `PendingNpcBatch::validate_dependencies` 的规则；该
    /// 跨 tick 载体退役后本检查即依赖语义的唯一承载）。
    fn validate_dependencies(&self) -> Result<(), String> {
        let mut seen = std::collections::BTreeSet::new();
        for &(before, after) in &self.dependencies {
            let prefix = format!("decision output dependency {before}->{after}");
            if before >= after || after >= self.intents.len() {
                return Err(format!(
                    "{prefix} must reference earlier and later queued intents"
                ));
            }
            if !seen.insert((before, after)) {
                return Err(format!("{prefix} is duplicated"));
            }
            let before = &self.intents[before];
            let after = &self.intents[after];
            let crate::Intent::Cancel {
                code: before_code, ..
            } = &before.intent
            else {
                return Err(format!("{prefix} predecessor is not a cancellation"));
            };
            let after_code = match &after.intent {
                crate::Intent::PlaceLimit { code, .. } | crate::Intent::PlaceMarket { code, .. } => {
                    code
                }
                crate::Intent::Cancel { .. } => {
                    return Err(format!("{prefix} successor is not a placement"));
                }
            };
            if before.owner != after.owner || before_code != after_code {
                return Err(format!(
                    "{prefix} must belong to the same account and stock"
                ));
            }
        }
        Ok(())
    }
}

/// 交接三检查之三（恰一次消费）：从决策阶段的移交槽位取走 NpcDecisionHandover。
/// 槽位为 None（未生产或已消费）⇒ InvariantViolation——生产先于消费在同一
/// 调用内执行时缺失即流协议破坏。
pub(in crate::session) fn take_decision_output(
    slot: &mut Option<NpcDecisionHandover>,
    consuming_tick: u64,
) -> Result<NpcDecisionHandover, StepFatal> {
    let output = slot.take().ok_or_else(|| {
        invariant("tick decision output is missing or was already consumed")
    })?;
    if output.observed_tick() != consuming_tick {
        return Err(invariant(
            "decision output epoch does not match the consuming tick",
        ));
    }
    Ok(output)
}

/// T3 批四·测试适配（cfg(test)）：直接消费当前 staged 批（旧
/// `take_ready_npc_batch` 的直接消费形态——不经 step 入口；跨 tick invariant
/// 已由 NpcDecisionHandover tick 内三检查承载）。
#[cfg(test)]
pub(in crate::session) fn take_staged_npc_batch(
    session: &mut GameSession,
) -> Result<(IntentCandidateBatch, Vec<AccountId>), StepFatal> {
    let output = staged_decision::take_matching(session.state.tick).ok_or_else(|| {
        invariant("next-tick NPC queue is missing from a committed market version")
    })?;
    output.into_batch(session.state.tick)
}

/// T3 批四·测试适配面（cfg(test)）：`pending_npc` 字段删除后的「排队→下一
/// step 消费」等价物（A1 §7 删除清单/A4 #8；48 冻结附页批四形状建议）。
/// staged 槽为 thread-local——libtest 每用例独立线程天然隔离；produce 入口
/// 在协调（测试）线程消费，同线程读写。与 session 状态/存档零关联。
#[cfg(test)]
pub(in crate::session) mod staged_decision {
    use super::NpcDecisionHandover;
    use std::cell::RefCell;

    thread_local! {
        static STAGED: RefCell<Vec<NpcDecisionHandover>> = const { RefCell::new(Vec::new()) };
    }

    /// 注入一批（旧 fixture 手动 `pending_npc = Some(PendingNpcBatch{..})`
    /// 注入的等价物，字段一一对应：observed_tick/observed_accounts/intents/
    /// dependencies → NpcDecisionHandover::new）。
    pub(in crate::session) fn stage(output: NpcDecisionHandover) {
        STAGED.with(|slot| slot.borrow_mut().push(output));
    }

    /// 旧「unconsumed batch」守卫：已存在 staged 批则拒绝再排队。
    pub(in crate::session) fn has_any() -> bool {
        STAGED.with(|slot| !slot.borrow().is_empty())
    }

    /// 清空槽位（旧「candidate 丢弃 ⇒ pending 批随之作废」的影子丢弃语义）。
    pub(in crate::session) fn clear() {
        STAGED.with(|slot| slot.borrow_mut().clear());
    }

    /// 旧 seam 的 take 语义：FIFO 取出首批（错位检查由调用方保留）。
    pub(in crate::session) fn take_any() -> Option<NpcDecisionHandover> {
        STAGED.with(|slot| slot.borrow_mut().pop())
    }

    /// 只读窥视（旧 `pending_npc.as_ref()` 探针语义——批留在槽内供后续
    /// step 消费；take 会破坏「探针后消费」的既有 fixture 流程）。
    pub(in crate::session) fn peek_any() -> Option<NpcDecisionHandover> {
        STAGED.with(|slot| slot.borrow().last().cloned())
    }

    /// 按 tick 匹配取出（直接消费形态；无匹配返回 None）。
    pub(in crate::session) fn take_matching(tick: u64) -> Option<NpcDecisionHandover> {
        STAGED.with(|slot| {
            let mut staged = slot.borrow_mut();
            staged
                .iter()
                .position(|output| output.observed_tick() == tick)
                .map(|index| staged.remove(index))
        })
    }
}



fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::npc_tick_preparation".to_owned(),
    }
}
#[cfg(test)]
fn projected_candidates(
    source: &NpcDecisionSourceOutput,
    projection: &NpcDecisionProjectionOutput,
) -> Result<IntentCandidateBatch, NpcDecisionPreparationError> {
    let ordered = projected_ordered_intents(source, projection)?;
    let mut local = BTreeMap::<AccountId, u64>::new();
    let mut projected = Vec::with_capacity(ordered.intents.len());
    for (account, intent) in ordered.intents {
        let index = local.entry(account).or_default();
        projected.push(IntentCandidate::new(
            IntentCandidateKey::npc(account, *index),
            account,
            intent,
        ));
        *index = index
            .checked_add(1)
            .ok_or(NpcDecisionSourceError::IntentOrdinalOverflow { account })?;
    }
    IntentCandidateBatch::new(with_dependencies(projected, ordered.dependencies))
        .map_err(CandidateCompositionError::Candidate)
        .map_err(Into::into)
}

struct ProjectedNpcIntents {
    intents: Vec<(AccountId, Intent)>,
    dependencies: Vec<(usize, usize)>,
}

fn with_dependencies(
    candidates: Vec<IntentCandidate>,
    dependencies: Vec<(usize, usize)>,
) -> Vec<IntentCandidate> {
    let mut predecessors = BTreeMap::<usize, Vec<IntentCandidateKey>>::new();
    for (before, after) in dependencies {
        predecessors
            .entry(after)
            .or_default()
            .push(candidates[before].key().clone());
    }
    candidates
        .into_iter()
        .enumerate()
        .map(|(index, candidate)| match predecessors.remove(&index) {
            Some(keys) => candidate.with_predecessors(keys),
            None => candidate,
        })
        .collect()
}

fn projected_ordered_intents(
    source: &NpcDecisionSourceOutput,
    projection: &NpcDecisionProjectionOutput,
) -> Result<ProjectedNpcIntents, NpcDecisionPreparationError> {
    let mut raw_by_key = BTreeMap::new();
    for raw in source.intents() {
        let key = raw.key();
        let account = match key {
            IntentCandidateKey::Npc { account, .. } => *account,
            _ => return Err(CandidateCompositionError::NonNpcKey(key.clone()).into()),
        };
        if raw_by_key.insert(key, account).is_some() {
            return Err(
                CandidateCompositionError::Candidate(IntentCandidateError::DuplicateKey(
                    key.clone(),
                ))
                .into(),
            );
        }
    }
    let mut by_account = BTreeMap::<AccountId, Vec<Intent>>::new();
    let mut reconciliation_cancels = BTreeMap::<(AccountId, StockId), Vec<usize>>::new();
    let mut local_dependencies = BTreeMap::<AccountId, Vec<(usize, usize)>>::new();
    for decision in projection.reconciliation_decisions() {
        let (account, code, order_id) = match decision {
            NpcReconciliationDecision::Keep { .. } => continue,
            NpcReconciliationDecision::Cancel {
                account,
                code,
                order_id,
            } => (*account, code, *order_id),
            NpcReconciliationDecision::Replace {
                account,
                old_order_id,
                new_intent,
            } => {
                let code = match new_intent {
                    Intent::PlaceLimit { code, .. }
                    | Intent::PlaceMarket { code, .. }
                    | Intent::Cancel { code, .. } => code,
                };
                (*account, code, *old_order_id)
            }
        };
        // The reconciliation planner keeps a replacement's place in residual_intents.
        // Several old quotes can refer to that same desired place: cancel all of them,
        // then route the cash-capped residual once, as in the existing NPC executor.
        let ordered = by_account.entry(account).or_default();
        reconciliation_cancels
            .entry((account, code.clone()))
            .or_default()
            .push(ordered.len());
        ordered.push(Intent::Cancel {
            code: code.clone(),
            id: order_id,
        });
    }
    for intent in projection.residual_intents() {
        let key = intent.source_key().cloned().ok_or(
            NpcDecisionPreparationError::UnkeyedProjectedIntent {
                account: intent.account(),
            },
        )?;
        let raw_owner = raw_by_key.get(&key).ok_or_else(|| {
            NpcDecisionPreparationError::UnknownProjectedSource { key: key.clone() }
        })?;
        if *raw_owner != intent.account() {
            return Err(CandidateCompositionError::NpcOwnerMismatch {
                key,
                owner: intent.account(),
            }
            .into());
        }
        let ordered = by_account.entry(intent.account()).or_default();
        // Only cancellations produced by reconciliation are predecessors. A raw
        // cancellation remains an independent request even on the same stock.
        if let Intent::PlaceLimit { code, .. } | Intent::PlaceMarket { code, .. } =
            intent.projected_intent()
        {
            if let Some(cancellations) =
                reconciliation_cancels.get(&(intent.account(), code.clone()))
            {
                local_dependencies
                    .entry(intent.account())
                    .or_default()
                    .extend(cancellations.iter().map(|before| (*before, ordered.len())));
            }
        }
        ordered.push(intent.projected_intent().clone());
    }
    let mut projected = Vec::new();
    let mut dependencies = Vec::new();
    for (account, intents) in by_account {
        let offset = projected.len();
        if let Some(local) = local_dependencies.remove(&account) {
            dependencies.extend(
                local
                    .into_iter()
                    .map(|(before, after)| (offset + before, offset + after)),
            );
        }
        for intent in intents {
            projected.push((account, intent));
        }
    }
    Ok(ProjectedNpcIntents {
        intents: projected,
        dependencies,
    })
}
