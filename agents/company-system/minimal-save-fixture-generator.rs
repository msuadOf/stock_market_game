use engine::accounting::{
    AccountingAmount, IncomeTaxPolicy, JournalLine, LedgerAccountId, PostingSide, TaxPolicy,
    VatPolicy,
};
use engine::company::config::CompanySystemConfig;
use engine::company::simple::period::{
    AnnualTrendConfig, PeriodGenerationParameters, PeriodNoiseConfig,
    PeriodVariableExpenseRule, SettlementCycle,
};
use engine::company::simple::{
    SimpleCompanyConfig, SimpleConfig, SimpleEnvironmentConfig, SimpleFinanceConfig,
    SimpleSummaryRule,
};
use engine::company::{CompanyId, CompanyKind};
use engine::session::protocol::ProtocolSession;
use engine::{
    CivilDate, GameConfig, HotParams, InstParams, Money, NpcSetup, RetailParams,
    SecurityCategory, SessionSetup, StockCode, StockExchange, StockSpec, StrategyParams,
};
use std::{env, error::Error, fs};

fn zero_noise() -> PeriodNoiseConfig {
    PeriodNoiseConfig {
        monthly_bp: 0,
        quarterly_bp: 0,
        half_year_bp: 0,
        annual_bp: 0,
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let output_path = env::args().nth(1).ok_or("缺少输出路径")?;
    if env::args().nth(2).is_some() {
        return Err("参数过多".into());
    }

    let code = StockCode("600101".into());
    let issuer = CompanyId(format!("C-{}", code.0));
    let setup = SessionSetup {
        stocks: vec![StockSpec {
            code,
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1_120),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.1,
            tick: Money::from_cents(1),
            total_shares: 8_928_571_429,
            float_shares: 169_318_418,
        }],
        company_system: CompanySystemConfig::Simple(SimpleConfig {
            environment: SimpleEnvironmentConfig {
                initial_change_bp: 0,
                persistence_bp: 0,
                noise: zero_noise(),
            },
            companies: vec![SimpleCompanyConfig {
                company: issuer,
                kind: CompanyKind::Industrial,
                generation: PeriodGenerationParameters {
                    initial_revenue: AccountingAmount::from_cents(100_000_000),
                    initial_fixed_expense: AccountingAmount::from_cents(90_000_000),
                    revenue_trend: AnnualTrendConfig::Fixed { annual_growth_bp: 0 },
                    fixed_expense_trend: AnnualTrendConfig::Fixed { annual_growth_bp: 0 },
                    revenue_noise: zero_noise(),
                    fixed_expense_noise: zero_noise(),
                    demand_sensitivity_bp: 0,
                    variable_expense: PeriodVariableExpenseRule::RevenueRatio {
                        ratio_bp: 0,
                        noise: zero_noise(),
                    },
                },
                finance: SimpleFinanceConfig {
                    opening_lines: vec![
                        JournalLine {
                            account: LedgerAccountId("1122".into()),
                            side: PostingSide::Debit,
                            amount: AccountingAmount::from_cents(500_000_000),
                        },
                        JournalLine {
                            account: LedgerAccountId("4001".into()),
                            side: PostingSide::Credit,
                            amount: AccountingAmount::from_cents(500_000_000),
                        },
                    ],
                    tax_policy: TaxPolicy {
                        version: 1,
                        vat: VatPolicy {
                            output_rate_bp: 1_300,
                            input_rate_bp: 1_300,
                            deductible_share_bp: 10_000,
                        },
                        income_tax: IncomeTaxPolicy {
                            rate_bp: 2_500,
                            loss_carryforward_years: 5,
                        },
                    },
                    summary_rule: SimpleSummaryRule::ReceivableRevenuePayableExpenses,
                },
            }],
            prehistory_periods: 24,
            settlement_cycle: SettlementCycle::Monthly,
        }),
        npcs: NpcSetup {
            retail_count: 1,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::from_cents(20_000_000),
        },
        config: GameConfig::new(
            0.00025,
            Money::from_cents(500),
            0.0005,
            0.1,
            0.1,
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
            1.0,
            0.0,
            0.0,
            engine::WithinKindDistribution::Random,
        ),
        report_frequency: engine::information::ReportFrequency::Quarterly,
        start_date: CivilDate::from_iso("2030-01-01")?,
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_string(),
        dividend_tax_mode: engine::company::cash_dividend_tax::CashDividendTaxMode::IndividualPublicMarket,
    };
    setup.validate()?;
    let mut session = ProtocolSession::new(setup, 42)?;
    if !session.civil_day_ready()? {
        return Err("2030-01-01 休市自然日未就绪，不能生成初始日终 fixture".into());
    }
    session.end_civil_day_update()?;
    let save = session.save()?;
    let encoded = serde_json::to_value(&save)?;
    let restored = ProtocolSession::restore(&save)?;
    if serde_json::to_value(restored.save()?)? != encoded {
        return Err("Engine fixture ProtocolSession 恢复后重新保存与原存档不一致".into());
    }
    for (path, actual, expected) in [
        ("setup.stocks[0].code", &encoded["setup"]["stocks"][0]["code"], serde_json::json!("600101")),
        ("seed", &encoded["seed"], serde_json::json!("42")),
        ("civil_clock.current_date", &encoded["civil_clock"]["current_date"], serde_json::json!("2030-01-02")),
        ("civil_clock.settled_through", &encoded["civil_clock"]["settled_through"], serde_json::json!("2030-01-01")),
        ("snapshot.tick", &encoded["snapshot"]["tick"], serde_json::json!(0)),
        ("company_system.implementation.mode", &encoded["company_system"]["implementation"]["mode"], serde_json::json!("Simple")),
    ] {
        if actual != &expected {
            return Err(format!("Engine fixture {path} 与预期不符：actual={actual}, expected={expected}").into());
        }
    }
    for account in ["0", "1"] {
        if encoded["snapshot"]["accounts"][account].is_null() {
            return Err(format!("Engine fixture snapshot.accounts.{account} 缺失：{}", encoded["snapshot"]["accounts"]).into());
        }
    }
    if encoded["market_memberships"]["members"]["local-owner"]["account_id"] != "0" {
        return Err(format!("Engine fixture local-owner 未对应玩家账户 0：{}", encoded["market_memberships"]["members"]).into());
    }
    if encoded["runtime_state"]["strategy_states"]["1"]["ZiNoise"]["retail_style"] != "Dormant" {
        return Err(format!("seed 42 的账户 1 当前真实策略应为 ZiNoise/Dormant：{}", encoded["runtime_state"]["strategy_states"]).into());
    }
    for field in ["company_operations", "groups", "ops_wiring"] {
        if encoded.get(field).is_some() {
            return Err(format!("Engine 生成存档包含旧字段 {field}").into());
        }
    }
    fs::write(output_path, serde_json::to_string(&save)? + "\n")?;
    Ok(())
}
