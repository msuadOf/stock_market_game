//! 从实际 Session 构造 driver 的 P2 数值通路，直接 company entry 不冒充批准。
use super::*;
use crate::runtime::protocol::AdminOperation;
use crate::{AccountId, AccountKind, GameSession, Money, StockId};
include!("../../../test-support/simple_company.rs");
fn setup() -> crate::session::SessionSetup {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.company_system = Some(simple_company_fixture!(crate; ["000001", "300001"]));
    setup.dividend_tax_mode = Some(crate::company::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1000); setup.par_value_per_share = Some(Money::from_cents(100));
    setup.issuer_repurchase_enabled = true; setup.auction_ticks = 0; setup.closing_auction_ticks = 0;
    setup
}
#[test]
fn funds_p2_actual_driver_01_real_shared_treasury_nonzero_owner_credit_survives_next_start() {
    let mut session = GameSession::new(setup(), 101).unwrap();
    let owner = session.account_roster().into_iter().find(|id| session.account(*id).unwrap().kind() == AccountKind::IssuerRepurchase).unwrap();
    let mut book = session.take_runtime_plans();
    let ids = [0, 1].map(|stock| book.create_issuer_repurchase(crate::plans::RepurchaseExecutionOpen {
        owner, issuer: crate::company::CompanyId(stock), stock: StockId(stock),
    }).unwrap());
    session.set_runtime_plans(book);
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    assert_eq!(driver.account_by_owner((1, 1)), Some(owner));
    driver.world.with_admin_transaction(vec![(1, vec![
        AdminOperation::SyntheticBudgetCredit { local: 1, stock: 0, execution_plan: ids[0], amount: 101 },
        AdminOperation::SyntheticBudgetCredit { local: 1, stock: 1, execution_plan: ids[1], amount: 203 },
    ])], |_| Ok(())).unwrap();
    assert_eq!(session.account(owner).unwrap().cash(), Money::ZERO);
    driver.step_frame_equivalent(&mut session).unwrap();
    assert_eq!(session.account(owner).unwrap().cash(), Money::from_cents(304));
    driver.world.shutdown().unwrap();
}
#[test]
fn funds_p2_actual_driver_02_account_number_does_not_authorize_a_changed_real_kind() {
    let mut session = GameSession::new(setup(), 102).unwrap();
    let owner = session.account_roster().into_iter().find(|id| session.account(*id).unwrap().kind() == AccountKind::IssuerRepurchase).unwrap();
    session.fixture_account_mut(&owner).unwrap().fixture_set_kind(AccountKind::Player);
    let bindings = admin_treasury_bindings_from_session(&session, &session.account_roster(), 2).unwrap();
    assert!(bindings.iter().all(|binding| binding.account != owner));
    assert!(bindings.iter().all(|binding| binding.account != AccountId(0)));
}

#[test]
fn funds_p2_actual_driver_03_lazy_builder_binding_is_live_before_session_activation() {
    let mut session = GameSession::new(setup(), 103).unwrap();
    let owner = session.account_roster().into_iter().find(|id| session.account(*id).unwrap().kind() == AccountKind::IssuerRepurchase).unwrap();
    let mut book = session.take_runtime_plans();
    let id = book.create_issuer_repurchase(crate::plans::RepurchaseExecutionOpen {
        owner, issuer: crate::company::CompanyId(0), stock: StockId(0),
    }).unwrap(); session.set_runtime_plans(book);
    let expected_epoch = session.next_runtime_authority_epoch().unwrap();
    let old_revision = session.external_revision(); let old_plans = session.runtime_plans_snapshot().clone();
    let mut driver = RuntimeDriver::prepare_from_session_grouped(&session, 2, expected_epoch).unwrap();
    assert_eq!(driver.account_by_owner((1, 1)), Some(owner));
    let initial_source = driver.mirror_source(); let published = driver.world.publication().load();
    driver.world.with_admin_transaction(vec![(1, vec![AdminOperation::SyntheticBudgetCredit {
        local: 1, stock: 0, execution_plan: id, amount: 101,
    }])], |_| Ok(())).unwrap();
    let source = driver.mirror_source();
    assert_eq!((source.authority_epoch, source.tick, source.generation), (expected_epoch, initial_source.tick, initial_source.generation));
    assert_eq!(source.non_tick_revision, initial_source.non_tick_revision + 1);
    assert!(std::sync::Arc::ptr_eq(&published, &driver.world.publication().load()));
    let pending = driver.pending_mirror_domains(); assert!(pending.accounts_full);
    let input = driver.prepare_static_driver_projection(&session).unwrap();
    assert_eq!(input.static_read.as_ref().unwrap().source, source);
    assert_eq!(input.accounts[1].iter().find(|(local, _)| *local == 1).unwrap().1.cash, 101);
    assert_eq!(driver.pending_mirror_domains(), pending, "静点读取不消费行政待刷原因");
    assert_eq!(session.account(owner).unwrap().cash(), Money::ZERO);
    assert_eq!(session.next_runtime_authority_epoch().unwrap(), expected_epoch);
    assert_eq!(session.external_revision(), old_revision);
    assert_eq!(session.runtime_plans_snapshot(), &old_plans);
    driver.world.shutdown().unwrap();
}
#[test]
fn funds_p2_actual_driver_04_lazy_builder_cannot_fund_same_number_with_player_kind() {
    let mut session = GameSession::new(setup(), 104).unwrap();
    let owner = session.account_roster().into_iter().find(|id| session.account(*id).unwrap().kind() == AccountKind::IssuerRepurchase).unwrap();
    session.fixture_account_mut(&owner).unwrap().fixture_set_kind(AccountKind::Player);
    let mut book = session.take_runtime_plans();
    let id = book.create_issuer_repurchase(crate::plans::RepurchaseExecutionOpen {
        owner, issuer: crate::company::CompanyId(0), stock: StockId(0),
    }).unwrap(); session.set_runtime_plans(book);
    let epoch = session.next_runtime_authority_epoch().unwrap();
    let mut driver = RuntimeDriver::prepare_from_session_grouped(&session, 2, epoch).unwrap();
    let before = driver.mirror_source(); let pending = driver.pending_mirror_domains();
    let error = driver.world.with_admin_transaction(vec![(1, vec![AdminOperation::SyntheticBudgetCredit {
        local: 1, stock: 0, execution_plan: id, amount: 101,
    }])], |_| -> Result<(), crate::runtime::protocol::RuntimeError> { panic!("实际 Player kind 不得到达信用 inspect") }).unwrap_err();
    assert!(error.to_string().contains("binding"), "{error}");
    assert_eq!(driver.mirror_source(), before); assert_eq!(driver.pending_mirror_domains(), pending);
    assert_eq!(session.account(owner).unwrap().cash(), Money::ZERO);
    driver.world.shutdown().unwrap();
}
