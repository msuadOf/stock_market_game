//! V10 工作草案：真实 Calendar mask 与既有 INFO/Admin 机制交集。
//! 应作为 world::admin_transaction 后代模块注册；不冒生产 civil dispatcher 验收。
use super::*;
use crate::calendar::{
    CalendarExchange, CalendarPolicy, CivilDate, CivilInstant, OfficialCoverageEntry,
    TradingCalendar,
};
use crate::observation::information_clock::InformationTradingAnchor;
use crate::runtime::group::TickPlan;
use crate::runtime::information_fixture as fixture;
use crate::runtime::protocol::{InformationObservation, InformationPreparePayload};
use crate::runtime::skeleton_tests::basic_spec;
use crate::{AccountId, StockExchange, StockId};

fn date(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

fn world(all_closed: bool) -> World {
    world_with_scripts(
        all_closed,
        Box::new(|_| Box::new(|_, _| TickPlan::default())),
    )
}

fn world_with_scripts(
    all_closed: bool,
    scripts: Box<dyn Fn(crate::runtime::GroupIdx) -> crate::runtime::group::GroupScript>,
) -> World {
    let mut spec = basic_spec(3, 2, 2, scripts);
    let start = date("2031-03-31");
    let current = date("2031-04-01");
    spec.ticks_per_day = 100;
    spec.auction_ticks = 0;
    spec.closing_auction_ticks = 0;
    spec.calendar_start_date = start;
    spec.civil_start_date = current;
    spec.initial_committed_tick = 100;
    spec.initial_last_day_end_tick = Some(100);
    spec.stocks[0].exchange = StockExchange::Shanghai;
    spec.stocks[1].exchange = StockExchange::Shenzhen;
    let ids = [11, 37, 51, 79, 113].map(AccountId);
    spec.root_projection = Some(fixture::projection(ids.to_vec()));
    spec.belief_seeds = ids
        .into_iter()
        .map(|id| {
            let mut personal = fixture::personal(id);
            personal.attention.information_cadence =
                crate::session::NpcInformationCadence::Immediate;
            personal.attention.next_information_check =
                CivilInstant::from_hms(current, 0, 0, 0).unwrap();
            Some(personal)
        })
        .collect();
    let mut policy = CalendarPolicy::current_default_policy().unwrap().spec();
    policy.official_coverage.push(OfficialCoverageEntry::new(
        CalendarExchange::Sse,
        2031,
        vec![(start, current)],
        "synthetic-c2c-v10-info".into(),
        "synthetic-c2c-v10-info-digest".into(),
    ));
    if all_closed {
        // 前日SZSE真实open已形成union day1；仅当前日双所closed，不伪造既有tick。
        policy.official_coverage.push(OfficialCoverageEntry::new(
            CalendarExchange::Szse,
            2031,
            vec![(current, current)],
            "synthetic-c2c-v10-allclosed".into(),
            "synthetic-c2c-v10-allclosed-digest".into(),
        ));
    }
    let calendar =
        TradingCalendar::from_policy(CalendarPolicy::from_parts(policy).unwrap()).unwrap();
    let bindings = vec![
        (StockId(0), CalendarExchange::Sse),
        (StockId(1), CalendarExchange::Szse),
    ];
    let source = crate::session::market_calendar_context::MarketCalendarSource::new(
        calendar,
        start,
        bindings.clone(),
    )
    .unwrap();
    let mut world = World::spawn_with_calendar_source(spec, source, bindings).unwrap();
    let authority_epoch = world.authority_epoch;
    let presence = ids
        .chunks(2)
        .enumerate()
        .map(|(group, group_ids)| {
            (
                group as crate::runtime::GroupIdx,
                group_ids
                    .iter()
                    .copied()
                    .map(|id| {
                        (
                            id,
                            crate::runtime::personal_full_validation::PersonalPresence::Required,
                        )
                    })
                    .collect(),
            )
        })
        .collect();
    world.install_personal_presence(authority_epoch, presence);
    world
}

fn material(
    clock: crate::runtime::InformationClockContext,
    tick: u64,
) -> InformationPreparePayload {
    let through = clock.cutoff();
    let anchors = clock.anchors().unwrap();
    let mut payload = fixture::payload(true);
    let market = std::sync::Arc::make_mut(&mut payload.observation.as_mut().unwrap().market);
    market.tick = tick;
    market.market_minute = crate::session::exchange_calendar_model::calendar_clock_projection(
        clock.day().day_model(),
        clock.layout(),
        clock.input(),
    )
    .unwrap()
    .completed_shared_minute_count();
    let market = payload.observation.as_ref().unwrap().market.clone();
    let memory_retention = payload.observation.as_ref().unwrap().memory_retention;
    payload.observation = Some(InformationObservation {
        clock,
        through,
        as_of_anchors: anchors,
        market,
        company_facts: payload.current_facts.clone(),
        memory_retention,
    });
    let occurred_on = if through.second_of_day() < crate::information::DISCLOSURE_PHASE_SECOND {
        through.date().prev().unwrap()
    } else {
        through.date()
    };
    let published_at = CivilInstant::from_hms(occurred_on, 18, 0, 0).unwrap();
    for company in [crate::company::CompanyId(0), crate::company::CompanyId(1)] {
        // 先放入真实closing产生的年度报告，重估不得缺本人已知annual材料。
        fixture::reports(&mut payload, company.0, false);
        std::sync::Arc::make_mut(&mut payload.library)
            .publish_announcement(crate::information::AnnouncementRequest {
                company,
                occurred_on,
                published_at,
                content: crate::information::AnnouncementContent::Shock(
                    crate::information::AnnouncedEvent {
                        kind: crate::company::ShockKind::ContractWon,
                        amplitude_bp: 100,
                        starts_on: occurred_on,
                        expires_on: occurred_on,
                    },
                ),
            })
            .unwrap();
    }
    payload
}

fn assert_all_live(
    groups: &[crate::runtime::protocol::AdminGroupCandidate],
    payload: &InformationPreparePayload,
) {
    assert_eq!(
        groups.iter().map(|row| row.group).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(
        groups
            .iter()
            .map(|row| row.information.as_ref().unwrap().personals.len())
            .collect::<Vec<_>>(),
        [2, 2, 1]
    );
    let point = payload.observation.as_ref().unwrap();
    for group in groups {
        assert!(
            group.accounts.is_empty(),
            "没有资产operation的empty locals仍须INFO参与"
        );
        let info = group.information.as_ref().unwrap();
        assert!(std::sync::Arc::ptr_eq(&info.library, &payload.library));
        for (_, owner, personal) in &info.personals {
            assert_eq!(personal.information.owner(), *owner);
            assert_eq!(
                personal.information.acquired_count(),
                4,
                "closed发行人披露也须真实获知"
            );
            for stock in [StockId(0), StockId(1)] {
                let entry = personal
                    .belief
                    .entry(&stock)
                    .expect("真实披露必须生成对应belief entry");
                assert_eq!(entry.anchor, point.as_of_anchors[&stock]);
                assert_eq!(
                    entry.last_cause.as_ref().unwrap().as_of_anchor,
                    entry.anchor
                );
            }
        }
    }
}

#[test]
fn mixed_committed_clock_information_includes_closed_issuer_and_all_live_empty_locals() {
    let mut world = world(false);
    world.run_ticks(1).unwrap();
    let calendar = world.committed_calendar_tick().unwrap().unwrap();
    let clock = crate::runtime::InformationClockContext::after_committed(calendar);
    let payload = material(clock, 101);
    let point = payload.observation.as_ref().unwrap();
    assert_eq!(
        point.as_of_anchors[&StockId(0)],
        InformationTradingAnchor::NoPriorSession
    );
    assert_eq!(
        point.as_of_anchors[&StockId(1)],
        InformationTradingAnchor::TradingDay { ordinal: 1 }
    );
    let before_market = world.public_views.clone();
    let before_tick = world.global.committed_tick;
    let groups = world
        .with_information_transaction(Vec::new(), payload.clone(), |candidate| {
            Ok(candidate.groups.clone())
        })
        .unwrap();
    assert_all_live(&groups, &payload);
    assert_eq!(world.public_views, before_market);
    assert_eq!(world.global.committed_tick, before_tick);
    world.shutdown().unwrap();
}

#[test]
fn all_closed_static_information_runs_without_market_tick_and_keeps_all_live_groups() {
    let mut world = world(true);
    let day = world.calendar_source.day(world.civil_date).unwrap();
    let input = crate::session::calendar_clock_input(
        &day,
        world.tick_layout,
        world.global.committed_tick,
        u64::from(world.global.day),
    )
    .unwrap();
    let clock = crate::runtime::InformationClockContext::fixture_static(
        day,
        world.tick_layout,
        input,
        CivilInstant::from_hms(world.civil_date, 19, 0, 0).unwrap(),
    );
    let payload = material(clock, 100);
    let point = payload.observation.as_ref().unwrap();
    assert_eq!(
        point.as_of_anchors[&StockId(0)],
        InformationTradingAnchor::NoPriorSession
    );
    assert_eq!(
        point.as_of_anchors[&StockId(1)],
        InformationTradingAnchor::DayBefore { ordinal: 0 }
    );
    let before_market = world.public_views.clone();
    let before_accounts = world.snapshot_accounts().unwrap();
    let groups = world
        .with_information_transaction(Vec::new(), payload.clone(), |candidate| {
            Ok(candidate.groups.clone())
        })
        .unwrap();
    assert_all_live(&groups, &payload);
    assert_eq!(world.public_views, before_market);
    assert_eq!(world.snapshot_accounts().unwrap(), before_accounts);
    assert_eq!(world.global.committed_tick, 100);
    assert!(world.report_log.is_empty());
    world.shutdown().unwrap();
}

#[test]
fn mixed_last_group_info_rejection_aborts_only_new_overlay_and_preserves_committed_market() {
    let mut world = world(false);
    world.run_ticks(1).unwrap();
    let calendar = world.committed_calendar_tick().unwrap().unwrap();
    let payload = material(
        crate::runtime::InformationClockContext::after_committed(calendar),
        101,
    );
    let before_source = world.mirror_source();
    let before_market = world.public_views.clone();
    let before_accounts = world.snapshot_accounts().unwrap();
    let before_personal = world.collect_personal_full().unwrap();
    world.group_txs[2]
        .send(crate::runtime::protocol::GroupIn::InstallPersonal {
            local: 0,
            snapshot: fixture::personal(AccountId(999)),
        })
        .unwrap();
    let result = world.with_information_transaction(
        vec![(
            0,
            vec![AdminOperation::CashDividend {
                local: 0,
                stock: 0,
                amount: 101,
            }],
        )],
        payload.clone(),
        |_| Ok(()),
    );
    assert!(matches!(
        result,
        Err(RuntimeError::InformationPrepare {
            group: 2,
            error: crate::runtime::protocol::InformationPrepareError::Identity { .. }
        })
    ));
    assert!(world.dead_reason.is_none());
    assert_eq!(world.global.committed_tick, 101);
    assert_eq!(world.mirror_source(), before_source);
    assert_eq!(world.public_views, before_market);
    assert_eq!(world.snapshot_accounts().unwrap(), before_accounts);
    // 只读真实组回执；坏owner所在group2不参与此等值断言，也不假设它被回滚。
    let source = world.mirror_source();
    for group in [0usize, 1] {
        world.group_txs[group]
            .send(crate::runtime::protocol::GroupIn::EchoPersonalFull { source })
            .unwrap();
    }
    let mut after_personal = Vec::new();
    for _ in 0..2 {
        match world
            .up_rx
            .recv_timeout(std::time::Duration::from_millis(1000))
            .unwrap()
        {
            crate::runtime::protocol::UpMsg::GroupPersonalFull {
                group,
                source: actual,
                slots,
            } => {
                assert_eq!(actual, source);
                assert!(group < 2);
                after_personal.push((group, slots));
            }
            other => panic!("INFO拒绝后的完整个人态回执错误：{other:?}"),
        }
    }
    after_personal.sort_by_key(|(group, _)| *group);
    assert_eq!(
        after_personal,
        before_personal
            .groups
            .into_iter()
            .filter(|(group, _)| *group < 2)
            .collect::<Vec<_>>(),
        "末组拒绝后前两组完整个人态必须未泄漏获知或cadence"
    );
    let mut personal = fixture::personal(AccountId(113));
    personal.attention.information_cadence = crate::session::NpcInformationCadence::Immediate;
    personal.attention.next_information_check =
        CivilInstant::from_hms(world.civil_date, 0, 0, 0).unwrap();
    world.group_txs[2]
        .send(crate::runtime::protocol::GroupIn::InstallPersonal {
            local: 0,
            snapshot: personal,
        })
        .unwrap();
    let groups = world
        .with_information_transaction(Vec::new(), payload.clone(), |candidate| {
            Ok(candidate.groups.clone())
        })
        .unwrap();
    assert_all_live(&groups, &payload);
    assert_eq!(
        world.snapshot_accounts().unwrap(),
        before_accounts,
        "失败本id的分红不能在后续INFO重现"
    );
    world.shutdown().unwrap();
}

#[test]
fn mixed_pending_admin_rejects_real_start_and_keeps_previous_market_baseline() {
    let mut world = world(false);
    world.run_ticks(1).unwrap();
    let source = world.mirror_source();
    let public = world.public_views.clone();
    let accounts = world.snapshot_accounts().unwrap();
    let id = crate::runtime::protocol::AdminTransactionId(99);
    world.group_txs[0]
        .send(crate::runtime::protocol::GroupIn::AdminPrepare {
            id,
            operations: vec![AdminOperation::CashDividend {
                local: 0,
                stock: 0,
                amount: 101,
            }],
            information: None,
            information_clock: None,
            repurchase: None,
            stage_layout: None,
        })
        .unwrap();
    let prepared = world
        .up_rx
        .recv_timeout(std::time::Duration::from_millis(1000))
        .unwrap();
    assert!(
        matches!(prepared, crate::runtime::protocol::UpMsg::AdminReceipt {
        id: actual, group: 0, phase: crate::runtime::protocol::AdminPhase::Prepare, result: Ok(_),
    } if actual == id),
        "必须先收到真实pending prepare回执，不能空pending假阳性"
    );
    let result = world.run_ticks(1);
    let is_expected_pending_rejection = matches!(&result, Err(RuntimeError::WorkerFailed {
        who: crate::runtime::protocol::WorkerId::Group(0), error,
    }) if matches!(error.as_ref(), RuntimeError::ProtocolInvariant { detail } if detail.contains("行政事务 pending")));
    assert!(is_expected_pending_rejection,
        "真实Start必须精确被group0的pending行政overlay互斥门拒绝；result={result:?}；dead_finals={:?}",
        world.dead_finals);
    assert_eq!(world.global.committed_tick, 101);
    assert_eq!(world.mirror_source(), source);
    assert_eq!(world.public_views, public);
    assert!(world.handles.is_empty(), "fatal必须回收真实workers");
    let finals = world
        .dead_finals
        .as_ref()
        .expect("真实fatal必须保Final原件");
    for (group, expected) in accounts {
        let actual = finals
            .group_finals
            .iter()
            .find(|row| row.group == group)
            .unwrap();
        assert_eq!(
            actual.accounts, expected,
            "只丢弃未提交overlay，不回滚此前市场版本"
        );
    }
}

#[test]
fn mixed_pending_admin_rejects_real_completed_market_tick_day_end_without_publishing_overlay() {
    let mut world = world(false);
    world.run_ticks(100).unwrap();
    let committed = world.committed_calendar_tick().unwrap().unwrap();
    assert_eq!(committed.source.tick, 200);
    assert_eq!(world.global.committed_tick, 200);
    assert!(matches!(
        committed.calendar.after_input().progress,
        crate::session::exchange_calendar_model::SharedDayProgress::TradingCompleted
    ));
    let source = world.mirror_source();
    let public = world.public_views.clone();
    let published = world.publication.load();
    let accounts = world.snapshot_accounts().unwrap();
    let books = world.collect_plans().unwrap();
    let payload = material(
        crate::runtime::InformationClockContext::after_committed(committed.clone()),
        200,
    );
    let point = payload.observation.as_ref().unwrap();
    let proof = world
        .authenticate_information_clock(
            &point.clock,
            point.through,
            std::sync::Arc::new(point.as_of_anchors.clone()),
        )
        .unwrap();
    let id = crate::runtime::protocol::AdminTransactionId(99);
    world.group_txs[0]
        .send(crate::runtime::protocol::GroupIn::AdminPrepare {
            id,
            operations: vec![AdminOperation::CashDividend {
                local: 0,
                stock: 0,
                amount: 101,
            }],
            information: Some(payload),
            information_clock: Some(proof),
            repurchase: None,
            stage_layout: None,
        })
        .unwrap();
    match world
        .up_rx
        .recv_timeout(std::time::Duration::from_millis(1000))
        .unwrap()
    {
        crate::runtime::protocol::UpMsg::AdminReceipt {
            id: actual,
            group: 0,
            phase: crate::runtime::protocol::AdminPhase::Prepare,
            result: Ok(candidate),
        } => {
            assert_eq!(actual, id);
            assert_eq!(candidate.accounts.len(), 1);
            let info = candidate
                .information
                .as_ref()
                .expect("必须是真实INFO pending候选");
            assert_eq!(info.personals.len(), 2);
            assert!(
                info.personals
                    .iter()
                    .all(|(_, _, personal)| personal.information.acquired_count() == 4),
                "closed发行人材料必须真实进入待提交INFO候选"
            );
            assert_eq!(candidate.accounts[0].0, 0);
            assert_eq!(
                candidate.accounts[0].1.cash,
                accounts[0].1[0].cash + 101,
                "必须真实准备非零overlay，不以空pending作为正控"
            );
        }
        other => panic!("必须先有精确pending Prepare Ack：{other:?}"),
    }
    // 使用本authority最后已提交原始TickArc，before/after均为真实市场拍；不伪造ManualDayEnd。
    world.group_txs[0]
        .send(crate::runtime::protocol::GroupIn::DayEnd(Box::new(
            crate::runtime::protocol::DayEndDirective {
                tick: 200,
                day: 1,
                civil_date: world.civil_date,
                calendar: crate::runtime::protocol::DayEndCalendar::MarketTick(
                    std::sync::Arc::clone(&committed.calendar),
                ),
                rights: Vec::new(),
            },
        )))
        .unwrap();
    let failure = match world
        .up_rx
        .recv_timeout(std::time::Duration::from_millis(1000))
        .unwrap()
    {
        crate::runtime::protocol::UpMsg::Failed {
            who: crate::runtime::protocol::WorkerId::Group(0),
            error: RuntimeError::ProtocolInvariant { detail },
        } => {
            assert!(
                detail.contains("行政事务 pending"),
                "不可用其他clock/protocol失败冒此互斥门：{detail}"
            );
            RuntimeError::WorkerFailed {
                who: crate::runtime::protocol::WorkerId::Group(0),
                error: Box::new(RuntimeError::ProtocolInvariant { detail }),
            }
        }
        other => panic!("合法MarketTick DayEnd必须精确由pending门拒绝：{other:?}"),
    };
    let error = world.stop_after_fatal_failure(
        200,
        0,
        "待审MarketTick DayEnd互斥见证",
        failure.clone(),
        FatalCleanupMode::StopImmediately,
        None,
    );
    assert_eq!(error, failure);
    assert!(world.handles.is_empty());
    assert!(world.dead_reason.is_some());
    assert_eq!(world.global.committed_tick, 200);
    assert_eq!(world.mirror_source(), source);
    assert_eq!(world.public_views, public);
    assert!(std::sync::Arc::ptr_eq(
        &world.publication.load(),
        &published
    ));
    let finals = world
        .dead_finals
        .as_ref()
        .expect("真实失败必须收齐Final且join");
    assert_eq!(finals.group_finals.len(), 3);
    assert_eq!(finals.desk_finals.len(), 2);
    assert_eq!(finals.worker_results.len(), 5);
    for (group, expected) in accounts {
        let actual = finals
            .group_finals
            .iter()
            .find(|row| row.group == group)
            .unwrap();
        assert_eq!(
            actual.accounts, expected,
            "未提交101分overlay不得泄漏，也不得回滚已发布市场"
        );
        assert_eq!(
            actual.plan_book_for_test,
            books.iter().find(|row| row.0 == group).unwrap().1
        );
    }
    assert_eq!(finals.worker_results.iter().filter(|(who, result)|
        *who == crate::runtime::protocol::WorkerId::Group(0) && matches!(result, Err(RuntimeError::ProtocolInvariant { detail }) if detail.contains("行政事务 pending"))).count(), 1);
    assert!(finals
        .worker_results
        .iter()
        .filter(|(who, _)| *who != crate::runtime::protocol::WorkerId::Group(0))
        .all(|(_, result)| result.is_ok()));
}

#[test]
fn mixed_pending_start_fatal_cleanup_fences_real_producers_before_desk_abort() {
    use crate::runtime::protocol::{
        DeskFatalCleanupEventForTest as DeskEvent, DeskFatalCleanupProbeForTest,
    };
    use crate::runtime::world::FatalCleanupPhaseForTest as Phase;
    use std::collections::BTreeSet;
    use std::sync::mpsc;
    use std::time::Duration;
    let deadline = Duration::from_millis(1000);
    let (ready_tx, ready_rx) = mpsc::channel();
    let mut group_release = Vec::new();
    let mut group_gates = Vec::new();
    for _ in 0..3 {
        let (release, gate) = mpsc::channel();
        group_release.push(release);
        group_gates.push(Some(gate));
    }
    // 工厂只在spawn线程消费一次Receiver；进入worker后没有共享锁或概率sleep。
    let group_gates = std::cell::RefCell::new(group_gates);
    let scripts = Box::new(move |group| -> crate::runtime::group::GroupScript {
        let gate = group_gates.borrow_mut()[usize::from(group)].take().unwrap();
        let ready = ready_tx.clone();
        Box::new(move |tick, round| {
            if group > 0 && tick == 102 && round == 0 {
                ready.send(group).unwrap();
                gate.recv_timeout(deadline)
                    .expect("真实group决策门须在1000ms内释放");
            }
            TickPlan::default()
        })
    });
    let mut world = world_with_scripts(false, scripts);
    world.run_ticks(1).unwrap();
    let source = world.mirror_source();
    let public = world.public_views.clone();
    let publication = world.publication.load();
    let accounts = world.snapshot_accounts().unwrap();
    let plans = world.collect_plans().unwrap();
    let committed = world.committed_calendar_tick().unwrap().unwrap();
    assert_eq!(committed.source.tick, 101);
    let (sent_tx, sent_rx) = mpsc::channel();
    for group in [1usize, 2] {
        world.group_txs[group]
            .send(
                crate::runtime::protocol::GroupIn::SignalFinalFlushOnceForTest {
                    sent: sent_tx.clone(),
                },
            )
            .unwrap();
    }
    drop(sent_tx);
    let (desk_tx, desk_rx) = mpsc::channel();
    let mut desk_release = Vec::new();
    for stock in 0..2 {
        let (release, after_abort) = mpsc::channel();
        desk_release.push(release);
        world.desk_txs[stock]
            .send(
                crate::runtime::protocol::DeskMsg::InstallFatalCleanupProbeForTest(
                    DeskFatalCleanupProbeForTest {
                        tick: 102,
                        events: desk_tx.clone(),
                        after_abort,
                    },
                ),
            )
            .unwrap();
    }
    drop(desk_tx);
    let (phase_tx, phase_rx) = mpsc::channel();
    let (phase_release, phase_gate) = mpsc::channel();
    world.fatal_cleanup_probe_for_test = Some(Box::new(move |phase| {
        let wait = matches!(
            &phase,
            Phase::BeforeProducerJoin | Phase::BeforeDeskShutdown
        );
        phase_tx.send(phase).unwrap();
        if wait {
            phase_gate
                .recv_timeout(deadline)
                .expect("协调者失败阶段门须在1000ms内释放");
        }
    }));
    let id = crate::runtime::protocol::AdminTransactionId(99);
    world.group_txs[0]
        .send(crate::runtime::protocol::GroupIn::AdminPrepare {
            id,
            operations: vec![AdminOperation::CashDividend {
                local: 0,
                stock: 0,
                amount: 101,
            }],
            information: None,
            information_clock: None,
            repurchase: None,
            stage_layout: None,
        })
        .unwrap();
    assert!(
        matches!(world.up_rx.recv_timeout(deadline).unwrap(), crate::runtime::protocol::UpMsg::AdminReceipt {
        id: actual, group: 0, phase: crate::runtime::protocol::AdminPhase::Prepare, result: Ok(_),
    } if actual == id)
    );

    let controller = std::thread::spawn(move || {
        let first_phase = phase_rx.recv_timeout(deadline).unwrap();
        let mut ready = BTreeSet::new();
        for _ in 0..2 {
            assert!(ready.insert(ready_rx.recv_timeout(deadline).unwrap()));
        }
        assert_eq!(ready, BTreeSet::from([1, 2]));
        let producer_fence = matches!(&first_phase, Phase::BeforeProducerJoin);
        let mut events = Vec::new();
        let mut aborted = BTreeSet::new();
        if !producer_fence {
            assert!(matches!(first_phase, Phase::BeforeDeskShutdown));
            // 旧序强制真实Abort先完成，再让健康producer发Ops；不伪造Failed。
            for _ in 0..2 {
                let event = desk_rx.recv_timeout(deadline).unwrap();
                match &event {
                    DeskEvent::AbortApplied {
                        stock,
                        tick: 102,
                        cur_tick: 101,
                        calendar_tick: Some(101),
                    } => assert!(aborted.insert(*stock)),
                    _ => panic!("健康producer仍受gate约束时必须先见真实desk Abort：{event:?}"),
                }
                events.push(event);
            }
        }
        for group in [1usize, 2] {
            group_release[group].send(()).unwrap();
        }
        for _ in 0..2 {
            sent_rx
                .recv_timeout(deadline)
                .expect("必须见两组真实final-flush已发送");
        }
        if producer_fence {
            phase_release.send(()).unwrap();
            match phase_rx.recv_timeout(deadline).unwrap() {
                Phase::ProducersJoined { workers } => assert_eq!(
                    workers,
                    vec![WorkerId::Group(0), WorkerId::Group(1), WorkerId::Group(2)]
                ),
                other => panic!("必须先取得三个group真实join：{other:?}"),
            }
            assert!(matches!(
                phase_rx.recv_timeout(deadline).unwrap(),
                Phase::BeforeDeskShutdown
            ));
            while aborted.len() < 2 {
                let event = desk_rx.recv_timeout(deadline).unwrap();
                match &event {
                    DeskEvent::AbortApplied {
                        stock,
                        tick: 102,
                        cur_tick: 101,
                        calendar_tick: Some(101),
                    } => assert!(aborted.insert(*stock)),
                    DeskEvent::OpsDequeued {
                        tick: 102,
                        round: 0,
                        group: 1 | 2,
                        final_flush: true,
                        ops_len: 0,
                        cur_tick: 102,
                        ..
                    } => {}
                    _ => panic!("producer fence之后不得有错位Ops或Abort：{event:?}"),
                }
                events.push(event);
            }
            let ops = events
                .iter()
                .filter_map(|event| match event {
                    DeskEvent::OpsDequeued { stock, group, .. } => Some((*stock, *group)),
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            assert_eq!(ops, BTreeSet::from([(0, 1), (0, 2), (1, 1), (1, 2)]));
        }
        assert_eq!(aborted, BTreeSet::from([0, 1]));
        for release in desk_release {
            release.send(()).unwrap();
        }
        if !producer_fence {
            let mut late = BTreeSet::new();
            for _ in 0..2 {
                let event = desk_rx.recv_timeout(deadline).unwrap();
                match &event {
                    DeskEvent::OpsDequeued {
                        stock,
                        tick: 102,
                        round: 0,
                        group: 1 | 2,
                        final_flush: true,
                        ops_len: 0,
                        cur_tick: 101,
                    } => assert!(late.insert(*stock)),
                    _ => panic!("旧序须在真实Abort后出队本轮迟到Ops：{event:?}"),
                }
                events.push(event);
            }
            assert_eq!(late, BTreeSet::from([0, 1]));
        }
        phase_release.send(()).unwrap();
        (producer_fence, events)
    });
    // 即使产品处理panic，也先等待有界controller退出，不能遗留被测试gate锁住的线程。
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| world.run_ticks(1)));
    let controller_result = controller.join();
    world.fatal_cleanup_probe_for_test = None;
    let (producer_fence, events) = controller_result.expect("失败时序controller必须完整结束");
    let result = result.unwrap_or_else(|payload| std::panic::resume_unwind(payload));
    eprintln!("pending Start真实清理次序：producer_fence={producer_fence}，events={events:?}，result={result:?}，Final={:?}", world.dead_finals);
    assert!(matches!(&result, Err(RuntimeError::WorkerFailed { who: WorkerId::Group(0), error })
        if matches!(error.as_ref(), RuntimeError::ProtocolInvariant { detail } if detail.contains("行政事务 pending"))),
        "PENDING_START_PRIMARY_MUST_SURVIVE_REAL_LATE_OPS：原typed首错必须自然保留；result={result:?}");
    assert!(producer_fence, "成功首错还必须有真实producer停止屏障证据");
    assert!(world.handles.is_empty());
    assert_eq!(world.global.committed_tick, 101);
    assert_eq!(world.mirror_source(), source);
    assert_eq!(world.public_views, public);
    assert!(
        std::sync::Arc::ptr_eq(&publication, &world.publication.load()),
        "失败拍不得发布新版本"
    );
    let after = world.last_committed_calendar.as_ref().unwrap();
    assert!(std::sync::Arc::ptr_eq(&committed.calendar, &after.calendar));
    assert_eq!(committed.source, after.source);
    let finals = world
        .dead_finals
        .as_ref()
        .expect("完整真实Final不能只由join数推断");
    assert_eq!(finals.group_finals.len(), 3);
    assert_eq!(finals.desk_finals.len(), 2);
    let joined = finals
        .worker_results
        .iter()
        .map(|(who, _)| *who)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        joined,
        std::collections::HashSet::from([
            WorkerId::Group(0),
            WorkerId::Group(1),
            WorkerId::Group(2),
            WorkerId::Desk(0),
            WorkerId::Desk(1)
        ])
    );
    assert_eq!(finals.worker_results.len(), 5);
    for (who, result) in &finals.worker_results {
        if *who == WorkerId::Group(0) {
            assert!(
                matches!(result, Err(RuntimeError::ProtocolInvariant { detail }) if detail.contains("行政事务 pending"))
            );
        } else {
            assert_eq!(result, &Ok(()), "健康worker不得被自产清理错误拖死：{who:?}");
        }
    }
    for (group, expected) in accounts {
        let actual = finals
            .group_finals
            .iter()
            .find(|row| row.group == group)
            .unwrap();
        assert_eq!(actual.accounts, expected);
        assert_eq!(
            actual.plan_book_for_test,
            plans.iter().find(|(id, _)| *id == group).unwrap().1
        );
    }
    for desk in &finals.desk_finals {
        assert_eq!(desk.cur_tick_for_test, 101);
        assert_eq!(desk.last_applied_start_for_test, Some(102));
        assert!(desk.rollback_hash.is_some());
        assert!(desk.resting.is_empty());
    }
}

#[test]
fn pending_start_rejection_intrinsic_metadata_must_match_the_real_calendar_round() {
    use crate::runtime::coordinator::{converge_round, RoundOutcome};
    use crate::runtime::protocol::{PendingStartRejection, UpMsg, WorkerId};
    use crate::runtime::watchdog::WatchdogConfig;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    let mut world = world(false);
    world.run_ticks(1).unwrap();
    let calendar = world.prepare_raw_calendar_tick().unwrap();
    assert_eq!(calendar.before_input().tick, 101);
    assert_eq!(calendar.after_input().tick, 102);
    let source_error = RuntimeError::ProtocolInvariant {
        detail: "原始pending来源错误marker".to_owned(),
    };
    let mismatches = [
        ("错group", 3, 102, 101, 0),
        ("错tick", 0, 103, 101, 0),
        ("错prior_tick", 0, 102, 100, 0),
        ("错round", 0, 102, 101, 1),
    ];
    let watchdog = WatchdogConfig {
        normal: Duration::from_secs(1),
        long: Duration::from_secs(2),
    };
    for (case, group, tick, prior_tick, round) in mismatches {
        let (tx, rx) = mpsc::channel::<UpMsg>();
        tx.send(UpMsg::PendingStartRejected(PendingStartRejection {
            group,
            tick,
            prior_tick,
            calendar: Arc::clone(&calendar),
            error: source_error.clone(),
        }))
        .unwrap();
        let outcome = converge_round(&rx, 3, 2, 102, round, &watchdog);
        match outcome {
            RoundOutcome::Failed(RuntimeError::WorkerFailed { who, error }) => {
                assert_eq!(who, WorkerId::Group(group), "{case}: 必须保留来源组身份");
                assert!(
                    matches!(error.as_ref(), RuntimeError::ProtocolInvariant { detail }
                    if detail.contains("pending Start 拒绝阶段证据失配")
                        && detail.contains("原始pending来源错误marker")),
                    "{case}: 必须显式拒绝错位内证并保留原错误：{error:?}"
                );
            }
            _ => panic!("{case}: 错位内证不得被误转发为合法pending拒绝"),
        }
    }

    let (tx, rx) = mpsc::channel::<UpMsg>();
    tx.send(UpMsg::PendingStartRejected(PendingStartRejection {
        group: 1,
        tick: 102,
        prior_tick: 101,
        calendar: Arc::clone(&calendar),
        error: source_error.clone(),
    }))
    .unwrap();
    match converge_round(&rx, 3, 2, 102, 0, &watchdog) {
        RoundOutcome::PendingStartRejected(rejection) => {
            assert_eq!(rejection.group, 1);
            assert_eq!(rejection.tick, 102);
            assert_eq!(rejection.prior_tick, 101);
            assert!(
                Arc::ptr_eq(&rejection.calendar, &calendar),
                "合法证据须原样转交真实Calendar Arc"
            );
            assert_eq!(rejection.error, source_error, "合法证据须原样转交原始错误");
            let read_error = crate::runtime::world::prepared_stop::read_failure(
                "pending Start只读诊断",
                UpMsg::PendingStartRejected(rejection),
            );
            assert_eq!(
                read_error,
                RuntimeError::WorkerFailed {
                    who: WorkerId::Group(1),
                    error: Box::new(source_error.clone()),
                },
                "新错误消息的只读消费也必须保完整typed来源"
            );
        }
        _ => panic!("合法内证应原样转发且仅表示协议拒绝，不是producer证明"),
    }
    world.shutdown().unwrap();
}

#[test]
fn two_real_pending_admin_groups_fail_start_without_losing_primary_or_secondary_errors() {
    use crate::runtime::protocol::{AdminPhase, AdminTransactionId, UpMsg, WorkerId};
    use std::sync::Arc;

    let mut world = world(false);
    world.run_ticks(1).unwrap();
    let source = world.mirror_source();
    let publication = world.publication.load();
    let public = world.public_views.clone();
    let accounts = world.snapshot_accounts().unwrap();
    let plans = world.collect_plans().unwrap();
    let committed_calendar = world.committed_calendar_tick().unwrap().unwrap();
    assert_eq!(committed_calendar.source.tick, 101);
    let operations = [101, 202];
    for (group, amount) in operations.into_iter().enumerate() {
        let id = AdminTransactionId(99 + group as u64);
        world.group_txs[group]
            .send(crate::runtime::protocol::GroupIn::AdminPrepare {
                id,
                operations: vec![AdminOperation::CashDividend {
                    local: 0,
                    stock: 0,
                    amount,
                }],
                information: None,
                information_clock: None,
                repurchase: None,
                stage_layout: None,
            })
            .unwrap();
        match world
            .up_rx
            .recv_timeout(std::time::Duration::from_millis(1000))
            .unwrap()
        {
            UpMsg::AdminReceipt {
                id: actual,
                group: actual_group,
                phase: AdminPhase::Prepare,
                result: Ok(candidate),
            } => {
                assert_eq!(actual, id);
                assert_eq!(actual_group, group as crate::runtime::GroupIdx);
                assert_eq!(candidate.accounts.len(), 1);
                assert_eq!(candidate.accounts[0].0, 0);
                assert_eq!(
                    candidate.accounts[0].1.cash,
                    accounts[group].1[0].cash + amount
                );
            }
            other => panic!("group {group} 必须先返回真实非零pending Prepare Ack：{other:?}"),
        }
    }

    let result = world.run_ticks(1);
    match &result {
        Err(RuntimeError::ProtocolInvariant { detail }) => {
            assert!(
                detail.contains("pending Start"),
                "首错必须显式保留：{detail}"
            );
            assert!(
                detail.contains("另一条pending Start拒绝"),
                "次生真实pending拒绝必须显式登记：{detail}"
            );
            assert!(
                detail.contains("组 group-0 行政事务 pending"),
                "必须保留group0原始拒绝：{detail}"
            );
            assert!(
                detail.contains("组 group-1 行政事务 pending"),
                "必须保留group1原始拒绝：{detail}"
            );
        }
        other => panic!(
            "两组真实pending Start拒绝必须显式失败并携带次生错误：{other:?}；Final={:?}",
            world.dead_finals
        ),
    }
    assert!(world.handles.is_empty(), "fatal必须join全部真实worker");
    assert!(world.dead_reason.is_some());
    assert_eq!(world.global.committed_tick, 101);
    assert_eq!(world.mirror_source(), source);
    assert_eq!(world.public_views, public);
    assert!(
        Arc::ptr_eq(&publication, &world.publication.load()),
        "失败拍不得发布新版本"
    );
    let after_calendar = world.last_committed_calendar.as_ref().unwrap();
    assert!(Arc::ptr_eq(
        &committed_calendar.calendar,
        &after_calendar.calendar
    ));
    assert_eq!(committed_calendar.source, after_calendar.source);

    let finals = world.dead_finals.as_ref().expect("必须收到完整真实Final");
    assert_eq!(finals.group_finals.len(), 3);
    assert_eq!(finals.desk_finals.len(), 2);
    assert_eq!(finals.worker_results.len(), 5);
    let joined = finals
        .worker_results
        .iter()
        .map(|(who, _)| *who)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        joined,
        std::collections::HashSet::from([
            WorkerId::Group(0),
            WorkerId::Group(1),
            WorkerId::Group(2),
            WorkerId::Desk(0),
            WorkerId::Desk(1),
        ])
    );
    for (who, worker_result) in &finals.worker_results {
        match who {
            WorkerId::Group(0) | WorkerId::Group(1) => assert!(
                matches!(worker_result,
                Err(RuntimeError::ProtocolInvariant { detail }) if detail.contains("行政事务 pending")),
                "真实pending来源组必须保留原始Err：{who:?} {worker_result:?}"
            ),
            WorkerId::Group(2) | WorkerId::Desk(0) | WorkerId::Desk(1) => assert_eq!(
                worker_result,
                &Ok(()),
                "健康producer与交易部须真实正常退出：{who:?}"
            ),
            other => panic!("出现不在真实五worker名册的join：{other:?}"),
        }
    }
    for (group, expected) in accounts {
        let actual = finals
            .group_finals
            .iter()
            .find(|row| row.group == group)
            .unwrap();
        assert_eq!(actual.accounts, expected, "未提交分红overlay不得泄漏");
        assert_eq!(
            actual.plan_book_for_test,
            plans.iter().find(|(id, _)| *id == group).unwrap().1,
            "未提交overlay不得改变已提交PlanBook"
        );
    }
}
