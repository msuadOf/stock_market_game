use engine::accounting::consolidation::{MemberId, ScopeId};
use engine::accounting::reports::ReportKind;
use engine::accounting::{AccountingPeriod, PeriodStatus};
use engine::calendar::CivilDate;
use engine::company::config::CompanySystemConfig;
use engine::company::simple::SimpleFinanceState;
use engine::company::{CompanyId, CompanyKind, PublicReportQuery, PublicReportScope};
use engine::{GameSession, StockCode};

#[path = "publications/session_fixture.rs"]
mod session_fixture;

fn setup() -> engine::session::SessionSetup {
    let mut setup = session_fixture::civil_setup(CivilDate::from_iso("2030-03-31").unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.ticks_per_day = 2;
    setup.stocks[0].total_shares = 10_000_000;
    setup.stocks[0].float_shares = 0;
    setup
}

fn finance_state(save: &engine::SaveSlot, company: &CompanyId) -> SimpleFinanceState {
    let system = serde_json::to_value(&save.company_system).unwrap();
    let finance = &system["implementation"]["state"]["companies"][&company.0]["finance"];
    serde_json::from_value(finance.clone()).unwrap()
}

#[test]
fn four_explicit_financial_kinds_close_and_publish_real_simple_reports() {
    let mut setup = setup();
    let stock_template = setup.stocks[0].clone();
    let CompanySystemConfig::Simple(config) = &mut setup.company_system else {
        unreachable!()
    };
    let kinds = [
        CompanyKind::Industrial,
        CompanyKind::Bank,
        CompanyKind::Insurance,
        CompanyKind::RealEstate,
    ];
    let mut stocks = Vec::new();
    let mut configs = Vec::new();
    for (index, kind) in kinds.into_iter().enumerate() {
        let mut stock = stock_template.clone();
        stock.code = StockCode(format!("60010{index}"));
        let company = CompanyId(format!("C-{}", stock.code.0));
        let mut company_config = config.companies[0].clone();
        company_config.company = company;
        company_config.kind = kind;
        match kind {
            CompanyKind::Bank => {
                company_config.finance.opening_lines[0].account.0 = "1003".into();
            }
            CompanyKind::Industrial | CompanyKind::Insurance | CompanyKind::RealEstate => {
                company_config.finance.opening_lines[0].account.0 = "1002".into();
            }
        }
        stock.total_shares = 10_000_000;
        stock.float_shares = 0;
        stocks.push(stock);
        configs.push(company_config);
    }
    config.companies = configs;
    setup.stocks = stocks;

    let mut session = GameSession::new(setup, 41).unwrap();
    for _ in 0..2 {
        session.step().unwrap();
    }
    session.end_civil_day().unwrap();
    let save = session.save().unwrap();
    let period = AccountingPeriod::from_iso("2030-03").unwrap();
    for (index, kind) in kinds.into_iter().enumerate() {
        let code = format!("60010{index}");
        let company_id = CompanyId(format!("C-{code}"));
        let stock = save.setup.stocks.iter().find(|stock| stock.code.0 == code).unwrap();
        let issuer = save.company_system.issuers().get(&company_id).unwrap();
        assert_eq!(issuer.kind, kind);
        assert_eq!(issuer.issued_shares, stock.total_shares);
        assert_eq!(issuer.listed_stock.as_ref(), Some(&stock.code));

        let finance = finance_state(&save, &company_id);
        assert_eq!(finance.books().journal().period_status(period), PeriodStatus::Closed);
        assert_eq!(
            finance.closing().versions(
                &ScopeId::Standalone(MemberId(company_id.0.clone())),
                period,
                ReportKind::Monthly,
            ).len(),
            1,
        );
        let public = session.query_public_reports(&PublicReportQuery {
            company_id: company_id.0.clone(),
            cursor: None,
            page_size: Some(100),
        }).unwrap();
        assert!(!public.reports.is_empty());
        assert!(public.reports.iter().all(|report| matches!(
            report.financials.scope,
            PublicReportScope::Standalone { .. }
        )));
    }
    let mut restored = GameSession::restore(&save).unwrap();
    assert_eq!(restored.civil_date(), session.civil_date());
    restored.end_civil_day().unwrap();
    assert_eq!(restored.civil_date(), session.civil_date().next().unwrap());
}
