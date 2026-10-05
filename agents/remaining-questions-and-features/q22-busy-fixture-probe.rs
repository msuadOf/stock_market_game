use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

fn main() {
    let filename = std::env::args()
        .nth(1)
        .expect("需要真实 DEFAULT_SETUP JSON 路径");
    let setup: engine::SessionSetup =
        serde_json::from_slice(&std::fs::read(filename).unwrap()).unwrap();
    let next = AtomicUsize::new(0);
    let results = Arc::new(Mutex::new(Vec::new()));
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let setup = &setup;
            let next = &next;
            let results = &results;
            scope.spawn(move || loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= 24 {
                    break;
                }
                let seed = (index % 8) as u64;
                let zero_float = index >= 8;
                let native_quote = index >= 16;
                let mut setup = setup.clone();
                setup.stocks.truncate(1);
                if zero_float {
                    setup.stocks[0].float_shares = 0;
                }
                if native_quote {
                    setup.stocks[0].code = engine::StockCode("600888".to_owned());
                    setup.stocks[0].initial_price = engine::Money::from_cents(1000);
                    setup.stocks[0].total_shares = 10_000_000;
                    setup.npcs.retail_cash_median = engine::Money::from_cents(10_000_000);
                    setup.strategy_params.retail.order_size_mean = 100;
                    setup.strategy_params.retail.chase_prob = 0.0;
                    setup.config = engine::GameConfig::proposed_defaults();
                    setup.float_allocation = engine::FloatAllocation::random();
                    setup.history_len = 10;
                }
                setup.npcs.retail_count = 1;
                setup.npcs.inst_count = 0;
                setup.npcs.hot_count = 0;
                setup.strategy_params.retail.arrival_rate = 1.0;
                setup.ticks_per_day = 4;
                setup.auction_ticks = 0;
                setup.closing_auction_ticks = 0;
                setup.start_date = engine::CivilDate::from_iso("2030-01-02").unwrap();
                let mut session = engine::GameSession::new(setup, seed).unwrap();
                let source = session.shared_ingress();
                source.verification_arm("cutoff", u64::MAX, None).unwrap();
                session.step().unwrap();
                let trace = source.verification_snapshot().unwrap();
                let save = session.save().unwrap();
                let value = serde_json::to_value(&save).unwrap();
                results.lock().unwrap().push(serde_json::json!({
                    "seed": seed,
                    "zero_float": zero_float,
                    "native_quote": native_quote,
                    "observed_accounts": save.pending_npc.as_ref().unwrap().observed_accounts,
                    "npc_receipts": trace.npc_receipts,
                    "attention": value["npc_attention"]["1"],
                    "retail": value["retail_experience"]["1"],
                }));
            });
        }
    });
    let mut results = results.lock().unwrap();
    results.sort_by_key(|value| {
        (
            value["zero_float"].as_bool().unwrap(),
            value["seed"].as_u64().unwrap(),
        )
    });
    println!("{}", serde_json::to_string(&*results).unwrap());
}
