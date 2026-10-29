//! 只固定现有 restore 的构造前校验与 G/N；成功不等于完整会话或日终恢复成功。

use super::*;

fn setup(stock_count: usize) -> SessionSetup {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.history_len = 1;
    let template = setup.stocks[0].clone();
    setup.stocks = (0..stock_count)
        .map(|index| StockSpec {
            code: StockCode(format!("{:06}", 600_100 + index)),
            ..template.clone()
        })
        .collect();
    setup
}

fn save(stock_count: usize) -> SaveSlot {
    GameSession::new(setup(stock_count), 42)
        .expect("小型合法会话必须可以构造")
        .save()
        .expect("小型合法会话必须可以生成完整 SaveSlot")
}

fn assert_invalid_save(error: SessionError, code: u16, context: Vec<u64>) {
    match error {
        SessionError::InvalidSave(fault) => {
            assert_eq!(fault, ValidationFault::new(code, context));
        }
        other => panic!("必须保留 InvalidSave 错误类型，实际为 {other:?}"),
    }
}

fn assert_original_prefix_error(save: &SaveSlot, code: u16, context: Vec<u64>) {
    assert_invalid_save(
        save.restore_runtime_topology().unwrap_err(),
        code,
        context.clone(),
    );
    let original_error = match GameSession::restore(save) {
        Ok(_) => panic!("现有真实 restore 必须拒绝相同的非法前缀"),
        Err(error) => error,
    };
    assert_invalid_save(original_error, code, context);
}

#[test]
fn restore_runtime_topology_matches_real_restore_for_one_stock() {
    let save = save(1);
    assert_eq!(save.restore_runtime_topology().unwrap(), (1, 1));
    let restored = GameSession::restore(&save).expect("合法单股档必须沿原路径恢复");
    assert_eq!(restored.runtime_group_count, 1);
    assert_eq!(restored.state.markets.len(), 1);
    assert!(
        restored.runtime_driver.is_none(),
        "投影与 restore 均不提前启动 World"
    );
}

#[test]
fn restore_runtime_topology_uses_complete_json_archive_for_three_stocks() {
    let save = save(3);
    let before = serde_json::to_vec(&save).unwrap();
    let decoded: SaveSlot = serde_json::from_slice(&before).unwrap();
    assert_eq!(decoded.restore_runtime_topology().unwrap(), (1, 3));
    let restored = GameSession::restore(&decoded).expect("完整三股 JSON 档必须沿原路径恢复");
    assert_eq!(restored.runtime_group_count, 1);
    assert_eq!(restored.state.markets.len(), 3);
    assert!(
        restored.runtime_driver.is_none(),
        "不能把预检成功当作 World 已就绪"
    );
    assert_eq!(
        serde_json::to_vec(&decoded).unwrap(),
        before,
        "只读投影不能改写完整存档"
    );
}

#[test]
fn restore_runtime_topology_does_not_persist_explicit_source_group_override() {
    let source =
        GameSession::new_with_runtime_groups(setup(1), 42, 3).expect("显式多组源会话必须可以构造");
    assert_eq!(source.runtime_group_count, 3);
    let save = source.save().unwrap();
    assert_eq!(save.restore_runtime_topology().unwrap(), (1, 1));
    let restored = GameSession::restore(&save).expect("存档恢复继续使用现有构造事实");
    assert_eq!(
        restored.runtime_group_count, 1,
        "现有 SaveSlot 不持久化显式 group override"
    );
    assert_eq!(
        source.runtime_group_count, 3,
        "只读预检与恢复不能改变源会话"
    );
}

#[test]
fn restore_runtime_topology_rejects_original_urgency_fault_1037() {
    let mut save = save(1);
    save.urgency_policy.policy_version += 1;
    assert_original_prefix_error(&save, 1037, vec![0]);
}

#[test]
fn restore_runtime_topology_rejects_original_setup_fault_2002() {
    let mut save = save(1);
    save.setup.ticks_per_day = 0;
    assert_original_prefix_error(&save, 2002, vec![]);
}

#[test]
fn restore_runtime_topology_rejects_original_market_set_fault_2003() {
    let mut save = save(1);
    save.snapshot.markets.clear();
    assert_original_prefix_error(&save, 2003, vec![]);
}

#[test]
fn restore_runtime_topology_preserves_urgency_before_setup_error_priority() {
    let mut save = save(1);
    save.urgency_policy.policy_version += 1;
    save.setup.ticks_per_day = 0;
    assert_original_prefix_error(&save, 1037, vec![0]);
}

#[test]
fn restore_runtime_topology_success_does_not_promise_poisoned_runtime_restore() {
    let mut save = save(1);
    save.runtime_state.poisoned = true;
    assert_eq!(save.restore_runtime_topology().unwrap(), (1, 1));
    let original_error = match GameSession::restore(&save) {
        Ok(_) => panic!("topology 预检不能放行 poisoned runtime 的完整恢复"),
        Err(error) => error,
    };
    assert_invalid_save(original_error, 2200, vec![]);
}
