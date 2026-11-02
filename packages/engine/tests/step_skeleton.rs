use engine::{
    CivilDate, Event, FloatAllocation, GameConfig, GameSession, HotParams, InstParams, Money,
    NpcSetup, RetailParams, RuntimeMirrorDayEndStage, RuntimeMirrorOrigin, RuntimeSyncPolicy,
    SecurityCategory, SessionSetup, StockCode, StockExchange, StockSpec, StrategyParams,
    TradingPhase,
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
        runtime_sync_policy: Default::default(),
    })
}

#[test]
fn characterization_ticks_preserve_price_and_day_boundaries(
) -> Result<(), Box<dyn std::error::Error>> {
    for auction_ticks in [0, 3, 6] {
        let mut game = GameSession::new(setup(auction_ticks)?, 42)?;
        let mut day_boundaries = 0;
        for expected_tick in 1_u64..=60 {
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
            if expected_tick.is_multiple_of(20) {
                // 市场日界只完成交易会话；下一交易日必须由真实 civil 日结进入。
                let settled_date = game.civil_date();
                let report = game.end_civil_day()?;
                assert_eq!(report.settled_date, settled_date);
                assert_eq!(report.next_date, settled_date.next()?);
                assert_eq!(game.civil_date(), report.next_date);
                assert_eq!(game.snapshot().tick, expected_tick);
                assert_eq!(game.snapshot().day, snapshot.day);
                assert_eq!(
                    report
                        .events
                        .iter()
                        .filter(|event| matches!(event, Event::CivilDateAdvanced { .. }))
                        .count(),
                    1
                );
                assert!(!report
                    .events
                    .iter()
                    .any(|event| matches!(event, Event::Trade { .. } | Event::DayBoundary { .. })));
            }
        }
        assert_eq!(day_boundaries, 3);
        assert_eq!(game.snapshot().day, 3);
    }
    Ok(())
}

fn complete_projection(auction_ticks: u64, seed: u64, ticks: u64) -> Vec<Vec<u8>> {
    // 零 NPC、零委托的原 fixture 仍走生产 step；本锚不承诺自由并发整局字节相同。
    let mut game = GameSession::new(setup(auction_ticks).unwrap(), seed).unwrap();
    (1..=ticks)
        .map(|expected_tick| {
            let events = game.step().expect("healthy step");
            let save = game.save().expect("healthy save");
            let snapshot = game.snapshot();
            assert_current_projection_fields(&snapshot, &save, auction_ticks, expected_tick);
            assert!(save.setup.config.price_cage_enabled);
            assert!(save.resting_orders.values().all(Vec::is_empty));
            assert!(game.player_working_orders().is_empty());
            assert!(!events
                .iter()
                .any(|event| matches!(event, Event::Trade { .. })));
            serde_json::to_vec(&(events, snapshot, save)).unwrap()
        })
        .collect()
}

fn assert_current_projection_fields(
    snapshot: &engine::Snapshot,
    save: &engine::SaveSlot,
    auction_ticks: u64,
    expected_tick: u64,
) {
    let day_tick = expected_tick % 20;
    let expected_phase = if day_tick < auction_ticks - auction_ticks / 3 {
        TradingPhase::CallAuction
    } else if day_tick < auction_ticks {
        TradingPhase::PreOpen
    } else if day_tick >= 18 {
        TradingPhase::ClosingAuction
    } else {
        TradingPhase::Continuous
    };
    assert_eq!(snapshot.tick, expected_tick);
    assert_eq!(snapshot.phase, expected_phase);
    // 首次 lazy 接管安装一次静点，其后每拍安装；末拍 retained 同 tick 再发布一次。
    assert_eq!(
        snapshot.mirror_origin,
        RuntimeMirrorOrigin {
            authority_epoch: 1,
            install_sequence: expected_tick + 1,
            source_tick: expected_tick,
            source_generation: expected_tick + u64::from(expected_tick == 20),
            day_end_stage: if expected_tick == 20 {
                RuntimeMirrorDayEndStage::RetainedPublished
            } else {
                RuntimeMirrorDayEndStage::Tick
            },
            non_tick_revision: 0,
            mirror_phase: expected_phase,
        }
    );
    assert_eq!(save.setup.runtime_sync_policy, RuntimeSyncPolicy::EveryTick);
    let calendar_policy =
        engine::calendar::CalendarPolicy::from_parts(save.civil_clock.policy.clone())
            .expect("存档日历政策必须能重新装配");
    assert_eq!(
        save.civil_clock.policy_digest.as_deref(),
        Some(calendar_policy.content_digest()),
        "存档 policy_digest 必须绑定完整 CalendarPolicySpec"
    );
    assert_eq!(
        save.civil_clock
            .policy
            .official_coverage
            .iter()
            .map(|entry| (entry.exchange, entry.year))
            .collect::<Vec<_>>(),
        vec![
            (engine::calendar::CalendarExchange::Sse, 2026),
            (engine::calendar::CalendarExchange::Szse, 2026),
        ],
        "2026 沪深官方休市覆盖必须按 (exchange, year) 规范顺序入档"
    );
    for entry in &save.civil_clock.policy.official_coverage {
        assert!(!entry.source_citation_id.is_empty());
        assert_eq!(entry.source_digest.len(), 64);
        assert!(entry
            .source_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit()));
        assert!(entry
            .closed_ranges
            .iter()
            .all(|range| range.from <= range.to));
    }
    assert!(matches!(
        &save.company_state,
        engine::session::SavedSessionCompanies::LegacyOperations { groups, .. } if groups.is_empty()
    ));
    assert!(save.report_correction_operations.is_empty());
    assert!(save.plans.owner_sequences().next().is_none());
    assert!(save.plans.plan_ids().next().is_none());
    assert_eq!(
        serde_json::to_value(&save.plans).unwrap(),
        serde_json::json!({
            "policy": {
                "reverse_revision_threshold_bp": 2000,
                "review_signal_delta_bp": 1000,
                "review_price_change_bp": 200
            },
            "owner_sequences": [],
            "plans": []
        })
    );
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
    // 2026-10-30 表示取证：旧 884bd5ad producer 的三锚实际精确复现；
    // 仅按已独审源契约前向搬移公司容器、增加 EveryTick/origin/空更正记录，
    // 并把 next_plan_seq=0、plans={} 的空计划簿转换为 owner 数组表示。
    // 从旧 raw 独立推导的新原字节与 c912 producer 全部 60 tuple 精确相同；
    // 当前完整 tuple 不剔字段，新增字段另有 typed/wire 强断言。
    // 原锚、source/probe 与原字节档案见 agents/branch-completion/
    // last-integration-2026-10-30/skeleton.md；未复用失配后重算的 actual 为期望值。
    // 2026-11-03 集成复验：main@0f40ffbc 与 965e5d84 的 60/60 拍 events、
    // live snapshot 原字节相同；save 仅新增 CalendarPolicySpec.official_coverage
    // （SSE/SZSE 2026）与 policy_digest。把集成 save 的该数组清空并删除
    // policy_digest 后，三个完整投影的 FNV 摘要精确回到 main 三锚。新增字段
    // 另有 from_parts 重算 digest、来源非空与规范顺序断言；旧锚全部保留。
    // 依据：agents/fix-projection-regression/projection-diff.md 与
    // agents/calendar-2026-official-20261102/evidence.md。
    assert_eq!(
        projection_digest(&complete_projection(auction_ticks, 42, 20)),
        expected
    );
}

// Keep each complete-day fixture independently bounded and runnable in parallel.
#[test]
fn characterization_complete_projection_without_opening_auction() {
    // Sol 六审段 1.2 重锚（逐字段取证后）：对齐批修复日结竞价事件序
    // （AuctionTick/AuctionCompleted 先于 DayBoundary、窗相位 N−1
    // 基准、drain 收盘清算事实上行——60/60 拍事件序列与基线零差，
    // 取证落盘 /tmp/forensic-final vs forensic-baseline）后，残余
    // 字节差=3 类已判表示面：desk serial 游标起点 0→1（v5 E 提交）、
    // desk_mirrored_resting 新键（runtime 形态标记）、收盘 cutoff
    // 跨日残留值 1（新日 CallAuction 窗全隐藏——值无消费面）。
    // 旧锚保留：14_183_549_150_061_140_820。
    // B4a 公司域重锚：AccountId serde 切 canonical 十进制字符串（存档
    // 字节形态变化）+ main 侧公司经营/披露结构接线，投影字节随之变化；
    // 事件与业务事实语义不变（域测试与 save 契约独立覆盖）。旧锚保留：
    // 16_114_484_025_310_678_034。
    // 2026 官方覆盖 + policy_digest 表示锚（依据同上）；
    // 旧锚保留：14_515_314_912_140_190_070。
    // 884bd5ad 原锚：16_039_080_022_122_580_669；当前锚由旧 raw 前向独立推导。
    assert_complete_projection(0, 7_902_364_060_655_783_278);
}

#[test]
fn characterization_complete_projection_with_three_auction_ticks() {
    // 同上重锚；旧锚保留：862_635_824_146_359_871。
    // B4a 公司域重锚（同 without 用例依据）；旧锚保留：
    // 15_244_279_962_941_296_384。
    // 2026 官方覆盖 + policy_digest 表示锚（依据同上）；
    // 旧锚保留：1_528_640_566_612_864_764。
    // 884bd5ad 原锚：7_734_365_835_074_963_189；当前锚由旧 raw 前向独立推导。
    assert_complete_projection(3, 13_359_828_032_896_992_400);
}

#[test]
fn characterization_complete_projection_with_six_auction_ticks() {
    // 同上重锚；旧锚保留：7_784_286_686_475_819_964。
    // B4a 公司域重锚（同 without 用例依据）；旧锚保留：
    // 12_603_764_770_532_460_219。
    // 2026 官方覆盖 + policy_digest 表示锚（依据同上）；
    // 旧锚保留：571_153_393_436_025_933。
    // 884bd5ad 原锚：6_825_886_869_182_669_854；当前锚由旧 raw 前向独立推导。
    assert_complete_projection(6, 4_756_588_382_933_877_897);
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
