use super::*;
use std::collections::BTreeMap;
use std::sync::Arc;

/// World 已发布后的只读投影输入；构造它不写 Session 镜像或消费完整读取。
#[derive(Clone)]
pub(crate) struct PreparedDriverStep {
    pub accounts: Vec<
        Arc<
            Vec<(
                super::super::LocalIdx,
                Arc<super::super::group::GroupAccount>,
            )>,
        >,
    >,
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

    #[cfg(feature = "simulation-diagnostics")]
    pub(crate) fn retain_terminal_causal(
        &mut self,
        time: crate::diagnostics::causal::FactTime,
        raw: Vec<crate::runtime::diagnostics::DeskCausalFact>,
        fatal: crate::session::StepFatal,
    ) {
        if self.terminal_causal.is_none() {
            self.terminal_causal = Some((time, raw, fatal));
        }
    }

    #[cfg(feature = "simulation-diagnostics")]
    pub(crate) fn retain_pending_causal_failure(
        &mut self,
        time: crate::diagnostics::causal::FactTime,
        fatal: crate::session::StepFatal,
    ) {
        let raw = self.take_causal_facts();
        self.retain_terminal_causal(time, raw, fatal);
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
        self.pending_receipts.clear();
        self.pending_hold_inits.clear();
    }

    pub(crate) fn pending_trade_receipts(
        &self,
    ) -> &[crate::session::personal_trade_history::PendingTradeReceipt] {
        &self.pending_receipts
    }

    /// 兼容直接 driver 调用；生产聚合入口在全部域准备成功后才调用安装。
    pub(super) fn install_driver_projection(
        &mut self,
        session: &mut GameSession,
        prepared: PreparedDriverStep,
    ) -> Result<Vec<Event>, String> {
        self.install_account_projection(
            session,
            prepared
                .accounts
                .iter()
                .enumerate()
                .flat_map(|(group, rows)| {
                    rows.iter()
                        .map(move |(local, account)| (group, *local, account.as_ref()))
                }),
            &prepared.buy_vwap,
            &prepared.last_prices,
        );
        for (global, id, snapshot) in prepared.personals {
            session.set_runtime_personal_state(id, &snapshot);
            self.belief_cache[global] = Some(snapshot);
            #[cfg(test)]
            {
                self.personal_installs += 1;
            }
        }
        self.install_pending_hold_inits(session, None)?;
        session.set_runtime_plans(prepared.plans);
        for (_, account) in prepared.touched {
            session.runtime_prune_personal_memory(account);
        }
        self.force_full_accounts = false;
        Ok(prepared.events)
    }

    pub(crate) fn prepare_driver_step(
        &mut self,
        session: &GameSession,
    ) -> Result<PreparedDriverStep, String> {
        self.world.set_civil_date(session.civil_date());
        self.world
            .set_tick_capture(crate::verification_evidence::current_phase_timing_binding());
        let result = self
            .prepare_step_with_capture(session)
            .and_then(|mut input| {
                let tick = self.world.global.committed_tick;
                if tick.is_multiple_of(session.export_setup_for_runtime().ticks_per_day) {
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
        #[cfg(feature = "simulation-diagnostics")]
        self.ensure_causal_day_end_supported()?;
        self.world
            .run_day_end_public()
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
            if !input.buy_vwap.contains_key(&key) {
                input.buy_vwap.insert(key, price);
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
        Ok(input)
    }

    /// 手动 API 不重跑 tick；仅将已提交市场日结组为同一私有镜像 candidate。
    pub(super) fn install_manual_day_end(
        &mut self,
        session: &mut GameSession,
        advance_market_day: bool,
    ) -> Result<(), String> {
        session
            .require_healthy()
            .map_err(|fatal| fatal.to_string())?;
        #[cfg(feature = "simulation-diagnostics")]
        self.ensure_causal_day_end_supported()?;
        self.world.set_civil_date(session.civil_date());
        let input = PreparedDriverStep {
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
        };
        // World 的日结 validator 拒绝仍由自身有效 Abort 窗处理；成功后投影失败不能重跑。
        let input = self
            .prepare_day_end_input(session, input, advance_market_day, false)
            .map_err(|description| {
                if self.world.last_day_end_candidate.is_some() {
                    session
                        .poison_runtime_step(crate::session::StepFatal::InvariantViolation {
                            location: "RuntimeDriver::day_end::collect".to_owned(),
                            description,
                        })
                        .to_string()
                } else {
                    description
                }
            })?;
        let tick = self.world_committed_tick();
        let prepared =
            session
                .prepare_runtime_mirror(self, input, tick)
                .map_err(|description| {
                    self.abort_unpublished_day_end();
                    session
                        .poison_runtime_step(crate::session::StepFatal::InvariantViolation {
                            location: "RuntimeDriver::day_end::prepare".to_owned(),
                            description,
                        })
                        .to_string()
                })?;
        let ack = session
            .validate_runtime_mirror_source(self, &prepared)
            .map_err(|fatal| session.poison_runtime_step(fatal).to_string())?;
        let events = session.install_runtime_mirror(self, prepared);
        self.ack_prepared_mirror(ack);
        self.drained_events.extend(events);
        if let Err(error) = session.end_civil_day() {
            let fatal = match error {
                crate::session::SessionError::Step(fatal) => fatal,
                error => crate::session::StepFatal::InvariantViolation {
                    location: "RuntimeDriver::day_end::civil".to_owned(),
                    description: format!("市场日结安装后的自然日推进失败：{error}"),
                },
            };
            return Err(session.poison_runtime_step(fatal).to_string());
        }
        self.world.set_civil_date(session.civil_date());
        Ok(())
    }

    pub(crate) fn resting_snapshot_for_prepared(
        &mut self,
        day_end: bool,
    ) -> Result<
        (
            Vec<Arc<Vec<crate::runtime::RestingFact>>>,
            Arc<Vec<AccountId>>,
        ),
        String,
    > {
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
