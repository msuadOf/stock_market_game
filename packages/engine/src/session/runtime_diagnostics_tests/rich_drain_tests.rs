//! D3/D4 rich Drain 的真实短 fixture；不制造成交、来源或提交时钟。

use super::*;
use crate::diagnostics::causal::Termination;
use crate::runtime::drive::RuntimeDriver;
use crate::runtime::world::CommittedCalendarTick;
use crate::session::exchange_calendar_model::SharedDayProgress;
use std::sync::{Arc, Mutex};

const BUYER: AccountId = AccountId(0);
const SELLER: AccountId = AccountId(1);
const CODE: StockId = StockId(0);

fn fixture(closing: bool) -> (GameSession, RuntimeDriver) {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.stocks[0].exchange = StockExchange::Shanghai;
    setup.stocks[0].float_shares = 2_000;
    setup.float_allocation =
        FloatAllocation::class_percentages(1.0, 0.0, 0.0, WithinKindDistribution::EqualPercentage);
    setup.ticks_per_day = if closing { 6 } else { 4 };
    setup.auction_ticks = if closing { 1 } else { 0 };
    setup.closing_auction_ticks = if closing { 2 } else { 0 };
    setup.strategy_params.retail.arrival_rate = 0.0;
    let mut game = GameSession::new(setup, 21).expect("构造合法沪市单股日终 fixture");
    assert_eq!(game.account(BUYER).unwrap().kind(), AccountKind::Player);
    assert_eq!(game.account(SELLER).unwrap().kind(), AccountKind::Retail);
    assert_eq!(game.account(SELLER).unwrap().sellable_qty(&CODE), 2_000);
    assert!(game.account(BUYER).unwrap().positions().is_empty());
    game.save().expect("真实初始现金、旧仓及经历必须可存档");
    let mut driver = RuntimeDriver::from_session(&mut game).expect("接管真实名册");
    assert!(
        driver.committed_calendar_tick().unwrap().is_none(),
        "初始 authority 不能从静态投影补造市场提交记录"
    );
    driver.force_due_for_test_at(&[SELLER], 1);
    (game, driver)
}

fn limit(side: Side, price: i64, qty: u32) -> Intent {
    Intent::PlaceLimit {
        code: CODE,
        side,
        price: LimitPrice::Fixed(Money::from_cents(price)),
        qty,
    }
}

fn inject(driver: &mut RuntimeDriver, intents: Vec<(AccountId, Intent)>) {
    let upper = driver
        .world_public_views()
        .iter()
        .map(|view| view.daily_upper_limit)
        .collect::<Vec<_>>();
    let intents = intents
        .into_iter()
        .enumerate()
        .map(|(ordinal, (owner, intent))| ReceiptBearingIntent {
            owner,
            intent,
            account_ordinal: ordinal as u64,
            stock_ordinal: ordinal as u64,
        })
        .collect::<Vec<_>>();
    // 既有外部路由允许真实 Retail owner；不把 Retail 冒充 Player 或伪造策略下单。
    driver
        .inject_player_intents(&intents, &upper)
        .expect("路由合法整手与真实可卖股份");
}

fn accepted(events: &[Event], owner: AccountId, remaining: u32) -> OrderId {
    let ids = events
        .iter()
        .filter_map(|event| match event {
            Event::OrderAccepted {
                account,
                code,
                id,
                remaining_qty,
                ..
            } if *account == owner && *code == CODE && *remaining_qty == remaining => Some(*id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ids.len(),
        1,
        "必须有唯一真实受理订单，不能以合成 OrderId 代替"
    );
    ids[0]
}

fn prior_trace(game: &GameSession) -> serde_json::Value {
    let trace = game
        .npc_decision_trace(SELLER)
        .expect("真实 Retail 首拍 observe 必须已发布 trace");
    assert!(!trace.is_empty());
    serde_json::to_value(trace).unwrap()
}

fn committed_after(driver: &RuntimeDriver, tick: u64, date: CivilDate) -> CommittedCalendarTick {
    let committed = driver
        .committed_calendar_tick()
        .expect("读取真实 World 已提交记录")
        .expect("必须已有真实成功市场拍，不能使用 pending 或 preview");
    assert_eq!(committed.calendar.after_input().tick, tick);
    assert_eq!(
        committed.calendar.after_input().progress,
        SharedDayProgress::TradingCompleted
    );
    assert_eq!(committed.calendar.day().day_model().current_date(), date);
    assert_eq!(committed.source.tick, tick);
    assert_eq!(committed.source.civil_date, date);
    assert_eq!(
        committed.source.day_end_stage,
        crate::runtime::source::MirrorDayEndStage::Tick
    );
    let reread = driver.committed_calendar_tick().unwrap().unwrap();
    assert!(
        Arc::ptr_eq(&committed.calendar, &reread.calendar),
        "getter必须返回原 Arc，不能按已安装 Session 重建"
    );
    assert_eq!(
        committed.source, reread.source,
        "历史提交 source 不由 getter 重新盖章"
    );
    committed
}

fn assert_after_time(
    fact: &crate::diagnostics::causal::CausalFact,
    committed: &CommittedCalendarTick,
) {
    assert_eq!(
        fact.time.market_minute,
        committed
            .calendar
            .after_projection()
            .completed_shared_minute_count(),
        "FactTime 保 shared 单位，Drain 必须取已提交结束拍的真实 after 投影"
    );
    assert_eq!(
        fact.time.civil,
        committed.calendar.observation_instant_after(),
        "civil 必须来自原结束拍，不从次日 Session 或常量倒推"
    );
    assert_eq!(fact.time.phase, committed.calendar.phase_before());
    let clock = fact
        .time
        .stock_clock
        .expect("真实 Drain 证券事实必须携本股 clock");
    assert_eq!(Some(clock.code), fact.kind.stock_code());
    assert_eq!(
        clock.completed_minutes,
        committed
            .calendar
            .after_projection()
            .stock(clock.code)
            .unwrap()
            .completed_minute_count()
    );
}

fn assert_retained_current_source(driver: &RuntimeDriver, committed: &CommittedCalendarTick) {
    let current = driver.mirror_source();
    assert_eq!(current.authority_epoch, committed.source.authority_epoch);
    assert_eq!(current.tick, committed.calendar.after_input().tick);
    assert_eq!(
        current.day_end_stage,
        crate::runtime::source::MirrorDayEndStage::RetainedPublished
    );
    assert!(
        current.generation > committed.source.generation,
        "retained 的当前发布与历史 Tick source 分别保真，不能重 stamp 历史或强求 generation 全等"
    );
}

fn acquisition_ids(game: &GameSession) -> Vec<crate::information::PublicationId> {
    game.state.belief_participants[&SELLER]
        .information()
        .companies()
        .flat_map(|(_, records)| records.iter().map(|record| record.id))
        .collect()
}

fn assert_civil_acquisitions(
    game: &GameSession,
    before: &[crate::information::PublicationId],
    facts: &[crate::diagnostics::causal::CausalFact],
    committed: &CommittedCalendarTick,
) {
    // 从本人新增获知记录与公开库独立构造期望，不用 causal 输出反推期望或去重掩盖重复。
    let mut expected = Vec::new();
    for (company, records) in game.state.belief_participants[&SELLER]
        .information()
        .companies()
    {
        for record in records.iter().filter(|record| !before.contains(&record.id)) {
            let published = match record.kind {
                crate::information::AcquiredKind::Report => {
                    game.state
                        .library
                        .report(record.id, record.observed_at)
                        .unwrap()
                        .published_at
                }
                crate::information::AcquiredKind::Announcement => {
                    game.state
                        .library
                        .announcement(record.id, record.observed_at)
                        .unwrap()
                        .published_at
                }
            };
            expected.push((
                *company,
                u64::from(record.id.value()),
                published,
                record.observed_at,
            ));
        }
    }
    assert!(
        !expected.is_empty(),
        "fixture 必须真实经过自然日披露与本人首次获知"
    );
    let mut actual = facts.iter().map(|fact| {
        let CausalFactKind::Acquisition { account, company, publication, published, acquired } = &fact.kind else {
            panic!("自然日增量只能是实际新增 Acquisition，不能重放 Decision、Drain 或其他 Tick 事实：{fact:?}");
        };
        assert_eq!(*account, SELLER);
        assert_eq!(acquired.date(), committed.calendar.day().day_model().current_date());
        assert_eq!(acquired.second_of_day(), 18 * 60 * 60, "本 fixture 的获知必须来自真实 18:00 披露窗口");
        assert_eq!(fact.time.stock_clock, None, "INFO 不得携带证券时钟");
        assert_eq!(fact.time.civil, *acquired);
        assert_eq!(fact.time.phase, committed.calendar.phase_before());
        assert_eq!(fact.time.market_minute, committed.calendar.after_projection().completed_shared_minute_count());
        (*company, *publication, *published, *acquired)
    }).collect::<Vec<_>>();
    expected.sort_by_key(|entry| (entry.0 .0, entry.1));
    actual.sort_by_key(|entry| (entry.0 .0, entry.1));
    assert_eq!(
        actual, expected,
        "每一条本次真实获知必须恰好交付一次，身份、publication 与公布/获知时点全部保真"
    );
}

#[test]
fn d3_rich_day_end_preserves_original_partial_order_and_ends_only_remaining_200() {
    let (mut game, mut driver) = fixture(false);
    inject(
        &mut driver,
        vec![
            (SELLER, limit(Side::Sell, 1_000, 100)),
            (BUYER, limit(Side::Buy, 1_000, 300)),
        ],
    );
    let first = game
        .runtime_step_with_install(&mut driver)
        .expect("真实连续竞价先成交 100 股");
    let order = accepted(&first, BUYER, 200);
    assert_eq!(
        first
            .iter()
            .filter(|event| matches!(event, Event::Trade { qty: 100, .. }))
            .count(),
        1
    );
    let prior = prior_trace(&game);
    driver.defer_due_for_test_at(&[SELLER], 1_000);
    for _ in 0..2 {
        game.runtime_step_with_install(&mut driver)
            .expect("无新观察的真实连续交易拍");
    }
    assert_eq!(game.tick(), 3);
    let ending_date = game.civil_date();
    let previous_count = game.causal_facts().len();
    let events = game
        .runtime_step_with_install(&mut driver)
        .expect("真实余单必须经过日终 Drain，不能停在能力拒绝");
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::OrderCanceled {
        account, code, id, remaining_qty: 200, ..
    } if *account == BUYER && *code == CODE && *id == order)));
    let terminated = game.causal_facts()[previous_count..].iter().filter(|fact| matches!(fact.kind,
        CausalFactKind::Terminated { order: id, account: BUYER, code: CODE, qty: 200, reason: Termination::DayEnd }
            if id == order)).collect::<Vec<_>>();
    assert_eq!(
        terminated.len(),
        1,
        "日终只终止原订单余 200 股，不撤销已成交的 100 股"
    );
    assert_eq!(
        terminated[0].time.civil.date(),
        ending_date,
        "Drain 归结束日，不能归次日"
    );
    assert_eq!(terminated[0].time.phase, TradingPhase::Continuous);
    let committed = committed_after(&driver, 4, ending_date);
    assert_after_time(terminated[0], &committed);
    assert_retained_current_source(&driver, &committed);
    assert_eq!(
        prior_trace(&game),
        prior,
        "无 observe 的 Drain 不制造或重放 Decision"
    );
    let report = game
        .causal_diagnostics()
        .expect("真实 Submitted/Filled/DayEnd 必须完整对账");
    let lifecycle = report
        .orders
        .iter()
        .find(|row| row.origin.order == order)
        .unwrap();
    assert_eq!(
        (
            lifecycle.origin.qty,
            lifecycle.filled_qty,
            lifecycle.canceled_qty,
            lifecycle.open_qty
        ),
        (300, 100, 200, 0)
    );
    assert_eq!(lifecycle.terminal_reason, Some(Termination::DayEnd));
    assert!(driver.collect_resting_facts_live().unwrap()[0].is_empty());
    let before_civil_count = game.causal_facts().len();
    let before_civil = serde_json::to_value(game.causal_facts()).unwrap();
    let known = acquisition_ids(&game);
    game.end_civil_day()
        .expect("低层市场日结完成后，真实推进自然日再运行次日首拍");
    assert_eq!(
        serde_json::to_value(&game.causal_facts()[..before_civil_count]).unwrap(),
        before_civil,
        "自然日推进保留完整原事实前缀"
    );
    assert_civil_acquisitions(
        &game,
        &known,
        &game.causal_facts()[before_civil_count..],
        &committed,
    );
    let after = serde_json::to_value(game.causal_facts()).unwrap();
    game.runtime_step_with_install(&mut driver)
        .expect("次日首拍不能再消费旧 Drain");
    assert_eq!(serde_json::to_value(game.causal_facts()).unwrap(), after);
}

#[test]
fn d4_rich_closing_execution_uses_real_internal_before_no_aggressor_and_day_end_remainder() {
    let (mut game, mut driver) = fixture(true);
    for tick in 1..=4 {
        game.runtime_step_with_install(&mut driver)
            .expect("真实开盘与连续阶段，无虚构成交");
        if tick == 1 {
            driver.defer_due_for_test_at(&[SELLER], 1_000);
        }
    }
    let trace = prior_trace(&game);
    let ending_date = game.civil_date();
    inject(
        &mut driver,
        vec![
            (SELLER, limit(Side::Sell, 1_000, 100)),
            (BUYER, limit(Side::Buy, 1_000, 300)),
        ],
    );
    let submitted = game
        .runtime_step_with_install(&mut driver)
        .expect("收盘限价申报必须携真实来源，不能丢 rich Submitted");
    let buy = accepted(&submitted, BUYER, 300);
    let sell = accepted(&submitted, SELLER, 100);
    assert!(
        !submitted
            .iter()
            .any(|event| matches!(event, Event::Trade { .. })),
        "收盘申报阶段不能提前成交"
    );
    let count = game.causal_facts().len();
    let events = game
        .runtime_step_with_install(&mut driver)
        .expect("末拍必须提交收盘单一价清算与余单失效");
    assert!(events.iter().any(|event| matches!(event, Event::AuctionCompleted {
        phase: TradingPhase::ClosingAuction, code: CODE, clearing_price: Some(price), matched_volume: 100, ..
    } if price.cents() == 1_000)));
    assert_eq!(events.iter().filter(|event| matches!(event, Event::Trade { code: CODE, qty: 100, price, maker: SELLER, taker: BUYER, .. }
        if price.cents() == 1_000)).count(), 1);
    let facts = &game.causal_facts()[count..];
    let executions = facts
        .iter()
        .filter_map(|fact| match &fact.kind {
            CausalFactKind::Execution {
                code: CODE,
                maker,
                taker,
                side,
                qty,
                price_cents,
                before,
            } => Some((fact, maker, taker, side, qty, price_cents, before)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(executions.len(), 1);
    let (fact, maker, taker, side, qty, price, before) = executions[0];
    assert_eq!((*maker, *taker, *qty, *price), (sell, buy, 100, 1_000));
    assert_eq!(
        *side, None,
        "集合竞价没有主动方向，不能借 settle_fill 的买腿实现细节填 Buy"
    );
    // 这里记录真实内部全簿，不是公开 StockPublic 的竞价披露盘口。
    assert_eq!(
        (
            before.bid_cents,
            before.ask_cents,
            before.bid_depth,
            before.ask_depth
        ),
        (Some(1_000), Some(1_000), 300, 100)
    );
    assert_eq!(fact.time.civil.date(), ending_date);
    assert_eq!(fact.time.phase, TradingPhase::ClosingAuction);
    let committed = committed_after(&driver, 6, ending_date);
    for closing_fact in facts {
        assert_after_time(closing_fact, &committed);
    }
    assert_retained_current_source(&driver, &committed);
    for (order, account) in [(sell, SELLER), (buy, BUYER)] {
        assert_eq!(
            facts
                .iter()
                .filter(|fact| matches!(fact.kind, CausalFactKind::Filled {
            order: id, account: actual, code: CODE, qty: 100, value_before: 0, gross: 100_000,
        } if id == order && actual == account))
                .count(),
            1
        );
    }
    assert_eq!(
        facts
            .iter()
            .filter(|fact| matches!(fact.kind, CausalFactKind::Terminated {
        order, account: BUYER, code: CODE, qty: 200, reason: Termination::DayEnd,
    } if order == buy))
            .count(),
        1
    );
    let report = game
        .causal_diagnostics()
        .expect("收盘双腿、单一价与余量必须守恒");
    assert_eq!(
        report.filled_qty, 200,
        "一笔 100 股成交对应真实双腿各 100 股"
    );
    assert_eq!(
        report.impacts.len(),
        1,
        "唯一真实 Execution 必须有唯一报告项，不能以空数组通过"
    );
    let impact = &report.impacts[0];
    assert_eq!(impact.execution_sequence, fact.sequence);
    assert_eq!(impact.signed_observational_bp, None);
    assert_eq!(impact.absent_reason, Some("auction_has_no_aggressor"));
    assert_eq!(
        report.direction_persistence, None,
        "收盘集合竞价不能被计为主动方向持续性"
    );
    assert_eq!(prior_trace(&game), trace);
    let receipts = game
        .personal_trade_confirmations(BUYER)
        .expect("真实收盘个人确认");
    assert_eq!(receipts.len(), 1);
    assert_eq!(
        (
            receipts[0].civil_date,
            receipts[0].quantity_shares,
            receipts[0].gross_cents
        ),
        (ending_date, 100, 100_000)
    );
    assert!(driver.collect_resting_facts_live().unwrap()[0].is_empty());
}

#[test]
fn d3_rich_manual_late_civil_failure_keeps_unpublished_prior_but_installed_accounts_receipts_retained(
) {
    let (mut game, mut driver) = fixture(true);
    game.runtime_step_with_install(&mut driver)
        .expect("真实首拍 observe");
    driver.defer_due_for_test_at(&[SELLER], 1_000);
    inject(&mut driver, vec![(BUYER, limit(Side::Buy, 990, 100))]);
    let accepted_events = game
        .runtime_step_with_install(&mut driver)
        .expect("先建立非空 rich prior");
    let prior_order = accepted(&accepted_events, BUYER, 100);
    inject(
        &mut driver,
        vec![(
            BUYER,
            Intent::Cancel {
                code: CODE,
                id: prior_order,
            },
        )],
    );
    game.runtime_step_with_install(&mut driver)
        .expect("真实主动撤单完成 prior，不留额外余单");
    game.runtime_step_with_install(&mut driver)
        .expect("最后连续拍");
    let rich_prior = serde_json::to_value(game.causal_facts()).unwrap();
    assert!(game.causal_facts().iter().any(|fact| matches!(
        fact.kind,
        CausalFactKind::Terminated {
            reason: Termination::Voluntary,
            ..
        }
    )));
    let trace_prior = prior_trace(&game);
    inject(
        &mut driver,
        vec![
            (SELLER, limit(Side::Sell, 1_000, 100)),
            (BUYER, limit(Side::Buy, 1_000, 300)),
        ],
    );
    // 既有低层 caller 保真实已提交末拍，但不调用自动日界外层；随后手动 API 的 civil 失败必须晚于安装。
    for _ in 0..2 {
        driver
            .step_frame_equivalent(&mut game)
            .expect("真实收盘申报拍，不伪造 TickRecord");
        driver.install_market_views(&mut game).unwrap();
        driver.install_candles(&mut game).unwrap();
    }
    assert_eq!(game.tick(), 6);
    let committed_before_manual = committed_after(&driver, 6, game.civil_date());
    assert!(
        game.account(BUYER).unwrap().positions().is_empty(),
        "收盘撮合尚未执行"
    );
    let candidate = Arc::new(Mutex::new(None));
    let hit = Arc::clone(&candidate);
    driver.set_day_end_validator_for_test(Box::new(move |actual| {
        *hit.lock().unwrap() = Some(actual.clone());
        Ok(())
    }));
    let publication = driver.publication();
    let generation = publication.load().generation;
    let date = game.civil_date();
    let receipts_before = game
        .personal_trade_confirmations(BUYER)
        .map_or(0, |rows| rows.len());
    let error = driver
        .day_end(&mut game)
        .expect_err("此手动入口不推进 market day，civil 必须晚失败");
    assert!(
        candidate.lock().unwrap().is_some(),
        "必须真实越过 Drain 和日结 validator；最早失败不能冒晚失败：{error}"
    );
    assert!(error.contains("自然日推进失败"), "{error}");
    let committed_after_manual = committed_after(&driver, 6, date);
    assert!(
        Arc::ptr_eq(
            &committed_before_manual.calendar,
            &committed_after_manual.calendar
        ),
        "ManualDayEnd 只有 DayArc，不得伪造新的 TickArc"
    );
    assert_eq!(
        committed_before_manual.source, committed_after_manual.source,
        "manual日结与retained不得重 stamp 历史市场提交 source"
    );
    assert_eq!(
        driver.world_committed_tick(),
        6,
        "手动日结不运行或补造 tick"
    );
    assert_retained_current_source(&driver, &committed_after_manual);
    let fatal = game
        .poison_reason()
        .expect("安装后的晚失败必须 poison")
        .clone();
    assert!(
        matches!(&fatal, StepFatal::InvariantViolation { location, description }
        if location == "RuntimeDriver::day_end::civil" && description.contains("自然日推进失败")),
        "{fatal:?}"
    );
    assert_eq!(
        serde_json::to_value(game.causal_facts()).unwrap(),
        rich_prior,
        "未发布 rich prior 不能被新 Drain 覆盖或追加"
    );
    assert_eq!(prior_trace(&game), trace_prior);
    let position = &game.account(BUYER).unwrap().positions()[&CODE];
    assert_eq!(
        (
            position.qty(),
            position.t1_locked(),
            position.invested_cents()
        ),
        (100, 0, 100_000),
        "已安装账户与 T+1 日结事实不能伪装回滚"
    );
    let receipts = game
        .personal_trade_confirmations(BUYER)
        .expect("已安装个人确认");
    assert_eq!(receipts.len(), receipts_before + 1);
    assert_eq!(
        (
            receipts.last().unwrap().civil_date,
            receipts.last().unwrap().quantity_shares
        ),
        (date, 100)
    );
    let retained = publication.load();
    assert_eq!(retained.tick, 6);
    assert!(retained.generation > generation);
    assert_eq!(retained.retained.days.len(), 1, "已发布 retained 不撤回");
    assert_eq!(game.civil_date(), date);
    assert!(
        matches!(game.save().unwrap_err(), crate::SessionError::Step(actual) if actual == fatal)
    );
    assert_eq!(driver.step_frame_equivalent(&mut game).unwrap_err(), fatal);
    assert_eq!(
        driver.day_end(&mut game).unwrap_err(),
        fatal.to_string(),
        "不能复活原 poison 实例重跑"
    );
    assert!(Arc::ptr_eq(
        &publication.load().retained,
        &retained.retained
    ));
    assert_eq!(
        serde_json::to_value(game.causal_facts()).unwrap(),
        rich_prior
    );
}

#[test]
fn d3_rich_healthy_manual_publishes_only_real_drain_and_keeps_pending_tick_roots_unpublished() {
    let (mut game, mut driver) = fixture(false);
    inject(
        &mut driver,
        vec![
            (SELLER, limit(Side::Sell, 1_000, 100)),
            (BUYER, limit(Side::Buy, 1_000, 300)),
        ],
    );
    let first = game.runtime_step_with_install(&mut driver).unwrap();
    let order = accepted(&first, BUYER, 200);
    let trace = prior_trace(&game);
    let count = game.causal_facts().len();
    let prior = serde_json::to_value(game.causal_facts()).unwrap();
    let known = acquisition_ids(&game);
    driver.force_due_for_test_at(&[SELLER], 4);
    for _ in 0..3 {
        driver.step_frame_equivalent(&mut game).unwrap();
        driver.install_market_views(&mut game).unwrap();
        driver.install_candles(&mut game).unwrap();
    }
    assert!(
        !driver.pending_decision_traces().is_empty(),
        "末拍必须真有未交付 Decision，才能证明 manual 不重吃 Tick 根"
    );
    let date = game.civil_date();
    let committed = committed_after(&driver, 4, date);
    driver
        .day_end_with_runtime_market_day(&mut game)
        .expect("合法手动日结必须发布本次 Drain");
    assert_eq!(
        serde_json::to_value(&game.causal_facts()[..count]).unwrap(),
        prior,
        "manual 保留完整原事实前缀"
    );
    let (fact, civil_facts) = game.causal_facts()[count..]
        .split_last()
        .expect("必须交付本次真实 Drain");
    assert_civil_acquisitions(&game, &known, civil_facts, &committed);
    assert!(
        matches!(fact.kind, CausalFactKind::Terminated { order: id, account: BUYER, code: CODE, qty: 200, reason: Termination::DayEnd } if id == order)
    );
    assert_after_time(fact, &committed);
    assert_eq!(
        prior_trace(&game),
        trace,
        "manual mirror 同样不得追加旧 Tick trace"
    );
    assert!(driver.pending_decision_traces().is_empty());
    let reread = driver.committed_calendar_tick().unwrap().unwrap();
    assert!(Arc::ptr_eq(&committed.calendar, &reread.calendar));
    assert_eq!(committed.source, reread.source);
    let lifecycle = game
        .causal_diagnostics()
        .unwrap()
        .orders
        .into_iter()
        .find(|row| row.origin.order == order)
        .unwrap();
    assert_eq!(
        (
            lifecycle.filled_qty,
            lifecycle.canceled_qty,
            lifecycle.open_qty
        ),
        (100, 200, 0)
    );
    assert_eq!(game.tick(), 4);
    assert_eq!(game.day(), 1);
    assert!(game.civil_date() > date);
    let after = serde_json::to_value(game.causal_facts()).unwrap();
    driver.defer_due_for_test_at(&[SELLER], 1_000);
    game.runtime_step_with_install(&mut driver).unwrap();
    assert_eq!(
        serde_json::to_value(game.causal_facts()).unwrap(),
        after,
        "下一拍不得再发旧 Drain"
    );
}

#[test]
fn d3_rich_external_driver_keeps_authority_through_closed_days_and_next_step() {
    // Closed 日没有既存 retained；不能让日终尾部又按全局 native 开关另建 authority。
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.start_date = CivilDate::from_ymd(2030, 1, 5).unwrap();
    setup.ticks_per_day = 4;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.strategy_params.retail.arrival_rate = 0.0;
    let mut closed = GameSession::new(setup, 21).expect("合法周末 Closed 日 fixture");
    let mut closed_driver =
        RuntimeDriver::from_session(&mut closed).expect("外持 Closed 日唯一 driver");
    let closed_epoch = closed_driver.mirror_source().authority_epoch;
    assert!(closed_epoch > 0);
    assert_eq!(closed.runtime_authority_epoch, closed_epoch);
    assert!(
        closed.runtime_driver.is_none(),
        "fixture 必须由调用方持有唯一 driver"
    );
    assert!(!closed.state.runtime_information_authority);
    assert!(closed.state.retained_market_days.is_empty());
    for _ in 0..2 {
        closed
            .end_civil_day()
            .expect("外持 driver 的周末自然日推进");
        assert!(
            closed.runtime_driver.is_none(),
            "Closed 日尾部不得暗建内部 driver"
        );
        assert_eq!(closed.runtime_authority_epoch, closed_epoch);
    }
    assert_eq!(
        closed.civil_date(),
        CivilDate::from_ymd(2030, 1, 7).unwrap()
    );
    let next = closed.runtime_step_with_install(&mut closed_driver);
    assert!(
        next.is_ok(),
        "Closed 日推进后的原外持 driver 必须继续同一 authority：{next:?}"
    );
    assert_eq!(closed.runtime_authority_epoch, closed_epoch);
    assert_eq!(closed_driver.mirror_source().authority_epoch, closed_epoch);
    assert_eq!(closed.tick(), 1);
}

#[test]
fn d3_rich_auto_and_manual_prepare_failure_preserve_drain_raw_before_abort() {
    use crate::runtime::{diagnostics::DeskCausalKind, retail_lifecycle::BatchContext};
    for manual in [false, true] {
        let (mut game, mut driver) = fixture(false);
        inject(
            &mut driver,
            vec![
                (SELLER, limit(Side::Sell, 1_000, 100)),
                (BUYER, limit(Side::Buy, 1_000, 300)),
            ],
        );
        let first = game.runtime_step_with_install(&mut driver).unwrap();
        let order = accepted(&first, BUYER, 200);
        driver.defer_due_for_test_at(&[SELLER], 1_000);
        for _ in 0..2 {
            game.runtime_step_with_install(&mut driver).unwrap();
        }
        if manual {
            driver.step_frame_equivalent(&mut game).unwrap();
            driver.install_market_views(&mut game).unwrap();
            driver.install_candles(&mut game).unwrap();
        }
        let date = game.civil_date();
        let rich_prior = serde_json::to_value(game.causal_facts()).unwrap();
        let trace = prior_trace(&game);
        let accounts = format!("{:?}", game.runtime_accounts_snapshot());
        let publication = driver.publication();
        let retained = Arc::clone(&publication.load().retained);
        // 既有真实序号耗尽门位于 retained 发布之前；不伪造交易事实或 clock。
        game.runtime_mirror_install_sequence = u64::MAX;
        if manual {
            let error = driver
                .day_end_with_runtime_market_day(&mut game)
                .unwrap_err();
            assert!(error.contains("install_sequence 已耗尽"), "{error}");
        } else {
            let fatal = game.runtime_step_with_install(&mut driver).unwrap_err();
            assert!(
                fatal.to_string().contains("install_sequence 已耗尽"),
                "{fatal}"
            );
        }
        let fatal = game.poison_reason().unwrap().clone();
        let terminal = driver
            .terminal_causal_snapshot_for_test()
            .expect("Abort 前必须封存真实 Drain");
        assert_eq!(terminal.2, fatal);
        assert!(terminal.0.is_some(), "真实 Drain 时间必须随失败证据封存");
        let ends = terminal
            .1
            .iter()
            .filter(|fact| {
                matches!(fact.kind, DeskCausalKind::DayEnded { order: id, qty: 200, .. }
            if crate::OrderId((u64::from(id.0) << 32) | id.1) == order)
            })
            .collect::<Vec<_>>();
        assert_eq!(ends.len(), 1);
        assert_eq!(
            ends[0].context,
            BatchContext::Drain {
                ended_date: date,
                final_tick: 4,
                stock: 0
            }
        );
        if manual {
            assert!(terminal
                .1
                .iter()
                .all(|fact| matches!(fact.context, BatchContext::Drain { .. })));
        }
        assert_eq!(
            serde_json::to_value(game.causal_facts()).unwrap(),
            rich_prior
        );
        assert_eq!(prior_trace(&game), trace);
        assert_eq!(format!("{:?}", game.runtime_accounts_snapshot()), accounts);
        assert!(
            Arc::ptr_eq(&publication.load().retained, &retained),
            "retained 前拒绝仍走真实 Abort"
        );
        let live = driver.collect_resting_facts_live().unwrap();
        assert_eq!(live[0].len(), 1);
        assert_eq!(live[0][0].qty, 200, "真实日结余单在可逆窗口恢复");
        assert_eq!(driver.world_committed_tick(), 4, "已提交普通市场拍不回退");
        assert!(
            matches!(game.save().unwrap_err(), crate::SessionError::Step(actual) if actual == fatal)
        );
        assert_eq!(driver.step_frame_equivalent(&mut game).unwrap_err(), fatal);
    }
}
