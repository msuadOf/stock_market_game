use engine::{
    CivilDate, Event, FloatAllocation, GameConfig, GameSession, HotParams, InstParams, Money,
    NpcSetup, RetailParams, SecurityCategory, SessionSetup, StockCode, StockExchange, StockSpec,
    StrategyParams,
};

fn setup(auction_ticks: u64) -> Result<SessionSetup, Box<dyn std::error::Error>> {
    Ok(SessionSetup {
        company_operations: None,
        groups: Vec::new(),
        stocks: vec![StockSpec {
            code: StockCode("600001".to_owned()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.1,
            tick: Money::from_cents(1),
            total_shares: 1_000_000,
            float_shares: 0,
        }],
        npcs: NpcSetup {
            retail_count: 0,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::ZERO,
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.0,
                order_size_mean: 100,
                chase_prob: 0.0,
            },
            inst: InstParams {
                margin: 0.03,
                order_size: 100,
            },
            hot: HotParams {
                lookback: 2,
                trend_threshold: 0.01,
                order_size: 100,
            },
        },
        ticks_per_day: 20,
        auction_ticks,
        closing_auction_ticks: 2,
        history_len: 20,
        t1_enabled: true,
        float_allocation: FloatAllocation::random(),
        start_date: CivilDate::from_iso("2030-01-02")?,
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_owned(),
    })
}

fn main() {
 let mode=std::env::args().nth(1).unwrap();
 if mode=="context" {
  let requested=engine::ComputeContext::with_workers(1).unwrap();
  requested.install(|| {let s=engine::session::protocol::ProtocolSession::new(setup(0).unwrap(),42).unwrap();println!("requested_budget={} session_compute_workers={} bound_to_requested={}",requested.workers(),s.game().compute_context().workers(),std::sync::Arc::ptr_eq(&requested,s.game().compute_context()));});
 } else if mode=="seq" {
  let mut game=GameSession::new(setup(3).unwrap(),42).unwrap();
  for t in 1..=20 {
   if t==19 { game.enqueue_player_intent(engine::AccountId(0),engine::Intent::PlaceLimit{code:engine::StockId(0),side:engine::Side::Buy,price:engine::LimitPrice::Fixed(Money::from_cents(1000)),qty:100}).unwrap(); }
   let ev=game.step().unwrap();
   if t>=19 { println!("t={t} {ev:?}"); }
  }
 } else if mode=="two-stock" {
  let mut cfg=setup(0).unwrap();let mut second=cfg.stocks[0].clone();second.code=StockCode("600002".to_owned());cfg.stocks.push(second);
  let mut game=GameSession::new(cfg,42).unwrap();
  for idx in 0..2 { game.enqueue_player_intent(engine::AccountId(0),engine::Intent::PlaceLimit{code:engine::StockId(idx),side:engine::Side::Buy,price:engine::LimitPrice::Fixed(Money::from_cents(1000)),qty:100}).unwrap(); }
  println!("events={:?}",game.step());let save=game.save().unwrap();println!("orders={:?}",save.resting_orders);println!("restore={:?}",GameSession::restore(&save).map(|_|()));
 } else {
  let game=GameSession::new(setup(0).unwrap(),42).unwrap();
  let mut save=game.save().unwrap();
  save.book_next_sequences.insert(engine::StockId(0),0xFFFF_FFFF);
  save.next_order_id=0x1_0000_0001;
  let mut game=GameSession::restore(&save).unwrap();
  for t in 1..=3 {
   game.enqueue_player_intent(engine::AccountId(0),engine::Intent::PlaceLimit{code:engine::StockId(0),side:engine::Side::Buy,price:engine::LimitPrice::Fixed(Money::from_cents(1000)),qty:100}).unwrap();
   let ev=game.step();println!("t={t} {ev:?}");
   if ev.is_err() {break;}
  }
 }
}
