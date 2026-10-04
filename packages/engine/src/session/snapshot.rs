//! 构建完整快照、运行时快照与玩家可见快照载荷。
//! 序列化契约不变：字段名、serde 属性与 JSON 形状与抽出前逐字节一致。

use super::*;

/// 市场快照子结构（单股）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct MarketSnap {
    pub last_price: Money,
    pub last_close: Money,
    pub best_bid: Option<Money>,
    pub best_ask: Option<Money>,
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
    #[serde(with = "crate::money::cents_decimal")]
    #[ts(type = "string")]
    pub invested_cents: i64,
    #[serde(with = "crate::money::cents_decimal")]
    #[ts(type = "string")]
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

/// 完整玩家状态快照（首次连接/重连）。
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

/// 单次 Snapshot 构建中，由全部在簿订单派生的资源预留总额。
#[derive(Default)]
struct LiveOrderReservations {
    reserved_cash: BTreeMap<AccountId, Money>,
    reserved_sell_qty: BTreeMap<AccountId, BTreeMap<StockCode, u32>>,
}

impl LiveOrderReservations {
    // 订单事实按原调用点逐项传入，避免增加仅用于参数打包的 DTO。
    #[allow(clippy::too_many_arguments)]
    fn record_order(
        &mut self,
        owner: AccountId,
        code: &StockCode,
        side: Side,
        price: Money,
        remaining_qty: u32,
        filled_value: Money,
        config: &GameConfig,
    ) {
        if side == Side::Sell {
            let reserved = self
                .reserved_sell_qty
                .entry(owner)
                .or_default()
                .entry(code.clone())
                .or_default();
            *reserved = reserved
                .checked_add(remaining_qty)
                .expect("validated live sell reservations cannot exceed u32 holdings");
        }
        let required = live_cash_reservation(config, side, price, remaining_qty, filled_value)
            .expect("validated live cash reservation must fit Money");
        let reserved = self.reserved_cash.entry(owner).or_default();
        *reserved = reserved
            .add(required)
            .expect("validated live cash reservations cannot exceed account cash");
    }

    fn take_for(&mut self, account: AccountId) -> (Money, BTreeMap<StockCode, u32>) {
        (
            self.reserved_cash.remove(&account).unwrap_or(Money::ZERO),
            self.reserved_sell_qty.remove(&account).unwrap_or_default(),
        )
    }
}

impl GameSession {
    /// 完整状态快照（首次连/重连/存档）。
    ///
    /// 遍历 markets/accounts 取只读值快照：market 的 last_price/last_close/
    /// best_bid/best_ask；account 的 cash + positions（qty/t1_locked/
    /// invested_cents/recovered_cents）。snapshot 自身只读、不影响 session 状态。
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot_inner(true, false)
    }

    /// 高频运行快照：刷新报价、账户、昨收与当前交易日累计，但不复制历史 K 线。
    /// 完整历史 K 线只在首次连接、重连和读档时通过 [`Self::snapshot`] 同步。
    pub fn runtime_snapshot(&self) -> Snapshot {
        self.snapshot_inner(false, false)
    }

    pub(super) fn snapshot_inner(
        &self,
        include_daily_candles: bool,
        include_npc_accounts: bool,
    ) -> Snapshot {
        let mut reservations = LiveOrderReservations::default();
        for (code, market) in &self.state.markets {
            for order in market.resting_orders() {
                reservations.record_order(
                    order.owner,
                    code,
                    order.side,
                    order.price,
                    order.qty,
                    order.filled_value,
                    &self.state.setup.config,
                );
            }
        }
        for (code, orders) in &self.state.auction_orders {
            for order in orders {
                reservations.record_order(
                    order.owner,
                    code,
                    order.side,
                    order.limit,
                    order.qty,
                    Money::ZERO,
                    &self.state.setup.config,
                );
            }
        }
        let markets = self
            .state
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
                        bids: m.bid_depth(),
                        asks: m.ask_depth(),
                    },
                )
            })
            .collect();
        let accounts = self
            .state
            .accounts
            .iter()
            .filter(|(id, _)| include_npc_accounts || id.0 == 0)
            .map(|(id, a)| {
                let (reserved_cash, reserved_sell_qty) = reservations.take_for(*id);
                (
                    *id,
                    AccountSnap {
                        cash: a.cash(),
                        positions: a
                            .positions()
                            .iter()
                            .map(|(c, p)| {
                                (
                                    c.clone(),
                                    PositionSnap {
                                        qty: p.qty(),
                                        t1_locked: p.t1_locked(),
                                        invested_cents: p.invested_cents(),
                                        recovered_cents: p.recovered_cents(),
                                    },
                                )
                            })
                            .collect(),
                        reserved_cash,
                        reserved_sell_qty,
                    },
                )
            })
            .collect();
        Snapshot {
            seq: self.state.seq,
            tick: self.state.tick,
            day: self.state.day,
            phase: self.phase(),
            markets,
            accounts,
            daily_candles: if include_daily_candles {
                self.state
                    .candle_book
                    .histories()
                    .iter()
                    .map(|(code, history)| (code.clone(), history.iter().cloned().collect()))
                    .collect()
            } else {
                BTreeMap::new()
            },
            // 当前交易日累计很小且是 UI 权威统计来源；成交/竞价完成等低频状态
            // 快照必须携带它，不能迫使客户端从可压缩逐笔流重算。
            active_daily_candles: self.state.candle_book.active().clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_reservations_combine_continuous_auction_and_partial_orders() {
        let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
        setup.npcs.inst_count = 0;
        let mut second = setup.stocks[0].clone();
        second.code = StockCode("600889".to_string());
        setup.stocks.push(second);
        let mut session = GameSession::new(setup, 42).unwrap();
        let first = session.state.setup.stocks[0].code.clone();
        let second = session.state.setup.stocks[1].code.clone();
        let owner = AccountId(0);
        for (code, id, side, price, qty, filled_qty, filled_value) in [
            (&first, 1, Side::Buy, 900, 150, 50, 45_000),
            (&first, 2, Side::Sell, 1_100, 30, 70, 77_000),
            (&second, 3, Side::Sell, 1_100, 200, 0, 0),
        ] {
            session
                .state
                .markets
                .get_mut(code)
                .unwrap()
                .place(Order {
                    id: OrderId(id),
                    side,
                    price: Money::from_cents(price),
                    qty,
                    original_qty: qty + filled_qty,
                    filled_qty,
                    filled_value: Money::from_cents(filled_value),
                    owner,
                    seq: id,
                })
                .unwrap();
        }
        for (code, id, side, qty) in [
            (&first, 4, Side::Sell, 70),
            (&second, 5, Side::Sell, 100),
            (&second, 6, Side::Buy, 100),
        ] {
            session
                .state
                .auction_orders
                .entry(code.clone())
                .or_default()
                .push(AuctionOrderSnap {
                    order_id: id,
                    owner,
                    side,
                    limit: Money::from_cents(1_000),
                    qty,
                });
        }
        let snapshot = session.runtime_snapshot();
        let account = &snapshot.accounts[&owner];
        // 部分买单只预留剩余成交额及累计费用增量；未成交买单仍收首笔佣金下限。
        assert_eq!(account.reserved_cash, Money::from_cents(235_503));
        assert_eq!(
            account.reserved_sell_qty,
            BTreeMap::from([(first, 100), (second, 300)])
        );
        assert!(snapshot.daily_candles.is_empty());
        assert_eq!(
            session.runtime_snapshot().accounts[&owner].reserved_cash,
            account.reserved_cash
        );
    }

    #[test]
    fn live_order_reservations_take_is_isolated_and_empty_accounts_have_zero() {
        let config = GameConfig::proposed_defaults();
        let code = StockCode("600888".to_string());
        let mut reservations = LiveOrderReservations::default();
        reservations.record_order(
            AccountId(1),
            &code,
            Side::Buy,
            Money::from_cents(1_000),
            100,
            Money::ZERO,
            &config,
        );
        reservations.record_order(
            AccountId(2),
            &code,
            Side::Sell,
            Money::from_cents(1_000),
            200,
            Money::ZERO,
            &config,
        );
        assert_eq!(
            reservations.take_for(AccountId(0)),
            (Money::ZERO, BTreeMap::new())
        );
        assert_eq!(
            reservations.take_for(AccountId(1)),
            (Money::from_cents(100_501), BTreeMap::new())
        );
        assert_eq!(
            reservations.take_for(AccountId(2)),
            (Money::ZERO, BTreeMap::from([(code, 200)]))
        );
        assert_eq!(
            reservations.take_for(AccountId(1)),
            (Money::ZERO, BTreeMap::new())
        );
    }

    #[test]
    #[should_panic(expected = "validated live sell reservations cannot exceed u32 holdings")]
    fn live_order_reservations_sell_overflow_is_explicit() {
        let config = GameConfig::proposed_defaults();
        let code = StockCode("600888".to_string());
        let mut reservations = LiveOrderReservations::default();
        for qty in [u32::MAX, 1] {
            reservations.record_order(
                AccountId(0),
                &code,
                Side::Sell,
                Money::from_cents(1),
                qty,
                Money::ZERO,
                &config,
            );
        }
    }

    #[test]
    #[should_panic(expected = "validated live cash reservations cannot exceed account cash")]
    fn live_order_reservations_cash_overflow_is_explicit() {
        let mut config = GameConfig::proposed_defaults();
        config.commission_rate = 0.0;
        config.commission_min = Money::ZERO;
        let code = StockCode("600888".to_string());
        let mut reservations = LiveOrderReservations::default();
        for _ in 0..2 {
            reservations.record_order(
                AccountId(0),
                &code,
                Side::Buy,
                Money::from_cents(i64::MAX / 2 + 1),
                1,
                Money::ZERO,
                &config,
            );
        }
    }
}
