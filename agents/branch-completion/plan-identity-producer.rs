// 最小真实生产存档见证；读取生产 API，不改动 wire 字节。
use engine::account::StockCode;
use engine::money::Money;
use engine::session::{FloatAllocation, GameSession, NpcSetup, SecurityCategory, SessionSetup,
    StockExchange, StockSpec, WithinKindDistribution, SIMULATION_POLICY_ID};
const SEED: u64 = 0x27_C0FFEE;
const TICKS_PER_DAY: usize = 1;

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

/// 一只默认股票 + 全 NPC 类型，真实运行两天的获知/信念/计划链。
/// 本套件验证存档字段与恢复行为；多只股票会重复生成公司前史，并不能
/// 增强这些断言。仍保留完整的公司账本、公开信息与个人决策状态。
fn contract_setup() -> SessionSetup {
    SessionSetup {
        company_system: None,
        dividend_tax_mode: None,
        flat_withholding_bp: None,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
        par_value_per_share: None,
        auto_corporate_foundation: false,
        report_frequency: None,
        company_operations: None,
        groups: Vec::new(),
        stocks: vec![stock(
            "600101",
            1_120,
            SecurityCategory::MainBoard,
            8_928_571_429,
        )],
        npcs: NpcSetup {
            retail_count: 2,
            inst_count: 2,
            hot_count: 1,
            retail_cash_median: Money::from_cents(100_000_000),
        },
        config: engine::config::GameConfig::proposed_defaults(),
        strategy_params: engine::StrategyParams {
            retail: engine::RetailParams {
                arrival_rate: 0.4,
                order_size_mean: 200,
                chase_prob: 0.3,
            },
            inst: engine::InstParams {
                margin: 0.03,
                order_size: 5_000,
            },
            hot: engine::HotParams {
                lookback: 10,
                trend_threshold: 0.02,
                order_size: 1_000,
            },
        },
        ticks_per_day: TICKS_PER_DAY as u64,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 10,
        t1_enabled: true,
        float_allocation: FloatAllocation::class_percentages(
            0.4,
            0.5,
            0.1,
            WithinKindDistribution::Random,
        ),
        start_date: engine::CivilDate::from_iso("2030-01-07").unwrap(),
        simulation_policy_id: SIMULATION_POLICY_ID.to_string(),
    }
}


fn main() {
    let mut session = GameSession::new(contract_setup(), SEED).unwrap();
    for _ in 0..2 {
        session.step().unwrap();
        session.end_civil_day().unwrap();
    }
    let save = session.save().unwrap();
    assert!(save.plans.plan_ids().next().is_some(), "生产链必须生成真实计划");
    let encoded = serde_json::to_vec(&save).unwrap();
    let decoded = engine::session::decode_save_slot(&encoded, &engine::session::SaveDecodeLimits::default()).unwrap();
    GameSession::restore(&decoded).unwrap();
    std::fs::write(std::env::args().nth(1).expect("输出路径必填"), encoded).unwrap();
}
