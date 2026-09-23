//! 任务 6（company-information-npc-intentions）：原子复式记账与科目总账底座集成测试。
//!
//! 政策基线：docs/company-accounting.md（K2/K3——借贷记账法、五要素、CAS 30 列报
//! 分类）与计划 K2 金额契约（checked i128 分、十进制字符串 serde、整数基点、
//! 合同累计余数、半偶舍入）。行业科目表落在任务 8–11，本套只用通用 v1 科目表。
//!
//! 金样单位约定（计划 Verification strategy）：注释与断言写「元」便于人读，
//! 执行值一律「分」。按场景拆分：`gold`（四大金样 + K2 主金样）、`amount_unit`
//! （金额算术/基点余数/serde 十进制字符串）、`failures`（类型化拒绝 + 完整状态
//! 不变断言）。
//!
//! QA 入口：`cargo test -p engine --test accounting`（happy 与 failure 同命令覆盖）。

mod amount_unit;
mod failures;
mod gold;

use engine::accounting::{
    AccountChart, AccountingAmount, Books, BusinessEventId, BusinessKind, CashFlowClass,
    JournalEntry, LedgerAccountId, PostingSide,
};
use engine::calendar::CivilDate;

/// 测试用 ISO 日期；输入本身必须合法（否则测试夹具写错）。
pub(crate) fn d(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("test fixture date must be valid ISO date")
}

/// 元 → AccountingAmount（分）。仅测试夹具用；执行值恒为分。
pub(crate) fn yuan(yuan_value: i128) -> AccountingAmount {
    AccountingAmount::from_cents(yuan_value.checked_mul(100).expect("fixture yuan overflow"))
}

/// 快速构造分录（金额参数单位 = 元）。字段直填：故意非法的分录是负向用例的输入面。
#[allow(clippy::too_many_arguments)]
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
            .map(|(account, side, amount)| engine::accounting::JournalLine {
                account: LedgerAccountId(account.to_string()),
                side: *side,
                amount: yuan(*amount),
            })
            .collect(),
    }
}

/// 通用 v1 科目表的空账套。
pub(crate) fn books() -> Books {
    Books::new(AccountChart::generic_v1())
}

/// 通用 v1 科目表常用科目代码（企业会计准则通用科目编号，docs/company-accounting.md §2.1）。
pub(crate) mod acct {
    pub const CASH: &str = "1002"; // 银行存款（现金类）
    pub const AR: &str = "1122"; // 应收账款
    pub const ACC_DEP: &str = "1602"; // 累计折旧（资产备抵）
    pub const LOAN: &str = "2001"; // 短期借款
    pub const PAYABLE: &str = "2202"; // 应付账款
    pub const TAX_PAYABLE: &str = "2221"; // 应交税费
    pub const INT_PAYABLE: &str = "2231"; // 应付利息
    pub const CAPITAL: &str = "4001"; // 实收资本
    pub const REVENUE: &str = "6001"; // 主营业务收入
    pub const OPEX: &str = "6602"; // 管理费用
    pub const FIN_EXP: &str = "6603"; // 财务费用
    pub const TAX_EXP: &str = "6801"; // 所得税费用
}

/// 科目表构造守卫（守卫可达性——每个错误变体都有触发路径与合法路径）。
#[test]
fn chart_construction_rejects_duplicates_and_empty_fields() {
    use engine::accounting::{AccountChart, AccountDef, AccountElement};

    let id = |code: &str| LedgerAccountId(code.to_string());
    let def = || AccountDef::new("测试科目", AccountElement::Asset);

    assert!(matches!(
        AccountChart::new(1, vec![(id("1001"), def()), (id("1001"), def())]),
        Err(engine::accounting::AccountingError::ChartInvalid { .. })
    ));
    assert!(matches!(
        AccountChart::new(1, vec![(id("  "), def())]),
        Err(engine::accounting::AccountingError::ChartInvalid { .. })
    ));
    assert!(matches!(
        AccountChart::new(
            1,
            vec![(id("1001"), AccountDef::new("", AccountElement::Asset))]
        ),
        Err(engine::accounting::AccountingError::ChartInvalid { .. })
    ));
    let ok = AccountChart::new(7, vec![(id("1001"), def().with_cash())]).expect("valid chart");
    assert_eq!(ok.version(), 7);
    assert!(ok.is_cash(&id("1001")));
    assert!(!ok.is_cash(&id("9999")));
}
