//! Task-38 fresh K7 baseline fixture. It constructs a new current session per
//! run, advances both market and civil clocks, and never accepts a save path.

use std::{env, process};

use engine::calendar::CalendarExchange;
use engine::{
    run_price_volume_baseline, CivilDate, FloatAllocation, GameConfig, GameSession, HotParams,
    InstParams, Money, NpcSetup, RetailParams, SecurityCategory, SessionSetup, StockCode,
    StockExchange, StockSpec, StrategyParams, TradingCalendar,
};

const SOURCE: &str = "fresh_current_k7_setup";
const USAGE: &str = "usage: k7_baseline_fixture <primary|cross-year> <seed> <days> <behavior_multiplier> <event_multiplier> <c01_denominator_multiplier>";

#[derive(Clone, Copy)]
struct Multipliers {
    behavior: f64,
    event: f64,
    c01: f64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("K7 baseline fixture failed: {error}\n\n{USAGE}");
        process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() != 6 {
        return Err(format!("expected 6 arguments, got {}", args.len()));
    }
    let scenario = parse_scenario(&args[0])?;
    let seed = parse_u64("seed", &args[1])?;
    let days = parse_u32("days", &args[2])?;
    let multipliers = Multipliers {
        behavior: parse_multiplier("behavior_multiplier", &args[3])?,
        event: parse_multiplier("event_multiplier", &args[4])?,
        c01: parse_multiplier("c01_denominator_multiplier", &args[5])?,
    };
    let mut setup = scenario_setup(scenario)?;
    apply_behavior_multiplier(&mut setup, multipliers.behavior)?;
    let (session, calendar) =
        run_fresh_session(setup.clone(), seed, days, multiplier_bp(multipliers.event)?)?;
    let trading_days = calendar.trading_days;
    let report = run_price_volume_baseline(&setup, &[seed], trading_days)
        .map_err(|error| format!("price-volume report failed: {error}"))?;
    let causal = session
        .causal_diagnostics()
        .map_err(|error| format!("causal report failed: {error}"))?;
    let output = serde_json::json!({
        "tool": "k7_baseline_fixture",
        "source": SOURCE,
        "scenario": scenario,
        "seed": seed.to_string(),
        "natural_days": days,
        "trading_days": trading_days,
        "multipliers": {
            "behavior": multipliers.behavior,
            "event": multipliers.event,
            "c01_denominator_assumption": multipliers.c01,
        },
        "calendar": calendar,
        "company": {
            "initial_conditions": "GameSession::new current company assembly and seeded prehistory",
            "four_industry_sample": scenario == "cross-year",
        },
        "price_volume": report,
        "causal": causal,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn parse_scenario(value: &str) -> Result<&str, String> {
    match value {
        "primary" | "cross-year" => Ok(value),
        _ => Err(format!(
            "unknown scenario `{value}`; expected primary or cross-year"
        )),
    }
}

fn parse_u64(name: &str, value: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|error| format!("{name} `{value}` is not u64: {error}"))
}

fn parse_u32(name: &str, value: &str) -> Result<u32, String> {
    let parsed: u32 = value
        .parse()
        .map_err(|error| format!("{name} `{value}` is not u32: {error}"))?;
    (parsed > 0)
        .then_some(parsed)
        .ok_or_else(|| format!("{name} must be > 0"))
}

fn parse_multiplier(name: &str, value: &str) -> Result<f64, String> {
    let parsed: f64 = value
        .parse()
        .map_err(|error| format!("{name} `{value}` is not f64: {error}"))?;
    match parsed {
        0.5 | 1.0 | 2.0 => Ok(parsed),
        _ => Err(format!("{name} must be exactly 0.5, 1, or 2")),
    }
}

fn multiplier_bp(value: f64) -> Result<u16, String> {
    match value {
        0.5 => Ok(5_000),
        1.0 => Ok(10_000),
        2.0 => Ok(20_000),
        _ => Err(format!("unsupported multiplier {value}")),
    }
}

fn apply_behavior_multiplier(setup: &mut SessionSetup, multiplier: f64) -> Result<(), String> {
    setup.strategy_params.retail.arrival_rate *= multiplier;
    setup.strategy_params.retail.chase_prob =
        (setup.strategy_params.retail.chase_prob * multiplier).min(1.0);
    setup
        .strategy_params
        .validate()
        .map_err(|error| format!("behavior multiplier invalidated setup: {error}"))
}

#[derive(serde::Serialize)]
struct CalendarSummary {
    start_date: CivilDate,
    end_date: CivilDate,
    natural_days: u32,
    trading_days: u32,
    closed_days: u32,
    policy_id: String,
}

fn run_fresh_session(
    setup: SessionSetup,
    seed: u64,
    natural_days: u32,
    event_multiplier_bp: u16,
) -> Result<(GameSession, CalendarSummary), String> {
    let calendar = TradingCalendar::default_v1().map_err(|error| error.to_string())?;
    let mut session =
        GameSession::new_with_company_event_multiplier(setup.clone(), seed, event_multiplier_bp)
            .map_err(|error| error.to_string())?;
    let mut trading_days = 0;
    let mut closed_days = 0;
    for _ in 0..natural_days {
        let date = session.civil_date();
        if calendar
            .is_trading_day(CalendarExchange::Sse, date)
            .map_err(|error| error.to_string())?
        {
            for _ in 0..setup.ticks_per_day {
                session.step().map_err(|error| error.to_string())?;
            }
            trading_days += 1;
        } else {
            closed_days += 1;
        }
        session
            .end_civil_day()
            .map_err(|error| format!("day-end {date} failed: {error}"))?;
    }
    let end_date = session.civil_date();
    Ok((
        session,
        CalendarSummary {
            start_date: setup.start_date,
            end_date,
            natural_days,
            trading_days,
            closed_days,
            policy_id: setup.simulation_policy_id,
        },
    ))
}

fn scenario_setup(scenario: &str) -> Result<SessionSetup, String> {
    let (retail_count, ticks_per_day, history_len) = match scenario {
        "primary" => (20_000, 15_300, 20),
        "cross-year" => (20, 300, 20),
        _ => return Err(format!("unknown scenario `{scenario}`")),
    };
    Ok(SessionSetup {
        stocks: vec![
            stock(
                "600101",
                StockExchange::Shanghai,
                SecurityCategory::MainBoard,
                1120,
                8_928_571_429,
                3_571_428_571,
            ),
            stock(
                "002156",
                StockExchange::Shenzhen,
                SecurityCategory::MainBoard,
                2735,
                2_925_045_704,
                2_047_531_993,
            ),
            stock(
                "300260",
                StockExchange::Shenzhen,
                SecurityCategory::ChiNext,
                3680,
                815_217_391,
                611_413_043,
            ),
            stock(
                "600610",
                StockExchange::Shanghai,
                SecurityCategory::MainBoard,
                755,
                1_059_602_649,
                847_682_119,
            ),
            stock(
                "000812",
                StockExchange::Shenzhen,
                SecurityCategory::StMainBoard,
                285,
                1_052_631_579,
                842_105_263,
            ),
        ],
        npcs: NpcSetup {
            retail_count,
            inst_count: 5,
            hot_count: 2,
            retail_cash_median: Money::from_cents(20_000_000),
        },
        config: GameConfig::new(
            0.00025,
            Money::from_cents(500),
            0.0005,
            0.10,
            0.10,
            100,
            Money::from_cents(1_000_000_000),
        )
        .map_err(|error| error.to_string())?,
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.3,
                order_size_mean: 300,
                chase_prob: 0.4,
                tick_cents: 1,
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
        ticks_per_day,
        auction_ticks: if scenario == "primary" { 900 } else { 0 },
        closing_auction_ticks: if scenario == "primary" { 180 } else { 0 },
        history_len,
        t1_enabled: true,
        float_allocation: FloatAllocation::ByKind {
            retail: 0.45,
            inst: 0.53,
            hot: 0.02,
        },
        start_date: CivilDate::from_iso("2030-01-01").map_err(|error| error.to_string())?,
        simulation_policy_id: engine::SIMULATION_POLICY_ID_V1.to_string(),
    })
}

fn stock(
    code: &str,
    exchange: StockExchange,
    category: SecurityCategory,
    price_cents: i64,
    total_shares: u64,
    float_shares: u32,
) -> StockSpec {
    StockSpec {
        code: StockCode(code.to_string()),
        exchange,
        initial_price: Money::from_cents(price_cents),
        category,
        limit_pct: category.limit_pct(),
        tick: Money::from_cents(1),
        total_shares,
        float_shares,
    }
}
