//! 公司互斥存档直接复用的真实 domain serde 必须拒绝缺字段和未知字段。
use engine::accounting::closing::ClosingEngine;
use engine::company::events::ShockParams;
use engine::company::operations::{CompanyOperations, CompanyOperationsConfig};
use engine::company::CompanyCatalog;

fn operations() -> CompanyOperations {
    CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 19,
            shock_params: ShockParams::current_default_parameters(),
            catalog: CompanyCatalog::from_codes(&[]).unwrap(),
            companies: vec![],
        },
        engine::CivilDate::from_iso("2030-06-03").unwrap(),
    )
    .unwrap()
}

#[test]
fn company_operations_rejects_unknown_saved_fields() {
    let mut value = serde_json::to_value(operations()).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<CompanyOperations>(value).is_err());
}

#[test]
fn company_operations_requires_nullable_next_expected_and_history() {
    for field in ["next_expected", "history"] {
        let mut value = serde_json::to_value(operations()).unwrap();
        value.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<CompanyOperations>(value).is_err(),
            "缺少 {field}"
        );
    }
}

#[test]
fn closing_engine_rejects_unknown_saved_fields() {
    let mut value = serde_json::to_value(ClosingEngine::new()).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ClosingEngine>(value).is_err());
}

// 合法Approved Book先验证完整JSON和每个显式null，再逐域验证missing必须拒绝。
fn assert_required_nullable<T>(book: T, fields: &[&str])
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    let original = serde_json::to_value(book).unwrap();
    let bytes = serde_json::to_vec(&original).unwrap();
    let decoded: T = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), original);
    let mut accepted = Vec::new();
    for field in fields {
        assert!(original[*field].is_null(), "合法Approved {field}应显式null");
        let mut explicit_null = original.clone();
        explicit_null[*field] = serde_json::Value::Null;
        assert!(
            serde_json::from_value::<T>(explicit_null).is_ok(),
            "{field}=null必须接受"
        );
        let mut missing = original.clone();
        missing.as_object_mut().unwrap().remove(*field);
        if serde_json::from_value::<T>(missing).is_ok() {
            accepted.push(*field);
        }
    }
    assert!(
        accepted.is_empty(),
        "缺失nullable字段被错误接受: {accepted:?}"
    );
}

mod split_required_nullable {
    use super::assert_required_nullable;
    use engine::account::StockCode;
    use engine::company::share_split::*;
    use engine::company::CompanyId;
    use engine::CivilDate;
    fn date(value: &str) -> CivilDate {
        CivilDate::from_iso(value).unwrap()
    }
    fn plan(direction: ShareSplitDirection, ratio: u64) -> ShareSplitEventPlan {
        ShareSplitEventPlan {
            event_id: "split-1".to_owned(),
            approval_reference: "shareholders-resolution-1".to_owned(),
            issuer: CompanyId(0),
            stock: StockCode("600001".to_owned()),
            exchange: engine::calendar::CalendarExchange::Sse,
            direction,
            ratio,
            approved_on: date("2030-06-01"),
            announced_on: date("2030-06-02"),
            registered_on: date("2030-06-03"),
            ex_rights_on: date("2030-06-04"),
        }
    }
    #[test]
    fn approved_book_requires_explicit_nullable_fields() {
        assert_required_nullable(
            ShareSplitBook::new(plan(ShareSplitDirection::Split, 10)).unwrap(),
            &["registration", "receipt", "settled_on"],
        );
    }
}

mod distribution_required_nullable {
    use super::assert_required_nullable;
    use engine::account::StockCode;
    use engine::calendar::CalendarExchange;
    use engine::company::stock_distribution::*;
    use engine::company::CompanyId;
    use engine::CivilDate;
    fn date(value: &str) -> CivilDate {
        CivilDate::from_iso(value).unwrap()
    }
    fn event_plan(ex_rights_on: &str) -> StockDistributionEventPlan {
        StockDistributionEventPlan {
            event_id: "distribution-1".into(),
            approval_reference: "shareholders-resolution-1".into(),
            issuer: CompanyId(0),
            stock: StockCode("600001".into()),
            exchange: CalendarExchange::Sse,
            kind: StockDistributionKind::BonusShares,
            approved_on: date("2030-06-01"),
            announced_on: date("2030-06-03"),
            registered_on: date("2030-06-06"),
            ex_rights_on: date(ex_rights_on),
            shares_per_existing_share_micros: 500_000,
            approved_total_new_shares: 3,
        }
    }
    #[test]
    fn approved_book_requires_explicit_nullable_fields() {
        assert_required_nullable(
            StockDistributionBook::new(event_plan("2030-06-07")).unwrap(),
            &["registration", "receipt", "credited_on"],
        );
    }
}

mod rights_required_nullable {
    use super::assert_required_nullable;
    use engine::account::StockCode;
    use engine::company::rights_offering::*;
    use engine::company::CompanyId;
    use engine::CivilDate;
    use engine::Money;
    fn date(value: &str) -> CivilDate {
        CivilDate::from_iso(value).unwrap()
    }
    fn rights_plan(mode: RightsOfferingMode) -> RightsOfferingEventPlan {
        RightsOfferingEventPlan {
            event_id: "rights-1".to_owned(),
            approval_reference: "board-resolution-1".to_owned(),
            issuer: CompanyId(0),
            stock: StockCode("600001".to_owned()),
            exchange: engine::calendar::CalendarExchange::Sse,
            approved_on: date("2030-06-01"),
            announced_on: date("2030-06-02"),
            registered_on: date("2030-06-03"),
            payment_start_on: date("2030-06-04"),
            payment_deadline_on: date("2030-06-11"),
            ex_rights_on: date("2030-06-12"),
            settlement_on: date("2030-06-13"),
            listing_on: date("2030-06-13"),
            price_per_share: Money::from_cents(800),
            mode,
            npc_subscription_strategy: RightsSubscriptionStrategy::FullByDefault,
        }
    }
    #[test]
    fn approved_book_requires_explicit_nullable_fields() {
        assert_required_nullable(
            RightsOfferingBook::new(rights_plan(RightsOfferingMode::RightsToAllShareholders {
                shares_per_existing_share_micros: 300_000,
            }))
            .unwrap(),
            &[
                "registration",
                "entitlement",
                "closed_on",
                "settlement",
                "credited_on",
            ],
        );
    }
}

mod repurchase_required_nullable {
    use super::assert_required_nullable;
    use engine::account::StockCode;
    use engine::company::issuer_repurchase::*;
    use engine::company::CompanyId;
    use engine::CivilDate;
    use engine::Money;
    fn date(value: &str) -> CivilDate {
        CivilDate::from_iso(value).unwrap()
    }
    fn plan() -> IssuerRepurchasePlan {
        IssuerRepurchasePlan {
            event_id: "repurchase-1".to_owned(),
            approval_reference: "board-resolution-r1".to_owned(),
            issuer: CompanyId(0),
            stock: StockCode("600001".to_owned()),
            exchange: engine::calendar::CalendarExchange::Sse,
            approved_on: date("2030-06-01"),
            announced_on: date("2030-06-02"),
            window_start_on: date("2030-06-03"),
            window_deadline_on: date("2030-06-11"),
            price_cap_per_share: Money::from_cents(1_200),
            total_budget: Money::from_cents(3_000_000),
            max_shares: 2_500,
            purpose: RepurchasePurpose::ReduceCapital,
            completion_policy: RepurchaseCompletionPolicy::CancelOnCompletion,
            approval_unit_event_id: None,
        }
    }
    #[test]
    fn approved_book_requires_explicit_nullable_fields() {
        assert_required_nullable(
            IssuerRepurchaseBook::new(plan()).unwrap(),
            &[
                "completed_on",
                "withdrawn_remainder",
                "cancelled_on",
                "last_order_day",
            ],
        );
    }
}
