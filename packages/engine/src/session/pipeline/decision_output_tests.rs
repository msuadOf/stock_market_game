//! T3·NpcDecisionHandover 交接三检查（ADR-0042 §9.1/§9.8；ADR-0032 R3；Astra
//! 四发现裁定·步 4 放行条件 1：**不切换生产调用位置**的预备批红绿证据）。
//!
//! 三检查各自有独立红测：epoch 匹配 / 产物完整（observed_accounts 严格递增、
//! intents owner ⊆ 观察集合、依赖表合法）/ 恰一次消费（槽位 take 语义）。
//! 空批与满批的合法构造→消费→ingress_order 携带为绿侧锚。

use super::npc_tick_preparation::{take_decision_output, NpcDecisionHandover};
use crate::{AccountId, Intent, LimitPrice, Money, ReceiptBearingIntent, Side, StockId};

fn buy(code: StockId, ordinal: u64) -> ReceiptBearingIntent {
    ReceiptBearingIntent {
        owner: AccountId(1),
        intent: Intent::PlaceLimit {
            code,
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(990)),
            qty: 100,
        },
        account_ordinal: ordinal,
        stock_ordinal: ordinal,
    }
}

fn cancel(code: StockId, ordinal: u64) -> ReceiptBearingIntent {
    ReceiptBearingIntent {
        owner: AccountId(1),
        intent: Intent::Cancel { code, id: crate::OrderId(7) },
        account_ordinal: ordinal,
        stock_ordinal: ordinal,
    }
}

fn description_of(error: &crate::session::StepFatal) -> String {
    match error {
        crate::session::StepFatal::InvariantViolation { description, .. } => description.clone(),
        other => format!("{other:?}"),
    }
}

/// 检查一（epoch 匹配）：跨 tick（错位 epoch）交接必须显式失败。
#[test]
fn decision_output_rejects_epoch_mismatch() {
    let output = NpcDecisionHandover::new(
        5,
        vec![AccountId(1)],
        vec![buy(StockId(0), 0)],
        Vec::new(),
    );
    let error = output
        .into_batch(6)
        .expect_err("epoch 不匹配必须拒绝交接");
    assert!(
        description_of(&error).contains("epoch does not match"),
        "实际错误: {}",
        description_of(&error)
    );
}

/// 检查二（产物完整）：observed_accounts 非严格递增必须失败（镜像 fault2069）。
#[test]
fn decision_output_rejects_non_increasing_observed_accounts() {
    let output = NpcDecisionHandover::new(
        5,
        vec![AccountId(2), AccountId(2)],
        vec![buy(StockId(0), 0), buy(StockId(0), 1)],
        Vec::new(),
    );
    let error = output
        .into_batch(5)
        .expect_err("observed_accounts 必须严格递增");
    assert!(
        description_of(&error).contains("strictly increasing"),
        "实际错误: {}",
        description_of(&error)
    );
}

// 退役登记（入口切换批）：owner⊆observed 检查经边界修正从交接契约移除
// ——它是旧跨 tick 存档校验 fault2070 的规则（批四随字段删除退役），运行时
// 交接不由观察集合推导 receipt 事实（ADR-0032 决策第 1 条）；红测
// decision_output_rejects_intent_owner_outside_observed_set 随之退役。

/// 检查二（产物完整）：非法依赖（前驱非撤单）必须失败（镜像 fault2068）。
#[test]
fn decision_output_rejects_invalid_dependency_predecessor() {
    let output = NpcDecisionHandover::new(
        5,
        vec![AccountId(1)],
        vec![buy(StockId(0), 0), buy(StockId(0), 1)],
        vec![(0, 1)],
    );
    let error = output.into_batch(5).expect_err("前驱必须为撤单");
    assert!(
        description_of(&error).contains("predecessor is not a cancellation"),
        "实际错误: {}",
        description_of(&error)
    );
}

/// 检查三（恰一次消费）：槽位二次消费与未生产均必须失败。
#[test]
fn decision_output_take_is_exactly_once() {
    let mut slot = Some(NpcDecisionHandover::empty(5));
    let first = take_decision_output(&mut slot, 5).expect("首次消费必须成功");
    assert_eq!(first.observed_tick(), 5);
    let second = take_decision_output(&mut slot, 5)
        .expect_err("二次消费（槽位已空）必须拒绝");
    assert!(
        description_of(&second).contains("missing or was already consumed"),
        "实际错误: {}",
        description_of(&second)
    );
}

/// 检查三（epoch）：take_decision_output 同样执行 epoch 匹配（槽位路径）。
#[test]
fn decision_output_take_rejects_epoch_mismatch() {
    let mut slot = Some(NpcDecisionHandover::empty(5));
    let error = take_decision_output(&mut slot, 6).expect_err("epoch 不匹配必须拒绝");
    assert!(
        description_of(&error).contains("epoch does not match"),
        "实际错误: {}",
        description_of(&error)
    );
}

/// 绿侧锚：合法空批与满批交接成功，ingress_order 与依赖转换保持
/// （与 take_ready_npc_batch 同构——未来切换批的消费语义不变项）。
#[test]
fn decision_output_legal_batches_hand_over_with_ingress_order() {
    // 空批。
    let (batch, observed) = NpcDecisionHandover::empty(5).into_batch(5).expect("空批交接");
    assert!(observed.is_empty());
    assert!(batch.candidates().is_empty());
    // 满批：撤单→下单依赖（同账户同股）。
    let output = NpcDecisionHandover::new(
        5,
        vec![AccountId(1)],
        vec![cancel(StockId(0), 0), buy(StockId(0), 1)],
        vec![(0, 1)],
    );
    let (batch, observed) = output.into_batch(5).expect("合法满批必须交接成功");
    assert_eq!(observed, vec![AccountId(1)]);
    let candidates = batch.candidates();
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].ingress_order(), Some((0, 0)));
    assert_eq!(candidates[1].ingress_order(), Some((1, 1)));
    assert_eq!(candidates[1].predecessors(), &[candidates[0].key().clone()]);
}
