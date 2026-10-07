// 主存档 fixture producer：以显式 SessionSetup（对齐 Web DEFAULT_SETUP 的五股
// 场景）用当前 release Engine 正规生成 `current-schema-save.json`。
// 场景：2030-01-07（周一）开局，完成 01-07、01-08 两个完整交易日日结，
// 在 01-09 开盘前（tick 120、无盘内 tick）保存；内置守卫验证 tick/日期/
// 公司数/公司行为空事实，并验证 ProtocolSession::restore 后重新保存深度相等。
// 本批（P 批公司行为偏好）起 setup 各公司必须显式携带 preferences 字段；
// fixture 保持「未配置偏好」的行为中性默认（不自动产生方案）。
// M 批（配股／增发与回购机制）起新局必须显式携带双开关（默认 false），
// 守卫同时验证 corporate_actions 机制账簿为空（本场景无配股/回购）。

use engine::session::protocol::ProtocolSession;
use engine::{
    CivilDate, FloatAllocation, GameConfig, HotParams, InstParams, Money, NpcSetup, RetailParams,
    SecurityCategory, SessionSetup, StockCode, StockExchange, StockSpec, StrategyParams,
    WithinKindDistribution,
};
use serde_json::Value;
use std::{env, error::Error, fs};

fn stock(
    code: &str,
    exchange: StockExchange,
    initial_price: i64,
    category: SecurityCategory,
    total_shares: u64,
    float_shares: u64,
) -> StockSpec {
    StockSpec {
        code: StockCode(code.into()),
        exchange,
        initial_price: Money::from_cents(initial_price),
        category,
        limit_pct: if matches!(category, SecurityCategory::ChiNext) { 0.20 } else { 0.10 },
        tick: Money::from_cents(1),
        total_shares,
        float_shares: u32::try_from(float_shares).expect("fixture float shares fit u32"),
    }
}

fn validate_main_save(encoded: &Value) -> Result<(), Box<dyn Error>> {
    let actions = encoded
        .get("corporate_actions")
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 corporate_actions 对象")?;
    for field in [
        "registries",
        "dividends",
        "stock_distributions",
        "rights_offerings",
        "rights_subscription_queue",
        "issuer_repurchases",
        "account_gross_receipts",
        "external_receipts",
        "applied_ex_reference_groups",
        "dividend_tax_books",
    ] {
        if actions
            .get(field)
            .and_then(Value::as_array)
            .is_none_or(|rows| !rows.is_empty())
        {
            return Err(format!("主档 corporate_actions.{field} 必须是 Engine 生成的空数组").into());
        }
    }
    let state = encoded
        .get("company_system")
        .and_then(|system| system.get("implementation"))
        .and_then(|implementation| implementation.get("state"))
        .ok_or("Engine 保存结果缺少 Simple 实现状态")?;
    let companies = state
        .get("companies")
        .and_then(Value::as_object)
        .ok_or("主档缺少 Simple companies")?;
    if companies.len() != 5 {
        return Err(format!("主档必须包含五个 Simple 公司，实际 {}", companies.len()).into());
    }
    for (company, entry) in companies {
        let ledger = entry
            .get("preference_ledger")
            .ok_or(format!("主档公司 {company} 缺少偏好台账字段"))?;
        if ledger
            .get("rejections")
            .and_then(Value::as_array)
            .is_none_or(|rows| !rows.is_empty())
        {
            return Err(format!("主档公司 {company} 偏好台账必须为空（未配置偏好）").into());
        }
        let config_preferences = state
            .get("config")
            .and_then(|config| config.get("companies"))
            .and_then(Value::as_array)
            .ok_or("主档缺少 config.companies")?
            .iter()
            .find_map(|item| {
                (item.get("company").and_then(Value::as_str) == Some(company))
                    .then(|| item.get("preferences"))
                    .flatten()
            })
            .ok_or(format!("主档 config 缺少公司 {company} 的偏好配置"))?;
        if !config_preferences
            .get("cash_dividend")
            .is_some_and(Value::is_null)
            || !config_preferences
                .get("stock_distribution")
                .is_some_and(Value::is_null)
        {
            return Err(format!("主档公司 {company} 的偏好配置必须为未配置（两项皆 null）").into());
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

    let mut setup = SessionSetup {
        stocks: vec![
            stock("600101", StockExchange::Shanghai, 1_120, SecurityCategory::MainBoard, 8_928_571_429, 169_318_418),
            stock("002156", StockExchange::Shenzhen, 2_735, SecurityCategory::MainBoard, 2_925_045_704, 1_462_522_852),
            stock("300260", StockExchange::Shenzhen, 3_680, SecurityCategory::ChiNext, 815_217_391, 407_608_695),
            stock("600610", StockExchange::Shanghai, 755, SecurityCategory::MainBoard, 1_059_602_649, 529_801_324),
            stock("000812", StockExchange::Shenzhen, 285, SecurityCategory::StMainBoard, 1_052_631_579, 526_315_789),
        ],
        company_system: engine::company::config::CompanySystemConfig::Simple(
            engine::company::simple::SimpleConfig {
                environment: engine::company::simple::SimpleEnvironmentConfig {
                    initial_change_bp: 0,
                    persistence_bp: 0,
                    noise: engine::company::simple::period::PeriodNoiseConfig {
                        monthly_bp: 0,
                        quarterly_bp: 0,
                        half_year_bp: 0,
                        annual_bp: 0,
                    },
                },
                companies: ["600101", "002156", "300260", "600610", "000812"]
                    .into_iter()
                    .map(|code| engine::company::simple::SimpleCompanyConfig {
                        company: engine::company::CompanyId(format!("C-{code}")),
                        kind: engine::company::CompanyKind::Industrial,
                        generation: engine::company::simple::period::PeriodGenerationParameters {
                            initial_revenue: engine::accounting::AccountingAmount::from_cents(100_000_000_000),
                            initial_fixed_expense: engine::accounting::AccountingAmount::from_cents(90_000_000_000),
                            revenue_trend: engine::company::simple::period::AnnualTrendConfig::Fixed { annual_growth_bp: 0 },
                            fixed_expense_trend: engine::company::simple::period::AnnualTrendConfig::Fixed { annual_growth_bp: 0 },
                            revenue_noise: engine::company::simple::period::PeriodNoiseConfig {
                                monthly_bp: 0, quarterly_bp: 0, half_year_bp: 0, annual_bp: 0,
                            },
                            fixed_expense_noise: engine::company::simple::period::PeriodNoiseConfig {
                                monthly_bp: 0, quarterly_bp: 0, half_year_bp: 0, annual_bp: 0,
                            },
                            demand_sensitivity_bp: 0,
                            variable_expense: engine::company::simple::period::PeriodVariableExpenseRule::RevenueRatio {
                                ratio_bp: 0,
                                noise: engine::company::simple::period::PeriodNoiseConfig {
                                    monthly_bp: 0, quarterly_bp: 0, half_year_bp: 0, annual_bp: 0,
                                },
                            },
                        },
                        finance: engine::company::simple::SimpleFinanceConfig {
                            opening_lines: vec![
                                engine::accounting::JournalLine {
                                    account: engine::accounting::LedgerAccountId("1122".into()),
                                    side: engine::accounting::PostingSide::Debit,
                                    amount: engine::accounting::AccountingAmount::from_cents(50_000_000_000),
                                },
                                engine::accounting::JournalLine {
                                    account: engine::accounting::LedgerAccountId("4001".into()),
                                    side: engine::accounting::PostingSide::Credit,
                                    amount: engine::accounting::AccountingAmount::from_cents(50_000_000_000),
                                },
                            ],
                            tax_policy: engine::accounting::TaxPolicy {
                                version: 1,
                                vat: engine::accounting::VatPolicy {
                                    output_rate_bp: 1_300,
                                    input_rate_bp: 1_300,
                                    deductible_share_bp: 10_000,
                                },
                                income_tax: engine::accounting::IncomeTaxPolicy {
                                    rate_bp: 2_500,
                                    loss_carryforward_years: 5,
                                },
                            },
                            summary_rule: engine::company::simple::SimpleSummaryRule::ReceivableRevenuePayableExpenses,
                        },
                        // 主档保持行为中性：未配置偏好（P 批严格持久化新字段）。
                        preferences: engine::company::simple::SimpleCompanyPreferences::none(),
                    })
                    .collect(),
                prehistory_periods: 24,
                settlement_cycle: engine::company::simple::period::SettlementCycle::Monthly,
            },
        ),
        npcs: NpcSetup {
            retail_count: 12,
            inst_count: 10,
            hot_count: 4,
            retail_cash_median: Money::from_cents(100_000_000),
        },
        config: GameConfig::new(
            0.00025,
            Money::from_cents(500),
            0.0005,
            0.10,
            0.10,
            true,
            100,
            Money::from_cents(10_000_000),
        )?,
        strategy_params: StrategyParams {
            retail: RetailParams { arrival_rate: 0.4, order_size_mean: 200, chase_prob: 0.3 },
            inst: InstParams { margin: 0.03, order_size: 5_000 },
            hot: HotParams { lookback: 10, trend_threshold: 0.02, order_size: 1_000 },
        },
        ticks_per_day: 60,
        auction_ticks: 6,
        closing_auction_ticks: 3,
        history_len: 10,
        t1_enabled: true,
        float_allocation: FloatAllocation::class_percentages(
            0.4,
            0.5,
            0.1,
            WithinKindDistribution::Random,
        ),
        report_frequency: engine::information::ReportFrequency::Quarterly,
        start_date: CivilDate::from_iso("2030-01-07")?,
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_string(),
        dividend_tax_mode: engine::company::cash_dividend_tax::CashDividendTaxMode::IndividualPublicMarket,
        // 主档保持机制关闭（M 批严格持久化新开关，默认 false）。
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
    };
    setup.validate()?;

    let seed = 666_959_854;
    let mut session = ProtocolSession::new(setup, seed)?;
    for expected_next in ["2030-01-08", "2030-01-09"] {
        while !session.civil_day_ready()? {
            session.step_frame()?;
        }
        let update = session.end_civil_day_update()?;
        if update.civil_date != expected_next {
            return Err(format!(
                "日结后的自然日 {} 与预期 {} 不一致",
                update.civil_date, expected_next
            )
            .into());
        }
    }
    let save = session.save()?;
    if save.snapshot.tick != 120
        || save.civil_clock.settled_through != Some(CivilDate::from_iso("2030-01-08")?)
        || save.civil_clock.current_date != CivilDate::from_iso("2030-01-09")?
    {
        return Err("主档未保持 tick 120、settledThrough 2030-01-08 与 currentDate 2030-01-09".into());
    }

    let encoded = serde_json::to_value(&save)?;
    validate_main_save(&encoded)?;
    let restored = ProtocolSession::restore(&save)?;
    if serde_json::to_value(restored.save()?)? != encoded {
        return Err("ProtocolSession::restore 后重新保存与原保存不一致".into());
    }
    fs::write(output_path, serde_json::to_string(&save)? + "\n")?;
    Ok(())
}
