//! 批 8a 桥接测试：SessionSetup → WorldSpec 投影 + runtime 世界与会话同构
//! 收敛（确定性配置面的接线地基验收）。

use super::bridge::{default_retail_decision, world_spec_from_setup};
use super::group::GroupScript;
use super::world::World;
use crate::money::Money;
use crate::session::SessionSetup;
use crate::StockExchange;
use crate::{SecurityCategory, StockCode, StockSpec};

fn setup_fixture() -> SessionSetup {
    let stocks = vec![
        StockSpec {
            code: StockCode("600000".to_owned()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 1_000_000,
            float_shares: 800_000,
        },
        StockSpec {
            code: StockCode("300001".to_owned()),
            exchange: StockExchange::Shenzhen,
            initial_price: Money::from_cents(2000),
            category: SecurityCategory::ChiNext,
            limit_pct: 0.20,
            tick: Money::from_cents(1),
            total_shares: 500_000,
            float_shares: 400_000,
        },
    ];
    SessionSetup {
        stocks,
        company_operations: None,
        groups: Vec::new(),
        npcs: crate::session::NpcSetup {
            retail_count: 0,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::ZERO,
        },
        config: crate::config::GameConfig::proposed_defaults(),
        strategy_params: crate::strategy::StrategyParams {
            retail: crate::strategy::RetailParams {
                arrival_rate: 0.0,
                order_size_mean: 1,
                chase_prob: 0.0,
            },
            inst: crate::strategy::InstParams {
                margin: 0.01,
                order_size: 1,
            },
            hot: crate::strategy::HotParams {
                lookback: 2,
                trend_threshold: 0.01,
                order_size: 1,
            },
        },
        ticks_per_day: 240,
        auction_ticks: 15,
        closing_auction_ticks: 3,
        history_len: 16,
        t1_enabled: true,
        float_allocation: crate::session::FloatAllocation::random(),
        start_date: crate::calendar::CivilDate::from_ymd(2026, 1, 5).expect("测试日期合法"),
        simulation_policy_id: "bridge-test".to_owned(),
    }
}

fn idle_script() -> Box<dyn Fn(super::GroupIdx) -> GroupScript> {
    Box::new(|_g| Box::new(|_t, _r| super::group::TickPlan::default()))
}

#[test]
fn bridge_projects_setup_into_world_spec() {
    let setup = setup_fixture();
    let spec = world_spec_from_setup(&setup, None, idle_script()).expect("投影成功");
    assert_eq!(spec.stocks.len(), 2);
    // 主板 10%（1000bps）/创业板 20%（2000bps）显式差异。
    assert_eq!(spec.stocks[0].limit_bps, 1000);
    assert_eq!(spec.stocks[1].limit_bps, 2000);
    // 单笔上限类别差异：主板 100 万、创业板限价 30 万/市价 15 万。
    assert_eq!(spec.stocks[0].max_order_qty_limit, 1_000_000);
    assert_eq!(spec.stocks[1].max_order_qty_limit, 300_000);
    assert_eq!(spec.stocks[1].max_order_qty_market, 150_000);
    // 相位参数直传。
    assert_eq!(spec.ticks_per_day, 240);
    assert_eq!(spec.auction_ticks, 15);
    assert_eq!(spec.closing_auction_ticks, 3);
}

#[test]
fn bridge_rejects_zero_ticks_per_day() {
    let mut setup = setup_fixture();
    setup.ticks_per_day = 0;
    let err = match world_spec_from_setup(&setup, None, idle_script()) {
        Err(e) => e,
        Ok(_) => panic!("零 ticks_per_day 必须显式报错"),
    };
    assert!(err.contains("ticks_per_day"), "错误应指明字段：{err}");
}

#[test]
fn bridged_world_converges_with_session_phase_parameters() {
    // 会话相位参数（240/15/3）下 runtime 世界可完整收敛 20 tick：
    // 开盘竞价申报窗（day_tick<10）→盘前→连续→收盘竞价→日结→day2。
    let setup = setup_fixture();
    let mut spec = world_spec_from_setup(&setup, None, idle_script()).expect("投影");
    spec.n_groups = 1;
    spec.accounts_per_group = 2;
    spec.initial_cash = 1_000_000;
    spec.initial_shares = vec![vec![vec![1_000u32; 2]; 2]];
    let mut world = World::spawn(spec).expect("世界构造");
    let committed = world.run_ticks(20).expect("20 tick 收敛（含一次日结）");
    assert_eq!(committed, 20);
    let no_day_end_yet = world.last_day_end_candidate.is_none();
    let report = world.shutdown().expect("关停");
    // 空闲脚本：守恒恒等。
    let cash: i64 = report.group_finals[0].accounts.iter().map(|a| a.cash).sum();
    assert_eq!(cash, 2 * 1_000_000);
    // tick 20 < ticks_per_day 240：未跨日，无日结候选。
    assert!(no_day_end_yet);
}

#[test]
fn default_retail_decision_is_seed_distinct() {
    let d = default_retail_decision(3, 42);
    assert_eq!(d.profiles.len(), 3);
    assert_ne!(d.profiles[0].seed, d.profiles[1].seed);
}
