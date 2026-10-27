use super::*;
#[path = "disclosure_correction_test_fixture.rs"]
mod fixture;
use crate::information::{MonthlyReportDelay, MonthlyReportSchedule, ReportFrequency};
use fixture::*;

fn setup(
    periods: u16,
) -> (
    crate::company::CompanySystem,
    PublicLibrary,
    Vec<PublicationId>,
) {
    let system = simple_model(periods);
    let frequency = ReportFrequency::Monthly {
        schedule: MonthlyReportSchedule::Custom {
            day: 1,
            second_of_day: 18 * 3600,
            delay: MonthlyReportDelay::None,
        },
    };
    let now = CivilInstant::from_hms(
        crate::calendar::CivilDate::from_iso("2030-01-01").unwrap(),
        18,
        0,
        0,
    )
    .unwrap();
    let mut library = PublicLibrary::new();
    let ids = crate::information::publish_simple_scheduled(
        &system,
        frequency,
        17,
        None,
        now,
        &mut library,
        false,
    )
    .unwrap();
    (system, library, ids)
}

#[test]
fn simple_wrapper_owns_changed_finance_and_all_cascade_reports_without_mutating_base() {
    let (system, base, originals) = setup(2);
    assert_eq!(originals.len(), 2);
    let source_before = system.clone();
    let base_before = base.clone();
    let date = crate::calendar::CivilDate::from_iso("2030-01-02").unwrap();
    let now = CivilInstant::from_hms(date, 18, 0, 0).unwrap();
    let prepared = system
        .prepare_disclosure_correction(
            &CompanyId(0),
            correction_operation(1000, date),
            originals[0],
            now,
            &base,
        )
        .unwrap();
    assert_eq!(system, source_before);
    assert_eq!(base, base_before);
    assert_ne!(
        prepared
            .source()
            .simple_book_display(&CompanyId(0))
            .unwrap(),
        system.simple_book_display(&CompanyId(0)).unwrap()
    );
    assert_eq!(prepared.proof().base(), &base);
    let reports = prepared.proof().reports();
    assert_eq!(reports.len(), 2, "主更正和后续期间 cascade 不能遗漏");
    assert!(reports.iter().all(|(request, report, source)| request
        .supersedes
        .is_some_and(|id| originals.contains(&id))
        && *source == PublicationSource::SimpleGenerated
        && report.version.sequence == 2));
    let second = system
        .prepare_disclosure_correction(
            &CompanyId(0),
            correction_operation(1000, date),
            originals[0],
            now,
            &base,
        )
        .unwrap();
    assert_eq!(prepared.source(), second.source());
    assert_eq!(
        prepared.proof().reports().len(),
        second.proof().reports().len()
    );
    for (left, right) in prepared
        .proof()
        .reports()
        .iter()
        .zip(second.proof().reports())
    {
        assert_eq!(left.0.supersedes, right.0.supersedes);
        assert_eq!(left.1, right.1);
        assert_eq!(left.2, right.2);
    }
    let final_source = prepared.into_source();
    assert_eq!(&final_source, second.source());
}

#[test]
fn simple_wrapper_preserves_actual_owner_rejection_and_original_finance() {
    let (system, base, originals) = setup(1);
    let before = system.clone();
    let library_before = base.clone();
    let date = crate::calendar::CivilDate::from_iso("2030-01-02").unwrap();
    let mut operation = correction_operation(1000, date);
    operation.entries.push(operation.entries[0].clone());
    let error = match system.prepare_disclosure_correction(
        &CompanyId(0),
        operation,
        originals[0],
        CivilInstant::from_hms(date, 18, 0, 0).unwrap(),
        &base,
    ) {
        Err(error) => error,
        Ok(_) => panic!("实际重复凭证应拒绝"),
    };
    assert!(
        matches!(
            error,
            crate::company::report_correction::DisclosureCorrectionPrepareError::Owner(
                crate::company::CompanyCorrectionError::Accounting { .. }
            )
        ),
        "{error}"
    );
    assert_eq!(system, before);
    assert_eq!(base, library_before);
}

#[test]
fn proof_error_keeps_its_typed_cause_in_wrapper_error_chain() {
    use crate::company::report_correction::{
        ConfirmedDisclosureCorrections, DisclosureCorrectionPrepareError,
        DisclosureCorrectionProofError,
    };
    use std::error::Error;
    let (base, private_library, ids) = owner_result();
    let result = ConfirmedDisclosureCorrections::from_owner_result(
        &base,
        private_library,
        ids[0],
        Some(&[]),
        CompanyId(0),
        PublicationSource::SimpleGenerated,
        vec![crate::accounting::BusinessEventId::new(1000)],
    )
    .map_err(DisclosureCorrectionPrepareError::from);
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("缺少已确认项应拒绝"),
    };
    assert!(matches!(
        error,
        DisclosureCorrectionPrepareError::Proof(DisclosureCorrectionProofError::IncompleteCoverage)
    ));
    assert!(error
        .source()
        .unwrap()
        .is::<DisclosureCorrectionProofError>());
}
