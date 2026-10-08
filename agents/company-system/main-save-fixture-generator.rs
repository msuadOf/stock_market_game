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

/// 温和默认偏好（N2a，2026-10-08 用户决策；与 Web DEFAULT_SETUP 同值）：
/// 现金分红启用（30% 派息、100 万元门槛、每结算周期），送转关闭。
fn mild_default_preferences() -> engine::company::simple::SimpleCompanyPreferences {
    engine::company::simple::SimpleCompanyPreferences {
        cash_dividend: Some(engine::company::simple::SimpleCashDividendPreference {
            target_payout_bp: 3_000,
            min_distributable_profit: Money::from_cents(100_000_000),
            cycles_between_proposals: 1,
        }),
        stock_distribution: None,
    }
}

fn validate_main_save(encoded: &Value) -> Result<(), Box<dyn Error>> {
    let actions = encoded
        .get("corporate_actions")
        .and_then(Value::as_object)
        .ok_or("Engine 保存结果缺少 corporate_actions 对象")?;
    // N2a（2026-10-08）起开局自动装配：主档必须恰好携带五只证券的自动名册
    //（发行股数守恒 + 具名外部股东承接未分配余量 + 面值推定的注册资本法定
    // 事实见下方 company_system 断言）。本场景（两交易日、无结算周期末日）
    // 不触发任何分红/送转/税账/代扣。
    let registries = actions
        .get("registries")
        .and_then(Value::as_array)
        .ok_or("主档 corporate_actions.registries 必须是 Engine 生成的数组")?;
    if registries.len() != 5 {
        return Err(format!("主档必须包含五只证券的自动装配名册，实际 {}", registries.len()).into());
    }
    for registry in registries {
        let stock = registry
            .get("stock")
            .and_then(Value::as_str)
            .ok_or("主档名册缺少 stock")?;
        let issued = registry
            .get("issued_shares")
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or("主档名册缺少 issued_shares")?;
        let mut holdings_total = 0_u64;
        let mut has_founding_external = false;
        for holding in registry
            .get("holdings")
            .and_then(Value::as_array)
            .ok_or("主档名册缺少 holdings")?
        {
            let holder = holding
                .get("holder")
                .ok_or("主档名册持有行缺少 holder")?;
            for lot in holding
                .get("lots")
                .and_then(Value::as_array)
                .ok_or("主档名册持有行缺少 lots")?
            {
                holdings_total += lot
                    .get("qty")
                    .and_then(Value::as_str)
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or("主档名册 lot 缺少 qty")?;
            }
            if let Some(external) = holder.get("External").and_then(Value::as_str) {
                if external.starts_with("session-auto:founding:") {
                    has_founding_external = true;
                }
            }
            if holder.get("IssuerTreasury").is_some() {
                return Err(format!("主档 {stock} 名册不得出现发行人自持股（全流通口径）").into());
            }
        }
        if holdings_total != issued || issued == 0 {
            return Err(format!("主档 {stock} 名册发行股数守恒失败：{holdings_total} != {issued}").into());
        }
        if !has_founding_external {
            return Err(format!("主档 {stock} 名册缺少承接未分配余量的具名外部股东").into());
        }
    }
    for field in [
        "dividends",
        "stock_distributions",
        "share_splits",
        "rights_offerings",
        "rights_subscription_queue",
        "rejected_rights_subscriptions",
        "issuer_repurchases",
        "account_gross_receipts",
        "external_receipts",
        "applied_ex_reference_groups",
        // 简税默认（三层税制）：不建任何个人税账。
        "dividend_tax_books",
        // 三层税制（2026-10-08）：简税默认；本场景无分红，不得产生任何代扣回执。
        "flat_withholding_receipts",
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
        // N2a 温和默认偏好：现金分红启用（3000bp / 100000000 分 / 1 周期）、
        // 送转关闭（null）。
        let cash = config_preferences
            .get("cash_dividend")
            .ok_or(format!("主档公司 {company} 缺少现金分红偏好字段"))?;
        if cash.get("target_payout_bp").and_then(Value::as_u64) != Some(3_000)
            || cash.get("min_distributable_profit").and_then(Value::as_str) != Some("100000000")
            || cash.get("cycles_between_proposals").and_then(Value::as_u64) != Some(1)
        {
            return Err(format!("主档公司 {company} 的现金分红偏好必须为温和默认值（3000bp/100000000 分/1 周期）").into());
        }
        if !config_preferences
            .get("stock_distribution")
            .is_some_and(Value::is_null)
        {
            return Err(format!("主档公司 {company} 的送转偏好必须为关闭（null）").into());
        }
        // 开局自动装配的注册资本法定事实：面值 100 分 × 总股本（bind-once）。
        // AccountingAmount 序列化为「元」两位小数字符串，换算回分比对。
        let legal = entry
            .get("finance")
            .and_then(|finance| finance.get("legal_facts"))
            .ok_or(format!("主档公司 {company} 缺少注册资本法定事实"))?;
        let registered_cents = legal
            .get("registered_capital")
            .and_then(|value| value.as_str())
            .and_then(|value| {
                let (whole, fraction) = value.split_once('.')?;
                let whole = whole.parse::<i128>().ok()?;
                let mut frac_cents = 0_i128;
                for (index, ch) in fraction.chars().take(2).enumerate() {
                    let digit = i128::from(ch.to_digit(10)?);
                    frac_cents += digit * if index == 0 { 10 } else { 1 };
                }
                whole.checked_mul(100)?.checked_add(frac_cents)
            })
            .ok_or(format!("主档公司 {company} 的注册资本法定事实缺少数值"))?;
        let issued = encoded
            .get("company_system")
            .and_then(|system| system.get("issuers"))
            .and_then(|issuers| issuers.get(company))
            .and_then(|issuer| issuer.get("issued_shares"))
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<i128>().ok())
            .ok_or(format!("主档发行人 {company} 缺少 issued_shares"))?;
        if registered_cents != 100 * issued {
            return Err(format!(
                "主档公司 {company} 注册资本 {registered_cents} 分必须等于面值 100 分 × 总股本 {issued}"
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
                            book_display: engine::company::simple::SimpleBookDisplayConfig::DEFAULT,
                        },
                        // N2a（2026-10-08）起 fixture 携带温和默认偏好（与 Web
                        // DEFAULT_SETUP 同值的游戏化虚拟参数）：现金分红启用、
                        // 送转关闭。本场景（两交易日、未到结算周期末日）不触发
                        // 偏好评估，台账保持空。
                        preferences: mild_default_preferences(),
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
        dividend_tax_mode: engine::company::cash_dividend_tax::CashDividendTaxMode::FlatWithholding,
        flat_withholding_bp: Some(1000),
        // 主档保持机制关闭（M 批严格持久化新开关，默认 false）。
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
        par_value_per_share: Money::from_cents(100),
        auto_corporate_foundation: true,
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
