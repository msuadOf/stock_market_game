//! 合成混合日历的八例对照：同一完整 SaveSlot 恢复后，比较 native 组权威/镜像与
//! step_pipeline_contract 的非零计划、本股交易日、事件及存档；覆盖两所、首日/次日，
//! 并分别验证恢复后的 native 续行与委托 FIFO。自然日推进由 native 路径执行；
//! 不覆盖 actual wasm32、serial civil 对照或真实 collector Create，不代表真实年度日历。
use super::*;
use crate::calendar::CalendarExchange;
use crate::runtime::calendar_public_restore_fixture as fixture;

fn assert_snapshot_with_expected_origins(
    native: crate::Snapshot,
    serial: crate::Snapshot,
    native_origin: crate::RuntimeMirrorOrigin,
    serial_origin: crate::RuntimeMirrorOrigin,
) {
    assert_eq!(
        native.mirror_origin, native_origin,
        "native 来源必须精确匹配"
    );
    assert_eq!(
        serial.mirror_origin, serial_origin,
        "serial 来源必须精确匹配"
    );
    // 两侧来源独立断言后，完整 Snapshot 仍比较全部业务字段和未来新增字段。
    let mut expected = native.clone();
    expected.mirror_origin = serial_origin;
    assert_eq!(
        serde_json::to_value(serial).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

fn assert_phase_save_projection(
    native: &SaveSlot,
    serial: &SaveSlot,
    closed: StockId,
    open: StockId,
    phase: usize,
) {
    let expected_cursors =
        |value| std::collections::BTreeMap::from([(closed, value), (open, value)]);
    let (
        native_cursor,
        native_next_id,
        native_mirrored,
        serial_cursor,
        serial_next_id,
        serial_mirrored,
    ) = if phase == 0 {
        (1, 4_294_967_297, true, 0, 1, false)
    } else {
        (1, 4_294_967_297, true, 1, 4_294_967_297, true)
    };
    assert_eq!(native.book_next_sequences, expected_cursors(native_cursor));
    assert_eq!(native.next_order_id, native_next_id);
    assert_eq!(native.desk_mirrored_resting, native_mirrored);
    assert_eq!(serial.book_next_sequences, expected_cursors(serial_cursor));
    assert_eq!(serial.next_order_id, serial_next_id);
    assert_eq!(serial.desk_mirrored_resting, serial_mirrored);

    let mut native_value = serde_json::to_value(native).unwrap();
    let mut serial_value = serde_json::to_value(serial).unwrap();
    if phase == 0 {
        // 只对齐已逐项验证的desk/旧簿游标表示差异；其他存档字段仍完整比较。
        for stock in [closed, open] {
            let key = stock.0.to_string();
            native_value["book_next_sequences"][&key] = serde_json::json!("1");
            serial_value["book_next_sequences"][&key] = serde_json::json!("1");
        }
        native_value["next_order_id"] = serde_json::json!(4_294_967_297_u64);
        serial_value["next_order_id"] = serde_json::json!(4_294_967_297_u64);
        native_value["desk_mirrored_resting"] = serde_json::json!(true);
        serial_value["desk_mirrored_resting"] = serde_json::json!(true);
    }
    assert_eq!(
        native_value, serial_value,
        "完整SaveSlot除已验证的phase表示标量外必须相等"
    );
}

fn business_hash_field_labels(session: &GameSession) -> Vec<String> {
    let mut labels = vec![
        "setup".to_owned(),
        "seed".to_owned(),
        "rng.state".to_owned(),
    ];
    for id in session.state.accounts.keys() {
        labels.push(format!("account[{id:?}].core"));
        labels.push(format!("account[{id:?}].strategy.production_state"));
    }
    for code in session.state.markets.keys() {
        labels.push(format!("market[{code:?}].hash_projection"));
    }
    labels.extend([
        "price_history".to_owned(),
        "market_minute_closes".to_owned(),
        "candle_book.histories".to_owned(),
        "candle_book.active".to_owned(),
        "pending_player".to_owned(),
        "ingress_receipt_cursors".to_owned(),
        "npc_attention".to_owned(),
        "history_reads".to_owned(),
        "retail_experience".to_owned(),
        "npc_order_lifecycles".to_owned(),
        "npc_attention.sorted_queue".to_owned(),
    ]);
    match &session.state.companies {
        super::company_state::SessionCompanies::LegacyOperations(_) => labels.extend([
            "companies.legacy.variant".to_owned(),
            "companies.legacy.registry".to_owned(),
            "companies.legacy.operations".to_owned(),
            "companies.legacy.closing".to_owned(),
            "companies.legacy.groups".to_owned(),
        ]),
        super::company_state::SessionCompanies::CompanySystem(_) => labels.extend([
            "companies.system.variant".to_owned(),
            "companies.system.projection".to_owned(),
            "companies.system.corporate_actions".to_owned(),
        ]),
    }
    labels.extend([
        "report_corrections".to_owned(),
        "library".to_owned(),
        "disclosures".to_owned(),
        "plans".to_owned(),
        "urgency_policy".to_owned(),
        "belief.information".to_owned(),
        "belief.belief".to_owned(),
        "belief.watchlist".to_owned(),
        "belief.price_memory".to_owned(),
        "envelope_ledger".to_owned(),
        "retail_projection_seen".to_owned(),
        "counters.next_order_id_tick_day_seq_next_receipt_base".to_owned(),
        "civil_clock".to_owned(),
    ]);
    labels
}

fn collect_json_leaves(
    value: &serde_json::Value,
    path: &str,
    out: &mut Vec<(String, serde_json::Value)>,
) {
    match value {
        serde_json::Value::Object(object) if object.is_empty() => {
            out.push((path.to_owned(), value.clone()));
        }
        serde_json::Value::Object(object) => {
            for (key, child) in object {
                collect_json_leaves(child, &format!("{path}.{key}"), out);
            }
        }
        serde_json::Value::Array(array) if array.is_empty() => {
            out.push((path.to_owned(), value.clone()));
        }
        serde_json::Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                collect_json_leaves(child, &format!("{path}[{index}]"), out);
            }
        }
        leaf => out.push((path.to_owned(), leaf.clone())),
    }
}

fn print_json_differences(label: &str, left: &serde_json::Value, right: &serde_json::Value) {
    let mut left_leaves = Vec::new();
    let mut right_leaves = Vec::new();
    collect_json_leaves(left, "$", &mut left_leaves);
    collect_json_leaves(right, "$", &mut right_leaves);
    let left_map: std::collections::BTreeMap<_, _> = left_leaves.into_iter().collect();
    let right_map: std::collections::BTreeMap<_, _> = right_leaves.into_iter().collect();
    let paths: std::collections::BTreeSet<_> =
        left_map.keys().chain(right_map.keys()).cloned().collect();
    for path in paths {
        if left_map.get(&path) != right_map.get(&path) {
            eprintln!(
                "  {label} {path}: left={:?}, right={:?}",
                left_map.get(&path),
                right_map.get(&path)
            );
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct NativeSemanticProjection {
    labels: Vec<String>,
    fields: Vec<Vec<u8>>,
    save: Vec<u8>,
}

fn restore_exactly(saved: &SaveSlot) -> GameSession {
    let bytes = serde_json::to_vec(saved).unwrap();
    let decoded = crate::decode_save_slot(&bytes, &crate::SaveDecodeLimits::default()).unwrap();
    let restored = GameSession::restore(&decoded).unwrap();
    assert!(restored.runtime_driver.is_none());
    assert_eq!(restored.state.next_order_id, saved.next_order_id);
    for (stock, cursor) in &saved.book_next_sequences {
        assert_eq!(restored.state.markets[stock].book_next_sequence(), *cursor);
    }
    assert_eq!(
        serde_json::to_vec(&restored.save().unwrap()).unwrap(),
        bytes
    );
    assert_eq!(restored.state.plans, saved.plans);
    restored
}

/// 仅本夹具已证明退役的三个 raw 计数可映射；其余真实 hash bytes 和完整 SaveSlot 均保留。
fn native_semantic_projection(
    session: &GameSession,
    raw: &SaveSlot,
    expected_cursors: &std::collections::BTreeMap<StockId, u64>,
    live: bool,
) -> NativeSemanticProjection {
    let (hash, mut fields) = super::hash::capture_business_hash_for_test(session).unwrap();
    let labels = business_hash_field_labels(session);
    assert_eq!(labels.len(), 36, "本夹具必须覆盖全部36个真实hash field");
    assert_eq!(fields.len(), labels.len());
    let saved = session.save().unwrap();
    assert!(saved.desk_mirrored_resting);
    assert_eq!(&saved.book_next_sequences, expected_cursors);
    assert_eq!(session.state.next_order_id, raw.next_order_id);
    let effective_id = expected_cursors
        .iter()
        .map(|(stock, cursor)| {
            assert!(*cursor < (1_u64 << 32), "不得归一已耗尽的desk序号");
            crate::runtime::group::desk_order_id(stock.0 as crate::runtime::StockIdx, *cursor)
        })
        .max()
        .unwrap()
        .max(raw.next_order_id);
    assert_eq!(saved.next_order_id, effective_id);
    let mut live_facts = None;
    if live {
        let mut driver = session
            .runtime_driver
            .as_ref()
            .expect("必须已进入真实native owner")
            .lock()
            .unwrap();
        let facts = driver.collect_resting_facts_live().unwrap();
        assert_eq!(facts.len(), 2);
        let publication = driver.publication().load();
        assert_eq!(publication.tick, session.tick());
        assert_eq!(publication.desks.len(), 2);
        assert_eq!(session.state.runtime_desk_serial_cursors.len(), 2);
        for (stock, cursor) in expected_cursors {
            let index = stock.0 as usize;
            let shard = publication
                .desks
                .iter()
                .find(|row| usize::from(row.stock) == index)
                .unwrap();
            assert_eq!(
                driver.desk_next_serials()[index],
                *cursor,
                "真实desk有效游标不得分歧"
            );
            assert_eq!(session.state.runtime_desk_serial_cursors[index], *cursor);
            assert_eq!(shard.next_serial, *cursor);
            assert_eq!(shard.resting.as_ref(), &facts[index]);
            assert_eq!(shard.resting_count as usize, facts[index].len());
            let mirror = &session.state.desk_book_mirror[stock];
            let expected_orders = facts[index]
                .iter()
                .map(|fact| {
                    (
                        driver.account_by_owner(fact.owner).unwrap(),
                        fact.side,
                        Money::from_cents(fact.price),
                        fact.qty,
                        fact.order_serial,
                        fact.filled_value,
                        fact.filled_qty,
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(
                mirror.orders, expected_orders,
                "完整镜像必须来自真实desk事实"
            );
        }
        live_facts = Some(facts);
    } else {
        assert!(
            session.runtime_driver.is_none(),
            "恢复静点不得暗用live owner"
        );
        assert!(session.state.runtime_desk_serial_cursors.is_empty());
    }
    assert_eq!(session.state.markets.len(), 2);
    assert_eq!(session.state.desk_book_mirror.len(), 2);
    for (stock, market) in &session.state.markets {
        let raw_cursor = raw.book_next_sequences[stock];
        let effective = expected_cursors[stock];
        assert_eq!(
            market.book_next_sequence(),
            raw_cursor,
            "每侧退役旧簿游标按原save事实精确核对"
        );
        assert!(raw_cursor <= effective);
        assert!(
            market.resting_orders().is_empty(),
            "非空旧簿不得使用此语义投影"
        );
        assert!(
            market.filled_orders().is_empty(),
            "旧成交身份不得被归一隐藏"
        );
        let mirror = &session.state.desk_book_mirror[stock];
        assert_eq!(mirror.next_serial, effective);
        assert_eq!(mirror.last_price, market.last_price());
        let saved_orders = saved.resting_orders[stock]
            .iter()
            .map(|order| {
                (
                    order.owner,
                    order.side,
                    order.price,
                    order.qty,
                    order.id.0,
                    order.filled_value.cents(),
                    order.filled_qty,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(mirror.orders, saved_orders);
        for order in &saved.resting_orders[stock] {
            assert_eq!(
                order.seq,
                crate::runtime::group::desk_order_serial(order.id.0)
            );
            assert!(order.seq < effective);
        }
        if let Some(facts) = &live_facts {
            assert_eq!(
                saved.resting_orders[stock].len(),
                facts[stock.0 as usize].len()
            );
        }
        let index = labels
            .iter()
            .position(|label| label == &format!("market[{stock:?}].hash_projection"))
            .unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&fields[index]).unwrap();
        assert_eq!(
            serde_json::to_vec(&value).unwrap(),
            fields[index],
            "已映射field须可逐bytes往返"
        );
        assert_eq!(value.as_array().unwrap().len(), 2);
        assert_eq!(value[0], serde_json::to_value(stock).unwrap());
        assert_eq!(value[1].as_array().unwrap().len(), 7);
        let book = value[1][6].as_array_mut().unwrap();
        assert_eq!(book.len(), 6);
        assert_eq!(book[1].as_u64(), Some(raw_cursor));
        assert!(book[3].as_array().unwrap().is_empty());
        assert!(book[4].as_array().unwrap().is_empty());
        assert!(book[5].as_array().unwrap().is_empty());
        book[1] = serde_json::json!(effective);
        fields[index] = serde_json::to_vec(&value).unwrap();
    }
    let index = labels
        .iter()
        .position(|label| label == "counters.next_order_id_tick_day_seq_next_receipt_base")
        .unwrap();
    let mut counters: serde_json::Value = serde_json::from_slice(&fields[index]).unwrap();
    assert_eq!(serde_json::to_vec(&counters).unwrap(), fields[index]);
    assert_eq!(counters.as_array().unwrap().len(), 5);
    assert_eq!(counters[0].as_u64(), Some(raw.next_order_id));
    counters[0] = serde_json::json!(effective_id);
    fields[index] = serde_json::to_vec(&counters).unwrap();
    let save = serde_json::to_vec(&saved).unwrap();
    assert_eq!(
        session.business_state_hash().unwrap(),
        hash,
        "save/真实只读观察不得改变raw hash"
    );
    assert_eq!(
        session
            .clone_for_tick_shadow()
            .unwrap()
            .business_state_hash()
            .unwrap(),
        hash
    );
    eprintln!("G8 raw hash={hash:?}，live={live}，old cursors={:?}，raw ID={}，effective={expected_cursors:?}", raw.book_next_sequences, raw.next_order_id);
    NativeSemanticProjection {
        labels,
        fields,
        save,
    }
}

fn assert_native_semantics_equal(
    left: &NativeSemanticProjection,
    right: &NativeSemanticProjection,
) {
    assert_eq!(left.labels, right.labels);
    for (index, label) in left.labels.iter().enumerate() {
        if left.fields[index] != right.fields[index] {
            let left_value = serde_json::from_slice(&left.fields[index]).unwrap();
            let right_value = serde_json::from_slice(&right.fields[index]).unwrap();
            print_json_differences(label, &left_value, &right_value);
        }
    }
    if left.save != right.save {
        print_json_differences(
            "完整SaveSlot",
            &serde_json::from_slice(&left.save).unwrap(),
            &serde_json::from_slice(&right.save).unwrap(),
        );
    }
    assert_eq!(
        left, right,
        "完整36 field bytes与完整SaveSlot除已证明三叶表示映射外必须相等"
    );
}

fn assert_native_restore_stable(
    session: &GameSession,
    raw: &SaveSlot,
    cursors: &std::collections::BTreeMap<StockId, u64>,
) {
    let live = native_semantic_projection(session, raw, cursors, true);
    let saved = session.save().unwrap();
    let restored = restore_exactly(&saved);
    let restored_projection = native_semantic_projection(&restored, &saved, cursors, false);
    assert_native_semantics_equal(&live, &restored_projection);
}

fn native_continuation(native: &SaveSlot, serial: &SaveSlot) -> (GameSession, GameSession) {
    let mut native = restore_exactly(native);
    let mut serial = restore_exactly(serial);
    native.end_civil_day().unwrap();
    serial.end_civil_day().unwrap();
    assert_eq!(native.civil_date(), serial.civil_date());
    (native, serial)
}

fn assert_same_delivery(
    native: &GameSession,
    serial: &GameSession,
    native_events: &[Event],
    serial_events: &[Event],
    expected_tick: u64,
) {
    assert_eq!(
        serde_json::to_value(native_events).unwrap(),
        serde_json::to_value(serial_events).unwrap()
    );
    // 两侧均从日终档恢复、完成一次 civil INFO 后真实走 native 首拍。
    let expected_origin = crate::RuntimeMirrorOrigin {
        authority_epoch: 1,
        install_sequence: 3,
        source_tick: expected_tick,
        source_generation: 1,
        day_end_stage: crate::RuntimeMirrorDayEndStage::Tick,
        non_tick_revision: 1,
        mirror_phase: TradingPhase::Continuous,
    };
    assert_snapshot_with_expected_origins(
        native.snapshot_inner(true, true),
        serial.snapshot_inner(true, true),
        expected_origin,
        expected_origin,
    );
    assert_eq!(native.state.plans, serial.state.plans);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Continuation {
    Restore,
    Fifo,
}

fn assert_save_restore_and_native_continuation(
    native_save: &SaveSlot,
    serial_save: &SaveSlot,
    expected_tick: u64,
    continuation: Continuation,
) {
    let cursors = native_save
        .book_next_sequences
        .keys()
        .map(|stock| (*stock, 1))
        .collect::<std::collections::BTreeMap<_, _>>();
    if continuation != Continuation::Fifo {
        let (mut native, mut serial) = native_continuation(native_save, serial_save);
        let native_events = native.step().unwrap();
        let serial_events = serial.step().unwrap();
        assert_same_delivery(
            &native,
            &serial,
            &native_events,
            &serial_events,
            expected_tick,
        );
        let native_projection = native_semantic_projection(&native, native_save, &cursors, true);
        let serial_projection = native_semantic_projection(&serial, serial_save, &cursors, true);
        assert_native_semantics_equal(&native_projection, &serial_projection);
        assert_native_restore_stable(&native, native_save, &cursors);
        assert_native_restore_stable(&serial, serial_save, &cursors);

        return;
    }

    // 从各侧原完整日结事实独立恢复，在下一日首拍真实受理，保留非日末在场单与FIFO。
    let (mut native, mut serial) = native_continuation(native_save, serial_save);
    let stock = *native_save.book_next_sequences.keys().next_back().unwrap();
    assert_eq!(stock, StockId(1));
    for session in [&mut native, &mut serial] {
        assert!(session.account(AccountId(0)).unwrap().cash().cents() > 301_000);
        for qty in [100, 200] {
            session
                .enqueue_player_intent(
                    AccountId(0),
                    Intent::PlaceLimit {
                        code: stock,
                        side: Side::Buy,
                        price: LimitPrice::Fixed(Money::from_cents(1_000)),
                        qty,
                    },
                )
                .unwrap();
        }
    }
    let native_events = native.step().unwrap();
    let serial_events = serial.step().unwrap();
    assert_same_delivery(
        &native,
        &serial,
        &native_events,
        &serial_events,
        expected_tick,
    );
    let accepted = native_events
        .iter()
        .filter_map(|event| match event {
            Event::OrderAccepted {
                account,
                code,
                id,
                side,
                price,
                remaining_qty,
                ..
            } => Some((*account, *code, *id, *side, *price, *remaining_qty)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let first = crate::runtime::group::desk_order_id(1, 1);
    assert_eq!(
        accepted,
        vec![
            (
                AccountId(0),
                stock,
                OrderId(first),
                Side::Buy,
                Money::from_cents(1_000),
                100
            ),
            (
                AccountId(0),
                stock,
                OrderId(first + 1),
                Side::Buy,
                Money::from_cents(1_000),
                200
            ),
        ]
    );
    let saved = native.save().unwrap();
    let orders = &saved.resting_orders[&stock];
    assert_eq!(orders.len(), 2);
    assert_eq!(
        orders
            .iter()
            .map(|order| (
                order.id,
                order.seq,
                order.qty,
                order.original_qty,
                order.filled_qty,
                order.filled_value
            ))
            .collect::<Vec<_>>(),
        vec![
            (OrderId(first), 1, 100, 100, 0, Money::ZERO),
            (OrderId(first + 1), 2, 200, 200, 0, Money::ZERO),
        ],
        "真实同价在场顺序/数量与取号不得漂移或复用"
    );
    let mut next_cursors = cursors;
    next_cursors.insert(stock, 3);
    assert_native_semantics_equal(
        &native_semantic_projection(&native, native_save, &next_cursors, true),
        &native_semantic_projection(&serial, serial_save, &next_cursors, true),
    );
    let native_accounts = native
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .world_accounts_snapshot()
        .unwrap();
    let serial_accounts = serial
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .world_accounts_snapshot()
        .unwrap();
    assert_eq!(
        native_accounts, serial_accounts,
        "真实组完整working账户事实必须一致"
    );
    assert_native_restore_stable(&native, native_save, &next_cursors);
    assert_native_restore_stable(&serial, serial_save, &next_cursors);
}

fn run_nonzero_plan_parity(
    exchange: CalendarExchange,
    continuation_phase: usize,
    continuation: Continuation,
) {
    let first = if exchange == CalendarExchange::Sse {
        StockExchange::Shanghai
    } else {
        StockExchange::Shenzhen
    };
    let initial = fixture::restore(exchange, first, false);
    let (source, closed, open) = (initial.session, initial.closed, initial.open);
    assert_eq!(source.civil_date(), fixture::date("2030-01-02"));
    let days = source.stock_calendar_days().unwrap();
    assert_eq!(days[&closed], None);
    assert_eq!(days[&open], Some(0));
    let mut edited = source.save().unwrap();
    let make_open = |stock, day| crate::plans::PlanOpen {
        account: AccountId(0),
        code: stock,
        direction: Side::Buy,
        target: crate::plans::PlanTarget::ShareCount(100),
        opinion: crate::plans::PlanOpinion {
            signal_score_bp: 3_000,
            source: crate::plans::OpinionSource::Blended,
        },
        confidence_bp: 8_000,
        urgency: crate::plans::Urgency::Normal,
        horizon_trading_days: 1,
        created_trading_day: day,
    };
    // 在首次restore前编辑完整初始档；closed的合法day0初态并不代表Jan2已交易。
    let closed_id = edited.plans.create(make_open(closed, 0)).unwrap();
    let open_id = edited.plans.create(make_open(open, 0)).unwrap();
    let closed_initial = edited.plans.plan(closed_id).unwrap().clone();
    let mut ids = vec![closed_id, open_id];
    for phase in 0..=continuation_phase {
        let bytes = serde_json::to_vec(&edited).unwrap();
        let decoded = crate::decode_save_slot(&bytes, &crate::SaveDecodeLimits::default()).unwrap();
        let mut native = GameSession::restore(&decoded).unwrap();
        let mut serial = GameSession::restore(&decoded).unwrap();
        assert_eq!(native.state.plans, serial.state.plans);
        assert_eq!(native.state.plans.plan_ids().collect::<Vec<_>>(), ids);
        assert!(native.runtime_driver.is_none());
        assert!(serial.runtime_driver.is_none());
        for index in 0..fixture::TICKS_PER_DAY {
            let native_events = native.step().unwrap();
            let serial_events = serial.step_pipeline_contract().unwrap();
            assert!(native.runtime_driver.is_some(), "native必须真实使用组权威");
            assert!(
                serial.runtime_driver.is_none(),
                "serial不得暗用native World使对照自证"
            );
            let mut driver = native.runtime_driver.as_ref().unwrap().lock().unwrap();
            let shards = driver.collect_plans().unwrap();
            assert_eq!(shards.len(), 1);
            assert_eq!(
                shards[0].1, native.state.plans,
                "镜像必须等真实组完整Book，不独立重算覆盖"
            );
            drop(driver);
            assert_eq!(
                native.state.plans, serial.state.plans,
                "每拍完整非零计划/游标/状态一致"
            );
            assert_eq!(
                serde_json::to_value(native_events).unwrap(),
                serde_json::to_value(serial_events).unwrap()
            );
            let completed_ticks = index + 1;
            let expected_tick = phase as u64 * fixture::TICKS_PER_DAY + completed_ticks;
            assert_snapshot_with_expected_origins(
                native.snapshot_inner(true, true),
                serial.snapshot_inner(true, true),
                crate::RuntimeMirrorOrigin {
                    authority_epoch: 1,
                    install_sequence: completed_ticks + 1,
                    source_tick: expected_tick,
                    source_generation: completed_ticks + completed_ticks / fixture::TICKS_PER_DAY,
                    day_end_stage: if completed_ticks == fixture::TICKS_PER_DAY {
                        crate::RuntimeMirrorDayEndStage::RetainedPublished
                    } else {
                        crate::RuntimeMirrorDayEndStage::Tick
                    },
                    non_tick_revision: 0,
                    mirror_phase: TradingPhase::Continuous,
                },
                crate::RuntimeMirrorOrigin {
                    authority_epoch: 0,
                    install_sequence: 0,
                    source_tick: expected_tick,
                    source_generation: 0,
                    day_end_stage: crate::RuntimeMirrorDayEndStage::Tick,
                    non_tick_revision: 0,
                    mirror_phase: TradingPhase::Continuous,
                },
            );
            if phase == 0 {
                assert_eq!(
                    native.state.plans.plan(closed_id).unwrap(),
                    &closed_initial,
                    "Jan2 closed整日不老化、不添事件"
                );
            }
            if index + 1 < fixture::TICKS_PER_DAY {
                let active_id = if phase == 0 { open_id } else { ids[2] };
                assert_eq!(
                    native.state.plans.plan(active_id).unwrap().status(),
                    crate::plans::PlanStatus::Active
                );
            }
        }
        let expected_terminal = crate::plans::PlanStatus::Terminated {
            reason: crate::plans::TerminationReason::HorizonExpired,
        };
        assert_eq!(
            native.state.plans.plan(open_id).unwrap().status(),
            expected_terminal
        );
        for (id, own_day) in if phase == 0 {
            vec![(open_id, 0)]
        } else {
            vec![(closed_id, 0), (open_id, 0), (ids[2], 1)]
        } {
            let plan = native.state.plans.plan(id).unwrap();
            assert_eq!(plan.created_trading_day(), own_day);
            assert_eq!(plan.last_event_trading_day(), own_day);
            assert_eq!(plan.status(), expected_terminal);
            assert!(plan.execution().is_none());
        }
        assert_eq!(native.state.plans.plan_ids().collect::<Vec<_>>(), ids);
        assert_eq!(
            native.state.plans.owner_sequences().collect::<Vec<_>>(),
            edited.plans.owner_sequences().collect::<Vec<_>>()
        );
        assert_eq!(
            native.business_state_hash().unwrap(),
            serial.business_state_hash().unwrap()
        );
        // save()用于完整SaveSlot观察值比较，不冒公共日终档已发布。
        let native_save = native.save().unwrap();
        let serial_save = serial.save().unwrap();
        assert_phase_save_projection(&native_save, &serial_save, closed, open, phase);
        if phase == continuation_phase {
            assert_save_restore_and_native_continuation(
                &native_save,
                &serial_save,
                (phase as u64 + 1) * fixture::TICKS_PER_DAY + 1,
                continuation,
            );
        }
        if phase == 0 {
            let old_open_terminal = native.state.plans.plan(open_id).unwrap().clone();
            native.end_civil_day().unwrap();
            assert_eq!(native.civil_date(), fixture::date("2030-01-03"));
            let days = native.stock_calendar_days().unwrap();
            assert_eq!(days[&closed], Some(0));
            assert_eq!(days[&open], Some(1));
            // 仅native完成真实civil；下一日严格decode同字节重建两个分支，不冒serial civil对拍。
            edited = native.save().unwrap();
            assert_eq!(edited.plans.plan(closed_id).unwrap(), &closed_initial);
            assert_eq!(edited.plans.plan(open_id).unwrap(), &old_open_terminal);
            ids.push(edited.plans.create(make_open(open, 1)).unwrap());
        }
    }
}

// 每个exchange/phase有独立十秒case预算，全部恢复与委托断言均保留。
#[test]
fn mixed_nonzero_investor_plans_keep_group_mirror_and_serial_own_day_events_equal() {
    run_nonzero_plan_parity(CalendarExchange::Sse, 0, Continuation::Restore);
}

#[test]
fn mixed_sse_closed_nonzero_plan_second_day_restore_is_equal() {
    run_nonzero_plan_parity(CalendarExchange::Sse, 1, Continuation::Restore);
}

#[test]
fn mixed_szse_closed_nonzero_plan_first_day_restore_is_equal() {
    run_nonzero_plan_parity(CalendarExchange::Szse, 0, Continuation::Restore);
}

#[test]
fn mixed_szse_closed_nonzero_plan_second_day_restore_is_equal() {
    run_nonzero_plan_parity(CalendarExchange::Szse, 1, Continuation::Restore);
}

#[test]
fn mixed_sse_closed_nonzero_plan_second_day_fifo_is_equal() {
    run_nonzero_plan_parity(CalendarExchange::Sse, 1, Continuation::Fifo);
}

#[test]
fn mixed_sse_closed_nonzero_plan_first_day_fifo_is_equal() {
    run_nonzero_plan_parity(CalendarExchange::Sse, 0, Continuation::Fifo);
}

#[test]
fn mixed_szse_closed_nonzero_plan_first_day_fifo_is_equal() {
    run_nonzero_plan_parity(CalendarExchange::Szse, 0, Continuation::Fifo);
}

#[test]
fn mixed_szse_closed_nonzero_plan_second_day_fifo_is_equal() {
    run_nonzero_plan_parity(CalendarExchange::Szse, 1, Continuation::Fifo);
}
