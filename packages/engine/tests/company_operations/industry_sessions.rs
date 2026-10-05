use super::fixtures::*;
use engine::accounting::closing::ClosingEngine;
use engine::calendar::CalendarExchange;
use engine::company::operations::{
    CompanyOperations, CompanyOperationsConfig, OperatingCompanyConfig,
};
use engine::company::{CompanyKind, ShockKind, ShockParams};
use engine::information::{PublicLibrary, ReportFrequency};
use engine::session::{CivilClock, DayEndDisclosureCtx, DisclosureDispatch};

fn assert_actual_publication(company: OperatingCompanyConfig, seed: u64, params: ShockParams) {
    let date = d("2030-01-05");
    let id = company.spec.id.clone();
    let kind = company.spec.kind;
    let mut operations = CompanyOperations::new(
        CompanyOperationsConfig {
            seed,
            shock_params: params,
            companies: vec![company],
        },
        date,
    )
    .unwrap();
    operations.advance_civil_day(date).unwrap();
    let mut clock = CivilClock::new(date, CalendarExchange::Sse).unwrap();
    let day_end = clock.end_day(date).unwrap();
    let mut library = PublicLibrary::new();
    let mut dispatch = DisclosureDispatch::new(Some(day_end.disclosure_instant));
    let mut closing = ClosingEngine::new();
    let published = dispatch
        .run_day_end(DayEndDisclosureCtx {
            report_frequency: ReportFrequency::Quarterly,
            groups: &[],
            report: &day_end,
            ops: &operations,
            closing: &mut closing,
            library: &mut library,
        })
        .unwrap();
    let saved: CompanyOperations =
        serde_json::from_value(serde_json::to_value(&operations).unwrap()).unwrap();
    let active = saved.company(&id).unwrap().economy().active();
    assert!(active.iter().any(|shock| shock.starts_on == date));
    assert!(active.iter().all(|shock| shock.kind.applies_to(kind)));
    let saved_library = library.save();
    let restored_library = PublicLibrary::from_parts(saved_library.clone()).unwrap();
    assert_eq!(restored_library.save(), saved_library);
    let announcements: Vec<_> = saved_library
        .announcements
        .into_iter()
        .filter(|announcement| announcement.company == id)
        .collect();
    assert_eq!(announcements.len(), published.announcements_published.len());
    assert!(!announcements.is_empty());
    assert!(announcements.iter().all(|announcement| {
        announcement.occurred_on == date
            && announcement.published_at == day_end.disclosure_instant
            && announcement.event.kind.applies_to(kind)
    }));
    let economic_announcements: Vec<_> = announcements
        .iter()
        .filter(|announcement| {
            !matches!(
                &announcement.event.kind,
                ShockKind::PaymentFailure { .. }
            )
        })
        .collect();
    assert!(!economic_announcements.is_empty());
    assert!(economic_announcements.iter().all(|announcement| !matches!(
        &announcement.event.kind,
        ShockKind::ProductionInterruption | ShockKind::AssetImpairmentSignal
    )));
    if kind == CompanyKind::Bank {
        assert!(economic_announcements
            .iter()
            .all(|announcement| matches!(
                &announcement.event.kind,
                ShockKind::CreditDeterioration
            )));
    }
}

#[test]
fn bank_operations_publish_nonempty_applicable_credit_material_not_production_shocks() {
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
fn insurance_operations_publish_nonempty_demand_material_not_production_shocks() {
    let mut company = insurance_c(d("2027-12-31"));
    if let engine::company::operations::FlowParams::Insurance(params) = &mut company.flow {
        params.daily_groups_base = 1;
        params.coverage_days = 2;
        params.claim_every_days = 1;
    }
    assert_actual_publication(company, 7, ShockParams::stress_parameters());
}
