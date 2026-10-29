use super::*;
use crate::company::disclosure_correction_test_fixture as fixture;
use fixture::*;

#[test]
fn actual_owner_complete_result_produces_proof_without_exposing_temporary_ids() {
    let (base, private_library, ids) = owner_result();
    let base_before = base.clone();
    let proof = ConfirmedDisclosureCorrections::from_owner_result(
        &base,
        private_library,
        ids[0],
        Some(&ids),
        CompanyId(0),
        PublicationSource::SimpleGenerated,
        vec![BusinessEventId::new(1000)],
    )
    .unwrap();
    assert_eq!(proof.base(), &base_before);
    assert_eq!(proof.operations()[0].reports().len(), ids.len());
    assert_eq!(proof.operations()[0].reports()[0].1.version.sequence, 2);
    assert_eq!(
        proof.operations()[0].reports()[0].0.supersedes,
        Some(PublicationId::new(0))
    );
    assert_eq!(
        proof.operations()[0].operation_sources(),
        &[BusinessEventId::new(1000)]
    );
    assert_eq!(base, base_before);
}

fn proof_error(
    result: Result<ConfirmedDisclosureCorrections<'_>, DisclosureCorrectionProofError>,
) -> DisclosureCorrectionProofError {
    match result {
        Err(error) => error,
        Ok(_) => panic!("无效授权应明确拒绝"),
    }
}

#[test]
fn missing_complete_owner_publication_is_rejected() {
    let (base, private_library, ids) = owner_result();
    let error = proof_error(ConfirmedDisclosureCorrections::from_owner_result(
        &base,
        private_library,
        ids[0],
        Some(&[]),
        CompanyId(0),
        PublicationSource::SimpleGenerated,
        vec![BusinessEventId::new(1000)],
    ));
    assert!(
        matches!(error, DisclosureCorrectionProofError::IncompleteCoverage),
        "{error}"
    );
}

#[test]
fn wrong_base_or_source_or_main_publication_is_rejected() {
    let (base, private_library, ids) = owner_result();
    let mut edited = base.save();
    edited.reports[0].policy.chart_version = 2;
    let wrong_base = PublicLibrary::from_parts(edited).unwrap();
    let error = proof_error(ConfirmedDisclosureCorrections::from_owner_result(
        &wrong_base,
        private_library.clone(),
        ids[0],
        Some(&ids),
        CompanyId(0),
        PublicationSource::SimpleGenerated,
        vec![BusinessEventId::new(1000)],
    ));
    assert!(
        matches!(error, DisclosureCorrectionProofError::BaseChanged),
        "{error}"
    );
    let error = proof_error(ConfirmedDisclosureCorrections::from_owner_result(
        &base,
        private_library.clone(),
        ids[0],
        Some(&ids),
        CompanyId(0),
        PublicationSource::SimulationAccounting,
        vec![BusinessEventId::new(1000)],
    ));
    assert!(
        matches!(
            error,
            DisclosureCorrectionProofError::WrongSource {
                expected: PublicationSource::SimulationAccounting,
                actual: PublicationSource::SimpleGenerated
            }
        ),
        "{error}"
    );
    let error = proof_error(ConfirmedDisclosureCorrections::from_owner_result(
        &base,
        private_library,
        PublicationId::new(99),
        Some(&ids),
        CompanyId(0),
        PublicationSource::SimpleGenerated,
        vec![BusinessEventId::new(1000)],
    ));
    assert!(
        matches!(error, DisclosureCorrectionProofError::InvalidMainPublication { publication } if publication == PublicationId::new(99)),
        "{error}"
    );
}

#[test]
fn actual_legacy_owner_allows_empty_operation_entries_without_new_source_identity() {
    use crate::accounting::closing::CorrectionRequest;
    use crate::accounting::consolidation::{MemberId, ScopeId};
    use crate::accounting::reports::ReportKind;
    use crate::accounting::AccountingPeriod;
    use crate::calendar::{CivilDate, CivilInstant};
    use crate::company::operations::OperatingReportCorrection;
    use crate::information::{
        AccountingPolicyRef, MonthlyReportDelay, MonthlyReportSchedule, PublicationOrigin,
    };
    let (mut operations, mut closing) = legacy_fixture();
    let period = AccountingPeriod::from_ymd(2029, 12).unwrap();
    operations
        .company_mut(&CompanyId(0))
        .unwrap()
        .books_mut()
        .close_period(period)
        .unwrap();
    let date = CivilDate::from_iso("2030-01-01").unwrap();
    let initial = PublicationRequest {
        company: CompanyId(0),
        scope: ScopeId::Standalone(MemberId(0)),
        period,
        kind: ReportKind::Monthly,
        sequence: 1,
        policy: AccountingPolicyRef { chart_version: 2 },
        approved_at: CivilInstant::from_hms(date, 8, 0, 0).unwrap(),
        published_at: CivilInstant::from_hms(date, 18, 0, 0).unwrap(),
        origin: PublicationOrigin::MonthlyDisclosure {
            schedule: MonthlyReportSchedule::Custom {
                day: 1,
                second_of_day: 18 * 3600,
                delay: MonthlyReportDelay::None,
            },
            delay_days: 0,
            seeded: false,
        },
        supersedes: None,
    };
    let mut base = PublicLibrary::new();
    let original = base.publish_closed(&closing, initial.clone()).unwrap();
    let date = date.next().unwrap();
    let mut publication = initial;
    publication.sequence = 2;
    publication.origin = PublicationOrigin::Correction;
    publication.supersedes = Some(original);
    publication.approved_at = CivilInstant::from_hms(date, 8, 0, 0).unwrap();
    publication.published_at = CivilInstant::from_hms(date, 18, 0, 0).unwrap();
    let mut private_library = base.clone();
    let (_, id) = operations
        .correct_industrial_report(
            &CompanyId(0),
            OperatingReportCorrection {
                closing: &mut closing,
                library: &mut private_library,
                correction: CorrectionRequest {
                    reason: "重新确认既有重述材料".into(),
                    entries: vec![],
                },
                publication,
                posted_on: date,
            },
        )
        .unwrap();
    let proof = ConfirmedDisclosureCorrections::from_owner_result(
        &base,
        private_library,
        id,
        None,
        CompanyId(0),
        PublicationSource::SimulationAccounting,
        vec![],
    )
    .unwrap();
    assert_eq!(proof.operations()[0].operation_sources(), &[]);
    assert_eq!(proof.operations()[0].reports().len(), 1);
    assert_eq!(proof.operations()[0].reports()[0].1.version.sequence, 2);
}
