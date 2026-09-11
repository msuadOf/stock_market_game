//! 任务 26 验收套件：真实 GameSession 内的完整决策链 + 共同 V 删除后的
//! 市场行为。夹具用**默认 5 股票的精确股本**（命中公司域默认表开局数字）
//! + 全部三类 NPC（5 种机构风格轮换、散户六风格、游资两风格）。

use engine::account::StockCode;
use engine::money::Money;
use engine::session::{
    FloatAllocation, GameSession, NpcSetup, SecurityCategory, SessionSetup, StockExchange,
    StockSpec,
};
use engine::strategy::{HotParams, InstParams, RetailParams, StrategyParams};

mod failures;
mod gold;

pub(crate) const SEED: u64 = 0x26_C0FFEE;

fn stock(code: &str, price_cents: i64, category: SecurityCategory, total_shares: u64) -> StockSpec {
    StockSpec {
        code: StockCode(code.to_string()),
        exchange: if code.starts_with('6') {
            StockExchange::Shanghai
        } else {
            StockExchange::Shenzhen
        },
        initial_price: Money::from_cents(price_cents),
        category,
        limit_pct: category.limit_pct(),
        tick: Money::from_cents(1),
        total_shares,
        float_shares: (total_shares / 2) as u32,
    }
}

/// 压缩时钟的多风格场景（默认 5 股票 × 真实默认股本——公司域默认表命中）。
pub(crate) fn chain_setup(start_iso: &str) -> SessionSetup {
    SessionSetup {
        stocks: vec![
            stock("600101", 1_120, SecurityCategory::MainBoard, 8_928_571_429),
            stock("002156", 2_735, SecurityCategory::MainBoard, 2_925_045_704),
            stock("300260", 3_680, SecurityCategory::ChiNext, 815_217_391),
            stock("600610", 755, SecurityCategory::MainBoard, 1_059_602_649),
            stock("000812", 285, SecurityCategory::StMainBoard, 1_052_631_579),
        ],
        npcs: NpcSetup {
            retail_count: 12,
            inst_count: 10,
            hot_count: 4,
            retail_cash_median: Money::from_cents(100_000_000),
        },
        config: engine::config::GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.4,
                order_size_mean: 200,
                chase_prob: 0.3,
                tick_cents: 2,
            },
            inst: InstParams {
                margin: 0.03,
                order_size: 5_000,
            },
            hot: HotParams {
                lookback: 10,
                trend_threshold: 0.02,
                order_size: 1_000,
            },
        },
        ticks_per_day: 60,
        auction_ticks: 6,
        closing_auction_ticks: 3,
        history_len: 10,
        t1_enabled: true,
        float_allocation: FloatAllocation::ByKind {
            retail: 0.4,
            inst: 0.5,
            hot: 0.1,
        },
        start_date: engine::CivilDate::from_iso(start_iso).unwrap(),
    }
}

/// 跑完一个完整交易日 + 当日 civil 日结（K4：经营终局 → 封账 → 披露）。
pub(crate) fn run_full_day(session: &mut GameSession) -> Vec<engine::session::Event> {
    let ticks = 60;
    let mut events = Vec::new();
    for _ in 0..ticks {
        events.extend(session.step());
    }
    session
        .end_civil_day()
        .expect("a fully completed trading day must settle");
    events
}

#[test]
fn fixture_is_a_legal_default_shaped_session() {
    let session = GameSession::new(chain_setup("2030-01-07"), SEED)
        .expect("default-shaped fixture must be valid");
    assert_eq!(session.market_count(), 5);
    assert_eq!(session.account_count(), 27);
}
