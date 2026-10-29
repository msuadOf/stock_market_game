//! 默认 new 的 G/N 只读装配合同；不构造 World、不改变 restore 或显式组数策略。

use super::*;

fn setup(stock_count: usize) -> SessionSetup {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.history_len = 1;
    let template = setup.stocks[0].clone();
    setup.stocks = (0..stock_count)
        .map(|index| StockSpec {
            code: StockCode(format!("{:06}", 600_100 + index)),
            ..template.clone()
        })
        .collect();
    setup
}

#[test]
fn default_new_runtime_topology_tracks_stock_count_without_constructing_session() {
    for count in [1, 3] {
        let setup = setup(count);
        setup.validate().unwrap();
        let (groups, desks) = setup.default_new_runtime_topology().unwrap();
        assert_eq!(groups, 1, "当前默认 new 的 NPC group 数必须保持原值");
        assert_eq!(desks, count, "每股必须对应一个 desk");
        assert_eq!(2 * (groups + desks + 2), 2 * (1 + count + 2));
    }
}

#[test]
fn default_new_runtime_topology_ignores_company_groups() {
    use crate::accounting::{
        AccountingAmount, InventoryItemCode, JournalLine, LedgerAccountId, PostingSide,
    };
    use crate::company::industrial::{industrial_account_chart, IndustrialBooks, IndustrialConfig};
    use crate::company::operations::{
        CompanyOperationsConfig, FlowParams, IndustrialFlowParams, IndustryBooks,
        OperatingCompanyConfig,
    };
    use crate::company::{
        CompanyCatalog, CompanyKind, CompanySpec, CounterpartyId, IndustryId, OperatingBudget,
    };

    let mut setup = setup(4);
    let ungrouped = setup.default_new_runtime_topology().unwrap();
    // 只为真实 new 提供合法静态发行人；零经营流避免为拓扑门构造额外交易。
    let books = IndustrialBooks::new(IndustrialConfig {
        chart: industrial_account_chart(),
        as_of: crate::calendar::CivilDate::from_ymd(setup.start_date.year() - 3, 12, 31).unwrap(),
        opening_lines: vec![
            JournalLine {
                account: LedgerAccountId("1002".into()),
                side: PostingSide::Debit,
                amount: AccountingAmount::from_cents(100_000),
            },
            JournalLine {
                account: LedgerAccountId("4001".into()),
                side: PostingSide::Credit,
                amount: AccountingAmount::from_cents(100_000),
            },
        ],
        opening_inventory: vec![],
        opening_assets: vec![],
        opening_debt: None,
        counterparties: vec![],
        budget: OperatingBudget::new(AccountingAmount::ZERO, vec![]).unwrap(),
        // 沿既有静态发行人夹具的合成税率，本用例没有应税经营流。
        tax_policy: crate::accounting::TaxPolicy {
            version: 1,
            vat: crate::accounting::VatPolicy {
                output_rate_bp: 1_300,
                input_rate_bp: 1_300,
                deductible_share_bp: 10_000,
            },
            income_tax: crate::accounting::IncomeTaxPolicy {
                rate_bp: 2_500,
                loss_carryforward_years: 5,
            },
        },
    })
    .unwrap();
    let codes = ["TOPO-ROOT-A", "TOPO-CHILD-A", "TOPO-ROOT-B", "TOPO-CHILD-B"];
    let catalog = CompanyCatalog::from_codes(&codes).unwrap();
    let companies = setup
        .stocks
        .iter_mut()
        .enumerate()
        .map(|(index, stock)| {
            stock.total_shares = 100_000;
            stock.float_shares = 0;
            OperatingCompanyConfig {
                spec: CompanySpec {
                    code: codes[index].into(),
                    name: "虚构拓扑发行人".into(),
                    industry: IndustryId("industrial".into()),
                    kind: CompanyKind::Industrial,
                    listed_stock: Some(stock.code.clone()),
                    issued_shares: stock.total_shares,
                    group_parent: match index {
                        1 => Some(codes[0].into()),
                        3 => Some(codes[2].into()),
                        _ => None,
                    },
                },
                books: IndustryBooks::Industrial(books.clone()),
                flow: FlowParams::Industrial(IndustrialFlowParams {
                    customer: CounterpartyId("UNUSED-CUSTOMER".into()),
                    supplier: CounterpartyId("UNUSED-SUPPLIER".into()),
                    raw_item: InventoryItemCode("RAW".into()),
                    finished_item: InventoryItemCode("FG".into()),
                    raw_account: LedgerAccountId("1403".into()),
                    finished_account: LedgerAccountId("1405".into()),
                    base_daily_demand_units: 0,
                    unit_price_excl_vat: AccountingAmount::from_cents(100),
                    receivable_credit_days: 1,
                    raw_replenish_target_units: 0,
                    raw_unit_cost_excl_vat: AccountingAmount::from_cents(1),
                    daily_production_units: 0,
                    daily_conversion_cost: AccountingAmount::ZERO,
                    daily_admin_expense: AccountingAmount::ZERO,
                    bad_debt_base_bp: 0,
                    asset_impairment_fraction_bp: 0,
                }),
            }
        })
        .collect();
    setup.groups = [(codes[0], codes[1]), (codes[2], codes[3])]
        .into_iter()
        .map(|(root, child)| GroupStructure {
            root: catalog.id(root).unwrap(),
            holdings: vec![GroupHolding {
                company: catalog.id(child).unwrap(),
                parent_held_shares: 80_000,
            }],
        })
        .collect();
    let mut shock_params = crate::company::events::ShockParams::current_default_parameters();
    shock_params.market_candidate_bp = 0;
    shock_params.industry_candidate_bp = 0;
    shock_params.company_candidate_bp = 0;
    setup.company_operations = Some(CompanyOperationsConfig {
        seed: 42,
        shock_params,
        catalog,
        companies,
    });
    setup.validate().unwrap();
    assert_eq!(setup.groups.len(), 2, "夹具必须让公司集团数量与默认 G 不同");
    assert_eq!(setup.default_new_runtime_topology().unwrap(), ungrouped);
    let session = GameSession::new(setup, 42).unwrap();
    assert_eq!(session.runtime_group_count, ungrouped.0);
    assert_eq!(session.state.markets.len(), ungrouped.1);
    assert!(
        session.runtime_driver.is_none(),
        "默认 new 不得提前启动 World"
    );
}

#[test]
fn default_new_runtime_topology_rejects_the_same_invalid_setup_as_validate() {
    let mut cases = Vec::new();
    cases.push(setup(0));
    let mut invalid = setup(1);
    invalid.ticks_per_day = 0;
    cases.push(invalid);
    let mut invalid = setup(1);
    invalid.stocks[0].tick = Money::from_cents(2);
    cases.push(invalid);
    let mut invalid = setup(1);
    invalid.flat_withholding_bp = Some(1_000);
    cases.push(invalid);
    for setup in cases {
        let expected = setup.validate().unwrap_err();
        let actual = setup.default_new_runtime_topology().unwrap_err();
        match (actual, expected) {
            (SessionError::InvalidSetup(actual), SessionError::InvalidSetup(expected)) => {
                assert_eq!(actual, expected, "不能只数股票而绕过完整 setup 校验");
            }
            (actual, expected) => {
                panic!("错误类型不一致：actual={actual:?}, expected={expected:?}")
            }
        }
    }
}

#[test]
fn default_new_runtime_topology_matches_real_default_new() {
    for count in [1, 3] {
        let setup = setup(count);
        let expected = setup.default_new_runtime_topology().unwrap();
        let session = GameSession::new(setup, 42).unwrap();
        assert_eq!(session.runtime_group_count, expected.0);
        assert_eq!(session.state.markets.len(), expected.1);
        assert!(
            session.runtime_driver.is_none(),
            "只做默认 new，不提前启动 World"
        );
    }
}
