use super::fixtures::*;
use engine::company::operations::CompanyOperations;
use engine::company::{ActiveShock, CompanyId, ShockKind};

#[test]
fn production_sampling_never_activates_inapplicable_industry_shocks() {
    let date = d("2030-01-01");
    let config = four_company_config(
        7,
        engine::company::ShockParams::stress_parameters(),
        date.prev().unwrap(),
    );
    let ids: Vec<_> = config
        .companies
        .iter()
        .map(|company| company.spec.id.clone())
        .collect();
    let mut ops = CompanyOperations::new(config, date).unwrap();
    ops.advance_civil_day(date).unwrap();
    for id in ids {
        let company = ops.company(&id).unwrap();
        assert!(company
            .economy()
            .active()
            .iter()
            .all(|shock| shock.kind.applies_to(company.spec().kind)));
    }
}

#[test]
fn unsupported_production_or_payment_fact_cannot_be_injected_as_bank_shock() {
    let date = d("2030-01-01");
    let mut ops = CompanyOperations::new(
        four_company_config(7, quiet_params(), date.prev().unwrap()),
        date,
    )
    .unwrap();
    for kind in [
        ShockKind::ProductionInterruption,
        ShockKind::AssetImpairmentSignal,
        ShockKind::PaymentFailure {
            what: "payment".into(),
            amount: amt(100),
        },
    ] {
        let before = serde_json::to_value(&ops).unwrap();
        assert!(ops
            .apply_company_shock(
                &CompanyId("C-BANK".into()),
                ActiveShock {
                    kind,
                    amplitude_bp: 0,
                    starts_on: date,
                    expires_on: date
                }
            )
            .is_err());
        assert_eq!(serde_json::to_value(&ops).unwrap(), before);
    }
}
