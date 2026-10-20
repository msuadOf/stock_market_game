// 集成测试共用夹具：极简 Simple 公司系统配置（B4b 恢复，基 18725d07 的
// `test-support/simple_company.rs` 数值化适配）。
//
// 适配点（std-thread 数值身份）：`CompanyId` 为数值 u32，按代码升序分配
// （`IssuerRegistry` 同款约定）——`codes` 按传入顺序即升序约定使用，
// 第 i 个代码的公司 id = i。其余参数（零扰动环境/固定期初/13%-25% 税制/
// 月度结算周期/24 期前史）保持 main 侧终态语义。
//
// 使用方式（集成测试 `include!`）：
// ```ignore
// include!("../test-support/simple_company.rs");
// let config = simple_company_fixture!(engine; ["600101"]);
// ```

/// 构造全部公司为 Industrial 的极简 [`CompanySystemConfig`]。
macro_rules! simple_company_fixture {
    ($engine:ident; [$($code:expr),+ $(,)?]) => {{
        use $engine as fixture_engine;
        use fixture_engine::accounting::AccountingAmount;
        use fixture_engine::company::config::CompanySystemConfig;
        use fixture_engine::company::simple::{
            period::{AnnualTrendConfig, PeriodGenerationParameters, PeriodNoiseConfig,
                     PeriodVariableExpenseRule, SettlementCycle},
            SimpleBookDisplayConfig, SimpleCompanyConfig, SimpleConfig, SimpleEnvironmentConfig,
            SimpleFinanceConfig, SimpleSummaryRule,
        };
        use fixture_engine::accounting::{
            IncomeTaxPolicy, JournalLine, LedgerAccountId, PostingSide, TaxPolicy, VatPolicy,
        };
        use fixture_engine::company::CompanyId;

        CompanySystemConfig::Simple(SimpleConfig {
            environment: SimpleEnvironmentConfig {
                initial_change_bp: 0,
                persistence_bp: 0,
                noise: PeriodNoiseConfig {
                    monthly_bp: 0,
                    quarterly_bp: 0,
                    half_year_bp: 0,
                    annual_bp: 0,
                },
            },
            companies: [$($code),+].into_iter().enumerate().map(|(index, code)| {
                let _ = code; // 代码不进配置：数值 id 按升序约定（CompanyId(index)）。
                SimpleCompanyConfig {
                    company: CompanyId(index as u32),
                    kind: fixture_engine::company::CompanyKind::Industrial,
                    generation: PeriodGenerationParameters {
                        initial_revenue: AccountingAmount::from_cents(100_000_000),
                        initial_fixed_expense: AccountingAmount::from_cents(90_000_000),
                        revenue_trend: AnnualTrendConfig::Fixed {
                            annual_growth_bp: 0,
                        },
                        fixed_expense_trend: AnnualTrendConfig::Fixed {
                            annual_growth_bp: 0,
                        },
                        revenue_noise: PeriodNoiseConfig {
                            monthly_bp: 0,
                            quarterly_bp: 0,
                            half_year_bp: 0,
                            annual_bp: 0,
                        },
                        fixed_expense_noise: PeriodNoiseConfig {
                            monthly_bp: 0,
                            quarterly_bp: 0,
                            half_year_bp: 0,
                            annual_bp: 0,
                        },
                        demand_sensitivity_bp: 0,
                        variable_expense: PeriodVariableExpenseRule::RevenueRatio {
                            ratio_bp: 0,
                            noise: PeriodNoiseConfig {
                                monthly_bp: 0,
                                quarterly_bp: 0,
                                half_year_bp: 0,
                                annual_bp: 0,
                            },
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
                                output_rate_bp: 1300,
                                input_rate_bp: 1300,
                                deductible_share_bp: 10_000,
                            },
                            income_tax: IncomeTaxPolicy {
                                rate_bp: 2500,
                                loss_carryforward_years: 5,
                            },
                        },
                        summary_rule: SimpleSummaryRule::ReceivableRevenuePayableExpenses,
                        book_display: SimpleBookDisplayConfig::DEFAULT,
                    },
                    preferences: fixture_engine::company::simple::SimpleCompanyPreferences::none(),
                }
            }).collect(),
            prehistory_periods: 24,
            settlement_cycle: SettlementCycle::Monthly,
        })
    }};
}
