//! 存档结构、市场、账户、订单与日 K 不变量校验。

use super::*;

pub(super) fn validate_save_slot(save: &SaveSlot) -> Result<(), SessionError> {
    save.setup
        .validate()
        .map_err(|error| SessionError::InvalidSave(format!("invalid setup: {error}")))?;
    if save.pending_player.len() > MAX_PENDING_PLAYER_INTENTS {
        return Err(SessionError::InvalidSave(format!(
            "pending player intents exceed {MAX_PENDING_PLAYER_INTENTS}"
        )));
    }

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
            "save contains depth but no restorable order ownership".to_string(),
        ));
    }
    let history_markets: BTreeSet<StockCode> = save.price_history.keys().cloned().collect();
    if history_markets != expected_markets {
        return Err(SessionError::InvalidSave(
            "price-history market set does not exactly match setup".to_string(),
        ));
    }
    for (code, prices) in &save.price_history {
        let completed_continuous_ticks = u64::from(save.snapshot.day)
            .checked_mul(save.setup.ticks_per_day - save.setup.auction_ticks)
            .and_then(|ticks| {
                ticks.checked_add(
                    (save.snapshot.tick % save.setup.ticks_per_day)
                        .saturating_sub(save.setup.auction_ticks),
                )
            })
            .ok_or_else(|| {
                SessionError::InvalidSave("price-history length overflow".to_string())
            })?;
        let expected_len = usize::try_from(completed_continuous_ticks)
            .unwrap_or(usize::MAX)
            .min(save.setup.history_len);
        if prices.len() != expected_len || prices.iter().any(|price| price.cents() <= 0) {
            return Err(SessionError::InvalidSave(format!(
                "price history for {} has length {}; expected {}",
                code.0,
                prices.len(),
                expected_len,
            )));
        }
    }

    let npc_count = u64::from(save.setup.npcs.retail_count)
        + u64::from(save.setup.npcs.inst_count)
        + u64::from(save.setup.npcs.hot_count);
    let expected_account_count = usize::try_from(npc_count)
        .ok()
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| {
            SessionError::InvalidSave("account count exceeds platform limits".to_string())
        })?;
    if save.snapshot.accounts.len() != expected_account_count
        || save
            .snapshot
            .accounts
            .keys()
            .enumerate()
            .any(|(index, id)| id.0 != index as u64)
    {
        return Err(SessionError::InvalidSave(
            "snapshot account set does not exactly match setup".to_string(),
        ));
    }

    let expected_attention_accounts: BTreeSet<AccountId> = (1..=npc_count).map(AccountId).collect();
    let actual_attention_accounts: BTreeSet<AccountId> =
        save.npc_attention.keys().copied().collect();
    if actual_attention_accounts != expected_attention_accounts {
        return Err(SessionError::InvalidSave(
            "NPC attention account set does not exactly match setup".to_string(),
        ));
    }
    for (id, state) in &save.npc_attention {
        if !(state.base_probability.is_finite()
            && 0.0 < state.base_probability
            && state.base_probability <= 1.0)
        {
            return Err(SessionError::InvalidSave(format!(
                "NPC {} attention probability must be finite and in (0,1]",
                id.0
            )));
        }
        if state.next_attention_candidate_tick < save.snapshot.tick {
            return Err(SessionError::InvalidSave(format!(
                "NPC {} next attention candidate tick {} precedes snapshot tick {}",
                id.0, state.next_attention_candidate_tick, save.snapshot.tick
            )));
        }
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

    let daily_candle_markets: BTreeSet<StockCode> =
        save.snapshot.daily_candles.keys().cloned().collect();
    if daily_candle_markets != expected_markets {
        return Err(SessionError::InvalidSave(
            "daily-candle market set does not exactly match setup".to_string(),
        ));
    }
    for (code, candles) in &save.snapshot.daily_candles {
        if !expected_markets.contains(code) {
            return Err(SessionError::InvalidSave(format!(
                "daily candles contain unknown stock {}",
                code.0
            )));
        }
        if candles.len() != 360 {
            return Err(SessionError::InvalidSave(format!(
                "daily candles for {} have length {}; expected 360",
                code.0,
                candles.len(),
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

    if save
        .pending_player
        .iter()
        .any(|(account, _)| *account != AccountId(0))
    {
        return Err(SessionError::InvalidSave(
            "pending player intent must belong to the player account".to_string(),
        ));
    }
    let active_candle_markets: BTreeSet<StockCode> =
        save.snapshot.active_daily_candles.keys().cloned().collect();
    let active_candle_set_is_valid = if day_tick == 0 || day_tick < auction_entry_ticks {
        active_candle_markets.is_empty()
    } else {
        active_candle_markets == expected_markets
    };
    if !active_candle_set_is_valid {
        return Err(SessionError::InvalidSave(
            "active-candle market set does not match the current trading tick".to_string(),
        ));
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
            let original_qty = order.original_qty;
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
    let SavedReservations { cash, sells, .. } = reservations;
    for (owner, reserved) in cash {
        let available = session
            .accounts
            .get(&owner)
            .expect("saved order owner was validated")
            .cash;
        if reserved > i128::from(available.cents()) {
            return Err(SessionError::InvalidSave(format!(
                "saved orders over-reserve cash for {owner:?}"
            )));
        }
    }
    for ((owner, code), reserved) in sells {
        let sellable = session
            .accounts
            .get(&owner)
            .expect("saved order owner was validated")
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
    cash: BTreeMap<AccountId, i128>,
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
                let reserved = self.cash.entry(owner).or_default();
                *reserved = reserved
                    .checked_add(i128::from(required.cents()))
                    .ok_or_else(|| {
                        SessionError::InvalidSave("saved buy reservations overflow".to_string())
                    })?;
            }
            Side::Sell => {
                let required =
                    sell_order_fee_reservation(config, order.price, order.qty, order.filled_value)
                        .map_err(|error| {
                            SessionError::InvalidSave(format!(
                                "saved sell cash reservation is invalid: {error}"
                            ))
                        })?;
                let cash = self.cash.entry(owner).or_default();
                *cash = cash
                    .checked_add(i128::from(required.cents()))
                    .ok_or_else(|| {
                        SessionError::InvalidSave(
                            "saved order cash reservations overflow".to_string(),
                        )
                    })?;
                let reserved = self.sells.entry((owner, code.clone())).or_default();
                *reserved = reserved.checked_add(u64::from(order.qty)).ok_or_else(|| {
                    SessionError::InvalidSave("saved sell reservations overflow".to_string())
                })?;
            }
        }
        Ok(())
    }
}
