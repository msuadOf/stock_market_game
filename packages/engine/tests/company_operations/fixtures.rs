//! 四行业核心装配与按需使用的扩展 fixture。

#[path = "fixtures/core.rs"]
mod shared;
use engine::accounting::{AccountingAmount, InventoryItemCode, LedgerAccountId, PostingSide};
use engine::calendar::CivilDate;
use engine::company::events::ShockParams;
use engine::company::industrial::{
    industrial_account_chart, IndustrialConfig, OpeningInventoryItem,
};
use engine::company::operations::{
    CompanyOperationsConfig, FlowParams, IndustrialFlowParams, OperatingCompanyConfig,
};
use engine::company::{CompanyKind, CounterpartyId, CounterpartyKind, OperatingBudget};
pub(crate) use shared::*;
use shared::{cp, spec};

pub(crate) const START: &str = "2030-01-01";

/// 由公司配置推导目录（代码序分配 id；重复代码由 from_codes 显式报错）。
pub(crate) fn catalog_for<'a>(
    companies: impl IntoIterator<Item = &'a engine::company::operations::OperatingCompanyConfig>,
) -> engine::company::CompanyCatalog {
    let codes: Vec<&str> = companies
        .into_iter()
        .map(|company| company.spec.code.as_str())
        .collect();
    engine::company::CompanyCatalog::from_codes(&codes).unwrap()
}

/// 资金断裂工商（无授信、现金仅 ¥5）：付款失败路径金样。
pub(crate) fn industrial_broke(as_of: CivilDate) -> OperatingCompanyConfig {
    let config = IndustrialConfig {
        chart: industrial_account_chart(),
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

/// 两工商对照集合（行业成本冲击门控金样）。
pub(crate) fn two_industrial_config(
    seed: u64,
    params: ShockParams,
    as_of: CivilDate,
) -> CompanyOperationsConfig {
    CompanyOperationsConfig {
        seed,
        shock_params: params,
        catalog: engine::company::CompanyCatalog::from_codes(&["C-IND-A", "C-IND-B"]).unwrap(),
        companies: vec![industrial_a(as_of), industrial_b(as_of)],
    }
}
