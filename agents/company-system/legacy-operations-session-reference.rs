// 历史源文本，仅供追溯旧 Session 经营接线；不是当前契约或可运行测试。
// income_tax.rs 在归档时不在 Git index 中，旧原文无法从仓库精确恢复，故不在此猜写。
const LEGACY_SESSION_BOUNDARY: &str = r##"
use super::fixtures::*;
use engine::account::StockCode;
use engine::calendar::{CalendarExchange, CivilDate};
use engine::company::operations::CompanyOperationsConfig;
use engine::company::{CompanyId, ShockKind};
use engine::money::Money;
use engine::session::{
    CivilClock, CivilPhase, CompanyDisclosureKind, Event, FloatAllocation, GameSession, NpcSetup,
    SaveSlot, SecurityCategory, SessionSetup, StockExchange, StockSpec,
};
use engine::strategy::{HotParams, InstParams, RetailParams, StrategyParams};

fn session(date: CivilDate, broke: bool) -> GameSession {
    let as_of = CivilDate::from_ymd(date.year() - 3, 12, 31).unwrap();
    let company = if broke {
        industrial_broke(as_of)
    } else {
        industrial_a(as_of)
    };
    session_with_company(date, company, 1, quiet_params())
}

pub(super) fn session_with_company(
    date: CivilDate,
    mut company: engine::company::operations::OperatingCompanyConfig,
    seed: u64,
    shock_params: engine::company::ShockParams,
) -> GameSession {
    company.spec.listed_stock = Some(StockCode("600101".into()));
    GameSession::new(
        SessionSetup {
            stocks: vec![StockSpec {
                code: StockCode("600101".into()),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(1_000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
                tick: Money::from_cents(1),
                total_shares: company.spec.issued_shares,
                float_shares: 0,
            }],
            company_operations: Some(CompanyOperationsConfig {
                seed,
                shock_params,
                companies: vec![company],
            }),
            groups: Vec::new(),
            npcs: NpcSetup {
                retail_count: 0,
                inst_count: 0,
                hot_count: 0,
                retail_cash_median: Money::from_cents(10_000_000),
            },
            config: engine::GameConfig::proposed_defaults(),
            strategy_params: StrategyParams {
                retail: RetailParams {
                    arrival_rate: 0.0,
                    order_size_mean: 100,
                    chase_prob: 0.0,
                },
                inst: InstParams {
                    margin: 0.05,
                    order_size: 100,
                },
                hot: HotParams {
                    lookback: 3,
                    trend_threshold: 0.02,
                    order_size: 100,
                },
            },
            ticks_per_day: 6,
            auction_ticks: 0,
            closing_auction_ticks: 0,
            history_len: 5,
            t1_enabled: true,
            report_frequency: engine::information::ReportFrequency::Quarterly,
            float_allocation: FloatAllocation::random(),
            start_date: date,
            simulation_policy_id: engine::SIMULATION_POLICY_ID.into(),
        },
        1,
    )
    .unwrap()
}

#[test]
fn real_session_failure_history_is_saved_restored_and_publicly_queryable() {
    let date = d("2030-01-05");
    let mut session = session(date, true);
    let report = session.end_civil_day().unwrap();
    let saved = session.save().unwrap();
    let failures = saved.company_operations.payment_failures_on(date);
    assert!(!failures.is_empty());
    let mut published = 0;
    for event in report.events {
        if let Event::CompanyDisclosurePublished {
            publication_id,
            kind: CompanyDisclosureKind::Announcement,
            ..
        } = event
        {
            let announcement = saved
                .public_library
                .announcement(publication_id, report.disclosure_instant)
                .unwrap();
            if let ShockKind::PaymentFailure {
                what,
                amount,
                obligation_status,
            } = &announcement.event.kind
            {
                assert!(failures.iter().any(|failure| &failure.what == what
                    && &failure.amount == amount
                    && &failure.obligation_status == obligation_status));
                published += 1;
            }
        }
    }
    assert_eq!(published, failures.len());
    let restored = GameSession::restore(&saved).unwrap();
    let after = restored.save().unwrap();
    assert_eq!(after.company_operations.payment_failures_on(date), failures);
    assert_eq!(after.public_library.save(), saved.public_library.save());
}

#[test]
fn real_session_restore_rejects_payment_dates_outside_calendar_policy() {
    let date = d("2030-01-05");
    let mut session = session(date, true);
    session.end_civil_day().unwrap();
    let saved = session.save().unwrap();
    for invalid in ["1997-12-31", "2100-01-01"] {
        let mut value = serde_json::to_value(&saved).unwrap();
        let failures = value["company_operations"]["payment_failures"]
            .as_object_mut()
            .unwrap()
            .remove(&date.to_iso())
            .unwrap();
        value["company_operations"]["payment_failures"][invalid] = failures;
        let corrupt: SaveSlot = serde_json::from_value(value).unwrap();
        assert!(GameSession::restore(&corrupt).is_err());
    }
}

#[test]
fn session_year_end_failure_rolls_back_tax_and_depreciation_before_retry() {
    let date = d("2030-12-31");
    let mut session = session(date, false);
    if session.civil_clock().phase() == CivilPhase::IntradayTrading {
        for _ in 0..6 {
            session.step().unwrap();
        }
    }
    let original_clock = session.civil_clock().save();
    let mut exhausted = original_clock.clone();
    exhausted.next_due_seq = u32::MAX;
    *session.civil_clock_mut() =
        CivilClock::from_parts(date, &exhausted, CalendarExchange::Sse).unwrap();
    let before = serde_json::to_vec(&session.save().unwrap()).unwrap();
    assert!(session.end_civil_day().is_err());
    assert_eq!(
        serde_json::to_vec(&session.save().unwrap()).unwrap(),
        before
    );
    *session.civil_clock_mut() =
        CivilClock::from_parts(date, &original_clock, CalendarExchange::Sse).unwrap();
    session.end_civil_day().unwrap();
    let saved = session.save().unwrap();
    let books = saved
        .company_operations
        .industrial_books(&CompanyId("C-IND-A".into()))
        .unwrap();
    assert_eq!(
        books
            .books()
            .journal()
            .entries()
            .filter(|entry| entry.date == date
                && entry.kind == engine::accounting::BusinessKind::TaxAccrual)
            .count(),
        1
    );
    assert_eq!(
        books
            .assets()
            .get(&engine::accounting::FixedAssetCode("FA-1".into()))
            .unwrap()
            .remaining_months(),
        84
    );
}

"##;
const LEGACY_INDUSTRY_SESSIONS: &str = r##"
use super::fixtures::*;
use super::session_boundary::session_with_company;
use engine::company::operations::{
    CompanyOperations, CompanyOperationsConfig, OperatingCompanyConfig,
};
use engine::company::{CompanyKind, ShockKind, ShockParams};
use engine::session::{CompanyDisclosureKind, Event};

fn assert_actual_publication(company: OperatingCompanyConfig, seed: u64, params: ShockParams) {
    let date = d("2030-01-05");
    let id = company.spec.id.clone();
    let kind = company.spec.kind;
    let mut session = session_with_company(date, company, seed, params);
    let report = session.end_civil_day().unwrap();
    let saved = session.save().unwrap();
    let active = saved
        .company_operations
        .company(&id)
        .unwrap()
        .economy()
        .active();
    assert!(active.iter().any(|shock| shock.starts_on == date));
    assert!(active.iter().all(|shock| shock.kind.applies_to(kind)));
    let mut economic_announcements = Vec::new();
    for event in &report.events {
        if let Event::CompanyDisclosurePublished {
            publication_id,
            company,
            kind: CompanyDisclosureKind::Announcement,
            ..
        } = event
        {
            assert_eq!(company, &id);
            let announcement = saved
                .public_library
                .announcement(*publication_id, report.disclosure_instant)
                .unwrap();
            if !matches!(announcement.event.kind, ShockKind::PaymentFailure { .. }) {
                economic_announcements.push(&announcement.event.kind);
            }
        }
    }
    assert!(!economic_announcements.is_empty());
    assert!(economic_announcements
        .iter()
        .all(|shock| shock.applies_to(kind)));
    assert!(economic_announcements.iter().all(|shock| !matches!(
        shock,
        ShockKind::ProductionInterruption | ShockKind::AssetImpairmentSignal
    )));
    if kind == CompanyKind::Bank {
        assert!(economic_announcements
            .iter()
            .all(|shock| matches!(shock, ShockKind::CreditDeterioration)));
    }
}

#[test]
fn real_bank_session_publishes_nonempty_applicable_credit_material_not_production_shocks() {
    let date = d("2030-01-05");
    let company = bank_c(d("2027-12-31"));
    let mut params = quiet_params();
    params.company_candidate_bp = 10_000;
    let seed = (0..24)
        .find(|seed| {
            let mut ops = CompanyOperations::new(
                CompanyOperationsConfig {
                    seed: *seed,
                    shock_params: params.clone(),
                    companies: vec![company.clone()],
                },
                date,
            )
            .unwrap();
            ops.advance_civil_day(date).unwrap();
            !ops.company(&company.spec.id)
                .unwrap()
                .economy()
                .active()
                .is_empty()
        })
        .expect("short fixture seed must activate bank credit deterioration");
    assert_actual_publication(company, seed, params);
}

#[test]
fn real_insurance_session_publishes_nonempty_demand_material_not_production_shocks() {
    let mut company = insurance_c(d("2027-12-31"));
    if let engine::company::operations::FlowParams::Insurance(params) = &mut company.flow {
        params.daily_groups_base = 1;
        params.coverage_days = 2;
        params.claim_every_days = 1;
    }
    assert_actual_publication(company, 7, ShockParams::stress_parameters());
}

"##;
