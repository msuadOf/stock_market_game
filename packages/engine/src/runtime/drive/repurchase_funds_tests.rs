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
