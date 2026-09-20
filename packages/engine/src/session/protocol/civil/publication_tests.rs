use crate::company::{ActiveShock, CompanyId, ShockKind};
use crate::session::{CompanyDisclosureKind, Event, GameSession};

#[test]
fn real_announcement_barrier_exposes_only_public_index_after_reconnect() {
    let session = GameSession::new(setup(), 41).unwrap();
    let mut save = session.save().unwrap();
    let company = CompanyId("C-600101".into());
    let date = save.civil_clock.current_date;
    save.company_operations
        .apply_company_shock(
            &company,
            ActiveShock {
                kind: ShockKind::ContractWon,
                amplitude_bp: 1000,
                starts_on: date,
                expires_on: date.next().unwrap(),
            },
        )
        .unwrap();
    let mut session = GameSession::restore(&save).unwrap();
    let mut with_acquisition = GameSession::restore(&save).unwrap();
    let public_id = *with_acquisition
        .library
        .all_publication_ids()
        .unwrap()
        .first()
        .unwrap();
    let owner = *with_acquisition.information.keys().next().unwrap();
    let mut private = crate::information::NpcInformationState::new(owner);
    private
        .record_acquisition(
            owner,
            &with_acquisition.library,
            public_id,
            crate::calendar::CivilInstant::new(date, 0).unwrap(),
        )
        .unwrap();
    with_acquisition.information.insert(owner, private);
    let update = session.end_civil_day_update(&[]).unwrap();
    let acquired_update = with_acquisition.end_civil_day_update(&[]).unwrap();
    assert_eq!(
        update.refresh.public_publication_ids,
        acquired_update.refresh.public_publication_ids
    );
    update.validate().unwrap();
    let reconnected = GameSession::restore(&session.save().unwrap()).unwrap();
    let public = &reconnected.library;
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

pub(super) fn setup() -> crate::SessionSetup {
    use crate::*;
    SessionSetup {
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
                tick_cents: 1,
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
        float_allocation: FloatAllocation::Random,
        start_date: CivilDate::from_iso("2030-01-01").unwrap(),
        simulation_policy_id: SIMULATION_POLICY_ID_V1.into(),
    }
}
