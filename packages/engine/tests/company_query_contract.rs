#[allow(dead_code)]
#[path = "publications/fixture.rs"]
mod fixture;
#[path = "publications/session_fixture.rs"]
mod session_fixture;

use engine::calendar::{CivilDate, CivilInstant};
use engine::company::{CompanyId, PublicReportQuery};
use engine::information::assemble_seeded_prehistory;

fn library_at_start() -> (engine::information::PublicLibrary, CivilInstant) {
    let start = CivilDate::from_iso("2000-01-01").expect("valid start date");
    let as_of = CivilInstant::new(start, 0).expect("valid start instant");
    let seeded = assemble_seeded_prehistory(fixture::two_company_config(11, start), start)
        .expect("seeded public reports");
    (seeded.library, as_of)
}

#[test]
fn paginates_published_reports_by_stable_id_without_private_state() {
    let (library, as_of) = library_at_start();
    let company_id = CompanyId(fixture::INDUSTRIAL_ID.to_string());
    let first = library
        .query_public_reports(
            &PublicReportQuery {
                company_id: company_id.0.clone(),
                cursor: None,
                page_size: Some(2),
            },
            as_of,
        )
        .expect("first public page");
    assert_eq!(first.reports.len(), 2);
    let first_id = first.reports[0]
        .id
        .parse::<u32>()
        .expect("decimal report id");
    let second_id = first.reports[1]
        .id
        .parse::<u32>()
        .expect("decimal report id");
    assert!(first_id < second_id);
    assert_eq!(
        first.next_cursor.as_deref(),
        Some(first.reports[1].id.as_str())
    );
    assert_eq!(first.reports[0].company_id, fixture::INDUSTRIAL_ID);
    assert!(
        first.reports[0].period.len() == 10
            && first.reports[0]
                .period
                .bytes()
                .enumerate()
                .all(|(index, byte)| {
                    matches!(index, 4 | 7) && byte == b'-'
                        || !matches!(index, 4 | 7) && byte.is_ascii_digit()
                }),
        "public period must be canonical YYYY-MM-DD: {}",
        first.reports[0].period
    );
    assert!(first.reports[0]
        .accounting
        .total_assets
        .chars()
        .all(|character| character.is_ascii_digit() || character == '.' || character == '-'));
    assert!(first.reports[0]
        .accounting
        .net_income
        .chars()
        .all(|character| character.is_ascii_digit() || character == '.' || character == '-'));

    let second = library
        .query_public_reports(
            &PublicReportQuery {
                company_id: company_id.0,
                cursor: first.next_cursor,
                page_size: Some(2),
            },
            as_of,
        )
        .expect("second public page");
    assert!(
        second.reports[0]
            .id
            .parse::<u32>()
            .expect("decimal report id")
            > second_id
    );
    assert!(
        second.reports[1]
            .id
            .parse::<u32>()
            .expect("decimal report id")
            > second_id
    );
    let json = serde_json::to_value(&second).expect("page serializes");
    assert_eq!(
        json.as_object()
            .expect("public report page is an object")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["next_cursor", "reports"],
    );
    let report_json = json["reports"][0]
        .as_object()
        .expect("public report is an object");
    assert_eq!(
        report_json.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "accounting",
            "approved_date",
            "approved_second_of_day",
            "company_id",
            "id",
            "kind",
            "period",
            "published_date",
            "published_second_of_day",
            "supersedes",
            "version_sequence",
        ],
    );
    assert_eq!(
        report_json["accounting"]
            .as_object()
            .expect("accounting summary is an object")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "closing_cash",
            "financing_cash_flow",
            "income_tax",
            "investing_cash_flow",
            "net_cash_change",
            "net_income",
            "operating_cash_flow",
            "prior_year_net_income",
            "quarter_net_income",
            "total_assets",
            "total_equity",
            "total_liabilities",
        ],
    );
    assert!(json.get("books").is_none());
    assert!(json.get("journal").is_none());
    assert!(json.get("npc_information_state").is_none());
    println!("{json}");
}

#[test]
fn report_lookup_preserves_old_versions_and_rejects_unpublished_or_bad_requests() {
    let (library, as_of) = library_at_start();
    let first_id = library.publication_ids()[0];
    let report = library
        .public_report_by_id(first_id.value().to_string(), as_of)
        .expect("published report lookup");
    assert_eq!(report.id, first_id.value().to_string());

    let early =
        CivilInstant::new(CivilDate::from_iso("1998-01-01").expect("date"), 0).expect("instant");
    assert!(library
        .public_report_by_id(first_id.value().to_string(), early)
        .is_err());
    for page_size in [Some(0), Some(101)] {
        assert!(library
            .query_public_reports(
                &PublicReportQuery {
                    company_id: fixture::INDUSTRIAL_ID.to_string(),
                    cursor: None,
                    page_size,
                },
                as_of,
            )
            .is_err());
    }
    assert!(library
        .query_public_reports(
            &PublicReportQuery {
                company_id: fixture::INDUSTRIAL_ID.to_string(),
                cursor: Some("not-a-report-id".to_string()),
                page_size: None,
            },
            as_of,
        )
        .is_err());
    assert!(library
        .public_report_by_id((u64::from(u32::MAX) + 1).to_string(), as_of)
        .is_err());
    let unlisted = library
        .query_public_reports(
            &PublicReportQuery {
                company_id: "C-UNLISTED-TEST".to_string(),
                cursor: None,
                page_size: None,
            },
            as_of,
        )
        .expect("unknown or unlisted company has no reports");
    assert!(unlisted.reports.is_empty());
}

#[test]
fn session_queries_use_its_current_civil_date() {
    let start = CivilDate::from_iso("2000-01-01").expect("valid start date");
    let setup = session_fixture::civil_setup(start);
    let session = engine::GameSession::new(setup, 11).expect("session assembles");
    let page = session
        .query_public_reports(&PublicReportQuery {
            company_id: "C-600101".to_string(),
            cursor: None,
            page_size: None,
        })
        .expect("current-date public reports");
    assert!(!page.reports.is_empty());
}
