use engine::session::protocol::{attach_facts, project_timeseries, AuctionPointKind};
use engine::{Event, Money, Snapshot, StockCode, TradingPhase};

fn snapshot() -> Snapshot {
    Snapshot {
        seq: 4,
        tick: 8,
        day: 0,
        phase: TradingPhase::Continuous,
        markets: Default::default(),
        accounts: Default::default(),
        daily_candles: Default::default(),
        active_daily_candles: Default::default(),
    }
}

#[test]
fn multiple_indications_and_completion_survive_permutation() {
    let code = StockCode("600001".into());
    let events = vec![
        Event::AuctionTick {
            seq: 1,
            tick: 8,
            phase: TradingPhase::CallAuction,
            code: code.clone(),
            indicative_price: Some(Money::from_cents(1000)),
            matched_volume: 100,
            imbalance: 200,
        },
        Event::AuctionTick {
            seq: 2,
            tick: 8,
            phase: TradingPhase::CallAuction,
            code: code.clone(),
            indicative_price: Some(Money::from_cents(1001)),
            matched_volume: 300,
            imbalance: 400,
        },
        Event::AuctionCompleted {
            seq: 3,
            tick: 8,
            phase: TradingPhase::CallAuction,
            code: code.clone(),
            clearing_price: Some(Money::from_cents(1002)),
            matched_volume: 500,
        },
    ];
    let mut facts = attach_facts(&events).unwrap();
    let expected = project_timeseries(&facts, snapshot());
    facts.reverse();
    let projected = project_timeseries(&facts, snapshot());
    assert_eq!(
        serde_json::to_value(&projected).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    let points = &projected.auction_points[&code];
    assert_eq!(points.len(), 3);
    assert!(points.windows(2).all(|pair| pair[0].key < pair[1].key));
    assert!(matches!(points[0].kind, AuctionPointKind::Indication));
    assert!(matches!(points[1].kind, AuctionPointKind::Indication));
    assert!(matches!(points[2].kind, AuctionPointKind::Completion));
    for (point, (price, volume, imbalance)) in points.iter().zip([
        (1000, 100, Some(200)),
        (1001, 300, Some(400)),
        (1002, 500, None),
    ]) {
        assert_eq!((point.tick, point.phase), (8, TradingPhase::CallAuction));
        assert_eq!(point.indicative_price, Some(Money::from_cents(price)));
        assert_eq!((point.matched_volume, point.imbalance), (volume, imbalance));
    }
}
