use super::*;
#[path = "../../information_fixture.rs"]
mod fixture;
use crate::calendar::CivilInstant;
use crate::runtime::protocol::{InformationPrepareError, InformationPreparePayload};
use crate::{AccountId, StockId};
use fixture::*;

fn state(ids: &[AccountId]) -> GroupState {
    let mut s = GroupState::new(
        2,
        vec![GroupAccount::new(100_000, 2); ids.len()],
        8,
        100,
        Arc::new(GameConfig::proposed_defaults()),
        100,
        None,
        Vec::new(),
        Box::new(|_, _| TickPlan::default()),
    );
    s.bind_calendar(calendar_source(), calendar_bindings());
    s.belief_states = ids.iter().map(|&id| Some(personal(id))).collect();
    s.belief_dirty = vec![false; ids.len()];
    s.root_projection = Some(projection(ids.to_vec()));
    s
}
/// 合成机制 fixture 迁移到真实静点；只重建可编辑恢复事实，不伪称发生了市场 tick。
fn set_observation_phase(s: &mut GroupState, p: &mut InformationPreparePayload, through: CivilInstant) {
    let day = calendar_source().day(through.date()).unwrap();
    let tick = day.day_model().shared_sessions_before_date() * 100;
    s.cur_tick = tick;
    s.committed_tick = tick;
    let point = p.observation.as_mut().unwrap();
    point.through = through;
    point.clock = clock_at(through, tick);
    point.as_of_anchors = point.clock.anchors().unwrap();
    Arc::make_mut(&mut point.market).tick = tick;
}

#[test]
fn info_01_empty_ops_prepare_is_overlay_abort_preserves_every_private_field() {
    let mut s = state(&[AccountId(71)]);
    s.accounts[0] = Arc::new({
        let mut a = (*s.accounts[0]).clone();
        a.pending_sell_qty[0] = 100;
        a
    });
    let mut p = payload(true);
    let publication = announcement(&mut p, 0);
    let before = s.belief_states.clone();
    let library = s.library.clone();
    let old_facts = facts(8000);
    s.install_initial_company_facts(old_facts.clone()).unwrap();
    s.belief_dirty[0] = true;
    s.belief_pending_dirty.push(0);
    let candidate = s
        .prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(p.clone()))
        .unwrap();
    let info = candidate.information.unwrap();
    assert_eq!(info.personals.len(), 1);
    assert_eq!(info.personals[0].1, AccountId(71));
    assert_eq!(
        info.personals[0].2.information.observed_at_of(publication),
        Some(at(19, 0))
    );
    assert_eq!(s.belief_states, before, "prepare 不安装私有候选");
    assert!(Arc::ptr_eq(&s.library, &library));
    s.complete_admin(AdminTransactionId(1), AdminPhase::Abort)
        .unwrap();
    assert_eq!(s.belief_states, before);
    assert_eq!(s.belief_dirty, [true]);
    assert_eq!(s.belief_pending_dirty, [0]);
    assert!(Arc::ptr_eq(s.current_company_facts().unwrap(), &old_facts));
    s.prepare_admin_with_information(AdminTransactionId(2), Vec::new(), Some(p.clone()))
        .unwrap();
    s.complete_admin(AdminTransactionId(2), AdminPhase::Commit)
        .unwrap();
    let after = s.belief_states[0].as_ref().unwrap();
    assert_eq!(
        after.information.observed_at_of(publication),
        Some(at(19, 0))
    );
    assert_eq!(
        after.attention.next_information_check,
        CivilInstant::from_hms(at(19, 0).date().next().unwrap(), 18, 0, 0).unwrap()
    );
    assert_eq!(after.watchlist, before[0].as_ref().unwrap().watchlist);
    assert_eq!(after.price_memory, before[0].as_ref().unwrap().price_memory);
    assert_eq!(
        after.history_reads,
        before[0].as_ref().unwrap().history_reads
    );
    assert_eq!(
        after.retail_experience,
        before[0].as_ref().unwrap().retail_experience
    );
    assert!(Arc::ptr_eq(&s.library, &p.library));
}
#[test]
fn info_02_missing_day_is_typed_rejection_with_no_asset_or_private_drift() {
    let mut s = state(&[AccountId(71)]);
    s.belief_states[0]
        .as_mut()
        .unwrap()
        .watchlist
        .stocks
        .insert(StockId(1), Default::default());
    let mut p = payload(true);
    p.observation
        .as_mut()
        .unwrap()
        .as_of_anchors
        .remove(&StockId(1));
    let before = s.belief_states.clone();
    let accounts = s.accounts.clone();
    let error = s
        .prepare_admin_with_information(
            AdminTransactionId(1),
            vec![AdminOperation::CashDividend {
                local: 0,
                stock: 0,
                amount: 100,
            }],
            Some(p),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        RuntimeError::InformationPrepare {
            error: InformationPrepareError::MissingInformationAnchor { stock: StockId(1) },
            ..
        }
    ));
    assert_eq!(s.accounts, accounts);
    assert_eq!(s.belief_states, before);
    assert!(s.admin_pending.is_none());
}
#[test]
fn info_03_library_only_and_facts_only_install_without_personal_rows() {
    let mut s = state(&[AccountId(71)]);
    let mut p = payload(false);
    announcement(&mut p, 0);
    let c = s
        .prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(p.clone()))
        .unwrap();
    assert!(c.information.unwrap().personals.is_empty());
    s.complete_admin(AdminTransactionId(1), AdminPhase::Commit)
        .unwrap();
    let before = s.belief_states.clone();
    p.current_facts = facts(20000);
    s.prepare_admin_with_information(AdminTransactionId(2), Vec::new(), Some(p.clone()))
        .unwrap();
    s.complete_admin(AdminTransactionId(2), AdminPhase::Commit)
        .unwrap();
    assert_eq!(s.belief_states, before);
    assert!(Arc::ptr_eq(
        s.current_company_facts().unwrap(),
        &p.current_facts
    ));
}
#[test]
fn info_04_owner_and_missing_personal_row_are_rejected() {
    for missing in [false, true] {
        let mut s = state(&[AccountId(71)]);
        s.belief_states[0] = if missing {
            None
        } else {
            Some(personal(AccountId(72)))
        };
        let error = s
            .prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(payload(true)))
            .unwrap_err();
        assert!(matches!(error, RuntimeError::InformationPrepare { .. }));
        assert!(s.admin_pending.is_none());
    }
}
#[test]
fn info_05_initial_facts_accept_restored_tick_and_cannot_overwrite() {
    let mut s = state(&[AccountId(71)]);
    s.cur_tick = 100;
    s.committed_tick = 100;
    assert!(s.current_company_facts().is_err());
    s.install_initial_company_facts(facts(8000)).unwrap();
    assert!(s.install_initial_company_facts(facts(9000)).is_err());
}

#[test]
fn info_06_mixed_days_reach_belief_inputs_and_active_plan_qualification() {
    let mut s = state(&[AccountId(71)]);
    let mut a = (*s.accounts[0]).clone();
    a.pending_sell_qty[0] = 100;
    s.accounts[0] = Arc::new(a);
    s.plan_book
        .create(crate::plans::PlanOpen {
            account: AccountId(71),
            code: StockId(1),
            direction: crate::orderbook::Side::Buy,
            target: crate::plans::PlanTarget::ShareCount(100),
            opinion: crate::plans::PlanOpinion {
                signal_score_bp: 5000,
                source: crate::plans::OpinionSource::Blended,
            },
            confidence_bp: 7000,
            urgency: crate::plans::Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 0,
        })
        .unwrap();
    let mut p = payload(true);
    let (annual0, _) = reports(&mut p, 0, false);
    let (annual1, _) = reports(&mut p, 1, false);
    use crate::calendar::{CalendarExchange, CalendarPolicy, OfficialCoverageEntry, TradingCalendar};
    use crate::session::exchange_calendar_model::{SessionTickLayout, SharedClockInput, SharedDayProgress};
    use crate::session::market_calendar_context::MarketCalendarSource;
    let start = at(0, 0).date();
    let current = start.next().unwrap();
    let mut policy = CalendarPolicy::current_default_policy().unwrap().spec();
    // 真实冻结政策的 synthetic-test 双所差异；不再手写无来源的 7/12 标量。
    policy.official_coverage.push(OfficialCoverageEntry::new(CalendarExchange::Sse, 2031,
        vec![(start, start)], "synthetic-test-info-mixed".into(), "synthetic-test-info-mixed-source".into()));
    let bindings = vec![(StockId(0), CalendarExchange::Sse), (StockId(1), CalendarExchange::Szse)];
    let source = MarketCalendarSource::new(TradingCalendar::from_policy(CalendarPolicy::from_parts(policy).unwrap()).unwrap(), start, bindings.clone()).unwrap();
    s.bind_calendar(Arc::clone(&source), bindings);
    s.cur_tick = 100;
    s.committed_tick = 100;
    let through = CivilInstant::from_hms(current, 19, 0, 0).unwrap();
    let point = p.observation.as_mut().unwrap();
    point.through = through;
    point.clock = crate::runtime::InformationClockContext::fixture_static(source.day(current).unwrap(),
        SessionTickLayout { ticks_per_day: 100, auction_ticks: 0, closing_auction_ticks: 0 },
        SharedClockInput { tick: 100, completed_shared_sessions: 1, progress: SharedDayProgress::Trading { completed_ticks: 0 } }, through);
    point.as_of_anchors = point.clock.anchors().unwrap();
    Arc::make_mut(&mut point.market).tick = 100;
    let c = s
        .prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(p))
        .unwrap();
    let prepared = &c.information.unwrap().personals[0].2;
    assert_eq!(
        prepared.information.observed_at_of(annual0),
        Some(through)
    );
    assert_eq!(
        prepared.information.observed_at_of(annual1),
        Some(through)
    );
    for (stock, ordinal) in [(StockId(0), 0), (StockId(1), 1)] {
        let entry = prepared.belief.entry(&stock).unwrap();
        assert_eq!(entry.anchor, crate::observation::information_clock::InformationTradingAnchor::TradingDay { ordinal });
        assert_eq!(entry.last_cause.as_ref().unwrap().as_of_anchor, entry.anchor);
    }
}
#[test]
fn info_07_custom_monthly_published_1030_observed_1100_and_same_version_due() {
    let mut s = state(&[AccountId(71)]);
    s.belief_states[0]
        .as_mut()
        .unwrap()
        .watchlist
        .stocks
        .insert(StockId(0), Default::default());
    let mut p = payload(true);
    set_observation_phase(&mut s, &mut p, at(11, 0));
    let (_, monthly) = reports(&mut p, 0, true);
    let id = monthly.unwrap();
    let c = s
        .prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(p.clone()))
        .unwrap();
    assert_eq!(
        p.library.report(id, at(11, 0)).unwrap().published_at,
        at(10, 30)
    );
    assert_eq!(
        c.information.unwrap().personals[0]
            .2
            .information
            .observed_at_of(id),
        Some(at(11, 0))
    );
    s.complete_admin(AdminTransactionId(1), AdminPhase::Commit)
        .unwrap();
    let first = s.belief_states.clone();
    let c = s
        .prepare_admin_with_information(AdminTransactionId(2), Vec::new(), Some(p.clone()))
        .unwrap();
    assert!(c.information.unwrap().personals.is_empty());
    s.complete_admin(AdminTransactionId(2), AdminPhase::Commit)
        .unwrap();
    set_observation_phase(&mut s, &mut p,
        CivilInstant::from_hms(at(11, 0).date().next().unwrap(), 19, 0, 0).unwrap());
    let c = s
        .prepare_admin_with_information(AdminTransactionId(3), Vec::new(), Some(p.clone()))
        .unwrap();
    assert_eq!(c.information.unwrap().personals.len(), 1);
    s.complete_admin(AdminTransactionId(3), AdminPhase::Commit)
        .unwrap();
    assert_eq!(s.library.version(), p.library.version());
    assert_ne!(
        s.belief_states[0]
            .as_ref()
            .unwrap()
            .attention
            .next_information_check,
        first[0].as_ref().unwrap().attention.next_information_check
    );
}
#[test]
fn info_08_single_point_rejects_spec_mismatch_and_observation_watermark_rollback() {
    let mut s = state(&[AccountId(71)]);
    let mut p = payload(true);
    p.current_facts = facts(20000);
    let e = s
        .prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(p))
        .unwrap_err();
    assert!(matches!(
        e,
        RuntimeError::InformationPrepare {
            error: InformationPrepareError::ObservationFactsMismatch,
            ..
        }
    ));
    let p = payload(true);
    s.prepare_admin_with_information(AdminTransactionId(2), Vec::new(), Some(p.clone()))
        .unwrap();
    s.complete_admin(AdminTransactionId(2), AdminPhase::Commit)
        .unwrap();
    let mut p = p;
    p.observation.as_mut().unwrap().through = at(18, 0);
    assert!(matches!(
        s.prepare_admin_with_information(AdminTransactionId(3), Vec::new(), Some(p))
            .unwrap_err(),
        RuntimeError::InformationPrepare {
            error: InformationPrepareError::ObservationOrder,
            ..
        }
    ));
}

#[test]
fn info_09_missing_market_issuer_and_spec_are_typed_without_panic() {
    for missing in 0..2 {
        let mut s = state(&[AccountId(71)]);
        s.belief_states[0]
            .as_mut()
            .unwrap()
            .watchlist
            .stocks
            .insert(StockId(0), Default::default());
        let mut p = payload(true);
        if missing == 0 {
            Arc::make_mut(&mut p.observation.as_mut().unwrap().market)
                .stocks
                .remove(&StockId(0));
        } else {
            let missing_facts = Arc::new(
                crate::company::CompanyReadFacts::new(Default::default(), Default::default())
                    .unwrap(),
            );
            p.current_facts = missing_facts.clone();
            p.observation.as_mut().unwrap().company_facts = missing_facts;
        }
        let e = s
            .prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(p))
            .unwrap_err();
        if missing == 0 {
            assert!(matches!(
                e,
                RuntimeError::InformationPrepare {
                    error: InformationPrepareError::MissingMarket { stock: StockId(0) },
                    ..
                }
            ));
        } else {
            assert!(matches!(
                e,
                RuntimeError::InformationPrepare {
                    error: InformationPrepareError::CompanyFacts(
                        crate::company::CompanyReadFactsError::MissingIssuer { stock: StockId(0) }
                    ),
                    ..
                }
            ));
        }
        assert!(s.admin_pending.is_none());
    }
}
#[test]
fn info_10_same_library_version_delays_real_acquisition_until_cadence_due() {
    let mut s = state(&[AccountId(71)]);
    s.belief_states[0]
        .as_mut()
        .unwrap()
        .watchlist
        .stocks
        .insert(StockId(0), Default::default());
    s.belief_states[0]
        .as_mut()
        .unwrap()
        .attention
        .next_information_check = at(19, 0);
    let mut p = payload(true);
    let (_, monthly) = reports(&mut p, 0, true);
    set_observation_phase(&mut s, &mut p, at(11, 0));
    let c = s
        .prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(p.clone()))
        .unwrap();
    assert!(c.information.unwrap().personals.is_empty());
    s.complete_admin(AdminTransactionId(1), AdminPhase::Commit)
        .unwrap();
    assert!(s.belief_states[0]
        .as_ref()
        .unwrap()
        .information
        .observed_at_of(monthly.unwrap())
        .is_none());
    let version = s.library.version();
    set_observation_phase(&mut s, &mut p, at(19, 0));
    let c = s
        .prepare_admin_with_information(AdminTransactionId(2), Vec::new(), Some(p.clone()))
        .unwrap();
    let info = c.information.unwrap();
    assert_eq!(info.library.version(), version);
    assert_eq!(
        info.personals[0]
            .2
            .information
            .observed_at_of(monthly.unwrap()),
        Some(at(19, 0))
    );
}
#[test]
fn info_11_held_and_watchlist_qualification_use_own_group_state() {
    for held in [false, true] {
        let mut s = state(&[AccountId(71), AccountId(193)]);
        if held {
            let mut account = (*s.accounts[1]).clone();
            account.shares[0] = 100;
            s.accounts[1] = Arc::new(account);
        } else {
            s.belief_states[1]
                .as_mut()
                .unwrap()
                .watchlist
                .stocks
                .insert(StockId(0), Default::default());
        }
        let mut p = payload(true);
        let id = announcement(&mut p, 0);
        let c = s
            .prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(p))
            .unwrap();
        let rows = c.information.unwrap().personals;
        assert_eq!(rows[0].1, AccountId(71));
        assert!(rows[0].2.information.observed_at_of(id).is_none());
        assert_eq!(rows[1].1, AccountId(193));
        assert_eq!(rows[1].2.information.observed_at_of(id), Some(at(19, 0)));
    }
}

#[test]
fn info_12_closed_day_evening_monthly_is_acquired_at_real_end_without_market_tick() {
    let mut s = state(&[AccountId(71)]);
    s.belief_states[0]
        .as_mut()
        .unwrap()
        .watchlist
        .stocks
        .insert(StockId(0), Default::default());
    let mut p = payload(true);
    let through = CivilInstant::from_hms(crate::CivilDate::from_iso("2031-04-05").unwrap(), 23, 59, 59).unwrap();
    set_observation_phase(&mut s, &mut p, through);
    assert!(!calendar_source().day(through.date()).unwrap().day_model().shared_trading_on_date(), "正控确为真实 Closed 日期");
    let (_, monthly) = reports_on(&mut p, 0, true, through.date(), 20 * 3600 + 30 * 60);
    let id = monthly.unwrap();
    assert_eq!(p.library.report(id, through).unwrap().published_at.date(), through.date(), "同日 Closed 晚间材料才检验实际披露相位");
    let before_tick = (s.cur_tick, s.committed_tick);
    let c = s
        .prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(p.clone()))
        .unwrap();
    assert_eq!(
        c.information.unwrap().personals[0]
            .2
            .information
            .observed_at_of(id),
        Some(through)
    );
    assert_eq!(
        p.library
            .report(id, through)
            .unwrap()
            .published_at
            .second_of_day(),
        20 * 3600 + 30 * 60
    );
    s.complete_admin(AdminTransactionId(1), AdminPhase::Commit)
        .unwrap();
    assert_eq!((s.cur_tick, s.committed_tick), before_tick);
    assert!(s.snapshot.is_none());
}

#[test]
fn info_13_factory_hot_without_belief_personal_is_a_valid_nonparticipant() {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 1;
    setup.strategy_params.hot.order_size = 100;
    let session = crate::GameSession::new(setup, 17).unwrap();
    assert!(session.runtime_personal_states().is_empty());
    let mut s = state(&[AccountId(71)]);
    s.belief_states[0] = None;
    let mut root = projection(vec![AccountId(71)]);
    Arc::get_mut(&mut root).unwrap().chain_params[0] = None;
    s.root_projection = Some(root);
    s.decision = Some((
        vec![crate::runtime::decision::NpcProfile {
            strategy: crate::strategy::StrategyData::hot(2, 0.01, 100),
            seed: 17,
            next_due: 1,
            attention_base: 1.0,
            retail_style: None,
        }],
        0,
    ));
    let c = s
        .prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(payload(true)))
        .unwrap();
    assert!(c.information.unwrap().personals.is_empty());
}
#[test]
fn info_14_not_due_does_not_hide_missing_observation_facts() {
    let mut s = state(&[AccountId(71)]);
    s.belief_states[0]
        .as_mut()
        .unwrap()
        .watchlist
        .stocks
        .insert(StockId(0), Default::default());
    s.belief_states[0]
        .as_mut()
        .unwrap()
        .attention
        .next_information_check = at(23, 0);
    let mut p = payload(true);
    p.observation
        .as_mut()
        .unwrap()
        .as_of_anchors
        .remove(&StockId(0));
    let result = s.prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(p));
    assert!(matches!(
        result,
        Err(RuntimeError::InformationPrepare {
            error: InformationPrepareError::MissingInformationAnchor { stock: StockId(0) },
            ..
        })
    ));
}

#[test]
fn info_15_information_qualification_reads_same_transaction_asset_working() {
    for reducing in [false, true] {
        let mut s = state(&[AccountId(71)]);
        if reducing {
            let mut a = (*s.accounts[0]).clone();
            a.shares[0] = 100;
            s.accounts[0] = Arc::new(a);
        }
        let before_accounts = s.accounts.clone();
        let before_personals = s.belief_states.clone();
        let op = if reducing {
            AdminOperation::ShareReduction {
                local: 0,
                stock: 0,
                qty: 100,
            }
        } else {
            AdminOperation::ShareSplit {
                local: 0,
                stock: 0,
                bonus_qty: 100,
            }
        };
        let mut p = payload(true);
        let id = announcement(&mut p, 0);
        let c = s
            .prepare_admin_with_information(AdminTransactionId(1), vec![op], Some(p.clone()))
            .unwrap();
        let acquired = c.information.unwrap().personals[0]
            .2
            .information
            .observed_at_of(id);
        assert_eq!(acquired, if reducing { None } else { Some(at(19, 0)) });
        assert_eq!(s.belief_states, before_personals);
        s.complete_admin(AdminTransactionId(1), AdminPhase::Abort)
            .unwrap();
        assert_eq!(s.accounts, before_accounts);
        assert_eq!(s.belief_states, before_personals);
        s.prepare_admin_with_information(AdminTransactionId(2), vec![op], Some(p))
            .unwrap();
        s.complete_admin(AdminTransactionId(2), AdminPhase::Commit)
            .unwrap();
        assert_eq!(s.accounts[0].shares[0], if reducing { 0 } else { 100 });
        assert_eq!(
            s.belief_states[0]
                .as_ref()
                .unwrap()
                .information
                .observed_at_of(id),
            acquired
        );
    }
}

#[test]
fn info_16_market_tick_must_match_static_working_and_restore_tick_is_valid() {
    for actual in [99, 100, 101] {
        let mut s = state(&[AccountId(71)]);
        s.cur_tick = 100;
        s.committed_tick = 100;
        let mut p = payload(true);
        p.observation.as_mut().unwrap().clock = clock_at(at(19, 0), 100);
        p.observation.as_mut().unwrap().as_of_anchors = p.observation.as_ref().unwrap().clock.anchors().unwrap();
        Arc::make_mut(&mut p.observation.as_mut().unwrap().market).tick = actual;
        let result = s.prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(p));
        if actual == 100 {
            assert!(result.is_ok());
        } else {
            assert!(
                matches!(result,Err(RuntimeError::InformationPrepare {error: InformationPrepareError::MarketTickMismatch {expected:100,actual:found},..}) if found==actual)
            );
        }
    }
}

#[test]
fn info_17_observation_requires_complete_live_stock_context_even_without_qualification() {
    for missing in 0..2 {
        let mut s = state(&[AccountId(71)]);
        s.belief_states[0]
            .as_mut()
            .unwrap()
            .attention
            .next_information_check = at(23, 0);
        let mut p = payload(true);
        if missing == 0 {
            p.observation
                .as_mut()
                .unwrap()
                .as_of_anchors
                .remove(&StockId(1));
        } else {
            Arc::make_mut(&mut p.observation.as_mut().unwrap().market)
                .stocks
                .remove(&StockId(1));
        }
        let result = s.prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(p));
        if missing == 0 {
            assert!(matches!(
                result,
                Err(RuntimeError::InformationPrepare {
                    error: InformationPrepareError::MissingInformationAnchor { stock: StockId(1) },
                    ..
                })
            ));
        } else {
            assert!(matches!(
                result,
                Err(RuntimeError::InformationPrepare {
                    error: InformationPrepareError::MissingMarket { stock: StockId(1) },
                    ..
                })
            ));
        }
    }
}

#[test]
fn info_18_real_player_baseline_retail_profile_does_not_require_personal_state() {
    let setup = crate::runtime::bridge_tests::setup_fixture();
    let session = crate::GameSession::new(setup.clone(), 17).unwrap();
    assert_eq!(
        session.account(AccountId(0)).unwrap().kind(),
        crate::AccountKind::Player
    );
    assert!(!session
        .runtime_personal_states()
        .contains_key(&AccountId(0)));
    let profiles = crate::runtime::drive::RuntimeDriver::profiles_from_session(
        &session,
        &setup,
        &[AccountId(0)],
    );
    assert_eq!(profiles[0].strategy.kind, crate::AccountKind::Retail);
    assert!(profiles[0].retail_style.is_none());
    let mut s = state(&[AccountId(0)]);
    s.belief_states[0] = None;
    let mut root = projection(vec![AccountId(0)]);
    Arc::get_mut(&mut root).unwrap().chain_params[0] = None;
    s.root_projection = Some(root);
    s.decision = Some((profiles, 0));
    assert!(s
        .prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(payload(true)))
        .is_ok());
}
#[test]
fn info_19_real_retail_restore_capability_still_requires_its_personal_row() {
    let setup = crate::runtime::bridge_tests::setup_fixture();
    let session = crate::GameSession::new(setup.clone(), 17).unwrap();
    let owner = *session.runtime_personal_states().keys().next().unwrap();
    let original = session.account(owner).unwrap().strategy().unwrap();
    let exported_state = original.production_state().unwrap();
    let wire = serde_json::to_vec(&exported_state).unwrap();
    let restored: crate::strategy::StrategyState = serde_json::from_slice(&wire).unwrap();
    let restored = restored.into_strategy().unwrap();
    assert_eq!(restored.retail_style(), original.retail_style());
    assert!(restored.retail_style().is_some());
    let profiles =
        crate::runtime::drive::RuntimeDriver::profiles_from_session(&session, &setup, &[owner]);
    assert!(profiles[0].retail_style.is_some());
    let mut s = state(&[owner]);
    s.belief_states[0] = None;
    let mut root = projection(vec![owner]);
    Arc::get_mut(&mut root).unwrap().chain_params[0] = None;
    s.root_projection = Some(root);
    s.decision = Some((profiles, 0));
    assert!(matches!(
        s.prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(payload(true))),
        Err(RuntimeError::InformationPrepare {
            error: InformationPrepareError::Identity { .. },
            ..
        })
    ));
}
#[test]
fn info_20_padding_slot_cannot_be_promoted_to_live_personal_by_profile_kind() {
    let mut s = state(&[AccountId(71)]);
    s.accounts.push(Arc::new(GroupAccount::new(0, 2)));
    s.belief_states.push(None);
    s.belief_dirty.push(false);
    let profile = crate::runtime::decision::NpcProfile {
        strategy: crate::strategy::StrategyData::retail(1.0, 100, 1.0),
        seed: 1,
        next_due: u64::MAX,
        attention_base: 1.0,
        retail_style: Some(crate::strategy::RetailStyle::Momentum),
    };
    s.decision = Some((vec![profile.clone(), profile], 0));
    let c = s
        .prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(payload(true)))
        .unwrap();
    assert_eq!(c.information.unwrap().personals.len(), 1);
}

#[test]
fn info_21_all_five_factory_institution_styles_keep_restored_chain_capability() {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.npcs.retail_count = 0;
    setup.npcs.hot_count = 0;
    setup.npcs.inst_count = 5;
    setup.strategy_params.inst.order_size = 100;
    let session = crate::GameSession::new(setup.clone(), 17).unwrap();
    let ids: Vec<_> = session
        .account_roster()
        .into_iter()
        .filter(|id| session.account(*id).unwrap().kind() == crate::AccountKind::Inst)
        .collect();
    assert_eq!(ids.len(), 5);
    let profiles =
        crate::runtime::drive::RuntimeDriver::profiles_from_session(&session, &setup, &ids);
    let chain = session.runtime_chain_params();
    let personals = session.runtime_personal_states();
    let mut s = state(&ids);
    let mut root = projection(ids.clone());
    Arc::get_mut(&mut root).unwrap().chain_params =
        ids.iter().map(|id| chain.get(id).copied()).collect();
    s.root_projection = Some(root);
    s.decision = Some((profiles, 0));
    let mut styles = std::collections::BTreeSet::new();
    for (local, id) in ids.iter().enumerate() {
        let original = session.account(*id).unwrap().strategy().unwrap();
        let encoded = serde_json::to_vec(&original.production_state().unwrap()).unwrap();
        let restored: crate::strategy::StrategyState = serde_json::from_slice(&encoded).unwrap();
        let restored = restored.into_strategy().unwrap();
        assert!(restored.belief_chain_params().is_some());
        assert_eq!(restored.institution_style(), original.institution_style());
        styles.insert(format!("{:?}", restored.institution_style().unwrap()));
        let p = personals[id].clone();
        s.belief_states[local] = Some(crate::runtime::world::BeliefParticipantSnapshot {
            watchlist: p.watchlist,
            price_memory: p.price_memory,
            history_reads: p.history_reads,
            information: p.information,
            belief: p.belief,
            attention: p.attention,
            retail_experience: None,
        });
    }
    assert_eq!(styles.len(), 5);
    let c = s
        .prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(payload(true)))
        .unwrap();
    assert_eq!(
        c.information
            .unwrap()
            .personals
            .iter()
            .map(|(_, id, _)| *id)
            .collect::<Vec<_>>(),
        ids
    );
}

#[test]
fn info_22_padding_with_personal_row_but_without_owner_is_rejected() {
    let mut s = state(&[AccountId(71)]);
    s.accounts.push(Arc::new(GroupAccount::new(0, 2)));
    s.belief_states.push(Some(personal(AccountId(999))));
    s.belief_dirty.push(false);
    let result =
        s.prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(payload(true)));
    assert!(matches!(
        result,
        Err(RuntimeError::InformationPrepare {
            error: InformationPrepareError::Identity { .. },
            ..
        })
    ));
}

#[test]
fn selected_company_source_group_review_cap_reads_facts_after_information_commit() {
    let mut s = state(&[AccountId(71)]);
    let initial = Arc::clone(&s.root_projection.as_ref().unwrap().initial_company_facts);
    s.install_initial_company_facts(initial).unwrap();
    let market = payload(true).observation.unwrap().market.as_ref().clone();
    let personal = s.belief_states[0].as_ref().unwrap().clone();
    let cap = |state: &GroupState| {
        let host = GroupReviewHost {
            state, local: 0, account: AccountId(71), market: market.clone(),
            belief: &personal.belief, information: &personal.information,
            minutes_by_code: Default::default(),
            moment: crate::experience::ExperienceMoment {
                civil_date: at(19, 0).date(), market_minute: 17, trading_day: 0,
            },
        };
        crate::session::PlanReviewHost::max_holdable_shares(&host, &StockId(0))
    };
    assert_eq!(cap(&s), 10_000);
    let mut p = payload(false);
    p.current_facts = facts(u64::from(u32::MAX) + 701);
    s.prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(p)).unwrap();
    assert_eq!(cap(&s), 10_000, "prepare 不提前改变 consumer 可见股本");
    s.complete_admin(AdminTransactionId(1), AdminPhase::Commit).unwrap();
    assert_eq!(cap(&s), u64::from(u32::MAX), "commit 后真实复审 consumer 读取当前事实");
    assert_eq!(s.root_projection.as_ref().unwrap().initial_company_facts.company_spec(&crate::company::CompanyId(0)).unwrap().1, 10_000);
}

#[test]
fn funds_p1_information_abort_restores_complete_private_overlay_then_commits_once() {
    let mut s = state(&[AccountId(71)]);
    let old_facts = facts(8000);
    s.install_initial_company_facts(old_facts.clone()).unwrap();
    s.information_observed_through = Some(at(18, 0));
    s.belief_dirty[0] = true;
    s.belief_pending_dirty.push(0);
    let before_accounts = s.accounts.clone();
    let before_personal = s.belief_states.clone();
    let before_library = s.library.clone();
    let before_watermark = s.information_observed_through;
    let before_dirty = s.belief_dirty.clone();
    let before_pending = s.belief_pending_dirty.clone();
    let before_hash = s.state_hash();
    let mut payload = payload(true);
    let publication = announcement(&mut payload, 0);
    let operations = vec![AdminOperation::SharesWithCost {
        local: 0,
        stock: 0,
        qty: 7,
        subscription_cost: 5600,
    }];
    let candidate = s
        .prepare_admin_with_information(
            AdminTransactionId(1),
            operations.clone(),
            Some(payload.clone()),
        )
        .unwrap();
    assert_eq!(
        candidate.accounts[0].1.shares[0],
        before_accounts[0].shares[0] + 7
    );
    assert_eq!(
        candidate.accounts[0].1.invested_cents[0],
        before_accounts[0].invested_cents[0] + 5600
    );
    let info = candidate.information.unwrap();
    assert!(Arc::ptr_eq(&info.library, &payload.library));
    assert!(Arc::ptr_eq(&info.current_facts, &payload.current_facts));
    assert_eq!(
        info.personals[0].2.information.observed_at_of(publication),
        Some(at(19, 0))
    );
    assert_eq!(
        s.belief_states, before_personal,
        "prepare不能提前安装含RNG的完整个人态"
    );
    s.complete_admin(AdminTransactionId(1), AdminPhase::Abort)
        .unwrap();
    assert_eq!(s.accounts, before_accounts);
    assert_eq!(s.belief_states, before_personal);
    assert!(Arc::ptr_eq(&s.library, &before_library));
    assert!(Arc::ptr_eq(s.current_company_facts().unwrap(), &old_facts));
    assert_eq!(s.information_observed_through, before_watermark);
    assert_eq!(s.belief_dirty, before_dirty);
    assert_eq!(s.belief_pending_dirty, before_pending);
    assert_eq!(s.state_hash(), before_hash);
    assert!(s.admin_pending.is_none());
    s.prepare_admin_with_information(AdminTransactionId(2), operations, Some(payload.clone()))
        .unwrap();
    s.complete_admin(AdminTransactionId(2), AdminPhase::Commit)
        .unwrap();
    assert_eq!(s.accounts[0].shares[0], before_accounts[0].shares[0] + 7);
    assert_eq!(
        s.accounts[0].invested_cents[0],
        before_accounts[0].invested_cents[0] + 5600
    );
    assert_eq!(s.accounts[0].cash, before_accounts[0].cash);
    assert_eq!(s.accounts[0].frozen_today, before_accounts[0].frozen_today);
    assert!(Arc::ptr_eq(&s.library, &payload.library));
    assert!(Arc::ptr_eq(
        s.current_company_facts().unwrap(),
        &payload.current_facts
    ));
    assert_eq!(s.information_observed_through, Some(at(19, 0)));
    assert!(s.admin_pending.is_none());
}

#[test]
fn typed_information_map_variant_is_validated_before_signature_and_cadence_shortcuts() {
    for not_due in [false, true] {
        let mut s = state(&[AccountId(71)]);
        s.belief_states[0].as_mut().unwrap().watchlist.stocks.insert(StockId(0), Default::default());
        let mut p = payload(true);
        reports(&mut p, 0, false);
        s.prepare_admin_with_information(AdminTransactionId(1), Vec::new(), Some(p.clone())).unwrap();
        s.complete_admin(AdminTransactionId(1), AdminPhase::Commit).unwrap();
        if not_due {
            s.belief_states[0].as_mut().unwrap().attention.next_information_check = at(23, 0);
        }
        let before = s.belief_states.clone();
        let library = Arc::clone(&s.library);
        let point = p.observation.as_mut().unwrap();
        point.as_of_anchors.insert(StockId(0), crate::observation::information_clock::InformationTradingAnchor::DayBefore { ordinal: 0 });
        let error = s.prepare_admin_with_information(AdminTransactionId(2), Vec::new(), Some(p)).unwrap_err();
        assert!(matches!(error, RuntimeError::ProtocolInvariant { .. } | RuntimeError::InformationPrepare { .. }));
        assert_eq!(s.belief_states, before, "同 position 的未来 variant 不得被 shortcut 吞掉或修改整本 Book");
        assert!(Arc::ptr_eq(&s.library, &library));
    }
}
