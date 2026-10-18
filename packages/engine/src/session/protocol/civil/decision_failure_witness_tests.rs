//! T3·三类失败位置的 receipt/payload 边界见证（ADR-0042 §9.4 共通条款 v3；
//! ADR-0032 R4.4；Astra 四发现裁定·发现 2 / 步 4 放行条件 2）。
//!
//! 三位置：receive 前（捕获错误）/ 部分 receive 后（流内任务错误）/
//! 执行或 publication 校验失败后（precommit frame 校验）。断言面全部
//! **形状无关**（step 边界 + 权威/游标/共享队列前后对照），旧形状（tail
//! 生产）与新形状（入口生产）都必须成立——失败归属的调用编号差异属
//! ADR-0042 §9.4 表登记面，不在本组断言范围。

use super::ProtocolSession;
use crate::*;

/// 双零售 fixture（ProtocolSession 层：协议入口的失败**不 poison**——既有错误
/// 契约允许同会话修复后重试，与 civil/session.rs:1130-1140 失败回滚对照同一
/// 语义；GameSession::step 的 poison 契约不适用本见证的重试面）。
fn witness_setup(retail_count: u32, seed: u64) -> ProtocolSession {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = retail_count;
    let mut session = ProtocolSession::new(setup, seed).unwrap();
    session
        .game_mut()
        .rebind_compute_for_verification(crate::compute::ComputeContext::with_workers(1).unwrap());
    session
}

fn force_due_game(session: &mut GameSession, account: AccountId, tick: u64, rng_state: u64) {
    let attention = session
        .state
        .npc_attention
        .get_mut(&account)
        .expect("账户必须已有 attention 状态");
    attention.base_probability = 1.0;
    attention.rng_state = rng_state;
    attention.next_attention_candidate_tick = tick;
    session.state.attention_scheduler.enqueue(tick, account);
    // 策略=ZiNoise arrival=1.0（每次观察产生交易 intents——t1 due_session 同款，
    // 保证 receive 事实非空）+ base 同步（fault2210 同款）。
    let probability = session.state.npc_attention[&account].base_probability;
    let mut strategy = crate::strategy::StrategyState::ZiNoise(
        crate::strategy::ZiNoiseStrategy::new(1.0, 100, 0.5).unwrap(),
    );
    strategy.set_base_observation_probability(probability);
    session
        .fixture_account_mut(&account)
        .unwrap()
        .set_strategy(strategy.into_strategy().unwrap());
    // 买入失衡信念（t1 due_session/use_buy_imbalance_analysis 同构）：使受理
    // 观察确定产生同股买入 intents——receive 事实非空的前置。
    let profile = session.state.accounts[&account]
        .strategy()
        .unwrap()
        .profile();
    let analysis = crate::strategy::AnalysisProfile::new(
        crate::strategy::AnalysisWeights::new(0, 0, 10_000, 0, 0).unwrap(),
        None,
    )
    .unwrap();
    *session
        .state
        .belief_participants
        .get_mut(&account)
        .unwrap()
        .belief_mut() = crate::strategy::BeliefBook::new(
        account,
        profile,
        analysis,
        &mut crate::session::SplitMix64::new(99),
    );
}

fn player_buy(session: &GameSession) -> Intent {
    let code = session
        .stock_id(&session.state.setup.stocks[0].code.0)
        .unwrap();
    Intent::PlaceLimit {
        code,
        side: Side::Buy,
        price: LimitPrice::Fixed(Money::from_cents(990)),
        qty: 100,
    }
}

/// 权威游标（直接读状态字段——save() 的 saved_runtime 校验在故意破坏的
/// fixture 上不适用；权威游标只在成功 commit 时同步）。
fn authority_cursors(session: &GameSession) -> crate::session::IngressReceiptCursors {
    session.state.ingress_receipt_cursors.clone()
}

/// 共享 receiver 当前游标——失败 tick 的永久 gap 事实在此读取
/// （receipts_probe；权威侧读不到失败 attempt 已分配的 ordinal）。
fn shared_cursors(session: &mut GameSession) -> crate::session::IngressReceiptCursors {
    session.shared_ingress().receipts_probe()
}

fn attention_snap(session: &GameSession) -> Vec<(AccountId, u64, u64)> {
    session
        .state
        .npc_attention
        .iter()
        .map(|(account, state)| (*account, state.next_attention_candidate_tick, state.rng_state))
        .collect()
}

/// 位置一：决策捕获错误于**任何 receive 之前**（MissingAttention 在 pop 校验
/// 抛出，早于账户任务流）。权威完全不变；**不产生任何 receipt**（游标零
/// 前进）；外部玩家输入在共享接收面保留（失败不丢外部事实）。
#[test]
fn failure_before_any_receive_allots_no_receipts_and_keeps_external_inputs() {
    let mut session = witness_setup(2, 11);
    session.step_frame_pipeline_contract().unwrap();
    // 破坏：due 账户缺 attention 状态 ⇒ pop 校验 MissingAttention（receive 前）。
    let due_tick = session.game().state.tick;
    force_due_game(session.game_mut(), AccountId(1), due_tick, 3);
    let removed_attention = session
        .game_mut()
        .state
        .npc_attention
        .remove(&AccountId(1))
        .unwrap();
    // 外部玩家请求先真实接收（enqueue 即登记 receipt）。
    session
        .game_mut()
        .shared_ingress()
        .enqueue_player_intent(AccountId(0), player_buy(session.game()))
        .unwrap();
    let before = session.game().business_state_hash().unwrap();
    let attention_before = attention_snap(session.game());
    let authority_before = authority_cursors(session.game());
    let shared_before = shared_cursors(session.game_mut());

    let error = session.step_frame_pipeline_contract().expect_err("MissingAttention 必须使整个 tick 失败");

    assert!(
        format!("{error:?}").contains("缺少 attention state"),
        "实际错误: {error:?}"
    );
    assert_eq!(
        session.game().business_state_hash().unwrap(),
        before,
        "权威业务状态不变"
    );
    assert_eq!(
        attention_snap(session.game()),
        attention_before,
        "权威注意力/RNG 未被消耗"
    );
    assert_eq!(
        serde_json::to_value(authority_cursors(session.game())).unwrap(),
        serde_json::to_value(&authority_before).unwrap(),
        "receive 前 failure：权威游标零前进"
    );
    // receive 前 failure：共享 receiver 亦零前进（失败 attempt 未产生任何
    // NPC receipt；玩家 enqueue 的前进发生在失败步骤**之前**，属已发生外部
    // 事实，不计入失败 attempt）。
    assert_eq!(
        serde_json::to_value(shared_cursors(session.game_mut())).unwrap(),
        serde_json::to_value(&shared_before).unwrap(),
        "receive 前 failure：共享 receiver 零前进（无 NPC receipt）"
    );
    // 外部输入保留：新 tick shadow 的 freeze 快照仍能截到该玩家请求。
    let mut probe = session.game().clone_for_tick_shadow().unwrap();
    probe.freeze_shared_ingress().unwrap();
    assert_eq!(probe.state.pending_player.len(), 1, "失败不丢失外部玩家输入");
    // 修复后重试（协议层失败不 poison——既有错误契约允许同会话修复后重试）：
    // 玩家请求被执行（接收事实唯一，恰一次消费）。
    session
        .game_mut()
        .state
        .npc_attention
        .insert(AccountId(1), removed_attention);
    let frame = session.step_frame_pipeline_contract().expect("修复后重试必须成功");
    assert!(
        frame
            .events()
            .any(|event| matches!(event, Event::OrderAccepted { account, .. } | Event::IntentRejected { account, .. } if *account == AccountId(0))),
        "重试后玩家请求真实执行"
    );
}

/// 位置二：**部分 receive 后**失败（账户 1 任务成功并登记 intents，账户 2
/// 任务内校验错误；单 worker 下到达序=due 序 ⇒ 账户 1 先 receive）。
/// 已分配 ordinal 留永久 gap；权威不变；外部输入保留；修复重算后旧 payload
/// 不复活、新批按新 ordinal 恰一次执行。
#[test]
fn partial_receive_failure_leaves_permanent_gap_and_discards_private_payload() {
    let mut session = witness_setup(2, 23);
    // 玩家买单先进簿（受理观察的失衡输入面：账户 1 的买入失衡信念在真实
    // book imbalance 下产生交易 intents——receive 事实非空的前提）。
    session
        .game_mut()
        .shared_ingress()
        .enqueue_player_intent(AccountId(0), player_buy(session.game()))
        .unwrap();
    session.step_frame_pipeline_contract().unwrap();
    session.step_frame_pipeline_contract().unwrap();
    let tick = session.game().state.tick;
    // 账户 1：受理并产生 intents（receive 发生在归并回调）。
    force_due_game(session.game_mut(), AccountId(1), tick, 3);
    // 账户 2：base=0.0 非法 ⇒ 账户任务内 validate 失败（在账户 1 之后到达）。
    force_due_game(session.game_mut(), AccountId(2), tick, 5);
    session
        .game_mut()
        .state
        .npc_attention
        .get_mut(&AccountId(2))
        .unwrap()
        .base_probability = 0.0;
    session
        .game_mut()
        .shared_ingress()
        .enqueue_player_intent(AccountId(0), player_buy(session.game()))
        .unwrap();
    let before = session.game().business_state_hash().unwrap();
    let shared_before = shared_cursors(session.game_mut());
    let _ = shared_before; // （玩家 enqueue 已发生的外部事实；失败 attempt 的
    // gap 由下方 shared-after-failure vs authority 之差见证）

    let error = session.step_frame_pipeline_contract().expect_err("账户 2 概率非法必须使整个 tick 失败");

    assert!(
        format!("{error:?}").contains("注意力概率"),
        "实际错误: {error:?}"
    );
    assert_eq!(
        session.game().business_state_hash().unwrap(),
        before,
        "权威业务状态不变"
    );
    // gap：账户 1 的 intents 已在共享 receiver 登记（ordinal 前进），权威侧
    // 未同步（candidate 丢弃）——两者之差即失败 attempt 的永久 gap。
    let shared_after_failure = shared_cursors(session.game_mut());
    let a1_authority = authority_cursors(session.game())
        .next_account_ordinal
        .get(&AccountId(1))
        .copied()
        .unwrap_or(0);
    let a1_shared = shared_after_failure
        .next_account_ordinal
        .get(&AccountId(1))
        .copied()
        .unwrap_or(0);
    assert!(
        a1_shared > a1_authority,
        "部分 receive：账户 1 的已登记 intents 留永久 gap（共享 {a1_shared} > 权威 {a1_authority}）"
    );
    // 外部输入保留。
    let mut probe = session.game().clone_for_tick_shadow().unwrap();
    probe.freeze_shared_ingress().unwrap();
    assert_eq!(probe.state.pending_player.len(), 1, "外部玩家输入不因失败丢失");
    // 修复重试（协议层失败不 poison）：旧 payload 不复活——账户 1 重算批按
    // **新** ordinal 恰一次执行，游标在 gap 之上继续单调前进（不复用、不回退）。
    session
        .game_mut()
        .state
        .npc_attention
        .get_mut(&AccountId(2))
        .unwrap()
        .base_probability = 0.5;
    // 形状无关的重算执行见证：旧形状 tail 生产在下一调用消费、新形状同 tick
    // 消费——收集至多 3 个重试帧内账户 1 的 OrderAccepted，断言非空（旧
    // payload 复活/重算批丢失都会打破）、全帧无重复 id（恰一次执行）。
    let mut accepted_ids: Vec<crate::OrderId> = Vec::new();
    for _ in 0..3 {
        let frame = session.step_frame_pipeline_contract().expect("修复后重试必须成功");
        accepted_ids.extend(
            frame
                .events()
                .filter_map(|event| match event {
                    Event::OrderAccepted { account, id, .. } if *account == AccountId(1) => {
                        Some(*id)
                    }
                    _ => None,
                }),
        );
        if !accepted_ids.is_empty() {
            break;
        }
    }
    let a1_retry = authority_cursors(session.game()).next_account_ordinal[&AccountId(1)];
    assert!(
        !accepted_ids.is_empty(),
        "重算批真实执行（旧 payload 复活则会出现跨批重复执行）"
    );
    let distinct: std::collections::BTreeSet<_> = accepted_ids.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        accepted_ids.len(),
        "重算批跨帧无重复执行（恰一次；旧 payload 复活会复用其订单身份）"
    );
    assert!(
        a1_retry >= a1_shared,
        "重试后权威游标越过 gap 继续前进（不复用、不回退：{a1_retry} ≥ {a1_shared}）"
    );
}

/// 位置三：**执行/publication 校验失败后**（precommit frame 校验，全批
/// intents 已 receive）。私有 payload/个人 patch 整体丢弃（权威注意力/RNG
/// 不被消耗）；已登记 receipt 留 gap；重算后一次执行。
#[test]
fn publication_failure_discards_payload_keeps_receipts_and_retries_once() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.stocks[0].float_shares = 10_000;
    setup.npcs.retail_count = 2;
    setup.npcs.inst_count = 0;
    setup.npcs.retail_cash_median = Money::from_cents(100_000_000);
    setup.strategy_params.retail.arrival_rate = 1.0;
    setup.ticks_per_day = 4;
    setup.start_date = CivilDate::from_iso("2030-01-07").unwrap();
    let mut session = ProtocolSession::new(setup, 5).unwrap();
    session
        .game_mut()
        .rebind_compute_for_verification(crate::compute::ComputeContext::with_workers(2).unwrap());
    let accounts: Vec<AccountId> = session.game().state.npc_attention.keys().copied().collect();
    for account in &accounts {
        let attention = session
            .game_mut()
            .state
            .npc_attention
            .get_mut(account)
            .unwrap();
        attention.rng_state = 3;
        attention.next_attention_candidate_tick = 0;
        session
            .game_mut()
            .state
            .attention_scheduler
            .enqueue(0, *account);
    }
    for _ in 0..4 {
        session.step_frame_pipeline_contract().unwrap();
    }
    session.end_civil_day_update().unwrap();
    // 次日首 tick：决策批已完整生产并 receive，precommit frame 校验失败。
    let attention_before = session
        .game()
        .state
        .npc_attention
        .iter()
        .map(|(account, state)| (*account, state.next_attention_candidate_tick, state.rng_state))
        .collect::<Vec<_>>();
    let shared_before = session.game_mut().shared_ingress().receipts_probe();
    session.malformed_frame = true;
    assert!(session.step_frame_pipeline_contract().is_err(), "malformed frame 必须使 publication 失败");
    // 权威注意力/RNG 未被消耗（个人 patch 随 candidate 整体丢弃）。
    let attention_after_failure = session
        .game()
        .state
        .npc_attention
        .iter()
        .map(|(account, state)| (*account, state.next_attention_candidate_tick, state.rng_state))
        .collect::<Vec<_>>();
    assert_eq!(
        attention_after_failure, attention_before,
        "publication 失败不消耗权威随机流（patch 整体丢弃）"
    );
    // 已登记 NPC receipt 留 gap（共享接收事实不回滚）：共享 receiver 前进、
    // 权威未同步——两者之差即失败 attempt 的 gap。
    let shared_after_failure = session.game_mut().shared_ingress().receipts_probe();
    let authority_after_failure = authority_cursors(session.game());
    let gap_witnessed = shared_after_failure
        .next_account_ordinal
        .iter()
        .any(|(account, shared)| {
            shared > &authority_after_failure
                .next_account_ordinal
                .get(account)
                .copied()
                .unwrap_or(0)
        });
    assert!(
        gap_witnessed,
        "失败 attempt 已 receive 的 NPC intents 留永久 gap（共享 receiver > 权威）"
    );
    let _ = shared_before;
    // 重试：D(4) 重算、按新 receive 恰一次执行；无旧 payload 复活。
    session.malformed_frame = false;
    let resumed = session.step_frame_pipeline_contract().expect("重试必须成功");
    let npc_events: Vec<_> = resumed
        .events()
        .filter(|event| {
            matches!(
                event,
                Event::OrderAccepted { account, .. } | Event::IntentRejected { account, .. }
                    if accounts.contains(account)
            )
        })
        .collect();
    assert!(
        !npc_events.is_empty(),
        "重算批真实执行（业务强度：次日 NPC 活跃）"
    );
    let cursors_retry = authority_cursors(session.game());
    for (account, after) in &cursors_retry.next_account_ordinal {
        let failure_level = shared_after_failure
            .next_account_ordinal
            .get(account)
            .copied()
            .unwrap_or(0);
        assert!(
            *after >= failure_level,
            "重试后权威游标越过 gap（{account:?} {failure_level}→{after}；不复用、不回退）"
        );
    }
}

// ---------------------------------------------------------------------------
// 修复批增补（Astra 入口切换检查点 P1）：按**实际 receipt ordinal 与精确
// 消费数量**验证完整重试窗口——上一组用例的「首个非空帧 + OrderId 去重 +
// cursor ≥ gap」对旧 payload 复活不充分（失败 intent 尚无 OrderId，复活后
// 可取得新 ID；去重与游标下界仍可能通过）。本组断言收紧为：任一成功帧内
// **执行数 == 该 attempt 新分配 ordinal 数**（无 gap ordinal 混入执行的
// 余地）、无执行帧不提交新 ordinal、帧间无新登记、末态游标越过 gap 水位。
// 口径锚定入口切换后契约（ADR-0042 §9.4 v3：重试重新 freeze、新请求按新
// receive 分配新 ordinal；旧 tail 形状的重试消费已提交存量批，不属本契约
// 形状——切换前旧形状绿证据由原三用例承载，见 logs/t3fix-preswitch-named.log）。
// ---------------------------------------------------------------------------

/// 帧内外部受理事件计数（OrderAccepted | IntentRejected；成交/撤销不计——
/// 受理才是一次 receive 的恰一次执行面）。
fn admission_count<'a>(events: impl IntoIterator<Item = &'a Event>, account: AccountId) -> u64 {
    events
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                Event::OrderAccepted { account: a, .. } | Event::IntentRejected { account: a, .. }
                    if *a == account
            )
        })
        .count() as u64
}

fn account_ordinal(
    cursors: &crate::session::IngressReceiptCursors,
    account: AccountId,
) -> u64 {
    cursors
        .next_account_ordinal
        .get(&account)
        .copied()
        .unwrap_or(0)
}

/// 部分 receive 后失败的**精确消费窗口**（ADR-0042 §9.4 v3 共通条款）：
/// 修复重试的每一帧按实际 receipt ordinal 对账——账户 1 的执行数恰等于该
/// attempt 在失败水位之上**新分配**的 ordinal 数；无执行帧不提交新 ordinal；
/// 失败 attempt 的 gap ordinal 永不进入消费窗口（执行数==新分配数 ⇒ 任何
/// gap ordinal 的混入都会使等式破裂）。
#[test]
fn partial_receive_failure_retry_executes_exact_new_receipt_window() {
    let mut session = witness_setup(2, 23);
    // 与 partial_receive_failure 同构的前置：玩家买单进簿（失衡输入面）。
    session
        .game_mut()
        .shared_ingress()
        .enqueue_player_intent(AccountId(0), player_buy(session.game()))
        .unwrap();
    session.step_frame_pipeline_contract().unwrap();
    session.step_frame_pipeline_contract().unwrap();
    let tick = session.game().state.tick;
    force_due_game(session.game_mut(), AccountId(1), tick, 3);
    force_due_game(session.game_mut(), AccountId(2), tick, 5);
    session
        .game_mut()
        .state
        .npc_attention
        .get_mut(&AccountId(2))
        .unwrap()
        .base_probability = 0.0;
    session
        .game_mut()
        .shared_ingress()
        .enqueue_player_intent(AccountId(0), player_buy(session.game()))
        .unwrap();
    let business_before = session.game().business_state_hash().unwrap();
    let a1_authority_before = account_ordinal(&authority_cursors(session.game()), AccountId(1));

    let error = session.step_frame_pipeline_contract().expect_err("账户 2 概率非法必须使整个 tick 失败");
    assert!(
        format!("{error:?}").contains("注意力概率"),
        "实际错误: {error:?}"
    );
    // 完整业务回滚（权威业务状态逐位不变）。
    assert_eq!(
        session.game().business_state_hash().unwrap(),
        business_before,
        "失败 tick 完整回滚（业务状态不变）"
    );
    // 失败 attempt 的实际 receipt gap（共享水位 − 权威水位）。
    let shared_after_failure = shared_cursors(session.game_mut());
    let a1_shared = account_ordinal(&shared_after_failure, AccountId(1));
    let a1_authority_after_failure = account_ordinal(&authority_cursors(session.game()), AccountId(1));
    assert_eq!(
        a1_authority_after_failure, a1_authority_before,
        "失败不提交任何账户 1 的权威游标"
    );
    assert!(
        a1_shared > a1_authority_before,
        "fixture 前提：失败 attempt 已实际登记账户 1 的 intents（gap={})",
        a1_shared - a1_authority_before
    );

    // 修复（协议层失败不 poison）后重试：完整窗口逐帧对账。
    session
        .game_mut()
        .state
        .npc_attention
        .get_mut(&AccountId(2))
        .unwrap()
        .base_probability = 0.5;
    let mut executed_window: Option<u64> = None;
    for _attempt in 0..3 {
        // 帧间无新登记：登记只发生在帧内生产（失败后至本帧前共享水位不动）。
        assert_eq!(
            account_ordinal(&shared_cursors(session.game_mut()), AccountId(1)),
            a1_shared,
            "帧间无新登记（共享水位保持失败后值）"
        );
        let authority_pre = account_ordinal(&authority_cursors(session.game()), AccountId(1));
        let frame = session.step_frame_pipeline_contract().expect("修复后重试必须成功");
        let executed = admission_count(frame.events(), AccountId(1));
        let authority_post = account_ordinal(&authority_cursors(session.game()), AccountId(1));
        if executed == 0 {
            assert_eq!(
                authority_post, authority_pre,
                "无执行帧不得提交账户 1 的新 ordinal（无隐藏复活）"
            );
            continue;
        }
        // 精确消费窗口：本 attempt 新分配 ordinal 数（权威推进越过失败水位后的
        // 增量）== 执行数。任何 gap ordinal 混入执行（旧 payload 复活）或新
        // intent 未执行/重复执行都会打破等式。
        assert_eq!(
            authority_post - a1_shared,
            executed,
            "重试帧执行数==新分配 ordinal 数（gap ordinal 不入消费窗口；执行={executed} 新分配={}）",
            authority_post - a1_shared
        );
        assert!(
            authority_post > a1_shared,
            "权威游标越过失败水位（{a1_shared}→{authority_post}；不复用、不回退）"
        );
        executed_window = Some(executed);
        break;
    }
    let executed = executed_window.expect("重试窗口内必须出现实际执行帧");
    assert!(executed >= 1, "重算批真实执行（业务强度）");
    // 末态：权威游标位于失败水位之上（gap 作为永久空档被跳过，不被逐个消费）。
    assert!(
        account_ordinal(&authority_cursors(session.game()), AccountId(1)) >= a1_shared,
        "末态权威游标 ≥ 失败水位（gap 永久保留，cursor 不回退）"
    );
}

/// publication 失败的**完整业务回滚 + 双侧恰一次**（ADR-0042 §9.4 v3 共通
/// 条款）：失败帧丢弃全部私有产出（业务哈希逐位不变、权威随机流不被消耗）；
/// 重试帧外部玩家**原 receipt 恰一次受理**（失败帧无事件、后续帧不重复受理）；
/// NPC 侧按实际 receipt ordinal 精确对账（执行数==新分配数，旧 payload 的
/// gap ordinal 不入消费窗口）。
#[test]
fn publication_failure_rolls_back_business_and_executes_each_receipt_exactly_once() {
    let mut setup = crate::session::protocol::civil::publication_tests::setup();
    setup.stocks[0].float_shares = 10_000;
    setup.npcs.retail_count = 2;
    setup.npcs.inst_count = 0;
    setup.npcs.retail_cash_median = Money::from_cents(100_000_000);
    setup.strategy_params.retail.arrival_rate = 1.0;
    setup.ticks_per_day = 4;
    setup.start_date = CivilDate::from_iso("2030-01-07").unwrap();
    let mut session = ProtocolSession::new(setup, 5).unwrap();
    session
        .game_mut()
        .rebind_compute_for_verification(crate::compute::ComputeContext::with_workers(2).unwrap());
    let accounts: Vec<AccountId> = session.game().state.npc_attention.keys().copied().collect();
    for account in &accounts {
        let attention = session
            .game_mut()
            .state
            .npc_attention
            .get_mut(account)
            .unwrap();
        attention.rng_state = 3;
        attention.next_attention_candidate_tick = 0;
        session
            .game_mut()
            .state
            .attention_scheduler
            .enqueue(0, *account);
    }
    for _ in 0..4 {
        session.step_frame_pipeline_contract().unwrap();
    }
    session.end_civil_day_update().unwrap();
    // 外部玩家请求在失败前真实接收（enqueue 即登记 receipt——外部事实，
    // 不因 tick 失败丢失，成功提交仅消费一次）。
    session
        .game_mut()
        .shared_ingress()
        .enqueue_player_intent(AccountId(0), player_buy(session.game()))
        .unwrap();
    let business_before = session.game().business_state_hash().unwrap();
    let attention_before = attention_snap(session.game());
    let authority_before = authority_cursors(session.game());

    session.malformed_frame = true;
    assert!(
        session.step_frame_pipeline_contract().is_err(),
        "malformed frame 必须使 publication 失败"
    );
    // 完整业务回滚：业务哈希逐位不变；权威注意力/RNG 不被消耗（私有 patch
    // 随 candidate 整体丢弃）。
    assert_eq!(
        session.game().business_state_hash().unwrap(),
        business_before,
        "publication 失败完整回滚（业务状态不变）"
    );
    assert_eq!(
        attention_snap(session.game()),
        attention_before,
        "publication 失败不消耗权威随机流"
    );
    let authority_after_failure = authority_cursors(session.game());
    let shared_after_failure = shared_cursors(session.game_mut());
    let mut gap_total = 0u64;
    for account in &accounts {
        let shared = account_ordinal(&shared_after_failure, *account);
        let authority = account_ordinal(&authority_after_failure, *account);
        assert!(
            authority
                <= account_ordinal(&authority_before, *account),
            "失败不提交 {account:?} 的权威游标"
        );
        gap_total += shared.saturating_sub(authority);
    }
    assert!(
        gap_total >= 1,
        "fixture 前提：失败 attempt 已实际登记 NPC intents（gap 总量={gap_total}）"
    );

    // 重试：外部玩家原 receipt 恰一次；NPC 精确窗口。
    session.malformed_frame = false;
    let resumed = session.step_frame_pipeline_contract().expect("重试必须成功");
    assert_eq!(
        admission_count(resumed.events(), AccountId(0)),
        1,
        "外部玩家原 receipt 恰一次受理（重试帧恰一条受理事件）"
    );
    let authority_after_retry = authority_cursors(session.game());
    let shared_after_retry = shared_cursors(session.game_mut());
    let mut npc_admissions_total = 0u64;
    let mut npc_new_allocations_total = 0u64;
    for account in &accounts {
        let failure_level = account_ordinal(&shared_after_failure, *account);
        let authority_post = account_ordinal(&authority_after_retry, *account);
        let shared_post = account_ordinal(&shared_after_retry, *account);
        // gap 跳跃：权威游标越过失败水位（gap ordinal 留永久空档——不逐个
        // 消费、不回退、不复用；旧批 payload 的 ordinal 不入新消费窗口）。
        assert!(
            authority_post >= failure_level,
            "{account:?} 权威游标越过失败水位（{failure_level}→{authority_post}）"
        );
        // 帧完整消费：成功帧提交其全部新登记（权威==共享水位，无半提交）。
        assert_eq!(
            authority_post, shared_post,
            "{account:?} 成功帧权威游标==共享水位（完整提交）"
        );
        let new_allocations = authority_post - failure_level;
        let admissions = admission_count(resumed.events(), *account);
        // 不复活双执行：每 intent 至多一条受理类事件 ⇒ 受理数 ≤ 新分配数
        //（撤单可静默成功无事件；等式强度由部分 receive 用例的纯 placements
        // fixture 承载——本 fixture 的 NPC intents 可含撤单）。
        assert!(
            admissions <= new_allocations,
            "{account:?} 受理事件数（{admissions}）≤ 新分配 ordinal 数（{new_allocations}；旧 payload 复活双执行会打破上界）"
        );
        npc_admissions_total += admissions;
        npc_new_allocations_total += new_allocations;
    }
    assert!(
        npc_new_allocations_total >= 1,
        "重算 NPC 批真实登记（新分配 ordinal 非空——外部玩家订单进簿后本
        fixture 的 NPC intents 可全为撤单（静默成功无受理事件），NPC 受理
        非空的业务强度由无玩家注入的原用例承载）"
    );
    // 后续帧：外部玩家原 receipt 不再重复受理（恰一次=全窗口恰一次）。
    let next = session.step_frame_pipeline_contract().expect("后续帧必须成功");
    assert_eq!(
        admission_count(next.events(), AccountId(0)),
        0,
        "后续帧不重复受理外部玩家原 receipt"
    );
}
