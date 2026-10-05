use engine::session::protocol::{
    attach_facts, EngineUpdate as ProtocolUpdate, TickBatch, TickFrame, TickTimeseriesPayload,
};
use engine::{AccountId, Event, Money, OrderId, RejectionReason, Side, StockCode};
use server::EngineUpdate;

#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

fn committed_update() -> EngineUpdate {
    let code = StockCode("600888".into());
    let events = vec![
        Event::Trade {
            seq: 1,
            code: code.clone(),
            price: Money::from_cents(1000),
            qty: 100,
            maker: AccountId(70),
            taker: AccountId(80),
        },
        Event::OrderAccepted {
            seq: 2,
            account: AccountId(70),
            code: code.clone(),
            id: OrderId(700),
            side: Side::Buy,
            price: Money::from_cents(990),
            remaining_qty: 100,
        },
        Event::OrderCanceled {
            seq: 3,
            account: AccountId(80),
            code: code.clone(),
            id: OrderId(800),
            remaining_qty: 200,
        },
        Event::IntentRejected {
            seq: 4,
            account: AccountId(80),
            code: code.clone(),
            reason: RejectionReason::InsufficientCash,
        },
        Event::SettlementError {
            seq: 5,
            account: AccountId(80),
            code,
            reason: "private-cash-detail-123456789".into(),
        },
    ];
    let facts = attach_facts(&events).unwrap();
    let batch = TickBatch {
        frames: vec![TickFrame {
            tick: 1,
            events,
            facts,
            timeseries_payload: TickTimeseriesPayload::default(),
            seq_from: 0,
            seq_to: 5,
        }],
        runtime_snapshot: None,
        runtime_delta: None,
    };
    batch.validate().unwrap();
    EngineUpdate {
        timeline_generation: 1,
        update: Some(ProtocolUpdate::TickBatch(Box::new(batch))),
        civil_date: "2030-01-02".into(),
        public_revision: 0,
        failure: None,
    }
}

fn projected_json(viewer: Option<AccountId>) -> serde_json::Value {
    let projection = committed_update().for_member_account(viewer);
    let Some(ProtocolUpdate::TickBatch(batch)) = projection.update else {
        panic!("投影必须保持真实TickBatch")
    };
    batch.validate().unwrap();
    assert_eq!((batch.frames[0].seq_from, batch.frames[0].seq_to), (0, 5));
    assert_eq!(
        batch.frames[0]
            .events
            .iter()
            .map(Event::seq)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    serde_json::to_value(batch.frames[0].clone()).unwrap()
}

#[test]
fn own_view_keeps_own_orders_but_anonymizes_all_trade_counterparties_and_other_private_facts() {
    let projection = projected_json(Some(AccountId(70)));
    assert_eq!(
        projection["events"][0],
        serde_json::json!({ "PublicTrade": { "seq": 1, "code": "600888", "price": "1000", "qty": 100 } })
    );
    assert!(projection["events"][1].get("OrderAccepted").is_some());
    for index in 2..5 {
        assert_eq!(
            projection["events"][index],
            serde_json::json!({ "PrivateEventOmitted": { "seq": index + 1 } })
        );
        assert!(projection["facts"][index]["key"]["entity"]
            .get("Account")
            .is_none());
        assert_eq!(
            projection["facts"][index]["event"],
            projection["events"][index]
        );
    }
    let payload = projection.to_string();
    assert!(!payload.contains("maker") && !payload.contains("taker"));
    assert!(!payload.contains("private-cash-detail-123456789"));
    assert!(!payload.contains("InsufficientCash"));
    assert!(!payload.contains("800"));
}

#[test]
fn other_member_view_keeps_own_private_result_without_exposing_another_members_order() {
    let projection = projected_json(Some(AccountId(80)));
    assert!(projection["events"][0].get("PublicTrade").is_some());
    assert_eq!(
        projection["events"][1],
        serde_json::json!({ "PrivateEventOmitted": { "seq": 2 } })
    );
    assert!(projection["events"][2].get("OrderCanceled").is_some());
    assert!(projection["events"][3].get("IntentRejected").is_some());
    assert!(projection["events"][4].get("SettlementError").is_some());
    assert!(!projection.to_string().contains("700"));
    assert_eq!(projection["facts"][1]["event"], projection["events"][1]);
    assert!(projection["facts"][1]["key"]["entity"]
        .get("Account")
        .is_none());
}

#[test]
fn controller_without_economic_membership_gets_only_public_trade_and_sequence_placeholders() {
    let original = committed_update();
    let original_bytes = serde_json::to_value(&original).unwrap();
    let projection = projected_json(None);
    assert_eq!(projection["events"][0]["PublicTrade"]["price"], "1000");
    assert_eq!(projection["events"][0]["PublicTrade"]["qty"], 100);
    for index in 1..5 {
        assert_eq!(
            projection["events"][index],
            serde_json::json!({ "PrivateEventOmitted": { "seq": index + 1 } })
        );
        assert_eq!(
            projection["facts"][index]["key"]["local_event_index"],
            index + 1
        );
        assert_eq!(projection["facts"][index]["key"]["entity"], "Session");
    }
    let payload = projection.to_string();
    for private_field in [
        "maker",
        "taker",
        "account",
        "Account",
        "OrderAccepted",
        "OrderCanceled",
        "IntentRejected",
        "SettlementError",
        "private-cash-detail-123456789",
        "InsufficientCash",
    ] {
        assert!(
            !payload.contains(private_field),
            "不能泄露私有字段{private_field}"
        );
    }
    assert_eq!(serde_json::to_value(original).unwrap(), original_bytes);
}

#[test]
fn completed_day_refresh_anonymizes_real_intraday_order_history_too() {
    let mut setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.ticks_per_day = 1;
    let mut game = engine::session::protocol::ProtocolSession::new(setup, 2).unwrap();
    let subject = engine::session::OpaqueSubjectId::new("private-trader".into()).unwrap();
    let member = game.join_market(subject, false).unwrap();
    game.enqueue_player_intent(
        member.account_id,
        engine::Intent::PlaceLimit {
            code: StockCode("600101".into()),
            side: Side::Buy,
            price: engine::LimitPrice::Fixed(Money::from_cents(1000)),
            qty: 100,
        },
    )
    .unwrap();
    let frame = game.step_frame().unwrap();
    assert!(frame.events.iter().any(|event| matches!(event, Event::OrderAccepted { account, .. } if *account == member.account_id)));
    let civil = game.end_civil_day_update().unwrap();
    assert!(civil.refresh.intraday.iter().flat_map(|frame| &frame.events).any(|event| matches!(event, Event::OrderAccepted { account, .. } if *account == member.account_id)));
    let update = EngineUpdate {
        timeline_generation: 1,
        update: Some(ProtocolUpdate::CivilUpdate(Box::new(civil))),
        civil_date: game.civil_date().to_iso(),
        public_revision: 1,
        failure: None,
    }
    .for_member_account(None);
    let Some(ProtocolUpdate::CivilUpdate(civil)) = update.update else {
        panic!("日终投影必须保持CivilUpdate")
    };
    civil.validate().unwrap();
    assert!(civil.refresh.snapshot.accounts.is_empty());
    let payload = serde_json::to_string(&civil).unwrap();
    for private_field in [
        "OrderAccepted",
        "OrderCanceled",
        "IntentRejected",
        "SettlementError",
        "Account",
        "maker",
        "taker",
    ] {
        assert!(
            !payload.contains(private_field),
            "日终历史不能重新泄露私有字段{private_field}"
        );
    }
    assert!(payload.contains("PrivateEventOmitted"));
}

#[test]
fn own_financial_event_accounts_preserve_full_u64_as_canonical_decimal_strings() {
    let account = AccountId(u64::MAX);
    let code = StockCode("600888".into());
    let events = vec![
        Event::OrderAccepted {
            seq: 1,
            account,
            code: code.clone(),
            id: OrderId(1),
            side: Side::Buy,
            price: Money::from_cents(1000),
            remaining_qty: 100,
        },
        Event::OrderCanceled {
            seq: 2,
            account,
            code: code.clone(),
            id: OrderId(1),
            remaining_qty: 100,
        },
        Event::IntentRejected {
            seq: 3,
            account,
            code: code.clone(),
            reason: RejectionReason::InsufficientCash,
        },
        Event::SettlementError {
            seq: 4,
            account,
            code,
            reason: "本人结算错误详情".into(),
        },
    ];
    let facts = attach_facts(&events).unwrap();
    let update = EngineUpdate {
        timeline_generation: 1,
        update: Some(ProtocolUpdate::TickBatch(Box::new(TickBatch {
            frames: vec![TickFrame {
                tick: 1,
                events,
                facts,
                timeseries_payload: TickTimeseriesPayload::default(),
                seq_from: 0,
                seq_to: 4,
            }],
            runtime_snapshot: None,
            runtime_delta: None,
        }))),
        civil_date: "2030-01-02".into(),
        public_revision: 0,
        failure: None,
    }
    .for_member_account(Some(account));
    let Some(ProtocolUpdate::TickBatch(batch)) = update.update else {
        panic!("本人金融事实投影保持TickBatch")
    };
    batch.validate().unwrap();
    let value = serde_json::to_value(&batch.frames[0]).unwrap();
    for (index, variant) in [
        "OrderAccepted",
        "OrderCanceled",
        "IntentRejected",
        "SettlementError",
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(
            value["events"][index][variant]["account"],
            "18446744073709551615"
        );
        assert_eq!(
            value["facts"][index]["key"]["entity"]["Account"],
            "18446744073709551615"
        );
        assert_eq!(value["facts"][index]["event"], value["events"][index]);
        assert_eq!(
            serde_json::from_value::<Event>(value["events"][index].clone())
                .unwrap()
                .seq(),
            index as u64 + 1
        );
        for invalid in [
            serde_json::json!(1),
            serde_json::json!("01"),
            serde_json::json!("+1"),
            serde_json::json!("-1"),
            serde_json::json!("1e3"),
            serde_json::json!("18446744073709551616"),
        ] {
            let mut invalid_event = value["events"][index].clone();
            invalid_event[variant]["account"] = invalid.clone();
            assert!(serde_json::from_value::<Event>(invalid_event).is_err());
            assert!(
                serde_json::from_value::<engine::session::pipeline::EntityTag>(
                    serde_json::json!({ "Account": invalid })
                )
                .is_err()
            );
        }
    }
}

#[test]
fn real_multiple_account_cancellations_emit_only_the_viewers_owned_removal_identity() {
    let mut setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    let mut game = engine::GameSession::new(setup, 3).unwrap();
    let other = game
        .join_market(
            engine::session::OpaqueSubjectId::new("second-player".into()).unwrap(),
            false,
        )
        .unwrap()
        .account_id;
    let code = StockCode("600101".into());
    for account in [AccountId(0), other] {
        game.enqueue_player_intent(
            account,
            engine::Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: engine::LimitPrice::Fixed(Money::from_cents(900)),
                qty: 100,
            },
        )
        .unwrap();
    }
    let frame = game.step_frame().unwrap();
    let (_, previous) = game.tick_batch_delta(vec![frame], None).unwrap();
    let first_order = game.account_working_orders(AccountId(0))[0].id;
    let second_order = game.account_working_orders(other)[0].id;
    assert_ne!(first_order, second_order);
    for (account, id) in [(AccountId(0), first_order), (other, second_order)] {
        game.enqueue_player_intent(
            account,
            engine::Intent::Cancel {
                code: code.clone(),
                id: OrderId(id),
            },
        )
        .unwrap();
    }
    let frame = game.step_frame().unwrap();
    assert!(game.account_working_orders(AccountId(0)).is_empty());
    assert!(game.account_working_orders(other).is_empty());
    let (batch, _) = game.tick_batch_delta(vec![frame], Some(&previous)).unwrap();
    batch.validate().unwrap();
    let committed = EngineUpdate {
        timeline_generation: 1,
        update: Some(ProtocolUpdate::TickBatch(Box::new(batch))),
        civil_date: game.civil_date().to_iso(),
        public_revision: 0,
        failure: None,
    };
    for (viewer, expected) in [
        (
            Some(AccountId(0)),
            serde_json::json!([{ "id": first_order, "owner": "0" }]),
        ),
        (
            Some(other),
            serde_json::json!([{ "id": second_order, "owner": other.0.to_string() }]),
        ),
        (None, serde_json::json!([])),
    ] {
        let projection = committed.clone().for_member_account(viewer);
        let Some(ProtocolUpdate::TickBatch(batch)) = projection.update else {
            panic!("撤单delta必须保持TickBatch")
        };
        batch.validate().unwrap();
        let orders = serde_json::to_value(&batch.runtime_delta.unwrap().working_orders).unwrap();
        assert_eq!(orders["removed"], expected);
    }
}
