//! 配股付款、退款与实缴成本增股的私有 checked 原语；不拥有事务、持久账或生产入口。

use super::GroupAccount;
use crate::runtime::StockIdx;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AdminFundsCostKind {
    PaymentDebit,
    SubscriptionRefund,
    SharesWithCost,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AccountField {
    Shares,
    FrozenToday,
    ForeignActive,
    Invested,
    Recovered,
    EverHeld,
    PendingSell,
    Cash,
    HeldCash,
    TotalCash,
    TotalShares,
    NetInvested,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum AdminFundsCostFailure {
    InvalidStock {
        stocks: usize,
    },
    ArrayLength {
        field: AccountField,
        expected: usize,
        actual: usize,
    },
    NonPositiveAmount {
        amount: i64,
    },
    ZeroQuantity,
    NegativeBalance {
        field: AccountField,
        value: i64,
    },
    FrozenExceedsShares {
        frozen: u32,
        shares: u32,
    },
    DirtyFlatCost {
        invested: i64,
        recovered: i64,
    },
    InsufficientCash {
        requested: i64,
        available: i64,
    },
    Overflow {
        field: AccountField,
        left: i128,
        right: i128,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AdminFundsCostError {
    pub operation: AdminFundsCostKind,
    pub stock: StockIdx,
    pub reason: AdminFundsCostFailure,
}

impl std::fmt::Display for AdminFundsCostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "配股资金原语 {:?}，stock={}：{:?}",
            self.operation, self.stock, self.reason
        )
    }
}
impl std::error::Error for AdminFundsCostError {}

fn error(
    operation: AdminFundsCostKind,
    stock: StockIdx,
    reason: AdminFundsCostFailure,
) -> AdminFundsCostError {
    AdminFundsCostError {
        operation,
        stock,
        reason,
    }
}

fn overflow(
    operation: AdminFundsCostKind,
    stock: StockIdx,
    field: AccountField,
    left: impl Into<i128>,
    right: impl Into<i128>,
) -> AdminFundsCostError {
    error(
        operation,
        stock,
        AdminFundsCostFailure::Overflow {
            field,
            left: left.into(),
            right: right.into(),
        },
    )
}

/// 先验证所有将读取的向量与完整公开资源，不通过默认值修补损坏账户。
fn validate_account(
    account: &GroupAccount,
    stock: StockIdx,
    operation: AdminFundsCostKind,
) -> Result<usize, AdminFundsCostError> {
    let index = usize::from(stock);
    let stocks = account.shares.len();
    if index >= stocks {
        return Err(error(
            operation,
            stock,
            AdminFundsCostFailure::InvalidStock { stocks },
        ));
    }
    for (field, actual) in [
        (AccountField::FrozenToday, account.frozen_today.len()),
        (AccountField::ForeignActive, account.foreign_active.len()),
        (AccountField::Invested, account.invested_cents.len()),
        (AccountField::Recovered, account.recovered_cents.len()),
        (AccountField::EverHeld, account.ever_held.len()),
        (AccountField::PendingSell, account.pending_sell_qty.len()),
    ] {
        if actual != stocks {
            return Err(error(
                operation,
                stock,
                AdminFundsCostFailure::ArrayLength {
                    field,
                    expected: stocks,
                    actual,
                },
            ));
        }
    }
    for (field, value) in [
        (AccountField::Cash, account.cash),
        (AccountField::HeldCash, account.buy_purse_held_cents),
        (AccountField::Invested, account.invested_cents[index]),
        (AccountField::Recovered, account.recovered_cents[index]),
    ] {
        if value < 0 {
            return Err(error(
                operation,
                stock,
                AdminFundsCostFailure::NegativeBalance { field, value },
            ));
        }
    }
    if account.frozen_today[index] > account.shares[index] {
        return Err(error(
            operation,
            stock,
            AdminFundsCostFailure::FrozenExceedsShares {
                frozen: account.frozen_today[index],
                shares: account.shares[index],
            },
        ));
    }
    account
        .cash
        .checked_add(account.buy_purse_held_cents)
        .ok_or_else(|| {
            overflow(
                operation,
                stock,
                AccountField::TotalCash,
                account.cash,
                account.buy_purse_held_cents,
            )
        })?;
    account.shares[index]
        .checked_add(account.pending_sell_qty[index])
        .ok_or_else(|| {
            overflow(
                operation,
                stock,
                AccountField::TotalShares,
                account.shares[index],
                account.pending_sell_qty[index],
            )
        })?;
    account.invested_cents[index]
        .checked_sub(account.recovered_cents[index])
        .ok_or_else(|| {
            overflow(
                operation,
                stock,
                AccountField::NetInvested,
                account.invested_cents[index],
                account.recovered_cents[index],
            )
        })?;
    if account.shares[index] == 0
        && account.pending_sell_qty[index] == 0
        && account.foreign_active[index] == 0
        && (account.invested_cents[index] != 0 || account.recovered_cents[index] != 0)
    {
        return Err(error(
            operation,
            stock,
            AdminFundsCostFailure::DirtyFlatCost {
                invested: account.invested_cents[index],
                recovered: account.recovered_cents[index],
            },
        ));
    }
    Ok(index)
}

fn validate_amount(
    operation: AdminFundsCostKind,
    stock: StockIdx,
    amount: i64,
) -> Result<(), AdminFundsCostError> {
    if amount <= 0 {
        return Err(error(
            operation,
            stock,
            AdminFundsCostFailure::NonPositiveAmount { amount },
        ));
    }
    Ok(())
}

pub(super) fn payment_debit(
    account: &mut GroupAccount,
    stock: StockIdx,
    amount: i64,
) -> Result<(), AdminFundsCostError> {
    let operation = AdminFundsCostKind::PaymentDebit;
    validate_amount(operation, stock, amount)?;
    validate_account(account, stock, operation)?;
    if amount > account.cash {
        return Err(error(
            operation,
            stock,
            AdminFundsCostFailure::InsufficientCash {
                requested: amount,
                available: account.cash,
            },
        ));
    }
    let cash = account
        .cash
        .checked_sub(amount)
        .ok_or_else(|| overflow(operation, stock, AccountField::Cash, account.cash, amount))?;
    account.cash = cash;
    Ok(())
}

pub(super) fn subscription_refund(
    account: &mut GroupAccount,
    stock: StockIdx,
    amount: i64,
) -> Result<(), AdminFundsCostError> {
    let operation = AdminFundsCostKind::SubscriptionRefund;
    validate_amount(operation, stock, amount)?;
    validate_account(account, stock, operation)?;
    let cash = account
        .cash
        .checked_add(amount)
        .ok_or_else(|| overflow(operation, stock, AccountField::Cash, account.cash, amount))?;
    cash.checked_add(account.buy_purse_held_cents)
        .ok_or_else(|| {
            overflow(
                operation,
                stock,
                AccountField::TotalCash,
                cash,
                account.buy_purse_held_cents,
            )
        })?;
    account.cash = cash;
    Ok(())
}

pub(super) fn shares_with_cost(
    account: &mut GroupAccount,
    stock: StockIdx,
    qty: u32,
    subscription_cost: i64,
) -> Result<(), AdminFundsCostError> {
    let operation = AdminFundsCostKind::SharesWithCost;
    validate_amount(operation, stock, subscription_cost)?;
    if qty == 0 {
        return Err(error(operation, stock, AdminFundsCostFailure::ZeroQuantity));
    }
    let index = validate_account(account, stock, operation)?;
    let shares = account.shares[index].checked_add(qty).ok_or_else(|| {
        overflow(
            operation,
            stock,
            AccountField::Shares,
            account.shares[index],
            qty,
        )
    })?;
    shares
        .checked_add(account.pending_sell_qty[index])
        .ok_or_else(|| {
            overflow(
                operation,
                stock,
                AccountField::TotalShares,
                shares,
                account.pending_sell_qty[index],
            )
        })?;
    let invested = account.invested_cents[index]
        .checked_add(subscription_cost)
        .ok_or_else(|| {
            overflow(
                operation,
                stock,
                AccountField::Invested,
                account.invested_cents[index],
                subscription_cost,
            )
        })?;
    invested
        .checked_sub(account.recovered_cents[index])
        .ok_or_else(|| {
            overflow(
                operation,
                stock,
                AccountField::NetInvested,
                invested,
                account.recovered_cents[index],
            )
        })?;
    // 所有可能失败的运算都已完成；一次写入承诺的股份与成本，现金／冻结／在途保持原值。
    account.shares[index] = shares;
    account.invested_cents[index] = invested;
    account.ever_held[index] = false;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> GroupAccount {
        let mut account = GroupAccount::new(2400, 2);
        account.buy_purse_held_cents = 8000;
        account.shares[0] = 20;
        account.frozen_today[0] = 3;
        account.pending_sell_qty[0] = 4;
        account.foreign_active[0] = 1;
        account.invested_cents[0] = 10_000;
        account.recovered_cents[0] = 2_000;
        account.ever_held[0] = false;
        account.shares[1] = 100;
        account.invested_cents[1] = 50_000;
        account
    }

    #[test]
    fn payment_debits_only_available_cash_without_changing_other_fields() {
        let mut account = account();
        let mut expected = account.clone();
        expected.cash = 0;
        payment_debit(&mut account, 0, 2400).unwrap();
        assert_eq!(account, expected);
    }

    #[test]
    fn payment_cannot_use_held_cash_or_silently_pay_a_partial_amount() {
        let mut account = account();
        let before = account.clone();
        let error = payment_debit(&mut account, 0, 2401).unwrap_err();
        assert_eq!(error.operation, AdminFundsCostKind::PaymentDebit);
        assert_eq!(
            error.reason,
            AdminFundsCostFailure::InsufficientCash {
                requested: 2401,
                available: 2400
            }
        );
        assert_eq!(account, before);
    }

    #[test]
    fn refund_credits_principal_without_altering_position_cost_or_pending() {
        let mut account = account();
        let mut expected = account.clone();
        expected.cash += 5600;
        subscription_refund(&mut account, 0, 5600).unwrap();
        assert_eq!(account, expected);
    }

    #[test]
    fn new_shares_add_exact_subscription_cost_without_second_payment_or_t1_lock() {
        let mut account = account();
        let mut expected = account.clone();
        expected.shares[0] = 27;
        expected.invested_cents[0] = 15_600;
        shares_with_cost(&mut account, 0, 7, 5600).unwrap();
        assert_eq!(account, expected);
    }

    #[test]
    fn shares_after_true_flat_start_with_the_original_subscription_cost() {
        let mut account = GroupAccount::new(1200, 1);
        account.ever_held[0] = true;
        shares_with_cost(&mut account, 0, 7, 5600).unwrap();
        assert_eq!(
            (
                account.shares[0],
                account.invested_cents[0],
                account.recovered_cents[0]
            ),
            (7, 5600, 0)
        );
        assert_eq!(account.cash, 1200);
        assert_eq!(account.frozen_today[0], 0);
        assert!(!account.ever_held[0]);
    }

    #[test]
    fn zero_and_negative_net_cost_are_valid_when_accumulators_are_nonnegative() {
        for recovered in [10_000, 20_000] {
            let mut account = account();
            account.recovered_cents[0] = recovered;
            shares_with_cost(&mut account, 0, 1, 800).unwrap();
            assert_eq!(account.invested_cents[0], 10_800);
            assert_eq!(account.recovered_cents[0], recovered);
        }
    }

    #[test]
    fn zero_raw_shares_keep_legal_old_cost_while_sell_or_other_ticket_is_pending() {
        for (pending, active) in [(4, 1), (0, 1)] {
            let mut account = account();
            account.shares[0] = 0;
            account.frozen_today[0] = 0;
            account.pending_sell_qty[0] = pending;
            account.foreign_active[0] = active;
            shares_with_cost(&mut account, 0, 7, 5600).unwrap();
            assert_eq!(
                (account.invested_cents[0], account.recovered_cents[0]),
                (15_600, 2000)
            );
            assert_eq!(
                (account.pending_sell_qty[0], account.foreign_active[0]),
                (pending, active)
            );
        }
    }

    #[test]
    fn negative_accumulators_and_dirty_true_flat_are_errors_without_silent_repair() {
        for (invested, recovered, flat) in [(-1, 0, false), (0, -1, false), (10_000, 2000, true)] {
            let mut account = account();
            account.invested_cents[0] = invested;
            account.recovered_cents[0] = recovered;
            if flat {
                account.shares[0] = 0;
                account.frozen_today[0] = 0;
                account.pending_sell_qty[0] = 0;
                account.foreign_active[0] = 0;
            }
            let before = account.clone();
            assert!(shares_with_cost(&mut account, 0, 7, 5600).is_err());
            assert_eq!(account, before);
        }
    }

    #[test]
    fn nonpositive_amounts_and_zero_quantity_are_rejected() {
        for amount in [0, -1] {
            let mut account = account();
            let before = account.clone();
            assert!(payment_debit(&mut account, 0, amount).is_err());
            assert!(subscription_refund(&mut account, 0, amount).is_err());
            assert!(shares_with_cost(&mut account, 0, 1, amount).is_err());
            assert_eq!(account, before);
        }
        let mut account = account();
        let before = account.clone();
        assert!(shares_with_cost(&mut account, 0, 0, 800).is_err());
        assert_eq!(account, before);
    }

    #[test]
    fn public_cash_overflow_is_rejected_even_when_raw_refund_fits() {
        let mut account = account();
        account.cash = i64::MAX - 8000;
        let before = account.clone();
        let error = subscription_refund(&mut account, 0, 1).unwrap_err();
        assert!(matches!(
            error.reason,
            AdminFundsCostFailure::Overflow {
                field: AccountField::TotalCash,
                ..
            }
        ));
        assert_eq!(account, before);
    }

    #[test]
    fn raw_cash_overflow_is_rejected_without_mutation() {
        let mut account = account();
        account.cash = i64::MAX;
        account.buy_purse_held_cents = 0;
        let before = account.clone();
        assert!(subscription_refund(&mut account, 0, 1).is_err());
        assert_eq!(account, before);
    }

    #[test]
    fn shares_beyond_i32_are_supported_without_signed_delta_truncation() {
        let mut account = GroupAccount::new(0, 1);
        let qty = i32::MAX as u32 + 1;
        shares_with_cost(&mut account, 0, qty, 1).unwrap();
        assert_eq!(account.shares[0], qty);
        assert_eq!(account.invested_cents[0], 1);
    }

    #[test]
    fn raw_and_public_share_overflows_leave_the_entire_account_unchanged() {
        for (shares, pending) in [(u32::MAX, 0), (u32::MAX - 1, 1)] {
            let mut account = account();
            account.shares[0] = shares;
            account.pending_sell_qty[0] = pending;
            let before = account.clone();
            assert!(shares_with_cost(&mut account, 0, 1, 800).is_err());
            assert_eq!(account, before);
        }
    }

    #[test]
    fn invested_overflow_does_not_leave_new_shares_partially_credited() {
        let mut account = account();
        account.invested_cents[0] = i64::MAX;
        let before = account.clone();
        assert!(shares_with_cost(&mut account, 0, 7, 5600).is_err());
        assert_eq!(account, before);
    }

    #[test]
    fn invalid_stock_or_malformed_array_lengths_return_errors_without_panicking() {
        let mut account = account();
        let before = account.clone();
        assert!(payment_debit(&mut account, 2, 1).is_err());
        assert!(subscription_refund(&mut account, 2, 1).is_err());
        assert!(shares_with_cost(&mut account, 2, 1, 800).is_err());
        assert_eq!(account, before);
        account.recovered_cents = super::super::InlineVec::filled(1, 0);
        let before = account.clone();
        assert!(shares_with_cost(&mut account, 1, 1, 800).is_err());
        assert_eq!(account, before);
    }

    #[test]
    fn negative_cash_held_or_excess_frozen_reject_instead_of_normalizing_input() {
        for (cash, held, frozen) in [(-1, 0, 0), (0, -1, 0), (2400, 8000, 21)] {
            let mut account = account();
            account.cash = cash;
            account.buy_purse_held_cents = held;
            account.frozen_today[0] = frozen;
            let before = account.clone();
            assert!(subscription_refund(&mut account, 0, 1).is_err());
            assert_eq!(account, before);
        }
    }
}
