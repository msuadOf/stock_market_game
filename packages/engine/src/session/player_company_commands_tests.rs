//! 直接调用真实 Session 命令；不以纯批准候选替代生产受理结果。

use super::*;
use crate::company::cash_dividend_tax::CashDividendTaxMode;
use crate::company::stock_distribution::StockDistributionKind;
use crate::session::company_state::SavedSessionCompanies;
use crate::{AccountId, Money, StockId};
include!("../../test-support/simple_company.rs");

#[path = "player_company_capital_tests.rs"]
mod capital_tests;
#[path = "player_company_repurchase_tests.rs"]
mod repurchase_tests;

fn game(holding: bool) -> GameSession {
    game_with_features(holding, false, false)
}

fn game_with_features(holding: bool, rights: bool, repurchase: bool) -> GameSession {
    game_with_features_on(holding, rights, repurchase, "2030-06-03")
}

fn game_with_features_on(
    holding: bool,
    rights: bool,
    repurchase: bool,
    start: &str,
) -> GameSession {
    game_with_tick_count(holding, rights, repurchase, start, None)
}

fn game_with_tick_count(
    holding: bool,
    rights: bool,
    repurchase: bool,
    start: &str,
    ticks: Option<u64>,
) -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    if let Some(ticks) = ticks {
        setup.ticks_per_day = ticks;
        setup.auction_ticks = 0;
        setup.closing_auction_ticks = 0;
    }
    setup.stocks[0].code = crate::StockCode("600101".into());
    setup.stocks[0].total_shares = 1_000;
    setup.stocks[0].float_shares = 1_000;
    setup.start_date = CivilDate::from_iso(start).unwrap();
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 1;
    setup.company_system = Some(simple_company_fixture!(crate; ["600101"]));
    setup.dividend_tax_mode = Some(CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1_000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup.auto_corporate_foundation = true;
    setup.rights_offering_enabled = rights;
    setup.issuer_repurchase_enabled = repurchase;
    let mut game = GameSession::new_with_runtime_groups(setup, 7, 3).unwrap();
    if holding {
        // 持仓即可的前置包含名册外持仓与 T+1 锁定股；此处不声称发生过市场成交。
        game.fixture_account_mut(&AccountId(0))
            .unwrap()
            .fixture_set_positions(std::collections::BTreeMap::from([(
                StockId(0),
                crate::account::Position::from_restored_parts(1, 1, 100, 0),
            )]));
    }
    game
}

fn cash(gross: i64) -> PlayerCompanyProposal {
    PlayerCompanyProposal::CashDividend {
        company: CompanyId(0),
        gross_per_share: Money::from_cents(gross),
    }
}

fn rights() -> PlayerCompanyProposal {
    PlayerCompanyProposal::RightsOffering {
        company: CompanyId(0),
        price_per_share: Money::from_cents(100),
        shares_per_existing_share_micros: 100_000,
        payment_days: 1,
    }
}

fn secondary() -> PlayerCompanyProposal {
    PlayerCompanyProposal::SecondaryOffering {
        company: CompanyId(0),
        price_per_share: Money::from_cents(100),
        shares: 100,
    }
}

fn repurchase() -> PlayerCompanyProposal {
    PlayerCompanyProposal::IssuerRepurchase {
        company: CompanyId(0),
        price_cap_per_share: Money::from_cents(100),
        total_budget: Money::from_cents(10_000),
        max_shares: 100,
        window_trading_days: 1,
        purpose: crate::company::issuer_repurchase::RepurchasePurpose::ReduceCapital,
    }
}

fn split() -> PlayerCompanyProposal {
    PlayerCompanyProposal::ShareSplit {
        company: CompanyId(0),
        direction: crate::company::share_split::ShareSplitDirection::Split,
        ratio: 2,
    }
}

#[test]
fn player_command_rights_approves_actual_plan_without_debiting_or_issuing() {
    use crate::company::rights_offering::{RightsOfferingMode, RightsOfferingStatus};
    let mut game = game_with_features(true, true, false);
    let before = game.save().unwrap();
    let receipt = game
        .propose_player_company_action(AccountId(0), rights())
        .unwrap();
    assert_eq!(receipt.kind, PlayerProposalKind::RightsOffering);
    let actions = game.corporate_actions().unwrap();
    assert_eq!(actions.rights_offerings.len(), 1);
    let book = &actions.rights_offerings[0];
    assert_eq!(book.plan().event_id, receipt.event_id);
    assert_eq!(book.plan().approved_on, receipt.approved_on);
    assert_eq!(book.status(), &RightsOfferingStatus::Approved);
    assert_eq!(
        book.plan().mode,
        RightsOfferingMode::RightsToAllShareholders {
            shares_per_existing_share_micros: 100_000,
        }
    );
    assert_eq!(
        book.plan().payment_start_on,
        book.plan().payment_deadline_on
    );
    assert!(book.registration().is_none());
    assert!(book.subscriptions().is_empty());
    assert!(book.settlement().is_none());
    let facts = game
        .company_system()
        .unwrap()
        .rights_offering_facts(&CompanyId(0))
        .unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].event_id, receipt.event_id);
    assert!(facts[0].settled_on.is_none());
    assert_eq!(facts[0].issued_shares, 0);
    assert_eq!(
        serde_json::to_value(game.save().unwrap().snapshot).unwrap(),
        serde_json::to_value(before.snapshot).unwrap()
    );
    assert!(game.runtime_driver.is_some());
}

#[test]
fn player_command_secondary_targets_requester_without_debiting_or_issuing() {
    use crate::company::rights_offering::{
        DirectedPlacementTarget, RightsOfferingMode, RightsOfferingStatus,
    };
    use crate::company::share_registry::HolderId;
    let mut game = game_with_features(true, true, false);
    let before = game.save().unwrap();
    let receipt = game
        .propose_player_company_action(AccountId(0), secondary())
        .unwrap();
    assert_eq!(receipt.kind, PlayerProposalKind::SecondaryOffering);
    let actions = game.corporate_actions().unwrap();
    assert_eq!(actions.rights_offerings.len(), 1);
    let book = &actions.rights_offerings[0];
    assert_eq!(book.plan().event_id, receipt.event_id);
    assert_eq!(book.status(), &RightsOfferingStatus::Approved);
    assert_eq!(
        book.plan().mode,
        RightsOfferingMode::DirectedPlacement {
            targets: vec![DirectedPlacementTarget::NamedHolder {
                holder: HolderId::Account(AccountId(0)),
                shares: 100,
                lock_until: None,
            }],
        }
    );
    assert_eq!(
        book.plan().payment_start_on,
        book.plan().payment_deadline_on
    );
    assert!(book.registration().is_none());
    assert!(book.subscriptions().is_empty());
    assert_eq!(
        serde_json::to_value(game.save().unwrap().snapshot).unwrap(),
        serde_json::to_value(before.snapshot).unwrap()
    );
    assert_eq!(
        game.company_system()
            .unwrap()
            .issuers()
            .get(&CompanyId(0))
            .unwrap()
            .issued_shares,
        1_000
    );
    assert!(game.runtime_driver.is_some());
}

#[test]
fn player_command_split_freezes_finance_fact_without_changing_share_units() {
    use crate::company::share_split::ShareSplitStatus;
    let mut game = game(true);
    let before = game.save().unwrap();
    let receipt = game
        .propose_player_company_action(AccountId(0), split())
        .unwrap();
    assert_eq!(receipt.kind, PlayerProposalKind::ShareSplit);
    let actions = game.corporate_actions().unwrap();
    assert_eq!(actions.share_splits.len(), 1);
    let book = &actions.share_splits[0];
    assert_eq!(book.plan().event_id, receipt.event_id);
    assert_eq!(book.status(), &ShareSplitStatus::Approved);
    assert_eq!(book.plan().ratio, 2);
    let system = game.company_system().unwrap();
    let facts = system.share_split_facts(&CompanyId(0)).unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].event_id, receipt.event_id);
    assert_eq!(facts[0].par_value_before, Money::from_cents(100));
    assert_eq!(facts[0].par_value_after, Money::from_cents(50));
    assert!(facts[0].settled_on.is_none());
    assert_eq!(
        system.issuers().get(&CompanyId(0)).unwrap().issued_shares,
        1_000
    );
    assert_eq!(
        serde_json::to_value(game.save().unwrap().snapshot).unwrap(),
        serde_json::to_value(before.snapshot).unwrap()
    );
    assert!(game.runtime_driver.is_some());
}

#[test]
fn player_command_repurchase_funds_real_special_account_without_fabricated_fills() {
    use crate::company::issuer_repurchase::IssuerRepurchaseStatus;
    let mut game = game_with_features(true, false, true);
    let special = game
        .account_roster()
        .into_iter()
        .find(|account| game.account(*account).unwrap().kind() == AccountKind::IssuerRepurchase)
        .expect("启用回购的新局必须已创建真实专户");
    let before = game.save().unwrap();
    assert_eq!(before.snapshot.accounts[&special].cash, Money::ZERO);
    let receipt = game
        .propose_player_company_action(AccountId(0), repurchase())
        .unwrap();
    assert_eq!(receipt.kind, PlayerProposalKind::IssuerRepurchase);
    let actions = game.corporate_actions().unwrap();
    assert_eq!(actions.issuer_repurchases.len(), 1);
    let book = &actions.issuer_repurchases[0];
    assert_eq!(book.plan().event_id, receipt.event_id);
    assert_eq!(book.status(), &IssuerRepurchaseStatus::Approved);
    assert!(book.fills().is_empty());
    assert!(book.completed_on().is_none());
    assert_eq!(book.cancelled_shares(), 0);
    let facts = game
        .company_system()
        .unwrap()
        .issuer_repurchase_facts(&CompanyId(0))
        .unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].event_id, receipt.event_id);
    assert_eq!(facts[0].synthetic_funding.cents(), 10_000);
    assert!(facts[0].spent.is_none());
    let after = game.save().unwrap();
    let mut expected = before.snapshot;
    expected.accounts.get_mut(&special).unwrap().cash = Money::from_cents(10_000);
    assert_eq!(
        serde_json::to_value(&after.snapshot).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    // 必须读真实组账户；仅改 Session 镜像的批准不能通过此正控。
    let mut driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
    let raw = driver.world_accounts_snapshot().unwrap();
    let driver_ref = &*driver;
    let mut special_accounts = raw.iter().flat_map(|(group, accounts)| {
        accounts
            .iter()
            .enumerate()
            .filter_map(move |(local, account)| {
                (driver_ref
                    .account_by_owner((*group, crate::runtime::LocalIdx::try_from(local).unwrap()))
                    == Some(special))
                .then_some(account)
            })
    });
    assert_eq!(
        special_accounts.next().unwrap(),
        &crate::runtime::group::GroupAccount::new(10_000, expected.markets.len())
    );
    assert!(special_accounts.next().is_none());
    drop(driver);
    assert_eq!(
        game.company_system()
            .unwrap()
            .issuers()
            .get(&CompanyId(0))
            .unwrap()
            .issued_shares,
        1_000
    );
    assert!(game.runtime_driver.is_some());
}

#[test]
fn player_command_disabled_mechanisms_reject_before_runtime_or_any_state_change() {
    for proposal in [rights(), secondary(), repurchase()] {
        let mut game = game(true);
        let before = serde_json::to_value(game.save().unwrap()).unwrap();
        let error = game
            .propose_player_company_action(AccountId(0), proposal)
            .unwrap_err();
        assert!(error.to_string().contains("本局未启用"), "{error}");
        assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
        assert!(game.runtime_driver.is_none());
        assert!(game.require_healthy().is_ok());
    }
}

#[test]
fn player_command_cash_commits_actual_plan_once_without_cash_or_personal_changes() {
    let mut game = game(true);
    let before = game.save().unwrap();
    let receipt = game
        .propose_player_company_action(AccountId(0), cash(1))
        .unwrap();
    let after = game.save().unwrap();
    assert_eq!(receipt.account, AccountId(0));
    assert_eq!(receipt.company, CompanyId(0));
    assert_eq!(receipt.approved_on, game.civil_date());
    let actions = game.corporate_actions().unwrap();
    assert_eq!(actions.dividends.len(), 1);
    assert_eq!(actions.dividends[0].plan().plan_id, receipt.event_id);
    assert_eq!(actions.dividends[0].plan().approved_on, receipt.approved_on);
    assert_eq!(
        serde_json::to_value(&after.snapshot).unwrap(),
        serde_json::to_value(&before.snapshot).unwrap()
    );
    assert_eq!(after.rng_state, before.rng_state);
    assert_eq!(after.npc_attention, before.npc_attention);
    assert_eq!(after.public_library, before.public_library);
    assert_eq!(after.information_states, before.information_states);
    assert_eq!(after.belief_books, before.belief_books);
    let encoded = serde_json::to_string(&receipt).unwrap();
    assert_eq!(
        serde_json::from_str::<PlayerCompanyCommandReceipt>(&encoded).unwrap(),
        receipt
    );
    assert!(
        game.runtime_driver.is_some(),
        "必须经真实 native World，不以 helper 当受理"
    );
}

#[test]
fn player_command_stock_commits_actual_plan_without_crediting_new_shares() {
    let mut game = game(true);
    let before = serde_json::to_value(game.save().unwrap().snapshot).unwrap();
    let receipt = game
        .propose_player_company_action(
            AccountId(0),
            PlayerCompanyProposal::StockDistribution {
                company: CompanyId(0),
                kind: StockDistributionKind::BonusShares,
                shares_per_existing_share_micros: 100_000,
            },
        )
        .unwrap();
    let actions = game.corporate_actions().unwrap();
    assert_eq!(actions.stock_distributions.len(), 1);
    assert_eq!(
        actions.stock_distributions[0].plan().event_id,
        receipt.event_id
    );
    assert_eq!(
        game.account(AccountId(0))
            .unwrap()
            .position(&StockId(0))
            .unwrap()
            .qty(),
        1
    );
    assert_eq!(
        serde_json::to_value(game.save().unwrap().snapshot).unwrap(),
        before
    );
    assert!(game
        .company_system()
        .unwrap()
        .stock_distribution_facts(&CompanyId(0))
        .unwrap()[0]
        .credited_on
        .is_none());
}

#[test]
fn player_command_stock_after_actual_settlement_uses_current_registry_without_capture() {
    let mut game = game_with_tick_count(false, false, false, "2030-06-03", Some(1));
    game.seed_order_for_test(
        AccountId(1),
        crate::Intent::PlaceLimit {
            code: StockId(0),
            side: crate::Side::Sell,
            price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
        &mut Vec::new(),
    );
    game.enqueue_player_intent(
        AccountId(0),
        crate::Intent::PlaceLimit {
            code: StockId(0),
            side: crate::Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
    )
    .unwrap();
    game.step().unwrap();
    game.end_civil_day().unwrap();
    let before = game.corporate_actions().unwrap().registries[0].clone();
    assert!(before.receipts().iter().any(|receipt| matches!(
        receipt.request.scope,
        crate::company::share_registry::MovementScope::SettlementDayMarket { .. }
    )));
    let receipt = game
        .propose_player_company_action(
            AccountId(0),
            PlayerCompanyProposal::StockDistribution {
                company: CompanyId(0),
                kind: StockDistributionKind::BonusShares,
                shares_per_existing_share_micros: 100_000,
            },
        )
        .unwrap();
    let actions = game.corporate_actions().unwrap();
    assert_eq!(actions.registries[0], before);
    assert!(actions.registries[0]
        .registration(&receipt.event_id)
        .is_none());
    assert_eq!(
        actions.stock_distributions[0].plan().event_id,
        receipt.event_id
    );
}

#[test]
fn player_command_unknown_nonplayer_and_zero_holding_preserve_the_full_session() {
    for account in [AccountId(999), AccountId(1), AccountId(0)] {
        let mut game = game(false);
        let before = serde_json::to_value(game.save().unwrap()).unwrap();
        let error = game
            .propose_player_company_action(account, cash(1))
            .unwrap_err();
        assert_eq!(error.account(), account);
        assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
        assert!(
            game.runtime_driver.is_none(),
            "前置拒绝不能启动 World 或消费队列"
        );
    }
}

#[test]
fn player_command_second_approval_consumes_current_profit_and_never_overwrites_first() {
    let mut game = game(true);
    let available = game
        .company_system()
        .unwrap()
        .distributable_profit(&CompanyId(0))
        .unwrap()
        .available_for_distribution
        .cents();
    let per_share = i64::try_from(available / 1_000 * 3 / 4).unwrap();
    assert!(per_share > 0);
    let first = game
        .propose_player_company_action(AccountId(0), cash(per_share))
        .unwrap();
    let before = serde_json::to_value(game.save().unwrap()).unwrap();
    assert!(game
        .propose_player_company_action(AccountId(0), cash(per_share))
        .is_err());
    assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
    assert_eq!(
        game.corporate_actions().unwrap().dividends[0]
            .plan()
            .plan_id,
        first.event_id
    );
}

#[test]
fn player_command_rejects_revision_overflow_before_any_world_commit() {
    for revision in [u64::MAX - 1, u64::MAX] {
        let mut game = game(true);
        game.revision
            .store(revision, std::sync::atomic::Ordering::Relaxed);
        let before = serde_json::to_value(game.save().unwrap()).unwrap();
        assert!(game
            .propose_player_company_action(AccountId(0), cash(1))
            .is_err());
        assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
        assert!(game.runtime_driver.is_none());
        assert_eq!(game.external_revision(), revision);
        assert!(game.require_healthy().is_ok());
    }
    for already_constructed in [false, true] {
        let mut game = game(true);
        if already_constructed {
            game.construct_runtime_driver_lazy().unwrap();
        }
        let remaining = if already_constructed { 1 } else { 2 };
        game.revision
            .store(u64::MAX - remaining, std::sync::atomic::Ordering::Relaxed);
        let receipt = game
            .propose_player_company_action(AccountId(0), cash(1))
            .unwrap();
        assert_eq!(game.external_revision(), u64::MAX);
        assert_eq!(
            game.corporate_actions().unwrap().dividends[0]
                .plan()
                .plan_id,
            receipt.event_id
        );
        assert!(game.require_healthy().is_ok());
    }
}

#[test]
fn player_command_keeps_the_selected_company_state_variant() {
    let mut game = game(true);
    game.propose_player_company_action(AccountId(0), cash(1))
        .unwrap();
    assert!(matches!(
        game.save().unwrap().company_state,
        SavedSessionCompanies::CompanySystem { .. }
    ));
}

#[test]
fn player_command_commit_protocol_failure_seals_the_first_session_fatal() {
    terminal_failure(false);
}

#[test]
fn player_command_transport_failure_seals_the_first_session_fatal() {
    terminal_failure(true);
}

fn terminal_failure(transport: bool) {
    use std::error::Error;
    let mut game = game(true);
    game.construct_runtime_driver_lazy().unwrap();
    {
        let mut driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
        if transport {
            driver.disconnect_company_approval_for_test();
        } else {
            driver.fail_company_approval_commit_for_test();
        }
    }
    let companies = serde_json::to_value(game.state.companies.save()).unwrap();
    let player = game.account(AccountId(0)).unwrap().clone();
    let revision = game.external_revision();
    let first = game
        .propose_player_company_action(AccountId(0), cash(1))
        .unwrap_err();
    assert!(
        first.source().is_some(),
        "首次拒绝必须保留真实 RuntimeError 原因"
    );
    let fatal = game
        .poison_reason()
        .expect("World terminal 必须同步封闭 Session 健康门")
        .clone();
    assert_eq!(game.require_healthy().unwrap_err(), fatal);
    assert!(matches!(game.save(), Err(crate::SessionError::Step(actual)) if actual == fatal));
    assert_eq!(game.step().unwrap_err(), fatal);
    assert!(
        matches!(game.end_civil_day(), Err(crate::SessionError::Step(ref error)) if error == &fatal)
    );
    let repeated = game
        .propose_player_company_action(AccountId(0), cash(1))
        .unwrap_err();
    assert_eq!(
        repeated
            .source()
            .unwrap()
            .downcast_ref::<crate::session::StepFatal>(),
        Some(&fatal)
    );
    assert_eq!(
        game.poison_reason(),
        Some(&fatal),
        "后续调用不能替换首次 terminal 原因"
    );
    assert_eq!(
        serde_json::to_value(game.state.companies.save()).unwrap(),
        companies
    );
    assert_eq!(game.account(AccountId(0)).unwrap().cash(), player.cash());
    assert_eq!(
        serde_json::to_value(game.account(AccountId(0)).unwrap().positions()).unwrap(),
        serde_json::to_value(player.positions()).unwrap()
    );
    assert_eq!(
        game.external_revision(),
        revision,
        "拒绝不得安装批准或消费 revision"
    );
}

#[test]
fn player_command_info_inspect_rejection_leaves_the_session_usable() {
    let mut game = game(true);
    game.construct_runtime_driver_lazy().unwrap();
    let payload = crate::runtime::world::InformationPreparePayload {
        price_unit_changes: Vec::new(),
        library: std::sync::Arc::clone(&game.state.library),
        current_facts: std::sync::Arc::new(game.state.company_read_facts().unwrap()),
        observation: None,
    };
    let before = serde_json::to_value(game.save().unwrap()).unwrap();
    {
        let mut driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
        assert!(driver.commit_company_approval(payload, || false).is_err());
    }
    assert_eq!(
        game.poison_reason(),
        None,
        "inspect 业务拒绝不能冒 terminal"
    );
    assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
    let accepted = game
        .propose_player_company_action(AccountId(0), cash(1))
        .unwrap();
    assert_eq!(
        game.corporate_actions().unwrap().dividends[0]
            .plan()
            .plan_id,
        accepted.event_id
    );
}

#[test]
fn player_command_poisoned_driver_lock_is_terminal() {
    let mut game = game(true);
    game.construct_runtime_driver_lazy().unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = game.runtime_driver.as_ref().unwrap().lock().unwrap();
        panic!("真实 driver 锁 poison 测试");
    }));
    assert!(result.is_err());
    let before = serde_json::to_value(game.state.companies.save()).unwrap();
    assert!(game
        .propose_player_company_action(AccountId(0), cash(1))
        .is_err());
    let fatal = game.poison_reason().unwrap();
    assert!(matches!(game.save(), Err(crate::SessionError::Step(actual)) if &actual == fatal));
    assert_eq!(
        serde_json::to_value(game.state.companies.save()).unwrap(),
        before
    );
}

#[test]
fn player_command_driver_tick_mismatch_is_terminal() {
    let mut game = game(true);
    game.construct_runtime_driver_lazy().unwrap();
    game.state.tick += 1;
    let before = serde_json::to_value(game.state.companies.save()).unwrap();
    assert!(game
        .propose_player_company_action(AccountId(0), cash(1))
        .is_err());
    let fatal = game.poison_reason().unwrap();
    assert!(matches!(game.save(), Err(crate::SessionError::Step(actual)) if &actual == fatal));
    assert_eq!(
        serde_json::to_value(game.state.companies.save()).unwrap(),
        before
    );
}

#[path = "player_company_classification_tests.rs"]
mod classification_tests;

#[test]
fn rights_subscription_rejects_zero_shares_without_queue_mutation() {
    let mut game = game_with_features(true, true, false);
    let before = game
        .corporate_actions()
        .unwrap()
        .rights_subscription_queue
        .clone();
    let result = game.subscribe_rights_offering(AccountId(0), "event-1", 0);
    assert!(matches!(
        result,
        Err(crate::session::RightsSubscriptionError::InvalidQuantity)
    ));
    assert_eq!(
        game.corporate_actions().unwrap().rights_subscription_queue,
        before
    );
}

#[test]
fn pending_rights_subscription_cannot_be_saved_as_unrestorable_public_slot() {
    let mut game = game_with_features(false, true, false);
    let submitted_on = game.civil_date();
    let crate::session::company_state::SessionCompanies::CompanySystem(state) =
        &mut game.state.companies
    else {
        panic!("测试 fixture 使用 CompanySystem")
    };
    state.corporate_actions.rights_subscription_queue.push(
        crate::session::corporate_actions::QueuedRightsSubscription {
            event_id: "pending-rights".into(),
            requested_shares: 1,
            submitted_on,
            account: AccountId(0),
        },
    );
    assert!(matches!(
        game.save(),
        Err(crate::session::SessionError::PendingRightsSubscriptions)
    ));
}

#[test]
fn cash_dividend_rejects_existing_rights_with_same_ex_date_without_mutation() {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.stocks.truncate(1);
    setup.stocks[0].code = crate::StockCode("600101".into());
    setup.stocks[0].total_shares = 1_000;
    setup.stocks[0].float_shares = 1_000;
    setup.start_date = CivilDate::from_iso("2030-06-03").unwrap();
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 1;
    setup.ticks_per_day = 1;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.company_system = Some(simple_company_fixture!(crate; ["600101"]));
    setup.dividend_tax_mode = Some(CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1_000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup.auto_corporate_foundation = true;
    setup.rights_offering_enabled = true;
    let mut game = GameSession::new_with_runtime_groups(setup, 7, 3).unwrap();
    game.fixture_account_mut(&AccountId(0))
        .unwrap()
        .fixture_set_positions(std::collections::BTreeMap::from([(
            StockId(0),
            crate::account::Position::from_restored_parts(1, 1, 100, 0),
        )]));
    use crate::company::rights_offering::{
        RightsOfferingBook, RightsOfferingDeclaration, RightsOfferingEventPlan, RightsOfferingMode,
        RightsSubscriptionStrategy,
    };
    let today = game.civil_date();
    let calendar = game.civil_clock().calendar();
    let company = CompanyId(0);
    let capital = game
        .company_system()
        .unwrap()
        .dividend_legal_facts(&company)
        .unwrap()
        .unwrap()
        .registered_capital;
    let mut rights_plan = RightsOfferingEventPlan {
        event_id: "fixture-matching-rights-ex-date".into(),
        approval_reference: "同日配股测试 fixture".into(),
        issuer: company,
        stock: crate::StockCode("600101".into()),
        exchange: crate::calendar::CalendarExchange::Sse,
        approved_on: today,
        announced_on: today,
        registered_on: today,
        payment_start_on: today,
        payment_deadline_on: today,
        ex_rights_on: today,
        settlement_on: today,
        listing_on: today,
        price_per_share: Money::from_cents(100),
        mode: RightsOfferingMode::RightsToAllShareholders {
            shares_per_existing_share_micros: 100_000,
        },
        npc_subscription_strategy: RightsSubscriptionStrategy::FullByDefault,
    };
    rights_plan
        .derive_schedule(calendar, rights_plan.exchange, 1)
        .unwrap();
    let cash_ex_date = calendar
        .next_trading_day(
            crate::calendar::CalendarExchange::Sse,
            calendar
                .next_trading_day(crate::calendar::CalendarExchange::Sse, today)
                .unwrap(),
        )
        .unwrap();
    assert_eq!(rights_plan.ex_rights_on, cash_ex_date);
    let crate::session::company_state::SessionCompanies::CompanySystem(state) =
        &mut game.state.companies
    else {
        panic!("测试 fixture 使用 CompanySystem")
    };
    std::sync::Arc::make_mut(&mut state.system)
        .declare_rights_offering(
            &company,
            RightsOfferingDeclaration {
                event_id: rights_plan.event_id.clone(),
                approval_reference: rights_plan.approval_reference.clone(),
                approved_on: today,
                price_per_share: rights_plan.price_per_share,
                par_value_per_share: Money::from_cents(100),
                registered_capital_at_approval: capital,
            },
        )
        .unwrap();
    state
        .corporate_actions
        .rights_offerings
        .push(RightsOfferingBook::new(rights_plan).unwrap());
    let before = serde_json::to_value(game.save().unwrap()).unwrap();
    let revision = game.external_revision();
    let error = game
        .propose_player_company_action(AccountId(0), cash(1))
        .unwrap_err();
    assert!(
        error.to_string().contains("UnsupportedRightsCombination"),
        "{error}"
    );
    assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
    assert_eq!(game.external_revision(), revision);
}

#[test]
fn rights_approval_rejects_existing_cash_dividend_with_same_ex_date_without_mutation() {
    use crate::calendar::CalendarExchange;
    use crate::company::cash_dividend::{CashDividendBook, CashDividendPlan};
    use crate::company::ex_reference_price::CashDividendFormula;

    let mut game = game_with_features(true, true, false);
    let today = game.civil_date();
    let calendar = game.civil_clock().calendar();
    let registered_on = calendar
        .next_trading_day(CalendarExchange::Sse, today)
        .unwrap();
    let payment_start = calendar
        .next_trading_day(CalendarExchange::Sse, registered_on)
        .unwrap();
    let shared_ex_date = calendar
        .next_trading_day(CalendarExchange::Sse, payment_start)
        .unwrap();
    let plan = CashDividendPlan::new(
        "fixture-shared-ex-date".into(),
        CompanyId(0),
        crate::StockCode("600101".into()),
        CalendarExchange::Sse,
        CashDividendFormula::StandardCashOnly,
        today,
        today,
        payment_start,
        shared_ex_date,
        shared_ex_date,
        Money::from_cents(1),
        Money::from_cents(1_000),
        calendar,
    )
    .unwrap();
    let capital = game
        .company_system()
        .unwrap()
        .dividend_legal_facts(&CompanyId(0))
        .unwrap()
        .unwrap()
        .registered_capital;
    let crate::session::company_state::SessionCompanies::CompanySystem(state) =
        &mut game.state.companies
    else {
        panic!("测试 fixture 使用 CompanySystem")
    };
    std::sync::Arc::make_mut(&mut state.system)
        .declare_dividend(
            &CompanyId(0),
            crate::company::DividendDeclaration {
                plan_id: plan.plan_id.clone(),
                approved_on: today,
                total_gross: crate::accounting::AccountingAmount::from_money(Money::from_cents(
                    1_000,
                )),
                registered_capital: capital,
            },
        )
        .unwrap();
    state
        .corporate_actions
        .dividends
        .push(CashDividendBook::new(plan).unwrap());
    let restored_before = GameSession::restore(&game.save().unwrap()).unwrap();
    assert_eq!(
        restored_before.corporate_actions().unwrap().dividends[0]
            .plan()
            .plan_id,
        "fixture-shared-ex-date"
    );
    let before = serde_json::to_value(game.save().unwrap()).unwrap();
    let revision = game.external_revision();
    let error = game
        .propose_player_company_action(AccountId(0), rights())
        .unwrap_err();
    assert!(
        error.to_string().contains("未支持合并的非现金除权事件"),
        "{error}"
    );
    assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
    assert_eq!(game.external_revision(), revision);
}

#[test]
fn rights_subscription_rejects_when_new_game_feature_is_disabled() {
    let mut game = game_with_features(true, false, false);
    let before = game
        .corporate_actions()
        .unwrap()
        .rights_subscription_queue
        .clone();
    let result = game.subscribe_rights_offering(AccountId(0), "event-1", 1);
    assert!(matches!(
        result,
        Err(crate::session::RightsSubscriptionError::Disabled)
    ));
    assert_eq!(
        game.corporate_actions().unwrap().rights_subscription_queue,
        before
    );
}

#[test]
fn rights_subscription_reports_unsupported_tax_mode_before_event_lookup() {
    let mut game = game_with_features(true, true, false);
    game.state.setup.dividend_tax_mode = Some(CashDividendTaxMode::AShareIndividual);
    assert!(matches!(
        game.subscribe_rights_offering(AccountId(0), "missing-event", 1),
        Err(crate::session::RightsSubscriptionError::UnsupportedTaxMode)
    ));
}

#[test]
fn rights_subscription_rejects_existing_personal_tax_books_before_window() {
    let mut game = game_with_features(true, true, false);
    let receipt = game
        .propose_player_company_action(AccountId(0), rights())
        .unwrap();
    let day = game.civil_date();
    let tax_book = crate::company::cash_dividend_tax::CashDividendTaxBook::new(
        AccountId(0),
        crate::StockCode("600101".into()),
        crate::company::cash_dividend_tax::DividendTaxProfile::IndividualPublicMarket,
        day,
        vec![],
    )
    .unwrap();
    let crate::session::company_state::SessionCompanies::CompanySystem(state) =
        &mut game.state.companies
    else {
        panic!("测试 fixture 使用 CompanySystem")
    };
    state.corporate_actions.dividend_tax_books.push(tax_book);
    assert!(matches!(
        game.subscribe_rights_offering(AccountId(0), &receipt.event_id, 1),
        Err(crate::session::RightsSubscriptionError::UnsupportedSettlementScope { .. })
    ));
}

#[test]
fn rights_subscription_preserves_poisoned_session_and_queue() {
    let mut game = game_with_features(true, true, false);
    let fatal = crate::session::StepFatal::InvariantViolation {
        description: "注入配股受理前故障".into(),
        location: "rights_subscription.test".into(),
    };
    game.inject_step_failure(fatal.clone());
    assert_eq!(game.step().unwrap_err(), fatal);
    let queued = game
        .corporate_actions()
        .unwrap()
        .rights_subscription_queue
        .clone();
    assert!(matches!(
        game.subscribe_rights_offering(AccountId(0), "missing-event", 1),
        Err(crate::session::RightsSubscriptionError::Step(actual)) if actual == fatal
    ));
    assert_eq!(
        game.corporate_actions().unwrap().rights_subscription_queue,
        queued
    );
}

#[test]
fn rights_subscription_receipt_uses_canonical_decimal_shares() {
    let receipt = crate::session::RightsSubscriptionReceipt {
        event_id: "配股-1".into(),
        requested_shares: u64::MAX,
        submitted_on: CivilDate::from_iso("2030-06-03").unwrap(),
    };
    let encoded = serde_json::to_value(&receipt).unwrap();
    assert_eq!(encoded["requested_shares"], u64::MAX.to_string());
    assert_eq!(
        serde_json::from_value::<crate::session::RightsSubscriptionReceipt>(encoded).unwrap(),
        receipt
    );
}
