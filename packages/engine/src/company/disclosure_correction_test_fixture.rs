//! Simple 更正短测的真实公司装配与正式 owner fixture。
use crate::calendar::{CivilDate, CivilInstant};
use crate::company::{CompanyId, CompanySystem};
use crate::information::{
    MonthlyReportDelay, MonthlyReportSchedule, PublicLibrary, PublicationId, ReportFrequency,
};

pub(crate) fn simple_model(prehistory_periods: u16) -> crate::company::CompanySystem {
    simple_model_with_cycle(
        prehistory_periods,
        crate::company::simple::period::SettlementCycle::Monthly,
    )
}

pub(crate) fn simple_model_with_cycle(
    prehistory_periods: u16,
    settlement_cycle: crate::company::simple::period::SettlementCycle,
) -> crate::company::CompanySystem {
    use crate::accounting::{
        AccountingAmount, IncomeTaxPolicy, JournalLine, LedgerAccountId, PostingSide, TaxPolicy,
        VatPolicy,
    };
    use crate::company::config::CompanySystemConfig;
    use crate::company::simple::finance::{
        SimpleBookDisplayConfig, SimpleFinanceConfig, SimpleSummaryRule,
    };
    use crate::company::simple::period::{
        AnnualTrendConfig, PeriodGenerationParameters, PeriodNoiseConfig,
        PeriodVariableExpenseRule, SettlementCycle,
    };
    use crate::company::simple::{SimpleCompanyConfig, SimpleConfig, SimpleEnvironmentConfig};
    use crate::company::{CompanyKind, CompanySpec, CompanySystem, IndustryId};
    let noise = PeriodNoiseConfig {
        monthly_bp: 0,
        quarterly_bp: 0,
        half_year_bp: 0,
        annual_bp: 0,
    };
    // spec code "empty-simple" 在单公司集合内分配数值 id 0。
    let company = CompanyId(0);
    let start = CivilDate::from_iso("2030-01-01").unwrap();
    let system = CompanySystem::create(
        vec![CompanySpec {
            code: "empty-simple".into(),
            name: "空历史简单公司".into(),
            industry: IndustryId("虚拟行业".into()),
            kind: CompanyKind::Bank,
            listed_stock: Some(crate::account::StockCode("600000".into())),
            issued_shares: 1000,
            group_parent: None,
        }],
        CompanySystemConfig::Simple(SimpleConfig {
            environment: SimpleEnvironmentConfig {
                initial_change_bp: 0,
                persistence_bp: 0,
                noise: noise.clone(),
            },
            prehistory_periods,
            settlement_cycle,
            companies: vec![SimpleCompanyConfig {
                company,
                kind: CompanyKind::Bank,
                generation: PeriodGenerationParameters {
                    initial_revenue: AccountingAmount::from_cents(1000),
                    initial_fixed_expense: AccountingAmount::from_cents(500),
                    revenue_trend: AnnualTrendConfig::Fixed {
                        annual_growth_bp: 0,
                    },
                    demand_sensitivity_bp: 0,
                    revenue_noise: noise.clone(),
                    fixed_expense_trend: AnnualTrendConfig::Fixed {
                        annual_growth_bp: 0,
                    },
                    fixed_expense_noise: noise.clone(),
                    variable_expense: PeriodVariableExpenseRule::RevenueRatio {
                        ratio_bp: 0,
                        noise,
                    },
                },
                finance: SimpleFinanceConfig {
                    opening_lines: vec![
                        JournalLine {
                            account: LedgerAccountId("1003".into()),
                            side: PostingSide::Debit,
                            amount: AccountingAmount::from_cents(100000),
                        },
                        JournalLine {
                            account: LedgerAccountId("4001".into()),
                            side: PostingSide::Credit,
                            amount: AccountingAmount::from_cents(100000),
                        },
                    ],
                    tax_policy: TaxPolicy {
                        version: 1,
                        vat: VatPolicy {
                            output_rate_bp: 0,
                            input_rate_bp: 0,
                            deductible_share_bp: 10000,
                        },
                        income_tax: IncomeTaxPolicy {
                            rate_bp: 2500,
                            loss_carryforward_years: 5,
                        },
                    },
                    summary_rule: SimpleSummaryRule::ReceivableRevenuePayableExpenses,
                    book_display: SimpleBookDisplayConfig::DEFAULT,
                },
                preferences: crate::company::simple::SimpleCompanyPreferences::none(),
            }],
        }),
        start,
        17,
    )
    .unwrap();
    system
}

pub(crate) fn simple_models(prehistory_periods: u16, count: u32) -> CompanySystem {
    let template = simple_model(prehistory_periods);
    let crate::company::config::CompanySystemConfig::Simple(mut config) = template.config() else {
        panic!("fixture 必须来自真实 Simple 配置");
    };
    let original_config = config.companies[0].clone();
    let original_spec = template.issuers().get(&CompanyId(0)).unwrap().clone();
    let mut specs = Vec::new();
    config.companies.clear();
    for index in 0..count {
        let mut spec = original_spec.clone();
        spec.code = format!("simple-batch-{index}");
        spec.name = format!("批量更正公司{index}");
        spec.listed_stock = Some(crate::account::StockCode(format!("{:06}", 600000 + index)));
        specs.push(spec);
        let mut company_config = original_config.clone();
        company_config.company = CompanyId(index);
        config.companies.push(company_config);
    }
    CompanySystem::create(
        specs,
        crate::company::config::CompanySystemConfig::Simple(config),
        CivilDate::from_iso("2030-01-01").unwrap(),
        17,
    )
    .unwrap()
}

pub(crate) fn correction_operation(
    source: u64,
    date: CivilDate,
) -> crate::accounting::closing::CorrectionRequest {
    use crate::accounting::{
        AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, JournalLine,
        LedgerAccountId, PostingSide,
    };
    crate::accounting::closing::CorrectionRequest {
        reason: "纠正历史汇总营收".into(),
        entries: vec![JournalEntry {
            source: BusinessEventId::new(source),
            date,
            kind: BusinessKind::SimpleRevenueAccrual,
            cash_flow: CashFlowClass::NonCash,
            lines: vec![
                JournalLine {
                    account: LedgerAccountId("simple_receivable".into()),
                    side: PostingSide::Debit,
                    amount: AccountingAmount::from_cents(100),
                },
                JournalLine {
                    account: LedgerAccountId("simple_revenue".into()),
                    side: PostingSide::Credit,
                    amount: AccountingAmount::from_cents(100),
                },
            ],
        }],
    }
}

pub(crate) fn owner_result() -> (PublicLibrary, PublicLibrary, Vec<PublicationId>) {
    let mut system = simple_model(1);
    let date = CivilDate::from_iso("2030-01-01").unwrap();
    let frequency = ReportFrequency::Monthly {
        schedule: MonthlyReportSchedule::Custom {
            day: 1,
            second_of_day: 18 * 3600,
            delay: MonthlyReportDelay::None,
        },
    };
    let mut base = PublicLibrary::new();
    let original = crate::information::publish_simple_scheduled(
        &system,
        frequency,
        17,
        None,
        CivilInstant::from_hms(date, 18, 0, 0).unwrap(),
        &mut base,
        false,
    )
    .unwrap()[0];
    let mut private_library = base.clone();
    let next = date.next().unwrap();
    let publications = system
        .correct_report(
            &CompanyId(0),
            correction_operation(1000, next),
            original,
            CivilInstant::from_hms(next, 18, 0, 0).unwrap(),
            &mut private_library,
        )
        .unwrap();
    (base, private_library, publications)
}

use crate::accounting::closing::ClosingEngine;
use crate::accounting::consolidation::{MemberId, ScopeId};
use crate::accounting::reports::{ReportKind, ReportVersion, VersionKind};
use crate::accounting::AccountingPeriod;
use crate::company::operations::CompanyOperations;
pub(crate) fn legacy_fixture() -> (CompanyOperations, ClosingEngine) {
    use crate::accounting::consolidation::{ConsolidationRequest, GroupMember, MemberSpec};
    use crate::accounting::reports::{
        generate_report_set, IndustryPresentation, ReportRequest, ReportSource,
    };
    use crate::accounting::{
        AccountingAmount as A, IncomeTaxPolicy, InventoryItemCode, JournalLine, LedgerAccountId,
        PostingSide, TaxPolicy, VatPolicy,
    };
    use crate::company::events::ShockParams;
    use crate::company::industrial::{industrial_account_chart, IndustrialBooks, IndustrialConfig};
    use crate::company::operations::{
        CompanyOperationsConfig, FlowParams, IndustrialFlowParams, IndustryBooks,
        OperatingCompanyConfig,
    };
    use crate::company::{CompanyKind, CompanySpec, CounterpartyId, IndustryId, OperatingBudget};
    use std::collections::BTreeMap;
    let date = CivilDate::from_iso("2029-12-01").unwrap();
    let mut companies = Vec::new();
    for code in ["parent", "subsidiary"] {
        companies.push(OperatingCompanyConfig {
            spec: CompanySpec {
                code: code.into(),
                name: code.into(),
                industry: IndustryId("虚拟行业".into()),
                kind: CompanyKind::Industrial,
                listed_stock: None,
                issued_shares: 100,
                group_parent: None,
            },
            books: IndustryBooks::Industrial(
                IndustrialBooks::new(IndustrialConfig {
                    chart: industrial_account_chart(),
                    as_of: date,
                    opening_lines: vec![
                        JournalLine {
                            account: LedgerAccountId("1002".into()),
                            side: PostingSide::Debit,
                            amount: A::from_cents(10000),
                        },
                        JournalLine {
                            account: LedgerAccountId("4001".into()),
                            side: PostingSide::Credit,
                            amount: A::from_cents(10000),
                        },
                    ],
                    opening_inventory: vec![],
                    opening_assets: vec![],
                    opening_debt: None,
                    counterparties: vec![],
                    budget: OperatingBudget::new(A::ZERO, vec![]).unwrap(),
                    tax_policy: TaxPolicy {
                        version: 1,
                        vat: VatPolicy {
                            output_rate_bp: 0,
                            input_rate_bp: 0,
                            deductible_share_bp: 10000,
                        },
                        income_tax: IncomeTaxPolicy {
                            rate_bp: 2500,
                            loss_carryforward_years: 5,
                        },
                    },
                })
                .unwrap(),
            ),
            flow: FlowParams::Industrial(IndustrialFlowParams {
                customer: CounterpartyId("客户".into()),
                supplier: CounterpartyId("供应商".into()),
                raw_item: InventoryItemCode("原料".into()),
                finished_item: InventoryItemCode("成品".into()),
                raw_account: LedgerAccountId("1403".into()),
                finished_account: LedgerAccountId("1405".into()),
                base_daily_demand_units: 0,
                unit_price_excl_vat: A::from_cents(1),
                receivable_credit_days: 1,
                raw_replenish_target_units: 0,
                raw_unit_cost_excl_vat: A::from_cents(1),
                daily_production_units: 0,
                daily_conversion_cost: A::ZERO,
                daily_admin_expense: A::ZERO,
                bad_debt_base_bp: 0,
                asset_impairment_fraction_bp: 0,
            }),
        });
    }
    let operations = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 17,
            catalog: crate::company::CompanyCatalog::from_codes(&["parent", "subsidiary"]).unwrap(),
            companies,
            shock_params: ShockParams {
                version: 1,
                market_candidate_bp: 0,
                industry_candidate_bp: 0,
                company_candidate_bp: 0,
                duration_min_days: 1,
                duration_max_days: 1,
                market_demand_band_bp: 1,
                industry_cost_band_bp: 1,
                company_demand_band_bp: 1,
                credit_deterioration_add_bp: 0,
            },
        },
        date.next().unwrap(),
    )
    .unwrap();
    let parent = operations.company(&CompanyId(0)).unwrap().books().books();
    let subsidiary = operations.company(&CompanyId(1)).unwrap().books().books();
    let period = AccountingPeriod::from_ymd(2029, 12).unwrap();
    let version = ReportVersion {
        sequence: 1,
        supersedes: None,
        kind: VersionKind::Original,
    };
    let mut closing = ClosingEngine::new();
    closing
        .record(
            generate_report_set(ReportRequest {
                period,
                kind: ReportKind::Monthly,
                source: ReportSource::Standalone {
                    id: MemberId(0),
                    books: parent,
                    industry: IndustryPresentation::Industrial,
                },
                version: version.clone(),
                adjustments: &BTreeMap::new(),
            })
            .unwrap(),
        )
        .unwrap();
    closing
        .record(
            generate_report_set(ReportRequest {
                period,
                kind: ReportKind::Monthly,
                source: ReportSource::Consolidated {
                    request: ConsolidationRequest {
                        root: MemberId(0),
                        members: vec![
                            GroupMember {
                                spec: MemberSpec {
                                    id: MemberId(0),
                                    group_parent: None,
                                    issued_shares: 100,
                                    parent_held_shares: 0,
                                },
                                books: parent,
                            },
                            GroupMember {
                                spec: MemberSpec {
                                    id: MemberId(1),
                                    group_parent: Some(MemberId(0)),
                                    issued_shares: 100,
                                    parent_held_shares: 60,
                                },
                                books: subsidiary,
                            },
                        ],
                        intercompany_balances: vec![],
                        intercompany_sales: vec![],
                    },
                },
                version,
                adjustments: &BTreeMap::new(),
            })
            .unwrap(),
        )
        .unwrap();
    (operations, closing)
}
