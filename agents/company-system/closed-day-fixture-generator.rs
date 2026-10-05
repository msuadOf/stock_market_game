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

    let setup = SessionSetup {
        stocks: vec![StockSpec {
            code: StockCode("600101".into()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1_000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 10_000_000,
            float_shares: 0,
        }],
        company_system: simple_company_fixture!(engine; ["600101"]),
        npcs: NpcSetup {
            retail_count: 0,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::ZERO,
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
        ticks_per_day: 15_300,
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
        start_date: CivilDate::from_iso("2026-01-01")?,
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_string(),
    };
    setup.validate()?;

    let start_date = CivilDate::from_iso("2026-01-01")?;
    if engine::TradingCalendar::current_default_calendar()?
        .is_trading_day(engine::calendar::CalendarExchange::Sse, start_date)?
    {
        return Err("默认沪市交易日历将 2026-01-01 标记为交易日，不能生成休市 fixture".into());
    }

    let seed = 666_959_854;
    let mut session = ProtocolSession::new(setup, seed)?;
    if !session.civil_day_ready()? {
        return Err("休市起始自然日未就绪，不能在无市场 tick 时执行日结".into());
    }
    session.end_civil_day_update()?;
    let save = session.save()?;
    if save.snapshot.tick != 0
        || save.civil_clock.settled_through != Some(start_date)
        || save.civil_clock.current_date != start_date.next()?
    {
        return Err(
            "休市日结未保持 tick 0、settledThrough 2026-01-01 与 currentDate 2026-01-02".into(),
        );
    }

    let encoded = serde_json::to_value(&save)?;
    let advanced_through = encoded
        .get("company_system")
        .and_then(|system| system.get("implementation"))
        .and_then(|implementation| implementation.get("state"))
        .and_then(|state| state.get("advanced_through"))
        .and_then(serde_json::Value::as_str);
    if advanced_through != Some("2026-01-01") {
        return Err(format!("Simple 公司日历推进日期不符：{advanced_through:?}").into());
    }

    let restored = ProtocolSession::restore(&save)?;
    if serde_json::to_value(restored.save()?)? != encoded {
        return Err("ProtocolSession::restore 后重新保存与原保存不一致".into());
    }
    fs::write(output_path, serde_json::to_string(&save)? + "\n")?;
    Ok(())
}
