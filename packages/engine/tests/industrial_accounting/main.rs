//! 工商经营与营运资金会计集成测试（company-information-npc-intentions W2-Task 8）。
//!
//! 政策基线：docs/company-accounting.md §2.1/§2.2（CAS 14 履约收入、CAS 22 §63
//! 整个存续期 ECL 简化法、财会〔2016〕22号增值税销项/进项分开核算）+ 计划 K3
//! （移动加权平均、直线折旧、当期+递延所得税与可抵扣亏损）。增值税/所得税税率
//! 在 docs §7 登记 `vat-law-current`/`cit-law-current` **取证受阻（blocked）**，
//! 故本套全部使用显式构造、标注为 Fixture 的合成税率，不声称真实参数。
//!
//! 金样单位约定：注释写「元」（1 元 = 100 分）便于人读，执行值一律「分」
//! （AccountingAmount）。BusinessKind 标签映射表见 src/company/industrial/mod.rs
//! 头注（journal.rs 属任务 6 语义冻结区，行业枚举扩充前以最接近的通用标签记录）。
//!
//! 按场景拆分：`chain_gold`（订单→生产→赊销→回款→结息全链金样）、`assets_gold`
//! （资本开支/折旧/减值）、`tax_gold`（增值税结算/当期+递延所得税）、`subledgers`
//! （共享子账单元）、`failures`（类型化拒绝 + 完整状态不变断言）。
//!
//! QA 入口：`cargo test -p engine --test industrial_accounting`（happy 与 failure
//! 同命令覆盖）。

mod assets_gold;
mod chain_gold;
mod failures;
mod subledgers;
mod tax_gold;

use engine::accounting::{AccountingAmount, JournalLine, LedgerAccountId, PostingSide, TaxPolicy};
use engine::calendar::CivilDate;
use engine::company::industrial::{industrial_chart_v2, IndustrialConfig};
use engine::company::{CreditLine, ExternalCounterparty, OperatingBudget};

/// 测试用 ISO 日期；输入本身必须合法（否则测试夹具写错）。
pub(crate) fn d(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("test fixture date must be valid ISO date")
}

/// 元 → AccountingAmount（分）。仅测试夹具用；执行值恒为分。
pub(crate) fn yuan(yuan_value: i128) -> AccountingAmount {
    AccountingAmount::from_cents(yuan_value.checked_mul(100).expect("fixture yuan overflow"))
}

/// 分 → AccountingAmount（小数金额断言用：如利息 8.43 元 = amt(843)）。
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

/// Fixture 合成税务政策（**不声称真实税率**：增值税 13%/13% 全抵扣、所得税 25%、
/// 亏损结转 5 年——数值刻意取整便于手算，docs §7 两条税法依据仍处 blocked）。
pub(crate) fn fixture_policy() -> TaxPolicy {
    TaxPolicy {
        version: 1,
        vat: engine::accounting::VatPolicy {
            output_rate_bp: 1_300,
            input_rate_bp: 1_300,
            deductible_share_bp: 10_000,
        },
        income_tax: engine::accounting::IncomeTaxPolicy {
            rate_bp: 2_500,
            loss_carryforward_years: 5,
        },
    }
}

/// 三类外部商业对手方（供应商 / 客户 / 贷款银行）。
pub(crate) fn counterparties() -> Vec<ExternalCounterparty> {
    use engine::company::CounterpartyId;
    vec![
        ExternalCounterparty {
            id: CounterpartyId("EXT-SUPP".to_string()),
            kind: engine::company::CounterpartyKind::Supplier,
            name: "虚构供应商".to_string(),
        },
        ExternalCounterparty {
            id: CounterpartyId("EXT-CUST".to_string()),
            kind: engine::company::CounterpartyKind::Customer,
            name: "虚构客户".to_string(),
        },
        ExternalCounterparty {
            id: CounterpartyId("EXT-BANK".to_string()),
            kind: engine::company::CounterpartyKind::Lender,
            name: "虚构合作银行".to_string(),
        },
    ]
}

/// 贷款人授信 5000 元、现金下限 0（硬下限由账套负现金守卫强制）。
pub(crate) fn bank_credit_budget() -> OperatingBudget {
    OperatingBudget::new(
        AccountingAmount::ZERO,
        vec![CreditLine {
            lender: engine::company::CounterpartyId("EXT-BANK".to_string()),
            limit: yuan(5_000),
        }],
    )
    .expect("fixture budget valid")
}

/// 无开局借款的基础配置：现金 10000 元 + 实收资本 10000 元。
pub(crate) fn base_config() -> IndustrialConfig {
    IndustrialConfig {
        chart: industrial_chart_v2(),
        as_of: d("2029-12-31"),
        opening_lines: vec![
            cent_line("1002", PostingSide::Debit, 1_000_000),
            cent_line("4001", PostingSide::Credit, 1_000_000),
        ],
        opening_inventory: Vec::new(),
        opening_assets: Vec::new(),
        opening_debt: None,
        counterparties: counterparties(),
        budget: bank_credit_budget(),
        tax_policy: fixture_policy(),
    }
}

/// 工业科目表 v2 常用科目代码（v1 通用科目 + 工业扩充，见 industrial_chart_v2）。
pub(crate) mod acct {
    pub const BANK: &str = "1002"; // 银行存款（现金类）
    pub const AR: &str = "1122"; // 应收账款
    pub const BAD_DEBT_ALLOW: &str = "1231"; // 坏账准备（资产备抵）
    pub const RAW: &str = "1403"; // 原材料
    pub const FINISHED: &str = "1405"; // 库存商品
    pub const FIXED_ASSET: &str = "1601"; // 固定资产
    pub const ACC_DEP: &str = "1602"; // 累计折旧（资产备抵）
    pub const ACC_IMPAIR: &str = "1603"; // 固定资产减值准备（资产备抵）
    pub const DTA: &str = "1811"; // 递延所得税资产
    pub const ST_DEBT: &str = "2001"; // 短期借款
    pub const PAYABLE: &str = "2202"; // 应付账款
    pub const VAT_OUT: &str = "222101"; // 应交增值税—销项税额
    pub const VAT_IN: &str = "222102"; // 应交增值税—进项税额
    pub const CIT_PAYABLE: &str = "222104"; // 应交所得税
    pub const INT_PAYABLE: &str = "2231"; // 应付利息
    pub const CAPITAL: &str = "4001"; // 实收资本
    pub const WIP: &str = "5001"; // 生产成本（在产品）
    pub const REVENUE: &str = "6001"; // 主营业务收入
    pub const ADMIN_EXP: &str = "6602"; // 管理费用
    pub const FIN_EXP: &str = "6603"; // 财务费用
    pub const IMPAIR_LOSS: &str = "6701"; // 资产减值损失
    pub const TAX_EXP: &str = "6801"; // 所得税费用
}

/// 由账套读科目净借方余额（辅助断言）。
pub(crate) fn net_debit(
    company: &engine::company::industrial::IndustrialBooks,
    code: &str,
) -> AccountingAmount {
    company
        .books()
        .ledger()
        .account_net_debit(&LedgerAccountId(code.to_string()))
        .expect("ledger query")
}
