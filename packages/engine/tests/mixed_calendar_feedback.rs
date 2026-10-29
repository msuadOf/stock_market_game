//! V4：公共closed反馈、可见资源与真实受理ordinal；不新增Intent或测试API。

#[path = "common/mixed_calendar_c2c_fixture.rs"]
mod fixture;

use engine::calendar::CalendarExchange;
use engine::{
    AccountId, Event, GameSession, Intent, LimitPrice, Money, OrderId, Side, StockExchange,
    StockId, TradingPhase,
};

#[derive(Clone, Copy)]
enum Request {
    Limit,
    Market,
    Cancel,
}

impl Request {
    fn intent(self, code: StockId) -> Intent {
        match self {
            Self::Limit => Intent::PlaceLimit {
                code,
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(1000)),
                qty: 100,
            },
            Self::Market => Intent::PlaceMarket {
                code,
                side: Side::Buy,
                qty: 100,
            },
            // closed优先于OrderNotFound；closed证券本来就不能有活动证券簿。
            Self::Cancel => Intent::Cancel {
                code,
                id: OrderId(777),
            },
        }
    }
}

fn assert_closed_feedback(events: &[Event], closed: StockId) {
    let reasons: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::IntentRejected {
                account: AccountId(0),
                code,
                reason,
                ..
            } if *code == closed => Some(serde_json::to_value(reason).unwrap()),
            _ => None,
        })
        .collect();
    assert_eq!(
        reasons,
        vec![serde_json::json!("ExchangeClosed")],
        "known closed请求必须恰有一次严格typed ExchangeClosed反馈"
    );
    assert!(!events.iter().any(|event| matches!(event,
        Event::OrderAccepted { code, .. } | Event::OrderCanceled { code, .. } | Event::Trade { code, .. } if *code == closed
    )), "closed请求不能受理、撤销或成交证券委托");
}

fn phase_request(request: Request, before_ticks: usize, expected: TradingPhase) {
    let mut fixture = fixture::restore(CalendarExchange::Sse, StockExchange::Shanghai, true);
    for _ in 0..before_ticks {
        fixture.session.step().expect("合法空拍到目标相位");
    }
    assert_eq!(fixture.session.phase(), expected);
    fixture
        .session
        .enqueue_player_intent(AccountId(0), request.intent(fixture.closed))
        .expect("合法公共受理身份");
    let events = fixture.session.step().expect("业务拒绝不能变成fatal");
    assert_closed_feedback(&events, fixture.closed);
}

macro_rules! phase_case {
    ($name:ident, $request:ident, $before:expr, $phase:ident) => {
        #[test]
        fn $name() {
            phase_request(Request::$request, $before, TradingPhase::$phase);
        }
    };
}

phase_case!(
    closed_limit_call_auction_is_exchange_closed,
    Limit,
    0,
    CallAuction
);
phase_case!(
    closed_market_call_auction_is_exchange_closed,
    Market,
    0,
    CallAuction
);
phase_case!(
    closed_cancel_call_auction_is_exchange_closed,
    Cancel,
    0,
    CallAuction
);
phase_case!(closed_limit_preopen_is_exchange_closed, Limit, 2, PreOpen);
phase_case!(closed_market_preopen_is_exchange_closed, Market, 2, PreOpen);
phase_case!(closed_cancel_preopen_is_exchange_closed, Cancel, 2, PreOpen);
phase_case!(
    closed_limit_continuous_is_exchange_closed,
    Limit,
    3,
    Continuous
);
phase_case!(
    closed_market_continuous_is_exchange_closed,
    Market,
    3,
    Continuous
);
phase_case!(
    closed_cancel_continuous_is_exchange_closed,
    Cancel,
    3,
    Continuous
);
phase_case!(
    closed_limit_closing_auction_is_exchange_closed,
    Limit,
    9,
    ClosingAuction
);
phase_case!(
    closed_market_closing_auction_is_exchange_closed,
    Market,
    9,
    ClosingAuction
);
phase_case!(
    closed_cancel_closing_auction_is_exchange_closed,
    Cancel,
    9,
    ClosingAuction
);

#[test]
fn closed_limit_keeps_public_assets_serial_and_plan_projection_unchanged() {
    let mut fixture = fixture::restore(CalendarExchange::Sse, StockExchange::Shanghai, false);
    // 首次静点接管会建立 desk cursor；目标请求前先取得同一 authority 的真实基线。
    let warmup = fixture.session.step().expect("无意图首拍完成真实 handover");
    assert_eq!(fixture.session.tick(), 1);
    assert!(
        !warmup.iter().any(|event| matches!(
            event,
            Event::OrderAccepted { .. } | Event::OrderCanceled { .. } | Event::Trade { .. }
        )),
        "无意图正控不能先产生订单或成交"
    );
    let before = fixture
        .session
        .save()
        .expect("健康内存投影，不用于跨日restore");
    assert!(
        before.book_next_sequences[&fixture.closed] > 0,
        "正控须已安装真实 desk cursor"
    );
    assert!(before.resting_orders.values().all(Vec::is_empty));
    let account_before = fixture.session.snapshot().accounts[&AccountId(0)].clone();
    fixture
        .session
        .enqueue_player_intent(AccountId(0), Request::Limit.intent(fixture.closed))
        .unwrap();
    let events = fixture.session.step().expect("合法mixed拒单拍");
    let after = fixture.session.save().expect("健康内存投影");
    assert_eq!(
        fixture.session.snapshot().accounts[&AccountId(0)],
        account_before,
        "closed请求不扣除或预留现金/股份"
    );
    assert_eq!(
        after.book_next_sequences[&fixture.closed], before.book_next_sequences[&fixture.closed],
        "closed请求不消耗真实desk serial镜像"
    );
    assert_eq!(
        serde_json::to_value(after.plans).unwrap(),
        serde_json::to_value(before.plans).unwrap(),
        "closed请求不改变公开plan投影；组私有游标仍须internal门另验"
    );
    assert_closed_feedback(&events, fixture.closed);
}

#[test]
fn closed_limit_rejection_precedes_insufficient_cash() {
    let mut save = fixture::initial_save(CalendarExchange::Sse, StockExchange::Shanghai, false);
    save.snapshot.accounts.get_mut(&AccountId(0)).unwrap().cash = Money::ZERO;
    let mut session = GameSession::restore(&save).expect("允许编辑合法账户余额的初始存档");
    let closed = session.stock_id("600101").unwrap();
    session
        .enqueue_player_intent(AccountId(0), Request::Limit.intent(closed))
        .unwrap();
    let events = session.step().expect("closed业务拒绝不应fatal");
    assert_closed_feedback(&events, closed);
}

#[test]
fn accepted_closed_then_open_inputs_keep_real_receipt_ordinals() {
    let mut fixture = fixture::restore(CalendarExchange::Sse, StockExchange::Shanghai, false);
    fixture
        .session
        .enqueue_player_intent(AccountId(0), Request::Limit.intent(fixture.closed))
        .unwrap();
    fixture
        .session
        .enqueue_player_intent(AccountId(0), Request::Limit.intent(fixture.open))
        .unwrap();
    let received = fixture.session.save().expect("真实受理后的健康内存投影");
    assert_eq!(received.pending_player.len(), 2);
    assert_eq!(
        (
            received.pending_player[0].account_ordinal,
            received.pending_player[1].account_ordinal
        ),
        (0, 1)
    );
    assert_eq!(
        (
            received.pending_player[0].stock_ordinal,
            received.pending_player[1].stock_ordinal
        ),
        (0, 0)
    );
    let events = fixture.session.step().expect("mixed双请求拍");
    assert!(
        events.iter().any(
            |event| matches!(event, Event::OrderAccepted { code, .. } if *code == fixture.open)
        ),
        "open请求必须正常受理，不因另一所closed丢弃"
    );
    let consumed = fixture.session.save().expect("成功拍后健康内存投影");
    assert_eq!(
        consumed.ingress_receipt_cursors.next_account_ordinal[&AccountId(0)],
        2,
        "已受理ordinal不能因closed拒绝回拨或重编号"
    );
    assert_eq!(
        consumed.ingress_receipt_cursors.next_stock_ordinal[&fixture.closed],
        1
    );
    assert_eq!(
        consumed.ingress_receipt_cursors.next_stock_ordinal[&fixture.open],
        1
    );
    assert!(consumed.pending_player.is_empty());
    assert_closed_feedback(&events, fixture.closed);
}

#[test]
fn open_limit_keeps_existing_acceptance_and_resource_reservation() {
    let mut fixture = fixture::restore(CalendarExchange::Sse, StockExchange::Shanghai, false);
    fixture
        .session
        .enqueue_player_intent(AccountId(0), Request::Limit.intent(fixture.open))
        .unwrap();
    let events = fixture.session.step().expect("open正常受理拍");
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::OrderAccepted { code, .. } if *code == fixture.open)));
    assert!(
        fixture.session.snapshot().accounts[&AccountId(0)].reserved_cash > Money::ZERO,
        "open正常未成交买单必须保留真实资源占用"
    );
}

#[test]
fn open_unknown_cancel_keeps_existing_already_filled_approximation() {
    let mut fixture = fixture::restore(CalendarExchange::Sse, StockExchange::Shanghai, false);
    fixture
        .session
        .step()
        .expect("无意图首拍建立真实 native authority");
    let before = fixture
        .session
        .save()
        .expect("同 authority 的真实撤单前投影");
    let accounts = fixture.session.snapshot().accounts;
    fixture
        .session
        .enqueue_player_intent(AccountId(0), Request::Cancel.intent(fixture.open))
        .unwrap();
    let events = fixture.session.step().expect("open未知撤单业务回执");
    // native desk 尚未区分本主未知与已结束订单，保留既有明确回执近似。
    let rejected: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::IntentRejected {
                account: AccountId(0),
                code,
                reason,
                ..
            } if *code == fixture.open => Some(reason.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(rejected, vec![engine::RejectionReason::OrderAlreadyFilled]);
    assert!(
        !events.iter().any(|event| matches!(
            event,
            Event::OrderAccepted { .. } | Event::OrderCanceled { .. } | Event::Trade { .. }
        )),
        "失败撤单不能产生受理、撤销或成交事实"
    );
    assert_eq!(fixture.session.snapshot().accounts, accounts);
    assert_eq!(
        fixture.session.save().unwrap().book_next_sequences,
        before.book_next_sequences
    );
}
