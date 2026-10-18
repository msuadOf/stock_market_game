use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{AccountKind, GameSession, SessionError, StockId};
use crate::error::ValidationFault;

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
    pub code: StockId,
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
            // fault1100: 初始分配只能在首次开跑前查询（非零 tick/已过开局日/非新局）。
            return Err(SessionError::InvalidSave(ValidationFault::bare(1100)));
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
                let position_code = self
                    .state
                    .stocks
                    .id(&spec.code.0)
                    .expect("配置股票必须已入目录");
                let quantity = account
                    .position(&position_code)
                    .map_or(0, |position| position.qty());
                let category = match account.kind() {
                    AccountKind::Retail => &mut categories[0],
                    AccountKind::Inst => &mut categories[1],
                    AccountKind::Hot => &mut categories[2],
                    AccountKind::Player => {
                        if quantity != 0 {
                            // fault1101: 初始玩家持股非零。槽0 待 C 批数值化（原文本为代码串 "{}"，spec.code.0）。
                            return Err(SessionError::InvalidSave(ValidationFault::new(
                                1101,
                                vec![0],
                            )));
                        }
                        continue;
                    }
                };
                // fault1102: 初始分配账户人数（u32）溢出。
                category.account_count = category
                    .account_count
                    .checked_add(1)
                    .ok_or_else(|| SessionError::InvalidSave(ValidationFault::bare(1102)))?;
                // fault1103: 初始分配类内股数（u32）溢出。
                category.shares = category
                    .shares
                    .checked_add(quantity)
                    .ok_or_else(|| SessionError::InvalidSave(ValidationFault::bare(1103)))?;
                if quantity == 0 {
                    category.zero_holders += 1;
                }
            }
            // fault1104: 初始分配三类合计股数（u32）溢出。
            let allocated = categories
                .iter()
                .try_fold(0u32, |total, category| total.checked_add(category.shares))
                .ok_or_else(|| SessionError::InvalidSave(ValidationFault::bare(1104)))?;
            let unallocated_shares = spec.float_shares.checked_sub(allocated).ok_or_else(|| {
                // fault1105: 初始分配超过流通盘。槽0 待 C 批数值化（原文本为代码串 "{}"，spec.code.0）。
                SessionError::InvalidSave(ValidationFault::new(1105, vec![0]))
            })?;
            if unallocated_shares > 0
                && categories.iter().any(|category| category.account_count > 0)
            {
                // fault1106: 存在 NPC 但初始流通盘未完整分配。槽0 待 C 批数值化（原文本为代码串 "{}"，spec.code.0）。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    1106,
                    vec![0],
                )));
            }
            stocks.push(InitialStockAllocation {
                code: self
                    .state
                    .stocks
                    .id(&spec.code.0)
                    .expect("配置股票必须已入目录"),
                float_shares: spec.float_shares,
                unallocated_shares,
                categories: categories.into(),
            });
        }
        Ok(InitialAllocation { stocks })
    }
}
