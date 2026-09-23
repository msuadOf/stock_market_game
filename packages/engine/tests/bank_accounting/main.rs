//! 银行经营会计集成测试（company-information-npc-intentions W2-Task 9）。
//!
//! 政策基线：docs/company-accounting.md §2.3（CAS 22（2017）§16/§17/§21/§38/
//! §39/§46–48/§57/§58/§60/§61/§63 + CAS 30（2026）§45–47/§16 银行列报——官方
//! 依据均已核验）。K3 红线：存款是负债不是收入；贷款发放是资产不是费用；
//! PD/LGD/EAD 为显式情景输入，绝不从股票跌幅推导信用损失。
//!
//! 金样单位约定：注释写「元」（1 元 = 100 分）便于人读，执行值一律「分」
//! （AccountingAmount）。ECL/利率参数全部为标注 **Fixture** 的合成游戏假设
//! （版本化显式配置），不声称真实银行参数。
//!
//! 按场景拆分：`gold`（存贷→利息→阶段转移→减值→核销→回收全链金样 + 报表
//! 分类层 + 存款到期停息）、`failures`（类型化拒绝 + 完整状态不变断言）。
//!
//! QA 入口：`cargo test -p engine --test bank_accounting`（happy 与 failure
//! 同命令覆盖）。

mod failures;
mod gold;

use engine::accounting::{AccountingAmount, JournalLine, LedgerAccountId, PostingSide};
use engine::calendar::CivilDate;
use engine::company::bank::{bank_chart_v3, BankConfig, EclPolicy, EclScenario};
use engine::company::{CounterpartyId, ExternalCounterparty};

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

/// Fixture 合成 ECL 政策（**不声称真实 PD/LGD**）：阶段 1 = 1%PD×50%LGD
/// （12 个月 ECL），存续期 = 8%PD×50%LGD。数值刻意取整便于手算。
pub(crate) fn fixture_ecl_policy() -> EclPolicy {
    EclPolicy {
        version: 1,
        stage1_default: vec![EclScenario {
            weight_bp: 10_000,
            pd_bp: 100,
            lgd_bp: 5_000,
        }],
        lifetime_default: vec![EclScenario {
            weight_bp: 10_000,
            pd_bp: 800,
            lgd_bp: 5_000,
        }],
    }
}

/// 两类银行客户：存款人 / 借款人（K2 外部对手方，非证券 NPC）。
pub(crate) fn counterparties() -> Vec<ExternalCounterparty> {
    vec![
        ExternalCounterparty {
            id: CounterpartyId("EXT-DEP-1".to_string()),
            kind: engine::company::CounterpartyKind::Customer,
            name: "虚构存款客户".to_string(),
        },
        ExternalCounterparty {
            id: CounterpartyId("EXT-BOR-1".to_string()),
            kind: engine::company::CounterpartyKind::Customer,
            name: "虚构借款客户".to_string(),
        },
    ]
}

/// 基础配置：现金 2000 元 + 实收资本 2000 元（2030-01-01 开局，Fixture ECL 政策）。
pub(crate) fn base_config() -> BankConfig {
    BankConfig {
        chart: bank_chart_v3(),
        as_of: d("2030-01-01"),
        opening_lines: vec![
            cent_line(acct::CASH, PostingSide::Debit, 200_000),
            cent_line(acct::CAPITAL, PostingSide::Credit, 200_000),
        ],
        counterparties: counterparties(),
        ecl_policy: fixture_ecl_policy(),
    }
}

/// 银行科目表 v3 常用科目代码（单一真源 = accounting::reports::bank::codes）。
pub(crate) mod acct {
    pub const CASH: &str = "1003"; // 存放中央银行款项（现金类）
    pub const LOAN_INT_RCV: &str = "1131"; // 应收利息（贷款）
    pub const LOAN_PRINCIPAL: &str = "1301"; // 贷款——本金
    pub const LOAN_ALLOWANCE: &str = "1303"; // 贷款减值准备（资产备抵）
    pub const ST_DEPOSIT: &str = "2011"; // 吸收存款——短期
    pub const LT_DEPOSIT: &str = "2601"; // 吸收存款——长期
    pub const DEP_INT_PAYABLE: &str = "2231"; // 应付利息（存款）
    pub const CAPITAL: &str = "4001"; // 实收资本
    pub const INTEREST_INCOME: &str = "6011"; // 利息收入
    pub const FEE_INCOME: &str = "6021"; // 手续费及佣金收入
    pub const INTEREST_EXPENSE: &str = "6411"; // 利息支出
    pub const CREDIT_IMPAIR: &str = "6701"; // 信用减值损失
}

/// 由账套读科目净借方余额（辅助断言）。
pub(crate) fn net_debit(bank: &engine::company::bank::BankBooks, code: &str) -> AccountingAmount {
    bank.books()
        .ledger()
        .account_net_debit(&LedgerAccountId(code.to_string()))
        .expect("ledger query")
}
