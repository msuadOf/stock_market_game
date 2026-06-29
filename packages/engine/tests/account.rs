//! engine account + strategy-trait 模块集成测试（TDD 红绿循环）。
// `Strategy` 当前在 Task 1 骨架测试中尚未直接使用，但后续 account 任务（持有 `Box<dyn Strategy>`、
// 注入 NPC 策略）会消费它——此处提前 import 以保持 plan 预期的导入集合，待消费后移除 allow。
#[allow(unused_imports)]
use engine::strategy::{Intent, MarketView, Strategy};
use engine::orderbook::Side;
use engine::Money;

#[test]
fn intent_and_marketview_construct() {
    use engine::strategy::{Intent, MarketView, StockView, SelfView, PositionView};
    use engine::account::StockCode;
    use engine::orderbook::Side;
    use std::collections::BTreeMap;
    use engine::Money;

    let mut stocks = BTreeMap::new();
    stocks.insert(StockCode("600101".to_string()), StockView {
        best_bid: Some(Money::from_cents(999)),
        best_ask: Some(Money::from_cents(1001)),
        last_price: Money::from_cents(1000),
        fundamental_value: None,
        recent_prices: vec![Money::from_cents(1000)],
    });
    let mv = MarketView { stocks };
    assert_eq!(mv.stocks.len(), 1);

    let i = Intent::PlaceLimit { code: StockCode("600101".to_string()), side: Side::Buy, price: Money::from_cents(1000), qty: 100 };
    assert!(matches!(i, Intent::PlaceLimit { side: Side::Buy, qty: 100, .. }));

    let sv = SelfView { cash: Money::from_cents(1_000_000), positions: BTreeMap::new() };
    assert_eq!(sv.cash.cents(), 1_000_000);
    let _pv = PositionView { qty: 0, sellable_qty: 0, cost_price: None };
}

use engine::account::{AccountError, AccountKind, StockCode};

#[test]
fn account_error_and_kind_basics() {
    let e1 = AccountError::InsufficientCash {
        needed: Money::from_cents(1500),
        have: Money::from_cents(1000),
    };
    assert!(e1.to_string().contains("cash"));
    let e2 = AccountError::NoPosition(StockCode("600101".to_string()));
    assert!(e2.to_string().contains("600101"));
    assert_ne!(AccountKind::Retail, AccountKind::Player);
}

use engine::account::Position;

#[test]
fn position_cost_price_integer_rounding() {
    // 整除：invested=100000(1000元), recovered=0, qty=100 → cost=1000 分/股 = 10.00 元
    let p = Position {
        qty: 100,
        t1_locked: 0,
        invested_cents: 100_000,
        recovered_cents: 0,
    };
    assert_eq!(p.cost_price().unwrap().cents(), 1000);

    // 加权：invested=220000, qty=200 → 1100 分 = 11.00
    let p2 = Position {
        qty: 200,
        t1_locked: 0,
        invested_cents: 220_000,
        recovered_cents: 0,
    };
    assert_eq!(p2.cost_price().unwrap().cents(), 1100);

    // 非整除 half-to-even：invested=1000, recovered=0, qty=3 → 1000/3=333.33… → 333（<.5 向下）
    let p3 = Position {
        qty: 3,
        t1_locked: 0,
        invested_cents: 1000,
        recovered_cents: 0,
    };
    assert_eq!(p3.cost_price().unwrap().cents(), 333);

    // 恰好半（half-to-even）：invested=5, qty=2 → 2.5 → 偶数取 2
    let p4 = Position {
        qty: 2,
        t1_locked: 0,
        invested_cents: 5,
        recovered_cents: 0,
    };
    assert_eq!(p4.cost_price().unwrap().cents(), 2); // 2.5 → 2 (偶)
    // invested=7, qty=2 → 3.5 → 偶数取 4
    let p5 = Position {
        qty: 2,
        t1_locked: 0,
        invested_cents: 7,
        recovered_cents: 0,
    };
    assert_eq!(p5.cost_price().unwrap().cents(), 4); // 3.5 → 4 (偶)
}

#[test]
fn position_cost_price_negative() {
    // 净投入/持仓：invested < recovered → 负成本
    // invested=100000, recovered=200000, qty=100 → (100000-200000)/100 = -1000 分
    let p = Position {
        qty: 100,
        t1_locked: 0,
        invested_cents: 100_000,
        recovered_cents: 200_000,
    };
    assert_eq!(p.cost_price().unwrap().cents(), -1000);
}

#[test]
fn position_cost_price_none_when_zero_qty() {
    let p = Position {
        qty: 0,
        t1_locked: 0,
        invested_cents: 0,
        recovered_cents: 0,
    };
    assert!(p.cost_price().is_none());
    assert_eq!(p.sellable(), 0);
}

#[test]
fn position_sellable_minus_t1_locked() {
    let p = Position {
        qty: 100,
        t1_locked: 30,
        invested_cents: 0,
        recovered_cents: 0,
    };
    assert_eq!(p.sellable(), 70);
}

use engine::account::Account;
use engine::orderbook::AccountId;

#[test]
fn account_new_player_has_no_strategy() {
    let a = Account::new(AccountId(1), AccountKind::Player, Money::from_cents(10_000_000));
    assert_eq!(a.cash.cents(), 10_000_000);
    assert!(a.positions.is_empty());
    assert!(!a.has_strategy()); // 玩家 None
    assert_eq!(a.cost_price(&StockCode("600101".to_string())), None); // 无持仓
    assert_eq!(a.sellable_qty(&StockCode("600101".to_string())), 0);
}

use engine::config::GameConfig;

/// T+0 默认配置（lot/费率用 ref 提议值），供 apply_buy/apply_sell 测试统一构造。
fn cfg_t0() -> GameConfig {
    GameConfig::proposed_defaults()
}

#[test]
fn apply_buy_debits_cash_and_adds_position() {
    let mut a = Account::new(AccountId(1), AccountKind::Player, Money::from_cents(10_000_000));
    let code = StockCode("600101".to_string());
    // 买 10.00 × 100 = 1000 元 = 100000 分；佣金 max(100000*0.00025=25, 500)=500 分
    a.apply_buy(&cfg_t0(), code.clone(), Money::from_cents(1000), 100, false)
        .unwrap();
    assert_eq!(a.cash.cents(), 10_000_000 - 100_000 - 500); // 9_899_500
    let pos = a.positions.get(&code).unwrap();
    assert_eq!(pos.qty, 100);
    assert_eq!(pos.invested_cents, 100_000);
    assert_eq!(pos.recovered_cents, 0);
    assert_eq!(pos.t1_locked, 0); // T+0
    assert_eq!(pos.cost_price().unwrap().cents(), 1000); // 10.00
    assert_eq!(a.sellable_qty(&code), 100); // T+0 可卖
}

#[test]
fn apply_buy_weighted_cost_price() {
    let mut a = Account::new(AccountId(1), AccountKind::Player, Money::from_cents(10_000_000));
    let code = StockCode("600101".to_string());
    a.apply_buy(&cfg_t0(), code.clone(), Money::from_cents(1000), 100, false)
        .unwrap(); // 10.00×100
    a.apply_buy(&cfg_t0(), code.clone(), Money::from_cents(1200), 100, false)
        .unwrap(); // 12.00×100
    let pos = a.positions.get(&code).unwrap();
    assert_eq!(pos.qty, 200);
    assert_eq!(pos.invested_cents, 220_000); // 100000+120000
    assert_eq!(pos.cost_price().unwrap().cents(), 1100); // 11.00 加权
}

#[test]
fn apply_buy_insufficient_cash_rejected_and_unchanged() {
    let mut a = Account::new(AccountId(1), AccountKind::Player, Money::from_cents(1000)); // 只有 10 元
    let code = StockCode("600101".to_string());
    // 买 10.00×100 需 1000 元，现金仅 10 元 → 拒绝
    let err = a
        .apply_buy(&cfg_t0(), code.clone(), Money::from_cents(1000), 100, false)
        .unwrap_err();
    assert!(matches!(err, AccountError::InsufficientCash { .. }));
    assert_eq!(a.cash.cents(), 1000); // 不变
    assert!(a.positions.is_empty()); // 无半成交
}

#[test]
fn apply_buy_t1_locks_sellable() {
    let mut a = Account::new(AccountId(1), AccountKind::Player, Money::from_cents(10_000_000));
    let code = StockCode("600101".to_string());
    a.apply_buy(&cfg_t0(), code.clone(), Money::from_cents(1000), 100, true)
        .unwrap(); // t1_enabled
    assert_eq!(a.positions.get(&code).unwrap().t1_locked, 100);
    assert_eq!(a.sellable_qty(&code), 0); // T+1 当日不可卖
}

#[test]
fn apply_sell_credits_net_and_clears_position() {
    let mut a = Account::new(AccountId(1), AccountKind::Player, Money::from_cents(10_000_000));
    let code = StockCode("600101".to_string());
    a.apply_buy(&cfg_t0(), code.clone(), Money::from_cents(1000), 100, false)
        .unwrap(); // 持 100 股
    let cash_before = a.cash.cents();
    // 卖 10.00×100：proceeds=100000 分；佣金 max(25,500)=500；印花 100000*0.0005=50；net=100000-500-50=99450
    a.apply_sell(&cfg_t0(), code.clone(), Money::from_cents(1000), 100)
        .unwrap();
    assert_eq!(a.cash.cents(), cash_before + 99_450);
    assert!(!a.positions.contains_key(&code)); // 清仓删除
}

#[test]
fn apply_sell_rejects_oversell() {
    let mut a = Account::new(AccountId(1), AccountKind::Player, Money::from_cents(10_000_000));
    let code = StockCode("600101".to_string());
    a.apply_buy(&cfg_t0(), code.clone(), Money::from_cents(1000), 100, false)
        .unwrap(); // 可卖 100
    let err = a
        .apply_sell(&cfg_t0(), code.clone(), Money::from_cents(1000), 200)
        .unwrap_err();
    assert!(matches!(
        err,
        AccountError::InsufficientShares {
            needed: 200,
            have: 100,
            ..
        }
    ));
    assert_eq!(a.positions.get(&code).unwrap().qty, 100); // 不变
}

#[test]
fn apply_sell_no_position_rejects() {
    let mut a = Account::new(AccountId(1), AccountKind::Player, Money::from_cents(10_000_000));
    let code = StockCode("600101".to_string());
    let err = a
        .apply_sell(&cfg_t0(), code.clone(), Money::from_cents(1000), 1)
        .unwrap_err();
    assert!(matches!(err, AccountError::InsufficientShares { have: 0, .. }));
}

#[test]
fn apply_sell_partial_updates_recovered() {
    let mut a = Account::new(AccountId(1), AccountKind::Player, Money::from_cents(10_000_000));
    let code = StockCode("600101".to_string());
    a.apply_buy(&cfg_t0(), code.clone(), Money::from_cents(1000), 100, false)
        .unwrap();
    a.apply_sell(&cfg_t0(), code.clone(), Money::from_cents(1200), 50)
        .unwrap(); // 12.00 卖 50
    let pos = a.positions.get(&code).unwrap();
    assert_eq!(pos.qty, 50);
    assert_eq!(pos.recovered_cents, 60_000); // 12.00×50=600 元=60000 分
}

#[test]
fn apply_trade_dispatches_buy_and_sell() {
    let mut a = Account::new(AccountId(1), AccountKind::Player, Money::from_cents(10_000_000));
    let code = StockCode("600101".to_string());
    a.apply_trade(
        &cfg_t0(),
        Side::Buy,
        code.clone(),
        Money::from_cents(1000),
        100,
        false,
    )
    .unwrap();
    assert_eq!(a.positions.get(&code).unwrap().qty, 100);
    a.apply_trade(
        &cfg_t0(),
        Side::Sell,
        code.clone(),
        Money::from_cents(1000),
        100,
        false,
    )
    .unwrap();
    assert!(!a.positions.contains_key(&code));
}

#[test]
fn market_value_and_unrealized_pnl() {
    let mut a = Account::new(AccountId(1), AccountKind::Player, Money::from_cents(10_000_000));
    let code = StockCode("600101".to_string());
    a.apply_buy(&cfg_t0(), code.clone(), Money::from_cents(1000), 100, false)
        .unwrap(); // 成本 10.00
               // 现价 11.00：市值=11.00×100=110000 分；未实现盈亏=(1100-1000)×100=10000 分
    assert_eq!(
        a.market_value(&code, Money::from_cents(1100))
            .unwrap()
            .cents(),
        110_000
    );
    assert_eq!(
        a.unrealized_pnl(&code, Money::from_cents(1100))
            .unwrap()
            .cents(),
        10_000
    );
}

#[test]
fn grant_position_sets_cost_basis() {
    use engine::account::StockCode;
    use engine::Money;
    let mut a =
        engine::Account::new(engine::AccountId(1), engine::AccountKind::Retail, Money::from_cents(1_000_000));
    a.grant_position(StockCode("600101".to_string()), 1000, Money::from_cents(1000));
    let p = a.positions.get(&StockCode("600101".to_string())).unwrap();
    assert_eq!(p.qty, 1000);
    assert_eq!(p.invested_cents, 1_000_000); // 1000 × 1000
    assert_eq!(p.recovered_cents, 0);
    assert_eq!(p.t1_locked, 0);
    assert_eq!(p.cost_price().unwrap().cents(), 1000);
}

#[test]
fn reexport_from_crate_root() {
    use engine::{Account, AccountError, AccountKind, Position, StockCode};
    use engine::{Intent, MarketView, Strategy};
    use engine::strategy::SelfView;

    // Strategy trait re-export：实现一个「不动作」（返回空 Vec）的策略并注入 NPC 账户。
    struct AlwaysIdle;
    impl Strategy for AlwaysIdle {
        fn decide(&mut self, _market: &MarketView, _own: &SelfView, _rng: &mut dyn engine::Rng) -> Vec<Intent> {
            Vec::new()
        }
    }

    let mut npc = Account::new(AccountId(2), AccountKind::Retail, Money::ZERO);
    npc.set_strategy(Box::new(AlwaysIdle));
    assert!(npc.has_strategy());

    let _ = Account::new(AccountId(1), AccountKind::Player, Money::ZERO);
    let _: Position = Position {
        qty: 0,
        t1_locked: 0,
        invested_cents: 0,
        recovered_cents: 0,
    };
    let _: AccountError = AccountError::NoPosition(StockCode("x".to_string()));
    let _: Intent = Intent::PlaceMarket { code: StockCode("x".to_string()), side: engine::Side::Buy, qty: 1 };
    let mv: MarketView = MarketView { stocks: std::collections::BTreeMap::new() };
    assert!(mv.stocks.is_empty());
}
