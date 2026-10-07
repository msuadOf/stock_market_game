include!("../../packages/engine/test-support/simple_company.rs");

use engine::session::protocol::ProtocolSession;
use engine::{
    CivilDate, GameConfig, HotParams, InstParams, Money, NpcSetup, RetailParams, SecurityCategory,
    SessionSetup, StockCode, StockExchange, StockSpec, StrategyParams,
};
use serde_json::Value;
use std::{env, error::Error, fs};

fn validate_new_save_state(encoded: &Value) -> Result<(), Box<dyn Error>> {
    let actions = encoded
        .get("corporate_actions")
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 corporate_actions 对象")?;
    for field in [
        "registries",
        "dividends",
        "stock_distributions",
        "account_gross_receipts",
        "external_receipts",
        "applied_ex_reference_groups",
        // 新局默认税务模式为大 A 个人差别化；未配置股东名册时不得产生任何税账。
        "dividend_tax_books",
        // M 批新契约字段：休市新局无配股/回购事实，必须是 Engine 生成的空数组。
        "rights_offerings",
        "rights_subscription_queue",
        "issuer_repurchases",
    ] {
        if actions
            .get(field)
            .and_then(Value::as_array)
            .is_none_or(|rows| !rows.is_empty())
        {
            return Err(
                format!("休市新局 corporate_actions.{field} 必须是 Engine 生成的空数组").into(),
            );
        }
    }

    let markets = encoded
        .get("snapshot")
        .and_then(|snapshot| snapshot.get("markets"))
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 snapshot.markets")?;
    for (code, market) in markets {
        for field in ["cash_ex_reference_pending_trade", "day_market_activity"] {
            if market.get(field).and_then(Value::as_bool).is_none() {
                return Err(format!("休市 snapshot.markets.{code}.{field} 缺少布尔状态").into());
            }
        }
        if !market
            .get("last_cash_ex_reference")
            .is_some_and(Value::is_null)
        {
            return Err(format!(
                "休市 snapshot.markets.{code}.last_cash_ex_reference 必须为 Engine 生成的 null"
            )
            .into());
        }
    }

    let companies = encoded
        .get("company_system")
        .and_then(|system| system.get("implementation"))
        .and_then(|implementation| implementation.get("state"))
        .and_then(|state| state.get("companies"))
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 Simple companies")?;
    for (company, state) in companies {
        let finance = state
            .get("finance")
            .and_then(Value::as_object)
            .ok_or_else(|| format!("休市 Simple company {company} 缺少 finance"))?;
        if !finance.get("legal_facts").is_some_and(Value::is_null)
            || finance
                .get("dividends")
                .and_then(Value::as_object)
                .is_none_or(|rows| !rows.is_empty())
            || finance
                .get("stock_distributions")
                .and_then(Value::as_object)
                .is_none_or(|rows| !rows.is_empty())
            || finance
                .get("rights_offerings")
                .and_then(Value::as_object)
                .is_none_or(|rows| !rows.is_empty())
            || finance
                .get("issuer_repurchases")
                .and_then(Value::as_object)
                .is_none_or(|rows| !rows.is_empty())
        {
            return Err(format!(
                "休市 Simple company {company} 的 legal_facts 必须为 null 且 dividends、stock_distributions、rights_offerings 与 issuer_repurchases 必须为空对象"
            )
            .into());
        }
        let journal = finance
            .get("books")
            .and_then(|books| books.get("journal"))
            .and_then(|journal| journal.get("batches"))
            .and_then(Value::as_array)
            .ok_or_else(|| format!("休市 Simple company {company} 缺少 journal source facts"))?;
        let mut sources = std::collections::BTreeSet::new();
        for batch in journal {
            for entry in batch
                .as_array()
                .ok_or_else(|| format!("休市 Simple company {company} 的 journal batch 不是数组"))?
            {
                let source = entry
                    .get("source")
                    .and_then(Value::as_u64)
                    .filter(|source| *source > 0)
                    .ok_or_else(|| {
                        format!("休市 Simple company {company} 的 journal entry 缺少正整数 source")
                    })?;
                if !sources.insert(source) {
                    return Err(
                        format!("休市 Simple company {company} 的 journal source 重复").into(),
                    );
                }
            }
        }
        let next_source = finance
            .get("next_event_id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("休市 Simple company {company} 缺少十进制字符串 next_event_id"))?
            .parse::<u64>()?;
        if sources.is_empty()
            || sources.iter().next() != Some(&1)
            || sources
                .iter()
                .next_back()
                .is_none_or(|source| next_source <= *source)
        {
            return Err(format!(
                "休市 Simple company {company} 的 journal source 与 next_event_id 不一致"
            )
            .into());
        }
    }
    Ok(())
}

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
        dividend_tax_mode: engine::company::cash_dividend_tax::CashDividendTaxMode::IndividualPublicMarket,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
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
    validate_new_save_state(&encoded)?;
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
