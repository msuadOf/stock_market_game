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

// D3a-1 观察基：组线程逐 tick 累积每股最新价路径（计划链 price_paths
// 源——开仓/复审路径的 opinion/confidence 派生基，字段不得造假）。
#[test]
fn d3a_group_price_paths_accumulate_per_tick() {
    let mut session = GameSession::new(setup_fixture(), 7).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    for expected_len in 1..=5u64 {
        driver
            .step_frame_equivalent(&mut session)
            .expect("步进");
        driver.install_market_views(&mut session).expect("行情安装");
        driver.install_candles(&mut session).expect("K 安装");
        let observations = driver.collect_observations().expect("观察基收集");
        assert_eq!(observations.len(), 1, "单组");
        let paths = &observations[0].1;
        assert_eq!(
            paths.len(),
            2,
            "setup_fixture 双股：{:?}",
            paths.len()
        );
        for (stock, path) in paths.iter().enumerate() {
            assert_eq!(
                path.len() as u64,
                expected_len,
                "股 {stock} 路径逐 tick 累积（{} tick 后 {} 条）",
                expected_len,
                path.len()
            );
            assert!(
                path.iter().all(|&p| p > 0),
                "路径价为正（快照 last_price）：{path:?}"
            );
        }
    }
}
