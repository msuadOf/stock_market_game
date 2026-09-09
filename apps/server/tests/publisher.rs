use engine::{
    AccountId, DailyCandle, DailyTradeStats, Event, Money, Snapshot, StockCode, TradingPhase,
};
use server::{ClientFrameBuffer, EngineUpdate, MAX_BUFFERED_EVENTS_PER_CLIENT};

fn price_tick(seq: u64, tick: u64) -> Event {
    Event::PriceTick {
        seq,
        tick,
        code: StockCode("600101".into()),
        last_price: Money::from_cents(1_000_000_000),
        daily_candle: DailyCandle {
            time: 0,
            open: Money::from_cents(1_000_000_000),
            high: Money::from_cents(1_000_000_000),
            low: Money::from_cents(1_000_000_000),
            close: Money::from_cents(1_000_000_000),
            volume: 9_007_200,
            trade_stats: Some(DailyTradeStats {
                turnover_cents: 9_007_200_000_000_000,
                trade_count: 7,
            }),
        },
        bids: Vec::new(),
        asks: Vec::new(),
    }
}

fn auction_tick(seq: u64, tick: u64) -> Event {
    Event::AuctionTick {
        seq,
        tick,
        code: StockCode("600101".into()),
        indicative_price: Some(Money::from_cents(1_000)),
        matched_volume: seq,
        imbalance: 0,
    }
}

fn trade(seq: u64) -> Event {
    Event::Trade {
        seq,
        code: StockCode("600101".into()),
        price: Money::from_cents(1_000),
        qty: 100,
        maker: AccountId(1),
        taker: AccountId(2),
    }
}

#[test]
fn client_frame_compaction_preserves_six_second_auction_and_minute_slots() {
    let mut buffer = ClientFrameBuffer::new(15_300, 900).unwrap();
    buffer
        .push(EngineUpdate {
            events: vec![
                auction_tick(1, 1),
                auction_tick(2, 5),
                auction_tick(3, 6),
                auction_tick(4, 7),
                price_tick(5, 901),
                price_tick(6, 959),
                price_tick(7, 961),
            ],
            runtime_snapshot: None,
        })
        .unwrap();

    let frame = buffer.take().expect("积累的更新应形成一帧");
    let seqs: Vec<_> = frame.events.iter().map(Event::seq).collect();
    assert_eq!(seqs, vec![3, 4, 6, 7]);
    assert_eq!(frame.from_seq, 1);
    assert_eq!(frame.to_seq, 7);
    let retained_stats = frame
        .events
        .iter()
        .filter_map(|event| match event {
            Event::PriceTick { daily_candle, .. } => daily_candle.trade_stats.as_ref(),
            _ => None,
        })
        .next_back()
        .expect("压缩后的 PriceTick 应保留累计成交统计");
    assert_eq!(retained_stats.turnover_cents, 9_007_200_000_000_000);
    assert_eq!(retained_stats.trade_count, 7);
    let json = serde_json::to_value(&frame).unwrap();
    assert_eq!(
        json["events"][3]["PriceTick"]["daily_candle"]["trade_stats"]["turnover_cents"],
        "9007200000000000"
    );
}

#[test]
fn client_frame_compaction_drops_closed_day_samples_but_keeps_boundary_and_new_day() {
    let mut buffer = ClientFrameBuffer::new(120, 0).unwrap();
    buffer
        .push(EngineUpdate {
            events: vec![
                price_tick(1, 1),
                price_tick(2, 61),
                Event::DayBoundary {
                    seq: 3,
                    day: 1,
                    closed_daily_candles: Default::default(),
                },
                price_tick(4, 121),
            ],
            runtime_snapshot: None,
        })
        .unwrap();

    let frame = buffer.take().expect("跨日更新应形成一帧");
    assert_eq!(
        frame.events.iter().map(Event::seq).collect::<Vec<_>>(),
        vec![3, 4]
    );
    assert_eq!((frame.from_seq, frame.to_seq), (1, 4));
}

#[test]
fn client_frame_buffer_rejects_non_contiguous_input_instead_of_hiding_loss() {
    let mut buffer = ClientFrameBuffer::new(120, 0).unwrap();
    buffer
        .push(EngineUpdate {
            events: vec![price_tick(1, 1)],
            runtime_snapshot: None,
        })
        .unwrap();
    let error = buffer
        .push(EngineUpdate {
            events: vec![price_tick(3, 2)],
            runtime_snapshot: None,
        })
        .expect_err("广播缺口必须显式暴露");
    assert!(error.to_string().contains("不连续"));
}

#[test]
fn publisher_does_not_apply_an_older_runtime_snapshot_after_newer_events() {
    let mut buffer = ClientFrameBuffer::new(120, 0).unwrap();
    let snapshot = Snapshot {
        seq: 1,
        tick: 1,
        day: 0,
        phase: TradingPhase::Continuous,
        markets: Default::default(),
        accounts: Default::default(),
        daily_candles: Default::default(),
        active_daily_candles: Default::default(),
    };
    buffer
        .push(EngineUpdate {
            events: vec![price_tick(1, 1)],
            runtime_snapshot: Some(snapshot),
        })
        .unwrap();
    buffer
        .push(EngineUpdate {
            events: vec![price_tick(2, 2)],
            runtime_snapshot: None,
        })
        .unwrap();

    let first = buffer.take().unwrap();
    assert_eq!((first.from_seq, first.to_seq), (1, 1));
    assert_eq!(first.runtime_snapshot.unwrap().seq, 1);
    let second = buffer.take().unwrap();
    assert_eq!((second.from_seq, second.to_seq), (2, 2));
    assert!(second.runtime_snapshot.is_none());
}

#[test]
fn publisher_keeps_the_latest_visible_trade_tape() {
    let mut buffer = ClientFrameBuffer::new(1_000, 0).unwrap();
    buffer
        .push(EngineUpdate {
            events: (1..=150).map(trade).collect(),
            runtime_snapshot: None,
        })
        .unwrap();

    let frame = buffer.take().unwrap();
    assert_eq!(frame.events.len(), 100);
    assert_eq!(frame.events.first().map(Event::seq), Some(51));
    assert_eq!(frame.events.last().map(Event::seq), Some(150));
}

#[test]
fn publisher_rejects_a_client_buffer_beyond_its_hard_budget() {
    let mut buffer = ClientFrameBuffer::new(100_000, 0).unwrap();
    let events = (1..=MAX_BUFFERED_EVENTS_PER_CLIENT as u64 + 1)
        .map(|seq| price_tick(seq, seq))
        .collect();
    let error = buffer
        .push(EngineUpdate {
            events,
            runtime_snapshot: None,
        })
        .expect_err("停止拉取的客户端不能无限积累原始事件");
    assert!(error.to_string().contains("缓冲超过"));
    assert!(buffer.take().is_none(), "超限批次不得被部分写入");
}
