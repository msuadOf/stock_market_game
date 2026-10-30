use crate::{Money, Side, StockId};

/// 委托所在交易场所（ADR 0041 数值枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum OrderVenue {
    Auction,
    Continuous,
}

/// 委托冻结资产类型（ADR 0041 数值枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum FrozenAsset {
    Cash,
    Shares,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct SaveCandidateKey {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub seq: u64,
    pub settled_date: crate::CivilDate,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct PlayerWorkingOrder {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub id: u64,
    pub code: StockId,
    pub side: Side,
    pub price: Money,
    pub remaining_qty: u32,
    pub venue: OrderVenue,
    pub frozen: FrozenAsset,
}

impl crate::session::GameSession {
    pub fn player_working_orders(&self) -> Vec<PlayerWorkingOrder> {
        let mut orders = Vec::new();
        // D5 attention：runtime 形态（desk 镜像在位）——玩家在途从镜像
        // （desk 权威）；venue=当前相位撮合模式。
        if !self.state.desk_book_mirror.is_empty() {
            return working_orders_from_mirror(
                &self.state.desk_book_mirror,
                self.runtime_mirror_phase(),
            );
        }
        for (code, market) in &self.state.markets {
            for order in market.resting_orders_for(crate::AccountId(0)) {
                if order.owner != crate::AccountId(0) || order.qty == 0 {
                    continue;
                }
                orders.push(PlayerWorkingOrder {
                    id: order.id.0,
                    code: *code,
                    side: order.side,
                    price: order.price,
                    remaining_qty: order.qty,
                    venue: if market.matching_mode() == crate::orderbook::MatchingMode::Auction {
                        OrderVenue::Auction
                    } else {
                        OrderVenue::Continuous
                    },
                    frozen: if order.side == Side::Buy {
                        FrozenAsset::Cash
                    } else {
                        FrozenAsset::Shares
                    },
                });
            }
        }
        orders.sort_by_key(|order| order.id);
        orders
    }
}

/// 将单条盘口镜像行转换为玩家在途委托；仅忽略非玩家委托与零数量行。
pub(in crate::session) fn player_working_order_from_mirror(
    code: StockId,
    owner: crate::AccountId,
    side: Side,
    price: Money,
    qty: u32,
    serial: u64,
    phase: crate::TradingPhase,
) -> Option<PlayerWorkingOrder> {
    if owner != crate::AccountId(0) || qty == 0 {
        return None;
    }
    Some(PlayerWorkingOrder {
        id: serial,
        code,
        side,
        price,
        remaining_qty: qty,
        venue: if matches!(
            phase,
            crate::TradingPhase::CallAuction | crate::TradingPhase::ClosingAuction
        ) {
            OrderVenue::Auction
        } else {
            OrderVenue::Continuous
        },
        frozen: if side == Side::Buy {
            FrozenAsset::Cash
        } else {
            FrozenAsset::Shares
        },
    })
}

/// 生产读面与私有 candidate 共用同一簿投影，不临时安装后再读取。
pub(in crate::session) fn working_orders_from_mirror(
    mirrors: &std::collections::BTreeMap<StockId, std::sync::Arc<crate::session::DeskBookMirror>>,
    phase: crate::TradingPhase,
) -> Vec<PlayerWorkingOrder> {
    let mut orders = Vec::new();
    for (code, mirror) in mirrors {
        for (owner, side, price, qty, serial, _, _) in &mirror.orders {
            #[cfg(test)]
            crate::session::runtime_mirror::record_working_order_row_visit_for_test();
            if let Some(order) = player_working_order_from_mirror(
                *code, *owner, *side, *price, *qty, *serial, phase,
            ) {
                orders.push(order);
            }
        }
    }
    orders.sort_by_key(|order| order.id);
    orders
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccountId, TradingPhase};
    use std::{collections::BTreeMap, sync::Arc};

    #[test]
    fn mirror_working_orders_keep_full_ids_and_filter_before_projection() {
        let first_id = crate::runtime::group::desk_order_id(0, 7);
        let second_id = crate::runtime::group::desk_order_id(1, 7);
        let mirrors = BTreeMap::from([
            (
                StockId(0),
                Arc::new(crate::session::DeskBookMirror {
                    orders: vec![
                        (
                            AccountId(1),
                            Side::Buy,
                            Money::from_cents(i64::MAX),
                            u32::MAX,
                            1,
                            i64::MAX,
                            0,
                        ),
                        (
                            AccountId(0),
                            Side::Buy,
                            Money::from_cents(i64::MAX),
                            0,
                            2,
                            i64::MAX,
                            0,
                        ),
                        (
                            AccountId(0),
                            Side::Sell,
                            Money::from_cents(1_005),
                            100,
                            first_id + 1,
                            0,
                            0,
                        ),
                        (
                            AccountId(0),
                            Side::Buy,
                            Money::from_cents(995),
                            100,
                            first_id,
                            0,
                            0,
                        ),
                    ],
                    last_price: Money::from_cents(1_000),
                    public_cutoff_serial: 10,
                    next_serial: 11,
                }),
            ),
            (
                StockId(1),
                Arc::new(crate::session::DeskBookMirror {
                    orders: vec![(
                        AccountId(0),
                        Side::Buy,
                        Money::from_cents(990),
                        200,
                        second_id,
                        0,
                        0,
                    )],
                    last_price: Money::from_cents(1_000),
                    public_cutoff_serial: 10,
                    next_serial: 11,
                }),
            ),
        ]);
        let expected = |venue| {
            vec![
                PlayerWorkingOrder {
                    id: first_id,
                    code: StockId(0),
                    side: Side::Buy,
                    price: Money::from_cents(995),
                    remaining_qty: 100,
                    venue,
                    frozen: FrozenAsset::Cash,
                },
                PlayerWorkingOrder {
                    id: first_id + 1,
                    code: StockId(0),
                    side: Side::Sell,
                    price: Money::from_cents(1_005),
                    remaining_qty: 100,
                    venue,
                    frozen: FrozenAsset::Shares,
                },
                PlayerWorkingOrder {
                    id: second_id,
                    code: StockId(1),
                    side: Side::Buy,
                    price: Money::from_cents(990),
                    remaining_qty: 200,
                    venue,
                    frozen: FrozenAsset::Cash,
                },
            ]
        };

        for (phase, venue) in [
            (TradingPhase::Continuous, OrderVenue::Continuous),
            (TradingPhase::CallAuction, OrderVenue::Auction),
            (TradingPhase::ClosingAuction, OrderVenue::Auction),
        ] {
            assert_eq!(working_orders_from_mirror(&mirrors, phase), expected(venue));
        }
    }
}
