//! 快照接缝：完整/运行时快照构建与玩家可见的快照载荷类型（W1-Task 3 抽出）。
//! 序列化契约不变：字段名、serde 属性与 JSON 形状与抽出前逐字节一致。

use super::*;

/// 市场快照子结构（单股）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct MarketSnap {
    pub last_price: Money,
    pub last_close: Money,
    pub best_bid: Option<Money>,
    pub best_ask: Option<Money>,
    /// 仅存档等可信内部边界携带。面向玩家的运行快照必须为 `None`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fundamental_value: Option<Money>,
    /// 买盘深度（价高→低，每价聚合总量）。前端取前 N 档渲染五档盘口。
    #[serde(with = "super::js_safe_depth")]
    #[ts(type = "Array<[Money, number]>")]
    pub bids: Vec<(Money, u64)>,
    /// 卖盘深度（价低→高，每价聚合总量）。
    #[serde(with = "super::js_safe_depth")]
    #[ts(type = "Array<[Money, number]>")]
    pub asks: Vec<(Money, u64)>,
}

/// 持仓快照子结构（单只股票）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct PositionSnap {
    pub qty: u32,
    pub t1_locked: u32,
    pub invested_cents: i64,
    pub recovered_cents: i64,
}

/// 账户快照子结构。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct AccountSnap {
    pub cash: Money,
    pub positions: BTreeMap<StockCode, PositionSnap>,
    /// 已被当日全部未成交委托占用的资金。
    pub reserved_cash: Money,
    /// 已被当日未成交卖单占用的股数，按股票汇总。
    pub reserved_sell_qty: BTreeMap<StockCode, u32>,
}

/// 完整玩家状态快照（首次连接/重连）。隐藏基本面 V 不跨玩家边界泄露。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct Snapshot {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub seq: u64,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub tick: u64,
    pub day: u32,
    pub phase: TradingPhase,
    pub markets: BTreeMap<StockCode, MarketSnap>,
    pub accounts: BTreeMap<AccountId, AccountSnap>,
    /// Rust 引擎持有的已完成日 K；首次连接、重连与存档恢复均由快照同步。
    pub daily_candles: BTreeMap<StockCode, Vec<DailyCandle>>,
    /// 当前交易日正在形成的日 K。
    pub active_daily_candles: BTreeMap<StockCode, DailyCandle>,
}

impl GameSession {
    /// 完整状态快照（首次连/重连/存档）。
    ///
    /// 遍历 markets/accounts 取只读值快照：market 的 last_price/last_close/best_bid/
    /// best_ask/fundamental_value；account 的 cash + positions（qty/t1_locked/
    /// invested_cents/recovered_cents）。snapshot 自身只读、不影响 session 状态。
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot_inner(true, false, false)
    }

    /// 高频运行快照：刷新报价、账户、昨收与当前交易日累计，但不复制历史 K 线。
    /// 完整历史 K 线只在首次连接、重连和读档时通过 [`Self::snapshot`] 同步。
    pub fn runtime_snapshot(&self) -> Snapshot {
        self.snapshot_inner(false, false, false)
    }

    pub(super) fn snapshot_inner(
        &self,
        include_daily_candles: bool,
        include_fundamental_value: bool,
        include_npc_accounts: bool,
    ) -> Snapshot {
        let mut reserved_sell_qty: BTreeMap<AccountId, BTreeMap<StockCode, u32>> = BTreeMap::new();
        let mut reserved_cash: BTreeMap<AccountId, Money> = BTreeMap::new();
        let mut record_reserved_sell = |owner: AccountId, code: &StockCode, qty: u32| {
            let reserved = reserved_sell_qty
                .entry(owner)
                .or_default()
                .entry(code.clone())
                .or_default();
            *reserved = reserved
                .checked_add(qty)
                .expect("validated live sell reservations cannot exceed u32 holdings");
        };
        let mut record_reserved_cash =
            |owner: AccountId, side: Side, price: Money, qty: u32, filled_value: Money| {
                let required = match side {
                    Side::Buy => {
                        buy_order_reservation(&self.setup.config, price, qty, filled_value)
                    }
                    Side::Sell => {
                        sell_order_fee_reservation(&self.setup.config, price, qty, filled_value)
                    }
                }
                .expect("validated live cash reservation must fit Money");
                let reserved = reserved_cash.entry(owner).or_default();
                *reserved = reserved
                    .add(required)
                    .expect("validated live cash reservations cannot exceed account cash");
            };
        for (code, market) in &self.markets {
            for order in market.resting_orders() {
                match order.side {
                    Side::Buy => record_reserved_cash(
                        order.owner,
                        order.side,
                        order.price,
                        order.qty,
                        order.filled_value,
                    ),
                    Side::Sell => {
                        record_reserved_sell(order.owner, code, order.qty);
                        record_reserved_cash(
                            order.owner,
                            order.side,
                            order.price,
                            order.qty,
                            order.filled_value,
                        );
                    }
                }
            }
        }
        for (code, orders) in &self.auction_orders {
            for order in orders {
                match order.side {
                    Side::Buy => record_reserved_cash(
                        order.owner,
                        order.side,
                        order.limit,
                        order.qty,
                        Money::ZERO,
                    ),
                    Side::Sell => {
                        record_reserved_sell(order.owner, code, order.qty);
                        record_reserved_cash(
                            order.owner,
                            order.side,
                            order.limit,
                            order.qty,
                            Money::ZERO,
                        );
                    }
                }
            }
        }
        let markets = self
            .markets
            .iter()
            .map(|(code, m)| {
                (
                    code.clone(),
                    MarketSnap {
                        last_price: m.last_price(),
                        last_close: m.last_close(),
                        best_bid: m.best_bid(),
                        best_ask: m.best_ask(),
                        fundamental_value: include_fundamental_value.then(|| m.fundamental_value()),
                        bids: m.bid_depth(),
                        asks: m.ask_depth(),
                    },
                )
            })
            .collect();
        let accounts = self
            .accounts
            .iter()
            .filter(|(id, _)| include_npc_accounts || id.0 == 0)
            .map(|(id, a)| {
                (
                    *id,
                    AccountSnap {
                        cash: a.cash,
                        positions: a
                            .positions
                            .iter()
                            .map(|(c, p)| {
                                (
                                    c.clone(),
                                    PositionSnap {
                                        qty: p.qty,
                                        t1_locked: p.t1_locked,
                                        invested_cents: p.invested_cents,
                                        recovered_cents: p.recovered_cents,
                                    },
                                )
                            })
                            .collect(),
                        reserved_cash: reserved_cash.remove(id).unwrap_or(Money::ZERO),
                        reserved_sell_qty: reserved_sell_qty.remove(id).unwrap_or_default(),
                    },
                )
            })
            .collect();
        Snapshot {
            seq: self.seq,
            tick: self.tick,
            day: self.day,
            phase: self.phase(),
            markets,
            accounts,
            daily_candles: if include_daily_candles {
                self.daily_candles.clone()
            } else {
                BTreeMap::new()
            },
            // 当前交易日累计很小且是 UI 权威统计来源；成交/竞价完成等低频状态
            // 快照必须携带它，不能迫使客户端从可压缩逐笔流重算。
            active_daily_candles: self.active_daily_candles.clone(),
        }
    }
}
