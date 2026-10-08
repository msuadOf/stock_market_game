//! 发行人回购 tick 事务探针（2026-10-07 诊断轮，仅测试，不改生产行为）。
//!
//! 目标：用真实最小用例回答疑问，而不是按静态调用位置推断缺陷：
//! 1. `place_issuer_repurchase_orders` 在 `execute_authoritative_tick` 之前直接写
//!    权威状态（pending_player、本地受理游标、`last_order_day`）——随后 tick 失败
//!    时这些写入是否残留（违反 ADR-0017 §4「失败 tick 权威业务状态不变」）。
//! 2. 绑定共享 ingress（真实宿主 `shared_ingress()` 路径）时，回购回执走本地游标
//!    `state.ingress_receipt_cursors.receive`，而玩家/NPC 走共享 source——同一
//!    stock 域 ordinal 是否碰撞；tick 冻结（freeze 覆盖本地游标）是否丢失本地
//!    回执的 ordinal 消耗事实（ADR-0032 receipt 身份契约）；碰撞是否在真实
//!    `step()` 中触发 `conflicting requests share a stock receipt` StepFatal。
//! 3. 回购委托部分成交留挂单（合法场景）是否在 NPC 生命周期投影处 panic；
//!    窗口中途按合法日终契约保存/恢复后，同日去重是否保持。
//!
//! 断言口径：失败 tick 不允许部分业务提交；但外部已合法接收的玩家输入事实
//! （共享 source 里的 receipt）与 poison 元数据不要求回滚。撮合全部走真实
//! `step()`，不伪造成交。

use super::*;
use crate::account::Position;
use crate::accounting::AccountingAmount;
use crate::company::issuer_repurchase::{IssuerRepurchasePlan, RepurchasePurpose};
use crate::company::share_registry::{
    AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
};
use crate::money::Money;
use crate::orderbook::{AccountId, Side};
use crate::strategy::{Intent, LimitPrice};
use crate::calendar::CivilDate;

/// 窗口首日（既有 `issuer_repurchase_session_tests` 同一合法方案口径）。
const WINDOW_FIRST_DAY: &str = "2030-01-03";

fn probe_setup() -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.ticks_per_day = 3;
    setup.start_date = CivilDate::from_iso("2030-01-02").unwrap();
    setup.issuer_repurchase_enabled = true;
    setup
}

fn fixture_lot(id: &str, qty: u64, acquired_on: CivilDate) -> ShareLot {
    ShareLot {
        id: id.into(),
        qty,
        acquired_on,
        source: AcquisitionSource::InitialAllocation {
            evidence: "repurchase transaction probe fixture".into(),
        },
        restriction: ShareRestriction::Unrestricted,
    }
}

/// 与既有回购 Session 测试同构的最小合法局：1 只股、玩家 + 回购专户两个账户、
/// 真实名册与分红法定事实。返回 (session, stock, issuer, 回购账户)。
fn probe_session_with_player_shares(
    player_shares: u64,
) -> (GameSession, StockCode, CompanyId, AccountId) {
    let mut session = GameSession::new(probe_setup(), 42).unwrap();
    let stock = session.state.setup.stocks[0].code.clone();
    let issuer = session
        .state
        .company_system
        .issuers()
        .issuer_of(&stock)
        .unwrap()
        .clone();
    let total_shares = session.state.setup.stocks[0].total_shares;
    let date = CivilDate::from_iso("2030-01-02").unwrap();
    session
        .state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .fixture_insert_position(
            stock.clone(),
            Position::from_restored_parts(
                player_shares.try_into().unwrap(),
                0,
                (player_shares * 1_000).try_into().unwrap(),
                0,
            ),
        );
    let holdings = vec![
        ShareHolding {
            holder: HolderId::Account(AccountId(0)),
            lots: vec![fixture_lot("player-lot", player_shares, date)],
        },
        ShareHolding {
            holder: HolderId::IssuerTreasury,
            lots: vec![fixture_lot("treasury-lot", total_shares - player_shares, date)],
        },
    ];
    let registry = ShareRegistry::new(stock.clone(), issuer.clone(), total_shares, date, holdings)
        .unwrap();
    session.configure_share_registry(registry).unwrap();
    session
        .define_dividend_legal_facts(
            &issuer,
            AccountingAmount::from_cents((total_shares / 10 * 100).try_into().unwrap()),
            "repurchase transaction probe legal fact".into(),
        )
        .unwrap();
    let repurchase_account = AccountId(1);
    (session, stock, issuer, repurchase_account)
}

fn probe_session() -> (GameSession, StockCode, CompanyId, AccountId) {
    probe_session_with_player_shares(1_000)
}

fn probe_plan(issuer: &CompanyId, stock: &StockCode) -> IssuerRepurchasePlan {
    IssuerRepurchasePlan {
        event_id: "repurchase-probe-2030".into(),
        approval_reference: "board-repurchase-probe-2030".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
        announced_on: CivilDate::from_iso("2030-01-02").unwrap(),
        window_start_on: CivilDate::from_iso(WINDOW_FIRST_DAY).unwrap(),
        window_deadline_on: CivilDate::from_iso("2030-01-07").unwrap(),
        price_cap_per_share: Money::from_cents(1_100),
        total_budget: Money::from_cents(5_000_000),
        max_shares: 100,
        purpose: RepurchasePurpose::ReduceCapital,
        completion_policy:
            crate::company::issuer_repurchase::RepurchaseCompletionPolicy::CancelOnCompletion,
    }
}

fn sell_intent(stock: &StockCode, qty: u32) -> Intent {
    Intent::PlaceLimit {
        code: stock.clone(),
        side: Side::Sell,
        price: LimitPrice::Fixed(Money::from_cents(1_000)),
        qty,
    }
}

fn buy_intent(stock: &StockCode, price_cents: i64, qty: u32) -> Intent {
    Intent::PlaceLimit {
        code: stock.clone(),
        side: Side::Buy,
        price: LimitPrice::Fixed(Money::from_cents(price_cents)),
        qty,
    }
}

/// 推进到窗口首日开盘（复用既有测试的双时钟推进方式）。
fn advance_to_window_first_day(session: &mut GameSession) {
    while session.civil_date() < CivilDate::from_iso(WINDOW_FIRST_DAY).unwrap() {
        if session.civil_clock().phase() != CivilPhase::ClosedDay {
            for _ in 0..session.state.setup.ticks_per_day {
                session.step().unwrap();
            }
        }
        session.end_civil_day().unwrap();
    }
}

fn probe_fatal(detail: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: format!("repurchase transaction probe: {detail}"),
        location: "issuer_repurchase_transaction_probe_tests".into(),
    }
}

fn repurchase_book<'a>(session: &'a GameSession, stock: &StockCode) -> &'a crate::company::issuer_repurchase::IssuerRepurchaseBook {
    session
        .state
        .corporate_actions
        .issuer_repurchases
        .iter()
        .find(|book| book.plan().stock == *stock)
        .expect("probe fixture must keep the repurchase book")
}

fn account_cash(session: &GameSession, account: AccountId) -> i64 {
    session
        .state
        .accounts
        .get(&account)
        .unwrap()
        .cash()
        .cents()
}

fn trade_between(events: &[Event], left: AccountId, right: AccountId) -> Vec<u32> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Trade {
                maker, taker, qty, ..
            } if (*maker == left && *taker == right) || (*maker == right && *taker == left) => {
                Some(*qty)
            }
            _ => None,
        })
        .collect()
}

/// 探针 1（复现预期）：生成之后、commit 之前失败的 tick，回购执行器在
/// `execute_authoritative_tick` 之前直接写入的权威状态（pending_player 回执、
/// 本地受理游标消耗、last_order_day）必须全部不残留——ADR-0017 §4 失败原子性
/// 与 `business_state_hash` 失败不变契约。先打印观察到的残留值，再按契约断言。
#[test]
fn post_shadow_failure_leaks_repurchase_placement_into_authority() {
    let (mut session, stock, issuer, repurchase_account) = probe_session();
    session
        .approve_issuer_repurchase(probe_plan(&issuer, &stock))
        .unwrap();
    advance_to_window_first_day(&mut session);
    // 未绑定共享 ingress：玩家输入走本地受理（与既有回购测试同路径）。
    session
        .enqueue_player_intent(AccountId(0), sell_intent(&stock, 100))
        .unwrap();
    let business_before = session.business_state_hash().unwrap();
    let session_before = session.session_state_hash().unwrap();
    assert_eq!(repurchase_book(&session, &stock).last_order_day(), None);
    let cursor_before = session
        .state
        .ingress_receipt_cursors
        .next_stock_ordinal
        .get(&stock)
        .copied()
        .unwrap_or(0);
    let repurchase_cash_before = account_cash(&session, repurchase_account);

    let fatal = probe_fatal("post-shadow failure after repurchase placement");
    session.inject_post_shadow_failure(fatal.clone());
    assert_eq!(session.step().unwrap_err(), fatal);

    // 观察值（缺陷证据，输出到红灯日志）：失败 tick 后权威状态的实际残留。
    let leaked_last_order_day = repurchase_book(&session, &stock).last_order_day();
    let leaked_receipt_count = session
        .state
        .pending_player
        .iter()
        .filter(|receipt| receipt.owner == repurchase_account)
        .count();
    let leaked_receipt_detail = session
        .state
        .pending_player
        .iter()
        .filter(|receipt| receipt.owner == repurchase_account)
        .map(|receipt| {
            format!(
                "(account_ordinal={}, stock_ordinal={})",
                receipt.account_ordinal, receipt.stock_ordinal
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let cursor_after = session
        .state
        .ingress_receipt_cursors
        .next_stock_ordinal
        .get(&stock)
        .copied()
        .unwrap_or(0);
    let player_receipt_kept = session
        .state
        .pending_player
        .iter()
        .any(|receipt| receipt.owner == AccountId(0));
    let repurchase_cash_after = account_cash(&session, repurchase_account);
    let business_after = session.business_state_hash().unwrap();
    println!(
        "probe1 observed: last_order_day={leaked_last_order_day:?} leaked_receipts={leaked_receipt_count}[{leaked_receipt_detail}] stock_cursor {cursor_before}->{cursor_after} player_receipt_kept={player_receipt_kept} repurchase_cash {repurchase_cash_before}->{repurchase_cash_after} business_hash_changed={} session_hash_changed={}",
        business_after != business_before,
        session.session_state_hash().unwrap() != session_before,
    );

    // 契约断言（当前实现预期红灯）。
    assert_eq!(
        business_after, business_before,
        "失败 tick 的权威业务状态必须不变（ADR-0017 §4）"
    );
    assert_eq!(
        leaked_last_order_day, None,
        "失败 tick 不得提前写入 last_order_day"
    );
    assert_eq!(
        leaked_receipt_count, 0,
        "失败 tick 不得在权威 pending_player 留下回购回执"
    );
    assert_eq!(
        cursor_after, cursor_before,
        "失败 tick 不得消耗本地受理游标"
    );
    // poison 契约（现状即如此，非缺陷）：重复 step/save 显式失败。
    assert_eq!(session.step().unwrap_err(), fatal);
    assert_eq!(session.save().unwrap_err(), fatal);
}

/// 探针 1 对照组：同 fixture，注入点在回购放置之前（pre-mutation hook），
/// 失败 tick 权威业务状态保持不变——证明残留来自「放置先于事务提交」本身。
#[test]
fn pre_mutation_failure_control_keeps_business_state_clean() {
    let (mut session, stock, issuer, repurchase_account) = probe_session();
    session
        .approve_issuer_repurchase(probe_plan(&issuer, &stock))
        .unwrap();
    advance_to_window_first_day(&mut session);
    session
        .enqueue_player_intent(AccountId(0), sell_intent(&stock, 100))
        .unwrap();
    let business_before = session.business_state_hash().unwrap();
    let fatal = probe_fatal("pre-mutation control failure");
    session.inject_step_failure(fatal.clone());
    assert_eq!(session.step().unwrap_err(), fatal);
    assert_eq!(
        session.business_state_hash().unwrap(),
        business_before,
        "对照：放置未发生时失败 tick 不改变权威业务状态"
    );
    assert_eq!(repurchase_book(&session, &stock).last_order_day(), None);
    assert!(
        session
            .state
            .pending_player
            .iter()
            .all(|receipt| receipt.owner != repurchase_account)
    );
}

/// 探针 2a：真实宿主绑定共享 ingress 后，回购回执（本地游标）与玩家回执
/// （共享 source）在同一 stock 域撞 ordinal；提交后冻结用 source 游标覆盖本地，
/// 本地回执的 ordinal 消耗事实从权威游标中消失。
#[test]
fn bound_ingress_repurchase_receipt_collides_and_freeze_drops_local_consumption() {
    let (mut session, stock, issuer, repurchase_account) = probe_session();
    session
        .approve_issuer_repurchase(probe_plan(&issuer, &stock))
        .unwrap();
    // 与 Server/Desktop 真实宿主一致：会话建立即绑定共享收件入口。
    let source = session.shared_ingress();
    advance_to_window_first_day(&mut session);
    source
        .enqueue_player_intent(AccountId(0), sell_intent(&stock, 100))
        .unwrap();
    let player_receipts = source.recorded_player_inputs().unwrap();
    assert_eq!(player_receipts.len(), 1);
    let player_stock_ordinal = player_receipts[0].stock_ordinal;

    // 生产执行器入口（step_inner 同一函数）：在真实 step 之前单独调用以便观察回执。
    session.place_issuer_repurchase_orders().unwrap();
    let repurchase_receipts: Vec<_> = session
        .state
        .pending_player
        .iter()
        .filter(|receipt| receipt.owner == repurchase_account)
        .collect();
    assert_eq!(repurchase_receipts.len(), 1, "执行器必须已生成回购委托");
    let repurchase_stock_ordinal = repurchase_receipts[0].stock_ordinal;
    let repurchase_account_ordinal = repurchase_receipts[0].account_ordinal;
    println!(
        "probe2a observed: player(stock_ordinal={player_stock_ordinal}) vs repurchase(stock_ordinal={repurchase_stock_ordinal}, account_ordinal={repurchase_account_ordinal})"
    );
    // 契约断言（ADR-0032：同一 receive 事实产生不冲突的局部 ordinal；当前预期红灯）。
    assert_ne!(
        repurchase_stock_ordinal, player_stock_ordinal,
        "本地回购回执不得与共享 source 已接受输入撞同一 stock ordinal"
    );

    // 真实 step 完成撮合（不伪造成交）。
    let player_cash_before = account_cash(&session, AccountId(0));
    let repurchase_cash_before = account_cash(&session, repurchase_account);
    let events = session.step().unwrap();
    let trades = trade_between(&events, AccountId(0), repurchase_account);
    assert_eq!(trades, vec![100], "回购买单必须与玩家卖单真实撮合");
    assert!(account_cash(&session, AccountId(0)) > player_cash_before);
    assert!(account_cash(&session, repurchase_account) < repurchase_cash_before);
    assert!(
        session
            .state
            .markets
            .get(&stock)
            .unwrap()
            .resting_orders_for(repurchase_account)
            .is_empty(),
        "全额成交后回购账户不留挂单"
    );
    // 冻结覆盖证据：提交后的权威游标应覆盖本 tick 全部真实 receive 消耗
    // （玩家 0 与回购 1 → next 2）；当前实现被 source 冻结快照覆盖为 1，
    // 本地回购回执的 ordinal 消耗从权威游标中消失（打印观察后按契约断言）。
    let committed_stock_cursor = session
        .state
        .ingress_receipt_cursors
        .next_stock_ordinal
        .get(&stock)
        .copied()
        .unwrap_or(0);
    println!(
        "probe2a observed: committed stock cursor={committed_stock_cursor} (player consumed {player_stock_ordinal}, repurchase consumed {repurchase_stock_ordinal})"
    );
    assert_eq!(
        committed_stock_cursor,
        player_stock_ordinal + 2,
        "提交游标必须计入玩家与回购两笔真实 receive 消耗"
    );
    let confirmations = session
        .state
        .personal_trade_confirmations
        .get(&repurchase_account)
        .map(|list| list.len())
        .unwrap_or(0);
    assert_eq!(confirmations, 1);
    assert_eq!(
        session
            .state
            .personal_trade_confirmations
            .get(&repurchase_account)
            .and_then(|list| list.get(0))
            .map(|confirmation| confirmation.quantity_shares),
        Some(100)
    );
}

/// 探针 2b：纯真实 step 路径 + NPC 代表输入（权威私人受理路径）。
/// 该路径下权威本地游标先被私人受理推进，回购回执恰好不撞 ordinal——
/// 证明碰撞是「source 已接受输入 + 本地游标未同步」的时序问题，不是必然。
/// 核对：合法请求不丢失、真实成交、同日跨 tick 不重复生成回购委托。
#[test]
fn bound_ingress_real_step_with_private_receive_keeps_inputs_and_dedup() {
    let (mut session, stock, issuer, repurchase_account) = probe_session();
    session
        .state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .fixture_set_cash(Money::from_cents(10_000_000));
    session
        .approve_issuer_repurchase(probe_plan(&issuer, &stock))
        .unwrap();
    let source = session.shared_ingress();
    advance_to_window_first_day(&mut session);
    source
        .enqueue_player_intent(AccountId(0), sell_intent(&stock, 100))
        .unwrap();
    // NPC 代表输入：走权威私人受理（共享 source.receive_private），
    // 按已提交批契约挂入下一 tick（与 shared_ingress 既有测试同一模式）。
    let npc_receipt = session
        .receive_private_intent(AccountId(0), buy_intent(&stock, 990, 100))
        .unwrap();
    println!(
        "probe2b observed: player(stock_ordinal=0) npc(stock_ordinal={}) local_cursor_now={}",
        npc_receipt.stock_ordinal,
        session
            .state
            .ingress_receipt_cursors
            .next_stock_ordinal
            .get(&stock)
            .copied()
            .unwrap_or(0),
    );
    session.state.pending_npc = Some(PendingNpcBatch {
        observed_tick: session.tick(),
        observed_accounts: vec![AccountId(0)],
        intents: vec![npc_receipt],
        dependencies: Vec::new(),
    });

    let player_cash_before = account_cash(&session, AccountId(0));
    let mut repurchase_trades = Vec::new();
    let mut player_rejections = 0usize;
    let mut first_step_resting_buy = None;
    for tick in 0..session.state.setup.ticks_per_day {
        let events = session.step().unwrap();
        repurchase_trades.extend(trade_between(&events, AccountId(0), repurchase_account));
        player_rejections += events
            .iter()
            .filter(
                |event| matches!(event, Event::IntentRejected { account, .. } if *account == AccountId(0)),
            )
            .count();
        if tick == 0 {
            first_step_resting_buy = Some(
                session
                    .state
                    .markets
                    .get(&stock)
                    .unwrap()
                    .resting_orders_for(AccountId(0))
                    .iter()
                    .any(|order| order.side == Side::Buy && order.qty == 100),
            );
        }
    }
    assert_eq!(repurchase_trades, vec![100], "当日恰好一笔回购真实成交");
    assert!(
        account_cash(&session, AccountId(0)) > player_cash_before,
        "卖方必须真实收到回购资金"
    );
    assert_eq!(player_rejections, 0, "玩家卖单与 NPC 代表输入均不得被拒");
    assert_eq!(
        first_step_resting_buy,
        Some(true),
        "NPC 代表输入首 tick 后必须真实挂单（不丢失）"
    );
    // 同日去重：整个交易日只生成一单回购委托（第二单会再成交或留下挂单）。
    assert!(
        session
            .state
            .markets
            .get(&stock)
            .unwrap()
            .resting_orders_for(repurchase_account)
            .is_empty(),
        "同日不得重复生成回购委托（无多余挂单）"
    );
    let confirmations = session
        .state
        .personal_trade_confirmations
        .get(&repurchase_account)
        .map(|list| list.len())
        .unwrap_or(0);
    assert_eq!(confirmations, 1, "同日只有一条回购成交确认");
}

/// 探针 2c（核心复现）：绑定共享 ingress 的真实宿主路径，玩家合法输入
/// 经 source 受理后，回购执行器本地回执与它撞同一 stock ordinal，
/// 真实 `step()` 在 AccountValidation/StockProcessing 准备时以
/// `conflicting requests share a stock receipt` StepFatal 毒化会话——
/// 全部输入合法的 tick 不得 fatal（当前预期红灯）。
#[test]
fn bound_ingress_player_only_real_step_must_not_fatal_on_receipt_conflict() {
    let (mut session, stock, issuer, repurchase_account) = probe_session();
    session
        .approve_issuer_repurchase(probe_plan(&issuer, &stock))
        .unwrap();
    let source = session.shared_ingress();
    advance_to_window_first_day(&mut session);
    // 真实宿主路径：玩家输入只经共享 source（无任何私人受理推进本地游标）。
    source
        .enqueue_player_intent(AccountId(0), sell_intent(&stock, 100))
        .unwrap();
    let business_before = session.business_state_hash().unwrap();
    let result = session.step();
    let observed_fatal = result.as_ref().err().map(|fatal| fatal.to_string());
    let leaked_last_order_day = repurchase_book(&session, &stock).last_order_day();
    let leaked_receipt_count = session
        .state
        .pending_player
        .iter()
        .filter(|receipt| receipt.owner == repurchase_account)
        .count();
    println!(
        "probe2c observed: step_result={} fatal={observed_fatal:?} leaked_last_order_day={leaked_last_order_day:?} leaked_receipts={leaked_receipt_count} business_hash_changed={} player_input_still_in_source={}",
        result.is_ok(),
        session.business_state_hash().unwrap() != business_before,
        source
            .recorded_player_inputs()
            .map(|inputs| !inputs.is_empty())
            .unwrap_or(false),
    );
    let events = result.expect("合法玩家输入 + 回购窗口首 tick 必须成功提交，不得以 receipt 冲突毒化会话");
    let trades = trade_between(&events, AccountId(0), repurchase_account);
    assert_eq!(trades, vec![100], "成功路径必须真实撮合");
}

/// 探针 3a：窗口中途按合法日终契约保存/恢复，恢复后当日不重复生成回购委托，
/// 次一交易日正常再生成一单（last_order_day 幂等去重跨恢复保持）。
/// 数量设计：预算约束使首日委托 4,800 股被玩家等量卖单当日全额吃掉、
/// 计划仍未完成，次日再生成小额委托——全程不留挂单（避开探针 3b 的独立缺陷）。
#[test]
fn day_end_restore_keeps_same_day_dedup_across_window() {
    let (mut session, stock, issuer, repurchase_account) =
        probe_session_with_player_shares(20_000);
    let mut plan = probe_plan(&issuer, &stock);
    plan.max_shares = 10_000;
    session.approve_issuer_repurchase(plan).unwrap();
    advance_to_window_first_day(&mut session);
    session
        .enqueue_player_intent(AccountId(0), sell_intent(&stock, 4_800))
        .unwrap();
    for _ in 0..session.state.setup.ticks_per_day {
        session.step().unwrap();
    }
    session.end_civil_day().unwrap();
    let day1_filled = repurchase_book(&session, &stock).total_filled_shares();
    println!("probe3a observed: day1 filled={day1_filled}");
    assert_eq!(day1_filled, 4_800, "窗口首日回购真实成交入账");
    assert_eq!(
        repurchase_book(&session, &stock).last_order_day(),
        Some(CivilDate::from_iso(WINDOW_FIRST_DAY).unwrap())
    );

    let save = session.save().unwrap();
    let mut restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        repurchase_book(&restored, &stock).total_filled_shares(),
        4_800,
        "恢复保留成交事实"
    );
    assert_eq!(
        repurchase_book(&restored, &stock).last_order_day(),
        Some(CivilDate::from_iso(WINDOW_FIRST_DAY).unwrap()),
        "恢复保留同日去重事实"
    );

    // 次一交易日（窗口内）：恢复后的会话再生成一单、当日全额成交、
    // 同日跨 tick 不重复生成。
    let confirmations_before = restored
        .state
        .personal_trade_confirmations
        .get(&repurchase_account)
        .map(|list| list.len())
        .unwrap_or(0);
    println!(
        "probe3a observed: pre-day2 status={:?} remaining={:?} last_order_day={:?} phase={:?} date={:?}",
        repurchase_book(&restored, &stock).status(),
        repurchase_book(&restored, &stock).remaining_budget(),
        repurchase_book(&restored, &stock).last_order_day(),
        restored.phase(),
        restored.civil_date(),
    );
    for tick in 0..restored.state.setup.ticks_per_day {
        if tick == 0 {
            restored
                .enqueue_player_intent(AccountId(0), sell_intent(&stock, 100))
                .unwrap();
        }
        let events = restored.step().unwrap();
        let summary = events
            .iter()
            .filter_map(|event| match event {
                Event::Trade { maker, taker, qty, .. } => {
                    Some(format!("Trade({maker:?}/{taker:?},{qty})"))
                }
                Event::OrderAccepted { account, remaining_qty, .. } => {
                    Some(format!("Accepted({account:?},{remaining_qty})"))
                }
                Event::IntentRejected { account, .. } => Some(format!("Rejected({account:?})")),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(",");
        println!("probe3a observed: day2 tick {tick} events=[{summary}] filled={}", repurchase_book(&restored, &stock).total_filled_shares());
    }
    let day2_filled_midday = repurchase_book(&restored, &stock).total_filled_shares();
    let confirmations_after = restored
        .state
        .personal_trade_confirmations
        .get(&repurchase_account)
        .map(|list| list.len())
        .unwrap_or(0);
    println!(
        "probe3a observed: day2 midday book filled={day2_filled_midday} (成交入账按日终聚合，此值在日结前不变) confirmations_delta={}",
        confirmations_after - confirmations_before
    );
    assert_eq!(
        confirmations_after - confirmations_before,
        1,
        "次一交易日恰好一笔回购成交（同日不重复生成委托）"
    );
    assert!(
        restored
            .state
            .markets
            .get(&stock)
            .unwrap()
            .resting_orders_for(repurchase_account)
            .is_empty(),
        "全额成交后无回购挂单残留"
    );
    restored.end_civil_day().unwrap();
    let day2_filled = repurchase_book(&restored, &stock).total_filled_shares();
    println!("probe3a observed: day2 book filled after day end={day2_filled}");
    assert!(
        day2_filled > 4_800,
        "次一交易日恢复后的会话必须继续生成回购委托并真实成交"
    );
}

/// 探针 3b（独立缺陷复现）：回购委托部分成交后留挂单，是合法业务场景；
/// 计划投影为挂单登记 NPC 撤单生命周期时，`AccountKind::IssuerRepurchase`
/// 未被跳过（守卫只排除了 Player），直接命中
/// `panic!("player/issuer orders must not receive NPC quote lifecycles")`
/// ——进程级 panic（非 StepFatal）。当前预期以该 panic 红灯。
#[test]
fn resting_repurchase_order_must_not_panic_npc_lifecycle() {
    let (mut session, stock, issuer, repurchase_account) = probe_session();
    let mut plan = probe_plan(&issuer, &stock);
    plan.max_shares = 300;
    session.approve_issuer_repurchase(plan).unwrap();
    advance_to_window_first_day(&mut session);
    session
        .enqueue_player_intent(AccountId(0), sell_intent(&stock, 100))
        .unwrap();
    // 首 tick：回购委托与玩家 100 股卖单部分成交后留挂单（合法）。
    println!("probe3b: stepping window-day tick 1 (placement + partial fill)");
    session.step().unwrap();
    println!(
        "probe3b: tick 1 ok, resting repurchase orders={}",
        session
            .state
            .markets
            .get(&stock)
            .unwrap()
            .resting_orders_for(repurchase_account)
            .len()
    );
    // 后续 tick：合法推进不得 panic（当前实现以生产 panic 中止）。
    for remaining in 0..session.state.setup.ticks_per_day - 1 {
        println!("probe3b: stepping window-day tick {}", remaining + 2);
        session.step().unwrap();
    }
    session.end_civil_day().unwrap();
}
