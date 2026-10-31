//! 从实际 Session 构造 driver 的 P2 数值通路，直接 company entry 不冒充批准。
use super::*;
use crate::runtime::protocol::AdminOperation;
use crate::{AccountId, AccountKind, GameSession, Money};
include!("../../../test-support/simple_company.rs");
fn setup() -> crate::session::SessionSetup {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.company_system = Some(simple_company_fixture!(crate; ["000001", "300001"]));
    setup.dividend_tax_mode = Some(crate::company::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup.issuer_repurchase_enabled = true;
    setup.auto_corporate_foundation = true;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup
}
fn approve(session: &mut GameSession, stock: u32, budget: i64) -> crate::plans::PlanId {
    use crate::company::issuer_repurchase::{
        IssuerRepurchasePlan, RepurchaseCompletionPolicy, RepurchasePurpose,
    };
    let day = session.civil_date();
    let plan = IssuerRepurchasePlan {
        event_id: format!("测试回购-{stock}"),
        approval_reference: format!("股东会批准-{stock}"),
        issuer: crate::company::CompanyId(stock),
        stock: crate::StockCode(if stock == 0 { "000001" } else { "300001" }.into()),
        exchange: crate::calendar::CalendarExchange::Szse,
        approved_on: day,
        announced_on: day,
        window_start_on: day,
        window_deadline_on: day,
        price_cap_per_share: Money::from_cents(1000),
        total_budget: Money::from_cents(budget),
        max_shares: 100,
        purpose: RepurchasePurpose::ReduceCapital,
        completion_policy: RepurchaseCompletionPolicy::CancelOnCompletion,
        approval_unit_event_id: None,
    };
    let receipt = session.approve_repurchase_for_runtime_test(plan).unwrap();
    let crate::session::RepurchaseApprovalReceipt::Approved { plan_id, .. } = receipt else {
        panic!("首次批准必须成功")
    };
    plan_id
}
fn restore_approved(session: &GameSession) -> GameSession {
    GameSession::restore(&session.save().unwrap()).unwrap()
}
#[test]
fn funds_p2_actual_driver_01_real_shared_treasury_nonzero_owner_credit_survives_next_start() {
    let mut session = GameSession::new(setup(), 101).unwrap();
    let owner = session
        .account_roster()
        .into_iter()
        .find(|id| session.account(*id).unwrap().kind() == AccountKind::IssuerRepurchase)
        .unwrap();
    let ids = [approve(&mut session, 0, 101), approve(&mut session, 1, 203)];
    let mut session = restore_approved(&session);
    let baseline = session.account(owner).unwrap().cash();
    assert_eq!(baseline, Money::from_cents(304));
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    assert_eq!(driver.account_by_owner((1, 1)), Some(owner));
    // 以下只验证隔离 driver 的现金原语与镜像，不代表再次批准公司预算。
    driver
        .world
        .with_admin_transaction(
            vec![(
                1,
                vec![
                    AdminOperation::SyntheticBudgetCredit {
                        local: 1,
                        stock: 0,
                        execution_plan: crate::runtime::protocol::AdminPlanReference::Existing(
                            ids[0],
                        ),
                        amount: 101,
                    },
                    AdminOperation::SyntheticBudgetCredit {
                        local: 1,
                        stock: 1,
                        execution_plan: crate::runtime::protocol::AdminPlanReference::Existing(
                            ids[1],
                        ),
                        amount: 203,
                    },
                ],
            )],
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(session.account(owner).unwrap().cash(), baseline);
    driver.step_frame_equivalent(&mut session).unwrap();
    assert_eq!(
        session.account(owner).unwrap().cash(),
        Money::from_cents(608)
    );
    driver.world.shutdown().unwrap();
}
#[test]
fn funds_p2_actual_driver_02_account_number_does_not_authorize_a_changed_real_kind() {
    let mut session = GameSession::new(setup(), 102).unwrap();
    let owner = session
        .account_roster()
        .into_iter()
        .find(|id| session.account(*id).unwrap().kind() == AccountKind::IssuerRepurchase)
        .unwrap();
    session
        .fixture_account_mut(&owner)
        .unwrap()
        .fixture_set_kind(AccountKind::Player);
    let bindings =
        admin_treasury_bindings_from_session(&session, &session.account_roster(), 2).unwrap();
    assert!(bindings.iter().all(|binding| binding.account != owner));
    assert!(bindings
        .iter()
        .all(|binding| binding.account != AccountId(0)));
}

#[test]
fn funds_p2_actual_driver_03_lazy_builder_binding_is_live_before_session_activation() {
    let mut session = GameSession::new(setup(), 103).unwrap();
    let owner = session
        .account_roster()
        .into_iter()
        .find(|id| session.account(*id).unwrap().kind() == AccountKind::IssuerRepurchase)
        .unwrap();
    let id = approve(&mut session, 0, 101);
    let session = restore_approved(&session);
    let baseline = session.account(owner).unwrap().cash();
    assert_eq!(baseline, Money::from_cents(101));
    let expected_epoch = session.next_runtime_authority_epoch().unwrap();
    let old_revision = session.external_revision();
    let old_plans = session.runtime_plans_snapshot().clone();
    let mut driver =
        RuntimeDriver::prepare_from_session_grouped(&session, 2, expected_epoch).unwrap();
    assert_eq!(driver.account_by_owner((1, 1)), Some(owner));
    let initial_source = driver.mirror_source();
    let published = driver.world.publication().load();
    // 以下只验证隔离 driver 的现金原语与镜像，不代表再次批准公司预算。
    driver
        .world
        .with_admin_transaction(
            vec![(
                1,
                vec![AdminOperation::SyntheticBudgetCredit {
                    local: 1,
                    stock: 0,
                    execution_plan: crate::runtime::protocol::AdminPlanReference::Existing(id),
                    amount: 101,
                }],
            )],
            |_| Ok(()),
        )
        .unwrap();
    let source = driver.mirror_source();
    assert_eq!(
        (source.authority_epoch, source.tick, source.generation),
        (
            expected_epoch,
            initial_source.tick,
            initial_source.generation
        )
    );
    assert_eq!(
        source.non_tick_revision,
        initial_source.non_tick_revision + 1
    );
    assert!(std::sync::Arc::ptr_eq(
        &published,
        &driver.world.publication().load()
    ));
    let pending = driver.pending_mirror_domains();
    assert!(pending.accounts_full);
    let input = driver.prepare_static_driver_projection(&session).unwrap();
    assert_eq!(input.static_read.as_ref().unwrap().source, source);
    assert_eq!(
        input.accounts[1]
            .iter()
            .find(|(local, _)| *local == 1)
            .unwrap()
            .1
            .cash,
        202
    );
    assert_eq!(
        driver.pending_mirror_domains(),
        pending,
        "静点读取不消费行政待刷原因"
    );
    assert_eq!(session.account(owner).unwrap().cash(), baseline);
    assert_eq!(
        session.next_runtime_authority_epoch().unwrap(),
        expected_epoch
    );
    assert_eq!(session.external_revision(), old_revision);
    assert_eq!(session.runtime_plans_snapshot(), &old_plans);
    driver.world.shutdown().unwrap();
}
#[test]
fn funds_p2_actual_driver_04_lazy_builder_cannot_fund_same_number_with_player_kind() {
    let mut session = GameSession::new(setup(), 104).unwrap();
    let owner = session
        .account_roster()
        .into_iter()
        .find(|id| session.account(*id).unwrap().kind() == AccountKind::IssuerRepurchase)
        .unwrap();
    let _id = approve(&mut session, 0, 101);
    let mut session = restore_approved(&session);
    session
        .fixture_account_mut(&owner)
        .unwrap()
        .fixture_set_kind(AccountKind::Player);
    let cash = session.account(owner).unwrap().cash();
    let epoch = session.next_runtime_authority_epoch().unwrap();
    let revision = session.external_revision();
    let plans = session.runtime_plans_snapshot().clone();
    let error = match RuntimeDriver::prepare_from_session_grouped(&session, 2, epoch) {
        Ok(driver) => {
            driver.world.shutdown().unwrap();
            panic!("Player kind 不得构造回购 binding")
        }
        Err(error) => error,
    };
    assert!(error.to_string().contains("binding"), "{error}");
    assert_eq!(session.account(owner).unwrap().kind(), AccountKind::Player);
    assert_eq!(session.account(owner).unwrap().cash(), cash);
    assert_eq!(session.next_runtime_authority_epoch().unwrap(), epoch);
    assert_eq!(session.external_revision(), revision);
    assert_eq!(session.runtime_plans_snapshot(), &plans);
}
