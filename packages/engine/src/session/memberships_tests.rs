//! 市场成员关系域测试（B3 远程登录批次取回自 18725d07，main 形态适配）。
//!
//! 取回裁剪注记（均为 main 与 18725d07 的既有分叉决策，非弱化断言）：
//! - `all_account_id_values_and_map_keys_use_one_strict_canonical_u64_contract`
//!   与两个 `maximum_member_account_*` 用例依赖 18725d07 的 AccountId 全局
//!   canonical-string serde 契约；main 现状 AccountId 是 js_safe number
//!   （上限 2^53-1），u64::MAX 账户在本契约下非法。全局 serde 契约切换
//!   不属 B3 范围，待独立裁决后恢复这三个用例。
//! - `new_member_and_working_order_account_ids_are_lossless_canonical_u64_strings`
//!   仅保留 MarketMembership.account_id 的 canonical string 段；其
//!   PlayerWorkingOrder/AccountId 本体段同上裁剪。
//! - `members_trade_in_one_shared_book_with_fifo_and_t1_and_cannot_cancel_others`
//!   的 personal_trade_confirmations_page 断言依赖 B2 批（个人成交历史
//!   查询网），该段落随 B2 落地恢复；account_working_orders 以 main 在位的
//!   player_working_orders().filter 等价表达；StockCode 以 ADR-0041 的
//!   StockId 数值寻址等价表达。

use super::*;

fn subject(value: &str) -> OpaqueSubjectId {
    OpaqueSubjectId::new(value.to_owned()).unwrap()
}

fn market() -> GameSession {
    GameSession::new(npc_working_quote_tests::quote_setup(0), 42).unwrap()
}

#[test]
fn export_bindings_memberships() {
    let config = ts_rs::Config::from_env();
    <OpaqueSubjectId as ts_rs::TS>::export(&config).unwrap();
    <AdmissionFunding as ts_rs::TS>::export(&config).unwrap();
    <MarketMembership as ts_rs::TS>::export(&config).unwrap();
    <MarketMembershipState as ts_rs::TS>::export(&config).unwrap();
}

#[test]
fn default_admission_cash_is_one_hundred_yi_yuan_in_cents() {
    assert_eq!(
        GameConfig::proposed_defaults().starting_cash,
        Money::from_cents(1_000_000_000_000)
    );
}

#[test]
fn joins_create_distinct_empty_accounts_and_never_repeat_admission_funding() {
    let mut game = market();
    game.bind_market_creator(subject("creator")).unwrap();
    let original = game.state.setup.config.starting_cash;
    let first = game.join_market(subject("first"), false).unwrap();
    assert_ne!(first.account_id, AccountId(0));
    assert_eq!(game.account(first.account_id).unwrap().cash(), original);
    assert!(game
        .account(first.account_id)
        .unwrap()
        .positions()
        .is_empty());
    game.set_admission_cash(Money::from_cents(12345)).unwrap();
    assert_eq!(game.join_market(subject("first"), true).unwrap(), first);
    assert_eq!(game.account(first.account_id).unwrap().cash(), original);
    let second = game.join_market(subject("second"), false).unwrap();
    assert_eq!(
        game.account(second.account_id).unwrap().cash(),
        Money::from_cents(12345)
    );
    assert_eq!(
        second.admission_funding.external_cash,
        Money::from_cents(12345)
    );
    assert!(game.resolve_trading_account(&subject("missing")).is_err());
    assert_eq!(
        game.resolve_trading_account(&subject("first")).unwrap(),
        first.account_id
    );
}

#[test]
fn absent_subject_after_restore_needs_confirmation_and_receives_only_one_admission() {
    let mut game = market();
    game.bind_market_creator(subject("creator")).unwrap();
    let saved = game.save_projection(persistence::capture_runtime_state(&game).unwrap());
    game.join_market(subject("later"), false).unwrap();
    let mut restored = GameSession::restore(&saved).unwrap();
    assert_eq!(
        restored.join_market(subject("later"), false),
        Err(MembershipError::RejoinConfirmationRequired)
    );
    let member = restored.join_market(subject("later"), true).unwrap();
    assert_eq!(
        restored.join_market(subject("later"), true).unwrap(),
        member
    );
    assert_eq!(
        restored.account(member.account_id).unwrap().cash(),
        member.admission_funding.external_cash
    );
}

#[test]
fn member_save_validation_rejects_duplicate_economic_account() {
    let mut game = market();
    game.bind_market_creator(subject("creator")).unwrap();
    game.join_market(subject("other"), false).unwrap();
    let mut saved = game.save_projection(persistence::capture_runtime_state(&game).unwrap());
    saved
        .market_memberships
        .members
        .get_mut(&subject("other"))
        .unwrap()
        .account_id = AccountId(0);
    assert!(GameSession::restore(&saved).is_err());
}

#[test]
fn new_member_membership_account_id_is_lossless_canonical_u64_string() {
    let member = MarketMembership {
        account_id: AccountId(u64::MAX),
        admission_funding: AdmissionFunding {
            external_cash: Money::ZERO,
        },
    };
    let encoded = serde_json::to_value(&member).unwrap();
    assert_eq!(encoded["account_id"], "18446744073709551615");
    assert_eq!(
        serde_json::from_value::<MarketMembership>(encoded.clone()).unwrap(),
        member
    );
    for invalid in [
        serde_json::json!(1),
        serde_json::json!("01"),
        serde_json::json!("+1"),
        serde_json::json!("-1"),
        serde_json::json!(" 1"),
        serde_json::json!("1e2"),
        serde_json::json!("18446744073709551616"),
    ] {
        let mut member_encoded = encoded.clone();
        member_encoded["account_id"] = invalid;
        assert!(serde_json::from_value::<MarketMembership>(member_encoded).is_err());
    }
    assert!(serde_json::from_str::<OpaqueSubjectId>("\"\"").is_err());
    let mut old_capabilities = encoded;
    old_capabilities["capabilities"] =
        serde_json::json!({ "market_control": true, "own_account_trading": true });
    assert!(serde_json::from_value::<MarketMembership>(old_capabilities).is_err());
}

#[test]
fn member_restore_preserves_cash_history_and_admission_without_another_payment() {
    let mut game = market();
    game.bind_market_creator(subject("creator")).unwrap();
    let member = game.join_market(subject("member"), false).unwrap();
    game.state
        .accounts
        .get_mut(&member.account_id)
        .unwrap()
        .fixture_set_cash(Money::from_cents(54321));
    let code = game.state.stocks.id("600888").unwrap();
    game.query_stock_history(member.account_id, &code).unwrap();
    let saved = game.save_projection(persistence::capture_runtime_state(&game).unwrap());
    let mut restored = GameSession::restore(&saved).unwrap();
    assert_eq!(
        restored.join_market(subject("member"), false).unwrap(),
        member
    );
    assert_eq!(
        restored.account(member.account_id).unwrap().cash(),
        Money::from_cents(54321)
    );
    assert_eq!(
        restored.state.history_reads[&member.account_id].stocks[&code].read_count,
        1
    );
    let mut negative = saved.clone();
    negative
        .market_memberships
        .members
        .get_mut(&subject("member"))
        .unwrap()
        .admission_funding
        .external_cash = Money::from_cents(-1);
    assert!(GameSession::restore(&negative).is_err());
    let mut missing = saved;
    missing
        .market_memberships
        .members
        .remove(&subject("member"));
    assert!(GameSession::restore(&missing).is_err());
}

#[test]
fn members_trade_in_one_shared_book_with_fifo_and_t1_and_cannot_cancel_others() {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::calendar::CivilDate::from_iso("2030-01-02").unwrap();
    setup.npcs.inst_count = 0;
    let mut game = GameSession::new(setup, 42).unwrap();
    game.bind_market_creator(subject("seller")).unwrap();
    let ingress = game.shared_ingress();
    let first = game
        .join_market(subject("first"), false)
        .unwrap()
        .account_id;
    let second = game
        .join_market(subject("second"), false)
        .unwrap()
        .account_id;
    let code = game.state.stocks.id("600888").unwrap();
    game.state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .grant_position(code.clone(), 100, Money::from_cents(1000))
        .unwrap();
    for account in [first, second] {
        ingress
            .enqueue_player_intent(
                account,
                Intent::PlaceLimit {
                    code: code.clone(),
                    side: Side::Buy,
                    price: LimitPrice::Fixed(Money::from_cents(1000)),
                    qty: 100,
                },
            )
            .unwrap();
    }
    let accepted = game.step().unwrap();
    let second_order = accepted
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted { account, id, .. } if *account == second => Some(*id),
            _ => None,
        })
        .unwrap();
    ingress
        .enqueue_player_intent(
            first,
            Intent::Cancel {
                code: code.clone(),
                id: second_order,
            },
        )
        .unwrap();
    // 语义对齐注记：18725d07 旧管线把"撤他人挂单"显式报 NotOrderOwner；
    // main 的 runtime desk 层（event_bridge）统一映射 OrderNotFound——
    /// 撤单目标不存在与非本人挂单不区分（防他人挂单探测，同真实市场
    /// 口径）。身份防御拒绝本身保留（first 的撤单未生效），仅错误码
    /// 按 main 现行契约断言。
    assert!(game.step().unwrap().iter().any(|event| matches!(event, Event::IntentRejected { account, reason: RejectionReason::OrderNotFound, .. } if *account == first)));
    ingress
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(1000)),
                qty: 100,
            },
        )
        .unwrap();
    // ADR-0042 语义对齐注记：main 的 Event::Trade maker/taker=卖腿/买腿
    // （腿方向，2026-10-16 ADR——非 18725d07 的被动/主动口径）：账户 0
    // 卖 100 股吃掉 first 先挂的买单 ⇒ maker=账户 0（卖腿）、taker=first
    // （买腿）。同价 FIFO 归 first 的断言由下方 t1 持仓事实承载。
    assert!(game.step().unwrap().iter().any(|event| matches!(event, Event::Trade { maker, taker, qty: 100, .. } if *maker == AccountId(0) && *taker == first)));
    assert_eq!(
        game.account(first)
            .unwrap()
            .position(&code)
            .unwrap()
            .t1_locked(),
        100
    );
    assert!(game.account(second).unwrap().positions().is_empty());
    // account_working_orders（18725d07 按账户过滤的玩家在途读面）在 main
    // 现状尚无对应多玩家形态（player_working_orders 硬编码账户 0）；且
    // runtime 形态下旧管线市场簿读面恒空（D5：在场委托权威=desk），此处
    // 以 desk_book_mirror 事实等价断言每账户在途单数，读面多玩家化随
    // web 远程接线批裁决。
    let resting_of = |owner: AccountId| -> usize {
        game.state
            .desk_book_mirror
            .get(&code)
            .map(|mirror| {
                mirror
                    .orders
                    .iter()
                    .filter(|(o, _, _, qty, _, _, _)| *o == owner && *qty > 0)
                    .count()
            })
            .unwrap_or(0)
    };
    assert_eq!(resting_of(second), 1);
    assert_eq!(resting_of(first), 0);
    // B2 裁剪注记：18725d07 在此断言 personal_trade_confirmations_page
    // （first 有 1 条确认、second 为空）；该查询网属 B2 批，落地后恢复。
    // 快照面注记：main 的快照读面（snapshot/runtime_snapshot）只发布账户
    // 0（单人高频 protocol 设计，无公开全账户形态）；多成员可见性由
    // 上方账户/持仓/t1 事实承载，per-subject 快照投影随 server actor
    // 成员面接线批扩展。
    game.public_runtime_state().unwrap();
    assert!(game.account(first).is_some());
}

#[test]
fn empty_npc_market_preserves_unallocated_shares_and_players_start_without_shares() {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    setup.stocks[0].float_shares = 10000;
    let mut game = GameSession::new(setup, 42).unwrap();
    let member = game.join_market(subject("new"), false).unwrap();
    assert!(game
        .account(member.account_id)
        .unwrap()
        .positions()
        .is_empty());
    assert_eq!(
        game.initial_allocation().unwrap().stocks[0].unallocated_shares,
        10000
    );
}
