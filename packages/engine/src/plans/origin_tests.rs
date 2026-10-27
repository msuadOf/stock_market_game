//! RP-Origin 真实 PlanBook 入口与直接 tagged wire 的独立首红；不表示已注资或已接真实执行器。

use super::*;
use crate::company::CompanyId;
use serde_json::{json, Value};

fn investor(owner: u64, stock: u32) -> PlanOpen {
    PlanOpen {
        account: AccountId(owner),
        code: StockId(stock),
        direction: Side::Buy,
        target: PlanTarget::ShareCount(100),
        opinion: PlanOpinion {
            signal_score_bp: 3000,
            source: OpinionSource::Blended,
        },
        confidence_bp: 8000,
        urgency: Urgency::Normal,
        horizon_trading_days: 5,
        created_trading_day: 0,
    }
}

fn issuer(stock: u32) -> RepurchaseExecutionOpen {
    RepurchaseExecutionOpen {
        owner: AccountId(17),
        issuer: CompanyId(3),
        stock: StockId(stock),
    }
}

fn issuer_wire(status: &str) -> Value {
    json!({
        "plan_id": {"owner": "17", "sequence": "0"},
        "issuer": 3,
        "stock": 7,
        "status": status,
        "execution": null,
    })
}

fn mixed_wire() -> Value {
    let mut book = PlanBook::default();
    let id = book.create(investor(1, 2)).unwrap();
    json!({
        "policy": book.policy(),
        "owner_sequences": [
            {"owner": "1", "next_sequence": "1"},
            {"owner": "17", "next_sequence": "1"},
        ],
        "plans": [
            {"Investor": book.plan(id).unwrap()},
            {"IssuerRepurchase": issuer_wire("Funded")},
        ],
    })
}

#[test]
fn origin_01_company_create_uses_the_existing_owner_sequence_without_personal_opinion() {
    let mut book = PlanBook::default();
    let personal = book.create(investor(17, 2)).unwrap();
    let company = book.create_issuer_repurchase(issuer(7)).unwrap();
    assert_eq!(
        personal,
        PlanId {
            owner: AccountId(17),
            sequence: 0
        }
    );
    assert_eq!(
        company,
        PlanId {
            owner: AccountId(17),
            sequence: 1
        }
    );
    assert_eq!(
        book.owner_sequences().collect::<Vec<_>>(),
        vec![OwnerSequence {
            owner: AccountId(17),
            next_sequence: Some(2)
        }]
    );
    let encoded = serde_json::to_value(&book).unwrap();
    let company = &encoded["plans"][1]["IssuerRepurchase"];
    assert_eq!(company["plan_id"]["sequence"], "1");
    assert_eq!(company["issuer"], 3);
    assert_eq!(company["stock"], 7);
    assert_eq!(company["status"], "Funded");
    assert_eq!(company["execution"], Value::Null);
    for field in [
        "opinion",
        "confidence_bp",
        "urgency",
        "horizon_trading_days",
    ] {
        assert!(
            company.get(field).is_none(),
            "公司授权不能伪造个人字段：{field}"
        );
    }
}

#[test]
fn origin_02_investor_conflict_rejects_company_create_without_consuming_sequence() {
    let mut book = PlanBook::default();
    book.create(investor(17, 7)).unwrap();
    let before = book.clone();
    assert_eq!(
        book.create_issuer_repurchase(issuer(7)),
        Err(PlanError::DuplicateActivePlan {
            account: AccountId(17),
            code: StockId(7)
        })
    );
    assert_eq!(book, before);
}

#[test]
fn origin_03_company_conflict_rejects_both_origins_and_keeps_the_shared_book_unchanged() {
    let mut book = PlanBook::default();
    let id = book.create_issuer_repurchase(issuer(7)).unwrap();
    let before = book.clone();
    for result in [
        book.create(investor(17, 7)),
        book.create_issuer_repurchase(issuer(7)),
    ] {
        assert_eq!(
            result,
            Err(PlanError::DuplicateActivePlan {
                account: AccountId(17),
                code: StockId(7)
            })
        );
    }
    assert_eq!(book, before);
    assert_eq!(book.plan_ids().collect::<Vec<_>>(), vec![id]);
}

#[test]
fn origin_04_company_allocates_u64_max_once_then_reports_exhaustion_without_reuse() {
    let mut book = PlanBook::from_parts(
        PlanPolicy::default(),
        vec![OwnerSequence {
            owner: AccountId(17),
            next_sequence: Some(u64::MAX),
        }],
        Vec::new(),
    )
    .unwrap();
    assert_eq!(
        book.create_issuer_repurchase(issuer(7)).unwrap(),
        PlanId {
            owner: AccountId(17),
            sequence: u64::MAX
        }
    );
    let before = book.clone();
    assert_eq!(
        book.create_issuer_repurchase(issuer(8)),
        Err(PlanError::PlanSequenceExhausted)
    );
    assert_eq!(book, before);
}

#[test]
fn origin_05_direct_mixed_tagged_wire_restores_both_input_orders_and_roundtrips_canonically() {
    let wire = mixed_wire();
    let restored: PlanBook = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        restored.plan_ids().collect::<Vec<_>>(),
        vec![
            PlanId {
                owner: AccountId(1),
                sequence: 0
            },
            PlanId {
                owner: AccountId(17),
                sequence: 0
            },
        ]
    );
    assert_eq!(serde_json::to_value(&restored).unwrap(), wire);
    let mut reversed = wire.clone();
    reversed["plans"].as_array_mut().unwrap().reverse();
    let restored_reversed: PlanBook = serde_json::from_value(reversed).unwrap();
    assert_eq!(restored_reversed, restored);
}

#[test]
fn origin_06_closed_company_history_allows_new_sequence_but_is_not_erased() {
    let mut wire = mixed_wire();
    wire["plans"][1]["IssuerRepurchase"]["status"] = json!("Closed");
    let mut book: PlanBook = serde_json::from_value(wire).unwrap();
    assert_eq!(
        book.create_issuer_repurchase(issuer(7)).unwrap(),
        PlanId {
            owner: AccountId(17),
            sequence: 1
        }
    );
    assert_eq!(book.plan_ids().count(), 3);
    let saved = serde_json::to_value(book).unwrap();
    assert_eq!(saved["plans"][1]["IssuerRepurchase"]["status"], "Closed");
    assert_eq!(saved["plans"][2]["IssuerRepurchase"]["status"], "Funded");
}

#[test]
fn origin_07_legacy_bare_investor_wire_is_rejected_without_an_origin_default() {
    let mut book = PlanBook::default();
    book.create(investor(1, 2)).unwrap();
    let mut wire = mixed_wire();
    wire["owner_sequences"] = json!([{"owner": "1", "next_sequence": "1"}]);
    wire["plans"] = json!([serde_json::to_value(
        book.plan(PlanId {
            owner: AccountId(1),
            sequence: 0
        })
        .unwrap()
    )
    .unwrap()]);
    assert!(
        serde_json::from_value::<PlanBook>(wire).is_err(),
        "旧裸个人结构不得默认成Investor"
    );
}

#[test]
fn origin_08_tagged_wire_rejects_unknown_multiple_and_missing_origin_or_cursor() {
    for invalid in 0..4 {
        let mut wire = mixed_wire();
        match invalid {
            0 => wire["plans"][1] = json!({"Unknown": issuer_wire("Funded")}),
            1 => wire["plans"][1]["Investor"] = wire["plans"][0]["Investor"].clone(),
            2 => wire["plans"][1] = issuer_wire("Funded"),
            _ => {
                wire["owner_sequences"].as_array_mut().unwrap().pop();
            }
        }
        assert!(
            serde_json::from_value::<PlanBook>(wire).is_err(),
            "损坏origin/cursor应明确拒绝：{invalid}"
        );
    }
}

#[test]
fn origin_09_direct_company_execution_wire_rejects_zero_quantity_and_nonpositive_price() {
    for (qty, price) in [(0, "1234"), (100, "0"), (100, "-1")] {
        let mut wire = issuer_wire("Funded");
        wire["execution"] = json!({
            "child_qty": qty,
            "limit_price": price,
            "expires_market_minute": 30,
            "active_child_remaining_qty": null,
        });
        assert!(
            serde_json::from_value::<RepurchaseExecutionPlan>(wire).is_err(),
            "公司执行腿损坏必须拒绝：qty={qty},price={price}"
        );
    }
}
