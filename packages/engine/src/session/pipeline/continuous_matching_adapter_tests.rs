use super::continuous_matching_adapter::prepare_incremental_continuous_inputs;
use super::*;
use crate::{AccountId, Money, Order, OrderId, Side, StockId, TradingPhase};

#[test]
fn continuous_inputs_capture_all_markets_without_preloaded_operations() {
    let game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    // 数值身份后无字符串可断言：two_stock_quote_setup 目录按代码序 rank 取 0/1。
    let first = StockId(0);
    let second = StockId(1);
    let before = game.session_state_hash().unwrap();

    let inputs = prepare_incremental_continuous_inputs(&game).unwrap();

    assert_eq!(
        inputs
            .iter()
            .map(|input| input.market.code().clone())
            .collect::<Vec<_>>(),
        vec![first, second]
    );
    assert!(inputs
        .iter()
        .all(|input| input.phase == TradingPhase::Continuous));
    assert!(inputs.iter().all(|input| input.envelopes.is_empty()));
    assert!(inputs.iter().all(|input| input.operations.is_empty()));
    for input in &inputs {
        assert_eq!(
            serde_json::to_vec(&input.config).unwrap(),
            serde_json::to_vec(&game.state.setup.config).unwrap()
        );
    }
    assert_eq!(game.session_state_hash().unwrap(), before);
}

#[test]
fn continuous_adapter_carries_only_tick_start_envelopes_matching_each_order_book() {
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    // 数值身份后无字符串可断言：two_stock_quote_setup 中 600889 按 rank 取 StockId(1)。
    let code = StockId(1);
    let order = resting_buy(OrderId(91), AccountId(1));
    game.state
        .markets
        .get_mut(&code)
        .unwrap()
        .place(order)
        .unwrap();
    game.hydrate_or_validate_envelope_ledger().unwrap();
    let inputs = prepare_incremental_continuous_inputs(&game).unwrap();

    let first = &inputs[0];
    let second = &inputs[1];
    assert!(first.envelopes.is_empty());
    assert_eq!(second.market.code(), &code);
    assert_eq!(second.envelopes.len(), 1);
    let snapshot = &second.envelopes[0];
    assert_eq!(snapshot.envelope.origin(), EnvelopeOrigin::TickStart);
    assert_eq!(snapshot.envelope.audit(), snapshot.audit);
    assert_eq!(snapshot.envelope.key().order, OrderId(91));
    assert_eq!(snapshot.audit.remaining_qty, 100);
}

#[test]
fn continuous_adapter_preserves_prior_fill_fee_audit_from_the_authoritative_ledger() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    // 数值身份后无字符串可断言：quote_setup 单股目录按 rank 取 StockId(0)。
    let code = StockId(0);
    let owner = AccountId(1);
    let order = Order {
        id: OrderId(92),
        side: Side::Buy,
        price: Money::from_cents(900),
        qty: 100,
        original_qty: 200,
        filled_qty: 100,
        filled_value: Money::from_cents(90_000),
        owner,
        seq: 0,
    };
    game.state
        .markets
        .get_mut(&code)
        .unwrap()
        .place(order)
        .unwrap();
    let nominal = FeeComponents {
        commission: Money::from_cents(500),
        stamp_tax: Money::ZERO,
        transfer_fee: Money::from_cents(1),
    };
    let cash = crate::session::buy_order_reservation(
        &game.state.setup.config,
        Money::from_cents(900),
        100,
        Money::from_cents(90_000),
    )
    .unwrap();
    game.state.envelope_ledger = EnvelopeLedger::new(
        0,
        [Envelope::tick_start_existing(
            EnvelopeKey {
                account: owner,
                stock: code,
                order: OrderId(92),
                side: Side::Buy,
            },
            cash,
            0,
            EnvelopeAudit {
                limit: Money::from_cents(900),
                remaining_qty: 100,
                filled_qty: 100,
                filled_value: Money::from_cents(90_000),
                nominal,
                charged: nominal,
            },
        )],
    )
    .unwrap();

    let inputs = prepare_incremental_continuous_inputs(&game).unwrap();

    assert_eq!(inputs[0].envelopes[0].audit.nominal, nominal);
    assert_eq!(inputs[0].envelopes[0].audit.charged, nominal);
}

#[test]
fn continuous_adapter_rejects_residual_auction_orders_in_continuous_phase() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    // 数值身份后无字符串可断言：quote_setup 单股目录按 rank 取 StockId(0)。
    let code = StockId(0);
    game.state
        .auction_orders
        .entry(code)
        .or_default()
        .push(crate::session::AuctionOrderSnap {
            owner: AccountId(0),
            side: Side::Buy,
            limit: Money::from_cents(900),
            qty: 100,
            order_id: 93,
        });

    let error = prepare_incremental_continuous_inputs(&game).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("residual auction")
                && location == "pipeline::continuous_matching_adapter"
    ));
}

#[test]
fn continuous_adapter_rejects_a_cross_stock_market_map() {
    let mut game = GameSession::new(
        crate::session::npc_working_quote_tests::two_stock_quote_setup(),
        42,
    )
    .unwrap();
    // 数值身份后无字符串可断言：two_stock_quote_setup 目录按代码序 rank 取 0/1。
    let first = StockId(0);
    let second = StockId(1);
    let first_market = game.state.markets.remove(&first).unwrap();
    game.state.markets.insert(second, first_market);

    let error = prepare_incremental_continuous_inputs(&game).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("market map key")
                && location == "pipeline::continuous_matching_adapter"
    ));
}

#[test]
fn continuous_adapter_rejects_missing_or_stale_ledger_evidence() {
    let mut missing =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    // 数值身份后无字符串可断言：quote_setup 单股目录按 rank 取 StockId(0)。
    let code = StockId(0);
    missing
        .state
        .markets
        .get_mut(&code)
        .unwrap()
        .place(resting_buy(OrderId(101), AccountId(1)))
        .unwrap();
    let error = prepare_incremental_continuous_inputs(&missing).unwrap_err();
    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description == "continuous order books contain an unledgered order"
                && location == "pipeline::continuous_matching_adapter"
    ));

    missing.hydrate_or_validate_envelope_ledger().unwrap();
    missing
        .state
        .markets
        .get_mut(&code)
        .unwrap()
        .cancel(OrderId(101))
        .unwrap();
    let error = prepare_incremental_continuous_inputs(&missing).unwrap_err();
    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description == "报价过期后的 envelope ledger 缺少对应活动订单簿"
                && location == "pipeline::continuous_matching_adapter"
    ));
}

#[test]
fn continuous_adapter_rejects_underfunded_live_resources_with_matching_order_fields() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    // 数值身份后无字符串可断言：quote_setup 单股目录按 rank 取 StockId(0)。
    let code = StockId(0);
    let owner = AccountId(1);
    game.state
        .markets
        .get_mut(&code)
        .unwrap()
        .place(resting_buy(OrderId(102), owner))
        .unwrap();
    game.state.envelope_ledger = EnvelopeLedger::new(
        0,
        [Envelope::tick_start_existing(
            EnvelopeKey {
                account: owner,
                stock: code,
                order: OrderId(102),
                side: Side::Buy,
            },
            Money::from_cents(1),
            0,
            EnvelopeAudit {
                limit: Money::from_cents(900),
                remaining_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        )],
    )
    .unwrap();

    let error = prepare_incremental_continuous_inputs(&game).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("live envelope resources")
                && location == "pipeline::continuous_matching_adapter"
    ));
}

#[test]
fn continuous_adapter_rejects_same_tick_envelopes_in_the_live_ledger() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    // 数值身份后无字符串可断言：quote_setup 单股目录按 rank 取 StockId(0)。
    let code = StockId(0);
    game.state.envelope_ledger = EnvelopeLedger::new(
        0,
        [Envelope::created_at_validation(
            EnvelopeKey {
                account: AccountId(0),
                stock: code,
                order: OrderId(44),
                side: Side::Buy,
            },
            Money::from_cents(90_000),
            0,
            EnvelopeAudit {
                limit: Money::from_cents(900),
                remaining_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        )],
    )
    .unwrap();

    let error = prepare_incremental_continuous_inputs(&game).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("TickStart")
                && location == "pipeline::continuous_matching_adapter"
    ));
}

#[test]
fn continuous_adapter_rejects_non_continuous_phase() {
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(3), 42).unwrap();
    assert_eq!(game.phase(), TradingPhase::CallAuction);

    let error = prepare_incremental_continuous_inputs(&game).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { description, location }
            if description.contains("Continuous")
                && location == "pipeline::continuous_matching_adapter"
    ));
}

fn resting_buy(id: OrderId, owner: AccountId) -> Order {
    Order {
        id,
        side: Side::Buy,
        price: Money::from_cents(900),
        qty: 100,
        original_qty: 100,
        filled_qty: 0,
        filled_value: Money::ZERO,
        owner,
        seq: 0,
    }
}

#[test]
fn adapter_validates_order_resources_without_materializing_an_extra_projection_envelope() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    // 数值身份后无字符串可断言：quote_setup 单股目录按 rank 取 StockId(0)。
    let code = StockId(0);
    game.state
        .markets
        .get_mut(&code)
        .unwrap()
        .place(resting_buy(OrderId(401), AccountId(1)))
        .unwrap();
    game.hydrate_or_validate_envelope_ledger().unwrap();
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap();
    let (inputs, owned_projections) = pool.install(|| {
        crate::session::envelope_projection::OWNED_PROJECTION_COUNT.with(|count| count.set(0));
        let inputs = prepare_incremental_continuous_inputs(&game).unwrap();
        let count =
            crate::session::envelope_projection::OWNED_PROJECTION_COUNT.with(|count| count.get());
        (inputs, count)
    });
    assert_eq!(
        owned_projections, 0,
        "adapter 只读取投影资源，不应额外构造 owned Envelope"
    );
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].envelopes.len(), 1);
    let envelope = &inputs[0].envelopes[0].envelope;
    assert_eq!(
        envelope,
        game.state.envelope_ledger.get(envelope.key()).unwrap()
    );
}

#[test]
fn one_dense_stock_input_validates_on_multiple_workers_and_keeps_the_authoritative_payload() {
    use std::sync::Mutex;
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    // 数值身份后无字符串可断言：quote_setup 单股目录按 rank 取 StockId(0)。
    let code = StockId(0);
    for index in 0..1024 {
        game.state
            .markets
            .get_mut(&code)
            .unwrap()
            .place(resting_buy(OrderId(1000 + index), AccountId(1)))
            .unwrap();
    }
    game.hydrate_or_validate_envelope_ledger().unwrap();
    let before = game.session_state_hash().unwrap();
    let workers = Mutex::new(std::collections::BTreeSet::new());
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    let inputs = pool
        .install(|| {
            super::continuous_matching_adapter::prepare_inputs_with_observer(&game, || {
                workers
                    .lock()
                    .unwrap()
                    .insert(rayon::current_thread_index().unwrap());
            })
        })
        .unwrap();
    assert!(
        workers.into_inner().unwrap().len() > 1,
        "单股 dense input 的独立行校验必须使用多个 worker，而不只靠多股并行"
    );
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].envelopes.len(), 1024);
    for snapshot in &inputs[0].envelopes {
        assert_eq!(
            &snapshot.envelope,
            game.state
                .envelope_ledger
                .get(snapshot.envelope.key())
                .unwrap()
        );
        assert_eq!(snapshot.audit, snapshot.envelope.audit());
    }
    assert_eq!(game.session_state_hash().unwrap(), before);
}

#[test]
fn dense_adapter_keeps_book_order_errors_when_envelope_keys_have_reverse_order() {
    for count in [511, 512, 513] {
        let mut source =
            GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
        // 数值身份后无字符串可断言：quote_setup 单股目录按 rank 取 StockId(0)。
        let code = StockId(0);
        for index in 0..count {
            source
                .state
                .markets
                .get_mut(&code)
                .unwrap()
                .place(resting_buy(OrderId(5000 - index), AccountId(1)))
                .unwrap();
        }
        source.hydrate_or_validate_envelope_ledger().unwrap();
        let first = EnvelopeKey {
            account: AccountId(1),
            stock: code.clone(),
            order: OrderId(5000),
            side: Side::Buy,
        };
        let last = EnvelopeKey {
            order: OrderId(5001 - count),
            ..first.clone()
        };
        for first_missing in [false, true] {
            let mut game = source.clone_for_tick_shadow().unwrap();
            let (missing, underfunded) = if first_missing {
                (&first, &last)
            } else {
                (&last, &first)
            };
            let audit = game.state.envelope_ledger.get(underfunded).unwrap().audit();
            game.state.envelope_ledger.envelopes.insert(
                underfunded.clone(),
                Envelope::tick_start_existing(underfunded.clone(), Money::from_cents(1), 0, audit),
            );
            game.state.envelope_ledger.envelopes.remove(missing);
            let before = game.state.envelope_ledger.clone();
            for threads in [1, 4] {
                let error = rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .unwrap()
                    .install(|| prepare_incremental_continuous_inputs(&game))
                    .unwrap_err();
                assert_eq!(
                    error,
                    StepFatal::InvariantViolation {
                        description: if first_missing {
                            "continuous order books contain an unledgered order"
                        } else {
                            "continuous order books disagree with live envelope resources"
                        }
                        .to_owned(),
                        location: "pipeline::continuous_matching_adapter".to_owned(),
                    },
                    "{count}行/{threads}worker应按book序选择首错"
                );
                assert_eq!(game.state.envelope_ledger, before);
            }
        }
    }
}

#[test]
fn dense_descending_books_emit_ascending_envelope_keys() {
    // 1024 单按 id 降序挂入同一价位，簿序为降序；输出必须仍按完整 EnvelopeKey 升序，
    // 供下游 PrivateStockLedger / 校验按序消费。融合排序方案不得改变这一契约。
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    // 数值身份后无字符串可断言：quote_setup 单股目录按 rank 取 StockId(0)。
    let code = StockId(0);
    for index in 0..1024 {
        game.state
            .markets
            .get_mut(&code)
            .unwrap()
            .place(resting_buy(OrderId(9000 - index), AccountId(1)))
            .unwrap();
    }
    game.hydrate_or_validate_envelope_ledger().unwrap();
    let inputs = prepare_incremental_continuous_inputs(&game).unwrap();
    let envelopes = &inputs[0].envelopes;
    assert_eq!(envelopes.len(), 1024);
    for pair in envelopes.windows(2) {
        assert!(
            pair[0].envelope.key() < pair[1].envelope.key(),
            "adapter 输出必须保持 envelope key 升序"
        );
    }
}

#[test]
fn prepare_stock_input_stops_using_incremental_identity_set() {
    use super::continuous_matching_adapter::IDENTITY_SET_INSERTS;
    use std::sync::atomic::Ordering;
    // 回归防线：本测试在融合排序重构前以 1024 次 BTreeSet 增量插入亮过真实红灯（红→绿证据）；
    // 重构后 IDENTITY_SET_INSERTS 不再有递增点，若查重被改回增量插入并重新接线计数，此处会再次亮红。
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    // 数值身份后无字符串可断言：quote_setup 单股目录按 rank 取 StockId(0)。
    let code = StockId(0);
    for index in 0..1024 {
        game.state
            .markets
            .get_mut(&code)
            .unwrap()
            .place(resting_buy(OrderId(7000 + index), AccountId(1)))
            .unwrap();
    }
    game.hydrate_or_validate_envelope_ledger().unwrap();
    let before = IDENTITY_SET_INSERTS.load(Ordering::SeqCst);
    let inputs = prepare_incremental_continuous_inputs(&game).unwrap();
    assert_eq!(inputs[0].envelopes.len(), 1024);
    let delta = IDENTITY_SET_INSERTS.load(Ordering::SeqCst) - before;
    assert_eq!(
        delta, 0,
        "查重应由一次排序的相邻比较承担，而不是 O(n log n) 的 BTreeSet 增量插入"
    );
}
