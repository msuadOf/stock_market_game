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
    assert_eq!(
        state.books().ledger().account_net_debit(&LedgerAccountId(
            crate::accounting::reports::simple_summary::STATUTORY_RESERVE.into(),
        )).unwrap().neg().unwrap(),
        amount(8_200),
        "送转增资会恢复计提门槛；批准时预留并于入账计提 2_700，不能只算上限而不计提"
    );
    assert_eq!(after.statutory_reserve, amount(0), "2030 年度计提已经完成，不得重复计提");
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
    // 送转入账已借记 4103（N3 批起行为分录进 Books），未分配利润随之减少：
    // 可分配上限 = 27_000 − 送转 3_000 − 公积金 2_700 = 21_300，24_000 超限被拒。
    assert!(
        state
            .declare_stock_distribution(one_cent_par_declaration(
                "dist-oversized-after-posting",
                24_000,
                13_000,
                in_2031(1, 3),
            ))
            .is_err(),
        "送股面值总额不得超过扣减已入账送转后的可分配利润"
    );
    // 不超过扣减后上限（21_300）的送股仍可受理。
    state
        .declare_stock_distribution(one_cent_par_declaration(
            "dist-2",
            21_000,
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

/// 送股声明必须按未入账已占用额预留可分配利润：两笔各占满可分配利润的送股
/// 不得先后批准（否则入账时借 4103 致留存收益为负而 validate 仍 Ok）。
#[test]
fn bonus_share_declarations_reserve_distributable_profit_at_declaration() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    let profit = state.distributable_profit().unwrap();
    let available = profit.available_for_distribution;
    assert!(available.is_positive());
    // 第一笔占满可分配利润。
    state
        .declare_stock_distribution(one_cent_par_declaration(
            "dist-a",
            available.to_money().unwrap().cents() as u64,
            10_000,
            in_2031(1, 1),
        ))
        .unwrap();
    // 第二笔即使金额很小也不得再占用已被 dist-a 预留的额度之外的利润；
    // 1 分在预留后仍超出剩余额（剩余额应为 0）时必须拒绝。
    let before = state.clone();
    let second = state.declare_stock_distribution(one_cent_par_declaration(
        "dist-b",
        1,
        10_000,
        in_2031(1, 2),
    ));
    assert!(
        second.is_err(),
        "未入账送股已预留的可分配利润不得被第二笔声明重复占用"
    );
    assert_eq!(state, before, "超额声明失败不得改变已批准事实或账簿");
}

/// 送股与现金分红不得各自全额占用同一可分配利润：送股声明预留后，
/// 现金分红批准总额不得超过剩余额。
#[test]
fn bonus_share_declaration_blocks_full_cash_dividend_double_spend() {
    // 税前 9_333 分 − 所得税 2_333 分 = 税后 7_000 分；
    // 法定公积金 700 分，可分配 6_300 分（独立探针同一边界）。
    let mut state = fixture();
    state
        .apply_period(
            date(1, 1),
            date(12, 31),
            &PeriodAmounts {
                revenue: amount(10_000),
                fixed_expense: amount(667),
                variable_expense: amount(0),
            },
        )
        .unwrap();
    bind_capital(&mut state);
    assert_eq!(state.distributable_profit().unwrap().available_for_distribution, amount(6_300));
    state
        .declare_stock_distribution(one_cent_par_declaration(
            "dist-reserve",
            6_000,
            10_000,
            in_2031(1, 1),
        ))
        .unwrap();
    let before = state.clone();
    assert!(
        state
            .declare_dividend(DividendDeclaration {
                plan_id: "cash-full".into(),
                approved_on: in_2031(1, 2),
                total_gross: amount(6_000),
                registered_capital: amount(10_000),
            })
            .is_err(),
        "送股声明已预留的可分配利润不得被现金分红重复占用"
    );
    assert_eq!(state, before, "超额现金分红失败必须原子回滚");
    // 剩余 300 分仍可受理，送股回填和后续月结都不得卡死。
    state
        .declare_dividend(DividendDeclaration {
            plan_id: "cash-rest".into(),
            approved_on: in_2031(1, 2),
            total_gross: amount(300),
            registered_capital: amount(10_000),
        })
        .unwrap();
    state.record_stock_distribution_credit("dist-reserve", in_2031(1, 3), 6_000).unwrap();
    state.apply_month(in_2031(1, 31), &PeriodAmounts {
        revenue: amount(0), fixed_expense: amount(0), variable_expense: amount(0),
    }).expect("已批准来源额度内的送股与现金分红不得卡死后续结算");
    assert_eq!(state.distributable_profit().unwrap().available_for_distribution, amount(0));
}

/// 转增声明必须按未入账已占用额预留资本公积余额：两笔各超余额的转增
/// 不得先后批准（否则第一笔入账后第二笔日终失败卡死）。
#[test]
fn capital_reserve_conversions_reserve_balance_at_declaration() {
    // 资本公积 5_000、两笔 3_000 不同除权日（code-review 用例）。
    let baseline = fixture();
    let mut config = baseline.config.clone();
    config.opening_lines = vec![
        line("1002", PostingSide::Debit, amount(15_000)),
        line("4001", PostingSide::Credit, amount(10_000)),
        line(
            crate::accounting::reports::simple_summary::CAPITAL_RESERVE,
            PostingSide::Credit,
            amount(5_000),
        ),
    ];
    let mut state = SimpleFinanceState::create(
        baseline.company.clone(),
        baseline.kind,
        &config,
        date(1, 1).prev().unwrap(),
    )
    .unwrap();
    bind_capital(&mut state);
    let conversion = |event_id: &str, shares: u64, approved: CivilDate| {
        let mut declaration =
            one_cent_par_declaration(event_id, shares, 10_000, approved);
        declaration.kind =
            crate::company::stock_distribution::StockDistributionKind::CapitalReserveConversion;
        declaration
    };
    state
        .declare_stock_distribution(conversion("conv-a", 3_000, in_2031(1, 1)))
        .unwrap();
    assert!(
        state
            .declare_stock_distribution(conversion("conv-b", 3_000, in_2031(1, 2)))
            .is_err(),
        "未入账转增已预留的资本公积不得被第二笔声明重复占用"
    );
    // 第一笔入账后（资本公积降至 2_000），预留语义下无卡死：不会出现
    // 「声明均通过、入账第二笔才失败」的死局；若后续有合法额度仍可声明。
    state
        .record_stock_distribution_credit("conv-a", in_2031(1, 3), 3_000)
        .unwrap();
    let mut rest = conversion("conv-rest", 1_500, in_2031(1, 4));
    rest.registered_capital_at_approval = amount(13_000);
    state.declare_stock_distribution(rest).unwrap();
    state.record_stock_distribution_credit("conv-rest", in_2031(1, 5), 1_500).unwrap();
    // 不生成额外收入费用，只核对行为后的完整会计期间仍能结算。
    let zero = PeriodAmounts { revenue: amount(0), fixed_expense: amount(0), variable_expense: amount(0) };
    state.apply_period(date(1, 1), date(12, 31), &zero).unwrap();
    state.apply_month(in_2031(1, 31), &zero).expect("拒绝超额声明后，合法转增与后续结算不能卡死");
    state.validate().unwrap();
}

/// 送股预留不得成为跨月不变量：批准后、入账前的亏损月仍必须能月结推进，
/// 否则合法已批准送转会把日终卡死在入账日之前（独立复核修复轮发现）。
#[test]
fn declared_bonus_reservation_does_not_block_later_loss_month_close() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    // 恰占满可分配利润（27_000 − 法定公积金 2_700 = 24_300）。
    state
        .declare_stock_distribution(one_cent_par_declaration(
            "dist-full",
            24_300,
            10_000,
            in_2031(1, 1),
        ))
        .unwrap();
    // 批准后发生亏损月（费用 100）：月结必须成功——预留只在批准时点判定，
    // 不按事后利润回溯；否则入账日永远无法到达（卡死）。
    state
        .apply_month(
            in_2031(1, 31),
            &PeriodAmounts {
                revenue: amount(0),
                fixed_expense: amount(100),
                variable_expense: amount(0),
            },
        )
        .expect("批准后亏损月必须仍可月结，送转预留不是跨月不变量");
    // 送股入账是冻结义务：入账不按当期利润复核，分录使留存收益转负也合法。
    state
        .record_stock_distribution_credit("dist-full", in_2031(2, 1), 24_300)
        .expect("已批准送转入账不得被当期利润下降阻断");
    state.validate().unwrap();
    assert_eq!(
        state.distributable_profit().unwrap().available_for_distribution,
        amount(0),
        "入账后可分配利润归零（留存收益 −100）"
    );
}

/// 送股入账必须计提法定公积金（与现金分红共用年度计提及幂等）：
/// 入账分录须贷记 simple_statutory_reserve。
#[test]
fn bonus_share_credit_accrues_statutory_reserve() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    let before = state.distributable_profit().unwrap();
    assert!(
        before.statutory_reserve.is_positive(),
        "fixture 必须要求计提法定公积金，否则用例无意义"
    );
    let reserve_required = before.statutory_reserve;
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
    // 入账分录必须贷记法定公积金（与现金分红共用年度计提）。
    let reserve_account = state
        .books()
        .ledger()
        .account_net_debit(&LedgerAccountId(
            crate::accounting::reports::simple_summary::STATUTORY_RESERVE.into(),
        ))
        .unwrap()
        .neg()
        .unwrap();
    assert_eq!(
        reserve_account, reserve_required,
        "送股入账必须计提法定公积金（分录贷记 simple_statutory_reserve）"
    );
    // 幂等：重复回填不得重复计提。
    state
        .record_stock_distribution_credit("dist-1", in_2031(1, 2), 3_000)
        .unwrap();
    let reserve_after = state
        .books()
        .ledger()
        .account_net_debit(&LedgerAccountId(
            crate::accounting::reports::simple_summary::STATUTORY_RESERVE.into(),
        ))
        .unwrap()
        .neg()
        .unwrap();
    assert_eq!(reserve_after, reserve_required);
    state.validate().unwrap();
}
