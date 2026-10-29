use super::*;
use crate::company::disclosure_correction_test_fixture as fixture;
use crate::company::SimpleDisclosureCorrectionInput;
use crate::information::{MonthlyReportDelay, MonthlyReportSchedule};
use fixture::*;

fn frequency() -> ReportFrequency {
    ReportFrequency::Monthly {
        schedule: MonthlyReportSchedule::Custom {
            day: 1,
            second_of_day: 18 * 3600,
            delay: MonthlyReportDelay::None,
        },
    }
}

fn setup() -> (
    CompanySystem,
    PublicLibrary,
    CivilInstant,
    Vec<SimpleDisclosureCorrectionInput>,
) {
    let system = simple_models(2, 3);
    let first =
        CivilInstant::from_hms(CivilDate::from_iso("2030-01-01").unwrap(), 18, 0, 0).unwrap();
    let now = CivilInstant::from_hms(first.date().next().unwrap(), 18, 0, 0).unwrap();
    let mut template = PublicLibrary::new();
    crate::information::publish_simple_scheduled(
        &system,
        frequency(),
        17,
        None,
        first,
        &mut template,
        false,
    )
    .unwrap();
    let mut base = PublicLibrary::new();
    // 真正只公布公司1/2；公司0的 Original 留待同一 canonical 扫描，不删库造洞。
    for report in template
        .save()
        .reports
        .into_iter()
        .filter(|r| r.company != CompanyId(0))
    {
        base.publish_simple_closed(
            system.closing_for(&report.company).unwrap(),
            PublicationRequest {
                company: report.company,
                scope: report.reports.scope.clone(),
                period: report.reports.period,
                kind: report.reports.kind,
                sequence: report.reports.version.sequence,
                policy: report.policy,
                approved_at: report.approved_at,
                published_at: report.published_at,
                origin: report.origin,
                supersedes: report.supersedes,
            },
        )
        .unwrap();
    }
    let inputs = [CompanyId(2), CompanyId(1)]
        .into_iter()
        .map(|company| {
            let supersedes = base
                .save()
                .reports
                .into_iter()
                .find(|r| r.company == company)
                .unwrap()
                .id;
            SimpleDisclosureCorrectionInput {
                company,
                correction: correction_operation(1000, now.date()),
                supersedes,
            }
        })
        .collect();
    (system, base, now, inputs)
}

#[test]
fn batch_cascade_with_other_originals_maps_complete_payload_to_canonical_ids() {
    let (system, base, now, inputs) = setup();
    let before = system.clone();
    let base_before = base.clone();
    let mut private_system = system.clone();
    let mut private_library = base.clone();
    let private_ids = inputs
        .iter()
        .map(|input| {
            private_system
                .correct_report(
                    &input.company,
                    crate::accounting::closing::CorrectionRequest {
                        entries: input.correction.entries.clone(),
                        reason: input.correction.reason.clone(),
                    },
                    input.supersedes,
                    now,
                    &mut private_library,
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(private_ids.iter().map(Vec::len).collect::<Vec<_>>(), [2, 2]);
    let prepared = system
        .prepare_disclosure_corrections(inputs, now, &base)
        .unwrap();
    assert_eq!(prepared.source(), &private_system);
    let dispatch = DisclosureDispatch::new(None);
    let candidate = prepare_disclosures(DisclosurePrepareCtx {
        source: DisclosureSourceRef::PreparedSimple(&prepared),
        previous_dispatch: &dispatch,
        previous_library: &base,
        seed: 17,
        report_frequency: frequency(),
        through: now,
        prehistory_opening: None,
        settled_announcements: None,
        corporate_actions: None,
    })
    .unwrap();
    assert_eq!(
        candidate.publications.len(),
        6,
        "两个真实 Original 和四个真实 cascade 报告必须完整"
    );
    let operations = prepared.proof().operations();
    assert_eq!(operations.len(), 2);
    let mut formal_ids = std::collections::BTreeSet::new();
    let mut ids_changed = 0;
    for (index, operation) in operations.iter().enumerate() {
        assert_eq!(operation.company(), [CompanyId(2), CompanyId(1)][index]);
        assert_eq!(operation.reports().len(), 2);
        for (row, (request, set, source)) in operation.reports().iter().enumerate() {
            let matches = candidate
                .publications
                .iter()
                .filter(|publication| {
                    publication.company == request.company
                        && publication.published_at == request.published_at
                        && publication.kind
                            == DisclosurePublicationKind::Report {
                                scope: request.scope.clone(),
                                period: request.period,
                                kind: request.kind,
                                version: set.version.clone(),
                                supersedes: request.supersedes,
                            }
                })
                .collect::<Vec<_>>();
            assert_eq!(
                matches.len(),
                1,
                "完整正式身份必须恰好一项，不能 zip 截断或靠临时ID偏移"
            );
            let id = matches[0].id;
            assert!(formal_ids.insert(id));
            let published = candidate.library.report(id, now).unwrap();
            assert_eq!(&published.reports, set);
            assert_eq!(published.policy, request.policy);
            assert_eq!(published.source, *source);
            assert_eq!(published.source, operation.source());
            assert_eq!(
                prepared
                    .source()
                    .closing_for(&request.company)
                    .unwrap()
                    .version(
                        &request.scope,
                        request.period,
                        request.kind,
                        request.sequence,
                    )
                    .unwrap(),
                set
            );
            ids_changed += usize::from(id != private_ids[index][row]);
        }
    }
    assert_eq!(formal_ids.len(), 4);
    assert!(ids_changed >= 2, "真实混排必须使临时和canonical ID不同");
    assert_eq!(system, before);
    assert_eq!(base, base_before);
    assert_eq!(prepared.into_source(), private_system);
}

#[test]
fn batch_future_window_rejects_the_complete_package_without_installing_finance() {
    let (system, base, now, inputs) = setup();
    let before = system.clone();
    let base_before = base.clone();
    let prepared = system
        .prepare_disclosure_corrections(inputs, now, &base)
        .unwrap();
    assert_ne!(prepared.source(), &system);
    let dispatch = DisclosureDispatch::new(None);
    let error = match prepare_disclosures(DisclosurePrepareCtx {
        source: DisclosureSourceRef::PreparedSimple(&prepared),
        previous_dispatch: &dispatch,
        previous_library: &base,
        seed: 17,
        report_frequency: frequency(),
        through: CivilInstant::from_hms(now.date(), 17, 59, 59).unwrap(),
        prehistory_opening: None,
        settled_announcements: None,
        corporate_actions: None,
    }) {
        Err(error) => error,
        Ok(_) => panic!("未来批更正不能安装金融source或公开半包"),
    };
    assert!(
        matches!(error, DisclosurePrepareError::CorrectionOutsideWindow { published_at, .. } if published_at == now),
        "{error}"
    );
    assert_eq!(system, before);
    assert_eq!(base, base_before);
    assert_eq!(dispatch.published_through(), None);
}
