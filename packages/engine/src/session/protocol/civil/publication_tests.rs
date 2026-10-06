use crate::company::{ActiveShock, CompanyId, ShockKind};
use crate::session::{CompanyDisclosureKind, Event, GameSession};

#[test]
fn confirmed_low_level_announcement_consumer_exposes_only_public_index_after_reconnect() {
    let session = GameSession::new(setup(), 41).unwrap();
    let save = session.save().unwrap();
    let company = CompanyId("C-600101".into());
    let date = save.civil_clock.current_date;
    let (library, confirmed, fact_report) = crate::session::company_assembly::financial_fixture_tests::confirmed_announcement_fixture(
            &session, ActiveShock {
                kind: ShockKind::ContractWon,
                amplitude_bp: 1000,
                starts_on: date,
                expires_on: date.next().unwrap(),
            },
        );
    let mut session = GameSession::restore(&save).unwrap();
    let mut with_acquisition = GameSession::restore(&save).unwrap();
    session.state.library = std::sync::Arc::new(library.clone());
    with_acquisition.state.library = std::sync::Arc::new(library);
    let public_id = *with_acquisition
        .state
        .library
        .all_publication_ids()
        .unwrap()
        .first()
        .unwrap();
    let owner = *with_acquisition
        .state
        .belief_participants
        .keys()
        .next()
        .unwrap();
    let mut private = crate::information::NpcInformationState::new(owner);
    private
        .record_acquisition(
            owner,
            &with_acquisition.state.library,
            public_id,
            crate::calendar::CivilInstant::new(date, 0).unwrap(),
        )
        .unwrap();
    *with_acquisition
        .state
        .belief_participants
        .get_mut(&owner)
        .unwrap()
        .information_mut() = private;
    let update = update_with_confirmed_announcement(&mut session, confirmed.clone(), fact_report.clone());
    let acquired_update = update_with_confirmed_announcement(&mut with_acquisition, confirmed, fact_report);
    assert_eq!(
        update.refresh.public_publication_ids,
        acquired_update.refresh.public_publication_ids
    );
    update.validate().unwrap();
    let reconnected = GameSession::restore(&session.save().unwrap()).unwrap();
    let public = &reconnected.state.library;
    let mut announcements = 0;
    for event in &update.events {
        if let Event::CompanyDisclosurePublished {
            publication_id,
            published_at,
            kind: CompanyDisclosureKind::Announcement,
            ..
        } = event
        {
            announcements += 1;
            assert!(update
                .refresh
                .public_publication_ids
                .contains(&publication_id.value().to_string()));
            let item = public.announcement(*publication_id, *published_at).unwrap();
            assert_eq!(item.company, company);
            assert!(public
                .announcements_for_company(&company, *published_at)
                .iter()
                .any(|entry| entry.id == *publication_id));
        }
    }
    assert!(announcements > 0);
    let ids = public.all_publication_ids().unwrap();
    assert_eq!(
        update.refresh.public_publication_ids,
        ids.iter()
            .map(|id| id.value().to_string())
            .collect::<Vec<_>>()
    );
    for id in ids {
        let now = crate::calendar::CivilInstant::new(reconnected.civil_date(), 0).unwrap();
        assert!(public.report(id, now).is_ok() || public.announcement(id, now).is_ok());
    }
}

fn update_with_confirmed_announcement(
    session: &mut GameSession,
    confirmed: crate::session::DayEndDisclosures,
    mut fact_report: crate::session::CivilDayEndReport,
) -> super::CivilUpdate {
    let mut update = session.end_civil_day_update(&[]).unwrap();
    session.record_company_disclosure_events(&mut fact_report, confirmed).unwrap();
    update.events.extend(fact_report.events);
    update.facts = super::attach_facts(&update.events).unwrap();
    update.seq_to = session.seq();
    update.refresh.snapshot = session.snapshot();
    update.validate().unwrap();
    update
}

pub(super) fn setup() -> crate::SessionSetup {
    use crate::*;
    SessionSetup {
        company_system: simple_company_fixture!(crate; ["600101"]),
        stocks: vec![StockSpec {
            code: StockCode("600101".into()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.1,
            tick: Money::from_cents(1),
            total_shares: 1_000_000,
            float_shares: 0,
        }],
        npcs: NpcSetup {
            retail_count: 1,
            inst_count: 1,
            hot_count: 0,
            retail_cash_median: Money::ZERO,
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
        ticks_per_day: 12,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 5,
        t1_enabled: true,
        report_frequency: crate::information::ReportFrequency::Quarterly,
        float_allocation: FloatAllocation::random(),
        start_date: CivilDate::from_iso("2030-01-01").unwrap(),
        simulation_policy_id: SIMULATION_POLICY_ID.into(),
        dividend_tax_mode: crate::company::cash_dividend_tax::CashDividendTaxMode::Exempt,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
    }
}
