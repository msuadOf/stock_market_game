use super::*;
use engine::calendar::{CivilDate, CivilInstant};
use engine::company::ShockKind;
use engine::company::events::PaymentObligationStatus;
use engine::information::{AnnouncementContent, AnnouncementRequest};
use engine::strategy::{ForecastBasis, ValuationOutcome, ValuationUnavailable};

#[test]
fn acquired_payment_failure_without_own_annual_records_unavailable_belief() {
    let mut sc = scenario();
    let npc = AccountId(3);
    let published_at =
        CivilInstant::from_hms(CivilDate::from_iso("2031-04-01").expect("date"), 18, 0, 0)
            .expect("publication time");
    let announcement = sc
        .library
        .publish_announcement(AnnouncementRequest {
            company: sc.company.clone(),
            occurred_on: published_at.date(),
            published_at,
            content: AnnouncementContent::Shock(engine::information::AnnouncedEvent {
                kind: ShockKind::PaymentFailure {
                    obligation_status: PaymentObligationStatus::ContractualOverdue,
                    what: "overdue contractual payment".to_owned(),
                    amount: engine::accounting::AccountingAmount::from_cents(100),
                },
                amplitude_bp: 0,
                starts_on: published_at.date(),
                expires_on: published_at.date(),
            }),
        })
        .expect("publish genuine contractual payment failure");
    let mut case = FundamentalBeliefCase::new(
        sc,
        npc,
        market(30_000),
        StrategyProfile::Retail(RetailStyle::Momentum),
        AnalysisProfile::new(
            AnalysisWeights::new(3_000, 2_000, 2_000, 1_500, 1_500).expect("weights"),
            Some(FundamentalMethod::CashFlow),
        )
        .expect("slot"),
        BeliefIssuerInputs {
            kind: CompanyKind::Industrial,
            total_issued_shares: ISSUED_SHARES,
        },
        &mut assumptions_rng(0.0),
    );
    case.acquire(announcement, hour_after(published_at))
        .expect("participant acquires the public announcement");

    case.apply_cause(
        &stock_code(),
        BeliefCause::CreditDefault { announcement },
        1_100,
    )
    .expect("missing own annual must be recorded as unavailable, not block the day");

    let entry = case.book.entry(&stock_code()).expect("belief entry");
    assert_eq!(entry.forecast.growth_bp, None);
    assert_eq!(
        entry.forecast.basis,
        ForecastBasis::AnnualBaselineUnavailable
    );
    assert_eq!(
        entry.valuation,
        ValuationOutcome::Unavailable {
            reason: ValuationUnavailable::AnnualBaselineNotOwnKnown,
        }
    );
    assert_eq!(entry.used_report_ids, Vec::new());
    assert_eq!(
        entry.last_cause.as_ref().unwrap().cause,
        BeliefCause::CreditDefault { announcement }
    );
    assert_eq!(entry.anchor_trading_day, 1_100);
}
