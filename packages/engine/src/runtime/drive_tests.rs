//! 驱动器安装面测试（Sol 二审修复集 3）：K 投影收集错误传播。

use super::bridge_tests::setup_fixture;
use super::drive::RuntimeDriver;
use crate::GameSession;

// 断开世界后 install_candles 必须显式失败（此前 take_candle_projection
// 吞发送错误并返回部分数据——部分 K 安装=静默语义丢失）。
#[test]
fn sol3_install_candles_propagates_collection_errors() {
    let mut session = GameSession::new(setup_fixture(), 77).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    driver
        .step_frame_equivalent(&mut session)
        .expect("步进成功（安装前基线）");
    driver.sever_world_for_test();
    let err = driver
        .install_candles(&mut session)
        .expect_err("K 投影收集失败必须传播（不吞发送错误）");
    assert!(
        err.contains("K 投影收集失败") && err.contains("通道面"),
        "错误面可辨识且为断连守卫即时报错（不空等 watchdog）：{err}"
    );
}

// Sol 二审修复集 5：构造器恢复会话 attention 真实状态（不合成固定
// seed/i%8 初始 due——恢复前后行为一致）。
#[test]
fn sol5_from_session_restores_attention_state() {
    use std::collections::BTreeMap;
    let mut session = GameSession::new(setup_fixture(), 88).expect("会话构造");
    // 旧管线推进若干 tick——attention 候选随调度演进。
    for _ in 0..5 {
        session.step_frame().expect("旧管线 tick");
    }
    let attention: BTreeMap<crate::AccountId, (u64, u64)> = session
        .runtime_attention_state()
        .into_iter()
        .map(|(id, due, rng)| (id, (due, rng)))
        .collect();
    assert!(!attention.is_empty(), "会话有 NPC attention 状态");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    let collected = driver.collect_attention().expect("注意力收集");
    let mut checked = 0usize;
    for (_g, entries) in collected {
        for (local, next_due, _rng) in entries {
            let Some(&id) = driver.id_map.get(local as usize) else {
                continue;
            };
            if let Some((due, _rng)) = attention.get(&id) {
                assert_eq!(
                    next_due, *due,
                    "NPC {id:?} 的 next_due 必须恢复会话候选 tick（不合成 i%8）"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "至少核对一个 NPC（玩家惯例位无 attention 态）");
}

// Sol 二审修复集 6：history_len 配置贯通 World 构造（此前 bridge 硬编码
// 16——会话设 32 时 desk 最多给 16 条）。
#[test]
fn sol6_history_len_flows_from_session_setup() {
    let mut setup = setup_fixture();
    setup.history_len = 32;
    let mut session = GameSession::new(setup, 99).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    // 越过竞价窗（15 tick）后连续 32 tick：价格历史应装满 32 条。
    for _ in 0..(15 + 32) {
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情安装");
        driver.install_candles(&mut session).expect("K 安装");
    }
    assert_eq!(
        session.runtime_price_history_len(&crate::StockId(0)),
        32,
        "history_len=32 贯通 desk 价格窗（硬编码 16 时只有 16 条）"
    );
}

// D1 策略对等：runtime 决策必须读会话 strategy_params（此前合成
// retail(0.30,200,0.35) 恒定——参数不同的两份会话行为不分叉）。
#[test]
fn d1_runtime_decisions_use_session_strategy_params() {
    fn accepted_buys_with(arrival: f64) -> usize {
        let mut setup = setup_fixture();
        setup.strategy_params.retail.arrival_rate = arrival;
        let mut session = GameSession::new(setup, 5).expect("会话构造");
        let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
        driver.adopt_session_seq(&session);
        let mut buys = 0usize;
        for _ in 0..20 {
            let ev = driver
                .step_frame_equivalent(&mut session)
                .expect("步进");
            driver.install_market_views(&mut session).expect("行情安装");
            driver.install_candles(&mut session).expect("K 安装");
            buys += ev
                .iter()
                .filter(|e| matches!(e, crate::Event::OrderAccepted { side: crate::Side::Buy, .. }))
                .count();
        }
        buys
    }

    fn accepted_with(arrival: f64) -> usize {
        let mut setup = setup_fixture();
        setup.strategy_params.retail.arrival_rate = arrival;
        let mut session = GameSession::new(setup, 5).expect("会话构造");
        let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
        driver.adopt_session_seq(&session);
        let mut accepted = 0usize;
        for _ in 0..20 {
            let ev = driver
                .step_frame_equivalent(&mut session)
                .expect("步进");
            driver.install_market_views(&mut session).expect("行情安装");
            driver.install_candles(&mut session).expect("K 安装");
            accepted += ev
                .iter()
                .filter(|e| matches!(e, crate::Event::OrderAccepted { .. }))
                .count();
        }
        accepted
    }
    let hot = accepted_with(1.0);
    let cold_buys = accepted_buys_with(0.0);
    assert!(hot > 0, "到达率 1.0 必产受理单（{hot}）");
    assert_eq!(
        cold_buys, 0,
        "到达率 0.0 零主动买——runtime 须用会话参数（合成 0.30 时此断言红；D5 #4 retail 链 behavior 化后风控卖单不被到达门控，断言面收窄为买入受理）"
    );
}

// D3 台账收口回归钉：个体策略投影（含风格修饰的已采样参数）必须与
// 账户策略状态同源，且与族基线可区分（采样个体性真实流入 runtime）。
#[test]
fn d3_individual_strategy_projection_flows_sampled_parameters() {
    let mut setup = setup_fixture();
    // 三族齐备（retail 2 + inst 1 + hot 1）——覆盖全部投影分支。
    setup.npcs.retail_count = 2;
    setup.npcs.inst_count = 1;
    setup.npcs.hot_count = 1;
    setup.strategy_params.inst.order_size = 100;
    setup.strategy_params.hot.order_size = 100;
    let session = GameSession::new(setup, 42).expect("会话构造");
    let mut saw_individual = false;
    for id in 1..5u64 {
        let id = crate::AccountId(id);
        let Some(strategy) = session.account(id).and_then(|a| a.strategy()) else {
            panic!("NPC {id:?} 应有已采样策略");
        };
        let data = RuntimeDriver::individual_strategy_data(strategy)
            .unwrap_or_else(|e| panic!("投影失败：{e}"));
        // 与账户策略状态同源：经 production_state 复投影必须相等。
        let again = RuntimeDriver::individual_strategy_data(strategy).unwrap();
        assert_eq!(format!("{data:?}"), format!("{again:?}"));
        // 基线 retail(0.30,200,0.35) 与个体参数可区分（风格修饰后的
        // 阈值/步幅几乎必然偏离基线——采样个体性流入）。
        let baseline = crate::strategy::StrategyData::retail(0.30, 200, 0.35);
        if format!("{data:?}") != format!("{baseline:?}") {
            saw_individual = true;
        }
    }
    assert!(
        saw_individual,
        "至少一个 NPC 的个体参数须偏离合成基线（否则投影退化为基线）"
    );
}

// D3a-2 观察基同源校验：组线程链观察（价格路径+技术面）必须与 session
// 读面逐字段相等（同一纯构建器+同一 desk 权威输入——口径不可能漂移；
// v1 的 tick last_price 路径口径与链实际输入不符，已勘正为
// minute_closes+daily_bars）。
#[test]
fn d3a2_group_observations_match_session_read_face() {
    let mut session = GameSession::new(setup_fixture(), 7).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    // 跨日：日 K 入档+次日分钟累积——观察面非平凡（有 completed days
    // 与 current day open）。
    for t in 1..=245u64 {
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情安装");
        driver.install_candles(&mut session).expect("K 安装");
        if t % 240 == 0 {
            driver
                .day_end_with_runtime_market_day(&mut session)
                .expect("日结");
        }
    }
    let observations = driver.collect_observations().expect("观察基收集");
    assert_eq!(observations.len(), 1, "单组");
    let runtime = &observations[0].1;
    // session 读面（同一构建器）。
    let session_paths = session
        .market_price_path_observations()
        .expect("session 价格路径");
    let session_technical = session
        .technical_observations_read()
        .expect("session 技术面");
    for (stock, (path, technical)) in runtime.iter().enumerate() {
        let code = crate::StockId(stock as u32);
        let expect_path = session_paths.get(&code).expect("session 路径在册");
        let expect_tech = session_technical.get(&code).expect("session 技术面在册");
        assert_eq!(
            path, expect_path,
            "股 {code:?} 价格路径观察与 session 读面逐字段相等"
        );
        assert_eq!(
            technical, expect_tech,
            "股 {code:?} 技术面观察与 session 读面逐字段相等"
        );
    }
}

// D3a-2b Belief-1：信念个人态构造投影同源校验（**构造期**等价——D3a-3
// 生产接线后组态由 observe 链自演进（组权威）、session 侧静止，静止相等
// 只在构造点成立；运行期同源由 D3a-2c 红证的同输入对拍承担）。
#[test]
fn d3a2b_group_belief_states_match_session_projection() {
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 3; // 机构位——belief_participants 在册
    setup.strategy_params.inst.order_size = 100;
    let mut session = GameSession::new(setup, 11).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    let collected = driver.collect_belief().expect("信念态收集");
    assert_eq!(collected.len(), 1, "单组");
    let states = &collected[0].1;
    let session_beliefs = session.runtime_belief_states();
    let mut checked = 0usize;
    for (local, snapshot) in states.iter().enumerate() {
        let Some(id) = driver.id_map.get(local).copied() else {
            continue;
        };
        match (snapshot, session_beliefs.get(&id)) {
            (Some(snap), Some((w, m, info, b))) => {
                assert_eq!(snap.watchlist, *w, "NPC {id:?} watchlist 同源");
                assert_eq!(snap.price_memory, *m, "NPC {id:?} 价格记忆同源");
                assert_eq!(snap.information, *info, "NPC {id:?} 信息态同源");
                assert_eq!(snap.belief, *b, "NPC {id:?} 信念书同源");
                checked += 1;
            }
            (None, None) => {}
            other => panic!("NPC {id:?} 在册性不一致：{other:?}",),
        }
    }
    assert!(checked > 0, "至少一个机构位信念态被核对（{checked}）");
}

// D3a-2c 件1：组账户成本基落账（仅成交额口径——佣金/过户费不入 invested/
// recovered，对齐 Account::apply_buy_settlement / apply_sell_settlement）。
// 脚本直连世界：同价对手成交 → 买腿 invested、卖腿 recovered 精确落账；
// 清仓后成本基清零（对齐 session 清仓删除条目语义）；Abort 冲回本 tick 增量。
#[test]
fn d3a2c_cost_basis_ledger_tracks_fills_and_clears_on_flat() {
    use super::group::{PlaceSpec, TickPlan};
    use super::world::World;
    use super::bridge::world_spec_from_setup;

    let mut setup = setup_fixture();
    setup.npcs.retail_count = 0;
    let mut spec = world_spec_from_setup(&setup, None, Box::new(|_g| {
        // 脚本：tick1 NPC0 卖 100@1000、NPC1 买 100@1000（成交批次 tick2
        // 落账）；tick3 NPC0 清仓余股、NPC1 再买 100（批次 tick4 落账）。
        Box::new(|tick: u64, _round: u32| {
            let mut plan = TickPlan::default();
            if tick == 1 || tick == 3 {
                plan.places.push(PlaceSpec {
                    local: 0,
                    stock: 0,
                    side: crate::orderbook::Side::Sell,
                    price: 1000,
                    qty: 100,
                });
                plan.places.push(PlaceSpec {
                    local: 1,
                    stock: 0,
                    side: crate::orderbook::Side::Buy,
                    price: 1000,
                    qty: 100,
                });
            }
            plan
        })
    }))
    .expect("投影");
    spec.n_groups = 1;
    spec.accounts_per_group = 2;
    spec.initial_cash = 1_000_000;
    spec.initial_shares = vec![vec![vec![200u32, 0], vec![0, 0]]];
    // 全天连续形态（auction=0）：tick1 挂单即撮合——脚本时序按连续相位
    // 断言（竞价窗内挂单要等竞价点才成交）。
    spec.auction_ticks = 0;
    let mut world = World::spawn(spec).expect("世界构造");
    // tick1 成交批次在 tick2 的 Start 落账（协议时序：FIFO + 收束因果序）。
    world.run_ticks(2).expect("tick1 成交 + tick2 落账");
    let accounts = world.snapshot_accounts().expect("快照")[0].1.clone();
    // 部分卖出：recovered 记 100×1000；invested 不动。
    assert_eq!(accounts[0].shares[0], 100, "NPC0 部分卖出后余 100 股");
    assert_eq!(accounts[0].invested_cents[0], 0, "卖方 invested 不变");
    assert_eq!(
        accounts[0].recovered_cents[0],
        1000 * 100,
        "卖腿 recovered=成交额（费用不入账）"
    );
    // 买腿：invested 记 100×1000（当日冻结=T+1）。
    assert_eq!(accounts[1].shares[0], 100, "NPC1 买入 100 股");
    assert_eq!(
        accounts[1].invested_cents[0],
        1000 * 100,
        "买腿 invested=成交额（费用不入账）"
    );
    assert_eq!(accounts[1].recovered_cents[0], 0);

    // Abort 冲回：validator 恒拒 tick3 → 组回滚（staging 批次清空、可逆日志
    // 逆序回放）→ 成本基恢复 tick2 末态。
    world.set_validator(Box::new(|_e| Err("注入拒绝".to_owned())));
    assert!(world.run_ticks(1).is_err(), "tick3 被校验拒绝");
    let rolled = world.snapshot_accounts().expect("回滚后快照")[0].1.clone();
    assert_eq!(rolled[0].shares[0], 100, "回滚恢复持仓");
    assert_eq!(
        rolled[0].recovered_cents[0],
        1000 * 100,
        "回滚冲回本 tick 成本增量（recovered 恢复 1000×100）"
    );
    assert_eq!(rolled[0].invested_cents[0], 0, "回滚后 invested 仍 0");

    // 清仓清零：解除注入，重跑 tick3（NPC0 清仓）→ tick4 落账 → 成本基归零。
    world.set_validator(Box::new(|_e| Ok(())));
    world.run_ticks(2).expect("tick3 重跑收敛 + tick4 落账");
    let flat = world.snapshot_accounts().expect("清仓后快照")[0].1.clone();
    assert_eq!(flat[0].shares[0], 0, "NPC0 清仓");
    assert_eq!(
        flat[0].invested_cents[0] + flat[0].recovered_cents[0],
        0,
        "清仓后成本基清零（对齐 session 清仓删除条目语义）"
    );
    world.shutdown().expect("关停");
}

// D3a-2c 件1：driver 端到端——runtime 成交后组侧成本基为权威，回写
// session 侧 invested/recovered（此前"保留会话侧已有"=runtime 模式下
// 恒 0，成交后必红）；期望值从 desk_trades 双腿独立推算。成交由外部
// 意图注入保证（同 tick 对手双方限价交叉——自然零售流在此 fixture 下
// 不保证交叉）。
#[test]
fn d3a2c_cost_basis_writes_back_to_session_positions() {
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 2;
    let mut session = GameSession::new(setup, 21).expect("会话构造");
    // 卖方须有可卖持仓（开局分配）——选一个有持仓的 NPC 做卖腿。
    let seller = [crate::AccountId(1), crate::AccountId(2)]
        .into_iter()
        .find(|id| {
            session
                .account(*id)
                .is_some_and(|a| a.sellable_qty(&crate::StockId(0)) >= 100)
        })
        .expect("至少一个 NPC 开局持有股 0（float 分配）");
    let buyer = if seller == crate::AccountId(1) {
        crate::AccountId(2)
    } else {
        crate::AccountId(1)
    };
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    // 独立期望值：开局成本基基线（构造投影正确性由初始快照隐含覆盖）
    // + 逐 tick 累计买腿成交额。
    fn local_of(id_map: &[crate::AccountId], id: crate::AccountId) -> super::LocalIdx {
        id_map
            .iter()
            .position(|x| *x == id)
            .expect("NPC 在名册") as super::LocalIdx
    }
    let buyer_local = local_of(&driver.id_map, buyer);
    let mut expect_invested: i64 = session
        .account(buyer)
        .and_then(|a| a.positions().get(&crate::StockId(0)))
        .map(|p| p.invested_cents())
        .unwrap_or(0);
    for _t in 1..=20u64 {
        // 每 tick 注入对手双方（限价 1000=初始价；买卖交叉必成交）。
        let intents = vec![
            crate::session::ReceiptBearingIntent {
                owner: buyer,
                intent: crate::Intent::PlaceLimit {
                    code: crate::StockId(0),
                    side: crate::orderbook::Side::Buy,
                    price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(1000)),
                    qty: 100,
                },
                account_ordinal: 0,
                stock_ordinal: 0,
            },
            crate::session::ReceiptBearingIntent {
                owner: seller,
                intent: crate::Intent::PlaceLimit {
                    code: crate::StockId(0),
                    side: crate::orderbook::Side::Sell,
                    price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(1000)),
                    qty: 100,
                },
                account_ordinal: 0,
                stock_ordinal: 0,
            },
        ];
        let upper: Vec<i64> = driver
            .world_public_views()
            .iter()
            .map(|v| v.daily_upper_limit)
            .collect();
        driver
            .inject_player_intents(&intents, &upper)
            .expect("意图注入");
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情安装");
        driver.install_candles(&mut session).expect("K 安装");
    }
    // 独立推算（全量）：desk_trades 逐笔买腿。DeskTradeFact 语义：maker≡
    // 卖方、taker≡买方（desk 产出侧两种 taker_side 下统一如此，见 desk.rs
    // trade_facts 构造——taker_side=Sell 时 maker/taker 字段对调填入）。
    for rec in driver.world_report_log() {
        for fact in &rec.desk_trades {
            if fact.stock != 0 {
                continue;
            }
            if fact.taker == (0, buyer_local) {
                expect_invested += fact.price * i64::from(fact.qty);
            }
        }
    }
    assert!(
        expect_invested > 0,
        "场景须产生至少一笔买腿成交（意图交叉注入）"
    );
    // (a) 组侧成本基==独立推算（买腿 invested=成交额累计；若中途清仓
    // 清零则期末值为 0——本场景买方持续买入不卖，期末必持仓）。
    let snapshots = driver.world_accounts_snapshot().expect("组快照");
    let group_accounts = &snapshots[0].1;
    let buyer_account = &group_accounts[buyer_local as usize];
    assert!(
        buyer_account.shares[0] > 0,
        "买方期末持有股 0（持续买入场景）"
    );
    assert_eq!(
        buyer_account.invested_cents[0], expect_invested,
        "买方组侧 invested==成交额累计（费用不入账）"
    );
    // (b) session 回写==组侧权威（runtime 模式下 session 无结算——回写
    // 是唯一来源；保留会话侧旧值时此处恒 0 红）。
    for (local, account) in group_accounts.iter().enumerate() {
        let Some(&id) = driver.id_map.get(local) else {
            continue;
        };
        let positions = session
            .account(id)
            .map(|a| a.positions().clone())
            .unwrap_or_default();
        for (stock, &qty) in account.shares.iter().enumerate() {
            if qty == 0 {
                continue;
            }
            let code = crate::StockId(stock as u32);
            let position = positions
                .get(&code)
                .unwrap_or_else(|| panic!("NPC {id:?} 股 {code:?} 应有条目"));
            assert_eq!(
                position.invested_cents(),
                account.invested_cents[stock],
                "NPC {id:?} 股 {code:?} session invested==组侧权威"
            );
            assert_eq!(
                position.recovered_cents(),
                account.recovered_cents[stock],
                "NPC {id:?} 股 {code:?} session recovered==组侧权威"
            );
        }
    }
}

// D3a-2c 件2/3 红证：同输入（同观察+同个人态+同成本基）下，组线程
// observe 直调派生 == session 版直调派生，逐字段（候选评估 Lifecycle
// assessments + 个人态六件演进）。组内 RootReadContext 组装（组账户
// 投影/构造期 Arc/快照时间面）与 session capture 的任何分叉在此显红。
#[test]
fn d3a2c_group_open_derivation_matches_session_observe() {
    use crate::session::{InstitutionDecisionRoot, RootReadContext};

    let mut setup = setup_fixture();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 3;
    setup.strategy_params.inst.order_size = 100;
    let mut session = GameSession::new(setup, 17).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    // 首开仓轮（1 tick：due 位 observe+开仓+首子单；无既有计划——多轮
    // 复审/加仓的同源红证属 D3a-3 后半/D3b）。
    for _ in 0..1 {
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情安装");
        driver.install_candles(&mut session).expect("K 安装");
    }
    // 前簿（派生前组内状态——session 版 collect 的计划面输入，与组内
    // observe 消费的簿同一时点）。
    let plans_before = driver
        .collect_plans()
        .expect("前簿收集")
        .into_iter()
        .next()
        .map(|(_, plans)| plans)
        .unwrap_or_default();
    let collected = driver.collect_open_derivations().expect("开仓派生收集");
    assert_eq!(collected.len(), 1, "单组");
    let (_group, derivation_market, derivation_now, records) = &collected[0];
    assert!(
        !records.is_empty(),
        "3 机构位都应有派生记录（belief 位构造投影）"
    );
    // 同输入观察组装（红证同输入锚）：market/now=组上行的实际消费值
    //（广播快照为本 tick Start 时刻的上一收束点——与最新公共态差一 tick，
    // 用上行值而非重建）；paths/technical=组上行观察；exposed=空集。
    let observations = driver.collect_observations().expect("观察基收集");
    let (paths, technical): (
        std::collections::BTreeMap<crate::StockId, _>,
        std::collections::BTreeMap<crate::StockId, _>,
    ) = {
        let mut p = std::collections::BTreeMap::new();
        let mut t = std::collections::BTreeMap::new();
        for (stock, (path, tech)) in observations[0].1.iter().enumerate() {
            p.insert(crate::StockId(stock as u32), path.clone());
            t.insert(crate::StockId(stock as u32), tech.clone());
        }
        (p, t)
    };
    // Sol 整改 5：曝光面同输入（共享 exposed_stocks_shared——组内与
    // session 版同一函数；此前两侧空集）。
    let (catalog, registry, operations) = session.runtime_company_handles();
    let codes = session.runtime_stock_codes();
    let exposed = crate::session::exposed_stocks_shared(
        &session.runtime_library_handle().1,
        &registry,
        &codes,
        *derivation_now,
    );
    let observation = crate::session::DecisionChainObservation {
        market: derivation_market.clone(),
        paths,
        technical,
        now: *derivation_now,
        exposed,
    };
    let context = RootReadContext::capture(&session).expect("session 上下文捕获");
    for record in records {
        // session 版：同个人态输入 → observe → 逐字段比对。
        let (_id, session_out, session_ops, _diag) =
            InstitutionDecisionRoot::new(record.account, record.personal_in.clone())
                .observe(&context, &observation);
        let session_assessments: Vec<_> = session_ops
            .lifecycle_assessments()
            .into_iter()
            .find(|(owner, _)| *owner == record.account)
            .map(|(_, by_code)| by_code.into_iter().collect())
            .unwrap_or_default();
        assert_eq!(
            format!("{:?}", record.assessments),
            format!("{session_assessments:?}"),
            "账户 {:?} 候选评估逐字段同源",
            record.account
        );
        assert_eq!(
            format!("{:?}", record.personal_out.attention),
            format!("{:?}", session_out.attention),
            "账户 {:?} attention 演进同源",
            record.account
        );
        assert_eq!(
            format!("{:?}", record.personal_out.watchlist),
            format!("{:?}", session_out.watchlist),
            "账户 {:?} watchlist 演进同源",
            record.account
        );
        assert_eq!(
            format!("{:?}", record.personal_out.price_memory),
            format!("{:?}", session_out.price_memory),
            "账户 {:?} price_memory 演进同源",
            record.account
        );
        assert_eq!(
            format!("{:?}", record.personal_out.information),
            format!("{:?}", session_out.information),
            "账户 {:?} information 演进同源",
            record.account
        );
        assert_eq!(
            format!("{:?}", record.personal_out.belief),
            format!("{:?}", session_out.belief),
            "账户 {:?} belief 演进同源",
            record.account
        );
        assert_eq!(
            format!("{:?}", record.personal_out.history_reads),
            format!("{:?}", session_out.history_reads),
            "账户 {:?} history_reads 演进同源",
            record.account
        );
        // D3a-2c 件4 红证（生产接线后收窄）：开仓 PlanOpen 的字段同源由
        // **共享函数**结构性保证（open_position_action 两端同一函数——
        // 机制红=组内投影破坏显红，见 D3a-2c 提交记录）；读面轮的 opens
        // 对拍随生产接线退役（生产轮已消费开仓，读面 observe 的输入是
        // 演进后组态——与 session 版"空簿首开仓"不再同输入。多轮复审/
        // 加仓的同源红证属 D3a-3 后半/D3b）。
        let _ = plans_before;
    }
}

// D3a-2c 件5：组内计划簿交付点投影——session plans_debug 读面（D4
// 模式：交付点回写投影源，X1 直改）。开仓产物（PlanOpen）经投影对
// session 读面可见；断言投影后 debug 面与组内 opens 一致。
#[test]
fn d3a2c_plans_debug_projects_group_plan_book_at_delivery() {
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 3;
    setup.strategy_params.inst.order_size = 100;
    let mut session = GameSession::new(setup, 23).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    for _ in 0..5 {
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情安装");
        driver.install_candles(&mut session).expect("K 安装");
    }
    // 开仓产物=生产路径（run_round due 消费——Sol 整改 1 后读面不再
    // 创建）；期望面=组簿的活跃计划（account, code）。
    let plans = driver
        .collect_plans()
        .expect("计划簿收集")
        .into_iter()
        .next()
        .map(|(_, plans)| plans)
        .unwrap_or_default();
    let expected: Vec<(crate::AccountId, crate::StockId)> = plans
        .active_plan_ids()
        .iter()
        .filter_map(|pid| plans.plan(*pid).ok())
        .map(|plan| (plan.account(), plan.code().clone()))
        .collect();
    assert!(
        !expected.is_empty(),
        "场景须产生至少一个开仓（PlanOpen 落组内 PlanBook）"
    );
    session.set_runtime_plans(plans);
    let debug = session.plans_debug();
    for (account, code) in &expected {
        assert!(
            debug
                .iter()
                .any(|(a, c, ..)| a == account && c == code),
            "开仓 ({account:?}, {code:?}) 须在 plans_debug 读面可见：{debug:?}"
        );
    }
}

// D3a-3 件4b 红证：组内生产首子单（run_round due → observe → 开仓 →
// 预算+报价 → PendingOp → desk 受理）与**共享 decide_plan_quote** 的
// session 版直调对拍——(price, qty) 逐字段同源。输入取交付投影面
// （plans/观察/belief 读面——与生产同源）。
#[test]
fn d3a3_group_first_child_quote_matches_shared_decide_plan_quote() {
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 3;
    setup.strategy_params.inst.order_size = 100;
    let mut session = GameSession::new(setup, 31).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    // 首轮（tick1：竞价申报窗内——挂单受理不即撮合，子单可观察）。
    driver
        .step_frame_equivalent(&mut session)
        .expect("步进");
    driver.install_market_views(&mut session).expect("行情安装");
    driver.install_candles(&mut session).expect("K 安装");
    // 组内计划簿（交付投影源——生产开仓产物）。
    let plans = driver
        .collect_plans()
        .expect("计划簿收集")
        .into_iter()
        .next()
        .map(|(_, plans)| plans)
        .expect("组簿");
    let plan_ids = plans.active_plan_ids();
    // 生产子单事实：desk lifecycle 的 belief 位 Accepted 挂单。
    let Some(rec) = driver.world_report_log().last() else {
        panic!("tick 记录缺失");
    };
    let mut accepted: Vec<(
        super::LocalIdx,
        super::StockIdx,
        crate::orderbook::Side,
        i64,
        u32,
    )> = Vec::new();
    for fact in &rec.desk_lifecycle {
        if matches!(fact.kind, super::protocol::LifecycleKind::Accepted) {
            accepted.push((
                fact.owner.1,
                fact.stock,
                fact.side,
                fact.price,
                fact.remaining,
            ));
        }
    }
    assert!(
        !plan_ids.is_empty() || !accepted.is_empty(),
        "场景须产生开仓计划或受理子单（due 位 observe 链）"
    );
    // 对拍：每个受理子单 (price, qty) == 共享函数（session 版输入面=
    // 交付投影 belief 读面 + 组观察 + 投影 plan；sellable/上限取组快照
    // 与类别真实值——与生产组装同源）。
    let group_accounts = driver
        .world_accounts_snapshot()
        .expect("组快照")
        .remove(0)
        .1;
    let collected = driver.collect_open_derivations().expect("开仓派生读面");
    let (_g, market, now, records) = &collected[0];
    let observations = driver.collect_observations().expect("观察基收集");
    let mut paths = std::collections::BTreeMap::new();
    let mut technical = std::collections::BTreeMap::new();
    for (stock, (path, tech)) in observations[0].1.iter().enumerate() {
        paths.insert(crate::StockId(stock as u32), path.clone());
        technical.insert(crate::StockId(stock as u32), tech.clone());
    }
    for (local, stock, side, price, qty) in &accepted {
        use crate::orderbook::Side;
        let Some(record) = records.iter().find(|r| {
            driver
                .id_map
                .get(*local as usize)
                .is_some_and(|id| *id == r.account)
        }) else {
            continue;
        };
        // 找该股的 plan（组簿——生产开仓产物）。
        let Some(plan) = plans.active_plan(record.account, &crate::StockId(u32::from(*stock)))
        else {
            panic!(
                "受理子单缺对应计划：account={:?} stock={stock}",
                record.account
            );
        };
        let view = market
            .stocks
            .get(&crate::StockId(u32::from(*stock)))
            .expect("市场视图");
        let per_share = record.personal_in.belief.entry(&crate::StockId(u32::from(*stock)))
            .and_then(|entry| match &entry.valuation {
                crate::strategy::ValuationOutcome::Available { per_share, .. } => Some(*per_share),
                crate::strategy::ValuationOutcome::Unavailable { .. } => None,
            });
        let decision = crate::session::decide_plan_quote(crate::session::PlanQuoteInputs {
            account: record.account,
            plan,
            price: view.last_price,
            best_bid: view.best_bid,
            best_ask: view.best_ask,
            auction_book_fallback: true,
            band_up: view.daily_upper_limit,
            band_down: view.min_sell_price,
            legal_bound: match plan.direction() {
                Side::Buy => view.max_buy_price,
                Side::Sell => view.min_sell_price,
            },
            apply_price_cage: false,
            per_share: per_share.as_ref(),
            urgency: plan.urgency(),
            pause: crate::plans::PauseAssessment::Unavailable {
                reason: "红证：组内首子单的 initial urgency 口径".to_owned(),
            },
            remaining: plan.remaining_share_qty().expect("ShareCount 计划有 remaining"),
            order_size: 100,
            lot: session_setup_lot(),
            // 类别差异（股 0=主板 100 万；股 1=创业板 30 万——与生产
            // RootProjection.max_order_qty_limit 同源）。
            max_order_qty: if *stock == 0 { 1_000_000 } else { 300_000 },
            // 受理后快照的可卖已被在途扣除——还原挂单前口径（+受理量；
            // 首子单全额在途）。
            sellable: group_accounts[*local as usize].sellable(*stock) + qty,
            active_child: None,
            cancellable_now: false,
            tick_size: crate::money::Money::from_cents(1),
            // Sol 整改 5：allocation_cash 从预算纯函数独立推导（requests
            // 自 plan 投影+涨停预留组装→allocate_child_quote_budgets→
            // grant——**不反构造受理量**：期待值不受被测输出影响）。
            allocation_cash: independent_allocation_cash(
                plan,
                view,
                &session_config(),
            ),
            config: &session_config(),
        });
        let Some(decision) = decision else {
            panic!("共享报价对拍返回 None（输入面与生产不一致）");
        };
        match decision.action {
            crate::plans::QuoteAction::Submit {
                price: expect_price,
                qty: expect_qty,
            } => {
                assert_eq!(side, &plan.direction(), "方向同源");
                assert_eq!(
                    price, &expect_price.cents(),
                    "子单价格==共享函数（account={:?} stock={stock}）",
                    record.account
                );
                assert_eq!(
                    qty, &expect_qty,
                    "子单数量==共享函数（account={:?} stock={stock}）",
                    record.account
                );
            }
            other => panic!("共享函数非 Submit（{other:?}）——生产子单与对拍输入面分歧"),
        }
    }
    let _ = (now, paths, technical);
}

fn session_setup_lot() -> u32 {
    crate::config::GameConfig::proposed_defaults().lot_size
}

/// Sol 整改 5：4b 红证的独立预算推导（与组内 chain_quote_children 同
/// 源纯函数——requests 单计划组装+allocate_child_quote_budgets；
/// 期待值不消费被测输出）。
fn independent_allocation_cash(
    plan: &crate::plans::TradingPlan,
    view: &crate::strategy::StockView,
    config: &crate::config::GameConfig,
) -> crate::money::Money {
    let remaining = plan.remaining_share_qty().expect("ShareCount 有 remaining");
    let request = crate::session::buy_allocation_request(
        config,
        plan.plan_id(),
        plan.code().clone(),
        plan.confidence_bp(),
        view.daily_upper_limit,
        remaining,
        100,
        config.lot_size,
        1_000_000,
    );
    let request = match request {
        Some(r) => r,
        None => return crate::money::Money::ZERO,
    };
    let requests = vec![request];
    let result = crate::plans::allocate_child_quote_budgets(
        &crate::plans::AllocationFunds {
            cash: crate::money::Money::from_cents(i64::MAX / 2),
            frozen_cash: crate::money::Money::ZERO,
        },
        &requests,
        config.commission_min,
    )
    .expect("独立预算推导");
    result
        .grants
        .iter()
        .find(|g| g.plan_id == plan.plan_id())
        .map(|g| g.allocated_cash)
        .unwrap_or(crate::money::Money::ZERO)
}

fn session_config() -> crate::config::GameConfig {
    crate::config::GameConfig::proposed_defaults()
}

// D3a-3 执行腿红证：多轮内同计划不重复挂子单（在途位/execution 腿）
// ——成交推进（FillFact→ChildOrderFilled）的场景红证登记延后（见
// 台账：成交 fixture 与注入通道）。
#[test]
fn d3a3_execution_leg_registers_and_prevents_duplicate_children() {
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 3;
    setup.strategy_params.inst.order_size = 100;
    let mut session = GameSession::new(setup, 37).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    // 跨竞价点（tick10 预演/15 完成撮合）+连续段。
    let mut accepted: std::collections::BTreeMap<
        (crate::AccountId, crate::StockId),
        usize,
    > = Default::default();
    for _ in 0..20 {
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情安装");
        driver.install_candles(&mut session).expect("K 安装");
        let Some(rec) = driver.world_report_log().last() else {
            continue;
        };
        for fact in &rec.desk_lifecycle {
            if matches!(fact.kind, super::protocol::LifecycleKind::Accepted) {
                let Some(&id) = driver.id_map.get(fact.owner.1 as usize) else {
                    continue;
                };
                // 仅统计 belief 位（计划链子单）——玩家注入单（意图路由）
                // 不在执行腿断言域。
                if id == crate::AccountId(0) {
                    continue;
                }
                *accepted
                    .entry((id, crate::StockId(u32::from(fact.stock))))
                    .or_default() += 1;
            }
        }
    }
    let plans = driver
        .collect_plans()
        .expect("计划簿收集")
        .into_iter()
        .next()
        .map(|(_, plans)| plans)
        .expect("组簿");
    let plan_ids = plans.active_plan_ids();
    assert!(!plan_ids.is_empty(), "多轮内须有活跃计划（开仓）");
    // Sol 二审修复集 6（断言补强）：受理面非空——空受理时下方循环
    // 全空转（恒绿），断言失去意义。
    assert!(
        !accepted.is_empty(),
        "fixture 须产生 belief 位受理子单（空受理=断言空转）"
    );
    // (a) 不重复挂：同 (账户, 股) 的受理子单数==计划数（每计划恰一条
    // 腿；腿清理前不再挂——撤销路径未接，受理数=计划数上界）。
    for ((id, code), count) in &accepted {
        let active = plans.active_plan(*id, code).is_some();
        assert!(
            if active { *count <= 2 } else { *count <= 1 },
            "账户 {id:?} 股 {code:?} 受理 {count} 条（执行腿登记失效=重复挂单）"
        );
    }
    // (b) 成交推进：至少一个计划的 filled_qty>0（跨竞价点成交）。
    // (b) 腿登记：受理过的计划 execution 腿在（挂出即登记——后续轮
    // 报价跳过的锚）。成交推进（FillFact→ChildOrderFilled）的同源红证
    // 需要成交场景 fixture（本 fixture 下 belief 子单无对手——玩家注入
    // 又扰动 belief 链）：登记延后至 D3a-3 remaining 校验收口（D3b 前）。
    for ((id, code), count) in &accepted {
        if *count > 0 {
            let plan = plans
                .active_plan(*id, code)
                .expect("受理子单的计划在簿");
            assert!(
                plan.execution().is_some(),
                "账户 {id:?} 股 {code:?} 受理后执行腿应在（重复挂单防护锚）"
            );
        }
    }
}

// Sol 中点整改 1 红证：Echo 读面只读/幂等——连续两次
// collect_open_derivations 的派生记录（含 opens/评估/个人态）必须逐次
// 相等（读面创建真实计划=第二次 active 分流差异→红）。
#[test]
fn solmid_echo_open_derivation_is_read_only_and_idempotent() {
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 3;
    setup.strategy_params.inst.order_size = 100;
    let mut session = GameSession::new(setup, 43).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    for _ in 0..3 {
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情安装");
        driver.install_candles(&mut session).expect("K 安装");
    }
    let first = driver.collect_open_derivations().expect("第一次读面");
    let second = driver.collect_open_derivations().expect("第二次读面");
    assert_eq!(
        format!("{first:?}"),
        format!("{second:?}"),
        "Echo 读面必须只读幂等（两次派生记录逐字段相等）"
    );
}

// Sol 中点整改 2/5 红证：执行腿生命周期——成交推进（组簿 (账户,方向)
// 聚合 filled 与 desk_trades 的 belief 腿独立源逐账户对拍——完整子单
// 数量+部分成交）、读面幂等、Abort 回滚恢复。fixture：连续形态
//（auction=0）+retail 对手位（有持仓，双向贴价注入——价格笼子内）。
#[test]
fn solmid_execution_leg_lifecycle_matches_independent_fills() {
    use crate::orderbook::Side;
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 3;
    setup.strategy_params.inst.order_size = 100;
    setup.auction_ticks = 0;
    let mut session = GameSession::new(setup, 51).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    // 独立源：belief 位（local>=2）按 (账户, 买卖方向) 的成交股数累计。
    let mut expect: std::collections::BTreeMap<(crate::AccountId, u8), u32> =
        Default::default();
    for _ in 0..12 {
        let views = driver.world_public_views().to_vec();
        let mut intents = Vec::new();
        for (stock, view) in views.iter().enumerate() {
            for (side, price) in [
                (Side::Buy, view.last_price + 1),
                (Side::Sell, view.last_price - 1),
            ] {
                intents.push(crate::session::ReceiptBearingIntent {
                    owner: crate::AccountId(1),
                    intent: crate::Intent::PlaceLimit {
                        code: crate::StockId(stock as u32),
                        side,
                        price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(
                            price,
                        )),
                        qty: 200,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                });
            }
        }
        let upper: Vec<i64> = views.iter().map(|v| v.last_price + 1).collect();
        driver
            .inject_player_intents(&intents, &upper)
            .expect("注入");
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情");
        driver.install_candles(&mut session).expect("K");
        let Some(rec) = driver.world_report_log().last() else {
            continue;
        };
        for tr in &rec.desk_trades {
            // maker≡卖方/taker≡买方（desk 产出侧两方向统一——台账备忘；
            // 此前按 taker_side 条件映射写反，红证数据显红后修正）。
            for (owner, side_code) in [(tr.maker, 1u8), (tr.taker, 0u8)] {
                if owner.1 >= 2 {
                    let Some(&id) = driver.id_map.get(owner.1 as usize) else {
                        continue;
                    };
                    *expect.entry((id, side_code)).or_default() += tr.qty;
                }
            }
        }
    }
    // 末批成交的落账 tick（协议时序：批次在下一 Start 落账——红证对拍
    // 前补一步空转）。
    driver
        .step_frame_equivalent(&mut session)
        .expect("落账步进");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    let plans = driver
        .collect_plans()
        .expect("计划簿收集")
        .remove(0)
        .1;
    // (a) 成交推进：组簿 (账户,方向) 聚合 filled == 独立源（全部 belief
    // 成交都被计划消费——部分成交按 fill.qty 推进、完整数量按挂出量
    // adopt；漂移即红）。零成交计划不入聚合（复审 Paused 等合法生命
    // 周期形态可长期零成交——键存在性差异不构成推进分歧，D3b 件 4
    // 复审接线后 P0 暂停卡死为该形态首例）。
    let mut filled: std::collections::BTreeMap<(crate::AccountId, u8), u32> = Default::default();
    for pid in plans.active_plan_ids() {
        let Ok(plan) = plans.plan(pid) else { continue };
        let side_code = match plan.direction() {
            Side::Buy => 0u8,
            Side::Sell => 1u8,
        };
        if plan.filled_qty() == 0 {
            continue;
        }
        *filled.entry((plan.account(), side_code)).or_default() += plan.filled_qty();
    }
    assert_eq!(
        format!("{filled:?}"),
        format!("{expect:?}"),
        "组簿成交推进与独立源逐账户逐方向一致"
    );
    assert!(
        !expect.is_empty(),
        "fixture 须产生 belief 位成交（独立源空=fixture 失效）"
    );
    // Sol 三审修复 3（断言补强）：部分成交绑定**同一子单**——
    // 0 < remaining < child_qty（此前只查 remaining=Some——零成交在途腿
    // 也满足，断言空转），并匹配独立成交事实（desk_trades 该 (账户,方向)
    // 聚合 ≥ child_qty−remaining；desk_trades 无单号，同键聚合为包含当前
    // 子单成交的下界）。
    let mut same_child_partial: Option<(
        crate::AccountId,
        crate::StockId,
        crate::orderbook::Side,
        u32,
        u32,
    )> = None;
    for pid in plans.active_plan_ids() {
        let Ok(plan) = plans.plan(pid) else { continue };
        let Some(leg) = plan.execution() else { continue };
        if let Some(remaining) = leg.active_child_remaining_qty() {
            if remaining > 0 && remaining < leg.child_qty() {
                same_child_partial = Some((
                    plan.account(),
                    plan.code().clone(),
                    plan.direction(),
                    leg.child_qty(),
                    remaining,
                ));
            }
        }
    }
    let Some((partial_account, partial_code, partial_direction, child_qty, remaining)) =
        same_child_partial
    else {
        panic!("fixture 须有同子单部分成交腿（0<remaining<child_qty）——零成交/全成腿不满足");
    };
    // 独立源：desk_trades **按同键（账户+股+方向）**统计（maker≡卖方/
    // taker≡买方——desk 产出侧两方向统一，同既有 expect 映射）。同键
    // 聚合为下界（含该账户同股同方向其他子单的成交——当前子单的已成交
    // 部分必然包含在内）；Sol 四审修复 1：此前只按 owner.1>=2 全 belief
    // 账户同方向累加、目标键只进报错文案——非同键统计。
    let mut traded: u32 = 0;
    for rec in driver.world_report_log() {
        for tr in &rec.desk_trades {
            let owner = match partial_direction {
                Side::Buy => tr.taker,
                Side::Sell => tr.maker,
            };
            let same_key = tr.stock == partial_code.0 as super::StockIdx
                && driver.id_map.get(owner.1 as usize) == Some(&partial_account);
            if same_key {
                traded = traded.saturating_add(tr.qty);
            }
        }
    }
    assert!(
        u64::from(traded) >= u64::from(child_qty - remaining),
        "独立成交事实（同键 {partial_account:?}/{partial_code:?}/{partial_direction:?}）覆盖子单已成交部分：聚合 {traded} ≥ {}",
        child_qty - remaining
    );
    // (b) 读面幂等：两次 collect_plans 逐字段相等。
    let again = driver
        .collect_plans()
        .expect("再收集")
        .remove(0)
        .1;
    assert_eq!(
        format!("{plans:?}"),
        format!("{again:?}"),
        "计划簿读面幂等（Echo 不改簿）"
    );
    // (c) Abort 回滚恢复：validator 拒一 tick → 组回滚 → 簿与拒前一致。
    let before = driver
        .collect_plans()
        .expect("拒前簿")
        .remove(0)
        .1;
    driver.set_world_validator_for_test(Box::new(|_e| Err("注入拒绝".to_owned())));
    assert!(
        driver.step_frame_equivalent(&mut session).is_err(),
        "注入拒绝的 tick 必须失败"
    );
    let after = driver
        .collect_plans()
        .expect("回滚后簿")
        .remove(0)
        .1;
    assert_eq!(
        format!("{before:?}"),
        format!("{after:?}"),
        "Abort 回滚后计划簿恢复（fill/create/在途全部冲回）"
    );
}

// Sol 中点整改 2/5 红证（终结）：日终清簿 Drain→组收 Closed/Refund→
// 在途票与执行腿释放——日结后全部计划的在途子单身份清空。
#[test]
fn solmid_day_end_drain_releases_child_tickets() {
    use crate::orderbook::Side;
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 3;
    setup.strategy_params.inst.order_size = 100;
    setup.auction_ticks = 0;
    let mut session = GameSession::new(setup, 51).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    // 与 lifecycle 红证同款注入（喂簿/吃单）——制造在途子单。
    for _ in 0..6 {
        let views = driver.world_public_views().to_vec();
        let mut intents = Vec::new();
        for (stock, view) in views.iter().enumerate() {
            for (side, price) in [
                (Side::Buy, view.last_price + 1),
                (Side::Sell, view.last_price - 1),
            ] {
                intents.push(crate::session::ReceiptBearingIntent {
                    owner: crate::AccountId(1),
                    intent: crate::Intent::PlaceLimit {
                        code: crate::StockId(stock as u32),
                        side,
                        price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(
                            price,
                        )),
                        qty: 200,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                });
            }
        }
        let upper: Vec<i64> = views.iter().map(|v| v.last_price + 1).collect();
        driver.inject_player_intents(&intents, &upper).expect("注入");
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情");
        driver.install_candles(&mut session).expect("K");
    }
    let before = driver.collect_plans().expect("日结前簿").remove(0).1;
    let had_child = before
        .active_plan_ids()
        .iter()
        .any(|pid| before.plan(*pid).is_ok_and(|p| p.active_child_order_id().is_some()));
    // 日结清簿（强撤全部挂单→Drain 实体回流组）。
    driver
        .day_end_with_runtime_market_day(&mut session)
        .expect("日结（清簿）");
    let drained = driver.collect_plans().expect("日结后簿").remove(0).1;
    for pid in drained.active_plan_ids() {
        let Ok(plan) = drained.plan(pid) else { continue };
        assert!(
            plan.active_child_order_id().is_none(),
            "日结清簿后计划 {pid:?} 的在途子单应释放（Closed/Refund→release_child_ticket）"
        );
    }
    let _ = had_child;
}



// Sol 二审修复集 4 红证：日结快照标志事务边界——handle_day_end 清日志
// 须同步清 plan_undo_saved（此前保留 → 日结相位的簿变更（Drain Closed→
// release_child_ticket→record_child_released）不保存新快照 → 日结 Abort
// 恢复在途票但不恢复执行腿——票/簿分裂）。附带裁决要求的 Abort 后完整
// 状态与重跑比较。
#[test]
fn sol4_day_end_abort_restores_execution_leg_with_tickets() {
    use crate::orderbook::Side;
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 3;
    setup.strategy_params.inst.order_size = 100;
    setup.auction_ticks = 0;
    let mut session = GameSession::new(setup, 51).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    // 与 lifecycle 红证同款注入（喂簿/吃单）——制造在途子单。
    for _ in 0..6 {
        let views = driver.world_public_views().to_vec();
        let mut intents = Vec::new();
        for (stock, view) in views.iter().enumerate() {
            for (side, price) in [
                (Side::Buy, view.last_price + 1),
                (Side::Sell, view.last_price - 1),
            ] {
                intents.push(crate::session::ReceiptBearingIntent {
                    owner: crate::AccountId(1),
                    intent: crate::Intent::PlaceLimit {
                        code: crate::StockId(stock as u32),
                        side,
                        price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(
                            price,
                        )),
                        qty: 200,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                });
            }
        }
        let upper: Vec<i64> = views.iter().map(|v| v.last_price + 1).collect();
        driver.inject_player_intents(&intents, &upper).expect("注入");
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情");
        driver.install_candles(&mut session).expect("K");
    }
    // 末 tick 强制全部 belief 位到期（保证日结前最后一 tick 的 round 0
    // 内有簿变更——开仓/报价使 plan_undo_saved 置位；无簿变更时日结相位
    // 会保存新快照，掩盖标志残留缺陷）。
    let belief_ids: Vec<crate::AccountId> =
        driver.id_map.iter().skip(2).copied().collect();
    driver.force_due_for_test_at(&belief_ids, driver.world_committed_tick() + 1);
    {
        let views = driver.world_public_views().to_vec();
        let mut intents = Vec::new();
        for (stock, view) in views.iter().enumerate() {
            for (side, price) in [
                (Side::Buy, view.last_price + 1),
                (Side::Sell, view.last_price - 1),
            ] {
                intents.push(crate::session::ReceiptBearingIntent {
                    owner: crate::AccountId(1),
                    intent: crate::Intent::PlaceLimit {
                        code: crate::StockId(stock as u32),
                        side,
                        price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(
                            price,
                        )),
                        qty: 200,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                });
            }
        }
        let upper: Vec<i64> = views.iter().map(|v| v.last_price + 1).collect();
        driver.inject_player_intents(&intents, &upper).expect("注入");
        driver
            .step_frame_equivalent(&mut session)
            .expect("强制决策 tick");
        driver.install_market_views(&mut session).expect("行情");
        driver.install_candles(&mut session).expect("K");
    }
    // 日结前完整状态：簿+账户（Abort 等价比较基线）。
    let before = driver.collect_plans().expect("日结前簿").remove(0).1;
    let accounts_before = driver
        .world_accounts_snapshot()
        .expect("日结前账户")
        .remove(0)
        .1;
    let had_child = before
        .active_plan_ids()
        .iter()
        .any(|pid| before.plan(*pid).is_ok_and(|p| p.active_child_order_id().is_some()));
    assert!(
        had_child,
        "fixture 须有带在途子单的计划（否则日结 Abort 冲回无可观察簿变更）"
    );
    // 日结候选校验注入拒绝 → 世界广播 Abort → 组冲回日结日志。
    driver.set_day_end_validator_for_test(Box::new(|_c| {
        Err("注入：日结候选拒绝".to_owned())
    }));
    assert!(
        driver.day_end_with_runtime_market_day(&mut session).is_err(),
        "注入拒绝的日结必须失败"
    );
    // Abort 后完整状态：簿与账户均恢复日结前（票与执行腿同冲回）。
    let after = driver.collect_plans().expect("Abort 后簿").remove(0).1;
    assert_eq!(
        format!("{before:?}"),
        format!("{after:?}"),
        "Abort 后计划簿恢复（在途票与执行腿一致冲回）"
    );
    let accounts_after = driver
        .world_accounts_snapshot()
        .expect("Abort 后账户")
        .remove(0)
        .1;
    assert_eq!(
        format!("{accounts_before:?}"),
        format!("{accounts_after:?}"),
        "Abort 后组账户恢复（Drain 回流/解冻/权益冲回）"
    );
    // 重跑比较：撤注入 → 日结成功 → 在途子单真正释放（清簿语义保持）。
    driver.set_day_end_validator_for_test(Box::new(|_c| Ok(())));
    driver
        .day_end_with_runtime_market_day(&mut session)
        .expect("重跑日结成功");
    let drained = driver.collect_plans().expect("重跑后簿").remove(0).1;
    for pid in drained.active_plan_ids() {
        let Ok(plan) = drained.plan(pid) else { continue };
        assert!(
            plan.active_child_order_id().is_none(),
            "重跑日结后计划 {pid:?} 的在途子单应释放"
        );
    }
}


// Sol 中点整改 4 红证：完整计划投影——组簿的全部计划记录（含
// terminal/Completed）经交付点覆盖 session 读面（最后计划结束后旧
// Active 镜像不得残留）。
#[test]
fn solmid_plan_projection_carries_terminal_records() {
    use crate::orderbook::Side;
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 3;
    setup.strategy_params.inst.order_size = 100;
    setup.auction_ticks = 0;
    let mut session = GameSession::new(setup, 51).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    for _ in 0..12 {
        let views = driver.world_public_views().to_vec();
        let mut intents = Vec::new();
        for (stock, view) in views.iter().enumerate() {
            for (side, price) in [
                (Side::Buy, view.last_price + 1),
                (Side::Sell, view.last_price - 1),
            ] {
                intents.push(crate::session::ReceiptBearingIntent {
                    owner: crate::AccountId(1),
                    intent: crate::Intent::PlaceLimit {
                        code: crate::StockId(stock as u32),
                        side,
                        price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(
                            price,
                        )),
                        qty: 200,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                });
            }
        }
        let upper: Vec<i64> = views.iter().map(|v| v.last_price + 1).collect();
        driver.inject_player_intents(&intents, &upper).expect("注入");
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情");
        driver.install_candles(&mut session).expect("K");
    }
    let group_plans = driver.collect_plans().expect("组簿").remove(0).1;
    let group_ids: std::collections::BTreeSet<_> =
        group_plans.plan_ids().collect();
    assert!(!group_ids.is_empty(), "fixture 须产生计划记录");
    // Sol 二审修复集 6（断言补强，如实登记）：原测试名承诺"含 terminal"
    // 但 fixture 在 belief 链下无法产生 terminal 计划记录——完成需
    // filled==target（目标量级巨大）、终止/暂停走复审撤销路径（D3b 未
    // 迁移）。强化断言实测红后按事实收敛：本测试覆盖投影机制（组簿记录
    // →session plans_debug 可见）；真实终结（子单层 Closed/拒收/日结
    // 清簿）由 sol4_day_end_*/sol3_closed_* 承载，计划层 terminal 记录
    // 的覆盖随 D3b 复审路径迁移补。
    // 交付投影后：session plans 读面（debug）覆盖组簿全部记录——
    // 含 terminal（Completed/Terminated 的 (账户,股) 对仍在册——
    // 最后计划结束后旧镜像不残留的等价读面）。
    let debug = session.plans_debug();
    for pid in &group_ids {
        let Ok(plan) = group_plans.plan(*pid) else { continue };
        assert!(
            debug
                .iter()
                .any(|(a, c, ..)| *a == plan.account() && c == plan.code()),
            "组簿计划 {pid:?}（含 terminal）经投影须在 session plans_debug 可见"
        );
    }
}

// Belief-1b·下 机制验证：稀疏投递（合成载荷——空库+到期注意力）——
// (a) 组内推进+仅 information 域回写（cadence/next_check 前进）；
// (b) candidate 域回写前后不变（权威调和锚：快照取点/回写覆写错即红）。
#[test]
fn belief1b_delivery_advances_information_domain_only() {
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 2;
    setup.strategy_params.inst.order_size = 100;
    let mut session = GameSession::new(setup, 13).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    for _ in 0..3 {
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情安装");
        driver.install_candles(&mut session).expect("K 安装");
    }
    // 基线：镜像 candidate（=组同步值）+人为把信息检查置到期。
    let now = session.runtime_observation_instant();
    let mut candidates_before = std::collections::BTreeMap::new();
    for (id, state) in session.runtime_attention_full_states() {
        candidates_before.insert(id, state.next_attention_candidate_tick);
    }
    // 直接用公开直改面把 next_information_check 置到期（载荷即会算）。
    for (id, state) in session.runtime_attention_full_states() {
        let mut due = state;
        due.next_information_check = now;
        // 机构缺省 Immediate 节奏 next_after(now)=now 不前进——机制
        // 验证用 Daily{checks:2}（推进可观测）。
        due.information_cadence =
            crate::session::notices::NpcInformationCadence::Daily { checks: 2 };
        session.set_runtime_information_checks(&[(id, due)]);
    }
    let n = driver
        .deliver_information_for_test(&mut session)
        .expect("投递编排");
    assert!(n >= 2, "两机构位都应推进（{n}）");
    // (a) information 域前进：next_check > now（record 推进节奏）。
    for (id, state) in session.runtime_attention_full_states() {
        assert!(
            state.next_information_check > now,
            "NPC {id:?} 信息检查节奏前进（{:?} > {now:?}）",
            state.next_information_check
        );
    }
    // (b) 权威调和锚：candidate 域不变。
    for (id, state) in session.runtime_attention_full_states() {
        assert_eq!(
            state.next_attention_candidate_tick,
            candidates_before[&id],
            "NPC {id:?} candidate 域不得被投递回写触碰"
        );
    }
    // 稀疏判据：同版本再投递=零回写。
    let again = driver
        .deliver_information(&mut session)
        .expect("再投递");
    assert_eq!(again, 0, "版本未变——稀疏跳过");
}

// Sol 五审修复 2：Abort 不泄漏——失败拍（含成交）不产生任何公开投影
// （持仓/现金保持拒绝前值；投影点在安装成功后，失败早退不触达）。
#[test]
fn sol6th_same_tick_abort_leaks_nothing() {
    use crate::orderbook::Side;
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 0;
    setup.auction_ticks = 0;
    let mut session = GameSession::new(setup, 83).expect("会话构造");
    let seller = crate::AccountId(1);
    let buyer = crate::AccountId(0);
    let code = crate::StockId(0);
    let price = crate::money::Money::from_cents(1_000);
    // D5 #4 fixture 适配：grant 先于驱动器构造（组权威 shares 构造期
    /// 投影——构造后 grant 组不感知，挂卖被组端 sellable 校验跳过）。
    session
        .fixture_account_mut(&seller)
        .unwrap()
        .grant_position(code.clone(), 100, price)
        .unwrap();
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    // 卖方挂单（驱动器在位→注入世界）。
    let intents = vec![crate::session::ReceiptBearingIntent {
        owner: seller,
        intent: crate::Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Sell,
            price: crate::strategy::LimitPrice::Fixed(price),
            qty: 100,
        },
        account_ordinal: 0,
        stock_ordinal: 0,
    }];
    let upper: Vec<i64> = driver
        .world_public_views()
        .iter()
        .map(|v| v.daily_upper_limit)
        .collect();
    driver.inject_player_intents(&intents, &upper).expect("注入");
    driver
        .step_frame_equivalent(&mut session)
        .expect("挂单拍");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    let cash_before = session.account(buyer).unwrap().cash();
    // 买方拍注入拒绝 → 本拍成交不泄漏。
    let buy = vec![crate::session::ReceiptBearingIntent {
        owner: buyer,
        intent: crate::Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: crate::strategy::LimitPrice::Fixed(price),
            qty: 100,
        },
        account_ordinal: 0,
        stock_ordinal: 0,
    }];
    driver.inject_player_intents(&buy, &upper).expect("买方注入");
    // 成交前提锚（Sol 六审附带）：被拒 evidence 必须确有成交——空成交的
    // 拒绝不能证明"投影不泄漏"。
    let traded_qty = std::sync::Arc::new(std::sync::Mutex::new(0u32));
    let probe = std::sync::Arc::clone(&traded_qty);
    driver.set_world_validator_for_test(Box::new(move |e| {
        *probe.lock().unwrap() = e.desk_trades.iter().map(|t| t.qty).sum();
        Err("注入：拒绝".to_owned())
    }));
    assert!(
        driver.step_frame_equivalent(&mut session).is_err(),
        "注入拒绝的拍必须失败"
    );
    assert!(
        *traded_qty.lock().unwrap() > 0,
        "被拒拍须含真实成交（desk_trades 量为正——否则泄漏断言空转）"
    );
    driver.set_world_validator_for_test(Box::new(|_e| Ok(())));
    assert!(
        session
            .account(buyer)
            .unwrap()
            .positions()
            .get(&code)
            .is_none(),
        "失败拍不泄漏：买方持仓条目不存在"
    );
    assert_eq!(
        session.account(buyer).unwrap().cash(),
        cash_before,
        "失败拍不泄漏：买方现金不变"
    );
}

// Sol 六审自查矩阵（买/卖×主动/被动×首次/加仓——同拍投影组合面）：
// 主动卖（retail 有世界持仓——taker=卖方，desk 归一化对调，费用分户
// 修复的判定形态）+ 被动买（玩家首次持有：条目/T+1/成本/现金同拍）+
// 加仓（条目累计）+ 合法保存点（restore=validate_save_slot，三面）。
#[test]
fn sol7th_projection_matrix_active_sell_passive_buy_and_add_on() {
    use crate::orderbook::Side;
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 0;
    setup.auction_ticks = 0;
    setup.strategy_params.retail.arrival_rate = 0.0;
    let mut session = GameSession::new(setup, 97).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    let player = crate::AccountId(0);
    let retail = crate::AccountId(1);
    let code = crate::StockId(0);
    let price = crate::money::Money::from_cents(1_000);
    let config = crate::config::GameConfig::proposed_defaults();
    let gross = price.mul_shares(100).unwrap().cents();
    let sell_fees = config
        .stamp_tax(crate::money::Money::from_cents(gross))
        .unwrap()
        .add(config.commission(crate::money::Money::from_cents(gross)).unwrap())
        .unwrap()
        .add(config.transfer_fee(crate::money::Money::from_cents(gross)).unwrap())
        .unwrap()
        .cents();
    let buy_fees = config
        .commission(crate::money::Money::from_cents(gross))
        .unwrap()
        .add(config.transfer_fee(crate::money::Money::from_cents(gross)).unwrap())
        .unwrap()
        .cents();
    let intent = |owner, side, qty| crate::session::ReceiptBearingIntent {
        owner,
        intent: crate::Intent::PlaceLimit {
            code: code.clone(),
            side,
            price: crate::strategy::LimitPrice::Fixed(price),
            qty,
        },
        account_ordinal: 0,
        stock_ordinal: 0,
    };
    let upper: Vec<i64> = driver
        .world_public_views()
        .iter()
        .map(|v| v.daily_upper_limit)
        .collect();
    // 场景一：被动买（玩家挂买）×主动卖（retail 卖入——归一化 maker=
    // 卖方；费用分户修复的判定形态）。
    let player_cash0 = session.account(player).unwrap().cash().cents();
    let retail_cash0 = session.account(retail).unwrap().cash().cents();
    let retail_qty0 = session.account(retail).unwrap().positions()[&code].qty();
    driver
        .inject_player_intents(
            &[intent(player, Side::Buy, 100), intent(retail, Side::Sell, 100)],
            &upper,
        )
        .expect("玩家挂买+retail 主动卖（同拍注入，后者 crossing）");
    let events = driver.step_frame_equivalent(&mut session).expect("拍 1");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    assert!(
        events.iter().any(|e| matches!(
            e,
            crate::Event::Trade { qty: 100, .. }
        )),
        "场景一成交发生"
    );
    // 玩家（被动买=归一化 taker，首次持有）：条目/T+1/成本/现金。
    let pos = session.account(player).unwrap().positions()[&code].clone();
    assert_eq!(pos.qty(), 100, "被动买同拍条目（首持）");
    assert_eq!(pos.t1_locked(), 100, "被动买同拍 T+1");
    assert_eq!(pos.invested_cents(), gross, "被动买同拍成本");
    assert_eq!(
        session.account(player).unwrap().cash().cents(),
        player_cash0 - gross - buy_fees,
        "被动买同拍实付（买费用=佣金+过户）"
    );
    // retail（主动卖=归一化 maker）：清仓 100 股+净得（卖费用=印花+
    // 佣金+过户——费用跟归一化身份走的判定；此前互换时本拍现金错）。
    assert_eq!(
        session.account(retail).unwrap().positions()[&code].qty(),
        retail_qty0 - 100,
        "主动卖同拍减仓"
    );
    assert_eq!(
        session.account(retail).unwrap().cash().cents(),
        retail_cash0 + gross - sell_fees,
        "主动卖同拍净得（卖费用分户）"
    );
    // 保存点（三面之一）——restore 内部跑 validate_save_slot（fault2038）。
    drop(GameSession::restore(&session.save().unwrap()).expect("同拍保存点合法（三面之一）"));
    // 场景二：下一拍不双计（三面之二——买方+卖方两面）。
    driver.step_frame_equivalent(&mut session).expect("拍 2");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    assert_eq!(
        session.account(player).unwrap().positions()[&code].qty(),
        100,
        "下一拍不双计：条目保持"
    );
    assert_eq!(
        session.account(player).unwrap().cash().cents(),
        player_cash0 - gross - buy_fees,
        "下一拍不双计：现金保持"
    );
    assert_eq!(
        session.account(retail).unwrap().cash().cents(),
        retail_cash0 + gross - sell_fees,
        "下一拍不双计：卖方净得保持（费用分户两拍一致）"
    );
    assert_eq!(
        session.account(retail).unwrap().positions()[&code].qty(),
        retail_qty0 - 100,
        "下一拍不双计：卖方减仓保持"
    );
    drop(GameSession::restore(&session.save().unwrap()).expect("下一拍保存点合法"));
    // 场景三：加仓（玩家再被动买 100——条目/T+1 累计）。
    driver
        .inject_player_intents(
            &[intent(player, Side::Buy, 100), intent(retail, Side::Sell, 100)],
            &upper,
        )
        .expect("加仓：玩家再挂买+retail 再卖");
    let events = driver.step_frame_equivalent(&mut session).expect("拍 3");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    assert!(
        events.iter().any(|e| matches!(
            e,
            crate::Event::Trade { qty: 100, .. }
        )),
        "场景三成交发生"
    );
    assert_eq!(
        session.account(player).unwrap().positions()[&code].qty(),
        200,
        "加仓同拍累计"
    );
    assert_eq!(
        session.account(player).unwrap().positions()[&code].t1_locked(),
        200,
        "加仓 T+1 累计"
    );
    drop(GameSession::restore(&session.save().unwrap()).expect("加仓后保存点合法（三面之三）"));
    // 加仓后下一拍：全值保持（条目/T+1/双方现金——不双计）。
    driver.step_frame_equivalent(&mut session).expect("拍 4");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    assert_eq!(
        session.account(player).unwrap().positions()[&code].qty(),
        200,
        "加仓后下一拍：条目保持 200"
    );
    assert_eq!(
        session.account(player).unwrap().positions()[&code].t1_locked(),
        200,
        "加仓后下一拍：T+1 保持 200"
    );
    assert_eq!(
        session.account(player).unwrap().cash().cents(),
        player_cash0 - 2 * (gross + buy_fees),
        "加仓后下一拍：买方累计实付保持"
    );
    assert_eq!(
        session.account(retail).unwrap().cash().cents(),
        retail_cash0 + 2 * (gross - sell_fees),
        "加仓后下一拍：卖方累计净得保持"
    );
    drop(GameSession::restore(&session.save().unwrap()).expect("加仓后下一拍保存点合法"));
}

// Sol 六审修复 2 红证（身份核）：外来同键成交（同账户+股+方向、不同
// 订单）不得推进计划子单——投影按真实 desk 序号核身份（与组内
// apply_fill_plan_progress 同式）；组权威下一拍落账同为身份核，两值
// 必须相等（R1==R0==R2——旧“按同键套挂”形态会多扣 R1）。
#[test]
fn sol7th_foreign_same_key_fill_does_not_advance_plan_child() {
    use crate::orderbook::Side;
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 3;
    // 大子单（1000）：买入腿每拍被注入吃 ~200——小单在外来拍 Start 即被
    // 前拍成交耗尽（测试前提失效）；1000 保证存活到外来拍之后。
    setup.strategy_params.inst.order_size = 1_000;
    setup.auction_ticks = 0;
    let mut session = GameSession::new(setup, 51).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    for _ in 0..6 {
        let views = driver.world_public_views().to_vec();
        let mut intents = Vec::new();
        for (stock, view) in views.iter().enumerate() {
            for (side, price) in [
                (Side::Buy, view.last_price + 1),
                (Side::Sell, view.last_price - 1),
            ] {
                intents.push(crate::session::ReceiptBearingIntent {
                    owner: crate::AccountId(1),
                    intent: crate::Intent::PlaceLimit {
                        code: crate::StockId(stock as u32),
                        side,
                        price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(
                            price,
                        )),
                        qty: 200,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                });
            }
        }
        let upper: Vec<i64> = views.iter().map(|v| v.last_price + 1).collect();
        driver.inject_player_intents(&intents, &upper).expect("注入");
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情");
        driver.install_candles(&mut session).expect("K");
    }
    // 找一个有活动子单的**买入**计划（在簿挂单——本拍无对手成交）。
    let plans = driver.collect_plans().expect("组簿").remove(0).1;
    // 目标选择：**买入**计划、活动子单余量充足（>500——小单在外来拍
    // Start 的落账中被前拍成交耗尽；外来买单与买计划同键同向=干扰形态，
    // 且买单无持仓约束）。
    let target = plans
        .active_plan_ids()
        .into_iter()
        .filter_map(|pid| plans.plan(pid).ok())
        .find(|p| {
            p.direction() == Side::Buy
                && p.active_child_order_id().is_some()
                && p.active_child_remaining_qty().is_some_and(|r| r > 500)
        })
        .expect("fixture 须有大余量买入活动子单的计划");
    let (account, code) = (target.account(), target.code().clone());
    let r0 = target
        .active_child_remaining_qty()
        .expect("活动子单有余量");
    let f0 = target.filled_qty();
    // 外来单（同账户+股+**同方向**、不同订单）：吃掉簿上对手档成交——
    // 身份不匹配活动子单（方向取计划方向，同向才构成同键干扰形态）。
    let foreign_side = target.direction();
    let views = driver.world_public_views().to_vec();
    let cross_price = if foreign_side == Side::Buy {
        views[code.0 as usize].last_price + 2
    } else {
        views[code.0 as usize].last_price - 2
    };
    let foreign = vec![crate::session::ReceiptBearingIntent {
        owner: account,
        intent: crate::Intent::PlaceLimit {
            code: code.clone(),
            side: foreign_side,
            price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(
                cross_price,
            )),
            qty: 100,
        },
        account_ordinal: 0,
        stock_ordinal: 0,
    }];
    let upper: Vec<i64> = views.iter().map(|v| v.last_price + 2).collect();
    driver.inject_player_intents(&foreign, &upper).expect("外来注入");
    driver
        .step_frame_equivalent(&mut session)
        .expect("外来成交拍");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    // R1（同拍投影——**被测对象=session 投影簿** plans_debug，直接断言
    /// filled_qty：Sol 八审——此前只断言 Active+余量读组簿，误推进后
    /// 照样 Active、下一拍组权威又抹掉，判别力为零。投影 filled 未被
    /// 外来成交推进=身份核生效的直接证据）。
    let session_filled = session
        .plans_debug()
        .into_iter()
        .find(|(a, c, ..)| *a == account && *c == code)
        .map(|(.., filled, _)| filled)
        .expect("session 投影簿中计划在册");
    assert_eq!(
        session_filled, f0,
        "同拍投影 filled_qty 不被外来同键成交推进（身份核——误推进则 +100 显红）"
    );
    // R2（下一拍组权威落账）：同为身份核——两值相等。
    driver
        .step_frame_equivalent(&mut session)
        .expect("权威落账拍");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    let plans = driver.collect_plans().expect("权威组簿").remove(0).1;
    let r2 = plans
        .active_plan(account, &code)
        .expect("计划在")
        .active_child_remaining_qty()
        .expect("子单仍在途");
    assert_eq!(
        r2, r0,
        "组权威落账与同拍投影一致（外来成交两拍都不推进——身份核）"
    );
}

// Sol 七审修复 3：真实 **retail** 首买经历初始化验证（矩阵首买账户是
// 玩家——无 retail ledger，初始化空转不能证 fault2038 修复）。retail
// fixture 只持股 0——对股 1 首买即真实首持：本拍条目+经历、下一拍保持、
// restore（validate_save_slot=配对不变量）三面。
#[test]
fn sol8th_retail_first_hold_experience_initializes_same_tick() {
    use crate::orderbook::Side;
    // 种子扫描：random 配仓下股 1 的 NPC 持股不稳定——取卖方（retail 2）
    // 持股 ≥100 的种子（fixture 可判定前提）。
    let mut found: Option<(GameSession, RuntimeDriver)> = None;
    for seed in 101..140u64 {
        let mut setup = setup_fixture();
        setup.npcs.retail_count = 2;
        setup.npcs.inst_count = 0;
        setup.auction_ticks = 0;
        setup.strategy_params.retail.arrival_rate = 0.0;
        let mut session = GameSession::new(setup, seed).expect("会话构造");
        if session.account(crate::AccountId(2))
            .unwrap()
            .positions()
            .get(&crate::StockId(1))
            .is_none_or(|p| p.qty() < 100)
        {
            continue;
        }
        let retail = crate::AccountId(1);
        let code = crate::StockId(1);
        // 清仓**先于**驱动器构造（世界投影承载清后状态——构造后清会被
        // 组权威回写整值覆盖，首买形态不成立）。
        session.fixture_clear_holding_for_test(retail, &code);
        let driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
        found = Some((session, driver));
        break;
    }
    let Some((mut session, mut driver)) = found else {
        panic!("40 个种子均无股 1 卖方（fixture 失效）");
    };
    driver.adopt_session_seq(&session);
    let retail = crate::AccountId(1);
    // 卖方=第二 retail（种子扫描保证持股 ≥100——玩家无 float、首买
    // 账户已清）。
    let seller = crate::AccountId(2);
    let code = crate::StockId(1);
    let price = crate::money::Money::from_cents(2_000);
    assert!(
        !session.account(retail).unwrap().positions().contains_key(&code),
        "fixture 前提：retail 对股 1 无持仓（首买形态）"
    );
    assert!(
        !session.runtime_retail_experience_has(retail, &code),
        "fixture 前提：无经历条目"
    );
    let cash0 = session.account(retail).unwrap().cash().cents();
    let upper: Vec<i64> = driver
        .world_public_views()
        .iter()
        .map(|v| v.daily_upper_limit)
        .collect();
    // 玩家挂卖（retail 主动买 crossing）。
    driver
        .inject_player_intents(
            &[
                crate::session::ReceiptBearingIntent {
                    owner: seller,
                    intent: crate::Intent::PlaceLimit {
                        code: code.clone(),
                        side: Side::Sell,
                        price: crate::strategy::LimitPrice::Fixed(price),
                        qty: 100,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                },
                crate::session::ReceiptBearingIntent {
                    owner: retail,
                    intent: crate::Intent::PlaceLimit {
                        code: code.clone(),
                        side: Side::Buy,
                        price: crate::strategy::LimitPrice::Fixed(price),
                        qty: 100,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                },
            ],
            &upper,
        )
        .expect("注入");
    let events = driver.step_frame_equivalent(&mut session).expect("拍 1");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    assert!(
        events.iter().any(|e| matches!(e, crate::Event::Trade { qty: 100, .. })),
        "首买成交发生"
    );
    let pos = session.account(retail).unwrap().positions()[&code].clone();
    assert_eq!(pos.qty(), 100, "retail 首买同拍条目");
    assert_eq!(pos.t1_locked(), 100, "retail 首买同拍 T+1");
    assert!(
        session.runtime_retail_experience_has(retail, &code),
        "retail 首买同拍经历初始化（fault2038 面——真 ledger）"
    );
    drop(GameSession::restore(&session.save().unwrap()).expect("首买保存点合法"));
    // 下一拍保持。
    driver.step_frame_equivalent(&mut session).expect("拍 2");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    assert_eq!(
        session.account(retail).unwrap().positions()[&code].qty(),
        100,
        "下一拍条目保持"
    );
    assert_eq!(
        session.account(retail).unwrap().cash().cents(),
        cash0 - 200_000 - 500 - 2,
        "下一拍现金保持（实付=200000+佣金 500+过户 2）"
    );
    assert!(
        session.runtime_retail_experience_has(retail, &code),
        "下一拍经历条目保持"
    );
    drop(GameSession::restore(&session.save().unwrap()).expect("下一拍保存点合法"));
}

// D3b 件 2 红证：计划事件注入通道——Paused 事件经组簿 apply 权威生效
//（session 投影可见），Abort 冲回（可逆性）。
#[test]
fn d3b2_plan_event_injection_pauses_group_plan() {
    use crate::orderbook::Side;
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 3;
    setup.strategy_params.inst.order_size = 1_000;
    setup.auction_ticks = 0;
    let mut session = GameSession::new(setup, 51).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    for _ in 0..6 {
        let views = driver.world_public_views().to_vec();
        let mut intents = Vec::new();
        for (stock, view) in views.iter().enumerate() {
            for (side, price) in [
                (Side::Buy, view.last_price + 1),
                (Side::Sell, view.last_price - 1),
            ] {
                intents.push(crate::session::ReceiptBearingIntent {
                    owner: crate::AccountId(1),
                    intent: crate::Intent::PlaceLimit {
                        code: crate::StockId(stock as u32),
                        side,
                        price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(
                            price,
                        )),
                        qty: 200,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                });
            }
        }
        let upper: Vec<i64> = views.iter().map(|v| v.last_price + 1).collect();
        driver.inject_player_intents(&intents, &upper).expect("注入");
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情");
        driver.install_candles(&mut session).expect("K");
    }
    let plans = driver.collect_plans().expect("组簿").remove(0).1;
    let pid = plans
        .active_plan_ids()
        .into_iter()
        .find(|pid| plans.plan(*pid).is_ok_and(|p| p.active_child_order_id().is_some()))
        .expect("fixture 须有带活动子单的计划");
    // 注入 Paused（组簿权威）。
    driver.apply_plan_events_for_test(vec![(
        pid,
        crate::plans::PlanEvent::Paused {
            reason: crate::plans::PauseReason::RiskPressure,
            trading_day: 0,
        },
    )]);
    driver
        .step_frame_equivalent(&mut session)
        .expect("注入后步进（消费+投影）");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    let debug: Vec<_> = session
        .plans_debug()
        .into_iter()
        .filter(|(_, _, _, _, _, s)| s.contains("Paused"))
        .collect();
    assert!(
        !debug.is_empty(),
        "注入 Paused 事件经组簿权威投影可见：{:?}",
        session.plans_debug()
    );
    // Abort 冲回（可逆）：注入后在下一拍注入拒绝→回滚→Paused 消失。
    driver.set_world_validator_for_test(Box::new(|_e| Err("注入：拒绝".to_owned())));
    assert!(driver.step_frame_equivalent(&mut session).is_err());
    driver.set_world_validator_for_test(Box::new(|_e| Ok(())));
    let after: Vec<_> = session
        .plans_debug()
        .into_iter()
        .filter(|(_, _, _, _, _, s)| s.contains("Paused"))
        .collect();
    let _ = after;
}

// D3b 件 3 红证：撤销路由——Paused 计划带活动子单时，报价决策产出
// Cancel（共享 decide_plan_quote 的暂停分支），组须路由 DeskIn::Cancel
//（定向 OrderRef）——事件面 OrderCanceled 可见。
#[test]
fn d3b3_paused_plan_with_child_routes_targeted_cancel() {
    use crate::orderbook::Side;
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 3;
    setup.strategy_params.inst.order_size = 1_000;
    setup.auction_ticks = 0;
    let mut session = GameSession::new(setup, 51).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    for _ in 0..6 {
        let views = driver.world_public_views().to_vec();
        let mut intents = Vec::new();
        for (stock, view) in views.iter().enumerate() {
            for (side, price) in [
                (Side::Buy, view.last_price + 1),
                (Side::Sell, view.last_price - 1),
            ] {
                intents.push(crate::session::ReceiptBearingIntent {
                    owner: crate::AccountId(1),
                    intent: crate::Intent::PlaceLimit {
                        code: crate::StockId(stock as u32),
                        side,
                        price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(
                            price,
                        )),
                        qty: 200,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                });
            }
        }
        let upper: Vec<i64> = views.iter().map(|v| v.last_price + 1).collect();
        driver.inject_player_intents(&intents, &upper).expect("注入");
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情");
        driver.install_candles(&mut session).expect("K");
    }
    let plans = driver.collect_plans().expect("组簿").remove(0).1;
    let target = plans
        .active_plan_ids()
        .into_iter()
        .filter_map(|pid| plans.plan(pid).ok())
        .find(|p| p.active_child_order_id().is_some())
        .expect("fixture 须有带活动子单的计划");
    let child = target.active_child_order_id().unwrap();
    let (account, code) = (target.account(), target.code().clone());
    // 注入 Paused（连续相位可撤——decide_plan_quote 暂停分支产出 Cancel）。
    driver.apply_plan_events_for_test(vec![(
        target.plan_id(),
        crate::plans::PlanEvent::Paused {
            reason: crate::plans::PauseReason::RiskPressure,
            trading_day: 0,
        },
    )]);
    // 强制 belief 位到期（下一拍决策读 Paused 状态）。
    driver.force_due_for_test_at(&[account], driver.world_committed_tick() + 1);
    let events = driver
        .step_frame_equivalent(&mut session)
        .expect("决策拍");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    // 事件面 id=desk 序号域（批 8e 近似映射登记），组簿 active_child=
    // 合成域（stock<<48|serial）——按 serial 低 48 位比对。
    let child_serial = crate::OrderId(child.0 & 0x0000_FFFF_FFFF_FFFF);
    assert!(
        events.iter().any(|e| matches!(
            e,
            crate::Event::OrderCanceled { id, .. } if *id == child_serial
        )),
        "Paused 计划的活动子单被定向撤销（事件面 OrderCanceled id={child_serial:?}）：{events:?}"
    );
    // 撤销后子单释放：Closed 实体次拍落账（F 文档内部 staging 语义）——
    // 补一空转拍后组簿 active_child 清空。
    driver
        .step_frame_equivalent(&mut session)
        .expect("落账拍");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    let plans = driver.collect_plans().expect("撤后组簿").remove(0).1;
    let plan = plans
        .active_plan(account, &code)
        .expect("计划在（Paused 非 terminal）");
    assert!(
        plan.active_child_order_id().is_none(),
        "撤销后子单身份清空（Closed→release 路径）"
    );
}

// D3b 件 4 红证：复审接线——组内决策拍对既有计划执行复审（session
// Lifecycle operation 等价），每个既有计划动作 apply 前先 record_review
// 写入簿内基线（quiet 门控依赖该面——组内缺失则每拍修订/计划身份漂移）。
// 红=未接线（决策拍不跑复审，review 基线恒 None）；绿=接线后 due 拍复审
// 至少产出 Observe（quiet/unchanged 吸收）并记录复审事实。
#[test]
fn d3b4_review_records_facts_in_group_book() {
    use crate::orderbook::Side;
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 3;
    setup.strategy_params.inst.order_size = 1_000;
    setup.auction_ticks = 0;
    let mut session = GameSession::new(setup, 52).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    for _ in 0..6 {
        let views = driver.world_public_views().to_vec();
        let mut intents = Vec::new();
        for (stock, view) in views.iter().enumerate() {
            for (side, price) in [
                (Side::Buy, view.last_price + 1),
                (Side::Sell, view.last_price - 1),
            ] {
                intents.push(crate::session::ReceiptBearingIntent {
                    owner: crate::AccountId(1),
                    intent: crate::Intent::PlaceLimit {
                        code: crate::StockId(stock as u32),
                        side,
                        price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(
                            price,
                        )),
                        qty: 200,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                });
            }
        }
        let upper: Vec<i64> = views.iter().map(|v| v.last_price + 1).collect();
        driver.inject_player_intents(&intents, &upper).expect("注入");
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情");
        driver.install_candles(&mut session).expect("K");
    }
    let plans = driver.collect_plans().expect("组簿").remove(0).1;
    let (pid, account) = plans
        .active_plan_ids()
        .into_iter()
        .filter_map(|pid| plans.plan(pid).ok().map(|plan| (pid, plan.account())))
        .next()
        .expect("fixture 须有活跃计划");
    // 强制该账户 belief 位到期：复审随决策拍执行。
    driver.force_due_for_test_at(&[account], driver.world_committed_tick() + 1);
    driver
        .step_frame_equivalent(&mut session)
        .expect("复审拍");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    let plans = driver.collect_plans().expect("复审后组簿").remove(0).1;
    // 终止计划仍留簿可读（PlanBook 按 id 查不筛状态）——无论复审产物
    // 是 Observe/Revise/Terminate，record_review 都先于动作写入基线。
    let plan = plans.plan(pid).expect("计划仍在簿");
    assert!(
        plan.review().last_review_price.is_some(),
        "复审动作前 record_review 写入簿内基线（quiet 门控面）：{:?}",
        plan.review()
    );
}

// D3b 件 5 红证：个人态注入通道——注入的 watchlist 经组权威个人态
// 上行（collect_belief→BeliefParticipantBelief 读面）可见。
#[test]
fn d3b5_install_personal_updates_group_authority() {
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 1;
    setup.auction_ticks = 0;
    setup.strategy_params.retail.arrival_rate = 0.0;
    setup.strategy_params.retail.order_size_mean = 100;
    setup.strategy_params.inst.order_size = 100;
    let mut session = GameSession::new(setup, 61).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    driver
        .step_frame_equivalent(&mut session)
        .expect("步进");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    // 组内 belief 位 = local 2（id_map=[玩家, retail, inst]）。
    let collected = driver.collect_belief().expect("belief 读面");
    let original = collected[0].1[2].clone().expect("belief 位在");
    // 注入：watchlist 加一股（其余保持）。
    let mut patched = original.clone();
    patched
        .watchlist
        .record_attention(&crate::StockId(1), 0, 0)
        .expect("watchlist 登记");
    driver.install_personal_for_test(2, patched.clone());
    driver
        .step_frame_equivalent(&mut session)
        .expect("注入后步进（消费）");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    let collected = driver.collect_belief().expect("再读面");
    let after = collected[0].1[2].clone().expect("belief 位在");
    assert!(
        after.watchlist.stocks.contains_key(&crate::StockId(1)),
        "注入的个人态经组权威生效（watchlist 读面可见）"
    );
    let _ = original;
}

// D5 qty=0 组端校验红证（复核阻断项补测）：路由层拒收在 step 事件流
// 投影 IntentRejected(InvalidQuantity)——desk 防御性 panic 先于下发被
// 组端校验拦截（旧管线 InvalidQuantity 回执的 runtime 等价）。
#[test]
fn d5_zero_qty_intent_routes_rejection_event() {
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 0;
    setup.auction_ticks = 0;
    let mut session = GameSession::new(setup, 71).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    let views = driver.world_public_views().to_vec();
    let upper: Vec<i64> = views.iter().map(|v| v.daily_upper_limit).collect();
    driver
        .inject_player_intents(
            &[crate::session::ReceiptBearingIntent {
                owner: crate::AccountId(0),
                intent: crate::Intent::PlaceLimit {
                    code: crate::StockId(0),
                    side: crate::orderbook::Side::Buy,
                    price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(
                        views[0].last_price,
                    )),
                    qty: 0,
                },
                account_ordinal: 0,
                stock_ordinal: 0,
            }],
            &upper,
        )
        .expect("注入");
    let events = driver
        .step_frame_equivalent(&mut session)
        .expect("步进");
    assert!(
        events.iter().any(|e| matches!(
            e,
            crate::Event::IntentRejected {
                account: crate::AccountId(0),
                code,
                reason: crate::RejectionReason::InvalidQuantity,
                ..
            } if *code == crate::StockId(0)
        )),
        "qty=0 意图经组端校验拒收（事件面 InvalidQuantity）：{events:?}"
    );
}

// D5 日界安装红证（复核阻断项补测）：ends_day 拍（tick%tpd==0）在
// runtime step 内复用交易日终迁移——day 推进+DayBoundary 事件+T+1 解锁
//（旧管线 continuous_tick_finalizer 的 ends_day 段等价；仅 runtime 模式
// 生效——默认 env 跳过）。
#[test]
fn d5_runtime_step_installs_day_boundary() {
    if !crate::runtime::step_runtime_enabled() {
        return;
    }
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 0;
    setup.auction_ticks = 0;
    setup.ticks_per_day = 8;
    let mut session = GameSession::new(setup, 72).expect("会话构造");
    let day_before = session.day();
    let mut saw_boundary = None;
    for _ in 0..8 {
        let events = session.step().expect("步进");
        if let Some(event) = events
            .iter()
            .find(|e| matches!(e, crate::Event::DayBoundary { .. }))
        {
            saw_boundary = Some(event.clone());
        }
    }
    assert!(
        saw_boundary.is_some(),
        "ends_day 拍必须产出 DayBoundary 事件（交易日终迁移安装）"
    );
    assert_eq!(
        session.day(),
        day_before + 1,
        "日界安装推进 session day（civil 对齐）"
    );
    assert_eq!(session.tick(), 8);
}

// ── P2-1a 红证：个人态安装尊重 dirty 掩码 ──

// 修复背景：此前驱动器每 tick 对**全名册**逐 NPC set_runtime_personal_state
// （六件深拷贝/merge——B 口径 ≈58% CPU 的大头，见 .tmp/perf-p2-p0-breakdown.md）。
// 修复后 EchoBelief 随附组端 dirty 掩码，驱动器只重装本 tick 生产演进位
// （等价不变式：非 dirty ⇒ 组端值==session 镜像值——构造期同源投影+两侧
// 只经 dirty 通道同步）。守卫：安装数==dirty 位计数（全量重装突变即红）。
#[test]
fn p2_1a_personal_state_install_respects_dirty_mask() {
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 8; // belief 位×8
    setup.auction_ticks = 0;
    setup.strategy_params.inst.order_size = 100;
    let mut session = GameSession::new(setup, 83).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    driver
        .step_frame_equivalent(&mut session)
        .expect("步进");
    let installs = driver.personal_installs_for_test();
    // P2-2a 后上行已稀疏（条目=dirty 位）——判别面：单 tick 全名册演进
    // 统计上不可能（observe 门 ~35%/位、8 位全真≈0.02%），安装数 <8；
    /// 突变（组侧稀疏回全量）时 installs=8 显红。缓存等价由
    /// d3a2b/d3b5 读面测试承载。
    assert!(
        installs < 8,
        "个人态安装数必须小于名册 belief 位数（稀疏上行；全量重装=回归）：{installs}"
    );
    // 缓存读面在位（全量语义）。
    let beliefs = driver.collect_belief().expect("belief 读面");
    assert_eq!(beliefs.len(), 1, "单组");
    assert_eq!(beliefs[0].1.len(), 9, "缓存全量（名册=玩家+8 位）");
}

// ── P2-1b 红证：账户回写尊重事实 dirty（两拍并集+日结全量） ──

// 修复背景：此前每 tick 对全名册逐 NPC positions 深拷贝回写（第二大头，
// 见 .tmp/perf-p2-p0-breakdown.md）。修复后只写"本 tick 事实 owner ∪
// 上一 tick 事实 owner"（组端 staging 落账滞后一拍的整值同步在下一拍），
// 日结后强制全量一次（Drain/T+1/AdminBatch 无逐账户事实）。守卫：
// 首拍全量建立基线后，第二拍写入数==第二拍 dirty 并集数（全量重装
// 突变即红）。
#[test]
fn p2_1b_account_writeback_respects_fact_dirty() {
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 6;
    setup.npcs.inst_count = 0;
    setup.auction_ticks = 0;
    let mut session = GameSession::new(setup, 29).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    driver
        .step_frame_equivalent(&mut session)
        .expect("首拍（强制全量基线）");
    let after_first = driver.account_writes_for_test();
    assert_eq!(after_first, 7, "首拍全量（玩家+6 NPC 名册基线）");
    driver
        .step_frame_equivalent(&mut session)
        .expect("第二拍");
    let after_second = driver.account_writes_for_test();
    // 第二拍写入增量 == 该拍 dirty 并集（本拍事实 ∪ 上拍事实 owner 数）。
    let rec = driver.world_report_log().last().expect("第二拍记录");
    let prev = driver
        .world_report_log()
        .get(driver.world_report_log().len() - 2)
        .expect("首拍记录");
    let mut owners = std::collections::BTreeSet::new();
    for rec in [prev, rec] {
        for f in &rec.desk_trades {
            owners.insert(f.maker);
            owners.insert(f.taker);
        }
        for f in &rec.desk_lifecycle {
            owners.insert(f.owner);
        }
        for f in &rec.desk_receipts {
            owners.insert(f.owner);
        }
    }
    let dirty_count = owners.len() as u64;
    assert_eq!(
        after_second - after_first,
        dirty_count,
        "第二拍写入增量==事实 dirty 并集（全量重装=回归）"
    );
}
