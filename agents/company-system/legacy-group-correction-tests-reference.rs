use super::*;
use crate::accounting::consolidation::{MemberId, ScopeId};
use crate::accounting::reports::{ReportKind, VersionKind};
use crate::accounting::{
    AccountingAmount, AccountingPeriod, BusinessEventId, BusinessKind, CashFlowClass,
    InventoryItemCode, JournalLine, LedgerAccountId, PostingSide, TaxPolicy,
};
use crate::calendar::CivilDate;
use crate::company::industrial::{industrial_account_chart, IndustrialBooks, IndustrialConfig};
use crate::company::operations::{
    CompanyOperationsConfig, FlowParams, IndustrialFlowParams, IndustryBooks,
    OperatingCompanyConfig,
};
use crate::company::{
    CompanyKind, CompanySpec, CounterpartyId, CounterpartyKind, ExternalCounterparty, IndustryId,
    OperatingBudget,
};
use crate::information::PublishedReport;
use crate::session::{GroupHolding, GroupStructure};

fn date(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).unwrap()
}
fn yuan(amount: i128) -> AccountingAmount {
    AccountingAmount::from_cents(amount * 100)
}
fn period(year: i32) -> AccountingPeriod {
    AccountingPeriod::from_ymd(year, 12).unwrap()
}
fn scope() -> ScopeId {
    ScopeId::Consolidated(MemberId("root".into()))
}

fn session() -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.start_date = date("2030-01-05");
    setup.stocks[0].total_shares = 100;
    setup.npcs.inst_count = 0;
    let mut companies = Vec::new();
    for id in ["root", "child"] {
        let books = IndustrialBooks::new(IndustrialConfig {
            chart: industrial_account_chart(),
            as_of: date("2027-12-31"),
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
            counterparties: vec![ExternalCounterparty {
                id: CounterpartyId("external".into()),
                kind: CounterpartyKind::Customer,
                name: "虚构客户与供应商".into(),
            }],
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
        companies.push(OperatingCompanyConfig {
            spec: CompanySpec {
                id: CompanyId(id.into()),
                name: id.into(),
                industry: IndustryId("fixture-industrial".into()),
                kind: CompanyKind::Industrial,
                listed_stock: (id == "root").then(|| setup.stocks[0].code.clone()),
                issued_shares: 100,
                group_parent: (id == "child").then(|| CompanyId("root".into())),
            },
            books: IndustryBooks::Industrial(books),
            flow: FlowParams::Industrial(IndustrialFlowParams {
                customer: CounterpartyId("external".into()),
                supplier: CounterpartyId("external".into()),
                raw_item: InventoryItemCode("RAW".into()),
                finished_item: InventoryItemCode("FG".into()),
                raw_account: LedgerAccountId("1403".into()),
                finished_account: LedgerAccountId("1405".into()),
                base_daily_demand_units: 0,
                unit_price_excl_vat: yuan(10),
                receivable_credit_days: 5,
                raw_replenish_target_units: 0,
                raw_unit_cost_excl_vat: yuan(1),
                daily_production_units: 0,
                daily_conversion_cost: AccountingAmount::ZERO,
                daily_admin_expense: AccountingAmount::ZERO,
                bad_debt_base_bp: 100,
                asset_impairment_fraction_bp: 100,
            }),
        });
    }
    let mut shock_params = crate::company::events::ShockParams::current_default_parameters();
    shock_params.market_candidate_bp = 0;
    shock_params.industry_candidate_bp = 0;
    shock_params.company_candidate_bp = 0;
    setup.company_operations = Some(CompanyOperationsConfig {
        seed: 7181,
        shock_params,
        companies,
    });
    setup.groups = vec![GroupStructure {
        root: CompanyId("root".into()),
        holdings: vec![GroupHolding {
            company: CompanyId("child".into()),
            parent_held_shares: 80,
        }],
    }];
    GameSession::new(setup, 7181).unwrap()
}

fn published(session: &GameSession, company: &str, report_scope: ScopeId) -> PublishedReport {
    session
        .state
        .library
        .save()
        .reports
        .into_iter()
        .filter(|report| {
            report.company.0 == company
                && report.reports.scope == report_scope
                && report.reports.period == period(2028)
                && report.reports.kind == ReportKind::Annual
        })
        .last()
        .unwrap()
}

fn request(session: &GameSession, company: &str, operation: &str) -> CompanyReportCorrection {
    let original = published(
        session,
        company,
        ScopeId::Standalone(MemberId(company.into())),
    );
    CompanyReportCorrection {
        operation_id: operation.into(),
        company: original.company,
        supersedes: original.id,
        entries: vec![JournalEntry {
            source: BusinessEventId::new(900_000_001),
            date: session.state.civil_clock.current_date(),
            kind: BusinessKind::CashExpense,
            cash_flow: CashFlowClass::Operating,
            lines: vec![
                JournalLine {
                    account: LedgerAccountId("6602".into()),
                    side: PostingSide::Debit,
                    amount: yuan(100),
                },
                JournalLine {
                    account: LedgerAccountId("1002".into()),
                    side: PostingSide::Credit,
                    amount: yuan(100),
                },
            ],
        }],
        reason: "补记已确认的前期管理费用".into(),
    }
}

#[test]
fn child_and_root_day_end_corrections_preserve_public_versions_and_minority_shares() {
    let mut session = session();
    let originals = session.state.library.save().reports;
    let group_original = published(&session, "root", scope());
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, request(&session, "child", "child-correction"))
        .unwrap();
    session.end_civil_day().unwrap();
    let child_corrected = published(&session, "root", scope());
    assert_eq!(child_corrected.reports.version.sequence, 2);
    assert_eq!(child_corrected.supersedes, Some(group_original.id));
    assert_eq!(
        child_corrected.reports.income.cumulative.net_income,
        yuan(-75)
    );
    assert_eq!(
        child_corrected.reports.income.minority_net_income,
        Some(yuan(-15))
    );
    assert_eq!(
        child_corrected.reports.income.net_income_to_parent,
        Some(yuan(-60))
    );
    assert_eq!(
        child_corrected.reports.balance_sheet.total_equity,
        yuan(1925)
    );
    assert_eq!(
        child_corrected.reports.balance_sheet.equity_to_parent,
        yuan(1740)
    );
    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(&save).unwrap()
    );
    session = restored;
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, request(&session, "root", "root-correction"))
        .unwrap();
    let report = session.end_civil_day().unwrap();
    let root_corrected = published(&session, "root", scope());
    assert_eq!(root_corrected.reports.version.sequence, 3);
    assert_eq!(root_corrected.supersedes, Some(child_corrected.id));
    assert_eq!(
        root_corrected.reports.income.cumulative.net_income,
        yuan(-150)
    );
    assert_eq!(
        root_corrected.reports.income.minority_net_income,
        Some(yuan(-15))
    );
    assert_eq!(
        root_corrected.reports.income.net_income_to_parent,
        Some(yuan(-135))
    );
    assert_eq!(
        root_corrected.reports.balance_sheet.equity_to_parent,
        yuan(1665)
    );
    for original in originals {
        assert_eq!(
            session
                .state
                .library
                .report(original.id, report.disclosure_instant)
                .unwrap(),
            &original
        );
    }
    assert_eq!(
        session
            .state
            .library
            .report(child_corrected.id, report.disclosure_instant)
            .unwrap(),
        &child_corrected
    );
    assert_eq!(
        session
            .state
            .operations
            .industrial_books(&CompanyId("root".into()))
            .unwrap()
            .books()
            .ledger()
            .cash_total()
            .unwrap(),
        yuan(900)
    );
    assert_eq!(
        session
            .state
            .operations
            .industrial_books(&CompanyId("child".into()))
            .unwrap()
            .books()
            .ledger()
            .cash_total()
            .unwrap(),
        yuan(900)
    );
}

#[test]
fn private_group_original_is_rebuilt_and_scheduled_publication_uses_latest_sequence() {
    let mut session = session();
    let group = session.state.groups[0].clone();
    crate::session::company_groups::ensure_group_report(
        &group,
        &session.state.operations,
        Arc::make_mut(&mut session.state.closing),
        period(2029),
        ReportKind::Annual,
    )
    .unwrap();
    assert!(!session.state.library.save().reports.iter().any(|report| {
        report.reports.scope == scope()
            && report.reports.period == period(2029)
            && report.reports.kind == ReportKind::Annual
    }));
    let old = session
        .state
        .closing
        .versions(&scope(), period(2029), ReportKind::Annual)
        .last()
        .unwrap()
        .clone();
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, request(&session, "child", "child-private-refresh"))
        .unwrap();
    session.end_civil_day().unwrap();
    let versions = session
        .state
        .closing
        .versions(&scope(), period(2029), ReportKind::Annual);
    assert_eq!(versions[0], old);
    let latest = versions.last().unwrap();
    assert_eq!(latest.version.sequence, 2);
    assert_eq!(latest.version.kind, VersionKind::Original);
    assert_eq!(latest.version.supersedes, None);
    let expected = latest.clone();
    let group = session.state.groups[0].clone();
    let offset =
        crate::information::stable_company_offset(session.state.operations.seed, &group.root);
    let instant = crate::information::scheduled_instant(
        crate::information::ScheduledReportKind::Annual,
        2029,
        offset,
    )
    .unwrap();
    let id = crate::session::company_groups::publish_group_scheduled(
        &group,
        &session.state.operations,
        Arc::make_mut(&mut session.state.closing),
        Arc::make_mut(&mut session.state.library),
        instant,
        2029,
        crate::information::ScheduledReportKind::Annual,
        crate::information::ReportFrequency::Quarterly,
    )
    .unwrap();
    assert_eq!(
        session.state.library.report(id, instant).unwrap().reports,
        expected
    );
}

#[test]
fn last_group_publication_failure_rolls_back_day_and_same_source_retries_after_cancel() {
    let mut session = session();
    let valid_library = session.state.library.clone();
    let group = session.state.groups[0].clone();
    let offset =
        crate::information::stable_company_offset(session.state.operations.seed, &group.root);
    let instant = crate::information::scheduled_instant(
        crate::information::ScheduledReportKind::Annual,
        2029,
        offset,
    )
    .unwrap();
    crate::session::company_groups::publish_group_scheduled(
        &group,
        &session.state.operations,
        Arc::make_mut(&mut session.state.closing),
        Arc::make_mut(&mut session.state.library),
        instant,
        2029,
        crate::information::ScheduledReportKind::Annual,
        crate::information::ReportFrequency::Quarterly,
    )
    .unwrap();
    let correction = request(&session, "child", "child-failed-group");
    let epoch = session.report_correction_epoch();
    session
        .enqueue_report_correction(&epoch, correction.clone())
        .unwrap();
    let before = session.business_state_hash().unwrap();
    let books = serde_json::to_value(session.state.operations.as_ref()).unwrap();
    let closing = serde_json::to_value(session.state.closing.as_ref()).unwrap();
    let library = session.state.library.save();
    let error = session.end_civil_day().unwrap_err();
    assert!(error.to_string().contains("集团更正公布时点早于被更正报告"));
    assert_eq!(session.business_state_hash().unwrap(), before);
    assert_eq!(
        serde_json::to_value(session.state.operations.as_ref()).unwrap(),
        books
    );
    assert_eq!(
        serde_json::to_value(session.state.closing.as_ref()).unwrap(),
        closing
    );
    assert_eq!(session.state.library.save(), library);
    assert_eq!(
        session.state.pending_report_corrections,
        vec![correction.clone()]
    );
    assert!(session.state.report_correction_operations.is_empty());
    session
        .cancel_report_correction(&epoch, &correction.operation_id)
        .unwrap();
    session.state.library = valid_library;
    session
        .enqueue_report_correction(&epoch, correction.clone())
        .unwrap();
    session.end_civil_day().unwrap();
    assert!(session
        .state
        .report_correction_operations
        .contains_key(&correction.operation_id));
    let save = session.save().unwrap();
    assert_eq!(
        serde_json::to_value(GameSession::restore(&save).unwrap().save().unwrap()).unwrap(),
        serde_json::to_value(save).unwrap()
    );
}

#[test]
fn completed_root_correction_cannot_replace_standalone_target_with_consolidated_identity() {
    let mut session = session();
    let group_original = published(&session, "root", scope());
    let epoch = session.report_correction_epoch();
    let correction = request(&session, "root", "root-target-scope-corruption");
    session
        .enqueue_report_correction(&epoch, correction.clone())
        .unwrap();
    session.end_civil_day().unwrap();
    let group_correction = published(&session, "root", scope());
    assert_eq!(group_correction.supersedes, Some(group_original.id));
    let save = session.save().unwrap();
    GameSession::restore(&save).unwrap();
    let mut corrupted = save;
    let completed = corrupted
        .report_correction_operations
        .get_mut(&correction.operation_id)
        .unwrap();
    assert_eq!(completed.request, correction);
    assert!(completed.publications.contains(&group_correction.id));
    completed.request.supersedes = group_original.id;
    completed.publications = vec![group_correction.id];
    assert!(
        matches!(GameSession::restore(&corrupted), Err(SessionError::InvalidSave(detail)) if detail == "更正操作公司与原始公开报告不一致")
    );
}
