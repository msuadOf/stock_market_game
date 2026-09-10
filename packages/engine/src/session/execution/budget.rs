//! NPC 意图现金预算：按仍真实冻结的余额做确定性累计，不足则缩量或丢弃。

use super::*;

impl GameSession {
    /// 工作单对齐后，以仍真实冻结的余额对本轮 NPC 买单做确定性累计预算。
    /// 资金不足是正常的策略约束：缩为可负担整手，连一手也不足则不进入路由队列。
    pub(in crate::session) fn cap_npc_intents_to_available_cash(
        &self,
        pending: Vec<(AccountId, Intent)>,
    ) -> Vec<(AccountId, Intent)> {
        let (continuous, auction) = self.working_orders_by_account();
        let mut reserved_by_account: BTreeMap<AccountId, Money> = BTreeMap::new();
        for (account, orders) in continuous {
            for (_, order) in orders {
                let required = match order.side {
                    Side::Buy => buy_order_reservation(
                        &self.setup.config,
                        order.price,
                        order.qty,
                        order.filled_value,
                    ),
                    Side::Sell => sell_order_fee_reservation(
                        &self.setup.config,
                        order.price,
                        order.qty,
                        order.filled_value,
                    ),
                }
                .expect("validated continuous reservation must remain computable");
                let total = reserved_by_account.entry(account).or_insert(Money::ZERO);
                *total = total
                    .add(required)
                    .expect("live reservation total must remain representable");
            }
        }
        for (account, orders) in auction {
            for (_, order) in orders {
                let required = match order.side {
                    Side::Buy => buy_order_reservation(
                        &self.setup.config,
                        order.limit,
                        order.qty,
                        Money::ZERO,
                    ),
                    Side::Sell => sell_order_fee_reservation(
                        &self.setup.config,
                        order.limit,
                        order.qty,
                        Money::ZERO,
                    ),
                }
                .expect("validated auction reservation must remain computable");
                let total = reserved_by_account.entry(account).or_insert(Money::ZERO);
                *total = total
                    .add(required)
                    .expect("live reservation total must remain representable");
            }
        }

        let mut planned = Vec::with_capacity(pending.len());
        for (account, intent) in pending {
            let Intent::PlaceLimit {
                code,
                side,
                price,
                qty,
            } = intent
            else {
                planned.push((account, intent));
                continue;
            };
            let Some(account_cash) = self.accounts.get(&account).map(|value| value.cash) else {
                planned.push((
                    account,
                    Intent::PlaceLimit {
                        code,
                        side,
                        price,
                        qty,
                    },
                ));
                continue;
            };
            let already_reserved = *reserved_by_account.get(&account).unwrap_or(&Money::ZERO);
            let available = account_cash
                .sub(already_reserved)
                .expect("validated live and planned reservations cannot exceed account cash");
            if side == Side::Sell {
                match sell_order_fee_reservation(&self.setup.config, price, qty, Money::ZERO) {
                    Ok(required) if required <= available => {
                        reserved_by_account.insert(
                            account,
                            already_reserved
                                .add(required)
                                .expect("planned reservation total must remain representable"),
                        );
                    }
                    Ok(_) => continue,
                    Err(_) => {
                        // 非法/溢出意图保留给权威路由层，后者会生成可见 SettlementError。
                    }
                }
                planned.push((
                    account,
                    Intent::PlaceLimit {
                        code,
                        side,
                        price,
                        qty,
                    },
                ));
                continue;
            }
            let affordable_qty =
                match affordable_board_lot_buy_qty(&self.setup.config, price, qty, available) {
                    Ok(quantity) => quantity,
                    Err(_) => {
                        // 非法/溢出意图保留给权威路由层，后者会生成可见 SettlementError。
                        planned.push((
                            account,
                            Intent::PlaceLimit {
                                code,
                                side,
                                price,
                                qty,
                            },
                        ));
                        continue;
                    }
                };
            let Some(affordable_qty) = affordable_qty else {
                continue;
            };
            let required =
                buy_order_reservation(&self.setup.config, price, affordable_qty, Money::ZERO)
                    .expect("affordable quantity reservation must remain computable");
            reserved_by_account.insert(
                account,
                already_reserved
                    .add(required)
                    .expect("planned reservation total must remain representable"),
            );
            planned.push((
                account,
                Intent::PlaceLimit {
                    code,
                    side,
                    price,
                    qty: affordable_qty,
                },
            ));
        }
        planned
    }
}
