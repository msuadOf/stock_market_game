//! 驱动器安装面测试（Sol 二审修复集 3）：K 投影收集错误传播。

use super::bridge_tests::setup_fixture;
use super::drive::RuntimeDriver;
use crate::GameSession;

// 断开世界后 install_candles 必须显式失败（此前 take_candle_projection
// 吞发送错误并返回部分数据——部分 K 安装=静默语义丢失）。
#[test]
fn sol3_install_candles_propagates_collection_errors() {
    let mut session = GameSession::new(setup_fixture(), 77).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器构造");
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
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器构造");
    let collected = driver.collect_attention().expect("注意力收集");
    let mut checked = 0usize;
    for (_g, entries) in collected {
        for (local, next_due) in entries {
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
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器构造");
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
    fn accepted_with(arrival: f64) -> usize {
        let mut setup = setup_fixture();
        setup.strategy_params.retail.arrival_rate = arrival;
        let mut session = GameSession::new(setup, 5).expect("会话构造");
        let mut driver = RuntimeDriver::from_session(&session).expect("驱动器构造");
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
    let cold = accepted_with(0.0);
    assert!(hot > 0, "到达率 1.0 必产受理单（{hot}）");
    assert_eq!(
        cold, 0,
        "到达率 0.0 零下单——runtime 须用会话参数（合成 0.30 时此断言红）"
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
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器构造");
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
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器构造");
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
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器构造");
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
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器构造");
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
    let observation = crate::session::DecisionChainObservation {
        market: derivation_market.clone(),
        paths,
        technical,
        now: *derivation_now,
        exposed: std::collections::BTreeSet::new(),
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
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    for _ in 0..5 {
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情安装");
        driver.install_candles(&mut session).expect("K 安装");
    }
    // 派生（组内 observe+开仓消费）→ 下一交付点投影。
    let collected = driver.collect_open_derivations().expect("开仓派生收集");
    let records = &collected[0].3;
    assert!(!records.is_empty(), "有派生记录");
    let mut expected: Vec<(crate::AccountId, crate::StockId)> = Vec::new();
    for record in records {
        for open in &record.opens {
            expected.push((open.account, open.code.clone()));
        }
    }
    assert!(
        !expected.is_empty(),
        "场景须产生至少一个开仓（PlanOpen 落组内 PlanBook）"
    );
    // 交付点投影（step 内同款路径——此处显式驱动一步后的读面断言）。
    let plans = driver
        .collect_plans()
        .expect("计划簿收集")
        .into_iter()
        .next()
        .map(|(_, plans)| plans)
        .unwrap_or_default();
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
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器构造");
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
            allocation_cash: view.daily_upper_limit
                .mul_shares(*qty)
                .expect("预留金额"),
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

fn session_config() -> crate::config::GameConfig {
    crate::config::GameConfig::proposed_defaults()
}

// D3a-3 执行腿闭环红证：多轮内同计划不重复挂子单（execution 腿在=报价
// 跳过）+成交推进（FillFact→ChildOrderFilled——filled_qty 真实前进）。
#[test]
fn d3a3_execution_leg_no_duplicate_children_and_fills_advance() {
    let mut setup = setup_fixture();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 3;
    setup.strategy_params.inst.order_size = 100;
    let mut session = GameSession::new(setup, 37).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器构造");
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
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器构造");
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
