//! 共同计划消费者必须穷尽处理 origin；公司执行仍受未接线门禁约束。

use super::*;
use crate::plans::{PlanBook, RepurchaseExecutionOpen};
use crate::{AccountId, StockId};

fn company_book() -> PlanBook {
    let mut book = PlanBook::default();
    book.create_issuer_repurchase(RepurchaseExecutionOpen {
        owner: AccountId(17),
        issuer: crate::company::CompanyId(3),
        stock: StockId(0),
    })
    .unwrap();
    book
}

fn state(book: PlanBook) -> GroupState {
    let mut state = GroupState::new(
        1,
        vec![GroupAccount::new(0, 1)],
        8,
        100,
        Arc::new(GameConfig::proposed_defaults()),
        240,
        None,
        Vec::new(),
        Box::new(|_, _| TickPlan::default()),
    );
    state.plan_book = book;
    state
}

#[test]
fn origin_seed_accepts_company_without_child_and_rejects_any_unconnected_execution() {
    let book = company_book();
    let prepared = prepare_plan_seeds(vec![book.clone()], &[AccountId(17)], 1, &[vec![]]).unwrap();
    assert_eq!(prepared[0].book, book);
    assert!(prepared[0].tickets.is_empty());
    let mut wire = serde_json::to_value(&book).unwrap();
    wire["plans"][0]["IssuerRepurchase"]["execution"] = serde_json::json!({
        "child_qty":100, "limit_price":"1234", "expires_market_minute":30,
        "active_child_remaining_qty":null,
    });
    let unsupported = serde_json::from_value(wire).unwrap();
    assert!(
        matches!(prepare_plan_seeds(vec![unsupported], &[AccountId(17)], 1, &[vec![]]),
        Err(RuntimeError::ProtocolInvariant { detail }) if detail.contains("公司计划尚未接入真实子单来源"))
    );
}

#[test]
fn origin_state_hash_covers_issuer_stock_status_and_complete_execution_fields() {
    let original = company_book();
    let original_hash = state(original.clone()).state_hash();
    for (field, value) in [
        ("issuer", serde_json::json!(4)),
        ("stock", serde_json::json!(1)),
        ("status", serde_json::json!("Draining")),
        ("status", serde_json::json!("Closed")),
    ] {
        let mut wire = serde_json::to_value(&original).unwrap();
        wire["plans"][0]["IssuerRepurchase"][field] = value;
        assert_ne!(
            state(serde_json::from_value(wire).unwrap()).state_hash(),
            original_hash,
            "公司字段不得漏hash：{field}"
        );
    }
    let mut wire = serde_json::to_value(&original).unwrap();
    wire["plans"][0]["IssuerRepurchase"]["execution"] = serde_json::json!({
        "child_qty":100, "limit_price":"1234", "expires_market_minute":30,
        "active_child_remaining_qty":null,
    });
    let execution_hash = state(serde_json::from_value(wire.clone()).unwrap()).state_hash();
    assert_ne!(execution_hash, original_hash);
    for (field, value) in [
        ("child_qty", serde_json::json!(200)),
        ("limit_price", serde_json::json!("1235")),
        ("expires_market_minute", serde_json::json!(31)),
        ("active_child_remaining_qty", serde_json::json!(100)),
    ] {
        let mut changed = wire.clone();
        changed["plans"][0]["IssuerRepurchase"]["execution"][field] = value;
        assert_ne!(
            state(serde_json::from_value(changed).unwrap()).state_hash(),
            execution_hash,
            "公司执行字段不得漏hash：{field}"
        );
    }
}

#[test]
fn origin_group_day_end_keeps_company_authorization_and_shared_owner_sequence() {
    let book = company_book();
    let mut state = state(book.clone());
    state
        .handle_day_end(&DayEndDirective {
            tick: 0,
            day: 99,
            rights: vec![],
        })
        .unwrap();
    assert_eq!(state.plan_book, book);
    assert!(state.child_in_flight.is_empty());
}
