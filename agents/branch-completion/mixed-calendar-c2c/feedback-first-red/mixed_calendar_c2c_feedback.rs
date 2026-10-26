//! V4 首片：同一合法mixed fixture上的open正控与closed typed反馈。

#[allow(dead_code)]
#[path = "common/mixed_calendar_c2c_fixture.rs"]
mod fixture;

use engine::calendar::CalendarExchange;
use engine::{AccountId, Event, Intent, LimitPrice, Money, Side, StockExchange, StockId};

fn limit(code: StockId) -> Intent {
    Intent::PlaceLimit {
        code,
        side: Side::Buy,
        price: LimitPrice::Fixed(Money::from_cents(1_000)),
        qty: 100,
    }
}

#[test]
fn open_limit_is_accepted_and_reserves_real_cash() {
    let mut fixture = fixture::restore(CalendarExchange::Sse, StockExchange::Shanghai, false);
    fixture
        .session
        .enqueue_player_intent(AccountId(0), limit(fixture.open))
        .expect("open合法请求入队");
    let events = fixture.session.step().expect("open合法市场拍");
    assert!(
        events.iter().any(|event| matches!(event,
            Event::OrderAccepted { code, .. } if *code == fixture.open
        )),
        "open限价买单必须真实受理"
    );
    assert!(
        !events.iter().any(|event| matches!(event,
            Event::IntentRejected { code, .. } if *code == fixture.open
        )),
        "open合法请求不能被拒绝"
    );
    assert!(
        fixture.session.snapshot().accounts[&AccountId(0)].reserved_cash > Money::ZERO,
        "无对手盘的open挂单必须占用真实现金"
    );
}

#[test]
fn closed_limit_returns_strict_exchange_closed_feedback() {
    let mut fixture = fixture::restore(CalendarExchange::Sse, StockExchange::Shanghai, false);
    fixture
        .session
        .enqueue_player_intent(AccountId(0), limit(fixture.closed))
        .expect("合法公共受理身份入队");
    let events = fixture.session.step().expect("closed业务拒绝不应变成fatal");
    let reasons: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::IntentRejected {
                account: AccountId(0),
                code,
                reason,
                ..
            } if *code == fixture.closed => {
                Some(serde_json::to_value(reason).expect("typed reason必须可序列化"))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        reasons,
        vec![serde_json::json!("ExchangeClosed")],
        "closed合法限价请求必须恰有一次ExchangeClosed业务反馈"
    );
}
