use super::*;

fn subject(value: &str) -> OpaqueSubjectId {
    OpaqueSubjectId::new(value.to_owned()).unwrap()
}

fn market() -> GameSession {
    GameSession::new(npc_working_quote_tests::quote_setup(0), 42).unwrap()
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
fn new_member_and_working_order_account_ids_are_lossless_canonical_u64_strings() {
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
    let order = protocol::PlayerWorkingOrder {
        owner: AccountId(u64::MAX),
        id: 1,
        code: StockCode("600888".into()),
        side: Side::Buy,
        price: Money::from_cents(1000),
        remaining_qty: 100,
        venue: "continuous".into(),
        frozen: "cash".into(),
    };
    let order_encoded = serde_json::to_value(order).unwrap();
    assert_eq!(order_encoded["owner"], "18446744073709551615");
    assert_eq!(
        serde_json::from_value::<protocol::PlayerWorkingOrder>(order_encoded.clone())
            .unwrap()
            .owner,
        AccountId(u64::MAX)
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
        member_encoded["account_id"] = invalid.clone();
        assert!(serde_json::from_value::<MarketMembership>(member_encoded).is_err());
        let mut invalid_order = order_encoded.clone();
        invalid_order["owner"] = invalid;
        assert!(serde_json::from_value::<protocol::PlayerWorkingOrder>(invalid_order).is_err());
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
    let code = game.state.setup.stocks[0].code.clone();
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
    setup.start_date = CivilDate::from_iso("2030-01-02").unwrap();
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
    let code = StockCode("600888".into());
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
    assert!(game.step().unwrap().iter().any(|event| matches!(event, Event::IntentRejected { account, reason: RejectionReason::NotOrderOwner, .. } if *account == first)));
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
    assert!(game.step().unwrap().iter().any(|event| matches!(event, Event::Trade { maker, taker, qty: 100, .. } if *maker == first && *taker == AccountId(0))));
    assert_eq!(
        game.account(first)
            .unwrap()
            .position(&code)
            .unwrap()
            .t1_locked(),
        100
    );
    assert!(game.account(second).unwrap().positions().is_empty());
    assert_eq!(game.account_working_orders(second).len(), 1);
    assert!(game.account_working_orders(first).is_empty());
    assert_eq!(game.personal_trade_confirmations_page(first, None).len(), 1);
    assert!(game
        .personal_trade_confirmations_page(second, None)
        .is_empty());
    game.public_runtime_state().unwrap();
    assert!(game.runtime_snapshot().accounts.contains_key(&first));
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

fn market_with_maximum_player_account() -> (GameSession, OpaqueSubjectId) {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.start_date = CivilDate::from_iso("2030-01-02").unwrap();
    setup.npcs.inst_count = 0;
    setup.npcs.retail_count = 1;
    setup.strategy_params.retail.arrival_rate = 0.0;
    setup.stocks[0].float_shares = 100;
    let mut game = GameSession::new(setup, 42).unwrap();
    let trader = subject("maximum-player");
    let member = game.join_market(trader.clone(), false).unwrap();
    game.state.accounts = game
        .state
        .accounts
        .iter()
        .map(|(account_id, account)| {
            if *account_id == member.account_id {
                (
                    AccountId(u64::MAX),
                    Account::new(AccountId(u64::MAX), AccountKind::Player, account.cash()),
                )
            } else {
                (*account_id, account.clone())
            }
        })
        .collect();
    game.state.history_reads = game
        .state
        .history_reads
        .to_map()
        .into_iter()
        .map(|(account, ledger)| {
            (
                if account == member.account_id {
                    AccountId(u64::MAX)
                } else {
                    account
                },
                ledger,
            )
        })
        .collect();
    game.state
        .memberships
        .members
        .get_mut(&trader)
        .unwrap()
        .account_id = AccountId(u64::MAX);
    (game, trader)
}

#[test]
fn maximum_member_account_roundtrips_real_snapshot_save_and_restored_history() {
    let (mut game, trader) = market_with_maximum_player_account();
    let account = game.resolve_trading_account(&trader).unwrap();
    assert_eq!(account, AccountId(u64::MAX));
    let snapshot = serde_json::to_value(game.snapshot()).unwrap();
    assert!(snapshot["accounts"].get("18446744073709551615").is_some());
    let code = StockCode("600888".into());
    game.query_stock_history(account, &code).unwrap();
    let saved = game.save().unwrap();
    let encoded = serde_json::to_vec(&saved).unwrap();
    let decoded: SaveSlot = serde_json::from_slice(&encoded).unwrap();
    let mut restored = GameSession::restore(&decoded).unwrap();
    assert_eq!(restored.resolve_trading_account(&trader).unwrap(), account);
    assert_eq!(
        restored.state.history_reads[&account].stocks[&code].read_count,
        1
    );
    assert_eq!(
        restored.join_market(trader, true).unwrap().account_id,
        account
    );
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(saved).unwrap()
    );
}

#[test]
fn all_account_id_values_and_map_keys_use_one_strict_canonical_u64_contract() {
    for account in [
        AccountId(0),
        AccountId(9_007_199_254_740_992),
        AccountId(u64::MAX),
    ] {
        let encoded = serde_json::to_value(account).unwrap();
        assert_eq!(encoded, serde_json::json!(account.0.to_string()));
        assert_eq!(
            serde_json::from_value::<AccountId>(encoded).unwrap(),
            account
        );
        let accounts = BTreeMap::from([(account, Money::from_cents(100))]);
        let encoded = serde_json::to_value(&accounts).unwrap();
        assert_eq!(
            serde_json::from_value::<BTreeMap<AccountId, Money>>(encoded).unwrap(),
            accounts
        );
    }
    for invalid in [
        serde_json::json!(0),
        serde_json::json!(""),
        serde_json::json!("00"),
        serde_json::json!("+1"),
        serde_json::json!("-1"),
        serde_json::json!(" 1"),
        serde_json::json!("1.0"),
        serde_json::json!("1e3"),
        serde_json::json!("18446744073709551616"),
    ] {
        assert!(serde_json::from_value::<AccountId>(invalid).is_err());
    }
    for invalid_key in ["01", "+1", "-1", " 1", "1e3", "18446744073709551616"] {
        let invalid = serde_json::json!({ (invalid_key): "100" });
        assert!(serde_json::from_value::<BTreeMap<AccountId, Money>>(invalid).is_err());
    }
}

#[test]
fn maximum_member_account_executes_real_fifo_trade_with_t1_and_serializable_committed_facts() {
    let (mut game, trader) = market_with_maximum_player_account();
    let account = game.resolve_trading_account(&trader).unwrap();
    let code = StockCode("600888".into());
    assert_eq!(
        game.account(AccountId(1))
            .unwrap()
            .position(&code)
            .unwrap()
            .qty(),
        100
    );
    assert!(game
        .state
        .pending_npc
        .as_ref()
        .is_none_or(|pending| pending.intents.is_empty()));
    let ingress = game.shared_ingress();
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
    let npc_receipt = game
        .receive_private_intent(
            AccountId(1),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(1000)),
                qty: 100,
            },
        )
        .unwrap();
    game.state.pending_npc = Some(PendingNpcBatch {
        observed_tick: game.tick(),
        observed_accounts: vec![AccountId(1)],
        intents: vec![npc_receipt],
        dependencies: Vec::new(),
    });
    let frame = game.step_frame().unwrap();
    assert!(frame.events.iter().any(|event| matches!(event, Event::Trade { maker, taker, qty: 100, .. } if *maker == account && *taker == AccountId(1))), "真实MAX账户FIFO成交缺失：{:?}", frame.events);
    assert_eq!(
        game.account(account)
            .unwrap()
            .position(&code)
            .unwrap()
            .t1_locked(),
        100
    );
    let json = serde_json::to_value(&frame).unwrap();
    let trade = json["events"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|event| event.get("Trade"))
        .unwrap();
    assert_eq!(trade["maker"], "18446744073709551615");
    assert_eq!(trade["taker"], "1");
    let saved = game.save().unwrap();
    let encoded = serde_json::to_vec(&saved).unwrap();
    let decoded: SaveSlot = serde_json::from_slice(&encoded).unwrap();
    let restored = GameSession::restore(&decoded).unwrap();
    assert_eq!(
        restored
            .account(account)
            .unwrap()
            .position(&code)
            .unwrap()
            .t1_locked(),
        100
    );
    assert_eq!(
        restored.personal_trade_confirmations_page(account, None),
        game.personal_trade_confirmations_page(account, None)
    );
}
