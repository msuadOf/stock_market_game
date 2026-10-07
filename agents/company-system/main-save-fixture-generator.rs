//! 主存档 fixture producer：与 Web 默认新局同构的 5 股场景，推进到 2030-01-09
//! （交易日 01-02..01-08 各完整 tick 日 + 日结）后保存；内置 restore/resave 深等
//! 与场景守卫。真值一律由当前 release Engine 正规生成，不手补 JSON。

include!("../../packages/engine/test-support/simple_company.rs");

use engine::session::protocol::ProtocolSession;
use engine::{
    CivilDate, GameConfig, HotParams, InstParams, Money, NpcSetup, RetailParams, SecurityCategory,
    SessionSetup, StockCode, StockExchange, StockSpec, StrategyParams,
};
use std::{env, error::Error, fs};

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = env::args().skip(1);
    let output_path = arguments.next().ok_or("缺少输出路径")?;
    if arguments.next().is_some() {
        return Err("参数过多".into());
    }
    let stocks = [
        ("600101", StockExchange::Shanghai, 1_120, SecurityCategory::MainBoard, 0.10),
        ("002156", StockExchange::Shenzhen, 1_000, SecurityCategory::MainBoard, 0.10),
        ("300260", StockExchange::Shenzhen, 2_150, SecurityCategory::ChiNext, 0.20),
        ("600610", StockExchange::Shanghai, 530, SecurityCategory::MainBoard, 0.10),
        ("000812", StockExchange::Shenzhen, 860, SecurityCategory::MainBoard, 0.10),
    ];
    let setup = SessionSetup {
        stocks: stocks
            .iter()
            .map(|(code, exchange, price, category, limit_pct)| StockSpec {
                code: StockCode((*code).into()),
                exchange: *exchange,
                initial_price: Money::from_cents(*price),
                category: *category,
                limit_pct: *limit_pct,
                tick: Money::from_cents(1),
                total_shares: 10_000_000,
                float_shares: 3_000_000,
            })
            .collect(),
        company_system: simple_company_fixture!(engine; [
            "600101", "002156", "300260", "600610", "000812"
        ]),
        npcs: NpcSetup {
            retail_count: 40,
            inst_count: 8,
            hot_count: 4,
            retail_cash_median: Money::from_cents(1_000_000),
        },
        config: GameConfig::new(
            0.00025,
            Money::from_cents(500),
            0.0005,
            0.10,
            0.10,
            true,
            100,
            Money::from_cents(1_000_000_000),
        )?,
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.3,
                order_size_mean: 300,
                chase_prob: 0.4,
            },
            inst: InstParams {
                margin: 0.02,
                order_size: 200_000,
            },
            hot: HotParams {
                lookback: 20,
                trend_threshold: 0.03,
                order_size: 100_000,
            },
        },
        ticks_per_day: 15_480,
        auction_ticks: 900,
        closing_auction_ticks: 180,
        history_len: 20,
        t1_enabled: true,
        float_allocation: engine::FloatAllocation::class_percentages(
            0.45,
            0.53,
            0.02,
            engine::WithinKindDistribution::Random,
        ),
        report_frequency: engine::information::ReportFrequency::Quarterly,
        start_date: CivilDate::from_iso("2030-01-01")?,
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_string(),
        dividend_tax_mode: engine::company::cash_dividend_tax::CashDividendTaxMode::IndividualPublicMarket,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
    };
    setup.validate()?;
    let mut session = ProtocolSession::new(setup, 20260101)?;
    if !session.civil_day_ready()? {
        return Err("2030-01-01 休市自然日未就绪".into());
    }
    session.end_civil_day_update()?;
    while session.civil_date() < CivilDate::from_iso("2030-01-09")? {
        while !session.civil_day_ready()? {
            session.step_frame()?;
        }
        session.end_civil_day_update()?;
    }
    let save = session.save()?;
    if save.civil_clock.current_date != CivilDate::from_iso("2030-01-09")? {
        return Err("主档未推进到 2030-01-09".into());
    }
    let encoded = serde_json::to_value(&save)?;
    for field in [
        "rights_offerings",
        "rights_subscription_queue",
        "issuer_repurchases",
    ] {
        if encoded["corporate_actions"][field].as_array().map(Vec::is_empty) != Some(true) {
            return Err(format!("主档 corporate_actions.{field} 必须为空数组（本场景无配股/回购）").into());
        }
    }
    let restored = ProtocolSession::restore(&save)?;
    if serde_json::to_value(restored.save()?)? != encoded {
        return Err("主档 restore 后重新保存与原存档不一致".into());
    }
    fs::write(output_path, serde_json::to_string(&save)? + "\n")?;
    Ok(())
}
