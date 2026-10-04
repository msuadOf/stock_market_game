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
            if let ShockKind::PaymentFailure { what, amount } = &announcement.event.kind {
                assert!(failures
                    .iter()
                    .any(|failure| &failure.what == what && &failure.amount == amount));
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
