// D3 首片沿真实 observe/report/mirror 安装验证 trace；完整 causal 来源另门接通。

fn d3_trace_session(institutions: u32, reports: bool) -> (GameSession, Vec<AccountId>) {
    d3_trace_session_with_day(institutions, reports, 1_000)
}

fn d3_trace_session_with_day(institutions: u32, reports: bool, ticks_per_day: u64) -> (GameSession, Vec<AccountId>) {
    let mut setup = probe_session().state.setup.clone();
    setup.npcs.inst_count = institutions;
    setup.npcs.retail_count = 0;
    setup.ticks_per_day = ticks_per_day;
    setup.closing_auction_ticks = 0;
    // 无可执行子单的真实观察，隔离当前 NPC causal 来源尚未接通的门禁。
    setup.strategy_params.inst.order_size = 100;
    setup.stocks[0].float_shares = 0;
    if reports {
        let mut config = simple_company_fixture!(crate; ["000812"]);
        let crate::company::config::CompanySystemConfig::Simple(simple) = &mut config else {
            unreachable!("fixture 必须为 Simple");
        };
        // 所选 Simple 自己生成已公开年报，不借 Legacy 材料满足估值前置。
        simple.prehistory_periods = 24;
        setup.company_system = Some(config);
        setup.dividend_tax_mode = Some(crate::company::cash_dividend_tax::CashDividendTaxMode::FlatWithholding);
        setup.flat_withholding_bp = Some(1_000);
        setup.par_value_per_share = Some(Money::from_cents(100));
    }
    let mut session = GameSession::new(setup, 42).unwrap();
    if reports {
        let stock = session.stock_id("000812").unwrap();
        let company = session.state.issuer_of_stock(stock).unwrap();
        let system = session.company_system().expect("报告正控必须选择 Simple");
        assert_eq!(system.issuers().issuer_of(&StockCode("000812".to_owned())).unwrap(), company);
        let through = session.chain_observation_instant();
        let published = session.state.library.reports_for_company(&company, through);
        assert!(!published.is_empty(), "D3 报告正控必须有所选公司真实已公开材料");
        assert!(published.iter().all(|report| report.company == company
            && report.source == crate::information::PublicationSource::SimpleGenerated
            && report.published_at <= through), "D3 不得用 Legacy 或未来材料补足公开前置");
        assert!(published.iter().any(|report| report.reports.kind == crate::accounting::reports::ReportKind::Annual),
            "EarningsMultiple 正控必须已有真实公开年报");
    }
    let ids = prepare_company_source_personal_inputs(&mut session, true);
    assert_eq!(ids.len(), institutions as usize);
    for account in &ids {
        // 共用选源 helper 为观察覆盖改过 attention；D3 用 ForceDue，不改变持久策略身份。
        let probability = session.account(*account).unwrap().strategy().unwrap().base_observation_probability();
        session.state.npc_attention.get_mut(account).unwrap().base_probability = probability;
        assert!(session.account(*account).unwrap().positions().is_empty());
        // 合法整手配置保持100股；本人一分现金不足购买一手，真实预算挡住 quote。
        session.fixture_account_mut(account).unwrap().fixture_set_cash(Money::from_cents(1));
    }
    session.save().expect("D3 初始 fixture 必须通过真实 SaveSlot 校验");
    (session, ids)
}

fn d3_install_real_candidate(
    session: &mut GameSession,
    driver: &mut crate::runtime::drive::RuntimeDriver,
    ids: &[AccountId],
) -> Vec<Event> {
    let tick = session.tick() + 1;
    driver.force_due_for_test_at(ids, tick);
    let input = driver.prepare_driver_step(session).expect("真实 World 生产候选");
    let candidate = session.prepare_runtime_mirror(driver, input, tick).expect("真实镜像候选");
    session.validate_runtime_mirror_source(driver, &candidate).unwrap();
    session.install_runtime_mirror(driver, candidate)
}

#[test]
fn d3_runtime_trace_installs_real_reports_and_current_belief_for_two_accounts() {
    let (mut session, ids) = d3_trace_session(2, true);
    let stock = session.stock_id("000812").unwrap();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    assert_eq!(driver.accounts_per_group(), 2, "玩家和首机构之后必须有尾组");
    for tick in 1..=2 {
        d3_install_real_candidate(&mut session, &mut driver, &ids);
        for account in &ids {
            let records = session.npc_decision_trace(*account).expect("真实机构必须发布 trace");
            assert_eq!(records.len(), tick as usize);
            let record = records.last().unwrap();
            assert_eq!((record.account, record.tick), (*account, tick));
            assert_eq!(record.codes, vec![stock]);
            let belief = session.state.belief_participants[account].belief();
            let entry = belief.entry(&stock).expect("真实公开报告形成信念");
            assert!(!entry.used_report_ids.is_empty(), "不能用无报告条目冒充获知正控");
            assert!(entry.method.is_some(), "必须真实形成估值方法");
            assert_eq!(record.expectation_method, entry.method.map(|method| format!("{method:?}")));
            for publication in &entry.used_report_ids {
                assert!(record.source_report_ids.contains(&publication.value().to_string()));
                session.state.library.report(*publication, session.chain_observation_instant()).unwrap();
            }
        }
    }
    let before = serde_json::to_vec(&session.save().unwrap()).unwrap();
    let hash = session.business_state_hash().unwrap();
    for account in ids {
        assert_eq!(session.npc_decision_trace(account), session.npc_decision_trace(account));
    }
    assert_eq!(serde_json::to_vec(&session.save().unwrap()).unwrap(), before);
    assert_eq!(session.business_state_hash().unwrap(), hash);
}

#[test]
fn d3_runtime_trace_evicts_only_after_129_real_observations_and_restores_checkpoint() {
    let (mut session, ids) = d3_trace_session(1, false);
    let account = ids[0];
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    let mut checkpoint = None;
    for tick in 1..=129 {
        let events = d3_install_real_candidate(&mut session, &mut driver, &ids);
        assert!(events.iter().all(|event| !matches!(event, Event::OrderAccepted { account: owner, .. } if *owner == account)), "no-action observe 不能伪造受理");
        let records = session.npc_decision_trace(account).expect("每次强制真实 observe 必须留 trace");
        assert_eq!(records.last().unwrap().tick, tick);
        assert_eq!(records.len(), (tick as usize).min(crate::MAX_NPC_DECISION_TRACE_RECORDS));
        if tick == 64 {
            checkpoint = Some(session.clone_for_tick_shadow().unwrap());
        }
    }
    let records = session.npc_decision_trace(account).unwrap();
    assert_eq!(records.iter().map(|record| record.tick).collect::<Vec<_>>(), (2..=129).collect::<Vec<_>>());
    let mut restored = checkpoint.unwrap();
    assert_eq!(restored.npc_decision_trace(account).unwrap().iter().map(|record| record.tick).collect::<Vec<_>>(), (1..=64).collect::<Vec<_>>());
    let mut rebuilt = crate::runtime::drive::RuntimeDriver::from_session(&mut restored).unwrap();
    d3_install_real_candidate(&mut restored, &mut rebuilt, &ids);
    assert_eq!(restored.npc_decision_trace(account).unwrap().last().unwrap().tick, 65);
    assert_eq!(session.npc_decision_trace(account).unwrap().last().unwrap().tick, 129, "跨 session 恢复不能改原 collector");
}

fn d3_assert_late_trace_failure(install_failure: bool) {
    let (mut session, ids) = d3_trace_session(1, true);
    let account = ids[0];
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    driver.force_due_for_test_at(&ids, 1);
    session.runtime_step_with_install(&mut driver).expect("真实 public outer 首拍");
    let prior = session.npc_decision_trace(account).expect("失败负控前必须已有真实非空 trace");
    assert_eq!(prior.len(), 1);
    assert!(!prior[0].source_report_ids.is_empty());
    let checkpoint = session.clone_for_tick_shadow().unwrap();
    let before = session.business_state_hash().unwrap();
    driver.force_due_for_test_at(&ids, 2);
    if install_failure {
        session.inject_runtime_install_failure();
        assert!(session.runtime_step_with_install(&mut driver).is_err());
    } else {
        let mut called = false;
        let result = session.runtime_step_with_prepared_mirror(&mut driver, |_, _| {
            called = true;
            Err::<(), _>(StepFatal::InvariantViolation {
                location: "d3_trace_callback".to_owned(),
                description: "真实观察后的候选拒绝".to_owned(),
            })
        });
        assert!(called, "必须真实到达 callback 晚失败点");
        assert!(result.is_err());
    }
    assert_eq!(session.npc_decision_trace(account).unwrap(), prior);
    let pending = driver.pending_decision_traces();
    assert_eq!(pending.len(), 1, "真实晚失败必须保留本拍未安装 trace");
    assert_eq!((pending[0].record.account, pending[0].record.tick), (account, 2));
    assert_eq!(session.business_state_hash().unwrap(), before);
    let mut retry = checkpoint;
    let mut rebuilt = crate::runtime::drive::RuntimeDriver::from_session(&mut retry).unwrap();
    rebuilt.force_due_for_test_at(&ids, 2);
    retry.runtime_step_with_install(&mut rebuilt).unwrap();
    assert_eq!(retry.npc_decision_trace(account).unwrap().iter().map(|record| record.tick).collect::<Vec<_>>(), vec![1, 2]);
}

#[test]
fn d3_runtime_trace_callback_failure_keeps_prior_and_checkpoint_retry_publishes_once() {
    d3_assert_late_trace_failure(false);
}

#[test]
fn d3_runtime_trace_install_failure_keeps_prior_and_checkpoint_retry_publishes_once() {
    d3_assert_late_trace_failure(true);
}

#[test]
fn d3_runtime_trace_without_new_observation_preserves_prior_and_echo_is_read_only() {
    let (mut session, ids) = d3_trace_session(1, true);
    let account = ids[0];
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    d3_install_real_candidate(&mut session, &mut driver, &ids);
    let prior = session.npc_decision_trace(account).expect("Echo 正控必须有真实非空 trace");
    assert!(!prior[0].source_report_ids.is_empty());
    let hash = session.business_state_hash().unwrap();
    let first = driver.collect_open_derivations().unwrap();
    let second = driver.collect_open_derivations().unwrap();
    assert_eq!(first.len(), second.len());
    for ((group, market, instant, records), (again_group, again_market, again_instant, again_records)) in first.iter().zip(&second) {
        assert_eq!(group, again_group);
        assert_eq!(serde_json::to_value(market).unwrap(), serde_json::to_value(again_market).unwrap(), "Echo 全部 MarketView 字段必须相等");
        assert_eq!(instant, again_instant);
        assert_eq!(records, again_records);
    }
    assert!(driver.pending_decision_traces().is_empty(), "Echo 不消费第四 diagnostics 或追加 pending");
    assert_eq!(session.npc_decision_trace(account).unwrap(), prior);
    assert_eq!(session.business_state_hash().unwrap(), hash);
    driver.defer_due_for_test_at(&ids, 1_000);
    d3_install_real_candidate(&mut session, &mut driver, &[]);
    assert_eq!(session.tick(), 2);
    assert_eq!(session.npc_decision_trace(account).unwrap(), prior, "无新 observe 不沿旧 trace 写新 tick");
}

#[test]
fn d3_runtime_trace_day_end_compares_the_final_expired_plan() {
    let (mut session, ids) = d3_trace_session_with_day(1, false, 2);
    let account = ids[0];
    let stock = session.stock_id("000812").unwrap();
    let plan = open_account_latch_plan(&mut session, account, stock, Side::Buy);
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    for tick in 1..=2 {
        driver.force_due_for_test_at(&ids, tick);
        let events = session.runtime_step_with_install(&mut driver).expect("真实无余单日结路径受支持");
        assert!(events.iter().all(|event| !matches!(event, Event::OrderAccepted { account: owner, .. } if *owner == account)), "不足一手预算不得伪造受理");
        let records = session.npc_decision_trace(account).expect("日结正控必须有真实 observe trace");
        assert_eq!(records.last().unwrap().tick, tick);
        assert!(records.last().unwrap().plan_ids.contains(&plan));
    }
    assert_eq!(session.day(), 1);
    let final_plan = session.state.plans.plan(plan).unwrap();
    assert!(matches!(final_plan.status(), crate::plans::PlanStatus::Terminated { reason: crate::plans::TerminationReason::HorizonExpired }), "真实日结必须先结束一日 horizon 计划");
    let final_state = plan_trace_state(final_plan);
    let records = session.npc_decision_trace(account).unwrap();
    assert!(records.last().unwrap().plan_changes.iter().any(|change| change.starts_with(&format!("{plan:?}: ")) && change.ends_with(&format!(" -> {final_state}"))), "trace 必须比较日结后最终 HorizonExpired 计划状态");
}
