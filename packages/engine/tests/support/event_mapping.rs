use engine::calendar::{CivilInstant, DayStatus};
use engine::company::CompanyId;
use engine::information::PublicationId;
use engine::session::CompanyDisclosureKind;
use engine::{
    AccountId, CivilDate, DailyCandle, Event, Money, OrderId, RejectionReason, Side, StockCode,
    TradingPhase,
};

pub fn adr_mapping(event: &Event) -> (&'static str, u8, String, u8) {
    match event {
        Event::Trade { code, .. } => ("Trade", 4, format!("Stock:{}", code.0), 0),
        Event::AuctionTick { code, .. } => ("AuctionTick", 4, format!("Stock:{}", code.0), 2),
        Event::AuctionCompleted { code, .. } => {
            ("AuctionCompleted", 4, format!("Stock:{}", code.0), 2)
        }
        Event::PriceTick { code, .. } => ("PriceTick", 4, format!("Stock:{}", code.0), 2),
        Event::DayBoundary { .. } => ("DayBoundary", 5, "Session".into(), 3),
        Event::CivilDateAdvanced { .. } => ("CivilDateAdvanced", 6, "Session".into(), 4),
        Event::CompanyDisclosurePublished { .. } => {
            ("CompanyDisclosurePublished", 6, "Session".into(), 4)
        }
        Event::OrderAccepted { account, .. } => {
            ("OrderAccepted", 4, format!("Account:{}", account.0), 0)
        }
        Event::OrderCanceled { account, .. } => {
            ("OrderCanceled", 4, format!("Account:{}", account.0), 0)
        }
        Event::IntentRejected { account, .. } => {
            ("IntentRejected", 4, format!("Account:{}", account.0), 0)
        }
        Event::SettlementError { account, .. } => {
            ("SettlementError", 4, format!("Account:{}", account.0), 0)
        }
    }
}

pub fn events() -> Vec<Event> {
    let code = StockCode("600519".into());
    let account = AccountId(73);
    let date = CivilDate::from_iso("2030-01-02").unwrap();
    let price = Money::from_cents(1234);
    vec![
        Event::Trade {
            seq: 1,
            code: code.clone(),
            price,
            qty: 100,
            maker: AccountId(41),
            taker: AccountId(42),
        },
        Event::AuctionTick {
            seq: 2,
            tick: 9,
            phase: TradingPhase::CallAuction,
            code: code.clone(),
            indicative_price: Some(price),
            matched_volume: 100,
            imbalance: 0,
        },
        Event::AuctionCompleted {
            seq: 3,
            tick: 9,
            phase: TradingPhase::CallAuction,
            code: code.clone(),
            clearing_price: Some(price),
            matched_volume: 100,
        },
        Event::PriceTick {
            seq: 4,
            tick: 10,
            code: code.clone(),
            last_price: price,
            daily_candle: DailyCandle {
                time: 0,
                open: price,
                high: price,
                low: price,
                close: price,
                volume: 0,
                trade_stats: None,
            },
            bids: vec![],
            asks: vec![],
        },
        Event::DayBoundary {
            seq: 5,
            day: 1,
            closed_daily_candles: Default::default(),
        },
        Event::CivilDateAdvanced {
            seq: 6,
            settled_date: date,
            next_date: date.next().unwrap(),
            next_status: DayStatus::Trading,
        },
        Event::CompanyDisclosurePublished {
            seq: 7,
            publication_id: PublicationId::new(17),
            company: CompanyId("company-distinct-from-stock".into()),
            published_at: CivilInstant::from_hms(date, 18, 0, 0).unwrap(),
            kind: CompanyDisclosureKind::Announcement,
        },
        Event::OrderAccepted {
            seq: 8,
            account,
            code: code.clone(),
            id: OrderId(101),
            side: Side::Buy,
            price,
            remaining_qty: 100,
        },
        Event::OrderCanceled {
            seq: 9,
            account,
            code: code.clone(),
            id: OrderId(101),
            remaining_qty: 100,
        },
        Event::IntentRejected {
            seq: 10,
            account,
            code: code.clone(),
            reason: RejectionReason::InsufficientCash,
        },
        Event::SettlementError {
            seq: 11,
            account,
            code,
            reason: "fixture".into(),
        },
    ]
}
