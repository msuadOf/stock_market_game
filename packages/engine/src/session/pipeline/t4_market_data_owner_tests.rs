//! T4（ADR-0042 §6-T4「行情」面）股票 owner 行情 patch 测试。
//!
//! 契约：本股 owner（IncrementalContinuousStockShadow）在收尾内**计算**本股
//! 行情——活动日 K 终值（含逐笔/收盘标记与溢出预检）、price_history 终值
//! （追加+按 history_len 裁剪）、market_minute_closes 回填条目、
//! PriceTick/AuctionTick/AuctionCompleted 事件；`finalize_continuous_tick`
//! 只做机械安装（同位安装保持事件顺序）。结构性断言：`ContinuousStockOutput`
//! 直载 `market_data` patch（回退=字段删除即编译可见）。

use super::continuous_matching_adapter::{
    market_data_priors, prepare_incremental_continuous_inputs_on,
};
use super::continuous_tick_finalizer::ContinuousTickBoundary;
use super::stock_stream::{continuous_shards, finish_market_shards};
use crate::{GameSession, StockId};

/// quote_setup(0) 变体：auction_ticks=0 且 closing_auction_ticks=0 ⇒ 全窗
/// Continuous（与 t3_boundary civil fixture 同构）。
fn continuous_session() -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.ticks_per_day = 120;
    GameSession::new(setup, 42).unwrap()
}

fn exchanges(session: &GameSession) -> std::collections::BTreeMap<StockId, crate::StockExchange> {
    session
        .state
        .setup
        .stocks
        .iter()
        .map(|spec| {
            (
                session.state.stocks.id(&spec.code.0).unwrap(),
                spec.exchange,
            )
        })
        .collect()
}

/// owner 收尾直接产出本股行情 patch（结构+行为双断言；TDD 红→绿载体）。
#[test]
fn stock_owner_finish_computes_market_data_patch_for_continuous_tick() {
    let session = continuous_session();
    let view = session.read_view();
    let inputs = prepare_incremental_continuous_inputs_on(&view).unwrap();
    let priors = market_data_priors(&view);
    assert_eq!(priors.len(), session.state.markets.len(), "先验面=全部股票");
    let shards = continuous_shards(inputs, priors).unwrap();
    let boundary = ContinuousTickBoundary::capture(&session).unwrap();
    let exchanges = exchanges(&session);
    let finish = finish_market_shards(shards, boundary, &exchanges).unwrap();
    assert!(!finish.workers.is_empty(), "每股一个 worker 输出");

    let history_len = session.state.setup.history_len;
    let completed = usize::from(boundary.completed_minutes.unwrap());
    for worker in &finish.workers {
        let code = worker.market.code();
        let prior_minutes = session.state.market_minute_closes[&code].len();
        let patch = &worker.market_data;
        // 无成交 Continuous tick：owner 仍完成收盘标记、历史追加与分钟回填。
        let candle = patch
            .candle
            .as_ref()
            .unwrap_or_else(|| panic!("股票 {code:?} 缺少活动日 K 终值"));
        assert_eq!(candle.close, worker.market.last_price());
        assert_eq!(candle.volume, 0, "无成交 tick 的收盘标记不增加成交量");
        let history = patch
            .price_history
            .as_ref()
            .unwrap_or_else(|| panic!("股票 {code:?} 缺少价格历史终值"));
        assert!(!history.is_empty());
        assert!(history.len() <= history_len, "历史长度受 history_len 约束");
        assert_eq!(
            *history.back().unwrap(),
            worker.market.last_price(),
            "历史末值=收盘标记价"
        );
        let minutes = patch
            .minute_closes
            .as_ref()
            .unwrap_or_else(|| panic!("股票 {code:?} 缺少分钟终值"));
        assert_eq!(
            minutes.len(),
            completed,
            "分钟终值长度=completed（prior+本 tick 回填）"
        );
        for (index, close) in minutes.iter().enumerate().skip(prior_minutes) {
            assert_eq!(
                close.absolute_trading_minute
                    % u64::from(crate::observation::GAME_INTRADAY_MINUTES_PER_DAY),
                index as u64,
                "绝对交易分钟=日界基线+序号"
            );
            assert_eq!(close.close, worker.market.last_price());
        }
        // 行情事件：恰一条 PriceTick，daily_candle=owner 终值（同一事实单一载体）。
        assert_eq!(patch.events.len(), 1, "Continuous tick 每股恰一条 PriceTick");
        match &patch.events[0].event {
            crate::Event::PriceTick {
                code: event_code,
                daily_candle,
                last_price,
                ..
            } => {
                assert_eq!(event_code, code);
                assert_eq!(daily_candle, candle, "事件内日 K=owner 终值");
                assert_eq!(*last_price, worker.market.last_price());
            }
            other => panic!("期望 PriceTick，实际 {other:?}"),
        }
    }
}

/// 安装保真：真实 step() 后 session 行情状态==owner patch 语义（等价钉）。
#[test]
fn finalize_installs_owner_market_data_patch_into_session() {
    let mut session = continuous_session();
    let code = session.state.markets.keys().next().unwrap().clone();
    let pre_history_len = session.state.price_history[&code].len();
    let pre_minutes = session.state.market_minute_closes[&code].len();
    let _ = session.step().expect("健康 tick");
    let history_len = session.state.setup.history_len;
    let history = &session.state.price_history[&code];
    assert_eq!(
        history.len(),
        (pre_history_len + 1).min(history_len),
        "安装后历史长度=追加并裁剪"
    );
    assert_eq!(
        *history.back().unwrap(),
        session.state.markets[&code].last_price()
    );
    let minutes = &session.state.market_minute_closes[&code];
    assert!(minutes.len() >= pre_minutes, "分钟回填单调不减");
    let candle = session
        .state
        .candle_book
        .active()
        .get(&code)
        .expect("收盘标记后必有活动日 K");
    assert_eq!(candle.close, session.state.markets[&code].last_price());
}


// ――― Luna 快检四项覆盖缺口补齐（增量七十一）―――
// 竞价事件顺序/竞价无成交 K 线标记/真实成交 candle 统计/分钟历史前缀保留。

use super::continuous_matching::{ContinuousEnvelopeSnapshot, StockMarketDataPrior};
use super::continuous_matching::incremental_continuous_stock_shadow::tests::{
    validated_operations_in_session,
};
use crate::session::pipeline::EnvelopeLedger;
use crate::{AccountId, Money, OrderId, Side};


/// add_resting 的限价参数化变体（测试内局部构造；与共享 helper 同构）。
fn add_resting_at(
    market: &mut crate::Market,
    code: &StockId,
    owner: AccountId,
    order_id: OrderId,
    price: Money,
    qty: u32,
) -> ContinuousEnvelopeSnapshot {
    use crate::session::pipeline::{Envelope, EnvelopeAudit, EnvelopeKey, FeeComponents, ResVec};
    let order = crate::Order {
        id: order_id,
        side: Side::Sell,
        price,
        qty,
        original_qty: qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        owner,
        seq: 0,
    };
    let result = market.place(order).unwrap();
    assert!(result.trades.is_empty());
    assert!(result.resting.is_some());
    let key = EnvelopeKey {
        account: owner,
        stock: code.clone(),
        order: order_id,
        side: Side::Sell,
    };
    let audit = EnvelopeAudit {
        limit: price,
        remaining_qty: qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
    };
    let live = ResVec::new(Money::ZERO, qty);
    ContinuousEnvelopeSnapshot {
        envelope: Envelope::tick_start_existing(key, live.cash, live.shares, audit),
    }
}

/// 真实成交（最重要的缺口）：owner 直跑见证——两笔不同价成交后 finish 的
/// candle 终值统计（首笔重置占位/累计路径）与 PriceTick 内日 K 同源。
#[test]
fn owner_finish_candle_statistics_under_real_trades() {
    for (prior, expect_prior_high) in [
        (None, Money::from_cents(1_010)),
        (Some((Money::from_cents(900), Money::from_cents(950), 40_u64)), Money::from_cents(1_010)),
        // Luna 建议：先验 high 高于全部成交价 → 保留分支。
        (Some((Money::from_cents(900), Money::from_cents(1_200), 40_u64)), Money::from_cents(1_200)),
    ] {
        let mut game = continuous_session();
        let code = game.state.markets.keys().next().unwrap().clone();
        // 卖方账户真实持仓背书（allocation seal 的诚实不变量要求
        // ledger 卖出预留不超可卖量）。
        let moment = crate::experience::ExperienceMoment {
            civil_date: game.civil_date(),
            market_minute: game.current_market_minute(),
            trading_day: u64::from(game.day()),
        };
        game.fixture_account_mut(&AccountId(1))
            .unwrap()
            .grant_position(code.clone(), 200, Money::from_cents(1_000))
            .unwrap();
        // inst 账户无 retail_experience 记忆（quote_setup inst_count=1）；
        // 持仓背书只需账户 position，无需零售持有期初始化。
        let _ = moment;
        let makers = [
            add_resting_at(
                game.fixture_market_mut(&code).unwrap(),
                &code,
                AccountId(1),
                OrderId(100),
                Money::from_cents(1_000),
                50,
            ),
            add_resting_at(
                game.fixture_market_mut(&code).unwrap(),
                &code,
                AccountId(1),
                OrderId(101),
                Money::from_cents(1_010),
                50,
            ),
        ];
        game.state.envelope_ledger =
            EnvelopeLedger::new(0, makers.iter().map(|maker| maker.envelope.clone())).unwrap();
        game.state.next_receipt_base = 0;
        game.state.next_order_id = 10_000;
        if let Some((open, close, volume)) = prior {
            game.state.candle_book.set_active(
                code.clone(),
                crate::DailyCandle {
                    time: 0,
                    open,
                    high: expect_prior_high,
                    low: open,
                    close,
                    volume,
                    trade_stats: Some(crate::DailyTradeStats {
                        turnover_cents: open.cents() as u64 * volume,
                        trade_count: 1,
                    }),
                },
            );
        }
        let operations = validated_operations_in_session(
            &game,
            AccountId(0),
            vec![crate::Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_010)),
                qty: 100,
            }],
        );
        let view = game.read_view();
        let inputs = prepare_incremental_continuous_inputs_on(&view).unwrap();
        let priors = market_data_priors(&view);
        drop(view);
        let mut shards = continuous_shards(inputs, priors).unwrap();
        let boundary = ContinuousTickBoundary::capture(&game).unwrap();
        let exchanges = exchanges(&game);
        let mut shard = shards.remove(&code).unwrap();
        let round = shard.apply_round(operations).unwrap();
        assert_eq!(round.trades.len(), 2, "两笔不同价成交（50@1000+50@1010）");
        let finish = shard
            .finish_for_market_tick(boundary, *exchanges.get(&code).unwrap())
            .unwrap();
        assert_eq!(finish.workers.len(), 1);
        let worker = &finish.workers[0];
        let patch = &worker.market_data;
        let candle = patch.candle.as_ref().unwrap();
        let stats = candle.trade_stats.as_ref().unwrap();
        match prior {
            None => {
                // 首笔真实成交重置占位：open=首笔价 1000；累计 100 股。
                assert_eq!(candle.open, Money::from_cents(1_000));
                assert_eq!(candle.volume, 100);
                assert_eq!(stats.trade_count, 2);
            }
            Some((open, _, volume)) => {
                // 已有成交 K：open 保留，统计在先验上累计。
                assert_eq!(candle.open, open);
                assert_eq!(candle.volume, volume + 100);
                assert_eq!(stats.trade_count, 3);
            }
        }
        assert_eq!(
            candle.high, expect_prior_high,
            "high=先验与全部成交价的极大值（保留/抬升两分支）"
        );
        assert_eq!(candle.low, if prior.is_some() {
            Money::from_cents(900)
        } else {
            Money::from_cents(1_000)
        });
        assert_eq!(candle.close, worker.market.last_price());
        assert_eq!(stats.turnover_cents, 50 * 1_000 + 50 * 1_010
            + prior.map_or(0, |(open, _, volume)| open.cents() as u64 * volume));
        match &patch.events[0].event {
            crate::Event::PriceTick { daily_candle, .. } => {
                assert_eq!(daily_candle, candle, "事件内日 K=owner 终值（同一事实单一载体）");
            }
            other => panic!("期望 PriceTick，实际 {other:?}"),
        }
    }
}

/// 分钟历史前缀保留：先验条目逐字节保留，仅追加段用本 tick 收盘价。
#[test]
fn minute_history_prefix_is_preserved_when_owner_appends() {
    let mut game = continuous_session();
    let code = game.state.markets.keys().next().unwrap().clone();
    let seeded = crate::MarketMinuteClose {
        absolute_trading_minute: 0,
        close: Money::from_cents(1_234),
    };
    game.state.market_minute_closes.insert(code.clone(), vec![seeded.clone()]);
    let _ = game.step().expect("健康 tick");
    let minutes = &game.state.market_minute_closes[&code];
    assert!(minutes.len() > 1, "本 tick 至少回填一条（completed>recorded）");
    assert_eq!(minutes[0], seeded, "先验前缀逐字节保留");
    let last = game.state.markets[&code].last_price();
    assert!(
        minutes[1..].iter().all(|close| close.close == last),
        "追加段全部=本 tick 收盘价"
    );
}

/// 竞价面（事件顺序+无成交标记）：非完结竞价 tick 仅 AuctionTick 且不标记
/// 日 K；完结 tick AuctionTick 先于 AuctionCompleted，无成交则以收盘价标记。
#[test]
fn auction_event_order_and_no_trade_candle_mark() {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(3);
    setup.ticks_per_day = 15_300;
    let mut game = GameSession::new(setup, 42).unwrap();
    let code = game.state.markets.keys().next().unwrap().clone();
    let tick1 = game.step().expect("竞价 tick 1");
    assert!(
        tick1
            .iter()
            .any(|event| matches!(event, crate::Event::AuctionTick { .. })),
        "竞价 tick 产出 AuctionTick"
    );
    assert!(
        !tick1
            .iter()
            .any(|event| matches!(event, crate::Event::AuctionCompleted { .. })),
        "非完结竞价 tick 不产出 AuctionCompleted"
    );
    assert!(
        game.state.candle_book.active().get(&code).is_none(),
        "非完结竞价 tick 不标记日 K"
    );
    let tick2 = game.step().expect("竞价完结 tick");
    let tick_pos = tick2
        .iter()
        .position(|event| matches!(event, crate::Event::AuctionTick { .. }))
        .expect("完结 tick 仍有 AuctionTick");
    let (done_pos, matched_volume) = tick2
        .iter()
        .position(|event| matches!(event, crate::Event::AuctionCompleted { .. }))
        .zip(
            tick2.iter().find_map(|event| match event {
                crate::Event::AuctionCompleted { matched_volume, .. } => Some(*matched_volume),
                _ => None,
            }),
        )
        .expect("完结 tick 产出 AuctionCompleted");
    assert!(tick_pos < done_pos, "AuctionTick 先于 AuctionCompleted（事件顺序）");
    assert_eq!(
        matched_volume, 0,
        "空簿竞价清算量恰为零（Luna 建议：无成交清算的精确断言）"
    );
    let candle = game
        .state
        .candle_book
        .active()
        .get(&code)
        .expect("完结竞价 tick 标记日 K");
    assert_eq!(candle.close, game.state.markets[&code].last_price());
    assert_eq!(candle.volume, matched_volume, "日 K 成交量=清算成交量（自洽钉）");
}

/// Astra T4 节点审·必修 2 佐证：股票收尾错误分类判别——行情面=MarketData
/// （上游 Finalization），其余=Other（上游 Preparation，不得一律改类）。
#[test]
fn stock_finish_fatal_classification_splits_market_data_from_other() {
    use super::continuous_matching::StockFinishFatal;
    let code = crate::StockId(0);
    let mut market = super::continuous_matching::incremental_continuous_stock_shadow::tests::empty_market(&code);
    let input =
        super::continuous_matching::incremental_continuous_stock_shadow::tests::stock_input(
            market.clone(),
            Vec::new(),
            crate::GameConfig::proposed_defaults(),
        );
    let mut coordinator = super::continuous_matching::IncrementalContinuousStockCoordinator::from_post_expiry(vec![input]).unwrap();
    // 行情面：先验 price_history 缺席 + Continuous 边界 → MarketData 分类。
    coordinator
        .install_market_prior(&code, StockMarketDataPrior::default())
        .unwrap();
    let error = coordinator
        .finish_for_market_tick(
            ContinuousTickBoundary {
                policy: crate::session::pipeline::phase::PhasePolicy::new(
                    crate::TradingPhase::Continuous,
                    0,
                    0,
                ),
                tick_after: 1,
                finishes_auction: false,
                ends_day: false,
                completed_minutes: Some(1),
            },
            crate::StockExchange::Shanghai,
        )
        .err()
        .expect("行情面先验缺席必须失败");
    assert!(
        matches!(&error, StockFinishFatal::MarketData(fatal)
            if matches!(fatal, crate::session::pipeline::StepFatal::InvariantViolation { description, .. }
                if description == "continuous market has no price history")),
        "行情面错误=MarketData（实际 {error:?}）"
    );
    // 其余收尾面：shard 阶段与边界不一致 → Other 分类（不得一律改类）。
    let input2 =
        super::continuous_matching::incremental_continuous_stock_shadow::tests::stock_input(
            market,
            Vec::new(),
            crate::GameConfig::proposed_defaults(),
        );
    let coordinator2 = super::continuous_matching::IncrementalContinuousStockCoordinator::from_post_expiry(vec![input2]).unwrap();
    let error2 = coordinator2
        .finish_for_market_tick(
            ContinuousTickBoundary {
                policy: crate::session::pipeline::phase::PhasePolicy::new(
                    crate::TradingPhase::PreOpen,
                    0,
                    0,
                ),
                tick_after: 1,
                finishes_auction: false,
                ends_day: false,
                completed_minutes: None,
            },
            crate::StockExchange::Shanghai,
        )
        .err()
        .expect("shard 与边界不一致必须失败");
    assert!(
        matches!(&error2, StockFinishFatal::Other(fatal)
            if matches!(fatal, crate::session::pipeline::StepFatal::InvariantViolation { description, .. }
                if description == "stock shard 与市场边界 PhasePolicy 不一致")),
        "其余收尾错误=Other（实际 {error2:?}）"
    );
}

/// Sol 节点复审·最小集 2：matcher 三形态红绿——部分事实/零事实（期望未清空
/// =typed feedback 缺失，须显式失败）与完整乱序（按事实序匹配成功）。
/// 真实期望经 `AccountValidatorDriver::consume_round`；真实轮次经
/// `IncrementalContinuousStockCoordinator::apply_round`（非伪造载体）。
#[test]
fn owner_matcher_rejects_leftover_expected_partial_facts() {
    let (code, ops, expected) = two_accepted_outcomes();
    let input =
        super::continuous_matching::incremental_continuous_stock_shadow::tests::stock_input(
            super::continuous_matching::incremental_continuous_stock_shadow::tests::empty_market(&code),
            Vec::new(),
            crate::GameConfig::proposed_defaults(),
        );
    let mut coordinator =
        super::continuous_matching::IncrementalContinuousStockCoordinator::from_post_expiry(vec![input])
            .unwrap();
    // 部分事实形态：仅执行第一个操作（第二个期望无事实）。
    let mut round = coordinator.apply_round(vec![ops[0].clone()]).unwrap();
    let error = super::market_tick_transaction::match_continuous_feedback(
        &code,
        &mut round,
        expected,
    )
    .err()
    .expect("部分事实（期望未清空）必须失败");
    assert!(
        error.to_string().contains("StockProcessing 的已受理候选缺少 typed feedback"),
        "实际 {error:?}"
    );
}

#[test]
fn owner_matcher_rejects_leftover_expected_zero_facts() {
    let (code, _ops, expected) = two_accepted_outcomes();
    let input =
        super::continuous_matching::incremental_continuous_stock_shadow::tests::stock_input(
            super::continuous_matching::incremental_continuous_stock_shadow::tests::empty_market(&code),
            Vec::new(),
            crate::GameConfig::proposed_defaults(),
        );
    let mut coordinator =
        super::continuous_matching::IncrementalContinuousStockCoordinator::from_post_expiry(vec![input])
            .unwrap();
    // 零事实形态：空操作轮（全部期望无事实）。
    let mut round = coordinator.apply_round(Vec::new()).unwrap();
    let error = super::market_tick_transaction::match_continuous_feedback(
        &code,
        &mut round,
        expected,
    )
    .err()
    .expect("零事实（期望未清空）必须失败");
    assert!(
        error.to_string().contains("StockProcessing 的已受理候选缺少 typed feedback"),
        "实际 {error:?}"
    );
}

#[test]
fn owner_matcher_matches_complete_round_in_fact_order() {
    let (code, ops, expected) = two_accepted_outcomes();
    let input =
        super::continuous_matching::incremental_continuous_stock_shadow::tests::stock_input(
            super::continuous_matching::incremental_continuous_stock_shadow::tests::empty_market(&code),
            Vec::new(),
            crate::GameConfig::proposed_defaults(),
        );
    let mut coordinator =
        super::continuous_matching::IncrementalContinuousStockCoordinator::from_post_expiry(vec![input])
            .unwrap();
    let mut round = coordinator.apply_round(ops).unwrap();
    assert_eq!(round.facts.len(), 2);
    // 完整乱序形态：期望按与事实相反的顺序传入——匹配按事实序完成。
    let mut reversed = vec![expected[1].clone(), expected[0].clone()];
    let outcomes = super::market_tick_transaction::match_continuous_feedback(
        &code,
        &mut round,
        std::mem::take(&mut reversed),
    )
    .expect("完整（乱序期望）必须匹配成功");
    assert_eq!(outcomes.len(), 2);
    assert_eq!(
        outcomes
            .iter()
            .map(super::CandidateValidationOutcome::candidate_key)
            .collect::<Vec<_>>(),
        round
            .facts
            .iter()
            .map(|fact| fact.candidate_key())
            .collect::<Vec<_>>(),
        "匹配结果按事实序"
    );
}

/// 两笔真实受理（玩家买单；期望含 operation）。
fn two_accepted_outcomes()
-> (crate::StockId, Vec<crate::session::pipeline::ValidatedOperation>, Vec<super::CandidateValidationOutcome>) {
    use super::account_validation_context::build_account_validation_context;
    use super::{AccountValidatorDriver, IntentCandidate, IntentCandidateKey, PhaseInput};
    use crate::{AccountId, Intent, Money, Side};
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.ticks_per_day = 120;
    let game = GameSession::new(setup, 42).unwrap();
    let code = game.state.markets.keys().next().unwrap().clone();
    let plan = super::plan_tick(PhaseInput { session: &game }).unwrap();
    let mut validator = AccountValidatorDriver::new(
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.state.next_order_id,
        game.state.setup.config.clone(),
        build_account_validation_context(&game).unwrap(),
    )
    .unwrap();
    let buy = |index: u64| {
        IntentCandidate::new(
            IntentCandidateKey::player(index),
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(700)),
                qty: 100,
            },
        )
    };
    let expected: Vec<super::CandidateValidationOutcome> =
        validator.consume_round([buy(0), buy(1)]).unwrap();
    assert_eq!(expected.len(), 2);
    let ops: Vec<crate::session::pipeline::ValidatedOperation> = expected
        .iter()
        .map(|outcome| outcome.operation().cloned().unwrap())
        .collect();
    (code, ops, expected)
}
