//! native Simple 单证券真实链；只用真实卖单，不种入公司 child 或成交。
use super::issuer_repurchase_approval::RepurchaseApprovalSource;
use super::*;
use crate::company::issuer_repurchase::*;
include!("../../test-support/simple_company.rs");

fn fixture() -> GameSession {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.stocks.truncate(1);
    setup.company_system = Some(simple_company_fixture!(crate; ["000001"]));
    setup.dividend_tax_mode = Some(crate::company::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup.issuer_repurchase_enabled = true;
    setup.auto_corporate_foundation = true;
    setup.start_date = CivilDate::from_iso("2026-01-05").unwrap();
    setup.ticks_per_day = 4;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    GameSession::new_with_runtime_groups(setup, 531, 2).unwrap()
}
fn plan(game: &GameSession) -> IssuerRepurchasePlan {
    let day = game.civil_date();
    let next = day.next().unwrap();
    IssuerRepurchasePlan {
        event_id: "真实回购".into(),
        approval_reference: "股东会批准".into(),
        issuer: crate::company::CompanyId(0),
        stock: StockCode("000001".into()),
        exchange: crate::calendar::CalendarExchange::Szse,
        approved_on: day,
        announced_on: day,
        window_start_on: next,
        window_deadline_on: next,
        price_cap_per_share: Money::from_cents(1000),
        total_budget: Money::from_cents(300_000),
        max_shares: 200,
        purpose: RepurchasePurpose::ReduceCapital,
        completion_policy: RepurchaseCompletionPolicy::CancelOnCompletion,
        approval_unit_event_id: None,
    }
}
fn day(game: &mut GameSession) {
    for _ in 0..4 {
        game.step().unwrap();
    }
    game.end_civil_day().unwrap();
}
fn book(game: &GameSession) -> &IssuerRepurchaseBook {
    &game.corporate_actions().unwrap().issuer_repurchases[0]
}
fn owner(game: &GameSession) -> AccountId {
    game.account_roster()
        .into_iter()
        .find(|id| game.account(*id).unwrap().kind() == AccountKind::IssuerRepurchase)
        .unwrap()
}
#[test]
fn repurchase_native_partial_fill_recycle_then_cancellation() {
    let mut game = fixture();
    let plan = plan(&game);
    game.approve_issuer_repurchase(plan, RepurchaseApprovalSource::InternalScenario)
        .unwrap();
    day(&mut game);
    let seller = game
        .account_roster()
        .into_iter()
        .find(|id| {
            let a = game.account(*id).unwrap();
            a.kind() == AccountKind::Retail
                && a.position(&StockId(0)).is_some_and(|p| p.sellable() >= 100)
        })
        .unwrap();
    let cash = game.account(seller).unwrap().cash();
    game.seed_order_for_test(
        seller,
        Intent::PlaceLimit {
            code: StockId(0),
            side: Side::Sell,
            price: LimitPrice::Fixed(Money::from_cents(1000)),
            qty: 100,
        },
        &mut Vec::new(),
    );
    day(&mut game);
    assert_eq!(book(&game).total_filled_shares(), 100);
    assert!(game.account(seller).unwrap().cash() > cash);
    assert_eq!(book(&game).status(), &IssuerRepurchaseStatus::Completed);
    assert_eq!(game.account(owner(&game)).unwrap().cash(), Money::ZERO);
    game = GameSession::restore(&game.save().unwrap()).unwrap();
    let capital = game
        .company_system()
        .unwrap()
        .registered_capital_at(&crate::company::CompanyId(0), game.civil_date())
        .unwrap()
        .unwrap();
    let anchor = game.state.markets[&StockId(0)].last_close();
    let issued = game
        .company_system()
        .unwrap()
        .issuers()
        .get(&crate::company::CompanyId(0))
        .unwrap()
        .issued_shares;
    day(&mut game);
    assert_eq!(book(&game).status(), &IssuerRepurchaseStatus::Cancelled);
    assert_eq!(book(&game).cancelled_shares(), 100);
    assert_eq!(
        game.state.markets[&StockId(0)].last_close(),
        anchor,
        "注销不调整价格锚"
    );
    assert_eq!(
        game.company_system()
            .unwrap()
            .registered_capital_at(&crate::company::CompanyId(0), game.civil_date())
            .unwrap()
            .unwrap(),
        capital
            .sub(crate::accounting::AccountingAmount::from_money(
                Money::from_cents(10_000)
            ))
            .unwrap()
    );
    assert_eq!(
        game.company_system()
            .unwrap()
            .issuers()
            .get(&crate::company::CompanyId(0))
            .unwrap()
            .issued_shares,
        issued - 100
    );
    assert_eq!(
        game.account(owner(&game))
            .unwrap()
            .position(&StockId(0))
            .map_or(0, |p| p.qty()),
        0
    );
    let restored = GameSession::restore(&game.save().unwrap()).unwrap();
    assert_eq!(book(&restored), book(&game));
}
#[test]
fn repurchase_native_zero_fill_closes_and_recycles_exact_budget() {
    let mut game = fixture();
    let plan = plan(&game);
    let mut too_long = plan.clone();
    too_long.window_deadline_on = CivilDate::from_iso("2027-01-06").unwrap();
    assert!(
        too_long.validate().is_err(),
        "期限须自批准日起不超过十二个月"
    );
    game.approve_issuer_repurchase(plan.clone(), RepurchaseApprovalSource::InternalScenario)
        .unwrap();
    day(&mut game);
    day(&mut game);
    assert!(book(&game).fills().is_empty());
    assert_eq!(book(&game).withdrawn_remainder(), Some(plan.total_budget));
    assert_eq!(book(&game).status(), &IssuerRepurchaseStatus::Completed);
    assert_eq!(game.account(owner(&game)).unwrap().cash(), Money::ZERO);
    let binding = game
        .issuer_repurchase_execution_binding(&plan.event_id)
        .unwrap();
    assert_eq!(binding.closing_state, RepurchaseBindingClosingState::Closed);
    assert!(
        binding.applied_fact_cursor > 0,
        "零成交也须有真实 Accepted/Return/Closed 来源"
    );
    let mut successor = plan.clone();
    successor.event_id = "后继回购".into();
    successor.approval_reference = "后继股东会批准".into();
    successor.approved_on = game.civil_date();
    successor.announced_on = game.civil_date();
    successor.window_start_on = game.civil_date().next().unwrap();
    successor.window_deadline_on = successor.window_start_on;
    game.approve_issuer_repurchase(successor, RepurchaseApprovalSource::InternalScenario)
        .unwrap();
    assert_eq!(
        game.corporate_actions()
            .unwrap()
            .repurchase_execution_bindings
            .len(),
        2
    );
}
#[test]
fn repurchase_native_restore_does_not_refund_twice_and_abort_keeps_facts() {
    let mut game = fixture();
    let plan = plan(&game);
    let guard = super::issuer_repurchase_approval::set_approval_fault(
        super::issuer_repurchase_approval::ApprovalFault::Inspect,
    );
    assert!(game
        .approve_issuer_repurchase(plan.clone(), RepurchaseApprovalSource::InternalScenario)
        .is_err());
    drop(guard);
    assert!(game
        .corporate_actions()
        .unwrap()
        .issuer_repurchases
        .is_empty());
    assert_eq!(game.account(owner(&game)).unwrap().cash(), Money::ZERO);
    game.approve_issuer_repurchase(plan.clone(), RepurchaseApprovalSource::InternalScenario)
        .unwrap();
    day(&mut game);
    let saved = game.save().unwrap();
    let mut restored = GameSession::restore(&saved).unwrap();
    for _ in 0..4 {
        restored.step().unwrap();
    }
    let before_book = book(&restored).clone();
    let before_cash = restored.account(owner(&restored)).unwrap().cash();
    let before_binding = restored
        .issuer_repurchase_execution_binding(&plan.event_id)
        .unwrap()
        .clone();
    super::issuer_repurchase_execution::reject_next_inspect();
    assert!(restored.end_civil_day().is_err());
    assert!(
        restored.poison_reason().is_none(),
        "首 Commit 前健康 Abort 应允许重试 civil"
    );
    assert_eq!(book(&restored), &before_book);
    assert_eq!(
        restored
            .issuer_repurchase_execution_binding(&plan.event_id)
            .unwrap(),
        &before_binding
    );
    assert_eq!(
        restored.account(owner(&restored)).unwrap().cash(),
        before_cash
    );
    restored.end_civil_day().unwrap();
    let cursor = restored
        .issuer_repurchase_execution_binding(&plan.event_id)
        .unwrap()
        .applied_fact_cursor;
    let mut continued = GameSession::restore(&restored.save().unwrap()).unwrap();
    day(&mut continued);
    assert_eq!(
        continued
            .issuer_repurchase_execution_binding(&plan.event_id)
            .unwrap()
            .applied_fact_cursor,
        cursor
    );
    assert_eq!(
        continued.account(owner(&continued)).unwrap().cash(),
        Money::ZERO
    );
    assert_eq!(
        book(&continued).withdrawn_remainder(),
        Some(plan.total_budget)
    );
}
