use crate::{Money, Side, StockCode};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct SaveCandidateKey {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub seq: u64,
    pub settled_date: crate::CivilDate,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[ts(export)]
pub struct PlayerWorkingOrder {
    #[serde(with = "crate::session::memberships::account_id_decimal")]
    #[ts(type = "string")]
    pub owner: crate::AccountId,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub id: u64,
    pub code: StockCode,
    pub side: Side,
    pub price: Money,
    pub remaining_qty: u32,
    pub venue: String,
    pub frozen: String,
}

impl crate::session::GameSession {
    pub fn player_working_orders(&self) -> Vec<PlayerWorkingOrder> {
        self.account_working_orders(crate::AccountId(0))
    }

    pub fn account_working_orders(&self, account: crate::AccountId) -> Vec<PlayerWorkingOrder> {
        let mut orders = Vec::new();
        for (code, auction_orders) in &self.state.auction_orders {
            for order in auction_orders {
                if order.owner != account || order.qty == 0 {
                    continue;
                }
                orders.push(PlayerWorkingOrder {
                    owner: account,
                    id: order.order_id,
                    code: code.clone(),
                    side: order.side,
                    price: order.limit,
                    remaining_qty: order.qty,
                    venue: "auction".into(),
                    frozen: if order.side == Side::Buy {
                        "cash"
                    } else {
                        "shares"
                    }
                    .into(),
                });
            }
        }
        for (code, market) in &self.state.markets {
            for order in market.resting_orders_for(account) {
                if order.owner != account || order.qty == 0 {
                    continue;
                }
                orders.push(PlayerWorkingOrder {
                    owner: account,
                    id: order.id.0,
                    code: code.clone(),
                    side: order.side,
                    price: order.price,
                    remaining_qty: order.qty,
                    venue: "continuous".into(),
                    frozen: if order.side == Side::Buy {
                        "cash"
                    } else {
                        "shares"
                    }
                    .into(),
                });
            }
        }
        orders.sort_by_key(|order| order.id);
        orders
    }
}
