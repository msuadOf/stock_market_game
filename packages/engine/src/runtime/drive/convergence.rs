use super::super::type_aliases::GroupAccountShard;
use super::*;
use std::collections::BTreeMap;
use std::sync::Arc;

/// World 已发布后的只读投影输入；构造它不写 Session 镜像或消费完整读取。
#[derive(Clone)]
pub(crate) struct PreparedDriverStep {
    pub calendar: PreparedDriverCalendar,
    /// None 为自带 raw Full；Some 表示本拍个人态已 move 到该真实来源的累计表。
    pub sync_source: Option<crate::runtime::source::WorldMirrorSource>,
    pub accounts: Vec<GroupAccountShard>,
    pub buy_vwap: BTreeMap<
        (
            super::super::GroupIdx,
            super::super::LocalIdx,
            super::super::StockIdx,
        ),
        i64,
    >,
    pub last_prices: Vec<i64>,
    pub personals: Vec<(
        usize,
        AccountId,
        super::super::world::BeliefParticipantSnapshot,
    )>,
    pub plans: crate::plans::PlanBook,
    pub touched: Vec<(usize, AccountId)>,
    pub events: Vec<Event>,
    pub day_end: Option<PreparedDriverDayEnd>,
    pub emit_price_tick: bool,
    pub static_read: Option<super::PreparedStaticRead>,
}

/// 市场拍、静点 Full 与手动日终各自保留真实时间来源；后两者不构造市场拍。
#[derive(Clone)]
pub(crate) enum PreparedDriverCalendar {
    MarketTick(Arc<crate::runtime::MarketTickContext>),
    Static(Arc<crate::runtime::MarketDayContext>),
    ManualDayEnd(Arc<crate::runtime::MarketDayContext>),
}

impl PreparedDriverCalendar {
    pub(crate) fn day(&self) -> &Arc<crate::runtime::MarketDayContext> {
        match self {
            Self::MarketTick(tick) => tick.day(),
            Self::Static(day) | Self::ManualDayEnd(day) => day,
        }
    }

    pub(crate) fn market_phase_before(&self) -> Option<crate::TradingPhase> {
        match self {
            Self::MarketTick(tick) => Some(tick.phase_before()),
            Self::Static(_) | Self::ManualDayEnd(_) => None,
        }
    }

    pub(crate) fn stock_is_open(&self, code: crate::StockId) -> Result<bool, String> {
        match self {
            Self::MarketTick(tick) => Ok(tick
                .before_projection()
                .stock(code)
                .map_err(|error| error.to_string())?
                .date_session_ordinal()
                .is_some()),
            Self::Static(day) | Self::ManualDayEnd(day) => Ok(matches!(
                day.day_model()
                    .stock(code)
                    .map_err(|error| error.to_string())?
                    .status(),
                crate::calendar::DayStatus::Trading
            )),
        }
    }
}

impl PreparedDriverStep {
    /// 在 candidate 投影、seq 与 retained 发布前拒绝模式或同局时间来源错配。
    pub(crate) fn validate_calendar(
        &self,
        session: &GameSession,
        tick: u64,
        world_tick: u64,
    ) -> Result<(), String> {
        let valid_mode = match &self.calendar {
            PreparedDriverCalendar::Static(_) => {
                self.static_read.is_some()
                    && !self.emit_price_tick
                    && self.day_end.is_none()
                    && self.events.is_empty()
            }
            PreparedDriverCalendar::MarketTick(_) => {
                self.static_read.is_none()
                    && self.day_end.as_ref().is_none_or(|data| data.emit_boundary)
            }
            PreparedDriverCalendar::ManualDayEnd(_) => {
                self.static_read.is_none()
                    && !self.emit_price_tick
                    && self
                        .day_end
                        .as_ref()
                        .is_some_and(|data| !data.emit_boundary)
            }
        };
        if !valid_mode {
            return Err("镜像 Calendar 模式与静点/市场拍/手动日终输入不一致".to_owned());
        }
        let bindings = session
            .market_calendar_bindings()
            .map_err(|error| error.to_string())?;
        self.calendar
            .day()
            .validate_for(
                session.market_calendar_source(),
                &bindings,
                session.civil_date(),
            )
            .map_err(|error| error.to_string())?;
        match &self.calendar {
            PreparedDriverCalendar::MarketTick(calendar) => {
                let setup = session.export_setup_for_runtime();
                let base = calendar
                    .day()
                    .day_model()
                    .shared_sessions_before_date()
                    .checked_mul(setup.ticks_per_day)
                    .ok_or_else(|| "市场拍 Calendar 当日 tick 基线溢出".to_owned())?;
                let before_tick = tick
                    .checked_sub(1)
                    .ok_or_else(|| "市场拍 Calendar 缺少前一 tick".to_owned())?;
                // 延迟镜像只覆盖同一 civil date 的已真实提交输入，不跨日补拍或跳过 preflight。
                if tick != world_tick
                    || session.tick() < base
                    || session.tick() > before_tick
                    || u64::from(session.day())
                        != calendar.day().day_model().shared_sessions_before_date()
                {
                    return Err(
                        "市场拍 Calendar 不属于同日未安装输入窗口或当前 World 来源".to_owned()
                    );
                }
                let completed_ticks = before_tick
                    .checked_sub(base)
                    .ok_or_else(|| "市场拍 Calendar 早于当前 civil date".to_owned())?;
                calendar.validate_for(
                    session.market_calendar_source(),
                    &bindings,
                    session.civil_date(),
                    crate::session::exchange_calendar_model::SessionTickLayout {
                        ticks_per_day: setup.ticks_per_day,
                        auction_ticks: setup.auction_ticks,
                        closing_auction_ticks: setup.closing_auction_ticks,
                    },
                    crate::session::exchange_calendar_model::SharedClockInput {
                        tick: before_tick,
                        completed_shared_sessions: calendar.day().day_model().shared_sessions_before_date(),
                        progress: crate::session::exchange_calendar_model::SharedDayProgress::Trading {
                            completed_ticks,
                        },
                    },
                ).map_err(|error| error.to_string())?;
                if calendar.after_input().tick != tick {
                    return Err("市场拍 Calendar after tick 与镜像输入不一致".to_owned());
                }
            }
            PreparedDriverCalendar::Static(day) => {
                if tick != session.tick() {
                    return Err("非市场 Calendar 不得推进镜像 tick".to_owned());
                }
                session
                    .market_calendar_projection_for_day(day)
                    .map_err(|error| error.to_string())?;
            }
            PreparedDriverCalendar::ManualDayEnd(_) => {
                if tick != session.tick() {
                    return Err("非市场 Calendar 不得推进镜像 tick".to_owned());
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct PreparedDriverDayEnd {
    pub market_day: u32,
    pub emit_boundary: bool,
    pub closing_buy_vwap: BTreeMap<
        (
            super::super::GroupIdx,
            super::super::LocalIdx,
            super::super::StockIdx,
        ),
        i64,
    >,
    pub closing_events: Vec<Event>,
    pub candles: Vec<(crate::StockId, crate::session::DailyCandle)>,
    pub bars: Vec<Option<Vec<crate::runtime::MinuteBarFact>>>,
}

impl RuntimeDriver {
    /// 显式同步当前已提交静点，不推进市场 tick 或事件 seq。
    pub fn flush_now(&mut self, session: &mut GameSession) -> Result<(), String> {
        self.flush_candidate(session, crate::runtime::sync_policy::FlushTrigger::OnDemand)
    }

    /// 三档及计算消费点共用的静点汇合；trigger 只标识调用时机。
    pub(crate) fn flush_candidate(
        &mut self,
        session: &mut GameSession,
        trigger: crate::runtime::sync_policy::FlushTrigger,
    ) -> Result<(), String> {
        self.ensure_session_authority(session)?;
        session
            .require_healthy()
            .map_err(|fatal| fatal.to_string())?;
        let source = self.mirror_source();
        if source.civil_date != session.civil_date() {
            return Err(session
                .poison_runtime_step(crate::session::StepFatal::InvariantViolation {
                    location: "runtime::flush_candidate".to_owned(),
                    description: "World 与 Session civil date 不一致".to_owned(),
                })
                .to_string());
        }
        let origin = session.runtime_mirror_origin();
        let current_stage = match source.day_end_stage {
            crate::runtime::source::MirrorDayEndStage::Tick => {
                crate::RuntimeMirrorDayEndStage::Tick
            }
            crate::runtime::source::MirrorDayEndStage::DayEndCommitted => {
                crate::RuntimeMirrorDayEndStage::DayEndCommitted
            }
            crate::runtime::source::MirrorDayEndStage::RetainedPublished => {
                crate::RuntimeMirrorDayEndStage::RetainedPublished
            }
        };
        if origin.install_sequence > 0
            && origin.authority_epoch == source.authority_epoch
            && origin.source_tick == source.tick
            && origin.source_generation == source.generation
            && origin.non_tick_revision == source.non_tick_revision
            && origin.day_end_stage == current_stage
            && self.pending_mirror_domains().is_empty()
            && !self.sync_inputs.plans_pending()
        {
            return Ok(());
        }
        let result = (|| {
            let input = self.prepare_flush_driver_projection(session)?;
            let prepared = self.prepare_flush_candidate(session, input, trigger)?;
            if !prepared.events.is_empty() {
                return Err("静点 flush 不得产生市场事件".to_owned());
            }
            let ack = session
                .validate_runtime_mirror_source(self, &prepared)
                .map_err(|fatal| fatal.to_string())?;
            session.install_runtime_mirror(self, prepared);
            self.ack_prepared_mirror(ack);
            Ok(())
        })();
        result.map_err(|description| {
            session
                .poison_runtime_step(crate::session::StepFatal::InvariantViolation {
                    location: "runtime::flush_candidate".to_owned(),
                    description,
                })
                .to_string()
        })
    }

    /// 市场拍与静点均在同一prepare/validate/move安装实现上构造镜像。
    pub(crate) fn prepare_flush_candidate(
        &mut self,
        session: &GameSession,
        input: PreparedDriverStep,
        _trigger: crate::runtime::sync_policy::FlushTrigger,
    ) -> Result<crate::session::PreparedRuntimeMirror, String> {
        let tick = self.world_committed_tick();
        session.prepare_runtime_mirror(self, input, tick)
    }

    pub(crate) fn mirror_source(&self) -> crate::runtime::source::WorldMirrorSource {
        self.world.mirror_source()
    }

    pub(crate) fn abort_unpublished_day_end(&mut self) {
        self.world.abort_unpublished_day_end();
    }

    pub(crate) fn pending_mirror_domains(&self) -> crate::runtime::source::MirrorDomains {
        self.world.pending_mirror_domains()
    }

    pub(crate) fn prepare_mirror_ack(
        &self,
        source: crate::runtime::source::WorldMirrorSource,
        domains: crate::runtime::source::MirrorDomains,
    ) -> Result<crate::runtime::source::PreparedMirrorAck, String> {
        self.world
            .prepare_mirror_ack(source, domains)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn ack_prepared_mirror(&mut self, ack: crate::runtime::source::PreparedMirrorAck) {
        self.world.ack_prepared_mirror(ack);
    }
    pub(crate) fn collect_personal_full(
        &mut self,
    ) -> Result<crate::runtime::world::PersonalFullSnapshot, String> {
        self.world
            .collect_personal_full()
            .map_err(|error| format!("完整个人态收集失败：{error}"))
    }

    /// 事实与本批时间成对 move，空批保 None，不为手动日结补造市场拍。
    #[cfg(feature = "simulation-diagnostics")]
    pub(crate) fn take_drain_causal_facts(
        &mut self,
    ) -> (
        Vec<super::super::diagnostics::DeskCausalFact>,
        Option<crate::diagnostics::causal::FactTime>,
        BTreeMap<crate::StockId, u64>,
    ) {
        (
            std::mem::take(&mut self.world.drained_causal),
            self.world.drained_causal_time.take(),
            std::mem::take(&mut self.world.drained_causal_clocks),
        )
    }

    #[cfg(feature = "simulation-diagnostics")]
    pub(crate) fn retain_terminal_causal(
        &mut self,
        time: crate::diagnostics::causal::FactTime,
        raw: Vec<crate::runtime::diagnostics::DeskCausalFact>,
        fatal: crate::session::StepFatal,
    ) {
        if self.terminal_causal.is_none() {
            self.terminal_causal = Some((Some(time), raw, fatal));
        }
    }

    /// 来源本身缺失时只封存真实 raw 与首错；None 明示没有可承诺的时间。
    #[cfg(feature = "simulation-diagnostics")]
    pub(crate) fn retain_terminal_causal_without_time(
        &mut self,
        raw: Vec<crate::runtime::diagnostics::DeskCausalFact>,
        fatal: crate::session::StepFatal,
    ) {
        if self.terminal_causal.is_none() {
            self.terminal_causal = Some((None, raw, fatal));
        }
    }

    #[cfg(feature = "simulation-diagnostics")]
    pub(crate) fn retain_pending_causal_failure(
        &mut self,
        time: crate::diagnostics::causal::FactTime,
        fatal: crate::session::StepFatal,
    ) {
        let (raw, _, _) = self.take_causal_facts();
        self.retain_terminal_causal(time, raw, fatal);
    }

    #[cfg(all(test, feature = "simulation-diagnostics"))]
    pub(crate) fn terminal_causal_snapshot_for_test(
        &self,
    ) -> Option<(
        Option<crate::diagnostics::causal::FactTime>,
        Vec<crate::runtime::diagnostics::DeskCausalFact>,
        crate::session::StepFatal,
    )> {
        self.terminal_causal.clone()
    }

    pub(crate) fn cached_personal_at(
        &self,
        index: usize,
    ) -> Option<&super::super::world::BeliefParticipantSnapshot> {
        self.belief_cache.get(index).and_then(Option::as_ref)
    }

    pub(crate) fn install_cached_personal_at(
        &mut self,
        index: usize,
        account: AccountId,
        value: super::super::world::BeliefParticipantSnapshot,
    ) {
        assert_eq!(
            self.id_map[index], account,
            "candidate 已验证个人态真实名册"
        );
        self.belief_cache[index] = Some(value);
        #[cfg(test)]
        {
            self.personal_installs += 1;
        }
    }

    pub(crate) fn finish_mirror_install(&mut self, accounts: usize) {
        #[cfg(test)]
        {
            self.account_writes += accounts as u64;
        }
        #[cfg(not(test))]
        let _ = accounts;
        self.force_full_accounts = false;
        // 初始静点安装与 retained 重发布都不是真实市场拍；首个真实 tick 后才退役行情种子。
        if !self.world.report_log.is_empty() {
            self.initial_static_seed = None;
        }
        self.sync_inputs
            .complete_flush(crate::runtime::driver_sync::FlushOutcome::Successful);
        self.finish_tick_fact_install();
    }

    /// 事实已在当前成功提交移交；不清 mirror 覆盖表及 Full 原因。
    pub(crate) fn finish_tick_fact_install(&mut self) {
        #[cfg(feature = "simulation-diagnostics")]
        self.pending_decision_traces.clear();
        self.pending_retail.clear();
        self.pending_retail_rejections.clear();
        self.world.drained_retail.clear();
        self.pending_receipts.clear();
        self.pending_hold_inits.clear();
    }

    pub(crate) fn pending_trade_receipts(
        &self,
    ) -> &[crate::session::personal_trade_history::PendingTradeReceipt] {
        &self.pending_receipts
    }

    /// 只供 Session routed 入口在调用共享安装 core 前检查；direct driver 沿已有同 owner World 路径。
    pub(crate) fn require_staged_calendar_tick(&self) -> Result<(), String> {
        if self.world.staged_calendar_tick().is_none() {
            return Err("Session 市场拍缺少已验证的 staged Calendar".to_owned());
        }
        Ok(())
    }

    pub(crate) fn prepare_driver_step(
        &mut self,
        session: &GameSession,
    ) -> Result<PreparedDriverStep, String> {
        self.prepare_driver_step_with_day_end(session, true)
    }

    pub(crate) fn prepare_driver_step_with_day_end(
        &mut self,
        session: &GameSession,
        automatic_day_end: bool,
    ) -> Result<PreparedDriverStep, String> {
        self.ensure_session_authority(session)?;
        let next_tick = self
            .world_committed_tick()
            .checked_add(1)
            .ok_or_else(|| "driver sync 下一 tick 已耗尽".to_owned())?;
        let registrations = if automatic_day_end
            && next_tick.is_multiple_of(session.export_setup_for_runtime().ticks_per_day)
        {
            2
        } else {
            1
        };
        self.sync_source_seq
            .checked_add(registrations)
            .ok_or_else(|| "driver sync source_seq 在本拍完整登记前已耗尽".to_owned())?;
        self.world.set_civil_date(session.civil_date());
        self.world
            .set_tick_capture(crate::verification_evidence::current_phase_timing_binding());
        let result = self
            .prepare_step_with_capture(session, true)
            .and_then(|mut input| {
                let tick = self.world.global.committed_tick;
                if automatic_day_end
                    && tick.is_multiple_of(session.export_setup_for_runtime().ticks_per_day)
                {
                    input = self.prepare_day_end_input(session, input, true, true)?;
                }
                Ok(input)
            });
        self.world.clear_tick_capture();
        result
    }

    /// 统一生产日界与手动日结的只读输入，所有账户、个人及计划均来自同一日结静点。
    fn prepare_day_end_input(
        &mut self,
        session: &GameSession,
        mut input: PreparedDriverStep,
        advance_market_day: bool,
        emit_boundary: bool,
    ) -> Result<PreparedDriverStep, String> {
        let next_sync_seq = self.next_sync_source_sequence()?;
        let calendar = match &input.calendar {
            PreparedDriverCalendar::MarketTick(tick) => {
                super::super::protocol::DayEndCalendar::MarketTick(Arc::clone(tick))
            }
            PreparedDriverCalendar::ManualDayEnd(day) => {
                super::super::protocol::DayEndCalendar::ManualDayEnd(Arc::clone(day))
            }
            PreparedDriverCalendar::Static(_) => {
                return Err("静点 Calendar 不得发起 DayEnd".to_owned())
            }
        };
        self.world
            .run_day_end_with_calendar(calendar)
            .map_err(|error| format!("日结私有准备失败：{error}"))?;
        let candidate = self
            .world
            .last_day_end_candidate
            .as_ref()
            .ok_or_else(|| "日结成功缺完整账户 candidate".to_owned())?;
        let mut account_groups = vec![None; self.world.n_groups()];
        for (group, rows) in &candidate.groups {
            let slot = account_groups
                .get_mut(usize::from(*group))
                .ok_or_else(|| format!("日结账户组号越界：{group}"))?;
            if slot.is_some() {
                return Err(format!("日结账户重复组：{group}"));
            }
            *slot = Some(Arc::new(
                rows.iter()
                    .enumerate()
                    .map(|(local, account)| (local as _, Arc::new(account.clone())))
                    .collect(),
            ));
        }
        input.accounts = account_groups
            .into_iter()
            .enumerate()
            .map(|(group, rows)| rows.ok_or_else(|| format!("日结账户缺组：{group}")))
            .collect::<Result<_, _>>()?;
        let mut closing_sums: BTreeMap<_, (i128, u64)> = BTreeMap::new();
        for fact in &self.world.drained_day_trades {
            let entry = closing_sums
                .entry((fact.taker.0, fact.taker.1, fact.stock))
                .or_default();
            entry.0 += i128::from(fact.price) * i128::from(fact.qty);
            entry.1 += u64::from(fact.qty);
            for owner in [fact.maker, fact.taker] {
                let id = self
                    .account_by_owner(owner)
                    .ok_or_else(|| format!("收盘成交 owner 不在真实名册：{owner:?}"))?;
                input.touched.push((self.global_index(owner), id));
            }
        }
        let mut closing_buy_vwap = BTreeMap::new();
        for (key, (value, qty)) in closing_sums {
            if qty == 0 {
                return Err("收盘成交 VWAP 股数为零".to_owned());
            }
            let price = i64::try_from(value / i128::from(qty))
                .map_err(|_| "收盘成交 VWAP 超 i64".to_owned())?;
            if price <= 0 {
                return Err("收盘成交 VWAP 非正".to_owned());
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = input.buy_vwap.entry(key) {
                entry.insert(price);
                closing_buy_vwap.insert(key, price);
            }
        }
        input.touched.sort();
        input.touched.dedup();
        let personals = self
            .world
            .collect_personal_full()
            .map_err(|error| error.to_string())?;
        input.personals = personals
            .groups
            .into_iter()
            .flat_map(|(group, rows)| {
                let per = self.accounts_per_group;
                rows.into_iter().filter_map(move |(local, account, value)| {
                    value.map(|value| (usize::from(group) * per + local as usize, account, value))
                })
            })
            .collect();
        input.plans = super::super::plan_ownership::merge(
            self.world
                .collect_plans()
                .map_err(|error| error.to_string())?,
            &self.id_map,
            self.world.n_groups(),
            self.accounts_per_group,
            self.plan_policy,
        )
        .map_err(|error| error.to_string())?;
        let receipts = self.collect_drained_events(session.civil_date())?;
        self.pending_receipts.extend(receipts);
        let closing_events = self.take_drained_auction_events();
        let candles = self.take_drained_day_candles(session)?;
        let bars = self.world.take_pending_day_bars();
        let market_day = if advance_market_day {
            session
                .day()
                .checked_add(1)
                .ok_or_else(|| "交易日计数溢出".to_owned())?
        } else {
            session.day()
        };
        input.day_end = Some(PreparedDriverDayEnd {
            market_day,
            emit_boundary,
            closing_buy_vwap,
            closing_events,
            candles,
            bars,
        });
        self.capture_sync_inputs(&mut input, next_sync_seq)?;
        Ok(input)
    }

    #[cfg(test)]
    pub(crate) fn force_full_prepared_input_for_test(
        &mut self,
        mut input: PreparedDriverStep,
    ) -> Result<PreparedDriverStep, String> {
        let mut groups = self
            .world
            .snapshot_accounts()
            .map_err(|error| error.to_string())?;
        groups.sort_by_key(|(group, _)| *group);
        input.accounts = groups
            .into_iter()
            .map(|(_, rows)| {
                Arc::new(
                    rows.into_iter()
                        .enumerate()
                        .map(|(local, account)| (local as _, Arc::new(account)))
                        .collect(),
                )
            })
            .collect();
        input.personals = self
            .world
            .collect_personal_full()
            .map_err(|error| error.to_string())?
            .groups
            .into_iter()
            .flat_map(|(group, rows)| {
                let per = self.accounts_per_group;
                rows.into_iter().filter_map(move |(local, id, value)| {
                    value.map(|value| (usize::from(group) * per + local as usize, id, value))
                })
            })
            .collect();
        input.plans = super::super::plan_ownership::merge(
            self.world
                .collect_plans()
                .map_err(|error| error.to_string())?,
            &self.id_map,
            self.world.n_groups(),
            self.accounts_per_group,
            self.plan_policy,
        )
        .map_err(|error| error.to_string())?;
        // 独立完整 getter 全部成功后保留其 raw 值，不再用累计表覆盖对照臂。
        input.sync_source = None;
        Ok(input)
    }

    /// 手动 API 不重跑 tick；仅将已提交市场日结组为同一私有镜像 candidate。
    pub(super) fn install_manual_day_end(
        &mut self,
        session: &mut GameSession,
        advance_market_day: bool,
    ) -> Result<(), String> {
        self.ensure_session_authority(session)?;
        session
            .require_healthy()
            .map_err(|fatal| fatal.to_string())?;
        let calendar_day = session
            .prepare_market_calendar_day()
            .map_err(|error| error.to_string())?;
        self.next_sync_source_sequence()?;
        self.world.set_civil_date(session.civil_date());
        let input = PreparedDriverStep {
            calendar: PreparedDriverCalendar::ManualDayEnd(calendar_day),
            sync_source: None,
            accounts: Vec::new(),
            buy_vwap: BTreeMap::new(),
            last_prices: self
                .world_public_views()
                .iter()
                .map(|view| view.last_price)
                .collect(),
            personals: Vec::new(),
            plans: crate::plans::PlanBook::new(self.plan_policy)
                .map_err(|error| error.to_string())?,
            touched: Vec::new(),
            events: Vec::new(),
            day_end: None,
            emit_price_tick: false,
            static_read: None,
        };
        // World 的日结 validator 拒绝仍由自身有效 Abort 窗处理；成功后投影失败不能重跑。
        let input = self
            .prepare_day_end_input(session, input, advance_market_day, false)
            .map_err(|description| {
                if self.world.last_day_end_candidate.is_some() {
                    let fatal = crate::session::StepFatal::InvariantViolation {
                        location: "RuntimeDriver::day_end::collect".to_owned(),
                        description,
                    };
                    #[cfg(feature = "simulation-diagnostics")]
                    {
                        let (raw, time, _) = self.take_drain_causal_facts();
                        if !raw.is_empty() {
                            match time {
                                Some(time) => self.retain_terminal_causal(time, raw, fatal.clone()),
                                None => {
                                    self.retain_terminal_causal_without_time(raw, fatal.clone())
                                }
                            }
                        }
                    }
                    session.poison_runtime_step(fatal).to_string()
                } else {
                    description
                }
            })?;
        let prepared = self
            .prepare_flush_candidate(
                session,
                input,
                crate::runtime::sync_policy::FlushTrigger::DayEnd,
            )
            .map_err(|description| {
                let fatal = crate::session::StepFatal::InvariantViolation {
                    location: "RuntimeDriver::day_end::prepare".to_owned(),
                    description,
                };
                #[cfg(feature = "simulation-diagnostics")]
                {
                    let (raw, time, _) = self.take_drain_causal_facts();
                    if !raw.is_empty() {
                        match time {
                            Some(time) => self.retain_terminal_causal(time, raw, fatal.clone()),
                            None => self.retain_terminal_causal_without_time(raw, fatal.clone()),
                        }
                    }
                }
                self.abort_unpublished_day_end();
                session.poison_runtime_step(fatal).to_string()
            })?;
        #[cfg(feature = "simulation-diagnostics")]
        let causal = session
            .prepare_runtime_drain_causal(self, prepared.causal_plans())
            .map_err(|fatal| session.poison_runtime_step(fatal).to_string())?;
        let ack = match session.validate_runtime_mirror_source(self, &prepared) {
            Ok(ack) => ack,
            Err(fatal) => {
                #[cfg(feature = "simulation-diagnostics")]
                if let Some(causal) = causal {
                    self.retain_terminal_causal(causal.time, causal.raw, fatal.clone());
                }
                return Err(session.poison_runtime_step(fatal).to_string());
            }
        };
        let events = session.install_runtime_mirror(self, prepared);
        self.ack_prepared_mirror(ack);
        self.drained_events.extend(events);
        if let Err(error) = session.end_civil_day() {
            if matches!(&error, crate::session::SessionError::ReportCorrectionDayEnd(cause)
                if matches!(cause.classification(),
                    crate::company::CompanyErrorClass::InvalidInput
                    | crate::company::CompanyErrorClass::BusinessCondition))
                && session.poison_reason().is_none()
                && !self.administrative_world_is_terminal()
            {
                // 市场收盘已经提交；明确业务拒绝只保留 civil 重试，不毒化健康 authority。
                #[cfg(feature = "simulation-diagnostics")]
                if let Some(causal) = causal {
                    session.publish_runtime_causal(causal);
                }
                return Err(format!("市场日结安装后的自然日推进失败：{error}"));
            }
            let fatal = match session.poison_reason() {
                Some(first) => first.clone(),
                None => match error {
                    crate::session::SessionError::Step(fatal) => fatal,
                    error => crate::session::StepFatal::InvariantViolation {
                        location: "RuntimeDriver::day_end::civil".to_owned(),
                        description: format!("市场日结安装后的自然日推进失败：{error}"),
                    },
                },
            };
            #[cfg(feature = "simulation-diagnostics")]
            if let Some(causal) = causal {
                self.retain_terminal_causal(causal.time, causal.raw, fatal.clone());
            }
            return Err(session.poison_runtime_step(fatal).to_string());
        }
        #[cfg(feature = "simulation-diagnostics")]
        if let Some(causal) = causal {
            session.publish_runtime_causal(causal);
        }
        self.world.set_civil_date(session.civil_date());
        Ok(())
    }

    pub(crate) fn resting_snapshot_for_prepared(
        &mut self,
        day_end: bool,
    ) -> Result<super::super::type_aliases::RestingSessionSnapshot, String> {
        if day_end {
            let facts = self.collect_resting_facts_live()?;
            Ok((
                facts.into_iter().map(Arc::new).collect(),
                Arc::clone(&self.id_map_shared),
            ))
        } else {
            self.resting_snapshot_for_session()
        }
    }

    pub(crate) fn publish_prepared_retained(
        &mut self,
        snapshot: Arc<crate::runtime::publication::RetainedDaySnapshot>,
    ) {
        self.world.freeze_retained_day(snapshot);
    }
}

impl RuntimeDriver {
    #[cfg(test)]
    pub(crate) fn record_root_handover(session: &GameSession, tick_after: u64, has_events: bool) {
        let probes = &session.compute_context().t1_probes;
        let handover_seq = probes
            .root_handover_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        // 交付槽消费（同计数域：完成=奇数位、交付=紧邻偶数位
        // ——旧管线交付侧 fetch；runtime 交付即事件返回本身，
        // 槽位仍须成对推进以保奇偶语义）。
        let _delivery_slot = probes
            .root_handover_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let attempt = probes
            .queue_attempt_seq
            .load(std::sync::atomic::Ordering::SeqCst);
        let roots = usize::from(has_events);
        let mut trace = probes
            .root_handover_trace
            .lock()
            .expect("root 交接见证锁被 poison");
        let capture_ordinal = trace.iter().filter(|r| r.tick == tick_after).count() as u32 + 1;
        let backtrace = if capture_ordinal >= 2 && trace.iter().all(|r| r.backtrace.is_none()) {
            Some(format!("{:?}", std::backtrace::Backtrace::force_capture()))
        } else {
            None
        };
        trace.push(crate::compute::RootHandoverRecord {
            // 旧口径：记录 tick=消费 tick（步进前值——交接
            // 发生在被消费 tick 的执行相位）。
            tick: tick_after - 1,
            attempt,
            roots,
            handover_seq,
            capture_ordinal,
            backtrace,
        });
    }
}
