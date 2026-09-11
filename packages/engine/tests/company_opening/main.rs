//! 公司实体与开局账套集成测试（company-information-npc-intentions W1-Task 7）。
//!
//! K2 资金边界：公司经营资金只存在于公司账套（accounting::Books），外部商业
//! 对手方用独立 `CounterpartyId`；投资者交易 `Account` 与公司账套互不复用。
//! 开局账套由显式平衡的 OpeningBalance 凭证构成，不依据初始股价或旧 V 反推
//! 资产。默认 5 股票仅新增发行人映射与虚构工商配置，不改变交易类别/股本。
//!
//! 默认 5 股票交易规格表是 `apps/web/src/config/defaults.ts` STOCK_SPECS 的
//! 逐字段副本（W1-Task 1 先例：外部真源副本必须被测试钉住防漂移）。
//!
//! 金样单位约定：注释写「元」便于人读，执行值一律「分」（AccountingAmount）。
//! 按场景拆分：`balanced`（四种独立测试实体 + 默认映射金样）、`isolation`
//! （无 NPC 支付 + 交易规格不变）、`failures`（类型化拒绝 + 完整状态不变）。
//!
//! QA 入口：`cargo test -p engine --test company_opening`（happy 与 failure
//! 同命令覆盖）。

mod balanced;
mod failures;
mod isolation;

use engine::account::StockCode;
use engine::accounting::{AccountingAmount, LedgerAccountId, PostingSide};
use engine::calendar::CivilDate;
use engine::company::{
    default_companies, CompanyConfig, CompanyId, CompanyKind, CompanyOpening, CompanySpec,
    CounterpartyKind, ExternalCounterparty, IndustryId, OpeningLine, OperatingBudget,
};
use engine::money::Money;
use engine::session::{SecurityCategory, StockExchange, StockSpec};

/// 默认开局账套锚点日：2030-01-01 开局（政策 v1 默认起点）的前一自然日。
pub(crate) const OPENING_AS_OF: &str = "2029-12-31";

/// 测试用 ISO 日期；输入本身必须合法（否则测试夹具写错）。
pub(crate) fn d(iso: &str) -> CivilDate {
    CivilDate::from_iso(iso).expect("test fixture date must be valid ISO date")
}

/// 元 → AccountingAmount（分）。仅测试夹具用；执行值恒为分。
pub(crate) fn yuan(yuan_value: i128) -> AccountingAmount {
    AccountingAmount::from_cents(yuan_value.checked_mul(100).expect("fixture yuan overflow"))
}

/// 快速构造开局分录行（金额单位 = 元）。
pub(crate) fn opening_line(code: &str, side: PostingSide, yuan_amount: i128) -> OpeningLine {
    OpeningLine {
        account: LedgerAccountId(code.to_string()),
        side,
        amount: yuan(yuan_amount),
    }
}

/// 通用 v1 科目表上的开局账套（行业科目表在任务 8–11 以新版本扩充）。
pub(crate) fn generic_opening(lines: Vec<OpeningLine>) -> CompanyOpening {
    CompanyOpening::generic_chart(d(OPENING_AS_OF), lines)
}

/// 未上市测试实体规格（id/会计类型/股本由调用处指定）。
pub(crate) fn unlisted_spec(id: &str, kind: CompanyKind, issued_shares: u64) -> CompanySpec {
    CompanySpec {
        id: CompanyId(id.to_string()),
        name: format!("虚构测试实体 {id}"),
        industry: IndustryId("machinery".to_string()),
        kind,
        listed_stock: None,
        issued_shares,
        group_parent: None,
    }
}

/// 最小合法预算：现金下限 100 万元、无授信。
pub(crate) fn empty_budget() -> OperatingBudget {
    OperatingBudget::new(yuan(1_000_000), Vec::new()).expect("minimal budget is valid")
}

/// 无对手方、无授信的最小公司配置。
pub(crate) fn bare_config(spec: CompanySpec, opening: CompanyOpening) -> CompanyConfig {
    CompanyConfig {
        spec,
        opening,
        counterparties: Vec::new(),
        budget: empty_budget(),
    }
}

/// 默认 5 股票交易规格（apps/web/src/config/defaults.ts STOCK_SPECS 逐字段副本）。
/// tick = 1 分，与 defaults.ts mkSpec 一致。
pub(crate) fn default_stock_specs() -> Vec<StockSpec> {
    fn spec(
        code: &str,
        exchange: StockExchange,
        initial_price_cents: i64,
        category: SecurityCategory,
        total_shares: u64,
        float_shares: u32,
    ) -> StockSpec {
        StockSpec {
            code: StockCode(code.to_string()),
            exchange,
            initial_price: Money::from_cents(initial_price_cents),
            category,
            limit_pct: if matches!(category, SecurityCategory::ChiNext) {
                0.20
            } else {
                0.10
            },
            tick: Money::from_cents(1),
            total_shares,
            float_shares,
        }
    }
    Vec::from([
        // 稳健实业 11.20 元 / 主板 10%
        spec(
            "600101",
            StockExchange::Shanghai,
            1120,
            SecurityCategory::MainBoard,
            8_928_571_429,
            3_571_428_571,
        ),
        // 芯片科技 27.35 元 / 主板 10%
        spec(
            "002156",
            StockExchange::Shenzhen,
            2735,
            SecurityCategory::MainBoard,
            2_925_045_704,
            2_047_531_993,
        ),
        // 短线题材 36.80 元 / 创业板 20%
        spec(
            "300260",
            StockExchange::Shenzhen,
            3680,
            SecurityCategory::ChiNext,
            815_217_391,
            611_413_043,
        ),
        // 人气妖股 7.55 元 / 主板 10%
        spec(
            "600610",
            StockExchange::Shanghai,
            755,
            SecurityCategory::MainBoard,
            1_059_602_649,
            847_682_119,
        ),
        // ST 低价股 2.85 元 / 主板风险警示 10%
        spec(
            "000812",
            StockExchange::Shenzhen,
            285,
            SecurityCategory::StMainBoard,
            1_052_631_579,
            842_105_263,
        ),
    ])
}

/// 默认注册表：5 家上市工商公司（映射默认 5 股票）+ 4 家未上市独立测试实体。
pub(crate) fn default_registry() -> engine::company::CompanyRegistry {
    engine::company::CompanyRegistry::new(
        default_companies(d(OPENING_AS_OF)).expect("default fixture valid"),
    )
    .expect("default company set must construct")
}

/// 600101 发行人对手方快照的期望：至少一个贷款人（开局短期借款的对端）。
pub(crate) fn lender_counterparty(id: &str, name: &str) -> ExternalCounterparty {
    ExternalCounterparty {
        id: engine::company::CounterpartyId(id.to_string()),
        kind: CounterpartyKind::Lender,
        name: name.to_string(),
    }
}
