//! 存档结构、市场、账户、订单与日 K 不变量校验。

use super::*;

mod saved_runtime;
#[cfg(test)]
mod saved_runtime_tests;

#[cfg(test)]
#[test]
fn consolidated_closing_state_rejects_missing_parent_income() {
    let (_, prehistory) = super::company_assembly::financial_fixture_tests::hash_financial_fixture();
    assert!(prehistory.closing.validate_consolidated_parent_income().is_ok());
    let invalid = crate::accounting::closing::ClosingEngine::missing_parent_income_test_fixture();
    assert!(
        matches!(invalid.validate_consolidated_parent_income(), Err(error @ crate::accounting::reports::ReportError::MissingParentIncome { .. })
        if error.to_string().contains("income.net_income_to_parent"))
    );
}

#[cfg(test)]
#[test]
fn retail_restore_rejects_institution_account_risk_pause() {
    let session =
        GameSession::new(super::npc_working_quote_tests::retail_quote_setup(), 41).unwrap();
    let save = session.save().unwrap();
    let account = save.belief_books.keys().next().unwrap().0.to_string();
    let mut encoded = serde_json::to_value(save).unwrap();
    assert_eq!(
        encoded["belief_books"][&account]["institution_account_risk_paused"],
        false
    );
    encoded["belief_books"][&account]["institution_account_risk_paused"] = serde_json::json!(true);
    let edited: SaveSlot = serde_json::from_value(encoded).unwrap();
    assert!(
        matches!(GameSession::restore(&edited), Err(SessionError::InvalidSave(message))
        if message.contains("institution account risk pause"))
    );
}

#[cfg(test)]
#[test]
fn institution_account_latch_save_is_required_strict_and_preserved_on_restore() {
    let session = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let save = session.save().unwrap();
    let account = save.belief_books.keys().next().unwrap().0.to_string();
    let mut encoded = serde_json::to_value(&save).unwrap();
    assert_eq!(
        encoded["belief_books"][&account]["institution_account_risk_paused"],
        serde_json::json!(false)
    );
    encoded["belief_books"][&account]["institution_account_risk_paused"] = serde_json::json!(true);
    let parsed: SaveSlot = serde_json::from_value(encoded.clone()).unwrap();
    let restored = GameSession::restore(&parsed).unwrap();
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap()["belief_books"][&account]
            ["institution_account_risk_paused"],
        serde_json::json!(true)
    );
    for invalid in [
        serde_json::Value::Null,
        serde_json::json!(0),
        serde_json::json!("false"),
    ] {
        let mut invalid_save = encoded.clone();
        invalid_save["belief_books"][&account]["institution_account_risk_paused"] = invalid;
        assert!(serde_json::from_value::<SaveSlot>(invalid_save).is_err());
    }
    encoded["belief_books"][&account]
        .as_object_mut()
        .unwrap()
        .remove("institution_account_risk_paused");
    assert!(serde_json::from_value::<SaveSlot>(encoded)
        .unwrap_err()
        .to_string()
        .contains("institution_account_risk_paused"));
}

#[cfg(test)]
#[test]
fn institution_boundary_restore_rejects_confidence_above_10000() {
    let session = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let save = session.save().unwrap();
    let account = *save.belief_books.keys().next().unwrap();
    let code = &save.setup.stocks[0].code;
    let company = save.company_system.issuers().iter().next().unwrap().0;
    for confidence in [0, 10_000, 10_001, 65_535] {
        let mut encoded = serde_json::to_value(&save).unwrap();
        encoded["belief_books"][account.0.to_string()]["entries"][&code.0] = serde_json::json!({
            "company": company,
            "method": null,
            "forecast": { "growth_bp": null, "basis": "InitialWithoutHistory" },
            "confidence_bp": confidence,
            "valuation": { "Unavailable": { "reason": "NonPositiveNetIncome" } },
            "used_report_ids": [],
            "anchor_trading_day": 0,
            "horizon_trading_days": 1,
            "last_cause": null,
            "applied_experience_orders": []
        });
        let edited: SaveSlot = serde_json::from_value(encoded).unwrap();
        let restored = GameSession::restore(&edited);
        if confidence <= 10_000 {
            assert!(restored.is_ok(), "legal confidence {confidence}");
        } else {
            assert!(
                matches!(restored, Err(SessionError::InvalidSave(message)) if message.contains("confidence")),
                "illegal confidence {confidence} must fail during restore"
            );
        }
    }
}

#[cfg(test)]
#[test]
fn saved_filled_identity_cannot_also_be_an_active_order() {
    let mut session =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = session.state.setup.stocks[0].code.clone();
    session.seed_order_for_test(
        AccountId(0),
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
        &mut Vec::new(),
    );
    let mut save = session.save().unwrap();
    let live = save.resting_orders[&code]
        .first()
        .expect("setup order must rest");
    save.filled_orders
        .get_mut(&code)
        .unwrap()
        .push(FilledOrderSnap {
            id: live.id,
            owner: live.owner,
        });
    assert!(
        matches!(validate_saved_order_state(&session, &save), Err(SessionError::InvalidSave(message)) if message.contains("duplicate saved order id"))
    );
}

#[cfg(test)]
#[test]
fn save_validation_context_preserves_phase_boundaries_and_shared_domain_facts() {
    let session = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let mut save = session.save().unwrap();
    save.setup.auction_ticks = 30;
    save.setup.closing_auction_ticks = 5;
    save.setup.validate().unwrap();
    for (tick, day, phase, market_minute) in [
        (0, 0, TradingPhase::CallAuction, 0),
        (19, 0, TradingPhase::CallAuction, 0),
        (20, 0, TradingPhase::PreOpen, 0),
        (29, 0, TradingPhase::PreOpen, 0),
        (30, 0, TradingPhase::Continuous, 0),
        (94, 0, TradingPhase::Continuous, 236),
        (95, 0, TradingPhase::ClosingAuction, 240),
        (99, 0, TradingPhase::ClosingAuction, 240),
        (100, 1, TradingPhase::CallAuction, 240),
    ] {
        save.snapshot.tick = tick;
        let (saved_day, day_tick, _, saved_phase) =
            SaveValidationContext::derive_trading_clock(&save).unwrap();
        assert_eq!((saved_day, day_tick, saved_phase), (day, tick % 100, phase));
        let completed_ticks = day_tick.saturating_sub(30).min(65);
        let current_market_minute = saved_day * u64::from(crate::GAME_INTRADAY_MINUTES_PER_DAY)
            + u64::from(crate::completed_market_minute_count(completed_ticks, 65).unwrap());
        let context = SaveValidationContext::new(
            &save,
            saved_day,
            day_tick,
            saved_phase,
            save.setup
                .stocks
                .iter()
                .map(|stock| stock.code.clone())
                .collect(),
            1,
            current_market_minute,
        ).unwrap();
        assert_eq!(context.current_market_minute, market_minute);
        assert_eq!(context.stock_codes.len(), save.setup.stocks.len());
        assert_eq!(
            context.issuer_ids.len(),
            save.company_system.issuers().iter().count()
        );
        assert_eq!(context.config.lot_size, 100);
        validate_personal_states(&context).unwrap();
        validate_plan_contract(&context).unwrap();
    }
}

#[cfg(test)]
#[test]
fn save_validation_context_preserves_first_error_before_domain_checks() {
    let session = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let mut save = session.save().unwrap();
    save.snapshot.markets.clear();
    save.information_states.clear();
    assert!(
        matches!(validate_save_slot(&save), Err(SessionError::InvalidSave(message)) if message == "snapshot market set does not exactly match setup")
    );
    save.snapshot.tick = (u64::from(u32::MAX) + 1) * save.setup.ticks_per_day;
    assert!(
        matches!(validate_save_slot(&save), Err(SessionError::InvalidSave(message)) if message == format!("tick {} exceeds the supported trading-day range", save.snapshot.tick))
    );
}
pub(super) use saved_runtime::{capture_runtime_state, restore_runtime_state};
pub use saved_runtime::{
    SavedEnvelopeKey, SavedFeeComponents, SavedJournalRank, SavedLiveEnvelope,
    SavedReceiptLocalKey, SavedReceiptSource, SavedReceiptTransition, SavedRetailReceiptIdentity,
    SavedRuntimeState, SIMULATION_POLICY_ID,
};

fn saved_stock_days(save: &SaveSlot, code: &StockCode) -> Result<u64, SessionError> {
    save.snapshot.daily_candles.get(code)
        .and_then(|candles| candles.len().checked_sub(360))
        .and_then(|count| u64::try_from(count).ok())
        .ok_or_else(|| SessionError::InvalidSave(format!("证券 {} 缺少完整交易日历史", code.0)))
}

/// 单次 SaveSlot 校验的只读事实：派生值在原门禁位置完成后才组合。
/// 不绑定恢复后的 GameSession，也不推断或修补可编辑存档事实。
struct SaveValidationContext<'a> {
    save: &'a SaveSlot,
    config: &'a GameConfig,
    saved_day: u64,
    day_tick: u64,
    phase: TradingPhase,
    stock_codes: BTreeSet<StockCode>,
    issuer_ids: BTreeSet<&'a crate::company::CompanyId>,
    npc_count: u64,
    current_market_minute: u64,
    civil_clock: CivilClock,
}

impl<'a> SaveValidationContext<'a> {
    fn new(
        save: &'a SaveSlot,
        saved_day: u64,
        day_tick: u64,
        phase: TradingPhase,
        stock_codes: BTreeSet<StockCode>,
        npc_count: u64,
        current_market_minute: u64,
    ) -> Result<Self, SessionError> {
        let civil_clock = CivilClock::from_parts_for_exchanges(
            save.setup.start_date, &save.civil_clock,
            save.setup.stocks.iter().map(|stock| session_calendar_exchange(stock.exchange)),
        )?;
        Ok(Self {
            save,
            config: &save.setup.config,
            saved_day,
            day_tick,
            phase,
            stock_codes,
            issuer_ids: save.company_system.issuers().iter().map(|(id, _)| id).collect(),
            npc_count,
            current_market_minute,
            civil_clock,
        })
    }

    fn stock_day(&self, code: &StockCode) -> Result<u64, SessionError> {
        saved_stock_days(self.save, code)
    }

    fn stock_is_open(&self, code: &StockCode) -> Result<bool, SessionError> {
        let stock = self.save.setup.stocks.iter().find(|stock| &stock.code == code)
            .ok_or_else(|| SessionError::InvalidSave(format!("日历事实指向未知证券 {}", code.0)))?;
        Ok(self.civil_clock.exchange_day_status(session_calendar_exchange(stock.exchange), self.save.civil_clock.current_date)? == crate::DayStatus::Trading)
    }

    fn stock_minute(&self, code: &StockCode) -> Result<u64, SessionError> {
        let day_start = self.stock_day(code)?.checked_mul(u64::from(GAME_INTRADAY_MINUTES_PER_DAY))
            .ok_or_else(|| SessionError::InvalidSave("证券分钟偏移溢出".into()))?;
        let completed = if self.stock_is_open(code)? {
            let continuous_ticks = self.save.setup.ticks_per_day
                - self.save.setup.auction_ticks
                - self.save.setup.closing_auction_ticks;
            let continuous_day_tick = self.day_tick
                .saturating_sub(self.save.setup.auction_ticks)
                .min(continuous_ticks);
            u64::from(completed_market_minute_count(continuous_day_tick, continuous_ticks)
                .map_err(|error| SessionError::InvalidSave(error.to_string()))?)
        } else { 0 };
        day_start.checked_add(completed).ok_or_else(|| SessionError::InvalidSave("证券分钟偏移溢出".into()))
    }

    fn completed_live_dates(&self, exchange: StockExchange) -> Result<Vec<crate::CivilDate>, SessionError> {
        let includes_current = self.day_tick == 0
            && self.saved_day == u64::from(self.civil_clock.completed_trading_sessions_expected()?)
            && self.civil_clock.phase() == CivilPhase::IntradayTrading;
        let mut dates = Vec::new();
        let mut date = self.save.setup.start_date;
        while date < self.save.civil_clock.current_date || (includes_current && date == self.save.civil_clock.current_date) {
            if self.civil_clock.exchange_day_status(session_calendar_exchange(exchange), date)? == crate::DayStatus::Trading {
                dates.push(date);
            }
            date = date.next().map_err(crate::calendar::CalendarError::from)?;
        }
        Ok(dates)
    }

    /// 仅在 schema/setup 守卫通过后调用，维持交易日范围错误先于集合校验。
    fn derive_trading_clock(
        save: &SaveSlot,
    ) -> Result<(u64, u64, u64, TradingPhase), SessionError> {
        let saved_day = save.snapshot.tick / save.setup.ticks_per_day;
        if saved_day > u64::from(u32::MAX) {
            return Err(SessionError::InvalidSave(format!(
                "tick {} exceeds the supported trading-day range",
                save.snapshot.tick
            )));
        }
        let day_tick = save.snapshot.tick % save.setup.ticks_per_day;
        let auction_entry_ticks = save.setup.auction_ticks - save.setup.auction_ticks / 3;
        let phase = if day_tick < auction_entry_ticks {
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

        Ok((saved_day, day_tick, auction_entry_ticks, phase))
    }
}

pub(super) fn validate_save_slot(save: &SaveSlot) -> Result<(), SessionError> {
    if save.setup.simulation_policy_id != SIMULATION_POLICY_ID {
        return Err(SessionError::InvalidSave(format!(
            "当前存档要求 simulation_policy_id 为 {SIMULATION_POLICY_ID:?}，实际为 {:?}",
            save.setup.simulation_policy_id
        )));
    }
    save.setup
        .validate()
        .map_err(|error| SessionError::InvalidSave(format!("invalid setup: {error}")))?;

    let (saved_day, day_tick, auction_entry_ticks, saved_phase) =
        SaveValidationContext::derive_trading_clock(save)?;

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
    let sequence_markets: BTreeSet<StockCode> = save.book_next_sequences.keys().cloned().collect();
    if sequence_markets != expected_markets {
        return Err(SessionError::InvalidSave(
            "book sequence market set does not exactly match setup".to_owned(),
        ));
    }
    let resting_markets: BTreeSet<StockCode> = save.resting_orders.keys().cloned().collect();
    if !resting_markets.is_empty() && resting_markets != expected_markets {
        return Err(SessionError::InvalidSave(
            "resting-order market set does not exactly match setup".to_string(),
        ));
    }
    let filled_markets: BTreeSet<StockCode> = save.filled_orders.keys().cloned().collect();
    if filled_markets != expected_markets {
        return Err(SessionError::InvalidSave(
            "filled-order market set does not exactly match setup".to_string(),
        ));
    }
    let mut filled_ids = BTreeSet::new();
    for (code, orders) in &save.filled_orders {
        for order in orders {
            if order.id.0 >= save.next_order_id
                || !save.snapshot.accounts.contains_key(&order.owner)
                || !filled_ids.insert(order.id)
            {
                return Err(SessionError::InvalidSave(format!(
                    "invalid filled-order identity {:?} for {}",
                    order.id, code.0
                )));
            }
        }
    }
    let history_markets: BTreeSet<StockCode> = save.price_history.keys().cloned().collect();
    if history_markets != expected_markets {
        return Err(SessionError::InvalidSave(
            "price-history market set does not exactly match setup".to_string(),
        ));
    }
    let stock_calendar = crate::calendar::TradingCalendar::from_policy(
        crate::calendar::CalendarPolicy::from_parts(save.civil_clock.policy.clone())?,
    )?;
    for (code, prices) in &save.price_history {
        let stock = save.setup.stocks.iter().find(|stock| &stock.code == code)
            .expect("已核对证券代码集合");
        let is_open = stock_calendar.is_trading_day(session_calendar_exchange(stock.exchange), save.civil_clock.current_date)?;
        let continuous_ticks_per_day = save
            .setup
            .ticks_per_day
            .saturating_sub(save.setup.auction_ticks)
            .saturating_sub(save.setup.closing_auction_ticks);
        let completed_continuous_ticks = saved_stock_days(save, code)?
            .checked_mul(continuous_ticks_per_day)
            .and_then(|ticks| {
                ticks.checked_add(
                    if is_open { (save.snapshot.tick % save.setup.ticks_per_day)
                        .saturating_sub(save.setup.auction_ticks)
                        .min(continuous_ticks_per_day) } else { 0 },
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
    for (code, market) in &save.snapshot.markets {
        let stock = save.setup.stocks.iter().find(|stock| &stock.code == code)
            .expect("已核对证券代码集合");
        crate::market::Market::validate_restored_facts(
            code,
            save.civil_clock.current_date,
            market.last_price,
            market.last_close,
            market.cash_ex_reference_pending_trade,
            market.last_cash_ex_reference,
            stock.tick,
        ).map_err(|error| SessionError::InvalidSave(format!("证券 {} 的恢复行情状态非法：{error}", code.0)))?;
        if market.day_market_activity {
            let is_open = stock_calendar.is_trading_day(session_calendar_exchange(stock.exchange), save.civil_clock.current_date)?;
            if !is_open || day_tick == 0 {
                return Err(SessionError::InvalidSave(format!("证券 {} 的日内市场活动标记与自然日时钟不一致", code.0)));
            }
        }
    }

    let minute_markets: BTreeSet<StockCode> = save.market_minute_closes.keys().cloned().collect();
    if minute_markets != expected_markets {
        return Err(SessionError::InvalidSave(
            "market-minute history market set does not exactly match setup".to_string(),
        ));
    }
    let continuous_ticks_per_day = save
        .setup
        .ticks_per_day
        .saturating_sub(save.setup.auction_ticks)
        .saturating_sub(save.setup.closing_auction_ticks);
    let completed_continuous_ticks = day_tick
        .saturating_sub(save.setup.auction_ticks)
        .min(continuous_ticks_per_day);
    let day_start = saved_day
        .checked_mul(u64::from(crate::GAME_INTRADAY_MINUTES_PER_DAY))
        .ok_or_else(|| {
            SessionError::InvalidSave("market-minute day offset overflow".to_string())
        })?;
    let completed_minutes = u64::from(
        crate::completed_market_minute_count(completed_continuous_ticks, continuous_ticks_per_day)
            .map_err(|error| SessionError::InvalidSave(error.to_string()))?,
    );
    for (code, minutes) in &save.market_minute_closes {
        let stock = save.setup.stocks.iter().find(|stock| &stock.code == code)
            .expect("已核对证券代码集合");
        let is_open = stock_calendar.is_trading_day(session_calendar_exchange(stock.exchange), save.civil_clock.current_date)?;
        let stock_start = saved_stock_days(save, code)?.checked_mul(u64::from(GAME_INTRADAY_MINUTES_PER_DAY))
            .ok_or_else(|| SessionError::InvalidSave("证券分钟偏移溢出".into()))?;
        let expected_minute_keys = (0..if is_open { completed_minutes } else { 0 })
            .map(|minute| stock_start + minute).collect::<Vec<_>>();
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
    save.market_memberships.validate(save)?;

    let expected_attention_accounts: BTreeSet<AccountId> = (1..=npc_count).map(AccountId).collect();
    let actual_attention_accounts: BTreeSet<AccountId> =
        save.npc_attention.keys().copied().collect();
    if actual_attention_accounts != expected_attention_accounts {
        return Err(SessionError::InvalidSave(
            "NPC attention account set does not exactly match setup".to_string(),
        ));
    }
    for (id, state) in &save.npc_attention {
        state.information_cadence.validate().map_err(|reason| {
            SessionError::InvalidSave(format!("NPC {id:?} 信息关注节奏无效: {reason}"))
        })?;
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
    let context = SaveValidationContext::new(
        save,
        saved_day,
        day_tick,
        saved_phase,
        expected_markets,
        npc_count,
        current_market_minute,
    )?;
    let expected_markets = &context.stock_codes;
    let expected_through_current = u64::from(context.civil_clock.completed_trading_sessions_expected()?);
    let expected_before_current = expected_through_current - u64::from(context.civil_clock.phase() == CivilPhase::IntradayTrading);
    if context.saved_day != expected_before_current
        && !(context.day_tick == 0 && context.saved_day == expected_through_current)
        || (context.day_tick > 0 && context.civil_clock.phase() == CivilPhase::ClosedDay) {
        return Err(SessionError::InvalidSave("市场tick与共享自然日的实际开市会话数不一致".into()));
    }
    let day_end_market_minute = day_start
        .checked_add(u64::from(crate::GAME_INTRADAY_MINUTES_PER_DAY))
        .ok_or_else(|| {
            SessionError::InvalidSave("NPC quote lifecycle day range overflows".to_string())
        })?;
    let mut lifecycle_keys = BTreeSet::new();
    for lifecycle in &save.npc_order_lifecycles {
        if lifecycle.account.0 == 0 || lifecycle.account.0 > context.npc_count {
            return Err(SessionError::InvalidSave(format!(
                "NPC quote lifecycle account {} is not an NPC",
                lifecycle.account.0
            )));
        }
        if !expected_markets.contains(&lifecycle.code)
            || lifecycle.order_id.0 == 0
            || lifecycle.order_id.0 >= save.next_order_id
            || lifecycle.placed_market_minute < day_start
            || lifecycle.placed_market_minute > context.current_market_minute
            || lifecycle.expires_market_minute <= lifecycle.placed_market_minute
            || lifecycle.expires_market_minute > day_end_market_minute
            || (context.phase == TradingPhase::Continuous
                && lifecycle.expires_market_minute <= context.current_market_minute)
            || (context.phase != TradingPhase::Continuous
                && context.phase != TradingPhase::ClosingAuction)
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
                || plan.target_qty % context.config.lot_size != 0
                || plan.child_qty % context.config.lot_size != 0
                || plan.limit_price.cents() <= 0
                || plan.expires_market_minute <= context.stock_minute(code)?
            {
                return Err(SessionError::InvalidSave(format!(
                    "parent-order account {} stock {} violates execution-plan invariants",
                    account.0, code.0
                )));
            }
            if plan
                .active_child_order_id
                .is_some_and(|id| id.0 == 0 || id.0 >= save.next_order_id)
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
            if stock.last_trade_market_minute > context.current_market_minute
                || stock.last_observed_market_minute > context.current_market_minute
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
        experience.feedback.validate().map_err(|error| {
            SessionError::InvalidSave(format!(
                "retail account {id:?} feedback is inconsistent: {error}"
            ))
        })?;
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
    }

    for (id, account) in &save.snapshot.accounts {
        for (code, position) in &account.positions {
            if (!save.setup.t1_enabled || (context.phase == TradingPhase::CallAuction && context.stock_is_open(code)?))
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
    if daily_candle_markets != *expected_markets {
        return Err(SessionError::InvalidSave(
            "daily-candle market set does not exactly match setup".to_string(),
        ));
    }
    let candle_calendar = crate::calendar::TradingCalendar::from_policy(
        crate::calendar::CalendarPolicy::from_parts(save.civil_clock.policy.clone())?,
    )?;
    for (code, candles) in &save.snapshot.daily_candles {
        if !expected_markets.contains(code) {
            return Err(SessionError::InvalidSave(format!(
                "daily candles contain unknown stock {}",
                code.0
            )));
        }
        let stock = save
            .setup
            .stocks
            .iter()
            .find(|stock| &stock.code == code)
            .expect("daily candle market is validated against setup");
        let mut expected_dates =
            candles::preset_candle_dates(&candle_calendar, save.setup.start_date, stock.exchange)?;
        expected_dates.extend(context.completed_live_dates(stock.exchange)?);
        if candles.len() != expected_dates.len() {
            return Err(SessionError::InvalidSave(format!(
                "daily candles for {} have length {}; expected {}", code.0, candles.len(), expected_dates.len(),
            )));
        }
        let mut previous_time = None;
        for (index, candle) in candles.iter().enumerate() {
            validate_candle(code, candle, index >= 360)?;
            if previous_time.is_some_and(|time| candle.time <= time) {
                return Err(SessionError::InvalidSave(format!(
                    "daily candles for {} are not strictly ordered",
                    code.0
                )));
            }
            let expected_time = candles::candle_date_time(expected_dates[index]);
            if candle.time != expected_time {
                return Err(SessionError::InvalidSave(format!(
                    "daily candle {} for {} has time {}; expected {}",
                    index, code.0, candle.time, expected_time
                )));
            }
            previous_time = Some(candle.time);
        }
    }

    if save
        .pending_player
        .iter()
        .any(|received| !save.market_memberships.members.values().any(|member| member.account_id == received.owner))
    {
        return Err(SessionError::InvalidSave(
            "pending player intent must belong to the player account".to_string(),
        ));
    }
    match &save.pending_npc {
        None if context.day_tick == 0 => {}
        Some(queued) if queued.observed_tick == save.snapshot.tick => {
            queued
                .validate_dependencies()
                .map_err(SessionError::InvalidSave)?;
            if queued
                .observed_accounts
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            {
                return Err(SessionError::InvalidSave(
                    "pending NPC observed accounts are not unique".to_string(),
                ));
            }
            let observed: BTreeSet<_> = queued.observed_accounts.iter().copied().collect();
            if observed.iter().any(|account| {
                *account == AccountId(0)
                    || !save.snapshot.accounts.contains_key(account)
                    || !save.npc_attention.contains_key(account)
            }) || queued
                .intents
                .iter()
                .any(|received| !observed.contains(&received.owner))
            {
                return Err(SessionError::InvalidSave(
                    "pending NPC request has no observed NPC account".to_string(),
                ));
            }
        }
        _ => {
            return Err(SessionError::InvalidSave(
                "pending NPC observation tick does not match the saved market tick".to_string(),
            ))
        }
    }
    let mut seen_account_receipts = BTreeSet::new();
    let mut seen_stock_receipts = BTreeSet::new();
    for received in save.pending_player.iter().chain(
        save.pending_npc
            .iter()
            .flat_map(|queued| queued.intents.iter()),
    ) {
        let next_account = save
            .ingress_receipt_cursors
            .next_account_ordinal
            .get(&received.owner)
            .ok_or_else(|| {
                SessionError::InvalidSave(format!(
                    "pending intent for account {:?} has no receipt cursor",
                    received.owner
                ))
            })?;
        if received.account_ordinal >= *next_account
            || !seen_account_receipts.insert((received.owner, received.account_ordinal))
        {
            return Err(SessionError::InvalidSave(format!(
                "pending intent for account {:?} has an invalid or duplicate receipt ordinal",
                received.owner
            )));
        }
        let next_stock = save
            .ingress_receipt_cursors
            .next_stock_ordinal
            .get(match &received.intent {
                Intent::PlaceLimit { code, .. }
                | Intent::PlaceMarket { code, .. }
                | Intent::Cancel { code, .. } => code,
            })
            .ok_or_else(|| {
                SessionError::InvalidSave(format!(
                    "pending intent for stock {:?} has no receipt cursor",
                    match &received.intent {
                        Intent::PlaceLimit { code, .. }
                        | Intent::PlaceMarket { code, .. }
                        | Intent::Cancel { code, .. } => code,
                    }
                ))
            })?;
        let stock_code = match &received.intent {
            Intent::PlaceLimit { code, .. }
            | Intent::PlaceMarket { code, .. }
            | Intent::Cancel { code, .. } => code,
        };
        if received.stock_ordinal >= *next_stock
            || !seen_stock_receipts.insert((stock_code.clone(), received.stock_ordinal))
        {
            return Err(SessionError::InvalidSave(format!(
                "pending intent for stock {} has an invalid or duplicate receipt ordinal",
                stock_code.0
            )));
        }
    }
    validate_pending_receipt_order(save)?;
    let active_candle_markets: BTreeSet<StockCode> =
        save.snapshot.active_daily_candles.keys().cloned().collect();
    let active_candle_set_is_valid =
        if context.day_tick == 0 || context.day_tick < auction_entry_ticks {
            active_candle_markets.is_empty()
        } else {
            let open = expected_markets.iter().filter_map(|code| match context.stock_is_open(code) {
                Ok(true) => Some(Ok(code.clone())), Ok(false) => None, Err(error) => Some(Err(error)),
            }).collect::<Result<BTreeSet<_>, _>>()?;
            active_candle_markets == open
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
        validate_candle(code, candle, true)?;
        if candle.time != candles::candle_date_time(save.civil_clock.current_date) {
            return Err(SessionError::InvalidSave(format!(
                "active candle for {} does not match the current civil date",
                code.0
            )));
        }
    }

    if save.next_order_id == 0
        || save
            .auction_orders
            .values()
            .flatten()
            .any(|order| order.order_id >= save.next_order_id)
    {
        return Err(SessionError::InvalidSave(
            "next_order_id is not greater than every saved order id".to_string(),
        ));
    }

    validate_company_domain(save)?;
    validate_disclosure_cursors(&context)?;
    validate_personal_states(&context)?;
    validate_plan_contract(&context)?;
    Ok(())
}

/// 存档解码的默认字节上限；不限制市场中的公司数量。
pub const MAX_SAVE_DECODE_BYTES: usize = 512 * 1024 * 1024;

/// 存档解码字节门禁（可按部署配置）。
#[derive(Clone, Copy, Debug)]
pub struct SaveDecodeLimits {
    /// 存档 JSON 总解码字节上限。
    pub max_total_bytes: usize,
}

impl Default for SaveDecodeLimits {
    fn default() -> Self {
        Self {
            max_total_bytes: MAX_SAVE_DECODE_BYTES,
        }
    }
}

/// 存档解码入口：先字节门禁，后 serde 解码（结构化字段缺失/多余/类型错误
/// 走通用拒绝）。任何失败 = 类型化错误，调用方会话与源字节保持原样。
pub fn decode_save_slot(json: &[u8], limits: &SaveDecodeLimits) -> Result<SaveSlot, SessionError> {
    if json.len() > limits.max_total_bytes {
        return Err(SessionError::ResourceLimit(format!(
            "save payload {} bytes exceeds the decode limit {} bytes",
            json.len(),
            limits.max_total_bytes
        )));
    }
    serde_json::from_slice(json)
        .map_err(|error| SessionError::InvalidSave(format!("save JSON is not decodable: {error}")))
}

/// 公司域权威状态校验：经营编排集合/推进时点、镜像与时钟到期一致性。
fn validate_pending_receipt_order(save: &SaveSlot) -> Result<(), SessionError> {
    #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
    enum ResourceLane {
        Cash(AccountId),
        Shares(AccountId, StockCode),
    }

    let mut intents = save.pending_player.clone();
    let npc_len = save
        .pending_npc
        .as_ref()
        .map_or(0, |queued| queued.intents.len());
    if let Some(queued) = &save.pending_npc {
        intents.extend(queued.intents.iter().cloned());
    }
    let player_len = intents.len() - npc_len;
    let mut edges = vec![BTreeSet::new(); intents.len()];
    let mut resource_lanes = BTreeMap::<ResourceLane, Vec<(usize, u64)>>::new();
    let mut stock_lanes = BTreeMap::<StockCode, Vec<(usize, u64)>>::new();
    for (index, received) in intents.iter().enumerate() {
        let code = match &received.intent {
            Intent::PlaceLimit { code, side, .. } | Intent::PlaceMarket { code, side, .. } => {
                let lane = match side {
                    Side::Buy => ResourceLane::Cash(received.owner),
                    Side::Sell => ResourceLane::Shares(received.owner, code.clone()),
                };
                resource_lanes
                    .entry(lane)
                    .or_default()
                    .push((index, received.account_ordinal));
                code.clone()
            }
            Intent::Cancel { code, .. } => code.clone(),
        };
        stock_lanes
            .entry(code)
            .or_default()
            .push((index, received.stock_ordinal));
    }
    for lane in resource_lanes.values_mut() {
        lane.sort_unstable_by_key(|(_, ordinal)| *ordinal);
        for pair in lane.windows(2) {
            edges[pair[0].0].insert(pair[1].0);
        }
    }
    for lane in stock_lanes.values_mut() {
        lane.sort_unstable_by_key(|(_, ordinal)| *ordinal);
        for pair in lane.windows(2) {
            edges[pair[0].0].insert(pair[1].0);
        }
    }
    if let Some(queued) = &save.pending_npc {
        for &(before, after) in &queued.dependencies {
            if before >= npc_len || after >= npc_len {
                return Err(SessionError::InvalidSave(
                    "pending NPC dependency index is outside the queued batch".to_string(),
                ));
            }
            edges[player_len + before].insert(player_len + after);
        }
    }
    let mut incoming = vec![0_usize; intents.len()];
    for successors in &edges {
        for &successor in successors {
            incoming[successor] = incoming[successor].checked_add(1).ok_or_else(|| {
                SessionError::InvalidSave(
                    "pending ingress receipt precedence edge count overflow".to_string(),
                )
            })?;
        }
    }
    let mut ready = incoming
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect::<Vec<_>>();
    let mut visited = 0_usize;
    while let Some(index) = ready.pop() {
        visited += 1;
        for &successor in &edges[index] {
            incoming[successor] = incoming[successor].checked_sub(1).ok_or_else(|| {
                SessionError::InvalidSave(
                    "pending ingress receipt precedence count underflow".to_string(),
                )
            })?;
            if incoming[successor] == 0 {
                ready.push(successor);
            }
        }
    }
    if visited != intents.len() {
        return Err(SessionError::InvalidSave(
            "pending ingress receipt precedence contains a cycle".to_string(),
        ));
    }
    Ok(())
}

fn validate_company_domain(save: &SaveSlot) -> Result<(), SessionError> {
    save.company_system.validate_restored().map_err(|error| SessionError::InvalidSave(format!("公司系统状态非法：{error}")))?;
    if save.company_system.config() != save.setup.company_system {
        return Err(SessionError::InvalidSave("公司系统配置与新局选定配置不一致".into()));
    }
    let specs = super::company_assembly::issuer_specs(&save.setup)
        .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
    let issuers = crate::company::identity::IssuerRegistry::new(specs)
        .map_err(|error| SessionError::InvalidSave(format!("发行人身份非法：{error}")))?;
    if save.company_system.issuers() != &issuers {
        return Err(SessionError::InvalidSave("公司系统发行人身份与股票配置不一致".into()));
    }
    save.company_system.issuers().validate_issuer_mapping(&save.setup.stocks.iter().map(|stock| (stock.code.clone(), stock.total_shares)).collect::<Vec<_>>())
        .map_err(|error| SessionError::InvalidSave(format!("发行人映射非法：{error}")))?;
    let account_positions = save.snapshot.accounts.iter().map(|(id, account)| {
        (*id, account.positions.iter().map(|(code, position)| (code.clone(), u64::from(position.qty))).collect())
    }).collect();
    save.corporate_actions.validate(&account_positions, &save.company_system, save.civil_clock.current_date)
        .map_err(|error| SessionError::InvalidSave(format!("公司行为状态非法：{error}")))?;
    let expected = save.civil_clock.current_date.prev().map_err(|error| SessionError::InvalidSave(error.to_string()))?;
    if save.company_system.advanced_through() != expected {
        return Err(SessionError::InvalidSave("公司系统推进日期与自然日时钟不一致".into()));
    }
    super::company_corrections::validate_completed(&save.report_correction_operations, &save.company_system, &save.public_library, save.civil_clock.current_date)?;
    let library = save.public_library.save();
    for report in &library.reports {
        if report.source != crate::information::PublicationSource::SimpleGenerated {
            return Err(SessionError::InvalidSave("公开报告来源与当前 Simple 公司系统不一致".into()));
        }
        if save.company_system.issuers().get(&report.company).is_none() {
            return Err(SessionError::InvalidSave("公开报告引用未知发行人".into()));
        }
    }
    let mut cash_dividend_announcements = std::collections::BTreeSet::new();
    for announcement in &library.announcements {
        if save.company_system.issuers().get(&announcement.company).is_none() {
            return Err(SessionError::InvalidSave("公开公告引用未知发行人".into()));
        }
        if let crate::information::AnnouncementContent::CashDividend(dividend) = &announcement.content {
            let identity = (announcement.company.clone(), dividend.plan.plan_id.clone());
            if !cash_dividend_announcements.insert(identity.clone()) {
                return Err(SessionError::InvalidSave(format!(
                    "现金分红计划 {} 存在重复公开公告",
                    dividend.plan.plan_id
                )));
            }
            let book = save.corporate_actions.dividends.iter().find(|book| {
                book.plan().issuer == announcement.company
                    && book.plan().plan_id == dividend.plan.plan_id
            }).ok_or_else(|| SessionError::InvalidSave(format!(
                "现金分红公告 {} 缺少对应公司行为账簿",
                dividend.plan.plan_id
            )))?;
            if book.plan() != &dividend.plan
                || book.status() == &crate::company::cash_dividend::CashDividendStatus::Approved
            {
                return Err(SessionError::InvalidSave(format!(
                    "现金分红公告 {} 与已批准计划或生命周期不一致",
                    dividend.plan.plan_id
                )));
            }
            let finance_plan = save.company_system.dividend_plan_facts(&announcement.company)
                .map_err(|error| SessionError::InvalidSave(format!(
                    "现金分红公告 {} 查询批准财务事实失败：{error}",
                    dividend.plan.plan_id
                )))?
                .into_iter()
                .find(|fact| fact.plan_id == dividend.plan.plan_id)
                .ok_or_else(|| SessionError::InvalidSave(format!(
                    "现金分红公告 {} 缺少 Simple 批准财务事实",
                    dividend.plan.plan_id
                )))?;
            let approved_gross = finance_plan.total_gross.to_money()
                .map_err(|error| SessionError::InvalidSave(format!(
                    "现金分红公告 {} 批准金额无法表示为 Money：{error}",
                    dividend.plan.plan_id
                )))?;
            if dividend.total_gross != approved_gross {
                return Err(SessionError::InvalidSave(format!(
                    "现金分红公告 {} 税前总额与 Simple 批准金额不一致",
                    dividend.plan.plan_id
                )));
            }
        }
    }
    for book in &save.corporate_actions.dividends {
        let announced = cash_dividend_announcements.contains(&(
            book.plan().issuer.clone(),
            book.plan().plan_id.clone(),
        ));
        if announced
            != (book.status() != &crate::company::cash_dividend::CashDividendStatus::Approved)
        {
            return Err(SessionError::InvalidSave(format!(
                "现金分红计划 {} 的公告与生命周期状态不一致",
                book.plan().plan_id
            )));
        }
    }
    crate::information::PublicLibrary::from_parts(library)
        .map_err(|error| SessionError::InvalidSave(format!("公开材料非法：{error}")))?;
    Ok(())
}

/// 披露派发游标自洽：恰好一次语义在存档时点的投影。
fn validate_disclosure_cursors(context: &SaveValidationContext) -> Result<(), SessionError> {
    let save = context.save;
    let current = save.civil_clock.current_date;
    let monthly = matches!(save.setup.report_frequency, crate::information::ReportFrequency::Monthly { .. });
    let closing = context.day_tick == 0
        && context.civil_clock.phase() == CivilPhase::IntradayTrading
        && context.saved_day == u64::from(context.civil_clock.completed_trading_sessions_expected()?);
    let has_intraday_dispatch = monthly && (context.day_tick > 0 || closing);
    let intraday_instant = if closing {
        CivilInstant::new(current, 15 * 3600).map_err(|error| SessionError::InvalidSave(error.to_string()))?
    } else {
        super::observation_clock::observation_instant_at(current, context.day_tick, &save.setup)
    };
    match (
        save.civil_clock.settled_through,
        save.disclosures.announced_through(),
    ) {
        (None, None) => {
            if has_intraday_dispatch {
                if save.disclosures.published_through() != Some(intraday_instant) {
                    return Err(SessionError::InvalidSave("disclosure cursor does not match the committed intraday instant".into()));
                }
            } else if save.disclosures.published_through().is_some_and(|through| through.date() >= current) {
                return Err(SessionError::InvalidSave("disclosure cursor precedes the first dispatch but points into the current or future day".into()));
            }
        }
        (Some(settled), Some(announced)) if settled == announced => {
            // 每个已日结自然日的披露相位都是 18:00；游标必须精确落在其上。
            let second = if monthly { 86399 } else { 18 * 3600 };
            let phase = crate::calendar::CivilInstant::new(settled, second)
                .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
            let expected = if has_intraday_dispatch { intraday_instant } else { phase };
            if save.disclosures.published_through() != Some(expected) {
                return Err(SessionError::InvalidSave(format!(
                    "disclosure cursor {:?} does not match the configured daily dispatch endpoint of settled {settled}",
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
fn validate_personal_states(context: &SaveValidationContext) -> Result<(), SessionError> {
    let save = context.save;
    let saved_day = context.saved_day;
    let belief_keys: BTreeSet<AccountId> = save.belief_books.keys().copied().collect();
    let information_keys: BTreeSet<AccountId> = save.information_states.keys().copied().collect();
    let watchlist_keys: BTreeSet<AccountId> = save.watchlists.keys().copied().collect();
    let memory_keys: BTreeSet<AccountId> = save.price_memories.keys().copied().collect();
    let history_read_keys: BTreeSet<AccountId> = save.history_reads.keys().copied().collect();
    if belief_keys != information_keys
        || belief_keys != watchlist_keys
        || belief_keys != memory_keys
    {
        return Err(SessionError::InvalidSave(
            "belief books, information states, watchlists, and price memories must cover the same account set"
                .to_string(),
        ));
    }
    let npc_count = context.npc_count;
    let current = save.civil_clock.current_date;
    let issuer_ids = &context.issuer_ids;
    let stock_codes = &context.stock_codes;
    let current_market_minute = context.current_market_minute;
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
    let account_keys: BTreeSet<AccountId> = save.snapshot.accounts.keys().copied().collect();
    if history_read_keys != account_keys {
        return Err(SessionError::InvalidSave(
            "history-read ledgers must cover every account exactly".to_string(),
        ));
    }
    for (id, watchlist) in &save.watchlists {
        let protected: BTreeSet<_> = save.snapshot.accounts[id]
            .positions
            .keys()
            .chain(save.plans.active_codes(*id))
            .collect();
        if watchlist
            .stocks
            .keys()
            .filter(|code| !protected.contains(*code))
            .count()
            > crate::MAX_UNHELD_WATCHLIST_STOCKS
        {
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
    for (id, memory) in &save.price_memories {
        let protected: BTreeSet<_> = save.snapshot.accounts[id]
            .positions
            .keys()
            .chain(save.plans.active_codes(*id))
            .collect();
        if memory
            .stocks
            .keys()
            .filter(|code| !protected.contains(*code))
            .count()
            > crate::MAX_UNHELD_WATCHLIST_STOCKS
        {
            return Err(SessionError::InvalidSave(format!(
                "account {id:?} price memory exceeds the held+8 eviction bound"
            )));
        }
        for (code, entry) in &memory.stocks {
            if !stock_codes.contains(code)
                || entry.first_observed_price.cents() <= 0
                || entry.last_observed_price.cents() <= 0
                || entry.observed_high.cents() <= 0
                || entry.observed_low.cents() <= 0
                || entry.first_observed_minute > entry.last_observed_minute
                || entry.last_observed_minute > context.stock_minute(code)?
                || entry.last_touched_minute > current_market_minute
                || entry.observed_low > entry.observed_high
                || entry.first_observed_price < entry.observed_low
                || entry.first_observed_price > entry.observed_high
                || entry.last_observed_price < entry.observed_low
                || entry.last_observed_price > entry.observed_high
            {
                return Err(SessionError::InvalidSave(format!(
                    "account {id:?} has inconsistent price memory for {code:?}"
                )));
            }
        }
    }
    for (id, ledger) in &save.history_reads {
        for (code, entry) in &ledger.stocks {
            if !stock_codes.contains(code)
                || entry.read_count == 0
                || entry.last_read_market_minute > current_market_minute
            {
                return Err(SessionError::InvalidSave(format!(
                    "account {id:?} has inconsistent history-read fact for {code:?}"
                )));
            }
        }
    }
    let saved_issuers = &context.issuer_ids;
    for (id, book) in &save.belief_books {
        if book.npc() != *id {
            return Err(SessionError::InvalidSave(format!(
                "belief book owner {:?} does not match its key {id:?}",
                book.npc()
            )));
        }
        let expected_kind = if id.0 <= u64::from(save.setup.npcs.retail_count) {
            AccountKind::Retail
        } else if id.0
            <= u64::from(save.setup.npcs.retail_count) + u64::from(save.setup.npcs.inst_count)
        {
            AccountKind::Inst
        } else {
            AccountKind::Hot
        };
        let profile_matches = matches!(
            (expected_kind, book.profile()),
            (AccountKind::Retail, StrategyProfile::Retail(_))
                | (AccountKind::Inst, StrategyProfile::Institution(_))
        );
        if !profile_matches {
            return Err(SessionError::InvalidSave(format!(
                "account {id:?} belief profile conflicts with account kind"
            )));
        }
        let institution = matches!(book.profile(), StrategyProfile::Institution(_));
        if institution && book.institution_policy().is_none() {
            return Err(SessionError::InvalidSave(format!(
                "institution account {} has no frozen experience policy",
                id.0
            )));
        }
        if !institution && book.institution_policy().is_some() {
            return Err(SessionError::InvalidSave(format!(
                "non-institution account {id:?} has institution experience policy"
            )));
        }
        if !institution && book.institution_account_risk_paused() {
            return Err(SessionError::InvalidSave(format!(
                "non-institution account {id:?} has institution account risk pause"
            )));
        }
        let experience = book.experience();
        match (experience.reference_equity, experience.peak_equity) {
            (None, None) => {}
            (Some(reference), Some(peak))
                if reference.cents() > 0 && peak >= reference && peak.cents() > 0 => {}
            _ => {
                return Err(SessionError::InvalidSave(format!(
                    "institution account {} has invalid equity experience references",
                    id.0
                )));
            }
        }
        experience.feedback.validate().map_err(|error| {
            SessionError::InvalidSave(format!(
                "institution account {} has invalid experience feedback: {error}",
                id.0
            ))
        })?;
        let moment_is_future = |moment: crate::experience::ExperienceMoment| {
            moment.civil_date > save.civil_clock.current_date
                || moment.market_minute > current_market_minute
                || moment.trading_day > saved_day
        };
        let moment_is_before =
            |left: crate::experience::ExperienceMoment,
             right: crate::experience::ExperienceMoment| {
                left.civil_date < right.civil_date
                    || left.market_minute < right.market_minute
                    || left.trading_day < right.trading_day
            };
        if experience
            .feedback
            .latest_moment
            .is_some_and(moment_is_future)
        {
            return Err(SessionError::InvalidSave(format!(
                "institution account {} has future experience clocks",
                id.0
            )));
        }
        for event in &experience.feedback.failure_events {
            if !stock_codes.contains(&event.code)
                || moment_is_future(event.moment)
                || event
                    .order_id
                    .is_none_or(|order_id| order_id == 0 || order_id >= save.next_order_id)
            {
                return Err(SessionError::InvalidSave(format!(
                    "institution account {} has invalid failure experience for stock {}",
                    id.0, event.code.0
                )));
            }
        }
        for (code, stock) in &experience.stocks {
            if !stock_codes.contains(code)
                || stock.last_buy_price.is_some() != stock.last_buy_order_id.is_some()
                || (stock.adverse_move_recorded && stock.last_buy_order_id.is_none())
                || [
                    stock.entry_reference_price,
                    stock.peak_price_since_entry,
                    stock.last_buy_price,
                ]
                .into_iter()
                .flatten()
                .any(|price| price.cents() <= 0)
                || stock.last_trade_market_minute > current_market_minute
                || stock.last_observed_market_minute > current_market_minute
                || stock.last_observed_market_minute < stock.last_trade_market_minute
                || (save.snapshot.accounts[id].positions.contains_key(code)
                    && experience
                        .feedback
                        .stocks
                        .get(code)
                        .is_some_and(|epoch| epoch.institutional_fees_paid.is_none()))
                || [stock.last_buy_order_id, stock.last_sell_order_id]
                    .into_iter()
                    .flatten()
                    .any(|order_id| order_id == 0 || order_id >= save.next_order_id)
            {
                return Err(SessionError::InvalidSave(format!(
                    "institution account {} has invalid trade experience for stock {}",
                    id.0, code.0
                )));
            }
        }
        for (code, epoch) in &experience.feedback.stocks {
            let observation_invalid = epoch.last_own_observation.is_some_and(|observation| {
                observation.price.cents() <= 0
                    || moment_is_before(observation.moment, epoch.entry_moment)
                    || experience
                        .feedback
                        .latest_moment
                        .is_some_and(|latest| moment_is_before(latest, observation.moment))
                    || moment_is_future(observation.moment)
            });
            if !stock_codes.contains(code)
                || epoch
                    .institutional_fees_paid
                    .is_some_and(|fees| fees.cents() < 0)
                || !experience.stocks.contains_key(code)
                || experience.feedback.latest_moment.is_none()
                || moment_is_future(epoch.entry_moment)
                || experience
                    .feedback
                    .latest_moment
                    .is_some_and(|latest| moment_is_before(latest, epoch.entry_moment))
                || observation_invalid
            {
                return Err(SessionError::InvalidSave(format!(
                    "institution account {} has future holding experience for stock {}",
                    id.0, code.0
                )));
            }
        }
        for exit in &experience.feedback.exit_records {
            if !stock_codes.contains(&exit.code)
                || moment_is_future(exit.moment)
                || exit
                    .order_id
                    .is_some_and(|order_id| order_id == 0 || order_id >= save.next_order_id)
            {
                return Err(SessionError::InvalidSave(format!(
                    "institution account {} has invalid exit experience for stock {}",
                    id.0, exit.code.0
                )));
            }
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
            if entry.anchor_trading_day > context.stock_day(code)? {
                return Err(SessionError::InvalidSave(format!("account {id:?} belief entry {code:?} has a future stock trading-day anchor")));
            }
            if entry.confidence_bp > 10_000 {
                return Err(SessionError::InvalidSave(format!(
                    "account {id:?} belief entry {code:?} confidence {} exceeds 10000 bp",
                    entry.confidence_bp
                )));
            }
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

/// 计划契约校验：计划引用域、链接母单互洽、待应用事实队列。
fn validate_plan_contract(context: &SaveValidationContext) -> Result<(), SessionError> {
    let save = context.save;
    let stock_codes = &context.stock_codes;
    let npc_count = context.npc_count;
    for plan_id in save.plans.plan_ids() {
        let plan = save.plans.plan(plan_id).expect("plan_ids always resolve");
        plan.validate_horizon().map_err(|error| {
            SessionError::InvalidSave(format!("plan {plan_id:?} has an invalid horizon: {error}"))
        })?;
        if plan.account().0 > npc_count {
            return Err(SessionError::InvalidSave(format!(
                "plan {plan_id:?} belongs to unknown account {:?}",
                plan.account()
            )));
        }
        if !stock_codes.contains(plan.code()) {
            return Err(SessionError::InvalidSave(format!(
                "plan {plan_id:?} targets unknown stock {:?}",
                plan.code()
            )));
        }
        if plan.created_trading_day() > context.stock_day(plan.code())?
            || plan.last_event_trading_day() > context.stock_day(plan.code())? {
            return Err(SessionError::InvalidSave(format!("plan {plan_id:?} is stamped after its stock trading day")));
        }
        if let crate::plans::PlanTarget::ShareCount(target) = plan.target() {
            if target == 0 || plan.filled_qty() > target {
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
            if linked.is_terminal() || linked.account() != *account || linked.code() != code {
                return Err(SessionError::InvalidSave(format!(
                    "parent order for account {account:?} {code:?} links to an incompatible plan {plan_id:?}"
                )));
            }
        }
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
                if trading_day > context.stock_day(plan.code())? {
                    return Err(SessionError::InvalidSave(
                        "pending plan event is stamped after the saved trading day".to_string(),
                    ));
                }
            }
            PendingPlanEvent::DayEnded { trading_day, .. } => {
                if trading_day > context.stock_day(plan.code())? {
                    return Err(SessionError::InvalidSave(
                        "pending plan day-end is stamped after the saved trading day".to_string(),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_candle(
    code: &StockCode,
    candle: &DailyCandle,
    real_day: bool,
) -> Result<(), SessionError> {
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
    if real_day {
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
    for stock in &save.setup.stocks {
        if matches!(session.stock_day_status(&stock.code)?, crate::DayStatus::Closed(_))
            && (save.resting_orders.get(&stock.code).is_some_and(|orders| !orders.is_empty())
                || save.auction_orders.get(&stock.code).is_some_and(|orders| !orders.is_empty())) {
            return Err(SessionError::InvalidSave(format!("休市证券 {} 不能包含活动委托", stock.code.0)));
        }
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
    // A completed order keeps its identity permanently. It cannot also be an
    // active order on this or another stock after a save is restored.
    let mut order_ids: BTreeSet<u64> = save
        .filled_orders
        .values()
        .flat_map(|orders| orders.iter().map(|order| order.id.0))
        .collect();
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
        let market = session.state.markets.get(code).ok_or_else(|| {
            SessionError::InvalidSave(format!(
                "save auction order references unknown stock {code:?}"
            ))
        })?;
        let stock = session
            .state
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
            if !session.state.accounts.contains_key(&order.owner) {
                return Err(SessionError::InvalidSave(format!(
                    "save auction order references unknown account {:?}",
                    order.owner
                )));
            }
            if order.qty == 0
                || order.qty > stock.category.max_order_qty(false)
                || (order.side == Side::Buy
                    && !order
                        .qty
                        .is_multiple_of(session.state.setup.config.lot_size))
                || order.limit < down
                || order.limit > up
                || order.limit.cents() % tick.cents() != 0
            {
                return Err(SessionError::InvalidSave(format!(
                    "invalid saved auction order for {code:?}: {order:?}"
                )));
            }
            if !order_ids.insert(order.order_id) {
                return Err(SessionError::InvalidSave(format!(
                    "duplicate saved order id {}",
                    order.order_id
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
            max_order_id = max_order_id.max(order.order_id);
            reservations.record(
                &session.state.setup.config,
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
        let market = session.state.markets.get(code).ok_or_else(|| {
            SessionError::InvalidSave(format!(
                "save resting order references unknown stock {code:?}"
            ))
        })?;
        let stock = session
            .state
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
            if !session.state.accounts.contains_key(&order.owner)
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
                &session.state.setup.config,
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
    for (owner, reserved) in &cash {
        let available = session
            .state
            .accounts
            .get(owner)
            .expect("saved order owner was validated")
            .cash();
        if *reserved > i128::from(available.cents()) {
            return Err(SessionError::InvalidSave(format!(
                "saved orders over-reserve cash for {owner:?}"
            )));
        }
    }
    for ((owner, code), reserved) in &sells {
        let sellable = session
            .state
            .accounts
            .get(owner)
            .expect("saved order owner was validated")
            .sellable_qty(code);
        if *reserved > u64::from(sellable) {
            return Err(SessionError::InvalidSave(format!(
                "saved sells over-reserve shares for {owner:?} {code:?}"
            )));
        }
    }
    for lifecycle in &save.npc_order_lifecycles {
        let account = session
            .state
            .accounts
            .get(&lifecycle.account)
            .expect("lifecycle account was validated against the NPC range");
        if account.kind() == AccountKind::Player {
            return Err(SessionError::InvalidSave(format!(
                "NPC quote lifecycle account {} is a player",
                lifecycle.account.0
            )));
        }
        let live_orders: Vec<_> = session
            .state
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
                        order.order_id == active_id.0
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
                        order.order_id == active_id.0
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
            if plan
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
                .state
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
        let lot_size = u64::from(session.state.setup.config.lot_size);
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
                .state
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
        if original_qty > reconstructed_sellable {
            return Err(SessionError::InvalidSave(format!(
                "saved sells over-reserve shares for {owner:?} {code:?}"
            )));
        }
        let is_board_lot = original_qty.is_multiple_of(lot_size);
        let odd_lot_is_valid = if is_board_lot {
            true
        } else {
            let key = (owner, code.clone());
            reconstructed_sellable % lot_size != 0
                && original_qty % lot_size == reconstructed_sellable % lot_size
                && self.validated_odd_lot_sells.insert(key)
        };
        if !odd_lot_is_valid {
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
                let reserved = self.sells.entry((owner, code.clone())).or_default();
                *reserved = reserved.checked_add(u64::from(order.qty)).ok_or_else(|| {
                    SessionError::InvalidSave("saved sell reservations overflow".to_string())
                })?;
            }
        }
        Ok(())
    }
}
