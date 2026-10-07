//! 公司域公共错误四分类（F 批共同契约收口）映射测试。
//!
//! 四分类是**附加结构**：具体错误信息（display 字符串）保持不变，本文件只锁定
//! [`crate::company::CompanySystemError::classification`] 的分类映射，并用真实
//! 行为路径（create／finance／submit_command）覆盖每类至少两例，防止分类面与
//! 实际调用点漂移。

use crate::calendar::CivilDate;
use crate::company::api::CompanyCommand;
use crate::company::config::CompanySystemConfig;
use crate::company::simple::{
    SimpleConfig, SimpleEnvironmentConfig, SimpleFinanceConfig, SimpleSummaryRule,
    finance::SimpleFinanceError,
    period::{
        AnnualTrendConfig, PeriodGenerationParameters, PeriodNoiseConfig,
        PeriodVariableExpenseRule, SettlementCycle,
    },
    preferences::SimpleCompanyPreferences,
};
use crate::company::spec::{CompanyId, CompanyKind, CompanySpec, IndustryId};
use crate::company::{CompanyErrorClass, CompanySystem, CompanySystemError};
use crate::account::StockCode;
use crate::accounting::{
    AccountingAmount, AccountingError, IncomeTaxPolicy, JournalLine, LedgerAccountId,
    PostingSide, TaxPolicy, VatPolicy,
};

fn date(text: &str) -> CivilDate {
    CivilDate::from_iso(text).unwrap()
}

fn amount(cents: i128) -> AccountingAmount {
    AccountingAmount::from_cents(cents)
}

fn spec() -> CompanySpec {
    CompanySpec {
        id: CompanyId("A".into()),
        name: "虚拟公司".into(),
        industry: IndustryId("虚拟行业".into()),
        kind: CompanyKind::Industrial,
        listed_stock: Some(StockCode("600101".into())),
        issued_shares: 1000,
        group_parent: None,
    }
}

fn zero_noise() -> PeriodNoiseConfig {
    PeriodNoiseConfig {
        monthly_bp: 0,
        quarterly_bp: 0,
        half_year_bp: 0,
        annual_bp: 0,
    }
}

fn config() -> SimpleConfig {
    SimpleConfig {
        environment: SimpleEnvironmentConfig {
            initial_change_bp: 0,
            persistence_bp: 0,
            noise: zero_noise(),
        },
        companies: vec![crate::company::simple::SimpleCompanyConfig {
            company: spec().id,
            kind: CompanyKind::Industrial,
            generation: PeriodGenerationParameters {
                initial_revenue: amount(100_000),
                initial_fixed_expense: amount(110_000),
                revenue_trend: AnnualTrendConfig::Fixed {
                    annual_growth_bp: 1268,
                },
                demand_sensitivity_bp: 0,
                revenue_noise: zero_noise(),
                fixed_expense_trend: AnnualTrendConfig::Fixed {
                    annual_growth_bp: 0,
                },
                fixed_expense_noise: zero_noise(),
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
                        amount: amount(500_000),
                    },
                    JournalLine {
                        account: LedgerAccountId("4001".into()),
                        side: PostingSide::Credit,
                        amount: amount(500_000),
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
                book_display: crate::company::simple::SimpleBookDisplayConfig::DEFAULT,
            },
            preferences: SimpleCompanyPreferences::none(),
        }],
        prehistory_periods: 2,
        settlement_cycle: SettlementCycle::Monthly,
    }
}

fn create_system() -> CompanySystem {
    CompanySystem::create(
        vec![spec()],
        CompanySystemConfig::Simple(config()),
        date("2030-01-01"),
        19,
    )
    .unwrap()
}

#[test]
fn unknown_company_query_maps_to_invalid_input() {
    let system = create_system();
    let error = system
        .distributable_profit(&CompanyId("不存在".into()))
        .unwrap_err();
    assert_eq!(error.classification(), CompanyErrorClass::InvalidInput);
    // 分类是附加结构：具体错误信息保持原样。
    assert_eq!(error.to_string(), "公司系统输入非法：未知公司 不存在");
}

#[test]
fn invalid_civil_date_maps_to_invalid_input() {
    let error = CompanySystemError::from(crate::calendar::CivilDate::from_ymd(2029, 2, 30).unwrap_err());
    assert_eq!(error.classification(), CompanyErrorClass::InvalidInput);
}

#[test]
fn restart_revenue_with_blank_source_maps_to_invalid_input() {
    let mut system = create_system();
    let error = system
        .submit_command(CompanyCommand::RestartRevenue {
            company: spec().id,
            revenue: amount(1_000),
            source: "  ".into(),
        })
        .unwrap_err();
    assert_eq!(error.classification(), CompanyErrorClass::InvalidInput);
    assert_eq!(
        error.to_string(),
        "公司系统输入非法：复业需要正收入与明确场景说明"
    );
}

#[test]
fn restart_revenue_on_active_company_maps_to_business_condition() {
    let mut system = create_system();
    // 该公司当期收入为正（初始前史已生成），业务条件不满足复业。
    let error = system
        .submit_command(CompanyCommand::RestartRevenue {
            company: spec().id,
            revenue: amount(1_000),
            source: "复业场景".into(),
        })
        .unwrap_err();
    assert_eq!(error.classification(), CompanyErrorClass::BusinessCondition);
    assert_eq!(
        error.to_string(),
        "公司系统输入非法：仅零收入且没有待执行复业的公司可以复业"
    );
}

#[test]
fn wrapped_plan_identity_conflict_maps_to_business_condition() {
    let error = CompanySystemError::from(SimpleFinanceError::DividendPlanConflict(
        "plan-1".into(),
    ));
    assert_eq!(error.classification(), CompanyErrorClass::BusinessCondition);
}

#[test]
fn simulation_creation_maps_to_unsupported_operation() {
    let error = CompanySystem::create(
        vec![spec()],
        CompanySystemConfig::Simulation,
        date("2030-01-01"),
        19,
    )
    .unwrap_err();
    assert_eq!(
        error.classification(),
        CompanyErrorClass::UnsupportedOperation
    );
    assert_eq!(
        error.to_string(),
        "公司系统尚不支持：Simulation 在独立分支实现；当前不能创建"
    );
}

#[test]
fn wrapped_dividend_unsupported_maps_to_unsupported_operation() {
    let error = CompanySystemError::from(SimpleFinanceError::DividendUnsupported(
        "缺少显式绑定的公司法定注册资本来源事实".into(),
    ));
    assert_eq!(
        error.classification(),
        CompanyErrorClass::UnsupportedOperation
    );
}

#[test]
fn accounting_overflow_maps_to_system_state() {
    let error = CompanySystemError::from(AccountingError::AmountOverflow {
        op: "add",
        detail: "i128 边界".into(),
    });
    assert_eq!(error.classification(), CompanyErrorClass::SystemState);
}

#[test]
fn explicit_system_state_variant_maps_to_system_state() {
    let error = CompanySystemError::SystemState("恢复校验发现账簿与声明事实不一致".into());
    assert_eq!(error.classification(), CompanyErrorClass::SystemState);
}
