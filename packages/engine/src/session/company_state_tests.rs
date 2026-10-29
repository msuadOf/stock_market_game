use super::*;
use crate::calendar::CivilDate;
use crate::company::config::CompanySystemConfig;
use crate::company::events::ShockParams;
use crate::company::operations::CompanyOperationsConfig;
use crate::company::simple::period::{PeriodNoiseConfig, SettlementCycle};
use crate::company::simple::{SimpleConfig, SimpleEnvironmentConfig};
use crate::company::{CompanyCatalog, CompanyId};

fn date() -> CivilDate {
    CivilDate::from_iso("2030-06-03").unwrap()
}

#[test]
fn selected_company_state_keeps_a_compact_session_envelope() {
    let size = std::mem::size_of::<SessionCompanies>();
    assert!(
        size <= 128,
        "SessionCompanies 占 {size} 字节，超过 128 字节运输预算"
    );
}

fn system_state() -> SessionCompanies {
    let system = CompanySystem::create(
        vec![],
        CompanySystemConfig::Simple(SimpleConfig {
            environment: SimpleEnvironmentConfig {
                initial_change_bp: 0,
                persistence_bp: 0,
                noise: PeriodNoiseConfig {
                    monthly_bp: 0,
                    quarterly_bp: 0,
                    half_year_bp: 0,
                    annual_bp: 0,
                },
            },
            companies: vec![],
            prehistory_periods: 1,
            settlement_cycle: SettlementCycle::Monthly,
        }),
        date(),
        19,
    )
    .unwrap();
    system.validate_restored().unwrap();
    SessionCompanies::CompanySystem(Box::new(SystemCompanyState {
        system: Arc::new(system),
        corporate_actions: SessionCorporateActions::default(),
    }))
}

fn legacy_state() -> SessionCompanies {
    let operations = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 19,
            shock_params: ShockParams::current_default_parameters(),
            catalog: CompanyCatalog::from_codes(&[]).unwrap(),
            companies: vec![],
        },
        date(),
    )
    .unwrap();
    SessionCompanies::LegacyOperations(LegacyCompanyState {
        registry: Arc::new(CompanyRegistry::new(vec![]).unwrap()),
        operations: Arc::new(operations),
        closing: Arc::new(ClosingEngine::new()),
        groups: vec![],
    })
}

#[test]
fn state_01_legacy_save_has_only_actual_selected_domain() {
    let state = legacy_state();
    assert!(state.company_system().is_none());
    assert!(state.corporate_actions().is_none());
    let saved = state.save();
    let value = serde_json::to_value(&saved).unwrap();
    let root = value.as_object().unwrap();
    assert_eq!(root.len(), 1);
    let legacy = root["LegacyOperations"].as_object().unwrap();
    assert_eq!(
        legacy.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["closing", "groups", "operations"]
    );
    assert_eq!(
        serde_json::from_value::<SavedSessionCompanies>(value).unwrap(),
        saved
    );
}

#[test]
fn state_02_system_save_has_only_actual_selected_domain() {
    let state = system_state();
    assert!(state.company_system().is_some());
    assert!(state.corporate_actions().is_some());
    let saved = state.save();
    let value = serde_json::to_value(&saved).unwrap();
    let root = value.as_object().unwrap();
    assert_eq!(root.len(), 1);
    let system = root["CompanySystem"].as_object().unwrap();
    assert_eq!(
        system.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["corporate_actions", "system"]
    );
    assert_eq!(
        serde_json::from_value::<SavedSessionCompanies>(value).unwrap(),
        saved
    );
}

#[test]
fn state_03_variant_payload_rejects_unknown_and_cross_source_fields() {
    for (state, tag, field) in [
        (legacy_state(), "LegacyOperations", "system"),
        (system_state(), "CompanySystem", "operations"),
    ] {
        for key in [field, "unexpected"] {
            let mut value = serde_json::to_value(state.save()).unwrap();
            value[tag][key] = serde_json::Value::Null;
            assert!(
                serde_json::from_value::<SavedSessionCompanies>(value).is_err(),
                "{tag}.{key}"
            );
        }
    }
}

#[test]
fn state_04_tag_rejects_null_unknown_and_multiple_variants() {
    let mut both = serde_json::to_value(legacy_state().save()).unwrap();
    both.as_object_mut().unwrap().insert(
        "CompanySystem".into(),
        serde_json::to_value(system_state().save()).unwrap()["CompanySystem"].clone(),
    );
    for value in [
        serde_json::Value::Null,
        serde_json::json!({}),
        serde_json::json!({"Unknown":{}}),
        both,
    ] {
        assert!(serde_json::from_value::<SavedSessionCompanies>(value).is_err());
    }
}

#[test]
fn state_05_every_variant_payload_field_is_required_and_nonnull() {
    for (state, tag) in [
        (legacy_state(), "LegacyOperations"),
        (system_state(), "CompanySystem"),
    ] {
        let value = serde_json::to_value(state.save()).unwrap();
        for key in value[tag].as_object().unwrap().keys() {
            let mut missing = value.clone();
            missing[tag].as_object_mut().unwrap().remove(key);
            assert!(
                serde_json::from_value::<SavedSessionCompanies>(missing).is_err(),
                "缺{tag}.{key}"
            );
            let mut null = value.clone();
            null[tag][key] = serde_json::Value::Null;
            assert!(
                serde_json::from_value::<SavedSessionCompanies>(null).is_err(),
                "null {tag}.{key}"
            );
        }
    }
}

#[test]
fn state_06_real_group_structure_rejects_unknown_nested_fields() {
    let mut value = serde_json::to_value(legacy_state().save()).unwrap();
    value["LegacyOperations"]["groups"] = serde_json::json!([{
        "root": 0, "holdings": [], "unexpected": true,
    }]);
    assert!(serde_json::from_value::<SavedSessionCompanies>(value).is_err());
}

#[test]
fn state_07_real_group_holding_rejects_unknown_nested_fields() {
    let mut value = serde_json::to_value(legacy_state().save()).unwrap();
    value["LegacyOperations"]["groups"] = serde_json::json!([{
        "root": 0, "holdings": [{"company": 1, "parent_held_shares": 1, "unexpected": true}],
    }]);
    assert!(serde_json::from_value::<SavedSessionCompanies>(value).is_err());
}

#[test]
fn state_08_group_required_fields_and_units_stay_explicit() {
    let group = GroupStructure {
        root: CompanyId(0),
        holdings: vec![crate::session::GroupHolding {
            company: CompanyId(1),
            parent_held_shares: 100,
        }],
    };
    let value = serde_json::to_value(&group).unwrap();
    assert_eq!(
        value,
        serde_json::json!({"root":0,"holdings":[{"company":1,"parent_held_shares":100}]})
    );
    assert_eq!(
        serde_json::from_value::<GroupStructure>(value).unwrap(),
        group
    );
    for invalid in [
        serde_json::json!({"root":0}),
        serde_json::json!({"root":0,"holdings":[{"company":1}]}),
        serde_json::json!({"root":0,"holdings":[{"company":1,"parent_held_shares":-1}]}),
    ] {
        assert!(serde_json::from_value::<GroupStructure>(invalid).is_err());
    }
}

#[test]
fn state_09_real_legacy_operations_rejects_unknown_fields() {
    let mut value = serde_json::to_value(legacy_state().save()).unwrap();
    value["LegacyOperations"]["operations"]["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SavedSessionCompanies>(value).is_err());
}

#[test]
fn state_10_legacy_nullable_facts_are_required_fields() {
    for field in ["next_expected", "history"] {
        let mut value = serde_json::to_value(legacy_state().save()).unwrap();
        value["LegacyOperations"]["operations"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(
            serde_json::from_value::<SavedSessionCompanies>(value).is_err(),
            "缺少 {field}"
        );
    }
}

#[test]
fn state_11_real_closing_engine_rejects_unknown_fields() {
    let mut value = serde_json::to_value(legacy_state().save()).unwrap();
    value["LegacyOperations"]["closing"]["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SavedSessionCompanies>(value).is_err());
}
