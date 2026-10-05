#[allow(dead_code)]
#[path = "../company_operations/fixtures.rs"]
mod operating_fixture;

use engine::company::operations::{CompanyOperations, CompanyOperationsConfig};

fn bank_save() -> serde_json::Value {
    let start = operating_fixture::d("2030-01-01");
    let company = operating_fixture::bank_c(start);
    let mut operations = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: super::SEED,
            shock_params: operating_fixture::quiet_params(),
            companies: vec![company],
        },
        start,
    )
    .unwrap();
    operations.advance_civil_day(start).unwrap();
    serde_json::to_value(operations).unwrap()
}

#[test]
fn independent_operations_restore_rejects_invalid_bank_ecl_with_company_context() {
    let baseline = bank_save();
    let valid: CompanyOperations = serde_json::from_value(baseline.clone()).unwrap();
    assert_eq!(serde_json::to_value(valid).unwrap(), baseline);
    for (table, field, value) in [
        ("stage1_default", None, serde_json::json!([])),
        ("lifetime_default", None, serde_json::json!([])),
        ("stage1_default", Some("weight_bp"), serde_json::json!(9999)),
        ("lifetime_default", Some("pd_bp"), serde_json::json!(-1)),
    ] {
        let mut wire = baseline.clone();
        let companies = wire["companies"].as_object_mut().unwrap();
        let (id, company) = companies.iter_mut().next().unwrap();
        let id = id.clone();
        let policy = &mut company["books"]["Bank"]["ecl_policy"];
        match field {
            Some(field) => policy[table][0][field] = value,
            None => policy[table] = value,
        }
        let error = match serde_json::from_value::<CompanyOperations>(wire) {
            Err(error) => error,
            Ok(_) => panic!("损坏 ECL 政策不得安装为独立经营状态"),
        };
        assert!(error.to_string().contains("ECL"), "{error}");
        assert!(error.to_string().contains(&id), "{error}");
    }
}
