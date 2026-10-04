use super::fixtures::*;
use super::session_boundary::session_with_company;
use engine::company::operations::{
    CompanyOperations, CompanyOperationsConfig, OperatingCompanyConfig,
};
use engine::company::{CompanyKind, ShockKind, ShockParams};
use engine::session::{CompanyDisclosureKind, Event};

fn assert_actual_publication(company: OperatingCompanyConfig, seed: u64, params: ShockParams) {
    let date = d("2030-01-05");
    let id = company.spec.id.clone();
    let kind = company.spec.kind;
    let mut session = session_with_company(date, company, seed, params);
    let report = session.end_civil_day().unwrap();
    let saved = session.save().unwrap();
    let active = saved
        .company_operations
        .company(&id)
        .unwrap()
        .economy()
        .active();
    assert!(active.iter().any(|shock| shock.starts_on == date));
    assert!(active.iter().all(|shock| shock.kind.applies_to(kind)));
    let mut economic_announcements = Vec::new();
    for event in &report.events {
        if let Event::CompanyDisclosurePublished {
            publication_id,
            company,
            kind: CompanyDisclosureKind::Announcement,
            ..
        } = event
        {
            assert_eq!(company, &id);
            let announcement = saved
                .public_library
                .announcement(*publication_id, report.disclosure_instant)
                .unwrap();
            if !matches!(announcement.event.kind, ShockKind::PaymentFailure { .. }) {
                economic_announcements.push(&announcement.event.kind);
            }
        }
    }
    assert!(!economic_announcements.is_empty());
    assert!(economic_announcements
        .iter()
        .all(|shock| shock.applies_to(kind)));
    assert!(economic_announcements.iter().all(|shock| !matches!(
        shock,
        ShockKind::ProductionInterruption | ShockKind::AssetImpairmentSignal
    )));
    if kind == CompanyKind::Bank {
        assert!(economic_announcements
            .iter()
            .all(|shock| matches!(shock, ShockKind::CreditDeterioration)));
    }
}

#[test]
fn real_bank_session_publishes_nonempty_applicable_credit_material_not_production_shocks() {
    let date = d("2030-01-05");
    let company = bank_c(d("2027-12-31"));
    let mut params = quiet_params();
    params.company_candidate_bp = 10_000;
    let seed = (0..24)
        .find(|seed| {
            let mut ops = CompanyOperations::new(
                CompanyOperationsConfig {
                    seed: *seed,
                    shock_params: params.clone(),
                    companies: vec![company.clone()],
                },
                date,
            )
            .unwrap();
            ops.advance_civil_day(date).unwrap();
            !ops.company(&company.spec.id)
                .unwrap()
                .economy()
                .active()
                .is_empty()
        })
        .expect("short fixture seed must activate bank credit deterioration");
    assert_actual_publication(company, seed, params);
}

#[test]
fn real_insurance_session_publishes_nonempty_demand_material_not_production_shocks() {
    let mut company = insurance_c(d("2027-12-31"));
    if let engine::company::operations::FlowParams::Insurance(params) = &mut company.flow {
        params.daily_groups_base = 1;
        params.coverage_days = 2;
        params.claim_every_days = 1;
    }
    assert_actual_publication(company, 7, ShockParams::stress_parameters());
}
