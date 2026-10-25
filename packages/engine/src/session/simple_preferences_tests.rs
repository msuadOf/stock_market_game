//! Simple 偏好编辑只提交配置；不会重建市场 runtime 或改写个人事实。

use super::*;
use crate::company::config::CompanySystemConfig;
use crate::company::simple::{
    SimpleCashDividendPreference, SimpleCompanyPreferences, SimpleStockDistributionPreference,
};
include!("../../test-support/simple_company.rs");

const COMPANY: CompanyId = CompanyId(0);

fn setup() -> SessionSetup {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    let mut config = simple_company_fixture!(crate; ["600888"]);
    let CompanySystemConfig::Simple(simple) = &mut config else {
        unreachable!()
    };
    simple.prehistory_periods = 2;
    setup.company_system = Some(config);
    setup.dividend_tax_mode = Some(crate::company::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1_000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup.history_len = 1;
    setup
}

fn preferences() -> SimpleCompanyPreferences {
    SimpleCompanyPreferences {
        cash_dividend: Some(SimpleCashDividendPreference {
            target_payout_bp: 3_000,
            min_distributable_profit: Money::from_cents(100_000_000),
            cycles_between_proposals: 2,
        }),
        stock_distribution: Some(SimpleStockDistributionPreference {
            min_distributable_profit: Money::from_cents(200_000_000),
            shares_per_existing_share_micros: 100_000,
            max_cumulative_expansion_micros: 1_000_000,
            cycles_between_proposals: 3,
        }),
    }
}

fn saved_bytes(game: &GameSession) -> Vec<u8> {
    serde_json::to_vec(&game.save().unwrap()).unwrap()
}

#[test]
fn simple_preferences_read_is_pure_and_edit_only_changes_both_configs() {
    let mut game = GameSession::new(setup(), 42).unwrap();
    game.step().unwrap();
    assert!(
        game.runtime_driver.is_some(),
        "夹具必须已建立常驻 runtime driver"
    );
    let publication = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .publication();
    let revision = game.external_revision();
    let mut before = game.save().unwrap();
    assert_eq!(
        game.company_simple_preferences(&COMPANY).unwrap(),
        SimpleCompanyPreferences::none()
    );
    assert_eq!(saved_bytes(&game), serde_json::to_vec(&before).unwrap());
    assert_eq!(game.external_revision(), revision);
    game.set_simple_preferences(&COMPANY, preferences())
        .unwrap();
    assert_eq!(
        game.company_simple_preferences(&COMPANY).unwrap(),
        preferences()
    );
    assert_eq!(game.external_revision(), revision + 1);
    assert!(std::sync::Arc::ptr_eq(
        &publication,
        &game
            .runtime_driver
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .publication()
    ));
    let after = game.save().unwrap();
    before
        .company_system
        .as_mut()
        .unwrap()
        .set_simple_preferences(&COMPANY, preferences())
        .unwrap();
    let Some(CompanySystemConfig::Simple(config)) = &mut before.setup.company_system else {
        unreachable!()
    };
    config.companies[0].preferences = preferences();
    assert_eq!(
        serde_json::to_vec(&after).unwrap(),
        serde_json::to_vec(&before).unwrap(),
        "资产、公司行为账簿、个人信息与计划均不能随配置编辑变化"
    );
    let restored = GameSession::restore(&after).unwrap();
    assert_eq!(saved_bytes(&restored), serde_json::to_vec(&after).unwrap());
    assert_eq!(
        restored.company_simple_preferences(&COMPANY).unwrap(),
        preferences()
    );
    game.set_simple_preferences(&COMPANY, SimpleCompanyPreferences::none())
        .unwrap();
    assert_eq!(
        game.company_simple_preferences(&COMPANY).unwrap(),
        SimpleCompanyPreferences::none()
    );
    game.step().unwrap();
    assert_eq!(game.tick(), 2, "编辑后继续使用同一市场 runtime 步进");
    game.step().unwrap();
    assert_eq!(game.tick(), 3, "编辑后至少连续两个真实市场 step");
    assert!(std::sync::Arc::ptr_eq(
        &publication,
        &game
            .runtime_driver
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .publication()
    ));
    assert_eq!(
        game.company_simple_preferences(&COMPANY).unwrap(),
        SimpleCompanyPreferences::none()
    );
}

#[test]
fn simple_preferences_accepts_domain_boundaries_and_counts_each_success_once() {
    let mut game = GameSession::new(setup(), 42).unwrap();
    let revision = game.external_revision();
    for (index, maximum) in [false, true].into_iter().enumerate() {
        let p = SimpleCompanyPreferences {
            cash_dividend: Some(SimpleCashDividendPreference {
                target_payout_bp: if maximum { 10_000 } else { 1 },
                min_distributable_profit: Money::from_cents(if maximum { i64::MAX } else { 1 }),
                cycles_between_proposals: if maximum { u16::MAX } else { 1 },
            }),
            stock_distribution: Some(SimpleStockDistributionPreference {
                min_distributable_profit: Money::from_cents(if maximum { i64::MAX } else { 1 }),
                shares_per_existing_share_micros: if maximum { 10_000_000 } else { 1 },
                max_cumulative_expansion_micros: if maximum { 1_000_000_000 } else { 1 },
                cycles_between_proposals: if maximum { u16::MAX } else { 1 },
            }),
        };
        game.set_simple_preferences(&COMPANY, p.clone()).unwrap();
        assert_eq!(game.company_simple_preferences(&COMPANY).unwrap(), p);
        assert_eq!(game.external_revision(), revision + index as u64 + 1);
    }
    let same = game.company_simple_preferences(&COMPANY).unwrap();
    game.set_simple_preferences(&COMPANY, same).unwrap();
    assert_eq!(
        game.external_revision(),
        revision + 3,
        "重复提交合法配置也是一次已受理直改"
    );
}

#[test]
fn simple_preferences_invalid_fields_and_unknown_company_are_atomic() {
    let mut game = GameSession::new(setup(), 42).unwrap();
    let before = saved_bytes(&game);
    let revision = game.external_revision();
    let invalid: Vec<fn(&mut SimpleCompanyPreferences)> = vec![
        |p| p.cash_dividend.as_mut().unwrap().target_payout_bp = 0,
        |p| p.cash_dividend.as_mut().unwrap().target_payout_bp = 10_001,
        |p| p.cash_dividend.as_mut().unwrap().min_distributable_profit = Money::ZERO,
        |p| p.cash_dividend.as_mut().unwrap().min_distributable_profit = Money::from_cents(-1),
        |p| p.cash_dividend.as_mut().unwrap().cycles_between_proposals = 0,
        |p| {
            p.stock_distribution
                .as_mut()
                .unwrap()
                .min_distributable_profit = Money::ZERO
        },
        |p| {
            p.stock_distribution
                .as_mut()
                .unwrap()
                .min_distributable_profit = Money::from_cents(-1)
        },
        |p| {
            p.stock_distribution
                .as_mut()
                .unwrap()
                .shares_per_existing_share_micros = 0
        },
        |p| {
            p.stock_distribution
                .as_mut()
                .unwrap()
                .shares_per_existing_share_micros = 10_000_001
        },
        |p| {
            p.stock_distribution
                .as_mut()
                .unwrap()
                .max_cumulative_expansion_micros = 0
        },
        |p| {
            p.stock_distribution
                .as_mut()
                .unwrap()
                .max_cumulative_expansion_micros = 1_000_000_001
        },
        |p| {
            p.stock_distribution
                .as_mut()
                .unwrap()
                .cycles_between_proposals = 0
        },
    ];
    for invalidate in invalid {
        let mut p = preferences();
        invalidate(&mut p);
        let expected = p.validate().unwrap_err().to_string();
        let error = game.set_simple_preferences(&COMPANY, p).unwrap_err();
        assert_eq!(error.to_string(), expected, "必须保留域校验的具体原因");
        assert!(
            matches!(
                error,
                SimplePreferencesError::Company(crate::company::CompanySystemError::Invalid(_))
            ),
            "保留域错误类型，不能压平成任意 fault 或文案"
        );
        assert_eq!(saved_bytes(&game), before);
        assert_eq!(game.external_revision(), revision);
    }
    let unknown = CompanyId(u32::MAX);
    assert!(game
        .company_simple_preferences(&unknown)
        .unwrap_err()
        .to_string()
        .contains(&u32::MAX.to_string()));
    assert!(game
        .set_simple_preferences(&unknown, preferences())
        .is_err());
    assert_eq!(saved_bytes(&game), before);
    assert_eq!(game.external_revision(), revision);
}

#[test]
fn simple_preferences_rejects_each_one_sided_presence_without_installing_candidates() {
    for missing_system in [false, true] {
        let mut game = GameSession::new(setup(), 42).unwrap();
        if missing_system {
            game.state.company_system = None;
        } else {
            game.state.setup.company_system = None;
        }
        let before = saved_bytes(&game);
        let revision = game.external_revision();
        assert!(matches!(
            game.company_simple_preferences(&COMPANY),
            Err(SimplePreferencesError::Company(
                crate::company::CompanySystemError::SystemState(_)
            ))
        ));
        assert!(game
            .set_simple_preferences(&COMPANY, preferences())
            .is_err());
        assert_eq!(saved_bytes(&game), before);
        assert_eq!(game.external_revision(), revision);
        assert!(super::persistence::validate_save_slot(&game.save().unwrap()).is_err());
        assert!(GameSession::restore(&game.save().unwrap()).is_err());
    }
}

#[test]
fn simple_preferences_absent_poisoned_and_torn_config_fail_without_mutation() {
    let mut absent = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let before = saved_bytes(&absent);
    let revision = absent.external_revision();
    assert!(absent.company_simple_preferences(&COMPANY).is_err());
    assert!(absent
        .set_simple_preferences(&COMPANY, preferences())
        .is_err());
    assert_eq!(saved_bytes(&absent), before);
    assert_eq!(absent.external_revision(), revision);
    let mut game = GameSession::new(setup(), 42).unwrap();
    let Some(CompanySystemConfig::Simple(config)) = &mut game.state.setup.company_system else {
        unreachable!()
    };
    config.companies[0].preferences = preferences();
    let before = saved_bytes(&game);
    let revision = game.external_revision();
    assert!(game
        .set_simple_preferences(&COMPANY, SimpleCompanyPreferences::none())
        .is_err());
    assert_eq!(saved_bytes(&game), before);
    assert_eq!(game.external_revision(), revision);
    let fatal = StepFatal::InvariantViolation {
        description: "偏好编辑前已毒化".into(),
        location: "simple_preferences_tests".into(),
    };
    game.poison = Some(fatal.clone());
    assert!(
        matches!(game.company_simple_preferences(&COMPANY), Err(SimplePreferencesError::Step(source)) if source == fatal)
    );
    assert!(
        matches!(game.set_simple_preferences(&COMPANY, preferences()), Err(SimplePreferencesError::Step(source)) if source == fatal)
    );
    assert_eq!(game.external_revision(), revision);
}

#[test]
fn simple_preferences_restore_rejects_one_sided_config_edits_in_memory_and_json() {
    let game = GameSession::new(setup(), 42).unwrap();
    for edit_setup in [false, true] {
        let mut save = game.save().unwrap();
        if edit_setup {
            let Some(CompanySystemConfig::Simple(config)) = &mut save.setup.company_system else {
                unreachable!()
            };
            config.companies[0].preferences = preferences();
        } else {
            save.company_system
                .as_mut()
                .unwrap()
                .set_simple_preferences(&COMPANY, preferences())
                .unwrap();
        }
        assert!(matches!(super::persistence::validate_save_slot(&save),
            Err(SessionError::Step(StepFatal::InvariantViolation { location, description }))
                if location == "validate_save_slot.company_system_config"
                    && description.contains("company_system.config")
                    && description.contains("setup.company_system")));
        assert!(GameSession::restore(&save).is_err());
        let decoded: SaveSlot =
            serde_json::from_slice(&serde_json::to_vec(&save).unwrap()).unwrap();
        assert!(GameSession::restore(&decoded).is_err());
    }
}

#[test]
fn simple_preferences_protocol_checkpoint_rolls_back_config_with_personal_facts() {
    let mut protocol = protocol::ProtocolSession::new(setup(), 42).unwrap();
    let before = saved_bytes(protocol.game_mut());
    let revision = protocol.game_mut().external_revision();
    let checkpoint = protocol.checkpoint().unwrap();
    protocol
        .set_simple_preferences(&COMPANY, preferences())
        .unwrap();
    protocol.rollback(checkpoint);
    assert_eq!(
        protocol.company_simple_preferences(&COMPANY).unwrap(),
        SimpleCompanyPreferences::none()
    );
    assert_eq!(
        saved_bytes(protocol.game_mut()),
        before,
        "checkpoint rollback 必须恢复配置、资产与个人事实"
    );
    assert_eq!(protocol.game_mut().external_revision(), revision);
}

#[test]
fn simple_preferences_protocol_edit_preserves_last_published_day_end_save() {
    let mut setup = setup();
    setup.start_date = crate::CivilDate::from_iso("2030-01-05").unwrap();
    let mut protocol = protocol::ProtocolSession::new(setup, 42).unwrap();
    protocol.end_civil_day_update().unwrap();
    let published = serde_json::to_vec(&protocol.save().unwrap()).unwrap();
    let revision = protocol.game_mut().external_revision();
    let before = saved_bytes(protocol.game_mut());
    assert!(protocol
        .set_simple_preferences(&CompanyId(u32::MAX), preferences())
        .is_err());
    assert_eq!(saved_bytes(protocol.game_mut()), before);
    assert_eq!(protocol.game_mut().external_revision(), revision);
    assert_eq!(
        serde_json::to_vec(&protocol.save().unwrap()).unwrap(),
        published
    );
    protocol
        .set_simple_preferences(&COMPANY, preferences())
        .unwrap();
    assert_eq!(
        protocol.company_simple_preferences(&COMPANY).unwrap(),
        preferences()
    );
    assert_eq!(
        serde_json::to_vec(&protocol.save().unwrap()).unwrap(),
        published
    );
    assert_eq!(
        protocol
            .game_mut()
            .company_simple_preferences(&COMPANY)
            .unwrap(),
        preferences()
    );
    protocol.end_civil_day_update().unwrap();
    let restored = GameSession::restore(&protocol.save().unwrap()).unwrap();
    assert_eq!(
        restored.company_simple_preferences(&COMPANY).unwrap(),
        preferences()
    );
}
