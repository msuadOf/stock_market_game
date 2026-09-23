//! 共享夹具：单/双公司经营配置（静默冲击，确定性内容；无借款开局——
//! 前史跨 2 年不引入还本/付息到期，账面纯经营流）。
//!
//! 压缩会话夹具在 `session_fixture`；原始账套夹具在 `books_fixture`。

use engine::accounting::{
    AccountingAmount, InventoryItemCode, JournalLine, LedgerAccountId, PostingSide,
};
use engine::calendar::CivilDate;
use engine::company::bank::{bank_chart_v3, BankConfig, EclPolicy, EclScenario};
use engine::company::events::ShockParams;
use engine::company::industrial::{industrial_chart_v2, IndustrialConfig};
use engine::company::operations::{
    CompanyOperationsConfig, FlowParams, IndustrialFlowParams, OperatingCompanyConfig,
};
use engine::company::{
    CompanyId, CompanyKind, CompanySpec, CounterpartyId, CounterpartyKind, ExternalCounterparty,
    IndustryId,
};

/// 经营域种子（测试固定值，确定性钉死）。
pub(crate) const OPS_SEED: u64 = 7;

/// 披露域唯一工商公司 id（周末发布/前史金样共用）。
pub(crate) const INDUSTRIAL_ID: &str = "C-IND-A";

/// 前史双公司金样的银行 id。
pub(crate) const BANK_ID: &str = "C-BANK";

pub(crate) fn d(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("test fixture date must be valid ISO date")
}

/// 自然日加法助手（排期/窗口构造）。
pub(crate) fn plus_days(
    base: CivilDate,
    days: u32,
) -> Result<CivilDate, engine::calendar::CivilDateError> {
    let mut cursor = base;
    for _ in 0..days {
        cursor = cursor.next()?;
    }
    Ok(cursor)
}

/// 元 → AccountingAmount（分）。
pub(crate) fn yuan(v: i128) -> AccountingAmount {
    AccountingAmount::from_cents(v.checked_mul(100).expect("fixture yuan overflow"))
}

fn cent_line(code: &str, side: PostingSide, cents: i128) -> JournalLine {
    JournalLine {
        account: LedgerAccountId(code.to_string()),
        side,
        amount: AccountingAmount::from_cents(cents),
    }
}

fn cp(id: &str, kind: CounterpartyKind) -> ExternalCounterparty {
    ExternalCounterparty {
        id: CounterpartyId(id.to_string()),
        kind,
        name: format!("虚构对手方{id}"),
    }
}

fn spec(id: &str, kind: CompanyKind) -> CompanySpec {
    CompanySpec {
        id: CompanyId(id.to_string()),
        name: format!("虚构公司{id}"),
        industry: IndustryId("fixture-industry".to_string()),
        kind,
        listed_stock: None,
        issued_shares: 100_000,
        group_parent: None,
    }
}

/// 工商公司（开局种子与总账逐科目对账守卫满足）。
fn industrial_company(as_of: CivilDate) -> OperatingCompanyConfig {
    let config = IndustrialConfig {
        chart: industrial_chart_v2(),
        as_of,
        opening_lines: vec![
            cent_line("1002", PostingSide::Debit, 2_000_000),
            cent_line("1403", PostingSide::Debit, 500_000),
            cent_line("1405", PostingSide::Debit, 200_000),
            cent_line("1601", PostingSide::Debit, 800_000),
            cent_line("4001", PostingSide::Credit, 3_500_000),
        ],
        opening_inventory: vec![
            engine::company::industrial::OpeningInventoryItem {
                account: LedgerAccountId("1403".to_string()),
                item: InventoryItemCode("RAW-1".to_string()),
                quantity: 100,
                cost: AccountingAmount::from_cents(500_000),
            },
            engine::company::industrial::OpeningInventoryItem {
                account: LedgerAccountId("1405".to_string()),
                item: InventoryItemCode("FG-1".to_string()),
                quantity: 20,
                cost: AccountingAmount::from_cents(200_000),
            },
        ],
        opening_assets: vec![engine::company::industrial::OpeningAssetItem {
            code: engine::accounting::FixedAssetCode("FA-1".to_string()),
            cost: AccountingAmount::from_cents(800_000),
            salvage_value: AccountingAmount::ZERO,
            life_months: 120,
        }],
        opening_debt: None,
        counterparties: vec![
            cp("EXT-SUPP", CounterpartyKind::Supplier),
            cp("EXT-CUST", CounterpartyKind::Customer),
        ],
        budget: engine::company::OperatingBudget::new(AccountingAmount::ZERO, Vec::new())
            .expect("fixture budget valid"),
        tax_policy: fixture_policy(),
    };
    OperatingCompanyConfig {
        spec: spec(INDUSTRIAL_ID, CompanyKind::Industrial),
        books: engine::company::operations::IndustryBooks::Industrial(
            engine::company::industrial::IndustrialBooks::new(config)
                .expect("fixture industrial assembles"),
        ),
        flow: FlowParams::Industrial(IndustrialFlowParams {
            customer: CounterpartyId("EXT-CUST".to_string()),
            supplier: CounterpartyId("EXT-SUPP".to_string()),
            raw_item: InventoryItemCode("RAW-1".to_string()),
            finished_item: InventoryItemCode("FG-1".to_string()),
            raw_account: LedgerAccountId("1403".to_string()),
            finished_account: LedgerAccountId("1405".to_string()),
            base_daily_demand_units: 4,
            unit_price_excl_vat: yuan(100),
            receivable_credit_days: 5,
            raw_replenish_target_units: 40,
            raw_unit_cost_excl_vat: yuan(40),
            daily_production_units: 4,
            daily_conversion_cost: yuan(20),
            daily_admin_expense: yuan(10),
            bad_debt_base_bp: 100,
            asset_impairment_fraction_bp: 2_000,
        }),
    }
}

/// 银行公司（跨行业前史覆盖：科目表 v3 列报路径）。
fn bank_company(as_of: CivilDate) -> OperatingCompanyConfig {
    let config = BankConfig {
        chart: bank_chart_v3(),
        as_of,
        opening_lines: vec![
            cent_line("1003", PostingSide::Debit, 3_000_000),
            cent_line("4001", PostingSide::Credit, 3_000_000),
        ],
        counterparties: vec![
            cp("EXT-DEP", CounterpartyKind::Customer),
            cp("EXT-BOR", CounterpartyKind::Customer),
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
        spec: spec(BANK_ID, CompanyKind::Bank),
        books: engine::company::operations::IndustryBooks::Bank(
            engine::company::bank::BankBooks::new(config).expect("fixture bank assembles"),
        ),
        flow: FlowParams::Bank(engine::company::operations::BankFlowParams {
            depositor: CounterpartyId("EXT-DEP".to_string()),
            borrower: CounterpartyId("EXT-BOR".to_string()),
            fee_customer: CounterpartyId("EXT-DEP".to_string()),
            deposit_principal: yuan(500),
            deposit_rate_bp: 150,
            deposit_term_days: 20,
            deposit_every_days: 5,
            loan_principal: yuan(400),
            loan_rate_bp: 400,
            loan_term_days: 10,
            lending_every_days: 5,
            daily_fee_income: yuan(5),
            credit_deterioration_scenarios: Vec::new(),
        }),
    }
}

/// Fixture 合成税务政策（13%/13% 全抵扣、25%）。
fn fixture_policy() -> engine::accounting::TaxPolicy {
    engine::accounting::TaxPolicy {
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

/// 静默冲击（无随机事件；确定性金样对照）。
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

/// 单工商公司经营配置（as_of 由调用方给；种子固定 OPS_SEED）。
pub(crate) fn single_company_config(as_of: CivilDate) -> CompanyOperationsConfig {
    CompanyOperationsConfig {
        seed: OPS_SEED,
        shock_params: quiet_params(),
        companies: vec![industrial_company(as_of)],
    }
}

/// 工商 + 银行双公司经营配置（前史跨行业金样）。
pub(crate) fn two_company_config(seed: u64, as_of: CivilDate) -> CompanyOperationsConfig {
    CompanyOperationsConfig {
        seed,
        shock_params: quiet_params(),
        companies: vec![industrial_company(as_of), bank_company(as_of)],
    }
}

/// 前史首日（= 开局年 − 2 年的 1 月 1 日；generate_history 同式）。
pub(crate) fn history_start_of(start: CivilDate) -> CivilDate {
    CivilDate::from_ymd(start.year() - 2, 1, 1).expect("history start within civil window")
}
