use super::fixtures::*;
use engine::account::StockCode;
use engine::calendar::CivilDate;
use engine::company::operations::CompanyOperationsConfig;
use engine::company::{CompanyId, ShockKind};
use engine::money::Money;
use engine::session::{
    CivilPhase, CompanyDisclosureKind, Event, FloatAllocation, GameSession, NpcSetup,
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
                catalog: catalog_for([&company]),
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
            if let engine::company::ShockKind::PaymentFailure {
                what,
                amount,
                obligation_status,
            } = match &announcement.content {
                engine::information::AnnouncementContent::Shock(event) => &event.kind,
                _ => continue,
            }
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
fn restored_run_matches_continuous_run_for_company_facts_across_rest_days() {
    // 恢复「恰好一次」的真实账务断言（公司 due 单一 authority）：工商赊销
    // 跨休市日到期，连续运行与中途存读档运行在相同自然日序列下，公司域
    // （journal/scheduler/RNG/账套）字节一致；应收到期回款恰好多条、不因
    // 恢复重复或丢失。
    fn settle_day(target: &mut GameSession) {
        if target.civil_clock().phase() == CivilPhase::IntradayTrading {
            for _ in 0..6 {
                target.step().expect("healthy step");
            }
        }
        target.end_civil_day().expect("civil day settles");
    }
    fn receivable_collections(save: &SaveSlot, start: CivilDate) -> usize {
        save.company_operations
            .industrial_books(&CompanyId(0))
            .expect("industrial company present")
            .books()
            .journal()
            .entries()
            .filter(|entry| entry.date >= start
                && entry.kind == engine::accounting::BusinessKind::ReceivableCollection)
            .count()
    }

    let start = d("2030-01-04");
    let mut continuous = session(start, false);
    for _ in 0..6 {
        settle_day(&mut continuous);
    }
    let final_continuous = continuous.save().expect("healthy save");

    let mut resumed = session(start, false);
    settle_day(&mut resumed);
    settle_day(&mut resumed);
    let mid = resumed.save().expect("healthy mid save");
    let mut restored = GameSession::restore(&mid).expect("mid-weekend save restores");
    for _ in 0..4 {
        settle_day(&mut restored);
    }
    let final_restored = restored.save().expect("healthy save");

    assert_eq!(
        serde_json::to_vec(&final_continuous.company_operations).unwrap(),
        serde_json::to_vec(&final_restored.company_operations).unwrap(),
        "连续与恢复运行的公司域必须字节一致（分录、到期派发、RNG 全量）"
    );
    assert_eq!(
        final_continuous.civil_clock, final_restored.civil_clock,
        "两条路径的自然日时钟终态一致"
    );
    let collections = receivable_collections(&final_restored, start);
    assert!(
        collections > 0,
        "窗口内必须发生真实应收到期回款（赊销信用期 3 日）"
    );
    assert_eq!(
        receivable_collections(&final_continuous, start),
        collections,
        "回款次数不因恢复重复或丢失"
    );
}

fn assert_due_free_month_end_still_operates_and_discloses(mut target: GameSession) {
    let date = target.civil_date();
    assert_eq!(target.civil_clock().phase(), CivilPhase::ClosedDay);
    let before = target.save().unwrap();
    let entries_before = before.company_operations.company(&CompanyId(0)).unwrap()
        .books().books().journal().entry_count();
    let closed_before = serde_json::to_value(&before.closing_registry).unwrap()["versions"]
        .as_array().unwrap().len();
    let report = target.end_civil_day().expect("没有当日 due 仍须完成经营日/月末封账/披露");
    let after = target.save().unwrap();
    assert_eq!(report.settled_date, date);
    assert_eq!(after.company_operations.next_expected_date(), date.next().unwrap());
    assert!(after.company_operations.company(&CompanyId(0)).unwrap().books().books()
        .journal().entry_count() > entries_before, "每日经营不因没有当日 due 而跳过");
    let closed = serde_json::to_value(&after.closing_registry).unwrap();
    assert!(closed["versions"].as_array().unwrap().len() > closed_before, "月末封账必须产生真实期间版本");
    assert_eq!(after.disclosures.announced_through(), Some(date), "18:00 披露相位照常完成");
    assert_eq!(target.tick(), 0, "休市日经营/封账不伪造市场 tick");
}

#[test]
fn none_company_due_does_not_skip_daily_flows_month_close_or_disclosure() {
    let date = d("2030-03-31");
    let as_of = d("2027-12-31");
    let target = session_with_company(date, insurance_c(as_of), 1, quiet_params());
    assert_eq!(target.save().unwrap().company_operations.next_due_date(), None,
        "保险 fixture 没有计息/合同到期队列，但仍有每日经营");
    assert_due_free_month_end_still_operates_and_discloses(target);
}

#[test]
fn future_company_due_does_not_skip_daily_flows_month_close_or_disclosure() {
    let date = d("2030-03-31");
    let as_of = d("2027-12-31");
    let source = session_with_company(date, industrial_b(as_of), 1, quiet_params());
    let mut value = serde_json::to_value(source.save().unwrap()).unwrap();
    // 当前可编辑日级事实中取消当日 due，保留将来到期合同；只改唯一 authority，
    // 不再需要同步修改一份时钟镜像。
    value["company_operations"]["scheduler"]["pending"].as_array_mut().unwrap()
        .retain(|due| due["due_date"].as_str().unwrap() > "2030-03-31");
    let save: SaveSlot = serde_json::from_value(value).unwrap();
    assert!(save.company_operations.next_due_date().unwrap() > date);
    let target = GameSession::restore(&save).expect("合法未来 due 不依赖公司镜像即可恢复");
    assert_due_free_month_end_still_operates_and_discloses(target);
}

#[test]
fn company_assembly_rejects_due_beyond_frozen_calendar_ceiling() {
    use engine::company::operations::FlowParams;
    use engine::session::SessionError;
    let date = d("2030-01-05");
    let source = session_with_company(date, bank_c(d("2027-12-31")), 1, quiet_params());
    let mut setup = source.save().unwrap().setup;
    if let FlowParams::Bank(params) = &mut setup.company_operations.as_mut().unwrap().companies[0].flow {
        params.loan_term_days = d("2100-01-01").days_since(d("2028-01-01"));
    }
    match GameSession::new(setup, 1) {
        Err(SessionError::CivilClock(engine::session::CivilClockError::Calendar(
            engine::calendar::CalendarError::AfterRuntimeCeiling { date, .. }
        ))) => assert!(date > d("2099-12-31")),
        Ok(_) => panic!("装配时的越界公司 due 不得通过"),
        Err(other) => panic!("装配须按冻结日历 horizon 拒绝，实际：{other}"),
    }
}

#[test]
fn day_end_new_due_beyond_calendar_ceiling_rolls_back_every_domain() {
    use engine::session::SessionError;
    let date = d("2099-12-28");
    let mut target = session_with_company(date, industrial_b(d("2096-12-31")), 1, quiet_params());
    for _ in 0..6 { target.step().unwrap(); }
    target.end_civil_day().expect("28 日新增三天后回款仍在 2099 年内");
    assert_eq!(target.civil_date(), d("2099-12-29"));
    for _ in 0..6 { target.step().unwrap(); }
    let before = serde_json::to_vec(&target.save().unwrap()).unwrap();
    match target.end_civil_day() {
        Err(SessionError::CivilClock(engine::session::CivilClockError::Calendar(
            engine::calendar::CalendarError::AfterRuntimeCeiling { date, .. }
        ))) => assert_eq!(date, d("2100-01-01")),
        Ok(_) => panic!("29 日新增三天后回款已越界，日结必须整体失败"),
        Err(other) => panic!("新增 due 应按冻结日历 horizon 拒绝，实际：{other}"),
    }
    assert_eq!(serde_json::to_vec(&target.save().unwrap()).unwrap(), before,
        "越界 due 失败不得提交公司分录、RNG、时钟、封账或披露的任何半成品");
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
    // 注入经营日末尾的显式失败：调度器 id 序列耗尽（单一 authority）使当日
    // 经营在完成全部过账（含年末税费/折旧）后、排队次日滚动利息时类型化失败；
    // 整个日结回滚，税费/折旧不残留半成品。
    let good_save = session.save().unwrap();
    let mut exhausted_value = serde_json::to_value(&good_save).unwrap();
    exhausted_value["company_operations"]["scheduler"]["next_seq"] = serde_json::json!(u64::MAX);
    let exhausted: SaveSlot = serde_json::from_value(exhausted_value).unwrap();
    let mut exhausted_session = GameSession::restore(&exhausted).unwrap();
    let before = serde_json::to_vec(&exhausted_session.save().unwrap()).unwrap();
    assert!(exhausted_session.end_civil_day().is_err());
    assert_eq!(
        serde_json::to_vec(&exhausted_session.save().unwrap()).unwrap(),
        before,
        "失败日结必须整体回滚，已过账的税费/折旧不得残留"
    );

    // 健康会话对同一天重试：年末税费恰好一次入账，折旧照常结转。
    session.end_civil_day().unwrap();
    let saved = session.save().unwrap();
    let books = saved
        .company_operations
        .industrial_books(&CompanyId(0))
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
