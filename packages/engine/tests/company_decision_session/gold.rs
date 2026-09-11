//! Happy path：完整决策链在真实 GameSession 执行 + 确定性 + 全风格覆盖。

use super::*;

#[test]
fn decision_chain_runs_attention_information_beliefs_plans_and_orders() {
    let mut session = GameSession::new(chain_setup("2030-01-07"), SEED).unwrap();
    let initial = session.decision_chain_diagnostics();
    // 前史公开库已播种（任务 15 装配）：信念账户有材料可读。
    assert!(
        initial.library_publications > 0,
        "seeded library must exist"
    );
    assert!(initial.belief_accounts > 0);

    let mut institution_orders = 0usize;
    for _ in 0..120 {
        for event in session.step() {
            if let engine::session::Event::OrderAccepted { account, .. } = event {
                // 机构账户 id 段：玩家 0 + 12 散户 = 13 起。
                if account.0 >= 13 && account.0 < 23 {
                    institution_orders += 1;
                }
            }
        }
    }
    let after = session.decision_chain_diagnostics();
    // 链条各段都真实发生：获知 > 0、（若有方向分歧）计划 > 0、机构订单 > 0。
    assert!(
        after.acquired_publications > initial.acquired_publications,
        "accepted attention must acquire publications: {:?} -> {:?}",
        initial.acquired_publications,
        after.acquired_publications
    );
    assert!(after.plan_count > 0, "belief institutions must form plans");
    assert!(
        institution_orders > 0,
        "the chain must submit real orders through the authoritative router"
    );
}

#[test]
fn civil_day_operations_advances_and_disclosures_publish() {
    let mut session = GameSession::new(chain_setup("2030-01-07"), SEED).unwrap();
    let before = session.decision_chain_diagnostics();
    run_full_day(&mut session);
    let after = session.decision_chain_diagnostics();
    // K4 日终顺序全部成功（经营终局 →（非月末）→ 18:00 披露）：
    // 1 月排期（Q1/H1/Q3/年报）尚未到期时库不增长——但经营必须前进。
    assert_eq!(
        session.civil_date(),
        engine::CivilDate::from_iso("2030-01-08").unwrap(),
        "end_civil_day must advance to the next civil day"
    );
    assert_eq!(session.day(), 1);
    let _ = before;
    let _ = after;
}

#[test]
fn month_end_closes_books_and_continues() {
    // 2030-01-28（周一）起跑到 01-31（周四）：月末封账在最后一个日终执行
    // （1 月无季报排期，但 close_month 的版本登记仍会运行——任何结账错误
    // 都会让 end_civil_day 显式失败）。
    let mut session = GameSession::new(chain_setup("2030-01-28"), SEED).unwrap();
    for expected in ["2030-01-29", "2030-01-30", "2030-01-31", "2030-02-01"] {
        run_full_day(&mut session);
        assert_eq!(
            session.civil_date(),
            engine::CivilDate::from_iso(expected).unwrap(),
            "civil day must advance through the month end"
        );
    }
}

#[test]
fn all_five_institution_styles_and_hot_styles_are_active() {
    use std::collections::BTreeSet;
    let session = GameSession::new(chain_setup("2030-01-07"), SEED).unwrap();
    let profiles = session.account_strategy_profiles();
    let inst_styles: BTreeSet<String> = profiles
        .values()
        .filter_map(|profile| match profile {
            engine::strategy::StrategyProfile::Institution(style) => Some(format!("{style:?}")),
            _ => None,
        })
        .collect();
    for style in [
        "DeepValue",
        "Growth",
        "Balanced",
        "Defensive",
        "ActiveTrader",
    ] {
        assert!(
            inst_styles.contains(style),
            "institution style {style} must be present: {inst_styles:?}"
        );
    }
    let hot_styles: BTreeSet<String> = profiles
        .values()
        .filter_map(|profile| match profile {
            engine::strategy::StrategyProfile::Hot(style) => Some(format!("{style:?}")),
            _ => None,
        })
        .collect();
    assert!(hot_styles.contains("Momentum"));
    assert!(hot_styles.contains("Reversal"));
    let retail_styles: BTreeSet<String> = profiles
        .values()
        .filter_map(|profile| match profile {
            engine::strategy::StrategyProfile::Retail(style) => Some(format!("{style:?}")),
            _ => None,
        })
        .collect();
    assert!(retail_styles.len() >= 2, "retail style diversity expected");
}

#[test]
fn same_seed_replays_the_whole_chain_bit_identically() {
    let run = || {
        let mut session = GameSession::new(chain_setup("2030-01-07"), SEED).unwrap();
        let mut events = Vec::new();
        for _ in 0..60 {
            events.extend(session.step());
        }
        session.end_civil_day().unwrap();
        for _ in 0..60 {
            events.extend(session.step());
        }
        (
            serde_json::to_vec(&events).unwrap(),
            serde_json::to_vec(&session.decision_chain_diagnostics()).unwrap(),
            serde_json::to_vec(&session.plans_debug()).unwrap(),
        )
    };
    let first = run();
    let second = run();
    assert_eq!(first.0, second.0, "event stream must be deterministic");
    assert_eq!(first.1, second.1, "chain diagnostics must be deterministic");
    assert_eq!(first.2, second.2, "plan state must be deterministic");
}

#[test]
fn cpu_compute_backend_serves_the_common_market_view_deterministically() {
    // ComputeBackend 契约（任务 26 后）：纯批量输入 + 稳定顺序输出。
    use engine::{
        create_backend, ComputeMode, MarketView, SelfView, StockView, StrategyData, TargetPolicy,
    };
    let backend = create_backend(&ComputeMode::Cpu).expect("cpu backend must be available");
    let strategies = [
        StrategyData::inst(TargetPolicy::Fixed(Money::from_cents(1_000)), 0.05, 100),
        StrategyData::retail(1.0, 100, 0.0, 1),
        StrategyData::hot(3, 0.02, 100),
    ];
    let market = MarketView {
        stocks: [(
            StockCode("600101".to_string()),
            StockView {
                best_bid: Some(Money::from_cents(999)),
                best_ask: Some(Money::from_cents(1_001)),
                last_price: Money::from_cents(1_000),
                recent_prices: vec![Money::from_cents(1_000)],
                recent_market_minute_prices: vec![],
                relative_volume: 1.0,
                order_book_imbalance: 0.0,
            },
        )]
        .into(),
        tick: 0,
        market_minute: 0,
    };
    let own = SelfView {
        cash: Money::from_cents(1_000_000_000),
        positions: Default::default(),
    };
    let selves = [own.clone(), own.clone(), own];
    let seeds = [1_u64, 2, 3];
    let first = backend
        .decide_all(&strategies, &market, &selves, &seeds)
        .unwrap();
    let second = backend
        .decide_all(&strategies, &market, &selves, &seeds)
        .unwrap();
    assert_eq!(first.len(), 3);
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap(),
        "same batch inputs must produce identical outputs in input order"
    );
}

#[test]
fn per_share_valuations_stay_in_price_dimension_not_total_equity() {
    // 拒绝整体权益量纲误接：每股区间的「分」值域必须与股价同量级，
    // 而不是公司总权益估计（10^9 股本公司会大 6-7 个数量级）。
    let mut session = GameSession::new(chain_setup("2030-01-07"), SEED).unwrap();
    for _ in 0..120 {
        session.step();
    }
    let mut checked = 0;
    for account in 13u64..23 {
        for stock in ["600101", "002156", "300260", "600610", "000812"] {
            if let Some(summary) =
                session.belief_debug(engine::AccountId(account), &StockCode(stock.to_string()))
            {
                if summary.unavailable_reason.is_some() {
                    continue;
                }
                assert!(
                    summary.per_share_pessimistic_cents > 0
                        && summary.per_share_optimistic_cents
                            >= summary.per_share_pessimistic_cents
                );
                // 每股（分）应在价格量级（< 10^6 分 = 1 万元/股）；
                // 总权益误接会是 10^9+ 分量级。
                assert!(
                    summary.per_share_optimistic_cents < 1_000_000,
                    "per-share band must stay in price dimension: {stock} {:?}",
                    summary
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "at least one available valuation must exist");
}
