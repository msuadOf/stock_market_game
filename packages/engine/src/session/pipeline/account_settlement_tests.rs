use super::account_settlement::{
    apply_session_settlement_transaction, apply_settlement_transaction,
    apply_settlement_transaction_with_beliefs, prepare_settlement_transaction,
    SettlementTransactionError,
};
use super::retail_projection::{RetailProjectionError, RetailProjectionSeen};
use super::{
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, EnvelopeReceipt, FeeComponents,
    JournalRank, ReceiptDelta, ReceiptKind, ReceiptLocalKey, ReceiptSource, ReceiptTransition,
    ResVec,
};
use crate::session::decision_chain::personal_state::BeliefParticipantState;
use crate::session::{account_book::AccountBook, account_paged_map::AccountPagedMap};
use crate::{
    Account, AccountId, AccountKind, Money, OrderId, RetailExperienceState, Side, StockCode,
};
use std::collections::BTreeMap;

fn retail_session(cash: i64) -> crate::GameSession {
    let mut game = crate::GameSession::new(
        crate::session::npc_working_quote_tests::retail_quote_setup(),
        42,
    )
    .unwrap();
    game.state.accounts = accounts(cash);
    game.state.retail_experience = retail();
    game.state.retail_projection_seen = RetailProjectionSeen::default();
    game
}

fn stock() -> StockCode {
    StockCode("600001".to_owned())
}

#[test]
fn session_settlement_records_retail_receipt_order_and_simulation_date_once() {
    let mut session = retail_session(200_000);
    let moment = crate::experience::ExperienceMoment {
        civil_date: session.civil_date(),
        market_minute: session.current_market_minute(),
        trading_day: u64::from(session.state.day),
    };
    let receipt = fill(1, Side::Buy, 17, 100, 100_000);
    apply_session_settlement_transaction(&mut session, &[receipt.clone()]).unwrap();
    let experience = &session.state.retail_experience[&AccountId(1)];
    assert_eq!(experience.feedback.stocks[&stock()].entry_moment, moment);
    assert_eq!(experience.stocks[&stock()].last_buy_order_id, Some(17));
    let before = experience.clone();
    apply_session_settlement_transaction(&mut session, &[receipt]).unwrap();
    assert_eq!(session.state.retail_experience[&AccountId(1)], before);
}

fn fill(index: u64, side: Side, order: u64, qty: u32, gross: i64) -> EnvelopeReceipt {
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: stock(),
        order: OrderId(order),
        side,
    };
    let charged = FeeComponents {
        commission: Money::from_cents(100),
        ..FeeComponents::ZERO
    };
    let gross = Money::from_cents(gross);
    let (delta, deliver_qty, deliver_cash) = match side {
        Side::Buy => (
            ReceiptDelta::sealed(
                ResVec::new(gross.add(charged.total().unwrap()).unwrap(), 0),
                ResVec::ZERO,
                ResVec::ZERO,
            ),
            qty,
            Money::ZERO,
        ),
        Side::Sell => (
            ReceiptDelta::sealed(ResVec::new(Money::ZERO, qty), ResVec::ZERO, ResVec::ZERO),
            0,
            gross.sub(charged.total().unwrap()).unwrap(),
        ),
    };
    EnvelopeReceipt {
        index,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::SealedIntent(index),
            ReceiptTransition {
                envelope: key.clone(),
                ordinal: 0,
            },
        )
        .unwrap(),
        envelope: key,
        kind: ReceiptKind::Fill,
        qty_before: qty,
        qty_after: 0,
        value_before: Money::ZERO,
        value_after: gross,
        delta,
        nominal: charged,
        charged,
        charged_before: FeeComponents::ZERO,
        charged_after: charged,
        deliver_qty,
        deliver_cash,
    }
}

fn normalized_buy_fill_chain(order: u64) -> (EnvelopeReceipt, EnvelopeReceipt) {
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: stock(),
        order: OrderId(order),
        side: Side::Buy,
    };
    let charged = FeeComponents {
        commission: Money::from_cents(100),
        ..FeeComponents::ZERO
    };
    let nominal = FeeComponents {
        commission: Money::from_cents(9_999),
        ..FeeComponents::ZERO
    };
    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::created_at_validation(
            key.clone(),
            Money::from_cents(100_200),
            0,
            EnvelopeAudit {
                limit: Money::from_cents(1_000),
                remaining_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        )],
    )
    .unwrap();
    let mut first = chained_buy_fill(
        &key,
        0,
        100,
        50,
        Money::ZERO,
        Money::from_cents(50_000),
        FeeComponents::ZERO,
        charged,
        nominal,
    );
    ledger.apply(&mut first).unwrap();
    let mut second = chained_buy_fill(
        &key,
        1,
        50,
        0,
        Money::from_cents(50_000),
        Money::from_cents(100_000),
        charged,
        charged,
        nominal,
    );
    ledger.apply(&mut second).unwrap();
    (first[0].clone(), second[0].clone())
}

#[allow(clippy::too_many_arguments)]
fn chained_buy_fill(
    key: &EnvelopeKey,
    source_index: u32,
    qty_before: u32,
    qty_after: u32,
    value_before: Money,
    value_after: Money,
    charged_before: FeeComponents,
    charged: FeeComponents,
    nominal: FeeComponents,
) -> [EnvelopeReceipt; 1] {
    let filled_qty = qty_before.checked_sub(qty_after).unwrap();
    let gross = value_after.sub(value_before).unwrap();
    let charged_after = FeeComponents {
        commission: charged_before.commission.add(charged.commission).unwrap(),
        stamp_tax: charged_before.stamp_tax.add(charged.stamp_tax).unwrap(),
        transfer_fee: charged_before
            .transfer_fee
            .add(charged.transfer_fee)
            .unwrap(),
    };
    [EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::Auction(source_index),
            ReceiptTransition {
                envelope: key.clone(),
                ordinal: 0,
            },
        )
        .unwrap(),
        envelope: key.clone(),
        kind: ReceiptKind::Fill,
        qty_before,
        qty_after,
        value_before,
        value_after,
        delta: ReceiptDelta::sealed(
            ResVec::new(gross.add(charged.total().unwrap()).unwrap(), 0),
            ResVec::ZERO,
            ResVec::new(Money::from_cents(i64::from(qty_after) * 1_002), 0),
        ),
        nominal,
        charged,
        charged_before,
        charged_after,
        deliver_qty: filled_qty,
        deliver_cash: Money::ZERO,
    }]
}

fn retail() -> AccountPagedMap<crate::RetailExperienceState> {
    BTreeMap::from([(
        AccountId(1),
        RetailExperienceState::without_equity_reference(),
    )])
    .into()
}

fn institutional_belief(account: AccountId) -> crate::strategy::BeliefBook {
    struct FixedRng;
    impl crate::strategy::Rng for FixedRng {
        fn next_f64(&mut self) -> f64 {
            0.5
        }
        fn next_range_u32(&mut self, low: u32, _high: u32) -> u32 {
            low
        }
    }

    let weights = crate::strategy::AnalysisWeights::new(0, 2_500, 2_500, 2_500, 2_500).unwrap();
    let analysis = crate::strategy::AnalysisProfile::new(weights, None).unwrap();
    crate::strategy::BeliefBook::new(
        account,
        crate::strategy::StrategyProfile::Institution(crate::strategy::InstitutionStyle::Balanced),
        analysis,
        &mut FixedRng,
    )
}

fn institutional_participant(account: AccountId) -> BeliefParticipantState {
    BeliefParticipantState::new(
        crate::experience::PersonalWatchlist::default(),
        crate::experience::PersonalPriceMemory::default(),
        crate::information::NpcInformationState::new(account),
        institutional_belief(account),
    )
}

fn participant_values(participants: &AccountPagedMap<BeliefParticipantState>) -> serde_json::Value {
    let values: BTreeMap<_, _> = participants
        .iter()
        .map(|(account, participant)| {
            (
                account.0.to_string(),
                (
                    participant.watchlist(),
                    participant.price_memory(),
                    participant.information(),
                    participant.belief(),
                ),
            )
        })
        .collect();
    serde_json::to_value(values).unwrap()
}

#[test]
fn institutional_fill_preserves_same_order_failure_and_rejects_unknown_fee_history_atomically() {
    let mut experience = RetailExperienceState::without_equity_reference();
    let code = stock();
    let moment = crate::experience::ExperienceMoment {
        civil_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
        market_minute: 0,
        trading_day: 0,
    };
    experience
        .record_institutional_fill_dated(
            &code,
            Side::Buy,
            Money::from_cents(1000),
            0,
            100,
            Money::from_cents(100),
            false,
            1,
            moment,
        )
        .unwrap();
    experience
        .stocks
        .get_mut(&code)
        .unwrap()
        .adverse_move_recorded = true;
    experience
        .record_institutional_fill_dated(
            &code,
            Side::Buy,
            Money::from_cents(1000),
            100,
            200,
            Money::ZERO,
            false,
            1,
            moment,
        )
        .unwrap();
    assert!(experience.stocks[&code].adverse_move_recorded);
    experience
        .record_institutional_fill_dated(
            &code,
            Side::Buy,
            Money::from_cents(1000),
            200,
            300,
            Money::from_cents(1),
            false,
            2,
            moment,
        )
        .unwrap();
    assert!(!experience.stocks[&code].adverse_move_recorded);
    assert_eq!(
        experience.feedback.stocks[&code].institutional_fees_paid,
        Some(Money::from_cents(101))
    );
    let mut missing = serde_json::to_value(&experience).unwrap();
    missing["feedback"]["stocks"]["600001"]
        .as_object_mut()
        .unwrap()
        .remove("institutional_fees_paid");
    assert!(serde_json::from_value::<RetailExperienceState>(missing).is_err());
    for previous in [None, Some(Money::from_cents(i64::MAX))] {
        experience
            .feedback
            .stocks
            .get_mut(&code)
            .unwrap()
            .institutional_fees_paid = previous;
        let before = experience.clone();
        assert!(experience
            .record_institutional_fill_dated(
                &code,
                Side::Buy,
                Money::from_cents(1000),
                300,
                400,
                Money::from_cents(1),
                false,
                3,
                moment
            )
            .is_err());
        assert_eq!(experience, before);
    }
}

#[test]
fn institution_fill_updates_only_belief_book_trade_facts_and_is_idempotent() {
    let account = AccountId(1);
    let mut accounts = accounts(200_000);
    let institutional = accounts.get_mut(&account).unwrap();
    institutional.fixture_set_kind(AccountKind::Inst);
    institutional.set_strategy(Box::new(
        crate::strategy::BeliefInstitutionStrategy::new(0.05, 100).unwrap(),
    ));
    let mut books: AccountPagedMap<BeliefParticipantState> =
        BTreeMap::from([(account, institutional_participant(account))]).into();
    let mut retail_experience = AccountPagedMap::default();
    let mut seen = RetailProjectionSeen::default();
    let receipt = fill(1, Side::Buy, 10, 100, 100_000);

    apply_settlement_transaction_with_beliefs(
        &mut accounts,
        &mut retail_experience,
        &mut books,
        &mut seen,
        crate::experience::ExperienceMoment {
            civil_date: crate::calendar::CivilDate::from_ymd(2030, 1, 1).unwrap(),
            market_minute: 10,
            trading_day: 0,
        },
        std::slice::from_ref(&receipt),
        false,
    )
    .unwrap();

    let experience = books.get(&account).unwrap().belief().experience();
    let traded = experience.stocks.get(&stock()).unwrap();
    assert_eq!(traded.last_trade_market_minute, 10);
    assert_eq!(traded.last_buy_order_id, Some(10));
    assert_eq!(traded.last_buy_price, Some(Money::from_cents(1_000)));
    assert_eq!(traded.cooldown_until_market_minute, None);
    assert_eq!(experience.consecutive_failed_buys, 0);
    assert_eq!(experience.reference_equity, None);
    assert_eq!(
        experience.feedback.stocks[&stock()]
            .last_own_observation
            .unwrap()
            .price,
        Money::from_cents(1_000)
    );

    let exit = fill(2, Side::Sell, 11, 100, 120_000);
    apply_settlement_transaction_with_beliefs(
        &mut accounts,
        &mut retail_experience,
        &mut books,
        &mut seen,
        crate::experience::ExperienceMoment {
            civil_date: crate::calendar::CivilDate::from_ymd(2030, 1, 1).unwrap(),
            market_minute: 11,
            trading_day: 0,
        },
        std::slice::from_ref(&exit),
        false,
    )
    .unwrap();
    let experience = books.get(&account).unwrap().belief().experience();
    assert!(experience.feedback.stocks.is_empty());
    assert_eq!(experience.feedback.exit_records.len(), 1);
    assert!(
        matches!(experience.feedback.exit_records.last(), Some(record)
        if record.order_id == Some(11)
            && record.cooldown_until_market_minute.is_none()
            && record.realized_profit)
    );
    let before_replay = experience.clone();

    apply_settlement_transaction_with_beliefs(
        &mut accounts,
        &mut retail_experience,
        &mut books,
        &mut seen,
        crate::experience::ExperienceMoment {
            civil_date: crate::calendar::CivilDate::from_ymd(2030, 1, 1).unwrap(),
            market_minute: 11,
            trading_day: 0,
        },
        &[exit],
        false,
    )
    .unwrap();
    assert_eq!(
        books.get(&account).unwrap().belief().experience(),
        &before_replay
    );
    assert!(retail_experience.is_empty());
}

#[test]
fn institutional_exit_profit_requires_net_cash_after_actual_fees() {
    let account = AccountId(1);
    let mut accounts = accounts(200_000);
    let institutional = accounts.get_mut(&account).unwrap();
    institutional.fixture_set_kind(AccountKind::Inst);
    institutional.set_strategy(Box::new(
        crate::strategy::BeliefInstitutionStrategy::new(0.05, 100).unwrap(),
    ));
    let mut books: AccountPagedMap<BeliefParticipantState> =
        BTreeMap::from([(account, institutional_participant(account))]).into();
    let mut retail_experience = AccountPagedMap::default();
    let mut seen = RetailProjectionSeen::default();
    let date = crate::calendar::CivilDate::from_ymd(2030, 1, 1).unwrap();
    for (minute, receipt) in [
        (10, fill(1, Side::Buy, 10, 10, 10_000)),
        (11, fill(2, Side::Sell, 11, 6, 6_120)),
        (12, fill(3, Side::Sell, 12, 4, 4_080)),
    ] {
        apply_settlement_transaction_with_beliefs(
            &mut accounts,
            &mut retail_experience,
            &mut books,
            &mut seen,
            crate::experience::ExperienceMoment {
                civil_date: date,
                market_minute: minute,
                trading_day: 0,
            },
            &[receipt],
            false,
        )
        .unwrap();
    }

    let experience = books.get(&account).unwrap().belief().experience();
    assert_eq!(experience.feedback.exit_records.len(), 1);
    assert!(
        matches!(experience.feedback.exit_records.last(), Some(record)
        if record.order_id == Some(12) && !record.realized_profit)
    );
    assert_eq!(
        accounts.get(&account).unwrap().cash(),
        Money::from_cents(200_000 - 10_100 + 6_020 + 3_980)
    );
    assert_eq!(experience.consecutive_failed_buys, 0);
    assert!(experience.feedback.failure_events.is_empty());
}

#[test]
fn institutional_settlement_records_a_loss_making_sell_when_fees_exceed_proceeds() {
    let account = AccountId(1);
    let mut accounts = accounts(200_000);
    let institutional = accounts.get_mut(&account).unwrap();
    institutional.fixture_set_kind(AccountKind::Inst);
    institutional.set_strategy(Box::new(
        crate::strategy::BeliefInstitutionStrategy::new(0.05, 100).unwrap(),
    ));
    let mut books: AccountPagedMap<BeliefParticipantState> =
        BTreeMap::from([(account, institutional_participant(account))]).into();
    let mut retail_experience = AccountPagedMap::default();
    let mut seen = RetailProjectionSeen::default();
    let date = crate::calendar::CivilDate::from_ymd(2030, 1, 1).unwrap();
    let buy = fill(1, Side::Buy, 10, 1, 100_000);
    apply_settlement_transaction_with_beliefs(
        &mut accounts,
        &mut retail_experience,
        &mut books,
        &mut seen,
        crate::experience::ExperienceMoment {
            civil_date: date,
            market_minute: 10,
            trading_day: 0,
        },
        &[buy],
        false,
    )
    .unwrap();

    let sell = fill(2, Side::Sell, 11, 1, 1);
    apply_settlement_transaction_with_beliefs(
        &mut accounts,
        &mut retail_experience,
        &mut books,
        &mut seen,
        crate::experience::ExperienceMoment {
            civil_date: date,
            market_minute: 11,
            trading_day: 0,
        },
        &[sell],
        false,
    )
    .unwrap();

    assert_eq!(
        accounts.get(&account).unwrap().cash(),
        Money::from_cents(99_801)
    );
    let experience = books.get(&account).unwrap().belief().experience();
    let exit = experience.feedback.exit_records.last().unwrap();
    assert_eq!(exit.order_id, Some(11));
    assert!(!exit.realized_profit);
    assert_eq!(experience.consecutive_failed_buys, 0);
}

#[test]
fn non_belief_institution_account_does_not_require_a_belief_book() {
    let mut accounts = accounts(200_000);
    accounts
        .get_mut(&AccountId(1))
        .unwrap()
        .fixture_set_kind(AccountKind::Inst);
    let mut experience = AccountPagedMap::default();
    let mut seen = RetailProjectionSeen::default();

    apply_settlement_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[fill(1, Side::Buy, 10, 100, 100_000)],
        true,
    )
    .unwrap();
}

fn accounts(cash: i64) -> AccountBook {
    BTreeMap::from([(
        AccountId(1),
        Account::new(AccountId(1), AccountKind::Retail, Money::from_cents(cash)),
    )])
    .into()
}

#[test]
fn settlement_without_new_fills_prepares_no_account_or_retail_state_patch() {
    let accounts = accounts(200_000);
    let experience = retail();
    let seen = RetailProjectionSeen::default();
    let prepared =
        prepare_settlement_transaction(&accounts, &experience, &seen, 10, &[], true).unwrap();

    assert!(prepared.account_patch.is_empty());
    assert!(prepared.retail_patch.is_empty());
    assert!(prepared.output.events.is_empty());
    assert_eq!(prepared.seen, seen);
    assert_eq!(accounts[&AccountId(1)].cash(), Money::from_cents(200_000));
    assert_eq!(experience, retail());
}

#[test]
fn settlement_empty_batch_does_not_repair_an_invalid_watchlist_during_settlement() {
    let mut accounts = accounts(200_000);
    let mut experience = retail();
    for index in 0..9 {
        experience
            .get_mut(&AccountId(1))
            .unwrap()
            .observe_stock(&StockCode(format!("6000{index:02}")), index);
    }
    let mut seen = RetailProjectionSeen::default();
    let prepared =
        prepare_settlement_transaction(&accounts, &experience, &seen, 10, &[], true).unwrap();
    assert!(prepared.account_patch.is_empty());
    assert!(prepared.retail_patch.is_empty());

    apply_settlement_transaction(&mut accounts, &mut experience, &mut seen, 10, &[], true).unwrap();
    assert_eq!(experience[&AccountId(1)].stocks.len(), 9);
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    let template = setup.stocks[0].clone();
    setup.stocks = (0..9)
        .map(|index| {
            let mut stock = template.clone();
            stock.code = StockCode(format!("6000{index:02}"));
            stock
        })
        .collect();
    setup.company_system = simple_company_fixture!(crate; codes = setup.stocks.iter().map(|stock| stock.code.0.as_str()));
    let game = crate::GameSession::new(setup, 42).unwrap();
    let mut save = game.save().unwrap();
    save.retail_experience = experience.to_map();
    assert!(matches!(
        crate::GameSession::restore(&save),
        Err(crate::SessionError::InvalidSave(reason)) if reason.contains("unheld watchlist limit")
    ));
}

#[test]
fn settlement_final_sell_prunes_only_the_affected_retail_account() {
    let mut accounts = accounts(200_000);
    accounts
        .get_mut(&AccountId(1))
        .unwrap()
        .grant_position(stock(), 100, Money::from_cents(1_000))
        .unwrap();
    let mut experience = retail();
    let state = experience.get_mut(&AccountId(1)).unwrap();
    for index in 0..8 {
        state.observe_stock(&StockCode(format!("6010{index:02}")), index);
    }
    state
        .initialize_holding_dated(
            &stock(),
            Some(Money::from_cents(1_000)),
            Money::from_cents(1_000),
            crate::experience::ExperienceMoment {
                civil_date: crate::CivilDate::from_iso("2030-01-01").unwrap(),
                market_minute: 0,
                trading_day: 0,
            },
        )
        .unwrap();
    let mut seen = RetailProjectionSeen::default();

    let result = apply_settlement_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[fill(1, Side::Sell, 10, 100, 100_000)],
        true,
    )
    .unwrap();

    assert_eq!(result.settlement.applied_receipts, 1);
    assert!(!accounts[&AccountId(1)].positions().contains_key(&stock()));
    assert_eq!(experience[&AccountId(1)].stocks.len(), 8);
    assert!(experience[&AccountId(1)].stocks.contains_key(&stock()));
    assert!(!experience[&AccountId(1)]
        .stocks
        .contains_key(&StockCode("601000".to_owned())));
}

#[test]
fn non_fill_receipt_advances_seen_without_changing_accounts_or_experience() {
    let mut receipt = fill(1, Side::Buy, 10, 100, 100_000);
    receipt.kind = ReceiptKind::Release;
    receipt.qty_after = receipt.qty_before;
    receipt.value_after = receipt.value_before;
    receipt.delta = ReceiptDelta::sealed(ResVec::ZERO, ResVec::ZERO, ResVec::ZERO);
    receipt.nominal = FeeComponents::ZERO;
    receipt.charged = FeeComponents::ZERO;
    receipt.charged_after = FeeComponents::ZERO;
    receipt.deliver_qty = 0;
    let mut accounts = accounts(200_000);
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let before_cash = accounts[&AccountId(1)].cash();
    let before_experience = experience.clone();

    for _ in 0..2 {
        let result = apply_settlement_transaction(
            &mut accounts,
            &mut experience,
            &mut seen,
            10,
            &[receipt.clone()],
            true,
        )
        .unwrap();
        assert_eq!(result.settlement.applied_receipts, 0);
        assert!(result.events.is_empty());
        assert_eq!(seen.len(), 1);
        assert_eq!(accounts[&AccountId(1)].cash(), before_cash);
        assert!(accounts[&AccountId(1)].positions().is_empty());
        assert_eq!(experience, before_experience);
    }
}

#[test]
fn session_settlement_consumes_shadow_owned_seen_and_is_idempotent() {
    let receipt = fill(1, Side::Buy, 10, 100, 100_000);
    let mut game = retail_session(200_000);

    let first =
        apply_session_settlement_transaction(&mut game, std::slice::from_ref(&receipt)).unwrap();
    let after_first = game.business_state_hash().unwrap();
    let second = apply_session_settlement_transaction(&mut game, &[receipt]).unwrap();

    assert_eq!(first.settlement.applied_receipts, 1);
    assert_eq!(second.settlement.applied_receipts, 0);
    assert!(second.events.is_empty());
    assert_eq!(game.business_state_hash().unwrap(), after_first);
}

#[test]
fn session_settlement_failure_keeps_all_authoritative_containers_unchanged() {
    let mut receipt = fill(1, Side::Buy, 10, 100, 100_000);
    receipt.value_after = Money::ZERO;
    let mut game = retail_session(200_000);
    let before = game.business_state_hash().unwrap();

    assert!(apply_session_settlement_transaction(&mut game, &[receipt]).is_err());

    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn session_settlement_uses_the_sessions_t1_policy() {
    for (t1_enabled, expected_locked) in [(false, 0), (true, 100)] {
        let mut game = retail_session(200_000);
        game.state.setup.t1_enabled = t1_enabled;

        apply_session_settlement_transaction(&mut game, &[fill(1, Side::Buy, 10, 100, 100_000)])
            .unwrap();

        let position = &game.state.accounts[&AccountId(1)].positions()[&stock()];
        assert_eq!(position.qty(), 100);
        assert_eq!(position.t1_locked(), expected_locked);
    }
}

#[test]
fn session_settlement_records_the_sessions_nonzero_market_minute() {
    let mut game = retail_session(200_000);
    game.state.tick = 1;
    let market_minute = game.current_market_minute();
    assert!(market_minute > 0);

    apply_session_settlement_transaction(&mut game, &[fill(1, Side::Buy, 10, 100, 100_000)])
        .unwrap();

    let experience = &game.state.retail_experience[&AccountId(1)].stocks[&stock()];
    assert_eq!(experience.last_trade_market_minute, market_minute);
    assert_eq!(experience.last_observed_market_minute, market_minute);
}

#[test]
fn duplicate_receipt_is_idempotent_for_accounts_experience_and_seen() {
    let receipt = fill(1, Side::Buy, 10, 100, 100_000);
    let mut accounts = accounts(200_000);
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let first = apply_settlement_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        std::slice::from_ref(&receipt),
        true,
    )
    .unwrap();
    let cash_after_first = accounts[&AccountId(1)].cash();
    let experience_after_first = experience.clone();
    let second = apply_settlement_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        11,
        &[receipt],
        true,
    )
    .unwrap();
    assert_eq!(first.settlement.applied_receipts, 1);
    assert_eq!(second.settlement.applied_receipts, 0);
    assert!(second.events.is_empty());
    assert_eq!(accounts[&AccountId(1)].cash(), cash_after_first);
    assert_eq!(experience, experience_after_first);
}

#[test]
fn projection_failure_rolls_back_successful_settlement() {
    let mut receipt = fill(1, Side::Buy, 10, 100, 100_000);
    receipt.value_after = Money::ZERO;
    let mut accounts = accounts(200_000);
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let before_cash = accounts[&AccountId(1)].cash();
    let before_experience = experience.clone();
    assert!(apply_settlement_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[receipt],
        true
    )
    .is_err());
    assert_eq!(accounts[&AccountId(1)].cash(), before_cash);
    assert_eq!(experience, before_experience);
    assert!(seen.is_empty());
}

#[test]
fn same_order_multi_leg_receipts_settle_once_and_project_one_event() {
    let mut accounts = accounts(200_000);
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let result = apply_settlement_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &{
            let (first, second) = normalized_buy_fill_chain(10);
            [second, first]
        },
        true,
    )
    .unwrap();
    assert_eq!(result.settlement.applied_receipts, 2);
    assert_eq!(result.events.len(), 1);
    assert_eq!(accounts[&AccountId(1)].positions()[&stock()].qty(), 100);
}

#[test]
fn same_account_buy_then_sell_preserves_the_buy_before_sell_lifecycle() {
    let mut accounts = accounts(300_000);
    accounts
        .get_mut(&AccountId(1))
        .unwrap()
        .grant_position(stock(), 100, Money::from_cents(1_000))
        .unwrap();
    let mut experience = retail();
    experience
        .get_mut(&AccountId(1))
        .unwrap()
        .initialize_holding_dated(
            &stock(),
            Some(Money::from_cents(1_000)),
            Money::from_cents(1_000),
            crate::experience::ExperienceMoment {
                civil_date: crate::CivilDate::from_iso("2030-01-01").unwrap(),
                market_minute: 0,
                trading_day: 0,
            },
        )
        .unwrap();
    let mut seen = RetailProjectionSeen::default();
    apply_settlement_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[
            fill(2, Side::Sell, 21, 100, 100_000),
            fill(1, Side::Buy, 20, 100, 100_000),
        ],
        true,
    )
    .unwrap();
    let position = &accounts[&AccountId(1)].positions()[&stock()];
    assert_eq!(position.qty(), 100);
    assert_eq!(position.t1_locked(), 100);
}

#[test]
fn missing_settlement_account_is_typed_and_leaves_every_container_unchanged() {
    let receipt = fill(1, Side::Buy, 10, 100, 100_000);
    let mut accounts = AccountBook::default();
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let before_experience = experience.clone();
    let error = apply_settlement_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[receipt],
        true,
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("settlement account 1 is missing"));
    assert!(accounts.is_empty());
    assert_eq!(experience, before_experience);
    assert!(seen.is_empty());
}

#[test]
fn retail_fill_without_experience_is_typed_and_rolls_back_every_container() {
    let receipt = fill(1, Side::Buy, 10, 100, 100_000);
    let mut accounts: AccountBook = BTreeMap::from([(
        AccountId(1),
        Account::new(
            AccountId(1),
            AccountKind::Retail,
            Money::from_cents(200_000),
        ),
    )])
    .into();
    let mut experience = AccountPagedMap::<crate::RetailExperienceState>::default();
    let mut seen = RetailProjectionSeen::default();
    let before_cash = accounts[&AccountId(1)].cash();

    let error = apply_settlement_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[receipt],
        true,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        SettlementTransactionError::Projection(RetailProjectionError::MissingRetailExperience {
            account: AccountId(1),
        })
    ));
    assert_eq!(accounts[&AccountId(1)].cash(), before_cash);
    assert!(accounts[&AccountId(1)].positions().is_empty());
    assert!(experience.is_empty());
    assert!(seen.is_empty());
}

#[test]
fn later_retail_account_failure_does_not_commit_an_earlier_accounts_settlement() {
    let first = fill(1, Side::Buy, 10, 100, 100_000);
    let mut second = fill(2, Side::Buy, 11, 100, 100_000);
    second.envelope.account = AccountId(2);
    second.local_key = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(2),
        ReceiptTransition {
            envelope: second.envelope.clone(),
            ordinal: 0,
        },
    )
    .unwrap();
    let mut accounts: AccountBook = BTreeMap::from([
        (
            AccountId(1),
            Account::new(
                AccountId(1),
                AccountKind::Retail,
                Money::from_cents(200_000),
            ),
        ),
        (
            AccountId(2),
            Account::new(
                AccountId(2),
                AccountKind::Retail,
                Money::from_cents(200_000),
            ),
        ),
    ])
    .into();
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let experience_before = experience.clone();
    let seen_before = seen.clone();

    let error = apply_settlement_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[first, second],
        true,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        SettlementTransactionError::Projection(RetailProjectionError::MissingRetailExperience {
            account: AccountId(2)
        })
    ));
    for account in accounts.values() {
        assert_eq!(account.cash(), Money::from_cents(200_000));
        assert!(account.positions().is_empty());
    }
    assert_eq!(experience, experience_before);
    assert_eq!(seen, seen_before);
}

#[test]
fn non_retail_fill_settles_without_projecting_even_if_an_experience_entry_exists() {
    let receipt = fill(1, Side::Buy, 10, 100, 100_000);
    let mut accounts: AccountBook = BTreeMap::from([(
        AccountId(1),
        Account::new(
            AccountId(1),
            AccountKind::Player,
            Money::from_cents(200_000),
        ),
    )])
    .into();
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let before_experience = experience.clone();

    let result = apply_settlement_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[receipt],
        true,
    )
    .unwrap();

    assert_eq!(result.settlement.applied_receipts, 1);
    assert!(result.events.is_empty());
    assert_eq!(accounts[&AccountId(1)].positions()[&stock()].qty(), 100);
    assert_eq!(experience, before_experience);
    assert_eq!(seen.len(), 1);
}

#[test]
fn settlement_overflow_rolls_back_every_container() {
    let mut first = fill(1, Side::Buy, 10, 1, i64::MAX - 100);
    first.charged = FeeComponents::ZERO;
    first.charged_after = FeeComponents::ZERO;
    first.delta = ReceiptDelta::sealed(
        ResVec::new(first.value_after, 0),
        ResVec::ZERO,
        ResVec::ZERO,
    );
    let mut second = first.clone();
    second.index = 2;
    second.local_key = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(2),
        ReceiptTransition {
            envelope: second.envelope.clone(),
            ordinal: 0,
        },
    )
    .unwrap();
    let mut accounts = accounts(i64::MAX);
    let mut experience = retail();
    let mut seen = RetailProjectionSeen::default();
    let before_cash = accounts[&AccountId(1)].cash();
    let before_experience = experience.clone();
    assert!(apply_settlement_transaction(
        &mut accounts,
        &mut experience,
        &mut seen,
        10,
        &[first, second],
        true
    )
    .is_err());
    assert_eq!(accounts[&AccountId(1)].cash(), before_cash);
    assert_eq!(experience, before_experience);
    assert!(seen.is_empty());
}

#[test]
fn settlement_plan_account_failure_preserves_all_four_transaction_containers() {
    let first = AccountId(1);
    let second = AccountId(2);
    let mut accounts = accounts(200_000);
    accounts.insert(
        second,
        Account::new(second, AccountKind::Inst, Money::from_cents(200_000)),
    );
    let mut experience = retail();
    let mut books: AccountPagedMap<BeliefParticipantState> =
        [(second, institutional_participant(second))]
            .into_iter()
            .collect();
    let mut seen = RetailProjectionSeen::default();
    let experience_before = experience.clone();
    let books_before = participant_values(&books);
    let seen_before = seen.clone();
    let buy = fill(1, Side::Buy, 10, 100, 100_000);
    let mut sell = fill(2, Side::Sell, 11, 100, 100_000);
    sell.envelope.account = second;
    sell.local_key = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(2),
        ReceiptTransition {
            envelope: sell.envelope.clone(),
            ordinal: 0,
        },
    )
    .unwrap();
    let error = apply_settlement_transaction_with_beliefs(
        &mut accounts,
        &mut experience,
        &mut books,
        &mut seen,
        crate::experience::ExperienceMoment {
            civil_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            market_minute: 10,
            trading_day: 0,
        },
        &[buy, sell],
        true,
    )
    .unwrap_err();
    assert!(matches!(error, SettlementTransactionError::Settlement(_)));
    for account in [first, second] {
        assert_eq!(accounts[&account].cash(), Money::from_cents(200_000));
        assert!(accounts[&account].positions().is_empty());
    }
    assert_eq!(experience, experience_before);
    assert_eq!(participant_values(&books), books_before);
    assert_eq!(seen, seen_before);
}
