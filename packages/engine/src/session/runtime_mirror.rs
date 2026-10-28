use super::*;
use crate::runtime::drive::{PreparedDriverStep, RuntimeDriver};
use crate::runtime::world::BeliefParticipantSnapshot;
#[cfg(feature = "simulation-diagnostics")]
use std::sync::Arc;

/// 仅持本次镜像替换值；不克隆 GameSession、World 或运行线程。
pub(crate) struct PreparedRuntimeMirror {
    #[cfg(feature = "simulation-diagnostics")]
    decision_traces: crate::diagnostics::decision_trace::NpcDecisionTraceCollector,
    accounts: BTreeMap<AccountId, Account>,
    personals: BTreeMap<AccountId, (usize, BeliefParticipantSnapshot)>,
    plans: crate::plans::PlanBook,
    markets: BTreeMap<StockId, Market>,
    histories: BTreeMap<StockId, VecDeque<Money>>,
    minutes: BTreeMap<StockId, Vec<MarketMinuteClose>>,
    minute_bars: BTreeMap<StockId, Vec<crate::runtime::MinuteBarFact>>,
    candle_book: SessionCandleBook,
    day: u32,
    retained: Option<std::sync::Arc<crate::runtime::publication::RetainedDaySnapshot>>,
    npc_attention: AccountPagedMap<NpcAttentionState>,
    beliefs: AccountPagedMap<BeliefParticipantState>,
    personal_history: AccountPagedMap<crate::experience::PersonalHistoryReadLedger>,
    retail: AccountPagedMap<RetailExperienceState>,
    resting: RuntimeRestingSnapshot,
    mirrors: BTreeMap<StockId, DeskBookMirror>,
    serials: Vec<u64>,
    receipts: BTreeMap<AccountId, crate::experience::AppendOnlyHistory<PersonalTradeConfirmation>>,
    receipt_watermark: u64,
    tick: u64,
    seq: u64,
    source: crate::runtime::source::WorldMirrorSource,
    captured_domains: crate::runtime::source::MirrorDomains,
    install_sequence: u64,
    session_authority_epoch: u64,
    is_handover: bool,
    static_session_revision: Option<u64>,
    pub projection: protocol::PreparedTickProjection,
    pub events: Vec<Event>,
    retail_decisions: Vec<RetailDecisionTrace>,
    retail_order_events: Vec<RetailOrderDiagnosticEvent>,
    npc_order_lifecycles: Vec<NpcOrderLifecycle>,
}

#[cfg(feature = "simulation-diagnostics")]
impl PreparedRuntimeMirror {
    pub(in crate::session) fn causal_plans(&self) -> &crate::plans::PlanBook {
        &self.plans
    }
}

impl GameSession {
    /// 只准备本拍真实观察的共享根；失败时 Session prior 与 driver pending 均保留。
    #[cfg(feature = "simulation-diagnostics")]
    pub(crate) fn prepare_runtime_decision_traces(
        &self,
        driver: &RuntimeDriver,
        tick: u64,
        plans: &crate::plans::PlanBook,
        events: &[Event],
    ) -> Result<crate::diagnostics::decision_trace::NpcDecisionTraceCollector, String> {
        let mut candidate = self.state.npc_decision_traces.clone();
        let mut seen = BTreeSet::new();
        for raw in driver.pending_decision_traces() {
            let record = &raw.record;
            if raw.owner.1 as usize >= driver.accounts_per_group()
                || driver.account_by_owner(raw.owner) != Some(record.account)
                || record.tick != tick || !seen.insert(record.account)
                || !matches!(raw.kind, AccountKind::Inst | AccountKind::Retail | AccountKind::Hot)
                || self.account(record.account).map(|account| account.kind()) != Some(raw.kind) {
                return Err(format!("NPC trace candidate 来源错配：owner={:?} kind={:?} account={:?} tick={} expected={tick}", raw.owner, raw.kind, record.account, record.tick));
            }
            if raw.kind != AccountKind::Inst
                && (!raw.plan_before.is_empty() || !record.plan_ids.is_empty()
                    || !record.plan_changes.is_empty() || !record.budget_constraints.is_empty()) {
                return Err(format!("非机构 trace candidate 含机构计划或预算字段：account={:?}", record.account));
            }
            if raw.kind == AccountKind::Hot
                && (!record.source_report_ids.is_empty() || record.expectation_method.is_some()) {
                return Err(format!("Hot trace candidate 含未发生的公司材料或估值：account={:?}", record.account));
            }
            let mut codes = BTreeSet::new();
            for code in &record.codes {
                if !self.state.markets.contains_key(code) || !codes.insert(*code) {
                    return Err(format!("NPC trace candidate 股票不存在或重复：{code:?}"));
                }
            }
            for plan_id in &record.plan_ids {
                if plan_id.owner != record.account || plans.plan(*plan_id).is_err() {
                    return Err(format!("机构 trace candidate 完整 PlanId 不属本人或不存在：{plan_id:?}"));
                }
            }
            let mut changes = Vec::new();
            let touched: BTreeSet<_> = raw.plan_before.keys().copied()
                .chain(record.plan_ids.iter().copied()).collect();
            for plan_id in touched {
                if plan_id.owner != record.account {
                    return Err(format!("机构 trace before PlanId 不属本人：{plan_id:?}"));
                }
                let plan = plans.plan(plan_id).map_err(|error| format!("机构 trace 最终计划消失：{plan_id:?} {error}"))?;
                let current = crate::session::plan_trace_state(plan);
                if raw.plan_before.get(&plan_id) != Some(&current) {
                    changes.push(format!("{plan_id:?}: {} -> {current}", raw.plan_before.get(&plan_id).map(String::as_str).unwrap_or("Created")));
                }
            }
            // 账户事件口径沿用 legacy trace；不把此投影冒充 causal PlanId→OrderId 归因。
            let orders: Vec<_> = events.iter().filter_map(|event| match event {
                Event::OrderAccepted { account, id, .. } | Event::OrderCanceled { account, id, .. }
                    if *account == record.account => Some(*id),
                _ => None,
            }).collect();
            let mut record = Arc::clone(record);
            if !orders.is_empty() || !changes.is_empty() {
                let record = Arc::make_mut(&mut record);
                record.order_ids = orders;
                record.plan_changes = changes;
            }
            candidate.record_shared(record);
        }
        Ok(candidate)
    }

    #[cfg(feature = "simulation-diagnostics")]
    pub(crate) fn install_runtime_decision_traces(
        &mut self,
        traces: crate::diagnostics::decision_trace::NpcDecisionTraceCollector,
    ) {
        self.state.npc_decision_traces = traces;
    }

    pub(crate) fn prepare_runtime_mirror(
        &self,
        driver: &mut RuntimeDriver,
        input: PreparedDriverStep,
        tick: u64,
    ) -> Result<PreparedRuntimeMirror, String> {
        self.prepare_runtime_mirror_with_epoch(driver, input, tick, self.runtime_authority_epoch)
    }

    pub(crate) fn prepare_runtime_handover_mirror(
        &self,
        driver: &mut RuntimeDriver,
        input: PreparedDriverStep,
    ) -> Result<PreparedRuntimeMirror, String> {
        if input.static_read.is_none() {
            return Err("首次接管必须使用完整静点读面".to_owned());
        }
        let epoch = self.next_runtime_authority_epoch()?;
        self.prepare_runtime_mirror_with_epoch(driver, input, self.tick(), epoch)
    }

    fn prepare_runtime_mirror_with_epoch(
        &self,
        driver: &mut RuntimeDriver,
        input: PreparedDriverStep,
        tick: u64,
        expected_authority_epoch: u64,
    ) -> Result<PreparedRuntimeMirror, String> {
        input.validate_calendar(self, tick, driver.world_committed_tick())?;
        let static_read = input.static_read;
        let is_static = static_read.is_some();
        let static_session_revision = static_read.as_ref().map(|read| read.session_revision);
        if static_session_revision.is_some_and(|revision| revision != self.external_revision()) {
            return Err("静点 Full 捕获后 Session revision 已改变".to_owned());
        }
        if is_static && (input.emit_price_tick || input.day_end.is_some() || !input.events.is_empty()) {
            return Err("静点 Full 不得产生 tick、日结或业务事件".to_owned());
        }
        let mut source = static_read.as_ref().map_or_else(|| driver.mirror_source(), |read| read.source);
        if source != driver.mirror_source() {
            return Err("静点 Full 捕获来源已过期".to_owned());
        }
        let captured_domains = driver.pending_mirror_domains();
        if source.authority_epoch != expected_authority_epoch || source.tick != tick
            || source.civil_date != self.civil_date()
        {
            return Err(format!(
                "镜像 candidate authority 不一致：source={source:?} session_epoch={} tick={tick}",
                self.runtime_authority_epoch
            ));
        }
        let install_sequence = self
            .runtime_mirror_install_sequence
            .checked_add(1)
            .ok_or_else(|| "镜像 install_sequence 已耗尽".to_owned())?;
        let mut accounts = BTreeMap::new();
        let mut personals: BTreeMap<_, _> = input
            .personals
            .into_iter()
            .map(|(index, id, value)| (id, (index, value)))
            .collect();
        let mut hold_inits = Vec::new();
        let per_group = driver.accounts_per_group();
        let n_stocks = self.state.stocks.len();
        let day_end = input.day_end;
        let is_day_end = day_end.is_some();
        let manual_day_end = day_end.as_ref().is_some_and(|data| !data.emit_boundary);
        for (group, rows) in input.accounts.iter().enumerate() {
            let mut seen = BTreeSet::new();
            for (local, raw) in rows.iter() {
                if *local as usize >= per_group || !seen.insert(*local) {
                    return Err(format!(
                        "账户 candidate local 越界或重复：group={group} local={local}"
                    ));
                }
                let global = group * per_group + *local as usize;
                let Some(id) = driver.id_map.get(global).copied() else {
                    // EchoAccounts 的尾部填充不是真实账户；非零填充不能被当作空槽忽略。
                    if raw.as_ref() != &crate::runtime::group::GroupAccount::new(0, n_stocks) {
                        return Err(format!(
                            "账户 candidate 非空 padding：group={group} local={local}"
                        ));
                    }
                    continue;
                };
                let widths = [
                    raw.shares.len(),
                    raw.pending_sell_qty.len(),
                    raw.frozen_today.len(),
                    raw.invested_cents.len(),
                    raw.recovered_cents.len(),
                ];
                if widths.iter().any(|width| *width != n_stocks) {
                    return Err(format!(
                        "账户 candidate 股票维度错误：account={id:?} widths={widths:?}"
                    ));
                }
                let mut account = self
                    .account(id)
                    .ok_or_else(|| format!("账户 candidate 名册缺失：{id:?}"))?
                    .clone();
                let mut positions = account.positions().clone();
                for stock in 0..n_stocks {
                    let code = StockId(u32::try_from(stock).map_err(|_| "股票下标超 u32")?);
                    let qty = raw.shares[stock]
                        .checked_add(raw.pending_sell_qty[stock])
                        .ok_or_else(|| {
                            format!("账户 candidate 股数溢出：account={id:?} stock={code:?}")
                        })?;
                    if raw.frozen_today[stock] > qty {
                        return Err(format!(
                            "账户 candidate T+1 冻结超过持仓：account={id:?} stock={code:?}"
                        ));
                    }
                    if qty == 0 {
                        positions.remove(&code);
                        continue;
                    }
                    if !positions.contains_key(&code) {
                        let entry = input
                            .buy_vwap
                            .get(&(group as _, *local, stock as _))
                            .copied()
                            .or_else(|| input.last_prices.get(stock).copied())
                            .ok_or_else(|| {
                                format!("首买 candidate 缺价格：account={id:?} stock={code:?}")
                            })?;
                        let closing_buy = day_end.as_ref().is_some_and(|data| {
                            data.closing_buy_vwap
                                .contains_key(&(group as _, *local, stock as _))
                        });
                        // 组整态合并不能先抹掉旧镜像的活跃 entry，从而掩盖重复首买或时钟错误。
                        if let Some(ledger) = self.state.retail_experience.get(&id) {
                            let mut checked = ledger.clone();
                            let price = Money::from_cents(entry.max(1));
                            let moment = crate::experience::ExperienceMoment {
                                civil_date: self.civil_date(),
                                market_minute: if closing_buy {
                                    (u64::from(self.day()) + 1)
                                        * u64::from(
                                            crate::observation::GAME_INTRADAY_MINUTES_PER_DAY,
                                        )
                                } else {
                                    self.current_market_minute()
                                },
                                trading_day: u64::from(self.day()),
                            };
                            checked
                                .initialize_holding_dated(&code, Some(price), price, moment)
                                .map_err(|error| {
                                    format!("首买经历初始化失败（旧镜像校验）：{id:?} {error:?}")
                                })?;
                        }
                        hold_inits.push((global, id, code, entry, closing_buy));
                    }
                    positions.insert(
                        code,
                        crate::account::Position::from_restored_parts(
                            qty,
                            raw.frozen_today[stock],
                            raw.invested_cents[stock],
                            raw.recovered_cents[stock],
                        ),
                    );
                }
                let cash = raw
                    .cash
                    .checked_add(raw.buy_purse_held_cents)
                    .ok_or_else(|| format!("账户 candidate 现金溢出：account={id:?}"))?;
                account.fixture_set_cash(Money::from_cents(cash));
                account.fixture_set_positions(positions);
                accounts.insert(id, account);
            }
        }
        // 稀疏个人态与组外首买经历保持既有合并优先级，在私有值上完成可失败初始化。
        for (id, (_, value)) in &mut personals {
            if let Some(group) = &value.retail_experience {
                value.retail_experience = Some(match self.state.retail_experience.get(id) {
                    Some(existing) => existing.clone().merged_with_group_authority(group),
                    None => group.clone(),
                });
            }
        }
        for (global, id, code, entry, closing_buy) in hold_inits {
            if !self.state.retail_experience.contains_key(&id) {
                continue;
            }
            if !personals.contains_key(&id) {
                let value = driver
                    .cached_personal_at(global)
                    .ok_or_else(|| format!("首买经历 candidate 缺完整个人态：{id:?}"))?;
                personals.insert(id, (global, value.clone()));
            }
            let (_, value) = personals.get_mut(&id).expect("刚插入或已有个人 candidate");
            let ledger = value
                .retail_experience
                .get_or_insert_with(|| self.state.retail_experience[&id].clone());
            let price = Money::from_cents(entry.max(1));
            ledger
                .initialize_holding_dated(
                    &code,
                    Some(price),
                    price,
                    crate::experience::ExperienceMoment {
                        civil_date: self.civil_date(),
                        market_minute: if closing_buy {
                            (u64::from(self.day()) + 1)
                                * u64::from(crate::observation::GAME_INTRADAY_MINUTES_PER_DAY)
                        } else {
                            self.current_market_minute()
                        },
                        trading_day: u64::from(self.day()),
                    },
                )
                .map_err(|error| {
                    format!("首买经历初始化失败（私有 candidate）：{id:?} {error:?}")
                })?;
        }
        for (global, id) in input.touched {
            if !personals.contains_key(&id) {
                match driver.cached_personal_at(global) {
                    Some(value) => {
                        personals.insert(id, (global, value.clone()));
                    }
                    None if self.runtime_has_belief_participant(id) => {
                        return Err(format!("个人裁剪 candidate 缺当前个人态：{id:?}"))
                    }
                    None => continue,
                }
            }
            let (_, value) = personals.get_mut(&id).expect("真实个人 candidate 已准备");
            let account = accounts
                .get(&id)
                .or_else(|| self.account(id))
                .ok_or_else(|| format!("个人裁剪 candidate 缺账户：{id:?}"))?;
            let protected = account
                .positions()
                .keys()
                .copied()
                .chain(input.plans.active_investor_codes(id).copied())
                .collect();
            let mut participant = decision_chain::personal_state::BeliefParticipantState::new(
                value.watchlist.clone(),
                value.price_memory.clone(),
                value.information.clone(),
                value.belief.clone(),
            );
            participant.prune_memory(&protected);
            value.watchlist = participant.watchlist().clone();
            value.price_memory = participant.price_memory().clone();
        }
        let mut markets = self.state.markets.clone();
        if is_static {
            for market in markets.values_mut() {
                for order in market.resting_orders() {
                    market.cancel(order.id).map_err(|error| format!("静点旧簿私有清理失败：{error:?}"))?;
                }
            }
        }
        let mut histories = self.state.price_history.clone();
        let mut minutes = self.state.market_minute_closes.clone();
        let mut minute_bars = self.state.market_minute_bars.clone();
        let mut active = self.state.candle_book.active().clone();
        let setup = &self.state.setup;
        let day_tick = tick % setup.ticks_per_day;
        // desk 的 recent_prices 已按真实 open 拍及 history_len 限界；不再用共享日数裁本股历史。
        let history_len = setup.history_len;
        let published = driver.publication().load();
        if published.tick != tick {
            return Err(format!(
                "镜像 candidate 发布 tick 错位：{} != {tick}",
                published.tick
            ));
        }
        let candle_time = self.candle_date_time_for_runtime();
        if !is_static {
        for (index, view) in driver.world_public_views().iter().enumerate() {
            let code = StockId(index as u32);
            if !input.calendar.stock_is_open(code)? {
                continue;
            }
            let market = markets
                .get_mut(&code)
                .ok_or_else(|| format!("行情 candidate 未知股票：{code:?}"))?;
            if view.last_price <= 0 {
                return Err(format!("行情 candidate 非正价格：{code:?}"));
            }
            market.restore_prices(
                Money::from_cents(view.last_price),
                Money::from_cents(view.last_price),
            );
            histories.insert(
                code,
                view.recent_prices
                    .iter()
                    .rev()
                    .take(history_len)
                    .rev()
                    .map(|price| Money::from_cents((*price).max(1)))
                    .collect(),
            );
            let target = minutes.entry(code).or_default();
            target.extend(
                view.minute_closes
                    .iter()
                    .skip(target.len())
                    .map(|(minute, close)| MarketMinuteClose {
                        absolute_trading_minute: *minute,
                        close: Money::from_cents((*close).max(1)),
                    }),
            );
            let target = minute_bars.entry(code).or_default();
            target.extend(view.minute_bars.iter().skip(target.len()).copied());
            let empty = day_tick < setup.auction_ticks - setup.auction_ticks / 3 && day_tick != 0;
            if empty {
                active.remove(&code);
            }
            let shard = published
                .desks
                .iter()
                .find(|shard| u32::from(shard.stock) == code.0)
                .ok_or_else(|| format!("K candidate 缺股票分片：{code:?}"))?;
            let view = &shard.view;
            if !empty {
                if let Some((open, high, low, close, volume)) =
                    view.daily_active.filter(|raw| raw.4 > 0)
                {
                    active.insert(
                        code,
                        DailyCandle {
                            time: candle_time,
                            open: Money::from_cents(open),
                            high: Money::from_cents(high),
                            low: Money::from_cents(low),
                            close: Money::from_cents(close),
                            volume,
                            trade_stats: Some(DailyTradeStats {
                                turnover_cents: view.daily_active_turnover_cents.max(0) as u64,
                                trade_count: view.daily_active_trade_count,
                            }),
                        },
                    );
                } else if active
                    .get(&code)
                    .is_none_or(|candle| candle.time != candle_time)
                {
                    let price = Money::from_cents(view.last_price.max(1));
                    active.insert(
                        code,
                        DailyCandle {
                            time: candle_time,
                            open: price,
                            high: price,
                            low: price,
                            close: price,
                            volume: 0,
                            trade_stats: Some(Default::default()),
                        },
                    );
                }
            }
        }
        }
        let (attention, facts, id_map, serials, static_markets) = if let Some(read) = static_read {
            (read.attention, read.resting, std::sync::Arc::new(driver.id_map.clone()), read.serials, Some(read.market_views))
        } else {
            let attention = driver.sync_attention_for_projection()?;
            let (facts, ids) = driver.resting_snapshot_for_prepared(is_day_end)?;
            (attention, facts, ids, driver.desk_next_serials().to_vec(), None)
        };
        let mut mirrors = BTreeMap::new();
        for (index, rows) in facts.iter().enumerate() {
            let view = driver
                .world_public_views()
                .get(index)
                .ok_or_else(|| format!("簿 candidate 缺股票行情：{index}"))?;
            let (last_price, public_cutoff_serial) = match &static_markets {
                Some(rows) => *rows.get(index).ok_or_else(|| format!("静点缺行情及截点种子：{index}"))?,
                None => (view.last_price, view.public_cutoff_serial),
            };
            let mut orders = Vec::new();
            for fact in rows.iter() {
                let account = driver
                    .account_by_owner(fact.owner)
                    .ok_or_else(|| format!("簿 candidate owner 不在真实名册：{:?}", fact.owner))?;
                orders.push((
                    account,
                    fact.side,
                    Money::from_cents(fact.price),
                    fact.qty,
                    fact.order_serial,
                    fact.filled_value,
                    fact.filled_qty,
                ));
            }
            mirrors.insert(
                StockId(index as u32),
                DeskBookMirror {
                    orders,
                    last_price: Money::from_cents(last_price),
                    public_cutoff_serial,
                    next_serial: *serials
                        .get(index)
                        .ok_or_else(|| format!("簿 candidate 缺游标：{index}"))?,
                },
            );
        }
        let resting = RuntimeRestingSnapshot {
            facts,
            id_map,
            accounts_per_group: per_group,
        };
        let mut receipts = BTreeMap::new();
        let mut receipt_watermark = self.state.receipt_watermark;
        for pending in driver.pending_trade_receipts() {
            let history = receipts.entry(pending.account).or_insert_with(|| {
                self.state
                    .personal_trade_confirmations
                    .get(&pending.account)
                    .cloned()
                    .unwrap_or_default()
            });
            if history
                .last()
                .is_some_and(|last| last.receipt_id >= pending.confirmation.receipt_id)
            {
                return Err(format!(
                    "个人确认 candidate receipt_id 回退或重复：account={:?} receipt={}",
                    pending.account, pending.confirmation.receipt_id
                ));
            }
            receipt_watermark = receipt_watermark.max(
                pending
                    .confirmation
                    .receipt_id
                    .checked_add(1)
                    .ok_or_else(|| "个人确认 candidate 水位溢出".to_owned())?,
            );
            history.push(pending.confirmation.clone());
        }
        let mut closed = BTreeMap::new();
        let mut candle_book = self.state.candle_book.clone();
        candle_book.replace_active(active.clone());
        let day = day_end.as_ref().map_or(self.day(), |data| data.market_day);
        let retained = if let Some(data) = &day_end {
            for (code, candle) in &data.candles {
                active.insert(*code, candle.clone());
            }
            candle_book.replace_active(active.clone());
            closed = candle_book.commit_active();
            let retained = self.assemble_retained_from_prepared(
                data.bars.clone(),
                self.civil_date(),
                &closed,
                true,
            )?;
            driver.publish_prepared_retained(std::sync::Arc::clone(&retained));
            source = driver.mirror_source();
            for values in minutes.values_mut() {
                values.clear();
            }
            for values in minute_bars.values_mut() {
                values.clear();
            }
            Some(retained)
        } else {
            None
        };
        let mut events = input.events;
        if input.emit_price_tick
            && input.calendar.market_phase_before() == Some(TradingPhase::Continuous)
        {
            let views: Vec<_> = driver
                .world_public_views()
                .iter()
                .map(|view| {
                    (
                        view.last_price,
                        view.depth_bids.clone(),
                        view.depth_asks.clone(),
                    )
                })
                .collect();
            for (index, (last_price, bids, asks)) in views.into_iter().enumerate() {
                let code = StockId(index as u32);
                if !input.calendar.stock_is_open(code)? {
                    continue;
                }
                let candle = if is_day_end { &closed } else { &active }
                    .get(&code)
                    .cloned()
                    .ok_or_else(|| format!("连续行情 candidate 缺活动 K：{code:?}"))?;
                let seq = driver.take_event_seq();
                events.push(Event::PriceTick {
                    seq,
                    tick,
                    code,
                    last_price: Money::from_cents(last_price),
                    daily_candle: candle,
                    bids: bids
                        .into_iter()
                        .map(|(price, qty)| (Money::from_cents(price), qty))
                        .collect(),
                    asks: asks
                        .into_iter()
                        .map(|(price, qty)| (Money::from_cents(price), qty))
                        .collect(),
                });
            }
        }
        if let Some(data) = day_end {
            events.extend(data.closing_events);
            if data.emit_boundary {
                let seq = driver.take_event_seq();
                events.push(Event::DayBoundary {
                    seq,
                    day,
                    closed_daily_candles: closed,
                });
            }
            driver.collect_drained_lifecycle_after_boundary();
            events.extend(driver.take_drained_events());
        }
        let seq = driver.next_seq();
        let phase = self.phase_at_tick(tick);
        let mut selected = self
            .state
            .memberships
            .members
            .values()
            .map(|member| member.account_id)
            .collect::<BTreeSet<_>>();
        selected.insert(AccountId(0));
        let mut reserved_cash: BTreeMap<AccountId, Money> = BTreeMap::new();
        let mut reserved_sell: BTreeMap<AccountId, BTreeMap<StockId, u32>> = BTreeMap::new();
        for (code, mirror) in &mirrors {
            for (owner, side, price, qty, _, value, _) in &mirror.orders {
                if !selected.contains(owner) {
                    continue;
                }
                let required = live_cash_reservation(
                    &setup.config,
                    *side,
                    *price,
                    *qty,
                    Money::from_cents(*value),
                )
                .map_err(|error| format!("预留 candidate 金额错误：owner={owner:?} {error}"))?;
                let cash = reserved_cash.entry(*owner).or_default();
                *cash = cash
                    .add(required)
                    .map_err(|error| format!("预留 candidate 现金溢出：owner={owner:?} {error}"))?;
                if *side == Side::Sell {
                    let shares = reserved_sell
                        .entry(*owner)
                        .or_default()
                        .entry(*code)
                        .or_default();
                    *shares = shares
                        .checked_add(*qty)
                        .ok_or_else(|| format!("预留 candidate 股数溢出：owner={owner:?}"))?;
                }
            }
        }
        let mut account_snaps = BTreeMap::new();
        for id in selected {
            let account = accounts
                .get(&id)
                .or_else(|| self.account(id))
                .ok_or_else(|| format!("快照 candidate 缺经济成员账户：{id:?}"))?;
            account_snaps.insert(
                id,
                AccountSnap {
                    cash: account.cash(),
                    positions: account
                        .positions()
                        .iter()
                        .map(|(code, pos)| {
                            (
                                *code,
                                PositionSnap {
                                    qty: pos.qty(),
                                    t1_locked: pos.t1_locked(),
                                    invested_cents: pos.invested_cents(),
                                    recovered_cents: pos.recovered_cents(),
                                },
                            )
                        })
                        .collect(),
                    reserved_cash: reserved_cash.remove(&id).unwrap_or(Money::ZERO),
                    reserved_sell_qty: reserved_sell.remove(&id).unwrap_or_default(),
                },
            );
        }
        let market_snaps = mirrors
            .iter()
            .map(|(code, mirror)| {
                let public = snapshot::project_mirror_public_book(phase, mirror);
                (
                    *code,
                    MarketSnap {
                        last_price: mirror.last_price,
                        last_close: markets[code].last_close(),
                        best_bid: public.best_bid,
                        best_ask: public.best_ask,
                        bids: public.bids,
                        asks: public.asks,
                    },
                )
            })
            .collect();
        let snapshot = Snapshot {
            tick,
            seq,
            day,
            phase,
            markets: market_snaps,
            accounts: account_snaps,
            daily_candles: BTreeMap::new(),
            active_daily_candles: candle_book.active().clone(),
        };
        let runtime = GameSession::public_runtime_from_prepared(
            &snapshot,
            protocol::working_orders_from_mirror(&mirrors, phase),
        )
        .map_err(|error| error.to_string())?;
        let projection = protocol::PreparedTickProjection { snapshot, runtime };
        let retail_decisions = if is_static {
            self.state.last_retail_decisions.clone()
        } else if manual_day_end {
            Vec::new()
        } else {
            driver.collect_retail_traces()
                .map_err(|error| format!("零售决策 candidate 收集失败：{error:?}"))?
                .into_iter().flatten().collect()
        };
        let (mut retail_facts, retail_rejections) = if is_static || manual_day_end {
            (Vec::new(), Vec::new())
        } else {
            driver.pending_retail_facts()
        };
        if is_day_end {
            retail_facts.extend(driver.drained_retail_facts());
        }
        let mut retail_candidate = super::retail_lifecycle::Candidate {
            events: if is_static { self.state.last_retail_order_events.clone() } else { Vec::new() },
            decisions: retail_decisions,
        };
        self.append_runtime_retail_facts_at(driver, &mut retail_candidate, retail_facts, tick)
            .map_err(|error| error.to_string())?;
        self.append_runtime_retail_rejections(&mut retail_candidate, retail_rejections)
            .map_err(|error| error.to_string())?;
        let retail_decisions = retail_candidate.decisions;
        let retail_order_events = retail_candidate.events;
        let mut npc_attention = self.state.npc_attention.clone();
        let mut beliefs = self.state.belief_participants.clone();
        let mut personal_history = self.state.history_reads.clone();
        let mut retail = self.state.retail_experience.clone();
        for (id, (_, value)) in &personals {
            npc_attention.insert(*id, value.attention.clone());
            beliefs.insert(
                *id,
                BeliefParticipantState::new(
                    value.watchlist.clone(),
                    value.price_memory.clone(),
                    value.information.clone(),
                    value.belief.clone(),
                ),
            );
            personal_history.insert(*id, value.history_reads.clone());
            if let Some(row) = &value.retail_experience {
                retail.insert(*id, row.clone());
            }
        }
        for (id, due, seed) in attention {
            if let Some(state) = npc_attention.get_mut(&id) {
                state.next_attention_candidate_tick = if is_static { due } else { due.max(tick) };
                state.rng_state = seed;
            }
        }
        #[cfg(test)]
        if self.runtime_install_failure {
            return Err("安装失败注入（私有镜像 candidate 校验失败）".to_owned());
        }
        Ok(PreparedRuntimeMirror {
            #[cfg(feature = "simulation-diagnostics")]
            decision_traces: self.prepare_runtime_decision_traces(driver, tick, &input.plans, &events)?,
            accounts,
            personals,
            plans: input.plans,
            markets,
            histories,
            minutes,
            minute_bars,
            candle_book,
            day,
            retained,
            npc_attention,
            beliefs,
            personal_history,
            retail,
            resting,
            mirrors,
            serials,
            receipts,
            receipt_watermark,
            tick,
            seq,
            source,
            captured_domains,
            install_sequence,
            session_authority_epoch: self.runtime_authority_epoch,
            is_handover: expected_authority_epoch != self.runtime_authority_epoch,
            static_session_revision,
            projection,
            events,
            retail_decisions,
            retail_order_events,
            npc_order_lifecycles: if is_static { self.state.npc_order_lifecycles.clone() } else { Vec::new() },
        })
    }

    pub(crate) fn install_runtime_mirror(
        &mut self,
        driver: &mut RuntimeDriver,
        prepared: PreparedRuntimeMirror,
    ) -> Vec<Event> {
        self.mark_external_mutation();
        let count = prepared.accounts.len();
        for (id, account) in prepared.accounts {
            self.state.accounts.insert(id, account);
        }
        for (id, (index, value)) in prepared.personals {
            driver.install_cached_personal_at(index, id, value);
        }
        self.state.plans = prepared.plans;
        self.state.markets = prepared.markets;
        self.state.price_history = prepared.histories;
        self.state.market_minute_closes = prepared.minutes;
        self.state.market_minute_bars = prepared.minute_bars;
        self.state.candle_book = prepared.candle_book;
        self.state.day = prepared.day;
        if let Some(retained) = prepared.retained {
            self.state
                .retained_market_days
                .push(crate::runtime::publication::SharedRetainedDay(retained));
        }
        self.state.npc_attention = prepared.npc_attention;
        self.state.belief_participants = prepared.beliefs;
        self.state.history_reads = prepared.personal_history;
        self.state.retail_experience = prepared.retail;
        self.state.runtime_heap_dirty = true;
        self.state.runtime_resting_snapshot = Some(prepared.resting);
        self.state.desk_book_mirror = prepared.mirrors;
        self.state.runtime_desk_serial_cursors = prepared.serials;
        for (account, history) in prepared.receipts {
            self.state
                .personal_trade_confirmations
                .insert(account, history);
        }
        self.state.receipt_watermark = prepared.receipt_watermark;
        self.state.npc_order_lifecycles = prepared.npc_order_lifecycles;
        self.state.last_retail_decisions = prepared.retail_decisions;
        self.state.last_retail_order_events = prepared.retail_order_events;
        #[cfg(feature = "simulation-diagnostics")]
        {
            self.state.npc_decision_traces = prepared.decision_traces;
        }
        self.state.tick = prepared.tick;
        self.state.seq = prepared.seq;
        self.runtime_mirror_install_sequence = prepared.install_sequence;
        driver.finish_mirror_install(count);
        prepared.events
    }

    pub(crate) fn validate_runtime_mirror_source(
        &self,
        driver: &RuntimeDriver,
        prepared: &PreparedRuntimeMirror,
    ) -> Result<crate::runtime::source::PreparedMirrorAck, StepFatal> {
        let expected_sequence = self
            .runtime_mirror_install_sequence
            .checked_add(1)
            .ok_or_else(|| StepFatal::InvariantViolation {
                location: "runtime_mirror::install_sequence".to_owned(),
                description: "镜像 install_sequence 已耗尽，拒绝旧 candidate 安装".to_owned(),
            })?;
        if prepared.install_sequence != expected_sequence {
            return Err(StepFatal::InvariantViolation {
                location: "runtime_mirror::install_sequence".to_owned(),
                description: format!(
                    "镜像 candidate 安装序号已过期：prepared={} expected={expected_sequence}",
                    prepared.install_sequence
                ),
            });
        }
        let source = driver.mirror_source();
        let valid_authority = if prepared.is_handover {
            self.runtime_authority_epoch.checked_add(1) == Some(source.authority_epoch)
        } else {
            source.authority_epoch == self.runtime_authority_epoch
        };
        if source != prepared.source || !valid_authority
            || prepared.static_session_revision.is_some_and(|revision| revision != self.external_revision())
            || prepared.session_authority_epoch != self.runtime_authority_epoch
        {
            return Err(StepFatal::InvariantViolation {
                location: "runtime_mirror::source".to_owned(),
                description: format!(
                    "镜像 candidate 静点来源已改变：prepared=({}, {}) current=({}, {})",
                    prepared.source.tick,
                    prepared.source.generation,
                    source.tick,
                    source.generation
                ),
            });
        }
        driver
            .prepare_mirror_ack(prepared.source, prepared.captured_domains)
            .map_err(|description| StepFatal::InvariantViolation {
                location: "runtime_mirror::source_ack".to_owned(),
                description,
            })
    }
}

#[cfg(test)]
impl PreparedRuntimeMirror {
    pub(in crate::session) fn retail_order_events_for_test(&self) -> &[RetailOrderDiagnosticEvent] {
        &self.retail_order_events
    }

    pub(in crate::session) fn prepared_values_for_test(&self) -> String {
        format!(
            "{:?}",
            (
                (
                    &self.projection.snapshot,
                    &self.npc_attention,
                    &self.beliefs,
                    &self.personal_history,
                    &self.retail,
                    &self.plans,
                    &self.markets
                ),
                (
                    &self.histories,
                    &self.minutes,
                    &self.minute_bars,
                    &self.receipts,
                    self.receipt_watermark,
                    self.source,
                    self.captured_domains
                ),
            )
        )
    }
}

#[cfg(test)]
mod source_tests {
    use super::*;

    #[test]
    fn each_private_source_component_and_pending_mask_is_checked_before_swap() {
        let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
        setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
        setup.ticks_per_day = 8;
        setup.auction_ticks = 3;
        setup.closing_auction_ticks = 0;
        let mut game = GameSession::new(setup, 42).unwrap();
        let mut driver = RuntimeDriver::from_session(&mut game).unwrap();
        driver.adopt_session_seq(&game);
        let input = driver.prepare_driver_step(&game).unwrap();
        let current = driver.mirror_source();
        let mut sources = vec![current; 7];
        sources[0].authority_epoch += 1;
        sources[1].tick += 1;
        sources[2].day += 1;
        sources[3].generation += 1;
        sources[4].day_end_stage = crate::runtime::source::MirrorDayEndStage::DayEndCommitted;
        sources[5].non_tick_revision += 1;
        sources[6].civil_date = crate::calendar::CivilDate::from_ymd(2026, 1, 6).unwrap();
        let pending = driver.pending_mirror_domains();
        let before = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
        for source in sources {
            let mut candidate = game
                .prepare_runtime_mirror(&mut driver, input.clone(), 1)
                .unwrap();
            candidate.source = source;
            assert!(
                game.validate_runtime_mirror_source(&driver, &candidate)
                    .is_err(),
                "来源字段负控应拒绝：{source:?}"
            );
            assert_eq!(driver.pending_mirror_domains(), pending);
            assert_eq!(game.runtime_mirror_install_sequence, 0);
            assert_eq!(driver.account_writes_for_test(), 0);
            assert_eq!(
                serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
                before
            );
        }
        let mut candidate = game.prepare_runtime_mirror(&mut driver, input, 1).unwrap();
        candidate.captured_domains.personal_full = true;
        assert!(
            game.validate_runtime_mirror_source(&driver, &candidate)
                .is_err(),
            "域掩码也必须精确匹配"
        );
        assert_eq!(driver.pending_mirror_domains(), pending);
    }

    #[test]
    fn exhausted_install_sequence_rejects_before_swap_without_ack() {
        let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
        setup.ticks_per_day = 8;
        setup.auction_ticks = 3;
        setup.closing_auction_ticks = 0;
        let mut game = GameSession::new(setup, 42).unwrap();
        let mut driver = RuntimeDriver::from_session(&mut game).unwrap();
        let input = driver.prepare_driver_step(&game).unwrap();
        let candidate = game.prepare_runtime_mirror(&mut driver, input, 1).unwrap();
        let pending = driver.pending_mirror_domains();
        game.runtime_mirror_install_sequence = u64::MAX;
        let fatal = game
            .validate_runtime_mirror_source(&driver, &candidate)
            .err()
            .expect("耗尽必须类型化拒绝");
        assert!(fatal.to_string().contains("install_sequence"));
        assert_eq!(driver.pending_mirror_domains(), pending);
        assert_eq!(driver.account_writes_for_test(), 0);
        assert_eq!(game.tick(), 0);
        assert_eq!(game.runtime_mirror_install_sequence, u64::MAX);
    }
}
