macro_rules! simple_company_fixture {
    ($engine:ident; [$($code:expr),+ $(,)?]) => {{
        simple_company_fixture!($engine; codes = [$($code),+])
    }};
    ($engine:ident; codes = $codes:expr) => {{
        use $engine as fixture_engine;
        use fixture_engine::accounting::AccountingAmount;
        use fixture_engine::company::config::CompanySystemConfig;
        use fixture_engine::company::simple::{
            period::{
                AnnualTrendConfig, PeriodGenerationParameters, PeriodNoiseConfig,
                PeriodVariableExpenseRule, SettlementCycle,
            },
            SimpleCompanyConfig, SimpleConfig, SimpleEnvironmentConfig, SimpleFinanceConfig,
            SimpleSummaryRule,
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
            companies: ($codes).into_iter().map(|code| SimpleCompanyConfig {
                company: CompanyId(format!("C-{}", code)),
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
                },
                preferences: fixture_engine::company::simple::SimpleCompanyPreferences::none(),
            }).collect(),
            prehistory_periods: 24,
            settlement_cycle: SettlementCycle::Monthly,
        })
    }};
}
