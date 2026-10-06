use super::*;
use crate::gold::{momentum_analysis, momentum_profile, stock_code};
use engine::strategy::{ForecastBasis, FundamentalMethod};

fn scenario_with_no_prior_annual_income(mut scenario: Scenario) -> Scenario {
    let target_id = scenario.annual_ids[3];
    let mut save = scenario.library.save();
    let target = save
        .reports
        .iter_mut()
        .find(|report| report.id == target_id)
        .expect("annual fixture report exists");
    target.reports.income.prior_year = engine::accounting::reports::Comparative::Unavailable {
        reason: engine::accounting::reports::UnavailableReason::NoPriorYearHistory,
    };
    scenario.library = engine::information::PublicLibrary::from_parts(save)
        .expect("annual fixture without prior income history remains valid");
    scenario
}

#[test]
fn own_known_interim_supplements_growth_without_replacing_annual_earnings() {
    for method in [
        FundamentalMethod::EarningsMultiple,
        FundamentalMethod::CashFlow,
    ] {
        let mut scenario = scenario();
        let mut books = Books::new(industrial_account_chart());
        books
            .post_batch(vec![
                entry(
                    1,
                    "2029-12-31",
                    BusinessKind::OpeningBalance,
                    CashFlowClass::Financing,
                    &[
                        ("1002", PostingSide::Debit, 100_000),
                        ("4001", PostingSide::Credit, 100_000),
                    ],
                ),
                entry(
                    2,
                    "2030-06-15",
                    BusinessKind::CashRevenue,
                    CashFlowClass::Operating,
                    &[
                        ("1002", PostingSide::Debit, 100_000),
                        ("6001", PostingSide::Credit, 100_000),
                    ],
                ),
                entry(
                    3,
                    "2031-06-15",
                    BusinessKind::CashRevenue,
                    CashFlowClass::Operating,
                    &[
                        ("1002", PostingSide::Debit, 120_000),
                        ("6001", PostingSide::Credit, 120_000),
                    ],
                ),
            ])
            .unwrap();
        let member = MemberId(COMPANY.to_owned());
        let mut closing = ClosingEngine::new();
        closing
            .snapshot_interim(
                &books,
                &member,
                IndustryPresentation::Industrial,
                AccountingPeriod::from_ymd(2031, 6).unwrap(),
                ReportKind::HalfYear,
            )
            .unwrap();
        let offset = stable_company_offset(OPS_SEED, &scenario.company);
        let published_at = scheduled_instant(ScheduledReportKind::HalfYear, 2031, offset).unwrap();
        let interim = scenario
            .library
            .publish_closed(
                &closing,
                PublicationRequest {
                    company: scenario.company.clone(),
                    scope: ScopeId::Standalone(member),
                    period: AccountingPeriod::from_ymd(2031, 6).unwrap(),
                    kind: ReportKind::HalfYear,
                    sequence: 1,
                    policy: AccountingPolicyRef { chart_version: 2 },
                    approved_at: CivilInstant::from_hms(published_at.date(), 8, 0, 0).unwrap(),
                    published_at,
                    origin: PublicationOrigin::ScheduledDisclosure {
                        fiscal_year: 2031,
                        kind: ScheduledReportKind::HalfYear,
                        offset_days: offset,
                    },
                    supersedes: None,
                },
            )
            .unwrap();
        let annual = scenario.annual_ids[3];
        let annual_at = scenario.annual_instants[3];
        let mut case = FundamentalBeliefCase::new(
            scenario,
            AccountId(2),
            market(ISSUED_SHARES),
            momentum_profile(),
            momentum_analysis(method),
            BeliefIssuerInputs {
                kind: CompanyKind::Industrial,
                total_issued_shares: ISSUED_SHARES,
            },
            &mut assumptions_rng(0.5),
        );
        let code = stock_code();
        case.acquire(annual, hour_after(annual_at)).unwrap();
        case.apply_cause(&code, BeliefCause::NewMaterial { report: annual }, 0)
            .unwrap();
        let before = case.book.entry(&code).unwrap().clone();
        assert!(
            case.apply_cause(&code, BeliefCause::NewMaterial { report: interim }, 1)
                .is_err()
        );
        assert_eq!(case.book.entry(&code).unwrap(), &before);
        case.acquire(interim, hour_after(published_at)).unwrap();
        case.apply_cause(&code, BeliefCause::NewMaterial { report: interim }, 1)
            .unwrap();
        let after = case.book.entry(&code).unwrap();
        assert_eq!(after.forecast.growth_bp, Some(1250));
        assert_eq!(
            after.forecast.basis,
            ForecastBasis::Revised { observed_bp: 2000 }
        );
        assert_eq!(after.used_report_ids, vec![annual, interim]);
        assert_eq!(after.confidence_bp, before.confidence_bp);
        match method {
            FundamentalMethod::EarningsMultiple => assert_eq!(after.valuation, before.valuation),
            FundamentalMethod::CashFlow => assert_ne!(after.valuation, before.valuation),
            _ => unreachable!(),
        }
        let mut no_annual_case = FundamentalBeliefCase::new(
            case.scenario,
            AccountId(3),
            market(ISSUED_SHARES),
            momentum_profile(),
            momentum_analysis(method),
            BeliefIssuerInputs {
                kind: CompanyKind::Industrial,
                total_issued_shares: ISSUED_SHARES,
            },
            &mut assumptions_rng(0.5),
        );
        no_annual_case
            .acquire(interim, hour_after(published_at))
            .unwrap();
        let result =
            no_annual_case.apply_cause(&code, BeliefCause::NewMaterial { report: interim }, 2);
        assert!(
            result.is_ok(),
            "legal own-known interim without Annual must not fail: {result:?}"
        );
        let unavailable = no_annual_case.book.entry(&code).unwrap();
        assert_eq!(unavailable.method, Some(method));
        assert_eq!(unavailable.used_report_ids, vec![interim]);
        assert_eq!(unavailable.forecast.growth_bp, None);
        assert_eq!(
            unavailable.forecast.basis,
            ForecastBasis::AnnualBaselineUnavailable
        );
        assert_eq!(unavailable.confidence_bp, 3_000);
        assert!(matches!(
            unavailable.valuation,
            engine::strategy::ValuationOutcome::Unavailable {
                reason: engine::strategy::ValuationUnavailable::AnnualBaselineNotOwnKnown
            }
        ));
        assert_eq!(
            unavailable.last_cause.as_ref().unwrap().cause,
            BeliefCause::NewMaterial { report: interim }
        );
        let expiry_day =
            unavailable.anchor_trading_day + u64::from(unavailable.horizon_trading_days);
        let expiry = no_annual_case.apply_cause(&code, BeliefCause::HorizonExpired, expiry_day);
        assert!(
            expiry.is_ok(),
            "unavailable belief horizon expiry must remain non-fatal: {expiry:?}"
        );
        let still_unavailable = no_annual_case.book.entry(&code).unwrap();
        assert_eq!(still_unavailable.used_report_ids, vec![interim]);
        assert!(matches!(
            still_unavailable.valuation,
            engine::strategy::ValuationOutcome::Unavailable {
                reason: engine::strategy::ValuationUnavailable::AnnualBaselineNotOwnKnown
            }
        ));
        assert_eq!(
            still_unavailable.last_cause.as_ref().unwrap().cause,
            BeliefCause::HorizonExpired
        );

        for (test_index, (annual_index, expected_basis, expected_confidence)) in [
            (None, ForecastBasis::InitialWithoutHistory, 3_000),
            (
                Some(3),
                ForecastBasis::InitialTwoYear { observed_bp: 1_000 },
                6_000,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let recovery_scenario = if annual_index.is_none() {
                scenario_with_no_prior_annual_income(no_annual_case.scenario.clone())
            } else {
                no_annual_case.scenario.clone()
            };
            let mut recovery = FundamentalBeliefCase::new(
                recovery_scenario,
                AccountId(4 + test_index as u64),
                market(ISSUED_SHARES),
                momentum_profile(),
                momentum_analysis(method),
                BeliefIssuerInputs {
                    kind: CompanyKind::Industrial,
                    total_issued_shares: ISSUED_SHARES,
                },
                &mut assumptions_rng(0.5),
            );
            let annual_id = recovery.scenario.annual_ids[annual_index.unwrap_or(3)];
            recovery.acquire(interim, hour_after(published_at)).unwrap();
            recovery
                .apply_cause(&code, BeliefCause::NewMaterial { report: interim }, 3)
                .unwrap();
            recovery
                .acquire(annual_id, hour_after(published_at))
                .unwrap();
            recovery
                .apply_cause(&code, BeliefCause::NewMaterial { report: annual_id }, 4)
                .unwrap();
            let recovered = recovery.book.entry(&code).unwrap();
            assert_eq!(recovered.forecast.basis, expected_basis);
            assert_eq!(recovered.confidence_bp, expected_confidence);
            assert_eq!(recovered.used_report_ids, vec![annual_id]);
            if method == FundamentalMethod::CashFlow {
                assert!(matches!(
                    recovered.valuation,
                    engine::strategy::ValuationOutcome::Available { .. }
                ));
            }

            let mut horizon_recovery = FundamentalBeliefCase::new(
                if annual_index.is_none() {
                    scenario_with_no_prior_annual_income(no_annual_case.scenario.clone())
                } else {
                    no_annual_case.scenario.clone()
                },
                AccountId(8 + test_index as u64),
                market(ISSUED_SHARES),
                momentum_profile(),
                momentum_analysis(method),
                BeliefIssuerInputs {
                    kind: CompanyKind::Industrial,
                    total_issued_shares: ISSUED_SHARES,
                },
                &mut assumptions_rng(0.5),
            );
            horizon_recovery
                .acquire(interim, hour_after(published_at))
                .unwrap();
            horizon_recovery
                .apply_cause(&code, BeliefCause::NewMaterial { report: interim }, 5)
                .unwrap();
            let before_horizon = horizon_recovery.book.entry(&code).unwrap().clone();
            let expiry_day =
                before_horizon.anchor_trading_day + u64::from(before_horizon.horizon_trading_days);
            horizon_recovery
                .acquire(annual_id, hour_after(published_at))
                .unwrap();
            horizon_recovery
                .apply_cause(&code, BeliefCause::HorizonExpired, expiry_day)
                .unwrap();
            let recovered = horizon_recovery.book.entry(&code).unwrap();
            assert_eq!(recovered.forecast.basis, expected_basis);
            assert_eq!(recovered.confidence_bp, 3_000);
            assert_eq!(recovered.used_report_ids, vec![annual_id]);
            assert_eq!(
                recovered.last_cause.as_ref().unwrap().cause,
                BeliefCause::HorizonExpired
            );
            if method == FundamentalMethod::CashFlow {
                assert!(matches!(
                    recovered.valuation,
                    engine::strategy::ValuationOutcome::Available { .. }
                ));
            }
        }
    }
}
