//! 拆缩股复用真实登记、Admin、Desk 与 opening 消费的生产旅程。
use super::*;
use crate::company::share_split::{
    ShareSplitBook, ShareSplitDeclaration, ShareSplitDirection, ShareSplitEventPlan,
    ShareSplitStatus,
};

fn split_session(direction: ShareSplitDirection, ratio: u64) -> GameSession {
    split_session_on(direction, ratio, START, PAYMENT_DAY)
}

fn split_session_on(
    direction: ShareSplitDirection,
    ratio: u64,
    start: &str,
    ex: &str,
) -> GameSession {
    let template = payment_session(CashDividendTaxMode::Exempt);
    let mut setup = template.state.setup.clone();
    setup.start_date = date(start);
    let session = GameSession::new_with_runtime_groups(setup, 7, 3).unwrap();
    let mut saved = session.save().unwrap();
    let capital = session
        .company_system()
        .unwrap()
        .dividend_legal_facts(&CompanyId(0))
        .unwrap()
        .unwrap()
        .registered_capital;
    let after = match direction {
        ShareSplitDirection::Split => 100 / ratio as i64,
        ShareSplitDirection::Consolidate => 100 * ratio as i64,
    };
    saved
        .company_state
        .system_for_test_mut()
        .declare_share_split(
            &CompanyId(0),
            ShareSplitDeclaration {
                event_id: "opening-split".into(),
                approval_reference: "游戏整数换算批准".into(),
                direction: direction.clone(),
                approved_on: date(start),
                ratio,
                par_value_before: Money::from_cents(100),
                par_value_after: Money::from_cents(after),
                registered_capital_at_approval: capital,
            },
        )
        .unwrap();
    saved
        .company_state
        .actions_for_test_mut()
        .share_splits
        .push(
            ShareSplitBook::new(ShareSplitEventPlan {
                event_id: "opening-split".into(),
                approval_reference: "游戏整数换算批准".into(),
                issuer: CompanyId(0),
                stock: crate::StockCode(STOCK.into()),
                exchange: crate::calendar::CalendarExchange::Sse,
                direction,
                ratio,
                approved_on: date(start),
                announced_on: date(start),
                registered_on: date(start),
                ex_rights_on: date(ex),
            })
            .unwrap(),
        );
    restore_with_three_groups(&saved)
}

#[test]
fn native_split_and_consolidation_restore_first_sale_and_real_close() {
    for direction in [ShareSplitDirection::Split, ShareSplitDirection::Consolidate] {
        let mut session = split_session(direction.clone(), 2);
        super::stock_opening_runtime_tests::trade(
            &mut session,
            AccountId(1),
            AccountId(0),
            1000,
            200,
        );
        let before = session
            .account(AccountId(0))
            .unwrap()
            .position(&StockId(0))
            .unwrap()
            .invested_cents();
        session.end_civil_day().unwrap();
        let shares = if direction == ShareSplitDirection::Split {
            400
        } else {
            100
        };
        let reference = if direction == ShareSplitDirection::Split {
            500
        } else {
            2000
        };
        let position = session
            .account(AccountId(0))
            .unwrap()
            .position(&StockId(0))
            .unwrap();
        assert_eq!(position.qty(), shares);
        assert_eq!(position.sellable(), shares);
        assert_eq!(position.invested_cents(), before);
        assert_eq!(
            session.corporate_actions().unwrap().share_splits[0].status(),
            &ShareSplitStatus::Settled
        );
        assert!(session.corporate_actions().unwrap().registries[0]
            .pending_opening()
            .is_some());
        assert_eq!(
            session.state.markets[&StockId(0)].last_close().cents(),
            reference
        );
        let experience = session.state.retail_experience.get(&AccountId(2)).unwrap();
        assert_eq!(
            experience.stocks[&StockId(0)].entry_reference_price,
            Some(Money::from_cents(reference))
        );
        assert_eq!(
            experience.feedback.stocks[&StockId(0)]
                .last_own_observation
                .unwrap()
                .price,
            Money::from_cents(reference)
        );
        let saved = session.save().unwrap();
        let json: crate::SaveSlot =
            serde_json::from_value(serde_json::to_value(&saved).unwrap()).unwrap();
        let mut restored = restore_with_three_groups(&json);
        let mut bad = saved.clone();
        bad.company_state
            .actions_for_test_mut()
            .applied_ex_reference_groups
            .clear();
        assert!(GameSession::restore(&bad).is_err());
        super::stock_opening_runtime_tests::trade(
            &mut restored,
            AccountId(0),
            AccountId(1),
            reference,
            shares,
        );
        restored.end_civil_day().unwrap();
        assert!(restored.corporate_actions().unwrap().registries[0]
            .pending_opening()
            .is_none());
        restore_with_three_groups(&restored.save().unwrap());
    }
}

#[test]
fn native_consolidation_zero_holders_and_projection_abort_are_atomic() {
    let mut session = split_session_on(
        ShareSplitDirection::Consolidate,
        1000,
        "2030-06-07",
        "2030-06-10",
    );
    session.step().unwrap();
    session.end_civil_day().unwrap();
    session.end_civil_day().unwrap();
    let before_saved = session.save().unwrap();
    let before = serde_json::to_value(&before_saved).unwrap();
    let (accounts, desks) = {
        let mut driver = session.runtime_driver.as_ref().unwrap().lock().unwrap();
        (
            driver.world_accounts_snapshot().unwrap(),
            driver.world_public_views().to_vec(),
        )
    };
    let result: Result<(), SessionError> = session.with_prepared_runtime_civil_day(|shadow, _| {
        assert_eq!(
            shadow.corporate_actions().unwrap().registries[0].issued_shares(),
            1
        );
        Err(SessionError::UnsupportedCapitalBoundary(
            "缩股投影主动拒绝".into(),
        ))
    });
    assert!(result.unwrap_err().to_string().contains("缩股投影主动拒绝"));
    assert_eq!(
        serde_json::to_value(session.save().unwrap()).unwrap(),
        before
    );
    {
        let mut driver = session.runtime_driver.as_ref().unwrap().lock().unwrap();
        assert!(!driver.administrative_world_is_terminal());
        assert_eq!(driver.world_accounts_snapshot().unwrap(), accounts);
        assert_eq!(driver.world_public_views(), desks.as_slice());
    }
    session.end_civil_day().unwrap();
    assert_eq!(
        session.corporate_actions().unwrap().registries[0].issued_shares(),
        1
    );
    let saved = session.save().unwrap();
    restore_with_three_groups(&saved);
    for (owner, experience) in &saved.retail_experience {
        let original = &before_saved.retail_experience[owner];
        assert_eq!(
            experience.feedback.failure_events,
            original.feedback.failure_events
        );
        assert_eq!(
            experience.feedback.exit_records,
            original.feedback.exit_records
        );
        if saved.snapshot.accounts[owner]
            .positions
            .get(&StockId(0))
            .is_none_or(|position| position.qty == 0)
        {
            assert!(!experience.feedback.stocks.contains_key(&StockId(0)));
            if let Some(stock) = experience.stocks.get(&StockId(0)) {
                assert!(stock.entry_reference_price.is_none());
                assert!(stock.peak_price_since_entry.is_none());
                assert!(stock.last_buy_order_id.is_none());
                assert!(stock.last_sell_order_id.is_none());
                assert!(stock.cooldown_until_market_minute.is_none());
            }
        }
    }
    assert_eq!(
        saved
            .snapshot
            .accounts
            .values()
            .filter_map(|account| account.positions.get(&StockId(0)))
            .map(|position| position.qty)
            .sum::<u32>(),
        1
    );
}
