//! 共享夹具：分录构造助手 + 四行业金样账套 + 合并集团夹具。
//!
//! 金额单位：entry() 参数为「元」，账面执行值一律「分」（yuan() 换算）。

use engine::accounting::consolidation::{
    ConsolidationRequest, GroupMember, IntercompanyBalance, IntercompanySale, MemberId, MemberSpec,
};
use engine::accounting::reports::IndustryPresentation;
use engine::accounting::{
    AccountChart, AccountingPeriod, Books, BusinessEventId, BusinessKind, CashFlowClass,
    JournalEntry, JournalLine, LedgerAccountId, PostingSide,
};
use engine::calendar::CivilDate;
use std::collections::BTreeMap;

/// 共享空重述映射（无更正的生成请求）。
pub(crate) static NO_ADJUSTMENTS: BTreeMap<BusinessEventId, AccountingPeriod> = BTreeMap::new();

/// 测试用 ISO 日期；输入本身必须合法（否则夹具写错）。
pub(crate) fn d(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("test fixture date must be valid ISO date")
}

/// 元 → AccountingAmount（分）。
pub(crate) fn yuan(yuan_value: i128) -> engine::accounting::AccountingAmount {
    engine::accounting::AccountingAmount::from_cents(
        yuan_value.checked_mul(100).expect("fixture yuan overflow"),
    )
}

/// 快速构造分录（金额参数单位 = 元）。
pub(crate) fn entry(
    source: u64,
    date: &str,
    kind: BusinessKind,
    cash_flow: CashFlowClass,
    lines: &[(&str, PostingSide, i128)],
) -> JournalEntry {
    JournalEntry {
        source: BusinessEventId::new(source),
        date: d(date),
        kind,
        cash_flow,
        lines: lines
            .iter()
            .map(|(account, side, amount)| JournalLine {
                account: LedgerAccountId((*account).to_string()),
                side: *side,
                amount: yuan(*amount),
            })
            .collect(),
    }
}

/// 由科目表 + 分录清单构造账套（夹具分录必须全部可过账）。
pub(crate) fn books_with(chart: AccountChart, entries: Vec<JournalEntry>) -> Books {
    let mut books = Books::new(chart);
    books
        .post_batch(entries)
        .expect("fixture entries must post");
    books
}

// —— 行业账套夹具（分录均为手工可核对的整数元）——

/// 工业金样账套（科目表 v2）：2030-01..11 经营流 + 2029-12 开局。
///
/// 手算锚（元）：6 月月报现金 97,300 / 权益 102,595 / 累计净利 2,595；
/// 年报现金 100,275 / 净利 5,595 / 经营 CF 5,300 / 筹资 CF 4,975。
pub(crate) fn industrial_fixture() -> Books {
    use BusinessKind::*;
    use CashFlowClass::*;
    use PostingSide::{Credit, Debit};
    books_with(
        engine::company::industrial::industrial_chart_v2(),
        vec![
            entry(1, "2029-12-31", OpeningBalance, Financing, &[
                ("1002", Debit, 90_000), ("1601", Debit, 10_000), ("4001", Credit, 100_000)]),
            entry(2, "2030-01-15", CashRevenue, Operating, &[
                ("1002", Debit, 2_000), ("6001", Credit, 2_000)]),
            entry(3, "2030-02-10", CreditSale, NonCash, &[
                ("1122", Debit, 1_500), ("6001", Credit, 1_500)]),
            entry(4, "2030-02-20", ReceivableCollection, Operating, &[
                ("1002", Debit, 1_000), ("1122", Credit, 1_000)]),
            entry(5, "2030-03-15", Depreciation, NonCash, &[
                ("6602", Debit, 120), ("1602", Credit, 120)]),
            entry(6, "2030-04-10", CashExpense, Operating, &[
                ("6401", Debit, 700), ("1002", Credit, 700)]),
            entry(7, "2030-05-10", LoanDisbursement, Financing, &[
                ("1002", Debit, 5_000), ("2001", Credit, 5_000)]),
            entry(8, "2030-06-10", InterestAccrual, NonCash, &[
                ("6603", Debit, 25), ("2231", Credit, 25)]),
            entry(9, "2030-06-20", TaxAccrual, NonCash, &[
                ("6801", Debit, 60), ("222104", Credit, 60)]),
            entry(10, "2030-11-05", CashRevenue, Operating, &[
                ("1002", Debit, 3_000), ("6001", Credit, 3_000)]),
            entry(11, "2030-11-10", InterestPayment, Financing, &[
                ("2231", Debit, 25), ("1002", Credit, 25)]),
        ],
    )
}

/// 银行金样账套（科目表 v3）：存入/贷出/计息/手续费。
///
/// 手算锚（元）：6 月月报现金 52,040 / 贷款净额 6,060 / 吸收存款 8,000 /
/// 应付利息 15 / 累计净利 85（利息净 45 + 手续费 40）。
pub(crate) fn bank_fixture() -> Books {
    use BusinessKind::*;
    use CashFlowClass::*;
    use PostingSide::{Credit, Debit};
    books_with(
        engine::company::bank::bank_chart_v3(),
        vec![
            entry(1, "2029-12-31", OpeningBalance, Financing, &[
                ("1003", Debit, 50_000), ("4001", Credit, 50_000)]),
            entry(2, "2030-02-01", CustomerDeposit, Operating, &[
                ("1003", Debit, 8_000), ("2011", Credit, 8_000)]),
            entry(3, "2030-03-01", LoanIssued, Operating, &[
                ("1301", Debit, 6_000), ("1003", Credit, 6_000)]),
            entry(4, "2030-04-01", LoanInterestAccrued, NonCash, &[
                ("1131", Debit, 60), ("6011", Credit, 60)]),
            entry(5, "2030-05-01", DepositInterestAccrued, NonCash, &[
                ("6411", Debit, 15), ("2231", Credit, 15)]),
            entry(6, "2030-06-01", FeeAndCommissionEarned, Operating, &[
                ("1003", Debit, 40), ("6021", Credit, 40)]),
        ],
    )
}

/// 保险金样账套（科目表 v4）：保费挂账/收讫/服务释放/赔案。
///
/// 手算锚（元）：6 月月报现金 31,200 / 保险合同负债 1,000（LRC 300 + LIC 700）/
/// 累计净利 200（服务业绩）；6 月当月净利 −700（赔案）。
pub(crate) fn insurance_fixture() -> Books {
    use BusinessKind::*;
    use CashFlowClass::*;
    use PostingSide::{Credit, Debit};
    books_with(
        engine::company::insurance::insurance_chart_v4(),
        vec![
            entry(1, "2029-12-31", OpeningBalance, Financing, &[
                ("1002", Debit, 30_000), ("4001", Credit, 30_000)]),
            entry(2, "2030-02-01", InsurancePremiumAccrued, NonCash, &[
                ("1122", Debit, 1_200), ("2501", Credit, 1_200)]),
            entry(3, "2030-03-01", InsurancePremiumCollected, Operating, &[
                ("1002", Debit, 1_200), ("1122", Credit, 1_200)]),
            entry(4, "2030-05-01", InsuranceServiceRevenue, NonCash, &[
                ("2501", Debit, 900), ("6051", Credit, 900)]),
            entry(5, "2030-06-01", InsuranceClaimIncurred, NonCash, &[
                ("6451", Debit, 700), ("2502", Credit, 700)]),
        ],
    )
}

/// 地产金样账套（科目表 v5）：购地/预售/交付/尾款应收。
///
/// 手算锚（元）：6 月月报现金 36,000 / 开发存货 5,000 / 应收尾款 1,000 /
/// 累计净利 2,000（交付月）；6 月经营 CF 0（购地预售均在前月）。
pub(crate) fn real_estate_fixture() -> Books {
    use BusinessKind::*;
    use CashFlowClass::*;
    use PostingSide::{Credit, Debit};
    books_with(
        engine::company::real_estate::real_estate_chart_v5(),
        vec![
            entry(1, "2029-12-31", OpeningBalance, Financing, &[
                ("1002", Debit, 40_000), ("4001", Credit, 40_000)]),
            entry(2, "2030-02-01", LandAcquisition, Operating, &[
                ("1541", Debit, 9_000), ("1002", Credit, 9_000)]),
            entry(3, "2030-03-01", PresaleCollection, Operating, &[
                ("1002", Debit, 5_000), ("2203", Credit, 5_000)]),
            // 交付：冲合同负债 + 挂应收尾款 + 确认收入（单张分录，CAS 14 §13）。
            entry(4, "2030-06-01", RealEstateDelivery, NonCash, &[
                ("2203", Debit, 5_000), ("1122", Debit, 1_000), ("6001", Credit, 6_000)]),
            entry(5, "2030-06-01", RealEstateDelivery, NonCash, &[
                ("6401", Debit, 4_000), ("1541", Credit, 4_000)]),
        ],
    )
}

// —— 合并集团夹具（工业母公司 + 工业子公司 80%）——

pub(crate) const GROUP_ROOT: &str = "C-GROUP-P";
pub(crate) const GROUP_SUB: &str = "C-GROUP-S";

/// 母公司账套：开局/外购存货/对外销售/内部赊销（均 2030-03）。
pub(crate) fn group_parent_books() -> Books {
    use BusinessKind::*;
    use CashFlowClass::*;
    use PostingSide::{Credit, Debit};
    books_with(
        engine::company::industrial::industrial_chart_v2(),
        vec![
            entry(1, "2029-12-31", OpeningBalance, Financing, &[
                ("1002", Debit, 100_000), ("4001", Credit, 100_000)]),
            entry(2, "2030-03-05", CashExpense, Operating, &[
                ("1405", Debit, 2_400), ("1002", Credit, 2_400)]),
            entry(3, "2030-03-10", CashRevenue, Operating, &[
                ("1002", Debit, 2_000), ("6001", Credit, 2_000)]),
            entry(4, "2030-03-10", CashExpense, NonCash, &[
                ("6401", Debit, 1_200), ("1405", Credit, 1_200)]),
            entry(5, "2030-03-15", CreditSale, NonCash, &[
                ("1122", Debit, 1_000), ("6001", Credit, 1_000)]),
            entry(6, "2030-03-15", CashExpense, NonCash, &[
                ("6401", Debit, 600), ("1405", Credit, 600)]),
        ],
    )
}

/// 子公司账套：开局/内部购入/对外售出一半（均 2030-03）。
pub(crate) fn group_sub_books() -> Books {
    use BusinessKind::*;
    use CashFlowClass::*;
    use PostingSide::{Credit, Debit};
    books_with(
        engine::company::industrial::industrial_chart_v2(),
        vec![
            entry(1, "2029-12-31", OpeningBalance, Financing, &[
                ("1002", Debit, 20_000), ("4001", Credit, 20_000)]),
            entry(2, "2030-03-20", CreditSale, NonCash, &[
                ("1405", Debit, 1_000), ("2202", Credit, 1_000)]),
            entry(3, "2030-03-25", CashRevenue, Operating, &[
                ("1002", Debit, 800), ("6001", Credit, 800)]),
            entry(4, "2030-03-25", CashExpense, NonCash, &[
                ("6401", Debit, 500), ("1405", Credit, 500)]),
        ],
    )
}

/// 构造合并请求（内部往来 1,000 元 + 内部销售：转移价 1,000/成本 600/未售 500）。
pub(crate) fn group_request<'a>(
    parent: &'a Books,
    sub: &'a Books,
) -> ConsolidationRequest<'a> {
    ConsolidationRequest {
        root: MemberId(GROUP_ROOT.to_string()),
        members: vec![
            GroupMember {
                spec: MemberSpec {
                    id: MemberId(GROUP_ROOT.to_string()),
                    group_parent: None,
                    issued_shares: 100_000,
                    parent_held_shares: 0,
                },
                books: parent,
            },
            GroupMember {
                spec: MemberSpec {
                    id: MemberId(GROUP_SUB.to_string()),
                    group_parent: Some(MemberId(GROUP_ROOT.to_string())),
                    issued_shares: 10_000,
                    parent_held_shares: 8_000,
                },
                books: sub,
            },
        ],
        intercompany_balances: vec![
            // 资产侧（母公司应收）+ 负债侧（子公司应付）各申报一次——
            // 任务 12 往来抵销的镜像申报形态。
            IntercompanyBalance {
                member: MemberId(GROUP_ROOT.to_string()),
                counterparty: MemberId(GROUP_SUB.to_string()),
                account: LedgerAccountId("1122".to_string()),
                amount: yuan(1_000),
            },
            IntercompanyBalance {
                member: MemberId(GROUP_SUB.to_string()),
                counterparty: MemberId(GROUP_ROOT.to_string()),
                account: LedgerAccountId("2202".to_string()),
                amount: yuan(1_000),
            },
        ],
        intercompany_sales: vec![IntercompanySale {
            seller: MemberId(GROUP_ROOT.to_string()),
            buyer: MemberId(GROUP_SUB.to_string()),
            revenue_account: LedgerAccountId("6001".to_string()),
            cost_account: LedgerAccountId("6401".to_string()),
            inventory_account: LedgerAccountId("1405".to_string()),
            invoice_amount: yuan(1_000),
            cost_amount: yuan(600),
            unsold_inventory: yuan(500),
        }],
    }
}

/// 报表单体来源的行业标签快捷面。
pub(crate) fn standalone<'a>(
    id: &str,
    books: &'a Books,
    industry: IndustryPresentation,
) -> engine::accounting::reports::ReportSource<'a> {
    engine::accounting::reports::ReportSource::Standalone {
        id: MemberId(id.to_string()),
        books,
        industry,
    }
}
