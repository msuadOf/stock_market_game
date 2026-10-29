//! ordinary REG 与个人税日的真实 civil 入口；不以领域候选代替生产提交。

#[allow(dead_code)]
#[path = "publications/session_fixture.rs"]
mod session_fixture;
include!("../test-support/simple_company.rs");

use engine::accounting::AccountingAmount;
use engine::calendar::{CalendarExchange, CalendarPolicy, TradingCalendar};
use engine::company::cash_dividend::{CashDividendBook, CashDividendPlan, CashDividendStatus};
use engine::company::cash_dividend_tax::CashDividendTaxMode;
use engine::company::config::CompanySystemConfig;
use engine::company::ex_reference_price::CashDividendFormula;
use engine::company::share_registry::settlement_day::{
    ShareMarketSource, SharePostActionRef, ShareUnitIdentity,
};
use engine::company::share_registry::MovementScope;
use engine::company::{CompanyId, DividendDeclaration};
use engine::session::{
    SavedSessionCompanies, SessionCorporateActions, SessionError, WithinKindDistribution,
};
use engine::{CivilDate, FloatAllocation, GameSession, Money, SaveSlot};

fn date(text: &str) -> CivilDate {
    CivilDate::from_iso(text).expect("夹具日期合法")
}

fn game(start: &str) -> GameSession {
    game_with_history(start, 2)
}

fn game_with_history(start: &str, prehistory_periods: u16) -> GameSession {
    let mut setup = session_fixture::civil_setup(date(start));
    // 7 个真实 owner 分到 3 组，尾组只有 1 个真实 owner；不为 padding 补个人态。
    setup.npcs.retail_count = 6;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.strategy_params.retail.arrival_rate = 0.0;
    setup.stocks[0].total_shares = 1_200;
    setup.stocks[0].float_shares = 1_200;
    setup.float_allocation = FloatAllocation::class_percentages(
        100.0,
        0.0,
        0.0,
        WithinKindDistribution::EqualPercentage,
    );
    setup.ticks_per_day = 1;
    setup.history_len = 1;
    let mut companies = simple_company_fixture!(engine; ["600101"]);
    let CompanySystemConfig::Simple(simple) = &mut companies else {
        unreachable!("夹具显式选择 Simple")
    };
    simple.prehistory_periods = prehistory_periods;
    setup.company_system = Some(companies);
    setup.dividend_tax_mode = Some(CashDividendTaxMode::AShareIndividual);
    setup.flat_withholding_bp = None;
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup.auto_corporate_foundation = true;
    let game = GameSession::new_with_runtime_groups(setup, 7, 3)
        .expect("公开 factory 应接受真实自动装配夹具");
    let actions = game.corporate_actions().expect("Simple 公司域在场");
    assert_eq!(actions.registries.len(), 1);
    assert_eq!(actions.dividend_tax_books.len(), 6);
    assert!(actions.registries[0].receipts().is_empty());
    for book in &actions.dividend_tax_books {
        assert!(
            !book.lots().is_empty(),
            "自动筹码分配产生真实个人 opening lots"
        );
    }
    game
}

fn assert_ordinary_days(
    game: &GameSession,
    opening: &SessionCorporateActions,
    days: &[CivilDate],
    captures: &[&str],
) -> SaveSlot {
    let saved = game.save().expect("成功 civil 日结后应可保存");
    assert_eq!(saved.civil_clock.settled_through, days.last().copied());
    assert!(saved
        .personal_trade_confirmations
        .values()
        .all(Vec::is_empty));
    let calendar = TradingCalendar::from_policy(
        CalendarPolicy::from_parts(saved.civil_clock.policy.clone()).unwrap(),
    )
    .unwrap();
    let actions = game.corporate_actions().unwrap();
    let registry_keys = |actions: &SessionCorporateActions| {
        let mut keys = actions
            .registries
            .iter()
            .map(|registry| (registry.stock().clone(), *registry.issuer()))
            .collect::<Vec<_>>();
        keys.sort();
        keys
    };
    assert_eq!(registry_keys(actions), registry_keys(opening));
    let book_keys = |actions: &SessionCorporateActions| {
        let mut keys = actions
            .dividend_tax_books
            .iter()
            .map(|book| (book.account(), book.stock().clone()))
            .collect::<Vec<_>>();
        keys.sort();
        keys
    };
    assert_eq!(
        book_keys(actions),
        book_keys(opening),
        "不能丢失、重复或替换个人税账"
    );
    assert_eq!(
        actions
            .dividends
            .iter()
            .map(|book| &book.plan().plan_id)
            .collect::<Vec<_>>(),
        opening
            .dividends
            .iter()
            .map(|book| &book.plan().plan_id)
            .collect::<Vec<_>>()
    );
    let registry = &actions.registries[0];
    let initial = &opening.registries[0];
    assert_eq!(
        registry.settled_on(),
        *days.last().unwrap(),
        "ordinary REG 必须由真实 civil 事务推进，不能仍停在开局前日"
    );
    assert_eq!(registry.holdings(), initial.holdings());
    assert_eq!(registry.receipts().len(), days.len());
    for (index, (receipt, day)) in registry.receipts().iter().zip(days).enumerate() {
        assert_eq!(receipt.request.day, *day);
        assert!(receipt
            .request
            .changes
            .iter()
            .all(|change| change.change == 0));
        let MovementScope::SettlementDayMarket { binding } = &receipt.request.scope else {
            panic!("普通生产日结必须绑定完整市场来源，不能以旧 PublicMarket marker 冒充")
        };
        assert_eq!(binding.predecessor.settled_on, day.prev().unwrap());
        assert_eq!(binding.predecessor.receipt_count, index as u64);
        assert_eq!(binding.predecessor.unit, ShareUnitIdentity::Initial);
        assert_eq!(binding.market_unit, ShareUnitIdentity::Initial);
        assert!(binding.opening.is_empty());
        assert_eq!(
            binding.post,
            captures
                .iter()
                .map(|event_id| SharePostActionRef::CaptureRegistration {
                    event_id: (*event_id).into(),
                })
                .collect::<Vec<_>>()
        );
        if calendar
            .is_trading_day(CalendarExchange::Sse, *day)
            .unwrap()
        {
            let ShareMarketSource::FinalConfirmations { receipts } = &binding.market_source else {
                panic!("交易日必须绑定实际最终确认全集")
            };
            assert!(receipts.is_empty(), "无委托夹具的真实最终确认全集为空");
        } else {
            let ShareMarketSource::CalendarClosed {
                exchange,
                policy_digest,
            } = &binding.market_source
            else {
                panic!("休市日必须使用冻结日历来源")
            };
            assert_eq!(*exchange, CalendarExchange::Sse);
            assert_eq!(policy_digest, calendar.policy().content_digest());
        }
    }
    for book in &actions.dividend_tax_books {
        let before = opening
            .dividend_tax_books
            .iter()
            .find(|before| before.account() == book.account() && before.stock() == book.stock())
            .expect("税账必须保持真实 account/stock 归属");
        assert_eq!(book.settled_on(), *days.last().unwrap());
        assert_eq!(book.lots(), before.lots());
        assert!(book.collections().is_empty());
        // 通过真实存档契约核逐自然日零事实，不开放税账内部可变字段。
        let encoded = serde_json::to_value(book).unwrap();
        let receipts = encoded["days"].as_array().expect("税账存档含完整 days");
        assert_eq!(receipts.len(), days.len());
        for (receipt, day) in receipts.iter().zip(days) {
            assert_eq!(receipt["day"], serde_json::to_value(day).unwrap());
            assert_eq!(receipt["net_change"], "0");
            assert_eq!(receipt["acquisition"], serde_json::Value::Null);
            assert_eq!(receipt["dispositions"], serde_json::json!([]));
        }
    }
    saved
}

#[test]
fn r4_tax_01_closed_civil_day_commits_ordinary_registry_and_tax_frontier() {
    let mut game = game("2030-06-01");
    let opening = game.corporate_actions().unwrap().clone();
    let before_tick = game.tick();
    let report = game.end_civil_day().expect("真实周六 civil 日结应成功");
    assert_eq!(report.settled_date, date("2030-06-01"));
    assert_eq!(game.tick(), before_tick, "休市日不能制造 market tick");
    assert_ordinary_days(&game, &opening, &[date("2030-06-01")], &[]);
}

#[test]
fn r4_tax_02_trading_day_rejects_before_close_then_commits_actual_zero_market_day() {
    let mut game = game("2030-06-03");
    let opening = game.corporate_actions().unwrap().clone();
    let before = serde_json::to_value(game.save().unwrap()).unwrap();
    let error = game
        .end_civil_day()
        .expect_err("交易日未完成不能制造完成水位");
    assert!(matches!(error, SessionError::CivilClock(_)), "{error:?}");
    assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
    game.step().expect("同一健康会话真实完成唯一 market tick");
    assert_eq!(game.tick(), 1);
    let report = game.end_civil_day().expect("实际收盘后 civil 日结应成功");
    assert_eq!(report.settled_date, date("2030-06-03"));
    assert_eq!(game.tick(), 1);
    assert_ordinary_days(&game, &opening, &[date("2030-06-03")], &[]);
}

#[test]
fn r4_tax_03_registration_day_captures_two_plans_and_keeps_real_tax_lots_on_restore() {
    // 17 个月覆盖 2029 完整自然年度，分红批准仍通过真实利润与法定公积金校验。
    let initial = game_with_history("2030-06-03", 17);
    let mut saved = initial.save().unwrap();
    let calendar = TradingCalendar::from_policy(
        CalendarPolicy::from_parts(saved.civil_clock.policy.clone()).unwrap(),
    )
    .unwrap();
    let registered_on = date("2030-06-03");
    let ex_on = calendar
        .next_trading_day(CalendarExchange::Sse, registered_on)
        .unwrap();
    let SavedSessionCompanies::CompanySystem {
        system,
        corporate_actions,
    } = &mut saved.company_state
    else {
        panic!("夹具显式选择 Simple")
    };
    let registered_capital = system
        .dividend_legal_facts(&CompanyId(0))
        .unwrap()
        .unwrap()
        .registered_capital;
    // 只构造受 strict restore 校验的批准事实；不冒玩家命令批准路径，绝不预登记。
    for (plan_id, gross_per_share) in [("ordinary-r-a", 1), ("ordinary-r-b", 2)] {
        let total_gross = Money::from_cents(gross_per_share * 1_200);
        system
            .declare_dividend(
                &CompanyId(0),
                DividendDeclaration {
                    plan_id: plan_id.into(),
                    approved_on: registered_on,
                    total_gross: AccountingAmount::from_money(total_gross),
                    registered_capital,
                },
            )
            .expect("唯一公司领域入口应接受合法批准事实");
        let plan = CashDividendPlan::new(
            plan_id.into(),
            CompanyId(0),
            saved.setup.stocks[0].code.clone(),
            CalendarExchange::Sse,
            CashDividendFormula::StandardCashOnly,
            registered_on,
            registered_on,
            registered_on,
            ex_on,
            ex_on,
            Money::from_cents(gross_per_share),
            total_gross,
            &calendar,
        )
        .unwrap();
        corporate_actions
            .dividends
            .push(CashDividendBook::new(plan).unwrap());
    }
    assert!(corporate_actions
        .dividends
        .iter()
        .all(|book| book.registration().is_none()));
    for book in &corporate_actions.dividend_tax_books {
        assert_eq!(
            serde_json::to_value(book).unwrap()["dividends"],
            serde_json::json!([])
        );
    }
    let mut game =
        GameSession::restore(&saved).expect("完整批准 fixture 必须先通过公共 strict restore");
    let opening = game.corporate_actions().unwrap().clone();
    game.step().expect("登记日真实市场末拍");
    game.end_civil_day()
        .expect("同一 civil 事务应完成市场税日与两份方案登记");
    let saved = assert_ordinary_days(
        &game,
        &opening,
        &[registered_on],
        &["ordinary-r-a", "ordinary-r-b"],
    );
    let actions = game.corporate_actions().unwrap();
    let registry = &actions.registries[0];
    for book in &actions.dividends {
        assert_eq!(book.status(), &CashDividendStatus::Registered);
        let snapshot = registry
            .registration(&book.plan().plan_id)
            .expect("实际 typed 日结必须产生方案 capture");
        assert_eq!(book.registration(), Some(snapshot));
        assert_eq!(snapshot.registered_on(), registered_on);
        assert_eq!(
            snapshot.settled_receipts(),
            registry.receipts().len() as u64
        );
        assert_eq!(snapshot.holdings(), registry.holdings());
    }
    for book in &actions.dividend_tax_books {
        let encoded = serde_json::to_value(book).unwrap();
        let market_seq = encoded["days"][0]["operation_seq"].as_u64().unwrap();
        let registered = encoded["dividends"].as_array().unwrap();
        assert_eq!(
            registered.len(),
            2,
            "同一市场 frontier 的第二份方案不能被第一份登记推进 seq 后误拒"
        );
        for plan in &actions.dividends {
            let event = registered
                .iter()
                .find(|event| event["event_id"] == format!("dividend-tax:{}", plan.plan().plan_id))
                .expect("每份真实 capture 都应关联本账户税基");
            assert_eq!(
                event["registered_on"],
                serde_json::to_value(registered_on).unwrap()
            );
            assert_eq!(event["lots"], serde_json::to_value(book.lots()).unwrap());
            assert!(event["operation_seq"].as_u64().unwrap() > market_seq);
            assert_eq!(
                event["payments"],
                serde_json::json!([]),
                "未到付款日不得提前记支付"
            );
        }
    }
    let restored = GameSession::restore(&saved).expect("完整 SaveSlot 应通过真实来源恢复校验");
    assert_eq!(restored.corporate_actions(), game.corporate_actions());
    assert_eq!(restored.civil_date(), ex_on);
    assert_eq!(restored.tick(), 1);
}

#[test]
fn r4_tax_04_existing_personal_holder_without_tax_book_rejects_without_mutation() {
    let initial = game("2030-06-01");
    let mut saved = initial.save().unwrap();
    let SavedSessionCompanies::CompanySystem {
        corporate_actions, ..
    } = &mut saved.company_state
    else {
        panic!("夹具显式选择 Simple")
    };
    let missing = corporate_actions.dividend_tax_books.remove(0);
    assert!(!missing.lots().is_empty(), "只删除已有个人持有人的税账");
    let mut game = GameSession::restore(&saved)
        .expect("当前 strict restore 允许该输入，真实日结必须显式核个人税账覆盖");
    let before = serde_json::to_value(game.save().unwrap()).unwrap();
    let error = game
        .end_civil_day()
        .expect_err("已有个人持有人缺失税账必须拒绝，不能静默跳过税日");
    assert!(
        error.to_string().contains("税账"),
        "必须保留缺失个人税账的上下文：{error}"
    );
    assert_eq!(
        serde_json::to_value(game.save().unwrap()).unwrap(),
        before,
        "税账覆盖拒绝不得提交公司、REG、civil frontier 或个人态"
    );
}

#[test]
fn r4_tax_05_orphan_tax_book_rejects_without_creating_account_or_tax_day() {
    let initial = game("2030-06-01");
    let orphan = engine::AccountId(99);
    assert!(
        initial.account(orphan).is_none(),
        "不存在夹具税账的归属账户"
    );
    let mut saved = initial.save().unwrap();
    let SavedSessionCompanies::CompanySystem {
        corporate_actions, ..
    } = &mut saved.company_state
    else {
        panic!("夹具显式选择 Simple")
    };
    let registry = &corporate_actions.registries[0];
    corporate_actions.dividend_tax_books.push(
        engine::company::cash_dividend_tax::CashDividendTaxBook::new(
            orphan,
            registry.stock().clone(),
            engine::company::cash_dividend_tax::DividendTaxProfile::IndividualPublicMarket,
            registry.settled_on(),
            Vec::new(),
        )
        .expect("空税账自身是合法领域值，Session 仍须核真实账户归属"),
    );
    let mut game = GameSession::restore(&saved)
        .expect("当前 strict restore 允许该输入，真实日结必须核税账归户");
    let before = serde_json::to_value(game.save().unwrap()).unwrap();
    let error = game
        .end_civil_day()
        .expect_err("孤儿个人税账必须拒绝，不能为不存在账户记录税日");
    assert!(
        error.to_string().contains("税账"),
        "必须保留孤儿税账的上下文：{error}"
    );
    assert!(game.account(orphan).is_none(), "失败不能补造账户");
    assert_eq!(
        serde_json::to_value(game.save().unwrap()).unwrap(),
        before,
        "孤儿税账拒绝不得提交公司、REG、civil frontier 或个人态"
    );
}
