use engine::runtime::{group::{GroupScript,TickPlan},watchdog::{WatchdogConfig},world::{World,WorldSpec},GroupIdx};
use std::time::Duration;
fn basic_spec(
    n_groups: usize,
    n_stocks: usize,
    accounts_per_group: usize,
    script_for: Box<dyn Fn(GroupIdx) -> GroupScript>,
) -> WorldSpec {
    let shares = vec![vec![vec![1_000u32; n_stocks]; accounts_per_group]; n_groups];
    WorldSpec {
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
        civil_start_date: engine::calendar::CivilDate::from_ymd(2026, 1, 5).expect("测试日期合法"),
        script_for,
        chunk: 8,
        watchdog: WatchdogConfig {
            normal: Duration::from_secs(5),
            long: Duration::from_secs(10),
        },
        rounds_cap: 4,
        stocks: (0..n_stocks)
            .map(|_| engine::runtime::world::DeskStockSpec::main_board(1000))
            .collect(),
        config: std::sync::Arc::new(engine::config::GameConfig::proposed_defaults()),
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

fn main(){
 let mut spec=basic_spec(1,1,1,Box::new(|_|Box::new(|_,_|TickPlan::default())));spec.ticks_per_day=4;spec.auction_ticks=1;spec.closing_auction_ticks=1;
 let mut world=World::spawn(spec).unwrap();world.run_ticks(4).unwrap();
 world.set_day_end_validator(Box::new(|_|Err("复核注入失败".to_owned())));
 println!("first_result={:?}",world.run_day_end_public());println!("auction_after_abort={}",world.drained_auction.len());
 world.set_day_end_validator(Box::new(|_|Ok(())));println!("retry_result={:?}",world.run_day_end_public());println!("auction_after_retry={}",world.drained_auction.len());
 let _=world.shutdown().unwrap();
}
