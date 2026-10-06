//! 真实 `GameSession` 的公司会计、信息、计划与撮合场景。
//!
//! 本套件使用会话的公共测试接口；断言中的每笔成交均由 orderbook 产生，
//! 不虚构 `Trade`、candle、fill 或流动性。

include!("../../test-support/simple_company.rs");

use engine::account::StockCode;
use engine::money::Money;
use engine::session::{
    FloatAllocation, GameSession, NpcSetup, SecurityCategory, SessionSetup, StockExchange,
    StockSpec,
};
use engine::WithinKindDistribution;
use engine::strategy::{HotParams, InstParams, RetailParams, StrategyParams};
use serde::Deserialize;

mod controlled;
mod controlled_experience;
mod lifecycle;
mod restore;

pub(crate) const SEED: u64 = 0x28_C0FFEE;
pub(crate) const TICKS_PER_DAY: u64 = 6;

#[derive(Deserialize)]
pub(crate) struct ScenarioFixture {
    pub start_date: String,
}

pub(crate) fn fixture() -> ScenarioFixture {
    serde_json::from_str(include_str!(
        "../fixtures/company-model/company-information-scenario.json"
    ))
    .expect("公司经营与交易场景 fixture 的结构必须合法")
}

pub(crate) fn code(value: &str) -> StockCode {
    StockCode(value.to_owned())
}

fn stock(
    value: &str,
    price_cents: i64,
    category: SecurityCategory,
    total_shares: u64,
) -> StockSpec {
    StockSpec {
        code: code(value),
        exchange: if value.starts_with('6') {
            StockExchange::Shanghai
        } else {
            StockExchange::Shenzhen
        },
        initial_price: Money::from_cents(price_cents),
        category,
        limit_pct: category.limit_pct(),
        tick: Money::from_cents(1),
        total_shares,
        float_shares: 1_000_000,
    }
}

pub(crate) fn setup(start_date: &str) -> SessionSetup {
    SessionSetup {
        company_system: simple_company_fixture!(engine; ["600101", "002156", "300260", "600610", "000812"]),
        stocks: vec![
            stock("600101", 1_120, SecurityCategory::MainBoard, 8_928_571_429),
            stock("002156", 2_735, SecurityCategory::MainBoard, 2_925_045_704),
            stock("300260", 3_680, SecurityCategory::ChiNext, 815_217_391),
            stock("600610", 755, SecurityCategory::MainBoard, 1_059_602_649),
            stock("000812", 285, SecurityCategory::StMainBoard, 1_052_631_579),
        ],
        npcs: NpcSetup {
            retail_count: 2,
            inst_count: 2,
            hot_count: 1,
            retail_cash_median: Money::from_cents(100_000_000),
        },
        config: engine::GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.4,
                order_size_mean: 200,
                chase_prob: 0.3,
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
        ticks_per_day: TICKS_PER_DAY,
        auction_ticks: 3,
        closing_auction_ticks: 1,
        history_len: 10,
        t1_enabled: true,
        report_frequency: engine::information::ReportFrequency::Quarterly,
        float_allocation: FloatAllocation::class_percentages(0.4, 0.5, 0.1, WithinKindDistribution::Random),
        start_date: engine::CivilDate::from_iso(start_date).expect("fixture civil date is valid"),
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_owned(),
        dividend_tax_mode: engine::company::cash_dividend_tax::CashDividendTaxMode::Exempt,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
    }
}

/// Disclosure-focused scenarios still use the real company, information and
/// belief pipelines, but do not need five issuers or six market ticks on every
/// trading day while advancing through a civil reporting window.
pub(crate) fn focused_disclosure_session(start_date: &str) -> GameSession {
    let mut focused = setup(start_date);
    focused.stocks.retain(|stock| stock.code.0 == "600101");
    focused.company_system = simple_company_fixture!(engine; codes = focused.stocks.iter().map(|stock| stock.code.0.as_str()));
    focused.npcs = NpcSetup {
        retail_count: 0,
        inst_count: 2,
        hot_count: 0,
        retail_cash_median: Money::from_cents(100_000_000),
    };
    focused.ticks_per_day = 1;
    focused.auction_ticks = 0;
    focused.closing_auction_ticks = 0;
    focused.history_len = 1;
    focused.float_allocation = FloatAllocation::class_percentages(0.0, 1.0, 0.0, WithinKindDistribution::Random);
    GameSession::new(focused, SEED).expect("focused disclosure fixture must assemble")
}

pub(crate) fn run_focused_trading_day(session: &mut GameSession) {
    session
        .step()
        .expect("focused one-tick trading day must complete");
}

fn representative_setup(start_date: &str) -> SessionSetup {
    let mut representative = setup(start_date);
    representative.stocks.retain(|stock| stock.code.0 == "600101");
    representative.company_system = simple_company_fixture!(engine; codes = representative.stocks.iter().map(|stock| stock.code.0.as_str()));
    representative
}

pub(crate) fn representative_session(start_date: &str) -> GameSession {
    GameSession::new(representative_setup(start_date), SEED)
        .expect("单发行人完整交易阶段 fixture 必须成功装配")
}

#[test]
fn representative_fixture_retains_all_npc_kinds_and_trading_phases() {
    let full = setup("2030-01-07");
    let representative = representative_setup("2030-01-07");
    assert_eq!(representative.stocks.len(), 1);
    assert_eq!(representative.npcs.retail_count, full.npcs.retail_count);
    assert_eq!(representative.npcs.inst_count, full.npcs.inst_count);
    assert_eq!(representative.npcs.hot_count, full.npcs.hot_count);
    assert_eq!(
        representative.npcs.retail_cash_median,
        full.npcs.retail_cash_median
    );
    assert_eq!(representative.ticks_per_day, full.ticks_per_day);
    assert_eq!(representative.auction_ticks, full.auction_ticks);
    assert_eq!(
        representative.closing_auction_ticks,
        full.closing_auction_ticks
    );
    assert_eq!(representative.t1_enabled, full.t1_enabled);
    assert_eq!(
        serde_json::to_value(representative.float_allocation).unwrap(),
        serde_json::to_value(full.float_allocation).unwrap()
    );
}

#[test]
fn cross_day_fixture_is_short_but_retains_market_phase_and_strategy_coverage() {
    let setup = setup("2030-01-07");

    assert_eq!(TICKS_PER_DAY, 6);
    assert_eq!(setup.auction_ticks, 3);
    assert_eq!(setup.closing_auction_ticks, 1);
    assert!(
        setup.auction_ticks / 3 > 0,
        "opening cancel window remains covered"
    );
    assert!(
        setup.ticks_per_day > setup.auction_ticks + setup.closing_auction_ticks,
        "continuous matching remains covered"
    );
    assert_eq!(
        setup.stocks.len(),
        5,
        "all configured A-share categories remain present"
    );
    assert_eq!(setup.npcs.retail_count, 2);
    assert_eq!(setup.npcs.inst_count, 2);
    assert_eq!(setup.npcs.hot_count, 1);
}

pub(crate) fn run_trading_day(session: &mut GameSession) -> Vec<engine::session::Event> {
    let mut events = Vec::new();
    for _ in 0..TICKS_PER_DAY {
        events.extend(session.step().expect("healthy step"));
    }
    events
}
