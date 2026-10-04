use super::*;
use crate::gold::{momentum_analysis, momentum_profile, stock_code};
use engine::strategy::{ForecastBasis, FundamentalMethod};

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
        assert!(case
            .apply_cause(&code, BeliefCause::NewMaterial { report: interim }, 1)
            .is_err());
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
        let before = case.book.entry(&code).unwrap().clone();
        let mut saved = case.scenario.library.save();
        let group_id = PublicationId::new(saved.next_seq);
        saved.next_seq += 1;
        let mut group = saved
            .reports
            .iter()
            .find(|report| report.id == interim)
            .unwrap()
            .clone();
        group.id = group_id;
        group.reports.scope = ScopeId::Consolidated(MemberId(COMPANY.to_owned()));
        group.reports.income.net_income_to_parent =
            Some(group.reports.income.cumulative.net_income);
        group.reports.income.minority_net_income = Some(AccountingAmount::ZERO);
        group.reports.equity.opening_minority = Some(AccountingAmount::ZERO);
        group.reports.equity.minority_net_income = Some(AccountingAmount::ZERO);
        group.reports.equity.closing_minority = Some(AccountingAmount::ZERO);
        group.reports.balance_sheet.equity_lines.push((
            engine::accounting::reports::BsLine::MinorityEquity,
            AccountingAmount::ZERO,
        ));
        saved.reports.push(group);
        case.scenario.library = PublicLibrary::from_parts(saved).unwrap();
        case.acquire(group_id, hour_after(published_at)).unwrap();
        assert!(matches!(
            case.apply_cause(&code, BeliefCause::NewMaterial { report: group_id }, 2),
            Err(BeliefError::NoOwnAnnualMaterial)
        ));
        assert_eq!(case.book.entry(&code).unwrap(), &before);
    }
}
