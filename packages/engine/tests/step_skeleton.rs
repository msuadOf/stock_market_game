use engine::{
    CivilDate, Event, FloatAllocation, GameConfig, GameSession, HotParams, InstParams, Money,
    NpcSetup, RetailParams, SecurityCategory, SessionSetup, StockCode, StockExchange, StockSpec,
    StrategyParams,
};

fn setup(auction_ticks: u64) -> Result<SessionSetup, Box<dyn std::error::Error>> {
    Ok(SessionSetup {
        company_system: None,
        dividend_tax_mode: None,
        flat_withholding_bp: None,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
        par_value_per_share: None,
        auto_corporate_foundation: false,
        report_frequency: None,
        company_operations: None,
        groups: Vec::new(),
        stocks: vec![StockSpec {
            code: StockCode("600001".to_owned()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.1,
            tick: Money::from_cents(1),
            total_shares: 1_000_000,
            float_shares: 0,
        }],
        npcs: NpcSetup {
            retail_count: 0,
            inst_count: 0,
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
                margin: 0.03,
                order_size: 100,
            },
            hot: HotParams {
                lookback: 2,
                trend_threshold: 0.01,
                order_size: 100,
            },
        },
        ticks_per_day: 20,
        auction_ticks,
        closing_auction_ticks: 2,
        history_len: 20,
        t1_enabled: true,
        float_allocation: FloatAllocation::random(),
        start_date: CivilDate::from_iso("2030-01-02")?,
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_owned(),
    })
}

#[test]
fn characterization_ticks_preserve_price_and_day_boundaries(
) -> Result<(), Box<dyn std::error::Error>> {
    for auction_ticks in [0, 3, 6] {
        let mut game = GameSession::new(setup(auction_ticks)?, 42)?;
        let mut day_boundaries = 0;
        for expected_tick in 1..=60 {
            let events = game.step().expect("healthy step");
            day_boundaries += events
                .iter()
                .filter(|event| matches!(event, Event::DayBoundary { .. }))
                .count();
            let snapshot = game.snapshot();
            assert_eq!(snapshot.tick, expected_tick);
            assert_eq!(
                snapshot.markets[&game.stock_id("600001").expect("夹具股票必须在目录中")]
                    .last_price,
                Money::from_cents(1000)
            );
        }
        assert_eq!(day_boundaries, 3);
        assert_eq!(game.snapshot().day, 3);
    }
    Ok(())
}

fn complete_projection(auction_ticks: u64, seed: u64, ticks: u64) -> Vec<Vec<u8>> {
    // A single worker controls scheduling on the same production path. The
    // harness still runs the independent tests concurrently (ADR-0017).
            let mut game = GameSession::new(setup(auction_ticks).unwrap(), seed).unwrap();
        (0..ticks)
            .map(|_| {
                let events = game.step().expect("healthy step");
                let save = game.save().expect("healthy save");
                assert!(save.setup.config.price_cage_enabled);
                assert!(save.resting_orders.values().all(Vec::is_empty));
                assert!(game.player_working_orders().is_empty());
                assert!(!events
                    .iter()
                    .any(|event| matches!(event, Event::Trade { .. })));
                serde_json::to_vec(&(events, game.snapshot(), save)).unwrap()
            })
            .collect()
}

fn projection_digest(projection: &[Vec<u8>]) -> u64 {
    projection
        .iter()
        .flatten()
        .fold(0xcbf29ce484222325, |digest, byte| {
            (digest ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

fn assert_complete_projection(auction_ticks: u64, expected: u64) {
    // Current-contract anchors after ADR-0021/0022, not historical equivalence
    // evidence. In particular the saved projection explicitly records the price
    // cage switch and every market's historical book cursor. Each anchor covers
    // one complete 20-tick day; the independent boundary test still covers three
    // days. This avoids serializing the same
    // company history 180 times in a short characterization. The scenario retains
    // its independent price, day-boundary, no-order and no-trade assertions;
    // the test below checks repeatability and distinguishes seed and tick-order
    // perturbations.
    // Old escrow-v2 digests: 54819b49083a6de9 / a22f8a5f963d6b66 / 626ae53ddcd6fa58.
    // e777c90 historical-source comparison (2026-10-01): in all 60 tick
    // projections, events and live snapshots are identical. Saves differ only
    // by 8 derived fields removed per tick, urgency_policy added per tick and
    // 14 prior_year_end account arrays reordered/deduplicated per tick. Every
    // account amount and all remaining authoritative fields are equal.
    // Old digests: 7e8ca11d72f312a9 / 46bf286fb78cd84a / 169259aea14df5a0.
    // ADR-0029 命名取证：实际运行 c0ab429 与本轮源码的同一 fixture/seed/
    // 单 worker producer，三个旧锚全部复现。60 个逐 tick projection 的事件
    // 与 live snapshot 原字节相同；每个 Save 只改变 schema_version 2→3、
    // runtime_v2→runtime_state 及 simulation_policy_id 身份。仅按许可 path
    // 逆替换原文 token 后，全部 projection 原字节精确复现，业务事实与顺序不变。
    // 旧锚保留（auction_ticks=0/3/6）：1454294662355836716 /
    // 5242099653663156573 / 4627285058458614055。只重钉当前表示锚。
    // 当前契约移除根版本字段；表示锚由既有原字节仅删除该字段独立计算，
    // 保留事件、快照及所有业务事实。依据见工作记录 current-save-contract。
    // 补缺合并的独立 producer 对照确认 60 个事件/live snapshot 投影均不变；
    // 存档变化限于 setup、company_operations、closing_registry、public_library、
    // ops_wiring、groups，包含真实支付/披露权威事实，并非命名替换。
    // 原锚保留：16816661624066813714 / 17087109400303676999 /
    // 2354296198442943349；独立证据见 merge-characterization.md。
    // Q01 两个实际 producer 复现旧三锚，只在已核对类型的 Money path
    // 给原文数字 token 加引号后，60 个完整 projection 原字节精确复现，
    // 保留事件、权威业务事实、原顺序与所有独立 guards。旧锚保留：
    // 18099143602724613260 / 16192716476036528973 / 2620780488067564097；
    // 原字节 FNV/SHA 与逐 path 核验见 money-wire-golden.md。
    // T3 线重锚（增量五十一·B 类双登记）：决策前移（四十~四十八：attention
    // 节律/受理时点/个人 patch 发布进度）改变 projection 字节——A1 §8 输出
    // 等价收窄+§9.7 登记面。基线证据：三用例在 48/49 态已红
    // （logs/t3b5-baseline48/49.txt），非批四回归。旧锚保留（T3 前最后形态）：
    // 1454294662355836716 系列之后末代=14588880118627202930 /
    // 10855500538460332623 / 14140194587992130069；新锚=下列常量。
    assert_eq!(
        projection_digest(&complete_projection(auction_ticks, 42, 20)),
        expected
    );
}

// Keep each complete-day fixture independently bounded and runnable in parallel.
#[test]
fn characterization_complete_projection_without_opening_auction() {
    // Sol 六审段 1.2 重锚（逐字段取证后）：对齐批修复日结竞价事件序
    /// （AuctionTick/AuctionCompleted 先于 DayBoundary、窗相位 N−1
    /// 基准、drain 收盘清算事实上行——60/60 拍事件序列与基线零差，
    /// 取证落盘 /tmp/forensic-final vs forensic-baseline）后，残余
    /// 字节差=3 类已判表示面：desk serial 游标起点 0→1（v5 E 提交）、
    /// desk_mirrored_resting 新键（runtime 形态标记）、收盘 cutoff
    /// 跨日残留值 1（新日 CallAuction 窗全隐藏——值无消费面）。
    /// 旧锚保留：14_183_549_150_061_140_820。
    /// B4a 公司域重锚：AccountId serde 切 canonical 十进制字符串（存档
    /// 字节形态变化）+ main 侧公司经营/披露结构接线，投影字节随之变化；
    /// 事件与业务事实语义不变（域测试与 save 契约独立覆盖）。旧锚保留：
    /// 16_114_484_025_310_678_034。
    assert_complete_projection(0, 16_039_080_022_122_580_669);
}

#[test]
fn characterization_complete_projection_with_three_auction_ticks() {
    // 同上重锚；旧锚保留：862_635_824_146_359_871。
    // B4a 公司域重锚（同 without 用例依据）；旧锚保留：
    // 15_244_279_962_941_296_384。
    assert_complete_projection(3, 7_734_365_835_074_963_189);
}

#[test]
fn characterization_complete_projection_with_six_auction_ticks() {
    // 同上重锚；旧锚保留：7_784_286_686_475_819_964。
    // B4a 公司域重锚（同 without 用例依据）；旧锚保留：
    // 12_603_764_770_532_460_219。
    assert_complete_projection(6, 6_825_886_869_182_669_854);
}

#[test]
fn complete_projection_is_repeatable_and_distinguishes_seed_and_tick_order() {
    let projection = complete_projection(3, 42, 2);
    assert_eq!(projection, complete_projection(3, 42, 2));
    assert_ne!(
        projection_digest(&projection),
        projection_digest(&complete_projection(3, 43, 2)),
        "the complete save projection must retain seed-dependent state"
    );
    let mut reordered = projection.clone();
    reordered.swap(0, 1);
    assert_ne!(
        projection_digest(&projection),
        projection_digest(&reordered),
        "the digest must distinguish causal tick order"
    );
}

/// 收盘竞价截点不能残留到下一日 Continuous，也不能使 runtime 自产档被拒绝。
#[test]
fn runtime_save_resets_closing_cutoff_without_opening_auction() {
    let mut game = GameSession::new(setup(0).unwrap(), 42).unwrap();
    for _ in 0..20 {
        game.step().expect("无委托的市场 tick 必须健康推进");
        game.save().expect("每个 quiet point 的自产档必须通过校验");
    }
}

#[test]
fn healthy_step_returns_ok_and_advances_business_state() -> Result<(), Box<dyn std::error::Error>> {
    let mut game = GameSession::new(setup(0)?, 42)?;
    let before = game.business_state_hash()?;
    let events: Vec<Event> = game.step()?;
    assert!(!events.is_empty());
    assert_ne!(game.business_state_hash()?, before);
    assert!(game.poison_reason().is_none());
    assert!(game.save().is_ok());
    Ok(())
}
