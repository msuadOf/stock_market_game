//! 公司行为机制新局开关的严格持久化测试（2026-10-07 产品决策）。
//!
//! 配股／增发与回购是两个独立的新局必填布尔开关（默认关闭的语义由宿主 UI 承担，
//! 存档契约本身无默认值）：新档必填、旧档缺失显式拒绝、恢复后开关语义不变。
//! 对齐 `dividend_tax_mode` 先例（ADR-0040）。

use super::*;

fn switch_setup(rights: bool, repurchase: bool) -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.npcs.retail_count = 1;
    setup.rights_offering_enabled = rights;
    setup.issuer_repurchase_enabled = repurchase;
    setup
}

#[test]
fn mechanism_switches_round_trip_through_save_and_restore() {
    let session = GameSession::new(switch_setup(true, false), 42).unwrap();
    let save = session.save().unwrap();
    assert!(
        save.setup.rights_offering_enabled,
        "配股／增发开关必须随存档显式写出"
    );
    assert!(
        !save.setup.issuer_repurchase_enabled,
        "回购开关必须随存档显式写出"
    );
    let restored = GameSession::restore(&save).unwrap();
    assert!(restored.state.setup.rights_offering_enabled);
    assert!(!restored.state.setup.issuer_repurchase_enabled);
}

/// 纯 JSON 层红测：新契约存档 setup 必须显式写出两个机制开关（缺失即红）。
#[test]
fn save_setup_json_contains_explicit_mechanism_switches() {
    let session = GameSession::new(npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let save = session.save().unwrap();
    let json = serde_json::to_value(&save).unwrap();
    let setup = json
        .get("setup")
        .and_then(|setup| setup.as_object())
        .expect("存档 JSON 含 setup 对象");
    assert!(
        setup.get("rights_offering_enabled").is_some_and(|value| value.is_boolean()),
        "新契约存档 setup 必须显式写出配股／增发布尔开关"
    );
    assert!(
        setup.get("issuer_repurchase_enabled").is_some_and(|value| value.is_boolean()),
        "新契约存档 setup 必须显式写出回购布尔开关"
    );
}

#[test]
fn save_without_mechanism_switch_fields_is_rejected_explicitly() {
    let session = GameSession::new(npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let save = session.save().unwrap();
    let mut json = serde_json::to_value(&save).unwrap();
    let setup = json
        .get_mut("setup")
        .and_then(|setup| setup.as_object_mut())
        .expect("存档 JSON 含 setup 对象");
    assert!(
        setup.remove("rights_offering_enabled").is_some(),
        "新契约存档必须显式写出配股／增发开关"
    );
    assert!(
        setup.remove("issuer_repurchase_enabled").is_some(),
        "新契约存档必须显式写出回购开关"
    );
    let text = serde_json::to_string(&json).unwrap();
    let error = serde_json::from_str::<SaveSlot>(&text)
        .expect_err("缺少机制开关字段的旧档必须被显式拒绝");
    assert!(
        error.to_string().contains("rights_offering_enabled"),
        "拒绝错误须指明缺失字段：{error}"
    );
}
