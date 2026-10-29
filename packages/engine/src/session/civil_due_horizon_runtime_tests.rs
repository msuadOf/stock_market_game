//! 真实市场收盘后新增经营 due 越过冻结日历上界的来源边界。

use super::*;
use crate::accounting::{
    AccountingAmount, InventoryItemCode, JournalLine, LedgerAccountId, PostingSide,
};
use crate::company::industrial::{industrial_account_chart, IndustrialBooks, IndustrialConfig};
use crate::company::operations::{
    CompanyOperationsConfig, FlowParams, IndustrialFlowParams, IndustryBooks,
    OperatingCompanyConfig,
};
use crate::company::{
    CompanyCatalog, CompanyKind, CompanySpec, CounterpartyId, CounterpartyKind, IndustryId,
    OperatingBudget,
};
use crate::runtime::drive::RuntimeDriver;
use std::sync::Arc;

const TICKS_PER_DAY: u64 = 6;

fn date(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).unwrap()
}

fn due_horizon_game() -> GameSession {
    let start = date("2099-12-28");
    let code = crate::account::StockCode("600101".into());
    let company_code = "DUE-HORIZON";
    let tax_policy = crate::accounting::TaxPolicy {
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
    };
    let books = IndustrialBooks::new(IndustrialConfig {
        chart: industrial_account_chart(),
        as_of: date("2096-12-31"),
        opening_lines: vec![
            JournalLine {
                account: LedgerAccountId("1002".into()),
                side: PostingSide::Debit,
                amount: AccountingAmount::from_cents(260_000),
            },
            JournalLine {
                account: LedgerAccountId("1403".into()),
                side: PostingSide::Debit,
                amount: AccountingAmount::from_cents(200_000),
            },
            JournalLine {
                account: LedgerAccountId("1405".into()),
                side: PostingSide::Debit,
                amount: AccountingAmount::from_cents(60_000),
            },
            JournalLine {
                account: LedgerAccountId("4001".into()),
                side: PostingSide::Credit,
                amount: AccountingAmount::from_cents(520_000),
            },
        ],
        opening_inventory: vec![
            crate::company::industrial::OpeningInventoryItem {
                account: LedgerAccountId("1403".into()),
                item: InventoryItemCode("RAW".into()),
                quantity: 100,
                cost: AccountingAmount::from_cents(200_000),
            },
            crate::company::industrial::OpeningInventoryItem {
                account: LedgerAccountId("1405".into()),
                item: InventoryItemCode("FINISHED".into()),
                quantity: 20,
                cost: AccountingAmount::from_cents(60_000),
            },
        ],
        opening_assets: vec![],
        opening_debt: None,
        counterparties: vec![
            crate::company::ExternalCounterparty {
                id: CounterpartyId("CUSTOMER".into()),
                kind: CounterpartyKind::Customer,
                name: "虚构客户".into(),
            },
            crate::company::ExternalCounterparty {
                id: CounterpartyId("SUPPLIER".into()),
                kind: CounterpartyKind::Supplier,
                name: "虚构供应商".into(),
            },
        ],
        budget: OperatingBudget::new(AccountingAmount::ZERO, vec![]).unwrap(),
        tax_policy,
    })
    .unwrap();
    let company = OperatingCompanyConfig {
        spec: CompanySpec {
            code: company_code.into(),
            name: "虚构工业公司".into(),
            industry: IndustryId("industrial".into()),
            kind: CompanyKind::Industrial,
            listed_stock: Some(code.clone()),
            issued_shares: 10_000_000,
            group_parent: None,
        },
        books: IndustryBooks::Industrial(books),
        flow: FlowParams::Industrial(IndustrialFlowParams {
            customer: CounterpartyId("CUSTOMER".into()),
            supplier: CounterpartyId("SUPPLIER".into()),
            raw_item: InventoryItemCode("RAW".into()),
            finished_item: InventoryItemCode("FINISHED".into()),
            raw_account: LedgerAccountId("1403".into()),
            finished_account: LedgerAccountId("1405".into()),
            base_daily_demand_units: 5,
            unit_price_excl_vat: AccountingAmount::from_cents(3_000),
            receivable_credit_days: 3,
            raw_replenish_target_units: 100,
            raw_unit_cost_excl_vat: AccountingAmount::from_cents(2_000),
            daily_production_units: 4,
            daily_conversion_cost: AccountingAmount::from_cents(1_000),
            daily_admin_expense: AccountingAmount::from_cents(500),
            bad_debt_base_bp: 0,
            asset_impairment_fraction_bp: 0,
        }),
    };
    let catalog = CompanyCatalog::from_codes(&[company_code]).unwrap();
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.start_date = start;
    setup.ticks_per_day = TICKS_PER_DAY;
    setup.npcs.inst_count = 0;
    setup.stocks[0].code = code;
    setup.stocks[0].total_shares = 10_000_000;
    setup.company_operations = Some(CompanyOperationsConfig {
        seed: 1,
        shock_params: {
            let mut params = crate::company::events::ShockParams::current_default_parameters();
            params.market_candidate_bp = 0;
            params.industry_candidate_bp = 0;
            params.company_candidate_bp = 0;
            params
        },
        catalog,
        companies: vec![company],
    });
    GameSession::new(setup, 7).unwrap()
}

fn close_real_market_day(game: &mut GameSession) {
    for _ in 0..TICKS_PER_DAY {
        game.step_via_runtime().unwrap();
    }
    assert!(game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .has_committed_market_close_for_civil(game.tick(), game.civil_date())
        .unwrap());
}

fn beyond_ceiling_error() -> SessionError {
    SessionError::from(
        CivilClock::new(date("2099-12-29"), crate::calendar::CalendarExchange::Sse)
            .unwrap()
            .validate_due_horizon(date("2100-01-01"))
            .unwrap_err(),
    )
}

fn assert_new_due_failure(game: &mut GameSession, external: bool) {
    let before = serde_json::to_vec(&game.save().unwrap()).unwrap();
    let error = game
        .with_prepared_civil_day_classified(|_, _| Ok(()))
        .unwrap_err();
    assert!(matches!(
        error.error(),
        SessionError::CivilClock(CivilClockError::Calendar(
            crate::calendar::CalendarError::AfterRuntimeCeiling { date: rejected, .. }
        )) if *rejected == date("2100-01-01")
    ));
    assert_eq!(error.recovery(), protocol::CivilRecovery::None);
    assert!(game.poison_reason().is_none());
    game.require_healthy().unwrap();
    assert_eq!(serde_json::to_vec(&game.save().unwrap()).unwrap(), before);
    assert_eq!(game.has_current_external_runtime_authority(), external);
}

#[test]
fn internal_new_due_horizon_failure_keeps_session_healthy_and_owner() {
    let mut game = due_horizon_game();
    close_real_market_day(&mut game);
    game.with_prepared_civil_day_classified(|_, _| Ok(()))
        .unwrap();
    assert_eq!(game.civil_date(), date("2099-12-29"));
    close_real_market_day(&mut game);
    let epoch = game.runtime_authority_epoch;
    let publication_slot = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .publication();
    let publication = publication_slot.load();

    assert_new_due_failure(&mut game, false);

    assert_eq!(game.runtime_authority_epoch, epoch);
    let driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
    assert_eq!(driver.world_committed_tick(), game.tick());
    assert!(Arc::ptr_eq(&publication_slot, &driver.publication()));
    assert!(Arc::ptr_eq(&publication, &driver.publication().load()));
}

#[test]
fn external_new_due_horizon_failure_keeps_session_healthy_and_owner() {
    let mut game = due_horizon_game();
    let mut driver = RuntimeDriver::from_session(&mut game).unwrap();
    close_real_market_day_external(&mut game, &mut driver);
    game.with_prepared_civil_day_classified(|_, _| Ok(()))
        .unwrap();
    assert_eq!(game.civil_date(), date("2099-12-29"));
    close_real_market_day_external(&mut game, &mut driver);
    let epoch = game.runtime_authority_epoch;
    let publication = driver.publication().load();

    assert_new_due_failure(&mut game, true);

    assert!(game.has_current_external_runtime_authority());
    assert_eq!(game.runtime_authority_epoch, epoch);
    assert_eq!(driver.world_committed_tick(), game.tick());
    assert!(Arc::ptr_eq(&publication, &driver.publication().load()));
}

fn close_real_market_day_external(game: &mut GameSession, driver: &mut RuntimeDriver) {
    for _ in 0..TICKS_PER_DAY {
        game.runtime_step_with_install(driver).unwrap();
    }
    assert!(driver
        .has_committed_market_close_for_civil(game.tick(), game.civil_date())
        .unwrap());
}

#[test]
fn internal_callback_with_same_calendar_error_remains_terminal() {
    let mut game = due_horizon_game();
    close_real_market_day(&mut game);
    let mut reached = false;
    let error = beyond_ceiling_error();
    let returned = game
        .with_prepared_civil_day_classified(|_, _| {
            reached = true;
            Err::<(), _>(error)
        })
        .unwrap_err();
    assert!(matches!(
        returned.error(),
        SessionError::CivilClock(CivilClockError::Calendar(
            crate::calendar::CalendarError::AfterRuntimeCeiling { date: rejected, .. }
        )) if *rejected == date("2100-01-01")
    ));
    assert!(reached, "合法 due prepare 后必须实际进入 callback");
    assert_eq!(returned.recovery(), protocol::CivilRecovery::None);
    assert!(game.poison_reason().is_some());
}

#[test]
fn external_callback_with_same_calendar_error_remains_terminal() {
    let mut game = due_horizon_game();
    let mut driver = RuntimeDriver::from_session(&mut game).unwrap();
    close_real_market_day_external(&mut game, &mut driver);
    let mut reached = false;
    let error = beyond_ceiling_error();
    let returned = game
        .with_prepared_civil_day_classified(|_, _| {
            reached = true;
            Err::<(), _>(error)
        })
        .unwrap_err();
    assert!(matches!(
        returned.error(),
        SessionError::CivilClock(CivilClockError::Calendar(
            crate::calendar::CalendarError::AfterRuntimeCeiling { date: rejected, .. }
        )) if *rejected == date("2100-01-01")
    ));
    assert!(reached, "合法 due prepare 后必须实际进入 callback");
    assert_eq!(returned.recovery(), protocol::CivilRecovery::None);
    assert!(game.poison_reason().is_some());
}
