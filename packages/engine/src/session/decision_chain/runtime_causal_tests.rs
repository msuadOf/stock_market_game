// D3 后片 tests-first：真实根事实与自动子单归因；尚未执行，不冒称有效首红。
// fixture 沿已实际绿的 d3b3_paused_plan_with_child_routes_targeted_cancel。

fn d3_causal_quote_setup() -> SessionSetup {
    let stocks = vec![
        StockSpec {
            // Sol 终审四审 A4 配套修订：600000→000001（深主板——代码
            // 序 000001<300001 与输入序一致，StockCatalog 按代码序分配
            // StockId 的 ADR-0041 约定下 id0/id1 语义与原 fixture 完全
            // 一致：id0=10 元主板、id1=20 元创业板 20%）；主板
            // 语义（10%/100 万限价）不变，引用 StockId(0) 的测试无需适配。
            code: StockCode("000001".to_owned()),
            exchange: crate::StockExchange::Shenzhen,
            initial_price: Money::from_cents(1000),
            category: crate::SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 1_000_000,
            float_shares: 800_000,
        },
        StockSpec {
            code: StockCode("300001".to_owned()),
            exchange: crate::StockExchange::Shenzhen,
            initial_price: Money::from_cents(2000),
            category: crate::SecurityCategory::ChiNext,
            limit_pct: 0.20,
            tick: Money::from_cents(1),
            total_shares: 500_000,
            float_shares: 400_000,
        },
    ];
    SessionSetup {
        company_system: None,
        dividend_tax_mode: None,
        flat_withholding_bp: None,
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
        par_value_per_share: None,
        auto_corporate_foundation: false,
        report_frequency: None,
        stocks,
        company_operations: None,
        groups: Vec::new(),
        npcs: crate::session::NpcSetup {
            retail_count: 2,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::from_cents(1_000_000),
        },
        config: crate::config::GameConfig::proposed_defaults(),
        strategy_params: crate::strategy::StrategyParams {
            retail: crate::strategy::RetailParams {
                arrival_rate: 0.0,
                order_size_mean: 100,
                chase_prob: 0.0,
            },
            inst: crate::strategy::InstParams {
                margin: 0.01,
                order_size: 1,
            },
            hot: crate::strategy::HotParams {
                lookback: 2,
                trend_threshold: 0.01,
                order_size: 1,
            },
        },
        ticks_per_day: 240,
        auction_ticks: 15,
        closing_auction_ticks: 3,
        history_len: 16,
        t1_enabled: true,
        float_allocation: crate::session::FloatAllocation::random(),
        start_date: crate::calendar::CivilDate::from_ymd(2026, 1, 5).expect("测试日期合法"),
        simulation_policy_id: crate::session::SIMULATION_POLICY_ID.to_owned(),
    }
}

fn d3_causal_quote_fixture() -> (GameSession, crate::runtime::drive::RuntimeDriver, Vec<AccountId>) {
    let mut setup = d3_causal_quote_setup();
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 3;
    setup.strategy_params.inst.order_size = 1_000;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.ticks_per_day = 1_000;
    let mut session = GameSession::new(setup, 51).unwrap();
    // 只在开局合法 fixture 内转移既有股份；外部铺簿来源是真玩家，不能把 NPC 注入标成 PlayerExternal。
    let retail = AccountId(1);
    let player = AccountId(0);
    assert!(session.account(player).unwrap().positions().is_empty());
    let stocks: Vec<_> = session.state.markets.keys().copied().collect();
    assert_eq!(stocks.len(), 2);
    let totals: BTreeMap<_, u64> = stocks.iter().map(|stock| (*stock, session.state.accounts.values()
        .map(|account| u64::from(account.positions().get(stock).map_or(0, |holding| holding.qty()))).sum())).collect();
    for stock in &stocks {
        let holding = session.fixture_account_mut(&retail).unwrap().fixture_remove_position(stock).unwrap();
        session.fixture_account_mut(&player).unwrap().fixture_insert_position(*stock, holding);
    }
    for stock in &stocks {
        assert!(session.account(player).unwrap().positions()[stock].sellable() >= 200, "两股铺簿均须有真实可卖股");
        let total: u64 = session.state.accounts.values().map(|account|
            u64::from(account.positions().get(stock).map_or(0, |holding| holding.qty()))).sum();
        assert_eq!(total, totals[stock], "开局转移既有股份必须保持总股数");
    }
    // 开局编辑后散户未持仓；重置无成交历史的初始持仓经历，不伪造退出成交。
    session.state.retail_experience.insert(retail, crate::RetailExperienceState::without_equity_reference());
    session.save().expect("真实 quote fixture 初始存档合法");
    let institutions = session.state.accounts.iter()
        .filter_map(|(id, account)| (account.kind() == AccountKind::Inst).then_some(*id)).collect();
    let driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    (session, driver, institutions)
}

fn d3_seed_real_external_book(driver: &mut crate::runtime::drive::RuntimeDriver) {
    // 真玩家外部意图沿真实 route_external_intents 受理；机构自动产物不走此来源。
    let views = driver.world_public_views().to_vec();
    let mut intents = Vec::new();
    for (stock, view) in views.iter().enumerate() {
        for (side, price) in [(Side::Buy, view.last_price + 1), (Side::Sell, view.last_price - 1)] {
            intents.push(crate::session::ReceiptBearingIntent {
                owner: AccountId(0),
                intent: crate::Intent::PlaceLimit {
                    code: StockId(stock as u32), side,
                    price: crate::strategy::LimitPrice::Fixed(Money::from_cents(price)), qty: 200,
                },
                account_ordinal: 0, stock_ordinal: 0,
            });
        }
    }
    let upper: Vec<_> = views.iter().map(|view| view.daily_upper_limit).collect();
    driver.inject_player_intents(&intents, &upper).unwrap();
}

#[test]
fn d3_causal_real_institution_submit_resolves_full_plan_and_current_decision() {
    use crate::diagnostics::causal::CausalFactKind;
    let (mut session, mut driver, institutions) = d3_causal_quote_fixture();
    let mut automatic_origins = 0;
    for tick in 1..=6 {
        d3_seed_real_external_book(&mut driver);
        driver.force_due_for_test_at(&institutions, tick);
        let prior_count = session.causal_facts().len();
        let result = session.runtime_step_with_install(&mut driver);
        if result.is_err() {
            let (_, raw, _) = driver.terminal_causal_snapshot_for_test()
                .expect("真实 quote 诊断失败必须保留 terminal 原事实");
            assert!(raw.iter().any(|fact| matches!(fact.kind,
                crate::runtime::diagnostics::DeskCausalKind::Submitted { owner, qty, .. }
                    if qty > 0 && institutions.contains(&driver.account_by_owner(owner).unwrap()))),
                "首红必须先有真实机构自动 Submitted，不能是配置或无 quote 前置失败");
        }
        result.expect("真实机构自动 quote 的 typed causal 来源必须接通");
        let facts = session.causal_facts();
        let new_facts = &facts[prior_count..];
        let decisions: BTreeMap<_, _> = new_facts.iter().filter_map(|fact| match fact.kind {
            CausalFactKind::Decision { account } if institutions.contains(&account) => Some((account, fact.sequence)), _ => None,
        }).collect();
        if new_facts.iter().any(|fact| matches!(&fact.kind, CausalFactKind::Submitted(origin) if institutions.contains(&origin.account))) {
            let count = new_facts.iter().filter(|fact| matches!(fact.kind,
                CausalFactKind::Decision { account } if institutions.contains(&account))).count();
            assert_eq!(count, institutions.len(), "同一次真实 observe 只能产生一个 Decision，不能被 Map 去重掩盖");
            assert_eq!(decisions.keys().copied().collect::<BTreeSet<_>>(), institutions.iter().copied().collect(),
                "每个强制真实 observe 必须有当次 Decision，不能只给有 quote 的账户补根");
        }
        for fact in new_facts {
            let CausalFactKind::Submitted(origin) = &fact.kind else { continue; };
            if !institutions.contains(&origin.account) { continue; }
            automatic_origins += 1;
            let plan_id = origin.plan.expect("机构自动子单必须有完整 PlanId");
            assert_eq!(plan_id.owner, origin.account);
            let plan = session.state.plans.plan(plan_id).unwrap();
            assert_eq!((plan.code(), plan.direction()), (&origin.code, origin.side));
            let sequence = origin.decision.expect("真实本次 observe 的子单必须关联 Decision");
            assert_eq!(Some(&sequence), decisions.get(&origin.account), "不得继承本账户旧 Decision");
            let decision = facts.iter().find(|prior| prior.sequence == sequence).unwrap();
            assert!(decision.sequence < fact.sequence);
            assert!(matches!(decision.kind, CausalFactKind::Decision { account } if account == origin.account));
            assert_eq!(decision.time.civil.date(), fact.time.civil.date());
        }
    }
    assert!(automatic_origins > 0, "不能以空机构报价通过归因门禁");
    assert!(session.causal_facts().iter().any(|fact| matches!(&fact.kind,
        CausalFactKind::Budget { account, available_cents, allocated_cents }
            if institutions.contains(account) && *available_cents > 0 && allocated_cents.iter().any(|cents| *cents > 0))),
        "真实 quote fixture 必须发布 AllocationResult 的非空 Budget");
    session.save().expect("真实 quote 成功后存档合法");
}

#[test]
fn d3_causal_noquote_observation_preserves_real_root_facts_without_empty_budget() {
    use crate::diagnostics::causal::CausalFactKind;
    let (mut session, ids) = d3_trace_session(2, true);
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    for tick in 1..=2 {
        let prior_count = session.causal_facts().len();
        driver.force_due_for_test_at(&ids, tick);
        let events = session.runtime_step_with_install(&mut driver).expect("真实无报价 observe 保持 healthy");
        assert!(!events.iter().any(|event| matches!(event, Event::OrderAccepted { account, .. } if ids.contains(account))));
        for account in &ids {
            let trace = session.npc_decision_trace(*account).expect("根事实正控已有真实 trace");
            assert_eq!(trace.last().unwrap().tick, tick);
            assert!(!trace.last().unwrap().source_report_ids.is_empty());
        }
        let facts = session.causal_facts();
        let new_facts = &facts[prior_count..];
        let decisions: BTreeMap<_, _> = new_facts.iter().filter_map(|fact| match fact.kind {
            CausalFactKind::Decision { account } => Some((account, fact.sequence)), _ => None,
        }).collect();
        let count = new_facts.iter().filter(|fact| matches!(fact.kind, CausalFactKind::Decision { .. })).count();
        assert_eq!(count, ids.len(), "当次真实 observe 的 PlanRootDiagnostics::facts 不能被 trace 接线丢弃或重复");
        assert_eq!(decisions.keys().copied().collect::<BTreeSet<_>>(), ids.iter().copied().collect(),
            "当次真实 observe 的 PlanRootDiagnostics::facts 不能被 trace 接线丢弃");
        assert!(!new_facts.iter().any(|fact| matches!(fact.kind, CausalFactKind::Budget { .. })),
            "无请求的真实 observe 不造空 Budget");
        let mut acquisitions = 0;
        for fact in new_facts {
            let CausalFactKind::Acquisition { account, company, publication, published, acquired } = &fact.kind else { continue; };
            acquisitions += 1;
            assert!(decisions[account] < fact.sequence, "保真实 Decision → Acquisition 原序");
            let personal = &session.state.belief_participants[account];
            let known = personal.information().records_for_company(company).iter()
                .find(|record| u64::from(record.id.value()) == *publication).unwrap();
            assert_eq!(*acquired, known.observed_at);
            let publication_id = PublicationId::new(u32::try_from(*publication).unwrap());
            let actual_published = session.state.library.report(publication_id, *acquired)
                .map(|report| report.published_at)
                .or_else(|_| session.state.library.announcement(publication_id, *acquired).map(|announcement| announcement.published_at)).unwrap();
            assert_eq!(*published, actual_published);
        }
        if tick == 1 {
            assert!(acquisitions > 0, "首次真实获知必须有非空 Acquisition 正控");
        } else {
            assert_eq!(acquisitions, 0, "没有新公开材料的第二拍不能重记已有获知");
        }
    }
    let prior_count = session.causal_facts().len();
    driver.defer_due_for_test_at(&ids, 1_000);
    session.runtime_step_with_install(&mut driver).unwrap();
    assert!(!session.causal_facts()[prior_count..].iter().any(|fact| matches!(fact.kind,
        CausalFactKind::Decision { .. } | CausalFactKind::Acquisition { .. })),
        "无本次 observe 不增发 Decision 或 Acquisition，也不能沿旧 account decision 补关联");
}
