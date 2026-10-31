use super::*;
use crate::runtime::drive::{PreparedDriverStep, RuntimeDriver};
use crate::runtime::world::BeliefParticipantSnapshot;
use std::sync::Arc;

#[cfg(test)]
thread_local! {
    static RUNTIME_BOOK_PROJECTION_COSTS: std::cell::Cell<(usize, usize)> =
        const { std::cell::Cell::new((0, 0)) };
    static WORKING_ORDER_ROW_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_runtime_book_projection_costs_for_test() {
    RUNTIME_BOOK_PROJECTION_COSTS.with(|costs| costs.set((0, 0)));
}

#[cfg(test)]
pub(crate) fn take_runtime_book_projection_costs_for_test() -> (usize, usize) {
    RUNTIME_BOOK_PROJECTION_COSTS.with(|costs| costs.replace((0, 0)))
}

#[cfg(test)]
pub(crate) fn reset_working_order_row_visits_for_test() {
    WORKING_ORDER_ROW_VISITS.with(|visits| visits.set(0));
}

#[cfg(test)]
pub(crate) fn take_working_order_row_visits_for_test() -> usize {
    WORKING_ORDER_ROW_VISITS.with(|visits| visits.replace(0))
}

#[cfg(test)]
pub(crate) fn record_working_order_row_visit_for_test() {
    WORKING_ORDER_ROW_VISITS.with(|visits| visits.set(visits.get() + 1));
}

#[cfg(test)]
pub(super) fn record_full_public_book_aggregation_for_test() {
    RUNTIME_BOOK_PROJECTION_COSTS.with(|costs| {
        let (mirrors, projections) = costs.get();
        costs.set((mirrors, projections + 1));
    });
}
fn runtime_book_cache_header_matches(
    previous: Option<&RuntimeRestingSnapshot>,
    id_map: &std::sync::Arc<Vec<AccountId>>,
    accounts_per_group: usize,
    authority_epoch: u64,
) -> bool {
    previous.is_some_and(|snapshot| {
        std::sync::Arc::ptr_eq(&snapshot.id_map, id_map)
            && snapshot.accounts_per_group == accounts_per_group
            && snapshot
                .projection_cache
                .as_ref()
                .is_some_and(|cache| cache.authority_epoch == authority_epoch)
    })
}

fn cached_public_projection(
    cache: Option<&RuntimeRestingProjectionCache>,
    entry: Option<&RuntimeRestingProjectionEntry>,
    mirror: &std::sync::Arc<DeskBookMirror>,
    phase: crate::TradingPhase,
) -> Option<std::sync::Arc<pipeline::phase::PublicBookProjection>> {
    cache
        .filter(|cache| cache.phase == phase)
        .and(entry)
        .filter(|entry| std::sync::Arc::ptr_eq(&entry.mirror, mirror))
        .map(|entry| std::sync::Arc::clone(&entry.projection))
}

fn runtime_book_cache_entry_matches(
    previous_facts: Option<&std::sync::Arc<Vec<crate::runtime::RestingFact>>>,
    current_facts: &std::sync::Arc<Vec<crate::runtime::RestingFact>>,
    entry: &RuntimeRestingProjectionEntry,
    committed_mirror: Option<&std::sync::Arc<DeskBookMirror>>,
    last_price: i64,
    public_cutoff_serial: u64,
    next_serial: u64,
) -> bool {
    previous_facts.is_some_and(|old| std::sync::Arc::ptr_eq(old, current_facts))
        && committed_mirror.is_some_and(|current| {
            std::sync::Arc::ptr_eq(current, &entry.mirror)
                && current.last_price.cents() == last_price
                && current.public_cutoff_serial == public_cutoff_serial
                && current.next_serial == next_serial
        })
}

/// Session 四表与 driver 缓存共享本人的最终只读值；写入仍由各表的 COW 隔离。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SharedRuntimePersonal {
    attention: Arc<NpcAttentionState>,
    participant: Arc<BeliefParticipantState>,
    history_reads: Arc<crate::experience::PersonalHistoryReadLedger>,
    retail_experience: Option<Arc<RetailExperienceState>>,
}

impl From<BeliefParticipantSnapshot> for SharedRuntimePersonal {
    fn from(value: BeliefParticipantSnapshot) -> Self {
        Self {
            attention: Arc::new(value.attention),
            participant: Arc::new(BeliefParticipantState::new(
                value.watchlist,
                value.price_memory,
                value.information,
                value.belief,
            )),
            history_reads: Arc::new(value.history_reads),
            retail_experience: value.retail_experience.map(Arc::new),
        }
    }
}

impl SharedRuntimePersonal {
    pub(crate) fn matches_owner(&self, owner: AccountId) -> bool {
        self.participant.information().owner() == owner && self.participant.belief().npc() == owner
    }

    fn merge_retail_from(&mut self, existing: Option<&RetailExperienceState>) {
        let (Some(existing), Some(group)) = (existing, self.retail_experience.as_mut()) else {
            return;
        };
        // 原合并只从镜像补 group 没有的 stock；其余字段与已知 stock 均以 group 为准。
        if !existing
            .stocks
            .keys()
            .any(|code| !group.stocks.contains_key(code))
        {
            return;
        }
        let group = Arc::make_mut(group);
        for (code, value) in &existing.stocks {
            group.stocks.entry(*code).or_insert_with(|| value.clone());
        }
    }

    fn prune_memory(&mut self, protected: &BTreeSet<StockId>) {
        // 两种 prune 都只 retain：总数不超过原未保护上限时，任何保护集下都无值变化。
        let cap = crate::experience::MAX_UNHELD_WATCHLIST_STOCKS;
        if self.participant.watchlist().stock_count() <= cap
            && self.participant.price_memory().stock_count() <= cap
        {
            return;
        }
        Arc::make_mut(&mut self.participant).prune_memory(protected);
    }

    /// 完整读面显式物化拥有值，调用方后续修改不影响 Session 或缓存。
    pub(crate) fn to_snapshot(&self) -> BeliefParticipantSnapshot {
        BeliefParticipantSnapshot {
            watchlist: self.participant.watchlist().clone(),
            price_memory: self.participant.price_memory().clone(),
            information: self.participant.information().clone(),
            belief: self.participant.belief().clone(),
            attention: self.attention.as_ref().clone(),
            history_reads: self.history_reads.as_ref().clone(),
            retail_experience: self.retail_experience.as_deref().cloned(),
        }
    }

    pub(crate) fn matches_snapshot(&self, value: &BeliefParticipantSnapshot) -> bool {
        self.participant.watchlist() == &value.watchlist
            && self.participant.price_memory() == &value.price_memory
            && self.participant.information() == &value.information
            && self.participant.belief() == &value.belief
            && self.attention.as_ref() == &value.attention
            && self.history_reads.as_ref() == &value.history_reads
            && self.retail_experience.as_deref() == value.retail_experience.as_ref()
    }
}

/// 仅持本次镜像替换值；不克隆 GameSession、World 或运行线程。
pub(crate) struct PreparedRuntimeMirror {
    #[cfg(feature = "simulation-diagnostics")]
    pub(crate) causal_calendar: crate::runtime::drive::PreparedDriverCalendar,
    #[cfg(feature = "simulation-diagnostics")]
    decision_traces: crate::diagnostics::decision_trace::NpcDecisionTraceCollector,
    accounts: BTreeMap<AccountId, Account>,
    personals: BTreeMap<AccountId, (usize, SharedRuntimePersonal)>,
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
    mirrors: BTreeMap<StockId, std::sync::Arc<DeskBookMirror>>,
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
    installed_market_close: bool,
    static_session_revision: Option<u64>,
    pub(super) projection: protocol::PreparedTickProjection,
    pub events: Vec<Event>,
    retail_decisions: Vec<RetailDecisionTrace>,
    retail_order_events: Vec<RetailOrderDiagnosticEvent>,
    npc_order_lifecycles: Vec<NpcOrderLifecycle>,
}

#[cfg(feature = "simulation-diagnostics")]
impl PreparedRuntimeMirror {
    pub(crate) fn causal_plans(&self) -> &crate::plans::PlanBook {
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
                || record.tick != tick
                || !seen.insert(record.account)
                || !matches!(
                    raw.kind,
                    AccountKind::Inst | AccountKind::Retail | AccountKind::Hot
                )
                || self.account(record.account).map(|account| account.kind()) != Some(raw.kind)
            {
                return Err(format!("NPC trace candidate 来源错配：owner={:?} kind={:?} account={:?} tick={} expected={tick}", raw.owner, raw.kind, record.account, record.tick));
            }
            if raw.kind != AccountKind::Inst
                && (!raw.plan_before.is_empty()
                    || !record.plan_ids.is_empty()
                    || !record.plan_changes.is_empty()
                    || !record.budget_constraints.is_empty())
            {
                return Err(format!(
                    "非机构 trace candidate 含机构计划或预算字段：account={:?}",
                    record.account
                ));
            }
            if raw.kind == AccountKind::Hot
                && (!record.source_report_ids.is_empty() || record.expectation_method.is_some())
            {
                return Err(format!(
                    "Hot trace candidate 含未发生的公司材料或估值：account={:?}",
                    record.account
                ));
            }
            let mut codes = BTreeSet::new();
            for code in &record.codes {
                if !self.state.markets.contains_key(code) || !codes.insert(*code) {
                    return Err(format!("NPC trace candidate 股票不存在或重复：{code:?}"));
                }
            }
            for plan_id in &record.plan_ids {
                if plan_id.owner != record.account || plans.plan(*plan_id).is_err() {
                    return Err(format!(
                        "机构 trace candidate 完整 PlanId 不属本人或不存在：{plan_id:?}"
                    ));
                }
            }
            let mut changes = Vec::new();
            let touched: BTreeSet<_> = raw
                .plan_before
                .keys()
                .copied()
                .chain(record.plan_ids.iter().copied())
                .collect();
            for plan_id in touched {
                if plan_id.owner != record.account {
                    return Err(format!("机构 trace before PlanId 不属本人：{plan_id:?}"));
                }
                let plan = plans
                    .plan(plan_id)
                    .map_err(|error| format!("机构 trace 最终计划消失：{plan_id:?} {error}"))?;
                let current = crate::session::plan_trace_state(plan);
                if raw.plan_before.get(&plan_id) != Some(&current) {
                    changes.push(format!(
                        "{plan_id:?}: {} -> {current}",
                        raw.plan_before
                            .get(&plan_id)
                            .map(String::as_str)
                            .unwrap_or("Created")
                    ));
                }
            }
            // 账户事件口径沿用 legacy trace；不把此投影冒充 causal PlanId→OrderId 归因。
            let orders: Vec<_> = events
                .iter()
                .filter_map(|event| match event {
                    Event::OrderAccepted { account, id, .. }
                    | Event::OrderCanceled { account, id, .. }
                        if *account == record.account =>
                    {
                        Some(*id)
                    }
                    _ => None,
                })
                .collect();
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
        mut input: PreparedDriverStep,
        tick: u64,
        expected_authority_epoch: u64,
    ) -> Result<PreparedRuntimeMirror, String> {
        input.validate_calendar(self, tick, driver.world_committed_tick())?;
        let phase = self.phase_at_tick(tick);
        #[cfg(feature = "simulation-diagnostics")]
        let causal_calendar = input.calendar.clone();
        let is_static = input.static_read.is_some();
        let refresh_market = input
            .static_read
            .as_ref()
            .is_some_and(|read| read.refresh_market);
        let static_session_revision = input.static_read.as_ref().map(|read| read.session_revision);
        if static_session_revision.is_some_and(|revision| revision != self.external_revision()) {
            return Err("静点 Full 捕获后 Session revision 已改变".to_owned());
        }
        if is_static
            && (input.emit_price_tick || input.day_end.is_some() || !input.events.is_empty())
        {
            return Err("静点 Full 不得产生 tick、日结或业务事件".to_owned());
        }
        let mut source = input
            .static_read
            .as_ref()
            .map_or_else(|| driver.mirror_source(), |read| read.source);
        if source != driver.mirror_source() {
            return Err("静点 Full 捕获来源已过期".to_owned());
        }
        let captured_domains = driver.pending_mirror_domains();
        if source.authority_epoch != expected_authority_epoch
            || source.tick != tick
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
        driver.materialize_sync_mirror_inputs(&mut input)?;
        let static_read = input.static_read;
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
        let installed_market_close = is_day_end
            && matches!(&input.calendar,
                crate::runtime::drive::PreparedDriverCalendar::MarketTick(calendar)
                    if calendar.after_input().progress
                        == super::exchange_calendar_model::SharedDayProgress::TradingCompleted);
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
            value.merge_retail_from(self.state.retail_experience.get(id));
        }
        for (global, id, code, entry, closing_buy) in hold_inits {
            if !self.state.retail_experience.contains_key(&id) {
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = personals.entry(id) {
                let value = driver
                    .cached_personal_at(global)
                    .ok_or_else(|| format!("首买经历 candidate 缺完整个人态：{id:?}"))?;
                entry.insert((global, value.clone()));
            }
            let (_, value) = personals.get_mut(&id).expect("刚插入或已有个人 candidate");
            let ledger = value
                .retail_experience
                .get_or_insert_with(|| Arc::new(self.state.retail_experience[&id].clone()));
            // 延迟窗口内组可能已在下一次观察创建完整持仓经历；该整态保持组权威。
            // 仅为组也尚未建立的首买补既有桥接，不再次初始化已有生命周期。
            if ledger.feedback.stocks.contains_key(&code) {
                continue;
            }
            let price = Money::from_cents(entry.max(1));
            Arc::make_mut(ledger)
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
            if let std::collections::btree_map::Entry::Vacant(entry) = personals.entry(id) {
                match driver.cached_personal_at(global) {
                    Some(value) => {
                        entry.insert((global, value.clone()));
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
            value.prune_memory(&protected);
        }
        let mut markets = self.state.markets.clone();
        if is_static {
            for market in markets.values_mut() {
                for order in market.resting_orders() {
                    market
                        .cancel(order.id)
                        .map_err(|error| format!("静点旧簿私有清理失败：{error:?}"))?;
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
        if !is_static || refresh_market {
            for (index, view) in driver.world_public_views().iter().enumerate() {
                let code = StockId(index as u32);
                if !input.calendar.stock_is_open(code)? {
                    continue;
                }
                let market = markets
                    .get_mut(&code)
                    .ok_or_else(|| format!("行情 candidate 未知股票：{code:?}"))?;
                if view.last_price <= 0 || view.last_close <= 0 {
                    return Err(format!("行情 candidate 非正价格：{code:?}"));
                }
                market.restore_prices(
                    Money::from_cents(view.last_price),
                    Money::from_cents(view.last_close),
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
                target.extend(view.minute_closes.iter().skip(target.len()).map(
                    |(minute, close)| MarketMinuteClose {
                        absolute_trading_minute: *minute,
                        close: Money::from_cents((*close).max(1)),
                    },
                ));
                let target = minute_bars.entry(code).or_default();
                target.extend(view.minute_bars.iter().skip(target.len()).copied());
                let empty =
                    day_tick < setup.auction_ticks - setup.auction_ticks / 3 && day_tick != 0;
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
            (
                read.attention,
                read.resting,
                std::sync::Arc::new(driver.id_map.clone()),
                read.serials,
                Some(read.market_views),
            )
        } else {
            let attention = driver.sync_attention_for_projection()?;
            let (facts, ids) = driver.resting_snapshot_for_prepared(is_day_end)?;
            (
                attention,
                facts,
                ids,
                driver.desk_next_serials().to_vec(),
                None,
            )
        };
        let previous_snapshot = self.state.runtime_resting_snapshot.as_ref();
        let previous_cache =
            previous_snapshot.and_then(|snapshot| snapshot.projection_cache.as_ref());
        let cache_header_matches = runtime_book_cache_header_matches(
            previous_snapshot,
            &id_map,
            per_group,
            source.authority_epoch,
        );
        let mut mirrors = BTreeMap::new();
        for (index, rows) in facts.iter().enumerate() {
            let code = StockId(index as u32);
            let view = driver
                .world_public_views()
                .get(index)
                .ok_or_else(|| format!("簿 candidate 缺股票行情：{index}"))?;
            let (last_price, public_cutoff_serial) = match &static_markets {
                Some(rows) => *rows
                    .get(index)
                    .ok_or_else(|| format!("静点缺行情及截点种子：{index}"))?,
                None => (view.last_price, view.public_cutoff_serial),
            };
            let next_serial = *serials
                .get(index)
                .ok_or_else(|| format!("簿 candidate 缺游标：{index}"))?;
            let committed_mirror = self.state.desk_book_mirror.get(&code);
            let reusable = if cache_header_matches {
                previous_cache
                    .and_then(|cache| cache.entries.get(&code))
                    .filter(|entry| {
                        runtime_book_cache_entry_matches(
                            previous_snapshot.and_then(|snapshot| snapshot.facts.get(index)),
                            rows,
                            entry,
                            committed_mirror,
                            last_price,
                            public_cutoff_serial,
                            next_serial,
                        )
                    })
            } else {
                None
            };
            let mirror = if let Some(entry) = reusable {
                std::sync::Arc::clone(&entry.mirror)
            } else {
                #[cfg(test)]
                RUNTIME_BOOK_PROJECTION_COSTS.with(|costs| {
                    let (mirrors, projections) = costs.get();
                    costs.set((mirrors + 1, projections));
                });
                let mut orders = Vec::new();
                for fact in rows.iter() {
                    let account = driver.account_by_owner(fact.owner).ok_or_else(|| {
                        format!("簿 candidate owner 不在真实名册：{:?}", fact.owner)
                    })?;
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
                std::sync::Arc::new(DeskBookMirror {
                    orders,
                    last_price: Money::from_cents(last_price),
                    public_cutoff_serial,
                    next_serial,
                })
            };
            mirrors.insert(code, mirror);
        }
        let mut resting = RuntimeRestingSnapshot {
            facts,
            id_map,
            accounts_per_group: per_group,
            projection_cache: None,
        };
        let (receipts, receipt_watermark) = self.prepare_runtime_receipts(driver)?;
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
            let bars = data.bars.clone();
            #[cfg(all(test, feature = "simulation-diagnostics", not(target_arch = "wasm32")))]
            let bars = super::runtime_diagnostics_tests::retained_tests::perturb_volume_once(bars);
            let retained =
                self.assemble_retained_from_prepared(bars, self.civil_date(), &closed, true)?;
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
        let mut working_orders = Vec::new();
        for (code, mirror) in &mirrors {
            for (owner, side, price, qty, serial, value, _) in &mirror.orders {
                #[cfg(test)]
                crate::session::runtime_mirror::record_working_order_row_visit_for_test();
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
                if let Some(order) = protocol::player_working_order_from_mirror(
                    *code, *owner, *side, *price, *qty, *serial, phase,
                ) {
                    working_orders.push(order);
                }
            }
        }
        working_orders.sort_by_key(|order| order.id);
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
        let mut cache_entries = BTreeMap::new();
        let market_snaps = mirrors
            .iter()
            .map(|(code, mirror)| {
                let old_entry = previous_cache.and_then(|cache| cache.entries.get(code));
                let cached_projection = if cache_header_matches {
                    cached_public_projection(previous_cache, old_entry, mirror, phase)
                } else {
                    None
                };
                let projection = cached_projection.unwrap_or_else(|| {
                    std::sync::Arc::new(snapshot::project_mirror_public_book(phase, mirror))
                });
                cache_entries.insert(
                    *code,
                    RuntimeRestingProjectionEntry {
                        mirror: std::sync::Arc::clone(mirror),
                        projection: std::sync::Arc::clone(&projection),
                    },
                );
                let public = &projection;
                (
                    *code,
                    MarketSnap {
                        last_price: mirror.last_price,
                        last_close: markets[code].last_close(),
                        best_bid: public.best_bid,
                        best_ask: public.best_ask,
                        bids: public.bids.clone(),
                        asks: public.asks.clone(),
                    },
                )
            })
            .collect();
        resting.projection_cache = Some(RuntimeRestingProjectionCache {
            authority_epoch: source.authority_epoch,
            phase,
            entries: cache_entries,
        });
        let origin = self.origin_from_runtime_source(source, install_sequence, phase)?;
        let snapshot = Snapshot {
            mirror_origin: origin,
            tick,
            seq,
            day,
            phase,
            markets: market_snaps,
            accounts: account_snaps,
            daily_candles: BTreeMap::new(),
            active_daily_candles: candle_book.active().clone(),
        };
        let runtime = GameSession::public_runtime_from_prepared(&snapshot, working_orders)
            .map_err(|error| error.to_string())?;
        let projection = protocol::PreparedTickProjection { snapshot, runtime };
        let retail_decisions = if is_static {
            self.state.last_retail_decisions.clone()
        } else if manual_day_end {
            Vec::new()
        } else {
            driver
                .collect_retail_traces()
                .map_err(|error| format!("零售决策 candidate 收集失败：{error:?}"))?
                .into_iter()
                .flatten()
                .collect()
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
            events: if is_static {
                self.state.last_retail_order_events.clone()
            } else {
                Vec::new()
            },
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
            npc_attention.insert_arc(*id, Arc::clone(&value.attention));
            beliefs.insert_arc(*id, Arc::clone(&value.participant));
            personal_history.insert_arc(*id, Arc::clone(&value.history_reads));
            if let Some(row) = &value.retail_experience {
                retail.insert_arc(*id, Arc::clone(row));
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
            causal_calendar,
            #[cfg(feature = "simulation-diagnostics")]
            decision_traces: if manual_day_end {
                self.state.npc_decision_traces.clone()
            } else {
                self.prepare_runtime_decision_traces(driver, tick, &input.plans, &events)?
            },
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
            installed_market_close,
            static_session_revision,
            projection,
            events,
            retail_decisions,
            retail_order_events,
            npc_order_lifecycles: if is_static {
                self.state.npc_order_lifecycles.clone()
            } else {
                Vec::new()
            },
        })
    }

    pub(crate) fn install_runtime_mirror(
        &mut self,
        driver: &mut RuntimeDriver,
        prepared: PreparedRuntimeMirror,
    ) -> Vec<Event> {
        self.mark_external_mutation();
        let (events, personals, count) = self.move_runtime_mirror_state(prepared, true);
        for (id, (index, value)) in personals {
            driver.install_cached_personal_at(index, id, value);
        }
        driver.finish_mirror_install(count);
        events
    }

    /// 只在私有 handover base 上移动完整领域值；不消费旧 driver 的覆盖表或缓存。
    pub(in crate::session) fn materialize_runtime_handover_base(
        &mut self,
        prepared: PreparedRuntimeMirror,
    ) {
        let (events, _, _) = self.move_runtime_mirror_state(prepared, false);
        assert!(events.is_empty(), "静点 handover base 不能产生市场事实");
    }

    fn move_runtime_mirror_state(
        &mut self,
        prepared: PreparedRuntimeMirror,
        commit_source: bool,
    ) -> (
        Vec<Event>,
        BTreeMap<AccountId, (usize, SharedRuntimePersonal)>,
        usize,
    ) {
        let count = prepared.accounts.len();
        for (id, account) in prepared.accounts {
            self.state.accounts.insert(id, account);
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
        if prepared.installed_market_close {
            self.runtime_installed_market_close = Some((
                prepared.source.authority_epoch,
                prepared.tick,
                prepared.source.civil_date,
            ));
        }
        if commit_source {
            self.runtime_mirror_install_sequence = prepared.install_sequence;
            self.runtime_mirror_origin = Some(prepared.projection.snapshot.mirror_origin);
            self.runtime_publication_generation = prepared.source.generation;
        }
        (prepared.events, prepared.personals, count)
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
        if source != prepared.source
            || !valid_authority
            || prepared
                .static_session_revision
                .is_some_and(|revision| revision != self.external_revision())
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
mod runtime_book_cache_guard_tests {
    use super::*;

    fn fact_rows() -> std::sync::Arc<Vec<crate::runtime::RestingFact>> {
        std::sync::Arc::new(Vec::new())
    }

    fn cache_entry() -> RuntimeRestingProjectionEntry {
        RuntimeRestingProjectionEntry {
            mirror: std::sync::Arc::new(DeskBookMirror {
                orders: Vec::new(),
                last_price: Money::from_cents(1_000),
                public_cutoff_serial: 7,
                next_serial: 8,
            }),
            projection: std::sync::Arc::new(pipeline::phase::PublicBookProjection {
                best_bid: None,
                best_ask: None,
                bids: Vec::new(),
                asks: Vec::new(),
            }),
        }
    }

    #[test]
    fn runtime_book_cache_candidate_failure_keeps_the_committed_cache_identity() {
        let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
        setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
        setup.ticks_per_day = 8;
        setup.auction_ticks = 0;
        setup.closing_auction_ticks = 0;
        setup.npcs.retail_count = 0;
        setup.npcs.inst_count = 0;
        setup.npcs.hot_count = 0;
        let mut game = GameSession::new(setup, 42).unwrap();
        let mut driver = RuntimeDriver::from_session(&mut game).unwrap();
        driver.step(&mut game).unwrap();
        let committed_snapshot = game.state.runtime_resting_snapshot.as_ref().unwrap();
        let committed_entry = &committed_snapshot
            .projection_cache
            .as_ref()
            .unwrap()
            .entries[&StockId(0)];
        let old_facts = std::sync::Arc::clone(&committed_snapshot.facts[0]);
        let old_mirror = std::sync::Arc::clone(&committed_entry.mirror);
        let old_projection = std::sync::Arc::clone(&committed_entry.projection);
        let before = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();

        game.enqueue_player_intent(
            AccountId(0),
            crate::Intent::PlaceLimit {
                code: StockId(0),
                side: crate::Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(995)),
                qty: 100,
            },
        )
        .unwrap();
        game.runtime_install_failure = true;
        let error = driver.step(&mut game).unwrap_err();

        let published_facts = driver.publication().load().desks[0].resting.clone();
        assert!(
            !published_facts.is_empty(),
            "失败前 World 已发布新非空在途簿"
        );
        assert!(
            !std::sync::Arc::ptr_eq(&old_facts, &published_facts),
            "失败前 World 的 resting facts 已变化"
        );
        assert!(
            error.contains("安装失败注入（私有镜像 candidate 校验失败）"),
            "必须报告原候选校验失败原因：{error}"
        );
        let after = game.state.runtime_resting_snapshot.as_ref().unwrap();
        let after_entry = &after.projection_cache.as_ref().unwrap().entries[&StockId(0)];
        assert!(std::sync::Arc::ptr_eq(&old_mirror, &after_entry.mirror));
        assert!(std::sync::Arc::ptr_eq(
            &old_projection,
            &after_entry.projection
        ));
        assert!(std::sync::Arc::ptr_eq(&old_facts, &after.facts[0]));
        assert_eq!(
            serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
            before,
            "失败 candidate 不得安装或污染已提交簿与公开快照"
        );

        let committed_world_tick = driver.world_committed_tick();
        assert_eq!(committed_world_tick, 2);
        let retry_error = driver.step(&mut game).unwrap_err();
        assert_eq!(retry_error, error, "poison 后必须重放首个 fatal");
        assert_eq!(driver.world_committed_tick(), committed_world_tick);
        let retry_snapshot = game.state.runtime_resting_snapshot.as_ref().unwrap();
        let retry_entry = &retry_snapshot.projection_cache.as_ref().unwrap().entries[&StockId(0)];
        assert!(std::sync::Arc::ptr_eq(&old_facts, &retry_snapshot.facts[0]));
        assert!(std::sync::Arc::ptr_eq(&old_mirror, &retry_entry.mirror));
        assert!(std::sync::Arc::ptr_eq(
            &old_projection,
            &retry_entry.projection
        ));
        assert_eq!(
            serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
            before,
            "poison 重放不得推进 World 或改变已提交簿与公开快照"
        );
    }

    #[test]
    fn runtime_book_cache_requires_the_same_facts_arc_even_when_contents_match() {
        let original = fact_rows();
        let same_contents_new_arc = std::sync::Arc::new(original.as_ref().clone());
        let entry = cache_entry();
        let committed = std::sync::Arc::clone(&entry.mirror);
        assert!(runtime_book_cache_entry_matches(
            Some(&original),
            &original,
            &entry,
            Some(&committed),
            1_000,
            7,
            8,
        ));
        assert!(!runtime_book_cache_entry_matches(
            Some(&original),
            &same_contents_new_arc,
            &entry,
            Some(&committed),
            1_000,
            7,
            8,
        ));
    }

    #[test]
    fn runtime_book_cache_header_requires_mapping_group_and_epoch_identity() {
        let id_map = std::sync::Arc::new(vec![AccountId(0)]);
        let snapshot = RuntimeRestingSnapshot {
            facts: vec![fact_rows()],
            id_map: std::sync::Arc::clone(&id_map),
            accounts_per_group: 1,
            projection_cache: Some(RuntimeRestingProjectionCache {
                authority_epoch: 4,
                phase: TradingPhase::Continuous,
                entries: BTreeMap::new(),
            }),
        };
        assert!(runtime_book_cache_header_matches(
            Some(&snapshot),
            &id_map,
            1,
            4
        ));
        assert!(!runtime_book_cache_header_matches(
            Some(&snapshot),
            &std::sync::Arc::new(vec![AccountId(0)]),
            1,
            4,
        ));
        assert!(!runtime_book_cache_header_matches(
            Some(&snapshot),
            &id_map,
            2,
            4
        ));
        assert!(!runtime_book_cache_header_matches(
            Some(&snapshot),
            &id_map,
            1,
            5
        ));
    }

    #[test]
    fn runtime_book_cache_rejects_changed_mirror_metadata_or_cow_identity() {
        let facts = fact_rows();
        let entry = cache_entry();
        let committed = std::sync::Arc::clone(&entry.mirror);
        let matches = |current: Option<&std::sync::Arc<DeskBookMirror>>, price, cutoff, serial| {
            runtime_book_cache_entry_matches(
                Some(&facts),
                &facts,
                &entry,
                current,
                price,
                cutoff,
                serial,
            )
        };
        assert!(matches(Some(&committed), 1_000, 7, 8));
        assert!(!matches(Some(&committed), 1_001, 7, 8));
        assert!(!matches(Some(&committed), 1_000, 9, 8));
        assert!(!matches(Some(&committed), 1_000, 7, 9));
        let cow_changed = std::sync::Arc::new(committed.as_ref().clone());
        assert!(!matches(Some(&cow_changed), 1_000, 7, 8));
    }

    #[test]
    fn changed_phase_reprojects_the_full_closing_book() {
        let mirror = std::sync::Arc::new(DeskBookMirror {
            orders: (1..=7_u64)
                .map(|serial| {
                    (
                        AccountId(1),
                        crate::Side::Buy,
                        Money::from_cents(1_000 - serial as i64),
                        100,
                        serial,
                        0,
                        0,
                    )
                })
                .collect(),
            last_price: Money::from_cents(1_000),
            public_cutoff_serial: 8,
            next_serial: 8,
        });
        let entry = RuntimeRestingProjectionEntry {
            mirror: std::sync::Arc::clone(&mirror),
            projection: std::sync::Arc::new(pipeline::phase::PublicBookProjection {
                best_bid: Some(Money::from_cents(999)),
                best_ask: None,
                bids: vec![(Money::from_cents(999), 100)],
                asks: Vec::new(),
            }),
        };
        let previous = RuntimeRestingProjectionCache {
            authority_epoch: 4,
            phase: TradingPhase::Continuous,
            entries: BTreeMap::from([(StockId(0), entry.clone())]),
        };
        assert!(cached_public_projection(
            Some(&previous),
            Some(&entry),
            &mirror,
            TradingPhase::ClosingAuction,
        )
        .is_none());

        let closing = snapshot::project_mirror_public_book(TradingPhase::ClosingAuction, &mirror);
        assert_eq!(
            closing.bids,
            (993..=999)
                .rev()
                .map(|price| (Money::from_cents(price), 100))
                .collect::<Vec<_>>(),
            "相位变化后仍完整投影七档且按买盘价格降序",
        );
    }
}

#[cfg(test)]
mod source_tests {
    use super::*;

    #[test]
    fn shared_retail_merge_keeps_group_authority_and_independent_candidates() {
        let mut setup = super::super::npc_working_quote_tests::two_stock_quote_setup();
        setup.npcs.retail_count = 1;
        setup.npcs.inst_count = 0;
        let mut game = GameSession::new(setup, 42).unwrap();
        let driver = RuntimeDriver::from_session(&mut game).unwrap();
        let mut snapshot = driver
            .collect_belief()
            .unwrap()
            .into_iter()
            .flat_map(|(_, rows)| rows)
            .flatten()
            .next()
            .expect("真实 Retail 完整个人态");
        let moment = crate::experience::ExperienceMoment {
            civil_date: game.civil_date(),
            market_minute: game.current_market_minute(),
            trading_day: u64::from(game.day()),
        };
        let mut group = RetailExperienceState::new(Money::from_cents(10_000)).unwrap();
        group
            .initialize_holding_dated(
                &StockId(0),
                Some(Money::from_cents(1_000)),
                Money::from_cents(1_000),
                moment,
            )
            .unwrap();
        let mut existing = RetailExperienceState::new(Money::from_cents(20_000)).unwrap();
        for code in 0..2 {
            let price = Money::from_cents(1_100 + i64::from(code));
            existing
                .initialize_holding_dated(&StockId(code), Some(price), price, moment)
                .unwrap();
            snapshot
                .watchlist
                .record_attention(&StockId(code), moment.market_minute, moment.market_minute)
                .unwrap();
            snapshot
                .price_memory
                .observe_price(&StockId(code), price, moment.market_minute)
                .unwrap();
        }
        snapshot.retail_experience = Some(group.clone());
        let original = SharedRuntimePersonal::from(snapshot.clone());
        let mut first = original.clone();
        let second = original.clone();
        first.merge_retail_from(Some(&existing));
        let mut expected = snapshot.clone();
        expected.retail_experience = Some(existing.clone().merged_with_group_authority(&group));
        assert_eq!(
            first.to_snapshot(),
            expected,
            "仅补镜像缺键，全部group字段保持权威"
        );
        assert_eq!(original.to_snapshot(), snapshot);
        assert_eq!(second.to_snapshot(), snapshot, "另一个候选不被merge写入");
        first.merge_retail_from(Some(&existing));
        assert_eq!(first.to_snapshot(), expected, "重复merge幂等");

        let mut unchanged = original.clone();
        unchanged.merge_retail_from(Some(&group));
        unchanged.prune_memory(&std::collections::BTreeSet::new());
        assert_eq!(
            unchanged.to_snapshot(),
            snapshot,
            "不缺键且小于原cap时完整个人值不变"
        );
        assert!(Arc::ptr_eq(&unchanged.participant, &original.participant));
        assert!(Arc::ptr_eq(
            unchanged.retail_experience.as_ref().unwrap(),
            original.retail_experience.as_ref().unwrap()
        ));
    }

    #[test]
    fn private_personal_prune_preserves_protection_order_and_failure_isolation() {
        let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
        let stock_template = setup.stocks[0].clone();
        setup.stocks.clear();
        for suffix in 0..12 {
            let mut spec = stock_template.clone();
            spec.code = crate::StockCode(format!("{:06}", 600_880 + suffix));
            setup.stocks.push(spec);
        }
        setup.start_date = crate::CivilDate::from_ymd(2026, 1, 5).unwrap();
        setup.ticks_per_day = 8;
        setup.auction_ticks = 0;
        setup.closing_auction_ticks = 0;
        let mut game = GameSession::new(setup, 42).unwrap();
        let owner = *game
            .state
            .belief_participants
            .keys()
            .next()
            .expect("真实信念机构名册非空");
        game.fixture_account_mut(&owner)
            .unwrap()
            .fixture_set_positions(std::collections::BTreeMap::from([(
                StockId(0),
                crate::account::Position::from_restored_parts(100, 0, 100_000, 0),
            )]));
        game.state
            .plans
            .create(crate::plans::PlanOpen {
                account: owner,
                code: StockId(1),
                direction: Side::Buy,
                target: crate::plans::PlanTarget::ShareCount(100),
                opinion: crate::plans::PlanOpinion {
                    signal_score_bp: 3000,
                    source: crate::plans::OpinionSource::Blended,
                },
                confidence_bp: 8000,
                urgency: crate::plans::Urgency::Normal,
                horizon_trading_days: 5,
                created_trading_day: 0,
            })
            .unwrap();

        let mut driver = RuntimeDriver::from_session(&mut game).unwrap();
        driver.adopt_session_seq(&game);
        driver.force_due_for_test_at(&[owner], 1);
        let captured_input = driver.prepare_driver_step(&game).unwrap();
        assert!(
            captured_input.sync_source.is_some(),
            "累计输入来自真实已提交拍"
        );
        let mut input = driver
            .force_full_prepared_input_for_test(captured_input.clone())
            .unwrap();
        assert!(
            input
                .plans
                .active_investor_codes(owner)
                .any(|code| *code == StockId(1)),
            "fixture 必须保留本人完整 active plan 的保护股"
        );
        let observed_minute = game.current_market_minute();
        let row = input
            .personals
            .iter_mut()
            .find(|(_, id, _)| *id == owner)
            .expect("完整 Full 输入含目标个人态");
        let global = row.0;
        let mut personal = row.2.to_snapshot();
        personal.watchlist = crate::experience::PersonalWatchlist::default();
        personal.price_memory = crate::experience::PersonalPriceMemory::default();
        for code in 0..12 {
            personal
                .watchlist
                .record_attention(&StockId(code), observed_minute, observed_minute)
                .unwrap();
            personal
                .price_memory
                .observe_price(
                    &StockId(code),
                    Money::from_cents(1_000 + i64::from(code)),
                    observed_minute,
                )
                .unwrap();
        }
        let original = personal.clone();
        row.2 = personal.into();
        // 无对手盘也要明确经过生产 touched 裁剪块，不能让 fixture 旁路该块。
        input.touched.push((global, owner));
        input.touched.sort();
        input.touched.dedup();
        let expected_protected = std::collections::BTreeSet::from([StockId(0), StockId(1)]);
        let mut reference =
            super::super::decision_chain::personal_state::BeliefParticipantState::new(
                original.watchlist.clone(),
                original.price_memory.clone(),
                original.information.clone(),
                original.belief.clone(),
            );
        reference.prune_memory(&expected_protected);
        let committed_before = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
        let cache_before = driver.administrative_cache_snapshot_for_test();
        let mut sync_before = captured_input.clone();
        driver
            .materialize_sync_mirror_inputs(&mut sync_before)
            .unwrap();
        assert!(
            sync_before.personals.iter().any(|(_, id, _)| *id == owner),
            "失败隔离守卫必须覆盖非空的本人累计个人输入"
        );
        let sync_personals_before: Vec<_> = sync_before
            .personals
            .iter()
            .map(|(index, id, value)| (*index, *id, value.to_snapshot()))
            .collect();

        let candidate = game
            .prepare_runtime_mirror(&mut driver, input.clone(), 1)
            .unwrap();
        let prepared = candidate.personals[&owner].1.to_snapshot();
        assert_eq!(
            prepared.watchlist,
            *reference.watchlist(),
            "真实 prepare 的结果与既有 prune_memory 逐值一致"
        );
        assert_eq!(
            prepared.price_memory,
            *reference.price_memory(),
            "价格记忆逐值与旧路径一致"
        );
        assert_eq!(
            prepared.information, original.information,
            "修剪不得改变 information"
        );
        assert_eq!(prepared.belief, original.belief, "修剪不得改变 belief");
        assert!(
            prepared.watchlist.stocks.contains_key(&StockId(0)),
            "持仓股票受保护"
        );
        assert!(
            prepared.watchlist.stocks.contains_key(&StockId(1)),
            "active plan 股票受保护"
        );
        assert_eq!(
            prepared.watchlist.latest_attention_minute, observed_minute,
            "修剪保留 watchlist 时钟"
        );
        assert_eq!(
            prepared.price_memory.stocks[&StockId(0)].last_observed_minute,
            observed_minute,
            "受保护条目的本人观察锚不变"
        );
        assert_eq!(
            prepared.price_memory.stocks[&StockId(0)].observed_high,
            Money::from_cents(1_000)
        );
        assert_eq!(
            prepared.price_memory.stocks[&StockId(0)].observed_low,
            Money::from_cents(1_000)
        );
        assert!(
            !prepared.watchlist.stocks.contains_key(&StockId(3)),
            "同分钟时较小 StockId 被稳定驱逐"
        );
        assert!(
            !prepared.watchlist.stocks.contains_key(&StockId(2)),
            "未保护超 8 项按分钟、StockId 排序裁剪"
        );
        assert!(
            !prepared.price_memory.stocks.contains_key(&StockId(2)),
            "价格记忆未保护超额项被裁剪"
        );
        assert!(
            !prepared.price_memory.stocks.contains_key(&StockId(3)),
            "价格记忆同分钟时较小 StockId 被稳定驱逐"
        );
        for code in 4..12 {
            assert!(prepared.watchlist.stocks.contains_key(&StockId(code)));
            assert!(prepared.price_memory.stocks.contains_key(&StockId(code)));
        }
        assert_eq!(
            serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
            committed_before,
            "prepare 只构造候选，不提前改写 committed Session"
        );
        assert_eq!(
            driver.administrative_cache_snapshot_for_test(),
            cache_before,
            "候选修剪不写入 driver 缓存"
        );

        game.runtime_install_failure = true;
        let error = match game.prepare_runtime_mirror(&mut driver, input, 1) {
            Ok(_) => panic!("后继私有 candidate 注入必须显式失败"),
            Err(error) => error,
        };
        assert_eq!(error, "安装失败注入（私有镜像 candidate 校验失败）");
        assert_eq!(
            serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
            committed_before,
            "后继 prepare 失败后 committed Session 保持权威原值"
        );
        assert_eq!(
            driver.administrative_cache_snapshot_for_test(),
            cache_before,
            "后继 prepare 失败不改 driver 缓存或 sync accumulator 摘要"
        );
        let mut sync_after = captured_input;
        driver
            .materialize_sync_mirror_inputs(&mut sync_after)
            .unwrap();
        assert_eq!(
            sync_after
                .personals
                .iter()
                .map(|(index, id, value)| (*index, *id, value.to_snapshot()))
                .collect::<Vec<_>>(),
            sync_personals_before,
            "后继 prepare 失败后重新物化的 sync accumulator 个人输入完整不变"
        );
        assert_eq!(
            sync_after.accounts, sync_before.accounts,
            "后继 prepare 失败后重新物化的 sync accumulator 账户输入完整不变"
        );
    }

    #[test]
    fn mirror_candidate_does_not_clone_replaced_shared_previous_values() {
        let mut setup = super::super::npc_working_quote_tests::retail_quote_setup();
        setup.npcs.inst_count = 1;
        setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
        setup.ticks_per_day = 8;
        setup.auction_ticks = 0;
        setup.closing_auction_ticks = 0;
        let mut game = GameSession::new(setup, 42).unwrap();
        let retail_account = *game
            .state
            .retail_experience
            .keys()
            .next()
            .expect("Retail 经历名册非空");
        // 历史已读账本按首次读取惰性创建；显式种入合法零读取态以覆盖替换路径。
        game.state.history_reads.insert_without_previous(
            retail_account,
            crate::experience::PersonalHistoryReadLedger::default(),
        );
        let mut driver = RuntimeDriver::from_session(&mut game).unwrap();
        driver.adopt_session_seq(&game);
        driver.force_due_for_test_at(&[retail_account], 1);
        let mut input = driver.prepare_driver_step(&game).unwrap();
        assert_eq!(
            input.calendar.market_phase_before(),
            Some(TradingPhase::Continuous),
            "fixture 使用真实连续竞价观察拍"
        );
        driver.materialize_sync_mirror_inputs(&mut input).unwrap();
        let observed_retail = input
            .personals
            .iter()
            .find(|(_, account, _)| *account == retail_account)
            .map(|(_, _, personal)| personal.clone())
            .expect("被真实 Continuous observe 更新的 Retail personal");
        assert!(observed_retail.retail_experience.is_some());
        let attention_before = game.state.npc_attention.clone();
        let beliefs_before = game.state.belief_participants.clone();
        let history_before = game.state.history_reads.clone();
        let retail_before = game.state.retail_experience.clone();
        let snapshot_before = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
        let cache_before = driver.administrative_cache_snapshot_for_test();
        let mut sync_before = input.clone();
        driver
            .materialize_sync_mirror_inputs(&mut sync_before)
            .unwrap();
        assert!(
            !sync_before.personals.is_empty(),
            "fixture 保留真实非空 sync accumulator 个人输入"
        );
        let sync_personals_before: Vec<_> = sync_before
            .personals
            .iter()
            .map(|(index, id, value)| (*index, *id, value.to_snapshot()))
            .collect();
        assert!(game.state.npc_attention.contains_key(&retail_account));
        assert!(game.state.belief_participants.contains_key(&retail_account));
        assert!(game.state.history_reads.contains_key(&retail_account));
        assert!(retail_before.contains_key(&retail_account));

        super::super::account_paged_map::reset_shared_previous_value_clones();
        let mut candidate = game
            .prepare_runtime_mirror(&mut driver, input.clone(), 1)
            .unwrap();
        let cloned_previous_values =
            super::super::account_paged_map::shared_previous_value_clones();
        assert_eq!(
            cloned_previous_values, 0,
            "生产 mirror 的四个稀疏域替换不消费 previous 值"
        );
        assert!(candidate.personals.contains_key(&retail_account));
        assert!(candidate.personals[&retail_account]
            .1
            .retail_experience
            .is_some());
        assert_eq!(game.state.npc_attention, attention_before);
        assert_eq!(game.state.belief_participants, beliefs_before);
        assert_eq!(game.state.history_reads, history_before);
        assert_eq!(game.state.retail_experience, retail_before);
        assert_eq!(
            serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
            snapshot_before,
            "准备 candidate 不得提前安装到 committed Session"
        );

        let account = retail_account;
        let global = candidate.personals[&account].0;
        let prepared_personal: BeliefParticipantSnapshot =
            candidate.personals[&account].1.to_snapshot();
        let price_code = *prepared_personal
            .price_memory
            .stocks
            .keys()
            .next()
            .expect("真实 Continuous observe 已建立价格记忆");
        let price_entry = &prepared_personal.price_memory.stocks[&price_code];
        let candidate_minute = price_entry.last_touched_minute;
        let history_minute = candidate_minute.max(
            prepared_personal
                .history_reads
                .stocks
                .get(&price_code)
                .map_or(0, |entry| entry.last_read_market_minute),
        );
        candidate
            .npc_attention
            .get_mut(&account)
            .unwrap()
            .next_attention_candidate_tick += 1;
        let belief = candidate.beliefs.get_mut(&account).unwrap();
        let price = price_entry.observed_high.add(Money::from_cents(1)).unwrap();
        belief
            .price_memory_mut()
            .observe_price(&price_code, price, candidate_minute)
            .unwrap();
        candidate
            .personal_history
            .get_mut(&account)
            .unwrap()
            .record(&price_code, history_minute)
            .unwrap();
        candidate
            .retail
            .get_mut(&account)
            .unwrap()
            .consecutive_failed_buys += 1;
        assert_eq!(
            candidate.personals[&account].1.to_snapshot(),
            prepared_personal,
            "candidate 四个可变表写入不改变同一完整个人快照"
        );
        assert_eq!(game.state.npc_attention, attention_before);
        assert_eq!(game.state.belief_participants, beliefs_before);
        assert_eq!(game.state.history_reads, history_before);
        assert_eq!(game.state.retail_experience, retail_before);
        assert_eq!(
            driver.administrative_cache_snapshot_for_test(),
            cache_before
        );
        assert_eq!(
            serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
            snapshot_before
        );
        let mut sync_after = input.clone();
        driver
            .materialize_sync_mirror_inputs(&mut sync_after)
            .unwrap();
        assert_eq!(
            sync_after
                .personals
                .iter()
                .map(|(index, id, value)| (*index, *id, value.to_snapshot()))
                .collect::<Vec<_>>(),
            sync_personals_before
        );
        assert_eq!(sync_after.accounts, sync_before.accounts);

        game.runtime_install_failure = true;
        super::super::account_paged_map::reset_shared_previous_value_clones();
        let failure = match game.prepare_runtime_mirror(&mut driver, input.clone(), 1) {
            Ok(_) => panic!("注入失败应拒绝镜像 candidate"),
            Err(error) => error,
        };
        assert_eq!(failure, "安装失败注入（私有镜像 candidate 校验失败）");
        assert_eq!(
            super::super::account_paged_map::shared_previous_value_clones(),
            0,
            "准备失败也不应克隆被替换的 previous 值"
        );
        assert_eq!(game.state.npc_attention, attention_before);
        assert_eq!(game.state.belief_participants, beliefs_before);
        assert_eq!(game.state.history_reads, history_before);
        assert_eq!(game.state.retail_experience, retail_before);
        assert_eq!(
            serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
            snapshot_before,
            "失败 candidate 不得半安装"
        );
        game.runtime_install_failure = false;

        drop(candidate);
        let normal = game
            .prepare_runtime_mirror(&mut driver, input.clone(), 1)
            .unwrap();
        let ack = game
            .validate_runtime_mirror_source(&driver, &normal)
            .unwrap();
        game.install_runtime_mirror(&mut driver, normal);
        driver.ack_prepared_mirror(ack);
        let installed_cache = driver.administrative_cache_snapshot_for_test();
        let other_owner = game
            .state
            .belief_participants
            .keys()
            .find(|id| **id != account)
            .copied()
            .expect("fixture 包含另一个真实 Inst owner");
        let other_personal = (
            game.state.npc_attention.get(&other_owner).cloned(),
            game.state.belief_participants.get(&other_owner).cloned(),
            game.state.history_reads.get(&other_owner).cloned(),
            game.state.retail_experience.get(&other_owner).cloned(),
        );
        game.state
            .npc_attention
            .get_mut(&account)
            .unwrap()
            .next_attention_candidate_tick += 1;
        let belief = game.state.belief_participants.get_mut(&account).unwrap();
        belief
            .price_memory_mut()
            .observe_price(&price_code, price, candidate_minute)
            .unwrap();
        game.state
            .history_reads
            .get_mut(&account)
            .unwrap()
            .record(&price_code, history_minute)
            .unwrap();
        game.state
            .retail_experience
            .get_mut(&account)
            .unwrap()
            .consecutive_failed_buys += 1;
        assert_eq!(
            (
                game.state.npc_attention.get(&other_owner).cloned(),
                game.state.belief_participants.get(&other_owner).cloned(),
                game.state.history_reads.get(&other_owner).cloned(),
                game.state.retail_experience.get(&other_owner).cloned(),
            ),
            other_personal,
            "另一 owner 四域完整值保持不变（None 能力位也保持 None）"
        );
        assert_eq!(
            driver.administrative_cache_snapshot_for_test(),
            installed_cache
        );
        let session_after_writes = (
            game.state.npc_attention.clone(),
            game.state.belief_participants.clone(),
            game.state.history_reads.clone(),
            game.state.retail_experience.clone(),
        );
        let mut collected = driver.collect_belief().unwrap();
        collected
            .iter_mut()
            .flat_map(|(_, rows)| rows.iter_mut())
            .nth(global)
            .and_then(Option::as_mut)
            .expect("collect_belief 返回目标完整个人态")
            .attention
            .next_attention_candidate_tick += 1;
        assert_eq!(
            driver.administrative_cache_snapshot_for_test(),
            installed_cache
        );
        assert_eq!(
            (
                game.state.npc_attention.clone(),
                game.state.belief_participants.clone(),
                game.state.history_reads.clone(),
                game.state.retail_experience.clone(),
            ),
            session_after_writes,
            "修改 collect_belief 的拥有 Snapshot 不改变 Session 四域"
        );
    }

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
