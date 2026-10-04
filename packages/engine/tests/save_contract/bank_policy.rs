#[allow(dead_code)]
#[path = "../company_operations/fixtures.rs"]
mod operating_fixture;

use engine::company::operations::CompanyOperationsConfig;
use engine::session::{GameSession, SaveSlot, SessionError};

fn bank_save() -> serde_json::Value {
    let mut setup = super::continuity_setup();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.stocks[0].float_shares = 0;
    let mut company = operating_fixture::bank_c(operating_fixture::d("2027-12-31"));
    company.spec.listed_stock = Some(setup.stocks[0].code.clone());
    company.spec.issued_shares = setup.stocks[0].total_shares;
    setup.company_operations = Some(CompanyOperationsConfig {
        seed: super::SEED,
        shock_params: operating_fixture::quiet_params(),
        companies: vec![company],
    });
    let mut session = GameSession::new(setup, super::SEED).unwrap();
    super::run_full_day(&mut session);
    serde_json::to_value(session.save().unwrap()).unwrap()
}

#[test]
fn complete_save_restore_rejects_invalid_bank_ecl_with_company_context() {
    let baseline = bank_save();
    let valid: SaveSlot = serde_json::from_value(baseline.clone()).unwrap();
    GameSession::restore(&valid).expect("真实 Bank 装配生成的合法政策必须恢复成功");
    for (table, field, value) in [
        ("stage1_default", None, serde_json::json!([])),
        ("lifetime_default", None, serde_json::json!([])),
        ("stage1_default", Some("weight_bp"), serde_json::json!(9999)),
        ("lifetime_default", Some("pd_bp"), serde_json::json!(-1)),
    ] {
        let mut wire = baseline.clone();
        let companies = wire["company_operations"]["companies"]
            .as_object_mut()
            .unwrap();
        let (id, company) = companies.iter_mut().next().unwrap();
        let id = id.clone();
        let policy = &mut company["books"]["Bank"]["ecl_policy"];
        match field {
            Some(field) => policy[table][0][field] = value,
            None => policy[table] = value,
        }
        let invalid: SaveSlot = serde_json::from_value(wire).expect("库级 serde 接受集合保持不变");
        let error = match GameSession::restore(&invalid) {
            Err(error) => error,
            Ok(_) => panic!("损坏 ECL 政策不得安装为运行会话"),
        };
        assert!(matches!(error, SessionError::InvalidSave(_)), "{error}");
        assert!(error.to_string().contains("ECL"), "{error}");
        assert!(error.to_string().contains(&id), "{error}");
    }
}
