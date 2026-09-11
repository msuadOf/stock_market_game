//! Failure path：无对手盘零成交、未读公布不改个人决策、非法分析输入显式报错。

use super::*;

#[test]
fn no_counterparty_means_zero_fills_and_unfilled_plans() {
    // 无散户/游资、零流通盘：信念机构只有买入方向（无持仓可卖），无对手盘
    // ⇒ 零成交、计划零进度（挂单可以存在，但 filled 恒 0）。
    let mut setup = chain_setup("2030-01-07");
    setup.npcs.retail_count = 0;
    setup.npcs.hot_count = 0;
    setup.npcs.inst_count = 5;
    for stock in &mut setup.stocks {
        stock.float_shares = 0;
    }
    let mut session = GameSession::new(setup, SEED).unwrap();
    let mut trades = 0usize;
    for _ in 0..180 {
        for event in session.step() {
            if matches!(event, engine::session::Event::Trade { .. }) {
                trades += 1;
            }
        }
    }
    assert_eq!(trades, 0, "no counterparty must produce zero fills");
    for (account, code, _direction, _target, filled, _status) in session.plans_debug() {
        assert_eq!(
            filled, 0,
            "plan {code:?} for account {account:?} must not advance without real fills"
        );
    }
}

#[test]
fn world_mutations_between_observations_do_not_change_personal_decisions() {
    // 日终窗口对世界的全部变更——当日经营过账（账面事实变化）、月末前的
    // 版本登记、18:00 披露（可能发布新公布）——在 NPC 下一次 accepted
    // 注意力（获知）之前，**不得**改变任何个人信念。这是「未披露/未读
    // 事实 ⇒ 同个人决策」的会话级锁：两次快照之间世界确实变了，但决策
    // 状态必须逐字节不动（只有获知事件才允许改变信念——K4/K5）。
    let mut session = GameSession::new(chain_setup("2030-01-07"), SEED).unwrap();
    for _ in 0..60 {
        session.step();
    }
    let before = belief_snapshot(&session);
    assert!(
        !before.is_empty(),
        "at least one belief entry must exist after a day of chain activity"
    );
    session
        .end_civil_day()
        .expect("day-end must settle the world");
    let after = belief_snapshot(&session);
    assert_eq!(
        before, after,
        "world mutations (ops/closing/disclosures) between observations must not touch beliefs"
    );
}

fn belief_snapshot(
    session: &GameSession,
) -> Vec<(u64, String, engine::session::BeliefDebugSummary)> {
    let mut out = Vec::new();
    for account in 13u64..23 {
        for code in ["600101", "002156", "300260", "600610", "000812"] {
            if let Some(value) =
                session.belief_debug(engine::AccountId(account), &StockCode(code.to_string()))
            {
                out.push((account, code.to_string(), value));
            }
        }
    }
    out
}

#[test]
fn illegal_analysis_inputs_are_typed_errors() {
    use engine::plans::{fundamental_range_signal, target_share_quantity, SignalScore};

    // 倒置的估值区间：类型化拒绝，绝不静默取反。
    let inverted = fundamental_range_signal(
        Money::from_cents(1_000),
        Money::from_cents(1_200),
        Money::from_cents(1_000),
    );
    assert!(matches!(
        inverted,
        Err(engine::plans::CandidateError::InvertedValuationRange { .. })
    ));

    // 非 100 股整手的换算请求：A 股申报单位硬约束。
    let bad_lot = target_share_quantity(
        5_000,
        Money::from_cents(1_000_000),
        Money::from_cents(1_000),
        200,
    );
    assert!(matches!(
        bad_lot,
        Err(engine::plans::CandidateError::InvalidBoardLotSize { lot_size: 200 })
    ));

    // 超界信号分：类型化拒绝。
    assert!(SignalScore::new(20_000).is_err());
    assert!(SignalScore::new(-20_000).is_err());

    // 非正价格进入基本面比较：类型化拒绝。
    let non_positive =
        fundamental_range_signal(Money::ZERO, Money::from_cents(100), Money::from_cents(200));
    assert!(matches!(
        non_positive,
        Err(engine::plans::CandidateError::NonPositiveMoney { .. })
    ));
}
