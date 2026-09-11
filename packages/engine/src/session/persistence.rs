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
        let continuous_ticks_per_day = save
            .setup
            .ticks_per_day
            .saturating_sub(save.setup.auction_ticks)
            .saturating_sub(save.setup.closing_auction_ticks);
        let completed_continuous_ticks = u64::from(save.snapshot.day)
            .checked_mul(continuous_ticks_per_day)
            .and_then(|ticks| {
                ticks.checked_add(
                    (save.snapshot.tick % save.setup.ticks_per_day)
                        .saturating_sub(save.setup.auction_ticks)
                        .min(continuous_ticks_per_day),
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

    let minute_markets: BTreeSet<StockCode> = save.market_minute_closes.keys().cloned().collect();
    if minute_markets != expected_markets {
        return Err(SessionError::InvalidSave(
            "market-minute history market set does not exactly match setup".to_string(),
        ));
    }
    let day_tick = save.snapshot.tick % save.setup.ticks_per_day;
    let continuous_ticks_per_day = save
        .setup
        .ticks_per_day
        .saturating_sub(save.setup.auction_ticks)
        .saturating_sub(save.setup.closing_auction_ticks);
    let completed_continuous_ticks = day_tick
        .saturating_sub(save.setup.auction_ticks)
        .min(continuous_ticks_per_day);
    let day_start = u64::from(save.snapshot.day)
        .checked_mul(u64::from(crate::GAME_INTRADAY_MINUTES_PER_DAY))
        .ok_or_else(|| {
            SessionError::InvalidSave("market-minute day offset overflow".to_string())
        })?;
    let completed_minutes = u64::from(
        crate::completed_market_minute_count(completed_continuous_ticks, continuous_ticks_per_day)
            .map_err(|error| SessionError::InvalidSave(error.to_string()))?,
    );
    let expected_minute_keys = (0..completed_minutes)
        .map(|minute_in_day| {
            day_start
                .checked_add(minute_in_day)
                .ok_or_else(|| SessionError::InvalidSave("market-minute key overflow".to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (code, minutes) in &save.market_minute_closes {
        let actual_keys: Vec<u64> = minutes
            .iter()
            .map(|sample| sample.absolute_trading_minute)
            .collect();
        if actual_keys.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(SessionError::InvalidSave(format!(
                "market-minute history for {} must be strictly increasing",
                code.0
            )));
        }
        if minutes.iter().any(|sample| sample.close.cents() <= 0) {
            return Err(SessionError::InvalidSave(format!(
                "market-minute history for {} must contain positive prices",
                code.0
            )));
        }
        if actual_keys != expected_minute_keys {
            let reason = if actual_keys
                .iter()
                .any(|minute| !expected_minute_keys.contains(minute))
            {
                "contains a missing or future market minute"
            } else {
                "does not contain every completed market minute"
            };
            return Err(SessionError::InvalidSave(format!(
                "market-minute history for {} {reason}",
                code.0
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
    let actual_strategy_profile_accounts: BTreeSet<AccountId> =
        save.strategy_profiles.keys().copied().collect();
    if actual_strategy_profile_accounts != expected_attention_accounts {
        return Err(SessionError::InvalidSave(
            "NPC strategy profile account set does not exactly match setup".to_string(),
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

    let expected_retail_accounts: BTreeSet<AccountId> =
        (1..=u64::from(save.setup.npcs.retail_count))
            .map(AccountId)
            .collect();
    let actual_retail_accounts: BTreeSet<AccountId> =
        save.retail_experience.keys().copied().collect();
    if actual_retail_accounts != expected_retail_accounts {
        return Err(SessionError::InvalidSave(
            "retail experience account set does not exactly match setup".to_string(),
        ));
    }
    let current_market_minute = day_start.checked_add(completed_minutes).ok_or_else(|| {
        SessionError::InvalidSave("experience market-minute overflow".to_string())
    })?;
    let day_end_market_minute = day_start
        .checked_add(u64::from(crate::GAME_INTRADAY_MINUTES_PER_DAY))
        .ok_or_else(|| {
            SessionError::InvalidSave("NPC quote lifecycle day range overflows".to_string())
        })?;
    let mut lifecycle_keys = BTreeSet::new();
    for lifecycle in &save.npc_order_lifecycles {
        if lifecycle.account.0 == 0 || lifecycle.account.0 > npc_count {
            return Err(SessionError::InvalidSave(format!(
                "NPC quote lifecycle account {} is not an NPC",
                lifecycle.account.0
            )));
        }
        if !expected_markets.contains(&lifecycle.code)
            || lifecycle.order_id.0 == 0
            || lifecycle.order_id.0 >= save.next_order_id
            || lifecycle.placed_market_minute < day_start
            || lifecycle.placed_market_minute > current_market_minute
            || lifecycle.expires_market_minute <= lifecycle.placed_market_minute
            || lifecycle.expires_market_minute > day_end_market_minute
            || (save.snapshot.phase == TradingPhase::Continuous
                && lifecycle.expires_market_minute <= current_market_minute)
            || (save.snapshot.phase != TradingPhase::Continuous
                && save.snapshot.phase != TradingPhase::ClosingAuction)
        {
            return Err(SessionError::InvalidSave(format!(
                "NPC quote lifecycle for account {} is invalid",
                lifecycle.account.0
            )));
        }
        if !lifecycle_keys.insert((
            lifecycle.account,
            lifecycle.code.clone(),
            lifecycle.order_id,
        )) {
            return Err(SessionError::InvalidSave(format!(
                "NPC quote lifecycle duplicates order {}",
                lifecycle.order_id.0
            )));
        }
    }
    let first_institution = u64::from(save.setup.npcs.retail_count) + 1;
    let last_institution = u64::from(save.setup.npcs.retail_count)
        .checked_add(u64::from(save.setup.npcs.inst_count))
        .ok_or_else(|| {
            SessionError::InvalidSave("institution account range overflows".to_string())
        })?;
    for (account, plans) in &save.parent_orders {
        if account.0 < first_institution || account.0 > last_institution {
            return Err(SessionError::InvalidSave(format!(
                "parent-order account {} is not an institution",
                account.0
            )));
        }
        if plans.is_empty() {
            return Err(SessionError::InvalidSave(format!(
                "parent-order account {} has an empty plan map",
                account.0
            )));
        }
        for (code, plan) in plans {
            if plan.code != *code || !expected_markets.contains(code) {
                return Err(SessionError::InvalidSave(format!(
                    "parent-order account {} contains an unknown or mismatched stock",
                    account.0
                )));
            }
            if plan.target_qty == 0
                || plan.child_qty == 0
                || plan.child_qty > plan.target_qty
                || plan.filled_qty >= plan.target_qty
                || plan.target_qty % save.setup.config.lot_size != 0
                || plan.child_qty % save.setup.config.lot_size != 0
                || plan.limit_price.cents() <= 0
                || plan.expires_market_minute <= current_market_minute
            {
                return Err(SessionError::InvalidSave(format!(
                    "parent-order account {} stock {} violates execution-plan invariants",
                    account.0, code.0
                )));
            }
            if plan
                .active_child_order_id
                .is_some_and(|id| id.0 == 0 || id.0 >= save.next_order_id)
                || (plan.active_child_order_id.is_some()
                    != plan.active_child_remaining_qty.is_some())
                || plan.active_child_remaining_qty.is_some_and(|qty| qty == 0)
            {
                return Err(SessionError::InvalidSave(format!(
                    "parent-order account {} stock {} has an invalid active child id",
                    account.0, code.0
                )));
            }
        }
    }
    for (id, experience) in &save.retail_experience {
        match (experience.reference_equity, experience.peak_equity) {
            (None, None) => {}
            (Some(reference), Some(peak))
                if reference.cents() > 0 && peak.cents() > 0 && peak >= reference => {}
            _ => {
                return Err(SessionError::InvalidSave(format!(
                    "retail account {} has invalid equity experience references",
                    id.0
                )));
            }
        }
        let held = &save.snapshot.accounts[id].positions;
        if held
            .keys()
            .any(|code| !experience.stocks.contains_key(code))
        {
            return Err(SessionError::InvalidSave(format!(
                "retail account {} is missing experience for a held stock",
                id.0
            )));
        }
        let unheld_count = experience
            .stocks
            .keys()
            .filter(|code| !held.contains_key(*code))
            .count();
        if unheld_count > crate::MAX_UNHELD_WATCHLIST_STOCKS {
            return Err(SessionError::InvalidSave(format!(
                "retail account {} exceeds the unheld watchlist limit",
                id.0
            )));
        }
        for (code, stock) in &experience.stocks {
            if !expected_markets.contains(code) {
                return Err(SessionError::InvalidSave(format!(
                    "retail account {} experience contains unknown stock {}",
                    id.0, code.0
                )));
            }
            if [
                stock.entry_reference_price,
                stock.peak_price_since_entry,
                stock.last_buy_price,
            ]
            .into_iter()
            .flatten()
            .any(|price| price.cents() <= 0)
            {
                return Err(SessionError::InvalidSave(format!(
                    "retail account {} stock {} has a non-positive experience price",
                    id.0, code.0
                )));
            }
            if stock.last_trade_market_minute > current_market_minute
                || stock.last_observed_market_minute > current_market_minute
            {
                return Err(SessionError::InvalidSave(format!(
                    "retail account {} stock {} experience reads a future market minute",
                    id.0, code.0
                )));
            }
            if stock.last_observed_market_minute < stock.last_trade_market_minute {
                return Err(SessionError::InvalidSave(format!(
                    "retail account {} stock {} was observed before its latest trade",
                    id.0, code.0
                )));
            }
            for (label, order_id) in [
                ("last buy", stock.last_buy_order_id),
                ("last sell", stock.last_sell_order_id),
            ] {
                if order_id.is_some_and(|order_id| order_id == 0 || order_id >= save.next_order_id)
                {
                    return Err(SessionError::InvalidSave(format!(
                        "retail account {} stock {} has an invalid {label} order id",
                        id.0, code.0
                    )));
                }
            }
            if stock.last_buy_order_id.is_some() && !held.contains_key(code) {
                return Err(SessionError::InvalidSave(format!(
                    "retail account {} unheld stock {} retains a buy order identity",
                    id.0, code.0
                )));
            }
            if stock.adverse_move_recorded
                && (stock.last_buy_price.is_none() || stock.last_buy_order_id.is_none())
            {
                return Err(SessionError::InvalidSave(format!(
                    "retail account {} stock {} records adversity without a buy identity",
                    id.0, code.0
                )));
            }
            if held.contains_key(code) {
                if stock.entry_reference_price.is_none()
                    || stock.peak_price_since_entry.is_none()
                    || stock.cooldown_until_market_minute.is_some()
                {
                    return Err(SessionError::InvalidSave(format!(
                        "retail account {} held stock {} has invalid active experience",
                        id.0, code.0
                    )));
                }
                if stock.last_buy_price.is_some() != stock.last_buy_order_id.is_some() {
                    return Err(SessionError::InvalidSave(format!(
                        "retail account {} held stock {} has an incomplete latest-buy identity",
                        id.0, code.0
                    )));
                }
                if let (Some(entry), Some(peak)) =
                    (stock.entry_reference_price, stock.peak_price_since_entry)
                {
                    if peak < entry {
                        return Err(SessionError::InvalidSave(format!(
                            "retail account {} held stock {} has a peak below entry",
                            id.0, code.0
                        )));
                    }
                }
                if let (Some(last_buy), Some(peak)) =
                    (stock.last_buy_price, stock.peak_price_since_entry)
                {
                    if peak < last_buy {
                        return Err(SessionError::InvalidSave(format!(
                            "retail account {} held stock {} has a peak below its latest buy",
                            id.0, code.0
                        )));
                    }
                }
            } else if stock.cooldown_until_market_minute.is_none()
                && (stock.entry_reference_price.is_some()
                    || stock.peak_price_since_entry.is_some()
                    || stock.last_buy_price.is_some()
                    || stock.last_buy_order_id.is_some()
                    || stock.last_sell_order_id.is_some()
                    || stock.adverse_move_recorded)
            {
                return Err(SessionError::InvalidSave(format!(
                    "retail account {} unheld stock {} has an invalid observation-only state",
                    id.0, code.0
                )));
            }
            if let Some(until) = stock.cooldown_until_market_minute {
                let expected = stock
                    .last_trade_market_minute
                    .checked_add(crate::POST_EXIT_COOLDOWN_MINUTES)
                    .ok_or_else(|| {
                        SessionError::InvalidSave(format!(
                            "retail account {} stock {} cooldown overflows",
                            id.0, code.0
                        ))
                    })?;
                if until != expected {
                    return Err(SessionError::InvalidSave(format!(
                        "retail account {} stock {} has invalid post-exit cooldown",
                        id.0, code.0
                    )));
                }
                if stock.last_sell_order_id.is_none()
                    || stock.last_buy_price.is_some()
                    || stock.last_buy_order_id.is_some()
                    || stock.adverse_move_recorded
                {
                    return Err(SessionError::InvalidSave(format!(
                        "retail account {} stock {} has inconsistent post-exit state",
                        id.0, code.0
                    )));
                }
            }
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
        if market.last_price.cents() <= 0 || market.last_close.cents() <= 0 {
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
    } else if day_tick
        >= save
            .setup
            .ticks_per_day
            .saturating_sub(save.setup.closing_auction_ticks)
    {
        TradingPhase::ClosingAuction
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

    validate_company_domain(save)?;
    validate_disclosure_cursors(save)?;
    validate_personal_states(save)?;
    validate_plan_contract(save)?;
    Ok(())
}

/// K7（任务 27）资源门禁常量：512 MiB 总解码字节 + 公司数 ≤ 256（行 180）。
pub const MAX_SAVE_DECODE_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_SAVE_COMPANIES: usize = 256;
/// 新增可增长集合的显式长度门限（字节门禁之外的第二道防线；超限 = 类型化
/// 拒绝，绝不静默截断）。
pub const MAX_SAVED_PLANS: usize = 1_000_000;
pub const MAX_SAVED_PUBLICATIONS: usize = 1_000_000;
pub const MAX_SAVED_PLAN_EVENTS: usize = 100_000;

/// 存档解码资源门禁（K7 行 180；可按部署配置）。
#[derive(Clone, Copy, Debug)]
pub struct SaveDecodeLimits {
    /// 存档 JSON 总解码字节上限。
    pub max_total_bytes: usize,
    /// 公司数上限。
    pub max_companies: usize,
}

impl Default for SaveDecodeLimits {
    fn default() -> Self {
        Self {
            max_total_bytes: MAX_SAVE_DECODE_BYTES,
            max_companies: MAX_SAVE_COMPANIES,
        }
    }
}

/// 存档解码入口：先资源门禁，后 serde 解码（结构化字段缺失/多余/类型错误
/// 走通用拒绝），最后复核公司数上限。任何失败 = 类型化错误，调用方会话与
/// 源字节保持原样。
pub fn decode_save_slot(json: &[u8], limits: &SaveDecodeLimits) -> Result<SaveSlot, SessionError> {
    if json.len() > limits.max_total_bytes {
        return Err(SessionError::ResourceLimit(format!(
            "save payload {} bytes exceeds the decode limit {} bytes",
            json.len(),
            limits.max_total_bytes
        )));
    }
    let slot: SaveSlot = serde_json::from_slice(json).map_err(|error| {
        SessionError::InvalidSave(format!("save JSON is not decodable: {error}"))
    })?;
    if slot.company_operations.companies.len() > limits.max_companies {
        return Err(SessionError::ResourceLimit(format!(
            "save contains {} companies which exceeds the limit {}",
            slot.company_operations.companies.len(),
            limits.max_companies
        )));
    }
    Ok(slot)
}

/// 公司域权威状态校验：经营编排集合/推进时点、镜像与时钟到期一致性。
fn validate_company_domain(save: &SaveSlot) -> Result<(), SessionError> {
    if save.company_operations.companies.len() > MAX_SAVE_COMPANIES {
        return Err(SessionError::ResourceLimit(format!(
            "save contains {} companies which exceeds the limit {MAX_SAVE_COMPANIES}",
            save.company_operations.companies.len()
        )));
    }
    // 公司集合精确：每家经营公司唯一映射一只 setup 股票且股本一致。
    let mut mapped: BTreeSet<&StockCode> = BTreeSet::new();
    for (id, company) in &save.company_operations.companies {
        let Some(listed) = company.spec().listed_stock.as_ref() else {
            return Err(SessionError::InvalidSave(format!(
                "saved company {id:?} has no listed stock mapping"
            )));
        };
        let Some(stock) = save.setup.stocks.iter().find(|s| &s.code == listed) else {
            return Err(SessionError::InvalidSave(format!(
                "saved company {id:?} maps to unknown stock {listed:?}"
            )));
        };
        if company.spec().issued_shares != stock.total_shares || !mapped.insert(listed) {
            return Err(SessionError::InvalidSave(format!(
                "saved company {id:?} share count or mapping conflicts with setup stock {listed:?}"
            )));
        }
    }
    if mapped.len() != save.setup.stocks.len() {
        return Err(SessionError::InvalidSave(
            "saved company set does not exactly cover the setup stocks".to_string(),
        ));
    }
    // 经营推进时点与时钟一致：存档时点 next_expected 恒等于当前自然日。
    if save.company_operations.next_expected_date() != save.civil_clock.current_date {
        return Err(SessionError::InvalidSave(format!(
            "company operations expect {} but the civil clock is at {}",
            save.company_operations.next_expected_date(),
            save.civil_clock.current_date
        )));
    }
    // 镜像集合与调度待办精确相等（sync 收编 + prune 收缩的生产不变量）。
    if !save.ops_wiring.mirror_is_exact(&save.company_operations) {
        return Err(SessionError::InvalidSave(
            "scheduler mirror does not exactly match the pending due set".to_string(),
        ));
    }
    // 时钟到期队列按 (日期,种类) 多重集包含调度待办：经营注册的每条 due 都
    // 能在时钟队列中配到一条不重复的到期项（丢失/错日 = 派发失步的篡改档）。
    // 两边 id 是不同空间（调度器 u64 自增 vs 时钟注册序 u32），无法逐 id 连接；
    // 时钟是通用注册表（register_due 面向第三方，测试自注册 dues 合法共存），
    // 因此只做包含校验，不要求全量相等。重复注册防线是上面的镜像精确校验。
    let mut clock_counts: BTreeMap<(crate::calendar::CivilDate, DueKind), usize> = BTreeMap::new();
    for due in &save.civil_clock.pending_due {
        *clock_counts.entry((due.due_date, due.kind)).or_default() += 1_usize;
    }
    for due in save.company_operations.scheduler().pending() {
        if due.due_date < save.civil_clock.current_date {
            return Err(SessionError::InvalidSave(format!(
                "scheduler pending {due:?} precedes the current civil date"
            )));
        }
        let kind = super::company_operations::due_kind_of(&due.action);
        let remaining = clock_counts
            .get_mut(&(due.due_date, kind))
            .filter(|count| **count > 0);
        match remaining {
            Some(count) => *count -= 1,
            None => {
                return Err(SessionError::InvalidSave(format!(
                    "civil-clock due queue is missing the operations due {:?} on {}",
                    due.id, due.due_date
                )));
            }
        }
    }
    // 公开信息库：全量重验（JSON 路径已验，这里覆盖直接内存构造的 SaveSlot）。
    crate::information::PublicLibrary::from_parts(save.public_library.save()).map_err(|error| {
        SessionError::InvalidSave(format!("saved public library is inconsistent: {error}"))
    })?;
    let reports = save.public_library.report_count();
    let announcements = save.public_library.announcement_count();
    let total_publications = reports
        .checked_add(announcements)
        .ok_or_else(|| SessionError::ResourceLimit("publication count overflows".to_string()))?;
    if total_publications > MAX_SAVED_PUBLICATIONS {
        return Err(SessionError::ResourceLimit(format!(
            "save contains {total_publications} publications which exceeds the limit {MAX_SAVED_PUBLICATIONS}"
        )));
    }
    Ok(())
}

/// 披露派发游标自洽：恰好一次语义在存档时点的投影。
fn validate_disclosure_cursors(save: &SaveSlot) -> Result<(), SessionError> {
    let current = save.civil_clock.current_date;
    match (
        save.civil_clock.settled_through,
        save.disclosures.announced_through(),
    ) {
        (None, None) => {}
        (Some(settled), Some(announced)) if settled == announced => {
            // 每个已日结自然日的披露相位都是 18:00；游标必须精确落在其上。
            let phase = crate::calendar::CivilInstant::from_hms(settled, 18, 0, 0)
                .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
            if save.disclosures.published_through() != Some(phase) {
                return Err(SessionError::InvalidSave(format!(
                    "disclosure cursor {:?} does not match the 18:00 phase of settled {settled}",
                    save.disclosures.published_through()
                )));
            }
        }
        _ => {
            return Err(SessionError::InvalidSave(
                "disclosure cursors do not match the settled-through date".to_string(),
            ));
        }
    }
    // 无未来公布：库内最晚发表时点不得晚于已派发游标。
    if let (Some(latest), Some(through)) = (
        save.public_library.latest_published_instant(),
        save.disclosures.published_through(),
    ) {
        if latest > through {
            return Err(SessionError::InvalidSave(
                "public library contains a publication after the dispatch cursor".to_string(),
            ));
        }
    }
    if save
        .public_library
        .latest_published_instant()
        .is_some_and(|latest| latest.date() > current)
    {
        return Err(SessionError::InvalidSave(
            "public library contains a publication dated after the current civil date".to_string(),
        ));
    }
    Ok(())
}

/// 个体决策链状态校验：三图同键、报告引用存在、无前视/未来观察。
fn validate_personal_states(save: &SaveSlot) -> Result<(), SessionError> {
    let belief_keys: BTreeSet<AccountId> = save.belief_books.keys().copied().collect();
    let information_keys: BTreeSet<AccountId> = save.information_states.keys().copied().collect();
    let watchlist_keys: BTreeSet<AccountId> = save.watchlists.keys().copied().collect();
    if belief_keys != information_keys || belief_keys != watchlist_keys {
        return Err(SessionError::InvalidSave(
            "belief books, information states, and watchlists must cover the same account set"
                .to_string(),
        ));
    }
    let npc_count = u64::from(save.setup.npcs.retail_count)
        + u64::from(save.setup.npcs.inst_count)
        + u64::from(save.setup.npcs.hot_count);
    let current = save.civil_clock.current_date;
    let issuer_ids: BTreeSet<&crate::company::CompanyId> =
        save.company_operations.companies.keys().collect();
    let stock_codes: BTreeSet<&StockCode> = save.setup.stocks.iter().map(|s| &s.code).collect();
    let mut total_acquisitions = 0_usize;
    for (id, state) in &save.information_states {
        if state.owner() != *id {
            return Err(SessionError::InvalidSave(format!(
                "information state owner {:?} does not match its key {id:?}",
                state.owner()
            )));
        }
        if id.0 == 0 || id.0 > npc_count {
            return Err(SessionError::InvalidSave(format!(
                "personal state account {id:?} is not an NPC"
            )));
        }
        // 恢复边界不变量（严格递增/跨公司唯一）对内存构造的 SaveSlot 显式复核。
        crate::information::NpcInformationState::from_parts(state.save())
            .map_err(|error| SessionError::InvalidSave(format!("account {id:?}: {error}")))?;
        for (company, records) in state.companies() {
            if !issuer_ids.contains(company) {
                return Err(SessionError::InvalidSave(format!(
                    "account {id:?} acquired publications of unknown company {company:?}"
                )));
            }
            total_acquisitions =
                total_acquisitions
                    .checked_add(records.len())
                    .ok_or_else(|| {
                        SessionError::ResourceLimit("acquisition count overflows".to_string())
                    })?;
            for record in records {
                let Some(published_at) = save.public_library.publication_instant(record.id) else {
                    return Err(SessionError::InvalidSave(format!(
                        "account {id:?} references publication {:?} missing from the library",
                        record.id
                    )));
                };
                if record.observed_at < published_at {
                    return Err(SessionError::InvalidSave(format!(
                        "account {id:?} acquired publication {:?} before it was published",
                        record.id
                    )));
                }
                if record.observed_at.date() > current {
                    return Err(SessionError::InvalidSave(format!(
                        "account {id:?} holds a future observation of publication {:?}",
                        record.id
                    )));
                }
            }
        }
    }
    if total_acquisitions > MAX_SAVED_PUBLICATIONS {
        return Err(SessionError::ResourceLimit(format!(
            "save contains {total_acquisitions} acquisitions which exceeds the limit {MAX_SAVED_PUBLICATIONS}"
        )));
    }
    for (id, watchlist) in &save.watchlists {
        let cap = save
            .setup
            .stocks
            .len()
            .saturating_add(crate::MAX_UNHELD_WATCHLIST_STOCKS);
        if watchlist.stock_count() > cap {
            return Err(SessionError::InvalidSave(format!(
                "account {id:?} watchlist exceeds the held+8 eviction bound"
            )));
        }
        for code in watchlist.stocks.keys() {
            if !stock_codes.contains(code) {
                return Err(SessionError::InvalidSave(format!(
                    "account {id:?} watches unknown stock {code:?}"
                )));
            }
        }
    }
    let saved_issuers: BTreeSet<&crate::company::CompanyId> =
        save.company_operations.companies.keys().collect();
    for (id, book) in &save.belief_books {
        if book.npc() != *id {
            return Err(SessionError::InvalidSave(format!(
                "belief book owner {:?} does not match its key {id:?}",
                book.npc()
            )));
        }
        let acquired: std::collections::BTreeSet<crate::information::PublicationId> = save
            .information_states
            .get(id)
            .map(|state| {
                state
                    .companies()
                    .flat_map(|(_, records)| records.iter().map(|record| record.id))
                    .collect()
            })
            .unwrap_or_default();
        for code in book.entry_stocks() {
            if !stock_codes.contains(code) {
                return Err(SessionError::InvalidSave(format!(
                    "account {id:?} holds a belief entry for unknown stock {code:?}"
                )));
            }
            let entry = book.entry(code).expect("entry_stocks keys always resolve");
            if !saved_issuers.contains(&entry.company) {
                return Err(SessionError::InvalidSave(format!(
                    "account {id:?} belief entry {code:?} references unknown company {:?}",
                    entry.company
                )));
            }
            if entry.horizon_trading_days == 0 {
                return Err(SessionError::InvalidSave(format!(
                    "account {id:?} belief entry {code:?} has a zero horizon"
                )));
            }
            for report in &entry.used_report_ids {
                if !acquired.contains(report) {
                    return Err(SessionError::InvalidSave(format!(
                        "account {id:?} belief entry {code:?} uses report {report:?} it never acquired"
                    )));
                }
            }
        }
    }
    Ok(())
}

/// 计划契约校验：簿规模、计划引用域、链接母单互洽、待应用事实队列。
fn validate_plan_contract(save: &SaveSlot) -> Result<(), SessionError> {
    let stock_codes: BTreeSet<&StockCode> = save.setup.stocks.iter().map(|s| &s.code).collect();
    let npc_count = u64::from(save.setup.npcs.retail_count)
        + u64::from(save.setup.npcs.inst_count)
        + u64::from(save.setup.npcs.hot_count);
    if save.plans.plan_ids().count() > MAX_SAVED_PLANS {
        return Err(SessionError::ResourceLimit(format!(
            "plan book exceeds {MAX_SAVED_PLANS} entries"
        )));
    }
    for plan_id in save.plans.plan_ids() {
        let plan = save.plans.plan(plan_id).expect("plan_ids always resolve");
        if plan.account.0 > npc_count {
            return Err(SessionError::InvalidSave(format!(
                "plan {plan_id:?} belongs to unknown account {:?}",
                plan.account
            )));
        }
        if !stock_codes.contains(&plan.code) {
            return Err(SessionError::InvalidSave(format!(
                "plan {plan_id:?} targets unknown stock {:?}",
                plan.code
            )));
        }
        if let crate::plans::PlanTarget::ShareCount(target) = plan.target {
            if target == 0 || plan.filled_qty > target {
                return Err(SessionError::InvalidSave(format!(
                    "plan {plan_id:?} share target/filled progress is invalid"
                )));
            }
        }
    }
    for (account, plans) in &save.parent_orders {
        for (code, parent) in plans {
            let Some(plan_id) = parent.linked_plan_id else {
                continue;
            };
            let linked = save.plans.plan(plan_id).map_err(|error| {
                SessionError::InvalidSave(format!(
                    "parent order for account {account:?} {code:?} links to {plan_id:?}: {error}"
                ))
            })?;
            if linked.is_terminal() || linked.account != *account || linked.code != *code {
                return Err(SessionError::InvalidSave(format!(
                    "parent order for account {account:?} {code:?} links to an incompatible plan {plan_id:?}"
                )));
            }
        }
    }
    if save.pending_plan_events.len() > MAX_SAVED_PLAN_EVENTS {
        return Err(SessionError::ResourceLimit(format!(
            "pending plan event queue exceeds {MAX_SAVED_PLAN_EVENTS} entries"
        )));
    }
    for event in &save.pending_plan_events {
        let plan = save
            .plans
            .plan(event.plan_id())
            .map_err(|error| SessionError::InvalidSave(format!("pending plan event: {error}")))?;
        if plan.is_terminal() {
            return Err(SessionError::InvalidSave(format!(
                "pending plan event targets terminal plan {:?}",
                event.plan_id()
            )));
        }
        match *event {
            PendingPlanEvent::Accepted {
                order_id,
                trading_day,
                ..
            }
            | PendingPlanEvent::Filled {
                order_id,
                trading_day,
                ..
            } => {
                if order_id.0 == 0 || order_id.0 >= save.next_order_id {
                    return Err(SessionError::InvalidSave(format!(
                        "pending plan event carries order id {order_id:?} outside the saved range"
                    )));
                }
                if trading_day > u64::from(save.snapshot.day) {
                    return Err(SessionError::InvalidSave(
                        "pending plan event is stamped after the saved trading day".to_string(),
                    ));
                }
            }
            PendingPlanEvent::DayEnded { trading_day, .. } => {
                if trading_day > u64::from(save.snapshot.day) {
                    return Err(SessionError::InvalidSave(
                        "pending plan day-end is stamped after the saved trading day".to_string(),
                    ));
                }
            }
        }
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
    if candle.time >= 0 {
        let statistics_are_valid = match (&candle.trade_stats, candle.volume) {
            (Some(stats), 0) => stats.turnover_cents == 0 && stats.trade_count == 0,
            (Some(stats), volume) => {
                // u128 represents the exact product of these u64-sized operands.
                // A wide upper bound can exceed u64 without making the candle impossible:
                // the high price may belong to only one small fill.
                let minimum_turnover = u128::from(candle.low.cents() as u64) * u128::from(volume);
                let maximum_turnover = u128::from(candle.high.cents() as u64) * u128::from(volume);
                let turnover = u128::from(stats.turnover_cents);
                turnover >= minimum_turnover
                    && turnover <= maximum_turnover
                    && stats.trade_count > 0
                    && stats.trade_count <= volume
            }
            (None, volume) => volume == 0,
        };
        if !statistics_are_valid {
            return Err(SessionError::InvalidSave(format!(
                "stock {} candle at {} contains inconsistent trade statistics",
                code.0, candle.time
            )));
        }
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
    if !matches!(
        session.phase(),
        TradingPhase::CallAuction | TradingPhase::ClosingAuction
    ) && !save.auction_orders.is_empty()
    {
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
    for lifecycle in &save.npc_order_lifecycles {
        let account = session
            .accounts
            .get(&lifecycle.account)
            .expect("lifecycle account was validated against the NPC range");
        if account.kind == AccountKind::Player {
            return Err(SessionError::InvalidSave(format!(
                "NPC quote lifecycle account {} is a player",
                lifecycle.account.0
            )));
        }
        let live_orders: Vec<_> = session
            .markets
            .get(&lifecycle.code)
            .expect("lifecycle stock was validated against setup")
            .resting_orders_for(lifecycle.account)
            .into_iter()
            .filter(|order| order.id == lifecycle.order_id)
            .collect();
        if live_orders.len() != 1 {
            return Err(SessionError::InvalidSave(format!(
                "NPC quote lifecycle order {} does not match one continuous order",
                lifecycle.order_id.0
            )));
        }
        if save
            .parent_orders
            .get(&lifecycle.account)
            .and_then(|plans| plans.get(&lifecycle.code))
            .is_some_and(|plan| plan.active_child_order_id == Some(lifecycle.order_id))
        {
            return Err(SessionError::InvalidSave(format!(
                "NPC quote lifecycle order {} duplicates an active parent child",
                lifecycle.order_id.0
            )));
        }
    }
    for (account, plans) in &save.parent_orders {
        for (code, plan) in plans {
            let Some(active_id) = plan.active_child_order_id else {
                continue;
            };
            let active_qty = match session.phase() {
                TradingPhase::CallAuction => save
                    .auction_orders
                    .get(code)
                    .into_iter()
                    .flatten()
                    .find(|order| {
                        order.arrival_seq == active_id.0
                            && order.owner == *account
                            && order.side == plan.side
                    })
                    .map(|order| order.qty)
                    .into_iter()
                    .collect::<Vec<_>>(),
                TradingPhase::Continuous | TradingPhase::PreOpen => save
                    .resting_orders
                    .get(code)
                    .into_iter()
                    .flatten()
                    .find(|order| {
                        order.id == active_id && order.owner == *account && order.side == plan.side
                    })
                    .map(|order| order.qty)
                    .into_iter()
                    .collect::<Vec<_>>(),
                TradingPhase::ClosingAuction => save
                    .auction_orders
                    .get(code)
                    .into_iter()
                    .flatten()
                    .filter(|order| {
                        order.arrival_seq == active_id.0
                            && order.owner == *account
                            && order.side == plan.side
                    })
                    .map(|order| order.qty)
                    .chain(
                        save.resting_orders
                            .get(code)
                            .into_iter()
                            .flatten()
                            .filter(|order| {
                                order.id == active_id
                                    && order.owner == *account
                                    && order.side == plan.side
                            })
                            .map(|order| order.qty),
                    )
                    .collect::<Vec<_>>(),
            };
            let [active_qty] = active_qty.as_slice() else {
                return Err(SessionError::InvalidSave(format!(
                    "parent-order account {} stock {} active child does not match a live order",
                    account.0, code.0
                )));
            };
            if plan.active_child_remaining_qty != Some(*active_qty)
                || plan
                    .filled_qty
                    .checked_add(*active_qty)
                    .is_none_or(|total| total > plan.target_qty)
            {
                return Err(SessionError::InvalidSave(format!(
                    "parent-order account {} stock {} active child exceeds remaining target",
                    account.0, code.0
                )));
            }
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
