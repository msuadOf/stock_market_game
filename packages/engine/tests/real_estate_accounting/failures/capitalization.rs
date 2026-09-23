//! 资本化政策边界（QA 指定）：**无限资本化**拒绝（完工后利息必须费用化、
//! 资产字节不涨）、短中断（< 90 日）不暂停资本化、**无现金支付**正确拒绝
//! （PaymentFailed + 完整状态字节不变）。

use super::super::{acct, amt, base_config, d, net_debit, yuan};
use engine::company::real_estate::{ProjectId, RealEstateBooks, RealEstateError};
use engine::company::ContractId;
use engine::company::CounterpartyId;

const LAND: &str = "EXT-LAND-1";
const CON: &str = "EXT-CON-1";
const LEND: &str = "EXT-LEND-1";

fn p1() -> ProjectId {
    ProjectId("P-1".to_string())
}

fn land_party() -> CounterpartyId {
    CounterpartyId(LAND.to_string())
}

/// 已起工项目 + 10000 元借款（730bp，每日 200 分）。
fn developing_with_loan() -> RealEstateBooks {
    let mut re = RealEstateBooks::new(base_config()).expect("assembly");
    re.acquire_land(p1(), &land_party(), 10, yuan(12_000), d("2030-01-01"))
        .expect("land");
    re.borrow_project_loan(
        ContractId("L-1".to_string()),
        &CounterpartyId(LEND.to_string()),
        yuan(10_000),
        730,
        d("2030-01-01"),
        d("2031-06-30"),
        Some(p1()),
    )
    .expect("borrow");
    re.incur_development(
        &p1(),
        &CounterpartyId(CON.to_string()),
        yuan(6_000),
        d("2030-01-01"),
    )
    .expect("dev");
    re
}

/// QA 指定拒绝（行为断言）：完工后利息必须全部费用化——不允许把利息
/// 无限藏进资产（1541 在完工后的计提中字节不涨）。
#[test]
fn post_completion_interest_must_expense_not_capitalize_forever() {
    let mut re = developing_with_loan();
    // 完工日 = 起工日：整个计提窗口都落在完工之后。
    re.complete_project(&p1(), d("2030-01-01"))
        .expect("complete");
    let inventory_before = net_debit(&re, acct::DEV_INVENTORY);
    let items = re
        .accrue_interest(d("2030-02-14"))
        .expect("accrue post-completion");
    assert_eq!(items[0].days, 44);
    assert_eq!(items[0].capitalized_days, 0);
    assert_eq!(items[0].expensed_days, 44);
    assert_eq!(items[0].capitalized_amount, amt(0));
    assert_eq!(items[0].expensed_amount, amt(8_800));
    assert_eq!(net_debit(&re, acct::DEV_INVENTORY), inventory_before);
    assert_eq!(net_debit(&re, acct::FIN_EXP), amt(8_800));

    // 再计提一段：仍然全部费用化（终止是永久的，不随时间恢复）。
    let items = re.accrue_interest(d("2030-03-16")).expect("accrue again");
    assert_eq!(items[0].capitalized_days, 0);
    assert_eq!(items[0].expensed_days, 30);
    assert_eq!(net_debit(&re, acct::DEV_INVENTORY), inventory_before);
}

/// 短中断（30 日 < 政策阈值 90 日）不暂停资本化：利息照常入资产。
#[test]
fn short_interruption_below_threshold_keeps_capitalizing() {
    let mut re = developing_with_loan();
    // 计提 1（through 3/31）：89d 全资本化 = 17,800。
    let items = re.accrue_interest(d("2030-03-31")).expect("accrue 1");
    assert_eq!(items[0].capitalized_days, 89);
    // 短中断 [4/1, 5/1)（30 日 < 90）。
    re.suspend_development(&p1(), d("2030-04-01"))
        .expect("suspend");
    re.resume_development(&p1(), d("2030-05-01"))
        .expect("resume");
    // 计提 2（through 4/30）：30d 全部落在短中断内，但短中断不暂停 ⇒ 全资本化。
    let items = re.accrue_interest(d("2030-04-30")).expect("accrue 2");
    assert_eq!(items[0].days, 30);
    assert_eq!(items[0].capitalized_days, 30);
    assert_eq!(items[0].capitalized_amount, amt(6_000));
    assert_eq!(items[0].expensed_amount, amt(0));
    assert_eq!(net_debit(&re, acct::FIN_EXP), amt(0));
}

/// QA 指定拒绝：无现金支付 → PaymentFailed + 完整状态不变（开发投入路径）。
#[test]
fn rejects_development_spend_without_cash_payment_failed() {
    let mut config = base_config();
    config.opening_lines = vec![
        super::super::cent_line(
            acct::CASH,
            engine::accounting::PostingSide::Debit,
            1_000_000,
        ),
        super::super::cent_line(
            acct::CAPITAL,
            engine::accounting::PostingSide::Credit,
            1_000_000,
        ),
    ];
    let mut re = RealEstateBooks::new(config).expect("assembly");
    re.acquire_land(p1(), &land_party(), 10, yuan(9_000), d("2030-01-01"))
        .expect("land"); // 现金余 1000 元
    let before = re.clone();
    let err = re
        .incur_development(
            &p1(),
            &CounterpartyId(CON.to_string()),
            yuan(6_000),
            d("2030-01-02"),
        )
        .expect_err("must fail: only 1000 yuan left");
    assert!(matches!(err, RealEstateError::PaymentFailed { .. }));
    assert_eq!(re, before);
}

/// QA 指定拒绝：无现金付息 → PaymentFailed + 完整状态不变（筹资路径）。
#[test]
fn rejects_interest_payment_without_cash_payment_failed() {
    // 开局现金 1000 元 + 无项目借款 10000 元；土地 10500 + 开发 500 ⇒ 现金
    // 归零，89d 计提 17,800 分（全部费用化——无指定项目）无现金可付。
    let mut config = base_config();
    config.opening_lines = vec![
        super::super::cent_line(acct::CASH, engine::accounting::PostingSide::Debit, 100_000),
        super::super::cent_line(
            acct::CAPITAL,
            engine::accounting::PostingSide::Credit,
            100_000,
        ),
    ];
    let mut re = RealEstateBooks::new(config).expect("assembly");
    re.borrow_project_loan(
        ContractId("L-1".to_string()),
        &CounterpartyId(LEND.to_string()),
        yuan(10_000),
        730,
        d("2030-01-01"),
        d("2031-06-30"),
        None,
    )
    .expect("borrow");
    re.acquire_land(p1(), &land_party(), 10, yuan(10_500), d("2030-01-01"))
        .expect("land");
    re.incur_development(
        &p1(),
        &CounterpartyId(CON.to_string()),
        yuan(500),
        d("2030-01-01"),
    )
    .expect("dev"); // 现金 0
    re.accrue_interest(d("2030-03-31"))
        .expect("accrue: 17,800 expensed");
    let before = re.clone();
    let err = re
        .pay_interest(&ContractId("L-1".to_string()), d("2030-04-01"))
        .expect_err("must fail: zero cash");
    assert!(matches!(err, RealEstateError::PaymentFailed { .. }));
    assert_eq!(re, before);
}
