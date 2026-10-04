#![allow(dead_code)]

#[path = "company_operations/fixtures.rs"]
mod operating_fixture;
#[path = "publications/session_fixture.rs"]
mod session_fixture;

use engine::accounting::consolidation::{MemberId, ScopeId};
use engine::accounting::reports::ReportKind;
use engine::accounting::{AccountingPeriod, PeriodStatus};
use engine::calendar::CivilDate;
use engine::company::operations::{CompanyOperationsConfig, FlowParams};
use engine::company::{CompanyId, CompanyKind, PublicReportQuery, PublicReportScope};
use engine::session::{GroupHolding, GroupStructure, SessionSetup};
use engine::{GameSession, StockCode};

fn small_industrial_books() -> engine::company::industrial::IndustrialBooks {
    use engine::accounting::{AccountingAmount, JournalLine, LedgerAccountId, PostingSide};
    use engine::company::{
        CounterpartyId, CounterpartyKind, ExternalCounterparty, OperatingBudget,
    };
    engine::company::industrial::IndustrialBooks::new(
        engine::company::industrial::IndustrialConfig {
            chart: engine::company::industrial::industrial_chart_v2(),
            as_of: CivilDate::from_iso("2027-12-31").unwrap(),
            opening_lines: vec![
                JournalLine {
                    account: LedgerAccountId("1002".into()),
                    side: PostingSide::Debit,
                    amount: AccountingAmount::from_cents(100000),
                },
                JournalLine {
                    account: LedgerAccountId("4001".into()),
                    side: PostingSide::Credit,
                    amount: AccountingAmount::from_cents(100000),
                },
            ],
            opening_inventory: vec![],
            opening_assets: vec![],
            opening_debt: None,
            counterparties: [
                ("EXT-CUST", CounterpartyKind::Customer),
                ("EXT-SUPP", CounterpartyKind::Supplier),
                ("C-IND-A", CounterpartyKind::Supplier),
                ("C-IND-SUB", CounterpartyKind::Customer),
            ]
            .map(|(id, kind)| ExternalCounterparty {
                id: CounterpartyId(id.into()),
                kind,
                name: "虚构经营对手方".into(),
            })
            .to_vec(),
            budget: OperatingBudget::new(AccountingAmount::ZERO, vec![]).unwrap(),
            tax_policy: operating_fixture::fixture_policy(),
        },
    )
    .unwrap()
}

fn setup(kinds: &[CompanyKind], grouped: bool) -> SessionSetup {
    let mut setup = session_fixture::civil_setup(CivilDate::from_iso("2030-03-31").unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.ticks_per_day = 2;
    let as_of = CivilDate::from_iso("2027-12-31").unwrap();
    let mut configs = Vec::new();
    let mut stocks = Vec::new();
    for (index, kind) in kinds.iter().enumerate() {
        let mut company = match kind {
            CompanyKind::Industrial => operating_fixture::industrial_a(as_of),
            CompanyKind::Bank => operating_fixture::bank_c(as_of),
            CompanyKind::Insurance => operating_fixture::insurance_c(as_of),
            CompanyKind::RealEstate => operating_fixture::real_estate_c(as_of),
        };
        if let FlowParams::Insurance(flow) = &mut company.flow {
            flow.daily_groups_base = 1;
            flow.coverage_days = 2;
        }
        let mut stock = setup.stocks[0].clone();
        stock.code = StockCode(format!("60010{index}"));
        stock.total_shares = company.spec.issued_shares;
        stock.float_shares = 0;
        company.spec.listed_stock = Some(stock.code.clone());
        if grouped && index != 0 {
            company.spec.group_parent = Some(CompanyId("C-IND-A".into()));
        }
        stocks.push(stock);
        configs.push(company);
    }
    if grouped {
        setup.groups = vec![GroupStructure {
            root: configs[0].spec.id.clone(),
            holdings: configs
                .iter()
                .skip(1)
                .map(|company| GroupHolding {
                    company: company.spec.id.clone(),
                    parent_held_shares: company.spec.issued_shares * 8 / 10,
                })
                .collect(),
        }];
    }
    setup.stocks = stocks;
    setup.company_operations = Some(CompanyOperationsConfig {
        seed: 17,
        shock_params: operating_fixture::quiet_params(),
        companies: configs,
    });
    setup
}

fn end_day(session: &mut GameSession) {
    if engine::calendar::TradingCalendar::default_v1()
        .unwrap()
        .is_trading_day(
            engine::calendar::CalendarExchange::Sse,
            session.civil_date(),
        )
        .unwrap()
    {
        for _ in 0..2 {
            session.step().unwrap();
        }
    }
    session.end_civil_day().unwrap();
}

fn export_save(name: &str, save: &engine::SaveSlot) {
    if let Some(directory) = std::env::var_os("ASSEMBLY_SAVE_EXPORT_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join(name), serde_json::to_vec(save).unwrap()).unwrap();
    }
}

#[test]
fn four_custom_industries_close_real_books_and_publish_queryable_reports() {
    let setup = setup(
        &[
            CompanyKind::Industrial,
            CompanyKind::Bank,
            CompanyKind::Insurance,
            CompanyKind::RealEstate,
        ],
        false,
    );
    let mut session = GameSession::new(setup, 41).unwrap();
    end_day(&mut session);
    let save = session.save().unwrap();
    export_save("four-industries.json", &save);
    let period = AccountingPeriod::from_iso("2030-03").unwrap();
    for company_id in ["C-IND-A", "C-BANK", "C-INS", "C-RE"].map(|id| CompanyId(id.into())) {
        let company = save.company_operations.company(&company_id).unwrap();
        assert_eq!(
            company.books().books().journal().period_status(period),
            PeriodStatus::Closed
        );
        assert_eq!(
            save.closing_registry
                .versions(
                    &ScopeId::Standalone(MemberId(company_id.0.clone())),
                    period,
                    ReportKind::Monthly
                )
                .len(),
            1
        );
        let public = session
            .query_public_reports(&PublicReportQuery {
                company_id: company_id.0.clone(),
                cursor: None,
                page_size: Some(100),
            })
            .unwrap();
        assert!(!public.reports.is_empty());
        assert!(public.reports.iter().all(|report| matches!(
            report.financials.scope,
            PublicReportScope::Standalone { .. }
        )));
    }
    let mut restored = GameSession::restore(&save).unwrap();
    assert_eq!(restored.civil_date(), session.civil_date());
    end_day(&mut restored);
    assert_eq!(restored.civil_date(), session.civil_date().next().unwrap());
}

#[test]
fn mixed_group_closes_then_publishes_distinct_consolidated_public_report() {
    let setup = setup(&[CompanyKind::Industrial, CompanyKind::Insurance], true);
    let mut session = GameSession::new(setup, 41).unwrap();
    end_day(&mut session);
    let march = AccountingPeriod::from_iso("2030-03").unwrap();
    let save = session.save().unwrap();
    let scope = ScopeId::Consolidated(MemberId("C-IND-A".into()));
    let closed = save
        .closing_registry
        .version(&scope, march, ReportKind::Quarter, 1)
        .unwrap();
    closed.validate().unwrap();
    assert!(session
        .query_public_reports(&PublicReportQuery {
            company_id: "C-IND-A".into(),
            cursor: None,
            page_size: Some(100)
        })
        .unwrap()
        .reports
        .iter()
        .all(|report| report.period != "2030-03-31"));
    let publish = engine::information::scheduled_instant(
        engine::information::ScheduledReportKind::Q1,
        2030,
        engine::information::stable_company_offset(17, &CompanyId("C-IND-A".into())),
    )
    .unwrap()
    .date();
    let mut session = GameSession::restore(&save).unwrap();
    while session.civil_date() <= publish {
        end_day(&mut session);
    }
    let page = session
        .query_public_reports(&PublicReportQuery {
            company_id: "C-IND-A".into(),
            cursor: None,
            page_size: Some(100),
        })
        .unwrap();
    let reports = page
        .reports
        .iter()
        .filter(|report| report.period == "2030-03-31")
        .collect::<Vec<_>>();
    assert_eq!(reports.len(), 2);
    assert_ne!(reports[0].id, reports[1].id);
    assert_eq!(
        reports
            .iter()
            .filter(|report| matches!(
                report.financials.scope,
                PublicReportScope::Consolidated { .. }
            ))
            .count(),
        1
    );
    let final_save = session.save().unwrap();
    export_save("mixed-group.json", &final_save);
    assert_eq!(
        final_save
            .closing_registry
            .version(&scope, march, ReportKind::Quarter, 1)
            .unwrap(),
        closed
    );
    let restored = GameSession::restore(&final_save).unwrap();
    assert_eq!(
        restored
            .query_public_reports(&PublicReportQuery {
                company_id: "C-IND-A".into(),
                cursor: None,
                page_size: Some(100)
            })
            .unwrap()
            .reports
            .len(),
        page.reports.len()
    );
}

#[test]
fn real_internal_sale_facts_in_editable_save_close_and_publish_through_session() {
    use engine::accounting::{AccountingAmount, InventoryItemCode, LedgerAccountId};
    use engine::company::industrial::Settlement;
    use engine::company::operations::IndustryBooks;
    use engine::company::CounterpartyId;
    let mut setup = setup(&[CompanyKind::Industrial], false);
    let mut config = setup.company_operations.take().unwrap();
    config.companies[0].books = IndustryBooks::Industrial(small_industrial_books());
    let FlowParams::Industrial(flow) = &mut config.companies[0].flow else {
        unreachable!()
    };
    flow.base_daily_demand_units = 0;
    flow.raw_replenish_target_units = 0;
    flow.daily_production_units = 0;
    flow.daily_admin_expense = AccountingAmount::from_cents(1);
    flow.bad_debt_base_bp = 0;
    let mut child = config.companies[0].clone();
    child.spec.id = CompanyId("C-IND-SUB".into());
    child.spec.group_parent = Some(CompanyId("C-IND-A".into()));
    let mut stock = setup.stocks[0].clone();
    stock.code = StockCode("600109".into());
    child.spec.listed_stock = Some(stock.code.clone());
    setup.stocks.push(stock);
    config.companies.push(child);
    setup.company_operations = Some(config);
    setup.groups = vec![GroupStructure {
        root: CompanyId("C-IND-A".into()),
        holdings: vec![GroupHolding {
            company: CompanyId("C-IND-SUB".into()),
            parent_held_shares: 80000,
        }],
    }];
    let session = GameSession::new(setup, 41).unwrap();
    let save = session.save().unwrap();
    let mut root = save
        .company_operations
        .company(&CompanyId("C-IND-A".into()))
        .unwrap()
        .books()
        .as_industrial()
        .unwrap()
        .clone();
    let mut child = save
        .company_operations
        .company(&CompanyId("C-IND-SUB".into()))
        .unwrap()
        .books()
        .as_industrial()
        .unwrap()
        .clone();
    let date = CivilDate::from_iso("2030-03-30").unwrap();
    let due = CivilDate::from_iso("2030-04-02").unwrap();
    root.purchase(
        &CounterpartyId("EXT-SUPP".into()),
        InventoryItemCode("FG-1".into()),
        LedgerAccountId("1405".into()),
        4,
        AccountingAmount::from_cents(5000),
        Settlement::Cash,
        due,
        date,
    )
    .unwrap();
    root.sell_credit(
        &CounterpartyId("C-IND-SUB".into()),
        InventoryItemCode("FG-1".into()),
        2,
        AccountingAmount::from_cents(10000),
        due,
        date,
    )
    .unwrap();
    child
        .purchase(
            &CounterpartyId("C-IND-A".into()),
            InventoryItemCode("FG-1".into()),
            LedgerAccountId("1405".into()),
            2,
            AccountingAmount::from_cents(10000),
            Settlement::Credit,
            due,
            date,
        )
        .unwrap();
    child
        .sell_credit(
            &CounterpartyId("EXT-CUST".into()),
            InventoryItemCode("FG-1".into()),
            1,
            AccountingAmount::from_cents(16000),
            due,
            date,
        )
        .unwrap();
    let mut edited = serde_json::to_value(save).unwrap();
    edited["company_operations"]["companies"]["C-IND-A"]["books"] =
        serde_json::to_value(IndustryBooks::Industrial(root)).unwrap();
    edited["company_operations"]["companies"]["C-IND-SUB"]["books"] =
        serde_json::to_value(IndustryBooks::Industrial(child)).unwrap();
    let edited: engine::SaveSlot = serde_json::from_value(edited).unwrap();
    let mut session = GameSession::restore(&edited).unwrap();
    end_day(&mut session);
    let closed_save = session.save().unwrap();
    let period = AccountingPeriod::from_iso("2030-03").unwrap();
    let scope = ScopeId::Consolidated(MemberId("C-IND-A".into()));
    let group = closed_save
        .closing_registry
        .version(&scope, period, ReportKind::Quarter, 1)
        .unwrap();
    group.validate().unwrap();
    let mut standalone_income = AccountingAmount::ZERO;
    for id in ["C-IND-A", "C-IND-SUB"] {
        standalone_income = standalone_income
            .add(
                closed_save
                    .closing_registry
                    .version(
                        &ScopeId::Standalone(MemberId(id.into())),
                        period,
                        ReportKind::Monthly,
                        1,
                    )
                    .unwrap()
                    .income
                    .cumulative
                    .net_income,
            )
            .unwrap();
    }
    assert_eq!(
        group.income.cumulative.net_income,
        standalone_income
            .sub(AccountingAmount::from_cents(5000))
            .unwrap()
    );
    assert_eq!(
        group
            .balance_sheet
            .asset_lines
            .iter()
            .find(|(line, _)| *line == engine::accounting::reports::BsLine::Inventory)
            .unwrap()
            .1,
        AccountingAmount::from_cents(15000)
    );
    let publish = engine::information::scheduled_instant(
        engine::information::ScheduledReportKind::Q1,
        2030,
        engine::information::stable_company_offset(17, &CompanyId("C-IND-A".into())),
    )
    .unwrap()
    .date();
    let mut session = GameSession::restore(&closed_save).unwrap();
    while session.civil_date() <= publish {
        end_day(&mut session);
    }
    let page = session
        .query_public_reports(&PublicReportQuery {
            company_id: "C-IND-A".into(),
            cursor: None,
            page_size: Some(100),
        })
        .unwrap();
    let report = page
        .reports
        .iter()
        .find(|report| {
            report.period == "2030-03-31"
                && matches!(
                    report.financials.scope,
                    PublicReportScope::Consolidated { .. }
                )
        })
        .unwrap();
    assert_eq!(
        report.accounting.net_income,
        group.income.cumulative.net_income.to_string()
    );
    let final_save = session.save().unwrap();
    let mut restored = GameSession::restore(&final_save).unwrap();
    end_day(&mut restored);
    assert!(restored
        .query_public_reports(&PublicReportQuery {
            company_id: "C-IND-A".into(),
            cursor: None,
            page_size: Some(100)
        })
        .unwrap()
        .reports
        .iter()
        .any(|report| report.id
            == page
                .reports
                .iter()
                .find(|candidate| candidate.period == "2030-03-31"
                    && matches!(
                        candidate.financials.scope,
                        PublicReportScope::Consolidated { .. }
                    ))
                .unwrap()
                .id));
    export_save("internal-sale-group.json", &final_save);
}
