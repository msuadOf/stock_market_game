use super::*;
use crate::accounting::closing::{ClosingEngine, CorrectionRequest};
use crate::accounting::consolidation::{MemberId, ScopeId};
use crate::accounting::reports::{IndustryPresentation, ReportKind};
use crate::accounting::{
    AccountingPeriod, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, JournalLine,
    LedgerAccountId, PostingSide,
};
use crate::calendar::{CivilDate, CivilInstant};
use crate::company::operations::{
    CompanyOperations, OperatingReportCorrection,
};
use crate::information::{
    AccountingPolicyRef, InformationError, PublicationOrigin, PublicationRequest, PublicLibrary,
    ScheduledReportKind,
};

fn annual_period() -> AccountingPeriod {
    AccountingPeriod::from_ymd(2029, 12).unwrap()
}

fn standalone_annual_publication() -> CivilInstant {
    crate::information::scheduled_instant(ScheduledReportKind::Annual, 2029, 0).unwrap()
}

fn group_annual_publication() -> CivilInstant {
    crate::information::scheduled_instant(ScheduledReportKind::Annual, 2029, 7).unwrap()
}

fn fixture() -> (
    CompanyOperations,
    GroupStructure,
    ClosingEngine,
    PublicLibrary,
    crate::information::PublicationId,
) {
    let mut shocks = crate::company::events::ShockParams::current_default_parameters();
    shocks.market_candidate_bp = 0;
    shocks.industry_candidate_bp = 0;
    shocks.company_candidate_bp = 0;
    let mut operations = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 7181,
            shock_params: shocks,
            companies: vec![company("root", None), company("sub", Some("root"))],
        },
        date("2030-05-01"),
    )
    .unwrap();
    let group = GroupStructure {
        root: crate::company::CompanyId("root".into()),
        holdings: vec![GroupHolding {
            company: crate::company::CompanyId("sub".into()),
            parent_held_shares: 80,
        }],
    };
    let mut closing = ClosingEngine::new();
    let mut library = PublicLibrary::new();
    for id in ["root", "sub"] {
        closing
            .close_year(
                operations
                    .company_mut(&crate::company::CompanyId(id.into()))
                    .unwrap()
                    .books_mut(),
                &MemberId(id.into()),
                IndustryPresentation::Industrial,
                2029,
            )
            .unwrap();
        let scope = ScopeId::Standalone(MemberId(id.into()));
        library
            .publish_closed(
                &closing,
                PublicationRequest {
                    company: crate::company::CompanyId(id.into()),
                    scope,
                    period: annual_period(),
                    kind: ReportKind::Annual,
                    sequence: 1,
                    policy: AccountingPolicyRef { chart_version: 2 },
                    approved_at: CivilInstant::from_hms(standalone_annual_publication().date(), 8, 0, 0)
                        .unwrap(),
                    published_at: standalone_annual_publication(),
                    origin: PublicationOrigin::SeededPrehistory {
                        fiscal_year: 2029,
                        kind: ScheduledReportKind::Annual,
                        offset_days: 0,
                    },
                    supersedes: None,
                },
            )
            .unwrap();
    }
    ensure_group_report(
        &group,
        &operations,
        &mut closing,
        annual_period(),
        ReportKind::Annual,
    )
    .unwrap();
    let original = library
        .publish_closed(
            &closing,
            PublicationRequest {
                company: group.root.clone(),
                scope: ScopeId::Consolidated(MemberId("root".into())),
                period: annual_period(),
                kind: ReportKind::Annual,
                sequence: 1,
                policy: AccountingPolicyRef { chart_version: 2 },
                approved_at: CivilInstant::from_hms(group_annual_publication().date(), 8, 0, 0)
                    .unwrap(),
                published_at: group_annual_publication(),
                origin: PublicationOrigin::SeededPrehistory {
                    fiscal_year: 2029,
                    kind: ScheduledReportKind::Annual,
                    offset_days: 7,
                },
                supersedes: None,
            },
        )
        .unwrap();
    (operations, group, closing, library, original)
}

fn correction(original: crate::information::PublicationId, posted_on: CivilDate) -> (CorrectionRequest, PublicationRequest) {
    (
        CorrectionRequest {
            reason: "补记已确认的前期管理费用".into(),
            entries: vec![JournalEntry {
                source: BusinessEventId::new(900_000_004),
                date: posted_on,
                kind: BusinessKind::CashExpense,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    JournalLine {
                        account: LedgerAccountId("6602".into()),
                        side: PostingSide::Debit,
                        amount: yuan(100),
                    },
                    JournalLine {
                        account: LedgerAccountId("1002".into()),
                        side: PostingSide::Credit,
                        amount: yuan(100),
                    },
                ],
            }],
        },
        PublicationRequest {
            company: crate::company::CompanyId("sub".into()),
            scope: ScopeId::Standalone(MemberId("sub".into())),
            period: annual_period(),
            kind: ReportKind::Annual,
            sequence: 2,
            policy: AccountingPolicyRef { chart_version: 2 },
            approved_at: instant("2030-03-21", 8),
            published_at: instant("2030-03-21", 18),
            origin: PublicationOrigin::Correction,
            supersedes: Some(original),
        },
    )
}

fn transact(
    operations: &mut CompanyOperations,
    group: &GroupStructure,
    closing: &mut ClosingEngine,
    library: &mut PublicLibrary,
    standalone_original: crate::information::PublicationId,
    group_published_at: CivilInstant,
) -> Result<(), (InformationError, CompanyOperations, ClosingEngine, PublicLibrary)> {
    let mut candidate_operations = operations.clone();
    let mut candidate_closing = closing.clone();
    let mut candidate_library = library.clone();
    let (correction, publication) = correction(standalone_original, date("2030-03-21"));
    if let Err(error) = candidate_operations
        .correct_company_report_for_session(
            &crate::company::CompanyId("sub".into()),
            OperatingReportCorrection {
                closing: &mut candidate_closing,
                library: &mut candidate_library,
                correction,
                publication,
                posted_on: date("2030-03-21"),
            },
        ) {
        return Err((InformationError::ReportNotPublishable(Box::new(
            crate::accounting::reports::ReportError::InternalWindowInconsistent {
                detail: error.to_string(),
            },
        )), candidate_operations, candidate_closing, candidate_library));
    }
    if let Err(error) = crate::session::company_groups::refresh_group_reports_after_correction(
        group,
        &candidate_operations,
        &mut candidate_closing,
        &mut candidate_library,
        annual_period(),
        "补记已确认的前期管理费用",
        instant("2030-03-21", 8),
        group_published_at,
    ) {
        return Err((error, candidate_operations, candidate_closing, candidate_library));
    }
    *operations = candidate_operations;
    *closing = candidate_closing;
    *library = candidate_library;
    Ok(())
}

#[test]
fn group_publication_before_original_discards_candidate_and_allows_same_source_retry() {
    let (mut operations, group, mut closing, mut library, group_original) = fixture();
    let standalone_original = library
        .save()
        .reports
        .into_iter()
        .find(|report| report.company.0 == "sub")
        .unwrap()
        .id;
    let original_reports = library.save().reports;
    let before_operations = operations.clone();
    let before_closing = closing.clone();
    let before_library = library.clone();

    let (error, candidate_operations, candidate_closing, candidate_library) = transact(
        &mut operations,
        &group,
        &mut closing,
        &mut library,
        standalone_original,
        instant("2030-03-21", 18),
    )
    .unwrap_err();
    assert!(error.to_string().contains("集团更正公布时点早于被更正报告"));
    assert_ne!(candidate_operations, before_operations);
    assert_ne!(candidate_closing, before_closing);
    assert_ne!(candidate_library, before_library);
    assert_eq!(
        candidate_operations
            .company(&crate::company::CompanyId("sub".into()))
            .unwrap()
            .books()
            .books()
            .ledger()
            .cash_total()
            .unwrap(),
        yuan(900)
    );
    assert_eq!(
        candidate_closing
            .versions(
                &ScopeId::Consolidated(MemberId("root".into())),
                annual_period(),
                ReportKind::Annual,
            )
            .last()
            .unwrap()
            .version
            .sequence,
        2
    );
    assert!(candidate_library.save().reports.iter().any(|report| {
        report.company.0 == "sub" && report.reports.version.sequence == 2
    }));
    assert_eq!(operations, before_operations);
    assert_eq!(closing, before_closing);
    assert_eq!(library, before_library);
    assert_eq!(
        library.report(group_original, group_annual_publication()).unwrap().published_at,
        group_annual_publication()
    );

    transact(
        &mut operations,
        &group,
        &mut closing,
        &mut library,
        standalone_original,
        instant("2030-03-28", 18),
    )
    .unwrap();
    for original in &original_reports {
        assert_eq!(
            library
                .report(original.id, instant("2030-03-28", 18))
                .unwrap(),
            original
        );
    }
    let corrected = library
        .save()
        .reports
        .into_iter()
        .find(|report| {
            report.reports.scope == ScopeId::Consolidated(MemberId("root".into()))
                && report.reports.version.sequence == 2
        })
        .unwrap();
    assert_eq!(corrected.supersedes, Some(group_original));
    assert_eq!(corrected.published_at, instant("2030-03-28", 18));
}

fn date(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

fn instant(value: &str, hour: u32) -> CivilInstant {
    CivilInstant::from_hms(date(value), hour, 0, 0).unwrap()
}

fn yuan(value: i128) -> crate::accounting::AccountingAmount {
    crate::accounting::AccountingAmount::from_cents(value * 100)
}
