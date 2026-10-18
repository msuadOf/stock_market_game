//! 串行脊 V0 合同测试：在**正式生产入口**（`step` / `step_frame` /
//! `step_frame_and_commit` / `step_with_commit_evidence`）钉住两树合并后的固定契约。
//!
//! 这些断言对应审查报告 V0 批次（固定合并契约）：
//! 1. publication 失败不安装任何权威状态，且会话保持可步进；
//! 2. 后轮（commit 准备段）失败完整保留 authority、RNG、ID 与 receipt/seq 游标；
//! 3. 报价过期释放可用于本 tick，且不为其他账户回补预算；
//! 4. 空 tick 也交付合法 frame。
//!
//! V1~V6 的编排重构不得弱化这里的任何断言；改动合同本身需要先改
//! `.tmp/spine/v0-contract.md` 并经独立复核。

use super::StepFatal;
use crate::{AccountId, Event, GameSession, Intent, Money, Side};

const PLAYER: AccountId = AccountId(0);

/// 有真实成交的 tick 夹具：两个 retail NPC 同价卖单 + 玩家全额买走。
/// 与 authoritative_tick_tests::single_pool 夹具同构（含 ingress 受理事实）。
fn filled_tick_session() -> (GameSession, crate::session::pipeline::NpcDecisionHandover) {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    setup.stocks[0].float_shares = 200;
    setup.float_allocation = crate::FloatAllocation::class_percentages(
        1.0,
        0.0,
        0.0,
        crate::WithinKindDistribution::EqualPercentage,
    );
    let mut session = GameSession::new(setup, 42).unwrap();
    let intents = [AccountId(1), AccountId(2)]
        .into_iter()
        .map(|account| {
            session
                .state
                .ingress_receipt_cursors
                .receive(
                    account,
                    Intent::PlaceLimit {
                        code: crate::StockId(0),
                        side: Side::Sell,
                        price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                        qty: 100,
                    },
                )
                .unwrap()
        })
        .collect();
    let batch = crate::session::pipeline::NpcDecisionHandover::new(
        0,
        vec![AccountId(1), AccountId(2)],
        intents,
        Vec::new(),
    );
    crate::session::pipeline::staged_decision::stage(batch.clone());
    session
        .enqueue_player_intent(
            PLAYER,
            Intent::PlaceLimit {
                code: crate::StockId(0),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 200,
            },
        )
        .unwrap();
    (session, batch)
}

/// 权威状态指纹：业务哈希 + 显式游标（tick / seq / next_order_id / receipt base / seed）。
/// 哈希覆盖全部业务状态；游标单独列出是因为它们是跨 tick 唯一性的事实来源。
struct AuthorityFingerprint {
    hash: crate::session::StateHash,
    tick: u64,
    seq: u64,
    next_order_id: u64,
    next_receipt_base: u64,
    seed: u64,
}

fn fingerprint(session: &GameSession) -> AuthorityFingerprint {
    AuthorityFingerprint {
        hash: session.business_state_hash().unwrap(),
        tick: session.state.tick,
        seq: session.seq(),
        next_order_id: session.state.next_order_id,
        next_receipt_base: session.state.next_receipt_base,
        seed: session.state.seed,
    }
}

fn assert_unchanged(session: &GameSession, before: &AuthorityFingerprint, context: &str) {
    assert_eq!(
        session.business_state_hash().unwrap(),
        before.hash,
        "{context}"
    );
    assert_eq!(session.state.tick, before.tick, "{context}");
    assert_eq!(session.seq(), before.seq, "{context}");
    assert_eq!(
        session.state.next_order_id, before.next_order_id,
        "{context}"
    );
    assert_eq!(
        session.state.next_receipt_base, before.next_receipt_base,
        "{context}"
    );
    assert_eq!(session.state.seed, before.seed, "{context}");
}

/// 合同 1：publication（提交门前投影）失败时，整 tick 丢弃，不安装任何权威状态。
/// 失败发生在唯一安装入口之前；会话未被毒化，下一次同入口调用照常完成并交付 frame。
#[test]
fn v0_publication_failure_installs_nothing_and_keeps_session_steppable() {
    let (mut session, batch) = filled_tick_session();
    let before = fingerprint(&session);

    let error = session
        .step_frame_and_commit(false, |_, _| {
            Err::<crate::session::protocol::TickFrame, _>(StepFatal::InvariantViolation {
                description: "V0 合同注入：publication 失败".to_owned(),
                location: "spine_v0_contract_tests".to_owned(),
            })
        })
        .err()
        .expect("publication 失败必须使整个 tick 失败");

    assert!(
        matches!(error, StepFatal::InvariantViolation { .. }),
        "注入的失败必须原样返回：{error:?}"
    );
    assert_unchanged(&session, &before, "publication 失败不得安装任何权威状态");

    // 批四等价改接：staged 批为 thread-local，随失败 attempt 的 produce 消费
    //（旧形态=authority pending 不随候选丢弃丢失）——重试前由 fixture 重放
    // 同批（失败不消费权威 attention/RNG ⇒ 重放批与首次生产逐位同构）。
    crate::session::pipeline::staged_decision::stage(batch);

    // 同一条生产入口的下一次调用必须照常完成：失败的尝试没有留下半安装状态。
    let frame = session
        .step_frame()
        .expect("publication 失败不得破坏会话可用性");
    assert_eq!(frame.tick, before.tick + 1);
    assert!(frame.validate().is_ok());
    assert!(
        frame
            .facts
            .iter()
            .any(|fact| matches!(fact.event, Event::Trade { .. })),
        "重放的成交 tick 必须重新产生真实成交事件"
    );
}

/// 合同 2：后轮（commit 准备段，candidate_commit 注入点）失败时，authority、
/// RNG 种子、订单/收据 ID 游标与事件 seq 游标全部保持失败前的值。
/// 注入点在 shadow 工作完成之后，验证的是“晚失败不得泄漏任何已计算状态”。
#[test]
fn v0_late_round_failure_preserves_authority_rng_ids_and_cursors() {
    let (mut session, _batch) = filled_tick_session();
    session
        .step()
        .expect("前置 tick 必须成功，建立非平凡权威状态");
    let before = fingerprint(&session);

    session.inject_post_shadow_failure(StepFatal::InvariantViolation {
        description: "V0 合同注入：后轮失败".to_owned(),
        location: "spine_v0_contract_tests".to_owned(),
    });
    assert!(session.step().is_err(), "后轮失败必须使整个 tick 失败");
    assert_unchanged(
        &session,
        &before,
        "后轮失败必须完整保留 authority/RNG/ID/receipt 与 seq 游标",
    );
}

/// 混合夹具：一条同分钟过期的 NPC 买单 + 有持仓的对手卖单 + 玩家全额买单。
/// 与 commit_evidence_tests::mixed_quote_expiry_and_sealed_receipt_session 同构，
/// 但这里经由正式入口 step_with_commit_evidence 驱动。
/// 资源截点加强版（Astra P2 重写）：同一 inst 账户还挂有一笔**不过期**的在途买单
/// （seal 时冻结其现金），并在本 tick 提交 [撤销该买单, 新的大额买单]。
/// seal 后该账户将经历两种资源释放：撤销导致的冻结解除 + 对手方成交回款；
/// 断言这些释放都不得为其同 tick 的新申报回补预算。
/// 玩家（他人账户视角）的第二笔买单金额落在临界窗内（round3 ③ 建窗、
/// round4 ③ 数值修正为实际受理口径）：买单受理预留 = 价款 + 佣金（小额
/// 触发下限 500 分）+ 过户费（0.01‰ 银行家舍入），首单 100×1000 实际
/// 预留 100,501 分（价款 100,000 + 500 + 1），在同一 tick 的账户校验中
/// 先被受理并占用预算，剩余可用 B=49,499 分（现金 150,000 − 100,501）；
/// 第二单需 99,501 分（价款 99,000 + 500 + 1——价款 99,000 **不是**受理
/// 阈值，阈值含费用）；临界窗取 (B, B+任一笔本 tick 他人释放额] =
/// (49,499, 148,000]（撤单冻结解除 98,501 / 过期释放 99,501 / 成交回款
/// ≈100,000，取最小 98,501 加 B）——只有错误回补才会让窗内申报变为
/// 可受理，负例具有区分力（见合同 3 注释）。
fn expiring_quote_and_fill_session() -> (GameSession, crate::OrderId, AccountId) {
    let code = crate::StockId(0);
    let account = AccountId(1);
    let mut session =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 51).unwrap();
    session
        .fixture_account_mut(&account)
        .unwrap()
        .fixture_set_strategy(None);
    let mut events = Vec::new();
    session.seed_order_for_test(
        account,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(990)),
            qty: 100,
        },
        &mut events,
    );
    assert!(events
        .iter()
        .any(|event| matches!(event, crate::Event::OrderAccepted { .. })));
    // 本分钟到期：ExpiryShadow 在冻结预算前先释放该报价的占用。
    session.state.npc_order_lifecycles[0].expires_market_minute = session.current_market_minute();
    // 与 anchor 夹具一致：策略从同 seed 源会话搬入，保证 inst 观察路径可用。
    let strategy_source =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 51).unwrap();
    session
        .fixture_account_mut(&account)
        .unwrap()
        .fixture_set_strategy(strategy_source.state.accounts[&account].strategy().cloned());

    // 同账户截点压力由一个 Player 账户承载：它同时是对手卖方（成交回款）
    // 与在途买单持有人（撤销冻结解除）。
    let releasing = AccountId(2);
    let seller = crate::Account::new(
        releasing,
        crate::AccountKind::Player,
        Money::from_cents(10_000_000),
    );
    session.state.accounts.insert(releasing, seller);
    session
        .fixture_account_mut(&releasing)
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(100_000))
        .unwrap();
    let mut setup_events = Vec::new();
    session.seed_order_for_test(
        releasing,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
        &mut setup_events,
    );
    assert!(setup_events.iter().any(
        |event| matches!(event, crate::Event::OrderAccepted { account, .. } if *account == releasing)
    ));
    // 不过期的在途买单：seal 时冻结现金，构成预算截点的一部分。
    let mut working = Vec::new();
    session.seed_order_for_test(
        releasing,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(980)),
            qty: 100,
        },
        &mut working,
    );
    let working_order = working
        .iter()
        .find_map(|event| match event {
            crate::Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .expect("在途买单必须已入簿");
    // 现金压到只够覆盖冻结 + 少量余量：seal 后同账户可用 = 余量（远小于新买单）。
    // 卖单冻结的是股份不是现金，故冻结额只含在途买单。
    let frozen = session.reserved_cash_for_account(releasing).unwrap();
    session
        .fixture_account_mut(&releasing)
        .unwrap()
        .fixture_set_cash(frozen.add(Money::from_cents(20_000)).unwrap());

    session
        .enqueue_player_intent(
            PLAYER,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        )
        .unwrap();
    // 同账户的本 tick 新申报：先撤销在途买单（冻结解除），再提交大额买单。
    session
        .enqueue_player_intent(
            releasing,
            Intent::Cancel {
                code: code.clone(),
                id: working_order,
            },
        )
        .unwrap();
    session
        .enqueue_player_intent(
            releasing,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                qty: 100,
            },
        )
        .unwrap();
    (session, working_order, releasing)
}

/// 合同 3（Astra P2 重写，本轮收紧前提与声明）：报价过期释放属于本 tick
/// （PreSeal 前缀先于冻结预算，随后被同一 tick 的收据链消费）；预算截点
/// （DecisionResourceSnapshot seal）之后同一账户发生的资源释放——撤销在途
/// 买单的冻结解除、对手方成交回款——本 tick 内不得回补该账户的新申报预算；
/// 他人账户的释放同样不得回补玩家预算。
///
/// 声明边界（不得扩大；round3 ③ 建窗 B<R≤B+L，round4 ③ 数值修正为实际
/// 受理口径——预留含最低佣金 500 分与过户费）：
/// - 同账户断言由变异红实证区分（seal 忽略封冻保留时新申报被错误受理）；
///   它对“任何让 seal 后资金对新申报可见的机制”都是临界拒绝（可用 20_000 分
///   < 申报所需 99_501 分，任何一笔释放流入都会翻转受理结论），但不区分
///   撤单冻结解除与成交回款**具体是哪一条**机制出错。
/// - 他人（玩家）断言的金额落在临界窗（B=剩余可用 49_499 分 = 现金
///   150_000 减首单实际预留 100_501；R=99_501 分 ∈ (B, B+最小单笔他人
///   释放 98_501] = (49_499, 148_000]）：正确截点必拒，任何一笔他人释放
///   被错误回补才够。该区分力是构造论证（窗口算术），变异红只覆盖同账户
///   封冻保留语义（round3 复核：freeze 忽略变异下玩家断言仍绿——首单在
///   同 tick 账户校验中先行占用预算，该变异不触玩家窗口）。
#[test]
fn v0_quote_expiry_is_usable_in_tick_and_post_seal_releases_never_replenish_budgets() {
    let (mut session, working_order, releasing_account) = expiring_quote_and_fill_session();
    // 费用口径前提（round4 ③ 钉住）：买单受理预留 = 价款 + max(佣金率×价款,
    // 下限 500 分) + 过户费(价款×0.01‰)。首单 100×1000 → 100,501；
    // 第二单 100×990 → 99,501（价款 99,000 非受理阈值）；在途买单
    // 100×980 的冻结 → 98,501（撤单冻结解除的释放额）。
    let config = session.state.setup.config.clone();
    assert_eq!(
        crate::config::buy_order_reservation(&config, Money::from_cents(1_000), 100, Money::ZERO)
            .unwrap()
            .cents(),
        100_501,
        "首单实际预留必须为价款 100,000+佣金 500+过户费 1（round4 ③）"
    );
    assert_eq!(
        crate::config::buy_order_reservation(&config, Money::from_cents(990), 100, Money::ZERO)
            .unwrap()
            .cents(),
        99_501,
        "第二单受理阈值必须为价款 99,000+佣金 500+过户费 1（99,000 是价款非阈值）"
    );
    assert_eq!(
        crate::config::buy_order_reservation(&config, Money::from_cents(980), 100, Money::ZERO)
            .unwrap()
            .cents(),
        98_501,
        "在途买单冻结必须为价款 98,000+佣金 500+过户费 1（撤单释放额 L）"
    );
    let sealed_available_cents = session
        .state
        .accounts
        .get(&releasing_account)
        .map(|account| account.cash().cents())
        .unwrap()
        - session
            .reserved_cash_for_account(releasing_account)
            .unwrap()
            .cents();
    let cash_at_fixture = session.state.accounts[&releasing_account].cash();
    // 把玩家现金压到 150_000 分：首单 100×1000 实际预留 100_501 分（价款
    // 100_000+佣金 500+过户费 1）在同一 tick 的账户校验中先被受理并占用
    // 预算，第二笔买单决策时剩余可用 B=49_499 分。他人（本例为同一 inst
    // 账户）的过期释放不得回补玩家预算。
    session
        .fixture_account_mut(&PLAYER)
        .unwrap()
        .fixture_set_cash(Money::from_cents(150_000));
    // 夹具前提钉住玩家预算算术（round3 ③ 建窗、round4 ③ 修正费用口径）：
    // seal 时玩家无在途冻结（两笔意图尚未入簿，封冻基线=现金 150_000 分）；
    // 首单实际预留 100_501 分占用后剩余可用 B=49_499 分是临界窗的预算起点，
    // 不得再用封冻前的全额现金当起点。
    let player_sealed_cents = session
        .state
        .accounts
        .get(&PLAYER)
        .map(|account| account.cash().cents())
        .unwrap()
        - session.reserved_cash_for_account(PLAYER).unwrap().cents();
    assert_eq!(
        player_sealed_cents, 150_000,
        "夹具前提必须钉住玩家封冻基线（现金 150_000 分、无在途冻结）：{player_sealed_cents}"
    );
    session
        .enqueue_player_intent(
            PLAYER,
            Intent::PlaceLimit {
                code: crate::StockId(0),
                side: Side::Buy,
                // 价格仍在笼内（98%~102%）且低于对手卖价，不与卖单成交，隔离验证预算判定。
                // 金额取临界窗（round3 ③ 建窗 B<R≤B+L、L=任一笔本 tick 他人
                // 释放额；round4 ③ 数值修正为实际受理口径，预留=价款+最低佣金
                // 500 分+过户费）：第二单需 R=99_501 分（价款 99_000+500+1，
                // **价款 99,000 不是受理阈值**），严格大于玩家剩余可用预算
                // （B=49_499 分 = 现金 150_000 − 首单实际预留 100_501），
                // 又不超过 B+任一笔他人释放额（撤单冻结解除 98_501 / 过期释放
                // 99_501 / 成交回款 ≈100_000 分，最小 98_501 → 上界 148_000）：
                // 正确截点必拒（99_501 > 49_499），而任何一笔释放被错误回补
                // 都会让它变为可受理（99_501 ≤ 148_000）。round2 的
                // 200×990=198_000 分起点错误：超出上界，单笔错误回补后仍必拒，
                // 负例对“单笔回补”没有区分力；round3 的 B=50,000/R=99,000
                // 漏算了最低佣金与过户费，round4 修正。
                price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                qty: 100,
            },
        )
        .unwrap();

    let (events, evidence) = session
        .step_with_commit_evidence()
        .expect("混合过期+成交 tick 必须经正式入口成功");

    // 过期释放与成交收据在同一条全局收据链：PreSeal 前缀在前，SealedBatch 在后。
    let journals: Vec<_> = evidence
        .receipts()
        .iter()
        .map(|receipt| (receipt.index, receipt.local_key.journal()))
        .collect();
    assert_eq!(
        evidence.expiry_receipts().len(),
        1,
        "必须恰好一条过期释放收据"
    );
    assert_eq!(
        journals.first().map(|(_, journal)| *journal),
        Some(super::JournalRank::PreSeal)
    );
    assert!(journals
        .iter()
        .skip(1)
        .all(|(_, journal)| *journal == super::JournalRank::SealedBatch));
    for (position, (index, _)) in journals.iter().enumerate() {
        assert_eq!(*index, position as u64, "收据 index 必须全局连续");
    }
    assert_eq!(
        session.state.next_receipt_base,
        evidence.next_receipt_index(),
        "会话 receipt 游标必须与收据链尾对齐"
    );

    // 过期释放可供本 tick 使用：对手卖单与玩家买单真实成交。
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::Trade { qty, .. } if *qty == 100)));
    // 不回补：临界窗买单（需 99_501 分 ∈ (玩家剩余可用 49_499 分,
    // 49_499+任一笔他人释放额 148_000 分]）在账户校验处被拒——任何一笔
    // 他人释放（过期 99_501/撤单冻结解除 98_501/成交回款 ≈100_000）被错误
    // 回补都会使其可受理（99_501 ≤ 148_000），负例对每一条单笔回补机制
    // 都有区分力。
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::IntentRejected {
                account,
                reason: crate::RejectionReason::InsufficientCash,
                ..
            } if *account == PLAYER
        )),
        "他人本 tick 的任何释放（过期/撤单/成交）不得回补玩家预算：{events:?}"
    );

    // —— 同账户 seal 后释放不回补（原 AllocationSnapshot 语义，第一断言）——
    // 释放确实发生：在途买单被撤销（冻结解除），且对手方成交回款已落地（现金增加）。
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::OrderCanceled { id, .. } if *id == working_order
        )),
        "同账户撤销必须真实执行（冻结解除事实）：{events:?}"
    );
    assert!(
        session.state.accounts[&releasing_account].cash().cents()
            > cash_at_fixture.cents() + 50_000,
        "对手方成交回款必须在本 tick 落地（释放事实）：after={:?} before={:?}",
        session.state.accounts[&releasing_account].cash(),
        cash_at_fixture,
    );
    // 封冻快照仍然约束同账户新申报：seal 时可用预算必须等于夹具构造值
    // （现金 = 冻结额 + 20_000 分），且严格小于新申报所需预算（受理口径
    // 99_501 分 = 价款 99_000 + 佣金 500 + 过户费 1——价款本身不是阈值，
    // round4 ③ 声明区分）。即便撤销冻结解除与成交回款都已发生，该申报
    // 也必须按封冻快照被拒。
    assert_eq!(
        sealed_available_cents, 20_000,
        "夹具前提必须钉住构造值（冻结额+20_000 分）：{sealed_available_cents}"
    );
    assert!(
        sealed_available_cents < 99_501,
        "封冻可用预算必须严格小于新申报受理阈值（价款 99_000+佣金 500+过户费 1=99_501 分）"
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            Event::IntentRejected {
                account,
                reason: crate::RejectionReason::InsufficientCash,
                ..
            } if *account == releasing_account
        )),
        "同账户 seal 后的冻结解除与成交回款不得回补其本 tick 新申报预算：{events:?}"
    );
}

/// 合同 4：没有任何交易活动的空 tick 也必须交付一个合法 frame（不报错、不跳过）。
#[test]
fn v0_empty_tick_still_delivers_a_valid_frame() {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(900);
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    let mut session = GameSession::new(setup, 0x5EED).unwrap();

    let frame = session
        .step_frame()
        .expect("空 tick 必须照常交付 frame，而不是报错或跳过");
    assert_eq!(frame.tick, 1, "空 tick 也要推进 tick 计数");
    assert!(frame.seq_to >= frame.seq_from);
    assert!(
        frame
            .facts
            .iter()
            .all(|fact| !matches!(fact.event, Event::Trade { .. })),
        "空 tick 不得包含成交事件"
    );
    assert!(frame.validate().is_ok(), "空 frame 必须通过协议校验");
}
