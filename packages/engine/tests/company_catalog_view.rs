#![allow(dead_code)]

#[path = "publications/fixture.rs"]
mod operating_fixture;
#[path = "publications/session_fixture.rs"]
mod session_fixture;

use engine::company::config::CompanySystemConfig;
use engine::company::operations::{CompanyOperationsConfig, FlowParams};
use engine::company::{CompanyCatalog, CompanyId, CompanyMeta};
use engine::session::{CompanyCatalogEntry, SessionSetup, StockExchange};
use engine::{CivilDate, GameSession, Money, StockCode, StockId};

include!("../test-support/simple_company.rs");

fn setup(codes: &[&str], simple: bool) -> SessionSetup {
    let mut setup = session_fixture::civil_setup(CivilDate::from_iso("2030-01-02").unwrap());
    let stock = setup.stocks[0].clone();
    setup.stocks = codes
        .iter()
        .map(|code| {
            let mut stock = stock.clone();
            stock.code = StockCode((*code).to_owned());
            stock.exchange = if code.starts_with('6') {
                StockExchange::Shanghai
            } else {
                StockExchange::Shenzhen
            };
            stock.float_shares = 0;
            stock
        })
        .collect();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.history_len = 1;
    if simple {
        let CompanySystemConfig::Simple(mut config) = simple_company_fixture!(engine; ["600101"])
        else {
            unreachable!()
        };
        let company = config.companies[0].clone();
        config.companies = (0..codes.len())
            .map(|index| {
                let mut company = company.clone();
                company.company = CompanyId(index as u32);
                company
            })
            .collect();
        config.prehistory_periods = 2;
        setup.company_system = Some(CompanySystemConfig::Simple(config));
        setup.dividend_tax_mode = Some(engine::company::CashDividendTaxMode::FlatWithholding);
        setup.flat_withholding_bp = Some(1_000);
        setup.par_value_per_share = Some(Money::from_cents(100));
    }
    setup
}

fn custom_operations() -> CompanyOperationsConfig {
    let mut config =
        operating_fixture::single_company_config(CivilDate::from_iso("2027-12-31").unwrap());
    let template = config.companies[0].clone();
    config.companies = [
        ("发行人｜中文", "虚构上市公司", Some("600101")),
        ("constructor", "虚构未上市公司二", None),
        ("__proto__", "虚构未上市公司一", None),
    ]
    .into_iter()
    .map(|(code, name, stock)| {
        let mut company = template.clone();
        company.spec.code = code.to_owned();
        company.spec.name = name.to_owned();
        company.spec.listed_stock = stock.map(|code| StockCode(code.to_owned()));
        company.spec.issued_shares = 10_000_000;
        // 身份测试保留真实装配与前史，但不制造与目录无关的大量日经营分录。
        let FlowParams::Industrial(flow) = &mut company.flow else {
            unreachable!()
        };
        flow.base_daily_demand_units = 0;
        flow.raw_replenish_target_units = 0;
        flow.daily_production_units = 0;
        flow.daily_admin_expense = engine::accounting::AccountingAmount::ZERO;
        company
    })
    .collect();
    config.catalog = CompanyCatalog::from_metas(
        config
            .companies
            .iter()
            .map(|company| CompanyMeta {
                code: company.spec.code.clone(),
                name: company.spec.name.clone(),
            })
            .collect(),
    )
    .unwrap();
    config
}

#[test]
fn single_stock_uses_the_actual_selected_company_zero() {
    for simple in [false, true] {
        let game = GameSession::new(setup(&["600101"], simple), 42).unwrap();
        assert_eq!(
            game.company_catalog_view().unwrap(),
            vec![CompanyCatalogEntry {
                id: CompanyId(0),
                code: "C-600101".to_owned(),
                name: "虚构上市公司600101".to_owned(),
                listed_stock: Some(StockId(0)),
            }]
        );
    }
}

#[test]
fn subset_uses_local_stock_catalog_and_preserves_actual_issuer_ids() {
    for simple in [false, true] {
        let game = GameSession::new(setup(&["600610", "002156"], simple), 42).unwrap();
        let entries = game.company_catalog_view().unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.id, entry.code.as_str(), entry.listed_stock))
                .collect::<Vec<_>>(),
            vec![
                (
                    CompanyId(0),
                    "C-002156",
                    Some(game.stock_id("002156").unwrap())
                ),
                (
                    CompanyId(1),
                    "C-600610",
                    Some(game.stock_id("600610").unwrap())
                ),
            ]
        );
    }
}

#[test]
fn custom_registry_retains_unlisted_entities_unicode_and_special_codes() {
    let mut setup = setup(&["600101"], false);
    setup.company_operations = Some(custom_operations());
    let game = GameSession::new(setup, 42).unwrap();
    let entries = game.company_catalog_view().unwrap();
    assert_eq!(
        entries,
        vec![
            CompanyCatalogEntry {
                id: CompanyId(0),
                code: "__proto__".into(),
                name: "虚构未上市公司一".into(),
                listed_stock: None
            },
            CompanyCatalogEntry {
                id: CompanyId(1),
                code: "constructor".into(),
                name: "虚构未上市公司二".into(),
                listed_stock: None
            },
            CompanyCatalogEntry {
                id: CompanyId(2),
                code: "发行人｜中文".into(),
                name: "虚构上市公司".into(),
                listed_stock: Some(StockId(0))
            },
        ]
    );
    let wire = serde_json::to_value(entries).unwrap();
    assert_eq!(wire[0]["listed_stock"], serde_json::Value::Null);
    assert_eq!(wire[2]["id"], 2);
    assert_eq!(wire[2]["listed_stock"], 0);
}

#[test]
fn selected_simple_uses_its_only_issuer_source() {
    let setup = setup(&["600101"], true);
    let game = GameSession::new(setup, 42).unwrap();
    let entries = game.company_catalog_view().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, CompanyId(0));
    assert_eq!(entries[0].code, "C-600101");
    assert_eq!(entries[0].name, "虚构上市公司600101");
    assert_eq!(entries[0].listed_stock, Some(StockId(0)));
}

#[test]
fn selected_simple_explicitly_rejects_parallel_legacy_operations() {
    let mut setup = setup(&["600101"], true);
    setup.company_operations = Some(custom_operations());
    match GameSession::new(setup, 42) {
        Err(engine::SessionError::InvalidSetup(fault)) => {
            assert_eq!(fault, engine::ValidationFault::new(2235, vec![0, 1]));
        }
        Err(other) => panic!("必须保留双源配置冲突2235，实际错误：{other:?}"),
        Ok(_) => panic!("Simple不得接受并列Legacy经营配置"),
    }
}

#[test]
fn query_is_pure_and_restore_uses_the_restored_selected_source() {
    for simple in [false, true] {
        let mut setup = setup(&["600101"], simple);
        if !simple {
            setup.company_operations = Some(custom_operations());
        }
        let game = GameSession::new(setup, 42).unwrap();
        let before = serde_json::to_vec(&game.save().unwrap()).unwrap();
        let entries = game.company_catalog_view().unwrap();
        assert_eq!(serde_json::to_vec(&game.save().unwrap()).unwrap(), before);
        let restored = GameSession::restore(&serde_json::from_slice(&before).unwrap()).unwrap();
        assert_eq!(restored.company_catalog_view().unwrap(), entries);
        assert_eq!(
            serde_json::to_vec(&restored.save().unwrap()).unwrap(),
            before
        );
    }
}

#[test]
fn invalid_custom_issuer_mapping_and_duplicate_codes_are_rejected() {
    for defect in ["unknown-stock", "duplicate-code", "duplicate-stock"] {
        let mut setup = setup(&["600101"], false);
        let mut config = custom_operations();
        match defect {
            "unknown-stock" => {
                config.companies[0].spec.listed_stock = Some(StockCode("600999".into()))
            }
            "duplicate-code" => {
                config.companies[1].spec.code = config.companies[0].spec.code.clone()
            }
            "duplicate-stock" => {
                config.companies[1].spec.listed_stock = Some(StockCode("600101".into()))
            }
            _ => unreachable!(),
        }
        setup.company_operations = Some(config);
        assert!(GameSession::new(setup, 42).is_err(), "必须拒绝 {defect}");
    }
}
