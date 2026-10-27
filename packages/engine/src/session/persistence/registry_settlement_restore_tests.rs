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
