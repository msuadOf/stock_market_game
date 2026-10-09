use super::*;
use crate::company::issuer_repurchase::IssuerRepurchaseStatus;
use crate::company::share_split::{ShareSplitDirection, ShareSplitEventPlan};

fn assert_repurchase_cancellation_uses_current_share_units(direction: ShareSplitDirection) {
    let (mut session, stock, issuer, seller) = normal_repurchase_session(true);
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    session
        .approve_share_split(ShareSplitEventPlan {
            event_id: "repurchase-redenomination".into(),
            approval_reference: "board-redenomination".into(),
            issuer: issuer.clone(),
            stock: stock.clone(),
            exchange: crate::calendar::CalendarExchange::Sse,
            direction: direction.clone(),
            ratio: 2,
            approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
            announced_on: CivilDate::from_iso("2030-01-02").unwrap(),
            registered_on: CivilDate::from_iso("2030-01-03").unwrap(),
            ex_rights_on: CivilDate::from_iso("2030-01-04").unwrap(),
        })
        .unwrap();
    let issued_before = session
        .state
        .company_system
        .issuers()
        .get(&issuer)
        .unwrap()
        .issued_shares;
    let capital_before = session
        .state
        .company_system
        .dividend_legal_facts(&issuer)
        .unwrap()
        .unwrap()
        .registered_capital;
    complete_day(&mut session);
    trade_repurchase_from_holder(&mut session, &stock, seller);
    session.end_civil_day().unwrap();
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases[0].total_filled_shares(),
        100
    );
    assert_eq!(
        *session.state.corporate_actions.issuer_repurchases[0].status(),
        IssuerRepurchaseStatus::Completed
    );
    assert_eq!(treasury_shares_of(&session, &stock), 100);
    let current_shares = match direction {
        ShareSplitDirection::Split => 200,
        ShareSplitDirection::Consolidate => 50,
    };
    let issued_redenominated = match direction {
        ShareSplitDirection::Split => issued_before * 2,
        ShareSplitDirection::Consolidate => issued_before / 2,
    };
    let treasury = session.state.corporate_actions.share_splits[0]
        .receipt()
        .unwrap()
        .holders
        .iter()
        .find(|holder| holder.holder == HolderId::IssuerTreasury)
        .unwrap();
    assert_eq!(treasury.original_shares, 100);
    assert_eq!(treasury.new_shares, current_shares);
    let confirmation_history = session.state.personal_trade_confirmations.clone();
    let fills = session.state.corporate_actions.issuer_repurchases[0]
        .fills()
        .to_vec();
    let account = session.issuer_repurchase_account_id().unwrap();
    let next_day = session.civil_date();
    for _ in 0..session.state.setup.ticks_per_day {
        session.step().unwrap();
    }
    let before = serde_json::to_value(session.save().unwrap()).unwrap();
    let result = session.end_civil_day();
    if result.is_err() {
        assert_eq!(
            serde_json::to_value(session.save().unwrap()).unwrap(),
            before
        );
    }
    result.expect("真实回购完成后的 R+1 日终应按当前股单位注销");
    let book = &session.state.corporate_actions.issuer_repurchases[0];
    assert_eq!(book.cancelled_on(), Some(next_day));
    assert_eq!(
        book.cancelled_shares(),
        current_shares,
        "注销数量必须使用重新计值后的股单位"
    );
    assert_eq!(book.fills(), fills);
    assert_eq!(treasury_shares_of(&session, &stock), 0);
    assert!(session
        .account(account)
        .unwrap()
        .positions()
        .get(&stock)
        .is_none());
    assert_eq!(
        session
            .state
            .company_system
            .issuers()
            .get(&issuer)
            .unwrap()
            .issued_shares,
        issued_redenominated - current_shares
    );
    assert_eq!(
        session
            .state
            .company_system
            .dividend_legal_facts(&issuer)
            .unwrap()
            .unwrap()
            .registered_capital,
        capital_before
            .sub(AccountingAmount::from_cents(10_000))
            .unwrap()
    );
    assert_eq!(
        session.state.personal_trade_confirmations,
        confirmation_history
    );
    assert_registry_conserves_issued(&session, &issuer);
    let saved = session.save().unwrap();
    let restored = GameSession::restore(&saved).unwrap();
    let restored_twice = GameSession::restore(&restored.save().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(restored_twice.save().unwrap()).unwrap(),
        serde_json::to_value(saved).unwrap()
    );
}

#[test]
fn completed_repurchase_then_same_day_split_cancels_current_units() {
    assert_repurchase_cancellation_uses_current_share_units(ShareSplitDirection::Split);
}

#[test]
fn completed_repurchase_then_same_day_consolidation_cancels_current_units() {
    assert_repurchase_cancellation_uses_current_share_units(ShareSplitDirection::Consolidate);
}

/// 非法批准锚拒绝：审批当刻无已生效重新计值时，携带任意锚 id 显式拒绝。
#[test]
fn bogus_approval_unit_anchor_is_rejected() {
    let (mut session, stock, issuer, _seller) = normal_repurchase_session(true);
    let mut plan = repurchase_plan(&issuer, &stock);
    plan.approval_unit_event_id = Some("no-such-split".into());
    let error = session.approve_issuer_repurchase(plan).unwrap_err();
    assert!(
        error.to_string().contains("单位锚"),
        "锚核验错误必须指明单位锚：{error}"
    );
}

/// 拆股已生效后的陈旧锚（None）拒绝：锚必须等于审批当刻真实交易单位。
#[test]
fn stale_approval_unit_anchor_after_settled_split_is_rejected() {
    let (mut session, stock, issuer, _seller) = normal_repurchase_session(true);
    session
        .approve_share_split(ShareSplitEventPlan {
            event_id: "anchor-split".into(),
            approval_reference: "board-anchor".into(),
            issuer: issuer.clone(),
            stock: stock.clone(),
            exchange: crate::calendar::CalendarExchange::Sse,
            direction: ShareSplitDirection::Split,
            ratio: 2,
            approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
            announced_on: CivilDate::from_iso("2030-01-02").unwrap(),
            registered_on: CivilDate::from_iso("2030-01-03").unwrap(),
            ex_rights_on: CivilDate::from_iso("2030-01-04").unwrap(),
        })
        .unwrap();
    complete_day(&mut session);
    complete_day(&mut session);
    complete_day(&mut session);
    // 现在是 2030-01-05（拆股已于 01-04 入账）：锚必须是 Some("anchor-split")。
    let mut plan = repurchase_plan(&issuer, &stock);
    plan.approved_on = session.civil_date();
    plan.announced_on = session.civil_date();
    plan.window_start_on = CivilDate::from_iso("2030-01-07").unwrap();
    plan.window_deadline_on = CivilDate::from_iso("2030-01-09").unwrap();
    let error = session.approve_issuer_repurchase(plan).unwrap_err();
    assert!(error.to_string().contains("单位锚"), "{error}");
    let mut anchored = repurchase_plan(&issuer, &stock);
    anchored.approved_on = session.civil_date();
    anchored.announced_on = session.civil_date();
    anchored.window_start_on = CivilDate::from_iso("2030-01-07").unwrap();
    anchored.window_deadline_on = CivilDate::from_iso("2030-01-09").unwrap();
    anchored.approval_unit_event_id = Some("anchor-split".into());
    session.approve_issuer_repurchase(anchored).unwrap();
}

/// 跨 ex 日执行：R 日旧单位成交 + ex 日新单位成交原量追加，执行数量/价格上限
/// 按当前交易单位换算（price cap 1100 分 ÷2 → ≤550；max 300 → 600 新单位）。
#[test]
fn executing_plan_spans_ex_date_with_converted_caps() {
    let (mut session, stock, issuer, seller) = normal_repurchase_session(true);
    let mut plan = repurchase_plan(&issuer, &stock);
    plan.max_shares = 300;
    plan.window_deadline_on = CivilDate::from_iso("2030-01-09").unwrap();
    session.approve_issuer_repurchase(plan).unwrap();
    session
        .approve_share_split(ShareSplitEventPlan {
            event_id: "span-split".into(),
            approval_reference: "board-span".into(),
            issuer: issuer.clone(),
            stock: stock.clone(),
            exchange: crate::calendar::CalendarExchange::Sse,
            direction: ShareSplitDirection::Split,
            ratio: 2,
            approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
            announced_on: CivilDate::from_iso("2030-01-02").unwrap(),
            registered_on: CivilDate::from_iso("2030-01-03").unwrap(),
            ex_rights_on: CivilDate::from_iso("2030-01-04").unwrap(),
        })
        .unwrap();
    complete_day(&mut session);
    // R = 2030-01-03：旧单位成交 100 股。
    trade_repurchase_from_holder(&mut session, &stock, seller);
    session.end_civil_day().unwrap();
    let book = &session.state.corporate_actions.issuer_repurchases[0];
    assert_eq!(book.total_filled_shares(), 100);
    // ex = 2030-01-04：新单位。先推进一 tick 让除权参考价落位（前收 → 500 分），
    // 再挂 500 分卖单（低于换算上限 550），执行器换算后的委托必须能真实撮合。
    let account = session.issuer_repurchase_account_id().unwrap();
    session.step().unwrap();
    // 经真实受理路径挂卖单（直接改订单簿会破坏 envelope 审计）：500 分 × 200 股。
    let received = session
        .receive_private_intent(
            seller,
            Intent::PlaceLimit {
                code: stock.clone(),
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(500)),
                qty: 200,
            },
        )
        .unwrap();
    session.state.pending_player.push(received);
    let mut traded = false;
    for _ in 1..session.state.setup.ticks_per_day {
        traded |= session
            .step()
            .unwrap()
            .iter()
            .any(|event| matches!(event, crate::Event::Trade { .. }));
    }
    assert!(traded, "ex 日换算后的执行器委托必须真实撮合");
    let issuer_rows: Vec<_> = session.state.personal_trade_confirmations[&account]
        .iter()
        .filter(|row| row.civil_date == session.civil_date())
        .collect();
    let new_unit_shares: u64 = issuer_rows
        .iter()
        .map(|row| u64::from(row.quantity_shares))
        .sum();
    assert_eq!(new_unit_shares, 200, "ex 日新单位成交必须原量追加");
    for row in &issuer_rows {
        let price = row.gross.cents() / i64::from(row.quantity_shares);
        assert!(
            price <= 550,
            "成交价 {price} 分不得超过换算后的价格上限 550 分"
        );
    }
    session.end_civil_day().unwrap();
    let book = &session.state.corporate_actions.issuer_repurchases[0];
    assert_eq!(
        book.total_filled_shares(),
        300,
        "旧 100 + 新 200，不改写历史"
    );
    assert_eq!(book.share_unit_adjustments().len(), 1);
    assert_eq!(book.share_unit_adjustments()[0].shares_before, 100);
    assert_eq!(book.share_unit_adjustments()[0].shares_after, 200);
    assert_eq!(
        book.current_inventory_shares(&session.state.corporate_actions.split_unit_context(&stock))
            .unwrap(),
        400
    );
    // 窗口截止完成 → 次日终自动注销 400 股（新单位），面值 50 分 × 400 = 20000 分。
    let capital_before = session
        .state
        .company_system
        .dividend_legal_facts(&issuer)
        .unwrap()
        .unwrap()
        .registered_capital;
    while *session.state.corporate_actions.issuer_repurchases[0].status()
        != IssuerRepurchaseStatus::Cancelled
    {
        complete_day(&mut session);
    }
    let book = &session.state.corporate_actions.issuer_repurchases[0];
    assert_eq!(book.cancelled_shares(), 400);
    assert_eq!(
        session
            .state
            .company_system
            .dividend_legal_facts(&issuer)
            .unwrap()
            .unwrap()
            .registered_capital,
        capital_before
            .sub(AccountingAmount::from_cents(20_000))
            .unwrap()
    );
    assert_eq!(treasury_shares_of(&session, &stock), 0);
    assert_registry_conserves_issued(&session, &issuer);
    GameSession::restore(&session.save().unwrap()).unwrap();
}

/// R 日到期取消先于当日登记冻结：快照 Treasury 不含已注销股份，R+1 入账不复活。
/// 时间线：01-02 批准（当日日终公告）；01-03 成交 100 股并完成；01-04（R 日）
/// 到期注销**先于**冻结；01-07（ex 日）入账。
#[test]
fn due_cancellation_precedes_same_day_registration_freeze() {
    let (mut session, stock, issuer, seller) = normal_repurchase_session(true);
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    session
        .approve_share_split(ShareSplitEventPlan {
            event_id: "freeze-after-cancel".into(),
            approval_reference: "board-freeze".into(),
            issuer: issuer.clone(),
            stock: stock.clone(),
            exchange: crate::calendar::CalendarExchange::Sse,
            direction: ShareSplitDirection::Split,
            ratio: 2,
            approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
            announced_on: CivilDate::from_iso("2030-01-02").unwrap(),
            registered_on: CivilDate::from_iso("2030-01-04").unwrap(),
            ex_rights_on: CivilDate::from_iso("2030-01-07").unwrap(),
        })
        .unwrap();
    complete_day(&mut session);
    // 01-03：真实成交 100 股并完成（当日日终）。
    trade_repurchase_from_holder(&mut session, &stock, seller);
    session.end_civil_day().unwrap();
    assert_eq!(
        *session.state.corporate_actions.issuer_repurchases[0].status(),
        IssuerRepurchaseStatus::Completed
    );
    // 01-04（R 日）：到期注销先于冻结执行——旧单位 100 股。
    complete_day(&mut session);
    let book = &session.state.corporate_actions.issuer_repurchases[0];
    assert_eq!(
        book.cancelled_on(),
        Some(CivilDate::from_iso("2030-01-04").unwrap())
    );
    assert_eq!(book.cancelled_shares(), 100);
    let snapshot = session.state.corporate_actions.share_splits[0]
        .registration()
        .unwrap();
    let treasury_shares = snapshot
        .holdings()
        .iter()
        .find(|holding| holding.holder == HolderId::IssuerTreasury)
        .map(|holding| holding.lots.iter().map(|lot| lot.qty).sum::<u64>())
        .unwrap_or(0);
    assert_eq!(treasury_shares, 0, "R 冻结快照不得包含已注销股份");
    // 01-07（ex 日）：重新计值入账不复活已注销库存。
    complete_day(&mut session);
    complete_day(&mut session);
    complete_day(&mut session);
    assert_eq!(treasury_shares_of(&session, &stock), 0);
    let adjustments =
        session.state.corporate_actions.issuer_repurchases[0].share_unit_adjustments();
    assert_eq!(adjustments.len(), 1);
    assert_eq!(
        (adjustments[0].shares_before, adjustments[0].shares_after),
        (0, 0)
    );
    assert_registry_conserves_issued(&session, &issuer);
    let saved = serde_json::to_value(session.save().unwrap()).unwrap();
    let restored = GameSession::restore(&session.save().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        saved,
        "恢复不得重复换算或注销"
    );
}

/// 换单位后注销失败必须原子回滚（无部分状态泄露）。
#[test]
fn failing_cancellation_after_unit_change_rolls_back_whole_state() {
    let (mut session, stock, issuer, seller) = normal_repurchase_session(true);
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    session
        .approve_share_split(ShareSplitEventPlan {
            event_id: "rollback-split".into(),
            approval_reference: "board-rollback".into(),
            issuer: issuer.clone(),
            stock: stock.clone(),
            exchange: crate::calendar::CalendarExchange::Sse,
            direction: ShareSplitDirection::Split,
            ratio: 2,
            approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
            announced_on: CivilDate::from_iso("2030-01-02").unwrap(),
            registered_on: CivilDate::from_iso("2030-01-03").unwrap(),
            ex_rights_on: CivilDate::from_iso("2030-01-04").unwrap(),
        })
        .unwrap();
    complete_day(&mut session);
    trade_repurchase_from_holder(&mut session, &stock, seller);
    session.end_civil_day().unwrap();
    complete_day(&mut session);
    let event_id = session.state.corporate_actions.issuer_repurchases[0]
        .plan()
        .event_id
        .clone();
    let book = &session.state.corporate_actions.issuer_repurchases[0];
    assert_eq!(book.cancelled_shares(), 200);
    // 已注销方案再手工注销：必须显式拒绝且状态字节不变。
    let before = serde_json::to_value(session.save().unwrap()).unwrap();
    let error = session
        .execute_issuer_repurchase_cancellation(&event_id, 1, session.civil_date())
        .unwrap_err();
    assert!(error.to_string().contains("当前库存"), "{error}");
    assert_eq!(
        serde_json::to_value(session.save().unwrap()).unwrap(),
        before,
        "失败的注销不得泄露部分状态"
    );
}

/// initial 残桶 + 两 plan（其一已部分注销残存）共享非整除缩股目标：分桶分配
/// 精确到汇总目标，定向注销只消耗本 plan 份额，不误扣 initial 或别的 plan。
#[test]
fn two_plans_and_initial_share_fractional_target_and_directional_cancellation() {
    let (mut session, stock, issuer, _player) = session_with_registry(true);
    // 方案 A：窗口 01-03，成交 100 股后完成，随后手工部分注销 40（残 60）。
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    complete_day(&mut session);
    assert_eq!(
        session.civil_date(),
        CivilDate::from_iso("2030-01-03").unwrap()
    );
    fill_max_shares_on_window_day(&mut session, &stock);
    let a_event = "repurchase-2030".to_string();
    // 01-04：手工部分注销 40 股 → A 残 60 股（Cancelled 终态、仍持库存）。
    session
        .execute_issuer_repurchase_cancellation(
            &a_event,
            40,
            CivilDate::from_iso("2030-01-04").unwrap(),
        )
        .unwrap();
    // 方案 B：01-04 批准，窗口 01-07（周一）起，成交 100 股后完成。
    let mut b_plan = repurchase_plan(&issuer, &stock);
    b_plan.event_id = "repurchase-2030-b".into();
    b_plan.approved_on = CivilDate::from_iso("2030-01-04").unwrap();
    b_plan.announced_on = CivilDate::from_iso("2030-01-04").unwrap();
    b_plan.window_start_on = CivilDate::from_iso("2030-01-07").unwrap();
    b_plan.window_deadline_on = CivilDate::from_iso("2030-01-09").unwrap();
    session.approve_issuer_repurchase(b_plan).unwrap();
    complete_day(&mut session);
    // 01-05：缩股 7:1 声明（避开同日注销的批准时点注册资本回放边界），
    // R=01-07，ex=01-08。
    session
        .approve_share_split(ShareSplitEventPlan {
            event_id: "two-plan-consolidation".into(),
            approval_reference: "board-two-plan".into(),
            issuer: issuer.clone(),
            stock: stock.clone(),
            exchange: crate::calendar::CalendarExchange::Sse,
            direction: ShareSplitDirection::Consolidate,
            ratio: 7,
            approved_on: CivilDate::from_iso("2030-01-05").unwrap(),
            announced_on: CivilDate::from_iso("2030-01-05").unwrap(),
            registered_on: CivilDate::from_iso("2030-01-07").unwrap(),
            ex_rights_on: CivilDate::from_iso("2030-01-08").unwrap(),
        })
        .unwrap();
    while session.civil_date() < CivilDate::from_iso("2030-01-07").unwrap() {
        complete_day(&mut session);
    }
    // 01-07：B 成交 100 股（R 日盘中，旧单位）；当日日终先冻结（含 A 残 60 +
    // B 100 + initial），后完成 B。
    fill_max_shares_on_window_day(&mut session, &stock);
    assert_eq!(session.state.corporate_actions.issuer_repurchases.len(), 2);
    // 01-08（ex 日）：入账换算分桶，随后 B 到期注销按新单位执行。
    complete_day(&mut session);
    let split_context = session.state.corporate_actions.split_unit_context(&stock);
    fn find_book<'a>(
        session: &'a GameSession,
        event: &str,
    ) -> &'a crate::company::issuer_repurchase::IssuerRepurchaseBook {
        session
            .state
            .corporate_actions
            .issuer_repurchases
            .iter()
            .find(|book| book.plan().event_id == event)
            .unwrap()
    }
    let a = find_book(&session, &a_event);
    let b = find_book(&session, "repurchase-2030-b");
    let a_adj = a
        .share_unit_adjustments()
        .iter()
        .find(|adj| adj.split_event_id == "two-plan-consolidation")
        .unwrap();
    let b_adj = b
        .share_unit_adjustments()
        .iter()
        .find(|adj| adj.split_event_id == "two-plan-consolidation")
        .unwrap();
    assert_eq!(a_adj.shares_before, 60);
    assert_eq!(b_adj.shares_before, 100);
    assert_eq!(b_adj.shares_after, 14, "100 ÷ 7 = 14 余 2（无奖励）");
    assert!(
        a_adj.shares_after == 8 || a_adj.shares_after == 9,
        "60 ÷ 7 = 8 余 4；余 4 并列最高，奖励 +1 与否由 seed 决定：{}",
        a_adj.shares_after
    );
    assert_eq!(
        b.cancelled_on(),
        Some(CivilDate::from_iso("2030-01-08").unwrap()),
        "B 完成后次日终（= ex 日，入账之后）按新单位自动注销"
    );
    assert_eq!(b.cancelled_shares(), 14);
    // 定向注销：A 残量与 initial 不被 B 的注销误扣。
    assert_eq!(
        a.current_inventory_shares(&split_context).unwrap(),
        a_adj.shares_after
    );
    let registry = session
        .state
        .corporate_actions
        .registries
        .iter()
        .find(|registry| *registry.stock() == stock)
        .unwrap();
    let a_lot_ids: std::collections::BTreeSet<String> = session
        .state
        .corporate_actions
        .plan_treasury_lot_ids(&a_event, &stock)
        .into_iter()
        .collect();
    let b_lot_ids: std::collections::BTreeSet<String> = session
        .state
        .corporate_actions
        .plan_treasury_lot_ids("repurchase-2030-b", &stock)
        .into_iter()
        .collect();
    let treasury_lots: Vec<_> = registry
        .holdings()
        .iter()
        .find(|holding| holding.holder == HolderId::IssuerTreasury)
        .map(|holding| holding.lots.clone())
        .unwrap_or_default();
    let a_attributed: u64 = treasury_lots
        .iter()
        .filter(|lot| a_lot_ids.contains(&lot.id))
        .map(|lot| lot.qty)
        .sum();
    let b_attributed: u64 = treasury_lots
        .iter()
        .filter(|lot| b_lot_ids.contains(&lot.id))
        .map(|lot| lot.qty)
        .sum();
    assert_eq!(
        a_attributed, a_adj.shares_after,
        "A 的名册份额保持换算后残量"
    );
    assert_eq!(b_attributed, 0, "B 的份额必须被定向注销耗尽");
    // 汇总守恒：initial_after + A_after == Treasury 当前总量。
    let receipt = session.state.corporate_actions.share_splits[0]
        .receipt()
        .unwrap();
    let treasury_outcome = receipt
        .holders
        .iter()
        .find(|outcome| outcome.holder == HolderId::IssuerTreasury)
        .unwrap();
    let expected_treasury = treasury_outcome.new_shares - b_adj.shares_after;
    assert_eq!(treasury_shares_of(&session, &stock), expected_treasury);
    // 回购账户只承接 plan 库存（A 残量），不凭空拿 initial。
    let repurchase_account = session.issuer_repurchase_account_id().unwrap();
    assert_eq!(
        session
            .account(repurchase_account)
            .unwrap()
            .positions()
            .get(&stock)
            .map(|position| u64::from(position.qty())),
        Some(a_adj.shares_after)
    );
    assert_registry_conserves_issued(&session, &issuer);
    let saved = serde_json::to_value(session.save().unwrap()).unwrap();
    let restored = GameSession::restore(&session.save().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        saved,
        "恢复不得重复换算或注销"
    );
}

/// P0 回归（非作者门禁）：initial 残桶 > 0 且 ex 日回购账户有真实成交——
/// ex 日名册先把当日回购买入落为 Treasury market lot，而拆股分桶在 record_fill
/// 之前执行；该 lot 无 plan 归属，不得落入 initial 残桶破坏桶勾稽（否则整日
/// InvalidSave 回滚且重试同败）。当日新买入是新单位，原量保留、不参与换算。
fn assert_initial_bucket_with_ex_date_purchase_settles(direction: ShareSplitDirection) {
    let (mut session, stock, issuer, _player) = session_with_registry(true);
    // 缩股 2:1 后执行上限 = max/2，须 ≥ 一手才能在 ex 日真实成交：max 取 300。
    let mut plan = repurchase_plan(&issuer, &stock);
    plan.max_shares = 400;
    session.approve_issuer_repurchase(plan).unwrap();
    session
        .approve_share_split(ShareSplitEventPlan {
            event_id: "initial-bucket-split".into(),
            approval_reference: "board-initial-bucket".into(),
            issuer: issuer.clone(),
            stock: stock.clone(),
            exchange: crate::calendar::CalendarExchange::Sse,
            direction: direction.clone(),
            ratio: 2,
            approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
            announced_on: CivilDate::from_iso("2030-01-02").unwrap(),
            registered_on: CivilDate::from_iso("2030-01-03").unwrap(),
            ex_rights_on: CivilDate::from_iso("2030-01-04").unwrap(),
        })
        .unwrap();
    // 01-02 日终（公告）；01-03（R）无成交过日、日终冻结（plan 库存 0，
    // initial 残桶 9,999,000）。
    complete_day(&mut session);
    complete_day(&mut session);
    assert!(session.state.corporate_actions.issuer_repurchases[0]
        .fills()
        .is_empty());
    // 01-04（ex 日）：玩家按当日新单位价位真实卖 100 股给回购账户。
    let ex_day_price = match direction {
        ShareSplitDirection::Split => 500,
        ShareSplitDirection::Consolidate => 2000,
    };
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: stock.clone(),
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(ex_day_price)),
                qty: 100,
            },
        )
        .unwrap();
    let mut traded = false;
    for _ in 0..session.state.setup.ticks_per_day {
        traded |= session
            .step()
            .unwrap()
            .iter()
            .any(|event| matches!(event, crate::Event::Trade { .. }));
    }
    eprintln!(
        "P0方向={direction:?} traded={traded} 玩家现金={:?}",
        session.account(AccountId(0)).unwrap().cash()
    );
    assert!(traded, "ex 日必须真实撮合（方向 {direction:?}）");
    session.end_civil_day().unwrap();
    let book = &session.state.corporate_actions.issuer_repurchases[0];
    assert_eq!(book.total_filled_shares(), 100);
    assert_eq!(book.share_unit_adjustments().len(), 1);
    assert_eq!(
        (
            book.share_unit_adjustments()[0].shares_before,
            book.share_unit_adjustments()[0].shares_after
        ),
        (0, 0),
        "冻结期库存为零也必须记录单位变化，新成交保留原量"
    );
    // initial 换算后残桶：split 9,999,000 → 19,998,000；consolidate → 4,999,500。
    let initial_after = match direction {
        ShareSplitDirection::Split => 19_998_000_u64,
        ShareSplitDirection::Consolidate => 4_999_500_u64,
    };
    assert_eq!(treasury_shares_of(&session, &stock), initial_after + 100);
    assert_registry_conserves_issued(&session, &issuer);
    let repurchase_account = session.issuer_repurchase_account_id().unwrap();
    assert_eq!(
        session
            .account(repurchase_account)
            .unwrap()
            .positions()
            .get(&stock)
            .map(|position| u64::from(position.qty())),
        Some(100),
        "ex 日新买入 100 股（新单位）必须原量保留在回购账户"
    );
    // 窗口截止完成（此后无对手成交）→ 完成后首个日终自动注销 100 股（新单位），
    // initial 残桶不受影响。
    while *session.state.corporate_actions.issuer_repurchases[0].status()
        != IssuerRepurchaseStatus::Cancelled
    {
        complete_day(&mut session);
    }
    let book = &session.state.corporate_actions.issuer_repurchases[0];
    assert_eq!(book.total_filled_shares(), 100);
    assert_eq!(book.cancelled_shares(), 100);
    assert_eq!(treasury_shares_of(&session, &stock), initial_after);
    assert_registry_conserves_issued(&session, &issuer);
    let saved = serde_json::to_value(session.save().unwrap()).unwrap();
    let restored = GameSession::restore(&session.save().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        saved
    );
}

#[test]
fn initial_bucket_survives_ex_date_purchase_under_split() {
    assert_initial_bucket_with_ex_date_purchase_settles(ShareSplitDirection::Split);
}

#[test]
fn initial_bucket_survives_ex_date_purchase_under_consolidation() {
    assert_initial_bucket_with_ex_date_purchase_settles(ShareSplitDirection::Consolidate);
}

fn assert_first_new_unit_fill_is_not_converted_again(
    direction: ShareSplitDirection,
    restore: bool,
) {
    let (mut session, stock, issuer, seller) = normal_repurchase_session(true);
    let mut plan = repurchase_plan(&issuer, &stock);
    plan.max_shares = 600;
    plan.window_deadline_on = CivilDate::from_iso("2030-01-09").unwrap();
    session.approve_issuer_repurchase(plan).unwrap();
    session
        .approve_share_split(ShareSplitEventPlan {
            event_id: "empty-r-inventory".into(),
            approval_reference: "board-empty-r".into(),
            issuer: issuer.clone(),
            stock: stock.clone(),
            exchange: crate::calendar::CalendarExchange::Sse,
            direction: direction.clone(),
            ratio: 2,
            approved_on: CivilDate::from_iso("2030-01-02").unwrap(),
            announced_on: CivilDate::from_iso("2030-01-02").unwrap(),
            registered_on: CivilDate::from_iso("2030-01-03").unwrap(),
            ex_rights_on: CivilDate::from_iso("2030-01-04").unwrap(),
        })
        .unwrap();
    complete_day(&mut session);
    complete_day(&mut session);
    let price = match direction {
        ShareSplitDirection::Split => Money::from_cents(500),
        ShareSplitDirection::Consolidate => Money::from_cents(2000),
    };
    let account = session.issuer_repurchase_account_id().unwrap();
    fn sell_today(
        session: &mut GameSession,
        stock: &StockCode,
        seller: AccountId,
        price: Money,
        qty: u32,
    ) -> Vec<PersonalTradeConfirmation> {
        let received = session
            .receive_private_intent(
                seller,
                Intent::PlaceLimit {
                    code: stock.clone(),
                    side: Side::Sell,
                    price: LimitPrice::Fixed(price),
                    qty,
                },
            )
            .unwrap();
        session.state.pending_player.push(received);
        let today = session.civil_date();
        for _ in 0..session.state.setup.ticks_per_day {
            session.step().unwrap();
        }
        let account = session.issuer_repurchase_account_id().unwrap();
        session.state.personal_trade_confirmations[&account]
            .iter()
            .filter(|row| row.civil_date == today)
            .cloned()
            .collect()
    }
    let seller_cash = session.account(seller).unwrap().cash();
    let first_rows = sell_today(&mut session, &stock, seller, price, 200);
    assert_eq!(
        first_rows
            .iter()
            .map(|row| row.quantity_shares)
            .sum::<u32>(),
        200
    );
    let first_cost = first_rows
        .iter()
        .map(|row| row.gross.cents() + row.actual_fees.total().unwrap().cents())
        .sum::<i64>();
    let first_net = session.state.personal_trade_confirmations[&seller]
        .iter()
        .filter(|row| row.civil_date == session.civil_date())
        .map(|row| row.gross.cents() - row.actual_fees.total().unwrap().cents())
        .sum::<i64>();
    assert_eq!(
        session.account(seller).unwrap().cash().cents(),
        seller_cash.cents() + first_net
    );
    session.end_civil_day().unwrap();
    let adjustment =
        &session.state.corporate_actions.issuer_repurchases[0].share_unit_adjustments()[0];
    assert_eq!((adjustment.shares_before, adjustment.shares_after), (0, 0));
    let historical_fills = session.state.corporate_actions.issuer_repurchases[0]
        .fills()
        .to_vec();
    if !restore {
        emit_schema_fixture(
            &session,
            match direction {
                ShareSplitDirection::Split => "zero-inventory-split-fill",
                ShareSplitDirection::Consolidate => "zero-inventory-consolidation-fill",
            },
        );
    }
    if restore {
        let saved = session.save().unwrap();
        session = GameSession::restore(&saved).unwrap();
        assert_eq!(
            serde_json::to_value(session.save().unwrap()).unwrap(),
            serde_json::to_value(saved).unwrap()
        );
    }
    while session.civil_date() < CivilDate::from_iso("2030-01-07").unwrap() {
        complete_day(&mut session);
    }
    let (offered, expected_next, expected_total) = match direction {
        ShareSplitDirection::Split => (1000, 1000, 1200),
        ShareSplitDirection::Consolidate => (200, 100, 300),
    };
    let next_rows = sell_today(&mut session, &stock, seller, price, offered);
    assert_eq!(
        next_rows.iter().map(|row| row.quantity_shares).sum::<u32>(),
        expected_next,
        "库存的单位必须包含首次真实 fill 的新单位；不得按旧批准锚二次乘除"
    );
    assert!(next_rows
        .iter()
        .all(|row| row.gross.cents() / i64::from(row.quantity_shares)
            <= match direction {
                ShareSplitDirection::Split => 550,
                ShareSplitDirection::Consolidate => 2200,
            }));
    let next_cost = next_rows
        .iter()
        .map(|row| row.gross.cents() + row.actual_fees.total().unwrap().cents())
        .sum::<i64>();
    assert_eq!(
        session.account(account).unwrap().cash().cents(),
        5_000_000 - first_cost - next_cost
    );
    let checkpoint = serde_json::to_value(session.save().unwrap()).unwrap();
    let settled = session.end_civil_day();
    if settled.is_err() {
        assert_eq!(
            serde_json::to_value(session.save().unwrap()).unwrap(),
            checkpoint
        );
    }
    settled.expect("下一交易日日终不得因错单位超额成交而永久回滚");
    let book = &session.state.corporate_actions.issuer_repurchases[0];
    assert_eq!(book.fills()[0], historical_fills[0]);
    assert_eq!(book.total_filled_shares(), expected_total);
    assert_eq!(
        book.current_inventory_shares(&session.state.corporate_actions.split_unit_context(&stock))
            .unwrap(),
        expected_total
    );
    let capital_before = session
        .state
        .company_system
        .dividend_legal_facts(&issuer)
        .unwrap()
        .unwrap()
        .registered_capital;
    complete_day(&mut session);
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases[0].cancelled_shares(),
        expected_total
    );
    let par_cents = match direction {
        ShareSplitDirection::Split => 50,
        ShareSplitDirection::Consolidate => 200,
    };
    assert_eq!(
        session
            .state
            .company_system
            .dividend_legal_facts(&issuer)
            .unwrap()
            .unwrap()
            .registered_capital,
        capital_before
            .sub(AccountingAmount::from_cents(
                i128::from(par_cents) * i128::from(expected_total)
            ))
            .unwrap()
    );
    assert_registry_conserves_issued(&session, &issuer);
    let saved = session.save().unwrap();
    let restored = GameSession::restore(&saved).unwrap();
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(saved).unwrap()
    );
}

#[test]
fn consecutive_split_fills_keep_new_inventory_units() {
    assert_first_new_unit_fill_is_not_converted_again(ShareSplitDirection::Split, false);
}

#[test]
fn consecutive_consolidation_fills_keep_new_inventory_units() {
    assert_first_new_unit_fill_is_not_converted_again(ShareSplitDirection::Consolidate, false);
}

#[test]
fn restored_consecutive_split_fills_keep_new_inventory_units() {
    assert_first_new_unit_fill_is_not_converted_again(ShareSplitDirection::Split, true);
}

#[test]
fn restored_consecutive_consolidation_fills_keep_new_inventory_units() {
    assert_first_new_unit_fill_is_not_converted_again(ShareSplitDirection::Consolidate, true);
}

fn small_treasury_session(initial: u64, seller_qty: u32) -> (GameSession, StockCode, CompanyId) {
    let mut setup = repurchase_setup(true);
    setup.rights_offering_enabled = true;
    setup.stocks[0].total_shares = initial + u64::from(seller_qty);
    let mut session = GameSession::new(setup, 42).unwrap();
    let stock = session.state.setup.stocks[0].code.clone();
    let issuer = session
        .state
        .company_system
        .issuers()
        .issuer_of(&stock)
        .unwrap()
        .clone();
    let date = session.civil_date();
    session
        .state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .fixture_insert_position(
            stock.clone(),
            Position::from_restored_parts(seller_qty, 0, i64::from(seller_qty) * 1000, 0),
        );
    let mut holdings = vec![ShareHolding {
        holder: HolderId::Account(AccountId(0)),
        lots: vec![fixture_lot("small-seller", u64::from(seller_qty), date)],
    }];
    if initial > 0 {
        holdings.push(ShareHolding {
            holder: HolderId::IssuerTreasury,
            lots: vec![fixture_lot("small-initial", initial, date)],
        });
    }
    session
        .configure_share_registry(
            ShareRegistry::new(
                stock.clone(),
                issuer.clone(),
                initial + u64::from(seller_qty),
                date,
                holdings,
            )
            .unwrap(),
        )
        .unwrap();
    session
        .define_dividend_legal_facts(
            &issuer,
            AccountingAmount::from_cents(i128::from(initial + u64::from(seller_qty)) * 100),
            "small legal capital".into(),
        )
        .unwrap();
    (session, stock, issuer)
}

fn approve_test_consolidation(
    session: &mut GameSession,
    stock: &StockCode,
    issuer: &CompanyId,
    event: &str,
    ratio: u64,
    registered: &str,
    ex: &str,
) {
    session
        .approve_share_split(ShareSplitEventPlan {
            event_id: event.into(),
            approval_reference: format!("board-{event}"),
            issuer: issuer.clone(),
            stock: stock.clone(),
            exchange: crate::calendar::CalendarExchange::Sse,
            direction: ShareSplitDirection::Consolidate,
            ratio,
            approved_on: session.civil_date(),
            announced_on: session.civil_date(),
            registered_on: CivilDate::from_iso(registered).unwrap(),
            ex_rights_on: CivilDate::from_iso(ex).unwrap(),
        })
        .unwrap();
}

fn advance_to_date(session: &mut GameSession, date: &str) {
    let target = CivilDate::from_iso(date).unwrap();
    while session.civil_date() < target {
        complete_day(session);
    }
}

fn emit_schema_fixture(session: &GameSession, name: &str) {
    // Web fixture 取真实 Rust codec，避免凭印象手写 AccountId/OrderId 或单位事实。
    let directory = match std::env::var("TASK174_SCHEMA_FIXTURE_DIR") {
        Ok(directory) => directory,
        Err(std::env::VarError::NotPresent) => return,
        Err(error) => panic!("schema fixture 环境路径非法：{error}"),
    };
    let saved = serde_json::to_value(session.save().unwrap()).unwrap();
    let issuers: serde_json::Map<String, serde_json::Value> = session
        .state
        .company_system
        .issuers()
        .iter()
        .map(|(id, issuer)| {
            (
                id.0.clone(),
                serde_json::json!({
                    "listed_stock": issuer.listed_stock.as_ref().map(|stock| &stock.0),
                    "issued_shares": issuer.issued_shares.to_string(),
                }),
            )
        })
        .collect();
    let projection = serde_json::json!({
        "actions": session.state.corporate_actions,
        "context": { "issuers": issuers, "setup": saved["setup"], "snapshot": saved["snapshot"],
                     "currentDate": session.civil_date().to_string() },
    });
    std::fs::write(
        std::path::Path::new(&directory).join(format!("task174-{name}.json")),
        serde_json::to_vec_pretty(&projection).unwrap(),
    )
    .unwrap();
}

fn assert_restores_twice_without_reposting(session: &GameSession) {
    let saved = session.save().unwrap();
    let first = GameSession::restore(&saved).unwrap();
    let second = GameSession::restore(&first.save().unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(second.save().unwrap()).unwrap(),
        serde_json::to_value(saved).unwrap()
    );
}

#[test]
fn cancelled_residual_award_carries_through_two_consolidations() {
    let (mut session, stock, issuer) = small_treasury_session(2, 1220);
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    complete_day(&mut session);
    fill_max_shares_on_window_day(&mut session, &stock);
    session
        .execute_issuer_repurchase_cancellation("repurchase-2030", 1, session.civil_date())
        .unwrap();
    let mut second = repurchase_plan(&issuer, &stock);
    second.event_id = "repurchase-2030-b".into();
    second.approved_on = session.civil_date();
    second.announced_on = session.civil_date();
    second.window_start_on = CivilDate::from_iso("2030-01-07").unwrap();
    second.window_deadline_on = CivilDate::from_iso("2030-01-09").unwrap();
    session.approve_issuer_repurchase(second).unwrap();
    advance_to_date(&mut session, "2030-01-07");
    fill_max_shares_on_window_day(&mut session, &stock);
    session
        .execute_issuer_repurchase_cancellation("repurchase-2030-b", 99, session.civil_date())
        .unwrap();
    complete_day(&mut session);
    assert_eq!(treasury_shares_of(&session, &stock), 102);
    assert_eq!(
        session
            .state
            .company_system
            .issuers()
            .get(&issuer)
            .unwrap()
            .issued_shares,
        1122
    );
    approve_test_consolidation(
        &mut session,
        &stock,
        &issuer,
        "award-six",
        6,
        "2030-01-10",
        "2030-01-11",
    );
    advance_to_date(&mut session, "2030-01-11");
    complete_day(&mut session);
    let first = session
        .state
        .corporate_actions
        .issuer_repurchases
        .iter()
        .find(|book| book.plan().event_id == "repurchase-2030")
        .unwrap();
    assert_eq!(*first.status(), IssuerRepurchaseStatus::Cancelled);
    assert_eq!(first.share_unit_adjustments()[0].shares_before, 99);
    assert_eq!(first.share_unit_adjustments()[0].shares_after, 17);
    assert_eq!(treasury_shares_of(&session, &stock), 17);
    assert_eq!(
        session
            .state
            .company_system
            .issuers()
            .get(&issuer)
            .unwrap()
            .issued_shares,
        187
    );
    assert_restores_twice_without_reposting(&session);
    approve_test_consolidation(
        &mut session,
        &stock,
        &issuer,
        "carry-seventeen",
        17,
        "2030-01-14",
        "2030-01-15",
    );
    advance_to_date(&mut session, "2030-01-15");
    complete_day(&mut session);
    let first = session
        .state
        .corporate_actions
        .issuer_repurchases
        .iter()
        .find(|book| book.plan().event_id == "repurchase-2030")
        .unwrap();
    let context = session.state.corporate_actions.split_unit_context(&stock);
    assert_eq!(first.current_inventory_shares(&context).unwrap(), 1);
    assert_eq!(
        first
            .plan()
            .execution_max_shares(CivilDate::from_iso("2030-01-15").unwrap(), &context)
            .unwrap(),
        0
    );
    assert_eq!(first.cancelled_shares(), 1);
    assert_eq!(first.fills()[0].shares, 100);
    assert_eq!(treasury_shares_of(&session, &stock), 1);
    assert_eq!(
        session
            .state
            .company_system
            .issuers()
            .get(&issuer)
            .unwrap()
            .issued_shares,
        11
    );
    assert_eq!(
        session
            .state
            .company_system
            .dividend_legal_facts(&issuer)
            .unwrap()
            .unwrap()
            .registered_capital,
        AccountingAmount::from_cents(112200)
    );
    assert_registry_conserves_issued(&session, &issuer);
    assert_restores_twice_without_reposting(&session);
    emit_schema_fixture(&session, "carry-two-consolidations");
}

#[test]
fn equal_quantity_unit_change_records_one_to_one_adjustment() {
    let (mut session, stock, issuer) = small_treasury_session(0, 201);
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    complete_day(&mut session);
    fill_max_shares_on_window_day(&mut session, &stock);
    session
        .execute_issuer_repurchase_cancellation("repurchase-2030", 99, session.civil_date())
        .unwrap();
    complete_day(&mut session);
    approve_test_consolidation(
        &mut session,
        &stock,
        &issuer,
        "equal-unit",
        2,
        "2030-01-07",
        "2030-01-08",
    );
    advance_to_date(&mut session, "2030-01-08");
    let treasury = session.state.corporate_actions.share_splits[0]
        .receipt()
        .unwrap()
        .holders
        .iter()
        .find(|outcome| outcome.holder == HolderId::IssuerTreasury)
        .unwrap();
    assert_eq!(
        (
            treasury.original_shares,
            treasury.new_shares,
            treasury.tie_break_award
        ),
        (1, 1, 1)
    );
    complete_day(&mut session);
    let book = &session.state.corporate_actions.issuer_repurchases[0];
    assert_eq!(book.share_unit_adjustments().len(), 1);
    assert_eq!(
        (
            book.share_unit_adjustments()[0].shares_before,
            book.share_unit_adjustments()[0].shares_after
        ),
        (1, 1)
    );
    assert_eq!(treasury_shares_of(&session, &stock), 1);
    assert_eq!(
        session
            .state
            .company_system
            .dividend_legal_facts(&issuer)
            .unwrap()
            .unwrap()
            .registered_capital,
        AccountingAmount::from_cents(10200)
    );
    assert_registry_conserves_issued(&session, &issuer);
    assert_restores_twice_without_reposting(&session);
    emit_schema_fixture(&session, "equal-one-unit");
}

fn cancelled_small_session() -> (
    GameSession,
    StockCode,
    CompanyId,
    CivilDate,
    AccountingAmount,
) {
    let (mut session, stock, issuer) = small_treasury_session(0, 1000);
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    complete_day(&mut session);
    fill_max_shares_on_window_day(&mut session, &stock);
    let cancelled_on = session.civil_date();
    complete_day(&mut session);
    let capital = session
        .state
        .company_system
        .dividend_legal_facts(&issuer)
        .unwrap()
        .unwrap()
        .registered_capital;
    assert_eq!(capital, AccountingAmount::from_cents(90000));
    (session, stock, issuer, cancelled_on, capital)
}

#[test]
fn historical_cancellation_par_survives_later_paid_rights_issuance() {
    use crate::company::rights_offering::{
        DirectedPlacementTarget, RightsOfferingEventPlan, RightsOfferingMode,
        RightsSubscriptionStrategy,
    };
    let (mut session, stock, issuer, cancelled_on, capital_before) = cancelled_small_session();
    let cancelled_fact = session
        .state
        .company_system
        .issuer_repurchase_facts(&issuer)
        .unwrap()[0]
        .clone();
    let plan = RightsOfferingEventPlan {
        event_id: "later-capital-rights".into(),
        approval_reference: "board-later-rights".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        approved_on: session.civil_date(),
        announced_on: session.civil_date(),
        registered_on: CivilDate::from_iso("2030-01-07").unwrap(),
        payment_start_on: CivilDate::from_iso("2030-01-08").unwrap(),
        payment_deadline_on: CivilDate::from_iso("2030-01-08").unwrap(),
        ex_rights_on: CivilDate::from_iso("2030-01-09").unwrap(),
        settlement_on: CivilDate::from_iso("2030-01-10").unwrap(),
        listing_on: CivilDate::from_iso("2030-01-10").unwrap(),
        price_per_share: Money::from_cents(1000),
        mode: RightsOfferingMode::DirectedPlacement {
            targets: vec![DirectedPlacementTarget::NamedHolder {
                holder: HolderId::Account(AccountId(0)),
                shares: 100,
                lock_until: None,
            }],
        },
        npc_subscription_strategy: RightsSubscriptionStrategy::FullByDefault,
    };
    session.approve_rights_offering(plan).unwrap();
    advance_to_date(&mut session, "2030-01-08");
    let investor_cash = session.account(AccountId(0)).unwrap().cash();
    session
        .subscribe_rights_offering("later-capital-rights", AccountId(0), 100)
        .unwrap();
    complete_day(&mut session);
    assert_eq!(
        session.account(AccountId(0)).unwrap().cash(),
        investor_cash.sub(Money::from_cents(100000)).unwrap()
    );
    advance_to_date(&mut session, "2030-01-10");
    complete_day(&mut session);
    assert_eq!(
        session
            .state
            .company_system
            .dividend_legal_facts(&issuer)
            .unwrap()
            .unwrap()
            .registered_capital,
        capital_before
            .add(AccountingAmount::from_cents(10000))
            .unwrap()
    );
    assert_eq!(
        session
            .state
            .company_system
            .registered_capital_at(&issuer, cancelled_on)
            .unwrap(),
        Some(capital_before)
    );
    assert_eq!(
        session
            .state
            .company_system
            .issuer_repurchase_facts(&issuer)
            .unwrap()[0],
        cancelled_fact
    );
    assert_registry_conserves_issued(&session, &issuer);
    assert_restores_twice_without_reposting(&session);
}

fn assert_named_plan_keeps_a_distinct_treasury_bucket(raw_id: &str) {
    let (mut session, stock, issuer) = small_treasury_session(2, 1000);
    let mut plan = repurchase_plan(&issuer, &stock);
    plan.event_id = raw_id.into();
    session.approve_issuer_repurchase(plan).unwrap();
    session
        .approve_share_split(ShareSplitEventPlan {
            event_id: "named-plan-split".into(),
            approval_reference: "board-named-plan".into(),
            issuer: issuer.clone(),
            stock: stock.clone(),
            exchange: crate::calendar::CalendarExchange::Sse,
            direction: ShareSplitDirection::Split,
            ratio: 2,
            approved_on: session.civil_date(),
            announced_on: session.civil_date(),
            registered_on: CivilDate::from_iso("2030-01-03").unwrap(),
            ex_rights_on: CivilDate::from_iso("2030-01-04").unwrap(),
        })
        .unwrap();
    complete_day(&mut session);
    fill_max_shares_on_window_day(&mut session, &stock);
    complete_day(&mut session);
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases[0].cancelled_shares(),
        200
    );
    assert_eq!(treasury_shares_of(&session, &stock), 4);
    assert_eq!(
        session
            .state
            .company_system
            .dividend_legal_facts(&issuer)
            .unwrap()
            .unwrap()
            .registered_capital,
        AccountingAmount::from_cents(90200)
    );
    assert_registry_conserves_issued(&session, &issuer);
    assert_restores_twice_without_reposting(&session);
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases[0]
            .plan()
            .event_id,
        raw_id
    );
    if raw_id == "initial" {
        emit_schema_fixture(&session, "named-initial-plan");
    }
}

#[test]
fn named_initial_plan_keeps_a_distinct_treasury_bucket() {
    assert_named_plan_keeps_a_distinct_treasury_bucket("initial");
}

#[test]
fn named_prefixed_initial_plan_keeps_a_distinct_treasury_bucket() {
    assert_named_plan_keeps_a_distinct_treasury_bucket("plan:initial");
}

#[test]
fn named_colon_plan_keeps_a_distinct_treasury_bucket() {
    assert_named_plan_keeps_a_distinct_treasury_bucket("left:issuer-treasury:plan:尾部");
}

#[test]
fn treasury_split_lot_identity_cannot_alias_string_boundaries() {
    let first = corporate_actions::treasury_split_lot_id("a", "plan:b:issuer-treasury:plan:c");
    let second = corporate_actions::treasury_split_lot_id("a:issuer-treasury:plan:b", "plan:c");
    assert_ne!(first, second, "两段任意身份必须无歧义编码，不能裸分隔拼接");
}

#[test]
fn r_day_restore_keeps_old_units_until_the_ex_event_is_applied() {
    let (mut session, stock, issuer, seller) = normal_repurchase_session(true);
    session
        .approve_issuer_repurchase(repurchase_plan(&issuer, &stock))
        .unwrap();
    approve_test_consolidation(
        &mut session,
        &stock,
        &issuer,
        "r-day-units",
        2,
        "2030-01-03",
        "2030-01-04",
    );
    complete_day(&mut session);
    trade_repurchase_from_holder(&mut session, &stock, seller);
    session.end_civil_day().unwrap();
    assert_eq!(
        session.civil_date(),
        CivilDate::from_iso("2030-01-04").unwrap()
    );
    assert_eq!(
        session.state.corporate_actions.issuer_repurchases[0]
            .current_inventory_shares(&session.state.corporate_actions.split_unit_context(&stock))
            .unwrap(),
        100
    );
    assert_restores_twice_without_reposting(&session);
    emit_schema_fixture(&session, "r-day-old-units");
}

#[test]
fn approval_unit_anchor_requires_the_actual_applied_trade_event() {
    let (mut session, stock, issuer, _) = normal_repurchase_session(true);
    // 合法零成交完成方案预先建立专户，避免首次开户跨 R 登记的既有独立限制。
    let mut opening = repurchase_plan(&issuer, &stock);
    opening.event_id = "opening-zero-fill".into();
    opening.total_budget = Money::from_cents(50000);
    opening.window_start_on = session.civil_date();
    session.approve_issuer_repurchase(opening).unwrap();
    approve_test_consolidation(
        &mut session,
        &stock,
        &issuer,
        "approval-applied-units",
        2,
        "2030-01-03",
        "2030-01-04",
    );
    complete_day(&mut session);
    complete_day(&mut session);
    assert_eq!(
        session.current_trading_unit_anchor(&stock, session.civil_date()),
        None,
        "ex 日期已到但首 tick 尚未应用事件，不能按日期猜批准锚"
    );
    let mut plan = repurchase_plan(&issuer, &stock);
    plan.approved_on = session.civil_date();
    plan.announced_on = session.civil_date();
    plan.window_start_on = CivilDate::from_iso("2030-01-07").unwrap();
    plan.window_deadline_on = CivilDate::from_iso("2030-01-09").unwrap();
    session.approve_issuer_repurchase(plan).unwrap();
    session.step().unwrap();
    assert_eq!(
        session.current_trading_unit_anchor(&stock, session.civil_date()),
        Some("approval-applied-units".into())
    );
}

#[test]
fn backfilled_approval_in_current_units_restores_and_executes_without_old_adjustments() {
    let (mut session, stock, issuer, seller) = normal_repurchase_session(true);
    let mut opening = repurchase_plan(&issuer, &stock);
    opening.event_id = "backfill-opening".into();
    opening.total_budget = Money::from_cents(50000);
    opening.window_start_on = session.civil_date();
    session.approve_issuer_repurchase(opening).unwrap();
    session.approve_share_split(ShareSplitEventPlan {
        event_id: "backfill-split1".into(), approval_reference: "board-backfill-split".into(),
        issuer: issuer.clone(), stock: stock.clone(), exchange: crate::calendar::CalendarExchange::Sse,
        direction: ShareSplitDirection::Split, ratio: 2,
        approved_on: session.civil_date(), announced_on: session.civil_date(),
        registered_on: CivilDate::from_iso("2030-01-03").unwrap(), ex_rights_on: CivilDate::from_iso("2030-01-04").unwrap(),
    }).unwrap();
    complete_day(&mut session);
    complete_day(&mut session);
    complete_day(&mut session);
    assert_eq!(session.civil_date(), CivilDate::from_iso("2030-01-05").unwrap());
    assert_eq!(session.current_trading_unit_anchor(&stock, session.civil_date()), Some("backfill-split1".into()));
    let mut plan = repurchase_plan(&issuer, &stock);
    plan.event_id = "backfill-current-unit".into();
    plan.approved_on = CivilDate::from_iso("2030-01-02").unwrap();
    plan.announced_on = session.civil_date();
    plan.window_start_on = CivilDate::from_iso("2030-01-07").unwrap();
    plan.window_deadline_on = CivilDate::from_iso("2030-01-09").unwrap();
    plan.approval_unit_event_id = Some("backfill-split1".into());
    plan.price_cap_per_share = Money::from_cents(550);
    let approved_plan = plan.clone();
    let admission = session.approve_issuer_repurchase(plan);
    eprintln!("回填 approved_on=01-02，01-05 受理当前 split1 单位：{admission:?}");
    admission.expect("既有 API 允许回填批准日，不增加必须今天的产品限制");
    let saved = session.save().unwrap();
    session = GameSession::restore(&saved).expect("受理成功的当前锚方案不得因旧事件缺假零事实而拒恢复");
    assert_restores_twice_without_reposting(&session);
    let book = session.state.corporate_actions.issuer_repurchases.iter().find(|book| book.plan().event_id == "backfill-current-unit").unwrap();
    assert_eq!(*book.plan(), approved_plan);
    assert!(book.share_unit_adjustments().is_empty(), "批准锚已经包含的旧事件不得重做调整");
    emit_schema_fixture(&session, "backfilled-current-unit");
    advance_to_date(&mut session, "2030-01-07");
    let received = session.receive_private_intent(seller, Intent::PlaceLimit { code: stock.clone(), side: Side::Sell, price: LimitPrice::Fixed(Money::from_cents(500)), qty: 100 }).unwrap();
    session.state.pending_player.push(received);
    for _ in 0..session.state.setup.ticks_per_day { session.step().unwrap(); }
    let account = session.issuer_repurchase_account_id().unwrap();
    let rows: Vec<_> = session.state.personal_trade_confirmations[&account].iter().filter(|row| row.civil_date == session.civil_date()).collect();
    assert_eq!(rows.iter().map(|row| row.quantity_shares).sum::<u32>(), 100);
    assert!(rows.iter().all(|row| row.gross.cents() / i64::from(row.quantity_shares) <= 550));
    let capital = session.state.company_system.dividend_legal_facts(&issuer).unwrap().unwrap().registered_capital;
    session.end_civil_day().unwrap();
    complete_day(&mut session);
    let book = session.state.corporate_actions.issuer_repurchases.iter().find(|book| book.plan().event_id == "backfill-current-unit").unwrap();
    assert_eq!(*book.plan(), approved_plan);
    assert_eq!(book.fills()[0].shares, 100);
    assert_eq!(book.cancelled_shares(), 100);
    assert!(book.share_unit_adjustments().is_empty());
    assert_eq!(session.state.company_system.dividend_legal_facts(&issuer).unwrap().unwrap().registered_capital, capital.sub(AccountingAmount::from_cents(5000)).unwrap());
    assert_registry_conserves_issued(&session, &issuer);
    assert_restores_twice_without_reposting(&session);
}

#[test]
fn historical_cancellation_capital_survives_later_stock_distribution() {
    use crate::company::stock_distribution::{StockDistributionEventPlan, StockDistributionKind};
    let (mut session, stock, issuer, cancelled_on, capital_before) = cancelled_small_session();
    let cancelled_fact = session
        .state
        .company_system
        .issuer_repurchase_facts(&issuer)
        .unwrap()[0]
        .clone();
    session
        .approve_stock_distribution(StockDistributionEventPlan {
            event_id: "later-capital-bonus".into(),
            approval_reference: "board-later-bonus".into(),
            issuer: issuer.clone(),
            stock: stock.clone(),
            exchange: crate::calendar::CalendarExchange::Sse,
            kind: StockDistributionKind::BonusShares,
            approved_on: session.civil_date(),
            announced_on: session.civil_date(),
            registered_on: CivilDate::from_iso("2030-01-07").unwrap(),
            ex_rights_on: CivilDate::from_iso("2030-01-08").unwrap(),
            shares_per_existing_share_micros: 100000,
            approved_total_new_shares: 90,
        })
        .unwrap();
    advance_to_date(&mut session, "2030-01-08");
    complete_day(&mut session);
    assert_eq!(
        session
            .state
            .company_system
            .dividend_legal_facts(&issuer)
            .unwrap()
            .unwrap()
            .registered_capital,
        capital_before
            .add(AccountingAmount::from_cents(9000))
            .unwrap()
    );
    assert_eq!(
        session
            .state
            .company_system
            .registered_capital_at(&issuer, cancelled_on)
            .unwrap(),
        Some(capital_before)
    );
    assert_eq!(
        session
            .state
            .company_system
            .issuer_repurchase_facts(&issuer)
            .unwrap()[0],
        cancelled_fact
    );
    assert_registry_conserves_issued(&session, &issuer);
    assert_restores_twice_without_reposting(&session);
}
