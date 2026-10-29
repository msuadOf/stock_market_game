use super::*;
use crate::accounting::closing::CorrectionRequest;
use crate::accounting::consolidation::{MemberId, ScopeId};
use crate::accounting::reports::ReportKind;
use crate::accounting::{
    AccountingAmount, AccountingPeriod, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry,
    JournalLine, LedgerAccountId, PostingSide,
};
use crate::calendar::CivilInstant;
use crate::company::disclosure_correction_test_fixture as fixture;
use crate::information::{
    AccountingPolicyRef, MonthlyReportDelay, MonthlyReportSchedule, PublicLibrary,
    PublicationOrigin, PublicationRequest,
};
use fixture::legacy_fixture;

#[test]
fn legacy_wrapper_preserves_real_finance_and_can_return_same_owned_candidate() {
    let (operations, closing) = legacy_fixture();
    let period = AccountingPeriod::from_ymd(2029, 12).unwrap();
    let mut operations = operations;
    operations
        .company_mut(&CompanyId(0))
        .unwrap()
        .books_mut()
        .close_period(period)
        .unwrap();
    let date = CivilDate::from_iso("2030-01-01").unwrap();
    let mut publication = PublicationRequest {
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
    let original = base.publish_closed(&closing, publication.clone()).unwrap();
    let date = date.next().unwrap();
    publication.sequence = 2;
    publication.origin = PublicationOrigin::Correction;
    publication.supersedes = Some(original);
    publication.approved_at = CivilInstant::from_hms(date, 8, 0, 0).unwrap();
    publication.published_at = CivilInstant::from_hms(date, 18, 0, 0).unwrap();
    let operation = CorrectionRequest {
        reason: "补正历史资本分录".into(),
        entries: vec![JournalEntry {
            source: BusinessEventId::new(1000),
            date,
            kind: BusinessKind::OpeningBalance,
            cash_flow: CashFlowClass::Financing,
            lines: vec![
                JournalLine {
                    account: LedgerAccountId("1002".into()),
                    side: PostingSide::Debit,
                    amount: AccountingAmount::from_cents(100),
                },
                JournalLine {
                    account: LedgerAccountId("4001".into()),
                    side: PostingSide::Credit,
                    amount: AccountingAmount::from_cents(100),
                },
            ],
        }],
    };
    let before = operations.clone();
    let closing_before = closing.clone();
    let base_before = base.clone();
    let prepared = operations
        .prepare_disclosure_correction(
            &CompanyId(0),
            LegacyDisclosureCorrectionCtx {
                closing: &closing,
                base: &base,
                correction: operation,
                publication,
                posted_on: date,
            },
        )
        .unwrap();
    assert_eq!(operations, before);
    assert_eq!(closing, closing_before);
    assert_eq!(base, base_before);
    assert_ne!(
        prepared
            .source()
            .0
            .company(&CompanyId(0))
            .unwrap()
            .books()
            .books(),
        operations.company(&CompanyId(0)).unwrap().books().books()
    );
    assert_eq!(prepared.proof().operations()[0].reports().len(), 1);
    let expected = prepared.proof().operations()[0].reports()[0].1.clone();
    let (final_operations, final_closing) = prepared.into_source();
    assert_ne!(final_operations, before);
    assert_eq!(
        final_closing.version(&expected.scope, period, ReportKind::Monthly, 2),
        Some(&expected)
    );
}
