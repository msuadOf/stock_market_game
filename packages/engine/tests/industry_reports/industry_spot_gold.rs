//! 银行/保险/地产金样 + 窗口化行与任务 9–11 列报分类层的一致性对照。

use crate::fixture::{bank_fixture, insurance_fixture, real_estate_fixture, standalone};
use engine::accounting::reports::bank::bank_presentation_lines;
use engine::accounting::reports::{
    generate_report_set, BsLine, Comparative, IncomeLine, IndustryPresentation, ReportKind,
    ReportRequest, ReportSource, ReportVersion, UnavailableReason, VersionKind,
};
use engine::accounting::{AccountingAmount, AccountingPeriod, BusinessEventId};
use std::collections::BTreeMap;

static EMPTY_ADJUSTMENTS: BTreeMap<BusinessEventId, AccountingPeriod> = BTreeMap::new();

fn yuan(v: i128) -> AccountingAmount {
    AccountingAmount::from_cents(v * 100)
}

fn request<'a>(source: ReportSource<'a>) -> ReportRequest<'a> {
    ReportRequest {
        period: AccountingPeriod::from_iso("2030-06").expect("period"),
        kind: ReportKind::Monthly,
        source,
        version: ReportVersion {
            sequence: 1,
            supersedes: None,
            kind: VersionKind::Original,
        },
        adjustments: &EMPTY_ADJUSTMENTS,
    }
}

fn bs_amount(set: &engine::accounting::reports::ReportSet, line: BsLine) -> AccountingAmount {
    set.balance_sheet
        .asset_lines
        .iter()
        .chain(set.balance_sheet.liability_lines.iter())
        .chain(set.balance_sheet.equity_lines.iter())
        .find(|(l, _)| *l == line)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("line {line:?} missing"))
}

fn is_amount(
    set: &engine::accounting::reports::ReportSet,
    line: IncomeLine,
) -> AccountingAmount {
    let columns = [&set.income.quarter, &set.income.cumulative];
    for col in columns {
        for (l, v) in col
            .operating
            .iter()
            .chain(col.financing.iter())
            .chain(col.investing.iter())
        {
            if *l == line {
                return *v;
            }
        }
    }
    panic!("income line {line:?} missing")
}

/// 银行 6 月月报：利息净收入/手续费/贷款净额/吸收存款 + 间接法。
#[test]
fn bank_june_gold() {
    let books = bank_fixture();
    let set = generate_report_set(request(standalone("C-BANK", &books, IndustryPresentation::Bank)))
        .expect("bank report must generate");
    set.validate().expect("bank set must cross-foot");

    // 累计（1–6 月）：利息收入 60 / 支出 15 / 净 45 / 手续费 40 ⇒ 净利 85。
    let cum = &set.income.cumulative;
    assert_eq!(is_amount(&set, IncomeLine::InterestIncome), yuan(60));
    assert_eq!(is_amount(&set, IncomeLine::InterestExpense), yuan(15));
    assert_eq!(is_amount(&set, IncomeLine::NetInterestIncome), yuan(45));
    assert_eq!(is_amount(&set, IncomeLine::FeeAndCommissionIncome), yuan(40));
    assert_eq!(cum.net_income, yuan(85));

    // 资产负债表：现金 52,040 / 贷款净额 6,060（本金 6,000 + 应计 60）/ 存款 8,000。
    assert_eq!(bs_amount(&set, BsLine::CashFunds), yuan(52_040));
    assert_eq!(bs_amount(&set, BsLine::LoansAndAdvances), yuan(6_060));
    assert_eq!(bs_amount(&set, BsLine::CustomerDeposits), yuan(8_000));
    assert_eq!(bs_amount(&set, BsLine::InterestPayable), yuan(15));
    assert_eq!(set.balance_sheet.total_assets, yuan(58_100));

    // 现金流量：6 月窗口经营 +40（手续费收现）；间接法 = 净利 40。
    let cf = &set.cash_flow;
    assert_eq!(cf.operating, yuan(40));
    assert_eq!(cf.indirect[0].amount, yuan(40));
    let sum: i128 = cf.indirect.iter().map(|l| l.amount.cents()).sum();
    assert_eq!(sum, 4_000);

    // 上年同期不可得（2029 无流量）。
    assert!(matches!(
        set.income.prior_year,
        Comparative::Unavailable {
            reason: UnavailableReason::NoPriorYearHistory
        }
    ));

    // 与任务 9 列报分类层的一致性：全期间窗口 == 期末窗口（6 月后无分录）。
    let presentation = bank_presentation_lines(books.ledger()).expect("presentation lines");
    assert_eq!(bs_amount(&set, BsLine::LoansAndAdvances), presentation.loans_and_advances_net);
    assert_eq!(bs_amount(&set, BsLine::CashFunds), presentation.cash_position);
    assert_eq!(bs_amount(&set, BsLine::CustomerDeposits), presentation.customer_deposits);
    assert_eq!(is_amount(&set, IncomeLine::NetInterestIncome), presentation.net_interest_income);
    assert_eq!(bs_amount(&set, BsLine::InterestPayable), presentation.deposit_interest_payable);
}

/// 保险 6 月月报：保险服务业绩 + 合同负债 + 当月赔案。
#[test]
fn insurance_june_gold() {
    let books = insurance_fixture();
    let set = generate_report_set(request(standalone(
        "C-INS",
        &books,
        IndustryPresentation::Insurance,
    )))
    .expect("insurance report must generate");
    set.validate().expect("insurance set must cross-foot");

    // 累计：服务收入 900 − 服务费用 700 = 业绩 200；当月 = −700（赔案）。
    assert_eq!(is_amount(&set, IncomeLine::InsuranceRevenue), yuan(900));
    assert_eq!(is_amount(&set, IncomeLine::InsuranceServiceExpense), yuan(700));
    assert_eq!(is_amount(&set, IncomeLine::InsuranceServiceResult), yuan(200));
    assert_eq!(set.income.cumulative.net_income, yuan(200));
    assert_eq!(set.income.quarter.net_income, yuan(200));

    // 资产负债表：应收保费 0（已收讫）/ 保险合同负债 1,000（LRC 300 + LIC 700）。
    assert_eq!(bs_amount(&set, BsLine::InsuranceReceivables), AccountingAmount::ZERO);
    assert_eq!(bs_amount(&set, BsLine::InsuranceContractLiabilities), yuan(1_000));
    assert_eq!(bs_amount(&set, BsLine::CashFunds), yuan(31_200));
    assert_eq!(set.balance_sheet.total_assets, yuan(31_200));

    // 现金流：6 月全非现金 ⇒ 三类皆 0；间接法 −700 + 赔款负债 +700 = 0。
    let cf = &set.cash_flow;
    assert_eq!(cf.operating, AccountingAmount::ZERO);
    let sum: i128 = cf.indirect.iter().map(|l| l.amount.cents()).sum();
    assert_eq!(sum, 0);
}

/// 地产 6 月月报：交付月收入成本 + 开发存货净额 + 合同负债冲平。
#[test]
fn real_estate_june_gold() {
    let books = real_estate_fixture();
    let set = generate_report_set(request(standalone(
        "C-RE",
        &books,
        IndustryPresentation::RealEstate,
    )))
    .expect("real estate report must generate");
    set.validate().expect("real estate set must cross-foot");

    assert_eq!(is_amount(&set, IncomeLine::OperatingRevenue), yuan(6_000));
    assert_eq!(is_amount(&set, IncomeLine::OperatingCost), yuan(4_000));
    assert_eq!(set.income.cumulative.net_income, yuan(2_000));

    assert_eq!(bs_amount(&set, BsLine::DevelopmentInventory), yuan(5_000));
    assert_eq!(bs_amount(&set, BsLine::Receivables), yuan(1_000));
    assert_eq!(bs_amount(&set, BsLine::ContractLiabilities), AccountingAmount::ZERO);
    assert_eq!(bs_amount(&set, BsLine::CashFunds), yuan(36_000));
    assert_eq!(set.balance_sheet.total_assets, yuan(42_000));

    // 现金流：6 月交付非现金 ⇒ 0；间接法 2,000 − 应收 1,000 − 存货 5,000 + 合同负债 5,000 = 1,000？
    // 复核：NI 2,000；非现金非损益科目 6 月运动 = 应收 +1,000 / 存货 −4,000 / 合同负债 +5,000(借)
    // ⇒ −Σm = −1,000 +4,000 −5,000 = −2,000 ⇒ 间接法 0 ✓（下方以总和断言）。
    let cf = &set.cash_flow;
    assert_eq!(cf.operating, AccountingAmount::ZERO);
    let sum: i128 = cf.indirect.iter().map(|l| l.amount.cents()).sum();
    assert_eq!(sum, 0);
    assert_eq!(cf.closing_cash, yuan(36_000));
}
