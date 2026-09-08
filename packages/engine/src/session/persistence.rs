//! 存档结构、市场、账户、订单与日 K 不变量校验。

use super::*;

/// 把可识别的 v1 存档升级为当前结构。迁移仅发生在明确标记为旧版本的
/// `SaveSlot` 上；当前版本里显式传入的 5% 配置仍由正式规则校验拒绝。
pub(super) fn migrate_save_slot(save: &SaveSlot) -> Result<SaveSlot, SessionError> {
    match save.schema_version {
        SAVE_SCHEMA_VERSION => Ok(save.clone()),
        LEGACY_SAVE_SCHEMA_VERSION => {
            let mut migrated = save.clone();
            let migrated_from_t0 = !migrated.setup.t1_enabled;
            if (migrated.setup.config.st_limit - 0.05).abs() <= f64::EPSILON {
                migrated.setup.config.st_limit = 0.10;
            }
            for stock in &mut migrated.setup.stocks {
                if stock.code.0.starts_with("300") || stock.code.0.starts_with("301") {
                    stock.category = SecurityCategory::ChiNext;
                    if (stock.limit_pct - 0.10).abs() <= f64::EPSILON {
                        stock.limit_pct = SecurityCategory::ChiNext.limit_pct();
                    }
                } else if (stock.limit_pct - 0.05).abs() <= f64::EPSILON {
                    // v1 没有证券类别；当时 5% 是风险警示股唯一可识别的持久化标记。
                    stock.category = SecurityCategory::StMainBoard;
                }
                if stock.category == SecurityCategory::StMainBoard
                    && (stock.limit_pct - 0.05).abs() <= f64::EPSILON
                {
                    stock.limit_pct = SecurityCategory::StMainBoard.limit_pct();
                }
            }
            if migrated_from_t0 {
                migrated.setup.t1_enabled = true;
                // v1 的 T+0 快照没有记录每笔持仓的买入日，无法准确重建当日新增股份。
                // 开盘集合竞价前的持仓必然来自前日；其余阶段保守锁定一日，杜绝迁移后超卖。
                let locked_until_next_day = migrated.snapshot.phase != TradingPhase::CallAuction;
                for account in migrated.snapshot.accounts.values_mut() {
                    for position in account.positions.values_mut() {
                        position.t1_locked = if locked_until_next_day {
                            position.qty
                        } else {
                            0
                        };
                    }
                }
            }
            // v1 只持久化聚合盘口，不保存连续委托的所有权与冻结信息；旧版 restore
            // 本就丢弃这些派生深度。仅在 v1 迁移时清空，v2 仍要求深度与订单逐笔一致。
            for market in migrated.snapshot.markets.values_mut() {
                market.best_bid = None;
                market.best_ask = None;
                market.bids.clear();
                market.asks.clear();
            }
            migrated.schema_version = SAVE_SCHEMA_VERSION;
            Ok(migrated)
        }
        version => Err(SessionError::InvalidSave(format!(
            "unsupported schema_version {version}; expected {SAVE_SCHEMA_VERSION} or legacy {LEGACY_SAVE_SCHEMA_VERSION}"
        ))),
    }
}

pub(super) fn validate_save_slot(save: &SaveSlot) -> Result<(), SessionError> {
    if save.schema_version != SAVE_SCHEMA_VERSION {
        return Err(SessionError::InvalidSave(format!(
            "unsupported schema_version {}; expected {}",
            save.schema_version, SAVE_SCHEMA_VERSION
        )));
    }
    save.setup
        .validate()
        .map_err(|error| SessionError::InvalidSave(format!("invalid setup: {error}")))?;

    let expected_markets: BTreeSet<StockCode> = save
        .setup
        .stocks
        .iter()
        .map(|stock| stock.code.clone())
        .collect();
    let actual_markets: BTreeSet<StockCode> = save.snapshot.markets.keys().cloned().collect();
    if actual_markets != expected_markets {
        return Err(SessionError::InvalidSave(
            "snapshot market set does not exactly match setup".to_string(),
        ));
    }
    let resting_markets: BTreeSet<StockCode> = save.resting_orders.keys().cloned().collect();
    if !resting_markets.is_empty() && resting_markets != expected_markets {
        return Err(SessionError::InvalidSave(
            "resting-order market set does not exactly match setup".to_string(),
        ));
    }
    if resting_markets.is_empty()
        && save
            .snapshot
            .markets
            .values()
            .any(|market| !market.bids.is_empty() || !market.asks.is_empty())
    {
        return Err(SessionError::InvalidSave(
            "legacy save contains depth but no restorable order ownership".to_string(),
        ));
    }
    let history_markets: BTreeSet<StockCode> = save.price_history.keys().cloned().collect();
    if !history_markets.is_empty() && history_markets != expected_markets {
        return Err(SessionError::InvalidSave(
            "price-history market set does not exactly match setup".to_string(),
        ));
    }
    for (code, prices) in &save.price_history {
        if prices.len() > save.setup.history_len || prices.iter().any(|price| price.cents() <= 0) {
            return Err(SessionError::InvalidSave(format!(
                "price history for {} is invalid",
                code.0
            )));
        }
    }

    let npc_count = u64::from(save.setup.npcs.retail_count)
        + u64::from(save.setup.npcs.inst_count)
        + u64::from(save.setup.npcs.hot_count);
    let expected_accounts: BTreeSet<AccountId> = (0..=npc_count).map(AccountId).collect();
    let actual_accounts: BTreeSet<AccountId> = save.snapshot.accounts.keys().copied().collect();
    if actual_accounts != expected_accounts {
        return Err(SessionError::InvalidSave(
            "snapshot account set does not exactly match setup".to_string(),
        ));
    }

    for (id, account) in &save.snapshot.accounts {
        if account.cash.cents() < 0 {
            return Err(SessionError::InvalidSave(format!(
                "account {} has negative cash",
                id.0
            )));
        }
        for (code, position) in &account.positions {
            if !expected_markets.contains(code) {
                return Err(SessionError::InvalidSave(format!(
                    "account {} contains unknown stock {}",
                    id.0, code.0
                )));
            }
            if position.qty == 0
                || position.t1_locked > position.qty
                || position.invested_cents < 0
                || position.recovered_cents < 0
            {
                return Err(SessionError::InvalidSave(format!(
                    "account {} stock {} has invalid position invariants",
                    id.0, code.0
                )));
            }
        }
    }

    for (code, market) in &save.snapshot.markets {
        let Some(fundamental_value) = market.fundamental_value else {
            return Err(SessionError::InvalidSave(format!(
                "market {} is missing its internal fundamental value",
                code.0
            )));
        };
        if market.last_price.cents() <= 0
            || market.last_close.cents() <= 0
            || fundamental_value.cents() <= 0
        {
            return Err(SessionError::InvalidSave(format!(
                "market {} contains a non-positive authoritative price",
                code.0
            )));
        }
        for (price, qty) in market.bids.iter().chain(&market.asks) {
            if price.cents() <= 0 || *qty == 0 {
                return Err(SessionError::InvalidSave(format!(
                    "market {} contains invalid depth",
                    code.0
                )));
            }
        }
    }

    let expected_day = save.snapshot.tick / save.setup.ticks_per_day;
    if expected_day > u64::from(u32::MAX) || u64::from(save.snapshot.day) != expected_day {
        return Err(SessionError::InvalidSave(format!(
            "day {} does not match tick {}",
            save.snapshot.day, save.snapshot.tick
        )));
    }
    let day_tick = save.snapshot.tick % save.setup.ticks_per_day;
    let auction_entry_ticks = save.setup.auction_ticks - save.setup.auction_ticks / 3;
    let expected_phase = if day_tick < auction_entry_ticks {
        TradingPhase::CallAuction
    } else if day_tick < save.setup.auction_ticks {
        TradingPhase::PreOpen
    } else {
        TradingPhase::Continuous
    };
    if save.snapshot.phase != expected_phase {
        return Err(SessionError::InvalidSave(
            "snapshot phase does not match tick".to_string(),
        ));
    }
    for (id, account) in &save.snapshot.accounts {
        for (code, position) in &account.positions {
            if (!save.setup.t1_enabled || expected_phase == TradingPhase::CallAuction)
                && position.t1_locked != 0
            {
                return Err(SessionError::InvalidSave(format!(
                    "account {} stock {} has unreachable T+1 lock state",
                    id.0, code.0
                )));
            }
        }
    }

    for (code, candles) in &save.snapshot.daily_candles {
        if !expected_markets.contains(code) {
            return Err(SessionError::InvalidSave(format!(
                "daily candles contain unknown stock {}",
                code.0
            )));
        }
        let mut previous_time = None;
        for candle in candles {
            validate_candle(code, candle)?;
            if previous_time.is_some_and(|time| candle.time <= time) {
                return Err(SessionError::InvalidSave(format!(
                    "daily candles for {} are not strictly ordered",
                    code.0
                )));
            }
            previous_time = Some(candle.time);
        }
    }
    for (code, candle) in &save.snapshot.active_daily_candles {
        if !expected_markets.contains(code) {
            return Err(SessionError::InvalidSave(format!(
                "active candle contains unknown stock {}",
                code.0
            )));
        }
        validate_candle(code, candle)?;
    }

    if save.next_order_id == 0
        || save
            .auction_orders
            .values()
            .flatten()
            .any(|order| order.arrival_seq >= save.next_order_id)
    {
        return Err(SessionError::InvalidSave(
            "next_order_id is not greater than every saved order id".to_string(),
        ));
    }
    Ok(())
}

fn validate_candle(code: &StockCode, candle: &DailyCandle) -> Result<(), SessionError> {
    if candle.open.cents() <= 0
        || candle.high.cents() <= 0
        || candle.low.cents() <= 0
        || candle.close.cents() <= 0
        || candle.low > candle.open
        || candle.low > candle.close
        || candle.high < candle.open
        || candle.high < candle.close
        || candle.low > candle.high
    {
        return Err(SessionError::InvalidSave(format!(
            "stock {} contains an invalid OHLC candle at {}",
            code.0, candle.time
        )));
    }
    Ok(())
}

pub(super) fn validate_saved_order_state(
    session: &GameSession,
    save: &SaveSlot,
) -> Result<(), SessionError> {
    if save.snapshot.phase != session.phase() {
        return Err(SessionError::InvalidSave(format!(
            "save phase {:?} does not match tick-derived phase {:?}",
            save.snapshot.phase,
            session.phase()
        )));
    }
    if session.phase() != TradingPhase::CallAuction && !save.auction_orders.is_empty() {
        return Err(SessionError::InvalidSave(
            "post-auction save must not contain auction orders".to_string(),
        ));
    }
    if session.phase() == TradingPhase::CallAuction
        && save
            .resting_orders
            .values()
            .any(|orders| !orders.is_empty())
    {
        return Err(SessionError::InvalidSave(
            "call-auction save must not contain continuous resting orders".to_string(),
        ));
    }
    let mut order_ids = BTreeSet::new();
    let mut book_sequences = BTreeSet::new();
    let mut max_order_id = 0_u64;
    let mut sell_filled_totals = BTreeMap::new();
    for (code, orders) in &save.resting_orders {
        for order in orders.iter().filter(|order| order.side == Side::Sell) {
            let total = sell_filled_totals
                .entry((order.owner, code.clone()))
                .or_insert(0_u64);
            *total = total
                .checked_add(u64::from(order.filled_qty))
                .ok_or_else(|| {
                    SessionError::InvalidSave("saved sell filled quantities overflow".to_string())
                })?;
        }
    }
    let mut reservations = SavedReservations {
        sell_filled_totals,
        ..SavedReservations::default()
    };

    for (code, orders) in &save.auction_orders {
        let market = session.markets.get(code).ok_or_else(|| {
            SessionError::InvalidSave(format!(
                "save auction order references unknown stock {code:?}"
            ))
        })?;
        let stock = session
            .setup
            .stocks
            .iter()
            .find(|stock| stock.code == *code)
            .expect("market code must have a stock spec");
        let tick = stock.tick;
        let down = market
            .down_stop()
            .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
        let up = market
            .up_stop()
            .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
        for order in orders {
            if !session.accounts.contains_key(&order.owner) {
                return Err(SessionError::InvalidSave(format!(
                    "save auction order references unknown account {:?}",
                    order.owner
                )));
            }
            if order.qty == 0
                || order.qty > stock.category.max_order_qty(false)
                || (order.side == Side::Buy
                    && !order.qty.is_multiple_of(session.setup.config.lot_size))
                || order.limit < down
                || order.limit > up
                || order.limit.cents() % tick.cents() != 0
            {
                return Err(SessionError::InvalidSave(format!(
                    "invalid saved auction order for {code:?}: {order:?}"
                )));
            }
            if !order_ids.insert(order.arrival_seq) {
                return Err(SessionError::InvalidSave(format!(
                    "duplicate saved order id {}",
                    order.arrival_seq
                )));
            }
            reservations.validate_quantity(
                session,
                order.owner,
                code,
                SavedOrderQuantity {
                    side: order.side,
                    limit: order.limit,
                    remaining_qty: order.qty,
                    original_qty: order.qty,
                    filled_qty: 0,
                    filled_value: Money::ZERO,
                },
            )?;
            max_order_id = max_order_id.max(order.arrival_seq);
            reservations.record(
                &session.setup.config,
                order.owner,
                code,
                ReservationOrder {
                    side: order.side,
                    price: order.limit,
                    qty: order.qty,
                    filled_value: Money::ZERO,
                },
            )?;
        }
    }

    for (code, orders) in &save.resting_orders {
        let market = session.markets.get(code).ok_or_else(|| {
            SessionError::InvalidSave(format!(
                "save resting order references unknown stock {code:?}"
            ))
        })?;
        let stock = session
            .setup
            .stocks
            .iter()
            .find(|stock| stock.code == *code)
            .expect("market code must have a stock spec");
        let tick = stock.tick;
        let down = market
            .down_stop()
            .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
        let up = market
            .up_stop()
            .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
        for order in orders {
            let original_qty = if order.original_qty == 0
                && order.filled_qty == 0
                && order.filled_value == Money::ZERO
            {
                // v1 旧存档的未成交委托没有原始量字段，可无损归一化。
                order.qty
            } else {
                order.original_qty
            };
            if !session.accounts.contains_key(&order.owner)
                || order.qty == 0
                || original_qty > stock.category.max_order_qty(false)
                || order.filled_value.cents() < 0
                || order
                    .filled_qty
                    .checked_add(order.qty)
                    .is_none_or(|total| total != original_qty)
                || (order.filled_qty == 0) != (order.filled_value == Money::ZERO)
                || order.price < down
                || order.price > up
                || order.price.cents() % tick.cents() != 0
            {
                return Err(SessionError::InvalidSave(format!(
                    "invalid saved resting order for {code:?}: {order:?}"
                )));
            }
            if !order_ids.insert(order.id.0) {
                return Err(SessionError::InvalidSave(format!(
                    "duplicate saved order id {}",
                    order.id.0
                )));
            }
            if !book_sequences.insert((code.clone(), order.seq)) {
                return Err(SessionError::InvalidSave(format!(
                    "duplicate saved book sequence {} for {}",
                    order.seq, code.0
                )));
            }
            reservations.validate_quantity(
                session,
                order.owner,
                code,
                SavedOrderQuantity {
                    side: order.side,
                    limit: order.price,
                    remaining_qty: order.qty,
                    original_qty,
                    filled_qty: order.filled_qty,
                    filled_value: order.filled_value,
                },
            )?;
            max_order_id = max_order_id.max(order.id.0);
            reservations.record(
                &session.setup.config,
                order.owner,
                code,
                ReservationOrder {
                    side: order.side,
                    price: order.price,
                    qty: order.qty,
                    filled_value: order.filled_value,
                },
            )?;
        }
    }

    if !order_ids.is_empty() && save.next_order_id <= max_order_id {
        return Err(SessionError::InvalidSave(format!(
            "next_order_id {} must exceed saved order id {}",
            save.next_order_id, max_order_id
        )));
    }
    let SavedReservations { buys, sells, .. } = reservations;
    for (owner, reserved) in buys {
        let cash = session
            .accounts
            .get(&owner)
            .expect("saved auction owner was validated")
            .cash;
        if reserved > i128::from(cash.cents()) {
            return Err(SessionError::InvalidSave(format!(
                "saved buys over-reserve cash for {owner:?}"
            )));
        }
    }
    for ((owner, code), reserved) in sells {
        let sellable = session
            .accounts
            .get(&owner)
            .expect("saved auction owner was validated")
            .sellable_qty(&code);
        if reserved > u64::from(sellable) {
            return Err(SessionError::InvalidSave(format!(
                "saved sells over-reserve shares for {owner:?} {code:?}"
            )));
        }
    }
    Ok(())
}

#[derive(Default)]
struct SavedReservations {
    buys: BTreeMap<AccountId, i128>,
    sells: BTreeMap<(AccountId, StockCode), u64>,
    sell_filled_totals: BTreeMap<(AccountId, StockCode), u64>,
    validated_odd_lot_sells: BTreeSet<(AccountId, StockCode)>,
}

struct ReservationOrder {
    side: Side,
    price: Money,
    qty: u32,
    filled_value: Money,
}

struct SavedOrderQuantity {
    side: Side,
    limit: Money,
    remaining_qty: u32,
    original_qty: u32,
    filled_qty: u32,
    filled_value: Money,
}

impl SavedReservations {
    fn validate_quantity(
        &mut self,
        session: &GameSession,
        owner: AccountId,
        code: &StockCode,
        quantity: SavedOrderQuantity,
    ) -> Result<(), SessionError> {
        let SavedOrderQuantity {
            side,
            limit,
            remaining_qty: qty,
            original_qty,
            filled_qty,
            filled_value,
        } = quantity;
        if original_qty == 0
            || filled_qty
                .checked_add(qty)
                .is_none_or(|total| total != original_qty)
            || (filled_qty == 0) != (filled_value == Money::ZERO)
        {
            return Err(SessionError::InvalidSave(format!(
                "saved order quantity progress is invalid for {owner:?} {code:?}"
            )));
        }
        if filled_qty > 0 {
            let market = session
                .markets
                .get(code)
                .expect("saved order market was validated");
            let daily_minimum = market
                .down_stop()
                .map_err(|error| SessionError::InvalidSave(error.to_string()))?
                .mul_shares(filled_qty)
                .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
            let daily_maximum = market
                .up_stop()
                .map_err(|error| SessionError::InvalidSave(error.to_string()))?
                .mul_shares(filled_qty)
                .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
            let limit_value = limit
                .mul_shares(filled_qty)
                .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
            let (minimum, maximum) = match side {
                Side::Buy => (daily_minimum, limit_value),
                Side::Sell => (limit_value, daily_maximum),
            };
            if filled_value < minimum || filled_value > maximum {
                return Err(SessionError::InvalidSave(format!(
                    "saved filled value is impossible for {owner:?} {code:?}"
                )));
            }
        }
        let lot_size = u64::from(session.setup.config.lot_size);
        if side == Side::Buy {
            if !u64::from(original_qty).is_multiple_of(lot_size) {
                return Err(SessionError::InvalidSave(format!(
                    "saved buy quantity is not a board lot for {owner:?} {code:?}"
                )));
            }
            return Ok(());
        }

        let sellable = u64::from(
            session
                .accounts
                .get(&owner)
                .expect("saved order owner was validated")
                .sellable_qty(code),
        );
        let reconstructed_sellable = sellable
            .checked_add(
                self.sell_filled_totals
                    .get(&(owner, code.clone()))
                    .copied()
                    .unwrap_or(0),
            )
            .ok_or_else(|| {
                SessionError::InvalidSave(format!(
                    "saved reconstructed sellable quantity overflows for {owner:?} {code:?}"
                ))
            })?;
        let original_qty = u64::from(original_qty);
        let is_board_lot = original_qty.is_multiple_of(lot_size);
        let odd_lot_is_valid = if is_board_lot {
            true
        } else {
            let key = (owner, code.clone());
            reconstructed_sellable % lot_size != 0
                && original_qty % lot_size == reconstructed_sellable % lot_size
                && self.validated_odd_lot_sells.insert(key)
        };
        let valid = original_qty <= reconstructed_sellable && odd_lot_is_valid;
        if !valid {
            return Err(SessionError::InvalidSave(format!(
                "saved sell quantity splits an odd-lot remainder for {owner:?} {code:?}"
            )));
        }
        Ok(())
    }

    fn record(
        &mut self,
        config: &GameConfig,
        owner: AccountId,
        code: &StockCode,
        order: ReservationOrder,
    ) -> Result<(), SessionError> {
        match order.side {
            Side::Buy => {
                let required =
                    buy_order_reservation(config, order.price, order.qty, order.filled_value)
                        .map_err(|error| {
                            SessionError::InvalidSave(format!(
                                "saved buy reservation is invalid: {error}"
                            ))
                        })?;
                let reserved = self.buys.entry(owner).or_default();
                *reserved = reserved
                    .checked_add(i128::from(required.cents()))
                    .ok_or_else(|| {
                        SessionError::InvalidSave("saved buy reservations overflow".to_string())
                    })?;
            }
            Side::Sell => {
                let reserved = self.sells.entry((owner, code.clone())).or_default();
                *reserved = reserved.checked_add(u64::from(order.qty)).ok_or_else(|| {
                    SessionError::InvalidSave("saved sell reservations overflow".to_string())
                })?;
            }
        }
        Ok(())
    }
}
