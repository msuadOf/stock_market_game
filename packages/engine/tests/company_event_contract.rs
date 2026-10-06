include!("../test-support/simple_company.rs");

#[path = "company_operations/fixtures.rs"]
mod operations_fixture;

use engine::account::StockCode;
use engine::calendar::CivilDate;
use engine::company::{ActiveShock, ShockKind};
use engine::money::Money;
use engine::session::{
    CivilPhase, CompanyDisclosureKind, Event, FloatAllocation, GameSession, NpcSetup,
    SecurityCategory, SessionSetup, StockExchange, StockSpec,
};
use engine::strategy::{HotParams, InstParams, RetailParams, StrategyParams};
use engine::GameConfig;

const TICKS_PER_DAY: u64 = 12;

fn setup(start_date: &str) -> SessionSetup {
    SessionSetup {
        company_system: simple_company_fixture!(engine; ["600101"]),
        stocks: vec![StockSpec {
            code: StockCode("600101".to_string()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1_000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 10_000_000,
            float_shares: 0,
        }],
        npcs: NpcSetup {
            retail_count: 0,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::from_cents(10_000_000),
        },
        config: GameConfig::proposed_defaults(),
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
        ticks_per_day: TICKS_PER_DAY,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 5,
        t1_enabled: true,
        report_frequency: engine::information::ReportFrequency::Quarterly,
        float_allocation: FloatAllocation::random(),
        start_date: CivilDate::from_iso(start_date).expect("fixture date is valid"),
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_string(),
        dividend_tax_mode: engine::company::cash_dividend_tax::CashDividendTaxMode::Exempt,
    }
}

fn advance_to_next_report(session: &mut GameSession) -> Vec<Event> {
    for _ in 0..150 {
        if session.civil_clock().phase() == CivilPhase::IntradayTrading {
            for _ in 0..TICKS_PER_DAY {
                session.step().expect("healthy step");
            }
        }
        let report = session.end_civil_day().expect("civil day must settle");
        if report.events.iter().any(|event| {
            matches!(
                event,
                Event::CompanyDisclosurePublished {
                    kind: CompanyDisclosureKind::Report { .. },
                    ..
                }
            )
        }) {
            return report.events;
        }
    }
    panic!("fixture must reach a scheduled report within 150 civil days");
}

#[test]
fn scheduled_publication_event_is_lossless_public_and_precedes_civil_advance() {
    // Given: an authoritative session before its next scheduled public report.
    let mut session = GameSession::new(setup("2030-01-01"), 29).expect("fixture session is valid");

    // When: the civil/disclosure path reaches a newly due report.
    let events = advance_to_next_report(&mut session);

    // Then: the immutable library entry is publicly queryable and precedes one date transition.
    let publication_index = events
        .iter()
        .position(|event| {
            matches!(
                event,
                Event::CompanyDisclosurePublished {
                    kind: CompanyDisclosureKind::Report { .. },
                    ..
                }
            )
        })
        .expect("a newly due report must emit its publication event");
    let announcement_index = events
        .iter()
        .position(|event| matches!(event, Event::CompanyDisclosurePublished { .. }))
        .expect("the fixture must publish an announcement before its scheduled report");
    let Event::CompanyDisclosurePublished {
        publication_id: announcement_id,
        company: announcement_company,
        published_at: announcement_instant,
        kind: CompanyDisclosureKind::Announcement,
        ..
    } = &events[announcement_index]
    else {
        panic!("the first publication in this fixture must be an announcement");
    };
    assert!(announcement_index < publication_index);
    let saved = session.save().expect("healthy save");
    let announcement = saved
        .public_library
        .announcement(*announcement_id, *announcement_instant)
        .expect("preceding announcement must resolve in the immutable public library");
    assert_eq!(&announcement.company, announcement_company);
    assert_eq!(announcement.published_at, *announcement_instant);
    assert!(events
        .windows(2)
        .all(|pair| pair[0].seq() < pair[1].seq()));
    let advance_index = events
        .iter()
        .position(|event| matches!(event, Event::CivilDateAdvanced { .. }))
        .expect("a successful civil settlement must emit one date event");
    assert!(publication_index < advance_index);
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::CivilDateAdvanced { .. }))
            .count(),
        1
    );
    let Event::CompanyDisclosurePublished {
        publication_id,
        company,
        published_at,
        kind: CompanyDisclosureKind::Report { report_revision },
        ..
    } = &events[publication_index]
    else {
        panic!("expected a public report publication event");
    };
    let public = session
        .public_report_by_id(publication_id.value().to_string())
        .expect("published report must resolve through the public query surface");
    assert_eq!(public.id, publication_id.value().to_string());
    assert_eq!(public.company_id, company.0);
    assert_eq!(public.published_date, published_at.date().to_iso());
    assert_eq!(public.published_second_of_day, published_at.second_of_day());
    assert_eq!(public.version_sequence, report_revision.to_string());
    let event_json = serde_json::to_value(&events[publication_index]).expect("event serializes");
    let payload = &event_json["CompanyDisclosurePublished"];
    assert_eq!(payload.as_object().map(|value| value.len()), Some(5));
    assert!(payload.get("books").is_none());
    assert!(payload.get("journal").is_none());
    assert!(payload.get("beliefs").is_none());
}

#[test]
fn restored_closed_day_keeps_civil_event_sequence_identical() {
    // Given: identical restored and uninterrupted sessions at a closed civil-day boundary.
    let original = GameSession::new(setup("2030-01-01"), 31).expect("fixture session is valid");
    let bytes =
        serde_json::to_vec(&original.save().expect("healthy save")).expect("save serializes");
    let mut uninterrupted = original;
    let mut restored =
        GameSession::restore(&uninterrupted.save().expect("healthy save")).expect("save restores");

    // When: both authoritative sessions settle the same closed day.
    let original_report = uninterrupted.end_civil_day().expect("closed day settles");
    let restored_report = restored
        .end_civil_day()
        .expect("restored closed day settles");

    // Then: event bytes, sequence continuation, and resulting saves are identical.
    assert_eq!(
        serde_json::to_vec(&original_report.events).unwrap(),
        serde_json::to_vec(&restored_report.events).unwrap()
    );
    assert_eq!(original_report.events[0].seq(), 1);
    assert_eq!(
        serde_json::to_vec(&uninterrupted.save().expect("healthy save")).unwrap(),
        serde_json::to_vec(&restored.save().expect("healthy save")).unwrap()
    );
    assert_eq!(
        bytes,
        serde_json::to_vec(
            &GameSession::restore(&bytes_to_save(&bytes))
                .unwrap()
                .save()
                .expect("healthy save")
        )
        .unwrap()
    );
}

#[test]
fn announcement_event_follows_successful_immutable_library_insertion() {
    use engine::calendar::CalendarExchange;
    use engine::company::operations::CompanyOperations;
    use engine::session::{CivilClock, DayEndDisclosureCtx, DisclosureDispatch};

    let date = engine::CivilDate::from_iso("2030-01-01").unwrap();
    let prior = date.prev().unwrap();
    let company = engine::company::CompanyId("C-IND-A".into());
    let config = operations_fixture::four_company_config(
        41,
        operations_fixture::quiet_params(),
        prior,
    );
    let mut operations = CompanyOperations::new(config, date).unwrap();
    operations
        .apply_company_shock(
            &company,
            ActiveShock {
                kind: ShockKind::ContractWon,
                amplitude_bp: 1_000,
                starts_on: date,
                expires_on: date.next().unwrap(),
            },
        )
        .unwrap();
    let before = serde_json::to_value(&operations).unwrap();
    let mut restored: CompanyOperations = serde_json::from_value(before.clone()).unwrap();
    assert_eq!(serde_json::to_value(&restored).unwrap(), before);
    assert!(restored
        .company(&company)
        .unwrap()
        .economy()
        .active()
        .iter()
        .any(|shock| shock.kind == ShockKind::ContractWon && shock.amplitude_bp == 1_000));
    restored.advance_civil_day(date).unwrap();

    let mut clock = CivilClock::new(date, CalendarExchange::Sse).unwrap();
    let civil = clock.end_day(date).unwrap();
    let mut closing = engine::accounting::closing::ClosingEngine::new();
    let mut library = engine::information::PublicLibrary::new();
    let mut dispatch = DisclosureDispatch::new(Some(civil.disclosure_instant));
    let outcome = dispatch.run_day_end(DayEndDisclosureCtx {
        report_frequency: engine::information::ReportFrequency::Quarterly,
        groups: &[],
        report: &civil,
        ops: &restored,
        closing: &mut closing,
        library: &mut library,
    }).unwrap();
    assert_eq!(outcome.announcements_published.len(), 1);
    let saved = library.save();
    let announcement = saved.announcements.first().unwrap();
    assert_eq!(announcement.company, company);
    assert_eq!(announcement.occurred_on, date);
    assert_eq!(announcement.published_at, civil.disclosure_instant);
    let restored_library = engine::information::PublicLibrary::from_parts(saved).unwrap();
    assert_eq!(restored_library.save(), library.save());
}

fn bytes_to_save(bytes: &[u8]) -> engine::session::SaveSlot {
    engine::session::decode_save_slot(bytes, &Default::default()).expect("save decodes")
}

#[test]
fn civil_report_refresh_validates_and_reconnect_resolves_publication() {
    use engine::session::protocol::ProtocolSession;
    let mut session = ProtocolSession::new(setup("2030-01-01"), 29).unwrap();
    for _ in 0..150 {
        if session.game().civil_clock().phase() == CivilPhase::IntradayTrading {
            for _ in 0..TICKS_PER_DAY {
                session.step_frame().unwrap();
            }
        }
        let update = session.end_civil_day_update().unwrap();
        update.validate().unwrap();
        for event in &update.events {
            if let Event::CompanyDisclosurePublished {
                publication_id,
                kind: CompanyDisclosureKind::Report { .. },
                ..
            } = event
            {
                let id = publication_id.value().to_string();
                assert!(update.refresh.public_publication_ids.contains(&id));
                let reconnected = GameSession::restore(&session.game().save().unwrap()).unwrap();
                assert!(reconnected.public_report_by_id(id).is_ok());
                return;
            }
        }
    }
    panic!("scheduled report not reached");
}
