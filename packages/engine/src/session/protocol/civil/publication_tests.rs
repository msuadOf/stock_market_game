use crate::company::CompanyId;
use crate::session::{CompanyDisclosureKind, Event, GameSession};

#[test]
fn confirmed_low_level_announcement_consumer_exposes_only_public_index_after_reconnect() {
    // Simple 管线重建：旧 CompanyOperations 冲击公告 fixture 的报告来源
    // （SimulationAccounting + fixture 自建发行人）已被 Simple-only 存档契约
    // 拒绝，本用例改用真实分红方案公告（flat_dividend_fixture 全链）验证
    // 同一意图——public index 只由已发布事实构成，私有获知不泄漏。
    let mut setup = setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    let holders = [(crate::AccountId(0), "player-lot", 100_u64)];
    let (mut session, ..) = crate::session::dividend_tax_mode_tests::flat_dividend_fixture(
        setup.clone(),
        1,
        &holders,
    );
    let (mut with_acquisition, ..) = crate::session::dividend_tax_mode_tests::flat_dividend_fixture(
        setup,
        1,
        &holders,
    );
    let company = CompanyId("C-600101".into());
    // 01-02 会话与日终；01-03 会话跑完后由 end_civil_day_update 执行当日日终，
    // 在 18:00 相位发布分红公告（真实披露链，announced_on=01-03）。
    for game in [&mut session, &mut with_acquisition] {
        for _ in 0..game.state.setup.ticks_per_day {
            game.step().expect("healthy step");
        }
        game.end_civil_day().expect("healthy civil day end");
    }
    let closing_frames = |game: &mut GameSession| {
        (0..game.state.setup.ticks_per_day)
            .map(|_| game.step_frame().expect("真实 closing session frame"))
            .collect::<Vec<_>>()
    };
    let frames = closing_frames(&mut session);
    let acquired_frames = closing_frames(&mut with_acquisition);
    let announced_update = session.end_civil_day_update(&frames).unwrap();
    announced_update.validate().unwrap();
    with_acquisition.end_civil_day_update(&acquired_frames).unwrap();
    let announcement_ids: Vec<_> = announced_update
        .events
        .iter()
        .filter_map(|event| match event {
            Event::CompanyDisclosurePublished {
                publication_id,
                kind: CompanyDisclosureKind::Announcement,
                ..
            } => Some(publication_id.clone()),
            _ => None,
        })
        .collect();
    assert!(
        !announcement_ids.is_empty(),
        "分红公告必须在 announced_on 日终真实发布"
    );

    // 私有获知发生在公告已发布之后，不得改变后续 public index。
    let public_id = *announcement_ids.first().unwrap();
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
            crate::calendar::CivilInstant::new(with_acquisition.civil_date(), 0).unwrap(),
        )
        .unwrap();
    *with_acquisition
        .state
        .belief_participants
        .get_mut(&owner)
        .unwrap()
        .information_mut() = private;

    // 开市日日终更新必须消费完整真实 TickFrame，不能用空批次冒充收盘。
    let frames = closing_frames(&mut session);
    let acquired_frames = closing_frames(&mut with_acquisition);
    let update = session.end_civil_day_update(&frames).unwrap();
    let acquired_update = with_acquisition.end_civil_day_update(&acquired_frames).unwrap();
    assert_eq!(
        update.refresh.public_publication_ids,
        acquired_update.refresh.public_publication_ids
    );
    assert!(update
        .refresh
        .public_publication_ids
        .contains(&public_id.value().to_string()));
    update.validate().unwrap();
    let reconnected = GameSession::restore(&session.save().unwrap()).unwrap();
    let public = &reconnected.state.library;
    let published_at = crate::calendar::CivilInstant::new(
        crate::CivilDate::from_iso("2030-01-03").unwrap(),
        64800,
    )
    .unwrap();
    for publication_id in &announcement_ids {
        let item = public.announcement(*publication_id, published_at).unwrap();
        assert_eq!(item.company, company);
        // 公司索引与 by-id/global 扫描是独立路径，恢复后必须分别锁定真实公告可见性。
        assert!(public
            .announcements_for_company(&company, published_at)
            .iter()
            .any(|entry| entry.id == *publication_id));
    }
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
        dividend_tax_mode: crate::company::cash_dividend_tax::CashDividendTaxMode::FlatWithholding,
        flat_withholding_bp: Some(1000),
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
        par_value_per_share: crate::Money::from_cents(100),
        auto_corporate_foundation: false,
    }
}
