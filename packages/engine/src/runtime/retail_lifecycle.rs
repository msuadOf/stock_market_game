//! 默认 Retail 诊断只携带真实订单生命周期；不采集 Quote、计划或历史。

use super::{OrderRef, Owner, StockIdx};
use crate::{CivilDate, Side};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Termination {
    Canceled,
    Aborted,
}

/// TradeRef 的 ordinal 仅属于携带它的原报告，离开原报告前必须解析。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawFact {
    Submitted {
        order: OrderRef,
        owner: Owner,
        side: Side,
        qty: u32,
    },
    TradeRef {
        ordinal: u64,
    },
    Terminated {
        order: OrderRef,
        owner: Owner,
        qty: u32,
        end: Termination,
    },
}

/// Tick 与 Drain 明确分域，不用虚构 round 或次日日期补造同一来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BatchContext {
    Tick {
        tick: u64,
        round: u32,
        stock: StockIdx,
    },
    Drain {
        ended_date: CivilDate,
        final_tick: u64,
        stock: StockIdx,
    },
}

impl BatchContext {
    pub(crate) fn stock(self) -> StockIdx {
        match self {
            Self::Tick { stock, .. } | Self::Drain { stock, .. } => stock,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactKind {
    Submitted {
        order: OrderRef,
        owner: Owner,
        side: Side,
        qty: u32,
    },
    Filled {
        order: OrderRef,
        owner: Owner,
        side: Side,
        qty: u32,
    },
    Terminated {
        order: OrderRef,
        owner: Owner,
        qty: u32,
        end: Termination,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fact {
    pub context: BatchContext,
    pub kind: FactKind,
}

/// 在原报告内解析 TradeRef，既核逐项 Encounter order，也核完整覆盖。
pub(crate) fn resolve_report(
    context: BatchContext,
    raw: Vec<RawFact>,
    trades: &[super::protocol::DeskTradeFact],
) -> Result<Vec<Fact>, super::protocol::RuntimeError> {
    #[cfg(test)]
    let observation = cost_probe::begin(raw.len(), raw.capacity(), context);
    let stock = context.stock();
    let fail = |detail: &str| super::protocol::RuntimeError::ProtocolInvariant {
        detail: format!("retail lifecycle {context:?}：{detail}"),
    };
    let valid = |order: OrderRef| order.0 == stock && order.1 <= u64::from(u32::MAX);
    let capacity = raw
        .len()
        .checked_add(trades.len())
        .ok_or_else(|| fail("报告容量溢出"))?;
    let mut result = Vec::new();
    result
        .try_reserve(capacity)
        .map_err(|_| fail("报告容量无法分配"))?;
    let mut expected = 0usize;
    for entry in raw {
        let kind = match entry {
            RawFact::Submitted {
                order,
                owner,
                side,
                qty,
            } => {
                if !valid(order) {
                    return Err(fail("Submitted 的 OrderRef 不属于原报告"));
                }
                FactKind::Submitted {
                    order,
                    owner,
                    side,
                    qty,
                }
            }
            RawFact::Terminated {
                order,
                owner,
                qty,
                end,
            } => {
                if !valid(order) {
                    return Err(fail("Terminated 的 OrderRef 不属于原报告"));
                }
                FactKind::Terminated {
                    order,
                    owner,
                    qty,
                    end,
                }
            }
            RawFact::TradeRef { ordinal } => {
                if usize::try_from(ordinal).ok() != Some(expected) {
                    return Err(fail("TradeRef ordinal 不连续或乱序"));
                }
                let trade = trades
                    .get(expected)
                    .ok_or_else(|| fail("TradeRef 越过原 trades"))?;
                if trade.stock != stock
                    || trade.maker_order_serial >> 32 != u64::from(stock)
                    || trade.taker_order_serial >> 32 != u64::from(stock)
                {
                    return Err(fail("成交双腿编码身份不属于原报告"));
                }
                let seller = FactKind::Filled {
                    order: (stock, trade.maker_order_serial & u64::from(u32::MAX)),
                    owner: trade.maker,
                    side: Side::Sell,
                    qty: trade.qty,
                };
                let buyer = FactKind::Filled {
                    order: (stock, trade.taker_order_serial & u64::from(u32::MAX)),
                    owner: trade.taker,
                    side: Side::Buy,
                    qty: trade.qty,
                };
                result.push(Fact {
                    context,
                    kind: seller,
                });
                expected = expected
                    .checked_add(1)
                    .ok_or_else(|| fail("成交序号溢出"))?;
                buyer
            }
        };
        result.push(Fact { context, kind });
    }
    if expected != trades.len() {
        return Err(fail("TradeRef 未完整覆盖原 trades"));
    }
    #[cfg(test)]
    cost_probe::finish(observation, result.len(), result.capacity());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::protocol::DeskTradeFact;

    fn trade(stock: u16) -> DeskTradeFact {
        DeskTradeFact {
            stock,
            price: 995,
            qty: 100,
            maker: (0, 1),
            taker: (1, 0),
            taker_side: Side::Sell,
            maker_fees_cents: 0,
            taker_fees_cents: 0,
            maker_fee_split: crate::fees::FeeComponents::ZERO,
            taker_fee_split: crate::fees::FeeComponents::ZERO,
            maker_order_serial: (u64::from(stock) << 32) | 1,
            taker_order_serial: (u64::from(stock) << 32) | 2,
        }
    }

    #[test]
    fn original_report_ordinal_requires_exact_order_and_complete_coverage() {
        let context = BatchContext::Tick {
            tick: 2,
            round: 1,
            stock: 1,
        };
        let trades = [trade(1), trade(1)];
        for raw in [
            vec![],
            vec![RawFact::TradeRef { ordinal: 0 }],
            vec![
                RawFact::TradeRef { ordinal: 0 },
                RawFact::TradeRef { ordinal: 0 },
            ],
            vec![
                RawFact::TradeRef { ordinal: 1 },
                RawFact::TradeRef { ordinal: 0 },
            ],
            vec![
                RawFact::TradeRef { ordinal: 0 },
                RawFact::TradeRef { ordinal: 2 },
            ],
        ] {
            assert!(resolve_report(context, raw, &trades).is_err());
        }
        let resolved = resolve_report(
            context,
            vec![
                RawFact::TradeRef { ordinal: 0 },
                RawFact::TradeRef { ordinal: 1 },
            ],
            &trades,
        )
        .unwrap();
        assert_eq!(resolved.len(), 4);
        assert_eq!(
            resolved[0].kind,
            FactKind::Filled {
                order: (1, 1),
                owner: (0, 1),
                side: Side::Sell,
                qty: 100
            }
        );
        assert_eq!(
            resolved[1].kind,
            FactKind::Filled {
                order: (1, 2),
                owner: (1, 0),
                side: Side::Buy,
                qty: 100
            }
        );
    }

    #[test]
    fn stock_identity_is_checked_and_tick_drain_ordinals_remain_separate() {
        let tick = BatchContext::Tick {
            tick: 30,
            round: 1,
            stock: 1,
        };
        let drain = BatchContext::Drain {
            ended_date: CivilDate::from_ymd(2030, 1, 2).unwrap(),
            final_tick: 30,
            stock: 1,
        };
        let raw = vec![RawFact::TradeRef { ordinal: 0 }];
        let a = resolve_report(tick, raw.clone(), &[trade(1)]).unwrap();
        let b = resolve_report(drain, raw.clone(), &[trade(1)]).unwrap();
        assert_eq!(a[0].kind, b[0].kind);
        assert_ne!(a[0].context, b[0].context);
        let mut bad = trade(1);
        bad.maker_order_serial = 1;
        assert!(resolve_report(tick, raw.clone(), &[bad]).is_err());
        assert!(resolve_report(tick, raw, &[trade(0)]).is_err());
        assert!(resolve_report(
            tick,
            vec![RawFact::Submitted {
                order: (0, 1),
                owner: (0, 1),
                side: Side::Sell,
                qty: 100
            }],
            &[]
        )
        .is_err());
    }
}

/// 仅测试 arm 时记录真实报告，不给 release 路径增加计时或采样字段。
#[cfg(test)]
pub(crate) mod cost_probe {
    use super::*;
    use std::{cell::RefCell, time::Instant};
    #[derive(Debug)]
    pub(crate) struct Observation {
        pub context: BatchContext,
        pub raw_len: usize,
        pub raw_capacity: usize,
        pub resolved_len: usize,
        pub resolved_capacity: usize,
        pub raw_element_bytes: usize,
        pub resolved_element_bytes: usize,
        pub elapsed_ns: u128,
    }
    thread_local! { static STATE: RefCell<Option<Vec<Observation>>> = const { RefCell::new(None) }; }
    pub(crate) struct Arm;
    impl Arm {
        pub(crate) fn new() -> Self {
            STATE.with(|state| {
                assert!(state.borrow().is_none());
                *state.borrow_mut() = Some(Vec::new());
            });
            Self
        }
        pub(crate) fn take(self) -> Vec<Observation> {
            STATE.with(|state| state.borrow_mut().take().expect("成本观测必须已 arm"))
        }
    }
    impl Drop for Arm {
        fn drop(&mut self) {
            STATE.with(|state| *state.borrow_mut() = None);
        }
    }
    pub(super) fn begin(
        raw_len: usize,
        raw_capacity: usize,
        context: BatchContext,
    ) -> Option<(Instant, usize, usize, BatchContext)> {
        STATE.with(|state| {
            state
                .borrow()
                .as_ref()
                .map(|_| (Instant::now(), raw_len, raw_capacity, context))
        })
    }
    pub(super) fn finish(
        input: Option<(Instant, usize, usize, BatchContext)>,
        len: usize,
        capacity: usize,
    ) {
        if let Some((start, raw_len, raw_capacity, context)) = input {
            STATE.with(|state| {
                state
                    .borrow_mut()
                    .as_mut()
                    .expect("观测期间必须保持 arm")
                    .push(Observation {
                        context,
                        raw_len,
                        raw_capacity,
                        resolved_len: len,
                        resolved_capacity: capacity,
                        raw_element_bytes: std::mem::size_of::<RawFact>(),
                        resolved_element_bytes: std::mem::size_of::<Fact>(),
                        elapsed_ns: start.elapsed().as_nanos(),
                    })
            });
        }
    }
}
