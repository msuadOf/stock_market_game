//! 玩家提案必须通过真实 Session 命令受理；批准不冒充未来登记、付款或股份到账。

#[path = "publications/session_fixture.rs"]
mod session_fixture;
include!("../test-support/simple_company.rs");

use engine::company::CompanyId;
use engine::session::{PlayerCompanyCommandReceipt, PlayerCompanyProposal};
use engine::{AccountId, CivilDate, GameSession, Money};

fn session() -> GameSession {
    let mut setup = session_fixture::civil_setup(CivilDate::from_iso("2030-06-03").unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 1;
    setup.company_system = Some(simple_company_fixture!(engine; ["600101"]));
    setup.dividend_tax_mode =
        Some(engine::company::cash_dividend_tax::CashDividendTaxMode::AShareIndividual);
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup.auto_corporate_foundation = true;
    GameSession::new_with_runtime_groups(setup, 7, 3).unwrap()
}

#[test]
fn player_company_command_is_registered_on_the_real_session() {
    let _entry = GameSession::propose_player_company_action;
}

#[test]
fn player_company_command_unknown_account_preserves_company_and_accounts() {
    let mut session = session();
    let before = serde_json::to_value(session.save().unwrap()).unwrap();
    let error = session
        .propose_player_company_action(
            AccountId(999),
            PlayerCompanyProposal::CashDividend {
                company: CompanyId(0),
                gross_per_share: Money::from_cents(1),
            },
        )
        .unwrap_err();
    assert_eq!(error.account(), AccountId(999));
    assert_eq!(
        serde_json::to_value(session.save().unwrap()).unwrap(),
        before
    );
}

#[test]
fn player_company_command_receipt_is_the_actual_return_type() {
    fn receipt(_: PlayerCompanyCommandReceipt) {}
    let _receipt = receipt;
}
