//! 任务65 #1（订单单一生命周期）红测。
//!
//! 目标契约：
//! - Place 的簿内 Order 只经 `EnvelopeDraft::into_book_order` 单一 typed 构造点产生；
//! - 逐 tick capture 不再对既有 resting 订单重算费用（费用重算只发生在变更集与新建），
//!   防御由 4 字段交叉核对 + 证据门 + 费率指纹守卫承担；
//! - 前置条件（见 .tmp/task1-11/design/inventory-and-design.md §E.1）：单存续期间
//!   费用输入不可变；任何费率改动必须被指纹守卫当场拒绝。
//!
//! 历史边界：旧批T5/T6只有编译红证据，T8未留下当时运行红，不追认。
//! 本轮真正运行红见repair_lifecycle_receipt_tests与behavior-red-index.json；
//! 费用计数已改为GameConfig实例作用域的真实helper探针。

use super::account_validation::CandidateValidationInput;
use super::account_validation_context::build_account_validation_context;
use super::continuous_matching_adapter::prepare_incremental_continuous_inputs;
use super::{
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, IntentCandidate, IntentCandidateBatch,
    IntentCandidateKey, PhaseInput, StepFatal,
};
use crate::session::npc_working_quote_tests::quote_setup;
use crate::{
    AccountId, GameSession, Intent, LimitPrice, Money, Order, OrderId, Side, StockId,
};

fn resting(side: Side, id: OrderId, owner: AccountId) -> Order {
    Order {
        id,
        side,
        price: Money::from_cents(900),
        qty: 100,
        original_qty: 100,
        filled_qty: 0,
        filled_value: Money::ZERO,
        owner,
        seq: 0,
    }
}

/// 已 hydrate 的带 resting 单会话：sell 侧费用审计只存在于 ledger（capture 不比对），
/// 是费率篡改能否静默通过的判别用例。
fn game_with_resting(side: Side) -> GameSession {
    let mut setup = quote_setup(0);
    setup.config.fee_probe = crate::config::FeeHelperProbe::scoped();
    let mut game = GameSession::new(setup, 42).unwrap();
    // 数值身份后无字符串可断言：quote_setup 单股目录按 rank 取 StockId(0)。
    let code = StockId(0);
    game.state
        .markets
        .get_mut(&code)
        .unwrap()
        .place(resting(side, OrderId(91), AccountId(1)))
        .unwrap();
    game.hydrate_or_validate_envelope_ledger().unwrap();
    game
}

/// T5：Place 的簿内 Order 由 draft 的单一 typed 构造点产生，字段与 resolved price 一致。
#[test]
fn envelope_draft_places_book_order_via_single_typed_constructor() {
    let game = GameSession::new(quote_setup(0), 42).unwrap();
    let plan = super::plan_tick(PhaseInput {
        session: &game,
    })
    .unwrap();
    let batch = IntentCandidateBatch::new(vec![IntentCandidate::new(
        IntentCandidateKey::player(0),
        AccountId(0),
        Intent::PlaceLimit {
            code: StockId(0),
            side: Side::Buy,
            price: LimitPrice::Lowest,
            qty: 100,
        },
    )])
    .unwrap();
    let output = CandidateValidationInput::new_with_context(
        batch,
        plan.decision_resources().unwrap().clone(),
        plan.envelope_ledger().unwrap(),
        game.state.next_order_id,
        game.state.setup.config.clone(),
        build_account_validation_context(&game).unwrap(),
    )
    .unwrap()
    .validate()
    .unwrap();

    let draft = output.drafts()[0].clone();
    let resolved = draft.limit();
    // 消费前先取访问器结果：into_book_order 按值消费 draft（一次创建、不再重建）。
    let (expected_id, expected_side, expected_qty, expected_owner) =
        (draft.order_id(), draft.side(), draft.qty(), draft.owner());
    let order = draft.into_book_order(resolved);

    // 单一构造点必须产出与撮合簿契约一致的完整 Order（id/side/price/qty/owner）。
    assert_eq!(order.id, expected_id);
    assert_eq!(order.side, expected_side);
    assert_eq!(order.price, resolved);
    assert_eq!(order.qty, expected_qty);
    assert_eq!(order.original_qty, expected_qty);
    assert_eq!(order.filled_qty, 0);
    assert_eq!(order.filled_value, Money::ZERO);
    assert_eq!(order.owner, expected_owner);
}

/// T6：既有 resting 订单在逐 tick capture 中不再重算费用（重算只属于变更集与新建）。
/// 计数器置零后跑 capture：无成交 tick 对既有单的费率乘法次数必须为 0。
#[test]
fn unchanged_resting_orders_skip_fee_recomputation_at_capture() {
    let game = game_with_resting(Side::Buy);
    let probe = game.state.setup.config.fee_probe.clone();
    // 实例作用域覆盖Rayon，hydrate在置零前完成，不借全局Atomic或observer模拟次数。
    probe.reset();
    let inputs = prepare_incremental_continuous_inputs(&game).unwrap();

    // capture 仍交付逐股输入；但既有单的 buy_order_reservation/cumulative_nominal_fees
    // 重算次数为 0（4 字段交叉核对与证据门保持，见 T7；费率静默改动见 T8）。
    assert_eq!(probe.snapshot(), (0, 0));
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].envelopes.len(), 1);
}

/// T7（keep-green 保级）：篡改既有单账本侧费用输入（audit.filled_value），
/// capture 的 4 字段交叉核对必须仍当场拒绝——重构不得弱化该防御。
#[test]
fn tampered_ledger_audit_for_unchanged_order_is_caught_at_capture() {
    let mut game = game_with_resting(Side::Buy);
    let code = StockId(0);
    let key = EnvelopeKey {
        account: AccountId(1),
        stock: code,
        order: OrderId(91),
        side: Side::Buy,
    };
    let live = game.state.envelope_ledger.get(&key).unwrap().live();
    let audit = game.state.envelope_ledger.get(&key).unwrap().audit();
    // 同样的 live 资源、篡改后的费用进度：只有 audit↔order 交叉核对能发现。
    let tampered = EnvelopeAudit {
        filled_value: audit.filled_value.add(Money::from_cents(1)).unwrap(),
        ..audit
    };
    let cursor = game.state.envelope_ledger.next_receipt_index();
    game.state.envelope_ledger = EnvelopeLedger::new(
        cursor,
        [Envelope::tick_start_existing(key, live.cash, live.shares, tampered)],
    )
    .unwrap();

    let error = prepare_incremental_continuous_inputs(&game).unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation { ref description, .. }
            if description == "continuous order books disagree with envelope audit"
    ));
}

/// T8（真实行为红）：单存续期间篡改费率（模拟未来热改路径的静默漏检），
/// 变更集校验的费率指纹守卫必须当场 fail closed。
/// 当前实现：SELL resting 单的 nominal 只存在于 ledger，capture 不比对——静默通过（红）。
#[test]
fn mid_session_fee_rate_tampering_fails_closed_at_capture() {
    let mut game = game_with_resting(Side::Sell);
    // 前置条件破坏：直接改写全局佣金率（cfg(test) 内直接字段写）。
    game.state.setup.config.commission_rate += 0.001;

    let error = prepare_incremental_continuous_inputs(&game).unwrap_err();

    // 守卫必须显式报告费率在订单存续期间被改动，不得静默继续。
    assert!(matches!(
        error,
        StepFatal::InvariantViolation { ref description, .. }
            if description.contains("费率") && description.contains("改动")
    ));
}
