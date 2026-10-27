//! 完整 GameSession::restore 门：本模块须由当前 engine 构建执行，旧 rlib 轻 harness 不代跑。

use super::*;

#[test]
fn whole_save_json_and_direct_memory_restore_validate_registry_market_sources() {
    let (mut save, registries) = registry_settlement::tests::completed_fixture();
    (match &mut save.company_state {
        SavedSessionCompanies::CompanySystem {
            corporate_actions, ..
        } => corporate_actions,
        _ => panic!("需要真实公司局"),
    })
    .registries = registries;
    GameSession::restore(&save).unwrap();
    let decoded: SaveSlot = serde_json::from_slice(&serde_json::to_vec(&save).unwrap()).unwrap();
    GameSession::restore(&decoded).unwrap();
    let mut encoded =
        serde_json::to_value(&save.company_state.actions().unwrap().registries[0]).unwrap();
    encoded["receipts"][0]["request"]["scope"]["SettlementDayMarket"]["binding"]["market_source"]
        ["FinalConfirmations"]["receipts"] =
        serde_json::json!([{ "account":"1", "receipt_id":"999" }]);
    (match &mut save.company_state {
        SavedSessionCompanies::CompanySystem {
            corporate_actions, ..
        } => corporate_actions,
        _ => panic!("需要真实公司局"),
    })
    .registries[0] = serde_json::from_value(encoded).unwrap();
    assert!(GameSession::restore(&save).is_err());
    let decoded: SaveSlot = serde_json::from_slice(&serde_json::to_vec(&save).unwrap()).unwrap();
    assert!(GameSession::restore(&decoded).is_err());
}

/// 输出由真实 Session 保存、真实物理日 producer 生成且完整恢复过的跨层样例。
#[test]
fn source_b_produces_real_full_saves_with_selected_state_and_registry_binding() {
    for automatic in [false, true] {
        let (mut candidate, registries) =
            registry_settlement::tests::completed_fixture_with_foundation(automatic);
        let SavedSessionCompanies::CompanySystem {
            corporate_actions, ..
        } = &mut candidate.company_state
        else {
            panic!("真实fixture必须创建所选公司系统");
        };
        corporate_actions.registries = registries;
        let installed =
            GameSession::restore(&candidate).expect("实际REG来源与资本验证必须同时通过");
        let save = installed.save().expect("完整Session保存真实选中authority");
        let bytes = serde_json::to_vec(&save).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let binding = &value["company_state"]["CompanySystem"]["corporate_actions"]["registries"]
            [0]["receipts"][0]["request"]["scope"]["SettlementDayMarket"]["binding"];
        assert!(
            binding.is_object(),
            "必须来自正式physical日producer，不能丢失真实binding"
        );
        for key in [
            "predecessor",
            "opening",
            "market_unit",
            "market_source",
            "post",
        ] {
            assert!(binding.get(key).is_some(), "真实binding必填 {key}");
        }
        let decoded: SaveSlot = serde_json::from_slice(&bytes).unwrap();
        let restored = GameSession::restore(&decoded).unwrap();
        assert_eq!(
            serde_json::to_vec(&restored.save().unwrap()).unwrap(),
            bytes
        );
        if let Some(directory) = std::env::var_os("SOURCE_B_SAVE_FIXTURE_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).expect("跨层样例目录必须可创建");
            let name = if automatic {
                "generated-company-state-registry.json"
            } else {
                "manual-company-state-registry.json"
            };
            std::fs::write(directory.join(name), &bytes).expect("完整真实样例必须成功写入");
        }
    }
}
