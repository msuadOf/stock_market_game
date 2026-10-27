//! 回购来源票的纯数值投影；不持有账户、交易部或计划簿权威。

use crate::StockId;
use crate::plans::PlanId;
use crate::runtime::{FillFact, OrderRef, Owner, StockIdx};

/// 回购请求的来源身份。账户身份来自 `plan_id.owner`，不从 runtime `Owner` 推导。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepurchaseRequestKey {
    pub plan_id: PlanId,
    pub stock: StockId,
    pub requested_trading_day: u64,
}

/// 累计实际扣款与退款后的回购订单来源票。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepurchaseOrderFact {
    pub request_key: RepurchaseRequestKey,
    pub owner: Owner,
    pub order_ref: Option<OrderRef>,
    pub submitted_tick: u64,
    pub qty: u32,
    pub purse_initial: i64,
    pub charged_cumulative: i64,
    pub refunded_cumulative: i64,
    pub closed: bool,
}

/// 已由上层关联到来源票的退款数值。当前 `CashRefund` 不携带 `OrderRef`；
/// 调用方须先以批次股票、owner 与唯一未闭 child 完成关联。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssociatedReturn {
    pub request_key: RepurchaseRequestKey,
    pub owner: Owner,
    pub order_ref: Option<OrderRef>,
    pub amount: i64,
}

/// 来源票拒绝不一致或不安全的事实。
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RepurchaseOrderFactError {
    #[error("回购来源票数量必须为正，收到 {qty}")]
    InvalidQty { qty: u32 },
    #[error("回购来源票初始资金不能为负，收到 {amount}")]
    NegativeInitialPurse { amount: i64 },
    #[error("回购来源事实累计扣款不能为负，收到 {amount}")]
    NegativeChargedCumulative { amount: i64 },
    #[error("回购来源事实累计退款不能为负，收到 {amount}")]
    NegativeRefundedCumulative { amount: i64 },
    #[error("回购来源事实的 owner 不匹配")]
    OwnerMismatch,
    #[error("回购来源事实的 RequestKey 不匹配")]
    RequestMismatch,
    #[error("回购来源事实的 StockIdx 与已验证证券映射不匹配")]
    StockMismatch,
    #[error("回购订单已绑定到另一 OrderRef")]
    OrderRefConflict,
    #[error("回购订单已绑定，退款事实必须携带相同 OrderRef")]
    MissingBoundOrderRef,
    #[error("退款事实不能首次绑定回购订单 OrderRef")]
    ReturnCannotBindOrderRef,
    #[error("回购退款金额不能为负，收到 {amount}")]
    NegativeReturn { amount: i64 },
    #[error("回购来源事实金额溢出")]
    AmountOverflow,
    #[error("回购来源事实导致 held purse 为负")]
    NegativeHeldPurse,
    #[error("已关闭回购来源票不能继续消费事实")]
    AlreadyClosed,
    #[error("回购来源游标不连续：current={current}, incoming={incoming}")]
    CursorOutOfOrder { current: u64, incoming: u64 },
    #[error("回购来源游标溢出")]
    CursorOverflow,
    #[error("回购真实成交事实无效：{0}")]
    InvalidFill(#[from] crate::runtime::FillFactError),
    #[error("回购来源成交必须是 Buy 腿")]
    NonBuyFill,
}

impl RepurchaseOrderFact {
    /// 建立尚无真实交易部身份的来源票，不分配或猜测 `OrderRef`。
    pub fn new(
        request_key: RepurchaseRequestKey,
        owner: Owner,
        submitted_tick: u64,
        qty: u32,
        purse_initial: i64,
    ) -> Result<Self, RepurchaseOrderFactError> {
        if qty == 0 {
            return Err(RepurchaseOrderFactError::InvalidQty { qty });
        }
        if purse_initial < 0 {
            return Err(RepurchaseOrderFactError::NegativeInitialPurse {
                amount: purse_initial,
            });
        }
        Ok(Self {
            request_key,
            owner,
            order_ref: None,
            submitted_tick,
            qty,
            purse_initial,
            charged_cumulative: 0,
            refunded_cumulative: 0,
            closed: false,
        })
    }

    /// 消费真实买入 `FillFact` 并按实际成交价和费用累计扣款。
    /// `stock_idx` 必须来自调用方已验证的 `StockId` 映射。
    pub fn apply_fill(
        &mut self,
        associated_key: RepurchaseRequestKey,
        stock_idx: StockIdx,
        fill: FillFact,
    ) -> Result<(), RepurchaseOrderFactError> {
        self.ensure_open()?;
        self.ensure_identity(associated_key, fill.owner, fill.order.0, stock_idx)?;
        fill.validate()?;
        if fill.side != crate::orderbook::Side::Buy {
            return Err(RepurchaseOrderFactError::NonBuyFill);
        }
        let gross = fill
            .price
            .checked_mul(i64::from(fill.qty))
            .ok_or(RepurchaseOrderFactError::AmountOverflow)?;
        let charge = gross
            .checked_add(fill.commission)
            .and_then(|amount| amount.checked_add(fill.transfer_fee))
            .ok_or(RepurchaseOrderFactError::AmountOverflow)?;
        let charged = self
            .charged_cumulative
            .checked_add(charge)
            .ok_or(RepurchaseOrderFactError::AmountOverflow)?;
        let held = self
            .purse_initial
            .checked_sub(charged)
            .and_then(|amount| amount.checked_sub(self.refunded_cumulative))
            .ok_or(RepurchaseOrderFactError::AmountOverflow)?;
        if held < 0 {
            return Err(RepurchaseOrderFactError::NegativeHeldPurse);
        }
        self.bind_order_ref(fill.order)?;
        self.charged_cumulative = charged;
        Ok(())
    }

    /// 消费上层已唯一关联的退款。尚未获知受理身份时允许 `order_ref=None`。
    pub fn apply_return(
        &mut self,
        stock_idx: StockIdx,
        returned: AssociatedReturn,
    ) -> Result<(), RepurchaseOrderFactError> {
        self.ensure_open()?;
        self.ensure_identity(
            returned.request_key,
            returned.owner,
            returned.order_ref.map_or(stock_idx, |order| order.0),
            stock_idx,
        )?;
        if returned.amount < 0 {
            return Err(RepurchaseOrderFactError::NegativeReturn {
                amount: returned.amount,
            });
        }
        match (self.order_ref, returned.order_ref) {
            (Some(_), None) => return Err(RepurchaseOrderFactError::MissingBoundOrderRef),
            (Some(bound), Some(actual)) if bound != actual => {
                return Err(RepurchaseOrderFactError::OrderRefConflict);
            }
            (None, Some(_)) => return Err(RepurchaseOrderFactError::ReturnCannotBindOrderRef),
            _ => {}
        }
        let refunded = self
            .refunded_cumulative
            .checked_add(returned.amount)
            .ok_or(RepurchaseOrderFactError::AmountOverflow)?;
        let held = self
            .purse_initial
            .checked_sub(self.charged_cumulative)
            .and_then(|amount| amount.checked_sub(refunded))
            .ok_or(RepurchaseOrderFactError::AmountOverflow)?;
        if held < 0 {
            return Err(RepurchaseOrderFactError::NegativeHeldPurse);
        }
        self.refunded_cumulative = refunded;
        Ok(())
    }

    /// 标记真实终结事实已消费；是否确实有完整批次和唯一来源由上层证明。
    pub fn mark_closed(&mut self) -> Result<(), RepurchaseOrderFactError> {
        self.ensure_open()?;
        self.closed = true;
        Ok(())
    }

    /// 返回尚未返还的实际来源资金，不按限价和剩余数量重算。
    pub fn held_purse(&self) -> Result<i64, RepurchaseOrderFactError> {
        self.validate_balances()?;
        Ok(self
            .purse_initial
            .checked_sub(self.charged_cumulative)
            .and_then(|amount| amount.checked_sub(self.refunded_cumulative))
            .ok_or(RepurchaseOrderFactError::AmountOverflow)?)
    }

    /// 仅给出纯数值释放条件；不会关闭计划或授权真实账户退款。
    pub fn releasable_amount(&self) -> Result<Option<i64>, RepurchaseOrderFactError> {
        let held = self.held_purse()?;
        if self.closed && held == 0 {
            Ok(Some(0))
        } else {
            Ok(None)
        }
    }

    fn ensure_open(&self) -> Result<(), RepurchaseOrderFactError> {
        self.validate_balances()?;
        if self.closed {
            Err(RepurchaseOrderFactError::AlreadyClosed)
        } else {
            Ok(())
        }
    }

    fn validate_balances(&self) -> Result<(), RepurchaseOrderFactError> {
        if self.purse_initial < 0 {
            return Err(RepurchaseOrderFactError::NegativeInitialPurse {
                amount: self.purse_initial,
            });
        }
        if self.charged_cumulative < 0 {
            return Err(RepurchaseOrderFactError::NegativeChargedCumulative {
                amount: self.charged_cumulative,
            });
        }
        if self.refunded_cumulative < 0 {
            return Err(RepurchaseOrderFactError::NegativeRefundedCumulative {
                amount: self.refunded_cumulative,
            });
        }
        let held = self
            .purse_initial
            .checked_sub(self.charged_cumulative)
            .and_then(|amount| amount.checked_sub(self.refunded_cumulative))
            .ok_or(RepurchaseOrderFactError::AmountOverflow)?;
        if held < 0 {
            return Err(RepurchaseOrderFactError::NegativeHeldPurse);
        }
        Ok(())
    }

    fn ensure_identity(
        &self,
        associated_key: RepurchaseRequestKey,
        owner: Owner,
        actual_stock: StockIdx,
        expected_stock: StockIdx,
    ) -> Result<(), RepurchaseOrderFactError> {
        if associated_key != self.request_key {
            return Err(RepurchaseOrderFactError::RequestMismatch);
        }
        if owner != self.owner {
            return Err(RepurchaseOrderFactError::OwnerMismatch);
        }
        if actual_stock != expected_stock {
            return Err(RepurchaseOrderFactError::StockMismatch);
        }
        Ok(())
    }

    fn bind_order_ref(&mut self, order_ref: OrderRef) -> Result<(), RepurchaseOrderFactError> {
        match self.order_ref {
            None => {
                self.order_ref = Some(order_ref);
                Ok(())
            }
            Some(bound) if bound == order_ref => Ok(()),
            Some(_) => Err(RepurchaseOrderFactError::OrderRefConflict),
        }
    }
}

/// 校验外层来源流的唯一游标；重复前缀和去重由流所有者负责。
pub fn next_repurchase_fact_cursor(
    current: u64,
    incoming: u64,
) -> Result<u64, RepurchaseOrderFactError> {
    let next = current
        .checked_add(1)
        .ok_or(RepurchaseOrderFactError::CursorOverflow)?;
    if incoming != next {
        return Err(RepurchaseOrderFactError::CursorOutOfOrder { current, incoming });
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orderbook::{AccountId, Side};

    const OWNER: Owner = (3, 7);
    const STOCK_IDX: StockIdx = 4;
    const REQUESTED_DAY: u64 = 20_261_027;
    const INITIAL: i64 = 100_000;

    fn request_key() -> RepurchaseRequestKey {
        RepurchaseRequestKey {
            plan_id: PlanId {
                owner: AccountId(77),
                sequence: 5,
            },
            stock: StockId(12),
            requested_trading_day: REQUESTED_DAY,
        }
    }

    fn fact() -> RepurchaseOrderFact {
        RepurchaseOrderFact::new(request_key(), OWNER, 30, 1_000, INITIAL).unwrap()
    }

    fn fill(order: OrderRef, owner: Owner, qty: u32, price: i64, commission: i64) -> FillFact {
        FillFact {
            order,
            owner,
            side: Side::Buy,
            price,
            qty,
            commission,
            stamp_tax: 0,
            transfer_fee: 2,
        }
    }

    #[test]
    fn new_source_has_no_guessed_order_identity() {
        let source = fact();
        assert_eq!(source.order_ref, None);
        assert_eq!(source.held_purse(), Ok(INITIAL));
        assert!(!source.closed);
    }

    #[test]
    fn a_real_fill_binds_the_order_ref_and_charges_actual_value() {
        let mut source = fact();
        source
            .apply_fill(
                request_key(),
                STOCK_IDX,
                fill((STOCK_IDX, 88), OWNER, 20, 1_200, 5),
            )
            .unwrap();
        assert_eq!(source.order_ref, Some((STOCK_IDX, 88)));
        assert_eq!(source.charged_cumulative, 24_007);
        assert_eq!(source.held_purse(), Ok(75_993));
        assert_eq!(source.releasable_amount(), Ok(None));
    }

    #[test]
    fn mismatched_owner_request_and_stock_are_rejected_atomically() {
        let cases = [
            (request_key(), (3, 8), STOCK_IDX),
            (
                RepurchaseRequestKey {
                    requested_trading_day: REQUESTED_DAY + 1,
                    ..request_key()
                },
                OWNER,
                STOCK_IDX,
            ),
            (request_key(), OWNER, STOCK_IDX + 1),
        ];
        for (key, owner, mapped_stock) in cases {
            let mut source = fact();
            let before = source;
            assert!(
                source
                    .apply_fill(key, mapped_stock, fill((STOCK_IDX, 88), owner, 1, 100, 0))
                    .is_err()
            );
            assert_eq!(source, before);
        }
    }

    #[test]
    fn partial_fill_price_improvement_and_terminal_return_accumulate_actual_cash() {
        let mut source = fact();
        source
            .apply_return(
                STOCK_IDX,
                AssociatedReturn {
                    request_key: request_key(),
                    owner: OWNER,
                    order_ref: None,
                    amount: 5_000,
                },
            )
            .unwrap();
        source
            .apply_fill(
                request_key(),
                STOCK_IDX,
                fill((STOCK_IDX, 88), OWNER, 10, 1_200, 5),
            )
            .unwrap();
        source
            .apply_return(
                STOCK_IDX,
                AssociatedReturn {
                    request_key: request_key(),
                    owner: OWNER,
                    order_ref: Some((STOCK_IDX, 88)),
                    amount: 10_000,
                },
            )
            .unwrap();
        assert_eq!(source.charged_cumulative, 12_007);
        assert_eq!(source.refunded_cumulative, 15_000);
        assert_eq!(source.held_purse(), Ok(72_993));
        assert!(!source.closed, "全成成交本身不代表订单来源已终结");
    }

    #[test]
    fn a_fill_for_the_entire_requested_quantity_does_not_close_the_source() {
        let mut source =
            RepurchaseOrderFact::new(request_key(), OWNER, 30, 1_000, 2_000_000).unwrap();
        source
            .apply_fill(
                request_key(),
                STOCK_IDX,
                fill((STOCK_IDX, 88), OWNER, 1_000, 1_200, 5),
            )
            .unwrap();
        assert!(!source.closed);
        assert_eq!(source.releasable_amount(), Ok(None));
    }

    #[test]
    fn a_conflicting_late_order_ref_does_not_change_the_source() {
        let mut source = fact();
        source
            .apply_fill(
                request_key(),
                STOCK_IDX,
                fill((STOCK_IDX, 88), OWNER, 1, 100, 0),
            )
            .unwrap();
        let before = source;
        assert_eq!(
            source.apply_return(
                STOCK_IDX,
                AssociatedReturn {
                    request_key: request_key(),
                    owner: OWNER,
                    order_ref: Some((STOCK_IDX, 89)),
                    amount: 1,
                },
            ),
            Err(RepurchaseOrderFactError::OrderRefConflict)
        );
        assert_eq!(source, before);
    }

    #[test]
    fn a_bound_source_requires_matching_refunds_and_overflow_is_atomic() {
        let mut source = fact();
        source
            .apply_fill(
                request_key(),
                STOCK_IDX,
                fill((STOCK_IDX, 88), OWNER, 1, 100, 0),
            )
            .unwrap();
        let before = source;
        assert_eq!(
            source.apply_return(
                STOCK_IDX,
                AssociatedReturn {
                    request_key: request_key(),
                    owner: OWNER,
                    order_ref: None,
                    amount: 1,
                },
            ),
            Err(RepurchaseOrderFactError::MissingBoundOrderRef)
        );
        assert_eq!(source, before);

        let mut overflowing =
            RepurchaseOrderFact::new(request_key(), OWNER, 30, 1, i64::MAX).unwrap();
        let before = overflowing;
        assert_eq!(
            overflowing.apply_fill(
                request_key(),
                STOCK_IDX,
                fill((STOCK_IDX, 89), OWNER, 2, i64::MAX, 0),
            ),
            Err(RepurchaseOrderFactError::AmountOverflow)
        );
        assert_eq!(overflowing, before);
    }

    #[test]
    fn cursor_accepts_only_the_checked_next_value() {
        assert_eq!(next_repurchase_fact_cursor(8, 9), Ok(9));
        assert_eq!(
            next_repurchase_fact_cursor(8, 8),
            Err(RepurchaseOrderFactError::CursorOutOfOrder {
                current: 8,
                incoming: 8
            })
        );
        assert_eq!(
            next_repurchase_fact_cursor(u64::MAX, 0),
            Err(RepurchaseOrderFactError::CursorOverflow)
        );
    }

    #[test]
    fn only_closed_zero_held_source_returns_a_pure_release_condition() {
        let mut source = fact();
        source.mark_closed().unwrap();
        assert_eq!(source.releasable_amount(), Ok(None));

        let mut empty = RepurchaseOrderFact::new(request_key(), OWNER, 30, 1_000, 0).unwrap();
        empty.mark_closed().unwrap();
        assert_eq!(empty.releasable_amount(), Ok(Some(0)));
    }

    #[test]
    fn negative_money_overflow_and_over_return_are_rejected() {
        assert_eq!(
            RepurchaseOrderFact::new(request_key(), OWNER, 0, 1, -1),
            Err(RepurchaseOrderFactError::NegativeInitialPurse { amount: -1 })
        );
        let mut source = RepurchaseOrderFact::new(request_key(), OWNER, 0, 1, i64::MAX).unwrap();
        source.charged_cumulative = i64::MAX;
        let before = source;
        assert_eq!(
            source.apply_return(
                STOCK_IDX,
                AssociatedReturn {
                    request_key: request_key(),
                    owner: OWNER,
                    order_ref: None,
                    amount: 1,
                },
            ),
            Err(RepurchaseOrderFactError::NegativeHeldPurse)
        );
        assert_eq!(source, before);
    }

    #[test]
    fn a_return_cannot_be_the_first_canonical_order_binding() {
        let mut source = fact();
        let before = source;
        assert!(
            source.apply_return(
                STOCK_IDX,
                AssociatedReturn {
                    request_key: request_key(),
                    owner: OWNER,
                    order_ref: Some((STOCK_IDX, 88)),
                    amount: 1,
                },
            )
            .is_err()
        );
        assert_eq!(source, before);
    }

    #[test]
    fn corrupted_public_balances_are_rejected_without_mutation() {
        let cases = [
            (-1, 0, 0),
            (INITIAL, -1, 0),
            (INITIAL, 0, -1),
            (INITIAL, INITIAL + 1, 0),
        ];
        for (initial, charged, refunded) in cases {
            let mut source = fact();
            source.purse_initial = initial;
            source.charged_cumulative = charged;
            source.refunded_cumulative = refunded;
            let before = source;

            assert!(source.held_purse().is_err());
            assert!(source.releasable_amount().is_err());
            assert!(source
                .apply_fill(
                    request_key(),
                    STOCK_IDX,
                    fill((STOCK_IDX, 88), OWNER, 1, 10, 0),
                )
                .is_err());
            assert_eq!(source, before);
            assert!(source
                .apply_return(
                    STOCK_IDX,
                    AssociatedReturn {
                        request_key: request_key(),
                        owner: OWNER,
                        order_ref: None,
                        amount: 1,
                    },
                )
                .is_err());
            assert_eq!(source, before);
        }
    }
}
