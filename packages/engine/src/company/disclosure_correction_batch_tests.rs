use super::*;
#[path = "disclosure_correction_test_fixture.rs"]
mod fixture;
use crate::company::report_correction::{
    DisclosureCorrectionPrepareError, DisclosureCorrectionProofError,
};
use crate::information::{MonthlyReportDelay, MonthlyReportSchedule, ReportFrequency};
use fixture::*;

fn setup(periods: u16, count: u32) -> (super::super::CompanySystem, PublicLibrary, CivilInstant) {
    let system = simple_models(periods, count);
    let mut library = PublicLibrary::new();
    let first = CivilInstant::from_hms(
        crate::calendar::CivilDate::from_iso("2030-01-01").unwrap(),
        18,
        0,
        0,
    )
    .unwrap();
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
        &mut library,
        false,
    )
    .unwrap();
    let now = CivilInstant::from_hms(first.date().next().unwrap(), 18, 0, 0).unwrap();
    (system, library, now)
}

fn input(
    company: u32,
    target: PublicationId,
    source: u64,
    now: CivilInstant,
) -> SimpleDisclosureCorrectionInput {
    SimpleDisclosureCorrectionInput {
        company: CompanyId(company),
        correction: correction_operation(source, now.date()),
        supersedes: target,
    }
}

fn target(base: &PublicLibrary, company: u32, ordinal: usize) -> PublicationId {
    base.save()
        .reports
        .into_iter()
        .filter(|r| r.company == CompanyId(company))
        .nth(ordinal)
        .unwrap()
        .id
}

fn owner_sequence(
    system: &super::super::CompanySystem,
    base: &PublicLibrary,
    inputs: &[SimpleDisclosureCorrectionInput],
    now: CivilInstant,
) -> (
    super::super::CompanySystem,
    PublicLibrary,
    Vec<Vec<PublicationId>>,
) {
    let mut expected = system.clone();
    let mut library = base.clone();
    let publications = inputs
        .iter()
        .map(|item| {
            expected
                .correct_report(
                    &item.company,
                    CorrectionRequest {
                        entries: item.correction.entries.clone(),
                        reason: item.correction.reason.clone(),
                    },
                    item.supersedes,
                    now,
                    &mut library,
                )
                .unwrap()
        })
        .collect();
    (expected, library, publications)
}

#[test]
fn batch_two_companies_preserves_queue_order_company_local_sources_and_exact_finance() {
    let (system, base, now) = setup(1, 2);
    let inputs = vec![
        input(1, target(&base, 1, 0), 1000, now),
        input(0, target(&base, 0, 0), 1000, now),
    ];
    let (expected, _, outputs) = owner_sequence(&system, &base, &inputs, now);
    assert_eq!(outputs.iter().map(Vec::len).collect::<Vec<_>>(), [1, 1]);
    let before = system.clone();
    let base_before = base.clone();
    let prepared = system
        .prepare_disclosure_corrections(inputs, now, &base)
        .unwrap();
    assert_eq!(prepared.source(), &expected);
    assert_eq!(prepared.proof().base(), &base);
    let operations = prepared.proof().operations();
    assert_eq!(operations.len(), 2);
    for (operation, company) in operations.iter().zip([CompanyId(1), CompanyId(0)]) {
        assert_eq!(operation.company(), company);
        assert_eq!(operation.source(), PublicationSource::SimpleGenerated);
        assert_eq!(
            operation.operation_sources(),
            &[crate::accounting::BusinessEventId::new(1000)]
        );
        assert_eq!(operation.reports().len(), 1);
        assert_eq!(operation.reports()[0].0.company, company);
        assert_eq!(operation.reports()[0].2, operation.source());
    }
    assert_eq!(system, before);
    assert_eq!(base, base_before);
    assert_eq!(prepared.into_source(), expected);
}

#[test]
fn batch_same_company_different_periods_keeps_real_owner_legal_input() {
    let (mut system, mut base, first) = setup(2, 1);
    // 先以真实历史更正建立已重述 base；未重述 base 的 ROE 变化会触发后期 cascade。
    let original = target(&base, 0, 0);
    let targets = system
        .correct_report(
            &CompanyId(0),
            correction_operation(5000, first.date()),
            original,
            first,
            &mut base,
        )
        .unwrap();
    assert_eq!(targets.len(), 2);
    let now = CivilInstant::from_hms(first.date().next().unwrap(), 18, 0, 0).unwrap();
    let mut inputs = vec![
        input(0, targets[0], 1000, now),
        input(0, targets[1], 2000, now),
    ];
    // 抵消分录不改变后期财务；合法性仍由真实 owner 正控确认，不假定不同期间必然独立。
    for item in &mut inputs {
        let mut reversing = item.correction.entries[0].lines.clone();
        for line in &mut reversing {
            line.side = match line.side {
                crate::accounting::PostingSide::Debit => crate::accounting::PostingSide::Credit,
                crate::accounting::PostingSide::Credit => crate::accounting::PostingSide::Debit,
            };
        }
        item.correction.entries[0].lines.extend(reversing);
    }
    let (expected, _, outputs) = owner_sequence(&system, &base, &inputs, now);
    assert_eq!(outputs.iter().map(Vec::len).collect::<Vec<_>>(), [1, 1]);
    let prepared = system
        .prepare_disclosure_corrections(inputs, now, &base)
        .unwrap();
    assert_eq!(prepared.source(), &expected);
    assert_eq!(prepared.proof().operations().len(), 2);
    assert_ne!(
        prepared.proof().operations()[0].reports()[0].0.period,
        prepared.proof().operations()[1].reports()[0].0.period
    );
}

#[test]
fn batch_duplicate_target_keeps_latest_owner_rejection_and_original_inputs() {
    let (system, base, now) = setup(1, 1);
    let before = system.clone();
    let base_before = base.clone();
    let inputs = vec![
        input(0, target(&base, 0, 0), 1000, now),
        input(0, target(&base, 0, 0), 2000, now),
    ];
    let error = match system.prepare_disclosure_corrections(inputs, now, &base) {
        Err(error) => error,
        Ok(_) => panic!("重复旧目标必须整批拒绝"),
    };
    assert!(
        matches!(error, DisclosureCorrectionPrepareError::Owner(super::super::CompanyCorrectionError::InvalidInput { ref detail, .. }) if detail == "只能更正该公司该期间该类型的最新公开版本"),
        "{error}"
    );
    assert_eq!(system, before);
    assert_eq!(base, base_before);
}

#[test]
fn batch_explicit_temporary_predecessor_is_rejected_without_source_installation() {
    let (system, base, now) = setup(1, 1);
    let before = system.clone();
    let base_before = base.clone();
    let temporary = PublicationId::new(base.version());
    let inputs = vec![
        input(0, target(&base, 0, 0), 1000, now),
        input(0, temporary, 2000, now),
    ];
    let (_, _, outputs) = owner_sequence(&system, &base, &inputs, now);
    assert_eq!(outputs[0][0], temporary);
    let error = match system.prepare_disclosure_corrections(inputs, now, &base) {
        Err(error) => error,
        Ok(_) => panic!("同批临时 predecessor 不得进入 proof"),
    };
    assert!(
        matches!(error, DisclosureCorrectionPrepareError::Proof(DisclosureCorrectionProofError::TemporaryPredecessor { target }) if target == temporary),
        "{error}"
    );
    assert_eq!(system, before);
    assert_eq!(base, base_before);
}

#[test]
fn batch_reverse_period_cascade_rejects_implicit_temporary_predecessor() {
    let (system, base, now) = setup(2, 1);
    let inputs = vec![
        input(0, target(&base, 0, 1), 1000, now),
        input(0, target(&base, 0, 0), 2000, now),
    ];
    let (_, private, outputs) = owner_sequence(&system, &base, &inputs, now);
    assert_eq!(outputs[0].len(), 1);
    assert_eq!(outputs[1].len(), 2);
    let temporary = outputs[0][0];
    assert!(private
        .save()
        .reports
        .iter()
        .any(|r| r.supersedes == Some(temporary)));
    let before = system.clone();
    let base_before = base.clone();
    let error = match system.prepare_disclosure_corrections(inputs, now, &base) {
        Err(error) => error,
        Ok(_) => panic!("自动 cascade 指向同批私有报告也必须拒绝"),
    };
    assert!(
        matches!(error, DisclosureCorrectionPrepareError::Proof(DisclosureCorrectionProofError::TemporaryPredecessor { target }) if target == temporary),
        "{error}"
    );
    assert_eq!(system, before);
    assert_eq!(base, base_before);
}

#[test]
fn batch_second_company_domain_failure_does_not_keep_first_company_finance() {
    let (system, base, now) = setup(1, 2);
    let before = system.clone();
    let base_before = base.clone();
    let mut bad = input(1, target(&base, 1, 0), 2000, now);
    bad.correction.entries.clear();
    let inputs = vec![input(0, target(&base, 0, 0), 1000, now), bad];
    let error = match system.prepare_disclosure_corrections(inputs, now, &base) {
        Err(error) => error,
        Ok(_) => panic!("后公司无凭证必须整批拒绝"),
    };
    assert!(
        matches!(error, DisclosureCorrectionPrepareError::Owner(super::super::CompanyCorrectionError::InvalidInput { ref detail, .. }) if detail == "更正理由和调整凭证均不能为空"),
        "{error}"
    );
    assert_eq!(system, before);
    assert_eq!(base, base_before);
}
