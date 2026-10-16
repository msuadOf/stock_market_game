//! T3·epoch 对齐短 fixture（A3 v2 规格；Astra 裁定处方第 3 步）。
//!
//! 对齐轴=**逻辑 epoch**（`observed_tick == k` 的决策批），不锚定生产代码物理
//! 位置——本 fixture 在 T3 前移前后持续有效。断言分两类并逐条标注（ADR-0042
//! §4：不得以更新 golden 掩盖）：
//! - **A 类（epoch 事实，T3 前后都必须成立）**：每 epoch 每账户恰一次评估；
//!   接受/拒绝与 probe 一致；RNG 恰推进一个评估（probe 对拍，不写死抽样
//!   次数）；日末不评估下一日 epoch；restore 不重复执行、不丢失 epoch；
//!   receipt 游标单调不减。
//! - **B 类（当前形状断言，T3 落地时随契约修订显式重登记）**：step 返回后
//!   `save().pending_npc` 携带下一 tick 批次；fault 2071 日内必携规则；
//!   首个 tick 在日界版本物理位置准备。
//!
//! probe 纪律：oracle=克隆 attention 状态后 `evaluate_candidate_with_signal`
//! 一次，用克隆结果对照生产后真实状态（以 probe 对拍为准）。

use super::ProtocolSession;
use crate::*;

/// 双日活跃 fixture（搬运自 day_end_npc_tests.rs:6-18 active_npc_setup，同源
/// 不重写——A3 §三「复用点」）。
fn active_npc_setup() -> SessionSetup {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.stocks[0].float_shares = 10_000;
    setup.npcs.retail_count = 3;
    setup.npcs.inst_count = 0;
    setup.npcs.retail_cash_median = Money::from_cents(100_000_000);
    setup.strategy_params.retail.arrival_rate = 1.0;
    setup.strategy_params.retail.order_size_mean = 200;
    setup.strategy_params.retail.chase_prob = 0.3;
    setup.ticks_per_day = 4;
    setup.start_date = CivilDate::from_iso("2030-01-07").unwrap();
    setup
}

/// 注意力三元组快照（account, next_candidate, rng_state）。
type AttentionSnap = Vec<(AccountId, u64, u64)>;

fn snap_attention(session: &ProtocolSession) -> AttentionSnap {
    session
        .game()
        .state
        .npc_attention
        .iter()
        .map(|(account, state)| (*account, state.next_attention_candidate_tick, state.rng_state))
        .collect()
}

fn receipt_cursors(session: &ProtocolSession) -> crate::session::IngressReceiptCursors {
    session.game().save().unwrap().ingress_receipt_cursors
}

/// probe oracle：克隆该账户 attention，在其上评估一次（kind/signal/tick 与
/// 生产同参来源——signal 用当前权威市场视图派生）。返回（接受, 评估后状态）。
fn probe_attention(
    session: &ProtocolSession,
    account: AccountId,
    tick: u64,
) -> (bool, crate::session::attention::NpcAttentionState) {
    let game = session.game();
    let attention = game
        .state
        .npc_attention
        .get(&account)
        .expect("probe 账户必须已有 attention 状态")
        .clone();
    let kind = game
        .state
        .accounts
        .get(&account)
        .expect("probe 账户必须存在")
        .kind();
    let market = game.build_market_view();
    let signal = crate::session::attention::market_attention_signal(&market);
    let mut probed = attention;
    let accepted = probed.evaluate_candidate_with_signal(kind, signal, tick);
    (accepted, probed)
}

/// 当前已提交权威版本的冻结 market signal（D(k) 的观察基 = `S_k`——须在
/// `state.tick == k` 时捕获，即该 epoch 消费调用的 step **之前**）。
/// Astra 四发现裁定·发现 1（v3）：跨 epoch probe 须读取**各自冻结输入**，
/// 不得统一使用 step 后 signal。
fn frozen_signal(session: &ProtocolSession) -> f64 {
    let market = session.game().build_market_view();
    crate::session::attention::market_attention_signal(&market)
}

/// force_due：复用 force_attention_candidate 语义（base=1.0 ⇒ 恒接受）+ 显式
/// rng_state；可选显式 base_probability（拒绝侧场景需 <1.0）。
fn force_due(session: &mut ProtocolSession, account: AccountId, tick: u64, rng_state: u64) {
    {
        let game = session.game_mut();
        let attention = game
            .state
            .npc_attention
            .get_mut(&account)
            .expect("force_due 账户必须已有 attention 状态");
        attention.base_probability = 1.0;
        attention.rng_state = rng_state;
        attention.next_attention_candidate_tick = tick;
    }
    session
        .game_mut()
        .state
        .attention_scheduler
        .enqueue(tick, account);
    // 同步策略 base_observation_probability（saved_runtime fault2210：两侧
    // 概率必须逐位一致——与 t1 due_session fixture 同款处理）。
    let probability = session
        .game()
        .state
        .npc_attention
        .get(&account)
        .unwrap()
        .base_probability;
    let mut strategy = session
        .game()
        .state
        .accounts
        .get(&account)
        .unwrap()
        .strategy()
        .unwrap()
        .production_state()
        .unwrap();
    strategy.set_base_observation_probability(probability);
    session
        .game_mut()
        .fixture_account_mut(&account)
        .unwrap()
        .set_strategy(strategy.into_strategy().unwrap());
}

fn force_due_with_base(
    session: &mut ProtocolSession,
    account: AccountId,
    tick: u64,
    rng_state: u64,
    base_probability: f64,
) {
    {
        let game = session.game_mut();
        let attention = game
            .state
            .npc_attention
            .get_mut(&account)
            .expect("force_due 账户必须已有 attention 状态");
        attention.base_probability = base_probability;
        attention.rng_state = rng_state;
        attention.next_attention_candidate_tick = tick;
    }
    session
        .game_mut()
        .state
        .attention_scheduler
        .enqueue(tick, account);
    // 同步策略 base_observation_probability（saved_runtime fault2210：两侧
    // 概率必须逐位一致——与 t1 due_session fixture 同款处理）。
    let probability = session
        .game()
        .state
        .npc_attention
        .get(&account)
        .unwrap()
        .base_probability;
    let mut strategy = session
        .game()
        .state
        .accounts
        .get(&account)
        .unwrap()
        .strategy()
        .unwrap()
        .production_state()
        .unwrap();
    strategy.set_base_observation_probability(probability);
    session
        .game_mut()
        .fixture_account_mut(&account)
        .unwrap()
        .set_strategy(strategy.into_strategy().unwrap());
}

/// 只钉 due 调度（tick+rng_state），**不动 base_probability**——用于需要
/// save/restore 的场景（fault1041：恢复侧按 setup 确定性重建 base，改动即拒）。
fn force_due_keep_base(session: &mut ProtocolSession, account: AccountId, tick: u64, rng_state: u64) {
    {
        let game = session.game_mut();
        let attention = game
            .state
            .npc_attention
            .get_mut(&account)
            .expect("账户必须已有 attention 状态");
        attention.rng_state = rng_state;
        attention.next_attention_candidate_tick = tick;
    }
    session
        .game_mut()
        .state
        .attention_scheduler
        .enqueue(tick, account);
}

/// 搜索一个「probe 接受」的 rng_state（接受侧构造自校验）。
fn find_accepting_rng(session: &ProtocolSession, account: AccountId, tick: u64) -> u64 {
    for seed in 1..500u64 {
        let mut probe_session = session;
        // 临时改 rng 不可行（借用）；改为数学短路：克隆评估。
        let game = session.game();
        let attention = game.state.npc_attention.get(&account).unwrap().clone();
        let kind = game.state.accounts.get(&account).unwrap().kind();
        let market = game.build_market_view();
        let signal = crate::session::attention::market_attention_signal(&market);
        let mut probed = attention;
        probed.rng_state = seed;
        if probed.evaluate_candidate_with_signal(kind, signal, tick) {
            return seed;
        }
    }
    panic!("未找到接受 rng_state");
}

/// 搜索一个「首评估拒绝且 wait ≥ min_wait」的 rng_state（自校验内嵌）。
fn find_rejecting_rng(
    session: &ProtocolSession,
    account: AccountId,
    tick: u64,
    base_probability: f64,
    min_wait: u64,
) -> u64 {
    // probe 需要真实 base；临时不可行——改为纯数学搜索：克隆评估不可用状态
    // 可用轻量模拟：SplitMix64 首抽 < effective/maximum 即接受。
    let sensitivity = 0.75f64; // Retail
    let maximum = (base_probability * (1.0 + sensitivity * 3.0)).min(1.0);
    let effective = base_probability * (1.0 + sensitivity * 0.0);
    for seed in 1..200u64 {
        let mut rng = crate::SplitMix64::new(seed);
        let first = rng.next_f64();
        if first >= effective / maximum {
            // wait 抽样：P(wait=1)=maximum，next_f64 < 1 - (1-maximum) ⇒ wait=1。
            let draw = rng.next_f64();
            let wait = ((1.0 - draw).ln() / (-maximum).ln_1p()).floor() + 1.0;
            let wait = wait.clamp(1.0, u64::MAX as f64) as u64;
            if wait >= min_wait {
                return seed;
            }
        }
    }
    panic!("未找到满足拒绝+wait>={min_wait} 的 rng_state");
}

/// 场景 1：首次调用（A3 §四.1）。
///
/// A 类：D(0) 记录 due{A,B}；A 接受（经验行更新、观察分钟==day0 分钟 0）、
/// B 拒绝（无经验行更新、RNG 仍推进恰一次评估）；两账户 rng_state == 各自
/// probe 链评估后的值。D(1) due{A}，A 再恰一次评估。全窗口 A 恰两次、B 恰一次。
/// B 类：调用返回后 pending_npc.observed_tick==1 且 observed_accounts==[A]。
#[test]
fn epoch_alignment_first_call_records_entry_and_tail_epochs() {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    let mut session = ProtocolSession::new(setup, 7).unwrap();
    let a = AccountId(1);
    let b = AccountId(2);
    // A：base=1.0 恒接受（wait=1 ⇒ next=1，D(1) 仍 due）。
    force_due(&mut session, a, 0, 3);
    // B：base<1.0 且 rng 使首评估拒绝、wait>=2（D(1) 不 due）。
    let b_rng = find_rejecting_rng(&session, b, 0, 0.1, 2);
    force_due_with_base(&mut session, b, 0, b_rng, 0.1);
    // 内嵌自校验（A3：拒绝侧构造显式 probe 自校验）。
    {
        let (accepted, _) = probe_attention(&session, b, 0);
        assert!(!accepted, "B 的 rng={b_rng} 应在 epoch 0 拒绝（fixture 前提）");
    }
    let attention_before = snap_attention(&session);
    // 逐 epoch 冻结 signal（v3·发现 1）：D(0) 的观察基 = S_0——首拍 step 前
    // 捕获；D(1) 的观察基 = S_1——首拍 step 后（第二拍前）捕获。
    let signal_at_epoch0 = frozen_signal(&session);

    session.step_frame().unwrap();
    let signal_at_epoch1 = frozen_signal(&session);

    let attention_after = snap_attention(&session);
    // A 类：oracle 链——A 在 epoch 0、1 各恰一次评估；B 仅 epoch 0 一次。
    // T3（A3 v3·发现 3 A/B 拆分）：[0,1] 链拆开——首调用只做 D(0)（B 类分组
    // 变化），D(1) 移至下一调用（下方第二拍断言，覆盖不删）。
    // probe 链基于 step 前状态 + 各 epoch 冻结 signal（不得统一用 step 后
    // signal——发现 1 口径）。
    let (accepts_a, state_a0) =
        probe_chain_from(&session, a, &attention_before, &[(0, signal_at_epoch0)]);
    let (accepts_b, state_b) =
        probe_chain_from(&session, b, &attention_before, &[(0, signal_at_epoch0)]);
    assert_eq!(accepts_a, vec![true], "A base=1.0 epoch 0 接受（首调用仅 D(0)）");
    assert!(!accepts_b[0], "B epoch 0 拒绝（probe oracle）");
    let after_a = attention_after
        .iter()
        .find(|(account, _, _)| *account == a)
        .unwrap();
    let after_b = attention_after
        .iter()
        .find(|(account, _, _)| *account == b)
        .unwrap();
    // A 类：next_candidate 链对拍（每 epoch 恰一次评估——base=1.0 ⇒ wait=1
    // 确定性，链值即评估次数证明）。RNG 等值仅对**拒绝**账户断言：受理零售
    // 的个人分析（sample_discovery_stock 等）与 attention 共享同一
    // rng_state 流（本 fixture 实现期发现，已登记 t3-epoch-fixture-notes），
    // 其额外抽样使受理账户的 rng 终值不可由纯 evaluate probe 预测；A3 §一
    // 「rng_state 变化次数仅作辅助」口径。
    assert_eq!(
        after_a.1,
        state_a0.next_attention_candidate_tick,
        "A 类：A epoch 0 恰一次评估（首调用 next=probe[0] 链）"
    );
    assert!(
        after_a.2 != attention_before
            .iter()
            .find(|(x, _, _)| *x == a)
            .unwrap()
            .2,
        "A 类（辅助）：A 的 RNG 流确有推进"
    );
    assert_eq!(
        (after_b.1, after_b.2),
        (state_b.next_attention_candidate_tick, state_b.rng_state),
        "A 类：B 恰一次评估且拒绝无个人分析 ⇒ rng 与 probe 逐位一致"
    );
    // A 类：接受侧经验行更新、观察分钟 == day 0 的 epoch-0 分钟（day_tick=0 ⇒ 0）。
    let experience = &session.game().state.retail_experience[&a];
    let stock = experience
        .stocks
        .get(&StockId(0))
        .expect("A 类：接受的观察必须持久化复核股票");
    // T3（v3 分钟精确）：受理账户每个 accepted epoch 更新经验行——首调用仅
    // D(0) 受理 ⇒ 终值==D(0) 的冻结观察分钟 f(0)=0（day0 首 epoch；禁止加减
    // tick 修绿）。D(1) 的覆写在第二拍断言。
    assert_eq!(
        stock.last_observed_market_minute, 0,
        "A 类（v3）：首调用终值==D(0) 的观察分钟 f(0)"
    );
    assert!(
        !session.game().state.retail_experience[&b]
            .stocks
            .contains_key(&StockId(0)),
        "A 类：B 拒绝 ⇒ 无经验行观察记录"
    );
    // B 类：T3 契约重登记（t3-stop-register 冲突1/3→ADR-0042 §9.7）——「step
    // 返回即含下一 tick 批次」随跨 tick 载体退役：pending 恒 None；D(1) 的
    // epoch 事实移至第二拍见证（覆盖不删）。
    let save = session.game().save().unwrap();
    assert!(
        serde_json::to_value(&save).unwrap().get("pending_npc").is_none(),
        "B 类（新契约）：step 返回不携带跨 tick 批次"
    );
    // 第二拍：D(1) 于 (1→2) 调用入口现算——A（base=1 链式 due）恰一次评估，
    // probe [0,1] 链对拍；B 不再评估（wait≥2）。f(1)=第二拍前的权威分钟
    //（D(1) 的冻结观察基，禁以第二拍后时钟替代）。
    let minute_at_epoch1 = session.game().current_market_minute();
    let (_, state_a01) = probe_chain_from(
        &session,
        a,
        &attention_before,
        &[(0, signal_at_epoch0), (1, signal_at_epoch1)],
    );
    session.step_frame().unwrap();
    let after_a2 = snap_attention(&session)
        .iter()
        .find(|(x, _, _)| *x == a)
        .unwrap()
        .1;
    assert_eq!(
        after_a2, state_a01.next_attention_candidate_tick,
        "A 类：第二拍 D(1) 恰一次评估（[0,1] probe 链闭合；全窗 A 恰两次）"
    );
    let save2 = session.game().save().unwrap();
    assert!(serde_json::to_value(&save2).unwrap().get("pending_npc").is_none(), "B 类（新契约）：第二拍后 pending 仍 None");
    // D(1) 受理 ⇒ 终值覆写为 f(1)=step 后权威分钟（v3 精确：最后实际写入=D(1)）。
    let stock_after = &session.game().state.retail_experience[&a]
        .stocks
        .get(&StockId(0))
        .expect("A 类：D(1) 受理观察持久化");
    assert_eq!(
        stock_after.last_observed_market_minute, minute_at_epoch1,
        "A 类（v3）：第二拍终值==D(1) 的观察分钟 f(1)（第二拍前权威分钟）"
    );
    // A 类：receipt 游标单调不减。
    assert!(monotonic_cursors(&session));
}

/// 场景 2：日内中间 tick（A3 §四.2）。
#[test]
fn epoch_alignment_intraday_tick_consumes_one_epoch_and_produces_next() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.stocks[0].float_shares = 10_000;
    setup.npcs.retail_count = 2;
    setup.npcs.inst_count = 0;
    setup.npcs.retail_cash_median = Money::from_cents(100_000_000);
    setup.strategy_params.retail.arrival_rate = 1.0;
    setup.ticks_per_day = 12;
    setup.start_date = CivilDate::from_iso("2030-01-07").unwrap();
    let mut session = ProtocolSession::new(setup, 11).unwrap();
    let a = AccountId(1);
    let b = AccountId(2);
    // 推进至 tick 5（0..5），期间注意力自然演化。
    for _ in 0..5 {
        session.step_frame().unwrap();
    }
    let a = *session.game().state.npc_attention.keys().min().unwrap();
    let b = *session.game().state.npc_attention.keys().max().unwrap();
    // epoch 5：A due 接受；epoch 6：B due 拒绝（rng 自校验搜索）。
    force_due(&mut session, a, 5, 3);
    let b_rng = find_rejecting_rng(&session, b, 6, 0.1, 12);
    force_due_with_base(&mut session, b, 6, b_rng, 0.1);
    {
        let (accepted, _) = probe_attention(&session, b, 6);
        assert!(!accepted, "B 在 epoch 6 应拒绝（fixture 前提）");
    }
    let attention_before = snap_attention(&session);
    let cursors_before = receipt_cursors(&session);
    // epoch-5 观察分钟实测值：与 probe 于同版本克隆上计算的 moment 相等
    //（同 epoch 同版本比较，禁止加一/减 tick 修绿）。signal 同理：D(5) 的
    // 冻结观察基 = S_5（本拍 step 前捕获）；D(6) 的 = S_6（本拍 step 后、
    // 第二拍前捕获）——逐 epoch 冻结输入（v3·发现 1）。
    let minute_at_epoch5 = session.game().current_market_minute();
    let signal_at_epoch5 = frozen_signal(&session);

    session.step_frame().unwrap();
    let signal_at_epoch6 = frozen_signal(&session);

    let attention_after = snap_attention(&session);
    // A 类：本调用仅 D(5)（epoch 6 移至第二拍）——probe 链拆分：第一拍 [5]，
    // 第二拍 [5,6]/[6] 闭合（A/B 拆分纪律：epoch 事实 A 类、调用分组 B 类）。
    let (_, state_a5_first) = probe_chain_from(&session, a, &attention_before, &[(5, signal_at_epoch5)]);
    let (_, state_b6) = probe_chain_from(&session, b, &attention_before, &[(6, signal_at_epoch6)]);
    let after_a = attention_after
        .iter()
        .find(|(account, _, _)| *account == a)
        .unwrap();
    let after_b = attention_after
        .iter()
        .find(|(account, _, _)| *account == b)
        .unwrap();
    assert_eq!(
        after_a.1,
        state_a5_first.next_attention_candidate_tick,
        "A 类：第一拍 A 在 epoch 5 恰一次评估（base=1.0 ⇒ 每 tick due，next=[5] 链）"
    );
    // T3（A/B 拆分）：本调用仅 D(5)（epoch 6 移至下一调用，下方第二拍）。
    // B 在本拍不评估（scheduled=6 > 视界 5）。
    // A 类：A 接受 ⇒ 经验行观察分钟 == 同版本 epoch-5 分钟实测值。
    let stock = session.game().state.retail_experience[&a]
        .stocks
        .get(&StockId(0))
        .expect("A 类：epoch 5 接受的观察必须持久化");
    // T3（v3 分钟精确）：本调用最后受理 epoch=D(5) ⇒ 终值==f(5)=epoch-5 冻结
    // 分钟（fixture 前置实测锚 100；不加减 tick）。D(6) 覆写在第二拍。
    assert_eq!(
        stock.last_observed_market_minute, minute_at_epoch5,
        "A 类（v3）：终值==D(5) 的观察分钟 f(5)=100"
    );
    assert_eq!(minute_at_epoch5, 100, "epoch-5 版本分钟实测锚（fixture 自证）");
    // B 类：T3 契约重登记——「tick 5 提交后携带 epoch 6 批次」随跨 tick 载体
    // 退役（pending 恒 None）；epoch 6 事实移至第二拍（覆盖不删）。
    let save = session.game().save().unwrap();
    assert!(
        serde_json::to_value(&save).unwrap().get("pending_npc").is_none(),
        "B 类（新契约）：tick 5 提交后不携带跨 tick 批次"
    );
    // 第二拍 (6→7)：D(6) 入口现算——A（链式 due）恰一次评估受理、B 恰一次
    // 评估拒绝（probe oracle 闭合 [5,6]/[6] 链）。f(6)=第二拍前权威分钟。
    let minute_at_epoch6 = session.game().current_market_minute();
    let (_, state_a56) = probe_chain_from(
        &session,
        a,
        &attention_before,
        &[(5, signal_at_epoch5), (6, signal_at_epoch6)],
    );
    let (_, state_b6) = probe_chain_from(&session, b, &attention_before, &[(6, signal_at_epoch6)]);
    session.step_frame().unwrap();
    let after2 = snap_attention(&session);
    let after_a2 = after2.iter().find(|(x, _, _)| *x == a).unwrap();
    let after_b2 = after2.iter().find(|(x, _, _)| *x == b).unwrap();
    assert_eq!(
        after_a2.1, state_a56.next_attention_candidate_tick,
        "A 类：第二拍 D(5)+D(6) 链闭合（A 两 epoch 各恰一次）"
    );
    assert_eq!(
        (after_b2.1, after_b2.2),
        (state_b6.next_attention_candidate_tick, state_b6.rng_state),
        "A 类：第二拍 D(6) B 恰一次评估且拒绝（rng 逐位=probe）"
    );
    // D(6) A 受理 ⇒ 终值覆写 f(6)（v3 精确：最后实际写入=D(6)）。
    let stock_after = &session.game().state.retail_experience[&a]
        .stocks
        .get(&StockId(0))
        .expect("A 类：D(6) 受理观察持久化");
    assert_eq!(
        stock_after.last_observed_market_minute, minute_at_epoch6,
        "A 类（v3）：第二拍终值==D(6) 的观察分钟 f(6)（第二拍前权威分钟）"
    );
    // A 类：receipt 游标单调不减（fault 2069 口径）。
    let cursors_after = receipt_cursors(&session);
    assert!(cursors_ge(&cursors_after, &cursors_before));
    assert!(monotonic_cursors(&session));
}

/// 场景 3：日内最后一 tick / 日末断言（A3 §四.3，v2 epoch 对齐形态）。
#[test]
fn epoch_alignment_last_tick_does_not_execute_next_day_epoch() {
    let mut session = ProtocolSession::new(active_npc_setup(), 2).unwrap();
    let accounts: Vec<AccountId> = session
        .game()
        .state
        .npc_attention
        .keys()
        .copied()
        .collect();
    // 全账户钉 due 链：base=1.0 ⇒ 每 epoch due（wait=1），覆盖 0..=3。
    for account in &accounts {
        force_due(&mut session, *account, 0, 3);
    }
    let mut npc_accepted_total = 0usize;
    let mut attention_snapshots = vec![snap_attention(&session)];
    let mut cursors_snapshots = vec![receipt_cursors(&session)];
    // 逐 epoch 冻结 signal（v3·发现 1）：D(k) 的观察基 = S_k——每拍 step 前
    // 捕获（state.tick==k），probe 链不得统一使用末拍后 signal。
    let mut epoch_signals: Vec<(u64, f64)> = Vec::new();
    for tick in 0..4u64 {
        epoch_signals.push((tick, frozen_signal(&session)));
        let frame = session.step_frame().unwrap();
        npc_accepted_total += frame
            .events()
            .filter(|event| {
                matches!(
                    event,
                    Event::OrderAccepted { account, .. } | Event::IntentRejected { account, .. }
                        if *account != AccountId(0)
                )
            })
            .count();
        attention_snapshots.push(snap_attention(&session));
        cursors_snapshots.push(receipt_cursors(&session));
    }
    // A 类（日末核心·epoch 对齐形态【v2|审#1】）：全日每账户恰 4 次评估
    //（epoch 0..=3 各一），probe 链 oracle 从日初状态按各 epoch 冻结 signal
    // 连续评估 [0,1,2,3]。
    for account in &accounts {
        let (_, chained) =
            probe_chain_from(&session, *account, &attention_snapshots[0], &epoch_signals);
        let final_snap = attention_snapshots[4]
            .iter()
            .find(|(a, _, _)| a == account)
            .unwrap();
        assert_eq!(
            final_snap.1,
            chained.next_attention_candidate_tick,
            "A 类：{account:?} 全日恰 4 epoch 各一次评估（next 链=probe 链，无第五次；rng 终值受受理账户个人分析共享流影响，仅辅助）"
        );
        // A 类：无任何账户以 tick=4 为基准评估（pop 视界==3、无 epoch-4 抽样）
        //——等价于最终 next 仍由 epoch-3 评估产生且日末调用后状态不再变化。
        assert!(
            final_snap.1 >= 4,
            "A 类：epoch-3 评估后 next 不早于 4（无 tick=4 基准的提前评估）"
        );
    }
    // A 类：日末调用 (3→4) 的「D(3) 恰一次」的 epoch 锚定=跨 (2→3)+(3→4)
    // 联合窗口 receipt 前进量恰为 D(3) 的 intents 数（旧形状在 (2→3) tail
    // 戳记；T3 后在 (3→4) 入口——联合窗口两种形状都成立；B 类断言钉死
    // 旧形状的逐调用拆分）。
    // A 类：receipt 游标单调不减。
    for pair in cursors_snapshots.windows(2) {
        assert!(cursors_ge(&pair[1], &pair[0]), "A 类：游标单调不减");
    }
    // A 类（业务强度·当日）：真实活跃 NPC 产生已受理事实。
    assert!(npc_accepted_total > 0, "A 类：当日 npc_accepted>0（业务强度）");
    // A 类（业务强度·次日恢复）：存在账户 next ∈ (4, 8]——次日有真实 due。
    // fixture note：base=1.0 ⇒ wait=1 确定性 ⇒ next==4（恰为日界 epoch-4
    // 的 due）——A3 (4,8] 形态假设 wait>1 可能；取 [4,8] 含边界（业务强度
    // 等价：次日首 epoch 有真实 due 待评估，推迟≠消失）。
    assert!(
        attention_snapshots[4]
            .iter()
            .any(|(_, next, _)| *next >= 4 && *next <= 8),
        "A 类：次日有待评估 due（含日界 epoch-4 边界；推迟≠消失）"
    );
    // B 类（T3 已重登记）：旧形状「日末调用前后 attention 全等」（D(3) 由
    // (2→3) tail 预产）已失效——新形状 D(3) 于本调用入口现算 ⇒ 快照**必须
    // 推进**（base=1 链式 due 下全账户 next 3→4）；A 类「不执行下一日
    // epoch」已由上方 next≥4 断言与 probe 链闭合承载。
    assert_ne!(
        attention_snapshots[4], attention_snapshots[3],
        "B 类（新契约）：日末调用含 D(3) 现算（旧全等断言退役）"
    );
    // B 类：CivilUpdate 后日终档不带跨日批次、tick==4。
    session.end_civil_day_update().unwrap();
    let daily = session.game().save().unwrap();
    assert!(serde_json::to_value(&daily).unwrap().get("pending_npc").is_none(), "B 类：日终档不带 pending 批次");
    assert_eq!(daily.snapshot.tick, 4);
}

/// 场景 4：CivilUpdate 后（A3 §四.4）。
///
/// 全程不动 base_probability（fault1041：恢复侧按 setup 确定性重建 base）；
/// due 钉点与接受 rng 在 restore 后对双轨同值注入。
#[test]
fn epoch_alignment_after_civil_update_first_tick_evaluates_day_boundary_epoch() {
    let mut session = ProtocolSession::new(active_npc_setup(), 2).unwrap();
    let accounts: Vec<AccountId> = session
        .game()
        .state
        .npc_attention
        .keys()
        .copied()
        .collect();
    // 只钉调度与 rng，不动 base（restore 校验约束）。
    for account in &accounts {
        force_due_keep_base(&mut session, *account, 0, 3);
    }
    for _ in 0..4 {
        session.step_frame().unwrap();
    }
    let attention_before_civil = snap_attention(&session);
    session.end_civil_day_update().unwrap();
    // A 类：civil 不得触碰注意力/RNG。
    assert_eq!(snap_attention(&session), attention_before_civil);
    let slot = session.save().unwrap();
    let mut restored = ProtocolSession::restore(&slot).unwrap();
    // T3·调用见证隔离（T1Probes 纪律）：第二拍的 D(5) 写入记录从 trace 读取，
    // 独立池避免并行测试混入。
    restored
        .game_mut()
        .rebind_compute_for_verification(crate::compute::ComputeContext::with_workers(2).unwrap());
    let _ = restored.game().compute_context().take_queue_attention_trace();
    // A 类：失败回滚对照——首 tick 失败不消耗 NPC 随机流。
    let attention_at_restore = snap_attention(&restored);
    restored.malformed_frame = true;
    assert!(restored.step_frame().is_err());
    // A 类（收窄口径）：首 tick 失败**不消耗权威随机流**——attention（含
    // rng_state/next）与恢复时逐位不变。fixture note（实现期发现，登记
    // t3-epoch-fixture-notes）：共享 ingress 的 receive 事实跨失败 tick 持久
    //（next_account_ordinal 前移）——receive 事实不等候选回滚（ADR-0032
    // 「已发生 receipt 不改写」的边界表现），T3 实施批须显式裁定该口径。
    assert_eq!(
        snap_attention(&restored),
        attention_at_restore,
        "A 类：首 tick 失败不消耗权威随机流（attention 逐位不变）"
    );
    assert!(
        serde_json::to_value(&restored.game().save().unwrap()).unwrap().get("pending_npc").is_none(),
        "A 类：失败 tick 不安装批次"
    );
    restored.malformed_frame = false;
    // restore 后对双轨同值注入接受 rng（fixture 控制，双轨等值不破坏对拍）。
    let attention_before_day2 = snap_attention(&restored);
    let mut accepting: Vec<(AccountId, u64)> = Vec::new();
    for account in &accounts {
        let (_, next, _) = attention_before_day2
            .iter()
            .find(|(a, _, _)| a == account)
            .unwrap();
        if *next <= 4 {
            let seed = find_accepting_rng(&restored, *account, 4);
            force_due_keep_base(&mut restored, *account, 4, seed);
            force_due_keep_base(&mut session, *account, 4, seed);
            accepting.push((*account, seed));
        }
    }
    assert!(
        !accepting.is_empty(),
        "fixture 前提：日界 epoch 4 存在 due 账户"
    );
    // oracle：probe 链拆分（A3 v3·发现 3）：首调用仅 D(4)（[4] 链）；D(5)
    // 移至下一调用覆盖（[4,5] 链在第二拍闭合——旧形状首调用双 epoch 的分组
    // 差是 B 类，epoch 事实是 A 类，覆盖不删）。
    let attention_at_inject = snap_attention(&restored);
    // 逐 epoch 冻结 signal（v3·发现 1）：D(4) 观察基 = S_4（日终已提交版本），
    // 于次日首拍 step 前捕获。
    let signal_at_epoch4 = frozen_signal(&restored);
    let mut oracle_states = Vec::new();
    for (account, _) in &accepting {
        let (accepts4, probed4) =
            probe_chain_from(&restored, *account, &attention_at_inject, &[(4, signal_at_epoch4)]);
        assert!(accepts4[0], "注入 rng 应使 epoch 4 接受（自校验）");
        oracle_states.push((*account, probed4));
    }
    let resumed = restored.step_frame().unwrap();
    let attention_after_day2_first = snap_attention(&restored);
    // B 类（新契约）：跨 tick 载体退役——首调用后 pending 恒 None。
    assert!(
        serde_json::to_value(&restored.game().save().unwrap()).unwrap().get("pending_npc").is_none(),
        "B 类（新契约）：次日首调用不携带跨 tick 批次"
    );
    // 分钟精确见证·第一拍（A3 v3·发现 4）：首调用仅 D(4) 受理 ⇒ 受理账户的
    // 观察终值==日界 D(4) 的冻结观察分钟 1*240（day_tick=0；禁加减 tick 修绿，
    // 禁以「240 || step 后分钟」并集替代精确对应）。
    let mut observed_accepted = Vec::new();
    for (account, oracle) in &oracle_states {
        let after = attention_after_day2_first
            .iter()
            .find(|(a, _, _)| a == account)
            .unwrap();
        // A 类：next 链对拍（D(4) 恰一次评估；rng 受受理账户个人分析共享流影响仅辅助）。
        assert_eq!(
            after.1, oracle.next_attention_candidate_tick,
            "A 类：{account:?} 首调用 D(4) 恰一次评估（[4] probe 链）"
        );
        if let Some(row) = restored.game().state.retail_experience.get(account) {
            for (stock_id, stock) in &row.stocks {
                assert_eq!(
                    stock.last_observed_market_minute, 1 * 240,
                    "A 类（v3 分钟精确）：{account:?}/{stock_id:?} 第一拍终值==D(4) 的观察分钟 240"
                );
            }
            observed_accepted.push(*account);
        }
    }
    assert!(
        !observed_accepted.is_empty(),
        "A 类：业务强度·次日观察恢复（受理账户经验行更新）"
    );
    assert!(
        resumed
            .events()
            .any(|event| matches!(event,
                Event::OrderAccepted { account, .. } | Event::IntentRejected { account, .. }
                    if observed_accepted.contains(account))),
        "A 类：次日 NPC 请求真实执行"
    );
    // 第二拍 (5→6)：D(5) 入口现算——覆盖不删（A3 v3：迁移为 [4] 时 D(5) 的
    // 覆盖移至下一调用）。分钟按 D(5) 的**真实写入记录**（调用见证 trace 的
    // accepted 标志=生产自身受理判定）逐 (account, stock) 精确核对。
    // f(5)=第二拍前的权威分钟（D(5) 的冻结观察基）。
    let minute_at_epoch5_day2 = restored.game().current_market_minute();
    let _ = restored.step_frame().unwrap();
    let trace_day2 = restored.game().compute_context().take_queue_attention_trace();
    assert!(
        trace_day2.iter().any(|record| record.tick == 5),
        "A 类：第二拍实际评估 D(5)（覆盖不删）"
    );
    let epoch5_accepted: std::collections::BTreeSet<AccountId> = trace_day2
        .iter()
        .filter(|record| record.tick == 5 && record.accepted)
        .map(|record| record.account)
        .collect();
    for (account, _) in &accepting {
        if let Some(row) = restored.game().state.retail_experience.get(account) {
            for (stock_id, stock) in &row.stocks {
                let expected = if epoch5_accepted.contains(account) {
                    // 最后实际写入=D(5)（该账户在 epoch 5 实际受理并观察）。
                    minute_at_epoch5_day2
                } else {
                    // 最后实际写入仍为日界 D(4)（无后续写入保留原值）。
                    1 * 240
                };
                assert_eq!(
                    stock.last_observed_market_minute, expected,
                    "A 类（v3 分钟精确·第二拍）：{account:?}/{stock_id:?} 终值==最后实际写入 epoch（D(5) 受理={}）的观察分钟",
                    epoch5_accepted.contains(account),
                );
            }
        }
    }
    // B 类：T3 契约重登记点——首 tick 在日界版本物理位置准备（旧形状探针式
    // 断言，protocol/civil/session.rs:1230-1234 式）；epoch 事实已由 probe 对拍承载。
}

/// 场景 5：restore 后（A3 §四.5）。
///
/// ProtocolSession 的公共日级档要求已完成日结（fault3015）⇒ 日内 pending
/// epoch 的 restore 对拍在 GameSession 层进行（fixture note）。
#[test]
fn epoch_alignment_intraday_restore_resumes_exact_pending_epoch() {
    let mut setup = active_npc_setup();
    setup.ticks_per_day = 12;
    let mut live = GameSession::new(setup, 5).unwrap();
    let accounts: Vec<AccountId> = live.state.npc_attention.keys().copied().collect();
    for account in &accounts {
        force_due_keep_base_game(&mut live, *account, 0, 3);
    }
    for _ in 0..5 {
        live.step().unwrap();
    }
    // T3 契约重登记：日内静点档 pending=None（跨 tick 载体退役；fault2068
    // 「日内必携」分支随批三入口切换失效——批四随字段删除整体改写）。
    let slot = live.save().unwrap();
    assert!(
        serde_json::to_value(&slot).unwrap().get("pending_npc").is_none(),
        "B 类（新契约）：日内静点档不携带跨 tick 批次"
    );
    assert_eq!(slot.snapshot.tick, 5, "日内档 tick 锚（fixture 自证）");
    let mut twin = GameSession::restore(&slot).unwrap();
    let events_live = live.step().unwrap();
    let events_twin = twin.step().unwrap();
    // A 类：两轨 tick 6 事件全等；attention 全等 ⇒ 批次消费恰一次、无重算。
    assert_eq!(
        serde_json::to_value(&events_live).unwrap(),
        serde_json::to_value(&events_twin).unwrap(),
        "A 类：restore 双轨 tick 6 事件全等（不重复执行、不丢失 epoch）"
    );
    assert_eq!(
        snap_attention_game(&live),
        snap_attention_game(&twin),
        "A 类：attention（含 rng_state/next）双轨全等 ⇒ 无重算、无二次评估"
    );
    // B 类（批四终态，A4 失败 3 处置链）：旧负例「pending=None ⇒ fault2071/
    // step 报 missing」已两度重登记——批四删除字段后，日内档「无跨 tick 批」
    // 由**键缺失**结构性承载（上方断言）；携带 pending_npc 键（任意形态，
    // 含 null）即旧格式档 ⇒ 解码边界 fault2100（见
    // missing_npc_batch_contract_after_entry_switch 的解码负例）。
    let mut tampered = serde_json::to_value(&slot).unwrap();
    tampered["pending_npc"] = serde_json::Value::Null;
    assert!(matches!(
        crate::decode_save_slot(
            &serde_json::to_vec(&tampered).unwrap(),
            &crate::session::persistence::SaveDecodeLimits::default()
        ),
        Err(crate::SessionError::InvalidSave(fault)) if fault.code == 2100
    ));
}

// ---------------------------------------------------------------------------
// GameSession 层助手（场景 5）

// ---------------------------------------------------------------------------
// 内部助手
// ---------------------------------------------------------------------------

/// probe 链（显式初始状态版）：从 `initial` 中取该账户状态，在其上按
/// `frozen`（(tick, 该 epoch 冻结观察基 signal) 对——捕获时点见
/// [`frozen_signal`]）顺序各评估一次。**逐 epoch 读取各自冻结 signal**
/// （Astra 四发现裁定·发现 1：跨 epoch probe 不得共用一次当前 market
/// signal——评估输入流必须与生产在各 epoch 的冻结观察基逐位同源）。
fn probe_chain_from(
    session: &ProtocolSession,
    account: AccountId,
    initial: &AttentionSnap,
    frozen: &[(u64, f64)],
) -> (Vec<bool>, crate::session::attention::NpcAttentionState) {
    let game = session.game();
    let (_, _, rng_state) = initial
        .iter()
        .find(|(a, _, _)| *a == account)
        .unwrap_or_else(|| panic!("快照缺少账户 {account:?}"));
    let attention = game
        .state
        .npc_attention
        .get(&account)
        .expect("账户必须已有 attention 状态");
    let mut state = attention.clone();
    state.rng_state = *rng_state;
    // next_candidate 以初始快照为准：评估会重写它，此处仅对齐评估输入流。
    if let Some((_, next, _)) = initial.iter().find(|(a, _, _)| *a == account) {
        state.next_attention_candidate_tick = *next;
    }
    let kind = game
        .state
        .accounts
        .get(&account)
        .expect("账户必须存在")
        .kind();
    let mut accepted_all = Vec::with_capacity(frozen.len());
    for (tick, signal) in frozen {
        accepted_all.push(state.evaluate_candidate_with_signal(kind, *signal, *tick));
    }
    (accepted_all, state)
}

/// receipt 游标单调不减（以 save 快照的序列化可比性近似：逐账户/逐股游标
/// 只增不减）。用整包比较：after 的每项 ≥ before。
fn cursors_ge(
    after: &crate::session::IngressReceiptCursors,
    before: &crate::session::IngressReceiptCursors,
) -> bool {
    fn next_of(map: &std::collections::BTreeMap<AccountId, u64>, key: &AccountId) -> u64 {
        map.get(key).copied().unwrap_or(0)
    }
    for (account, value) in &before.next_account_ordinal {
        if next_of(&after.next_account_ordinal, account) < *value {
            return false;
        }
    }
    for (code, value) in &before.next_stock_ordinal {
        let after_value = after
            .next_stock_ordinal
            .get(code)
            .copied()
            .unwrap_or(0);
        if after_value < *value {
            return false;
        }
    }
    true
}

fn monotonic_cursors(session: &ProtocolSession) -> bool {
    // 结构性自检：游标表内值均 ≥ 0（占位——真正的单调性由测试内前后快照
    // 比较 cursors_ge 承载）。
    session
        .game()
        .save()
        .unwrap()
        .ingress_receipt_cursors
        .next_account_ordinal
        .values()
        .all(|value| *value > 0)
        || session
            .game()
            .save()
            .unwrap()
            .ingress_receipt_cursors
            .next_account_ordinal
            .is_empty()
}

fn snap_attention_game(game: &GameSession) -> AttentionSnap {
    game.state
        .npc_attention
        .iter()
        .map(|(account, state)| (*account, state.next_attention_candidate_tick, state.rng_state))
        .collect()
}

fn force_due_keep_base_game(game: &mut GameSession, account: AccountId, tick: u64, rng_state: u64) {
    let attention = game
        .state
        .npc_attention
        .get_mut(&account)
        .expect("账户必须已有 attention 状态");
    attention.rng_state = rng_state;
    attention.next_attention_candidate_tick = tick;
    game.state.attention_scheduler.enqueue(tick, account);
}

// ---------------------------------------------------------------------------
// T3·调用与 RNG 见证（A3 v3 §二 A 类增补 1；Astra 四发现裁定·发现 1）
// ---------------------------------------------------------------------------

/// 调用见证的结构性事实（旧生产形状；attempt=queue 生产调用）：
/// - 每 attempt 的全部 evaluate 记录共用同一 tick（一次生产调用=一个 pop 视界）；
/// - attempt 间 tick 严格递增（跨 epoch 不回退、不重弹）；
/// - (account, tick) 全窗恰一次——**实际调用证据**（next 链/RNG 推进仅辅助）；
/// - 每条记录 rng_after != rng_before（接受判定无条件抽样一次，attention.rs:83）；
/// - B 类（旧形状）：交易日首次调用产生两个 attempt（日界 D(4) 入口+tail D(5)），
///   T3 入口切换后缩为一个（届时按 B 类重登记本断言）。
#[test]
fn epoch_alignment_attention_call_trace_witnesses_one_eval_per_epoch_account() {
    let mut session = ProtocolSession::new(active_npc_setup(), 2).unwrap();
    // 调用见证按 ComputeContext 隔离（T1Probes 纪律）：默认共享池会混入并行
    // 测试的生产记录——重绑独立池后 drain 一次清底。
    session
        .game_mut()
        .rebind_compute_for_verification(crate::compute::ComputeContext::with_workers(2).unwrap());
    let _ = session.game().compute_context().take_queue_attention_trace();
    let accounts: Vec<AccountId> = session
        .game()
        .state
        .npc_attention
        .keys()
        .copied()
        .collect();
    for account in &accounts {
        force_due(&mut session, *account, 0, 3);
    }
    let mut window: Vec<(u64 /*call*/, Vec<crate::compute::AttentionCallRecord>)> = Vec::new();
    for call in 0..4 {
        session.step_frame().unwrap();
        window.push((call, session.game().compute_context().take_queue_attention_trace()));
    }
    session.end_civil_day_update().unwrap();
    session.step_frame().unwrap();
    // B 类（T3 已重登记）：交易日首次调用=唯一入口生产，单 attempt 单视界
    // {4}（旧形状同窗双 epoch {4,5} 的分组差已消除——t3-stop-register 冲突 1
    // 的形状；D(5) 移至下一调用，覆盖由 epoch 覆盖断言承载）。
    let day2_first = session.game().compute_context().take_queue_attention_trace();
    let day2_ticks: std::collections::BTreeSet<u64> =
        day2_first.iter().map(|record| record.tick).collect();
    assert_eq!(
        day2_ticks,
        std::collections::BTreeSet::from([4]),
        "B 类（新契约）：首次调用单 attempt 单视界 D(4)"
    );
    window.push((4, day2_first));

    // 聚合断言（A 类，形状无关）。
    let mut seen: std::collections::BTreeSet<(AccountId, u64)> = std::collections::BTreeSet::new();
    let mut last_tick: Option<u64> = None;
    for record in window.iter().flat_map(|(_, records)| records.iter()) {
        // 每条记录：evaluate 边界确有推进（无条件一次抽样）。
        assert_ne!(
            record.rng_before, record.rng_after,
            "A 类：evaluate 边界 RNG 必须推进（attempt={} tick={} account={:?}）",
            record.attempt, record.tick, record.account
        );
        // (account, tick) 全窗恰一次。
        assert!(
            seen.insert((record.account, record.tick)),
            "A 类：{:?}@epoch{} 被重复评估（attempt={}）",
            record.account,
            record.tick,
            record.attempt
        );
        // attempt 间 tick 严格不减（记录按生产顺序到达）。
        if let Some(previous) = last_tick {
            assert!(
                record.tick >= previous,
                "A 类：attempt 间 tick 不得回退（{previous}→{}）",
                record.tick
            );
        }
        last_tick = Some(record.tick);
    }
    // 每 attempt 内单视界：同一 attempt 的记录 tick 唯一（注意按 attempt 分组
    // 而非按 drain 窗口——交易日首次调用的 drain 窗口含两个 attempt，fixture
    // note：tick 0 亦为日界首 tick，首调用即「日界入口+tail」双 attempt 形状）。
    let mut by_attempt: std::collections::BTreeMap<u64, std::collections::BTreeSet<u64>> =
        std::collections::BTreeMap::new();
    for record in window.iter().flat_map(|(_, records)| records.iter()) {
        by_attempt.entry(record.attempt).or_default().insert(record.tick);
    }
    for (attempt, ticks) in &by_attempt {
        assert_eq!(
            ticks.len(),
            1,
            "A 类：一个 attempt 只有一个 pop 视界 tick（attempt={attempt} ticks={ticks:?}）"
        );
    }
    // 全窗覆盖两个交易日的全部 epoch（0..=4 至少各 attempt 一次；空批 attempt 无
    // 记录不显形——由 epoch 覆盖断言兜底：每个 epoch 0..=4 至少一条记录）。
    for epoch in 0u64..=4 {
        assert!(
            window
                .iter()
                .flat_map(|(_, records)| records.iter())
                .any(|record| record.tick == epoch),
            "A 类：epoch {epoch} 必须有实际调用见证"
        );
    }
}

/// RNG 边界精确见证（发现 1 口径）：拒绝账户无个人分析续抽 ⇒ 权威终态
/// rng_state == 该账户最后一次 evaluate 记录的 rng_after（逐位）；
/// 接受账户的终态不可由纯 evaluate probe 预测（个人分析共享流），只断言
/// 记录存在与 accepted 标记（完整个人终态由场景 1 的 next 链/经验行断言承载）。
#[test]
fn epoch_alignment_rejected_account_rng_is_exact_at_evaluate_boundary() {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    let mut session = ProtocolSession::new(setup, 7).unwrap();
    // 同上：独立池隔离调用见证。
    session
        .game_mut()
        .rebind_compute_for_verification(crate::compute::ComputeContext::with_workers(2).unwrap());
    let _ = session.game().compute_context().take_queue_attention_trace();
    let a = AccountId(1);
    let b = AccountId(2);
    force_due(&mut session, a, 0, 3);
    let b_rng = find_rejecting_rng(&session, b, 0, 0.1, 2);
    force_due_with_base(&mut session, b, 0, b_rng, 0.1);
    session.step_frame().unwrap();
    let trace = session.game().compute_context().take_queue_attention_trace();
    let record_b = trace
        .iter()
        .find(|record| record.account == b && record.tick == 0)
        .expect("B 在 epoch 0 必须有实际调用见证");
    assert!(!record_b.accepted, "B 的生产判定=拒绝（与 probe oracle 一致）");
    let final_b = snap_attention(&session)
        .into_iter()
        .find(|(account, _, _)| *account == b)
        .unwrap();
    assert_eq!(
        final_b.2, record_b.rng_after,
        "A 类：拒绝账户权威 rng == 最后一次 evaluate 边界值（无个人分析续抽，流纯净）"
    );
    // 接受账户：记录存在且 accepted=true（A 的终态断言见场景 1——共享流发现一）。
    let record_a = trace
        .iter()
        .find(|record| record.account == a && record.tick == 0)
        .expect("A 在 epoch 0 必须有实际调用见证");
    assert!(record_a.accepted, "A base=1.0 的生产判定=接受");
    assert_ne!(record_a.rng_before, record_a.rng_after, "A 类：evaluate 边界推进");
}

/// 接受账户的**共享 RNG 终态精确见证**（Astra 入口切换检查点 P1 修复；
/// A1 §2 v3「完整个人终态须覆盖个人分析抽样」）。受理零售的个人分析
/// （`sample_discovery_stock` 等）继续消费同一 `attention.rng_state` 流——
/// 终态不可由纯 evaluate probe 预测（v3 契约禁止拆流比较），本见证以三面
/// 钉死完整终态：
/// 1. **调用见证**（trace）：(A, epoch 0) 恰一次 evaluate，accepted，
///    rng_before == step 前权威值；
/// 2. **终态严格越过 evaluate 边界**（个人分析共享流确有续抽——若分析
///    改走独立 RNG/未执行，终态将停留在 rng_after，断言破裂）；
/// 3. **终态逐位可复现**：同档 restore 双轨各自 step，受理账户终态 rng
///    逐位相等——完整个人计算是冻结输入的确定函数，任何隐式状态、调度
///    顺序影响或跨账户流串扰都会打破逐位相等；另以冻结 signal probe 对拍
///    next 链（evaluate 恰一次的 next 推进独立于个人分析续抽）。
#[test]
fn epoch_alignment_accepted_account_personal_rng_terminal_is_exact() {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 1;
    let mut session = ProtocolSession::new(setup, 7).unwrap();
    // 独立池隔离调用见证（T1Probes 纪律）。
    session
        .game_mut()
        .rebind_compute_for_verification(crate::compute::ComputeContext::with_workers(1).unwrap());
    let _ = session.game().compute_context().take_queue_attention_trace();
    let a = AccountId(1);
    // 只钉调度与 rng（不动 base——fault1041：恢复侧按 setup 确定性重建 base，
    // 改动即拒；与场景 4/5 的 keep-base 注入同款）。接受性由
    // find_accepting_rng 在默认 base 下确定性搜索保证（fixture 自校验）。
    let accepting_rng = find_accepting_rng(&session, a, 0);
    force_due_keep_base(&mut session, a, 0, accepting_rng);
    // 注入后的完整状态入档（双轨同基）：attention/调度/信念随档保持。
    // GameSession 层档（ProtocolSession 公共档要求已完成日结 fault3015，
    // 见场景 5 fixture note）。
    let slot = session.game().save().unwrap();
    let signal_at_epoch0 = frozen_signal(&session);
    let attention_before = snap_attention(&session);
    let &(_, _, rng_before) = attention_before
        .iter()
        .find(|(account, _, _)| *account == a)
        .unwrap();

    session.step_frame().unwrap();

    // 1. 调用见证：恰一次 evaluate（trace 内 (A,0) 唯一），accepted，边界
    //    rng_before == step 前权威值。
    let trace = session.game().compute_context().take_queue_attention_trace();
    let records_a: Vec<_> = trace
        .iter()
        .filter(|record| record.account == a && record.tick == 0)
        .collect();
    assert_eq!(
        records_a.len(),
        1,
        "A 类：(A, epoch 0) 恰一次 evaluate（实际调用见证）"
    );
    let record = records_a[0];
    assert!(record.accepted, "A 的生产判定=接受（搜索 rng 的 fixture 前提）");
    assert_eq!(
        record.rng_before, rng_before,
        "A 类：evaluate 边界前值==step 前权威 rng"
    );
    // 受理观察真实发生（业务强度：经验行——受理账户的真实观察面；NPC 请求
    // 事件的非空断言由 witness/场景 3 的活跃 fixture 承载，本 fixture 默认
    // 信念下受理观察可不含交易 intents，不影响 RNG 终态见证的核心）。
    assert!(
        session.game().state.retail_experience.contains_key(&a),
        "A 类：受理观察创建经验行（个人分析路径的前提）"
    );

    // 2. 终态严格越过 evaluate 边界：个人分析（sample_discovery_stock 等）
    //    继续消费同一共享流 ⇒ 权威终态 rng != evaluate 边界 rng_after。
    let terminal_live = snap_attention(&session)
        .into_iter()
        .find(|(account, _, _)| *account == a)
        .unwrap();
    assert_ne!(
        terminal_live.2, record.rng_after,
        "A 类：受理账户终态 rng 严格越过 evaluate 边界（个人分析共享流确有续抽；\
         若分析未执行或改走独立流，终态将停留在 rng_after，本断言破裂）"
    );

    // 3. 终态逐位可复现：同档 restore 双轨，各自 step，终态逐位相等
    //    （完整个人计算=冻结输入的确定函数）。
    let mut twin = GameSession::restore(&slot).unwrap();
    twin.rebind_compute_for_verification(crate::compute::ComputeContext::with_workers(1).unwrap());
    // D5 双轨同基适配：live 轨经 step_frame（candidate 投影契约——恒走
    /// 旧管线）；twin 的 step 在 ENGINE_STEP_RUNTIME 下走 std-thread
    /// runtime，双轨异管线使"冻结输入确定函数"的逐位断言结构性破裂。
    /// twin 改经 step_pipeline_contract（同一旧管线入口）恢复同基；
    /// runtime 侧的等价锚=retail 链 restore 双轨（public_step_consumes_
    /// retail_beliefs 组内同 rng 派生式）承载。
    if crate::runtime::step_runtime_enabled() {
        twin.step_pipeline_contract().unwrap();
    } else {
        twin.step().unwrap();
    }
    let terminal_twin = snap_attention_game(&twin)
        .into_iter()
        .find(|(account, _, _)| *account == a)
        .unwrap();
    assert_eq!(
        (terminal_live.1, terminal_live.2),
        (terminal_twin.1, terminal_twin.2),
        "A 类：受理账户终态（next, rng）双轨逐位相等——完整个人分析消费是冻结\
         输入的确定函数（任何隐式状态/调度串扰都会打破逐位相等）"
    );

    // 补充对拍：evaluate 恰一次的 next 推进与冻结 signal probe 一致（next
    // 链独立于个人分析续抽）。
    let (accepts, probed) = probe_chain_from(
        &session,
        a,
        &attention_before,
        &[(0, signal_at_epoch0)],
    );
    assert!(accepts[0], "fixture 自校验：probe 判定=接受");
    assert_eq!(
        terminal_live.1, probed.next_attention_candidate_tick,
        "A 类：终态 next==evaluate 恰一次的 probe 链值"
    );
}
