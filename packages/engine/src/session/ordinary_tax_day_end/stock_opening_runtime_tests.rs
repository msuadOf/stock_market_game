//! 送转盘前生产旅程；只装配批准事实，登记、到账、除权和消费均走真实入口。
use super::*;
use crate::company::stock_distribution::{
    StockDistributionBook, StockDistributionDeclaration, StockDistributionEventPlan,
    StockDistributionKind, StockDistributionStatus,
};

fn stock_session(start: &str, registered: &str, ex: &str) -> GameSession {
    stock_session_with_repurchase(start, registered, ex, false)
}

fn stock_session_with_repurchase(
    start: &str,
    registered: &str,
    ex: &str,
    enabled: bool,
) -> GameSession {
    let template = payment_session(CashDividendTaxMode::Exempt);
    let mut setup = template.state.setup.clone();
    setup.start_date = date(start);
    setup.issuer_repurchase_enabled = enabled;
    let session = GameSession::new_with_runtime_groups(setup, 7, 3).unwrap();
    let mut saved = session.save().unwrap();
    let capital = session
        .company_system()
        .unwrap()
        .dividend_legal_facts(&CompanyId(0))
        .unwrap()
        .unwrap()
        .registered_capital;
    saved
        .company_state
        .system_for_test_mut()
        .declare_stock_distribution(
            &CompanyId(0),
            StockDistributionDeclaration {
                event_id: "opening-stock".into(),
                approval_reference: "真实批准测试".into(),
                kind: StockDistributionKind::BonusShares,
                approved_on: date(start),
                new_shares: 1200,
                par_value_per_share: Money::from_cents(100),
                capital_increase: crate::accounting::AccountingAmount::from_cents(120_000),
                registered_capital_at_approval: capital,
            },
        )
        .unwrap();
    saved
        .company_state
        .actions_for_test_mut()
        .stock_distributions
        .push(
            StockDistributionBook::new(StockDistributionEventPlan {
                event_id: "opening-stock".into(),
                approval_reference: "真实批准测试".into(),
                issuer: CompanyId(0),
                stock: crate::StockCode(STOCK.into()),
                exchange: crate::calendar::CalendarExchange::Sse,
                kind: StockDistributionKind::BonusShares,
                approved_on: date(start),
                announced_on: date(start),
                registered_on: date(registered),
                ex_rights_on: date(ex),
                shares_per_existing_share_micros: 1_000_000,
                approved_total_new_shares: 1200,
            })
            .unwrap(),
        );
    restore_with_three_groups(&saved)
}

pub(super) fn trade(
    session: &mut GameSession,
    seller: AccountId,
    buyer: AccountId,
    price: i64,
    qty: u32,
) {
    if session.runtime_driver.is_none() {
        session.construct_runtime_driver_lazy_for_test().unwrap();
    }
    let sell = Intent::PlaceLimit {
        code: StockId(0),
        side: Side::Sell,
        price: LimitPrice::Fixed(Money::from_cents(price)),
        qty,
    };
    let buy = Intent::PlaceLimit {
        code: StockId(0),
        side: Side::Buy,
        price: LimitPrice::Fixed(Money::from_cents(price)),
        qty,
    };
    if buyer == AccountId(0) {
        session.seed_order_for_test(seller, sell, &mut Vec::new());
        session.enqueue_player_intent(buyer, buy).unwrap();
    } else {
        assert_eq!(seller, AccountId(0));
        session.seed_order_for_test(buyer, buy, &mut Vec::new());
        session.enqueue_player_intent(seller, sell).unwrap();
    }
    let events = session.step().unwrap();
    assert!(
        events.iter().any(|event| matches!(event,
        Event::Trade { maker, taker, qty: actual, price: actual_price, .. }
        if *maker == seller && *taker == buyer && *actual == qty && actual_price.cents() == price))
    );
}

#[test]
fn stock_opening_with_repurchase_enabled_and_no_repurchase_event() {
    let mut session = stock_session_with_repurchase(START, START, PAYMENT_DAY, true);
    trade(&mut session, AccountId(1), AccountId(0), 1000, 100);
    session.end_civil_day().unwrap();
    assert_eq!(
        session.corporate_actions().unwrap().stock_distributions[0].status(),
        &StockDistributionStatus::Credited
    );
    assert_eq!(
        session
            .account(AccountId(0))
            .unwrap()
            .position(&StockId(0))
            .unwrap()
            .qty(),
        200
    );
    restore_with_three_groups(&session.save().unwrap());
}

#[test]
fn native_stock_opening_real_registration_restore_first_sale_and_close() {
    let mut session = stock_session(START, START, PAYMENT_DAY);
    trade(&mut session, AccountId(1), AccountId(0), 1000, 100);
    let before_cash = session.account(AccountId(0)).unwrap().cash();
    let invested = session
        .account(AccountId(0))
        .unwrap()
        .position(&StockId(0))
        .unwrap()
        .invested_cents();
    session.end_civil_day().unwrap();
    let actions = session.corporate_actions().unwrap();
    assert_eq!(
        actions.stock_distributions[0].status(),
        &StockDistributionStatus::Credited
    );
    let registry = &actions.registries[0];
    assert_eq!(registry.settled_on(), date(START));
    assert_eq!(registry.issued_shares(), 2400);
    assert!(registry.pending_opening().is_some());
    assert_eq!(
        session
            .company_system()
            .unwrap()
            .issuers()
            .get(&CompanyId(0))
            .unwrap()
            .issued_shares,
        2400
    );
    let position = session
        .account(AccountId(0))
        .unwrap()
        .position(&StockId(0))
        .unwrap();
    assert_eq!(position.qty(), 200);
    assert_eq!(position.sellable(), 200);
    assert_eq!(position.invested_cents(), invested);
    assert_eq!(position.recovered_cents(), 0);
    assert_eq!(session.account(AccountId(0)).unwrap().cash(), before_cash);
    for account in [AccountId(1), AccountId(2)] {
        let expected = if account == AccountId(1) { 200 } else { 400 };
        assert_eq!(
            session
                .account(account)
                .unwrap()
                .position(&StockId(0))
                .unwrap()
                .qty(),
            expected
        );
    }
    assert_eq!(
        session
            .runtime_driver
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .world_public_views()[0]
            .last_close,
        500
    );
    assert_eq!(session.state.markets[&StockId(0)].last_close().cents(), 500);
    let experience = session.state.retail_experience.get(&AccountId(2)).unwrap();
    assert_eq!(
        experience.stocks[&StockId(0)].entry_reference_price,
        Some(Money::from_cents(500))
    );
    assert_eq!(
        experience.feedback.stocks[&StockId(0)]
            .last_own_observation
            .unwrap()
            .price,
        Money::from_cents(500)
    );
    let saved = session.save().unwrap();
    let json: crate::SaveSlot =
        serde_json::from_value(serde_json::to_value(&saved).unwrap()).unwrap();
    let mut restored = restore_with_three_groups(&json);
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(saved).unwrap()
    );
    assert_eq!(
        restored.state.markets[&StockId(0)].last_price().cents(),
        1000,
        "原始成交价不能追溯改写"
    );
    assert_eq!(
        restored.build_market_view().stocks[&StockId(0)]
            .last_price
            .cents(),
        500
    );
    assert_eq!(
        restored.snapshot().markets[&StockId(0)].last_price.cents(),
        500
    );
    let failures_before = restored
        .state
        .retail_experience
        .get(&AccountId(2))
        .unwrap()
        .consecutive_failed_buys;
    restored.construct_runtime_driver_lazy_for_test().unwrap();
    let next_tick = restored.tick() + 1;
    restored
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .force_due_for_test_at(&[AccountId(2)], next_tick);
    trade(&mut restored, AccountId(0), AccountId(1), 500, 200);
    let observed = restored
        .state
        .retail_experience
        .get(&AccountId(2))
        .unwrap()
        .feedback
        .stocks[&StockId(0)]
        .last_own_observation
        .unwrap();
    assert_eq!(observed.price, Money::from_cents(500));
    assert_eq!(
        restored
            .state
            .retail_experience
            .get(&AccountId(2))
            .unwrap()
            .consecutive_failed_buys,
        failures_before,
        "机械除权不能产生失败经历"
    );
    assert_eq!(
        observed.moment.civil_date,
        date(PAYMENT_DAY),
        "必须是 E 首拍真实观察"
    );
    restored.end_civil_day().unwrap();
    let registry = &restored.corporate_actions().unwrap().registries[0];
    assert_eq!(registry.settled_on(), date(PAYMENT_DAY));
    assert_eq!(registry.issued_shares(), 2400);
    assert!(registry.pending_opening().is_none());
    assert_eq!(
        registry
            .receipts()
            .iter()
            .filter(|r| r.request.event_id == "stock-distribution:opening-stock")
            .count(),
        1
    );
    let saved = restored.save().unwrap();
    restore_with_three_groups(&saved);
    let mut corrupt = saved.clone();
    corrupt
        .company_state
        .actions_for_test_mut()
        .applied_ex_reference_groups
        .clear();
    assert!(GameSession::restore(&corrupt).is_err());
}

#[test]
fn native_stock_opening_weekend_projection_abort_leaves_all_authorities_unchanged() {
    let mut session = stock_session("2030-06-07", "2030-06-07", "2030-06-10");
    session.step().unwrap();
    session.end_civil_day().unwrap();
    session.end_civil_day().unwrap();
    assert_eq!(session.civil_date(), date("2030-06-09"));
    let before = serde_json::to_value(session.save().unwrap()).unwrap();
    let before_desks = session
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .world_public_views()
        .to_vec();
    let before_raw = session
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .world_accounts_snapshot()
        .unwrap();
    let result: Result<(), SessionError> = session.with_prepared_runtime_civil_day(|shadow, _| {
        assert!(shadow.corporate_actions().unwrap().registries[0]
            .pending_opening()
            .is_some());
        assert_eq!(shadow.state.markets[&StockId(0)].last_close().cents(), 500);
        Err(SessionError::UnsupportedCapitalBoundary(
            "送转最终投影主动拒绝".into(),
        ))
    });
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("送转最终投影主动拒绝"));
    assert_eq!(
        serde_json::to_value(session.save().unwrap()).unwrap(),
        before
    );
    {
        let mut driver = session.runtime_driver.as_ref().unwrap().lock().unwrap();
        assert!(!driver.administrative_world_is_terminal());
        assert_eq!(driver.world_accounts_snapshot().unwrap(), before_raw);
        assert_eq!(driver.world_public_views(), before_desks.as_slice());
    }
    session.end_civil_day().unwrap();
    restore_with_three_groups(&session.save().unwrap());
}

#[test]
fn native_stock_opening_real_close_projection_failure_seals_session_without_credit() {
    let mut session = stock_session(START, START, PAYMENT_DAY);
    session.step().unwrap();
    let before = session.corporate_actions().unwrap().clone();
    let before_raw = session
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .world_accounts_snapshot()
        .unwrap();
    let result: Result<(), SessionError> = session.with_prepared_runtime_civil_day(|shadow, _| {
        assert!(shadow.corporate_actions().unwrap().registries[0]
            .pending_opening()
            .is_some());
        Err(SessionError::UnsupportedCapitalBoundary(
            "真实收盘后送转投影拒绝".into(),
        ))
    });
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("真实收盘后送转投影拒绝"));
    assert!(session.poison_reason().is_some());
    assert_eq!(session.corporate_actions().unwrap(), &before);
    assert_eq!(
        session
            .runtime_driver
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .world_accounts_snapshot()
            .unwrap(),
        before_raw
    );
    assert!(session.step().is_err());
}
