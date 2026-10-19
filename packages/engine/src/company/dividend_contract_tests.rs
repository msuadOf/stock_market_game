use super::{
    DistributableProfit, DividendDeclaration, DividendLegalFacts, DividendPaymentFact,
    DividendPaymentReceipt, DividendPlanFact, DividendPlanReceipt,
};
use crate::accounting::{AccountingAmount, BusinessEventId};
use crate::calendar::CivilDate;

fn assert_strict_roundtrip<T>(value: &T)
where
    T: serde::Serialize + for<'de> serde::Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    let wire = serde_json::to_value(value).unwrap();
    assert_eq!(serde_json::from_value::<T>(wire.clone()).unwrap(), *value);
    let mut unknown_field = wire;
    unknown_field
        .as_object_mut()
        .unwrap()
        .insert("unexpected".into(), serde_json::Value::Null);
    assert!(serde_json::from_value::<T>(unknown_field).is_err());
}

#[test]
fn public_dividend_contract_types_have_strict_wire_roundtrips() {
    let amount = AccountingAmount::from_cents(12_345);
    let date = CivilDate::from_ymd(2030, 6, 30).unwrap();
    let payment = DividendPaymentFact {
        payment_id: "payment-1".into(),
        paid_on: date,
        amount,
        source: BusinessEventId::new(7),
    };
    let contract_values: Vec<serde_json::Value> = vec![
        serde_json::to_value(DividendLegalFacts {
            registered_capital: amount,
            source_evidence: "registry record".into(),
        })
        .unwrap(),
        serde_json::to_value(DistributableProfit {
            accumulated_after_loss: amount,
            statutory_reserve: amount,
            available_for_distribution: amount,
            reserve_basis_year: Some(2029),
        })
        .unwrap(),
        serde_json::to_value(DividendDeclaration {
            plan_id: "plan-1".into(),
            approved_on: date,
            total_gross: amount,
            registered_capital: amount,
        })
        .unwrap(),
        serde_json::to_value(DividendPlanReceipt {
            plan_id: "plan-1".into(),
            amount,
            statutory_reserve: amount,
            already_declared: false,
        })
        .unwrap(),
        serde_json::to_value(DividendPaymentReceipt {
            plan_id: "plan-1".into(),
            payment_id: "payment-1".into(),
            amount,
            paid_on: date,
            already_paid: false,
            simple_display_only: true,
            within_six_month_deadline: true,
        })
        .unwrap(),
        serde_json::to_value(DividendPlanFact {
            plan_id: "plan-1".into(),
            approved_on: date,
            total_gross: amount,
            registered_capital: amount,
            registered_capital_source_evidence: "registry record".into(),
            declaration_source: BusinessEventId::new(6),
            payments: vec![payment.clone()],
        })
        .unwrap(),
        serde_json::to_value(payment).unwrap(),
    ];

    assert_eq!(contract_values[0]["registered_capital"], "123.45");
    assert_eq!(contract_values[2]["approved_on"], "2030-06-30");

    assert_strict_roundtrip(&DividendLegalFacts {
        registered_capital: amount,
        source_evidence: "registry record".into(),
    });
    assert_strict_roundtrip(&DistributableProfit {
        accumulated_after_loss: amount,
        statutory_reserve: amount,
        available_for_distribution: amount,
        reserve_basis_year: Some(2029),
    });
    assert_strict_roundtrip(&DividendDeclaration {
        plan_id: "plan-1".into(),
        approved_on: date,
        total_gross: amount,
        registered_capital: amount,
    });
    assert_strict_roundtrip(&DividendPlanReceipt {
        plan_id: "plan-1".into(),
        amount,
        statutory_reserve: amount,
        already_declared: false,
    });
    assert_strict_roundtrip(&DividendPaymentReceipt {
        plan_id: "plan-1".into(),
        payment_id: "payment-1".into(),
        amount,
        paid_on: date,
        already_paid: false,
        simple_display_only: true,
        within_six_month_deadline: true,
    });
    assert_strict_roundtrip(&DividendPlanFact {
        plan_id: "plan-1".into(),
        approved_on: date,
        total_gross: amount,
        registered_capital: amount,
        registered_capital_source_evidence: "registry record".into(),
        declaration_source: BusinessEventId::new(6),
        payments: vec![DividendPaymentFact {
            payment_id: "payment-1".into(),
            paid_on: date,
            amount,
            source: BusinessEventId::new(7),
        }],
    });
    assert_strict_roundtrip(&DividendPaymentFact {
        payment_id: "payment-1".into(),
        paid_on: date,
        amount,
        source: BusinessEventId::new(7),
    });
}
