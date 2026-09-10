//! 项目生命周期与交付守卫：暂停/复工/完工状态机的类型化拒绝；
//! QA 指定拒绝——**未达到交付条件确认收入**（交付先于完工）与**重复交付**。

use super::super::{base_config, d, yuan};
use engine::company::real_estate::{ProjectId, RealEstateBooks, RealEstateError};
use engine::company::ContractId;
use engine::company::CounterpartyId;

const LAND: &str = "EXT-LAND-1";
const CON: &str = "EXT-CON-1";
const BUY: &str = "EXT-BUY-1";

fn p1() -> ProjectId {
    ProjectId("P-1".to_string())
}

fn land_party() -> CounterpartyId {
    CounterpartyId(LAND.to_string())
}

/// 已起工、未完工的项目（10 套，含 6 套已签约预售）。
fn developing() -> RealEstateBooks {
    let mut re = RealEstateBooks::new(base_config()).expect("assembly");
    re.acquire_land(p1(), &land_party(), 10, yuan(12_000), d("2030-01-01"))
        .expect("land");
    re.incur_development(
        &p1(),
        &CounterpartyId(CON.to_string()),
        yuan(6_000),
        d("2030-01-01"),
    )
    .expect("dev");
    re.sign_presale(
        ContractId("C-1".to_string()),
        &p1(),
        &CounterpartyId(BUY.to_string()),
        6,
        yuan(30_000),
        d("2030-01-15"),
    )
    .expect("sign");
    re
}

#[test]
fn rejects_suspend_resume_and_complete_state_violations() {
    let mut re = RealEstateBooks::new(base_config()).expect("assembly");
    re.acquire_land(p1(), &land_party(), 10, yuan(12_000), d("2030-01-01"))
        .expect("land");
    let before = re.clone();
    // 未起工不得暂停/完工。
    assert!(matches!(
        re.suspend_development(&p1(), d("2030-01-02")),
        Err(RealEstateError::SuspensionBeforeDevelopment { .. })
    ));
    assert!(matches!(
        re.complete_project(&p1(), d("2030-01-02")),
        Err(RealEstateError::CompleteBeforeDevelopment { .. })
    ));
    // 未暂停不得复工。
    assert!(matches!(
        re.resume_development(&p1(), d("2030-01-02")),
        Err(RealEstateError::NotSuspended { .. })
    ));
    assert_eq!(re, before);

    re.incur_development(
        &p1(),
        &CounterpartyId(CON.to_string()),
        yuan(6_000),
        d("2030-01-02"),
    )
    .expect("dev");
    re.suspend_development(&p1(), d("2030-02-01"))
        .expect("suspend");
    let before = re.clone();
    // 重复暂停；暂停中完工/继续投入。
    assert!(matches!(
        re.suspend_development(&p1(), d("2030-02-02")),
        Err(RealEstateError::AlreadySuspended { .. })
    ));
    assert!(matches!(
        re.complete_project(&p1(), d("2030-02-02")),
        Err(RealEstateError::CompleteWhileSuspended { .. })
    ));
    assert!(matches!(
        re.incur_development(
            &p1(),
            &CounterpartyId(CON.to_string()),
            yuan(1),
            d("2030-02-02")
        ),
        Err(RealEstateError::DevelopmentWhileSuspended { .. })
    ));
    // 复工日不晚于暂停日。
    assert!(matches!(
        re.resume_development(&p1(), d("2030-02-01")),
        Err(RealEstateError::ResumeNotForward { .. })
    ));
    assert_eq!(re, before);

    re.resume_development(&p1(), d("2030-05-01"))
        .expect("resume");
    re.complete_project(&p1(), d("2030-06-01"))
        .expect("complete");
    let before = re.clone();
    // 重复完工；完工后继续投入。
    assert!(matches!(
        re.complete_project(&p1(), d("2030-06-02")),
        Err(RealEstateError::AlreadyCompleted { .. })
    ));
    assert!(matches!(
        re.incur_development(
            &p1(),
            &CounterpartyId(CON.to_string()),
            yuan(1),
            d("2030-06-02")
        ),
        Err(RealEstateError::DevelopmentAfterCompletion { .. })
    ));
    assert_eq!(re, before);
}

/// QA 指定拒绝：未达到交付条件（项目未完工）确认收入。
#[test]
fn rejects_delivery_before_completion() {
    let mut re = developing();
    re.collect_presale(
        &ContractId("C-1".to_string()),
        yuan(18_000),
        d("2030-01-16"),
    )
    .expect("collect");
    let before = re.clone();
    assert!(matches!(
        re.deliver(&ContractId("C-1".to_string()), d("2030-02-01")),
        Err(RealEstateError::DeliveryBeforeCompletion { .. })
    ));
    // 收入分文未动（拒绝后完整状态不变）。
    assert_eq!(re, before);
}

/// QA 指定拒绝：重复交付（同一预售合同只交付一次）。
#[test]
fn rejects_duplicate_delivery() {
    let mut re = developing();
    re.collect_presale(
        &ContractId("C-1".to_string()),
        yuan(18_000),
        d("2030-01-16"),
    )
    .expect("collect");
    re.complete_project(&p1(), d("2030-02-01"))
        .expect("complete");
    re.deliver(&ContractId("C-1".to_string()), d("2030-02-02"))
        .expect("deliver");
    let before = re.clone();
    assert!(matches!(
        re.deliver(&ContractId("C-1".to_string()), d("2030-02-03")),
        Err(RealEstateError::PresaleAlreadyDelivered { .. })
    ));
    assert_eq!(re, before);
}

/// 交付后预售收款通道关闭（尾款走应收路径）。
#[test]
fn rejects_presale_collection_after_delivery() {
    let mut re = developing();
    re.collect_presale(
        &ContractId("C-1".to_string()),
        yuan(12_000),
        d("2030-01-16"),
    )
    .expect("collect");
    re.complete_project(&p1(), d("2030-02-01"))
        .expect("complete");
    re.deliver(&ContractId("C-1".to_string()), d("2030-02-02"))
        .expect("deliver");
    let before = re.clone();
    assert!(matches!(
        re.collect_presale(&ContractId("C-1".to_string()), yuan(1), d("2030-02-03")),
        Err(RealEstateError::CollectionAfterDelivery { .. })
    ));
    assert_eq!(re, before);
}

#[test]
fn rejects_unknown_project_and_presale_lookups() {
    let mut re = RealEstateBooks::new(base_config()).expect("assembly");
    let before = re.clone();
    let ghost = ProjectId("P-404".to_string());
    assert!(matches!(
        re.incur_development(
            &ghost,
            &CounterpartyId(CON.to_string()),
            yuan(1),
            d("2030-01-02")
        ),
        Err(RealEstateError::UnknownProject { .. })
    ));
    assert!(matches!(
        re.suspend_development(&ghost, d("2030-01-02")),
        Err(RealEstateError::UnknownProject { .. })
    ));
    assert!(matches!(
        re.complete_project(&ghost, d("2030-01-02")),
        Err(RealEstateError::UnknownProject { .. })
    ));
    assert!(matches!(
        re.collect_presale(&ContractId("C-404".to_string()), yuan(1), d("2030-01-02")),
        Err(RealEstateError::UnknownPresale { .. })
    ));
    assert!(matches!(
        re.deliver(&ContractId("C-404".to_string()), d("2030-01-02")),
        Err(RealEstateError::UnknownPresale { .. })
    ));
    assert_eq!(re, before);
}
