use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{AccountKind, GameSession, SessionError, StockCode};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum InitialAllocationKind {
    Retail,
    Inst,
    Hot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct InitialAllocationCategory {
    pub kind: InitialAllocationKind,
    pub shares: u32,
    pub account_count: u32,
    pub zero_holders: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct InitialStockAllocation {
    pub code: StockCode,
    pub float_shares: u32,
    pub unallocated_shares: u32,
    pub categories: Vec<InitialAllocationCategory>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct InitialAllocation {
    pub stocks: Vec<InitialStockAllocation>,
}

impl GameSession {
    pub fn initial_allocation(&self) -> Result<InitialAllocation, SessionError> {
        if !self.fresh_initial_allocation
            || self.tick() != 0
            || self.civil_date() != self.state.setup.start_date
        {
            return Err(SessionError::InvalidSave(
                "初始分配只能在本局首次开跑前查询，不能用交易后的持仓冒充初始分配".to_owned(),
            ));
        }
        let mut stocks = Vec::with_capacity(self.state.setup.stocks.len());
        for spec in &self.state.setup.stocks {
            let mut categories = [
                InitialAllocationKind::Retail,
                InitialAllocationKind::Inst,
                InitialAllocationKind::Hot,
            ]
            .map(|kind| InitialAllocationCategory {
                kind,
                shares: 0,
                account_count: 0,
                zero_holders: 0,
            });
            for account in self.state.accounts.values() {
                let quantity = account
                    .position(&spec.code)
                    .map_or(0, |position| position.qty());
                let category = match account.kind() {
                    AccountKind::Retail => &mut categories[0],
                    AccountKind::Inst => &mut categories[1],
                    AccountKind::Hot => &mut categories[2],
                    AccountKind::Player => {
                        if quantity != 0 {
                            return Err(SessionError::InvalidSave(format!(
                                "{} 初始玩家持股非零，无法作为 NPC 初始化分配",
                                spec.code.0
                            )));
                        }
                        continue;
                    }
                };
                category.account_count = category
                    .account_count
                    .checked_add(1)
                    .ok_or_else(|| SessionError::InvalidSave("初始分配账户人数溢出".to_owned()))?;
                category.shares = category
                    .shares
                    .checked_add(quantity)
                    .ok_or_else(|| SessionError::InvalidSave("初始分配股数溢出".to_owned()))?;
                if quantity == 0 {
                    category.zero_holders += 1;
                }
            }
            let allocated = categories
                .iter()
                .try_fold(0u32, |total, category| total.checked_add(category.shares))
                .ok_or_else(|| SessionError::InvalidSave("初始分配合计股数溢出".to_owned()))?;
            let unallocated_shares = spec.float_shares.checked_sub(allocated).ok_or_else(|| {
                SessionError::InvalidSave(format!("{} 初始分配超过流通盘", spec.code.0))
            })?;
            if unallocated_shares > 0
                && categories.iter().any(|category| category.account_count > 0)
            {
                return Err(SessionError::InvalidSave(format!(
                    "{} 存在 NPC 但初始流通盘未完整分配",
                    spec.code.0
                )));
            }
            stocks.push(InitialStockAllocation {
                code: spec.code.clone(),
                float_shares: spec.float_shares,
                unallocated_shares,
                categories: categories.into(),
            });
        }
        Ok(InitialAllocation { stocks })
    }
}
