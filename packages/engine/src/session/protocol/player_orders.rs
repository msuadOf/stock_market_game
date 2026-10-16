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
        /// （desk 权威）；venue=当前相位撮合模式。
        if !self.state.desk_book_mirror.is_empty() {
            let auction = matches!(
                self.phase(),
                crate::TradingPhase::CallAuction | crate::TradingPhase::ClosingAuction
            );
            for (code, mirror) in &self.state.desk_book_mirror {
                for (owner, side, price, qty, serial, _) in &mirror.orders {
                    if *owner != crate::AccountId(0) || *qty == 0 {
                        continue;
                    }
                    orders.push(PlayerWorkingOrder {
                        id: *serial,
                        code: code.clone(),
                        side: *side,
                        price: *price,
                        remaining_qty: *qty,
                        venue: if auction {
                            OrderVenue::Auction
                        } else {
                            OrderVenue::Continuous
                        },
                        frozen: if *side == Side::Buy {
                            FrozenAsset::Cash
                        } else {
                            FrozenAsset::Shares
                        },
                    });
                }
            }
            orders.sort_by_key(|order| order.id);
            return orders;
        }
        for (code, market) in &self.state.markets {
            for order in market.resting_orders_for(crate::AccountId(0)) {
                if order.owner != crate::AccountId(0) || order.qty == 0 {
                    continue;
                }
                orders.push(PlayerWorkingOrder {
                    id: order.id.0,
                    code: code.clone(),
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
