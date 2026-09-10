//! 测试公司装配（四行业各一 + 资金断裂变体）。全部为虚构实体（未上市、
//! 行业标签来自 `CompanySpec.industry`——刻意不读证券类别）。

use engine::accounting::{
    AccountingAmount, InventoryItemCode, JournalLine, LedgerAccountId, PostingSide, TaxPolicy,
};
use engine::calendar::CivilDate;
use engine::company::bank::{bank_chart_v3, BankConfig, EclPolicy, EclScenario};
use engine::company::events::ShockParams;
use engine::company::industrial::{
    industrial_chart_v2, IndustrialConfig, OpeningAssetItem, OpeningDebtTerms, OpeningInventoryItem,
};
use engine::company::insurance::{insurance_chart_v4, DiscountAssumption, InsuranceConfig};
use engine::company::operations::{
    BankFlowParams, CompanyOperationsConfig, FlowParams, IndustrialFlowParams, InsuranceFlowParams,
    OperatingCompanyConfig, RealEstateFlowParams,
};
use engine::company::real_estate::{
    real_estate_chart_v5, CapitalizationPolicy, ProjectId, RealEstateConfig,
};
use engine::company::{
    CompanyId, CompanyKind, CompanySpec, CounterpartyId, CounterpartyKind, CreditLine,
    ExternalCounterparty, IndustryId, OperatingBudget,
};

pub(crate) const START: &str = "2030-01-01";

pub(crate) fn d(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("test fixture date must be valid ISO date")
}

/// 元 → AccountingAmount（分）。仅测试夹具用；执行值恒为分。
pub(crate) fn yuan(yuan_value: i128) -> AccountingAmount {
    AccountingAmount::from_cents(yuan_value.checked_mul(100).expect("fixture yuan overflow"))
}

pub(crate) fn amt(cents: i128) -> AccountingAmount {
    AccountingAmount::from_cents(cents)
}

pub(crate) fn cent_line(code: &str, side: PostingSide, cents: i128) -> JournalLine {
    JournalLine {
        account: LedgerAccountId(code.to_string()),
        side,
        amount: AccountingAmount::from_cents(cents),
    }
}

/// Fixture 合成税务政策（与 industrial 套件一致：13%/13% 全抵扣、25%）。
pub(crate) fn fixture_policy() -> TaxPolicy {
    TaxPolicy {
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
    }
}

fn cp(id: &str, kind: CounterpartyKind) -> ExternalCounterparty {
    ExternalCounterparty {
        id: CounterpartyId(id.to_string()),
        kind,
        name: format!("虚构对手方{id}"),
    }
}

fn spec(id: &str, industry: &str, kind: CompanyKind) -> CompanySpec {
    CompanySpec {
        id: CompanyId(id.to_string()),
        name: format!("虚构公司{id}"),
        industry: IndustryId(industry.to_string()),
        kind,
        listed_stock: None,
        issued_shares: 100_000,
        group_parent: None,
    }
}

/// 工商 A（家电行业）：开局现金/存货/固定资产/开局借款齐备（利息与减值有真实驱动面）。
pub(crate) fn industrial_a(as_of: CivilDate) -> OperatingCompanyConfig {
    let config = IndustrialConfig {
        chart: industrial_chart_v2(),
        as_of,
        opening_lines: vec![
            cent_line("1002", PostingSide::Debit, 2_200_000),
            cent_line("1403", PostingSide::Debit, 800_000),
            cent_line("1405", PostingSide::Debit, 300_000),
            cent_line("1601", PostingSide::Debit, 1_000_000),
            cent_line("2001", PostingSide::Credit, 200_000),
            cent_line("4001", PostingSide::Credit, 4_100_000),
        ],
        opening_inventory: vec![
            OpeningInventoryItem {
                account: LedgerAccountId("1403".to_string()),
                item: InventoryItemCode("RAW-1".to_string()),
                quantity: 200,
                cost: amt(800_000),
            },
            OpeningInventoryItem {
                account: LedgerAccountId("1405".to_string()),
                item: InventoryItemCode("FG-1".to_string()),
                quantity: 50,
                cost: amt(300_000),
            },
        ],
        opening_assets: vec![OpeningAssetItem {
            code: engine::accounting::FixedAssetCode("FA-1".to_string()),
            cost: amt(1_000_000),
            salvage_value: AccountingAmount::ZERO,
            life_months: 120,
        }],
        opening_debt: Some(OpeningDebtTerms {
            lender: CounterpartyId("EXT-BANK".to_string()),
            principal: amt(200_000),
            annual_rate_bp: 365,
            maturity_date: d("2030-06-30"),
        }),
        counterparties: vec![
            cp("EXT-SUPP", CounterpartyKind::Supplier),
            cp("EXT-CUST", CounterpartyKind::Customer),
            cp("EXT-BANK", CounterpartyKind::Lender),
        ],
        budget: OperatingBudget::new(
            AccountingAmount::ZERO,
            vec![CreditLine {
                lender: CounterpartyId("EXT-BANK".to_string()),
                limit: yuan(20_000),
            }],
        )
        .expect("fixture budget valid"),
        tax_policy: fixture_policy(),
    };
    OperatingCompanyConfig {
        spec: spec("C-IND-A", "home-appliances", CompanyKind::Industrial),
        books: engine::company::operations::IndustryBooks::Industrial(
            engine::company::industrial::IndustrialBooks::new(config)
                .expect("fixture industrial A assembles"),
        ),
        flow: FlowParams::Industrial(IndustrialFlowParams {
            customer: CounterpartyId("EXT-CUST".to_string()),
            supplier: CounterpartyId("EXT-SUPP".to_string()),
            raw_item: InventoryItemCode("RAW-1".to_string()),
            finished_item: InventoryItemCode("FG-1".to_string()),
            raw_account: LedgerAccountId("1403".to_string()),
            finished_account: LedgerAccountId("1405".to_string()),
            base_daily_demand_units: 10,
            unit_price_excl_vat: yuan(100),
            receivable_credit_days: 5,
            raw_replenish_target_units: 200,
            raw_unit_cost_excl_vat: yuan(40),
            daily_production_units: 8,
            daily_conversion_cost: yuan(30),
            daily_admin_expense: yuan(10),
            bad_debt_base_bp: 100,
            asset_impairment_fraction_bp: 2_000,
        }),
    }
}

/// 工商 B（化工行业）：与 A 不同行业——行业成本冲击只作用于被标签的一方。
pub(crate) fn industrial_b(as_of: CivilDate) -> OperatingCompanyConfig {
    let config = IndustrialConfig {
        chart: industrial_chart_v2(),
        as_of,
        opening_lines: vec![
            cent_line("1002", PostingSide::Debit, 260_000),
            cent_line("1403", PostingSide::Debit, 200_000),
            cent_line("1405", PostingSide::Debit, 60_000),
            cent_line("4001", PostingSide::Credit, 520_000),
        ],
        opening_inventory: vec![
            OpeningInventoryItem {
                account: LedgerAccountId("1403".to_string()),
                item: InventoryItemCode("RAW-1".to_string()),
                quantity: 100,
                cost: amt(200_000),
            },
            OpeningInventoryItem {
                account: LedgerAccountId("1405".to_string()),
                item: InventoryItemCode("FG-1".to_string()),
                quantity: 20,
                cost: amt(60_000),
            },
        ],
        opening_assets: Vec::new(),
        opening_debt: None,
        counterparties: vec![
            cp("EXT-SUPP", CounterpartyKind::Supplier),
            cp("EXT-CUST", CounterpartyKind::Customer),
        ],
        budget: OperatingBudget::new(AccountingAmount::ZERO, Vec::new())
            .expect("fixture budget valid"),
        tax_policy: fixture_policy(),
    };
    OperatingCompanyConfig {
        spec: spec("C-IND-B", "industrial-chemicals", CompanyKind::Industrial),
        books: engine::company::operations::IndustryBooks::Industrial(
            engine::company::industrial::IndustrialBooks::new(config)
                .expect("fixture industrial B assembles"),
        ),
        flow: FlowParams::Industrial(IndustrialFlowParams {
            customer: CounterpartyId("EXT-CUST".to_string()),
            supplier: CounterpartyId("EXT-SUPP".to_string()),
            raw_item: InventoryItemCode("RAW-1".to_string()),
            finished_item: InventoryItemCode("FG-1".to_string()),
            raw_account: LedgerAccountId("1403".to_string()),
            finished_account: LedgerAccountId("1405".to_string()),
            base_daily_demand_units: 5,
            unit_price_excl_vat: yuan(30),
            receivable_credit_days: 3,
            raw_replenish_target_units: 100,
            raw_unit_cost_excl_vat: yuan(20),
            daily_production_units: 4,
            daily_conversion_cost: yuan(10),
            daily_admin_expense: yuan(5),
            bad_debt_base_bp: 100,
            asset_impairment_fraction_bp: 2_000,
        }),
    }
}

/// 银行（banking 行业）：存贷/手续费流——**不读商品需求字段**（K4 跨行业红线）。
pub(crate) fn bank_c(as_of: CivilDate) -> OperatingCompanyConfig {
    let config = BankConfig {
        chart: bank_chart_v3(),
        as_of,
        opening_lines: vec![
            cent_line("1003", PostingSide::Debit, 5_000_000),
            cent_line("4001", PostingSide::Credit, 5_000_000),
        ],
        counterparties: vec![
            cp("EXT-DEP", CounterpartyKind::Customer),
            cp("EXT-BOR", CounterpartyKind::Customer),
            cp("EXT-FEE", CounterpartyKind::Customer),
        ],
        ecl_policy: EclPolicy {
            version: 1,
            stage1_default: vec![EclScenario {
                weight_bp: 10_000,
                pd_bp: 100,
                lgd_bp: 4_000,
            }],
            lifetime_default: vec![EclScenario {
                weight_bp: 10_000,
                pd_bp: 800,
                lgd_bp: 6_000,
            }],
        },
    };
    OperatingCompanyConfig {
        spec: spec("C-BANK", "banking", CompanyKind::Bank),
        books: engine::company::operations::IndustryBooks::Bank(
            engine::company::bank::BankBooks::new(config).expect("fixture bank assembles"),
        ),
        flow: FlowParams::Bank(BankFlowParams {
            depositor: CounterpartyId("EXT-DEP".to_string()),
            borrower: CounterpartyId("EXT-BOR".to_string()),
            fee_customer: CounterpartyId("EXT-FEE".to_string()),
            deposit_principal: yuan(1_000),
            deposit_rate_bp: 150,
            deposit_term_days: 10,
            deposit_every_days: 3,
            loan_principal: yuan(800),
            loan_rate_bp: 400,
            loan_term_days: 5,
            lending_every_days: 3,
            daily_fee_income: yuan(5),
            credit_deterioration_scenarios: vec![EclScenario {
                weight_bp: 10_000,
                pd_bp: 2_000,
                lgd_bp: 5_000,
            }],
        }),
    }
}

/// 保险（property-insurance 行业）：新单量响应需求字段（2 组/日基数）。
pub(crate) fn insurance_c(as_of: CivilDate) -> OperatingCompanyConfig {
    let config = InsuranceConfig {
        chart: insurance_chart_v4(),
        as_of,
        opening_lines: vec![
            cent_line("1002", PostingSide::Debit, 2_000_000),
            cent_line("4001", PostingSide::Credit, 2_000_000),
        ],
        counterparties: vec![cp("EXT-POL", CounterpartyKind::Customer)],
        discount: DiscountAssumption {
            version: 1,
            rate_bp: 400,
        },
    };
    OperatingCompanyConfig {
        spec: spec("C-INS", "property-insurance", CompanyKind::Insurance),
        books: engine::company::operations::IndustryBooks::Insurance(
            engine::company::insurance::InsuranceBooks::new(config)
                .expect("fixture insurance assembles"),
        ),
        flow: FlowParams::Insurance(InsuranceFlowParams {
            policyholder: CounterpartyId("EXT-POL".to_string()),
            daily_groups_base: 2,
            premium: yuan(60),
            expected_claims: yuan(50),
            risk_adjustment: yuan(3),
            coverage_days: 30,
            claim_every_days: 10,
            claim_size: yuan(10),
        }),
    }
}

/// 地产（residential-development 行业）：购地 → 开发 → 预售 → 完工 → 交付。
pub(crate) fn real_estate_c(as_of: CivilDate) -> OperatingCompanyConfig {
    let config = RealEstateConfig {
        chart: real_estate_chart_v5(),
        as_of,
        opening_lines: vec![
            cent_line("1002", PostingSide::Debit, 10_000_000),
            cent_line("4001", PostingSide::Credit, 10_000_000),
        ],
        counterparties: vec![
            cp("EXT-LAND", CounterpartyKind::Supplier),
            cp("EXT-CON", CounterpartyKind::Supplier),
            cp("EXT-BUYER", CounterpartyKind::Customer),
        ],
        budget: OperatingBudget::new(AccountingAmount::ZERO, Vec::new())
            .expect("fixture budget valid"),
        capitalization_policy: CapitalizationPolicy {
            version: 1,
            suspension_min_days: 90,
        },
        max_projects: 2,
    };
    OperatingCompanyConfig {
        spec: spec("C-RE", "residential-development", CompanyKind::RealEstate),
        books: engine::company::operations::IndustryBooks::RealEstate(
            engine::company::real_estate::RealEstateBooks::new(config)
                .expect("fixture real estate assembles"),
        ),
        flow: FlowParams::RealEstate(RealEstateFlowParams {
            land_seller: CounterpartyId("EXT-LAND".to_string()),
            contractor: CounterpartyId("EXT-CON".to_string()),
            buyer: CounterpartyId("EXT-BUYER".to_string()),
            project: ProjectId("P-1".to_string()),
            total_units: 8,
            land_cost: yuan(2_000),
            development_days: 6,
            daily_development_spend: yuan(500),
            presale_open_day: 1,
            presale_units_per_day: 2,
            unit_price: yuan(300),
            delivery_lag_days: 2,
        }),
    }
}

/// 资金断裂工商（无授信、现金仅 ¥5）：付款失败路径金样。
pub(crate) fn industrial_broke(as_of: CivilDate) -> OperatingCompanyConfig {
    let config = IndustrialConfig {
        chart: industrial_chart_v2(),
        as_of,
        opening_lines: vec![
            cent_line("1002", PostingSide::Debit, 500),
            cent_line("1403", PostingSide::Debit, 500),
            cent_line("1405", PostingSide::Debit, 300),
            cent_line("4001", PostingSide::Credit, 1_300),
        ],
        opening_inventory: vec![
            OpeningInventoryItem {
                account: LedgerAccountId("1403".to_string()),
                item: InventoryItemCode("RAW-1".to_string()),
                quantity: 5,
                cost: amt(500),
            },
            OpeningInventoryItem {
                account: LedgerAccountId("1405".to_string()),
                item: InventoryItemCode("FG-1".to_string()),
                quantity: 2,
                cost: amt(300),
            },
        ],
        opening_assets: Vec::new(),
        opening_debt: None,
        counterparties: vec![
            cp("EXT-SUPP", CounterpartyKind::Supplier),
            cp("EXT-CUST", CounterpartyKind::Customer),
        ],
        budget: OperatingBudget::new(AccountingAmount::ZERO, Vec::new())
            .expect("fixture budget valid"),
        tax_policy: fixture_policy(),
    };
    OperatingCompanyConfig {
        spec: spec("C-IND-BROKE", "home-appliances", CompanyKind::Industrial),
        books: engine::company::operations::IndustryBooks::Industrial(
            engine::company::industrial::IndustrialBooks::new(config)
                .expect("fixture broke industrial assembles"),
        ),
        flow: FlowParams::Industrial(IndustrialFlowParams {
            customer: CounterpartyId("EXT-CUST".to_string()),
            supplier: CounterpartyId("EXT-SUPP".to_string()),
            raw_item: InventoryItemCode("RAW-1".to_string()),
            finished_item: InventoryItemCode("FG-1".to_string()),
            raw_account: LedgerAccountId("1403".to_string()),
            finished_account: LedgerAccountId("1405".to_string()),
            base_daily_demand_units: 1,
            unit_price_excl_vat: yuan(2),
            receivable_credit_days: 2,
            raw_replenish_target_units: 5,
            raw_unit_cost_excl_vat: yuan(1),
            daily_production_units: 2,
            daily_conversion_cost: yuan(3),
            daily_admin_expense: yuan(1),
            bad_debt_base_bp: 100,
            asset_impairment_fraction_bp: 2_000,
        }),
    }
}

/// 全量冲击参数关闭（无随机事件）——金样对照跑用。
pub(crate) fn quiet_params() -> ShockParams {
    ShockParams {
        version: 1,
        market_candidate_bp: 0,
        industry_candidate_bp: 0,
        company_candidate_bp: 0,
        duration_min_days: 5,
        duration_max_days: 30,
        market_demand_band_bp: 500,
        industry_cost_band_bp: 1_000,
        company_demand_band_bp: 2_000,
        credit_deterioration_add_bp: 500,
    }
}

/// 四行业公司集合（seed 决定 RNG 流；as_of 由调用方给）。
pub(crate) fn four_company_config(
    seed: u64,
    params: ShockParams,
    as_of: CivilDate,
) -> CompanyOperationsConfig {
    CompanyOperationsConfig {
        seed,
        shock_params: params,
        companies: vec![
            industrial_a(as_of),
            industrial_b(as_of),
            bank_c(as_of),
            insurance_c(as_of),
            real_estate_c(as_of),
        ],
    }
}

/// 两工商对照集合（行业成本冲击门控金样）。
pub(crate) fn two_industrial_config(
    seed: u64,
    params: ShockParams,
    as_of: CivilDate,
) -> CompanyOperationsConfig {
    CompanyOperationsConfig {
        seed,
        shock_params: params,
        companies: vec![industrial_a(as_of), industrial_b(as_of)],
    }
}
