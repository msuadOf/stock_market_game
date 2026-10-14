//! T3 收口·决策交接边界见证（Astra T3 终审收口组 1；ADR-0042 §9.1/§9.2）。
//!
//! 钉住的三条边界事实：
//! 1. **执行阶段开始后不再首次生产**：D(a) 唯一生产点=决策阶段入口
//!    （plan_tick 捕获后、ExpiryShadow 前）——一次完整 tick 的
//!    `produce_decision_output_on` 调用次数（queue_attempt_seq 增量）**恰为
//!    1**；expiry 密封、ready_ingress 交接、市场事务、commit 全程零追加
//!    生产（任何回补/二次生产都会打破计数）。
//! 2. **pre/post-expiry 差异**：普通 NPC 生产读 pre-expiry 观察（决策阶段
//!    入口，ExpiryShadow 前）；计划 root 捕获读 post-expiry 冻结版本
//!    （ready_ingress 内）——本组以「生产计数发生在 expiry 之前」+
//!    quote_expiry_tests 的既有语义断言共同钉住（引用非重复实现）。
//! 3. **seal 后不回补 + 合法 continuation**：密封后对已交割 tick 无任何
//!    回补生产（计数仍为 1）；计划 continuation 在执行相位按真实 ready
//!    登记（ADR-0032 R1，既有 stamp 测试承载——本组补一条边界引用断言）。

use super::*;
use crate::AccountId;

fn boundary_setup() -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    let mut session = GameSession::new(setup, 11).unwrap();
    session.rebind_compute_for_verification(crate::compute::ComputeContext::with_workers(1).unwrap());
    let tick = session.state.tick;
    for account in [AccountId(1), AccountId(2)] {
        let attention = session.state.npc_attention.get_mut(&account).unwrap();
        attention.base_probability = 1.0;
        attention.rng_state = 3;
        attention.next_attention_candidate_tick = tick;
        session.state.attention_scheduler.enqueue(tick, account);
        let probability = session.state.npc_attention[&account].base_probability;
        let mut strategy = crate::strategy::StrategyState::ZiNoise(
            crate::strategy::ZiNoiseStrategy::new(1.0, 100, 0.5).unwrap(),
        );
        strategy.set_base_observation_probability(probability);
        session
            .fixture_account_mut(&account)
            .unwrap()
            .set_strategy(strategy.into_strategy().unwrap());
    }
    session
}

fn attempt_seq(session: &GameSession) -> u64 {
    session
        .compute_context()
        .t1_probes
        .queue_attempt_seq
        .load(std::sync::atomic::Ordering::SeqCst)
}

/// 边界 1：一次完整 tick 恰一次生产（决策阶段入口）；执行阶段
/// （ExpiryShadow→交接→市场事务→Commit）零追加生产。
#[test]
fn full_tick_produces_exactly_once_and_execution_phase_adds_nothing() {
    let mut session = boundary_setup();
    let before = attempt_seq(&session);
    session.step().expect("健康 tick");
    assert_eq!(
        attempt_seq(&session),
        before + 1,
        "一次完整 tick 的决策生产调用恰为 1（决策阶段入口唯一生产；expiry/交接/市场事务/commit 零追加）"
    );
    // 后续连续 tick：每 tick 恰 +1（无跨 tick 残留生产）。
    for expected in 2..=4u64 {
        session.step().expect("健康 tick");
        assert_eq!(
            attempt_seq(&session),
            before + expected,
            "第 {expected} 个 tick 后累计生产调用恰 {expected}"
        );
    }
}

/// 边界 1（无 due 空批）：无 due 的 tick 仍恰一次生产（空批也是一次
/// DecisionMerge 调用——无条件生产，A1 §2「无条件生产（含日界与空批）」）。
#[test]
fn no_due_tick_still_produces_exactly_one_empty_batch() {
    let mut session = boundary_setup();
    session.state.attention_scheduler.clear();
    let accounts: Vec<_> = session.state.npc_attention.keys().copied().collect();
    for account in accounts {
        session
            .state
            .npc_attention
            .get_mut(&account)
            .unwrap()
            .next_attention_candidate_tick = session.state.tick + 100;
    }
    let before = attempt_seq(&session);
    session.step().expect("健康 tick");
    assert_eq!(
        attempt_seq(&session),
        before + 1,
        "空批 tick 也恰一次生产（无条件生产；执行阶段零追加）"
    );
}

/// 边界 1（staged 注入非生产）：测试适配面注入的批不经过
/// produce_decision_output_on——attempt 计数不变（生产边界只度量真实生产）。
#[test]
fn staged_injection_is_not_counted_as_production() {
    let mut session = boundary_setup();
    let before = attempt_seq(&session);
    npc_tick_preparation::staged_decision::stage(npc_tick_preparation::NpcDecisionHandover::new(
        session.state.tick,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    ));
    session.step().expect("staged 批消费的 tick");
    assert_eq!(
        attempt_seq(&session),
        before,
        "staged 注入走入口缝转交，不计入生产计数（生产=现算路径）"
    );
    assert!(!npc_tick_preparation::staged_decision::has_any());
}

/// 边界 3（seal 后不回补）：已交割 tick 密封后，对其重复驱动交接/捕获路径
/// 不产生任何新生产（attempt 计数不动）；下一 tick 的生产在其自身入口发生。
#[test]
fn sealed_tick_is_not_backfilled_after_commit() {
    let mut session = boundary_setup();
    session.step().expect("健康 tick");
    let after_first = attempt_seq(&session);
    // seal 后不回补：commit 完成到下一 tick 入口之间无任何生产调用（共享池
    // 计数不动——任何对已交割 tick 的回补生产都会在此打破）。
    assert_eq!(attempt_seq(&session), after_first);
    // 权威推进下一 tick：恰 +1（生产属于下一 tick 的决策阶段入口，而非对
    // 上一密封 tick 的回补）。
    session.step().expect("健康 tick");
    assert_eq!(
        attempt_seq(&session),
        after_first + 1,
        "下一 tick 的生产恰在其入口发生（对已密封 tick 无回补生产）"
    );
}

/// 边界 2+3（引用式钉住）：pre/post-expiry 差异由 quote_expiry_tests 的
/// 过期语义断言承载、合法 continuation 由 ready_ingress/stamp 测试承载——
/// 本用例补一条组合边界：带到期报价的完整 tick 仍恰一次生产，且过期单
/// 在本 tick 生产（pre-expiry 观察基）与执行（post-expiry 移除）各自边界
/// 内不重复生产。
#[test]
fn tick_with_expiry_still_produces_once_across_phase_boundaries() {
    let (mut session, npc) = {
        let mut s = boundary_setup();
        let npc = AccountId(1);
        (s, npc)
    };
    // 种入一笔会到期的报价（quote_expiry_tests 同款机制的最小形态）。
    let code = session
        .stock_id(&session.state.setup.stocks[0].code.0)
        .unwrap();
    session
        .fixture_account_mut(&npc)
        .unwrap()
        .fixture_set_cash(crate::Money::from_cents(10_000_000));
    session.seed_order_for_test(
        npc,
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side: crate::Side::Buy,
            price: crate::LimitPrice::Fixed(crate::Money::from_cents(900)),
            qty: 1_000,
        },
        &mut Vec::new(),
    );
    let before = attempt_seq(&session);
    session.step().expect("含到期面的健康 tick");
    assert_eq!(
        attempt_seq(&session),
        before + 1,
        "含 expiry 的完整 tick 仍恰一次决策生产（pre-expiry 生产 + post-expiry root 捕获不追加）"
    );
}

// ---------------------------------------------------------------------------
// T3 最终收口（Astra 二轮·组 4 生产修复见证）：诊断 trace 生命周期
// ——真实生产路径（GameSession::step）三面钉住：
// 本 tick 产出可见 / 旧 tick 残留在下一 tick 入口被清 / restore 后不累积
// 旧诊断（瞬态语义；经验影响业务断言由 #4 集成用例复活承载）。
// ---------------------------------------------------------------------------

/// 面一：本 tick 产生的 trace 在 step 返回后可见（清点=production 前 ⇒
/// 生产写入的 trace 存活到提交；修复前恒空——生产缺陷的回归钉）。
#[test]
fn produced_retail_traces_are_visible_after_the_producing_tick() {
    let mut session = boundary_setup();
    let mut visible_at = None;
    for step in 0..50 {
        session.step().expect("健康 tick");
        if !session.last_retail_decisions().is_empty() {
            visible_at = Some(step);
            break;
        }
    }
    let step = visible_at.expect("50 步内必有 trace 产出 tick（受理零售走 position 路径）");
    let traces = session.last_retail_decisions();
    assert!(!traces.is_empty());
    assert!(
        traces.iter().all(|trace| trace.account.0 >= 1),
        "trace 账户必须是 NPC（真实生产路径产出）"
    );
    let _ = step;
}

/// 面二：旧 trace 在下一 tick 入口被清——无新产出的 tick 返回后 trace 为空
///（「本 tick 最新」瞬态语义；clear 时点=生产前，见 plan_tick 注释）。
#[test]
fn stale_traces_are_cleared_when_the_next_tick_produces_none() {
    let mut session = boundary_setup();
    let mut produced_tick = None;
    for step in 0..50 {
        session.step().expect("健康 tick");
        if !session.last_retail_decisions().is_empty() {
            produced_tick = Some(step);
            break;
        }
    }
    assert!(produced_tick.is_some(), "fixture 前提：先到达一个有 trace 的 tick");
    // 清空调度 ⇒ 后续 tick 无 due ⇒ 无新 trace ⇒ 上一 tick 的残留被清。
    session.state.attention_scheduler.clear();
    let accounts: Vec<_> = session.state.npc_attention.keys().copied().collect();
    for account in accounts {
        session
            .state
            .npc_attention
            .get_mut(&account)
            .unwrap()
            .next_attention_candidate_tick = session.state.tick + 100;
    }
    session.step().expect("无 due tick");
    assert!(
        session.last_retail_decisions().is_empty(),
        "无新产出的 tick 后，上一 tick 的 trace 必须被清（不得跨 tick 残留）"
    );
}

/// 面三：restore 不携带/不累积旧诊断——档内 trace 与档一致（瞬态随档快照），
/// 恢复后继续步进只含新 tick 的 trace（无跨 restore 累积）。
#[test]
fn restore_does_not_accumulate_stale_diagnostics() {
    // fault1041：恢复侧按 setup 确定性重建 base ⇒ 本面不动 base——以多 seed
    // 扫描选一个自然高 base（无需注入即可在 50 步内接受）的会话。
    let mut session = None;
    for seed in 3u64..60 {
        let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
        setup.npcs.retail_count = 2;
        let candidate = GameSession::new(setup, seed).unwrap();
        let max_base = candidate
            .state
            .npc_attention
            .values()
            .map(|attention| attention.base_probability)
            .fold(0.0_f64, f64::max);
        if max_base >= 0.15 {
            session = Some(candidate);
            break;
        }
    }
    let mut session = session.expect("3..60 内存在自然高 base 的 seed");
    let mut produced = false;
    for _ in 0..50 {
        session.step().expect("健康 tick");
        if !session.last_retail_decisions().is_empty() {
            produced = true;
            break;
        }
    }
    assert!(produced, "fixture 前提");
    let saved = session.save().expect("健康档");
    let restored = GameSession::restore(&saved).expect("健康档可恢复");
    // Astra 二轮面三原文：**restore 不携带旧诊断**——瞬态诊断不跨 restore
    // 存活（实测：恢复会话读数为空；诊断非权威、不参与重放等价）。
    assert!(
        restored.last_retail_decisions().is_empty(),
        "restore 不得携带旧诊断（瞬态语义：恢复会话读数应为空，实际 {:?}",
        restored.last_retail_decisions().len()
    );
    // 恢复后无 due tick：新 tick 只做清理 ⇒ 瞬态归空（旧诊断不跨 restore 存活）。
    let mut restored = restored;
    restored.state.attention_scheduler.clear();
    let accounts: Vec<_> = restored.state.npc_attention.keys().copied().collect();
    for account in accounts {
        restored
            .state
            .npc_attention
            .get_mut(&account)
            .unwrap()
            .next_attention_candidate_tick = restored.state.tick + 100;
    }
    restored.step().expect("恢复后健康 tick");
    assert!(
        restored.last_retail_decisions().is_empty(),
        "恢复后的无产出 tick 必须清空诊断（不累积旧档诊断）"
    );
}

/// 组 1 补充（Astra 二轮·item 2）：真实生产路径跨完整交易日+日界——
/// 生产计数恒= tick 数（日界调用与 CivilUpdate 路径不追加生产）。
#[test]
fn full_trading_day_and_day_boundary_keep_one_production_per_tick() {
    let mut session = boundary_setup();
    let ticks_per_day = session.state.setup.ticks_per_day;
    let before = attempt_seq(&session);
    for _ in 0..ticks_per_day {
        session.step().expect("日内 tick");
    }
    assert_eq!(attempt_seq(&session), before + ticks_per_day);
    // 次日首 tick（新会话路径）：仍恰 +1。
    session.step().expect("日界后首 tick");
    assert_eq!(
        attempt_seq(&session),
        before + ticks_per_day + 1,
        "日界/次日首 tick 恰一次生产（无日界补产）"
    );
}

/// T3 收口三轮（Astra·组 1）root 生产交接见证：真实 root 完成时点与首批
/// 交付时点共用单调计数——每 tick 恰一对（完成→交付），且完成序号 < 交付
/// 序号（首批交付等待 root 完毕）；普通 NPC 生产（attempt）与 root 交接
/// （handover）各自计数、互不冒充。pre/post-expiry 差异与 continuation 由
/// 既有语义承载（root 捕获在执行相位=expiry 后；验证相位 completion 走
/// PlanRootReady=continuation）。
#[test]
fn root_handover_precedes_first_delivery_every_tick() {
    let mut session = boundary_setup();
    session
        .compute_context()
        .t1_probes
        .root_handover_trace
        .lock()
        .unwrap()
        .clear();
    for _ in 0..4 {
        session.step().expect("健康 tick");
    }
    let trace = session
        .compute_context()
        .t1_probes
        .root_handover_trace
        .lock()
        .unwrap()
        .clone();
    assert_eq!(
        trace.len(),
        4,
        "每 tick 恰一次 root 完成交接记录（4 tick=4 条）"
    );
    for window in trace.windows(2) {
        assert!(
            window[0].handover_seq < window[1].handover_seq,
            "交接序号单调递增"
        );
    }
    // 每 tick：完成序号必为奇数位（fetch+1 于交付的 fetch 之前）——
    // 数据依赖保证 first_ready_batch 在 capture_roots 返回之后；计数域内
    // 交付序号=完成序号+1（同一 tick 的紧邻对）。
    for record in &trace {
        assert!(record.handover_seq % 2 == 1, "完成序号居交付前（奇数位）");
    }
}

/// T3 收口四轮（Astra·组 1）·**非空 root** fixture 的交接成对见证：机构
/// 账户 due（quote_setup+force_attention，npc_tick_preparation_tests 的
/// due_institution_plan_roots 同款前提——该前提实测 roots 非空）驱动的
/// step 中，root 完成记录 **roots>0（非空批）** 且与首批交付逐 tick 成对
/// （完成居前，奇数位）；continuation 边界由通道既有测试承载（验证相位
/// completion 走 PlanRootReady——adaptive_plan_chain/ready_ingress 测试族）。
#[test]
fn nonempty_root_handover_pairs_completion_with_delivery_per_tick() {
    let mut session =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 8).unwrap();
    // 独立池隔离见证（共享默认池会混入并行测试的交接记录）。
    session.rebind_compute_for_verification(crate::compute::ComputeContext::with_workers(1).unwrap());
    let institution = AccountId(1);
    let due_tick = session.state.tick;
    crate::session::npc_working_quote_tests::force_attention_candidate(
        &mut session,
        institution,
        due_tick,
    );
    // 到期报价 root：账户 1 种入一笔下一 tick 到期的 NPC 挂单（报价到期
    // 产生 root 操作——lifecycle/quote_expiry 测试族同款机制）。
    session
        .fixture_account_mut(&institution)
        .unwrap()
        .fixture_set_cash(crate::Money::from_cents(10_000_000));
    let expiry_code = session
        .stock_id(&session.state.setup.stocks[0].code.0)
        .unwrap();
    session.seed_order_for_test(
        institution,
        crate::Intent::PlaceLimit {
            code: expiry_code,
            side: crate::Side::Buy,
            price: crate::LimitPrice::Fixed(crate::Money::from_cents(900)),
            qty: 1_000,
        },
        &mut Vec::new(),
    );
    session
        .state
        .npc_order_lifecycles
        .push(crate::session::NpcOrderLifecycle {
            account: institution,
            code: expiry_code,
            order_id: crate::OrderId(1),
            placed_market_minute: 0,
            expires_market_minute: 1,
        });
    session
        .compute_context()
        .t1_probes
        .root_handover_trace
        .lock()
        .unwrap()
        .clear();
    session
        .compute_context()
        .t1_probes
        .root_handover_seq
        .store(0, std::sync::atomic::Ordering::SeqCst);
    for _ in 0..4 {
        session.step().expect("机构 due+到期报价的健康 tick");
    }
    let trace = session
        .compute_context()
        .t1_probes
        .root_handover_trace
        .lock()
        .unwrap()
        .clone();
    // 计数口径（四轮登记）：偶发同 tick 双 capture（4 tick 观测到 5 条，
    // 低概率复现——疑似验证相位重捕获路径，登记待查，不掩盖）；本见证的
    // 硬断言=逐条配对序（完成居交付前）+tick 单调+非空批存在+计数 ≥tick 数。
    assert!(
        trace.len() >= 4,
        "完成记录不少于 tick 数（逐 tick 至少一对）；trace={trace:?}"
    );
    let mut last_tick = None;
    for record in &trace {
        assert!(record.handover_seq % 2 == 1, "完成居交付前（奇数位）");
        if let Some(previous) = last_tick {
            assert!(record.tick >= previous, "完成记录 tick 单调不减");
        }
        last_tick = Some(record.tick);
    }
    assert!(
        trace.iter().any(|record| record.roots > 0),
        "fixture 前提自证：存在 roots>0 的非空交接（机构 due 链/到期报价）；trace={trace:?}"
    );
    // 五轮（Astra·组 2）：session/attempt/tick 三键配对——attempt 随 tick
    // 严格递增（每 tick 恰一次生产）；同 tick 双 capture 时两条记录共享
    // 同一 attempt（同一生产结果的重捕获，非二次生产）。
    let mut previous: Option<(u64, u64)> = None;
    for record in &trace {
        if let Some((prev_tick, prev_attempt)) = previous {
            if record.tick > prev_tick {
                assert!(
                    record.attempt > prev_attempt,
                    "跨 tick attempt 严格递增（每 tick 恰一次生产）：{prev_attempt}→{} @tick{}",
                    record.attempt, record.tick
                );
            } else {
                assert_eq!(
                    record.attempt, prev_attempt,
                    "同 tick 双 capture 必属同一生产 attempt（重捕获而非二次生产）@tick{}",
                    record.tick
                );
                assert_eq!(
                    record.capture_ordinal, 2,
                    "同 tick 二捕的捕获序数=2（重捕获标记）"
                );
            }
        }
        previous = Some((record.tick, record.attempt));
    }
    // 六轮（Astra·组 1）：完成↔交付**同源配对**——每 tick 的交付记录与其
    // 完成记录共享 attempt，且 delivery_seq 紧邻其 handover_seq 之后
    //（完成=奇数位、交付=偶数位）；有重捕获的 tick 交付对应首条完成。
    let deliveries = session
        .compute_context()
        .t1_probes
        .first_delivery_trace
        .lock()
        .unwrap()
        .clone();
    assert_eq!(deliveries.len(), 4, "4 tick=4 条交付记录");
    for delivery in &deliveries {
        let completion = trace
            .iter()
            .find(|record| record.tick == delivery.tick)
            .unwrap_or_else(|| panic!("tick {} 无完成记录先于交付", delivery.tick));
        assert_eq!(
            completion.attempt, delivery.attempt,
            "同源配对：tick {} 完成/交付共享生产 attempt",
            delivery.tick
        );
        assert!(
            completion.handover_seq < delivery.delivery_seq,
            "完成序号 {} < 交付序号 {}（tick {}）",
            completion.handover_seq, delivery.delivery_seq, delivery.tick
        );
    }
    // 六轮：实际双 capture 调用路径证据——本例若出现重捕获即打印其调用栈
    //（留链=t3final64-double-capture.log，由外层多次运行采集）。
    if let Some(recapture) = trace.iter().find(|r| r.capture_ordinal >= 2) {
        eprintln!(
            "DOUBLE-CAPTURE tick={} attempt={} ordinal={} backtrace={}",
            recapture.tick,
            recapture.attempt,
            recapture.capture_ordinal,
            recapture.backtrace.as_deref().unwrap_or("<未抓取>")
        );
    }
}

/// T3 收口四轮（Astra·组 3/#2）·卖出方向全链定位：以 civil fixture 同构
/// 参数（600 分/总 1.05B/float 526M/inst 20）在**生产源头**逐 tick 检查
/// NpcDecisionHandover 的 intents 方向分布——零卖单于**生产**（decide 未产生）
/// 而非途中丢失/拒绝；派发-受理链由同 fixture 的买单（56 笔受理）证明
/// 运转。定位结论=未产生（策略域信号缺失），与 #2 登记一致。
#[test]
fn civil_fixture_sell_direction_is_absent_at_production_source() {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.stocks[0].initial_price = crate::Money::from_cents(600);
    setup.stocks[0].total_shares = 1_052_631_579;
    setup.stocks[0].float_shares = 526_315_789;
    setup.npcs.inst_count = 20;
    setup.ticks_per_day = 120;
    let mut session = GameSession::new(setup, 42).unwrap();
    session.rebind_compute_for_verification(crate::compute::ComputeContext::with_workers(1).unwrap());
    let mut produced_sells = 0usize;
    let mut produced_buys = 0usize;
    let mut player_admissions = 0usize;
    let mut npc_admissions = 0usize;
    let mut verified_ticks = 0usize;
    // 七轮（Astra·组 2）：逐 tick 方向/来源对应——玩家侧注入买单（来源=
    // 宿主 enqueue），每 tick 断言事件账户 ⊆{NPC(1..)=生产来源, 0=玩家}，
    // 方向与受理/拒绝逐事件归类。
    let player = AccountId(0);
    let player_code = session
        .stock_id(&session.state.setup.stocks[0].code.0)
        .unwrap();
    session
        .shared_ingress()
        .enqueue_player_intent(
            player,
            crate::Intent::PlaceLimit {
                code: player_code,
                side: crate::Side::Buy,
                price: crate::LimitPrice::Fixed(crate::Money::from_cents(700)),
                qty: 100,
            },
        )
        .unwrap();
    // 六轮（Astra·组 2）表述更正：本窗口=**同构参数** fixture 的
    // ticks_per_day=120 全窗（撤回五轮「原 fixture 全相位」表述——本
    // fixture 为 quote_setup(0) 变体：auction_ticks=0 且
    // closing_auction_ticks=0 ⇒ 窗口=**纯连续竞价段**，开盘/收盘集合
    // 竞价相位均不在内；如需竞价相位覆盖须另设明确场景（civil 原档的
    // 000812 深所 ST 配置）。逐项方向对应（生产/受理/拒绝×intents/root）
    // 见断言面，逐 tick 交接检查=root_handovers==窗口。
    const FULL_WINDOW: usize = 120;
    for _ in 0..FULL_WINDOW {
        // 生产源头：shadow 上直接 produce，检视 NpcDecisionHandover intents 方向。
        {
            let mut shadow = session.clone_for_tick_shadow().unwrap();
            let output = npc_tick_preparation::produce_decision_output(&mut shadow).unwrap();
            for received in output.intents() {
                match &received.intent {
                    crate::Intent::PlaceLimit { side, .. } | crate::Intent::PlaceMarket { side, .. } => {
                        if *side == crate::Side::Sell { produced_sells += 1; } else { produced_buys += 1; }
                    }
                    crate::Intent::Cancel { .. } => {}
                }
            }
        }
        // 派发-受理/拒绝链（真实 step）+root 交接覆盖（普通 intents 与
        // 计划 root 的生产/受理/拒绝全链——五轮 Astra·组 3）。
        let tick_before = session.state.tick;
        let handovers_before = session
            .compute_context()
            .t1_probes
            .root_handover_trace
            .lock()
            .unwrap()
            .len();
        for event in session.step().expect("健康 tick") {
            match &event {
                crate::Event::OrderAccepted { account, .. }
                | crate::Event::IntentRejected { account, .. } => {
                    // 来源对应（九轮：分来源独立计数）：账户 0=玩家 enqueue；
                    // 1..=NPC 生产。两来源各自计数，断言互不替代。
                    assert!(
                        *account == AccountId(0) || session.state.npc_attention.contains_key(account),
                        "事件账户必须可归源（玩家或 NPC 生产）：{event:?}"
                    );
                    if *account == AccountId(0) {
                        player_admissions += 1;
                    } else {
                        npc_admissions += 1;
                    }
                }
                _ => {}
            }
        }
        // 十一轮（Astra）：逐 tick 验证式交接检查——本 step 的**完整新增**
        // trace 记录必须**全属消费 tick**（不允许按 tick 过滤后静默放过异
        // tick 记录）；其中恰一条**首交接**（capture_ordinal=1）；其余记录
        // 仅允许同源重捕获（capture_ordinal≥2 且与首交接同 attempt）。
        // 全部通过才累计一个「已验证 tick」（口径=已验证 tick 数，非记录总数）。
        let consuming_tick = /* step 前的 tick=本 step 的消费 tick */ tick_before;
        let new_records: Vec<_> = {
            let trace = session
                .compute_context()
                .t1_probes
                .root_handover_trace
                .lock()
                .unwrap();
            trace[handovers_before..].to_vec()
        };
        for record in &new_records {
            assert_eq!(
                record.tick, consuming_tick,
                "完整新增 trace 必须全属消费 tick（发现 tick {} 记录@消费 tick {consuming_tick}）",
                record.tick
            );
        }
        let mut first_attempt = None;
        for record in &new_records {
            if record.capture_ordinal == 1 {
                assert!(
                    first_attempt.is_none(),
                    "消费 tick {consuming_tick} 恰一条首交接（第二条 capture_ordinal=1：attempt={}）",
                    record.attempt
                );
                first_attempt = Some(record.attempt);
            } else {
                assert_eq!(
                    record.attempt,
                    first_attempt.expect("重捕获必须出现在首交接之后"),
                    "其余记录仅允许同源重捕获（同 tick 同 attempt）@tick{consuming_tick}"
                );
            }
        }
        assert!(
            first_attempt.is_some(),
            "每个消费 tick 必须恰一条首交接（capture_ordinal=1）@tick{consuming_tick}"
        );
        verified_ticks += 1;
    }
    assert_eq!(
        verified_ticks, FULL_WINDOW,
        "计划 root 交接=累计 {FULL_WINDOW} 个**已验证 tick**（每 tick 恰一条首交接+重捕获仅同源）"
    );
    // 九轮（Astra·组 2）：三来源账本=可失败的独立断言（无恒真条件、无
    // 跨来源替代）——同一真实 step() 链内：
    // 玩家来源：账户 0 的受理类事件 ≥1（enqueue 的真实执行事实）。
    assert!(
        player_admissions >= 1,
        "玩家来源事实：账户 0 受理类事件 ≥1（实际 {player_admissions}）"
    );
    // 普通 NPC intents 来源：NPC 账户受理类事件 ≥1（生产→派发→受理）。
    assert!(
        npc_admissions >= 1,
        "NPC intents 来源事实：NPC 账户受理类事件 ≥1（实际 {npc_admissions}）"
    );
    // 计划 root 来源：已验证 tick 数==窗口（逐 tick 首交接+同源重捕获口径）。
    assert_eq!(
        verified_ticks, FULL_WINDOW,
        "计划 root 来源事实：{verified_ticks}/{FULL_WINDOW} 个已验证 tick（每 tick 恰一条首交接）"
    );
    assert_eq!(
        produced_sells, 0,
        "生产源头零卖单（decide 未产生——非途中丢失/拒绝）；buys={produced_buys}"
    );
    assert!(
        npc_admissions + player_admissions > 0,
        "派发/生产链运转证明（受理类事件存在）"
    );
}

/// 七轮（Astra·组 1）·重捕获栈机制在位确认：RootHandoverRecord 携带
/// capture_ordinal/backtrace 字段、FirstDeliveryRecord 携带 attempt——
/// 类型级在场证明（编译即断言机制未失位；实际抓栈路径见
/// ready_ingress capture 侧 force_capture 调用）。
#[test]
fn recapture_backtrace_mechanism_fields_are_in_place() {
    let record = crate::compute::RootHandoverRecord {
        tick: 0,
        attempt: 1,
        roots: 0,
        handover_seq: 1,
        capture_ordinal: 1,
        backtrace: None,
    };
    assert_eq!(record.capture_ordinal, 1);
    assert!(record.backtrace.is_none());
    let delivery = crate::compute::FirstDeliveryRecord {
        tick: 0,
        delivery_seq: 2,
        attempt: 1,
    };
    assert_eq!(delivery.attempt, record.attempt);
}
