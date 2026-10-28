// D3 后片 tests-first：真实 Retail/Hot 观察与自动来源；尚未编译或取得本批首红。

fn d3_other_npc_decision(facts: &[crate::diagnostics::causal::CausalFact], account: AccountId) -> u64 {
    let all_decisions: Vec<_> = facts.iter().filter_map(|fact| match fact.kind {
        crate::diagnostics::causal::CausalFactKind::Decision { account: owner } => Some(owner), _ => None,
    }).collect();
    assert_eq!(all_decisions, vec![account], "单 NPC fixture 当批只能给实际观察者发一个 Decision，不能向玩家或未观察者补根");
    let decisions: Vec<_> = facts.iter().filter_map(|fact| match fact.kind {
        crate::diagnostics::causal::CausalFactKind::Decision { account: owner } if owner == account => Some(fact.sequence),
        _ => None,
    }).collect();
    assert_eq!(decisions.len(), 1, "本次真实 NPC observe 必须恰有一个 Decision，不能靠 Map 去重");
    decisions[0]
}

fn d3_other_npc_error_requires_real_submit(
    driver: &crate::runtime::drive::RuntimeDriver,
    account: AccountId,
    kind: AccountKind,
    session: &GameSession,
) {
    assert_eq!(session.account(account).unwrap().kind(), kind);
    let (_, raw, _) = driver.terminal_causal_snapshot_for_test().expect("来源失败必须保留 terminal 原事实");
    assert!(raw.iter().any(|fact| matches!(fact.kind,
        crate::runtime::diagnostics::DeskCausalKind::Submitted { owner, qty, .. }
            if qty > 0 && driver.account_by_owner(owner) == Some(account))),
        "来源红必须先有该真实 NPC 的非空自动 Submitted，不能把 fixture 或空 quote 当业务红");
}

fn d3_other_npc_player_book(driver: &mut crate::runtime::drive::RuntimeDriver) {
    let view = &driver.world_public_views()[0];
    let upper = view.daily_upper_limit;
    let intents = [(Side::Buy, view.last_price - 1), (Side::Sell, view.last_price + 1)].map(|(side, price)|
        crate::session::ReceiptBearingIntent {
            owner: AccountId(0), account_ordinal: 0, stock_ordinal: 0,
            intent: Intent::PlaceLimit { code: StockId(0), side, price: crate::LimitPrice::Fixed(Money::from_cents(price)), qty: 100 },
        });
    driver.inject_player_intents(&intents, &[upper]).unwrap();
}

#[test]
fn d3_retail_real_auto_submit_uses_current_observation_without_fake_plan_or_budget() {
    use crate::diagnostics::causal::CausalFactKind;
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.strategy_params.retail.arrival_rate = 1.0;
    setup.strategy_params.retail.order_size_mean = 100;
    setup.stocks[0].float_shares = 1_000;
    setup.float_allocation = FloatAllocation::class_percentages(1.0, 0.0, 0.0, crate::session::WithinKindDistribution::EqualPercentage);
    setup.ticks_per_day = 240;
    let mut session = GameSession::new(setup, 5).unwrap();
    let account = AccountId(1);
    let stock = StockId(0);
    assert_eq!(session.account(account).unwrap().kind(), AccountKind::Retail);
    // 开局把既有股份给真 Player0 铺簿；保总数，不把 NPC 注入冒作玩家来源。
    let holding = session.fixture_account_mut(&account).unwrap().fixture_remove_position(&stock).unwrap();
    let total = holding.qty();
    assert_eq!(total, 1_000);
    session.fixture_account_mut(&AccountId(0)).unwrap().fixture_insert_position(stock, holding);
    session.state.retail_experience.insert(account, crate::RetailExperienceState::without_equity_reference());
    assert!(session.account(account).unwrap().positions().is_empty());
    assert_eq!(session.account(AccountId(0)).unwrap().sellable_qty(&stock), total);
    session.save().expect("Retail 自动报价 fixture 初始存档合法");
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    let mut submitted = 0;
    for tick in 1..=4 {
        d3_other_npc_player_book(&mut driver);
        driver.force_due_for_test_at(&[account], tick);
        let prior = session.causal_facts().len();
        let result = session.runtime_step_with_install(&mut driver);
        if result.is_err() {
            d3_other_npc_error_requires_real_submit(&driver, account, AccountKind::Retail, &session);
        }
        result.expect("真实 Retail 自动 Submitted 的 typed 来源必须接通");
        let facts = &session.causal_facts()[prior..];
        let origins: Vec<_> = facts.iter().filter_map(|fact| match &fact.kind {
            CausalFactKind::Submitted(origin) if origin.account == account => Some((fact.sequence, origin)), _ => None,
        }).collect();
        if !origins.is_empty() {
            let decision = d3_other_npc_decision(facts, account);
            for (sequence, origin) in origins {
                submitted += 1;
                assert_eq!(origin.decision, Some(decision), "不能继承 Retail 的旧 Decision 或补无关联来源");
                assert!(decision < sequence);
                assert_eq!(origin.plan, None, "Retail 自动意图没有机构 PlanId，不能伪造");
            }
        }
        assert!(!facts.iter().any(|fact| matches!(fact.kind, CausalFactKind::Budget { .. })),
            "Retail 未调用 AllocationResult，不能用空 Budget 冒充来源");
    }
    assert!(submitted > 0, "不能以 Retail 空报价通过来源门");
    session.save().expect("Retail 自动报价成功后存档合法");
}

#[test]
fn d3_retail_noquote_observation_keeps_real_acquisition_and_defer_has_no_root() {
    use crate::diagnostics::causal::CausalFactKind;
    let mut setup = probe_session().state.setup.clone();
    setup.npcs.inst_count = 0;
    setup.npcs.retail_count = 1;
    setup.strategy_params.retail.arrival_rate = 0.0;
    setup.stocks[0].float_shares = 0;
    setup.ticks_per_day = 240;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.company_system = Some(simple_company_fixture!(crate; ["000812"]));
    setup.dividend_tax_mode = Some(crate::company::cash_dividend_tax::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1_000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    let mut session = GameSession::new(setup, 42).unwrap();
    let ids = prepare_company_source_personal_inputs(&mut session, true);
    assert_eq!(ids, vec![AccountId(1)]);
    let account = ids[0];
    let stock = session.stock_id("000812").unwrap();
    let company = session.state.issuer_of_stock(stock).unwrap();
    assert_eq!(session.account(account).unwrap().kind(), AccountKind::Retail);
    let through = session.chain_observation_instant();
    let reports = session.state.library.reports_for_company(&company, through);
    assert!(!reports.is_empty());
    assert!(reports.iter().all(|report| report.company == company
        && report.source == crate::information::PublicationSource::SimpleGenerated && report.published_at <= through));
    assert!(reports.iter().any(|report| report.reports.kind == crate::accounting::reports::ReportKind::Annual),
        "Retail 获知正控必须使用所选 Simple 自己的已公开 Annual");
    let probability = session.account(account).unwrap().strategy().unwrap().base_observation_probability();
    session.state.npc_attention.get_mut(&account).unwrap().base_probability = probability;
    session.fixture_account_mut(&account).unwrap().fixture_set_cash(Money::from_cents(1));
    session.state.retail_experience.insert(account, crate::RetailExperienceState::without_equity_reference());
    assert!(session.account(account).unwrap().positions().is_empty());
    assert_eq!(session.state.belief_participants[&account].information().acquired_count(), 0);
    session.save().expect("Retail noquote fixture 必须通过真实存档校验");
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    for tick in 1..=2 {
        driver.force_due_for_test_at(&[account], tick);
        let prior = session.causal_facts().len();
        let events = session.runtime_step_with_install(&mut driver).expect("真实 Retail noquote observe 必须 healthy");
        assert!(!events.iter().any(|event| matches!(event, Event::OrderAccepted { account: owner, .. } if *owner == account)));
        assert_eq!(session.last_retail_decisions().len(), 1, "先有真实 Retail 决策投影，才可判断根事实丢失");
        assert_eq!(session.last_retail_decisions()[0].account, account);
        let personal = &session.state.belief_participants[&account];
        assert!(personal.information().acquired_count() > 0, "先有本人真实已获知材料，不能以空材料当获知正控");
        let facts = &session.causal_facts()[prior..];
        let decision = d3_other_npc_decision(facts, account);
        let mut acquired = 0;
        for fact in facts {
            if let CausalFactKind::Acquisition { account: owner, company, publication, published, acquired: observed } = &fact.kind {
                acquired += 1;
                assert_eq!(*owner, account);
                assert!(decision < fact.sequence);
                let known = personal.information().records_for_company(company).iter()
                    .find(|record| u64::from(record.id.value()) == *publication).unwrap();
                assert_eq!(*observed, known.observed_at);
                let id = PublicationId::new(u32::try_from(*publication).unwrap());
                let actual = session.state.library.report(id, *observed).map(|report| report.published_at)
                    .or_else(|_| session.state.library.announcement(id, *observed).map(|notice| notice.published_at)).unwrap();
                assert_eq!(*published, actual);
            }
        }
        if tick == 1 { assert!(acquired > 0, "首次 Retail 获知必须有非空 Acquisition"); }
        else { assert_eq!(acquired, 0, "已有材料不能逐拍重记获知"); }
        assert!(!facts.iter().any(|fact| matches!(fact.kind, CausalFactKind::Budget { .. })), "无真实 Allocation 不造 Budget");
    }
    driver.defer_due_for_test_at(&[account], 1_000);
    let prior = session.causal_facts().len();
    session.runtime_step_with_install(&mut driver).unwrap();
    assert!(!session.causal_facts()[prior..].iter().any(|fact| matches!(fact.kind,
        CausalFactKind::Decision { .. } | CausalFactKind::Acquisition { .. } | CausalFactKind::Budget { .. })),
        "Retail 没有本次 observe 时不补根或沿旧 Decision 发号");
}

fn d3_hot_legal_negative_minute_fixture() -> (GameSession, AccountId) {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 1;
    setup.stocks[0].float_shares = 100;
    setup.float_allocation = FloatAllocation::class_percentages(0.0, 0.0, 1.0, crate::session::WithinKindDistribution::EqualPercentage);
    setup.strategy_params.hot.order_size = 100;
    setup.strategy_params.hot.lookback = 3;
    setup.strategy_params.hot.trend_threshold = 0.001;
    setup.ticks_per_day = 240;
    let mut session = GameSession::new(setup, 42).unwrap();
    let account = AccountId(1);
    assert_eq!(session.account(account).unwrap().kind(), AccountKind::Hot);
    assert_eq!(session.account(account).unwrap().strategy().unwrap().production_state().unwrap().profile(),
        crate::strategy::StrategyProfile::Hot(crate::strategy::HotStyle::Momentum));
    assert_eq!(session.account(account).unwrap().sellable_qty(&StockId(0)), 100);
    let strategy = crate::runtime::drive::RuntimeDriver::individual_strategy_data(session.account(account).unwrap().strategy().unwrap()).unwrap();
    assert_eq!(strategy.order_size, 100);
    assert!(strategy.trend_threshold < 0.10);
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    driver.defer_due_for_test_at(&[account], 1_000);
    for _ in 0..2 { session.runtime_step_with_install(&mut driver).expect("未观察 Hot 的两个真实静默分钟"); }
    let mut saved = session.save().expect("编辑前真实静点存档合法");
    drop(driver);
    assert_eq!(saved.snapshot.tick, 2);
    assert_eq!(saved.market_minute_closes[&StockId(0)].len(), 2);
    // 只在合法可编辑存档中设置已完成分钟趋势，不伪造 live TickSnapshot 或成交记录。
    saved.price_history.insert(StockId(0), vec![Money::from_cents(1_000), Money::from_cents(900)]);
    saved.market_minute_closes.get_mut(&StockId(0)).unwrap()[1].close = Money::from_cents(900);
    saved.snapshot.markets.get_mut(&StockId(0)).unwrap().last_price = Money::from_cents(900);
    let active = saved.snapshot.active_daily_candles.get_mut(&StockId(0)).unwrap();
    active.close = Money::from_cents(900);
    active.low = Money::from_cents(900);
    let restored = GameSession::restore(&saved).expect("Hot 两分钟历史必须先经真实 restore 校验");
    let again = restored.save().expect("Hot restore 后再 save 必须合法");
    assert_eq!(again.market_minute_closes[&StockId(0)].iter().map(|minute| (minute.absolute_trading_minute, minute.close.cents())).collect::<Vec<_>>(), vec![(0, 1_000), (1, 900)]);
    let seed = restored.runtime_market_seeds().remove(&StockId(0)).unwrap();
    assert_eq!(seed.minute_closes, vec![(0, 1_000), (1, 900)]);
    assert_eq!(seed.last_price, 900);
    assert_eq!(restored.current_market_minute(), 2);
    (restored, account)
}

#[test]
fn d3_hot_real_auto_submit_and_noquote_observation_keep_current_decision() {
    use crate::diagnostics::causal::CausalFactKind;
    let (mut session, account) = d3_hot_legal_negative_minute_fixture();
    let stock = StockId(0);
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session).unwrap();
    // 构造期公开视图不携 seed 的分钟历史；先走一次真实 report 发布，禁止改 live snapshot。
    driver.defer_due_for_test_at(&[account], 1_000);
    session.runtime_step_with_install(&mut driver).expect("恢复后无 observe 同步拍必须真实发布 seed 分钟");
    assert_eq!(session.tick(), 3);
    assert_eq!(driver.world_public_views()[0].minute_closes, vec![(0, 1_000), (1, 900), (2, 900)]);
    assert_eq!(driver.world_public_views()[0].last_price, 900);
    assert_eq!(session.account(account).unwrap().sellable_qty(&stock), 100);
    let buy = crate::session::ReceiptBearingIntent {
        owner: AccountId(0), account_ordinal: 0, stock_ordinal: 0,
        intent: Intent::PlaceLimit { code: stock, side: Side::Buy, price: crate::LimitPrice::Fixed(Money::from_cents(900)), qty: 100 },
    };
    let upper = driver.world_public_views()[0].daily_upper_limit;
    driver.inject_player_intents(&[buy], &[upper]).unwrap();
    driver.force_due_for_test_at(&[account], 4);
    let prior = session.causal_facts().len();
    let result = session.runtime_step_with_install(&mut driver);
    if result.is_err() { d3_other_npc_error_requires_real_submit(&driver, account, AccountKind::Hot, &session); }
    result.expect("真实 Hot 自动 Submitted 的 typed 来源必须接通");
    let facts = &session.causal_facts()[prior..];
    let decision = d3_other_npc_decision(facts, account);
    let origins: Vec<_> = facts.iter().filter_map(|fact| match &fact.kind {
        CausalFactKind::Submitted(origin) if origin.account == account => Some((fact.sequence, origin)), _ => None,
    }).collect();
    assert!(!origins.is_empty(), "Hot 真实负趋势必须触发非空自动卖单");
    for (sequence, origin) in origins {
        assert_eq!((origin.side, origin.qty), (Side::Sell, 100));
        assert_eq!(origin.plan, None, "Hot 意图没有机构 PlanId，不能补造");
        assert_eq!(origin.decision, Some(decision));
        assert!(decision < sequence);
    }
    assert_eq!(session.account(account).unwrap().sellable_qty(&stock), 0, "真实售出唯一整手后才进入 noquote 正控");
    assert!(!facts.iter().any(|fact| matches!(fact.kind, CausalFactKind::Acquisition { .. } | CausalFactKind::Budget { .. })));
    driver.force_due_for_test_at(&[account], 5);
    let prior = session.causal_facts().len();
    session.runtime_step_with_install(&mut driver).unwrap();
    let facts = &session.causal_facts()[prior..];
    assert!(!facts.iter().any(|fact| matches!(&fact.kind, CausalFactKind::Submitted(origin) if origin.account == account)), "Hot 无可卖股份，真实第二次 observe 不得补单");
    d3_other_npc_decision(facts, account);
    assert!(!facts.iter().any(|fact| matches!(fact.kind, CausalFactKind::Acquisition { .. } | CausalFactKind::Budget { .. })),
        "Hot 第二次无报价观察同样没有信息或 AllocationResult，不能补造 Acquisition/Budget");
    driver.defer_due_for_test_at(&[account], 1_000);
    let prior = session.causal_facts().len();
    session.runtime_step_with_install(&mut driver).unwrap();
    assert!(!session.causal_facts()[prior..].iter().any(|fact| matches!(fact.kind,
        CausalFactKind::Decision { .. } | CausalFactKind::Acquisition { .. } | CausalFactKind::Budget { .. })), "Hot defer 不沿旧 Decision 补根");
    session.save().expect("Hot 两次真实 observe 与 defer 后存档合法");
}
