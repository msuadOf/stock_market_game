//! Sol 终裁二轮·观察面补强+文案收窄（增量七十七更新注释）。
//! **形态边界（收窄）**：同次 yield 人工拆分后两次 initial=**流级组合验证**
//! ——验证流实例对跨批输入的路由账目正确性，不是生产 drive 分批来源的
//! 见证（生产分批来源=逐 FeedOutcome 的进度回传；本形态是流 API 层的合法
//! 组合）。**不可区分结论（收窄）**：在无专用注入的真实流可达状态内，72
//! 按股清除与 75 逐操作释放不可区分（四重防线见
//! logs/t4inc76-negative-attribution.log）；不代表注入形态下的行为等价。
//! **观察面（七十七补强；七十八叙述修正）**：真实观察者=账户 2 计划的
//! **未成交子单后的同计划后续操作**（对手卖单固定价 1000 与买价 900 不
//! 成交——子单 resting 未成交，计划仍推进下一命令）——前批回传后仍扣留
//! （(2,stock) 路由在飞），尾批回传后实际放行，且按 owner+code+计划链键+
//! sealed_index 前进断言**操作身份**（非仅 Operations 变体形态）。
use super::account_validation_context::build_account_validation_context;
use super::adaptive_plan_chain::AdaptivePlanChainCoordinator;
use super::continuous_matching::ContinuousExecutionRound;
use super::ready_stock_stream::{FeedOutcome, ReadyStockStream};
use super::stock_stream::StockStreamProgress;
use super::{
    AccountValidatorDriver, CandidateValidationOutcome, DecisionResourceSnapshot, EnvelopeLedger,
    IntentCandidate, IntentCandidateKey, StepFatal,
};
use crate::session::plan_continuation_domain::PlanContinuationDomain;
use crate::{AccountId, GameSession, Intent, Money, Side, StockId};

struct RouteRig {
    session: GameSession,
    chain: AdaptivePlanChainCoordinator,
    validator: AccountValidatorDriver,
    receipts: super::local_admission::AccountReceipts,
    all_candidates: Vec<IntentCandidate>,
}

/// 为指定账户现建一份 (stock=0) 买入计划执行请求（fixture 同形状）。
fn plan_request_for(
    session: &mut GameSession,
    account: AccountId,
    salt: i64,
    target: u64,
) -> crate::session::plan_execution::PlanExecutionRequest {
    use crate::plans::quote_policy::{QuoteAction, QuoteDecision, QuoteReason};
    use crate::plans::{AllocationGrant, PlanOpen, PlanOpinion, PlanTarget, Urgency};
    let code = StockId(0);
    let plan_id = session
        .state
        .plans
        .create(PlanOpen {
            account,
            code: code.clone(),
            direction: Side::Buy,
            target: PlanTarget::ShareCount(u32::try_from(target).unwrap()),
            opinion: PlanOpinion {
                signal_score_bp: 3_000,
                source: crate::plans::OpinionSource::Blended,
            },
            confidence_bp: 8_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 0,
        })
        .unwrap();
    crate::session::plan_execution::PlanExecutionRequest {
        plan_id,
        allocation: AllocationGrant {
            plan_id,
            code,
            allocated_cash: Money::from_cents(1_000_000),
            constraint: None,
        },
        decision: QuoteDecision {
            action: QuoteAction::Submit {
                price: Money::from_cents(900 + salt),
                qty: 100,
            },
            reason: QuoteReason::OppositeQuoteProbe,
        },
        trading_day: 0,
    }
}

fn route_rig() -> RouteRig {
    let compute = crate::compute::ComputeContext::with_workers(2).unwrap();
    let (mut session, request) =
        crate::session::plan_chain_candidates_tests::execution_fixture_with_compute(
            std::sync::Arc::clone(&compute),
        );
    // 对手卖单：账户 1 持仓背书后挂 200@1000（add_resting 固定价）——
    // 与账户 2 买价 900 **不成交**：子单 resting 未成交，计划在未成交状态
    // 下推进下一命令=尾批复批后的真实观察者（七十八叙述修正：七十七
    // 「子单成交后续行」表述不实）。
    let code = StockId(0);
    session
        .fixture_account_mut(&AccountId(1))
        .unwrap()
        .grant_position(code.clone(), 400, Money::from_cents(900))
        .unwrap();
    let sell = super::continuous_matching::incremental_continuous_stock_shadow::tests::add_resting(
        session.fixture_market_mut(&code).unwrap(),
        &code,
        AccountId(1),
        crate::OrderId(9_001),
        Side::Sell,
        200,
    );
    session.state.next_receipt_base = 0;
    session.state.envelope_ledger =
        crate::session::pipeline::EnvelopeLedger::new(0, std::iter::once(sell.envelope.clone()))
            .unwrap();
    // 批 2：账户 2 计划（target 200；子单未成交后同计划后续操作=观察者）。
    let second = plan_request_for(&mut session, AccountId(2), 0, 200);
    let market = session.build_market_view();
    let mut roots = crate::session::plan_chain_candidates::PlanChainOperationBatch::accounts(
        vec![AccountId(2)],
        market,
        Default::default(),
        Default::default(),
        crate::calendar::CivilInstant::new(
            crate::calendar::CivilDate::from_iso("2030-01-02").unwrap(),
            0,
        )
        .unwrap(),
        Default::default(),
    );
    roots.push_execution(request);
    roots.push_execution(second);
    let mut chain =
        AdaptivePlanChainCoordinator::capture_batch(&session, roots).expect("增量链捕获不得失败");
    // Sol 终审·冗余收口后测试对齐生产形状：账户 root 在 capture 期排空后再执行
    //（yield 不再懒计算 root）。drain 在 ledger 密封前执行——root 计算只用
    // 冻结观察基，不消费 ledger。
    chain
        .drain_ready_accounts_at_capture(&mut session)
        .expect("capture 期 root 排空不得失败");
    session.state.envelope_ledger = EnvelopeLedger::new(
        session.state.next_receipt_base,
        session.project_live_envelopes().unwrap(),
    )
    .unwrap();
    let validator = AccountValidatorDriver::new(
        DecisionResourceSnapshot::seal(&session).unwrap(),
        session.state.envelope_ledger.clone(),
        session.state.next_order_id,
        session.state.setup.config.clone(),
        build_account_validation_context(&session).unwrap(),
    )
    .unwrap();
    RouteRig {
        session,
        chain,
        validator,
        receipts: Default::default(),
        all_candidates: Vec::new(),
    }
}

impl RouteRig {
    fn stream(&mut self) -> ReadyStockStream<'_> {
        ReadyStockStream::new(
            &mut self.chain,
            &mut self.receipts,
            &mut self.session,
            &mut self.validator,
            &mut self.all_candidates,
        )
    }
}

/// 预建真实 shards（不持 session 借用——流实例贯穿期间可继续构轮次）。
fn prepared_shards(
    session: &GameSession,
) -> std::collections::BTreeMap<
    StockId,
    super::continuous_matching::IncrementalContinuousStockCoordinator,
> {
    use super::continuous_matching_adapter::{
        market_data_priors, prepare_incremental_continuous_inputs_on,
    };
    let view = session.read_view();
    let inputs = prepare_incremental_continuous_inputs_on(&view).unwrap();
    let priors = market_data_priors(&view);
    drop(view);
    super::stock_stream::continuous_shards(inputs, priors).unwrap()
}

/// 真实同股轮次：从预建 shards 取本股 apply 后**归位**（书本连续推进——
/// 下一轮在同一 shard 上继续，对齐 drive 往返形态）。
fn real_round(
    shards: &mut std::collections::BTreeMap<
        StockId,
        super::continuous_matching::IncrementalContinuousStockCoordinator,
    >,
    code: &StockId,
    operations: Vec<super::ValidatedOperation>,
) -> ContinuousExecutionRound {
    let mut shard = shards.remove(code).unwrap();
    let round = shard.apply_round(operations).unwrap();
    shards.insert(code.clone(), shard);
    round
}

#[test]
fn cross_batch_route_stays_blocked_until_last_batch_feedback() {
    let mut rig = route_rig();
    let code = StockId(0);
    // 真实 chain 首 yield：两路由（owner=1/2 同股）候选同时给出——人工
    // 拆分后两次 initial=流级组合验证（非生产 drive 分批来源见证，收窄口径）。
    let yielded = PlanContinuationDomain::scoped(&mut rig.session, |domain| {
        rig.chain.next_ready_batch(domain)
    })
    .unwrap();
    assert_eq!(yielded.len(), 2, "首 yield 给出两条路由候选");
    let mut by_owner: std::collections::BTreeMap<AccountId, Vec<IntentCandidate>> =
        Default::default();
    for candidate in yielded {
        by_owner
            .entry(candidate.owner())
            .or_default()
            .push(candidate);
    }
    let first = by_owner
        .remove(&AccountId(1))
        .expect("账户 1 计划候选必须在批");
    let second = by_owner
        .remove(&AccountId(2))
        .expect("账户 2 计划候选必须在批");
    // 预建 shards（轮次构建不借 session，流实例得以贯穿全程）。
    let mut shards = prepared_shards(&rig.session);
    // 生产形态：单个 ReadyStockStream 实例贯穿受理与回传（路由表在流实例内）。
    let mut stream = rig.stream();
    let (ops1, expected1) = stream.initial(first).unwrap();
    assert_eq!(ops1.len(), 1, "批 1 受理恰一个操作");
    let (ops2, expected2) = stream.initial(second).unwrap();
    assert_eq!(ops2.len(), 1, "批 2（同股跨 owner）受理恰一个操作");

    // 前批回传：真实轮次+真实 matcher+真实 continuous_progress。
    let mut round1 = real_round(&mut shards, &code, ops1);
    let matched1: Vec<CandidateValidationOutcome> =
        super::market_tick_transaction::match_continuous_feedback(&code, &mut round1, expected1)
            .unwrap();
    let feed1 = stream
        .continuous_progress(StockStreamProgress::StockCompleted {
            code: &code,
            round: &mut round1,
            outcomes: &matched1,
        })
        .unwrap();
    match feed1 {
        FeedOutcome::Operations { operations, .. } => {
            let owner2_ops = operations
                .iter()
                .filter(|op| super::stock_stream::operation_owner(op) == AccountId(2))
                .collect::<Vec<_>>();
            assert!(
                owner2_ops.is_empty(),
                "前批回传后 (2,stock) 路由仍扣留：账户 2 计划后续操作不得 yield（Sol 二轮观察面；实际 {:?} 个操作）",
                owner2_ops.len()
            );
        }
        FeedOutcome::Complete => {
            panic!("前批回传应返回 Operations")
        }
    }

    // 尾批回传：同路由计数清零，plan2 候选才放行。
    let mut round2 = real_round(&mut shards, &code, ops2);
    let matched2 =
        super::market_tick_transaction::match_continuous_feedback(&code, &mut round2, expected2)
            .unwrap();
    let feed2 = stream
        .continuous_progress(StockStreamProgress::StockCompleted {
            code: &code,
            round: &mut round2,
            outcomes: &matched2,
        })
        .unwrap();
    match feed2 {
        FeedOutcome::Operations { operations, .. } => {
            // Sol 二轮·操作身份断言（非仅变体形态）：尾批回传后账户 2 计划
            // 的后续操作实际放行——按 owner+code+计划键身份核验。
            let continuation = operations
                .iter()
                .find(|op| {
                    super::stock_stream::operation_owner(op) == AccountId(2)
                        && *super::stock_stream::operation_code(op) == code
                })
                .expect("尾批回传后账户 2 计划后续操作必须实际放行");
            assert!(
                matches!(
                    continuation.candidate_key(),
                    IntentCandidateKey::PlanChain { .. }
                ),
                "后续操作身份=计划链候选（实际 {:?}）",
                continuation.candidate_key()
            );
            assert_ne!(
                continuation.sealed_index(),
                matched2.iter().map(|o| o.sealed_index()).max().unwrap(),
                "后续操作是新身份（sealed_index 前进）"
            );
        }
        FeedOutcome::Complete => {
            panic!("尾批回传应返回 Operations")
        }
    }
    // 尾批回传干净完成即契约达成（不追加深层 Idle——阻塞 root 等待需要
    // 真实 drive 供 payload，超出本用例的流级驱动边界）。
}

/// 钉住 yield 形态事实（负结果归因背景）：同股两路由（不同 owner）候选
/// 在首 yield 同时给出——路由扣留按 (owner,stock) 精确键判。
#[test]
fn same_stock_two_routes_yield_together_at_first_yield() {
    let mut rig = route_rig();
    let yielded = PlanContinuationDomain::scoped(&mut rig.session, |domain| {
        rig.chain.next_ready_batch(domain)
    })
    .unwrap();
    assert_eq!(yielded.len(), 2, "两条路由候选同时 yield");
    let owners = yielded
        .iter()
        .map(|candidate| candidate.owner())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(owners, [AccountId(1), AccountId(2)].into());
}
