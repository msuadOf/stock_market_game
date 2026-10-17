use super::*;

#[test]
fn skeleton_failure_preserves_business_and_poison_context() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let business = game.business_state_hash().unwrap();
    let session = game.session_state_hash().unwrap();
    let fatal = StepFatal::InvariantViolation {
        description: "injected skeleton failure".to_owned(),
        location: "step.pre_mutation".to_owned(),
    };
    game.inject_step_failure(fatal.clone());
    assert_eq!(game.session_state_hash().unwrap(), session);
    assert_eq!(game.step().unwrap_err(), fatal);
    assert_eq!(game.business_state_hash().unwrap(), business);
    assert_ne!(game.session_state_hash().unwrap(), session);
    assert_eq!(game.poison_reason(), Some(&fatal));
    assert_eq!(game.step().unwrap_err(), fatal);
    assert_eq!(game.save().unwrap_err(), fatal);
    assert!(matches!(game.end_civil_day(), Err(SessionError::Step(error)) if error == fatal));
    let poisoned = game.poison.take();
    assert_eq!(game.session_state_hash().unwrap(), session);
    game.poison = poisoned;
    println!(
        "poison QA: exact Err; business unchanged; session changes only by poison; repeated step/save reject"
    );
}

#[test]
fn public_step_rejects_and_poisons_a_missing_npc_strategy() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.fixture_account_mut(&AccountId(1))
        .unwrap()
        .fixture_set_strategy(None);
    let business = game.business_state_hash().unwrap();

    let fatal = game.step().unwrap_err();

    assert_eq!(
        fatal,
        StepFatal::InvariantViolation {
            description: "non-player account has no authoritative strategy".to_owned(),
            location: "GameSession::step".to_owned(),
        }
    );
    assert_eq!(game.business_state_hash().unwrap(), business);
    assert_eq!(game.poison_reason(), Some(&fatal));
}

#[test]
fn injected_failure_runs_before_missing_npc_strategy_validation() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.fixture_account_mut(&AccountId(1))
        .unwrap()
        .fixture_set_strategy(None);
    let injected = StepFatal::InvariantViolation {
        description: "injected before malformed strategy validation".to_owned(),
        location: "failure_tests::missing_strategy_hook_order".to_owned(),
    };
    game.inject_step_failure(injected.clone());

    assert_eq!(game.step().unwrap_err(), injected);
    assert_eq!(game.poison_reason(), Some(&injected));
}

#[test]
fn business_hash_detects_pending_facts_counters_and_strategy_state() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let before = game.business_state_hash().unwrap();
    game.state.next_order_id += 1;
    assert_ne!(game.business_state_hash().unwrap(), before);
    game.state.next_order_id -= 1;
    game.state.next_receipt_base += 1;
    assert_ne!(game.business_state_hash().unwrap(), before);
    game.state.next_receipt_base -= 1;
    let plan_id = game
        .state
        .plans
        .create(crate::plans::PlanOpen {
            account: AccountId(1),
            code: StockId(0),
            direction: crate::Side::Buy,
            target: crate::plans::PlanTarget::ShareCount(100),
            opinion: crate::plans::PlanOpinion {
                signal_score_bp: 3_000,
                source: crate::plans::OpinionSource::Blended,
            },
            confidence_bp: 8_000,
            urgency: crate::plans::Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 0,
        })
        .unwrap();
    // Astra 终审 P2a：execution 与 strategy 各自建立"变更前"独立基线——
    // 旧写法在新增计划后仍用开局基线断言 strategy 差异，计划本身即让断言通过，
    // 检错力被稀释；现改为逐变更独立对照，恢复各自独立的检错能力。
    let with_plan = game.business_state_hash().unwrap();
    game.state
        .plans
        .install_execution(plan_id, 100, crate::Money::from_cents(1_000), 240)
        .unwrap();
    assert_ne!(
        game.business_state_hash().unwrap(),
        with_plan,
        "执行腿变更必须独立于计划创建被检出"
    );
    let with_execution = game.business_state_hash().unwrap();
    let account = game.fixture_account_mut(&AccountId(1)).unwrap();
    account.set_strategy(Box::new(
        crate::MomentumStrategy::new(5, 0.04, 100).unwrap(),
    ));
    assert_ne!(
        game.business_state_hash().unwrap(),
        with_execution,
        "strategy 变更必须独立于计划/执行腿被检出"
    );
}

#[test]
fn diagnostic_metadata_changes_only_session_hash() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let business = game.business_state_hash().unwrap();
    let session = game.session_state_hash().unwrap();
    game.state
        .last_retail_order_events
        .push(RetailOrderDiagnosticEvent::Rejected {
            account: AccountId(1),
            code: StockId(0),
            reason: RejectionReason::InsufficientCash,
        });
    assert_eq!(game.business_state_hash().unwrap(), business);
    assert_ne!(game.session_state_hash().unwrap(), session);
}

#[test]
fn typed_private_plan_failure_is_poisoned_without_mutating_authority() {
    let code = StockId(0);
    let account = AccountId(1);
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.fixture_account_mut(&account)
        .unwrap()
        .fixture_set_strategy(None);
    game.fixture_account_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 2, Money::from_cents(1))
        .unwrap();
    game.state
        .markets
        .get_mut(&code)
        .unwrap()
        .fixture_set_last_price(Money::from_cents(i64::MAX));
    let business_before = game.business_state_hash().unwrap();
    let session_before = game.session_state_hash().unwrap();

    let fatal = match super::pipeline::plan_tick(super::pipeline::PhaseInput {
        session: &game,
    }) {
        Ok(_) => panic!("private plan must reject the overflowing allocation snapshot"),
        Err(fatal) => fatal,
    };
    let fatal = game.poison_failed_step(fatal);

    assert!(matches!(fatal, StepFatal::InvariantViolation { .. }));
    assert_eq!(game.business_state_hash().unwrap(), business_before);
    assert_ne!(game.session_state_hash().unwrap(), session_before);
    assert_eq!(game.poison_reason(), Some(&fatal));
    assert_eq!(game.step().unwrap_err(), fatal);
    assert_eq!(game.save().unwrap_err(), fatal);
}

// ── Sol 二审修复集 3：runtime 步进失败立即停止安装 + 安装失败撤销已推进状态 ──

#[test]
fn sol3_runtime_step_failure_stops_install() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    driver.adopt_session_seq(&game);
    // 注入 tick1 校验失败（世界 Abort——step 层失败）。
    driver.set_world_validator_for_test(Box::new(|ev| {
        if ev.tick == 1 {
            Err("sol3 step 注入".to_owned())
        } else {
            Ok(())
        }
    }));
    // 同时注入安装失败：fail-fast 下返回的一定是 **step 错误**（安装
    // 根本不执行）；旧行为（无条件安装）会返回安装错误——用安装失败
    // 掩盖 tick 失败的真实原因。
    game.inject_runtime_install_failure();
    let err = game.runtime_step_with_install(&mut driver).unwrap_err();
    assert!(
        err.to_string().contains("sol3 step 注入"),
        "step 失败必须先于安装报告（不掩盖）：{err}"
    );
    assert!(
        !err.to_string().contains("安装失败注入"),
        "不得以安装失败掩盖 step 失败：{err}"
    );
    assert_eq!(game.poison_reason(), Some(&err), "失败 poison");
    // fail-fast：安装未执行（tick 0 已回滚世界本就无可安装内容——
    // 控制面判据即上面的错误身份断言）。
    assert_eq!(game.tick(), 0, "tick 不推进");
}

#[test]
fn sol3_runtime_install_failure_rolls_back_advanced_state() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    driver.adopt_session_seq(&game);
    let tick_before = game.tick();
    let seq_before = game.current_seq();
    let accounts_before = game.runtime_accounts_snapshot();
    // 注入安装失败（step 成功后、安装前——真实安装失败经 install_* 的
    // Result 走同一回滚分支）。
    game.inject_runtime_install_failure();
    let err = game.runtime_step_with_install(&mut driver).unwrap_err();
    assert!(
        err.to_string().contains("安装失败注入"),
        "安装失败可辨识：{err}"
    );
    // 撤销已推进状态：tick/seq/账户全量回到基线（此前残留半安装会话）。
    assert_eq!(game.tick(), tick_before, "tick 回滚到基线");
    assert_eq!(game.current_seq(), seq_before, "seq 回滚到基线");
    let accounts_after = game.runtime_accounts_snapshot();
    assert_eq!(
        accounts_after.len(),
        accounts_before.len(),
        "账户面完整回滚（名册不变）"
    );
    for (id, cash, positions) in &accounts_before {
        let (_, cash_after, positions_after) = accounts_after
            .iter()
            .find(|(i, _, _)| i == id)
            .expect("账户仍在名册");
        assert_eq!(cash_after, cash, "账户 {id:?} 现金回滚");
        // Position 无 PartialEq——按键集+股数比对（回滚是整值替换，
        // 键集与股数一致即整表一致）。
        let keys_after: Vec<_> = positions_after.keys().collect();
        let keys_before: Vec<_> = positions.keys().collect();
        assert_eq!(keys_after, keys_before, "账户 {id:?} 持仓键集回滚");
        for (code, pos) in positions {
            assert_eq!(
                positions_after.get(code).expect("键集已比对").qty(),
                pos.qty(),
                "账户 {id:?} 股 {code:?} 股数回滚"
            );
        }
    }
    assert_eq!(game.poison_reason(), Some(&err), "安装失败 poison");
}

// Sol 二审修复集 5：candidate 写回与 attention_scheduler heap 同步。
#[test]
fn sol5_set_runtime_attention_syncs_scheduler_heap() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let id = crate::AccountId(1);
    game.set_runtime_attention(&[(id, 999_999, 7)]);
    // candidate 已更新。
    assert_eq!(
        game.state
            .npc_attention
            .get(&id)
            .expect("NPC 有 attention 状态")
            .next_attention_candidate_tick,
        999_999,
        "candidate 写回"
    );
    // heap 同步：新候选入堆（pop_due 由候选核对去陈旧项——堆可含旧项，
    // 但必须含新候选，否则调度面永远看不到它）。
    let heap: Vec<_> = game.state.attention_scheduler.iter().copied().collect();
    assert!(
        heap.contains(&(999_999, id)),
        "新候选必须入 attention_scheduler 堆：{heap:?}"
    );
}

// P1-C 红证：runtime 路径 attention_scheduler heap **无消费增长**——
// install_market_views 每 tick 经 set_runtime_attention 把全名册候选
// enqueue 入堆，而 runtime 路径永不 pop_due（pop 只在旧管线决策链），
// 安装基线又每 tick clone 整堆（O(N·T²) 累计复制——B 口径随时长恶化
// 的结构性来源，Sol 终裁判定，见 .tmp/perf-p0-diff-profile.md）。
// 修复合同：heap=权威 npc_attention 的**镜像**（每账户恰一项、值=权威
// 候选 tick、不随 tick 增长；Sol 二审修复集 5 的"新候选入堆"语义保持
// ——重建形态下新候选必然在堆）。
#[test]
fn runtime_attention_heap_mirror_does_not_grow_across_ticks() {
    if !crate::runtime::step_runtime_enabled() {
        // 缺省旧管线不走 runtime 安装链——本红证由 env=1 批次承载。
        return;
    }
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let roster = game.state.npc_attention.len();
    for _ in 0..5 {
        game.step().unwrap();
    }
    let heap_len = game.state.attention_scheduler.iter().count();
    assert!(
        heap_len <= roster,
        "attention heap 不得随 tick 无界增长：heap={heap_len} 项 > 名册 {roster}（runtime 每 tick 全名册 enqueue 且永不 pop_due）"
    );
    // 镜像完整性：堆项=权威候选 tick 且每账户恰一项。
    let mut per_id: std::collections::BTreeMap<crate::AccountId, usize> = Default::default();
    for (heap_tick, id) in game.state.attention_scheduler.iter().copied() {
        *per_id.entry(id).or_default() += 1;
        let authority = game
            .state
            .npc_attention
            .get(&id)
            .expect("堆内 id 必在名册")
            .next_attention_candidate_tick;
        assert_eq!(
            heap_tick, authority,
            "堆项必须=权威候选 tick（镜像不陈旧）：{id:?}"
        );
    }
    assert_eq!(per_id.len(), roster, "镜像覆盖全名册");
    for (id, count) in per_id {
        assert_eq!(count, 1, "每账户恰一项（镜像不重复）：{id:?}");
    }
}

// Sol 三审 (b)：安装失败回滚缺口——market_views 已写入行情/历史/
// 注意力后失败，回滚必须覆盖安装面写入（此前只恢复 tick/seq/账户，
// 半安装会话残留）。
#[test]
fn sol3b_install_failure_rolls_back_market_view_writes() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    driver.adopt_session_seq(&game);
    let hist_before = game.runtime_price_history_len(&crate::StockId(0));
    let attention_before = game.runtime_attention_state();
    let markets_before: Vec<_> = [0u32]
        .iter()
        .map(|&i| {
            let id = crate::StockId(i);
            let m = game.market_mut_for_runtime(&id).expect("市场在册");
            (id, m.last_price(), m.last_close())
        })
        .collect();
    game.inject_runtime_install_failure();
    let err = game.runtime_step_with_install(&mut driver).unwrap_err();
    assert!(
        err.to_string().contains("安装失败注入"),
        "安装失败可辨识：{err}"
    );
    // 注入点在 market_views 之后：行情/历史/注意力已被写入——回滚后
    // 必须全部回到基线（Sol 三审：此前残留半安装状态）。
    assert_eq!(
        game.runtime_price_history_len(&crate::StockId(0)),
        hist_before,
        "价格历史回滚（market_views 已写入）"
    );
    assert_eq!(
        game.runtime_attention_state(),
        attention_before,
        "attention candidate/RNG 回滚（market_views 已写入）"
    );
    for (id, last, close) in &markets_before {
        let m = game.market_mut_for_runtime(id).expect("市场在册");
        assert_eq!(
            (m.last_price(), m.last_close()),
            (*last, *close),
            "市场权威价回滚：{id:?}"
        );
    }
    assert_eq!(game.tick(), 0, "tick 回滚");
}

// ── D0（常驻 driver）：非首 tick 门禁移除 + 锁步不变量 ──

#[test]
fn d0_step_via_runtime_continues_across_ticks_and_keeps_book() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    // 玩家现金置充足 + 挂跌停价买单（900=带下限，闭区间合法；无对手
    // 档 ⇒ 确定性挂簿不成交——inst NPC 报价围绕 1000±5%，不会到 900）。
    game.fixture_account_mut(&crate::AccountId(0))
        .unwrap()
        .fixture_set_cash(crate::money::Money::from_cents(100_000_000));
    game.enqueue_player_intent(
        crate::AccountId(0),
        crate::strategy::Intent::PlaceLimit {
            code: crate::StockId(0),
            side: crate::orderbook::Side::Buy,
            price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(900)),
            qty: 100,
        },
    )
    .expect("玩家意图入队");
    // 第一步：挂单受理（OrderAccepted 事件）。
    let ev1 = game.step_via_runtime().expect("第一步 step");
    assert!(
        ev1.iter().any(|e| matches!(
            e,
            Event::OrderAccepted { account, .. } if account.0 == 0
        )),
        "玩家挂单受理：{ev1:?}"
    );
    assert_eq!(game.tick(), 1);
    // 第二步：不再被首 tick 门禁拒绝（D0 前红：仅支持首 tick）。
    game.step_via_runtime().expect("第二步 step（门禁移除）");
    assert_eq!(game.tick(), 2);
    // 第三步：常驻 driver——report_log 连续累积（每步重建则新世界
    // 只有 1 条记录），玩家挂单仍在簿（desk_resting ≥ 1）。
    game.step_via_runtime().expect("第三步 step");
    assert_eq!(game.tick(), 3);
    let driver = game
        .runtime_driver
        .as_ref()
        .expect("常驻驱动器")
        .lock()
        .expect("常驻驱动器锁");
    assert_eq!(driver.world_report_log().len(), 3, "单一世界连续三 tick");
    assert_eq!(driver.world_committed_tick(), 3, "锁步：world==session");
    let last = driver.world_report_log().last().expect("三条记录");
    assert!(
        last.desk_resting.iter().any(|(s, r)| *s == 0 && *r >= 1),
        "挂单跨步保持（tick3 仍在簿）：{:?}",
        last.desk_resting
    );
}

#[test]
fn d0_lockstep_violation_fails_explicitly() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.step_via_runtime().expect("第一步");
    // 人为破坏锁步（直改入口）——下一步必须显式报锁步失配，
    // 不静默重放也不误报为门禁。
    game.set_runtime_tick(50);
    let err = game.step_via_runtime().expect_err("锁步失配必须显式失败");
    assert!(
        err.to_string().contains("锁步"),
        "错误须指明锁步失配：{err}"
    );
}
