//! P2 真实 PendingAdmin 短 fixture；数值正控不冒充公司批准或逐计划预算来源证明。

use super::*;
#[path = "../../information_fixture.rs"]
mod fixture;
use crate::company::CompanyId;
use crate::plans::{PlanId, RepurchaseExecutionOpen};
use crate::runtime::protocol::AdminTreasuryBinding;
use crate::{AccountId, StockId};

fn state() -> (GroupState, [PlanId; 2]) {
    let mut s = GroupState::new(1, vec![GroupAccount::new(700, 2); 2], 8, 100,
        Arc::new(GameConfig::proposed_defaults()), 100, None, Vec::new(),
        Box::new(|_, _| TickPlan::default()));
    s.root_projection = Some(fixture::projection([11, 37, 51, 79, 113].map(AccountId).to_vec()));
    s.roster_offset = 2;
    let ids = [0, 1].map(|stock| s.plan_book.create_issuer_repurchase(RepurchaseExecutionOpen {
        owner: AccountId(79), issuer: CompanyId(stock), stock: StockId(stock),
    }).unwrap());
    s.admin_treasury_bindings = [0, 1].map(|stock| AdminTreasuryBinding {
        owner: (1, 1), account: AccountId(79), issuer: CompanyId(stock), stock: stock as u16,
    }).to_vec();
    (s, ids)
}
fn credit(id: PlanId, stock: u16, amount: i64) -> AdminOperation {
    AdminOperation::SyntheticBudgetCredit { local: 1, stock, execution_plan: id, amount }
}
fn recycle(id: PlanId, stock: u16, amount: i64) -> AdminOperation {
    AdminOperation::SyntheticBudgetRecycle { local: 1, stock, execution_plan: id, amount }
}
fn replace(s: &mut GroupState, f: impl FnOnce(&mut GroupAccount)) {
    let mut account = (*s.accounts[1]).clone(); f(&mut account); s.accounts[1] = Arc::new(account);
}
fn rejected_without_drift(s: &mut GroupState, operations: Vec<AdminOperation>) {
    let accounts = s.accounts.clone(); let plans = s.plan_book.clone(); let hash = s.state_hash();
    assert!(s.prepare_admin(AdminTransactionId(1), operations).is_err());
    assert_eq!(s.accounts, accounts); assert_eq!(s.plan_book, plans); assert_eq!(s.state_hash(), hash);
    assert!(s.admin_pending.is_none());
}

#[test]
fn funds_p2_group_01_shared_account_two_company_entries_credit_and_recycle_only_requested_cents() {
    let (mut s, ids) = state();
    let before = s.accounts.clone(); let book = s.plan_book.clone();
    s.prepare_admin(AdminTransactionId(1), vec![credit(ids[0], 0, 101), credit(ids[1], 1, 203)]).unwrap();
    assert_eq!(s.accounts[1].cash, 1004);
    s.complete_admin(AdminTransactionId(1), AdminPhase::Commit).unwrap();
    s.prepare_admin(AdminTransactionId(2), vec![recycle(ids[0], 0, 101)]).unwrap();
    s.complete_admin(AdminTransactionId(2), AdminPhase::Commit).unwrap();
    let mut expected = (*before[1]).clone(); expected.cash = 903;
    assert_eq!(s.accounts[1].as_ref(), &expected);
    assert_eq!(s.accounts[0], before[0]); assert_eq!(s.plan_book, book);
}

#[test]
fn funds_p2_group_02_abort_restores_accounts_and_does_not_touch_tick_plan_undo() {
    let (mut s, ids) = state();
    s.cur_tick = 1; s.cur_round = 2; s.seal_acked_len = Some(1);
    s.undo.push(GroupUndo::Cash { local: 0, delta: 7 }); s.plan_undo_saved = true;
    s.plan_book_dirty = true; s.account_dirty_set.insert(0);
    let before = s.accounts.clone(); let book = s.plan_book.clone(); let hash = s.state_hash();
    let undo = format!("{:?}", s.undo); let published_hash = s.committed_hash;
    s.prepare_admin(AdminTransactionId(1), vec![credit(ids[0], 0, 101), recycle(ids[1], 1, 51)]).unwrap();
    s.complete_admin(AdminTransactionId(1), AdminPhase::Abort).unwrap();
    assert_eq!(s.accounts, before); assert_eq!(s.plan_book, book); assert_eq!(s.state_hash(), hash);
    assert_eq!(format!("{:?}", s.undo), undo); assert_eq!(s.committed_hash, published_hash);
    assert_eq!((s.cur_tick, s.committed_tick, s.cur_round), (1, 0, 2));
    assert_eq!(s.seal_acked_len, Some(1)); assert!(s.plan_undo_saved); assert!(s.plan_book_dirty);
    assert!(s.account_dirty_set.contains(&0));
}

#[test]
fn funds_p2_group_03_credit_allows_other_stock_live_resources_and_checks_public_cash() {
    let (mut s, ids) = state();
    replace(&mut s, |a| { a.buy_purse_held_cents = 31; a.foreign_active[0] = 1; });
    let before = s.accounts[1].clone();
    s.prepare_admin(AdminTransactionId(1), vec![credit(ids[1], 1, 101)]).unwrap();
    assert_eq!(s.accounts[1].cash, 801); assert_eq!(s.accounts[1].buy_purse_held_cents, 31);
    s.complete_admin(AdminTransactionId(1), AdminPhase::Abort).unwrap(); assert_eq!(s.accounts[1], before);
}

#[test]
fn funds_p2_group_04_nonpositive_overflow_and_shortfall_reject_without_partial_first_credit() {
    for mode in 0..7 {
        let (mut s, ids) = state();
        let op = match mode {
            0 => credit(ids[0], 0, 0), 1 => credit(ids[0], 0, -1),
            2 => recycle(ids[0], 0, 0), 3 => recycle(ids[0], 0, -1),
            4 => { replace(&mut s, |a| a.cash = i64::MAX - 1); credit(ids[0], 0, 2) },
            5 => { replace(&mut s, |a| { a.cash = i64::MAX - 2; a.buy_purse_held_cents = 2; }); credit(ids[0], 0, 1) },
            _ => recycle(ids[0], 0, 802),
        };
        let ops = if mode < 4 || mode == 6 { vec![credit(ids[1], 1, 101), op] } else { vec![op] };
        rejected_without_drift(&mut s, ops);
    }
}

#[test]
fn funds_p2_group_05_every_stock_pending_resources_and_real_ticket_independently_block_recycle() {
    for mode in 0..5 {
        let (mut s, ids) = state();
        match mode {
            0 => replace(&mut s, |a| a.buy_purse_held_cents = 1),
            1 => replace(&mut s, |a| a.foreign_active[1] = 1),
            2 => replace(&mut s, |a| a.pending_sell_qty[1] = 1),
            _ => { s.child_in_flight.insert((AccountId(79), StockId(1)), ChildTicket {
                qty: 100, side: Side::Buy, price: 1000, order_id: None, remaining: if mode == 3 { 100 } else { 0 }, charged_cents: 0,
            }); },
        }
        rejected_without_drift(&mut s, vec![recycle(ids[0], 0, 101)]);
    }
}

#[test]
fn funds_p2_group_06_binding_owner_account_issuer_stock_and_padding_are_not_guessed() {
    for mode in 0..8 {
        let (mut s, ids) = state();
        match mode {
            0 => s.admin_treasury_bindings.clear(),
            1 => s.admin_treasury_bindings[0].owner = (0, 1),
            2 => s.admin_treasury_bindings[0].owner = (1, 0),
            3 => s.admin_treasury_bindings[0].account = AccountId(51),
            4 => s.admin_treasury_bindings[0].issuer = CompanyId(99),
            5 => s.admin_treasury_bindings[0].stock = 1,
            6 => s.roster_offset = 4,
            _ => s.root_projection = None,
        }
        rejected_without_drift(&mut s, vec![credit(ids[0], 0, 101)]);
    }
}

#[test]
fn funds_p2_group_07_unknown_wrong_owner_wrong_stock_and_closed_entries_are_rejected() {
    for mode in 0..4 {
        let (mut s, ids) = state(); let mut id = ids[0]; let mut stock = 0;
        match mode {
            0 => id.sequence += 100, 1 => id.owner = AccountId(51), 2 => stock = 1,
            _ => { let mut wire = serde_json::to_value(&s.plan_book).unwrap();
                wire["plans"][0]["IssuerRepurchase"]["status"] = serde_json::json!("Closed");
                s.plan_book = serde_json::from_value(wire).unwrap(); },
        }
        rejected_without_drift(&mut s, vec![credit(id, stock, 101)]);
    }
}

#[test]
fn funds_p2_group_08_malformed_vectors_and_negative_balances_are_explicit_errors() {
    for mode in 0..9 {
        let (mut s, ids) = state();
        replace(&mut s, |a| match mode {
            0 => a.frozen_today = InlineVec::filled(0, 0), 1 => a.foreign_active = InlineVec::filled(0, 0),
            2 => a.pending_sell_qty = InlineVec::filled(0, 0), 3 => a.invested_cents = InlineVec::filled(0, 0),
            4 => a.recovered_cents = InlineVec::filled(0, 0), 5 => a.ever_held = InlineVec::filled(0, false),
            6 => a.cash = -1, 7 => a.buy_purse_held_cents = -1, _ => a.frozen_today[0] = 1,
        });
        let before = s.accounts.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(||
            s.prepare_admin(AdminTransactionId(1), vec![credit(ids[0], 0, 101)])));
        assert!(result.is_ok(), "损坏向量必须返回错误而非 panic：mode={mode}");
        assert!(result.unwrap().is_err()); assert_eq!(s.accounts, before); assert!(s.admin_pending.is_none());
    }
}

#[test]
fn funds_p2_group_09_valid_investor_identity_is_not_a_treasury_funding_source() {
    let (mut s, _) = state(); s.plan_book = crate::plans::PlanBook::default();
    let id = s.plan_book.create(crate::plans::PlanOpen {
        account: AccountId(79), code: StockId(0), direction: Side::Buy,
        target: crate::plans::PlanTarget::ShareCount(100),
        opinion: crate::plans::PlanOpinion { signal_score_bp: 0, source: crate::plans::OpinionSource::Fundamental },
        confidence_bp: 0, urgency: crate::plans::Urgency::Patient, horizon_trading_days: 20, created_trading_day: 0,
    }).unwrap();
    rejected_without_drift(&mut s, vec![credit(id, 0, 101)]);
}

#[test]
fn funds_p2_group_10_same_pending_info_failure_and_abort_restore_credit_and_library() {
    let (mut s, ids) = state();
    let old_facts = fixture::facts(8000); s.install_initial_company_facts(old_facts.clone()).unwrap();
    let old_library = s.library.clone(); let before = s.accounts.clone(); let book = s.plan_book.clone();
    let personal = s.belief_states.clone();
    let mut broken = fixture::payload(true);
    broken.observation.as_mut().unwrap().as_of_anchors.remove(&StockId(0));
    let error = s.prepare_admin_with_information(AdminTransactionId(1), vec![credit(ids[0], 0, 101)], Some(broken)).unwrap_err();
    assert!(matches!(error, RuntimeError::InformationPrepare { error:
        crate::runtime::protocol::InformationPrepareError::MissingInformationAnchor { stock: StockId(0) }, .. }));
    assert!(s.admin_pending.is_none()); assert_eq!(s.accounts, before); assert_eq!(s.plan_book, book);
    assert!(Arc::ptr_eq(&s.library, &old_library)); assert!(Arc::ptr_eq(s.current_company_facts().unwrap(), &old_facts));
    let payload = fixture::payload(false);
    let candidate = s.prepare_admin_with_information(AdminTransactionId(2), vec![credit(ids[0], 0, 101)], Some(payload.clone())).unwrap();
    assert!(candidate.information.unwrap().personals.is_empty());
    s.complete_admin(AdminTransactionId(2), AdminPhase::Abort).unwrap();
    assert_eq!(s.accounts, before); assert_eq!(s.belief_states, personal);
    assert!(Arc::ptr_eq(&s.library, &old_library)); assert!(Arc::ptr_eq(s.current_company_facts().unwrap(), &old_facts));
    s.prepare_admin_with_information(AdminTransactionId(3), vec![credit(ids[0], 0, 101)], Some(payload.clone())).unwrap();
    s.complete_admin(AdminTransactionId(3), AdminPhase::Commit).unwrap();
    assert_eq!(s.accounts[1].cash, 801); assert_eq!(s.belief_states, personal);
    assert!(Arc::ptr_eq(&s.library, &payload.library));
    assert!(Arc::ptr_eq(s.current_company_facts().unwrap(), &payload.current_facts));
}
