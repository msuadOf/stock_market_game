use super::*;
use crate::calendar::{CivilDate, CivilInstant};
use crate::company::disclosure_correction_test_fixture as fixture;
use crate::information::{MonthlyReportDelay, MonthlyReportSchedule, ReportFrequency};
use fixture::*;

fn owner_results() -> (
    PublicLibrary,
    PublicLibrary,
    Vec<DisclosureCorrectionOwnerResult>,
) {
    let mut system = simple_models(1, 2);
    let first =
        CivilInstant::from_hms(CivilDate::from_iso("2030-01-01").unwrap(), 18, 0, 0).unwrap();
    let now = CivilInstant::from_hms(first.date().next().unwrap(), 18, 0, 0).unwrap();
    let mut base = PublicLibrary::new();
    crate::information::publish_simple_scheduled(
        &system,
        ReportFrequency::Monthly {
            schedule: MonthlyReportSchedule::Custom {
                day: 1,
                second_of_day: 18 * 3600,
                delay: MonthlyReportDelay::None,
            },
        },
        17,
        None,
        first,
        &mut base,
        false,
    )
    .unwrap();
    let mut private = base.clone();
    let mut results = Vec::new();
    for company in [CompanyId(1), CompanyId(0)] {
        let supersedes = base
            .save()
            .reports
            .into_iter()
            .find(|r| r.company == company)
            .unwrap()
            .id;
        let publications = system
            .correct_report(
                &company,
                correction_operation(1000, now.date()),
                supersedes,
                now,
                &mut private,
            )
            .unwrap();
        assert_eq!(publications.len(), 1);
        results.push(DisclosureCorrectionOwnerResult {
            main_publication: publications[0],
            publications,
            company,
            source: PublicationSource::SimpleGenerated,
            operation_sources: vec![BusinessEventId::new(1000)],
        });
    }
    (base, private, results)
}

#[test]
fn batch_proof_partitions_complete_actual_owner_results_in_request_order() {
    let (base, private, results) = owner_results();
    let proof =
        ConfirmedDisclosureCorrections::from_owner_results(&base, private, results).unwrap();
    assert_eq!(proof.base(), &base);
    assert_eq!(proof.operations().len(), 2);
    for (operation, company) in proof.operations().iter().zip([CompanyId(1), CompanyId(0)]) {
        assert_eq!(operation.company(), company);
        assert_eq!(operation.source(), PublicationSource::SimpleGenerated);
        assert_eq!(operation.operation_sources(), &[BusinessEventId::new(1000)]);
        assert_eq!(operation.reports().len(), 1);
        assert_eq!(operation.reports()[0].0.company, company);
    }
}

#[test]
fn batch_proof_rejects_overlapping_or_missing_partition_with_exact_coverage_error() {
    let (base, private, results) = owner_results();
    let mut overlap = results.clone();
    overlap[1].publications = overlap[0].publications.clone();
    overlap[1].main_publication = overlap[0].main_publication;
    for invalid in [overlap, results[..1].to_vec()] {
        let error = match ConfirmedDisclosureCorrections::from_owner_results(
            &base,
            private.clone(),
            invalid,
        ) {
            Err(error) => error,
            Ok(_) => panic!("重叠或漏掉operation必须拒绝完整差集"),
        };
        assert!(
            matches!(error, DisclosureCorrectionProofError::IncompleteCoverage),
            "{error}"
        );
    }
}

#[test]
fn batch_proof_rejects_wrong_operation_company_without_losing_typed_cause() {
    let (base, private, mut results) = owner_results();
    results[0].company = CompanyId(0);
    let error = match ConfirmedDisclosureCorrections::from_owner_results(&base, private, results) {
        Err(error) => error,
        Ok(_) => panic!("operation公司必须与真实完整报告相等"),
    };
    assert!(
        matches!(
            error,
            DisclosureCorrectionProofError::WrongCompany {
                expected: CompanyId(0),
                actual: CompanyId(1)
            }
        ),
        "{error}"
    );
}
