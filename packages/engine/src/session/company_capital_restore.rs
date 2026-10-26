//! selected CompanySystem 的已提交资本恢复验证；不创建或修改持久事实。

use crate::Money;
use crate::accounting::AccountingAmount;
use crate::calendar::{CivilDate, TradingCalendar};
use crate::company::identity::IssuerRegistry;
use crate::company::{CompanyId, CompanySystem, CompanySystemError};
use crate::session::SessionCorporateActions;
use crate::stock::{StockCatalog, StockId};
use std::collections::BTreeMap;

/// 日期校验借用本局冻结 policy，不在恢复时另建默认日历。
pub(crate) struct CapitalRestoreInput<'a> {
    pub(crate) setup_issuers: &'a IssuerRegistry,
    pub(crate) system: &'a CompanySystem,
    pub(crate) actions: &'a SessionCorporateActions,
    pub(crate) catalog: &'a StockCatalog,
    pub(crate) calendar: &'a TradingCalendar,
    pub(crate) committed_through: CivilDate,
    pub(crate) initial_par: Option<Money>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) enum CorporateCapitalKind {
    PublicMarket,
    StockDistribution,
    RightsOffering,
    ShareReDenomination,
    IssuerRepurchaseCancellation,
}

/// 数值身份与原 receipt 下标都保留；错误文本不承担跨层分类职责。
#[derive(Clone, Debug, Eq, PartialEq, Default)]
pub(crate) struct CapitalRestoreContext {
    pub(crate) company: Option<CompanyId>,
    pub(crate) stock: Option<StockId>,
    pub(crate) kind: Option<CorporateCapitalKind>,
    pub(crate) event_id: Option<String>,
    pub(crate) receipt_index: Option<usize>,
    pub(crate) day: Option<CivilDate>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CompanyCapitalRestoreError {
    #[error("资本恢复验证尚未实现")]
    NotImplemented,
    #[error("资本恢复公司事实校验失败：{context:?}：{source}")]
    System {
        context: CapitalRestoreContext,
        #[source]
        source: CompanySystemError,
    },
    #[error("资本恢复稳定身份不一致：公司 {company:?}，字段 {field}")]
    StableIdentity {
        company: CompanyId,
        field: &'static str,
    },
    #[error("资本恢复发行股数不一致：{context:?}，预期 {expected} 股，实际 {actual} 股")]
    IssuedShares {
        context: CapitalRestoreContext,
        expected: u64,
        actual: u64,
    },
    #[error("资本恢复名册校验失败：{context:?}：{source}")]
    Registry {
        context: CapitalRestoreContext,
        #[source]
        source: crate::company::share_registry::ShareRegistryError,
    },
    #[error("资本恢复缺少名册：{context:?}")]
    MissingRegistry { context: CapitalRestoreContext },
    #[error("资本恢复存在未绑定动作的名册回执：{context:?}")]
    UnboundReceipt { context: CapitalRestoreContext },
    #[error("资本恢复已发生事实晚于提交截止 {committed_through}：{context:?}")]
    FutureFact {
        context: CapitalRestoreContext,
        committed_through: CivilDate,
    },
    #[error("资本恢复输入身份重复：{context:?}")]
    Duplicate { context: CapitalRestoreContext },
    #[error("资本恢复证券目录映射无效：公司 {company:?}，证券 {stock_code}")]
    CatalogMapping {
        company: Option<CompanyId>,
        stock_code: String,
    },
    #[error("资本恢复算术不能表示合法股本：{context:?}")]
    Arithmetic { context: CapitalRestoreContext },
    #[error("资本恢复所需动作验证尚未实现：{context:?}")]
    UnsupportedMovement { context: CapitalRestoreContext },
}

/// 验证结果是恢复过程的派生结论，不进入 SaveSlot。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompanyCapitalReplay {
    pub(crate) stock: Option<StockId>,
    pub(crate) issued_shares: u64,
    pub(crate) receipt_frontier: usize,
    pub(crate) registered_capital: Option<AccountingAmount>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CapitalReplaySummary {
    pub(crate) companies: BTreeMap<CompanyId, CompanyCapitalReplay>,
}

pub(crate) fn validate_company_capital_restore(
    _input: CapitalRestoreInput<'_>,
) -> Result<CapitalReplaySummary, CompanyCapitalRestoreError> {
    Err(CompanyCapitalRestoreError::NotImplemented)
}

#[cfg(test)]
#[path = "company_capital_restore_tests.rs"]
mod tests;
