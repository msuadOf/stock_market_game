//! 公司共同契约能力面与解释查询（F 批收口）测试。
//!
//! R4 最小公共合同：能力面携带**当前事实**（面值／股本／可分配利润快照／
//! 法定事实），不可用显式给 reason 不填零；解释查询按公司+期间读取既有
//! `history` 中的 [`PeriodChangeExplanation`]，不新建状态。

use crate::account::StockCode;
use crate::accounting::{
    AccountingAmount, IncomeTaxPolicy, JournalLine, LedgerAccountId, PostingSide, TaxPolicy,
    VatPolicy,
};
use crate::calendar::CivilDate;
use crate::company::capabilities::{
    CapabilityAmount, CapabilityMoney, CompanyFacts, DistributableProfitSnapshot,
};
use crate::company::config::CompanySystemConfig;
use crate::company::simple::period::{
    AnnualTrendConfig, PeriodGenerationParameters, PeriodNoiseConfig, PeriodVariableExpenseRule,
    SettlementCycle,
};
use crate::company::simple::preferences::SimpleCompanyPreferences;
use crate::company::simple::{
    SimpleCompanyConfig, SimpleConfig, SimpleEnvironmentConfig, SimpleFinanceConfig,
    SimpleSummaryRule,
};
use crate::company::spec::{CompanyId, CompanyKind, CompanySpec, IndustryId};
use crate::company::{CompanyErrorClass, CompanySystem, CompanySystemError};

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
        companies: vec![SimpleCompanyConfig {
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

fn company() -> CompanyId {
    spec().id
}

#[test]
fn company_facts_report_static_booleans_and_current_shares() {
    let system = create_system();
    let facts: CompanyFacts = system.company_facts(&company()).unwrap();
    assert!(
        facts.revenue
            && facts.net_income
            && facts.equity
            && facts.cash_flow
            && facts.full_financial_statements
    );
    assert!(!facts.cash_settlement);
    assert!(!facts.unsupported_reason.trim().is_empty());
    assert_eq!(facts.issued_shares, 1000);
}

#[test]
fn company_facts_without_legal_facts_report_explicit_reasons_not_zero() {
    let system = create_system();
    let facts = system.company_facts(&company()).unwrap();
    // 无送转／拆股绑定事实：面值不可用，显式 reason，不填零。
    match &facts.par_value_per_share {
        CapabilityMoney::Unavailable { reason } => assert!(!reason.trim().is_empty()),
        CapabilityMoney::Available { .. } => panic!("未绑定面值事实时不得冒充可用"),
    }
    match &facts.registered_capital {
        CapabilityAmount::Unavailable { reason } => assert!(!reason.trim().is_empty()),
        CapabilityAmount::Available { .. } => panic!("未绑定法定事实时不得冒充可用"),
    }
    match &facts.distributable_profit {
        DistributableProfitSnapshot::Unavailable { reason } => {
            assert!(!reason.trim().is_empty())
        }
        DistributableProfitSnapshot::Available { .. } => {
            panic!("未绑定法定事实时不得冒充可分配利润快照")
        }
    }
}

#[test]
fn company_facts_after_legal_facts_report_yuan_strings() {
    let mut system = create_system();
    system
        .define_dividend_legal_facts(&company(), amount(1_000_000), "股东会决议第 1 号".into())
        .unwrap();
    let facts = system.company_facts(&company()).unwrap();
    match &facts.registered_capital {
        CapabilityAmount::Available { amount_yuan } => {
            assert_eq!(amount_yuan, "10000.00");
        }
        CapabilityAmount::Unavailable { reason } => {
            panic!("已绑定法定事实仍不可用：{reason}")
        }
    }
    match &facts.distributable_profit {
        DistributableProfitSnapshot::Available {
            available_for_distribution_yuan,
            ..
        } => {
            // 可分配利润快照必须是非负十进制元字符串，与公开报表同口径。
            assert!(
                !available_for_distribution_yuan.starts_with('-'),
                "可分配利润不得为负：{available_for_distribution_yuan}"
            );
        }
        DistributableProfitSnapshot::Unavailable { reason } => {
            panic!("已绑定法定事实仍不可用：{reason}")
        }
    }
}

#[test]
fn company_facts_unknown_company_is_invalid_input() {
    let system = create_system();
    let error = system
        .company_facts(&CompanyId("不存在".into()))
        .unwrap_err();
    assert_eq!(error.classification(), CompanyErrorClass::InvalidInput);
}

/// F 修复轮 low-5：能力面的每股面值必须按**已入账**（`settled_on` 回填）的
/// 重新计值口径（`outstanding_par_value`）报告，保证快照自洽
/// （par × issued_shares 与 registered_capital 同口径）；声明链
/// （`current_par_value`）含未生效重锚——已批准未入账的拆股会把面值提前
/// 翻新，造成批未入账时 par×issued≠registered_capital（曾用声明链，反向漂移）。
#[test]
fn company_facts_par_value_uses_booked_outstanding_anchor_not_declared_chain() {
    let mut system = create_system();
    system
        .define_dividend_legal_facts(&company(), amount(1_000_000), "股东会决议第 1 号".into())
        .unwrap();
    // 前置：无任何送转／拆股事实，两条面值链都无锚。
    match system.company_facts(&company()).unwrap().par_value_per_share {
        CapabilityMoney::Unavailable { .. } => {}
        CapabilityMoney::Available { .. } => panic!("无面值事实时不得冒充可用"),
    }
    // 声明一起 1 拆 2（面值 1000 分 → 500 分；未入账 settled_on=None）。
    system
        .declare_share_split(
            &company(),
            crate::company::share_split::ShareSplitDeclaration {
                event_id: "split-1".into(),
                approval_reference: "board-split-1".into(),
                direction: crate::company::share_split::ShareSplitDirection::Split,
                approved_on: date("2030-02-01"),
                ratio: 2,
                par_value_before: crate::money::Money::from_cents(1_000),
                par_value_after: crate::money::Money::from_cents(500),
                registered_capital_at_approval: amount(1_000_000),
            },
        )
        .unwrap();
    // 声明链已重锚到 500 分（两条口径确实分叉的证据）。
    assert_eq!(
        system
            .current_par_value(&company())
            .unwrap()
            .expect("声明链必须已有锚")
            .cents(),
        500
    );
    // 能力面：在册口径无已入账重锚 → 显式不可用（issued 仍 1000 股、注册资本
    // 未变，报 500 分会造成 par×issued≠registered_capital）。
    match system.company_facts(&company()).unwrap().par_value_per_share {
        CapabilityMoney::Unavailable { reason } => {
            assert!(!reason.trim().is_empty(), "不可用必须显式给原因");
        }
        CapabilityMoney::Available { cents } => {
            panic!("批未入账时不得提前采用重锚面值：{cents} 分")
        }
    }
}

#[test]
fn period_explanation_reads_existing_history_entry() {
    let system = create_system();
    let explanation = system
        .period_change_explanation(&company(), date("2029-12-31"))
        .unwrap();
    assert_eq!(explanation.cycle, SettlementCycle::Monthly);
    assert_eq!(explanation.environment_change_bp, 0);
    assert_eq!(explanation.revenue_noise_bp, 0);
    // 前史最后一个期间（12 月）的解释 previous 必须等于 11 月生成额
    // （首个前史期间已应用年化趋势），复用既有 history 数据，不新建状态。
    assert_eq!(
        explanation.previous,
        crate::company::api::PeriodAmounts {
            revenue: amount(101_000),
            fixed_expense: amount(110_000),
            variable_expense: amount(0),
        }
    );
    assert!(explanation.restart_revenue.is_none());
}

#[test]
fn period_explanation_rejects_non_cycle_end_date() {
    let system = create_system();
    let error = system
        .period_change_explanation(&company(), date("2029-12-15"))
        .unwrap_err();
    assert_eq!(error.classification(), CompanyErrorClass::InvalidInput);
}

#[test]
fn period_explanation_rejects_unsettled_future_period() {
    let system = create_system();
    let error = system
        .period_change_explanation(&company(), date("2030-01-31"))
        .unwrap_err();
    assert_eq!(error.classification(), CompanyErrorClass::BusinessCondition);
}

#[test]
fn period_explanation_rejects_period_before_prehistory() {
    let system = create_system();
    let error = system
        .period_change_explanation(&company(), date("2029-10-31"))
        .unwrap_err();
    assert_eq!(error.classification(), CompanyErrorClass::BusinessCondition);
}

#[test]
fn period_explanation_unknown_company_is_invalid_input() {
    let system = create_system();
    let error = system
        .period_change_explanation(&CompanyId("不存在".into()), date("2029-12-31"))
        .unwrap_err();
    assert_eq!(error.classification(), CompanyErrorClass::InvalidInput);
}
