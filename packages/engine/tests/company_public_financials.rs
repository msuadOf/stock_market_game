#[allow(dead_code)]
#[path = "industry_reports/fixture.rs"]
mod fixture;

use engine::accounting::reports::{
    generate_report_set, BsLine, Comparative, IndustryPresentation, ReportKind, ReportRequest,
    ReportSet, ReportSource, ReportVersion, VersionKind,
};
use engine::accounting::AccountingPeriod;
use engine::calendar::CivilInstant;
use engine::company::{CompanyId, PublicReportSummary};
use engine::information::{AccountingPolicyRef, PublicationId, PublicationOrigin, PublishedReport};

fn public_json(reports: ReportSet, chart_version: u32) -> serde_json::Value {
    let published = PublishedReport {
        id: PublicationId::new(7),
        company: CompanyId("C-PUBLIC-GOLD".to_string()),
        policy: AccountingPolicyRef { chart_version },
        approved_at: CivilInstant::from_hms(fixture::d("2031-03-20"), 8, 0, 0).expect("approval"),
        published_at: CivilInstant::from_hms(fixture::d("2031-03-20"), 18, 0, 0)
            .expect("publication"),
        origin: PublicationOrigin::Correction,
        supersedes: Some(PublicationId::new(6)),
        reports,
    };
    serde_json::to_value(PublicReportSummary::from(&published)).expect("public JSON")
}

fn request(source: ReportSource<'_>, month: u8) -> ReportRequest<'_> {
    ReportRequest {
        period: AccountingPeriod::from_ymd(2030, month).expect("period"),
        kind: ReportKind::Monthly,
        source,
        version: ReportVersion {
            sequence: 2,
            supersedes: Some(1),
            kind: VersionKind::Correction {
                reason: "已公布更正说明".to_string(),
            },
        },
        adjustments: &fixture::NO_ADJUSTMENTS,
    }
}

#[test]
fn public_financials_project_all_four_industry_gold_reports() {
    for (books, industry, chart, subject, amount) in [
        (
            fixture::industrial_fixture(),
            IndustryPresentation::Industrial,
            2,
            "固定资产",
            "9880.00",
        ),
        (
            fixture::bank_fixture(),
            IndustryPresentation::Bank,
            3,
            "贷款及垫款",
            "6060.00",
        ),
        (
            fixture::insurance_fixture(),
            IndustryPresentation::Insurance,
            4,
            "保险合同负债",
            "1000.00",
        ),
        (
            fixture::real_estate_fixture(),
            IndustryPresentation::RealEstate,
            5,
            "开发存货",
            "5000.00",
        ),
    ] {
        let reports =
            generate_report_set(request(fixture::standalone("C-GOLD", &books, industry), 6))
                .expect("industry golden report");
        reports.validate().expect("gold cross-foot");
        let expected_cash = reports.cash_flow.opening_cash.to_yuan_string();
        let expected_equity = reports.equity.opening_parent.to_yuan_string();
        let expected_notes = reports.notes.items.len();
        let json = public_json(reports, chart);
        let financials = &json["financials"];
        assert!(
            financials.is_object(),
            "public DTO must expose complete published financials: {json}"
        );
        assert_eq!(
            financials["scope"],
            serde_json::json!({"Standalone": {"entity_id": "C-GOLD"}})
        );
        assert_eq!(financials["window_start"], "2030-06-01");
        assert_eq!(financials["window_end"], "2030-06-30");
        assert_eq!(
            financials["version_kind"],
            serde_json::json!({"Correction": {"reason": "已公布更正说明"}})
        );
        assert_eq!(financials["version_supersedes"], "1");
        let balance = &financials["balance_sheet"];
        let line = balance["asset_lines"]
            .as_array()
            .expect("assets")
            .iter()
            .chain(
                balance["liability_lines"]
                    .as_array()
                    .expect("liabilities")
                    .iter(),
            )
            .find(|line| line["subject"] == subject)
            .expect("industry-specific subject");
        assert_eq!(line["amount"], amount);
        assert_eq!(financials["cash_flow"]["opening_cash"], expected_cash);
        assert_eq!(financials["equity"]["opening_parent"], expected_equity);
        assert_eq!(
            financials["notes"]["items"]
                .as_array()
                .expect("published notes")
                .len(),
            expected_notes
        );
        assert!(financials["income"]["quarter"]["operating"].is_array());
        assert!(financials["income"]["cumulative"]["operating"].is_array());
        assert!(
            financials["income"]["prior_year"]
                .get("Available")
                .is_some()
                || financials["income"]["prior_year"]["Unavailable"]["reason"]
                    == "NoPriorYearHistory"
        );
        assert!(json.get("books").is_none());
        assert!(json.get("journal").is_none());
        assert!(json.get("npc_information_state").is_none());
    }
}

#[test]
fn public_financials_preserve_consolidation_and_minority_gold() {
    let parent = fixture::group_parent_books();
    let sub = fixture::group_sub_books();
    let reports = generate_report_set(request(
        ReportSource::Consolidated {
            request: fixture::group_request(&parent, &sub),
        },
        3,
    ))
    .expect("consolidated gold");
    reports.validate().expect("consolidated cross-foot");
    let json = public_json(reports, 2);
    assert!(
        json["financials"].is_object(),
        "public consolidated financials missing"
    );
    assert_eq!(
        json["financials"]["scope"],
        serde_json::json!({"Consolidated": {"root_entity_id": fixture::GROUP_ROOT}})
    );
    assert_eq!(json["financials"]["income"]["minority_net_income"], "60.00");
    assert_eq!(
        json["financials"]["income"]["net_income_to_parent"],
        "1240.00"
    );
    assert_eq!(json["financials"]["equity"]["closing_minority"], "4060.00");
    assert!(!json["financials"]["notes"]["consolidation_split_items"]
        .as_array()
        .expect("split notes")
        .is_empty());
}

#[test]
fn public_prior_year_balance_totals_are_exact_published_sums() {
    let books = fixture::industrial_fixture();
    let reports = generate_report_set(request(
        fixture::standalone("C-GOLD", &books, IndustryPresentation::Industrial),
        6,
    ))
    .expect("industrial comparative gold");
    let json = public_json(reports, 2);
    let prior = &json["financials"]["balance_sheet"]["prior_year_end"]["Available"];
    assert_eq!(
        prior["total_assets"], "100000.00",
        "available prior-year assets must have a real aggregate"
    );
    assert_eq!(prior["total_liabilities"], "0.00");
    assert_eq!(prior["total_equity"], "100000.00");
    assert_eq!(prior["equity_to_parent"], "100000.00");
    assert_eq!(prior["liabilities_and_equity"], "100000.00");
    assert!(prior["lines"]
        .as_array()
        .expect("published comparative lines")
        .iter()
        .any(|line| line["subject"] == "固定资产" && line["amount"] == "10000.00"));

    let parent = fixture::group_parent_books();
    let sub = fixture::group_sub_books();
    let reports = generate_report_set(request(
        ReportSource::Consolidated {
            request: fixture::group_request(&parent, &sub),
        },
        3,
    ))
    .expect("group comparative gold");
    let json = public_json(reports, 2);
    let prior = &json["financials"]["balance_sheet"]["prior_year_end"]["Available"];
    assert_eq!(prior["total_assets"], "120000.00");
    assert_eq!(prior["total_equity"], "120000.00");
    assert_eq!(prior["equity_to_parent"], "116000.00");
}

#[test]
fn prior_year_producer_standalone_capital_is_not_duplicated() {
    let books = fixture::industrial_fixture();
    let reports = generate_report_set(request(
        fixture::standalone("C-GOLD", &books, IndustryPresentation::Industrial),
        6,
    ))
    .expect("standalone producer gold");
    let Comparative::Available(lines) = &reports.balance_sheet.prior_year_end else {
        panic!("gold has available prior-year balances");
    };
    let capital = lines
        .iter()
        .filter(|(line, _)| *line == BsLine::PaidInCapital)
        .map(|(_, amount)| *amount)
        .collect::<Vec<_>>();
    assert_eq!(
        capital,
        vec![fixture::yuan(100_000)],
        "standalone producer must publish capital exactly once"
    );
    let equity_cents: i128 = lines
        .iter()
        .filter(|(line, _)| line.is_equity())
        .map(|(_, amount)| amount.cents())
        .sum();
    assert_eq!(equity_cents, fixture::yuan(100_000).cents());
}

#[test]
fn prior_year_producer_consolidated_capital_is_root_only_once() {
    let parent = fixture::group_parent_books();
    let sub = fixture::group_sub_books();
    let reports = generate_report_set(request(
        ReportSource::Consolidated {
            request: fixture::group_request(&parent, &sub),
        },
        3,
    ))
    .expect("consolidated producer gold");
    let Comparative::Available(lines) = &reports.balance_sheet.prior_year_end else {
        panic!("group gold has available prior-year balances");
    };
    let capital = lines
        .iter()
        .filter(|(line, _)| *line == BsLine::PaidInCapital)
        .map(|(_, amount)| *amount)
        .collect::<Vec<_>>();
    assert_eq!(
        capital,
        vec![fixture::yuan(100_000)],
        "publish root capital, not member capital followed by root capital"
    );
    let equity_cents: i128 = lines
        .iter()
        .filter(|(line, _)| line.is_equity())
        .map(|(_, amount)| amount.cents())
        .sum();
    assert_eq!(equity_cents, fixture::yuan(120_000).cents());
    let parent_cents: i128 = lines
        .iter()
        .filter(|(line, _)| line.is_equity() && *line != BsLine::MinorityEquity)
        .map(|(_, amount)| amount.cents())
        .sum();
    assert_eq!(parent_cents, fixture::yuan(116_000).cents());
}
