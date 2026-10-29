//! String 入口预检只用完整真实小档，不在 native 上构造 JsValue 或分配大 arena。

use super::{facade_slot_capacity_for_restore_json, RestoreFacadeInitError};
use engine::{GameSession, SaveSlot, SessionError, StockCode};

#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

fn save(stock_count: usize) -> SaveSlot {
    let mut setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.history_len = 1;
    let template = setup.stocks[0].clone();
    setup.stocks = (0..stock_count)
        .map(|index| engine::session::StockSpec {
            code: StockCode(format!("{:06}", 600_101 + index)),
            float_shares: 0,
            ..template.clone()
        })
        .collect();
    GameSession::new(setup, 42).unwrap().save().unwrap()
}

fn assert_prefix_error(save: &SaveSlot, code: u16, context: Vec<u64>) {
    let json = serde_json::to_string(save).unwrap();
    match facade_slot_capacity_for_restore_json(&json) {
        Err(RestoreFacadeInitError::Topology(SessionError::InvalidSave(fault))) => {
            assert_eq!(fault, engine::error::ValidationFault::new(code, context));
        }
        other => panic!("必须保真实 prefix typed 错误，实际 {other:?}"),
    }
}

#[test]
fn restore_json_capacity_uses_complete_one_and_three_stock_archives() {
    for (stocks, capacity) in [(1, 8), (3, 12)] {
        let json = serde_json::to_string(&save(stocks)).unwrap();
        assert_eq!(facade_slot_capacity_for_restore_json(&json).unwrap(), capacity);
    }
}

#[test]
fn restore_json_capacity_rejects_malformed_json_and_setup_only() {
    let setup_json = serde_json::to_string(&save(1).setup).unwrap();
    for json in ["{", "null", "17", "[]", "{}", setup_json.as_str()] {
        assert!(
            matches!(
                facade_slot_capacity_for_restore_json(json),
                Err(RestoreFacadeInitError::Decode(_))
            ),
            "完整 SaveSlot 之外的输入必须明确 Decode 拒绝"
        );
    }
}

#[test]
fn restore_json_capacity_preserves_urgency_before_setup_fault() {
    let mut save = save(1);
    save.urgency_policy.policy_version += 1;
    save.setup.ticks_per_day = 0;
    let json = serde_json::to_string(&save).unwrap();
    // 公共 JSON 边界由 UrgencyPolicy Deserialize 在 setup prefix 之前拒绝 version；
    // 内存 SaveSlot 的 typed fault1037 与优先级仍由原 engine 测试独立约束。
    let original_decode = serde_json::from_str::<SaveSlot>(&json).unwrap_err();
    assert_eq!(
        original_decode.classify(),
        serde_json::error::Category::Data
    );
    assert_eq!(
        original_decode.to_string(),
        format!(
            "unsupported urgency policy version 2; expected 1 at line {} column {}",
            original_decode.line(),
            original_decode.column()
        )
    );
    match facade_slot_capacity_for_restore_json(&json) {
        Err(RestoreFacadeInitError::Decode(error)) => {
            assert_eq!(error.classify(), serde_json::error::Category::Data);
            assert_eq!(error.to_string(), original_decode.to_string());
            assert_eq!(error.line(), original_decode.line());
            assert_eq!(error.column(), original_decode.column());
        }
        other => panic!("必须保公共 JSON urgency Decode 错误，实际 {other:?}"),
    }
}

#[test]
fn restore_json_capacity_preserves_setup_fault() {
    let mut save = save(1);
    save.setup.ticks_per_day = 0;
    assert_prefix_error(&save, 2002, vec![]);
}

#[test]
fn restore_json_capacity_preserves_market_set_fault() {
    let mut save = save(1);
    save.snapshot.markets.clear();
    assert_prefix_error(&save, 2003, vec![]);
}

#[test]
fn restore_json_capacity_success_is_not_complete_restore_success() {
    let mut save = save(1);
    save.runtime_state.poisoned = true;
    let json = serde_json::to_string(&save).unwrap();
    assert_eq!(facade_slot_capacity_for_restore_json(&json).unwrap(), 8);
    match GameSession::restore(&save) {
        Err(SessionError::InvalidSave(fault)) => assert_eq!(fault.code, 2200),
        Ok(_) => panic!("String prefix 预检不得代替真实 poisoned runtime 拒绝"),
        Err(error) => panic!("真实 restore 应保 fault2200，实际 {error:?}"),
    }
}

#[test]
fn restore_json_capacity_rejects_unsupported_sync_policy_before_capacity_setup() {
    for policy in [engine::RuntimeSyncPolicy::DayEnd, engine::RuntimeSyncPolicy::OnDemand] {
        let mut save = save(1);
        save.setup.runtime_sync_policy = policy;
        let json = serde_json::to_string(&save).unwrap();
        match facade_slot_capacity_for_restore_json(&json) {
            Err(error) => assert!(
                format!("{error:?}").contains("WASM_RUNTIME_SYNC_POLICY_UNSUPPORTED"),
                "必须明确拒绝 WASM 不支持的策略 {policy:?}，实际：{error:?}"
            ),
            Ok(capacity) => panic!("策略 {policy:?} 应在 facade 容量预检前拒绝，得到容量 {capacity}"),
        }
    }
}

#[test]
fn restore_json_capacity_rejects_unsupported_policy_before_topology_validation() {
    for policy in [engine::RuntimeSyncPolicy::DayEnd, engine::RuntimeSyncPolicy::OnDemand] {
        let mut save = save(1);
        save.setup.runtime_sync_policy = policy;
        save.setup.ticks_per_day = 0;
        let json = serde_json::to_string(&save).unwrap();
        match facade_slot_capacity_for_restore_json(&json) {
            Err(error) => assert!(
                format!("{error:?}").contains("WASM_RUNTIME_SYNC_POLICY_UNSUPPORTED"),
                "策略拒绝应先于 topology 校验 {policy:?}，实际：{error:?}"
            ),
            Ok(capacity) => panic!("策略 {policy:?} 应在 topology 校验前拒绝，容量 {capacity}"),
        }
    }
}
