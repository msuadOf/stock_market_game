//! 同 PendingAdmin 的回购角色与 checked 现金原语；不保存预算余额或批准幂等集合。
use super::*;
use crate::runtime::protocol::AdminOperation;

fn invalid(detail: impl Into<String>) -> RuntimeError {
    RuntimeError::ProtocolInvariant {
        detail: detail.into(),
    }
}
impl GroupState {
    pub(super) fn validate_repurchase_funds_operation(
        &self,
        operation: AdminOperation,
    ) -> Result<(), RuntimeError> {
        self.validate_repurchase_funds_in_book(operation, &self.plan_book)
    }

    pub(super) fn validate_repurchase_funds_in_book(
        &self,
        operation: AdminOperation,
        book: &crate::plans::PlanBook,
    ) -> Result<(), RuntimeError> {
        let (local, stock, id, recycle) = match operation {
            AdminOperation::SyntheticBudgetCredit {
                local,
                stock,
                execution_plan,
                ..
            } => {
                let crate::runtime::protocol::AdminPlanReference::Existing(id) = execution_plan
                else {
                    return Err(invalid("APPROVAL-C CreatedInThisPrepare 引用尚未实现"));
                };
                (local, stock, id, false)
            }
            AdminOperation::SyntheticBudgetRecycle {
                local,
                stock,
                execution_plan,
                ..
            } => (local, stock, execution_plan, true),
            _ => return Ok(()),
        };
        let reject = |reason: &str| {
            invalid(format!(
                "回购资金身份无效：group={}, local={local}, stock={stock}, plan={id:?}：{reason}",
                self.group
            ))
        };
        let root = self
            .root_projection
            .as_ref()
            .ok_or_else(|| reject("缺少实际名册"))?;
        let global = self
            .roster_offset
            .checked_add(local as usize)
            .ok_or_else(|| reject("名册坐标溢出"))?;
        let account = root
            .account_ids
            .get(global)
            .copied()
            .ok_or_else(|| reject("local 为尾部虚位或越界"))?;
        if local as usize >= self.accounts.len() || id.owner != account {
            return Err(reject("PlanId owner 与实际账户不一致"));
        }
        let entry = book.entry(id).map_err(|_| reject("未知 PlanId"))?;
        let crate::plans::PlanEntry::IssuerRepurchase(plan) = entry else {
            return Err(reject("Investor 个人判断不能作为公司资金来源"));
        };
        if plan.stock() != crate::StockId(u32::from(stock)) || entry.is_terminal() {
            return Err(reject("证券不一致或公司计划已 Closed"));
        }
        let mut bindings = self
            .admin_treasury_bindings
            .iter()
            .filter(|binding| binding.owner == (self.group, local) && binding.stock == stock);
        let binding = bindings
            .next()
            .ok_or_else(|| reject("缺少实际专户 binding"))?;
        if bindings.next().is_some()
            || binding.account != account
            || binding.issuer != plan.issuer()
        {
            return Err(reject("专户 binding 重复或 account/issuer 不一致"));
        }
        // 真实残票是独立事实；零余量、未绑定 OrderRef 均不能证明来源已消费完。
        if recycle
            && self
                .child_in_flight
                .keys()
                .any(|(owner, _)| *owner == account)
        {
            return Err(reject("PendingDrain：共享专户仍有真实 child ticket"));
        }
        Ok(())
    }
}

pub(super) fn apply_cash(
    account: &mut GroupAccount,
    stock: StockIdx,
    amount: i64,
    recycle: bool,
) -> Result<(), RuntimeError> {
    let operation = if recycle {
        "SyntheticBudgetRecycle"
    } else {
        "SyntheticBudgetCredit"
    };
    let reject = |reason: &str| {
        invalid(format!(
            "回购资金 {operation}，stock={stock}, amount={amount}：{reason}"
        ))
    };
    if amount <= 0 {
        return Err(reject("金额必须为正分额"));
    }
    let count = account.shares.len();
    if usize::from(stock) >= count {
        return Err(reject("stock 越界"));
    }
    if [
        account.frozen_today.len(),
        account.foreign_active.len(),
        account.pending_sell_qty.len(),
        account.invested_cents.len(),
        account.recovered_cents.len(),
        account.ever_held.len(),
    ]
    .iter()
    .any(|len| *len != count)
    {
        return Err(reject("账户资源向量长度不一致"));
    }
    if account.cash < 0 || account.buy_purse_held_cents < 0 {
        return Err(reject("raw cash 或 held 为负"));
    }
    account
        .cash
        .checked_add(account.buy_purse_held_cents)
        .ok_or_else(|| reject("原公开 cash+held 溢出"))?;
    for index in 0..count {
        if account.frozen_today[index] > account.shares[index] {
            return Err(reject("冻结股份超过在手股份"));
        }
        account.shares[index]
            .checked_add(account.pending_sell_qty[index])
            .ok_or_else(|| reject("在手加在途卖股溢出"))?;
    }
    if recycle
        && (account.buy_purse_held_cents != 0
            || account.foreign_active.iter().any(|v| *v != 0)
            || account.pending_sell_qty.iter().any(|v| *v != 0))
    {
        return Err(reject(
            "PendingDrain：共享专户仍有 held/foreign/pending 资源",
        ));
    }
    let cash = if recycle {
        if amount > account.cash {
            return Err(invalid(format!(
                "回购资金 {operation}，stock={stock}：申请 {amount} 分，可用 {} 分，现金不足",
                account.cash
            )));
        }
        account
            .cash
            .checked_sub(amount)
            .ok_or_else(|| reject("raw cash 扣减溢出"))?
    } else {
        account
            .cash
            .checked_add(amount)
            .ok_or_else(|| reject("raw cash 加额溢出"))?
    };
    cash.checked_add(account.buy_purse_held_cents)
        .ok_or_else(|| reject("候选公开 cash+held 溢出"))?;
    account.cash = cash;
    Ok(())
}
