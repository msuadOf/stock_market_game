use super::*;
use crate::runtime::group::{GroupScript, TickPlan};
use crate::runtime::world::{DeskStockSpec, World, WorldSpec};
use crate::runtime::watchdog::WatchdogConfig;
use std::time::Duration;
use std::sync::Arc;
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

fn world() -> World {
    let mut spec = basic_spec(2, 2, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.initial_cash = 100;
    spec.watchdog = WatchdogConfig { normal: Duration::from_millis(500), long: Duration::from_millis(500) };
    World::spawn(spec).unwrap()
}
fn ops() -> Vec<(GroupIdx, Vec<AdminOperation>)> {
    vec![(1, vec![AdminOperation::WithholdingTax { local: 1, stock: 1, amount: 30 },
                 AdminOperation::WithholdingTax { local: 1, stock: 0, amount: 20 }]),
         (0, vec![AdminOperation::WithholdingTax { local: 0, stock: 0, amount: 10 }])]
}
#[test]
fn real_tax_only_commit_preserves_id_operation_order_and_complete_accounts() {
    let mut w = world();
    let before = w.snapshot_accounts().unwrap();
    let grouped = ops();
    let prepared = w.with_admin_transaction(grouped.clone(), |candidate| {
        let value = inspect_tax_only(&before, &grouped, candidate).expect("真实税扣款应通过");
        assert_eq!(value.id, candidate.id);
        Ok(value)
    }).unwrap();
    // 发布只发生在 with_admin_transaction 成功返回之后。
    assert_eq!(prepared.operations.iter().map(|p| (p.group_index,p.operation_index,p.group,p.local,p.stock,p.amount)).collect::<Vec<_>>(),
               vec![(0,0,1,1,1,30),(0,1,1,1,0,20),(1,0,0,0,0,10)]);
    let after = w.snapshot_accounts().unwrap();
    let mut expected = before;
    expected[1].1[1].cash = 50;
    expected[0].1[0].cash = 90;
    assert_eq!(after, expected);
    w.shutdown().unwrap();
}
