//! 资本批准边界：真实 Session 命令与已安装资本读取分开验证。

use super::*;
use crate::accounting::AccountingAmount;
use crate::calendar::CalendarExchange;
use crate::company::share_registry::{MovementScope, ShareDayRequest};
use crate::company::share_split::{ShareSplitBook, ShareSplitDeclaration, ShareSplitEventPlan};
use crate::session::capital_approval_context::CapitalApprovalError;
use crate::session::capital_command_candidates::CapitalCommandError;

fn date(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

fn state_mut(game: &mut GameSession) -> &mut SystemCompanyState {
    let SessionCompanies::CompanySystem(state) = &mut game.state.companies else {
        panic!("本 fixture 必须选择 CompanySystem");
    };
    state
}

fn stock() -> PlayerCompanyProposal {
    PlayerCompanyProposal::StockDistribution {
        company: CompanyId(0),
        kind: StockDistributionKind::BonusShares,
        shares_per_existing_share_micros: 100_000,
    }
}

fn assert_rejected_unchanged(
    game: &mut GameSession,
    proposal: PlayerCompanyProposal,
) -> PlayerCompanyCommandError {
    let before = serde_json::to_value(game.save().unwrap()).unwrap();
    let revision = game.external_revision();
    let had_runtime = game.runtime_driver.is_some();
    let error = game
        .propose_player_company_action(AccountId(0), proposal)
        .unwrap_err();
    assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
    assert_eq!(game.external_revision(), revision);
    assert_eq!(game.runtime_driver.is_some(), had_runtime);
    assert!(game.require_healthy().is_ok());
    error
}

fn has_unit_transition(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut cursor = Some(error);
    while let Some(cause) = cursor {
        if matches!(
            cause.downcast_ref::<CapitalCommandError>(),
            Some(CapitalCommandError::PendingShareUnitTransition { .. })
        ) {
            return true;
        }
        cursor = cause.source();
    }
    false
}

#[test]
fn player_command_disabled_rights_has_its_own_zero_change_case() {
    let mut game = game(true);
    let error = assert_rejected_unchanged(&mut game, rights());
    assert!(
        error.to_string().contains("rights_offering_enabled=false"),
        "{error}"
    );
}

#[test]
fn player_command_disabled_secondary_has_its_own_zero_change_case() {
    let mut game = game(true);
    let error = assert_rejected_unchanged(&mut game, secondary());
    assert!(
        error.to_string().contains("rights_offering_enabled=false"),
        "{error}"
    );
}

#[test]
fn player_command_disabled_repurchase_has_its_own_zero_change_case() {
    let mut game = game(true);
    let error = assert_rejected_unchanged(&mut game, repurchase());
    assert!(
        error
            .to_string()
            .contains("issuer_repurchase_enabled=false"),
        "{error}"
    );
}

#[test]
fn player_command_cash_then_rights_on_same_day_uses_current_committed_cutoff() {
    let mut game = game_with_features(true, true, false);
    let before = serde_json::to_value(game.save().unwrap().snapshot).unwrap();
    let cash_receipt = game
        .propose_player_company_action(AccountId(0), cash(1))
        .unwrap();
    let rights_receipt = game
        .propose_player_company_action(AccountId(0), rights())
        .unwrap();
    assert_eq!(cash_receipt.approved_on, rights_receipt.approved_on);
    let actions = game.corporate_actions().unwrap();
    assert_eq!(actions.dividends[0].plan().plan_id, cash_receipt.event_id);
    assert_eq!(
        actions.rights_offerings[0].plan().event_id,
        rights_receipt.event_id
    );
    assert_eq!(
        serde_json::to_value(game.save().unwrap().snapshot).unwrap(),
        before
    );
}

#[test]
fn player_command_pending_split_rejects_later_rights_without_partial_approval() {
    let mut game = game_with_features(true, true, false);
    game.propose_player_company_action(AccountId(0), split())
        .unwrap();
    let error = assert_rejected_unchanged(&mut game, rights());
    assert!(has_unit_transition(&error), "{error}");
}

#[test]
fn player_command_pending_rights_rejects_earlier_split_without_partial_approval() {
    let mut game = game_with_features(true, true, false);
    game.propose_player_company_action(AccountId(0), rights())
        .unwrap();
    let error = assert_rejected_unchanged(&mut game, split());
    assert!(has_unit_transition(&error), "{error}");
}

#[test]
fn player_command_pending_split_rejects_stock_without_future_par_value_leak() {
    let mut game = game(true);
    game.propose_player_company_action(AccountId(0), split())
        .unwrap();
    let context = CapitalApprovalContext::prepare(&game, CompanyId(0)).unwrap();
    assert_eq!(context.par_value(), Money::from_cents(100));
    assert_eq!(
        context.registered_capital(),
        AccountingAmount::from_cents(100_000)
    );
    let error = assert_rejected_unchanged(&mut game, stock());
    assert!(has_unit_transition(&error), "{error}");
}

#[test]
fn player_command_pending_stock_rejects_same_ex_split_without_partial_approval() {
    let mut game = game(true);
    game.propose_player_company_action(AccountId(0), stock())
        .unwrap();
    let error = assert_rejected_unchanged(&mut game, split());
    assert!(
        matches!(
            std::error::Error::source(&error)
                .unwrap()
                .downcast_ref::<CapitalCommandError>(),
            Some(CapitalCommandError::SameExDate { .. })
        ),
        "{error}"
    );
}

/// 显式装配未来领域方案，用于资本读取边界；不冒充生产相位已执行。
fn install_pending_split(
    game: &mut GameSession,
    registered_on: CivilDate,
    ex_rights_on: CivilDate,
) {
    let context = CapitalApprovalContext::prepare(game, CompanyId(0)).unwrap();
    let plan = ShareSplitEventPlan {
        event_id: "fixture-future-split".into(),
        approval_reference: "资本批准测试的显式领域方案".into(),
        issuer: CompanyId(0),
        stock: context.code().clone(),
        exchange: CalendarExchange::Sse,
        direction: crate::company::share_split::ShareSplitDirection::Split,
        ratio: 2,
        approved_on: game.civil_date(),
        announced_on: game.civil_date(),
        registered_on,
        ex_rights_on,
    };
    plan.validate_calendar(game.state.civil_clock.calendar())
        .unwrap();
    let state = state_mut(game);
    Arc::make_mut(&mut state.system)
        .declare_share_split(
            &CompanyId(0),
            ShareSplitDeclaration {
                event_id: plan.event_id.clone(),
                approval_reference: plan.approval_reference.clone(),
                direction: plan.direction.clone(),
                approved_on: plan.approved_on,
                ratio: plan.ratio,
                par_value_before: context.par_value(),
                par_value_after: Money::from_cents(context.par_value().cents() / 2),
                registered_capital_at_approval: context.registered_capital(),
            },
        )
        .unwrap();
    state
        .corporate_actions
        .share_splits
        .push(ShareSplitBook::new(plan).unwrap());
}

#[test]
fn player_command_rights_before_later_pending_split_is_allowed_in_current_units() {
    let mut game = game_with_features(true, true, false);
    install_pending_split(&mut game, date("2030-06-13"), date("2030-06-14"));
    game.propose_player_company_action(AccountId(0), rights())
        .unwrap();
    let actions = game.corporate_actions().unwrap();
    assert!(
        actions.rights_offerings[0].plan().listing_on < actions.share_splits[0].plan().ex_rights_on
    );
    let fact = &game
        .company_system()
        .unwrap()
        .rights_offering_facts(&CompanyId(0))
        .unwrap()[0];
    assert_eq!(fact.par_value_per_share, Money::from_cents(100));
    assert_eq!(
        fact.registered_capital_at_approval,
        AccountingAmount::from_cents(100_000)
    );
}

#[test]
fn player_command_stock_before_later_pending_split_is_allowed_in_current_units() {
    let mut game = game(true);
    install_pending_split(&mut game, date("2030-06-13"), date("2030-06-14"));
    game.propose_player_company_action(AccountId(0), stock())
        .unwrap();
    let actions = game.corporate_actions().unwrap();
    assert!(
        actions.stock_distributions[0].plan().ex_rights_on
            < actions.share_splits[0].plan().registered_on
    );
    assert_eq!(
        game.company_system()
            .unwrap()
            .stock_distribution_facts(&CompanyId(0))
            .unwrap()[0]
            .par_value_per_share,
        Money::from_cents(100)
    );
}

#[test]
fn player_command_rights_credit_on_pending_split_registration_requires_phase_source() {
    let mut game = game_with_features(true, true, false);
    let SessionCompanies::CompanySystem(state) = &game.state.companies else {
        panic!("必须使用 CompanySystem");
    };
    let plan = crate::session::player_company_command_candidates::build_player_rights_plan(
        crate::session::player_company_command_candidates::PlayerCompanyCommandInput {
            requester: game.account(AccountId(0)).unwrap(),
            catalog: &game.state.stocks,
            security: &game.state.setup.stocks[0],
            system: &state.system,
            actions: &state.corporate_actions,
            calendar: game.state.civil_clock.calendar(),
            day: game.civil_date(),
            proposal: rights(),
        },
        true,
    )
    .unwrap();
    // 由真实日历推导相等边界；不能凭工作日印象跳过本局冻结休市政策。
    let registered_on = plan.listing_on;
    let ex_on = game
        .state
        .civil_clock
        .calendar()
        .next_trading_day(CalendarExchange::Sse, registered_on)
        .unwrap();
    install_pending_split(&mut game, registered_on, ex_on);
    assert_eq!(
        game.corporate_actions().unwrap().share_splits[0]
            .plan()
            .registered_on,
        plan.listing_on
    );
    assert!(plan.listing_on < ex_on);
    let error = assert_rejected_unchanged(&mut game, rights());
    assert!(has_unit_transition(&error), "{error}");
}

#[test]
fn player_command_split_registration_on_pending_stock_credit_requires_phase_source() {
    use crate::company::stock_distribution::{
        StockDistributionBook, StockDistributionDeclaration, StockDistributionEventPlan,
    };
    let mut game = game(true);
    let day = game.civil_date();
    let context = CapitalApprovalContext::prepare(&game, CompanyId(0)).unwrap();
    let plan = StockDistributionEventPlan {
        event_id: "fixture-stock-before-next-ex".into(),
        approval_reference: "测试显式领域送股方案".into(),
        issuer: CompanyId(0),
        stock: context.code().clone(),
        exchange: CalendarExchange::Sse,
        kind: StockDistributionKind::BonusShares,
        approved_on: day,
        announced_on: day,
        registered_on: day,
        ex_rights_on: date("2030-06-04"),
        shares_per_existing_share_micros: 100_000,
        approved_total_new_shares: 100,
    };
    plan.validate_calendar(game.state.civil_clock.calendar())
        .unwrap();
    let state = state_mut(&mut game);
    Arc::make_mut(&mut state.system)
        .declare_stock_distribution(
            &CompanyId(0),
            StockDistributionDeclaration {
                event_id: plan.event_id.clone(),
                approval_reference: plan.approval_reference.clone(),
                kind: plan.kind.clone(),
                approved_on: day,
                new_shares: 100,
                par_value_per_share: context.par_value(),
                capital_increase: AccountingAmount::from_cents(10_000),
                registered_capital_at_approval: context.registered_capital(),
            },
        )
        .unwrap();
    state
        .corporate_actions
        .stock_distributions
        .push(StockDistributionBook::new(plan).unwrap());
    let error = assert_rejected_unchanged(&mut game, split());
    assert!(has_unit_transition(&error), "{error}");
}

#[test]
fn capital_context_uncredited_stock_keeps_current_capital_and_par() {
    let mut game = game(true);
    game.propose_player_company_action(AccountId(0), stock())
        .unwrap();
    let context = CapitalApprovalContext::prepare(&game, CompanyId(0)).unwrap();
    assert_eq!(context.par_value(), Money::from_cents(100));
    assert_eq!(
        context.registered_capital(),
        AccountingAmount::from_cents(100_000)
    );
    assert!(context.matches_current(&game));
}

#[test]
fn capital_context_detects_revision_source_and_exact_registry_frontier_changes() {
    let mut game = game(true);
    let context = CapitalApprovalContext::prepare(&game, CompanyId(0)).unwrap();
    assert!(context.matches_current(&game));
    assert!(context.matches_prepared_revision(&game));
    game.revision
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    assert!(!context.matches_prepared_revision(&game));
    let day = game.civil_date();
    state_mut(&mut game).corporate_actions.registries[0]
        .close_day(ShareDayRequest {
            event_id: "fixture-zero-market-frontier".into(),
            day,
            scope: MovementScope::PublicMarket,
            changes: vec![],
        })
        .unwrap();
    assert!(!context.matches_current(&game));
    let fresh = CapitalApprovalContext::prepare(&game, CompanyId(0)).unwrap();
    assert!(fresh.matches_current(&game));
    let state = state_mut(&mut game);
    state.system = Arc::new(state.system.as_ref().clone());
    assert!(!fresh.matches_current(&game));
}

fn manual_game(capital: AccountingAmount) -> GameSession {
    let template = game(true);
    let mut setup = template.state.setup.clone();
    setup.auto_corporate_foundation = false;
    setup.rights_offering_enabled = true;
    let mut game = GameSession::new_with_runtime_groups(setup, 7, 3).unwrap();
    let state = state_mut(&mut game);
    Arc::make_mut(&mut state.system)
        .define_dividend_legal_facts(
            &CompanyId(0),
            capital,
            "测试手工装配资本；不是外部资产来源证明".into(),
        )
        .unwrap();
    state.corporate_actions.registries = template.corporate_actions().unwrap().registries.clone();
    game.fixture_account_mut(&AccountId(0))
        .unwrap()
        .fixture_set_positions(template.account(AccountId(0)).unwrap().positions().clone());
    game
}

#[test]
fn capital_context_manual_foundation_uses_installed_capital_instead_of_setup_par() {
    let mut game = manual_game(AccountingAmount::from_cents(200_000));
    assert_eq!(
        game.state.setup.par_value_per_share,
        Some(Money::from_cents(100))
    );
    let SessionCompanies::CompanySystem(state) = &game.state.companies else {
        panic!("必须使用 CompanySystem");
    };
    let initial = crate::company::identity::IssuerRegistry::new(
        crate::session::company_assembly::issuer_specs(&game.state.setup).unwrap(),
    )
    .unwrap();
    // 先证沿既有手工装配 restore 合同合法，再让命令口径接受相同事实。
    let replay = crate::session::company_capital_restore::validate_company_capital_restore(
        crate::session::company_capital_restore::CapitalRestoreInput {
            setup_stocks: &game.state.setup.stocks,
            setup_issuers: &initial,
            system: &state.system,
            actions: &state.corporate_actions,
            catalog: &game.state.stocks,
            calendar: game.state.civil_clock.calendar(),
            committed_through: game.civil_date(),
            approved_through: game.civil_date(),
            initial_par: None,
        },
    )
    .unwrap();
    assert_eq!(
        replay.companies[&CompanyId(0)].registered_capital,
        Some(AccountingAmount::from_cents(200_000))
    );
    let context = CapitalApprovalContext::prepare(&game, CompanyId(0)).unwrap();
    assert_eq!(context.par_value(), Money::from_cents(200));
    let proposal = PlayerCompanyProposal::RightsOffering {
        company: CompanyId(0),
        price_per_share: Money::from_cents(200),
        shares_per_existing_share_micros: 100_000,
        payment_days: 1,
    };
    game.propose_player_company_action(AccountId(0), proposal)
        .unwrap();
    assert_eq!(
        game.company_system()
            .unwrap()
            .rights_offering_facts(&CompanyId(0))
            .unwrap()[0]
            .par_value_per_share,
        Money::from_cents(200)
    );
}

#[test]
fn capital_context_rejects_nonintegral_manual_par_without_truncation() {
    let game = manual_game(AccountingAmount::from_cents(100_001));
    assert_invalid_par(&game);
}

fn assert_invalid_par(game: &GameSession) {
    match CapitalApprovalContext::prepare(game, CompanyId(0)) {
        Err(CapitalApprovalError::InvalidPar { .. }) => {}
        Err(other) => panic!("必须保留非法面值原因，实际收到：{other:?}"),
        Ok(_) => panic!("不能截断或默认补齐非法面值"),
    }
}

#[test]
fn capital_context_rejects_manual_par_over_money_range_without_default() {
    let game = manual_game(AccountingAmount::from_cents(
        (i128::from(i64::MAX) + 1) * 1_000,
    ));
    assert_invalid_par(&game);
}

#[test]
fn capital_context_future_registry_receipt_is_rejected_before_world_or_approval() {
    let mut game = game_with_features(true, true, false);
    let day = game.civil_date();
    let registry = &mut state_mut(&mut game).corporate_actions.registries[0];
    for current in [day, day.next().unwrap()] {
        registry
            .close_day(ShareDayRequest {
                event_id: format!("fixture-future-market:{current}"),
                day: current,
                scope: MovementScope::PublicMarket,
                changes: vec![],
            })
            .unwrap();
    }
    assert!(matches!(
        CapitalApprovalContext::prepare(&game, CompanyId(0)),
        Err(CapitalApprovalError::Replay(source))
            if matches!(
                source.as_ref(),
                crate::session::company_capital_restore::CompanyCapitalRestoreError::FutureFact { .. }
            )
    ));
    assert_rejected_unchanged(&mut game, rights());
}

#[test]
fn capital_context_missing_registry_is_rejected_before_world_or_approval() {
    let mut game = game_with_features(true, true, false);
    state_mut(&mut game).corporate_actions.registries.clear();
    assert_rejected_unchanged(&mut game, rights());
}

/// 消费真实领域冻结／换算候选；这里只装配资本上下文测试历史，不计生产执行验收。
fn install_settled_split_history(game: &mut GameSession) {
    use crate::company::issuer_repurchase::SplitUnitContext;
    use crate::session::corporate_actions::share_transform_day_end::{
        share_transform_day_end_candidate, ShareTransformDayEndInput,
    };
    let (registered_on, ex_on) = crate::company::simple::preferences::preference_ex_dates(
        game.state.civil_clock.calendar(),
        CalendarExchange::Sse,
        game.civil_date(),
    )
    .unwrap();
    install_pending_split(game, registered_on, ex_on);
    let calendar = game.state.civil_clock.calendar().clone();
    let state = state_mut(game);
    let plan = state.corporate_actions.share_splits[0].plan().clone();
    let registry = &mut state.corporate_actions.registries[0];
    while registry.settled_on() < plan.registered_on {
        let day = registry.settled_on().next().unwrap();
        registry
            .close_day(ShareDayRequest {
                event_id: format!("fixture-market:{day}"),
                day,
                scope: MovementScope::PublicMarket,
                changes: vec![],
            })
            .unwrap();
    }
    let snapshot = registry
        .register(plan.event_id.clone(), plan.registered_on)
        .unwrap()
        .clone();
    let book = &mut state.corporate_actions.share_splits[0];
    book.announce(plan.announced_on).unwrap();
    book.register(snapshot, &calendar).unwrap();
    // R 与 ex 之间可能有真实休市日，逐自然日补齐零市场历史再消费冻结换算。
    while registry.settled_on().next().unwrap() < plan.ex_rights_on {
        let day = registry.settled_on().next().unwrap();
        registry
            .close_day(ShareDayRequest {
                event_id: format!("fixture-market:{day}"),
                day,
                scope: MovementScope::PublicMarket,
                changes: vec![],
            })
            .unwrap();
    }
    let units = [SplitUnitContext {
        event_id: plan.event_id.clone(),
        ex_on: plan.ex_rights_on,
        direction: plan.direction.clone(),
        ratio: plan.ratio,
    }];
    let candidate = share_transform_day_end_candidate(ShareTransformDayEndInput {
        day: plan.ex_rights_on,
        book,
        registry_in_registered_units: registry,
        dividend_tax_books: &[],
        unit_chain: &units,
        treasury_buckets: &[],
    })
    .unwrap();
    let receipt = candidate.settled_book.receipt().unwrap();
    Arc::make_mut(&mut state.system)
        .record_share_split_credit(
            &CompanyId(0),
            &plan.event_id,
            plan.ex_rights_on,
            receipt.issued_shares_before,
            receipt.issued_shares_after,
        )
        .unwrap();
    state.corporate_actions.registries[0] = candidate.registry_before_new_unit_market;
    state.corporate_actions.share_splits[0] = candidate.settled_book;
    while game.civil_date() <= plan.ex_rights_on {
        let day = game.civil_date();
        game.state.civil_clock.end_day(day).unwrap();
        game.market_calendar_day = None;
        // 本测试只需构造已结算股本与后续批准上下文；日历仍是会话时钟的权威，
        // 因而按其实际共享交易会话数同步 tick/day，不伪造连续市场成交。
        let day_context = game.prepare_market_calendar_day().unwrap();
        let completed_sessions = day_context.day_model().shared_sessions_before_date();
        game.state.day = u32::try_from(completed_sessions).unwrap();
        game.state.tick = completed_sessions
            .checked_mul(game.state.setup.ticks_per_day)
            .unwrap();
    }
}

#[test]
fn capital_context_settled_split_reads_new_par_and_allows_later_rights() {
    let mut game = game_with_features(true, true, false);
    install_settled_split_history(&mut game);
    let context = CapitalApprovalContext::prepare(&game, CompanyId(0)).unwrap();
    assert_eq!(context.par_value(), Money::from_cents(50));
    assert_eq!(
        context.registered_capital(),
        AccountingAmount::from_cents(100_000)
    );
    assert_eq!(
        game.corporate_actions().unwrap().registries[0].issued_shares(),
        2_000
    );
    game.propose_player_company_action(AccountId(0), rights())
        .unwrap();
    assert_eq!(
        game.company_system()
            .unwrap()
            .rights_offering_facts(&CompanyId(0))
            .unwrap()[0]
            .par_value_per_share,
        Money::from_cents(50)
    );
}

fn assert_old_book_handover_preserved(proposal: PlayerCompanyProposal, rights_enabled: bool) {
    let mut source = game_with_features(true, rights_enabled, false);
    let mut events = Vec::new();
    source.seed_order_for_test(
        AccountId(0),
        crate::Intent::PlaceLimit {
            code: StockId(0),
            side: crate::Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(900)),
            qty: 100,
        },
        &mut events,
    );
    let order_id = events
        .iter()
        .find_map(|event| match event {
            crate::Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .expect("真实旧簿前置必须有合法委托受理事件");
    let before = source.save().unwrap();
    assert!(!before.desk_mirrored_resting);
    assert_eq!(before.resting_orders[&StockId(0)].len(), 1);
    assert_eq!(before.resting_orders[&StockId(0)][0].id, order_id);
    assert_eq!(before.resting_orders[&StockId(0)][0].qty, 100);
    let mut restored = GameSession::restore(&before).expect("必须先证明旧簿 checkpoint 真实合法");
    restored.runtime_group_count = 3;
    assert!(restored.runtime_driver.is_none());
    let revision = restored.external_revision();
    let receipt = restored
        .propose_player_company_action(AccountId(0), proposal)
        .unwrap();
    assert_eq!(receipt.approved_on, before.civil_clock.current_date);
    assert_eq!(
        restored.external_revision(),
        revision.checked_add(2).unwrap(),
        "handover 与批准各消费一次 revision"
    );
    let after = restored.save().unwrap();
    assert!(after.desk_mirrored_resting);
    assert!(
        restored.state.markets[&StockId(0)]
            .resting_orders()
            .is_empty(),
        "旧簿不得留下第二份在途权威"
    );
    let mut expected_orders = before.resting_orders[&StockId(0)].clone();
    // E2 的既有合同将旧本地 seq 规范为原 OrderId 中保留的 desk serial；
    // 完整身份、价格、数量与成交进度仍逐字段相同，不能要求旧 seq 数值不变。
    for order in &mut expected_orders {
        order.seq = crate::runtime::group::desk_order_serial(order.id.0);
    }
    assert_eq!(after.resting_orders[&StockId(0)], expected_orders);
    assert_eq!(
        serde_json::to_value(&after.snapshot).unwrap(),
        serde_json::to_value(&before.snapshot).unwrap()
    );
    for (actual, expected) in [
        (
            serde_json::to_value(&after.runtime_state).unwrap(),
            serde_json::to_value(&before.runtime_state).unwrap(),
        ),
        (
            serde_json::to_value(&after.public_library).unwrap(),
            serde_json::to_value(&before.public_library).unwrap(),
        ),
        (
            serde_json::to_value(&after.npc_attention).unwrap(),
            serde_json::to_value(&before.npc_attention).unwrap(),
        ),
        (
            serde_json::to_value(&after.belief_books).unwrap(),
            serde_json::to_value(&before.belief_books).unwrap(),
        ),
        (
            serde_json::to_value(&after.information_states).unwrap(),
            serde_json::to_value(&before.information_states).unwrap(),
        ),
        (
            serde_json::to_value(&after.personal_trade_confirmations).unwrap(),
            serde_json::to_value(&before.personal_trade_confirmations).unwrap(),
        ),
        (
            serde_json::to_value(&after.civil_clock).unwrap(),
            serde_json::to_value(&before.civil_clock).unwrap(),
        ),
    ] {
        assert_eq!(actual, expected);
    }
    let mut driver = restored.runtime_driver.as_ref().unwrap().lock().unwrap();
    assert_eq!(driver.world_committed_tick(), restored.tick());
    let live = driver.collect_resting_facts_live().unwrap();
    assert_eq!(live[0].len(), 1);
    assert_eq!(live[0][0].qty, 100);
    assert_eq!(
        driver.account_by_owner(live[0][0].owner),
        Some(AccountId(0))
    );
    drop(driver);
    GameSession::restore(&after).expect("批准后的同静点存档必须真实可恢复");
}

#[test]
fn player_command_cash_after_restored_old_book_preserves_live_world_and_checkpoint() {
    assert_old_book_handover_preserved(cash(1), false);
}

#[test]
fn player_command_rights_after_restored_old_book_preserves_live_world_and_checkpoint() {
    assert_old_book_handover_preserved(rights(), true);
}

#[test]
fn player_command_stock_after_restored_old_book_preserves_live_world_and_checkpoint() {
    assert_old_book_handover_preserved(stock(), false);
}

#[test]
fn player_command_split_after_restored_old_book_preserves_live_world_and_checkpoint() {
    assert_old_book_handover_preserved(split(), false);
}

#[test]
fn split_registration_captures_final_market_registry_in_civil_candidate() {
    let mut game = game(false);
    install_pending_split(&mut game, date("2030-06-03"), date("2030-06-04"));
    game.prepare_ordinary_tax_day_end(date("2030-06-03"), true)
        .unwrap();
    let actions = game.corporate_actions().unwrap();
    let book = &actions.share_splits[0];
    let snapshot = book.registration().expect("日终登记必须冻结实际名册");
    assert_eq!(snapshot.issued_shares(), 1_000);
    assert_eq!(
        snapshot,
        actions.registries[0]
            .registration("fixture-future-split")
            .unwrap()
    );
    assert_eq!(
        book.status(),
        &crate::company::share_split::ShareSplitStatus::Registered
    );
}
