use super::*;
use crate::accounting::{InventoryItemCode, JournalLine, PostingSide, TaxPolicy};
use crate::company::industrial::{
    industrial_account_chart, IndustrialBooks, IndustrialConfig, Settlement,
};
use crate::company::operations::{
    CompanyOperationsConfig, FlowParams, IndustrialFlowParams, IndustryBooks,
    OperatingCompanyConfig,
};
use crate::company::{
    CompanyKind, CompanySpec, CounterpartyId, CounterpartyKind, ExternalCounterparty, IndustryId,
    OperatingBudget,
};

#[path = "../company_corrections/group_tests.rs"]
mod correction_cases;
#[path = "correction_time_tests.rs"]
mod correction_time_tests;

fn date(iso: &str) -> crate::calendar::CivilDate {
    crate::calendar::CivilDate::from_iso(iso).unwrap()
}
fn yuan(value: i128) -> AccountingAmount {
    AccountingAmount::from_cents(value * 100)
}

fn company(id: &str, parent: Option<&str>) -> OperatingCompanyConfig {
    let books = IndustrialBooks::new(IndustrialConfig {
        chart: industrial_account_chart(),
        as_of: date("2029-12-31"),
        opening_lines: vec![
            JournalLine {
                account: LedgerAccountId("1002".into()),
                side: PostingSide::Debit,
                amount: yuan(1000),
            },
            JournalLine {
                account: LedgerAccountId("4001".into()),
                side: PostingSide::Credit,
                amount: yuan(1000),
            },
        ],
        opening_inventory: vec![],
        opening_assets: vec![],
        opening_debt: None,
        counterparties: ["root", "sub", "external"]
            .map(|party| ExternalCounterparty {
                id: CounterpartyId(party.into()),
                kind: CounterpartyKind::Customer,
                name: party.into(),
            })
            .to_vec(),
        budget: OperatingBudget::new(AccountingAmount::ZERO, vec![]).unwrap(),
        tax_policy: TaxPolicy {
            version: 1,
            vat: crate::accounting::VatPolicy {
                output_rate_bp: 1300,
                input_rate_bp: 1300,
                deductible_share_bp: 10000,
            },
            income_tax: crate::accounting::IncomeTaxPolicy {
                rate_bp: 2500,
                loss_carryforward_years: 5,
            },
        },
    })
    .unwrap();
    OperatingCompanyConfig {
        spec: CompanySpec {
            id: CompanyId(id.into()),
            name: id.into(),
            industry: IndustryId("industrial".into()),
            kind: CompanyKind::Industrial,
            listed_stock: None,
            issued_shares: 100,
            group_parent: parent.map(|parent| CompanyId(parent.into())),
        },
        books: IndustryBooks::Industrial(books),
        flow: FlowParams::Industrial(IndustrialFlowParams {
            customer: CounterpartyId("external".into()),
            supplier: CounterpartyId("external".into()),
            raw_item: InventoryItemCode("RAW".into()),
            finished_item: InventoryItemCode("FG".into()),
            raw_account: LedgerAccountId("1403".into()),
            finished_account: LedgerAccountId("1405".into()),
            base_daily_demand_units: 1,
            unit_price_excl_vat: yuan(100),
            receivable_credit_days: 5,
            raw_replenish_target_units: 1,
            raw_unit_cost_excl_vat: yuan(50),
            daily_production_units: 1,
            daily_conversion_cost: yuan(1),
            daily_admin_expense: yuan(1),
            bad_debt_base_bp: 500,
            asset_impairment_fraction_bp: 1000,
        }),
    }
}

#[test]
fn historical_group_balances_survive_real_trade_settlement_and_restore() {
    let mut root = company("root", None);
    let mut sub = company("sub", Some("root"));
    let IndustryBooks::Industrial(root_books) = &mut root.books else {
        unreachable!()
    };
    let IndustryBooks::Industrial(sub_books) = &mut sub.books else {
        unreachable!()
    };
    let march = date("2030-03-30");
    let april = date("2030-04-02");
    root_books
        .purchase(
            &CounterpartyId("external".into()),
            InventoryItemCode("FG".into()),
            LedgerAccountId("1405".into()),
            4,
            yuan(50),
            Settlement::Cash,
            april,
            march,
        )
        .unwrap();
    let sale = root_books
        .sell_credit(
            &CounterpartyId("sub".into()),
            InventoryItemCode("FG".into()),
            2,
            yuan(100),
            april,
            march,
        )
        .unwrap();
    let purchase = sub_books
        .purchase(
            &CounterpartyId("root".into()),
            InventoryItemCode("FG".into()),
            LedgerAccountId("1405".into()),
            2,
            yuan(100),
            Settlement::Credit,
            april,
            march,
        )
        .unwrap();
    sub_books
        .sell_credit(
            &CounterpartyId("external".into()),
            InventoryItemCode("FG".into()),
            1,
            yuan(160),
            april,
            date("2030-03-31"),
        )
        .unwrap();
    root_books
        .collect(&sale.receivable, sale.receivable_amount, april)
        .unwrap();
    sub_books
        .settle_payable(&purchase.payable.unwrap(), april)
        .unwrap();
    root_books
        .purchase(
            &CounterpartyId("external".into()),
            InventoryItemCode("OTHER".into()),
            LedgerAccountId("1405".into()),
            1,
            yuan(20),
            Settlement::Cash,
            date("2030-05-01"),
            april,
        )
        .unwrap();
    sub_books
        .purchase(
            &CounterpartyId("external".into()),
            InventoryItemCode("OTHER".into()),
            LedgerAccountId("1405".into()),
            1,
            yuan(20),
            Settlement::Cash,
            date("2030-05-01"),
            april,
        )
        .unwrap();
    let mut shocks = crate::company::events::ShockParams::current_default_parameters();
    shocks.market_candidate_bp = 0;
    shocks.industry_candidate_bp = 0;
    shocks.company_candidate_bp = 0;
    let ops = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 17,
            shock_params: shocks,
            companies: vec![root, sub],
        },
        date("2030-05-01"),
    )
    .unwrap();
    let group = GroupStructure {
        root: CompanyId("root".into()),
        holdings: vec![GroupHolding {
            company: CompanyId("sub".into()),
            parent_held_shares: 80,
        }],
    };
    let period = AccountingPeriod::from_iso("2030-03").unwrap();
    let historical_request = request(&group, &ops, Some(period)).unwrap();
    assert_eq!(historical_request.intercompany_balances.len(), 2);
    assert!(historical_request
        .intercompany_balances
        .iter()
        .all(|balance| balance.amount == yuan(226)));
    assert_eq!(historical_request.intercompany_sales.len(), 1);
    assert_eq!(
        historical_request.intercompany_sales[0].unsold_inventory,
        yuan(100)
    );
    assert!(request(
        &group,
        &ops,
        Some(AccountingPeriod::from_iso("2030-04").unwrap())
    )
    .unwrap_err()
    .to_string()
    .contains("multiple item identities"));
    let mut closing = ClosingEngine::new();
    let adjustments = BTreeMap::from([(crate::accounting::BusinessEventId::new(2), period)]);
    assert!(matches!(
        generate_report_set(ReportRequest {
            period,
            kind: ReportKind::Monthly,
            source: ReportSource::Consolidated {
                request: request(&group, &ops, Some(period)).unwrap()
            },
            version: ReportVersion {
                sequence: 1,
                supersedes: None,
                kind: VersionKind::Original
            },
            adjustments: &adjustments
        }),
        Err(crate::accounting::reports::ReportError::ConsolidatedRestatementUnsupported)
    ));
    ensure_group_report(&group, &ops, &mut closing, period, ReportKind::Monthly).unwrap();
    let scope = ScopeId::Consolidated(MemberId("root".into()));
    let report = closing
        .version(&scope, period, ReportKind::Monthly, 1)
        .unwrap();
    assert_eq!(report.cash_flow.closing_cash, yuan(1774));
    assert_eq!(report.cash_flow.operating, yuan(-226));
    assert_eq!(report.income.cumulative.net_income, yuan(110));
    assert_eq!(report.income.minority_net_income, Some(yuan(12)));
    assert_eq!(
        report.balance_sheet.total_assets,
        AccountingAmount::from_cents(210480)
    );
    assert_eq!(
        report.balance_sheet.total_liabilities,
        AccountingAmount::from_cents(-520)
    );
    assert_eq!(report.balance_sheet.total_equity, yuan(2110));
    assert_eq!(report.equity.closing_minority, Some(yuan(212)));
    assert_eq!(
        report
            .balance_sheet
            .asset_lines
            .iter()
            .find(|(line, _)| *line == crate::accounting::reports::BsLine::Receivables)
            .unwrap()
            .1,
        AccountingAmount::from_cents(18080)
    );
    let restored: CompanyOperations =
        serde_json::from_slice(&serde_json::to_vec(&ops).unwrap()).unwrap();
    let mut restored_closing = ClosingEngine::new();
    ensure_group_report(
        &group,
        &restored,
        &mut restored_closing,
        period,
        ReportKind::Monthly,
    )
    .unwrap();
    assert_eq!(
        report,
        restored_closing
            .version(&scope, period, ReportKind::Monthly, 1)
            .unwrap()
    );
    let mut corrupted = serde_json::to_value(&ops).unwrap();
    let sources = corrupted["companies"]["sub"]["books"]["Industrial"]["inventory_source_events"]
        .as_array_mut()
        .unwrap();
    sources.retain(|source| source["event"] != serde_json::json!(3));
    assert!(serde_json::from_value::<CompanyOperations>(corrupted)
        .unwrap_err()
        .to_string()
        .contains("missing item identity"));
}

#[test]
fn custom_monthly_group_publication_requires_real_member_closing_and_keeps_scope() {
    let mut ops = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 17,
            shock_params: crate::company::events::ShockParams::current_default_parameters(),
            companies: vec![company("root", None), company("sub", Some("root"))],
        },
        date("2030-05-01"),
    )
    .unwrap();
    let group = GroupStructure {
        root: CompanyId("root".into()),
        holdings: vec![GroupHolding {
            company: CompanyId("sub".into()),
            parent_held_shares: 75,
        }],
    };
    let frequency = crate::information::ReportFrequency::Monthly {
        schedule: crate::information::MonthlyReportSchedule::Custom {
            day: 2,
            second_of_day: 35400,
            delay: crate::information::MonthlyReportDelay::None,
        },
    };
    let kind = ScheduledReportKind::Monthly { month: 4 };
    let instant = frequency
        .scheduled_instant(kind, 2030, ops.seed, &group.root)
        .unwrap();
    let mut closing = ClosingEngine::new();
    let mut library = PublicLibrary::new();
    let failed = publish_group_scheduled(
        &group,
        &ops,
        &mut closing,
        &mut library,
        instant,
        2030,
        kind,
        frequency,
    )
    .unwrap_err();
    assert!(failed.to_string().contains("requires closed member"));
    assert_eq!(library.report_count(), 0);
    let period = AccountingPeriod::from_ymd(2030, 4).unwrap();
    for company in ops.companies.values_mut() {
        let member = MemberId(company.spec().id.0.clone());
        closing
            .close_month(
                company.books_mut(),
                &member,
                crate::accounting::reports::IndustryPresentation::Industrial,
                period,
            )
            .unwrap();
    }
    let id = publish_group_scheduled(
        &group,
        &ops,
        &mut closing,
        &mut library,
        instant,
        2030,
        kind,
        frequency,
    )
    .unwrap();
    let published = library.report(id, instant).unwrap();
    assert_eq!(published.reports.kind, ReportKind::Monthly);
    assert_eq!(
        published.reports.scope,
        ScopeId::Consolidated(MemberId("root".into()))
    );
    assert_eq!(published.published_at, instant);
    assert_eq!(published.approved_at, instant);
}
