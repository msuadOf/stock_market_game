//! 真实 desk 来源运输首红；不伪造 Fill，不证明批准、执行资格或公司日净提交。
use super::*;
use crate::runtime::{RepurchaseRequestKey, RepurchaseReturnKind, RepurchaseSourceKind};

const COMPANY_OWNER: Owner = (1, 1);
const SELLER: Owner = (0, 0);
const STOCK: StockIdx = 3;

fn calendar() -> Arc<super::super::MarketTickContext> {
    static CONTEXT: std::sync::OnceLock<Arc<super::super::MarketTickContext>> =
        std::sync::OnceLock::new();
    Arc::clone(CONTEXT.get_or_init(|| {
        let date = crate::CivilDate::from_iso("2026-01-05").unwrap();
        let source = crate::session::market_calendar_context::MarketCalendarSource::new(
            crate::TradingCalendar::current_default_calendar().unwrap(),
            date,
            vec![(
                crate::StockId(u32::from(STOCK)),
                crate::calendar::CalendarExchange::Szse,
            )],
        )
        .unwrap();
        source
            .day(date)
            .unwrap()
            .tick(
                crate::session::exchange_calendar_model::SessionTickLayout {
                    ticks_per_day: 100,
                    auction_ticks: 0,
                    closing_auction_ticks: 0,
                },
                crate::session::exchange_calendar_model::SharedClockInput {
                    tick: 0,
                    completed_shared_sessions: 0,
                    progress: crate::session::exchange_calendar_model::SharedDayProgress::Trading {
                        completed_ticks: 0,
                    },
                },
            )
            .unwrap()
    }))
}
fn calendar_after(before_tick: u64) -> Arc<super::super::MarketTickContext> {
    calendar()
        .day()
        .tick(
            crate::session::exchange_calendar_model::SessionTickLayout {
                ticks_per_day: 100,
                auction_ticks: 0,
                closing_auction_ticks: 0,
            },
            crate::session::exchange_calendar_model::SharedClockInput {
                tick: before_tick,
                completed_shared_sessions: 0,
                progress: crate::session::exchange_calendar_model::SharedDayProgress::Trading {
                    completed_ticks: before_tick,
                },
            },
        )
        .unwrap()
}
fn request() -> RepurchaseRequestKey {
    RepurchaseRequestKey {
        plan_id: crate::plans::PlanId {
            owner: crate::AccountId(79),
            sequence: 3,
        },
        stock: crate::StockId(u32::from(STOCK)),
        requested_trading_day: 0,
    }
}
fn state() -> DeskState {
    state_with_auction(0)
}
fn state_with_auction(auction_ticks: u64) -> DeskState {
    let config = Arc::new(
        GameConfig::new(
            0.000_25,
            Money::from_cents(500),
            0.000_5,
            0.10,
            0.10,
            false,
            100,
            Money::from_cents(1_000_000_000_000),
        )
        .unwrap(),
    );
    let mut state = DeskState::new(
        DeskSetup {
            stock: STOCK,
            exchange: StockExchange::Shenzhen,
            initial_price: 1000,
            limit_bps: 1000,
            max_order_qty_limit: 1_000_000,
            max_order_qty_market: 150_000,
            ticks_per_day: 100,
            auction_ticks,
            closing_auction_ticks: 0,
            order_ttl_ticks: 10,
            history_len: 8,
            initial_tick: 0,
            initial_orders: Vec::new(),
            initial_recent_prices: Vec::new(),
            initial_daily_bars: Vec::new(),
            initial_traded_days: 0,
            initial_market: None,
            initial_public_cutoff: 0,
            initial_next_serial: 1,
        },
        2,
        config,
    );
    let tick = if auction_ticks == 0 {
        calendar()
    } else {
        calendar()
            .day()
            .tick(
                crate::session::exchange_calendar_model::SessionTickLayout {
                    ticks_per_day: 100,
                    auction_ticks,
                    closing_auction_ticks: 0,
                },
                crate::session::exchange_calendar_model::SharedClockInput {
                    tick: 0,
                    completed_shared_sessions: 0,
                    progress: crate::session::exchange_calendar_model::SharedDayProgress::Trading {
                        completed_ticks: 0,
                    },
                },
            )
            .unwrap()
    };
    state.begin_tick(1, tick).unwrap();
    state
}
fn ordinary_sell(price: i64, qty: u32) -> DeskIn {
    DeskIn::Place {
        owner: SELLER,
        side: Side::Sell,
        price: RoutedPrice::Fixed(price),
        qty,
        purse: 0,
        #[cfg(feature = "simulation-diagnostics")]
        source: super::super::diagnostics::OperationSource::NonPlayerUnresolved,
    }
}
fn company_buy(state: &DeskState, price: i64, qty: u32) -> DeskIn {
    let purse = buy_purse_limit(&state.config, Money::from_cents(price), qty, Money::ZERO)
        .unwrap()
        .cents();
    DeskIn::PlaceIssuerRepurchase {
        owner: COMPANY_OWNER,
        request: request(),
        price,
        qty,
        purse,
    }
}
fn labels(state: &DeskState) -> Vec<&'static str> {
    state.repurchase_out[1]
        .iter()
        .map(|fact| match fact.kind {
            RepurchaseSourceKind::Accepted { .. } => "accepted",
            RepurchaseSourceKind::Rejected { .. } => "rejected",
            RepurchaseSourceKind::Fill(_) => "fill",
            RepurchaseSourceKind::Return {
                kind: RepurchaseReturnKind::PriceImprovement,
                ..
            } => "price",
            RepurchaseSourceKind::Return {
                kind: RepurchaseReturnKind::TerminalRefund,
                ..
            } => "terminal",
            RepurchaseSourceKind::Closed => "closed",
        })
        .collect()
}
fn assert_exact_source(state: &DeskState, operation: DeskIn) -> (OrderRef, i64, i64, i64) {
    let DeskIn::PlaceIssuerRepurchase {
        price, qty, purse, ..
    } = operation
    else {
        unreachable!()
    };
    let facts = &state.repurchase_out[1];
    assert!(!facts.is_empty());
    assert!(state.repurchase_out[0].is_empty());
    let order = facts[0].order_ref;
    assert_eq!(order.0, STOCK);
    assert_eq!(
        facts[0].kind,
        RepurchaseSourceKind::Accepted {
            price,
            qty,
            purse_initial: purse
        }
    );
    for fact in facts {
        assert_eq!(
            (fact.request_key, fact.owner, fact.order_ref),
            (request(), COMPANY_OWNER, order)
        );
    }
    let initial = facts
        .iter()
        .find_map(|fact| match fact.kind {
            RepurchaseSourceKind::Accepted { purse_initial, .. } => Some(purse_initial),
            _ => None,
        })
        .unwrap();
    let source_fills: Vec<_> = facts
        .iter()
        .filter_map(|fact| match fact.kind {
            RepurchaseSourceKind::Fill(fill) => Some(fill),
            _ => None,
        })
        .collect();
    assert_eq!(
        source_fills, state.fills_out[1],
        "费用与价格必须来自原真实 FillFact，不能重算或造第二成交"
    );
    for fill in &source_fills {
        assert_eq!((fill.order, fill.owner), (order, COMPANY_OWNER));
    }
    let charged = source_fills
        .iter()
        .map(|fill| {
            fill.validate().unwrap();
            assert_eq!(fill.side, Side::Buy);
            fill.price * i64::from(fill.qty) + fill.commission + fill.transfer_fee
        })
        .sum::<i64>();
    let returned = facts
        .iter()
        .filter_map(|fact| match fact.kind {
            RepurchaseSourceKind::Return { amount, .. } => Some(amount),
            _ => None,
        })
        .sum::<i64>();
    let entities = state.out[1]
        .iter()
        .filter_map(|entity| match *entity {
            Entity::CashRefund { local: 1, amount } => Some(amount),
            _ => None,
        })
        .sum::<i64>();
    assert_eq!(returned, entities, "来源退款逐分等于唯一原实体退款");
    (order, initial, charged, returned)
}

#[test]
fn repurchase_x_transport_01_taker_and_maker_full_fill_precede_terminal_return_and_closed() {
    for company_is_maker in [false, true] {
        let mut state = state();
        if !company_is_maker {
            state.apply_ops(1, vec![ordinary_sell(1000, 100)]).unwrap();
        }
        let operation = company_buy(&state, 1090, 100);
        state.apply_ops(1, vec![operation]).unwrap();
        if company_is_maker {
            state.apply_ops(1, vec![ordinary_sell(1000, 100)]).unwrap();
        }
        // maker 按自己限价全成，精确 purse 已耗尽；不得为统一形状伪造零额退款。
        let expected = if company_is_maker {
            vec!["accepted", "fill", "closed"]
        } else {
            vec!["accepted", "fill", "terminal", "closed"]
        };
        assert_eq!(
            labels(&state),
            expected,
            "最后真实 Fill 必须先于退款和 Closed：maker={company_is_maker}"
        );
        let (order, initial, charged, returned) = assert_exact_source(&state, operation);
        assert_eq!(initial - charged - returned, 0);
        assert!(state.out[1].iter().any(|entity| matches!(*entity, Entity::Closed { local: 1, order_serial } if order_serial == ((u64::from(order.0) << 32) | order.1))));
        assert_eq!(state.resting_count, 0);
    }
}

#[test]
fn repurchase_x_transport_02_partial_price_improvement_then_real_cancel_reconciles_actual_purse() {
    let mut state = state();
    state.apply_ops(1, vec![ordinary_sell(1000, 100)]).unwrap();
    let operation = company_buy(&state, 1090, 200);
    state.apply_ops(1, vec![operation]).unwrap();
    assert_eq!(labels(&state), vec!["accepted", "fill", "price"]);
    let (order, initial, charged, returned) = assert_exact_source(&state, operation);
    let live = state
        .bids
        .values()
        .flatten()
        .find(|order| order.owner == COMPANY_OWNER)
        .unwrap();
    assert_eq!((live.qty, live.purse), (100, initial - charged - returned));
    assert!(returned > 0);
    assert!(!labels(&state).contains(&"closed"));
    state
        .apply_ops(
            1,
            vec![DeskIn::Cancel {
                owner: COMPANY_OWNER,
                order,
                #[cfg(feature = "simulation-diagnostics")]
                source: super::super::diagnostics::OperationSource::NonPlayerUnresolved,
            }],
        )
        .unwrap();
    assert_eq!(
        labels(&state),
        vec!["accepted", "fill", "price", "terminal", "closed"]
    );
    let (_, initial, charged, returned) = assert_exact_source(&state, operation);
    assert_eq!(initial - charged - returned, 0);
}

#[test]
fn repurchase_x_transport_03_rejection_binds_real_receipt_identity_without_fake_acceptance() {
    let mut state = state();
    let operation = company_buy(&state, 1200, 100);
    let DeskIn::PlaceIssuerRepurchase { purse, .. } = operation else {
        unreachable!()
    };
    state.apply_ops(1, vec![operation]).unwrap();
    assert_eq!(labels(&state), vec!["rejected", "terminal", "closed"]);
    let facts = &state.repurchase_out[1];
    let receipt = state.receipts_out[1][0];
    assert_eq!(facts[0].order_ref, receipt.order);
    assert_eq!(receipt.order.0, STOCK);
    assert_eq!(facts[0].request_key, request());
    assert_eq!(
        facts[0].kind,
        RepurchaseSourceKind::Rejected {
            reason: ReceiptKind::PriceOutOfBand
        }
    );
    assert_eq!(
        facts[1].kind,
        RepurchaseSourceKind::Return {
            amount: purse,
            kind: RepurchaseReturnKind::TerminalRefund
        }
    );
    for fact in facts {
        assert_eq!(
            (fact.request_key, fact.owner, fact.order_ref),
            (request(), COMPANY_OWNER, receipt.order)
        );
    }
    assert_eq!(
        state.out[1],
        vec![
            Entity::CashRefund {
                local: 1,
                amount: purse
            },
            Entity::Closed {
                local: 1,
                order_serial: (u64::from(receipt.order.0) << 32) | receipt.order.1
            }
        ]
    );
    assert!(state.out[0].is_empty());
    assert!(state.fills_out[1].is_empty());
    assert_eq!(state.resting_count, 0);
}

#[test]
fn repurchase_x_transport_04_source_only_flush_and_real_abort_retry_keep_one_request() {
    let mut state = state();
    let before = state.state_hash();
    let operation = company_buy(&state, 1000, 100);
    state.apply_ops(1, vec![operation]).unwrap();
    let DeskIn::PlaceIssuerRepurchase { purse, .. } = operation else {
        unreachable!()
    };
    assert_eq!(labels(&state), vec!["accepted"]);
    assert!(state.out[1].is_empty());
    assert!(state.fills_out[1].is_empty());
    assert!(state.receipts_out[1].is_empty());
    assert_eq!(
        state.repurchase_out[1][0].kind,
        RepurchaseSourceKind::Accepted {
            price: 1000,
            qty: 100,
            purse_initial: purse
        }
    );
    let mut different_source = self::state();
    let mut other_request = request();
    other_request.plan_id.sequence += 1;
    different_source
        .apply_ops(
            1,
            vec![DeskIn::PlaceIssuerRepurchase {
                owner: COMPANY_OWNER,
                request: other_request,
                price: 1000,
                qty: 100,
                purse,
            }],
        )
        .unwrap();
    assert_ne!(
        state.state_hash(),
        different_source.state_hash(),
        "相同订单数值的不同公司来源必须进入业务 hash"
    );
    let (tx0, rx0) = std::sync::mpsc::channel();
    let (tx1, rx1) = std::sync::mpsc::channel();
    state.flush_batches(&[tx0, tx1], 1, 0, DeskFlushPhase::TickRound).unwrap();
    assert!(rx0.try_recv().is_err());
    let super::super::protocol::GroupIn::Batch(batch) =
        rx1.try_recv().expect("仅来源的非空批次也必须发送")
    else {
        panic!("必须为原 Batch 消息")
    };
    assert_eq!(batch.repurchase.len(), 1);
    assert!(state.repurchase_out[1].is_empty());
    state.rollback();
    assert_eq!(state.state_hash(), before);
    assert!(state.repurchase_out.iter().all(Vec::is_empty));
    state.begin_tick(1, calendar()).unwrap();
    let operation = company_buy(&state, 1000, 100);
    state.apply_ops(1, vec![operation]).unwrap();
    assert_eq!(
        labels(&state),
        vec!["accepted"],
        "Abort 后重试不得残留旧来源事实"
    );
    assert_eq!(
        state.repurchase_out[1][0].order_ref, batch.repurchase[0].order_ref,
        "撤销取号后允许原真实序号重试，不能把旧来源当已提交"
    );
    state.rollback();
    assert!(state.repurchase_out.iter().all(Vec::is_empty));
    assert_eq!(
        state.state_hash(),
        before,
        "未 flush 的来源缓冲也必须随真实 Abort 撤销"
    );
}

#[test]
fn repurchase_x_transport_05_committed_request_survives_next_tick_fill_and_abort_retry() {
    let mut state = state();
    let operation = company_buy(&state, 1000, 100);
    state.apply_ops(1, vec![operation]).unwrap();
    let request_order = state.repurchase_out[1][0].order_ref;
    let (tx0, _rx0) = std::sync::mpsc::channel();
    let (tx1, rx1) = std::sync::mpsc::channel();
    state.flush_batches(&[tx0, tx1], 1, 0, DeskFlushPhase::TickRound).unwrap();
    rx1.recv().unwrap();
    let committed = state.state_hash();
    for _ in 0..2 {
        state.begin_tick(2, calendar_after(1)).unwrap();
        state.apply_ops(2, vec![ordinary_sell(1000, 100)]).unwrap();
        assert_eq!(labels(&state), vec!["fill", "closed"]);
        assert!(state.repurchase_requests.is_empty());
        assert!(state.repurchase_out[1]
            .iter()
            .all(|fact| fact.order_ref == request_order && fact.request_key == request()));
        state.rollback();
        assert_eq!(
            state.state_hash(),
            committed,
            "撤销次拍终结须恢复前拍原请求，不回退已转正的受理"
        );
        assert_eq!(state.repurchase_requests[&request_order], request());
    }
}

#[test]
fn repurchase_x_transport_06_terminal_pending_sources_keep_request_identity_in_hash() {
    for rejected in [false, true] {
        let mut left = state();
        let mut right = state();
        if !rejected {
            left.apply_ops(1, vec![ordinary_sell(1000, 100)]).unwrap();
            right.apply_ops(1, vec![ordinary_sell(1000, 100)]).unwrap();
        }
        let price = if rejected { 1200 } else { 1000 };
        let operation = company_buy(&left, price, 100);
        let DeskIn::PlaceIssuerRepurchase {
            owner,
            request,
            price,
            qty,
            purse,
        } = operation
        else {
            unreachable!()
        };
        let mut changed = request;
        changed.plan_id.sequence += 1;
        left.apply_ops(1, vec![operation]).unwrap();
        right
            .apply_ops(
                1,
                vec![DeskIn::PlaceIssuerRepurchase {
                    owner,
                    request: changed,
                    price,
                    qty,
                    purse,
                }],
            )
            .unwrap();
        assert!(left.repurchase_requests.is_empty() && right.repurchase_requests.is_empty());
        assert_ne!(
            left.state_hash(),
            right.state_hash(),
            "已终结来源仍待flush，不能因live map为空而丢失身份"
        );
        let (tx0, _rx0) = std::sync::mpsc::channel();
        let (tx1, _rx1) = std::sync::mpsc::channel();
        left.flush_batches(&[tx0, tx1], 1, 0, DeskFlushPhase::TickRound).unwrap();
        let (tx0, _rx0) = std::sync::mpsc::channel();
        let (tx1, _rx1) = std::sync::mpsc::channel();
        right.flush_batches(&[tx0, tx1], 1, 0, DeskFlushPhase::TickRound).unwrap();
        assert_eq!(
            left.state_hash(),
            right.state_hash(),
            "来源交付后相同订单域仍相同，不留已终结的影子订单"
        );
    }
}

#[test]
fn repurchase_x_transport_07_cancel_all_ttl_and_drain_return_the_original_purse_once() {
    for ending in 0..3 {
        let mut state = state();
        let before = state.state_hash();
        if ending == 1 {
            state.setup.order_ttl_ticks = 1;
        }
        let operation = company_buy(&state, 1000, 100);
        let DeskIn::PlaceIssuerRepurchase { purse, .. } = operation else {
            unreachable!()
        };
        state.apply_ops(1, vec![operation]).unwrap();
        let mut prior_tick = None;
        if ending == 1 {
            let (tx0, _rx0) = std::sync::mpsc::channel();
            let (tx1, rx1) = std::sync::mpsc::channel();
            state.flush_batches(&[tx0, tx1], 1, 0, DeskFlushPhase::TickRound).unwrap();
            rx1.recv().unwrap();
            prior_tick = Some(state.state_hash());
        }
        match ending {
            0 => state
                .apply_ops(
                    1,
                    vec![DeskIn::CancelAll {
                        owner: COMPANY_OWNER,
                    }],
                )
                .unwrap(),
            1 => state.begin_tick(2, calendar_after(1)).unwrap(),
            _ => {
                state.drain_all().unwrap();
            }
        }
        let expected = if ending == 1 {
            vec!["terminal", "closed"]
        } else {
            vec!["accepted", "terminal", "closed"]
        };
        assert_eq!(labels(&state), expected);
        assert!(state.repurchase_requests.is_empty());
        assert_eq!(state.resting_count, 0);
        let returned = state.repurchase_out[1]
            .iter()
            .filter_map(|fact| match fact.kind {
                RepurchaseSourceKind::Return { amount, .. } => Some(amount),
                _ => None,
            })
            .sum::<i64>();
        assert_eq!(returned, purse);
        assert_eq!(
            state.out[1]
                .iter()
                .filter_map(|entity| match entity {
                    Entity::CashRefund { amount, .. } => Some(*amount),
                    _ => None,
                })
                .sum::<i64>(),
            purse,
            "新来源不另造第二退款实体"
        );
        if ending != 1 {
            state.rollback();
            assert_eq!(state.state_hash(), before);
            assert!(
                state.repurchase_requests.is_empty()
                    && state.repurchase_out.iter().all(Vec::is_empty)
            );
        } else {
            state.rollback();
            assert_eq!(state.state_hash(), prior_tick.unwrap());
            assert_eq!(
                state.repurchase_requests.len(),
                1,
                "TTL Abort 恢复前拍已转正的原公司来源"
            );
            assert!(state.repurchase_out.iter().all(Vec::is_empty));
        }
    }
}

#[test]
fn repurchase_x_transport_08_invalid_source_and_serial_exhaustion_publish_no_company_fact() {
    for invalid in 0..7 {
        let mut state = if invalid == 6 {
            state_with_auction(3)
        } else {
            state()
        };
        let DeskIn::PlaceIssuerRepurchase {
            mut owner,
            mut request,
            mut price,
            qty,
            mut purse,
        } = company_buy(&state, 1000, 100)
        else {
            unreachable!()
        };
        match invalid {
            0 => request.stock = crate::StockId(2),
            1 => request.requested_trading_day = 1,
            2 => owner.0 = 2,
            3 => purse = 0,
            _ => {
                state.next_serial = 0xFFFF_FFFF;
                if invalid == 5 {
                    price = 1200;
                }
            }
        }
        let before = state.state_hash();
        let operation = if invalid == 6 {
            DeskIn::Place {
                owner,
                side: Side::Buy,
                price: RoutedPrice::Market,
                qty,
                purse,
                #[cfg(feature = "simulation-diagnostics")]
                source: super::super::diagnostics::OperationSource::NonPlayerUnresolved,
            }
        } else {
            DeskIn::PlaceIssuerRepurchase {
                owner,
                request,
                price,
                qty,
                purse,
            }
        };
        assert!(
            state.apply_ops(1, vec![operation]).is_err(),
            "竞价市价拒收的真实序号耗尽也必须显式传播"
        );
        assert!(
            state.repurchase_out.iter().all(Vec::is_empty),
            "取号失败或非法来源不能发布公司受理/拒单事实"
        );
        assert!(state.repurchase_requests.is_empty());
        state.rollback();
        assert_eq!(state.state_hash(), before);
        assert!(state.out.iter().all(Vec::is_empty));
    }
}
