//! 公司行为接续进汇总账簿与账面展示字段的集成测试（N3 批，2026-10-08）。
//!
//! 覆盖口径：
//! - 送转／配股结算／回购注销／缩股在真实入账回填时过账进 `Books`，
//!   下期报表自然反映股本／资本公积变化，再下期不回退；
//! - 现金分红宣告／付款两步的权益负债联动（既有行为回归断言）；
//! - `cash_book`／`investment_book` 展示字段按用户公式更新（含分红减少 cash）；
//! - `book_display` 配置严格持久化、旧档显式拒绝、restore 深等；
//! - 行为分录不得向已封账期间回写（日期守卫）；
//! - 跨行为期间的报告 ROE 不因新行为分录变成 UnclassifiedEquityEvent。

use super::dividend_tests::{bind_capital, line, profitable_annual_fixture};
use super::tests::{amount, date, fixture};
use super::*;
use crate::accounting::reports::simple_summary as accounts;
use crate::accounting::reports::{BsLine, ReportKind};
use crate::money::Money;

fn in_2031(month: u8, day: u8) -> CivilDate {
    CivilDate::from_ymd(2031, month, day).unwrap()
}

fn month_end(year: i32, month: u8) -> CivilDate {
    if month == 12 {
        CivilDate::from_ymd(year + 1, 1, 1).unwrap().prev().unwrap()
    } else {
        CivilDate::from_ymd(year, month + 1, 1)
            .unwrap()
            .prev()
            .unwrap()
    }
}

/// 标准盈利月：收入 10_000、固定 3_000、变动 4_000（分）。
fn apply_profit_month(state: &mut SimpleFinanceState, year: i32, month: u8) {
    state
        .apply_month(
            month_end(year, month),
            &PeriodAmounts {
                revenue: amount(10_000),
                fixed_expense: amount(3_000),
                variable_expense: amount(4_000),
            },
        )
        .unwrap();
}

fn credit_balance(state: &SimpleFinanceState, account: &str) -> AccountingAmount {
    state
        .books()
        .ledger()
        .account_net_debit(&LedgerAccountId(account.into()))
        .unwrap()
        .neg()
        .unwrap()
}

fn equity_line(
    state: &mut SimpleFinanceState,
    year: i32,
    month: u8,
    target: BsLine,
) -> Option<AccountingAmount> {
    let report = state
        .prepare_report(
            crate::accounting::AccountingPeriod::from_ymd(year, month).unwrap(),
            ReportKind::Monthly,
        )
        .unwrap();
    report
        .balance_sheet
        .equity_lines
        .iter()
        .find(|(line, _)| *line == target)
        .map(|(_, value)| *value)
}

fn asset_line(
    state: &mut SimpleFinanceState,
    year: i32,
    month: u8,
    target: BsLine,
) -> Option<AccountingAmount> {
    let report = state
        .prepare_report(
            crate::accounting::AccountingPeriod::from_ymd(year, month).unwrap(),
            ReportKind::Monthly,
        )
        .unwrap();
    report
        .balance_sheet
        .asset_lines
        .iter()
        .find(|(line, _)| *line == target)
        .map(|(_, value)| *value)
}

fn count_entries(state: &SimpleFinanceState, kind: BusinessKind) -> usize {
    state
        .books()
        .journal()
        .entries()
        .filter(|entry| entry.kind == kind)
        .count()
}

fn bonus_declaration(
    event_id: &str,
    new_shares: u64,
    capital_at_approval: i128,
    approved_on: CivilDate,
) -> crate::company::stock_distribution::StockDistributionDeclaration {
    crate::company::stock_distribution::StockDistributionDeclaration {
        event_id: event_id.into(),
        approval_reference: format!("shareholders-resolution-{event_id}"),
        kind: crate::company::stock_distribution::StockDistributionKind::BonusShares,
        approved_on,
        new_shares,
        par_value_per_share: Money::from_cents(1),
        capital_increase: AccountingAmount::from_cents(i128::from(new_shares)),
        registered_capital_at_approval: AccountingAmount::from_cents(capital_at_approval),
    }
}

/// 送转入账：股本按面值增加、来源科目核减；下期资产负债表反映新股本且再下期不回退。
#[test]
fn credited_bonus_distribution_posts_entries_reflected_in_next_reports() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    state
        .declare_stock_distribution(bonus_declaration(
            "dist-1",
            3_000,
            10_000,
            in_2031(1, 1),
        ))
        .unwrap();
    state
        .record_stock_distribution_credit("dist-1", in_2031(1, 2), 3_000)
        .unwrap();
    // 分录：借 4103（送股来自可分配利润）、贷 4001（股本按面值增加）。
    assert_eq!(credit_balance(&state, "4001"), amount(13_000));
    assert_eq!(
        state
            .books()
            .ledger()
            .account_net_debit(&LedgerAccountId("4103".into()))
            .unwrap(),
        amount(3_000)
    );
    // 下期报表：实收资本行 = 新注册资本 13_000。
    apply_profit_month(&mut state, 2031, 1);
    assert_eq!(
        equity_line(&mut state, 2031, 1, BsLine::PaidInCapital),
        Some(amount(13_000))
    );
    // 再下期不回退：行为分录是一次性事实，周期结算不得回退。
    apply_profit_month(&mut state, 2031, 2);
    assert_eq!(
        equity_line(&mut state, 2031, 2, BsLine::PaidInCapital),
        Some(amount(13_000))
    );
    assert_eq!(
        count_entries(
            &state,
            BusinessKind::CompanyStockDistributionCredit
        ),
        1
    );
    // 重复回填幂等：不追加第二笔分录。
    assert!(state
        .record_stock_distribution_credit("dist-1", in_2031(1, 2), 3_000)
        .unwrap());
    assert_eq!(
        count_entries(
            &state,
            BusinessKind::CompanyStockDistributionCredit
        ),
        1
    );
    // 报表恒等式仍成立。
    let report = state
        .prepare_report(
            crate::accounting::AccountingPeriod::from_ymd(2031, 2).unwrap(),
            ReportKind::Monthly,
        )
        .unwrap();
    report.validate().unwrap();
}

/// 转增：来源为资本公积科目，余额不足时显式拒绝。
#[test]
fn capital_reserve_conversion_debits_reserve_and_requires_balance() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    // 无资本公积余额时声明转增被显式拒绝。
    let mut no_reserve = bonus_declaration("conv-0", 1_000, 10_000, in_2031(1, 1));
    no_reserve.kind =
        crate::company::stock_distribution::StockDistributionKind::CapitalReserveConversion;
    assert!(state.declare_stock_distribution(no_reserve).is_err());
    // 先配股溢价形成资本公积 5_000，再转增 1_000 可受理。
    state
        .declare_rights_offering(crate::company::rights_offering::RightsOfferingDeclaration {
            event_id: "rights-1".into(),
            approval_reference: "board-rights-1".into(),
            approved_on: in_2031(1, 1),
            price_per_share: Money::from_cents(2),
            par_value_per_share: Money::from_cents(1),
            registered_capital_at_approval: amount(10_000),
        })
        .unwrap();
    state
        .record_rights_offering_settlement("rights-1", in_2031(1, 2), 5_000, amount(10_000))
        .unwrap();
    assert_eq!(
        credit_balance(&state, accounts::CAPITAL_RESERVE),
        amount(5_000)
    );
    let mut conversion = bonus_declaration("conv-1", 1_000, 15_000, in_2031(1, 3));
    conversion.kind =
        crate::company::stock_distribution::StockDistributionKind::CapitalReserveConversion;
    state.declare_stock_distribution(conversion).unwrap();
    state
        .record_stock_distribution_credit("conv-1", in_2031(1, 4), 1_000)
        .unwrap();
    assert_eq!(credit_balance(&state, "4001"), amount(16_000));
    assert_eq!(
        credit_balance(&state, accounts::CAPITAL_RESERVE),
        amount(4_000)
    );
    state.validate().unwrap();
}

/// 配股结算：面值进股本、溢价进资本公积、募集资金挂非现金资产调整；权益与资产同步上升。
#[test]
fn rights_settlement_posts_capital_premium_and_funding_asset() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    state
        .declare_rights_offering(crate::company::rights_offering::RightsOfferingDeclaration {
            event_id: "rights-1".into(),
            approval_reference: "board-rights-1".into(),
            approved_on: in_2031(1, 1),
            price_per_share: Money::from_cents(2),
            par_value_per_share: Money::from_cents(1),
            registered_capital_at_approval: amount(10_000),
        })
        .unwrap();
    state
        .record_rights_offering_settlement("rights-1", in_2031(1, 2), 5_000, amount(10_000))
        .unwrap();
    assert_eq!(credit_balance(&state, "4001"), amount(15_000));
    assert_eq!(
        credit_balance(&state, accounts::CAPITAL_RESERVE),
        amount(5_000)
    );
    assert_eq!(
        state
            .books()
            .ledger()
            .account_net_debit(&LedgerAccountId(accounts::ISSUER_FUNDING_ASSET.into()))
            .unwrap(),
        amount(10_000)
    );
    apply_profit_month(&mut state, 2031, 1);
    assert_eq!(
        equity_line(&mut state, 2031, 1, BsLine::PaidInCapital),
        Some(amount(15_000))
    );
    assert_eq!(
        equity_line(&mut state, 2031, 1, BsLine::CapitalReserve),
        Some(amount(5_000))
    );
    assert_eq!(
        asset_line(&mut state, 2031, 1, BsLine::SimpleIssuerFundingAdjustment),
        Some(amount(10_000))
    );
    // 幂等：重复结算不追加分录。
    assert!(state
        .record_rights_offering_settlement("rights-1", in_2031(1, 2), 5_000, amount(10_000))
        .unwrap());
    assert_eq!(
        count_entries(&state, BusinessKind::CompanyRightsOfferingSettlement),
        1
    );
    let report = state
        .prepare_report(
            crate::accounting::AccountingPeriod::from_ymd(2031, 1).unwrap(),
            ReportKind::Monthly,
        )
        .unwrap();
    report.validate().unwrap();
}

/// 回购注销：按面值核减股本并等额归集资本公积（简化口径），下期报表反映核减。
#[test]
fn repurchase_cancellation_reduces_share_capital_in_books() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    state
        .declare_issuer_repurchase(crate::company::issuer_repurchase::IssuerRepurchaseFinanceFact {
            event_id: "buy-1".into(),
            approval_reference: "board-buy-1".into(),
            approved_on: in_2031(1, 1),
            synthetic_funding: amount(10_000),
            purpose: crate::company::issuer_repurchase::RepurchasePurpose::ReduceCapital,
            spent: None,
            withdrawn_remainder: None,
            completed_on: None,
            cancelled_shares: 0,
            cancelled_on: None,
            capital_reduction: None,
        })
        .unwrap();
    state
        .record_issuer_repurchase_completion("buy-1", in_2031(1, 10), amount(6_000), amount(4_000))
        .unwrap();
    state
        .record_issuer_repurchase_cancellation("buy-1", in_2031(1, 15), 2_000, 10_000)
        .unwrap();
    assert_eq!(credit_balance(&state, "4001"), amount(8_000));
    assert_eq!(
        credit_balance(&state, accounts::CAPITAL_RESERVE),
        amount(2_000)
    );
    assert_eq!(
        state.legal_facts().as_ref().unwrap().registered_capital,
        amount(8_000)
    );
    apply_profit_month(&mut state, 2031, 1);
    assert_eq!(
        equity_line(&mut state, 2031, 1, BsLine::PaidInCapital),
        Some(amount(8_000))
    );
    // 幂等：重复注销回填不追加分录。
    assert!(state
        .record_issuer_repurchase_cancellation("buy-1", in_2031(1, 15), 2_000, 10_000)
        .unwrap());
    assert_eq!(
        count_entries(&state, BusinessKind::CompanyRepurchaseCancellation),
        1
    );
    let report = state
        .prepare_report(
            crate::accounting::AccountingPeriod::from_ymd(2031, 1).unwrap(),
            ReportKind::Monthly,
        )
        .unwrap();
    report.validate().unwrap();
}

/// 缩股按消灭面值核减股本；拆股面值总额不变、不过账。
#[test]
fn share_consolidation_posts_reduction_and_split_posts_nothing() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    // 拆股 1 拆 2：面值 1 分 → 0 分不合法（须整除为正整数分），改用面值 2 分拆 2。
    // 先重新绑定面值口径：以 10_000 股、面值 1 分为准（注册资本 10_000 分）。
    state
        .declare_share_split(crate::company::share_split::ShareSplitDeclaration {
            event_id: "split-1".into(),
            approval_reference: "board-split-1".into(),
            direction: crate::company::share_split::ShareSplitDirection::Split,
            approved_on: in_2031(1, 1),
            ratio: 2,
            par_value_before: Money::from_cents(2),
            par_value_after: Money::from_cents(1),
            registered_capital_at_approval: amount(10_000),
        })
        .unwrap();
    let entries_before = state.books().journal().entries().count();
    state
        .record_share_split_credit("split-1", in_2031(1, 2), 5_000, 10_000)
        .unwrap();
    // 拆股总面值不变：不产生行为分录，4001 余额不变。
    assert_eq!(state.books().journal().entries().count(), entries_before);
    assert_eq!(credit_balance(&state, "4001"), amount(10_000));
    // 缩股 2 并 1（当前面值 1 分 → 2 分）：10_000 → 4_999 股，
    // 旧股口径消灭面值 = 1 分 × (10_000 − 2×4_999) = 2 分。
    state
        .declare_share_split(crate::company::share_split::ShareSplitDeclaration {
            event_id: "consol-1".into(),
            approval_reference: "board-consol-1".into(),
            direction: crate::company::share_split::ShareSplitDirection::Consolidate,
            approved_on: in_2031(1, 3),
            ratio: 2,
            par_value_before: Money::from_cents(1),
            par_value_after: Money::from_cents(2),
            registered_capital_at_approval: amount(10_000),
        })
        .unwrap();
    state
        .record_share_split_credit("consol-1", in_2031(1, 4), 10_000, 4_999)
        .unwrap();
    assert_eq!(credit_balance(&state, "4001"), amount(9_998));
    assert_eq!(
        credit_balance(&state, accounts::CAPITAL_RESERVE),
        amount(2)
    );
    state.validate().unwrap();
}

/// 现金分红宣告／付款两步的权益负债联动（既有行为回归断言）。
#[test]
fn dividend_declaration_and_payment_move_equity_and_liability() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    state
        .declare_dividend(DividendDeclaration {
            plan_id: "plan-1".into(),
            approved_on: in_2031(1, 5),
            total_gross: amount(1_000),
            registered_capital: amount(10_000),
        })
        .unwrap();
    // 宣告：应付股利负债 1_000；4103 借方增加（权益减少）。
    assert_eq!(
        credit_balance(&state, accounts::DIVIDEND_PAYABLE),
        amount(1_000)
    );
    state
        .pay_dividend("plan-1", "pay-1", in_2031(2, 1), amount(400))
        .unwrap();
    // 付款：负债清偿至 600；非现金结算资产出现负余额（账面/真实分离）。
    assert_eq!(
        credit_balance(&state, accounts::DIVIDEND_PAYABLE),
        amount(600)
    );
    assert_eq!(
        state
            .books()
            .ledger()
            .account_net_debit(&LedgerAccountId(accounts::DIVIDEND_SETTLEMENT_ASSET.into()))
            .unwrap(),
        amount(-400)
    );
    apply_profit_month(&mut state, 2031, 1);
    apply_profit_month(&mut state, 2031, 2);
    let report = state
        .prepare_report(
            crate::accounting::AccountingPeriod::from_ymd(2031, 2).unwrap(),
            ReportKind::Monthly,
        )
        .unwrap();
    report.validate().unwrap();
}

/// cash_book / investment_book 按用户公式更新：cash = 累计净利润 − 累计已付分红，
/// investment = 累计收入 × 配置比例（默认 30%）。
#[test]
fn book_display_fields_follow_user_formula() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    // profitable_annual_fixture 已有 12 个月（2030）：每月收入 10_000、税后 2_250。
    assert_eq!(state.cash_book(), amount(27_000));
    assert_eq!(state.investment_book(), amount(36_000));
    // 第 13 个月：累计净利润 29_250、累计收入 130_000 → investment 39_000。
    apply_profit_month(&mut state, 2031, 1);
    assert_eq!(state.cash_book(), amount(29_250));
    assert_eq!(state.investment_book(), amount(39_000));
    // 分红付款减少 cash_book，不影响 investment_book。
    state
        .declare_dividend(DividendDeclaration {
            plan_id: "plan-1".into(),
            approved_on: in_2031(2, 1),
            total_gross: amount(1_000),
            registered_capital: amount(10_000),
        })
        .unwrap();
    state
        .pay_dividend("plan-1", "pay-1", in_2031(2, 2), amount(1_000))
        .unwrap();
    assert_eq!(state.cash_book(), amount(28_250));
    assert_eq!(state.investment_book(), amount(39_000));
}

/// 投资额比例可配置（0 与 100% 边界 + 半偶舍入）。
#[test]
fn book_display_investment_ratio_is_configurable() {
    let baseline = fixture();
    for (bp, expected) in [(0, 0), (10_000, 10_000)] {
        let mut config = baseline.config().clone();
        config.book_display.investment_of_revenue_bp = bp;
        let mut state = SimpleFinanceState::create(
            baseline.company_id().clone(),
            baseline.kind(),
            &config,
            date(1, 1).prev().unwrap(),
        )
        .unwrap();
        apply_profit_month(&mut state, 2030, 1);
        assert_eq!(state.investment_book(), amount(expected));
    }
    // 半偶舍入：收入 5 分 × 30% = 1.5 分 → 2 分。
    let mut state = fixture();
    state
        .apply_month(
            month_end(2030, 1),
            &PeriodAmounts {
                revenue: amount(5),
                fixed_expense: amount(0),
                variable_expense: amount(0),
            },
        )
        .unwrap();
    assert_eq!(state.investment_book(), amount(2));
}

/// book_display 配置与展示字段严格持久化：旧档缺字段显式拒绝、restore 深等。
#[test]
fn book_display_fields_are_strictly_persisted() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    let saved = serde_json::to_value(&state).unwrap();
    // restore 深等。
    let restored: SimpleFinanceState = serde_json::from_value(saved.clone()).unwrap();
    assert_eq!(restored, state);
    // 旧档缺少 cash_book / investment_book 被显式拒绝。
    let mut missing_cash = saved.clone();
    missing_cash
        .as_object_mut()
        .unwrap()
        .remove("cash_book");
    assert!(serde_json::from_value::<SimpleFinanceState>(missing_cash).is_err());
    let mut missing_investment = saved.clone();
    missing_investment
        .as_object_mut()
        .unwrap()
        .remove("investment_book");
    assert!(serde_json::from_value::<SimpleFinanceState>(missing_investment).is_err());
    // 旧档配置缺少 book_display 被显式拒绝。
    let mut missing_config = saved.clone();
    missing_config["config"]
        .as_object_mut()
        .unwrap()
        .remove("book_display");
    assert!(serde_json::from_value::<SimpleFinanceState>(missing_config).is_err());
    // 比例越界（>10000bp）被拒绝。
    let mut invalid_bp = saved.clone();
    invalid_bp["config"]["book_display"]["investment_of_revenue_bp"] =
        serde_json::json!(10_001);
    let parsed: Result<SimpleFinanceState, _> = serde_json::from_value(invalid_bp);
    assert!(parsed.unwrap().validate().is_err());
}

/// 行为分录不得向已封账期间回写：入账日期落在已结算期间时显式拒绝。
#[test]
fn action_entry_backdated_into_closed_period_is_rejected() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    state
        .declare_stock_distribution(bonus_declaration(
            "dist-1",
            3_000,
            10_000,
            in_2031(1, 1),
        ))
        .unwrap();
    // 先结算 2031-01（as_of 推进到 2031-01-31），再以月初日期回填入账：
    // 2031-01-02 已落在封账期间内，必须显式拒绝且不落任何分录。
    apply_profit_month(&mut state, 2031, 1);
    assert!(state
        .record_stock_distribution_credit("dist-1", in_2031(1, 2), 3_000)
        .is_err());
    assert_eq!(credit_balance(&state, "4001"), amount(10_000));
    assert_eq!(
        count_entries(
            &state,
            BusinessKind::CompanyStockDistributionCredit
        ),
        0
    );
}

/// 跨行为期间的报告 ROE：配股（权益增加）正确进入加权平均；送转为权益内部
/// 结转不触发 UnclassifiedEquityEvent。
#[test]
fn report_roe_survives_cross_action_windows() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    state
        .declare_rights_offering(crate::company::rights_offering::RightsOfferingDeclaration {
            event_id: "rights-1".into(),
            approval_reference: "board-rights-1".into(),
            approved_on: in_2031(1, 1),
            price_per_share: Money::from_cents(2),
            par_value_per_share: Money::from_cents(1),
            registered_capital_at_approval: amount(10_000),
        })
        .unwrap();
    state
        .record_rights_offering_settlement("rights-1", in_2031(1, 2), 5_000, amount(10_000))
        .unwrap();
    state
        .declare_stock_distribution(bonus_declaration(
            "dist-1",
            1_000,
            15_000,
            in_2031(1, 3),
        ))
        .unwrap();
    state
        .record_stock_distribution_credit("dist-1", in_2031(1, 4), 1_000)
        .unwrap();
    apply_profit_month(&mut state, 2031, 1);
    let report = state
        .prepare_report(
            crate::accounting::AccountingPeriod::from_ymd(2031, 1).unwrap(),
            ReportKind::Monthly,
        )
        .unwrap();
    report.validate().unwrap();
    assert_eq!(
        report.roe.basis,
        crate::accounting::reports::ReportRoeBasis::AttributableToOrdinaryShareholders
    );
    assert!(matches!(
        report.roe.ordinary_roe,
        crate::accounting::reports::ReportRoeValue::Available(_)
    ));
    // prior_lines 去重不变量：比较项每个权益行恰好出现一次（HEAD 曾出现
    // 法定公积金被主循环与权益追加段各列一次的重复列示）。
    let prior = match &report.balance_sheet.prior_year_end {
        crate::accounting::reports::Comparative::Available(lines) => lines.clone(),
        crate::accounting::reports::Comparative::Unavailable { .. } => {
            panic!("12 个月盈利 fixture 的年报必须可用上年年末比较项")
        }
    };
    for line in prior
        .iter()
        .filter(|(line, _)| line.is_equity())
        .map(|(line, _)| *line)
    {
        assert_eq!(
            prior.iter().filter(|(other, _)| other == &line).count(),
            1,
            "比较项权益行 {line:?} 必须恰好出现一次"
        );
    }
}

/// 恢复勾稽负路径：删除、篡改或伪造行为分录都必须被 validate 显式拒绝
///（「行为分录不被周期结算回退」不变量的篡改侧覆盖）。
#[test]
fn tampered_capital_action_entries_are_rejected_by_restore_validation() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    state
        .declare_stock_distribution(bonus_declaration(
            "dist-1",
            3_000,
            10_000,
            in_2031(1, 1),
        ))
        .unwrap();
    state
        .record_stock_distribution_credit("dist-1", in_2031(1, 2), 3_000)
        .unwrap();
    let saved = serde_json::to_value(&state).unwrap();
    // 1) 删除行为分录（凭证批次中移除该来源的凭证行）。
    let mut removed = saved.clone();
    let batches = removed["books"]["journal"]["batches"]
        .as_array_mut()
        .unwrap();
    'outer: for batch in batches.iter_mut() {
        let entries = batch.as_array_mut().unwrap();
        for index in 0..entries.len() {
            if entries[index]["kind"] == "CompanyStockDistributionCredit" {
                entries.remove(index);
                break 'outer;
            }
        }
    }
    let restored: SimpleFinanceState = serde_json::from_value(removed).unwrap();
    assert!(
        restored.validate().is_err(),
        "已入账送转缺少对应汇总分录必须被恢复校验拒绝"
    );
    // 2) 篡改分录金额（4103 借方 30.00 → 20.00；借贷同时改以维持平衡）。
    let mut tampered = saved.clone();
    {
        let batches = tampered["books"]["journal"]["batches"]
            .as_array_mut()
            .unwrap();
        'find: for batch in batches.iter_mut() {
            for entry in batch.as_array_mut().unwrap() {
                if entry["kind"] == "CompanyStockDistributionCredit" {
                    entry["lines"][0]["amount"] = serde_json::json!("20.00");
                    entry["lines"][1]["amount"] = serde_json::json!("20.00");
                    break 'find;
                }
            }
        }
    }
    let restored: SimpleFinanceState = serde_json::from_value(tampered).unwrap();
    assert!(
        restored.validate().is_err(),
        "行为分录金额与行为事实不一致必须被恢复校验拒绝"
    );
    // 3) 伪造多余行为分录（重复同一来源序列之外追加一对平衡行）。
    let mut extra = saved.clone();
    {
        let next_source = state.next_event_id();
        let batches = extra["books"]["journal"]["batches"]
            .as_array_mut()
            .unwrap();
        batches.push(serde_json::json!([{
            "source": next_source,
            "date": "2031-01-02",
            "kind": "CompanyRepurchaseCancellation",
            "cash_flow": "NonCash",
            "lines": [
                { "account": "4001", "side": "Debit", "amount": "1.00" },
                { "account": "simple_capital_reserve", "side": "Credit", "amount": "1.00" },
            ],
        }]));
        extra["next_event_id"] = serde_json::json!((next_source + 1).to_string());
    }
    let restored: Result<SimpleFinanceState, _> = serde_json::from_value(extra);
    assert!(
        restored.unwrap().validate().is_err(),
        "无对应回购事实的多余注销分录必须被恢复校验拒绝"
    );
}
