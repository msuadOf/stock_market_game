//! 真实 Session 四分类的 tests-first；首红分类必须是明确 stub panic，不能冒业务断言红。
use super::*;
use crate::company::CompanyErrorClass;
use crate::session::StepFatal;
use std::error::Error;

fn preserve_source(error: &PlayerCompanyCommandError, expected: CompanyErrorClass) {
    let original = error.source().expect("真实拒绝必须保留 typed source");
    assert_eq!(error.classification(), expected);
    assert!(
        std::ptr::eq(original, error.source().unwrap()),
        "分类不能替换或消费 boxed cause"
    );
    assert!(!error.to_string().is_empty());
}

#[test]
fn player_command_unknown_issuer_is_invalid_input_with_original_source() {
    let mut game = game(true);
    let error = game
        .propose_player_company_action(
            AccountId(0),
            PlayerCompanyProposal::CashDividend {
                company: CompanyId(999),
                gross_per_share: Money::from_cents(1),
            },
        )
        .unwrap_err();
    preserve_source(&error, CompanyErrorClass::InvalidInput);
    assert_eq!(error.company, CompanyId(999));
    assert!(game.require_healthy().is_ok());
}

#[test]
fn player_command_no_holding_is_business_condition_with_original_source() {
    let mut game = game(false);
    let error = game
        .propose_player_company_action(AccountId(0), cash(1))
        .unwrap_err();
    preserve_source(&error, CompanyErrorClass::BusinessCondition);
    assert!(error.to_string().contains("总持仓为零"));
    assert!(game.require_healthy().is_ok());
}

#[test]
fn player_command_legacy_mode_is_unsupported_with_original_source() {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.company_system = None;
    setup.company_operations = None;
    let mut game = GameSession::new_with_runtime_groups(setup, 7181, 3).unwrap();
    let error = game
        .propose_player_company_action(AccountId(0), cash(1))
        .unwrap_err();
    preserve_source(&error, CompanyErrorClass::UnsupportedOperation);
    assert!(game.require_healthy().is_ok());
}

#[test]
fn player_command_poison_is_system_state_and_remains_fatal() {
    let mut game = game(true);
    let fatal = StepFatal::InvariantViolation {
        location: "公司命令分类夹具".into(),
        description: "真实健康门拒绝".into(),
    };
    game.poison_runtime_step(fatal.clone());
    let error = game
        .propose_player_company_action(AccountId(0), cash(1))
        .unwrap_err();
    preserve_source(&error, CompanyErrorClass::SystemState);
    assert_eq!(
        error.source().unwrap().downcast_ref::<StepFatal>(),
        Some(&fatal)
    );
    assert_eq!(game.poison_reason(), Some(&fatal));
}


#[test]
fn player_command_invalid_amount_and_disabled_mechanism_have_typed_classes() {
    let mut game = game(true);
    let invalid = game.propose_player_company_action(AccountId(0), cash(0)).unwrap_err();
    preserve_source(&invalid, CompanyErrorClass::InvalidInput);
    let disabled = game.propose_player_company_action(AccountId(0), repurchase()).unwrap_err();
    preserve_source(&disabled, CompanyErrorClass::UnsupportedOperation);
    assert!(game.require_healthy().is_ok());
}

#[test]
fn player_command_repurchase_share_limit_is_business_condition_without_funding() {
    let mut game = game_with_features(true, false, true);
    let before = serde_json::to_value(game.save().unwrap()).unwrap();
    let mut proposal = repurchase();
    let PlayerCompanyProposal::IssuerRepurchase { max_shares, .. } = &mut proposal else {
        unreachable!()
    };
    *max_shares = 1_001;
    let error = game.propose_player_company_action(AccountId(0), proposal).unwrap_err();
    preserve_source(&error, CompanyErrorClass::BusinessCondition);
    assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
    assert!(game.require_healthy().is_ok());
}
