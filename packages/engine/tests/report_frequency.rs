use engine::calendar::CivilDate;
use engine::information::{
    scheduled_instant, MonthlyReportDelay, MonthlyReportPreset, MonthlyReportSchedule,
    ReportFrequency, ScheduledReportKind,
};

#[path = "publications/session_fixture.rs"]
mod session_fixture;

#[test]
fn monthly_frequency_requires_user_selected_schedule_and_optional_delay() {
    let custom = serde_json::json!({"Monthly": {"schedule": {
        "Custom": {"day": 2, "second_of_day": 37800, "delay": {"Uniform": {"max_days": 2}}}
    }}});
    let parsed: ReportFrequency = serde_json::from_value(custom.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), custom);
    assert!(serde_json::from_value::<ReportFrequency>(serde_json::json!("Monthly")).is_err());
    assert_eq!(
        serde_json::from_value::<ReportFrequency>(serde_json::json!("Quarterly")).unwrap(),
        ReportFrequency::Quarterly
    );
}

#[test]
fn monthly_reporting_adds_closed_months_without_replacing_statutory_reports() {
    let frequency = ReportFrequency::Monthly {
        schedule: MonthlyReportSchedule::Preset {
            preset: MonthlyReportPreset::FirstDayEvening,
            delay: MonthlyReportDelay::None,
        },
    };
    let kinds = frequency.scheduled_kinds();
    assert_eq!(kinds.len(), 16);
    for kind in ScheduledReportKind::ALL {
        assert!(kinds.contains(&kind));
    }
    for month in 1..=12 {
        let kind = ScheduledReportKind::Monthly { month };
        assert_eq!(kind.landing_period(2030).unwrap().month(), month);
        assert_eq!(
            kind.report_kind(),
            engine::accounting::reports::ReportKind::Monthly
        );
        let instant = frequency
            .scheduled_instant(kind, 2030, 7, &engine::company::CompanyId("company".into()))
            .unwrap();
        let period_end = if month == 12 {
            CivilDate::from_ymd(2030, 12, 31).unwrap()
        } else {
            CivilDate::from_ymd(2030, month + 1, 1)
                .unwrap()
                .prev()
                .unwrap()
        };
        assert!(instant.date() > period_end);
        assert_eq!(instant.second_of_day(), 18 * 3600);
    }
    assert_eq!(
        ReportFrequency::Quarterly.scheduled_kinds(),
        ScheduledReportKind::ALL
    );
    assert!(scheduled_instant(ScheduledReportKind::Monthly { month: 0 }, 2030, 0).is_err());
    assert!(scheduled_instant(ScheduledReportKind::Monthly { month: 13 }, 2030, 0).is_err());
}

#[test]
fn monthly_delay_is_seed_company_period_bound_and_does_not_mutate_on_retry() {
    let schedule = MonthlyReportSchedule::Custom {
        day: 2,
        second_of_day: 37800,
        delay: MonthlyReportDelay::Uniform { max_days: 2 },
    };
    let company = engine::company::CompanyId("company".into());
    let period = engine::accounting::AccountingPeriod::from_ymd(2030, 12).unwrap();
    let delay = schedule.delay_days(42, &company, period).unwrap();
    let instant = schedule.instant(period, delay).unwrap();
    for _ in 0..20 {
        assert_eq!(schedule.delay_days(42, &company, period).unwrap(), delay);
        assert_eq!(schedule.instant(period, delay).unwrap(), instant);
    }
    assert!(delay <= 2);
    assert_eq!(instant.second_of_day(), 37800);
    assert_eq!(instant.date().year(), 2031);
    let delays = (0..40)
        .map(|seed| schedule.delay_days(seed, &company, period).unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(delays, std::collections::BTreeSet::from([0, 1, 2]));
    assert!(schedule.instant(period, 3).is_err());
    for (day, second_of_day, max_days) in
        [(0, 0, 1), (29, 0, 1), (1, 86400, 1), (1, 0, 0), (1, 0, 32)]
    {
        assert!(MonthlyReportSchedule::Custom {
            day,
            second_of_day,
            delay: MonthlyReportDelay::Uniform { max_days }
        }
        .validate()
        .is_err());
    }
}

#[test]
fn custom_monthly_report_is_public_at_intraday_tick_not_delayed_to_day_end() {
    let start = CivilDate::from_ymd(2030, 1, 2).unwrap();
    let mut setup = session_fixture::civil_setup(start);
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.report_frequency = ReportFrequency::Monthly {
        schedule: MonthlyReportSchedule::Custom {
            day: 2,
            second_of_day: 10 * 3600 + 30 * 60,
            delay: MonthlyReportDelay::None,
        },
    };
    let mut session = engine::GameSession::new(setup, 42).unwrap();
    let query = engine::company::PublicReportQuery {
        company_id: "C-600101".into(),
        cursor: None,
        page_size: Some(100),
    };
    for _ in 0..29 {
        session.step().unwrap();
    }
    assert!(!session
        .query_public_reports(&query)
        .unwrap()
        .reports
        .iter()
        .any(|report| report.period == "2029-12-31"
            && matches!(report.kind, engine::company::PublicReportKind::Monthly)));
    let events = session.step().unwrap();
    assert!(events.iter().any(|event| matches!(event, engine::Event::CompanyDisclosurePublished { published_at, .. } if published_at.date() == start && published_at.second_of_day() == 37800)));
    let published = session
        .query_public_reports(&query)
        .unwrap()
        .reports
        .into_iter()
        .find(|report| {
            report.period == "2029-12-31"
                && matches!(report.kind, engine::company::PublicReportKind::Monthly)
        })
        .unwrap();
    assert_eq!(
        session
            .public_report_by_id(published.id.clone())
            .unwrap()
            .period,
        "2029-12-31"
    );
    assert_eq!(session.civil_date(), start);
    assert!(!session
        .step()
        .unwrap()
        .iter()
        .any(|event| matches!(event, engine::Event::CompanyDisclosurePublished { .. })));
    for _ in 31..120 {
        session.step().unwrap();
    }
    assert_eq!(
        session.public_report_by_id(published.id).unwrap().period,
        "2029-12-31"
    );
}

#[test]
fn subscribed_institution_acquires_monthly_report_without_treating_it_as_annual() {
    let mut setup = session_fixture::civil_setup(CivilDate::from_ymd(2030, 1, 2).unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 1;
    setup.npcs.hot_count = 0;
    setup.report_frequency = ReportFrequency::Monthly {
        schedule: MonthlyReportSchedule::Custom {
            day: 2,
            second_of_day: 37800,
            delay: MonthlyReportDelay::None,
        },
    };
    let mut session = engine::GameSession::new(setup, 42).unwrap();
    for _ in 0..29 {
        session.step().unwrap();
    }
    let events = session.step().unwrap();
    let publication = events
        .iter()
        .find_map(|event| match event {
            engine::Event::CompanyDisclosurePublished {
                publication_id,
                published_at,
                ..
            } if published_at.date() == session.civil_date()
                && published_at.second_of_day() == 37800 =>
            {
                Some(*publication_id)
            }
            _ => None,
        })
        .unwrap();
    let saved = session.save().unwrap();
    let acquired = saved.information_states.get(&engine::AccountId(1)).unwrap();
    let record = acquired
        .records_for_company(&engine::company::CompanyId("C-600101".into()))
        .iter()
        .find(|record| record.id == publication)
        .unwrap();
    assert_eq!(record.observed_at.date(), session.civil_date());
    assert_eq!(record.observed_at.second_of_day(), 37800);
    let book = saved.belief_books.get(&engine::AccountId(1)).unwrap();
    let belief = book.entry(&engine::StockCode("600101".into())).unwrap();
    assert!(belief.used_report_ids.contains(&record.id));
    assert!(
        matches!(belief.last_cause.as_ref().unwrap().cause, engine::strategy::BeliefCause::NewMaterial { report } if report == record.id)
    );
}

#[test]
fn holiday_late_monthly_report_is_saved_and_restored_without_republication() {
    let start = CivilDate::from_ymd(2030, 1, 1).unwrap();
    let mut setup = session_fixture::civil_setup(start);
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.ticks_per_day = 2;
    setup.report_frequency = ReportFrequency::Monthly {
        schedule: MonthlyReportSchedule::Custom {
            day: 1,
            second_of_day: 21 * 3600,
            delay: MonthlyReportDelay::None,
        },
    };
    let mut session = engine::GameSession::new(setup, 42).unwrap();
    let before_tick = session.tick();
    let report = session.end_civil_day().unwrap();
    assert_eq!(session.tick(), before_tick);
    let publication_ids = report
        .events
        .iter()
        .filter_map(|event| match event {
            engine::Event::CompanyDisclosurePublished {
                publication_id,
                published_at,
                ..
            } if published_at.date() == start && published_at.second_of_day() == 75600 => {
                Some(*publication_id)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(publication_ids.len(), 1);
    let saved = session.save().unwrap();
    assert_eq!(
        saved
            .disclosures
            .published_through()
            .unwrap()
            .second_of_day(),
        86399
    );
    let mut restored = engine::GameSession::restore(&saved).unwrap();
    for _ in 0..2 {
        restored.step().unwrap();
    }
    let report = restored.end_civil_day().unwrap();
    assert!(!report.events.iter().any(|event| matches!(event, engine::Event::CompanyDisclosurePublished { publication_id, .. } if publication_ids.contains(publication_id))));
    let query = engine::company::PublicReportQuery {
        company_id: "C-600101".into(),
        cursor: None,
        page_size: Some(100),
    };
    let december = restored
        .query_public_reports(&query)
        .unwrap()
        .reports
        .into_iter()
        .filter(|report| {
            report.period == "2029-12-31"
                && matches!(report.kind, engine::company::PublicReportKind::Monthly)
        })
        .count();
    assert_eq!(december, 1);
}
