use super::*;
use crate::calendar::{CalendarExchange, TradingCalendar};
use crate::company::share_registry::settlement_day::*;
use crate::company::share_registry::*;
fn date(s: &str) -> CivilDate {
    CivilDate::from_iso(s).unwrap()
}
fn fixture() -> (
    ShareRegistry,
    CashDividendTaxBook,
    ShareSettlementDay,
    TradingCalendar,
) {
    let registry = ShareRegistry::new(
        StockCode("600001".into()),
        crate::company::CompanyId(0),
        100,
        date("2026-01-05"),
        vec![ShareHolding {
            holder: HolderId::Account(AccountId(1)),
            lots: vec![ShareLot {
                id: "initial".into(),
                qty: 100,
                acquired_on: date("2024-01-02"),
                source: AcquisitionSource::InitialAllocation {
                    evidence: "真实装配期来源".into(),
                },
                restriction: ShareRestriction::Unrestricted,
            }],
        }],
    )
    .unwrap();
    let book = CashDividendTaxBook::new(
        AccountId(1),
        registry.stock().clone(),
        DividendTaxProfile::IndividualPublicMarket,
        registry.settled_on(),
        vec![DividendTaxLot {
            id: "tax:initial".into(),
            qty: 100,
            acquired_on: date("2024-01-02"),
            source: TaxAcquisitionSource::InitialAllocation {
                evidence: "真实装配期来源".into(),
            },
            class: TaxShareClass::PublicMarket,
        }],
    )
    .unwrap();
    let day = ShareSettlementDay {
        stock: registry.stock().clone(),
        day: date("2026-01-06"),
        predecessor: registry.settlement_frontier().unwrap(),
        opening: vec![],
        actual_market_close: ConfirmedShareMarketClose {
            unit: ShareUnitIdentity::Initial,
            source: ShareMarketSource::FinalConfirmations { receipts: vec![] },
            request: ShareDayRequest {
                event_id: "market:2026-01-06".into(),
                day: date("2026-01-06"),
                scope: MovementScope::PublicMarket,
                changes: vec![],
            },
        },
        post_market_actions: vec![SharePostAction::CaptureRegistration {
            event_id: "actual-plan".into(),
        }],
    };
    (
        registry,
        book,
        day,
        TradingCalendar::current_default_calendar().unwrap(),
    )
}
fn context<'a>(
    calendar: &'a TradingCalendar,
    rows: &'a [ShareMarketConfirmation],
    through: Option<CivilDate>,
) -> ShareMarketSourceContext<'a> {
    ShareMarketSourceContext {
        calendar,
        exchange: CalendarExchange::Sse,
        confirmations: rows,
        treasury_account: None,
        completed_through: through,
    }
}
#[test]
fn actual_reg_prepare_zero_market_day_advances_tax_and_binds_exact_plan_capture() {
    let (registry, book, day, calendar) = fixture();
    let result = TaxDayBoundaryFacts::prepare(
        &registry,
        &book,
        &day,
        &context(&calendar, &[], Some(day.day)),
    )
    .expect("已完成普通零日也必须推进税日");
    assert_eq!(result.book().settled_on(), day.day);
    assert_eq!(result.book().lots(), book.lots());
    assert_eq!(result.book().days.len(), 1);
    assert_eq!(result.book().days[0].net_change, 0);
    assert!(result.book().days[0].acquisition.is_none());
    let capture = &result.registrations()[0];
    assert_eq!(capture.event_id, "actual-plan");
    assert_eq!(
        capture.settled_receipts,
        result.registry().receipts().len() as u64
    );
    assert_eq!(capture.lots, book.lots());
    assert_eq!(capture.after_book_operation, result.book().operation_seq);
    assert_eq!(book.settled_on(), date("2026-01-05"));
}

#[test]
fn missing_watermark_and_omitted_confirmations_reject_without_mutating_inputs() {
    let (registry, book, day, calendar) = fixture();
    let before = (registry.clone(), book.clone());
    assert!(
        TaxDayBoundaryFacts::prepare(&registry, &book, &day, &context(&calendar, &[], None))
            .is_err()
    );
    let rows = vec![ShareMarketConfirmation {
        account: AccountId(1),
        receipt_id: 70,
        stock: registry.stock().clone(),
        day: day.day,
        side: crate::orderbook::Side::Sell,
        qty: 10,
    }];
    assert!(TaxDayBoundaryFacts::prepare(
        &registry,
        &book,
        &day,
        &context(&calendar, &rows, Some(day.day))
    )
    .is_err());
    assert_eq!((registry, book), before);
}
#[test]
fn complete_lot_attribution_rejects_90_physical_100_tax_and_equal_total_spoofs() {
    let (registry, book, day, calendar) = fixture();
    for variant in 0..5 {
        let mut changed = book.clone();
        let mut lot = changed.lots()[0].clone();
        match variant {
            0 => lot.qty = 110,
            1 => lot.id = "tax:impostor".into(),
            2 => lot.acquired_on = date("2024-01-03"),
            3 => {
                lot.source = TaxAcquisitionSource::InitialAllocation {
                    evidence: "无受控对应来源".into(),
                }
            }
            4 => {
                changed = CashDividendTaxBook::new(
                    AccountId(2),
                    registry.stock().clone(),
                    DividendTaxProfile::IndividualPublicMarket,
                    registry.settled_on(),
                    changed.lots().to_vec(),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        if variant != 4 {
            changed = CashDividendTaxBook::new(
                changed.account(),
                changed.stock().clone(),
                DividendTaxProfile::IndividualPublicMarket,
                changed.settled_on(),
                vec![lot],
            )
            .unwrap();
        }
        let failure = TaxDayBoundaryFacts::prepare(
            &registry,
            &changed,
            &day,
            &context(&calendar, &[], Some(day.day)),
        )
        .expect_err("完整 lot 对应不能由等总量、前缀或比例猜出");
        assert!(
            matches!(failure, TaxBoundaryError::MissingAttribution { .. }),
            "{failure:?}"
        );
    }
    let physical90 = ShareRegistry::new(
        registry.stock().clone(),
        crate::company::CompanyId(0),
        90,
        registry.settled_on(),
        vec![ShareHolding {
            holder: HolderId::Account(AccountId(1)),
            lots: vec![ShareLot {
                qty: 90,
                ..registry.holdings()[0].lots[0].clone()
            }],
        }],
    )
    .unwrap();
    let mut day90 = day.clone();
    day90.predecessor = physical90.settlement_frontier().unwrap();
    assert!(matches!(
        TaxDayBoundaryFacts::prepare(
            &physical90,
            &book,
            &day90,
            &context(&calendar, &[], Some(day.day))
        ),
        Err(TaxBoundaryError::MissingAttribution { .. })
    ));
}
#[test]
fn calendar_closed_and_absent_holder_change_still_record_exactly_one_zero_tax_day() {
    let (registry, book, mut day, calendar) = fixture();
    day.day = date("2026-01-10");
    let registry = ShareRegistry::new(
        registry.stock().clone(),
        crate::company::CompanyId(0),
        100,
        date("2026-01-09"),
        registry.holdings().to_vec(),
    )
    .unwrap();
    let book = CashDividendTaxBook::new(
        book.account(),
        book.stock().clone(),
        DividendTaxProfile::IndividualPublicMarket,
        registry.settled_on(),
        book.lots().to_vec(),
    )
    .unwrap();
    day.predecessor = registry.settlement_frontier().unwrap();
    day.actual_market_close.request.day = day.day;
    day.actual_market_close.source = ShareMarketSource::CalendarClosed {
        exchange: CalendarExchange::Sse,
        policy_digest: calendar.policy().content_digest().into(),
    };
    let result = TaxDayBoundaryFacts::prepare(
        &registry,
        &book,
        &day,
        &context(&calendar, &[], Some(day.day)),
    )
    .unwrap();
    assert_eq!(result.book().days.len(), 1);
    assert_eq!(result.book().days[0].net_change, 0);
    assert_eq!(result.book().settled_on(), day.day);
    assert_eq!(result.registrations().len(), 1);
}
#[test]
fn any_current_or_historical_nonmarket_movement_is_explicitly_unsupported() {
    let (registry, book, mut day, calendar) = fixture();
    day.post_market_actions
        .push(SharePostAction::Movement(ShareDayRequest {
            event_id: "transfer".into(),
            day: day.day,
            scope: MovementScope::NonTradingTransfer {
                basis: "真实过户另需税来源".into(),
            },
            changes: vec![],
        }));
    assert!(TaxDayBoundaryFacts::prepare(
        &registry,
        &book,
        &day,
        &context(&calendar, &[], Some(day.day))
    )
    .is_err());
    let mut historical = registry.clone();
    historical
        .close_day(ShareDayRequest {
            event_id: "historical-transfer".into(),
            day: day.day,
            scope: MovementScope::NonTradingTransfer {
                basis: "不能忽略历史".into(),
            },
            changes: vec![DayNetChange {
                holder: HolderId::Account(AccountId(1)),
                change: 1,
                acquisition: Some(NetAcquisition {
                    lot_id: "historical-company".into(),
                    source: AcquisitionSource::CorporateAction {
                        event: "historical-transfer".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }),
                consume: None,
            }],
        })
        .unwrap();
    let book = CashDividendTaxBook::new(
        book.account(),
        book.stock().clone(),
        DividendTaxProfile::IndividualPublicMarket,
        historical.settled_on(),
        book.lots().to_vec(),
    )
    .unwrap();
    let mut later = day.clone();
    later.day = date("2026-01-07");
    later.predecessor = historical.settlement_frontier().unwrap();
    later.actual_market_close.request.day = later.day;
    later.actual_market_close.request.event_id = "later-market".into();
    later.post_market_actions.clear();
    assert!(TaxDayBoundaryFacts::prepare(
        &historical,
        &book,
        &later,
        &context(&calendar, &[], Some(later.day))
    )
    .is_err());
}

use crate::runtime::group::{GroupScript, PlaceSpec, TickPlan};
use crate::runtime::watchdog::WatchdogConfig;
use crate::runtime::world::{DeskStockSpec, World, WorldSpec};
use crate::runtime::GroupIdx;
use std::time::Duration;
fn basic_spec(
    n_groups: usize,
    n_stocks: usize,
    accounts_per_group: usize,
    script_for: Box<dyn Fn(GroupIdx) -> GroupScript>,
) -> WorldSpec {
    let shares = vec![vec![vec![1_000u32; n_stocks]; accounts_per_group]; n_groups];
    WorldSpec {
        initial_plan_books: Vec::new(),
        initial_desk_orders: Vec::new(),
        initial_desk_next_serial: Vec::new(),
        n_groups,
        accounts_per_group,
        initial_cash: 1_000_000,
        initial_shares: shares,
        initial_invested: Vec::new(),
        initial_recovered: Vec::new(),
        initial_ever_held: Vec::new(),
        initial_frozen_today: Vec::new(),
        initial_purse_held: Vec::new(),
        initial_pending_sell: Vec::new(),
        root_projection: None,
        civil_start_date: crate::calendar::CivilDate::from_ymd(2026, 1, 5).expect("测试日期合法"),
        script_for,
        chunk: 8,
        watchdog: WatchdogConfig {
            normal: Duration::from_secs(5),
            long: Duration::from_secs(10),
        },
        rounds_cap: 4,
        stocks: (0..n_stocks)
            .map(|_| DeskStockSpec::main_board(1000))
            .collect(),
        config: std::sync::Arc::new(crate::config::GameConfig::proposed_defaults()),
        ticks_per_day: 30,
        auction_ticks: 0,
        closing_auction_ticks: 3,
        order_ttl_ticks: 1_000_000,
        history_len: 16,
        initial_committed_tick: 0,
        initial_last_day_end_tick: None,
        belief_seeds: Vec::new(),
        library: None,
        decision: None,
        rights_computer: None,
        initial_cash_override: None,
        plan_factory: None,
    }
}

/// fixture 的 seq 由本测试的独立确认命名空间提供；成交与双腿确认均使用真实实现。
fn actual_world_source() -> (
    ShareRegistry,
    ShareSettlementDay,
    TradingCalendar,
    Vec<ShareMarketConfirmation>,
) {
    let (_, _, mut day, calendar) = fixture();
    let mut spec = basic_spec(
        1,
        1,
        2,
        Box::new(|_| {
            Box::new(|tick, _| {
                let mut plan = TickPlan::default();
                if tick == 1 {
                    plan.places.push(PlaceSpec {
                        local: 0,
                        stock: 0,
                        side: crate::Side::Sell,
                        price: 1000,
                        qty: 100,
                    });
                }
                if tick == 2 {
                    plan.places.push(PlaceSpec {
                        local: 1,
                        stock: 0,
                        side: crate::Side::Buy,
                        price: 1000,
                        qty: 100,
                    });
                }
                plan
            })
        }),
    );
    spec.civil_start_date = day.day;
    let mut world = World::spawn(spec).unwrap();
    let before = world.snapshot_accounts().unwrap();
    world.run_ticks(3).unwrap();
    let after = world.snapshot_accounts().unwrap();
    let mut confirmations = Vec::new();
    let mut seq = 0;
    for trade in world
        .report_log
        .iter()
        .flat_map(|record| &record.desk_trades)
    {
        seq += 1;
        let legs = crate::session::personal_trade_history::PersonalTradeConfirmation::from_legs(
            seq, day.day, trade,
        );
        for (owner, row) in [trade.maker, trade.taker].into_iter().zip(legs) {
            assert_eq!(owner.0, 0, "fixture 单组真实名册");
            confirmations.push(ShareMarketConfirmation {
                account: AccountId(u64::from(owner.1)),
                receipt_id: row.receipt_id,
                stock: day.stock.clone(),
                day: row.civil_date,
                side: row.side,
                qty: u64::from(row.quantity_shares),
            });
        }
    }
    assert_eq!(confirmations.len(), 2, "fixture 必须产生真实双腿成交");
    assert_eq!(after[0].1[0].shares[0], before[0].1[0].shares[0] - 100);
    assert_eq!(after[0].1[1].shares[0], before[0].1[1].shares[0] + 100);
    world.shutdown().unwrap();
    let registry = ShareRegistry::new(
        day.stock.clone(),
        crate::company::CompanyId(0),
        2000,
        day.predecessor.settled_on,
        (0..2)
            .map(|local| ShareHolding {
                holder: HolderId::Account(AccountId(local)),
                lots: vec![ShareLot {
                    id: format!("actual-initial:{local}"),
                    qty: u64::from(before[0].1[local as usize].shares[0]),
                    acquired_on: date("2024-01-02"),
                    source: AcquisitionSource::InitialAllocation {
                        evidence: "World装配期账户".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            })
            .collect(),
    )
    .unwrap();
    day.predecessor = registry.settlement_frontier().unwrap();
    day.actual_market_close.source = ShareMarketSource::FinalConfirmations {
        receipts: confirmations
            .iter()
            .map(|row| ShareConfirmationRef {
                account: row.account,
                receipt_id: row.receipt_id,
            })
            .collect(),
    };
    day.actual_market_close.request.changes = vec![
        DayNetChange {
            holder: HolderId::Account(AccountId(0)),
            change: -100,
            acquisition: None,
            consume: None,
        },
        DayNetChange {
            holder: HolderId::Account(AccountId(1)),
            change: 100,
            acquisition: Some(NetAcquisition {
                lot_id: "actual-buy".into(),
                source: AcquisitionSource::SecondaryMarket {
                    settlement: day.actual_market_close.request.event_id.clone(),
                },
                restriction: ShareRestriction::Unrestricted,
            }),
            consume: None,
        },
    ];
    (registry, day, calendar, confirmations)
}
#[test]
fn actual_world_fills_normalize_into_reg_and_exact_tax_net_acquisition_and_disposition() {
    let (registry, day, calendar, rows) = actual_world_source();
    for account in [AccountId(0), AccountId(1)] {
        let lot = &registry
            .holdings()
            .iter()
            .find(|holding| holding.holder == HolderId::Account(account))
            .unwrap()
            .lots[0];
        let book = CashDividendTaxBook::new(
            account,
            registry.stock().clone(),
            DividendTaxProfile::IndividualPublicMarket,
            registry.settled_on(),
            vec![DividendTaxLot {
                id: format!("tax:{}", lot.id),
                qty: lot.qty,
                acquired_on: lot.acquired_on,
                source: convert_tax_source(&lot.source),
                class: TaxShareClass::PublicMarket,
            }],
        )
        .unwrap();
        let result = TaxDayBoundaryFacts::prepare(
            &registry,
            &book,
            &day,
            &context(&calendar, &rows, Some(day.day)),
        )
        .unwrap();
        let expected = if account == AccountId(0) { 900 } else { 1100 };
        assert_eq!(
            result.book().lots().iter().map(|lot| lot.qty).sum::<u64>(),
            expected
        );
        assert_eq!(
            result.book().days[0].net_change,
            if account == AccountId(0) { -100 } else { 100 }
        );
        assert_eq!(result.registrations()[0].lots, result.book().lots());
        if account == AccountId(1) {
            let acquired = result.book().days[0].acquisition.as_ref().unwrap();
            assert_eq!(
                (acquired.id.as_str(), acquired.acquired_on, acquired.qty),
                ("tax:actual-buy", day.day, 100)
            );
        } else {
            assert!(result.book().days[0].acquisition.is_none());
            assert_eq!(result.book().days[0].dispositions.len(), 1);
        }
        let mut omitted = day.clone();
        let ShareMarketSource::FinalConfirmations { receipts } =
            &mut omitted.actual_market_close.source
        else {
            unreachable!()
        };
        receipts.pop();
        assert!(TaxDayBoundaryFacts::prepare(
            &registry,
            &book,
            &omitted,
            &context(&calendar, &rows, Some(day.day))
        )
        .is_err());
    }
}

#[test]
fn equal_tail_cannot_move_a_historical_sale_across_tax_holding_period_boundary() {
    let (initial,_,mut market,calendar)=fixture();
    let registry=ShareRegistry::new(initial.stock().clone(),crate::company::CompanyId(0),1000,date("2026-01-29"),vec![ShareHolding {holder:HolderId::Account(AccountId(1)),lots:vec![ShareLot {qty:1000,acquired_on:date("2025-12-30"),..initial.holdings()[0].lots[0].clone()}]}]).unwrap();
    let lot=&registry.holdings()[0].lots[0];
    let mut opening=CashDividendTaxBook::new(AccountId(1),registry.stock().clone(),DividendTaxProfile::IndividualPublicMarket,registry.settled_on(),vec![DividendTaxLot {id:format!("tax:{}",lot.id),qty:lot.qty,acquired_on:lot.acquired_on,source:convert_tax_source(&lot.source),class:TaxShareClass::PublicMarket}]).unwrap();
    opening.register_dividend("real-dividend".into(),registry.settled_on(),ExactDividendTaxAmount::new(100,1).unwrap()).unwrap();
    opening.record_payment("real-dividend","real-cash".into(),registry.settled_on(),Money::from_cents(100000),"真实付款事实".into()).unwrap();
    market.day=date("2026-01-30");market.predecessor=registry.settlement_frontier().unwrap();market.actual_market_close.request.day=market.day;
    let rows=vec![ShareMarketConfirmation {account:AccountId(1),receipt_id:100,stock:registry.stock().clone(),day:market.day,side:crate::Side::Sell,qty:100},ShareMarketConfirmation {account:AccountId(2),receipt_id:101,stock:registry.stock().clone(),day:market.day,side:crate::Side::Buy,qty:100}];
    market.actual_market_close.source=ShareMarketSource::FinalConfirmations {receipts:rows.iter().map(|row|ShareConfirmationRef {account:row.account,receipt_id:row.receipt_id}).collect()};
    market.actual_market_close.request.changes=vec![DayNetChange {holder:HolderId::Account(AccountId(1)),change:-100,acquisition:None,consume:None},DayNetChange {holder:HolderId::Account(AccountId(2)),change:100,acquisition:Some(NetAcquisition {lot_id:"real-Jan30-buy".into(),source:AcquisitionSource::SecondaryMarket {settlement:market.actual_market_close.request.event_id.clone()},restriction:ShareRestriction::Unrestricted}),consume:None}];
    market.post_market_actions.clear();
    let registry=registry.prepare_settlement_day(&market,&context(&calendar,&rows,Some(market.day))).unwrap();
    let mut closed=market.clone();closed.day=date("2026-01-31");closed.predecessor=registry.settlement_frontier().unwrap();closed.actual_market_close.request.day=closed.day;closed.actual_market_close.request.event_id="closed-Jan31".into();closed.actual_market_close.request.changes.clear();closed.actual_market_close.source=ShareMarketSource::CalendarClosed {exchange:CalendarExchange::Sse,policy_digest:calendar.policy().content_digest().into()};
    let registry=registry.prepare_settlement_day(&closed,&context(&calendar,&rows,Some(closed.day))).unwrap();
    let mut honest=opening.clone();honest.record_net_day("jan30".into(),market.day,-100,None).unwrap();honest.record_net_day("jan31".into(),closed.day,0,None).unwrap();
    let mut dishonest=opening;dishonest.record_net_day("jan30".into(),market.day,0,None).unwrap();dishonest.record_net_day("jan31".into(),closed.day,-100,None).unwrap();
    assert_eq!(honest.lots(),dishonest.lots());
    assert_eq!(honest.outstanding().unwrap().round_half_up_cents().unwrap(),2000);
    assert_eq!(dishonest.outstanding().unwrap().round_half_up_cents().unwrap(),1000);
    let mut next=closed;next.day=date("2026-02-01");next.predecessor=registry.settlement_frontier().unwrap();next.actual_market_close.request.day=next.day;next.actual_market_close.request.event_id="closed-Feb01".into();
    assert!(TaxDayBoundaryFacts::prepare(&registry,&honest,&next,&context(&calendar,&rows,Some(next.day))).is_ok());
    assert!(matches!(TaxDayBoundaryFacts::prepare(&registry,&dishonest,&next,&context(&calendar,&rows,Some(next.day))),Err(TaxBoundaryError::MissingAttribution {..})),"税历史不能因相同期末lot而移日减税");
}
