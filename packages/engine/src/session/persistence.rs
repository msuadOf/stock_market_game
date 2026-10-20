//! 存档结构、市场、账户、订单与日 K 不变量校验。

use super::*;
use crate::error::ValidationFault;

mod saved_runtime;
#[cfg(test)]
mod saved_runtime_tests;

#[cfg(test)]
#[test]
fn direct_save_slot_rejects_missing_consolidated_parent_income() {
    let session = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let mut save = session.save().unwrap();
    assert!(GameSession::restore(&save).is_ok());
    save.closing_registry =
        crate::accounting::closing::ClosingEngine::missing_parent_income_test_fixture();
    // fault2085：closing registry 缺少合并母公司净利（结构断言替代原文本 contains）。
    assert!(
        matches!(GameSession::restore(&save), Err(SessionError::InvalidSave(fault))
        if fault.code == 2085)
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
    // fault2128：非机构账户携带机构账户风险暂停（结构断言替代原文本 contains）。
    assert!(
        matches!(GameSession::restore(&edited), Err(SessionError::InvalidSave(fault))
        if fault.code == 2128)
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
    let code = StockId(0);
    let company = save.company_operations.companies.keys().next().unwrap();
    for confidence in [0, 10_000, 10_001, 65_535] {
        let mut encoded = serde_json::to_value(&save).unwrap();
        encoded["belief_books"][account.0.to_string()]["entries"][code.0.to_string()] = serde_json::json!({
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
                // fault2137：信念条目置信度超 10000bp（结构断言替代原文本 contains）。
                matches!(restored, Err(SessionError::InvalidSave(fault)) if fault.code == 2137),
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
    let code = session.stock_id("600888").unwrap();
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
    // fault2162：已保存委托 id 跨集合重复（结构断言替代原文本 contains）。
    assert!(
        matches!(validate_saved_order_state(&session, &save), Err(SessionError::InvalidSave(fault)) if fault.code == 2162)
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
            session.state.stocks.iter().map(|(id, _meta)| id).collect(),
            1,
            current_market_minute,
        );
        assert_eq!(context.current_market_minute, market_minute);
        assert_eq!(context.stock_codes.len(), save.setup.stocks.len());
        assert_eq!(
            context.issuer_ids.len(),
            save.company_operations.companies.len()
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
    // fault2003：快照市场集合与 setup 不一致（首错先于集合校验）。
    assert!(
        matches!(validate_save_slot(&save), Err(SessionError::InvalidSave(fault)) if fault.code == 2003)
    );
    save.snapshot.tick = (u64::from(u32::MAX) + 1) * save.setup.ticks_per_day;
    // fault2000：tick 超出支持的最大交易日范围；ctx[0]=tick（结构比较）。
    assert!(
        matches!(validate_save_slot(&save), Err(SessionError::InvalidSave(fault)) if fault.code == 2000
            && fault.context == vec![save.snapshot.tick])
    );
}
pub(super) use saved_runtime::{capture_runtime_state, restore_runtime_state};
pub use saved_runtime::{
    SavedEnvelopeKey, SavedFeeComponents, SavedJournalRank, SavedLiveEnvelope,
    SavedReceiptLocalKey, SavedReceiptSource, SavedReceiptTransition, SavedRetailReceiptIdentity,
    SavedRuntimeState, SIMULATION_POLICY_ID,
};

/// 单次 SaveSlot 校验的只读事实：派生值在原门禁位置完成后才组合。
/// 不绑定恢复后的 GameSession，也不推断或修补可编辑存档事实。
struct SaveValidationContext<'a> {
    save: &'a SaveSlot,
    config: &'a GameConfig,
    saved_day: u64,
    day_tick: u64,
    phase: TradingPhase,
    stock_codes: BTreeSet<StockId>,
    issuer_ids: BTreeSet<&'a crate::company::CompanyId>,
    npc_count: u64,
    current_market_minute: u64,
}

impl<'a> SaveValidationContext<'a> {
    fn new(
        save: &'a SaveSlot,
        saved_day: u64,
        day_tick: u64,
        phase: TradingPhase,
        stock_codes: BTreeSet<StockId>,
        npc_count: u64,
        current_market_minute: u64,
    ) -> Self {
        Self {
            save,
            config: &save.setup.config,
            saved_day,
            day_tick,
            phase,
            stock_codes,
            issuer_ids: save.company_operations.companies.keys().collect(),
            npc_count,
            current_market_minute,
        }
    }

    /// 仅在 schema/setup 守卫通过后调用，维持交易日范围错误先于集合校验。
    fn derive_trading_clock(
        save: &SaveSlot,
    ) -> Result<(u64, u64, u64, TradingPhase), SessionError> {
        let saved_day = save.snapshot.tick / save.setup.ticks_per_day;
        if saved_day > u64::from(u32::MAX) {
            // fault2000：tick 超出支持的最大交易日范围；ctx[0]=tick。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2000,
                vec![save.snapshot.tick],
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
        // fault2001：simulation_policy_id 与引擎政策不符；ctx[0] 待 C 批数值化（原文本 "{:?}" 策略 id）。
        return Err(SessionError::InvalidSave(ValidationFault::new(
            2001,
            vec![0],
        )));
    }
    // fault2002：setup schema 校验失败透传包装；ctx 无（内层 InvalidSetup 码被压平，待 C 批登记）。
    save.setup
        .validate()
        .map_err(|_| SessionError::InvalidSave(ValidationFault::bare(2002)))?;

    let (saved_day, day_tick, auction_entry_ticks, saved_phase) =
        SaveValidationContext::derive_trading_clock(save)?;

    // fault2002：同上——stock_catalog 构造失败的 schema 守卫同码复用。
    let save_catalog = save
        .setup
        .stock_catalog()
        .map_err(|_| SessionError::InvalidSave(ValidationFault::bare(2002)))?;
    let expected_markets: BTreeSet<StockId> = save
        .setup
        .stocks
        .iter()
        .map(|stock| {
            save_catalog
                .id(&stock.code.0)
                .expect("目录由同一批规格构造，必须命中")
        })
        .collect();
    let actual_markets: BTreeSet<StockId> = save.snapshot.markets.keys().cloned().collect();
    if actual_markets != expected_markets {
        // fault2003：快照市场集合与 setup 不完全一致。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2003)));
    }
    let sequence_markets: BTreeSet<StockId> = save.book_next_sequences.keys().cloned().collect();
    if sequence_markets != expected_markets {
        // fault2004：簿序号市场集合与 setup 不完全一致。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2004)));
    }
    let resting_markets: BTreeSet<StockId> = save.resting_orders.keys().cloned().collect();
    if !resting_markets.is_empty() && resting_markets != expected_markets {
        // fault2005：挂单市场集合与 setup 不完全一致。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2005)));
    }
    let filled_markets: BTreeSet<StockId> = save.filled_orders.keys().cloned().collect();
    if filled_markets != expected_markets {
        // fault2006：已成交单市场集合与 setup 不完全一致。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2006)));
    }
    let mut filled_ids = BTreeSet::new();
    for (code, orders) in &save.filled_orders {
        for order in orders {
            if order.id.0 >= save.next_order_id
                || !save.snapshot.accounts.contains_key(&order.owner)
                || !filled_ids.insert(order.id)
            {
                // fault2007：已成交单身份非法（id 越界/账户缺失/重复）；ctx[0]=订单 id，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2007,
                    vec![order.id.0, u64::from(code.0)],
                )));
            }
        }
    }
    let history_markets: BTreeSet<StockId> = save.price_history.keys().cloned().collect();
    if history_markets != expected_markets {
        // fault2008：价格历史市场集合与 setup 不完全一致。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2008)));
    }
    for (code, prices) in &save.price_history {
        let continuous_ticks_per_day = save
            .setup
            .ticks_per_day
            .saturating_sub(save.setup.auction_ticks)
            .saturating_sub(save.setup.closing_auction_ticks);
        let completed_continuous_ticks = saved_day
            .checked_mul(continuous_ticks_per_day)
            .and_then(|ticks| {
                ticks.checked_add(
                    (save.snapshot.tick % save.setup.ticks_per_day)
                        .saturating_sub(save.setup.auction_ticks)
                        .min(continuous_ticks_per_day),
                )
            })
            .ok_or_else(|| {
                // fault2009：价格历史长度计算溢出。
                SessionError::InvalidSave(ValidationFault::bare(2009))
            })?;
        let expected_len = usize::try_from(completed_continuous_ticks)
            .unwrap_or(usize::MAX)
            .min(save.setup.history_len);
        if prices.len() != expected_len || prices.iter().any(|price| price.cents() <= 0) {
            // fault2010：价格历史长度或价格非法；ctx[0]=StockId，ctx[1]=实际长度，ctx[2]=期望长度。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2010,
                vec![u64::from(code.0), prices.len() as u64, expected_len as u64],
            )));
        }
    }

    let minute_markets: BTreeSet<StockId> = save.market_minute_closes.keys().cloned().collect();
    if minute_markets != expected_markets {
        // fault2011：市场分钟收盘市场集合与 setup 不完全一致。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2011)));
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
            // fault2012：市场分钟日偏移计算溢出。
            SessionError::InvalidSave(ValidationFault::bare(2012))
        })?;
    let completed_minutes = u64::from(
        crate::completed_market_minute_count(completed_continuous_ticks, continuous_ticks_per_day)
            // fault2013：分钟计数子错误压平；ctx[0] 待 C 批数值化（原文本 "{error}"）。
            .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2013, vec![0])))?,
    );
    let expected_minute_keys = (0..completed_minutes)
        .map(|minute_in_day| {
            day_start.checked_add(minute_in_day).ok_or_else(|| {
                // fault2014：市场分钟键计算溢出。
                SessionError::InvalidSave(ValidationFault::bare(2014))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (code, minutes) in &save.market_minute_closes {
        let actual_keys: Vec<u64> = minutes
            .iter()
            .map(|sample| sample.absolute_trading_minute)
            .collect();
        if actual_keys.windows(2).any(|pair| pair[0] >= pair[1]) {
            // fault2015：市场分钟历史键非严格递增；ctx[0]=StockId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2015,
                vec![u64::from(code.0)],
            )));
        }
        if minutes.iter().any(|sample| sample.close.cents() <= 0) {
            // fault2016：市场分钟历史含非正收盘价；ctx[0]=StockId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2016,
                vec![u64::from(code.0)],
            )));
        }
        if actual_keys != expected_minute_keys {
            if actual_keys
                .iter()
                .any(|minute| !expected_minute_keys.contains(minute))
            {
                // fault2017：市场分钟历史含缺失或未来分钟；ctx[0]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2017,
                    vec![u64::from(code.0)],
                )));
            }
            // fault2018：市场分钟历史未覆盖全部已完成分钟；ctx[0]=StockId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2018,
                vec![u64::from(code.0)],
            )));
        }
    }

    let npc_count = u64::from(save.setup.npcs.retail_count)
        + u64::from(save.setup.npcs.inst_count)
        + u64::from(save.setup.npcs.hot_count);
    // B3 远程登录：账户集合校验升级为成员关系语义（fault2020 域）——
    // 快照账户必须恰好为 NPC 账户（1..=npc_count）与全部市场成员账户，
    // 账户 0 必须是经济成员，成员账户不得占用 NPC 区间（单人 local-owner
    // 局与多成员共享局同校验；取代旧"连续 0..=npc_count"单玩家口径）。
    save.market_memberships
        .validate(save)
        .map_err(|_| SessionError::InvalidSave(ValidationFault::bare(2020)))?;

    let expected_attention_accounts: BTreeSet<AccountId> = (1..=npc_count).map(AccountId).collect();
    let actual_attention_accounts: BTreeSet<AccountId> =
        save.npc_attention.keys().copied().collect();
    if actual_attention_accounts != expected_attention_accounts {
        // fault2021：NPC 关注账户集合与 setup 不完全一致。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2021)));
    }
    for (id, state) in &save.npc_attention {
        // fault2022：NPC 信息关注节奏无效；ctx[0]=AccountId，ctx[1] 待 C 批数值化（原文本 "{reason}"）。
        state
            .information_cadence
            .validate()
            .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2022, vec![id.0, 0])))?;
        if !(state.base_probability.is_finite()
            && 0.0 < state.base_probability
            && state.base_probability <= 1.0)
        {
            // fault2023：NPC 关注概率非有限或不在 (0,1]；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2023,
                vec![id.0],
            )));
        }
        if state.next_attention_candidate_tick < save.snapshot.tick {
            // fault2024：NPC 下次关注候选 tick 早于快照 tick；ctx[0]=AccountId，ctx[1]=候选 tick，ctx[2]=快照 tick。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2024,
                vec![
                    id.0,
                    state.next_attention_candidate_tick,
                    save.snapshot.tick,
                ],
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
        // fault2025：散户经历账户集合与 setup 不完全一致。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2025)));
    }
    let current_market_minute = day_start.checked_add(completed_minutes).ok_or_else(|| {
        // fault2026：经历市场分钟计算溢出。
        SessionError::InvalidSave(ValidationFault::bare(2026))
    })?;
    let context = SaveValidationContext::new(
        save,
        saved_day,
        day_tick,
        saved_phase,
        expected_markets,
        npc_count,
        current_market_minute,
    );
    let expected_markets = &context.stock_codes;
    let day_end_market_minute = day_start
        .checked_add(u64::from(crate::GAME_INTRADAY_MINUTES_PER_DAY))
        .ok_or_else(|| {
            // fault2027：NPC 报价生命周期日范围计算溢出。
            SessionError::InvalidSave(ValidationFault::bare(2027))
        })?;
    let mut lifecycle_keys = BTreeSet::new();
    for lifecycle in &save.npc_order_lifecycles {
        if lifecycle.account.0 == 0 || lifecycle.account.0 > context.npc_count {
            // fault2028：NPC 报价生命周期账户不在 NPC 区间；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2028,
                vec![lifecycle.account.0],
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
            // fault2029：NPC 报价生命周期字段组合非法；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2029,
                vec![lifecycle.account.0],
            )));
        }
        if !lifecycle_keys.insert((
            lifecycle.account,
            lifecycle.code.clone(),
            lifecycle.order_id,
        )) {
            // fault2030：NPC 报价生命周期重复委托；ctx[0]=订单 id。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2030,
                vec![lifecycle.order_id.0],
            )));
        }
    }
    let first_institution = u64::from(save.setup.npcs.retail_count) + 1;
    let last_institution = u64::from(save.setup.npcs.retail_count)
        .checked_add(u64::from(save.setup.npcs.inst_count))
        .ok_or_else(|| {
            // fault2031：机构账户区间计算溢出。
            SessionError::InvalidSave(ValidationFault::bare(2031))
        })?;
    for (id, experience) in &save.retail_experience {
        match (experience.reference_equity, experience.peak_equity) {
            (None, None) => {}
            (Some(reference), Some(peak))
                if reference.cents() > 0 && peak.cents() > 0 && peak >= reference => {}
            _ => {
                // fault2037：散户账户权益经历基准/峰值引用非法；ctx[0]=AccountId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2037,
                    vec![id.0],
                )));
            }
        }
        let held = &save.snapshot.accounts[id].positions;
        if held
            .keys()
            .any(|code| !experience.stocks.contains_key(code))
        {
            // fault2038：持仓股票缺少经历条目；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2038,
                vec![id.0],
            )));
        }
        let unheld_count = experience
            .stocks
            .keys()
            .filter(|code| !held.contains_key(*code))
            .count();
        if unheld_count > crate::MAX_UNHELD_WATCHLIST_STOCKS {
            // fault2039：未持仓观察股票数超上限；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2039,
                vec![id.0],
            )));
        }
        for (code, stock) in &experience.stocks {
            if !expected_markets.contains(code) {
                // fault2040：经历含未知股票；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2040,
                    vec![id.0, u64::from(code.0)],
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
                // fault2041：经历价格非正；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2041,
                    vec![id.0, u64::from(code.0)],
                )));
            }
            if stock.last_trade_market_minute > context.current_market_minute
                || stock.last_observed_market_minute > context.current_market_minute
            {
                // fault2042：经历读取未来市场分钟；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2042,
                    vec![id.0, u64::from(code.0)],
                )));
            }
            if stock.last_observed_market_minute < stock.last_trade_market_minute {
                // fault2043：观察时刻早于最近成交；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2043,
                    vec![id.0, u64::from(code.0)],
                )));
            }
            for (_label, order_id) in [
                ("last buy", stock.last_buy_order_id),
                ("last sell", stock.last_sell_order_id),
            ] {
                if order_id.is_some_and(|order_id| order_id == 0 || order_id >= save.next_order_id)
                {
                    // fault2044：最近买/卖委托 id 非法；ctx[0]=AccountId，ctx[1]=StockId，
                    // ctx[2] 待 C 批数值化（原文本 "{label}"：last buy/last sell）。
                    return Err(SessionError::InvalidSave(ValidationFault::new(
                        2044,
                        vec![id.0, u64::from(code.0), 0],
                    )));
                }
            }
            if stock.last_buy_order_id.is_some() && !held.contains_key(code) {
                // fault2045：未持仓股票保留买入委托身份；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2045,
                    vec![id.0, u64::from(code.0)],
                )));
            }
            if stock.adverse_move_recorded
                && (stock.last_buy_price.is_none() || stock.last_buy_order_id.is_none())
            {
                // fault2046：记录逆境但缺买入身份；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2046,
                    vec![id.0, u64::from(code.0)],
                )));
            }
            if held.contains_key(code) {
                if stock.entry_reference_price.is_none()
                    || stock.peak_price_since_entry.is_none()
                    || stock.cooldown_until_market_minute.is_some()
                {
                    // fault2047：持仓活跃经历字段非法；ctx[0]=AccountId，ctx[1]=StockId。
                    return Err(SessionError::InvalidSave(ValidationFault::new(
                        2047,
                        vec![id.0, u64::from(code.0)],
                    )));
                }
                if stock.last_buy_price.is_some() != stock.last_buy_order_id.is_some() {
                    // fault2048：持仓最近买入身份不完整；ctx[0]=AccountId，ctx[1]=StockId。
                    return Err(SessionError::InvalidSave(ValidationFault::new(
                        2048,
                        vec![id.0, u64::from(code.0)],
                    )));
                }
                if let (Some(entry), Some(peak)) =
                    (stock.entry_reference_price, stock.peak_price_since_entry)
                {
                    if peak < entry {
                        // fault2049：峰值低于入场基准；ctx[0]=AccountId，ctx[1]=StockId。
                        return Err(SessionError::InvalidSave(ValidationFault::new(
                            2049,
                            vec![id.0, u64::from(code.0)],
                        )));
                    }
                }
                if let (Some(last_buy), Some(peak)) =
                    (stock.last_buy_price, stock.peak_price_since_entry)
                {
                    if peak < last_buy {
                        // fault2050：峰值低于最近买入价；ctx[0]=AccountId，ctx[1]=StockId。
                        return Err(SessionError::InvalidSave(ValidationFault::new(
                            2050,
                            vec![id.0, u64::from(code.0)],
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
                // fault2051：未持仓观察态字段非法；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2051,
                    vec![id.0, u64::from(code.0)],
                )));
            }
            if let Some(until) = stock.cooldown_until_market_minute {
                let expected = stock
                    .last_trade_market_minute
                    .checked_add(crate::POST_EXIT_COOLDOWN_MINUTES)
                    .ok_or_else(|| {
                        // fault2052：退出冷却计算溢出；ctx[0]=AccountId，ctx[1]=StockId。
                        SessionError::InvalidSave(ValidationFault::new(
                            2052,
                            vec![id.0, u64::from(code.0)],
                        ))
                    })?;
                if until != expected {
                    // fault2053：退出冷却值与最近成交分钟不一致；ctx[0]=AccountId，ctx[1]=StockId。
                    return Err(SessionError::InvalidSave(ValidationFault::new(
                        2053,
                        vec![id.0, u64::from(code.0)],
                    )));
                }
                if stock.last_sell_order_id.is_none()
                    || stock.last_buy_price.is_some()
                    || stock.last_buy_order_id.is_some()
                    || stock.adverse_move_recorded
                {
                    // fault2054：退出后状态字段不一致；ctx[0]=AccountId，ctx[1]=StockId。
                    return Err(SessionError::InvalidSave(ValidationFault::new(
                        2054,
                        vec![id.0, u64::from(code.0)],
                    )));
                }
            }
        }
        // fault2055：散户反馈不一致；ctx[0]=AccountId，ctx[1] 待 C 批数值化（原文本 "{error}"）。
        experience
            .feedback
            .validate()
            .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2055, vec![id.0, 0])))?;
    }

    for (id, account) in &save.snapshot.accounts {
        if account.cash.cents() < 0 {
            // fault2056：账户现金为负；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2056,
                vec![id.0],
            )));
        }
        for (code, position) in &account.positions {
            if !expected_markets.contains(code) {
                // fault2057：持仓含未知股票；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2057,
                    vec![id.0, u64::from(code.0)],
                )));
            }
            if position.qty == 0
                || position.t1_locked > position.qty
                || position.invested_cents < 0
                || position.recovered_cents < 0
            {
                // fault2058：持仓不变量非法；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2058,
                    vec![id.0, u64::from(code.0)],
                )));
            }
        }
    }

    for (code, market) in &save.snapshot.markets {
        if market.last_price.cents() <= 0 || market.last_close.cents() <= 0 {
            // fault2059：市场权威价格非正；ctx[0]=StockId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2059,
                vec![u64::from(code.0)],
            )));
        }
    }

    for (id, account) in &save.snapshot.accounts {
        for (code, position) in &account.positions {
            if (!save.setup.t1_enabled || context.phase == TradingPhase::CallAuction)
                && position.t1_locked != 0
            {
                // fault2060：当前相位下 T+1 锁定不可达；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2060,
                    vec![id.0, u64::from(code.0)],
                )));
            }
        }
    }

    let daily_candle_markets: BTreeSet<StockId> =
        save.snapshot.daily_candles.keys().cloned().collect();
    if daily_candle_markets != *expected_markets {
        // fault2061：日 K 市场集合与 setup 不完全一致。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2061)));
    }
    // 日历政策属 civil_clock 域（save.civil_clock.policy）：政策/digest 校验失败
    // 必须与会话恢复路径（CivilClock::from_parts）同域透传为
    // SessionError::CivilClock(CivilClockError::Calendar(..))，不得经 From 直落
    // SessionError::Calendar 造成同一篡改在两个校验点产生不同错误域。
    let candle_calendar = crate::calendar::TradingCalendar::from_policy(
        crate::calendar::CalendarPolicy::from_parts(save.civil_clock.policy.clone())
            .map_err(CivilClockError::from)?,
    )
    .map_err(CivilClockError::from)?;
    for (code, candles) in &save.snapshot.daily_candles {
        if !expected_markets.contains(code) {
            // fault2062：日 K 含未知股票；ctx[0]=StockId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2062,
                vec![u64::from(code.0)],
            )));
        }
        let expected_candles = usize::try_from(context.saved_day)
            .ok()
            .and_then(|completed_days| completed_days.checked_add(360))
            .ok_or_else(|| {
                // fault2063：日 K 历史长度计算溢出。
                SessionError::InvalidSave(ValidationFault::bare(2063))
            })?;
        if candles.len() != expected_candles {
            // fault2064：日 K 长度与期望不符；ctx[0]=StockId，ctx[1]=实际长度，ctx[2]=期望长度。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2064,
                vec![
                    u64::from(code.0),
                    candles.len() as u64,
                    expected_candles as u64,
                ],
            )));
        }
        let stock = save
            .setup
            .stocks
            .iter()
            .find(|stock| {
                stock.code.0
                    == save_catalog
                        .code(*code)
                        .expect("已验证的市场代码必须入目录")
            })
            .expect("daily candle market is validated against setup");
        let exchange = session_calendar_exchange(stock.exchange);
        let mut expected_dates =
            candles::preset_candle_dates(&candle_calendar, save.setup.start_date, stock.exchange)?;
        let mut live_date = save.setup.start_date;
        if !candle_calendar.is_trading_day(exchange, live_date)? {
            live_date = candle_calendar.next_trading_day(exchange, live_date)?;
        }
        for completed in 0..context.saved_day {
            if completed > 0 {
                live_date = candle_calendar.next_trading_day(exchange, live_date)?;
            }
            expected_dates.push(live_date);
        }
        let mut previous_time = None;
        for (index, candle) in candles.iter().enumerate() {
            validate_candle(code, candle, index >= 360)?;
            if previous_time.is_some_and(|time| candle.time <= time) {
                // fault2065：日 K 时间非严格递增；ctx[0]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2065,
                    vec![u64::from(code.0)],
                )));
            }
            let expected_time = candles::candle_date_time(expected_dates[index]);
            if candle.time != expected_time {
                // fault2066：日 K 时间与日历期望不符；ctx[0]=下标，ctx[1]=StockId，
                // ctx[2]=实际时间，ctx[3]=期望时间（i64 时间以 unsigned_abs 保幅值）。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2066,
                    vec![
                        index as u64,
                        u64::from(code.0),
                        candle.time.unsigned_abs(),
                        expected_time.unsigned_abs(),
                    ],
                )));
            }
            previous_time = Some(candle.time);
        }
    }

    if save
        .pending_player
        .iter()
        .any(|received| received.owner != AccountId(0))
    {
        // fault2067：pending 玩家意图不属于玩家账户。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2067)));
    }
    let mut seen_account_receipts = BTreeSet::new();
    let mut seen_stock_receipts = BTreeSet::new();
    for received in save.pending_player.iter() {
        let next_account = save
            .ingress_receipt_cursors
            .next_account_ordinal
            .get(&received.owner)
            .ok_or_else(|| {
                // fault2072：pending intent 的账户无回执游标；ctx[0]=AccountId。
                SessionError::InvalidSave(ValidationFault::new(2072, vec![received.owner.0]))
            })?;
        if received.account_ordinal >= *next_account
            || !seen_account_receipts.insert((received.owner, received.account_ordinal))
        {
            // fault2073：pending intent 的账户回执序号非法或重复；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2073,
                vec![received.owner.0],
            )));
        }
        let stock_code = match &received.intent {
            Intent::PlaceLimit { code, .. }
            | Intent::PlaceMarket { code, .. }
            | Intent::Cancel { code, .. } => code,
        };
        let next_stock = save
            .ingress_receipt_cursors
            .next_stock_ordinal
            .get(stock_code)
            .ok_or_else(|| {
                // fault2074：pending intent 的股票无回执游标；ctx[0]=StockId。
                SessionError::InvalidSave(ValidationFault::new(2074, vec![u64::from(stock_code.0)]))
            })?;
        if received.stock_ordinal >= *next_stock
            || !seen_stock_receipts.insert((stock_code.clone(), received.stock_ordinal))
        {
            // fault2075：pending intent 的股票回执序号非法或重复；ctx[0]=StockId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2075,
                vec![u64::from(stock_code.0)],
            )));
        }
    }
    validate_pending_receipt_order(save)?;
    let active_candle_markets: BTreeSet<StockId> =
        save.snapshot.active_daily_candles.keys().cloned().collect();
    let active_candle_set_is_valid =
        if context.day_tick == 0 || context.day_tick < auction_entry_ticks {
            active_candle_markets.is_empty()
        } else {
            active_candle_markets == *expected_markets
        };
    if !active_candle_set_is_valid {
        // fault2076：活跃日 K 市场集合与当前交易 tick 不符。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2076)));
    }
    for (code, candle) in &save.snapshot.active_daily_candles {
        if !expected_markets.contains(code) {
            // fault2077：活跃日 K 含未知股票；ctx[0]=StockId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2077,
                vec![u64::from(code.0)],
            )));
        }
        validate_candle(code, candle, true)?;
        if candle.time != candles::candle_date_time(save.civil_clock.current_date) {
            // fault2078：活跃日 K 与当前自然日不符；ctx[0]=StockId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2078,
                vec![u64::from(code.0)],
            )));
        }
    }

    if save.next_order_id == 0
        || save
            .resting_orders
            .values()
            .flatten()
            .any(|order| order.id.0 >= save.next_order_id)
    {
        // fault2079：next_order_id 未严格大于所有已保存委托 id。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2079)));
    }

    validate_company_domain(save)?;
    validate_disclosure_cursors(save)?;
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
        // 1253：存档解码超字节上限；ctx[0]=实际字节数，ctx[1]=上限。
        return Err(SessionError::ResourceLimit(
            crate::error::ValidationFault::new(
                1253,
                vec![json.len() as u64, limits.max_total_bytes as u64],
            ),
        ));
    }
    // T3 批四（ADR-0042 §9.6 选项甲；三轮收口=解析后结构检测替代文本预检）：
    // `pending_npc` 字段已随跨 tick 载体删除。旧档识别=先解析为
    // serde_json::Value，检查根对象**是否实持该键**——任意位置的 unicode
    // 转义（pending\u005fnpc、\u0070ending_npc 等）解析后统一为原键，全部
    // 落 fault2100（不落通用 2080）；字符串**值**含该子串不触发（值非键）；
    // 键存在（null/非空皆同）即旧格式，先于 SaveSlot 解码与任何深层业务
    // 校验（外层 schema 判定优先）；不重算、不静默丢键。ctx=[0]。
    if serde_json::from_slice::<serde_json::Value>(json)
        .ok()
        .and_then(|root| {
            root.as_object()
                .map(|object| object.contains_key("pending_npc"))
        })
        .unwrap_or(false)
    {
        // fault2100：旧跨 tick pending_npc 档不被 T3 入口切换后版本接受。
        return Err(SessionError::InvalidSave(ValidationFault::new(2100, vec![0])));
    }
    serde_json::from_slice(json)
        // fault2080：存档 JSON 不可解码；ctx[0] 待 C 批数值化（原文本 "{error}" serde 错误）。
        .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2080, vec![0])))
}

/// 公司域权威状态校验：经营编排集合/推进时点、镜像与时钟到期一致性。
fn validate_pending_receipt_order(save: &SaveSlot) -> Result<(), SessionError> {
    #[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
    enum ResourceLane {
        Cash(AccountId),
        Shares(AccountId, StockId),
    }

    // 批四：pending_npc 载体删除——先序图仅覆盖 pending_player（跨 lane 环
    // fault2084 的玩家路径见证保留；NPC 依赖边 fault2081 随载体退役）。
    let mut intents = save.pending_player.clone();
    let mut edges = vec![BTreeSet::new(); intents.len()];
    let mut resource_lanes = BTreeMap::<ResourceLane, Vec<(usize, u64)>>::new();
    let mut stock_lanes = BTreeMap::<StockId, Vec<(usize, u64)>>::new();
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
    let mut incoming = vec![0_usize; intents.len()];
    for successors in &edges {
        for &successor in successors {
            incoming[successor] = incoming[successor].checked_add(1).ok_or_else(|| {
                // fault2082：回执先序边计数溢出。
                SessionError::InvalidSave(ValidationFault::bare(2082))
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
                // fault2083：回执先序计数下溢。
                SessionError::InvalidSave(ValidationFault::bare(2083))
            })?;
            if incoming[successor] == 0 {
                ready.push(successor);
            }
        }
    }
    if visited != intents.len() {
        // fault2084：回执先序图含环。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2084)));
    }
    Ok(())
}

fn validate_company_domain(save: &SaveSlot) -> Result<(), SessionError> {
    // fault2085：closing registry 缺少合并母公司净利；ctx[0] 待 C 批数值化（原文本 "{error}" closing 子错误）。
    save.closing_registry
        .validate_consolidated_parent_income()
        .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2085, vec![0])))?;
    for date in save.company_operations.payment_failures.keys() {
        if *date < save.civil_clock.policy.init_only_min_start
            || *date > save.civil_clock.policy.runtime_max_end
        {
            // fault2086：付款失败日期越出日历政策窗；ctx[0]=yyyymmdd。
            let (year, month, day) = date.ymd();
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2086,
                vec![crate::error::ymd_context(year, month, day)],
            )));
        }
    }
    // fault2087：付款历史不一致；ctx[0] 待 C 批数值化（原文本 "{error}"）。
    save.company_operations
        .validate_payment_history()
        .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2087, vec![0])))?;
    if save.groups != save.setup.groups {
        // fault2088：存档分组与 setup 不一致。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2088)));
    }
    // fault2089：存档公司分组非法；ctx[0] 待 C 批数值化（原文本 "{error}"）。
    super::company_groups::validate_groups(&save.groups, &save.company_operations)
        .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2089, vec![0])))?;
    // 公司集合精确：每家经营公司唯一映射一只 setup 股票且股本一致。
    // 2090-2096 的 ctx[0] 统一落数值 CompanyId（C 批数值化收口：定位到具体公司，
    // 不再以 0 占位）；ctx[1]（错误种别/字符串 id）仍待 C 批登记。
    let mut mapped: BTreeSet<&StockCode> = BTreeSet::new();
    for (id, company) in &save.company_operations.companies {
        if company
            .economy()
            .active()
            .iter()
            .any(|shock| !shock.kind.applies_to(company.spec().kind))
        {
            // fault2090：公司存有行业不适用的活跃冲击；ctx[0]=CompanyId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2090,
                vec![u64::from(id.0)],
            )));
        }
        if let Some(real_estate) = company.books().as_real_estate() {
            for (_contract, loan) in real_estate.loans() {
                if loan.maturity_date() < save.civil_clock.policy.init_only_min_start
                    || loan.maturity_date() > save.civil_clock.policy.runtime_max_end
                {
                    // fault2091：项目贷款到期日越出日历政策窗；ctx[0]=CompanyId，
                    // ctx[1] 待 C 批数值化（原文本 "{contract:?}" 合同 id）。
                    return Err(SessionError::InvalidSave(ValidationFault::new(
                        2091,
                        vec![u64::from(id.0), 0],
                    )));
                }
            }
        }
        if let Some(insurance) = company.books().as_insurance() {
            // fault2092：保险账状态非法；ctx[0]=CompanyId，ctx[1] 待 C 批（原文本 "{error}"）。
            insurance.validate_restore().map_err(|_| {
                SessionError::InvalidSave(ValidationFault::new(2092, vec![u64::from(id.0), 0]))
            })?;
        }
        if let Some(industrial) = company.books().as_industrial() {
            // fault2093：工业贸易对手方事件非法；ctx[0]=CompanyId，ctx[1] 待 C 批（"{error}"）。
            industrial.validate_trade_counterparty_events().map_err(|_| {
                SessionError::InvalidSave(ValidationFault::new(2093, vec![u64::from(id.0), 0]))
            })?;
            // fault2094：工业库存来源事件非法；ctx[0]=CompanyId，ctx[1] 待 C 批（"{error}"）。
            industrial.validate_inventory_source_events().map_err(|_| {
                SessionError::InvalidSave(ValidationFault::new(2094, vec![u64::from(id.0), 0]))
            })?;
            // fault2095：工业信用状态非法；ctx[0]=CompanyId，ctx[1] 待 C 批（"{error}"）。
            industrial.validate_credit_state().map_err(|_| {
                SessionError::InvalidSave(ValidationFault::new(2095, vec![u64::from(id.0), 0]))
            })?;
        }
        if let Some(bank) = company.books().as_bank() {
            // fault2096：银行 ECL 政策非法；ctx[0]=CompanyId，ctx[1] 待 C 批（"{error}"）。
            bank.ecl_policy().validate().map_err(|_| {
                SessionError::InvalidSave(ValidationFault::new(2096, vec![u64::from(id.0), 0]))
            })?;
        }
        let Some(listed) = company.spec().listed_stock.as_ref() else {
            continue;
        };
        let Some(stock) = save.setup.stocks.iter().find(|s| &s.code == listed) else {
            // fault2097：公司映射到未知股票；ctx[0] 待 C 批（CompanyId），ctx[1] 待 C 批（原文本 "{listed:?}" StockCode）。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2097,
                vec![0, 0],
            )));
        };
        if company.spec().issued_shares != stock.total_shares || !mapped.insert(listed) {
            // fault2098：公司股本或映射与 setup 股票冲突；ctx[0] 待 C 批（CompanyId），ctx[1] 待 C 批（"{listed:?}"）。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2098,
                vec![0, 0],
            )));
        }
    }
    if mapped.len() != save.setup.stocks.len() {
        // fault2099：公司集合未精确覆盖 setup 股票。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2099)));
    }
    // 经营推进时点与时钟一致：存档时点 next_expected 恒等于当前自然日。
    if save.company_operations.next_expected_date() != save.civil_clock.current_date {
        // fault2100：经营推进时点与自然日不一致；ctx[0]=期望日 yyyymmdd，ctx[1]=当前日 yyyymmdd。
        let expected_date = save.company_operations.next_expected_date();
        let (expect_y, expect_m, expect_d) = expected_date.ymd();
        let (clock_y, clock_m, clock_d) = save.civil_clock.current_date.ymd();
        return Err(SessionError::InvalidSave(ValidationFault::new(
            2100,
            vec![
                crate::error::ymd_context(expect_y, expect_m, expect_d),
                crate::error::ymd_context(clock_y, clock_m, clock_d),
            ],
        )));
    }
    // 公司 due 单一 authority：待办只存在于调度器；不允许任何待办早于当前
    // 自然日，或超出冻结日历适用范围（均失去恰好一次派发机会）。
    for due in save.company_operations.scheduler().pending() {
        if due.due_date < save.civil_clock.policy.init_only_min_start
            || due.due_date > save.civil_clock.policy.runtime_max_end
        {
            // fault2234：调度待办日期越出冻结日历适用范围；ctx[0]=due id，ctx[1]=yyyymmdd。
            let (year, month, day) = due.due_date.ymd();
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2234,
                vec![due.id.as_u64(), crate::error::ymd_context(year, month, day)],
            )));
        }
        if due.due_date < save.civil_clock.current_date {
            // fault2102：调度待办早于当前自然日；ctx[0]=due id，ctx[1]=due_date yyyymmdd，
            // ctx[2] 待 C 批数值化（原文本 "{due:?}" 其余字段 key/action）。
            let (year, month, day) = due.due_date.ymd();
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2102,
                vec![
                    due.id.as_u64(),
                    crate::error::ymd_context(year, month, day),
                    0,
                ],
            )));
        }
    }
    // 公开信息库：全量重验（JSON 路径已验，这里覆盖直接内存构造的 SaveSlot）。
    // fault2104：公开信息库不一致；ctx[0] 待 C 批数值化（原文本 "{error}"）。
    crate::information::PublicLibrary::from_parts(save.public_library.save())
        .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2104, vec![0])))?;
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
            // fault2105：settled 日 18:00 相位构造失败；ctx[0] 待 C 批数值化（原文本 "{error}" CalendarError）。
            let phase = crate::calendar::CivilInstant::from_hms(settled, 18, 0, 0)
                .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2105, vec![0])))?;
            if save.disclosures.published_through() != Some(phase) {
                // fault2106：披露游标未落在 settled 日 18:00 相位；
                // ctx[0] 待 C 批数值化（原文本 "{:?}" published_through CivilInstant），ctx[1]=settled yyyymmdd。
                let (year, month, day) = settled.ymd();
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2106,
                    vec![0, crate::error::ymd_context(year, month, day)],
                )));
            }
        }
        _ => {
            // fault2107：披露游标与已日结日期不匹配。
            return Err(SessionError::InvalidSave(ValidationFault::bare(2107)));
        }
    }
    // 无未来公布：库内最晚发表时点不得晚于已派发游标。
    if let (Some(latest), Some(through)) = (
        save.public_library.latest_published_instant(),
        save.disclosures.published_through(),
    ) {
        if latest > through {
            // fault2108：公开库含晚于派发游标的发表。
            return Err(SessionError::InvalidSave(ValidationFault::bare(2108)));
        }
    }
    if save
        .public_library
        .latest_published_instant()
        .is_some_and(|latest| latest.date() > current)
    {
        // fault2109：公开库含晚于当前自然日的发表。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2109)));
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
        // fault2110：信念/信息/关注/价格记忆账户集合不一致。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2110)));
    }
    let npc_count = context.npc_count;
    let current = save.civil_clock.current_date;
    let issuer_ids = &context.issuer_ids;
    let stock_codes = &context.stock_codes;
    let current_market_minute = context.current_market_minute;
    for (id, state) in &save.information_states {
        if state.owner() != *id {
            // fault2111：信息状态 owner 与键不一致；ctx[0]=owner AccountId，ctx[1]=键 AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2111,
                vec![state.owner().0, id.0],
            )));
        }
        if id.0 == 0 || id.0 > npc_count {
            // fault2112：个体状态账户不在 NPC 区间；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2112,
                vec![id.0],
            )));
        }
        // 恢复边界不变量（严格递增/跨公司唯一）对内存构造的 SaveSlot 显式复核。
        // fault2113：信息状态重建失败；ctx[0]=AccountId，ctx[1] 待 C 批数值化（原文本 "{error}"）。
        crate::information::NpcInformationState::from_parts(state.save())
            .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2113, vec![id.0, 0])))?;
        for (company, records) in state.companies() {
            if !issuer_ids.contains(company) {
                // fault2114：账户获取了未知公司的披露；ctx[0]=AccountId，ctx[1] 待 C 批（CompanyId 文本）。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2114,
                    vec![id.0, 0],
                )));
            }
            for record in records {
                let Some(published_at) = save.public_library.publication_instant(record.id) else {
                    // fault2115：引用的披露不在公开库；ctx[0]=AccountId，ctx[1]=PublicationId。
                    return Err(SessionError::InvalidSave(ValidationFault::new(
                        2115,
                        vec![id.0, u64::from(record.id.as_u32())],
                    )));
                };
                if record.observed_at < published_at {
                    // fault2116：披露发表前即被获取；ctx[0]=AccountId，ctx[1]=PublicationId。
                    return Err(SessionError::InvalidSave(ValidationFault::new(
                        2116,
                        vec![id.0, u64::from(record.id.as_u32())],
                    )));
                }
                if record.observed_at.date() > current {
                    // fault2117：持有未来观察的披露；ctx[0]=AccountId，ctx[1]=PublicationId。
                    return Err(SessionError::InvalidSave(ValidationFault::new(
                        2117,
                        vec![id.0, u64::from(record.id.as_u32())],
                    )));
                }
            }
        }
    }
    let account_keys: BTreeSet<AccountId> = save.snapshot.accounts.keys().copied().collect();
    if history_read_keys != account_keys {
        // fault2118：历史读取账本未精确覆盖全部账户。
        return Err(SessionError::InvalidSave(ValidationFault::bare(2118)));
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
            // fault2119：关注表超 held+8 驱逐界；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2119,
                vec![id.0],
            )));
        }
        for code in watchlist.stocks.keys() {
            if !stock_codes.contains(code) {
                // fault2120：关注未知股票；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2120,
                    vec![id.0, u64::from(code.0)],
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
            // fault2121：价格记忆超 held+8 驱逐界；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2121,
                vec![id.0],
            )));
        }
        for (code, entry) in &memory.stocks {
            if !stock_codes.contains(code)
                || entry.first_observed_price.cents() <= 0
                || entry.last_observed_price.cents() <= 0
                || entry.observed_high.cents() <= 0
                || entry.observed_low.cents() <= 0
                || entry.first_observed_minute > entry.last_observed_minute
                || entry.last_observed_minute > entry.last_touched_minute
                || entry.last_touched_minute > current_market_minute
                || entry.observed_low > entry.observed_high
                || entry.first_observed_price < entry.observed_low
                || entry.first_observed_price > entry.observed_high
                || entry.last_observed_price < entry.observed_low
                || entry.last_observed_price > entry.observed_high
            {
                // fault2122：价格记忆不一致；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2122,
                    vec![id.0, u64::from(code.0)],
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
                // fault2123：历史读取事实不一致；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2123,
                    vec![id.0, u64::from(code.0)],
                )));
            }
        }
    }
    let saved_issuers = &context.issuer_ids;
    for (id, book) in &save.belief_books {
        if book.npc() != *id {
            // fault2124：信念账 owner 与键不一致；ctx[0]=owner AccountId，ctx[1]=键 AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2124,
                vec![book.npc().0, id.0],
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
            // fault2125：信念画像与账户类别冲突；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2125,
                vec![id.0],
            )));
        }
        let institution = matches!(book.profile(), StrategyProfile::Institution(_));
        if institution && book.institution_policy().is_none() {
            // fault2126：机构账户缺冻结经历策略；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2126,
                vec![id.0],
            )));
        }
        if !institution && book.institution_policy().is_some() {
            // fault2127：非机构账户携带机构经历策略；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2127,
                vec![id.0],
            )));
        }
        if !institution && book.institution_account_risk_paused() {
            // fault2128：非机构账户携带机构账户风险暂停；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2128,
                vec![id.0],
            )));
        }
        let experience = book.experience();
        match (experience.reference_equity, experience.peak_equity) {
            (None, None) => {}
            (Some(reference), Some(peak))
                if reference.cents() > 0 && peak >= reference && peak.cents() > 0 => {}
            _ => {
                // fault2129：机构账户权益经历基准/峰值引用非法；ctx[0]=AccountId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2129,
                    vec![id.0],
                )));
            }
        }
        // fault2130：机构账户经历反馈非法；ctx[0]=AccountId，ctx[1] 待 C 批数值化（原文本 "{error}"）。
        experience
            .feedback
            .validate()
            .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2130, vec![id.0, 0])))?;
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
            // fault2131：机构账户经历时钟指向未来；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2131,
                vec![id.0],
            )));
        }
        for event in &experience.feedback.failure_events {
            if !stock_codes.contains(&event.code)
                || moment_is_future(event.moment)
                || event
                    .order_id
                    .is_none_or(|order_id| order_id == 0 || order_id >= save.next_order_id)
            {
                // fault2132：机构账户失败经历非法；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2132,
                    vec![id.0, u64::from(event.code.0)],
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
                // fault2133：机构账户交易经历非法；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2133,
                    vec![id.0, u64::from(code.0)],
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
                // fault2134：机构账户持仓经历指向未来；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2134,
                    vec![id.0, u64::from(code.0)],
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
                // fault2135：机构账户退出经历非法；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2135,
                    vec![id.0, u64::from(exit.code.0)],
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
                // fault2136：账户持有未知股票的信念条目；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2136,
                    vec![id.0, u64::from(code.0)],
                )));
            }
            let entry = book.entry(code).expect("entry_stocks keys always resolve");
            if entry.confidence_bp > 10_000 {
                // fault2137：信念条目置信度超 10000bp；ctx[0]=AccountId，ctx[1]=StockId，ctx[2]=confidence_bp。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2137,
                    vec![id.0, u64::from(code.0), u64::from(entry.confidence_bp)],
                )));
            }
            if !saved_issuers.contains(&entry.company) {
                // fault2138：信念条目引用未知公司；ctx[0]=AccountId，ctx[1]=StockId，
                // ctx[2] 待 C 批数值化（原文本 "{:?}" CompanyId）。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2138,
                    vec![id.0, u64::from(code.0), 0],
                )));
            }
            if entry.horizon_trading_days == 0 {
                // fault2139：信念条目持有期为零；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2139,
                    vec![id.0, u64::from(code.0)],
                )));
            }
            for report in &entry.used_report_ids {
                if !acquired.contains(report) {
                    // fault2140：信念条目引用未获取的报告；ctx[0]=AccountId，ctx[1]=StockId，ctx[2]=PublicationId。
                    return Err(SessionError::InvalidSave(ValidationFault::new(
                        2140,
                        vec![id.0, u64::from(code.0), u64::from(report.as_u32())],
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
    let saved_day = context.saved_day;
    let stock_codes = &context.stock_codes;
    let npc_count = context.npc_count;
    for plan_id in save.plans.plan_ids() {
        let plan = save.plans.plan(plan_id).expect("plan_ids always resolve");
        // fault2141：计划持有期非法；ctx[0]=PlanId，ctx[1] 待 C 批数值化（原文本 "{error}"）。
        plan.validate_horizon().map_err(|_| {
            SessionError::InvalidSave(ValidationFault::new(2141, vec![plan_id.0, 0]))
        })?;
        if plan.account().0 > npc_count {
            // fault2142：计划属于未知账户；ctx[0]=PlanId，ctx[1]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2142,
                vec![plan_id.0, plan.account().0],
            )));
        }
        if !stock_codes.contains(plan.code()) {
            // fault2143：计划指向未知股票；ctx[0]=PlanId，ctx[1]=StockId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2143,
                vec![plan_id.0, u64::from(plan.code().0)],
            )));
        }
        if let crate::plans::PlanTarget::ShareCount(target) = plan.target() {
            if target == 0 || plan.filled_qty() > target {
                // fault2144：计划份额目标/进度非法；ctx[0]=PlanId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2144,
                    vec![plan_id.0],
                )));
            }
        }
        if let Some(leg) = plan.execution() {
            if leg.child_qty() == 0 || leg.limit_price().cents() <= 0 {
                // fault2183：执行腿参数非法（子单数量/限价非正）；ctx[0]=PlanId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2183,
                    vec![plan_id.0],
                )));
            }
            if let Some(active_id) = plan.active_child_order_id() {
                if active_id.0 == 0 || active_id.0 >= save.next_order_id {
                    // fault2184：执行腿在途子单 id 越界；ctx[0]=PlanId，ctx[1]=订单 id。
                    return Err(SessionError::InvalidSave(ValidationFault::new(
                        2184,
                        vec![plan_id.0, active_id.0],
                    )));
                }
                // 校验必须用真实订单簿余量：序列化的腿余量在 restore 会被真实
                // 余量覆盖，不能作为敞口依据（旧 fault2173 语义迁移）。
                let live_quantities: Vec<u32> = save
                    .resting_orders
                    .get(plan.code())
                    .into_iter()
                    .flatten()
                    .filter(|order| {
                        order.id == active_id
                            && order.owner == plan.account()
                            && order.side == plan.direction()
                    })
                    .map(|order| order.qty)
                    .collect();
                let [live_remaining] = live_quantities.as_slice() else {
                    // fault1042：计划在途子委托无法唯一重建（竞价/连续簿命中数 ≠ 1）。
                    // ctx[0]=PlanId，ctx[1]=StockId。
                    return Err(SessionError::InvalidSave(ValidationFault::new(
                        1042,
                        vec![plan_id.0, u64::from(plan.code().0)],
                    )));
                };
                let exposure = u64::from(plan.filled_qty())
                    .checked_add(u64::from(*live_remaining))
                    .ok_or_else(|| {
                        // fault2185：执行腿在途风险敞口溢出或超目标；ctx[0]=PlanId，ctx[1]=StockId。
                        SessionError::InvalidSave(ValidationFault::new(
                            2185,
                            vec![plan_id.0, u64::from(plan.code().0)],
                        ))
                    })?;
                if let crate::plans::PlanTarget::ShareCount(target) = plan.target() {
                    if exposure > u64::from(target) {
                        // fault2185：执行腿在途风险敞口溢出或超目标；ctx[0]=PlanId，ctx[1]=StockId。
                        return Err(SessionError::InvalidSave(ValidationFault::new(
                            2185,
                            vec![plan_id.0, u64::from(plan.code().0)],
                        )));
                    }
                }
                if leg.active_child_remaining_qty().is_some_and(|stored| stored > *live_remaining)
                {
                    // fault2185：序列化余量大于真实订单余量（虚增敞口）。
                    return Err(SessionError::InvalidSave(ValidationFault::new(
                        2185,
                        vec![plan_id.0, u64::from(plan.code().0)],
                    )));
                }
            }
        }
    }
    Ok(())
}

fn validate_candle(
    code: &StockId,
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
        // fault2152：OHLC 蜡烛非法；ctx[0]=StockId，ctx[1]=时间（i64 以 unsigned_abs 保幅值）。
        return Err(SessionError::InvalidSave(ValidationFault::new(
            2152,
            vec![u64::from(code.0), candle.time.unsigned_abs()],
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
            // fault2153：蜡烛成交统计不一致；ctx[0]=StockId，ctx[1]=时间（i64 以 unsigned_abs 保幅值）。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2153,
                vec![u64::from(code.0), candle.time.unsigned_abs()],
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_saved_order_state(
    session: &GameSession,
    save: &SaveSlot,
) -> Result<(), SessionError> {
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
                    // fault2156：已保存卖单累计成交量溢出。
                    SessionError::InvalidSave(ValidationFault::bare(2156))
                })?;
        }
    }
    let mut reservations = SavedReservations {
        sell_filled_totals,
        ..SavedReservations::default()
    };

    for (code, orders) in &save.resting_orders {
        let market = session.state.markets.get(code).ok_or_else(|| {
            // fault2163：存档挂单引用未知股票；ctx[0]=StockId。
            SessionError::InvalidSave(ValidationFault::new(2163, vec![u64::from(code.0)]))
        })?;
        let stock = session
            .state
            .setup
            .stocks
            .iter()
            .find(|stock| {
                stock.code.0
                    == session
                        .state
                        .stocks
                        .code(*code)
                        .expect("market code must be cataloged")
            })
            .expect("market code must have a stock spec");
        let tick = stock.tick;
        // fault2158：跌停价计算子错误压平（同码复用）；ctx[0] 待 C 批数值化（原文本 "{error}"）。
        let down = market
            .down_stop()
            .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2158, vec![0])))?;
        // fault2159：涨停价计算子错误压平（同码复用）；ctx[0] 待 C 批数值化（原文本 "{error}"）。
        let up = market
            .up_stop()
            .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2159, vec![0])))?;
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
                // fault2164：存档挂单字段非法；ctx[0]=StockId，
                // ctx[1] 待 C 批数值化（原文本 "{order:?}" 委托快照）。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2164,
                    vec![u64::from(code.0), 0],
                )));
            }
            if !order_ids.insert(order.id.0) {
                // fault2162：已保存委托 id 跨集合重复（同码复用）；ctx[0]=订单 id。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2162,
                    vec![order.id.0],
                )));
            }
            if !book_sequences.insert((code.clone(), order.seq)) {
                // fault2165：簿序号重复；ctx[0]=seq，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2165,
                    vec![order.seq, u64::from(code.0)],
                )));
            }
            // D5：desk 镜像档的卖量/现金预留校验跳过（desk/组权威
            /// ——session sellable/cash 口径不可比对）。
            if !save.desk_mirrored_resting {
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
            }
            max_order_id = max_order_id.max(order.id.0);
            if !save.desk_mirrored_resting {
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
    }

    if !order_ids.is_empty() && save.next_order_id <= max_order_id {
        // fault2166：next_order_id 未超过已保存订单 id；ctx[0]=next_order_id，ctx[1]=最大订单 id。
        return Err(SessionError::InvalidSave(ValidationFault::new(
            2166,
            vec![save.next_order_id, max_order_id],
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
            // fault2167：存档委托现金预留超可用；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2167,
                vec![owner.0],
            )));
        }
    }
    for ((owner, code), reserved) in &sells {
        if save.desk_mirrored_resting {
            // D5：desk 镜像档——卖量锁定的权威=组 pending_sell/desk
            /// （runtime 回写口径 qty 含在途），session 侧 sellable 不可
            /// 比对（镜像单不占 session 预留）。
            continue;
        }
        let sellable = session
            .state
            .accounts
            .get(owner)
            .expect("saved order owner was validated")
            .sellable_qty(code);
        if *reserved > u64::from(sellable) {
            // fault2168：存档卖单股份预留超可卖；ctx[0]=AccountId，ctx[1]=StockId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2168,
                vec![owner.0, u64::from(code.0)],
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
            // fault2169：NPC 报价生命周期账户是玩家；ctx[0]=AccountId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2169,
                vec![lifecycle.account.0],
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
            // fault2170：NPC 报价生命周期订单未唯一对应一笔连续委托；ctx[0]=订单 id。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2170,
                vec![lifecycle.order_id.0],
            )));
        }
        if save
            .plans
            .active_plan(lifecycle.account, &lifecycle.code)
            .is_some_and(|plan| plan.active_child_order_id() == Some(lifecycle.order_id))
        {
            // fault2171：NPC 报价生命周期订单与计划执行腿活跃子委托重复；ctx[0]=订单 id。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2171,
                vec![lifecycle.order_id.0],
            )));
        }
    }
    Ok(())
}

#[derive(Default)]
struct SavedReservations {
    cash: BTreeMap<AccountId, i128>,
    sells: BTreeMap<(AccountId, StockId), u64>,
    sell_filled_totals: BTreeMap<(AccountId, StockId), u64>,
    validated_odd_lot_sells: BTreeSet<(AccountId, StockId)>,
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
        code: &StockId,
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
            // fault2174：已保存委托数量进度非法；ctx[0]=AccountId，ctx[1]=StockId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2174,
                vec![owner.0, u64::from(code.0)],
            )));
        }
        if filled_qty > 0 {
            let market = session
                .state
                .markets
                .get(code)
                .expect("saved order market was validated");
            // fault2158：跌停价计算子错误压平（同码复用）；ctx[0] 待 C 批数值化（原文本 "{error}"）。
            let daily_minimum = market
                .down_stop()
                .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2158, vec![0])))?
                // fault2175：价格×数量乘积子错误压平；ctx[0] 待 C 批数值化（原文本 "{error}" MoneyError）。
                .mul_shares(filled_qty)
                .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2175, vec![0])))?;
            // fault2159：涨停价计算子错误压平（同码复用）；ctx[0] 待 C 批数值化（原文本 "{error}"）。
            let daily_maximum = market
                .up_stop()
                .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2159, vec![0])))?
                // fault2175：价格×数量乘积子错误压平（同码复用）；ctx[0] 待 C 批数值化。
                .mul_shares(filled_qty)
                .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2175, vec![0])))?;
            // fault2175：价格×数量乘积子错误压平（同码复用）；ctx[0] 待 C 批数值化。
            let limit_value = limit
                .mul_shares(filled_qty)
                .map_err(|_| SessionError::InvalidSave(ValidationFault::new(2175, vec![0])))?;
            let (minimum, maximum) = match side {
                Side::Buy => (daily_minimum, limit_value),
                Side::Sell => (limit_value, daily_maximum),
            };
            if filled_value < minimum || filled_value > maximum {
                // fault2176：已成交金额超出价格界限乘积；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2176,
                    vec![owner.0, u64::from(code.0)],
                )));
            }
        }
        let lot_size = u64::from(session.state.setup.config.lot_size);
        if side == Side::Buy {
            if !u64::from(original_qty).is_multiple_of(lot_size) {
                // fault2177：买入数量非整手；ctx[0]=AccountId，ctx[1]=StockId。
                return Err(SessionError::InvalidSave(ValidationFault::new(
                    2177,
                    vec![owner.0, u64::from(code.0)],
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
                // fault2178：重建可卖数量溢出；ctx[0]=AccountId，ctx[1]=StockId。
                SessionError::InvalidSave(ValidationFault::new(
                    2178,
                    vec![owner.0, u64::from(code.0)],
                ))
            })?;
        let original_qty = u64::from(original_qty);
        if original_qty > reconstructed_sellable {
            // fault2168：存档卖单股份预留超可卖（同码复用）；ctx[0]=AccountId，ctx[1]=StockId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2168,
                vec![owner.0, u64::from(code.0)],
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
            // fault2179：卖出数量拆分零股尾差违规；ctx[0]=AccountId，ctx[1]=StockId。
            return Err(SessionError::InvalidSave(ValidationFault::new(
                2179,
                vec![owner.0, u64::from(code.0)],
            )));
        }
        Ok(())
    }

    fn record(
        &mut self,
        config: &GameConfig,
        owner: AccountId,
        code: &StockId,
        order: ReservationOrder,
    ) -> Result<(), SessionError> {
        match order.side {
            Side::Buy => {
                // fault2180：买入预留计算子错误压平；ctx[0] 待 C 批数值化（原文本 "{error}"）。
                let required =
                    buy_order_reservation(config, order.price, order.qty, order.filled_value)
                        .map_err(|_| {
                            SessionError::InvalidSave(ValidationFault::new(2180, vec![0]))
                        })?;
                let reserved = self.cash.entry(owner).or_default();
                *reserved = reserved
                    .checked_add(i128::from(required.cents()))
                    .ok_or_else(|| {
                        // fault2181：买入现金预留累计溢出。
                        SessionError::InvalidSave(ValidationFault::bare(2181))
                    })?;
            }
            Side::Sell => {
                let reserved = self.sells.entry((owner, code.clone())).or_default();
                *reserved = reserved.checked_add(u64::from(order.qty)).ok_or_else(|| {
                    // fault2182：卖出股份预留累计溢出。
                    SessionError::InvalidSave(ValidationFault::bare(2182))
                })?;
            }
        }
        Ok(())
    }
}
