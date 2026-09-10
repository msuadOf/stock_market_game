//! 保险经营会计集成测试（company-information-npc-intentions W2-Task 10）。
//!
//! 政策基线：docs/company-accounting.md §2.4（CAS 25（2020）§11/§12/§20 合同
//! 分组、§21/§23–§26 履约现金流量三元组（未来现金流量估计 + 货币时间价值及
//! 金融风险调整 + 非金融风险调整）、§27/§28 初始确认与后续计量（CSM / 首日
//! 亏损 / 未到期责任负债 + 已发生赔款负债）、§29–§32 责任单元释放（保险服务
//! 收入/费用不含投资成分）、§33/§34 保险财务损益、§46–§49 亏损合同组、
//! §84/§85 列报 + CAS 30（2026）§55(二) 保险财务损益列报经营类别——官方依据
//! 均已核验，财会〔2020〕20号）。适用窗口：非境内外同时上市 2026-01-01 ——
//! 默认 2030 开局直接适用；更早开局为提前执行游戏假设。
//!
//! K3 红线：保费不立即全额计收入（收保费贷记未到期责任负债，随责任单元释放）；
//! 分红/投连/再保险合同一律类型化 UnsupportedContract；只有一般计量模型
//! （GMM）的明确期限非分红保障合同被支持。
//!
//! 金样单位约定：注释写「元」（1 元 = 100 分），执行值一律「分」
//! （AccountingAmount）。贴现率/风险调整为标注 **Fixture** 的合成游戏假设
//! （版本化显式配置），不声称真实保险精算参数。
//!
//! 手算公式（简单贴现，ACT/365F 整数）：PV = rhe(cents × 3_650_000 /
//! (3_650_000 + rate_bp × days))；CSM₀ = premium − PV(预期赔付) − 风险调整；
//! F（保险财务损益总额）= 预期赔付 − PV。释放按责任单元线性分摊 + 余数守恒。
//! 400bp × 365 天的分母 = 3_650_000 + 146_000 = 3_796_000。
//!
//! 按场景拆分：`gold`（盈利组/部分释放/亏损组）、`remeasure`（估计改变三向
//! 分流）、`claims`（赔案发生与支付分离 + 调节表对账）、`failures`（类型化
//! 拒绝 + 完整状态不变断言）。
//!
//! QA 入口：`cargo test -p engine --test insurance_accounting`（happy 与
//! failure 同命令覆盖）。

mod claims;
mod failures;
mod gold;
mod remeasure;

use engine::accounting::{AccountingAmount, JournalLine, LedgerAccountId, PostingSide};
use engine::calendar::CivilDate;
use engine::company::insurance::{insurance_chart_v4, DiscountAssumption, InsuranceConfig};
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

/// Fixture 贴现假设（**不声称真实精算利率**）：4%/年 单一利率，365 天按
/// ACT/365F 简单贴现。分母示例：3_650_000 + 400×365 = 3_796_000。
pub(crate) fn fixture_discount() -> DiscountAssumption {
    DiscountAssumption {
        version: 1,
        rate_bp: 400,
    }
}

/// 投保人（K2 外部对手方，非证券 NPC）。
pub(crate) fn counterparties() -> Vec<ExternalCounterparty> {
    vec![ExternalCounterparty {
        id: CounterpartyId("EXT-POL-1".to_string()),
        kind: engine::company::CounterpartyKind::Customer,
        name: "虚构投保人".to_string(),
    }]
}

pub(crate) fn policyholder() -> CounterpartyId {
    CounterpartyId("EXT-POL-1".to_string())
}

/// 基础配置：现金 2000 元 + 实收资本 2000 元（2030-01-01 开局，Fixture 贴现）。
pub(crate) fn base_config() -> InsuranceConfig {
    InsuranceConfig {
        chart: insurance_chart_v4(),
        as_of: d("2030-01-01"),
        opening_lines: vec![
            cent_line(acct::CASH, PostingSide::Debit, 200_000),
            cent_line(acct::CAPITAL, PostingSide::Credit, 200_000),
        ],
        counterparties: counterparties(),
        discount: fixture_discount(),
    }
}

/// 保险科目表 v4 常用科目代码（单一真源 = accounting::reports::insurance::codes）。
pub(crate) mod acct {
    pub const CASH: &str = "1002"; // 银行存款（现金类）
    pub const PREMIUM_RECEIVABLE: &str = "1122"; // 应收保费
    pub const LRC: &str = "2501"; // 未到期责任负债
    pub const LIC: &str = "2502"; // 已发生赔款负债
    pub const CAPITAL: &str = "4001"; // 实收资本
    pub const INSURANCE_REVENUE: &str = "6051"; // 保险服务收入
    pub const INSURANCE_EXPENSE: &str = "6451"; // 保险服务费用
    pub const INSURANCE_FINANCE: &str = "6541"; // 保险财务损益
}

/// 由账套读科目净借方余额（辅助断言）。
pub(crate) fn net_debit(
    insurance: &engine::company::insurance::InsuranceBooks,
    code: &str,
) -> AccountingAmount {
    insurance
        .books()
        .ledger()
        .account_net_debit(&LedgerAccountId(code.to_string()))
        .expect("ledger query")
}
