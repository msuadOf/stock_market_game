//! 地产开发经营会计集成测试（company-information-npc-intentions W2-Task 11）。
//!
//! 政策基线：docs/company-accounting.md §2.5——预售收款先确认为负债
//! （CAS 14（2017）§39）、交付时点控制权转移确认收入并结转成本
//! （CAS 14 §4/§13，官方依据均已核验）；**借款费用资本化属版本化游戏假设**
//! （`game-assumption-borrowing-capitalization`：CAS 17（2006 批）原文两轮取证
//! 受阻 + 本任务 tfs 令/档通道一次补证仍 404，条款号一律不引用）。
//! K3 红线：预售不是交付收入；资本化必须暂停/终止，不把利息永远藏进资产。
//!
//! 金样单位约定：注释写「元」（1 元 = 100 分）便于人读，执行值一律「分」
//! （AccountingAmount）。利率/暂停阈值参数全部为标注 **Fixture** 的合成游戏
//! 假设（版本化显式配置），不声称真实地产参数或 CAS 17 合规。
//!
//! 按场景拆分：`gold`（购地→开发→预售→暂停（资本化中断）→交付→收尾全链
//! 金样 + 勾稽 + 列报 + serde 往返）、`failures`（类型化拒绝 + 完整状态不变）。
//!
//! QA 入口：`cargo test -p engine --test real_estate_accounting`（happy 与
//! failure 同命令覆盖）。

mod failures;
mod gold;

use engine::accounting::{AccountingAmount, JournalLine, LedgerAccountId, PostingSide};
use engine::calendar::CivilDate;
use engine::company::real_estate::{real_estate_chart_v5, CapitalizationPolicy, RealEstateConfig};
use engine::company::{CounterpartyId, ExternalCounterparty};
use engine::company::{CreditLine, OperatingBudget};

/// 测试用 ISO 日期；输入本身必须合法（否则测试夹具写错）。
pub(crate) fn d(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("test fixture date must be valid ISO date")
}

/// 元 → AccountingAmount（分）。
pub(crate) fn yuan(yuan_value: i128) -> AccountingAmount {
    AccountingAmount::from_cents(yuan_value.checked_mul(100).expect("fixture yuan overflow"))
}

/// 分 → AccountingAmount。
pub(crate) fn amt(cents: i128) -> AccountingAmount {
    AccountingAmount::from_cents(cents)
}

/// 快速构造开局分录行（金额单位 = 分）。
pub(crate) fn cent_line(code: &str, side: PostingSide, cents: i128) -> JournalLine {
    JournalLine {
        account: LedgerAccountId(code.to_string()),
        side,
        amount: AccountingAmount::from_cents(cents),
    }
}

/// 五类对手方：土地出让方 / 施工承包商 / 购房者 / 贷款人（K2 外部对手方）。
pub(crate) fn counterparties() -> Vec<ExternalCounterparty> {
    use engine::company::CounterpartyKind;
    vec![
        ExternalCounterparty {
            id: CounterpartyId("EXT-LAND-1".to_string()),
            kind: CounterpartyKind::Supplier,
            name: "虚构土地出让方".to_string(),
        },
        ExternalCounterparty {
            id: CounterpartyId("EXT-CON-1".to_string()),
            kind: CounterpartyKind::Supplier,
            name: "虚构施工承包商".to_string(),
        },
        ExternalCounterparty {
            id: CounterpartyId("EXT-BUY-1".to_string()),
            kind: CounterpartyKind::Customer,
            name: "虚构购房客户".to_string(),
        },
        ExternalCounterparty {
            id: CounterpartyId("EXT-BUY-2".to_string()),
            kind: CounterpartyKind::Customer,
            name: "虚构购房客户二".to_string(),
        },
        ExternalCounterparty {
            id: CounterpartyId("EXT-LEND-1".to_string()),
            kind: CounterpartyKind::Lender,
            name: "虚构项目贷款人".to_string(),
        },
    ]
}

/// Fixture 资本化政策（**游戏假设，不声称 CAS 17 合规**）：中断 ≥ 90 自然日
/// 暂停资本化。数值刻意取整便于手算。
pub(crate) fn fixture_capitalization_policy() -> CapitalizationPolicy {
    CapitalizationPolicy {
        version: 1,
        suspension_min_days: 90,
    }
}

/// 基础配置：现金 30000 元 + 实收资本 30000 元（2030-01-01 开局）、单公司
/// 项目数上限 2、贷款人授信 20000 元。
pub(crate) fn base_config() -> RealEstateConfig {
    RealEstateConfig {
        chart: real_estate_chart_v5(),
        as_of: d("2030-01-01"),
        opening_lines: vec![
            cent_line(acct::CASH, PostingSide::Debit, 3_000_000),
            cent_line(acct::CAPITAL, PostingSide::Credit, 3_000_000),
        ],
        counterparties: counterparties(),
        budget: OperatingBudget::new(
            AccountingAmount::ZERO,
            vec![CreditLine {
                lender: CounterpartyId("EXT-LEND-1".to_string()),
                limit: yuan(20_000),
            }],
        )
        .expect("fixture budget"),
        capitalization_policy: fixture_capitalization_policy(),
        max_projects: 2,
    }
}

/// 地产科目表 v5 常用科目代码（单一真源 = accounting::reports::real_estate::codes）。
pub(crate) mod acct {
    pub const CASH: &str = "1002"; // 银行存款（现金类）
    pub const AR: &str = "1122"; // 应收账款（尾款）
    pub const DEV_INVENTORY: &str = "1541"; // 开发存货
    pub const DEV_IMPAIR_ALLOW: &str = "1542"; // 开发存货减值准备（备抵）
    pub const CONTRACT_LIAB: &str = "2203"; // 合同负债（预售款）
    pub const LT_DEBT: &str = "2501"; // 长期借款
    pub const INT_PAYABLE: &str = "2231"; // 应付利息
    pub const CAPITAL: &str = "4001"; // 实收资本
    pub const REVENUE: &str = "6001"; // 主营业务收入
    pub const COGS: &str = "6401"; // 主营业务成本
    pub const FIN_EXP: &str = "6603"; // 财务费用（费用化利息）
    pub const IMPAIR_LOSS: &str = "6701"; // 资产减值损失
}

/// 由账套读科目净借方余额（辅助断言）。
pub(crate) fn net_debit(
    re: &engine::company::real_estate::RealEstateBooks,
    code: &str,
) -> AccountingAmount {
    re.books()
        .ledger()
        .account_net_debit(&LedgerAccountId(code.to_string()))
        .expect("ledger query")
}
