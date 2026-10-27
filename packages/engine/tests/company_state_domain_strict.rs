//! 公司互斥存档直接复用的真实 domain serde 必须拒绝缺字段和未知字段。
use engine::accounting::closing::ClosingEngine;
use engine::company::events::ShockParams;
use engine::company::operations::{CompanyOperations, CompanyOperationsConfig};
use engine::company::CompanyCatalog;

fn operations() -> CompanyOperations {
    CompanyOperations::new(CompanyOperationsConfig {
        seed: 19, shock_params: ShockParams::current_default_parameters(),
        catalog: CompanyCatalog::from_codes(&[]).unwrap(), companies: vec![],
    }, engine::CivilDate::from_iso("2030-06-03").unwrap()).unwrap()
}

#[test]
fn company_operations_rejects_unknown_saved_fields() {
    let mut value = serde_json::to_value(operations()).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<CompanyOperations>(value).is_err());
}

#[test]
fn company_operations_requires_nullable_next_expected_and_history() {
    for field in ["next_expected", "history"] {
        let mut value = serde_json::to_value(operations()).unwrap();
        value.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<CompanyOperations>(value).is_err(), "缺少 {field}");
    }
}

#[test]
fn closing_engine_rejects_unknown_saved_fields() {
    let mut value = serde_json::to_value(ClosingEngine::new()).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ClosingEngine>(value).is_err());
}
