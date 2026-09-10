//! 存货子账（共享，K3）：数量 + 成本双轨，发出按**移动加权平均**计价
//! （`game-assumption-inventory-method`，docs/company-accounting.md §2.1——
//! K3 固定的游戏假设；CAS 1 原文取证受阻，不声称准则原文依据）。
//!
//! 计价与守恒：发出成本 = rhe(结存成本 × 发出数量 / 结存数量)（整数半偶舍入），
//! 结存成本按差额结转——Σ发出成本 + 期末结存 == Σ入库成本，分毫不差；末批
//! 发出自然带走全部剩余成本。发出数量不得超过结存数量（类型化拒绝，绝不
//! 负数量）；入库数量/成本必须为正；每个项目的总账科目在首次入库时绑定。
//!
//! 行业中立：本模块不知道「工商」——银行/保险/地产（任务 9–11）以任意科目
//! 绑定复用同一数量/成本轨迹。过账由调用方（行业处理器）完成；本模块只维护
//! 子账事实。

use std::collections::BTreeMap;

use crate::accounting::amount::AccountingAmount;
use crate::accounting::error::AccountingError;
use crate::accounting::ledger::LedgerAccountId;
use thiserror::Error;

/// 存货项目代码 newtype。
#[derive(
    Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
pub struct InventoryItemCode(pub String);

/// 单个存货项目的子账状态：科目绑定 + 数量（整数件）+ 结存成本（分）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct InventoryItemState {
    account: LedgerAccountId,
    quantity: i128,
    total_cost: AccountingAmount,
}

impl InventoryItemState {
    pub fn account(&self) -> &LedgerAccountId {
        &self.account
    }

    pub fn quantity(&self) -> i128 {
        self.quantity
    }

    pub fn total_cost(&self) -> AccountingAmount {
        self.total_cost
    }
}

/// 存货子账：项目代码 → 数量/成本轨迹（与总账同存同档）。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct InventoryLedger {
    items: BTreeMap<InventoryItemCode, InventoryItemState>,
}

impl InventoryLedger {
    /// 入库：数量/成本必须为正；已有项目的科目必须与首次入库一致。
    pub fn receipt(
        &mut self,
        code: InventoryItemCode,
        account: LedgerAccountId,
        quantity: i128,
        cost: AccountingAmount,
    ) -> Result<(), InventoryError> {
        if quantity <= 0 {
            return Err(InventoryError::NonPositiveQuantity {
                item: code,
                quantity,
            });
        }
        if !cost.is_positive() {
            return Err(InventoryError::NonPositiveCost { item: code, cost });
        }
        match self.items.get_mut(&code) {
            Some(state) => {
                if state.account != account {
                    return Err(InventoryError::AccountMismatch {
                        item: code,
                        registered: state.account.clone(),
                        requested: account,
                    });
                }
                let new_quantity =
                    state
                        .quantity
                        .checked_add(quantity)
                        .ok_or(InventoryError::Accounting(
                            AccountingError::AmountOverflow {
                                op: "inventory receipt",
                                detail: format!(
                                    "quantity {} + {quantity} overflow",
                                    state.quantity
                                ),
                            },
                        ))?;
                state.quantity = new_quantity;
                state.total_cost = state
                    .total_cost
                    .add(cost)
                    .map_err(InventoryError::Accounting)?;
            }
            None => {
                self.items.insert(
                    code,
                    InventoryItemState {
                        account,
                        quantity,
                        total_cost: cost,
                    },
                );
            }
        }
        Ok(())
    }

    /// 预览发出成本（纯读；与 [`Self::apply_issue`] 同一公式，状态不变）。
    pub fn preview_issue(
        &self,
        code: &InventoryItemCode,
        quantity: i128,
    ) -> Result<AccountingAmount, InventoryError> {
        let state = self
            .items
            .get(code)
            .ok_or(InventoryError::UnknownItem { item: code.clone() })?;
        weighted_issue_cost(code, state, quantity)
    }

    /// 发出：验证 → 移动加权成本出账 → 结存按差额结转（守恒）。
    pub fn apply_issue(
        &mut self,
        code: &InventoryItemCode,
        quantity: i128,
    ) -> Result<AccountingAmount, InventoryError> {
        let cost = self.preview_issue(code, quantity)?;
        let state = self
            .items
            .get_mut(code)
            .expect("preview_issue validated existence");
        state.quantity -= quantity;
        state.total_cost = state
            .total_cost
            .sub(cost)
            .map_err(InventoryError::Accounting)?;
        Ok(cost)
    }

    pub fn quantity(&self, code: &InventoryItemCode) -> i128 {
        self.items.get(code).map_or(0, |state| state.quantity)
    }

    pub fn total_cost(&self, code: &InventoryItemCode) -> AccountingAmount {
        self.items
            .get(code)
            .map_or(AccountingAmount::ZERO, |state| state.total_cost)
    }

    pub fn get(&self, code: &InventoryItemCode) -> Option<&InventoryItemState> {
        self.items.get(code)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&InventoryItemCode, &InventoryItemState)> {
        self.items.iter()
    }
}

/// 移动加权发出成本（纯函数；数量 ≤ 0 或超过结存 → 类型化拒绝）。
fn weighted_issue_cost(
    code: &InventoryItemCode,
    state: &InventoryItemState,
    quantity: i128,
) -> Result<AccountingAmount, InventoryError> {
    if quantity <= 0 {
        return Err(InventoryError::NonPositiveQuantity {
            item: code.clone(),
            quantity,
        });
    }
    if quantity > state.quantity {
        return Err(InventoryError::InsufficientQuantity {
            item: code.clone(),
            requested: quantity,
            available: state.quantity,
        });
    }
    let scaled =
        state
            .total_cost
            .cents()
            .checked_mul(quantity)
            .ok_or(InventoryError::Accounting(
                AccountingError::AmountOverflow {
                    op: "weighted_issue_cost",
                    detail: format!("{} * {quantity}", state.total_cost.cents()),
                },
            ))?;
    let cents = rhe_div(scaled, state.quantity)?;
    Ok(AccountingAmount::from_cents(cents))
}

/// 存货子账错误（类型化，携带项目与数值上下文）。
#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum InventoryError {
    #[error("unknown inventory item {item:?}")]
    UnknownItem { item: InventoryItemCode },
    #[error("issue {requested} of {item:?} exceeds on-hand {available}")]
    InsufficientQuantity {
        item: InventoryItemCode,
        requested: i128,
        available: i128,
    },
    #[error("non-positive quantity {quantity} for {item:?}")]
    NonPositiveQuantity {
        item: InventoryItemCode,
        quantity: i128,
    },
    #[error("non-positive cost {cost:?} for {item:?}")]
    NonPositiveCost {
        item: InventoryItemCode,
        cost: AccountingAmount,
    },
    #[error("item {item:?} is bound to account {registered:?}, not {requested:?}")]
    AccountMismatch {
        item: InventoryItemCode,
        registered: LedgerAccountId,
        requested: LedgerAccountId,
    },
    #[error(transparent)]
    Accounting(#[from] AccountingError),
}

/// 整数半偶舍入除法 `n/d`（d > 0）。任务 8 共享子账（存货/固定资产）的统一
/// 舍入入口——与任务 6 `amount.rs` 私有实现同算法；amount 属任务 6 语义冻结
/// 区不改动，故此处维护共享副本（公司域 interest 另有一份带说明的副本）。
pub(in crate::accounting) fn rhe_div(n: i128, d: i128) -> Result<i128, AccountingError> {
    if d <= 0 {
        return Err(AccountingError::AmountOverflow {
            op: "rhe_div",
            detail: format!("divisor {d} must be positive"),
        });
    }
    let negative = n < 0;
    let numerator = n.unsigned_abs();
    let divisor = d.unsigned_abs();
    let quotient = numerator / divisor;
    let remainder = numerator % divisor;
    let doubled = remainder * 2;
    let round_up = doubled > divisor || (doubled == divisor && !quotient.is_multiple_of(2));
    let magnitude = i128::try_from(if round_up { quotient + 1 } else { quotient })
        .expect("quotient of |i128| by positive divisor fits i128");
    Ok(if negative { -magnitude } else { magnitude })
}
