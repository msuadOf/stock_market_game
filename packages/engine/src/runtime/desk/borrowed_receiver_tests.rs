//! 真实交易部收件入口的 receiver 所有权验证。
use super::*;
use std::sync::atomic::AtomicBool;

fn setup() -> DeskSetup {
    DeskSetup {
        stock: 0, exchange: StockExchange::Shanghai, initial_price: 1000, limit_bps: 1000,
        max_order_qty_limit: 1_000_000, max_order_qty_market: 150_000,
        ticks_per_day: 240, auction_ticks: 0, closing_auction_ticks: 0,
        order_ttl_ticks: 10, history_len: 8, initial_tick: 0, initial_orders: Vec::new(),
        initial_recent_prices: Vec::new(), initial_daily_bars: Vec::new(), initial_traded_days: 0,
        initial_market: None, initial_public_cutoff: 0, initial_next_serial: 1,
    }
}

fn run(first: DeskMsg) -> (Result<(), RuntimeError>, Receiver<DeskMsg>, Vec<UpMsg>) {
    let (tx, rx) = std::sync::mpsc::channel();
    let (group_tx, _group_rx) = std::sync::mpsc::channel();
    let (up_tx, up_rx) = crate::libos::mailbox::channel();
    tx.send(first).unwrap();
    tx.send(DeskMsg::Ops {
        tick: 1, round: 0, group: 0, final_flush: true,
        ops: vec![DeskIn::Place {
            owner: (0, 3), side: Side::Buy, price: RoutedPrice::Fixed(100), qty: 100, purse: 10005,
            #[cfg(feature = "simulation-diagnostics")]
            source: super::super::diagnostics::OperationSource::PlayerExternal,
        }],
    }).unwrap();
    drop(tx);
    let handle = std::thread::spawn(move || {
        super::super::worker_mailbox::handoff_after_worker(rx, |rx| desk_main_borrowed(setup(), 1, Arc::new(GameConfig::proposed_defaults()), rx, vec![group_tx], super::super::worker_context::WorkerContext {
            up_tx, abort: Arc::new(AtomicBool::new(false)), shutdown: Arc::new(AtomicBool::new(false)),
            capture: Arc::new(std::sync::Mutex::new(crate::verification_evidence::current_phase_timing_binding())),
        }))
    });
    let (result, remaining) = handle.join().unwrap().into_parts();
    (result, remaining, up_rx.try_iter().collect())
}

fn assert_remaining(rx: &Receiver<DeskMsg>) {
    match rx.try_recv().unwrap() {
        DeskMsg::Ops { tick, round, group, ops, final_flush } => {
            assert_eq!((tick, round, group, final_flush), (1, 0, 0, true));
            assert!(matches!(&ops[..], [DeskIn::Place { owner:(0, 3), side:Side::Buy, price:RoutedPrice::Fixed(100), qty:100, purse:10005, .. }]));
        }
        _ => panic!("后续下单资源载荷未保留"),
    }
    assert!(matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)));
}

#[test]
fn 正常关停上交最终态后保留后续下单载荷() {
    let (result, rx, replies) = run(DeskMsg::Shutdown);
    result.unwrap();
    assert!(matches!(&replies[..], [UpMsg::DeskFinal(_)]));
    assert_remaining(&rx);
}

#[test]
fn 错位批次显式失败后保留后续下单载荷() {
    let (result, rx, replies) = run(DeskMsg::Ops {tick:99, round:0, group:0, ops:Vec::new(), final_flush:true});
    assert!(result.is_err());
    assert!(matches!(&replies[..], [UpMsg::Failed { .. }, UpMsg::DeskFinal(_)]));
    assert_remaining(&rx);
}

#[test]
fn 真实交易部循环使用_libos_收件并保留后续载荷() {
    let domain = crate::libos::fault_domain::FaultDomain::new(2);
    let (tx, rx) = domain.channel::<DeskMsg>().unwrap();
    let waiting = rx.abort_handle();
    let (group_tx, _group_rx) = domain.channel::<super::super::protocol::GroupIn>().unwrap();
    let (up_tx, up_rx) = crate::libos::mailbox::channel();
    let worker = std::thread::spawn(move || {
        super::super::worker_mailbox::handoff_after_worker(rx, |rx| desk_main_borrowed(setup(), 1, Arc::new(GameConfig::proposed_defaults()), rx, vec![group_tx], super::super::worker_context::WorkerContext {
            up_tx, abort: Arc::new(AtomicBool::new(false)), shutdown: Arc::new(AtomicBool::new(false)),
            capture: Arc::new(std::sync::Mutex::new(crate::verification_evidence::current_phase_timing_binding())),
        }))
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while !waiting.waiting_registered() { assert!(std::time::Instant::now() < deadline, "交易部未登记等待"); std::thread::yield_now(); }
    tx.send(DeskMsg::Shutdown).unwrap();
    tx.send(DeskMsg::EchoCandles).unwrap();
    drop(tx);
    let (result, remaining) = worker.join().unwrap().into_parts();
    result.unwrap();
    assert!(matches!(up_rx.try_recv().unwrap(), UpMsg::DeskFinal(_)));
    assert!(matches!(remaining.try_recv().unwrap(), DeskMsg::EchoCandles));
}

#[test]
fn 真实交易部拒绝买单后回传断连保留退款诊断() {
    let date = crate::CivilDate::from_iso("2026-01-05").unwrap();
    let calendar = crate::runtime::calendar_test_support::open_tick(1, 240, 0, 0, 1, date);
    let (tx, rx) = std::sync::mpsc::channel();
    let (group_tx, group_rx) = std::sync::mpsc::channel();
    drop(group_rx);
    let (up_tx, up_rx) = crate::libos::mailbox::channel();
    tx.send(DeskMsg::BindCalendar { day:Arc::clone(calendar.day()) }).unwrap();
    tx.send(DeskMsg::Start { tick:1, calendar }).unwrap();
    tx.send(DeskMsg::Ops { tick:1, round:0, group:0, final_flush:true, ops:vec![DeskIn::Place {
        owner:(0, 0), side:Side::Buy, price:RoutedPrice::Fixed(2000), qty:100, purse:200_500,
        #[cfg(feature = "simulation-diagnostics")]
        source:super::super::diagnostics::OperationSource::PlayerExternal,
    }] }).unwrap();
    drop(tx);
    let worker = std::thread::spawn(move || desk_main_borrowed(setup(), 1, Arc::new(GameConfig::proposed_defaults()), &rx, vec![group_tx], super::super::worker_context::WorkerContext {
        up_tx, abort:Arc::new(AtomicBool::new(false)), shutdown:Arc::new(AtomicBool::new(false)),
        capture:Arc::new(std::sync::Mutex::new(crate::verification_evidence::current_phase_timing_binding())),
    }));
    let result = worker.join().unwrap();
    assert!(matches!(result, Err(RuntimeError::SendFailed { ref detail }) if detail == "交易部 0 → 组 0 实体批次发送失败"));
    assert!(matches!(up_rx.try_recv().unwrap(), UpMsg::Failed { who:WorkerId::Desk(0), .. }));
    let final_state = match up_rx.try_recv().unwrap() { UpMsg::DeskFinal(final_state) => final_state, other => panic!("必须上交真实 Final：{other:?}") };
    let retained = final_state.undelivered_batches.as_ref().expect("Final 缺少未发退款诊断");
    assert_eq!((retained.tick, retained.round, retained.phase, retained.first_failed_group), (1, 0, DeskFlushPhase::TickRound, 0));
    let batch = retained.by_group[0].as_ref().expect("退款批次缺失");
    assert!(batch.entities.contains(&super::super::Entity::CashRefund {local:0, amount:200_500}));
    assert!(batch.receipts.iter().any(|receipt| receipt.kind == super::super::ReceiptKind::PriceOutOfBand));
    assert!(matches!(up_rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)));
}

#[test]
fn 交易部退款和_final_均发送失败时随_join_保留载荷及首错() {
    let date = crate::CivilDate::from_iso("2026-01-05").unwrap();
    let calendar = crate::runtime::calendar_test_support::open_tick(1, 240, 0, 0, 1, date);
    let (tx, rx) = std::sync::mpsc::channel();
    let (group_tx, group_rx) = std::sync::mpsc::channel();
    drop(group_rx);
    let (up_tx, up_rx) = crate::libos::mailbox::channel();
    drop(up_rx);
    tx.send(DeskMsg::BindCalendar { day:Arc::clone(calendar.day()) }).unwrap();
    tx.send(DeskMsg::Start { tick:1, calendar }).unwrap();
    tx.send(DeskMsg::Ops { tick:1, round:0, group:0, final_flush:true, ops:vec![DeskIn::Place {
        owner:(0, 0), side:Side::Buy, price:RoutedPrice::Fixed(2000), qty:100, purse:200_500,
        #[cfg(feature = "simulation-diagnostics")]
        source:super::super::diagnostics::OperationSource::PlayerExternal,
    }] }).unwrap();
    drop(tx);
    let worker = std::thread::spawn(move || desk_main_borrowed(setup(), 1, Arc::new(GameConfig::proposed_defaults()), &rx, vec![group_tx], super::super::worker_context::WorkerContext {
        up_tx, abort:Arc::new(AtomicBool::new(false)), shutdown:Arc::new(AtomicBool::new(false)),
        capture:Arc::new(std::sync::Mutex::new(crate::verification_evidence::current_phase_timing_binding())),
    }));
    let error = worker.join().unwrap().unwrap_err();
    assert!(error.to_string().contains("实体批次发送失败"));
    match error {
        RuntimeError::DeskFinalUndelivered { who, primary:Some(primary), final_state } => {
            assert_eq!(who, WorkerId::Desk(0));
            assert!(matches!(*primary, RuntimeError::SendFailed { ref detail } if detail.contains("实体批次发送失败")));
            let retained = final_state.as_ref().undelivered_batches.as_ref().expect("未发退款诊断必须保留");
            assert!(retained.by_group[0].as_ref().unwrap().entities.contains(&super::super::Entity::CashRefund {local:0, amount:200_500}));
        }
        other => panic!("必须保留交易部 Final 与原错误：{other:?}"),
    }
}

#[test]
fn 真实交易部清簿退款发送失败保留drain诊断() {
    let date = crate::CivilDate::from_iso("2026-01-05").unwrap();
    let calendar = crate::runtime::calendar_test_support::open_tick(1, 240, 0, 0, 1, date);
    let mut setup = setup();
    setup.initial_orders.push(DeskRestingSeed {
        owner:(0, 0), side:Side::Buy, price:1000, qty:100, purse:100_500,
        filled_value:0, filled_qty:0, charged_commission:0, charged_stamp_tax:0,
        charged_transfer_fee:0, accepted_delivered:true, serial:1,
    });
    let (tx, rx) = std::sync::mpsc::channel();
    let (group_tx, group_rx) = std::sync::mpsc::channel();
    drop(group_rx);
    let (up_tx, up_rx) = crate::libos::mailbox::channel();
    tx.send(DeskMsg::BindCalendar { day:Arc::clone(calendar.day()) }).unwrap();
    tx.send(DeskMsg::Drain).unwrap();
    drop(tx);
    let worker = std::thread::spawn(move || desk_main_borrowed(setup, 1, Arc::new(GameConfig::proposed_defaults()), &rx, vec![group_tx], super::super::worker_context::WorkerContext {
        up_tx, abort:Arc::new(AtomicBool::new(false)), shutdown:Arc::new(AtomicBool::new(false)),
        capture:Arc::new(std::sync::Mutex::new(crate::verification_evidence::current_phase_timing_binding())),
    }));
    let result = worker.join().unwrap();
    assert!(matches!(result, Err(RuntimeError::SendFailed { ref detail }) if detail == "交易部 0 → 组 0 实体批次发送失败"));
    assert!(matches!(up_rx.try_recv().unwrap(), UpMsg::Failed { who:WorkerId::Desk(0), .. }));
    let final_state = match up_rx.try_recv().unwrap() { UpMsg::DeskFinal(state) => state, other => panic!("必须上交真实 Final：{other:?}") };
    let diagnostic = final_state.undelivered_batches.as_ref().expect("Final 缺少未发退款诊断");
    assert_eq!((diagnostic.tick, diagnostic.round, diagnostic.phase, diagnostic.first_failed_group), (0, 0, DeskFlushPhase::Drain, 0));
    let batch = diagnostic.by_group[0].as_ref().unwrap();
    assert!(batch.entities.contains(&super::super::Entity::CashRefund {local:0, amount:100_500}));
    assert!(batch.entities.iter().any(|entity| matches!(entity, super::super::Entity::Closed {local:0, ..})));
    assert!(matches!(up_rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)));
}

#[test]
fn 真实交易部_flush_经_libos_发送资源批次并唤醒接收者() {
    let (tx, rx) = crate::libos::mailbox::channel();
    let waiting = rx.abort_handle();
    let receiver = std::thread::spawn(move || crate::libos::mailbox::receive_interruptible(&rx, std::time::Duration::from_secs(1)));
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    while !waiting.waiting_registered() { assert!(std::time::Instant::now() < deadline); std::thread::yield_now(); }
    let mut state = DeskState::new(setup(), 1, Arc::new(GameConfig::proposed_defaults()));
    state.out[0].push(super::super::Entity::CashRefund {local:0, amount:500});
    assert_eq!(state.flush_batches(&[tx], 1, 0, DeskFlushPhase::TickRound).unwrap(), (1, 0, 0));
    match receiver.join().unwrap().unwrap_or_else(|_| panic!("LibOS 接收失败")) {
        super::super::protocol::GroupIn::Batch(batch) => {
            assert_eq!((batch.stock, batch.tick, batch.round), (0, 1, 0));
            assert_eq!(batch.entities, [super::super::Entity::CashRefund {local:0, amount:500}]);
        }
        _ => panic!("接收方未取得真实资源批次"),
    }
}

#[test]
fn 交易部_flush_发送失败保留失败批次及未发送尾部() {
    let (first, first_rx) = crate::libos::mailbox::channel();
    let (failed, failed_rx) = crate::libos::mailbox::channel();
    let (last, last_rx) = crate::libos::mailbox::channel();
    drop(failed_rx);
    let mut state = DeskState::new(setup(), 3, Arc::new(GameConfig::proposed_defaults()));
    for (index, amount) in [100, 200, 300].into_iter().enumerate() {
        state.out[index].push(super::super::Entity::CashRefund {local:index as u32, amount});
    }
    let receipt = super::super::DeskReceipt { order:(0, 17), owner:(1, 0), kind:super::super::ReceiptKind::PriceOutOfBand };
    state.receipts_out[1].push(receipt);
    let fill = super::super::FillFact { order:(0, 18), owner:(1, 0), side:Side::Buy, price:100, qty:100, commission:5, stamp_tax:0, transfer_fee:1 };
    let source = super::super::RepurchaseSourceFact {
        request_key:super::super::RepurchaseRequestKey { plan_id:crate::plans::PlanId {owner:crate::AccountId(79), sequence:3}, stock:crate::StockId(0), requested_trading_day:0 },
        owner:(1, 0), order_ref:(0, 18), kind:super::super::RepurchaseSourceKind::Fill(fill),
    };
    state.fills_out[1].push(fill);
    state.repurchase_out[1].push(source);
    state.fills_out[2].push(fill);
    state.receipts_out[2].push(receipt);
    state.repurchase_out[2].push(source);
    assert!(state.flush_batches(&[first, failed, last], 1, 0, DeskFlushPhase::TickRound).is_err());
    assert!(matches!(first_rx.try_recv().unwrap(), super::super::protocol::GroupIn::Batch(_)));
    assert!(state.out[0].is_empty());
    assert_eq!(state.out[1], [super::super::Entity::CashRefund {local:1, amount:200}]);
    assert_eq!(state.receipts_out[1], [receipt]);
    assert_eq!(state.fills_out[1], [fill]);
    assert_eq!(state.repurchase_out[1], [source]);
    assert_eq!(state.out[2], [super::super::Entity::CashRefund {local:2, amount:300}]);
    assert_eq!(state.fills_out[2], [fill]);
    assert_eq!(state.receipts_out[2], [receipt]);
    assert_eq!(state.repurchase_out[2], [source]);
    assert!(matches!(last_rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Disconnected)));
    state.rollback();
    let final_state = state.final_state();
    let retained = final_state.undelivered_batches.expect("回滚后 Final 必须保留发送失败诊断");
    assert_eq!((retained.tick, retained.round, retained.first_failed_group), (1, 0, 1));
    assert_eq!(retained.phase, DeskFlushPhase::TickRound);
    assert!(retained.by_group[0].is_none(), "已成功发送的批次不能再次保留");
    let failed = retained.by_group[1].as_ref().expect("缺少失败组批次");
    assert_eq!((failed.stock, failed.tick, failed.round), (0, 1, 0));
    assert_eq!(failed.entities, [super::super::Entity::CashRefund {local:1, amount:200}]);
    assert_eq!(failed.fills, [fill]);
    assert_eq!(failed.receipts, [receipt]);
    assert_eq!(failed.repurchase, [source]);
    let tail = retained.by_group[2].as_ref().expect("缺少尚未发送尾部批次");
    assert_eq!(tail.entities, [super::super::Entity::CashRefund {local:2, amount:300}]);
    assert_eq!(tail.fills, [fill]);
    assert_eq!(tail.receipts, [receipt]);
    assert_eq!(tail.repurchase, [source]);
}

#[test]
fn drain阶段发送失败在回滚后保留阶段及原始资源() {
    let (tx, rx) = crate::libos::mailbox::channel();
    drop(rx);
    let mut state = DeskState::new(setup(), 1, Arc::new(GameConfig::proposed_defaults()));
    state.out[0].push(super::super::Entity::CashRefund { local:0, amount:500 });
    assert!(state.flush_batches(&[tx], 8, 2, DeskFlushPhase::Drain).is_err());
    state.rollback();
    let diagnostic = state.final_state().undelivered_batches.unwrap();
    assert!(state.undelivered_batches.is_none(), "Final 必须接管未发送批次而不是克隆诊断槽");
    assert!(state.final_state().undelivered_batches.is_none(), "重复生成 Final 不能再次移交诊断");
    assert_eq!((diagnostic.tick, diagnostic.round, diagnostic.phase, diagnostic.first_failed_group), (8, 2, DeskFlushPhase::Drain, 0));
    assert_eq!(diagnostic.by_group[0].as_ref().unwrap().entities, [super::super::Entity::CashRefund {local:0, amount:500}]);
}
