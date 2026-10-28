//! 回购玩家前置必须路由至真实共同批准，不从镜像账户构造资金成功。

use super::*;
use crate::company::issuer_repurchase::{RepurchaseCompletionPolicy, RepurchasePurpose};

fn assert_rejected_without_mutation(
    game: &mut GameSession,
    proposal: PlayerCompanyProposal,
) -> PlayerCompanyCommandError {
    let save = serde_json::to_value(game.save().unwrap()).unwrap();
    let revision = game.external_revision();
    let had_driver = game.runtime_driver.is_some();
    let error = game
        .propose_player_company_action(AccountId(0), proposal)
        .unwrap_err();
    assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), save);
    assert_eq!(game.external_revision(), revision);
    assert_eq!(game.runtime_driver.is_some(), had_driver);
    assert!(game.require_healthy().is_ok());
    error
}

#[test]
fn player_repurchase_route_commits_real_company_plan_funding_and_execution_once() {
    let mut game = game_with_features(true, false, true);
    let player = game.account(AccountId(0)).unwrap().clone();
    let repurchase_accounts: Vec<_> = game
        .account_roster()
        .into_iter()
        .filter(|id| game.account(*id).unwrap().kind() == crate::AccountKind::IssuerRepurchase)
        .collect();
    assert_eq!(repurchase_accounts.len(), 1);
    let issuer_account = repurchase_accounts[0];
    let cash_before = game.account(issuer_account).unwrap().cash();
    let receipt = game
        .propose_player_company_action(AccountId(0), repurchase())
        .unwrap();
    let actions = game.corporate_actions().unwrap();
    assert_eq!(actions.issuer_repurchases.len(), 1);
    let plan = actions.issuer_repurchases[0].plan();
    assert_eq!(plan.event_id, receipt.event_id);
    assert_eq!(plan.issuer, receipt.company);
    assert_eq!(plan.approved_on, receipt.approved_on);
    assert_eq!(receipt.kind, PlayerProposalKind::IssuerRepurchase);
    assert_eq!(plan.price_cap_per_share, Money::from_cents(100));
    assert_eq!(plan.total_budget, Money::from_cents(10_000));
    assert_eq!(plan.max_shares, 100);
    assert_eq!(plan.purpose, RepurchasePurpose::ReduceCapital);
    assert_eq!(
        plan.completion_policy,
        RepurchaseCompletionPolicy::CancelOnCompletion
    );
    assert_eq!(plan.approval_unit_event_id, None);
    let facts = game
        .company_system()
        .unwrap()
        .issuer_repurchase_facts(&CompanyId(0))
        .unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].event_id, receipt.event_id);
    assert_eq!(
        facts[0].synthetic_funding,
        crate::accounting::AccountingAmount::from_cents(10_000)
    );
    assert_eq!(
        game.account(issuer_account).unwrap().cash().cents() - cash_before.cents(),
        10_000
    );
    let plans = game.runtime_plans_snapshot();
    let execution = plans.active_entry(issuer_account, &StockId(0)).unwrap();
    assert!(matches!(
        execution,
        crate::plans::PlanEntry::IssuerRepurchase(_)
    ));
    assert_eq!(execution.plan_id().owner, issuer_account);
    let execution = execution.clone();
    // 读真实 Group owner，排除只改 Session 镜像就回成功。
    {
        let mut driver = game
            .runtime_driver
            .as_ref()
            .expect("共同批准必须实际构造驱动器")
            .lock()
            .unwrap();
        let groups = driver.collect_plans().unwrap();
        let actual: Vec<_> = groups
            .iter()
            .filter_map(|(_, book)| book.entry(execution.plan_id()).ok())
            .collect();
        assert_eq!(actual, vec![&execution]);
        let accounts = driver.world_accounts_snapshot().unwrap();
        let actual_cash: Vec<_> = accounts
            .into_iter()
            .flat_map(|(group, accounts)| {
                accounts
                    .into_iter()
                    .enumerate()
                    .filter_map(|(local, account)| {
                        (driver.account_by_owner((group, local as u32)) == Some(issuer_account))
                            .then_some(account.cash)
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(actual_cash, vec![cash_before.cents() + 10_000]);
        assert_eq!(driver.world_committed_tick(), game.tick());
    }
    assert_eq!(game.account(AccountId(0)).unwrap().cash(), player.cash());
    assert_eq!(
        serde_json::to_value(game.account(AccountId(0)).unwrap().positions()).unwrap(),
        serde_json::to_value(player.positions()).unwrap()
    );
    assert!(game.require_healthy().is_ok());
}

#[test]
fn player_repurchase_window_uses_session_exchange_calendar_after_weekend() {
    let mut game = game_with_features_on(true, false, true, "2030-06-08");
    let proposal = PlayerCompanyProposal::IssuerRepurchase {
        company: CompanyId(0),
        price_cap_per_share: Money::from_cents(100),
        total_budget: Money::from_cents(10_000),
        max_shares: 100,
        window_trading_days: 2,
        purpose: RepurchasePurpose::ReduceCapital,
    };
    let receipt = game
        .propose_player_company_action(AccountId(0), proposal)
        .unwrap();
    let plan = game.corporate_actions().unwrap().issuer_repurchases[0].plan();
    assert_eq!(
        receipt.approved_on,
        CivilDate::from_iso("2030-06-08").unwrap()
    );
    assert_eq!(plan.stock, crate::StockCode("600101".into()));
    assert_eq!(plan.exchange, crate::calendar::CalendarExchange::Sse);
    assert_eq!(
        plan.window_start_on,
        CivilDate::from_iso("2030-06-10").unwrap()
    );
    assert_eq!(
        plan.window_deadline_on,
        CivilDate::from_iso("2030-06-11").unwrap()
    );
    plan.validate_calendar(game.state.civil_clock.calendar())
        .unwrap();
}

#[test]
fn player_repurchase_rejects_zero_window_and_invalid_terms_without_partial_funding() {
    for condition in 0..4 {
        let mut game = game_with_features(true, false, true);
        let mut proposal = repurchase();
        let PlayerCompanyProposal::IssuerRepurchase {
            price_cap_per_share,
            total_budget,
            max_shares,
            window_trading_days,
            ..
        } = &mut proposal
        else {
            unreachable!()
        };
        match condition {
            0 => *price_cap_per_share = Money::ZERO,
            1 => *total_budget = Money::ZERO,
            2 => *max_shares = 0,
            3 => *window_trading_days = 0,
            _ => unreachable!(),
        }
        let error = assert_rejected_without_mutation(&mut game, proposal);
        let mut source = std::error::Error::source(&error);
        let mut expected_cause = false;
        while let Some(cause) = source {
            if let Some(player) = cause.downcast_ref::<crate::session::player_company_command_candidates::PlayerCommandError>() {
                use crate::session::player_company_command_candidates::{PlayerCommandCause, PlayerCommandIssue};
                expected_cause = if condition == 3 {
                    matches!(player.cause, PlayerCommandCause::Validation(PlayerCommandIssue::NonPositiveRepurchaseWindow))
                } else {
                    matches!(player.cause, PlayerCommandCause::RepurchasePlan(_))
                };
                break;
            }
            source = cause.source();
        }
        assert!(
            expected_cause,
            "非法回购参数必须保留真实Plan拒绝，而非占位或无关错误：{error}"
        );
    }
}

#[test]
fn player_repurchase_second_click_preserves_first_real_funding_and_plan_identity() {
    let mut game = game_with_features(true, false, true);
    game.propose_player_company_action(AccountId(0), repurchase())
        .unwrap();
    let error = assert_rejected_without_mutation(&mut game, repurchase());
    let mut source = std::error::Error::source(&error);
    let mut plan_conflict = false;
    while let Some(cause) = source {
        if matches!(
            cause.downcast_ref::<crate::session::SessionError>(),
            Some(crate::session::SessionError::RepurchaseApproval(
                crate::session::RepurchaseApprovalError::PlanConflict { .. }
            ))
        ) {
            plan_conflict = true;
            break;
        }
        source = cause.source();
    }
    assert!(
        plan_conflict,
        "第二次点击必须保留唯一共同批准的真实活动计划冲突：{error}"
    );
    assert_eq!(
        game.corporate_actions().unwrap().issuer_repurchases.len(),
        1
    );
}

/// 只验证 pure builder 对真实 Book 状态戳的消费，不把此领域夹具当 Session 已入账。
fn repurchase_plan_at_registered_split_boundary(
    settled: bool,
) -> crate::company::issuer_repurchase::IssuerRepurchasePlan {
    use crate::company::share_registry::{
        AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
    };
    use crate::company::share_split::{
        ShareSplitBook, ShareSplitDirection, ShareSplitEventPlan, ShareSplitStatus,
    };
    use crate::session::player_company_command_candidates::{
        PlayerCompanyCommandInput, build_player_repurchase_plan,
    };
    let game = game_with_features_on(true, false, true, "2030-06-04");
    let registered_on = CivilDate::from_iso("2030-06-03").unwrap();
    let ex_rights_on = game.civil_date();
    let event_id = "fixture-registered-split-unit".to_string();
    let plan = ShareSplitEventPlan {
        event_id: event_id.clone(),
        approval_reference: "真实Book单位状态边界".into(),
        issuer: CompanyId(0),
        stock: crate::StockCode("600101".into()),
        exchange: crate::calendar::CalendarExchange::Sse,
        direction: ShareSplitDirection::Split,
        ratio: 2,
        approved_on: CivilDate::from_iso("2030-06-01").unwrap(),
        announced_on: CivilDate::from_iso("2030-06-02").unwrap(),
        registered_on,
        ex_rights_on,
    };
    let mut registry = ShareRegistry::new(
        plan.stock.clone(),
        plan.issuer,
        1_000,
        registered_on,
        vec![ShareHolding {
            holder: HolderId::Account(AccountId(0)),
            lots: vec![ShareLot {
                id: "fixture-split-opening".into(),
                qty: 1_000,
                acquired_on: registered_on,
                source: AcquisitionSource::InitialAllocation {
                    evidence: "领域登记边界夹具".into(),
                },
                restriction: ShareRestriction::Unrestricted,
            }],
        }],
    )
    .unwrap();
    let registration = registry.register(event_id, registered_on).unwrap().clone();
    let mut book = ShareSplitBook::new(plan).unwrap();
    book.announce(CivilDate::from_iso("2030-06-02").unwrap())
        .unwrap();
    book.register(registration, game.state.civil_clock.calendar())
        .unwrap();
    assert!(book.receipt().is_some());
    assert_eq!(book.status(), &ShareSplitStatus::Registered);
    assert_eq!(book.settled_on(), None);
    if settled {
        book.mark_settled(ex_rights_on).unwrap();
        assert_eq!(book.status(), &ShareSplitStatus::Settled);
        assert_eq!(book.settled_on(), Some(ex_rights_on));
    }
    book.validate().unwrap();
    let mut actions = game.corporate_actions().unwrap().clone();
    actions.share_splits.push(book);
    let plan = build_player_repurchase_plan(PlayerCompanyCommandInput {
        requester: game.account(AccountId(0)).unwrap(),
        catalog: &game.state.stocks,
        security: &game.state.setup.stocks[0],
        system: game.company_system().unwrap(),
        actions: &actions,
        calendar: game.state.civil_clock.calendar(),
        day: game.civil_date(),
        proposal: repurchase(),
    })
    .unwrap();
    assert_eq!(plan.price_cap_per_share, Money::from_cents(100));
    assert_eq!(plan.max_shares, 100);
    plan
}

#[test]
fn player_repurchase_builder_does_not_treat_registration_receipt_as_applied_share_units() {
    let plan = repurchase_plan_at_registered_split_boundary(false);
    assert_eq!(plan.approval_unit_event_id, None);
}

#[test]
fn player_repurchase_builder_uses_actual_settled_share_unit_anchor_without_rescaling_inputs() {
    let plan = repurchase_plan_at_registered_split_boundary(true);
    assert_eq!(
        plan.approval_unit_event_id.as_deref(),
        Some("fixture-registered-split-unit")
    );
}
