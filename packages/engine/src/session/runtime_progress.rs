//! 延迟镜像时仍逐拍准备并交付事实；覆盖表只保最新整态，不接管事件历史。
use super::*;
use crate::runtime::drive::{PreparedDriverStep, RuntimeDriver};
use crate::runtime::source::WorldMirrorSource;

pub(crate) enum PreparedRuntimeCommit {
    Mirror(Box<super::runtime_mirror::PreparedRuntimeMirror>),
    Progress(Box<PreparedRuntimeProgress>),
}

pub(crate) struct PreparedRuntimeProgress {
    source: WorldMirrorSource,
    projection: protocol::PreparedTickProjection,
    events: Vec<Event>,
    _plans: crate::plans::PlanBook,
    _calendar: crate::runtime::drive::PreparedDriverCalendar,
    receipts: BTreeMap<AccountId, crate::experience::AppendOnlyHistory<PersonalTradeConfirmation>>,
    receipt_watermark: u64,
    retail: super::retail_lifecycle::Candidate,
    intraday_library: Option<std::sync::Arc<crate::information::PublicLibrary>>,
    intraday_dispatches: Option<super::disclosures::DisclosureDispatch>,
    information_check_floor: Option<crate::calendar::CivilInstant>,
    #[cfg(feature = "simulation-diagnostics")]
    traces: crate::diagnostics::decision_trace::NpcDecisionTraceCollector,
}

impl PreparedRuntimeCommit {
    pub(super) fn projection(&self) -> &protocol::PreparedTickProjection {
        match self {
            Self::Mirror(value) => &value.projection,
            Self::Progress(value) => &value.projection,
        }
    }
    #[cfg(test)]
    pub(crate) fn events(&self) -> &[Event] {
        match self {
            Self::Mirror(value) => &value.events,
            Self::Progress(value) => &value.events,
        }
    }
    pub(crate) fn take_events(&mut self) -> Vec<Event> {
        match self {
            Self::Mirror(value) => std::mem::take(&mut value.events),
            Self::Progress(value) => std::mem::take(&mut value.events),
        }
    }
    pub(crate) fn has_intraday_publication(&self) -> bool {
        match self {
            Self::Mirror(value) => value.has_intraday_publication(),
            Self::Progress(value) => value.intraday_library.is_some(),
        }
    }
    #[cfg(feature = "simulation-diagnostics")]
    pub(crate) fn plans(&self) -> &crate::plans::PlanBook {
        match self {
            Self::Mirror(value) => value.causal_plans(),
            Self::Progress(value) => &value._plans,
        }
    }
    #[cfg(feature = "simulation-diagnostics")]
    pub(crate) fn calendar(&self) -> &crate::runtime::drive::PreparedDriverCalendar {
        match self {
            Self::Mirror(value) => &value.causal_calendar,
            Self::Progress(value) => &value._calendar,
        }
    }
}

impl GameSession {
    /// 诊断读面标识实际安装来源；首次延迟提交时冻结初始镜像来源。
    pub fn runtime_mirror_origin(&self) -> crate::RuntimeMirrorOrigin {
        self.runtime_mirror_origin
            .unwrap_or(crate::RuntimeMirrorOrigin {
                authority_epoch: self.runtime_authority_epoch,
                install_sequence: self.runtime_mirror_install_sequence,
                source_tick: self.tick(),
                source_generation: 0,
                non_tick_revision: 0,
                day_end_stage: crate::RuntimeMirrorDayEndStage::Tick,
                mirror_phase: self.phase(),
            })
    }
    pub(crate) fn runtime_mirror_phase(&self) -> TradingPhase {
        self.runtime_mirror_origin().mirror_phase
    }

    pub(super) fn require_player_ingress_policy(&self) -> Result<(), SessionError> {
        self.require_healthy()?;
        if !self.state.setup.runtime_sync_policy.allows_player_ingress() {
            return Err(SessionError::SyncPolicyViolation(
                crate::error::ValidationFault::new(
                    crate::error::RUNTIME_SYNC_POLICY_VIOLATION,
                    vec![1, self.tick(), self.runtime_publication_generation],
                ),
            ));
        }
        Ok(())
    }

    pub(crate) fn origin_from_runtime_source(
        &self,
        source: WorldMirrorSource,
        sequence: u64,
        phase: TradingPhase,
    ) -> Result<crate::RuntimeMirrorOrigin, String> {
        let origin = crate::RuntimeMirrorOrigin {
            authority_epoch: source.authority_epoch,
            install_sequence: sequence,
            source_tick: source.tick,
            source_generation: source.generation,
            non_tick_revision: source.non_tick_revision,
            mirror_phase: phase,
            day_end_stage: match source.day_end_stage {
                crate::runtime::source::MirrorDayEndStage::Tick => {
                    crate::RuntimeMirrorDayEndStage::Tick
                }
                crate::runtime::source::MirrorDayEndStage::DayEndCommitted => {
                    crate::RuntimeMirrorDayEndStage::DayEndCommitted
                }
                crate::runtime::source::MirrorDayEndStage::RetainedPublished => {
                    crate::RuntimeMirrorDayEndStage::RetainedPublished
                }
            },
        };
        origin.validate()?;
        Ok(origin)
    }

    pub(crate) fn prepare_runtime_publication(
        &self,
        driver: &mut RuntimeDriver,
        input: PreparedDriverStep,
        tick: u64,
    ) -> Result<PreparedRuntimeCommit, String> {
        if self.state.setup.runtime_sync_policy.needs_tick_flush() || input.day_end.is_some() {
            let trigger = if input.day_end.is_some() {
                crate::runtime::sync_policy::FlushTrigger::DayEnd
            } else {
                crate::runtime::sync_policy::FlushTrigger::EveryTick
            };
            return driver
                .prepare_flush_candidate(self, input, trigger)
                .map(|value| PreparedRuntimeCommit::Mirror(Box::new(value)));
        }
        input.validate_calendar(self, tick, driver.world_committed_tick())?;
        let source = driver.mirror_source();
        if source.civil_date != self.civil_date()
            || source.authority_epoch != self.runtime_authority_epoch
        {
            return Err("逐拍事实来源与 Session authority/civil date 不一致".to_owned());
        }
        let published = driver.publication().load();
        if published.tick != tick || published.generation != source.generation {
            return Err("逐拍事实与当前发布版本不一致".to_owned());
        }
        let mut current_markets = BTreeMap::new();
        let mut active = BTreeMap::new();
        let mut events = input.events;
        for (index, view) in driver.world_public_views().iter().enumerate() {
            let code = StockId(index as u32);
            if !input.calendar.stock_is_open(code)? {
                continue;
            }
            if view.last_price <= 0 {
                return Err(format!("逐拍事实价格非正：{code:?}"));
            }
            let last = Money::from_cents(view.observation_price());
            let candle = match view.daily_active {
                Some((open, high, low, close, volume)) => DailyCandle {
                    time: self.candle_date_time_for_runtime(),
                    open: Money::from_cents(open),
                    high: Money::from_cents(high),
                    low: Money::from_cents(low),
                    close: Money::from_cents(close),
                    volume,
                    trade_stats: Some(DailyTradeStats {
                        turnover_cents: u64::try_from(view.daily_active_turnover_cents)
                            .map_err(|_| "逐拍成交额为负")?,
                        trade_count: view.daily_active_trade_count,
                    }),
                },
                None => DailyCandle {
                    time: self.candle_date_time_for_runtime(),
                    open: last,
                    high: last,
                    low: last,
                    close: last,
                    volume: 0,
                    trade_stats: Some(Default::default()),
                },
            };
            let bids: Vec<_> = view
                .depth_bids
                .iter()
                .map(|(price, qty)| (Money::from_cents(*price), *qty))
                .collect();
            let asks: Vec<_> = view
                .depth_asks
                .iter()
                .map(|(price, qty)| (Money::from_cents(*price), *qty))
                .collect();
            current_markets.insert(
                code,
                MarketSnap {
                    last_price: last,
                    last_close: self.state.markets[&code].last_close(),
                    best_bid: bids.first().map(|row| row.0),
                    best_ask: asks.first().map(|row| row.0),
                    bids: bids.clone(),
                    asks: asks.clone(),
                },
            );
            let day_tick = tick % self.state.setup.ticks_per_day;
            let before_open_match = day_tick != 0
                && day_tick < self.state.setup.auction_ticks - self.state.setup.auction_ticks / 3;
            if !before_open_match {
                active.insert(code, candle.clone());
            }
            if input.emit_price_tick
                && input.calendar.market_phase_before() == Some(TradingPhase::Continuous)
            {
                // driver 游标仅在全部当前股读面捕获后分配，避免与不可变 World 借用交叉。
                events.push(Event::PriceTick {
                    seq: 0,
                    tick,
                    code,
                    last_price: last,
                    daily_candle: candle,
                    bids,
                    asks,
                });
            }
        }
        for event in &mut events {
            if let Event::PriceTick { seq, .. } = event {
                if *seq == 0 {
                    *seq = driver.take_event_seq();
                }
            }
        }
        let intraday_now = match &input.calendar {
            crate::runtime::drive::PreparedDriverCalendar::MarketTick(tick) => {
                Some(tick.observation_instant_after())
            }
            _ => None,
        };
        let previous_floor = self.information_check_floor();
        let (information_due, information_check_floor) =
            match intraday_now.filter(|now| previous_floor.is_none_or(|floor| *now >= floor)) {
                Some(now) => {
                    let due = self.has_due_non_immediate_information_check(now);
                    let floor = if due && !driver.has_information_delivery_baseline() {
                        GameSession::no_information_check_instant()
                    } else {
                        self.scan_information_check_floor()
                            .unwrap_or_else(GameSession::no_information_check_instant)
                    };
                    (due, Some(floor))
                }
                None => (false, previous_floor),
            };
        driver.set_information_due_after_prepare(information_due);
        let intraday_candidate = intraday_now
            .map(|now| self.prepare_intraday_disclosure_candidate(now))
            .transpose()
            .map_err(|error| format!("盘中月报候选准备失败：{error}"))?
            .flatten();
        let (intraday_library, intraday_dispatches) = if let Some(candidate) = intraday_candidate {
            let mut publications = candidate.publications;
            publications.sort_by_key(|publication| (publication.published_at, publication.id));
            for publication in publications {
                let super::disclosures::candidate::DisclosurePublicationKind::Report {
                    version,
                    ..
                } = publication.kind
                else {
                    return Err("盘中月报候选不得包含公告".to_owned());
                };
                events.push(Event::CompanyDisclosurePublished {
                    seq: driver.take_event_seq(),
                    publication_id: publication.id,
                    company: publication.company,
                    published_at: publication.published_at,
                    kind: CompanyDisclosureKind::Report {
                        report_revision: version.sequence,
                    },
                });
            }
            (
                Some(std::sync::Arc::new(candidate.library)),
                Some(candidate.dispatch),
            )
        } else {
            (None, None)
        };
        let seq = driver.next_seq();
        let mut snapshot = self.runtime_snapshot();
        snapshot.tick = tick;
        snapshot.seq = seq;
        snapshot.phase = self.phase_at_tick(tick);
        let runtime = Self::public_runtime_from_prepared(&snapshot, self.player_working_orders())
            .map_err(|error| error.to_string())?;
        // TickFrame 的时序事实保持当前；账户及簿读面仍标上次 mirror_origin。
        snapshot.markets = current_markets;
        snapshot.active_daily_candles = active;
        let (receipts, receipt_watermark) = self.prepare_runtime_receipts(driver)?;
        let mut retail = super::retail_lifecycle::Candidate {
            events: Vec::new(),
            decisions: driver
                .collect_retail_traces()
                .map_err(|error| error.to_string())?
                .into_iter()
                .flatten()
                .collect(),
        };
        let (facts, rejections) = driver.pending_retail_facts();
        self.append_runtime_retail_facts_at(driver, &mut retail, facts, tick)
            .map_err(|error| error.to_string())?;
        self.append_runtime_retail_rejections(&mut retail, rejections)
            .map_err(|error| error.to_string())?;
        #[cfg(feature = "simulation-diagnostics")]
        let traces = self.prepare_runtime_decision_traces(driver, tick, &input.plans, &events)?;
        #[cfg(test)]
        if self.runtime_install_failure {
            return Err("安装失败注入（逐拍事实 candidate 校验失败）".to_owned());
        }
        Ok(PreparedRuntimeCommit::Progress(Box::new(
            PreparedRuntimeProgress {
                source,
                projection: protocol::PreparedTickProjection { snapshot, runtime },
                events,
                _plans: input.plans,
                _calendar: input.calendar,
                receipts,
                receipt_watermark,
                retail,
                intraday_library,
                intraday_dispatches,
                information_check_floor,
                #[cfg(feature = "simulation-diagnostics")]
                traces,
            },
        )))
    }

    pub(crate) fn prepare_runtime_receipts(
        &self,
        driver: &RuntimeDriver,
    ) -> Result<
        (
            BTreeMap<AccountId, crate::experience::AppendOnlyHistory<PersonalTradeConfirmation>>,
            u64,
        ),
        String,
    > {
        let mut receipts: BTreeMap<
            AccountId,
            crate::experience::AppendOnlyHistory<PersonalTradeConfirmation>,
        > = BTreeMap::new();
        let mut watermark = self.state.receipt_watermark;
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
                    "个人确认 candidate receipt_id 回退或重复：{:?}",
                    pending.account
                ));
            }
            watermark = watermark.max(
                pending
                    .confirmation
                    .receipt_id
                    .checked_add(1)
                    .ok_or("个人确认 candidate 水位溢出")?,
            );
            history.push(pending.confirmation.clone());
        }
        Ok((receipts, watermark))
    }

    pub(crate) fn validate_runtime_publication(
        &self,
        driver: &RuntimeDriver,
        prepared: &PreparedRuntimeCommit,
    ) -> Result<Option<crate::runtime::source::PreparedMirrorAck>, StepFatal> {
        match prepared {
            PreparedRuntimeCommit::Mirror(value) => self
                .validate_runtime_mirror_source(driver, value.as_ref())
                .map(Some),
            PreparedRuntimeCommit::Progress(value) => {
                if value.source != driver.mirror_source()
                    || value.source.authority_epoch != self.runtime_authority_epoch
                {
                    return Err(StepFatal::InvariantViolation {
                        location: "runtime_progress::source".to_owned(),
                        description: "逐拍事实 candidate 来源已改变".to_owned(),
                    });
                }
                Ok(None)
            }
        }
    }

    pub(crate) fn install_runtime_publication(
        &mut self,
        driver: &mut RuntimeDriver,
        prepared: PreparedRuntimeCommit,
    ) {
        match prepared {
            PreparedRuntimeCommit::Mirror(value) => {
                self.install_runtime_mirror(driver, *value);
            }
            PreparedRuntimeCommit::Progress(value) => {
                let value = *value;
                if self.runtime_mirror_origin.is_none() {
                    self.runtime_mirror_origin = Some(value.projection.snapshot.mirror_origin);
                }
                self.state.tick = value.projection.snapshot.tick;
                self.state.seq = value.projection.snapshot.seq;
                self.runtime_publication_generation = value.source.generation;
                for (account, history) in value.receipts {
                    self.state
                        .personal_trade_confirmations
                        .insert(account, history);
                }
                self.state.receipt_watermark = value.receipt_watermark;
                if let Some(library) = value.intraday_library {
                    self.state.library = library;
                }
                if let Some(dispatches) = value.intraday_dispatches {
                    self.state.disclosures = dispatches;
                }
                self.set_information_check_floor(value.information_check_floor);
                self.publish_runtime_retail(value.retail);
                #[cfg(feature = "simulation-diagnostics")]
                {
                    self.state.npc_decision_traces = value.traces;
                }
                driver.finish_tick_fact_install();
            }
        }
    }
}

#[cfg(test)]
mod transport_layout_tests {
    #[test]
    fn runtime_commit_envelope_stays_within_transport_budget() {
        let size = std::mem::size_of::<super::PreparedRuntimeCommit>();
        assert!(
            size <= 128,
            "PreparedRuntimeCommit 占 {size} 字节，超过 128 字节运输预算"
        );
    }
}
