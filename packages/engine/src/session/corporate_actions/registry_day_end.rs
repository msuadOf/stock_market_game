//! 股东名册与个人股息税账的独立日终候选计算；尚未接入生产日终链。

use super::*;
use crate::calendar::CivilDate;
use crate::company::cash_dividend_tax::CashDividendTaxBook;
use crate::session::PersonalTradeConfirmation;
use crate::stock::{StockCatalog, StockId};

/// 输入必须是普通撮合与收盘集合竞价全部完成后的最终持仓事实。
pub(crate) struct RegistryDayEndInput<'a> {
    pub day: CivilDate,
    pub catalog: &'a StockCatalog,
    pub accounts: &'a BTreeMap<AccountId, BTreeMap<StockId, u64>>,
    pub confirmations: &'a BTreeMap<AccountId, Vec<PersonalTradeConfirmation>>,
    pub issuer_repurchase_account: Option<AccountId>,
    /// 仅 AShareIndividual 模式传入个人身份集合；其他模式传 None。
    pub late_holder_tax_accounts: Option<&'a BTreeSet<AccountId>>,
}

/// 仅包含本相位计算出的候选事实；调用者决定是否安装。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RegistryDayEndCandidate {
    pub registries: Vec<ShareRegistry>,
    pub dividend_tax_books: Vec<CashDividendTaxBook>,
}

impl SessionCorporateActions {
    pub(crate) fn registry_day_end_candidate(
        &self,
        _input: RegistryDayEndInput<'_>,
    ) -> Result<RegistryDayEndCandidate, SessionCorporateActionsError> {
        Err(SessionCorporateActionsError::Invalid("R1 尚未实现".into()))
    }
}

#[cfg(test)]
mod tests;
