use super::*;

fn simple_setup() -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    if let crate::company::config::CompanySystemConfig::Simple(config) = &mut setup.company_system {
        config.prehistory_periods = 12;
    }
    setup
}

#[test]
fn issuer_identity_uses_explicit_company_kind_instead_of_stock_code() {
    for kind in [crate::company::CompanyKind::Bank, crate::company::CompanyKind::Insurance, crate::company::CompanyKind::RealEstate] {
        let mut setup = simple_setup();
        if let crate::company::config::CompanySystemConfig::Simple(config) = &mut setup.company_system {
            config.companies[0].kind = kind;
        }
        let specs = company_assembly::issuer_specs(&setup).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].kind, kind);
        assert_eq!(specs[0].listed_stock, Some(setup.stocks[0].code.clone()));
        assert_eq!(specs[0].issued_shares, setup.stocks[0].total_shares);
    }
}

#[test]
fn issuer_identity_rejects_missing_duplicate_configuration_and_unavailable_simulation() {
    let setup = simple_setup();
    for duplicate in [false, true] {
        let mut changed = setup.clone();
        if let crate::company::config::CompanySystemConfig::Simple(config) = &mut changed.company_system {
            if duplicate {
                config.companies.push(config.companies[0].clone());
            } else {
                config.companies.clear();
            }
        }
        assert!(matches!(company_assembly::issuer_specs(&changed), Err(SessionError::InvalidSetup(_))));
    }
    let mut unavailable = setup;
    unavailable.company_system = crate::company::config::CompanySystemConfig::Simulation;
    assert!(matches!(company_assembly::issuer_specs(&unavailable), Err(SessionError::InvalidSetup(message)) if message.contains("Simulation")));
}

#[test]
fn explicit_financial_kinds_publish_matching_policy_and_restore_actual_session() {
    use crate::accounting::{AccountingAmount, JournalLine, LedgerAccountId, PostingSide};
    for (kind, cash_code, chart_version) in [
        (crate::company::CompanyKind::Bank, "1003", 3),
        (crate::company::CompanyKind::Insurance, "1002", 4),
        (crate::company::CompanyKind::RealEstate, "1002", 5),
    ] {
        let mut setup = simple_setup();
        if let crate::company::config::CompanySystemConfig::Simple(config) = &mut setup.company_system {
            config.companies[0].kind = kind;
            config.companies[0].finance.opening_lines = vec![
                JournalLine { account: LedgerAccountId(cash_code.into()), side: PostingSide::Debit, amount: AccountingAmount::from_cents(500_000_000) },
                JournalLine { account: LedgerAccountId("4001".into()), side: PostingSide::Credit, amount: AccountingAmount::from_cents(500_000_000) },
            ];
        }
        let session = GameSession::new(setup, 42).unwrap();
        let save = session.save().unwrap();
        let company = crate::company::CompanyId(format!("C-{}", save.setup.stocks[0].code.0));
        assert_eq!(save.company_system.issuers().get(&company).unwrap().kind, kind);
        let public = save.public_library.save();
        assert!(!public.reports.is_empty());
        assert!(public.reports.iter().all(|report| report.company == company && report.policy.chart_version == chart_version));
        let restored = GameSession::restore(&save).unwrap();
        assert_eq!(restored.business_state_hash().unwrap(), session.business_state_hash().unwrap());
    }
}

#[test]
fn session_setup_rejects_legacy_company_fields_without_compatibility() {
    for field in ["company_operations", "groups"] {
        let mut wire = serde_json::to_value(simple_setup()).unwrap();
        wire[field] = serde_json::json!(null);
        assert!(serde_json::from_value::<SessionSetup>(wire).is_err());
    }
}

#[test]
fn restore_rejects_company_config_and_civil_date_drift() {
    let session = GameSession::new(simple_setup(), 42).unwrap();
    let save = session.save().unwrap();
    let mut changed = save.clone();
    if let crate::company::config::CompanySystemConfig::Simple(config) = &mut changed.setup.company_system {
        config.environment.noise.monthly_bp += 1;
    }
    assert!(matches!(GameSession::restore(&changed), Err(SessionError::InvalidSave(message)) if message.contains("配置")));
    let mut wire = serde_json::to_value(&save).unwrap();
    wire["company_system"]["implementation"]["state"]["advanced_through"] = serde_json::json!("2030-01-01");
    let edited: SaveSlot = serde_json::from_value(wire).unwrap();
    assert!(matches!(GameSession::restore(&edited), Err(SessionError::InvalidSave(message)) if message.contains("日期")));
}

#[test]
fn simple_restore_rejects_foreign_model_publication_source() {
    let session = GameSession::new(simple_setup(), 42).unwrap();
    let mut save = session.save().unwrap();
    let mut library = save.public_library.save();
    assert!(!library.reports.is_empty());
    library.reports[0].source = crate::information::PublicationSource::SimulationAccounting;
    save.public_library = crate::information::PublicLibrary::from_parts(library).unwrap();
    assert!(matches!(GameSession::restore(&save), Err(SessionError::InvalidSave(message)) if message.contains("来源")));
}

#[test]
fn simple_session_save_has_selected_system_without_financial_simulation() {
    let session = GameSession::new(simple_setup(), 42).unwrap();
    let save = serde_json::to_value(session.save().unwrap()).unwrap();
    assert_eq!(save["company_system"]["implementation"]["mode"], "Simple");
    for field in ["company_operations", "closing_registry", "ops_wiring"] {
        assert!(save.get(field).is_none(), "Simple 存档不得携带 {field}");
    }
    assert_eq!(save["company_system"]["implementation"]["state"]["advanced_through"], "2029-12-31");
}

#[test]
fn session_setup_requires_explicit_company_model_without_legacy_fallback() {
    let mut setup = serde_json::to_value(simple_setup()).unwrap();
    setup.as_object_mut().unwrap().remove("company_system");
    assert!(serde_json::from_value::<SessionSetup>(setup).is_err());
}

#[test]
fn unavailable_simulation_is_rejected_before_creating_session() {
    let mut setup = serde_json::to_value(simple_setup()).unwrap();
    setup["company_system"] = serde_json::json!({ "mode": "Simulation" });
    let parsed: SessionSetup = serde_json::from_value(setup).unwrap();
    assert!(matches!(GameSession::new(parsed, 42), Err(SessionError::InvalidSetup(message)) if message.contains("Simulation")));
}

#[test]
fn simple_day_end_advances_selected_state_and_restore_preserves_it() {
    let mut session = GameSession::new(simple_setup(), 42).unwrap();
    session.end_civil_day().unwrap();
    let save = session.save().unwrap();
    let wire = serde_json::to_value(&save).unwrap();
    assert_eq!(wire["company_system"]["implementation"]["state"]["advanced_through"], "2030-01-01");
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(restored.business_state_hash().unwrap(), session.business_state_hash().unwrap());
}

#[test]
fn monthly_intraday_checkpoint_restores_without_becoming_public_day_end_archive() {
    let mut setup = simple_setup();
    setup.report_frequency = crate::information::ReportFrequency::Monthly {
        schedule: crate::information::MonthlyReportSchedule::Custom {
            day: 2,
            second_of_day: 37800,
            delay: crate::information::MonthlyReportDelay::None,
        },
    };
    let mut session = GameSession::new(setup, 42).unwrap();
    session.end_civil_day().unwrap();
    session.step().unwrap();
    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(restored.business_state_hash().unwrap(), session.business_state_hash().unwrap());
    assert!(crate::session::protocol::ProtocolSession::restore(&save).is_err());
    for future_second in [37800, 54000, 86399] {
        let mut wire = serde_json::to_value(&save).unwrap();
        wire["disclosures"]["published_through"] = serde_json::to_value(CivilInstant::new(session.civil_date(), future_second).unwrap()).unwrap();
        let edited: SaveSlot = serde_json::from_value(wire).unwrap();
        assert!(matches!(GameSession::restore(&edited), Err(SessionError::InvalidSave(message)) if message.contains("disclosure cursor")));
    }
}

#[test]
fn disclosure_checkpoint_handles_open_close_and_closed_day_without_relaxing_public_archive() {
    for frequency in [
        crate::information::ReportFrequency::Quarterly,
        crate::information::ReportFrequency::Monthly {
            schedule: crate::information::MonthlyReportSchedule::Custom {
                day: 2,
                second_of_day: 37800,
                delay: crate::information::MonthlyReportDelay::None,
            },
        },
    ] {
        let mut setup = simple_setup();
        setup.start_date = CivilDate::from_ymd(2030, 1, 4).unwrap();
        setup.ticks_per_day = 2;
        setup.npcs.inst_count = 0;
        if let crate::company::config::CompanySystemConfig::Simple(config) = &mut setup.company_system {
            config.prehistory_periods = 0;
        }
        setup.report_frequency = frequency;
        let mut session = GameSession::new(setup, 42).unwrap();
        let initial = session.save().unwrap();
        assert!(GameSession::restore(&initial).is_ok());
        for _ in 0..2 {
            session.step().unwrap();
            let checkpoint = session.save().unwrap();
            let restored = GameSession::restore(&checkpoint).unwrap();
            assert_eq!(restored.business_state_hash().unwrap(), session.business_state_hash().unwrap());
            assert!(crate::session::protocol::ProtocolSession::restore(&checkpoint).is_err());
        }
        for _ in 0..2 {
            session.end_civil_day().unwrap();
            assert_eq!(session.civil_clock().phase(), CivilPhase::ClosedDay);
            let archive = session.save().unwrap();
            assert!(GameSession::restore(&archive).is_ok());
            assert!(crate::session::protocol::ProtocolSession::restore(&archive).is_ok());
        }
    }
}
