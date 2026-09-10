//! 应收/应付开项子账（共享，K3）：账龄、部分核销、逾期与坏账核销面。
//!
//! 同一结构服务应收与应付两侧（方向由调用方语义决定）。回款/付款**只清算开项**
//! ——不确认收入（收入在履约时一次确认，CAS 14 §4/§13 已核验）；超额核销是
//! 类型化拒绝。坏账准备目标值 = 开项余额 × 政策比率（**整个存续期 ECL 简化法**，
//! CAS 22（2017）第六十三条——官方依据已核验，docs/company-accounting.md §2.2；
//! 不做三阶段滚动，属简化选择而非准则缺失）。核销要求准备足以覆盖。
//!
//! 行业中立：bank 收息应收、real-estate 尾款（任务 9/11）复用同一开项轨迹。

use std::collections::BTreeMap;

use crate::accounting::amount::AccountingAmount;
use crate::accounting::error::AccountingError;
use crate::calendar::CivilDate;
use thiserror::Error;

/// 开项 id newtype（业务事件分配，保证唯一，如 `AR-{event}`）。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct OpenItemId(pub String);

/// 单个未结开项：对手方引用（透明字符串）+ 开立/到期日 + 未结金额。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct OpenItem {
    party: String,
    opened_on: CivilDate,
    due_on: CivilDate,
    open_amount: AccountingAmount,
}

impl OpenItem {
    pub fn party(&self) -> &str {
        &self.party
    }

    pub fn opened_on(&self) -> CivilDate {
        self.opened_on
    }

    pub fn due_on(&self) -> CivilDate {
        self.due_on
    }

    pub fn open_amount(&self) -> AccountingAmount {
        self.open_amount
    }
}

/// 开项子账（应收侧/应付侧各一）：未结项 + 核销合计（历史事实）。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct TradeOpenLedger {
    items: BTreeMap<OpenItemId, OpenItem>,
    written_off_total: AccountingAmount,
}

impl TradeOpenLedger {
    /// 开立开项：正金额、到期日不早于开立日、id 唯一。
    pub fn open(
        &mut self,
        id: OpenItemId,
        party: &str,
        opened_on: CivilDate,
        due_on: CivilDate,
        amount: AccountingAmount,
    ) -> Result<(), TradeLedgerError> {
        if !amount.is_positive() {
            return Err(TradeLedgerError::NonPositiveAmount { amount });
        }
        if due_on < opened_on {
            return Err(TradeLedgerError::DueBeforeOpen { opened_on, due_on });
        }
        if self.items.contains_key(&id) {
            return Err(TradeLedgerError::DuplicateItem { id });
        }
        self.items.insert(
            id,
            OpenItem {
                party: party.to_string(),
                opened_on,
                due_on,
                open_amount: amount,
            },
        );
        Ok(())
    }

    /// 核销前纯校验（回款/付款共用）：存在、未结清、正金额且不超额。
    pub fn check_apply(
        &self,
        id: &OpenItemId,
        amount: AccountingAmount,
    ) -> Result<(), TradeLedgerError> {
        let item = self.item(id)?;
        if item.open_amount.is_zero() {
            return Err(TradeLedgerError::ItemCleared { id: id.clone() });
        }
        if !amount.is_positive() {
            return Err(TradeLedgerError::NonPositiveAmount { amount });
        }
        if amount > item.open_amount {
            return Err(TradeLedgerError::OverApplication {
                id: id.clone(),
                requested: amount,
                open: item.open_amount,
            });
        }
        Ok(())
    }

    /// 核销（回款/付款）：只减开项金额，返回剩余未结。**不产生任何收入**。
    pub fn apply(
        &mut self,
        id: &OpenItemId,
        amount: AccountingAmount,
    ) -> Result<AccountingAmount, TradeLedgerError> {
        self.check_apply(id, amount)?;
        let item = self
            .items
            .get_mut(id)
            .expect("check_apply validated existence");
        item.open_amount = item
            .open_amount
            .sub(amount)
            .map_err(TradeLedgerError::Accounting)?;
        Ok(item.open_amount)
    }

    /// 坏账核销：要求未结金额为正，整项清零并计入核销合计（准备消耗由调用方过账）。
    pub fn write_off(&mut self, id: &OpenItemId) -> Result<AccountingAmount, TradeLedgerError> {
        let item = self.item(id)?;
        if item.open_amount.is_zero() {
            return Err(TradeLedgerError::ItemCleared { id: id.clone() });
        }
        let amount = item.open_amount;
        let item = self
            .items
            .get_mut(id)
            .expect("item existence re-checked above");
        item.open_amount = AccountingAmount::ZERO;
        self.written_off_total = self
            .written_off_total
            .add(amount)
            .map_err(TradeLedgerError::Accounting)?;
        Ok(amount)
    }

    pub fn open_amount(&self, id: &OpenItemId) -> Result<AccountingAmount, TradeLedgerError> {
        Ok(self.item(id)?.open_amount)
    }

    pub fn total_open(&self) -> Result<AccountingAmount, TradeLedgerError> {
        let mut total = AccountingAmount::ZERO;
        for item in self.items.values() {
            total = total
                .add(item.open_amount)
                .map_err(TradeLedgerError::Accounting)?;
        }
        Ok(total)
    }

    pub fn written_off_total(&self) -> Result<AccountingAmount, TradeLedgerError> {
        Ok(self.written_off_total)
    }

    /// 逾期开项（到期日 < as_of 且未结清）——「Overdue」业务状态的数据面。
    pub fn overdue(&self, as_of: CivilDate) -> Vec<(&OpenItemId, &OpenItem)> {
        self.items
            .iter()
            .filter(|(_, item)| item.due_on < as_of && !item.open_amount.is_zero())
            .collect()
    }

    /// 账龄桶（按开立日至 as_of 的自然日）：[≤30, 31–60, 61–90, >90]；仅统计未结项。
    pub fn aging_buckets(
        &self,
        as_of: CivilDate,
    ) -> Result<[AccountingAmount; 4], TradeLedgerError> {
        let mut buckets = [AccountingAmount::ZERO; 4];
        for item in self.items.values() {
            if item.open_amount.is_zero() {
                continue;
            }
            let days = as_of.days_since(item.opened_on).max(0);
            let index = if days <= 30 {
                0
            } else if days <= 60 {
                1
            } else if days <= 90 {
                2
            } else {
                3
            };
            buckets[index] = buckets[index]
                .add(item.open_amount)
                .map_err(TradeLedgerError::Accounting)?;
        }
        Ok(buckets)
    }

    pub fn get(&self, id: &OpenItemId) -> Option<&OpenItem> {
        self.items.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&OpenItemId, &OpenItem)> {
        self.items.iter()
    }

    fn item(&self, id: &OpenItemId) -> Result<&OpenItem, TradeLedgerError> {
        self.items
            .get(id)
            .ok_or(TradeLedgerError::UnknownItem { id: id.clone() })
    }
}

/// 坏账准备目标值（整个存续期 ECL 简化法，CAS 22 §63）：未结应收 × 比率。
pub fn ecl_allowance_target(
    total_open: AccountingAmount,
    rate_bp: i32,
) -> Result<AccountingAmount, AccountingError> {
    total_open.apply_basis_points(rate_bp)
}

/// 开项子账错误（类型化）。
#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum TradeLedgerError {
    #[error("unknown open item {id:?}")]
    UnknownItem { id: OpenItemId },
    #[error("open item {id:?} is already cleared")]
    ItemCleared { id: OpenItemId },
    #[error("duplicate open item {id:?}")]
    DuplicateItem { id: OpenItemId },
    #[error("non-positive amount {amount:?} on open item")]
    NonPositiveAmount { amount: AccountingAmount },
    #[error("due {due_on} is before open date {opened_on}")]
    DueBeforeOpen {
        opened_on: CivilDate,
        due_on: CivilDate,
    },
    #[error("application {requested:?} over open balance {open:?} of {id:?}")]
    OverApplication {
        id: OpenItemId,
        requested: AccountingAmount,
        open: AccountingAmount,
    },
    #[error(transparent)]
    Accounting(#[from] AccountingError),
}
