use super::dividend_tests::{bind_capital, line, profitable_annual_fixture};
use super::tests::{amount, date, fixture};
use super::*;
use crate::company::stock_distribution::StockDistributionKind;
use crate::money::Money;

fn in_2031(month: u8, day: u8) -> CivilDate {
    CivilDate::from_ymd(2031, month, day).unwrap()
}

/// 面值固定为 1 分的送转声明；`capital_at_approval` 显式给出批准时点注册资本。
/// 真实会话流中新声明的 `approved_on` 必须晚于最后结算日，因而严格晚于既有送转
/// 的入账日期；fixture 保持同一时序约束。
fn one_cent_par_declaration(
    event_id: &str,
    new_shares: u64,
    capital_at_approval: i128,
    approved_on: CivilDate,
) -> StockDistributionDeclaration {
    StockDistributionDeclaration {
        event_id: event_id.into(),
        approval_reference: format!("shareholders-resolution-{event_id}"),
        kind: StockDistributionKind::BonusShares,
        approved_on,
        new_shares,
        par_value_per_share: Money::from_cents(1),
        capital_increase: AccountingAmount::from_cents(i128::from(new_shares)),
        registered_capital_at_approval: AccountingAmount::from_cents(capital_at_approval),
    }
}

#[test]
fn credited_stock_distribution_evolves_registered_capital_and_keeps_frozen_history() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    state
        .declare_stock_distribution(one_cent_par_declaration(
            "dist-1",
            3_000,
            10_000,
            in_2031(1, 1),
        ))
        .unwrap();
    state
        .record_stock_distribution_credit("dist-1", in_2031(1, 2), 3_000)
        .unwrap();
    // 送转入账按 面值×新增股数 演进注册资本法定事实；source_evidence 保留。
    let facts = state.legal_facts().clone().unwrap();
    assert_eq!(facts.registered_capital, amount(13_000));
    assert_eq!(facts.source_evidence, "explicit test fixture fact");
    // 历史由事实冻结：批准时点注册资本与面值不变。
    let frozen = state
        .stock_distribution_facts()
        .unwrap()
        .into_iter()
        .find(|fact| fact.event_id == "dist-1")
        .unwrap();
    assert_eq!(frozen.registered_capital_at_approval, amount(10_000));
    assert_eq!(frozen.par_value_per_share, Money::from_cents(1));
    // 重复回填幂等，不重复演进注册资本。
    assert!(state
        .record_stock_distribution_credit("dist-1", in_2031(1, 2), 3_000)
        .unwrap());
    assert_eq!(
        state.legal_facts().as_ref().unwrap().registered_capital,
        amount(13_000)
    );
    // 第二次送转必须按演进后注册资本与同一面值受理（面值恒定，不随股数缩小）。
    state
        .declare_stock_distribution(one_cent_par_declaration(
            "dist-2",
            3_900,
            13_000,
            in_2031(1, 3),
        ))
        .unwrap();
    state
        .record_stock_distribution_credit("dist-2", in_2031(1, 3), 3_900)
        .unwrap();
    assert_eq!(
        state.legal_facts().as_ref().unwrap().registered_capital,
        amount(16_900)
    );
    let second = state
        .stock_distribution_facts()
        .unwrap()
        .into_iter()
        .find(|fact| fact.event_id == "dist-2")
        .unwrap();
    assert_eq!(second.par_value_per_share, Money::from_cents(1));
    assert_eq!(second.registered_capital_at_approval, amount(13_000));
    // 声明过时（演进前）注册资本被显式拒绝。
    assert!(state
        .declare_stock_distribution(one_cent_par_declaration(
            "dist-stale",
            1,
            10_000,
            in_2031(1, 4)
        ))
        .is_err());
}

#[test]
fn bonus_share_cap_uses_registered_capital_evolved_by_prior_credit() {
    // 期初法定公积金 5_500：注册资本 10_000 时已达 50% 免计提门槛；
    // 送转入账把注册资本演进到 13_000 后门槛抬高，恢复计提并压缩可分配利润上限。
    let baseline = fixture();
    let mut config = baseline.config.clone();
    config.opening_lines = vec![
        line("1002", PostingSide::Debit, amount(15_500)),
        line("4001", PostingSide::Credit, amount(10_000)),
        line(
            crate::accounting::reports::simple_summary::STATUTORY_RESERVE,
            PostingSide::Credit,
            amount(5_500),
        ),
    ];
    let mut state = SimpleFinanceState::create(
        baseline.company.clone(),
        baseline.kind,
        &config,
        date(1, 1).prev().unwrap(),
    )
    .unwrap();
    for month in 1..=12 {
        let end = if month == 12 {
            CivilDate::from_ymd(2031, 1, 1).unwrap().prev().unwrap()
        } else {
            CivilDate::from_ymd(2030, month + 1, 1)
                .unwrap()
                .prev()
                .unwrap()
        };
        state
            .apply_month(
                end,
                &PeriodAmounts {
                    revenue: amount(10_000),
                    fixed_expense: amount(3_000),
                    variable_expense: amount(4_000),
                },
            )
            .unwrap();
    }
    bind_capital(&mut state);
    let before = state.distributable_profit().unwrap();
    assert_eq!(before.statutory_reserve, AccountingAmount::ZERO);
    state
        .declare_stock_distribution(one_cent_par_declaration(
            "dist-1",
            3_000,
            10_000,
            in_2031(1, 1),
        ))
        .unwrap();
    state
        .record_stock_distribution_credit("dist-1", in_2031(1, 2), 3_000)
        .unwrap();
    assert_eq!(
        state.legal_facts().as_ref().unwrap().registered_capital,
        amount(13_000)
    );
    // 可分配利润上限按演进后注册资本核定：50% 门槛重新生效并要求计提法定公积金。
    let after = state.distributable_profit().unwrap();
    assert!(
        after.statutory_reserve.is_positive(),
        "送转入账演进注册资本后，法定公积金免计提门槛必须按新事实重新核定"
    );
    // 按演进前口径本可受理（27_000 ≥ 26_000）的送股面值总额，现按演进后上限被拒。
    assert!(
        state
            .declare_stock_distribution(one_cent_par_declaration(
                "dist-oversized",
                26_000,
                13_000,
                in_2031(1, 3)
            ))
            .is_err(),
        "送股面值总额不得超过按演进后注册资本核定的可分配利润"
    );
    // 不超过演进后上限（24_300）的送股仍可受理。
    state
        .declare_stock_distribution(one_cent_par_declaration(
            "dist-2",
            24_000,
            13_000,
            in_2031(1, 3),
        ))
        .unwrap();
}

#[test]
fn credited_stock_distribution_keeps_earlier_dividend_declaration_valid() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    state
        .declare_dividend(DividendDeclaration {
            plan_id: "cash-before-dist".into(),
            approved_on: in_2031(1, 1),
            total_gross: amount(1_000),
            registered_capital: amount(10_000),
        })
        .unwrap();
    state
        .declare_stock_distribution(one_cent_par_declaration(
            "dist-1",
            3_000,
            10_000,
            in_2031(1, 1),
        ))
        .unwrap();
    state
        .record_stock_distribution_credit("dist-1", in_2031(1, 2), 3_000)
        .unwrap();
    // 送转入账演进注册资本后，既有分红声明冻结的批准时点注册资本仍须通过恢复校验。
    state.validate().unwrap();
    // 演进后新分红声明必须按当前（演进后）注册资本受理。
    state
        .declare_dividend(DividendDeclaration {
            plan_id: "cash-after-dist".into(),
            approved_on: in_2031(1, 3),
            total_gross: amount(100),
            registered_capital: amount(13_000),
        })
        .unwrap();
    state.validate().unwrap();
}

#[test]
fn tampered_fact_capital_at_approval_is_rejected_by_restore_validation() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    state
        .declare_stock_distribution(one_cent_par_declaration(
            "dist-1",
            3_000,
            10_000,
            in_2031(1, 1),
        ))
        .unwrap();
    state
        .record_stock_distribution_credit("dist-1", in_2031(1, 2), 3_000)
        .unwrap();
    let mut saved = serde_json::to_value(&state).unwrap();
    // 篡改：把冻结的批准时点注册资本改为演进后金额（130.00）。
    saved["stock_distributions"]["dist-1"]["registered_capital_at_approval"] =
        serde_json::json!("130.00");
    let tampered: SimpleFinanceState = serde_json::from_value(saved).unwrap();
    assert!(
        tampered.validate().is_err(),
        "批准时点注册资本不在法定事实演进历史上的送转声明必须被恢复校验拒绝"
    );
}
