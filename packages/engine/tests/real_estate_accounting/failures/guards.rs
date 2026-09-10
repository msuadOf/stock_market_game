//! 输入/治理守卫拒绝：对手方未登记、非正金额/数量、重复 id、项目数上限
//! （超项目数量）、预售超套数/超合同价、借款条款/授信、计提回拨、
//! 还本超额、开局种子守卫、政策校验、尾款超收。

use super::super::{base_config, d, yuan};
use engine::company::real_estate::{
    CapitalizationPolicy, ProjectId, RealEstateBooks, RealEstateError,
};
use engine::company::{ContractId, CounterpartyId};

const LAND: &str = "EXT-LAND-1";
const CON: &str = "EXT-CON-1";
const BUY: &str = "EXT-BUY-1";

fn p1() -> ProjectId {
    ProjectId("P-1".to_string())
}

fn buyer2() -> CounterpartyId {
    CounterpartyId("EXT-BUY-2".to_string())
}

fn land_party() -> CounterpartyId {
    CounterpartyId(LAND.to_string())
}

#[test]
fn rejects_unknown_counterparty_on_every_cash_surface() {
    let mut re = RealEstateBooks::new(base_config()).expect("assembly");
    let stranger = CounterpartyId("EXT-NOBODY".to_string());
    let before = re.clone();

    assert!(matches!(
        re.acquire_land(p1(), &stranger, 10, yuan(1_000), d("2030-01-02")),
        Err(RealEstateError::Company(
            engine::company::CompanyError::UnknownCounterparty { .. }
        ))
    ));
    assert!(matches!(
        re.borrow_project_loan(
            ContractId("L-X".to_string()),
            &stranger,
            yuan(1_000),
            500,
            d("2030-01-02"),
            d("2031-01-02"),
            None,
        ),
        Err(RealEstateError::Company(
            engine::company::CompanyError::UnknownCounterparty { .. }
        ))
    ));
    assert_eq!(re, before);
}

#[test]
fn rejects_non_positive_land_and_units() {
    let mut re = RealEstateBooks::new(base_config()).expect("assembly");
    let before = re.clone();
    assert!(matches!(
        re.acquire_land(p1(), &land_party(), 10, yuan(0), d("2030-01-02")),
        Err(RealEstateError::NonPositiveAmount { .. })
    ));
    assert!(matches!(
        re.acquire_land(p1(), &land_party(), 0, yuan(1_000), d("2030-01-02")),
        Err(RealEstateError::NonPositiveUnits { units: 0 })
    ));
    assert_eq!(re, before);
}

#[test]
fn rejects_duplicate_project_id() {
    let mut re = RealEstateBooks::new(base_config()).expect("assembly");
    re.acquire_land(p1(), &land_party(), 10, yuan(1_000), d("2030-01-02"))
        .expect("first land");
    let before = re.clone();
    assert!(matches!(
        re.acquire_land(p1(), &land_party(), 5, yuan(500), d("2030-01-03")),
        Err(RealEstateError::DuplicateProject { .. })
    ));
    assert_eq!(re, before);
}

/// QA 指定拒绝：超项目数量（单公司项目数上限来自配置 → 类型化拒绝）。
#[test]
fn rejects_project_count_beyond_configured_limit() {
    let mut re = RealEstateBooks::new(base_config()).expect("assembly"); // max_projects = 2
    re.acquire_land(p1(), &land_party(), 10, yuan(1_000), d("2030-01-02"))
        .expect("land 1");
    re.acquire_land(
        ProjectId("P-2".to_string()),
        &land_party(),
        8,
        yuan(900),
        d("2030-01-03"),
    )
    .expect("land 2");
    let before = re.clone();
    assert!(matches!(
        re.acquire_land(
            ProjectId("P-3".to_string()),
            &land_party(),
            6,
            yuan(800),
            d("2030-01-04"),
        ),
        Err(RealEstateError::ProjectCountLimit { limit: 2 })
    ));
    assert_eq!(re, before);
}

#[test]
fn rejects_presale_beyond_available_units_and_price() {
    let mut re = RealEstateBooks::new(base_config()).expect("assembly");
    re.acquire_land(p1(), &land_party(), 10, yuan(1_000), d("2030-01-02"))
        .expect("land");
    re.incur_development(
        &p1(),
        &CounterpartyId(CON.to_string()),
        yuan(500),
        d("2030-01-02"),
    )
    .expect("dev");
    let before = re.clone();
    let buyer = CounterpartyId(BUY.to_string());

    // 签约 11 套 > 项目 10 套。
    assert!(matches!(
        re.sign_presale(
            ContractId("C-B1".to_string()),
            &p1(),
            &buyer,
            11,
            yuan(30_000),
            d("2030-02-01"),
        ),
        Err(RealEstateError::PresaleBeyondAvailableUnits { .. })
    ));
    assert_eq!(re, before);
    // 签约 6 套后，再签 5 套超出余量 4。
    re.sign_presale(
        ContractId("C-1".to_string()),
        &p1(),
        &buyer,
        6,
        yuan(18_000),
        d("2030-02-01"),
    )
    .expect("sign 6");
    let before = re.clone();
    assert!(matches!(
        re.sign_presale(
            ContractId("C-2".to_string()),
            &p1(),
            &buyer2(),
            5,
            yuan(15_000),
            d("2030-02-02"),
        ),
        Err(RealEstateError::PresaleBeyondAvailableUnits {
            requested: 5,
            available: 4,
            ..
        })
    ));
    // 非正价格。
    assert!(matches!(
        re.sign_presale(
            ContractId("C-3".to_string()),
            &p1(),
            &buyer2(),
            1,
            yuan(0),
            d("2030-02-02"),
        ),
        Err(RealEstateError::NonPositiveAmount { .. })
    ));
    // 收款超出合同总价（已收 0 + 19,000 > 18,000）。
    assert!(matches!(
        re.collect_presale(
            &ContractId("C-1".to_string()),
            yuan(19_000),
            d("2030-02-03")
        ),
        Err(RealEstateError::PresaleBeyondContract { .. })
    ));
    assert_eq!(re, before);
}

#[test]
fn rejects_opening_lines_seeding_real_estate_accounts() {
    let mut config = base_config();
    config.opening_lines = vec![
        engine::accounting::JournalLine {
            account: engine::accounting::LedgerAccountId("1541".to_string()),
            side: engine::accounting::PostingSide::Debit,
            amount: yuan(1_000),
        },
        engine::accounting::JournalLine {
            account: engine::accounting::LedgerAccountId("4001".to_string()),
            side: engine::accounting::PostingSide::Credit,
            amount: yuan(1_000),
        },
    ];
    assert!(matches!(
        RealEstateBooks::new(config),
        Err(RealEstateError::OpeningRealEstateSeeded { .. })
    ));
}

#[test]
fn rejects_invalid_config_policy_and_project_cap() {
    let mut config = base_config();
    config.capitalization_policy = CapitalizationPolicy {
        version: 1,
        suspension_min_days: 0,
    };
    assert!(matches!(
        RealEstateBooks::new(config.clone()),
        Err(RealEstateError::InvalidPolicy { .. })
    ));
    config.capitalization_policy = CapitalizationPolicy {
        version: 1,
        suspension_min_days: 90,
    };
    config.max_projects = 0;
    assert!(matches!(
        RealEstateBooks::new(config),
        Err(RealEstateError::InvalidPolicy { .. })
    ));
}
