//! 压缩会话夹具（civil_clock 套件同款 120 tick/日）：周末发布/公告金样用。

use engine::calendar::CivilDate;
use engine::money::Money;
use engine::session::{
    FloatAllocation, NpcSetup, SecurityCategory, SessionSetup, StockExchange, StockSpec,
};
use engine::strategy::{HotParams, InstParams, RetailParams};
use engine::{GameConfig, StockCode};

/// 压缩会话每日 tick 数（civil_clock 套件先例）。
pub(crate) const TICKS_PER_DAY: u64 = 120;

/// 压缩会话 setup（start 由调用方给）。
pub(crate) fn civil_setup(start: CivilDate) -> SessionSetup {
    SessionSetup {
        stocks: vec![StockSpec {
            code: StockCode("600101".to_string()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 10_000_000,
            float_shares: 1_000_000,
        }],
        npcs: NpcSetup {
            retail_count: 16,
            inst_count: 1,
            hot_count: 1,
            retail_cash_median: Money::from_cents(10_000_000),
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: engine::StrategyParams {
            retail: RetailParams {
                arrival_rate: 1.0,
                order_size_mean: 100,
                chase_prob: 0.2,
                tick_cents: 1,
            },
            inst: InstParams {
                margin: 0.05,
                order_size: 200,
            },
            hot: HotParams {
                lookback: 3,
                trend_threshold: 0.02,
                order_size: 200,
            },
        },
        ticks_per_day: TICKS_PER_DAY,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 5,
        t1_enabled: true,
        float_allocation: FloatAllocation::Random,
        start_date: start,
    }
}
