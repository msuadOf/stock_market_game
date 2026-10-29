use super::{Event, GameSession, SaveSlot, SessionError};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, thiserror::Error)]
pub enum StepFatal {
    #[error("invariant violation at {location}: {description}")]
    InvariantViolation {
        description: String,
        location: String,
    },
}

/// 首次 lazy 接管的完整私有值；全部校验成功后只移动镜像和 driver。
struct PreparedRuntimeHandover {
    driver: crate::runtime::drive::RuntimeDriver,
    mirror: super::runtime_mirror::PreparedRuntimeMirror,
    ack: crate::runtime::source::PreparedMirrorAck,
    authority_epoch: u64,
    revision: u64,
}

impl GameSession {
    pub fn publication_ids(&self) -> Vec<crate::information::PublicationId> {
        self.state.library.publication_ids()
    }
    pub const fn poison_reason(&self) -> Option<&StepFatal> {
        self.poison.as_ref()
    }

    pub fn require_healthy(&self) -> Result<(), StepFatal> {
        match &self.poison {
            Some(fatal) => Err(fatal.clone()),
            None => Ok(()),
        }
    }

    /// runtime 驱动器翻转面（D2）：玩家意图冻结取出。
    pub fn take_runtime_player_intents(
        &mut self,
    ) -> Vec<crate::session::ReceiptBearingIntent> {
        #[cfg(all(test, not(target_family = "wasm")))]
        crate::session::calendar_entry_cache_tests::record_entry_call(crate::session::calendar_entry_cache_tests::EntryCall::Take);
        self.mark_external_mutation();
        self.state.pending_player.drain(..).collect()
    }

    /// runtime 驱动器翻转面（D2）：失败回灌玩家意图（旧管线 ready
    /// ingress 语义——失败 tick 不消费外部队列，权威 pending_player
    /// 原样保留给下一次尝试；世界侧已随失败回滚/毒化，不构成双权威）。
    /// Sol 整改注入通道（cfg(test)）：显式 ForceDue 指令下行（调度权威
    /// 在组——组验证目标 tick 后前移 next_due；Abort 冲回后指令失效
    /// 需重发）。
    /// 个人态生产交付投影（Sol 整改注入通道配套：runtime 模式下 observe
    /// 演进的组权威个人态经 Belief 读面上行→驱动器交付点回写——
    /// session 侧 belief/experience/watchlist/attention 镜像随组演进；
    /// X1 直改标记外部直改）。
    pub fn set_runtime_personal_state(
        &mut self,
        id: crate::AccountId,
        snapshot: &crate::runtime::world::BeliefParticipantSnapshot,
    ) {
        use super::decision_chain::personal_state::BeliefParticipantState;
        // P2-3b：四域首写前值入 undo（改写前记录——回滚源）。
        self.runtime_record_attention_undo(&id);
        self.runtime_record_belief_undo(&id);
        self.runtime_record_history_undo(&id);
        self.runtime_record_retail_undo(&id);
        // attention（调度域 candidate 保持组权威值——P1-10 链路回写的
        /// 镜像同步；information 域经 Belief-1b 投递演进——此处整态
        /// 覆盖为组快照，两域同源）。
        self.state.npc_attention.insert(id, snapshot.attention.clone());
        // P2-1：npc_attention 变更即置 heap 惰性标记（镜像同步在消费点
        // 重建——见 set_runtime_attention 注释）。
        self.state.runtime_heap_dirty = true;
        if let Some(mut previous) = self.state.belief_participants.remove(&id) {
            // 保留 participant 结构语义：以组快照重建（BeliefParticipantState
            // 的字段面=快照四件+attention/history 由各自容器承载）。
            let _ = &mut previous;
        }
        self.state.belief_participants.insert(
            id,
            BeliefParticipantState::new(
                snapshot.watchlist.clone(),
                snapshot.price_memory.clone(),
                snapshot.information.clone(),
                snapshot.belief.clone(),
            ),
        );
        self.state
            .history_reads
            .insert(id, snapshot.history_reads.clone());
        // D5 #4 retail 链：经验行镜像随组演进——组行为权威基，**保留
        /// 组外新增键**（同拍投影段的首买 init 在组内观察接管前由合并
        /// 承载；组内下一 due 的 capture initialize 同键幂等，不双载）。
        if let Some(row) = &snapshot.retail_experience {
            let merged = match self.state.retail_experience.remove(&id) {
                Some(existing) => existing.merged_with_group_authority(row),
                None => row.clone(),
            };
            self.state.retail_experience.insert(id, merged);
        }
    }

    /// Sol 整改注入通道（cfg(test)）：queue_npc 语义的 runtime 承载——
    /// 全名册（belief+retail 位）下一 tick 到期。
    #[cfg(test)]
    pub(crate) fn force_all_due_via_runtime(&mut self) {
        let (ids, world_next) = {
            let driver = self
                .runtime_driver
                .as_ref()
                .and_then(|cell| cell.lock().ok())
                .expect("runtime 驱动器在位（调用方已判）");
            (driver.id_map.clone(), driver.world_committed_tick() + 1)
        };
        // Sol 二审修复集 5：此前传 u64::MAX——组端守卫 `next_due > tick`
        // 对 MAX 恒假，正常调度下 no-op（queue 语义没真承载）。改传世界
        // 下一 tick（全部名册下一 tick 到期）。
        self.force_due_via_runtime(&ids, world_next);
    }

    /// D5 restore 等价：信封账本条目快照（desk 种子恢复——runtime 形态
    /// 下旧簿空，restore 重建的账本承载单身份）。
    /// Sol 复核阻断 P1-1：有快照→派生（P2-4 起 ledger 停逐拍维护，快照
    /// 是唯一现势源——回滚恢复/重建种子读现势在途）；无快照（restore
    /// 后未步进）→实际 ledger（restore 时刻的权威值）。
    pub fn runtime_envelope_ledger_snapshot(
        &self,
    ) -> Result<
        Vec<(
            crate::session::pipeline::EnvelopeKey,
            crate::session::pipeline::Envelope,
        )>,
        StepFatal,
    > {
        if self.state.runtime_resting_snapshot.is_some() {
            return self.runtime_derive_envelopes().map(|envelopes| {
                envelopes
                    .into_iter()
                    .map(|envelope| (envelope.key().clone(), envelope))
                    .collect()
            });
        }
        Ok(self
            .state
            .envelope_ledger
            .iter()
            .map(|(key, envelope)| (key.clone(), envelope.clone()))
            .collect())
    }

    /// D5 restore 等价：逐股 tick 价格历史读面（组世界构造的 desk
    /// recent_prices 种子——restore/接管场景长度与 save 校验一致）。
    pub fn runtime_price_histories(&self) -> Vec<Vec<i64>> {
        self.state
            .price_history
            .iter()
            .map(|(_, history)| history.iter().map(|price| price.cents()).collect())
            .collect()
    }

    /// D5 adopt 收口：旧簿在途单迁入 desk 后清空 session 侧（双域一致
    /// ——save 的 project_live_envelopes 读旧簿，残留使 fault2226 集合
    /// 失配；runtime 下旧簿不再权威）。
    pub fn runtime_clear_resting_for_adoption(&mut self) {
        self.mark_external_mutation();
        let snapshot = self.state.markets.clone();
        for (code, market) in snapshot.iter() {
            for order in market.resting_orders() {
                if let Some(m) = self.state.markets.get_mut(code) {
                    let _ = m.cancel(order.id);
                }
            }
        }
    }

    /// D5 #2：个体基础观察概率读面（npc_attention 的个体采样值——组内
    /// evaluate 门输入；无 attention 态的位返回 None）。
    pub fn runtime_attention_base_for(&self, id: &crate::AccountId) -> Option<f64> {
        self.state
            .npc_attention
            .get(id)
            .map(|state| state.base_probability)
    }

    /// D5 读面（cfg(test)）：desk 在途簿事实（owner/方向/价/量——旧管线
    /// bid_depth/resting_orders_for 断言面的 runtime 等价源；desk 权威）。
    #[cfg(test)]
    pub(crate) fn runtime_resting_book_for_test(
        &mut self,
        code: &crate::StockId,
    ) -> Vec<(crate::AccountId, crate::Side, crate::money::Money, u32)> {
        let mut driver = self
            .runtime_driver
            .as_mut()
            .and_then(|cell| cell.lock().ok())
            .expect("runtime 驱动器在位（调用方已判）");
        let facts = driver.collect_resting_facts_live().expect("在途簿收集");
        let idx = code.0 as usize;
        facts
            .get(idx)
            .map(|book| {
                book.iter()
                    .map(|fact| {
                        // Sol 终审修复 1c：归户保留组号（同镜像/信封）。
                        let account = driver
                            .account_by_owner(fact.owner)
                            .expect("owner 在名册");
                        (account, fact.side, crate::money::Money::from_cents(fact.price), fact.qty)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// D5 #4 fixture 通道（cfg(test)）：推远名册账户的组内 next_due
    ///（"无 due tick"形态——t3 stale trace 面的 runtime 等价）。
    #[cfg(test)]
    pub(crate) fn defer_due_via_runtime(&mut self, ids: &[crate::AccountId], tick: u64) {
        let mut driver = self
            .runtime_driver
            .as_mut()
            .and_then(|cell| cell.lock().ok())
            .expect("runtime 驱动器在位（调用方已判）");
        driver.defer_due_for_test_at(ids, tick);
    }

    #[cfg(test)]
    pub(super) fn force_due_via_runtime(&mut self, ids: &[crate::AccountId], tick: u64) {
        let mut driver = self
            .runtime_driver
            .as_mut()
            .and_then(|cell| cell.lock().ok())
            .expect("runtime 驱动器在位（调用方已判）");
        // 目标 tick 取 max(指令, 世界下一 tick)——世界游标可能已领先。
        driver.force_due_for_test_at(ids, tick);
    }

    /// D3b 件 4 fixture 通道（cfg(test)）：复审候选注入下行（组内
    /// run_chain_decision 的复审块+开仓循环消费——chain_restructure
    /// 测试 drive_once 的 runtime 等价输入面）。
    #[cfg(test)]
    pub(super) fn inject_review_assessments_via_runtime(
        &mut self,
        account: crate::AccountId,
        assessments: std::collections::BTreeMap<crate::StockId, crate::plans::CandidateAssessment>,
    ) {
        let mut driver = self
            .runtime_driver
            .as_mut()
            .and_then(|cell| cell.lock().ok())
            .expect("runtime 驱动器在位（调用方已判）");
        driver.inject_review_assessments_for_test(account, assessments);
    }

    /// D3b 件 4 fixture 通道（cfg(test)）：组簿测试变换下行（手工簿
    /// 推进的组内等价——镜像簿写入组权威不感知）。
    #[cfg(test)]
    pub(super) fn mutate_plan_book_via_runtime(
        &mut self,
        owner: crate::AccountId,
        mutate: Box<dyn FnOnce(&mut crate::plans::PlanBook) + Send>,
    ) {
        let mut driver = self
            .runtime_driver
            .as_mut()
            .and_then(|cell| cell.lock().ok())
            .expect("runtime 驱动器在位（调用方已判）");
        driver.mutate_plan_book_for_test(owner, mutate);
    }

    /// D3b 件 4 fixture 通道（cfg(test)）：观察基分钟序列注入下行
    ///（set_urgency_minutes 的组内等价——session 侧分钟缓存组内不感知）。
    #[cfg(test)]
    pub(super) fn inject_observation_minutes_via_runtime(
        &mut self,
        stock: u16,
        minutes: Vec<(u64, i64)>,
    ) {
        let mut driver = self
            .runtime_driver
            .as_mut()
            .and_then(|cell| cell.lock().ok())
            .expect("runtime 驱动器在位（调用方已判）");
        driver.inject_observation_minutes_for_test(stock, minutes);
    }

    /// D3b 件 4 fixture 通道（cfg(test)）：计划事件下行（组簿权威 apply
    ///——镜像簿直改组权威不感知）。
    #[cfg(test)]
    pub(super) fn apply_plan_events_via_runtime(
        &mut self,
        events: Vec<(crate::plans::PlanId, crate::plans::PlanEvent)>,
    ) {
        let mut driver = self
            .runtime_driver
            .as_mut()
            .and_then(|cell| cell.lock().ok())
            .expect("runtime 驱动器在位（调用方已判）");
        driver.apply_plan_events_for_test(events);
    }

    /// D3b 件 4 fixture 通道（cfg(test)）：belief 簿测试变换下行
    ///（observe_equity 等手工 belief 面直改的组内等价）。
    #[cfg(test)]
    pub(super) fn mutate_belief_via_runtime(
        &mut self,
        account: crate::AccountId,
        mutate: Box<dyn FnOnce(&mut crate::strategy::BeliefBook) + Send>,
    ) {
        let mut driver = self
            .runtime_driver
            .as_mut()
            .and_then(|cell| cell.lock().ok())
            .expect("runtime 驱动器在位（调用方已判）");
        driver.mutate_belief_for_test(account, mutate);
    }

    fn restore_runtime_player_intents(
        &mut self,
        intents: Vec<crate::session::ReceiptBearingIntent>,
    ) {
        if intents.is_empty() {
            return;
        }
        self.mark_external_mutation();
        self.state.pending_player.splice(0..0, intents);
    }

    /// B2 件 4：休市自然日补保留条目（末条日期==settled 跳过；惰性构造
    /// 驱动器与 step 同路由——休市日无 step 也保证 world 发布面同步）。
    pub(in crate::session) fn freeze_closed_retained_day_if_needed(
        &mut self,
        settled: crate::calendar::CivilDate,
    ) -> Result<(), StepFatal> {
        let already = self
            .state
            .retained_market_days
            .last()
            .is_some_and(|day| day.0.date == settled);
        if already {
            return Ok(());
        }
        // 直调模式（不走 `with_routed_runtime_driver`——其锁步校验是
        /// step 拍语义的前置，end_civil_day 时点（restore 重放等形态）
        /// 校验错位；此处仅需驱动器在位）。
        if self.runtime_driver.is_none() {
            self.construct_runtime_driver_lazy()?;
        }
        let cell = self
            .runtime_driver
            .take()
            .expect("上方已构造或在位");
        let outcome = (|| {
            let mut driver = cell.lock().map_err(|_| {
                StepFatal::InvariantViolation {
                    location: "GameSession::end_civil_day::retained_closed".to_owned(),
                    description: "runtime 驱动器锁 poison（休市冻结）".to_owned(),
                }
            })?;
            driver
                .freeze_retained_closed_day(self, settled)
                .map_err(|e| StepFatal::InvariantViolation {
                    location: "GameSession::end_civil_day::retained_closed".to_owned(),
                    description: e,
                })
        })();
        self.runtime_driver = Some(cell);
        outcome
    }

    /// 惰性构造常驻驱动器（`with_routed_runtime_driver` 的构造段抽公共
    /// ——首步与 end_civil_day 补冻结两消费面共用）。
    #[cfg(test)]
    pub(in crate::session) fn construct_runtime_driver_lazy_for_test(&mut self) -> Result<(), StepFatal> {
        self.construct_runtime_driver_lazy()
    }

    pub(in crate::session) fn construct_runtime_driver_lazy(&mut self) -> Result<(), StepFatal> {
        self.require_healthy()?;
        let fail = |description| StepFatal::InvariantViolation {
            location: "GameSession::ensure_runtime_driver".to_owned(), description,
        };
        // private markets 已去旧簿，activation 只有 mirror install 的一次 revision 写入。
        let revision = self.external_revision();
        revision.checked_add(1).ok_or_else(|| fail("静点移交 external revision 已耗尽".to_owned()))?;
        let authority_epoch = self.next_runtime_authority_epoch().map_err(fail)?;
        self.runtime_mirror_install_sequence.checked_add(1)
            .ok_or_else(|| fail("静点移交 install_sequence 已耗尽".to_owned()))?;
        crate::runtime::drive::RuntimeDriver::validate_static_adoption_identity(self).map_err(fail)?;
        let mut driver = crate::runtime::drive::RuntimeDriver::prepare_from_session_grouped(
            self, self.runtime_group_count, authority_epoch,
        ).map_err(fail)?;
        driver.adopt_session_seq(self);
        let prepared = (|| {
            let input = driver.prepare_static_driver_projection(self)?;
            let mirror = self.prepare_runtime_handover_mirror(&mut driver, input)?;
            // staged fixture 仅窥视；全部 prepare 成功前仍保留原批。
            #[cfg(test)]
            if let Some(queued) = crate::session::pipeline::staged_decision::peek_any() {
                if !queued.intents().is_empty() {
                    let upper: Vec<_> = driver.world_public_views().iter()
                        .map(|view| view.daily_upper_limit).collect();
                    driver.inject_player_intents(queued.intents(), &upper)?;
                }
            }
            let ack = self.validate_runtime_mirror_source(&driver, &mirror)
                .map_err(|error| error.to_string())?;
            Ok::<_, String>((mirror, ack))
        })();
        let (mirror, ack) = match prepared {
            Ok(value) => value,
            Err(error) => {
                return match driver.stop_prepared_without_drain() {
                    Ok(()) => Err(fail(error)),
                    Err(cleanup) => Err(self.poison_runtime_step(fail(format!(
                        "{error}；prepared World 回收失败：{cleanup}"
                    )))),
                };
            }
        };
        let prepared = PreparedRuntimeHandover { driver, mirror, ack, authority_epoch, revision };
        assert_eq!(self.external_revision(), prepared.revision, "prepare 与 activation 之间不允许 Session 来源改变");
        self.install_runtime_authority_epoch(prepared.authority_epoch);
        let mut driver = prepared.driver;
        let events = self.install_runtime_mirror(&mut driver, prepared.mirror);
        assert!(events.is_empty(), "静点接管不产生业务事件");
        driver.ack_prepared_mirror(prepared.ack);
        #[cfg(test)]
        let _ = crate::session::pipeline::staged_decision::take_any();
        self.runtime_driver = Some(std::sync::Mutex::new(driver));
        self.state.runtime_information_authority = true;
        Ok(())
    }

    /// runtime 驱动翻转入口（翻转批）：环境变量 ENGINE_STEP_RUNTIME=1
    /// 时 `step()` 走 RuntimeDriver（常驻 World+安装面），否则旧管线
    /// （缺省 0=旧管线，见 `runtime::step_runtime_enabled`）。
    /// `step_frame_and_commit`（candidate 投影契约/Escrow 验证）恒走
    /// 旧管线为 fallback——翻转范围=生产便捷入口。
    /// Sol 二审修复集 3：前置校验**真正共用** `check_step_start`
    /// （健康→注入失败→缺策略，与旧管线同判同序——此前 runtime 路径
    /// 自带一份缺策略检查且先于注入失败执行，两路径不同判）。
    /// runtime 驱动翻转入口（翻转批）：环境变量 ENGINE_STEP_RUNTIME=1
    /// 时 `step()` 走 RuntimeDriver（常驻 World+安装面），否则旧管线
    /// （缺省 0=旧管线，见 `runtime::step_runtime_enabled`）。
    /// step_frame 接轨件 4 起 `step_frame_and_commit`（生产便捷入口，
    /// capture=false）同走 runtime 分叉；Escrow 验证面
    /// （capture_commit_evidence=true）恒走旧管线。
    /// Sol 二审修复集 3：前置校验**真正共用** `check_step_start`
    /// （健康→注入失败→缺策略，与旧管线同判同序）。
    pub fn step_via_runtime(&mut self) -> Result<Vec<Event>, StepFatal> {
        // 【P0 画像】入口检查（require_healthy + has_missing_npc_strategy
        // ——回写 invalidate 后的页级重扫面）。
        let _seg_check = crate::runtime::step_profile::scope(
            crate::runtime::step_profile::StepSegment::StepEntry,
        );
        let check = self.check_step_start();
        drop(_seg_check);
        check.map_err(|fatal| self.poison_runtime_step(fatal))?;
        self.with_routed_runtime_driver(|session, driver| {
            // 刀③：body 总段（driver_step+安装链+尾部全含）——与
            /// step_outer/lock_route 差额二分定位未段化归属。
            let _seg_body = crate::runtime::step_profile::scope(
                crate::runtime::step_profile::StepSegment::StepViaBody,
            );
            session.runtime_step_with_install(driver)
        })
    }

    /// step_frame 接轨件 4：驱动器获取+前置编排共用段（step_via_runtime
    /// 与 step_frame_and_commit_via_runtime 共享）。语义与原
    /// step_via_runtime 内联段逐字一致：D0 惰性构造（含 cfg(test) 的
    /// staged handover 消费）→ 锁单元取出/放回 → poison 拒绝 → 锁步
    /// 不变量校验 → ingress 冻结 → 玩家意图路由（失败回灌——ready
    /// ingress 语义）。`body` 在驱动器锁内执行；body 失败时本段回灌
    /// 意图（外部队列不被失败 tick 消费）。
    fn with_routed_runtime_driver<R>(
        &mut self,
        body: impl FnOnce(
            &mut Self,
            &mut crate::runtime::drive::RuntimeDriver,
        ) -> Result<R, StepFatal>,
    ) -> Result<R, StepFatal> {
        // Calendar 非法时不得构造 driver、截取 ingress 或消费受理身份。
        let (calendar_day, calendar_tick) = self.prepare_market_calendar_tick()?;
        // D0 常驻 driver：首次调用惰性构造（initial_committed_tick 对齐
        // 当前 session.tick——全新会话即 0）。
        if self.runtime_driver.is_none() {
            // 构造细节见 `construct_runtime_driver_lazy`（D0 惰性构造+
            /// cfg(test) staged handover 消费——两消费面共用）。
            self.construct_runtime_driver_lazy()?;
        }
        // 取出锁单元（避免 &mut self 与 guard 交叉借用）；失败/成功都
        // 放回——Abort 语义下 worker 存活，driver 可重跑。例外：body 内
        /// 置 runtime_discard_driver_flag（step_frame 分叉的投影段失败——
        /// world 已转正而会话已回滚，锁步断裂的实例不可复用）时丢弃，
        /// 重试经 from_session 重建（等价旧管线"candidate 丢弃+可重试"）。
        let cell = self
            .runtime_driver
            .take()
            .expect("runtime_driver 刚构造或在位");
        let outcome = self.with_routed_runtime_driver_locked(&cell, calendar_tick, body);
        if outcome.is_ok() {
            self.install_market_calendar_day(calendar_day);
        }
        if self.runtime_discard_driver_flag {
            self.runtime_discard_driver_flag = false;
            drop(cell);
        } else {
            self.runtime_driver = Some(cell);
        }
        outcome
    }

    /// 共用段锁内部分（cell 已取出——失败路径由调用方放回）。
    fn with_routed_runtime_driver_locked<R>(
        &mut self,
        cell: &std::sync::Mutex<crate::runtime::drive::RuntimeDriver>,
        calendar: std::sync::Arc<crate::runtime::MarketTickContext>,
        body: impl FnOnce(&mut Self, &mut crate::runtime::drive::RuntimeDriver) -> Result<R, StepFatal>,
    ) -> Result<R, StepFatal> {
        // 【P0 画像】lock_route 段（锁+锁步校验+ingress freeze+意图路由）。
        let _seg_lock = crate::runtime::step_profile::scope(
            crate::runtime::step_profile::StepSegment::LockRoute,
        );
        let guard = cell.lock();
        let poisoned = guard.is_err();
        let mut driver = if poisoned {
            // 锁被 poison=前次持有期 panic——显式失败，不静默复用
            // （不 into_inner：poison 期状态未知）。
            return Err(self.poison_runtime_step(StepFatal::InvariantViolation {
                location: "GameSession::step_via_runtime".to_owned(),
                description: "常驻驱动器锁被 poison（前次持有期 panic）".to_owned(),
            }));
        } else {
            guard.expect("is_err 已排阴")
        };
        // 锁步不变量：world 已转正 tick 必须 == session.tick
        // （失配=两权威漂移，显式拒绝——不静默重放）。
        let world_tick = driver.world_committed_tick();
        if world_tick != self.tick() {
            return Err(self.poison_runtime_step(StepFatal::InvariantViolation {
                location: "GameSession::step_via_runtime".to_owned(),
                description: format!(
                    "锁步失配：world 已转正 tick={world_tick} 而 session.tick={}（两权威漂移——不得静默重放）",
                    self.tick()
                ),
            }));
        }
        driver
            .stage_calendar_tick(calendar)
            .map_err(|description| StepFatal::InvariantViolation {
                location: "GameSession::with_routed_runtime_driver_locked::calendar".to_owned(),
                description,
            })?;
        driver.require_staged_calendar_tick().map_err(|description| StepFatal::InvariantViolation {
            location: "GameSession::with_routed_runtime_driver_locked::calendar".to_owned(),
            description,
        })?;
        // D4（t3 对等）：ingress 截取时刻——宿主经 shared ingress
        // 绑定入队的玩家意图搬运进 pending_player（旧管线 cutoff
        // 同口径；无绑定为 no-op）。
        if let Err(fatal) = self.freeze_shared_ingress() {
            return Err(self.poison_runtime_step(fatal));
        }
        // 同一cutoff内输入已真实接收；在读取预算或发送 ExternalPlan 前同步旧镜像。
        // flush 失败保留 pending 与原 ordinal，不发下一市场 Start，也不回退接收事实。
        if self.state.setup.runtime_sync_policy.needs_compute_flush() && !self.state.pending_player.is_empty() {
            if let Err(description) = driver.flush_candidate(self, crate::runtime::sync_policy::FlushTrigger::ComputeRead) {
                let fatal = match self.poison_reason() {
                    Some(first) => first.clone(),
                    None => StepFatal::InvariantViolation {
                        location: "GameSession::compute_read_flush".to_owned(), description,
                    },
                };
                return Err(self.poison_runtime_step(fatal));
            }
        }
        // 玩家意图路由（D2）：冻结取出→ExternalPlan 注入组 0；
        // 失败回灌（ready ingress 语义——外部队列不被失败 tick 消费）。
        let intents = self.take_runtime_player_intents();
        if !intents.is_empty() {
            let uppers: Vec<i64> = {
                let views = driver.world_public_views();
                views.iter().map(|v| v.daily_upper_limit).collect()
            };
            if let Err(route_err) = driver.inject_player_intents(&intents, &uppers) {
                self.restore_runtime_player_intents(intents);
                return Err(self.poison_runtime_step(StepFatal::InvariantViolation {
                    location: "GameSession::step_via_runtime".to_owned(),
                    description: route_err,
                }));
            }
        }
        drop(_seg_lock);
        let outcome = body(self, &mut driver);
        if outcome.is_err() {
            self.restore_runtime_player_intents(intents);
        }
        outcome
    }

    /// Frame callback 只读取私有 candidate；全部验证成功才唯一安装。
    /// callback 失败保旧镜像与已发布 World，封存首错并禁止重跑。
    pub(in crate::session) fn step_frame_and_commit_via_runtime<P>(
        &mut self,
        project: impl FnOnce(
            crate::session::protocol::TickCandidateProjection<'_>,
            Vec<crate::session::protocol::EventFact>,
        ) -> Result<P, StepFatal>,
    ) -> Result<(P, Option<super::pipeline::TickCommitEvidence>), StepFatal> {
        self.check_step_start()
            .map_err(|fatal| self.poison_runtime_step(fatal))?;
        self.with_routed_runtime_driver(|session, driver| {
            session.runtime_step_with_prepared_mirror(driver, project).map(|value| (value, None))
        })
    }

    /// 普通拍与日界共用私有 prepare、最后 callback、source 验证及 move install。
    pub(in crate::session) fn runtime_step_with_install(
        &mut self,
        driver: &mut crate::runtime::drive::RuntimeDriver,
    ) -> Result<Vec<Event>, StepFatal> {
        self.runtime_step_with_prepared_mirror(driver, |_, facts| {
            Ok(facts.into_iter().map(|fact| fact.event).collect())
        })
    }

    /// 每拍在私有镜像与全部输出验证成功后一次安装；失败保留 World 已发布事实。
    pub(in crate::session) fn runtime_step_with_prepared_mirror<P>(
        &mut self,
        driver: &mut crate::runtime::drive::RuntimeDriver,
        project: impl FnOnce(crate::session::protocol::TickCandidateProjection<'_>, Vec<crate::session::protocol::EventFact>) -> Result<P, StepFatal>,
    ) -> Result<P, StepFatal> {
        self.require_healthy()?;
        driver.ensure_session_authority(self).map_err(|description| {
            StepFatal::InvariantViolation {
                location: "runtime_mirror::authority".to_owned(),
                description,
            }
        })?;
        self.runtime_clear_account_undo();
        #[cfg(test)]
        self.compute_context().t1_probes.queue_attempt_seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let tick = self.tick().checked_add(1).ok_or_else(|| self.poison_runtime_step(StepFatal::InvariantViolation {
            location: "runtime_mirror::prepare".to_owned(), description: "运行 tick 溢出".to_owned(),
        }))?;
        let seq_from = self.seq();
        driver.adopt_session_seq(self);
        crate::verification_evidence::begin_authoritative_tick(self.tick(), self.compute_context())?;
        #[cfg(feature = "simulation-diagnostics")]
        let causal_time = self.causal_time().map_err(|fatal| self.poison_runtime_step(fatal))?;
        let prepared = driver.prepare_driver_step(self).and_then(|input| self.prepare_runtime_publication(driver, input, tick));
        let mut prepared = prepared.map_err(|description| {
            let fatal = StepFatal::InvariantViolation { location: "runtime_mirror::prepare".to_owned(), description };
            #[cfg(feature = "simulation-diagnostics")]
            driver.retain_pending_causal_failure(causal_time, fatal.clone());
            driver.abort_unpublished_day_end();
            self.poison_runtime_step(fatal)
        })?;
        #[cfg(feature = "simulation-diagnostics")]
        let causal = self.prepare_runtime_causal_at(driver, causal_time, tick, prepared.plans(), prepared.calendar())
            .map_err(|fatal| self.poison_runtime_step(fatal))?;
        #[cfg(test)]
        if let Some(fatal) = self.post_shadow_failure.take() {
            #[cfg(feature = "simulation-diagnostics")]
            driver.retain_terminal_causal(causal.time, causal.raw, fatal.clone());
            return Err(self.poison_runtime_step(fatal));
        }
        crate::verification_evidence::enter_phase(crate::session::pipeline::TickPhase::PreCommitValidation);
        if let Err(fatal) = crate::verification_evidence::validate_precommit() {
            #[cfg(feature = "simulation-diagnostics")]
            driver.retain_terminal_causal(causal.time, causal.raw, fatal.clone());
            return Err(self.poison_runtime_step(fatal));
        }
        #[cfg(test)]
        let has_events = !prepared.events().is_empty();
        let facts = crate::runtime::event_bridge::event_facts_with_stable_keys(prepared.take_events());
        let value = match project(crate::session::protocol::TickCandidateProjection::from_prepared(prepared.projection(), seq_from), facts) {
            Ok(value) => value,
            Err(fatal) => {
                #[cfg(feature = "simulation-diagnostics")]
                driver.retain_terminal_causal(causal.time, causal.raw, fatal.clone());
                return Err(self.poison_runtime_step(fatal));
            }
        };
        let ack = match self.validate_runtime_publication(driver, &prepared) {
            Ok(ack) => ack,
            Err(fatal) => {
                #[cfg(feature = "simulation-diagnostics")]
                driver.retain_terminal_causal(causal.time, causal.raw, fatal.clone());
                return Err(self.poison_runtime_step(fatal));
            }
        };
        if let Some(binding) = &self.ingress {
            binding.source.update_runtime_progress(tick, driver.mirror_source().generation)
                .map_err(|error| self.poison_runtime_step(super::shared_ingress::ingress_fatal(error)))?;
        }
        self.install_runtime_publication(driver, prepared);
        if let Some(ack) = ack { driver.ack_prepared_mirror(ack); }
        #[cfg(feature = "simulation-diagnostics")]
        self.publish_runtime_causal(causal);
        crate::verification_evidence::enter_phase(crate::session::pipeline::TickPhase::CommitTick);
        crate::verification_evidence::mark_committed(tick);
        #[cfg(test)]
        crate::runtime::drive::RuntimeDriver::record_root_handover(self, tick, has_events);
        self.runtime_clear_account_undo();
        Ok(value)
    }



    /// P2-3b：五域写时 undo 记录面（幂等首写——undo_ids 判重；None=拍前
    /// 不在册墓碑）。写入点在**改写前**调用。
    fn runtime_record_attention_undo(&mut self, id: &crate::AccountId) {
        if self.state.runtime_attention_undo_ids.insert(*id) {
            let prev = self.state.npc_attention.get(id).cloned();
            self.state.runtime_attention_undo.push((*id, prev));
        }
    }

    fn runtime_record_belief_undo(&mut self, id: &crate::AccountId) {
        if self.state.runtime_belief_undo_ids.insert(*id) {
            let prev = self.state.belief_participants.get(id).cloned();
            self.state.runtime_belief_undo.push((*id, prev));
        }
    }

    fn runtime_record_retail_undo(&mut self, id: &crate::AccountId) {
        if self.state.runtime_retail_undo_ids.insert(*id) {
            let prev = self.state.retail_experience.get(id).cloned();
            self.state.runtime_retail_undo.push((*id, prev));
        }
    }

    fn runtime_record_history_undo(&mut self, id: &crate::AccountId) {
        if self.state.runtime_history_undo_ids.insert(*id) {
            let prev = self.state.history_reads.get(id).cloned();
            self.state.runtime_history_undo.push((*id, prev));
        }
    }


    /// 所有市场工作运行在私有 tick shadow；失败候选被丢弃。
    /// 权威状态只接收最终不会失败的 CommitTick 状态交换。
    ///
    /// 返回的事件数组是从已提交事实记录批次投影出的交付视图（架构项 #6）：
    /// 会话内部不保留第二份事件权威，身份仍只存在于 `EventFact` 记录中。
    pub fn step(&mut self) -> Result<Vec<Event>, StepFatal> {
        // 翻转批：opt-in 翻转（ENGINE_STEP_RUNTIME=1 显式走 runtime；
        // 缺省 0=旧管线——106 失败实测后回退，见 runtime/mod.rs 开关
        /// 注释；全局域迁移完成后才改缺省）。
        if crate::runtime::step_runtime_enabled() {
            return self.step_via_runtime();
        }
        self.step_inner(false)
            .map(|committed| committed.facts.into_iter().map(|fact| fact.event).collect())
    }

    /// 执行与 [`Self::step`] 相同的生产权威路径，并返回
    /// 保留成功提交前一刻捕获的不可变执行事实。
    ///
    /// 此验证入口只用于观察；证据不会留在 session、序列化、参与 hash 或后续决策。
    /// 旧管线契约测试入口（D2 登记）：staged handover / 旧簿 fixture 等
    /// **旧管线专有注入面**的契约测试显式走旧管线——runtime step() 无
    /// 对应注入面（决策在组线程、簿在交易部），经 step() 混跑只会把
    /// 注入面缺失误报为语义失败。这些测试的 runtime 等价锚另行登记
    /// （玩家路由/世界级 TTL 撤单测试）；旧管线退役时随语义一并迁移。
    #[cfg(test)]
    pub(in crate::session) fn step_pipeline_contract(&mut self) -> Result<Vec<Event>, StepFatal> {
        self.step_inner(false)
            .map(|committed| committed.facts.into_iter().map(|f| f.event).collect())
    }

    pub fn step_with_commit_evidence(
        &mut self,
    ) -> Result<(Vec<Event>, super::pipeline::TickCommitEvidence), StepFatal> {
        let committed = self.step_inner(true)?;
        let evidence = committed
            .evidence
            .expect("commit evidence was requested at the authoritative entry");
        Ok((
            committed.facts.into_iter().map(|fact| fact.event).collect(),
            evidence,
        ))
    }

    pub(super) fn step_inner(
        &mut self,
        capture_commit_evidence: bool,
    ) -> Result<super::pipeline::AuthoritativeTickCommit, StepFatal> {
        self.step_on_context(capture_commit_evidence)
    }

    /// 协调逻辑在调用线程；各并行 burst 经会话上下文进入其池（架构项 14）。
    pub(crate) fn step_on_context(
        &mut self,
        capture_commit_evidence: bool,
    ) -> Result<super::pipeline::AuthoritativeTickCommit, StepFatal> {
        self.check_step_start()
            .map_err(|fatal| self.poison_failed_step(fatal))?;
        // 每个生产交易阶段均进入同一 Escrow 完整 tick 事务。
        // 只有完成不会失败的权威状态交换后才交付结果。
        let result = super::pipeline::execute_authoritative_tick(self, capture_commit_evidence);
        match result {
            Ok(committed) => Ok(committed),
            Err(fatal) => Err(self.poison_failed_step(fatal)),
        }
    }

    /// 按每只证券所属交易所及本局冻结政策判断开市数量。
    fn open_stock_count_today(&self) -> Result<usize, StepFatal> {
        let date = self.state.civil_clock.current_date();
        let calendar = self.state.civil_clock.calendar();
        let mut open = 0;
        for stock in &self.state.setup.stocks {
            let exchange = super::session_calendar_exchange(stock.exchange);
            let trading = calendar.is_trading_day(exchange, date).map_err(|error| {
                StepFatal::InvariantViolation {
                    description: format!("交易日历无法判定 {date} {exchange:?} 是否开市：{error}"),
                    location: "GameSession::check_step_start::calendar_gate".to_owned(),
                }
            })?;
            open += usize::from(trading);
        }
        Ok(open)
    }

    /// step 家族共用的前置检查：健康状态、测试注入失败与 NPC 策略完整性。
    /// frame 投影路径（step_frame_and_commit）与 step_inner 都从这里进入同一 dispatcher。
    pub(super) fn check_step_start(&mut self) -> Result<(), StepFatal> {
        self.require_healthy()?;
        #[cfg(test)]
        self.run_pre_mutation_hook()?;
        if self.state.accounts.has_missing_npc_strategy() {
            return Err(StepFatal::InvariantViolation {
                description: "non-player account has no authoritative strategy".to_owned(),
                location: "GameSession::step".to_owned(),
            });
        }
        // 在市场事务及 RNG 消费之前拒绝全场休市；自然日日结走独立入口。
        if self.open_stock_count_today()? == 0 {
            return Err(StepFatal::InvariantViolation {
                description: format!(
                    "休市日不得成交：{} 全场休市（本局冻结日历判定）",
                    self.state.civil_clock.current_date()
                ),
                location: "GameSession::check_step_start::calendar_gate".to_owned(),
            });
        }
        Ok(())
    }

    /// Q1 发布槽句柄（session 侧读者面）：外部读者（server actor/测试/
    /// 存档）经此取一致读点——load() 一次 clone 即得"一个固定提交版本
    /// + 它的全部分片句柄"。驱动器不在位=尚未步入 runtime 模式，显式
    /// 失败（不静默返回空版本）。
    pub fn runtime_publication(
        &self,
    ) -> Result<std::sync::Arc<crate::runtime::publication::ViewPublication>, String>
    {
        let cell = self
            .runtime_driver
            .as_ref()
            .ok_or_else(|| "发布槽不可用：runtime 驱动器尚未构造".to_owned())?;
        let driver = cell
            .lock()
            .map_err(|_| "驱动器锁被 poison（前次持有期 panic）".to_owned())?;
        Ok(driver.publication())
    }

    /// runtime 驱动器翻转面（P1-10）：逐账户注意力候选真实写回
    /// （组线程 next_due 是调度权威——替换"推到 tick"的假迁移）。
    /// Sol 二审修复集 5：candidate 与 attention_scheduler heap 同步
    /// （heap 是旧管线 pop_due 的驱动面——只更 candidate 不入堆，
    /// 调度面看不到新候选；陈旧堆项由 pop_due 的候选核对自然跳过）。
    /// P1-C（性能修复）：heap 改为从权威 npc_attention **全量重建镜像**——
    /// 此前逐条 enqueue 且 runtime 路径永不 pop_due（pop 只在旧管线决策
    /// 链），heap 每 tick +名册无界增长（O(N·T²) 累计复制，B 口径随时长
    /// 恶化——Sol 终裁判定，见 .tmp/perf-p0-diff-profile.md）。重建后
    /// 每账户恰一项、值=权威候选 tick（"新候选入堆"语义保持——新候选
    /// 必然在堆；陈旧项消失，pop_due 的候选核对语义不受影响）。
    pub fn set_runtime_attention(
        &mut self,
        entries: &[(crate::AccountId, u64, u64)],
    ) {
        self.mark_external_mutation();
        let tick = self.state.tick;
        for (id, next_due, rng_state) in entries {
            // P2-3b：首写前值入 undo（改写前记录——回滚源）。
            self.runtime_record_attention_undo(id);
            if let Some(state) = self.state.npc_attention.get_mut(id) {
                let due = (*next_due).max(tick);
                state.next_attention_candidate_tick = due;
                // D5 #2：注意力共享流续传写回（组内 evaluate 消费后的
                // 权威——restore 双轨逐位等价依赖该面）。
                state.rng_state = *rng_state;
            }
        }
        // P2-1（视图内生战役）：每拍 heap 全名册重建退役（P0 画像 D6 大头
        // ≈14% wall）。heap 是旧管线 pop_due 的驱动镜像——runtime 路径
        // 无 pop_due 消费；候选权威在 npc_attention，镜像改惰性（消费点
        // [`Self::ensure_attention_heap_current`] 重建一次）。hash 面已改
        // 由 npc_attention 派生（集合等价——见 hash.rs）。move-undo 槽
        // 随急切重建一起退役（槽恒 None——回滚路径 no-op；回滚恢复的
        // npc_attention 即拍前值，heap 未被动过仍同步）。
        self.state.runtime_heap_dirty = true;
    }

    /// P2-1：attention heap 惰性重建（消费点调用——dirty 时从候选权威
    /// npc_attention 重建一次）。等价合同：重建后 heap 集合==权威快照
    /// （与旧每拍急切重建逐项同值——"新候选入堆"语义由重建保持）。
    pub fn ensure_attention_heap_current(&mut self) {
        if !self.state.runtime_heap_dirty {
            return;
        }
        self.state.attention_scheduler = self
            .state
            .npc_attention
            .iter()
            .map(|(id, state)| (state.next_attention_candidate_tick, *id))
            .collect();
        self.state.runtime_heap_dirty = false;
    }

    /// runtime 驱动器翻转面（Sol 二审修复集 5）：attention 状态只读投影
    /// `(AccountId, 下次候选 tick, 注意力 RNG 状态)`——驱动器构造器
    /// 恢复会话真实调度态用（不合成固定 seed/初始 due）。
    /// D5 attention：收盘竞价公开截点读面（restore 链的 desk 种子投影源）。
    pub fn runtime_public_cutoff_for(&self, code: &crate::StockId) -> u64 {
        // runtime 形态（desk 镜像在位）——截点权威在镜像；旧簿形态读 market。
        if !self.state.desk_book_mirror.is_empty() {
            self.state
                .desk_book_mirror
                .get(code)
                .map(|mirror| mirror.public_cutoff_serial)
                .unwrap_or(0)
        } else {
            self.state
                .markets
                .get(code)
                .map(|m| m.auction_public_cutoff_sequence())
                .unwrap_or(0)
        }
    }

    /// D5 #4 retail 链：会话种子读面（决策 rng 的 npc_seed_base 同源）。
    pub fn runtime_session_seed(&self) -> u64 {
        self.state.seed
    }

    pub fn runtime_attention_state(&self) -> Vec<(crate::AccountId, u64, u64)> {
        self.state
            .npc_attention
            .iter()
            .map(|(id, s)| (*id, s.next_attention_candidate_tick, s.rng_state))
            .collect()
    }

    /// 测试面（Sol 六审修复 3）：retail 经历条目存在性——同拍投影首买
    /// 经历初始化的断言锚（fault2038 面）。
    #[cfg(test)]
    pub fn runtime_retail_experience_has(
        &self,
        id: crate::AccountId,
        code: &crate::StockId,
    ) -> bool {
        self.state
            .retail_experience
            .get(&id)
            .is_some_and(|exp| exp.stocks.contains_key(code))
    }

    /// Sol 复核阻断 P1-2 四面对账（cfg(test)）：session 镜像的逐股
    /// retail 经历行（与组权威/发布分片行做逐位对照——真实对账面）。
    #[cfg(test)]
    pub fn runtime_retail_experience_stock_for_test(
        &self,
        id: crate::AccountId,
        code: &crate::StockId,
    ) -> Option<crate::experience::RetailStockExperience> {
        self.state
            .retail_experience
            .get(&id)
            .and_then(|exp| exp.stocks.get(code).cloned())
    }

    /// 测试编排面（Sol 七审修复 3）：构造"从未持有"首买形态——移除
    /// 指定股的持仓条目与 retail 经历条目（float_allocation 默认全股
    /// 配仓；被测对象是投影层初始化，非配仓面）。
    #[cfg(test)]
    pub fn fixture_clear_holding_for_test(&mut self, id: crate::AccountId, code: &crate::StockId) {
        self.mark_external_mutation();
        if let Some(acct) = self.state.accounts.get_mut(&id) {
            acct.fixture_clear_position_for_test(code);
        }
        if let Some(exp) = self.state.retail_experience.get_mut(&id) {
            // 两面同清（initialize_holding_dated 的守卫查 feedback.stocks
            /// ——只清 stocks 会被 ActiveEntryAlreadyExists 拒绝）。
            exp.stocks.remove(code);
            exp.feedback.stocks.remove(code);
        }
    }

    /// runtime 驱动器翻转面（Sol 二审修复集 5）：会话旧簿在途挂单只读
    /// 投影 `(StockId, Order 列表)`——驱动器构造期迁入世界（首步前
    /// seed 的挂单此前只在旧簿、runtime 世界不可见——裁决点名的
    /// "driver 首构造不迁入旧挂单"缺口）。
    /// runtime 恢复链：每股序号游标（Sol 四轮修复——R26 空簿保留已耗
    /// 序号）。取 desk 镜像 next_serial（运行中权威）与旧簿
    /// book_next_sequence（restore 后镜像空、旧簿载 save 值）的逐股
    /// max；输出按下标（StockId.0）索引。
    /// 旧全局 ID 域的已消耗上界；desk 形态已有逐股 cursor，不把 encoded ID 塞入本地 serial。
    pub(crate) fn runtime_legacy_order_id_floor(&self) -> Result<Option<u64>, String> {
        if !self.state.desk_book_mirror.is_empty() {
            return Ok(None);
        }
        let floor = self.state.next_order_id;
        if floor > (1_u64 << 32) {
            return Err(format!("初始静点接管不能保真旧全局 OrderId 游标：next_order_id={floor}"));
        }
        Ok(Some(floor))
    }

    pub fn runtime_book_next_sequences(&self) -> Vec<u64> {
        let n = self
            .state
            .markets
            .keys()
            .map(|code| code.0 as usize + 1)
            .max()
            .unwrap_or(0);
        let mut out = vec![0u64; n];
        for (code, market) in &self.state.markets {
            let idx = code.0 as usize;
            out[idx] = out[idx].max(market.book_next_sequence());
        }
        for (code, mirror) in &self.state.desk_book_mirror {
            let idx = code.0 as usize;
            if idx < n {
                out[idx] = out[idx].max(mirror.next_serial);
            }
        }
        for (idx, cursor) in self.state.runtime_desk_serial_cursors.iter().enumerate() {
            if idx < n {
                out[idx] = out[idx].max(*cursor);
            }
        }
        out
    }

    pub fn runtime_resting_orders_snapshot(
        &self,
    ) -> Vec<(crate::StockId, Vec<crate::orderbook::Order>)> {
        self.state
            .markets
            .iter()
            .map(|(code, market)| (code.clone(), market.resting_orders()))
            .collect()
    }

    /// 手动日结只撤销新增账户投影、经历与个人确认；不承诺回退已发布市场 tick。
    pub(crate) fn runtime_day_end_mirror_rollback(&self) -> impl FnOnce(&mut Self) {
        let accounts = self.runtime_accounts_snapshot();
        let retail = self.state.retail_experience.clone();
        let confirmations = self.state.personal_trade_confirmations.clone();
        let watermark = self.state.receipt_watermark;
        let account_undo = self.state.runtime_account_undo.clone();
        let account_ids = self.state.runtime_account_undo_ids.clone();
        let retail_undo = self.state.runtime_retail_undo.clone();
        let retail_ids = self.state.runtime_retail_undo_ids.clone();
        let receipt_undo = self.state.runtime_receipts_undo.clone();
        let receipt_ids = self.state.runtime_receipts_undo_ids.clone();
        move |session| {
            for (id, cash, positions) in accounts {
                let account = session
                    .fixture_account_mut(&id)
                    .expect("日结前名册账户仍在册");
                account.fixture_set_cash(crate::Money::from_cents(cash));
                account.fixture_set_positions(positions);
            }
            session.state.retail_experience = retail;
            session.state.personal_trade_confirmations = confirmations;
            session.state.receipt_watermark = watermark;
            session.state.runtime_account_undo = account_undo;
            session.state.runtime_account_undo_ids = account_ids;
            session.state.runtime_retail_undo = retail_undo;
            session.state.runtime_retail_undo_ids = retail_ids;
            session.state.runtime_receipts_undo = receipt_undo;
            session.state.runtime_receipts_undo_ids = receipt_ids;
        }
    }

    /// runtime 驱动器翻转面（P1-7）：日结失败回滚 day/seq。
    pub fn rollback_runtime_day_end(&mut self, day: u32, seq: u64) {
        self.mark_external_mutation();
        self.state.day = day;
        self.state.seq = seq;
    }

    /// runtime 驱动器翻转面（Sol 二审修复集 3）：全账户基线快照
    /// （现金+持仓——安装失败回滚用）。
    /// P2-2b：账户写时 undo 记录（首写幂等）——runtime 安装基线的账户
    /// 域回滚源（drive 回写/同拍投影两写入点在写前调用；成功 step 尾
    /// 清空、失败回滚消费）。O(step 内实际写入账户数)。
    pub fn runtime_record_account_undo(&mut self, id: crate::AccountId) {
        if self.state.runtime_account_undo_ids.contains(&id) {
            return;
        }
        let Some(acct) = self.account(id) else {
            return;
        };
        // 先取首写前值再入记录（借用分离）。
        let prev = (id, acct.cash().cents(), acct.positions().clone());
        self.state.runtime_account_undo_ids.insert(id);
        self.state.runtime_account_undo.push(prev);
    }

    /// P2-2b 红证读面（cfg(test)）：写时 undo 当前记录数。
    #[cfg(test)]
    pub fn runtime_account_undo_len_for_test(&self) -> usize {
        self.state.runtime_account_undo.len()
    }

    /// P2-2b：成功 step 尾清 undo（失败路径由 rollback 消费）。
    /// P2-3b：清全部写时 undo 槽（账户+五域）。成功 step 尾部与 step
    /// 入口各清一次——入口清消除驱动器手动序列（测试直调安装面）跨
    /// step 的记录残留（残留记录会把回滚恢复到错误的更早状态）。
    pub fn runtime_clear_account_undo(&mut self) {
        self.state.runtime_account_undo.clear();
        self.state.runtime_account_undo_ids.clear();
        self.state.runtime_attention_undo.clear();
        self.state.runtime_attention_undo_ids.clear();
        self.state.runtime_heap_undo = None;
        self.state.runtime_belief_undo.clear();
        self.state.runtime_belief_undo_ids.clear();
        self.state.runtime_retail_undo.clear();
        self.state.runtime_retail_undo_ids.clear();
        self.state.runtime_receipts_undo.clear();
        self.state.runtime_receipts_undo_ids.clear();
        self.state.runtime_history_undo.clear();
        self.state.runtime_history_undo_ids.clear();
        // step_frame 接轨件 4b：扩展回滚面同清。
        self.state.runtime_mirror_undo = None;
        self.state.runtime_plans_undo = None;
        self.state.runtime_envelope_undo = None;
        self.state.runtime_lifecycles_undo = None;
        self.state.runtime_retail_diag_undo = None;
    }

    pub fn runtime_accounts_snapshot(
        &self,
    ) -> Vec<(
        crate::AccountId,
        i64,
        std::collections::BTreeMap<crate::StockId, crate::account::Position>,
    )> {
        self.state
            .accounts
            .iter()
            .map(|(id, account)| (*id, account.cash().cents(), account.positions().clone()))
            .collect()
    }


    /// 注意力镜像全量只读（Belief-1b·下：投递下行快照源——candidate
    /// 域=上一 tick 组同步值）。
    pub fn runtime_attention_full_states(&self) -> Vec<(crate::AccountId, crate::session::NpcAttentionState)> {
        self.state
            .npc_attention
            .iter()
            .map(|(id, s)| (*id, s.clone()))
            .collect()
    }

    /// 公开库版本（Belief-1b·下：稀疏刷新判据）+当前 Arc。
    pub fn runtime_library_handle(
        &self,
    ) -> (u32, std::sync::Arc<crate::information::PublicLibrary>) {
        (self.state.library.version(), std::sync::Arc::clone(&self.state.library))
    }

    /// 从会话选定公司模型构造不含私有财务的共同公开事实。
    pub fn runtime_company_read_facts(
        &self,
    ) -> Result<std::sync::Arc<crate::company::CompanyReadFacts>, StepFatal> {
        self.state.company_read_facts().map(std::sync::Arc::new).map_err(|error| {
            StepFatal::InvariantViolation {
                description: error.to_string(),
                location: "GameSession::runtime_company_read_facts".to_owned(),
            }
        })
    }

    /// 数值股票到发行人的同源只读投影，已验证上市股不得遗漏。
    pub fn runtime_issuer_map(
        &self,
    ) -> std::collections::BTreeMap<crate::StockId, crate::company::CompanyId> {
        self.runtime_company_read_facts().unwrap_or_else(|error| panic!("发行人投影失败：{error}"))
            .issuers().map(|(stock, company)| (*stock, *company)).collect()
    }

    /// 公司 kind 与当前 issued_shares 的同源只读投影，包含未上市公司。
    pub fn runtime_company_specs(
        &self,
    ) -> std::collections::BTreeMap<crate::company::CompanyId, (crate::company::CompanyKind, u64)> {
        self.runtime_company_read_facts().unwrap_or_else(|error| panic!("公司规格投影失败：{error}"))
            .company_specs().map(|(company, spec)| (*company, *spec)).collect()
    }

    /// 观察时刻（投递载荷——披露相位 instant）。
    pub fn runtime_observation_instant(&self) -> crate::calendar::CivilInstant {
        self.observation_civil_instant()
    }

    /// Belief-1b·下：投递回写——**仅 information 域**（cadence/
    /// next_information_check），不触碰 candidate（组权威）/RNG——
    /// 权威调和注记见 .tmp/multi-tick-design.md。
    pub fn set_runtime_information_checks(
        &mut self,
        updates: &[(crate::AccountId, crate::session::NpcAttentionState)],
    ) {
        #[cfg(test)]
        eprintln!("PROBE set-info-checks n={} first={:?}", updates.len(), updates.first().map(|u| (u.0, u.1.next_information_check)));
        self.mark_external_mutation();
        for (id, updated) in updates {
            // P2-3b：首写前值入 undo（改写前记录——回滚源）。
            self.runtime_record_attention_undo(id);
            if let Some(state) = self.state.npc_attention.get_mut(id) {
                state.information_cadence = updated.information_cadence;
                state.next_information_check = updated.next_information_check;
            }
        }
    }

    /// 每股限价单笔上限投影（D3a-3 件4b：组内报价/预算输入——category
    /// 差异显式建模的主板/创业板上限）。
    pub fn runtime_max_order_qty_limit(
        &self,
    ) -> std::collections::BTreeMap<crate::StockId, u32> {
        self.state
            .setup
            .stocks
            .iter()
            .map(|stock| {
                (
                    self.state.stocks.id(stock.code.0.as_str()).expect("setup 股在目录"),
                    stock.category.max_order_qty(false),
                )
            })
            .collect()
    }

    /// (发行代码, 数值 id) 序列投影（Sol 整改 5：曝光面共享函数输入）。
    pub fn runtime_stock_codes(&self) -> Vec<(crate::account::StockCode, crate::StockId)> {
        self.state
            .setup
            .stocks
            .iter()
            .map(|stock| {
                (
                    stock.code.clone(),
                    self.state.stocks.id(stock.code.0.as_str()).expect("setup 股在目录"),
                )
            })
            .collect()
    }

    /// 每股最小报价单位投影（D3a-3 复核收口：组内报价取整口径——
    /// setup 权威，与 session stock.tick 同源）。
    pub fn runtime_tick_size_cents(
        &self,
    ) -> std::collections::BTreeMap<crate::StockId, i64> {
        self.state
            .setup
            .stocks
            .iter()
            .map(|stock| {
                (
                    self.state.stocks.id(stock.code.0.as_str()).expect("setup 股在目录"),
                    stock.tick.cents(),
                )
            })
            .collect()
    }

    /// 组内计划簿安装（D3a-2c 件5：交付点投影——runtime 模式下 session
    /// plans 的唯一来源；X1 直改标记外部直改。plans_debug 等读面消费）。
    pub fn set_runtime_plans(&mut self, plans: crate::plans::PlanBook) {
        // step_frame 接轨件 4b：投影失败回滚面——计划簿整值旧值入
        /// undo（幂等首写；PlanBook 规模小，无条件记录成本可忽略）。
        self.state
            .runtime_plans_undo
            .get_or_insert_with(|| self.state.plans.clone());
        self.state.plans = plans;
    }

    /// native 构造期读取完整计划镜像，成功构造后由所属组独占写入。
    pub(crate) fn runtime_plans_snapshot(&self) -> &crate::plans::PlanBook {
        &self.state.plans
    }

    /// D5 cross_stock：session 侧计划簿 take/restore（迁移期注入面保护
    /// 的计划——driver 段同拍成交合入用；组簿权威接管前）。
    pub fn take_runtime_plans(&mut self) -> crate::plans::PlanBook {
        std::mem::take(&mut self.state.plans)
    }

    /// 信念链参数/机构风格投影（D3a-2c 件4：组线程 Lifecycle 消费的策略面
    /// ——id→(链参数, 风格)；非信念机构 None）。
    pub fn runtime_chain_params(
        &self,
    ) -> std::collections::BTreeMap<
        crate::AccountId,
        (
            crate::strategy::BeliefChainParams,
            Option<crate::strategy::InstitutionStyle>,
        ),
    > {
        self.state
            .accounts
            .iter()
            .filter_map(|(id, account)| {
                let strategy = account.strategy()?;
                let params = strategy.belief_chain_params()?;
                Some((*id, (params, strategy.institution_style())))
            })
            .collect()
    }

    /// 紧迫度策略投影（D3a-2c 件4：组内 initial_urgency 输入——与 session
    /// `state.urgency_policy` 同一对象）。
    pub fn runtime_urgency_policy(&self) -> crate::plans::UrgencyPolicy {
        self.state.urgency_policy.clone()
    }

    /// 每股当前已发行股数；保留原始 u64，数量边界由策略与下单入口处理。
    pub fn runtime_total_shares(&self) -> std::collections::BTreeMap<crate::StockId, u64> {
        let facts = self.runtime_company_read_facts().unwrap_or_else(|error| panic!("当前股本投影失败：{error}"));
        facts.issuers().map(|(stock, company)| {
            let (_, issued_shares) = facts.company_spec(company).unwrap_or_else(|error| panic!("已验证发行人规格缺失：{error}"));
            (*stock, issued_shares)
        }).collect()
    }

    /// 构造期 NPC 只读输入只暴露目录与共同事实，不持有经营账簿。
    pub fn runtime_company_handles(
        &self,
    ) -> Result<(
        crate::stock::StockCatalog,
        std::sync::Arc<crate::company::CompanyReadFacts>,
    ), StepFatal> {
        Ok((self.state.stocks.clone(), self.runtime_company_read_facts()?))
    }

    /// 完整个人态只读投影（D3a-2c 件2：PlanPersonalState 六件——含 attention
    /// 与 history_reads，observe 直调的组内承载/同源红证源）。过滤面对齐
    /// `capture_decision_chain_roots` 的能力探针：只有暴露链参数的信念
    /// 机构策略进链（retail 位无 frozen policy——误投影导致 observe
    /// panic，D3a-3 接线显红后收口）。
    pub fn runtime_personal_states(
        &self,
    ) -> std::collections::BTreeMap<
        crate::AccountId,
        crate::session::decision_chain::personal_state::PlanPersonalState,
    > {
        self.state
            .npc_attention
            .iter()
            .filter(|(id, _)| {
                // D5 #4 retail 链：belief 位不限机构——retail 位同投影
                // （观察/决策/经验反馈的组内权威；玩家无 strategy 自然
                // 排除）。
                self.state.belief_participants.contains_key(id)
                    && self
                        .state
                        .accounts
                        .get(id)
                        .and_then(|account| account.strategy())
                        .is_some()
            })
            .map(|(id, attention)| {
                let participant = &self.state.belief_participants[id];
                (
                    *id,
                    crate::session::decision_chain::personal_state::PlanPersonalState {
                        attention: attention.clone(),
                        watchlist: participant.watchlist().clone(),
                        price_memory: participant.price_memory().clone(),
                        history_reads: self.state.history_reads.get(id).cloned().unwrap_or_default(),
                        information: participant.information().clone(),
                        belief: participant.belief().clone(),
                    },
                )
            })
            .collect()
    }

    /// D5 #4 retail 链：零售经验行只读（构造投影源；非 retail 位 None）。
    pub fn runtime_retail_experience_for(
        &self,
        id: &crate::AccountId,
    ) -> Option<crate::RetailExperienceState> {
        let is_retail = self
            .state
            .accounts
            .get(id)
            .is_some_and(|account| account.kind() == crate::account::AccountKind::Retail);
        if is_retail {
            self.state.retail_experience.get(id).cloned()
        } else {
            None
        }
    }

    /// 只借用 participant 名册，避免仅判断存在性时深克隆全部个人态。
    pub(crate) fn runtime_has_belief_participant(&self, account: crate::AccountId) -> bool {
        self.state.belief_participants.contains_key(&account)
    }

    pub(crate) fn next_runtime_authority_epoch(&self) -> Result<u64, String> {
        self.runtime_authority_epoch.checked_add(1).ok_or_else(|| "driver authority_epoch 已耗尽，拒绝构造替换".to_owned())
    }

    pub(crate) fn install_runtime_authority_epoch(&mut self, epoch: u64) {
        assert_eq!(self.runtime_authority_epoch.checked_add(1), Some(epoch), "driver epoch 已在 spawn 前验证且仅成功安装一次");
        self.runtime_authority_epoch = epoch;
        self.runtime_external_authority = std::sync::Weak::new();
        self.runtime_installed_market_close = None;
    }

    pub(crate) fn runtime_authority_identity(&self) -> std::sync::Weak<()> {
        std::sync::Arc::downgrade(&self.runtime_authority_identity)
    }

    pub(crate) fn matches_runtime_authority(&self, identity: &std::sync::Weak<()>, epoch: u64) -> bool {
        epoch == self.runtime_authority_epoch
            && std::sync::Weak::ptr_eq(identity, &self.runtime_authority_identity())
    }

    pub(crate) fn check_external_runtime_factory(&self) -> Result<(), String> {
        if let Some(cell) = &self.runtime_driver {
            let driver = cell
                .lock()
                .map_err(|_| "内部 runtime owner 锁 poison，拒绝未知状态替换".to_owned())?;
            driver.ensure_session_authority(self)?;
            if self.poison.is_none() || driver.world_committed_tick() <= self.tick() {
                return Err("公开 driver factory 不得替换活跃内部 runtime owner".to_owned());
            }
            // 显式 factory 可读取最后已提交 Session 种子；这不清除首错或恢复可玩。
            // 失败 World 的 cell/raw 保留到新 driver 全部准备成功，不能提前退役。
        }
        Ok(())
    }

    pub(crate) fn retire_replaced_internal_runtime_driver(&mut self) {
        // 仅在 factory 的全部可失败工作成功后调用；World Drop 先 shutdown/join。
        drop(self.runtime_driver.take());
    }

    pub(crate) fn install_external_runtime_authority(
        &mut self,
        lease: std::sync::Weak<crate::runtime::drive::RuntimeAuthorityLease>,
    ) {
        self.runtime_external_authority = lease;
    }

    pub(super) fn has_current_external_runtime_authority(&self) -> bool {
        self.runtime_external_authority
            .upgrade()
            .is_some_and(|lease| lease.matches(self))
    }

    pub(super) fn uses_runtime_civil_authority(&self) -> bool {
        !self.has_current_external_runtime_authority()
            && (self.state.runtime_information_authority || crate::runtime::step_runtime_enabled())
    }

    pub(crate) fn runtime_personal_presence(
        &self,
    ) -> Result<std::collections::BTreeMap<crate::AccountId, crate::runtime::personal_full_validation::PersonalPresence>, String> {
        use crate::runtime::personal_full_validation::{PersonalAbsenceReason, PersonalPresence};
        let mut expected = std::collections::BTreeMap::new();
        for (id, account) in self.state.accounts.iter() {
            let Some(strategy) = account.strategy() else {
                if self.state.belief_participants.contains_key(id) {
                    return Err(format!("个人能力名册：无实际 strategy 却有 participant：{id:?}"));
                }
                expected.insert(*id, PersonalPresence::Absent(PersonalAbsenceReason::NoStrategy));
                continue;
            };
            let participant = self.state.belief_participants.get(id);
            let required = strategy.retail_style().is_some() || strategy.belief_chain_params().is_some();
            if required && participant.is_none() {
                return Err(format!("个人能力名册：实际 strategy 要求 participant 但缺行：{id:?}"));
            }
            if let Some(participant) = participant {
                if !self.state.npc_attention.contains_key(id) {
                    return Err(format!("个人能力名册：participant 缺 attention：{id:?}"));
                }
                if participant.information().owner() != *id || participant.belief().npc() != *id {
                    return Err(format!("个人能力名册：participant 属主不匹配：{id:?}"));
                }
                expected.insert(*id, PersonalPresence::Required);
            } else {
                expected.insert(*id, PersonalPresence::Absent(PersonalAbsenceReason::NoBeliefParticipant));
            }
        }
        Ok(expected)
    }

    /// 信念个人态只读投影（D3a-2b Belief-1：构造投影/同源校验源）。
    pub fn runtime_belief_states(
        &self,
    ) -> std::collections::BTreeMap<
        crate::AccountId,
        (
            crate::experience::PersonalWatchlist,
            crate::experience::PersonalPriceMemory,
            crate::information::NpcInformationState,
            crate::strategy::BeliefBook,
        ),
    > {
        self.state
            .belief_participants
            .iter()
            .map(|(id, participant)| {
                (
                    *id,
                    (
                        participant.watchlist().clone(),
                        participant.price_memory().clone(),
                        participant.information().clone(),
                        participant.belief().clone(),
                    ),
                )
            })
            .collect()
    }

    /// runtime 日 K 种子投影（D3a-2）：每股（全量日 K (o,h,l,c,vol)，
    /// 累计 traded 日数）——desk 链观察日 K 权威的初始承载（bridge/
    /// 世界构造用；不过滤零成交日——行为路径与技术面口径在观察构建
    /// 侧分流）。
    pub fn runtime_daily_histories(
        &self,
    ) -> std::collections::BTreeMap<
        crate::StockId,
        (Vec<(i64, i64, i64, i64, u64)>, u64),
    > {
        self.state
            .candle_book
            .histories()
            .iter()
            .map(|(code, history)| {
                let bars: Vec<(i64, i64, i64, i64, u64)> = history
                    .iter()
                    .map(|candle| {
                        (
                            candle.open.cents(),
                            candle.high.cents(),
                            candle.low.cents(),
                            candle.close.cents(),
                            candle.volume,
                        )
                    })
                    .collect();
                (*code, (bars, history.traded_count() as u64))
            })
            .collect()
    }

    /// runtime 分钟历史只读（D4·t4 位置合并的前缀源）。
    pub fn runtime_minute_history(
        &self,
        code: &crate::StockId,
    ) -> &[crate::observation::MarketMinuteClose] {
        self.state
            .market_minute_closes
            .get(code)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// B2：在册股数（驱动器保留快照组装的维度源）。
    pub fn stock_count_for_runtime(&self) -> usize {
        self.state.setup.stocks.len()
    }

    /// B2：日K历史只读（保留快照日K对账的数据源——按时间倒序）。
    pub fn candle_book_histories_for_runtime(
        &self,
        code: &crate::StockId,
    ) -> Vec<crate::session::DailyCandle> {
        self.state
            .candle_book
            .histories()
            .get(code)
            .map(|h| h.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// B2：保留历史链只读（查询面/发布对账的数据源）。
    pub fn retained_market_days(
        &self,
    ) -> &[crate::runtime::publication::SharedRetainedDay] {
        &self.state.retained_market_days
    }

    /// B2：日结冻结条目安装（交易日=驱动器编排；休市日=end_civil_day
    /// 补条目——两者都经此 push 镜像）。
    pub fn set_runtime_retained_day(
        &mut self,
        snapshot: std::sync::Arc<crate::runtime::publication::RetainedDaySnapshot>,
    ) {
        self.mark_external_mutation();
        self.state
            .retained_market_days
            .push(crate::runtime::publication::SharedRetainedDay(snapshot));
    }

    /// 日界批：以日结 Drain 终 K 补真 install 面（candle_book 历史中
    /// `time` 匹配的**末条**整值替换——`commit_active` 先于 Drain 提交
    /// 的是日内段形态，收盘竞价段在此并入）。缺条目=迁移提交面缺失，
    /// 显式失败。
    pub fn replace_runtime_committed_daily_candles(
        &mut self,
        candles: &[(crate::StockId, crate::session::DailyCandle)],
    ) -> Result<(), String> {
        self.mark_external_mutation();
        for (code, candle) in candles {
            if !self
                .state
                .candle_book
                .patch_history_entry(code, candle.clone())
            {
                return Err(format!(
                    "股 {code:?} 日 K 补真缺当日条目（time={}）——迁移提交面缺失",
                    candle.time
                ));
            }
        }
        Ok(())
    }

    /// B2 件 4 组装核心（单一事实源——runtime driver 编排与**串行安装**
    /// 共用同一实现；原 `RuntimeDriver::assemble_retained_day_at` 挪入）：
    /// 每股开市判定+桶校验（+交易日日K对账：量/额/笔/OHLC 逐位——旧
    /// `validate_bars_against_candle` 语义）+保留快照组装。
    pub(crate) fn assemble_retained_day_snapshot(
        &self,
        bars: Vec<Option<Vec<crate::runtime::MinuteBarFact>>>,
        date: crate::calendar::CivilDate,
    ) -> Result<std::sync::Arc<crate::runtime::publication::RetainedDaySnapshot>, String> {
        let candle_time = self.candle_date_time_for_runtime();
        let closed = self.state.candle_book.histories().iter().filter_map(|(code, history)| {
            history.iter_rev().find(|candle| candle.time == candle_time).map(|candle| (*code, candle.clone()))
        }).collect();
        self.assemble_retained_from_prepared(bars, date, &closed, false)
    }

    pub(in crate::session) fn assemble_retained_from_prepared(
        &self,
        bars: Vec<Option<Vec<crate::runtime::MinuteBarFact>>>,
        date: crate::calendar::CivilDate,
        closed: &std::collections::BTreeMap<crate::StockId, crate::session::DailyCandle>,
        require_candles: bool,
    ) -> Result<std::sync::Arc<crate::runtime::publication::RetainedDaySnapshot>, String> {
        let n = self.stock_count_for_runtime();
        if bars.len() != n {
            return Err(format!(
                "保留快照股数不齐：pending={} 而 in-session={n}",
                bars.len()
            ));
        }
        let mut securities = std::collections::BTreeMap::new();
        for (idx, slot) in bars.into_iter().enumerate() {
            let bars_vec = slot.ok_or_else(|| format!("保留快照缺股 {idx} 的桶投影"))?;
            let trading = self
                .runtime_stock_trading_on(idx as u32, date)
                .map_err(|e| format!("股 {idx} 开市判定失败：{e:?}"))?;
            if !trading && !bars_vec.is_empty() {
                return Err(format!("休市股 {idx} 出现分钟桶（{} 条）", bars_vec.len()));
            }
            // 交易日对账：bars 累计与当日日K逐位（量/额/笔/OHLC——旧
            /// validate_bars_against_candle 语义；占位日K（volume=0）与
            /// 空 bars 互恰）。
            if trading {
                let code = crate::StockId(idx as u32);
                let candle = closed.get(&code);
                let Some(candle) = candle else {
                    if require_candles { return Err(format!("Full 日结候选缺当日 K：{code:?}")); }
                    // 【既有登记（manual 面）】手动 day_end 编排不做
                    /// boundary 迁移（日K未提交 install 面）——对账基缺席
                    /// 跳过（生产 step 的 ends_day 段恒有提交条目+日界批
                    /// 终 K 补真；manual 面差异登记于
                    /// agents/day-boundary-kline/ledger.md 遗留节）。
                    continue;
                };

                let volume: u64 = bars_vec.iter().map(|b| b.volume_shares).sum();
                let turnover: u128 = bars_vec.iter().map(|b| b.turnover_cents).sum();
                let count: u64 = bars_vec.iter().map(|b| b.trade_count).sum();
                // 日界批收口：原"占位 K（volume=0）配非零 bars 放行"的
                /// 豁免退役——日结 Drain 终 K 已补真 install 面（含收盘
                /// 竞价段），全形态逐位对账（B2 台账"占位形态跳过逐位"
                /// 收紧；占位 K 仅与空 bars 互恰仍自然通过）。
                let (c_turnover, c_count) = candle
                    .trade_stats
                    .as_ref()
                    .map(|s| (u128::from(s.turnover_cents), s.trade_count))
                    .ok_or_else(|| format!("股 {idx} 日K缺成交统计"))?;
                if volume != candle.volume || turnover != c_turnover || count != c_count {
                    return Err(format!(
                        "股 {idx} 分钟累计量/额/笔与日K不一致：bars=({volume},{turnover},{count}) candle=({},{c_turnover},{c_count})",
                        candle.volume
                    ));
                }
                if let (Some(first), Some(last)) = (bars_vec.first(), bars_vec.last()) {
                    let high = bars_vec.iter().map(|b| b.high.cents()).max();
                    let low = bars_vec.iter().map(|b| b.low.cents()).min();
                    if first.open != candle.open
                        || last.close != candle.close
                        || high != Some(candle.high.cents())
                        || low != Some(candle.low.cents())
                    {
                        return Err(format!("股 {idx} 分钟 OHLC 与日K不一致"));
                    }
                }
            }
            securities.insert(
                idx as u32,
                crate::runtime::publication::RetainedMinuteSession {
                    trading,
                    bars: std::sync::Arc::new(bars_vec),
                },
            );
        }
        Ok(std::sync::Arc::new(crate::runtime::publication::RetainedDaySnapshot {
            date,
            securities,
        }))
    }

    /// B2：指定自然日每股开市判定（retained 冻结的休市/开市口径——
    /// 沪深日历按股上市交易所；与公司域 history 同一 `TradingCalendar`
    /// 数据源）。
    pub fn runtime_stock_trading_on(
        &self,
        stock_idx: u32,
        date: crate::calendar::CivilDate,
    ) -> Result<bool, crate::calendar::CalendarError> {
        // StockId 属于按代码排序的 catalog；按完整 code 找回显式 exchange。
        let missing_stock = || crate::calendar::CalendarError::BeforeInitFloor {
            date,
            floor: date,
        };
        let code = self.state.stocks.code(crate::StockId(stock_idx)).ok_or_else(missing_stock)?;
        let stock = self.state.setup.stocks.iter()
            .find(|stock| stock.code.0 == code)
            .ok_or_else(missing_stock)?;
        self.state
            .civil_clock
            .calendar()
            .is_trading_day(super::session_calendar_exchange(stock.exchange), date)
    }

    /// B2：当日分钟K桶镜像**增量追加**（install 位置合并的低拷贝形态——
    /// 调用方保证 tail 为镜像尾部延续；跨日 desk 空 tail=无追加）。
    pub fn runtime_append_minute_bars(
        &mut self,
        code: &crate::StockId,
        tail: &[crate::runtime::MinuteBarFact],
    ) {
        if tail.is_empty() {
            return;
        }
        self.mark_external_mutation();
        self.state
            .market_minute_bars
            .entry(*code)
            .or_default()
            .extend_from_slice(tail);
    }

    /// B2：当日分钟K桶镜像只读（install 位置合并的前缀源——同
    /// `runtime_minute_history` 式）。
    pub fn runtime_minute_bars(
        &self,
        code: &crate::StockId,
    ) -> &[crate::runtime::MinuteBarFact] {
        self.state
            .market_minute_bars
            .get(code)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// D5 restore 链最后一层：日内行情聚合种子读面（每股——save 点的
    /// desk 行情权威投影源）。活动 K 为空（日界空窗/竞价窗）→ None
    /// （desk 从构造零点=昨收锚定起，恰为新日起点语义）。昨收=日 K
    /// 历史末条 close，首日无历史时退初始价。
    /// 构造 World 与首次 mirror 共用的行情/公开截点种子；不读取 v0 占位视图。
    pub(crate) fn runtime_static_market_seed(&self, code: crate::StockId) -> Result<(i64, u64), String> {
        let market = self.state.markets.get(&code)
            .ok_or_else(|| format!("初始静点行情缺股票：{code:?}"))?;
        Ok((market.last_price().cents(), self.runtime_public_cutoff_for(&code)))
    }

    pub fn runtime_market_seeds(
        &self,
    ) -> std::collections::BTreeMap<crate::StockId, crate::runtime::desk::DeskMarketSeed> {
        let mut seeds = std::collections::BTreeMap::new();
        for (code, active) in self.state.candle_book.active() {
            let last_close = self
                .state
                .candle_book
                .histories()
                .get(code)
                .and_then(|h| h.iter_rev().next())
                .map(|c| c.close.cents())
                .or_else(|| {
                    // 首日无历史：昨收=初始价（StockId=股序数值——setup
                    /// stocks 的下标约定）。
                    self.state
                        .setup
                        .stocks
                        .get(code.0 as usize)
                        .map(|s| s.initial_price.cents())
                });
            let Some(last_close) = last_close else {
                continue;
            };
            let (turnover, trade_count) = active
                .trade_stats
                .as_ref()
                .map(|s| (s.turnover_cents as i64, s.trade_count))
                .unwrap_or((0, 0));
            seeds.insert(
                *code,
                crate::runtime::desk::DeskMarketSeed {
                    last_price: active.close.cents(),
                    last_close,
                    day_high: active.high.cents(),
                    day_low: active.low.cents(),
                    volume_today: active.volume,
                    turnover_today: turnover,
                    trade_count,
                    candle: Some((
                        active.open.cents(),
                        active.high.cents(),
                        active.low.cents(),
                        active.close.cents(),
                        active.volume,
                    )),
                    minute_closes: self
                        .runtime_minute_history(code)
                        .iter()
                        .map(|m| (m.absolute_trading_minute, m.close.cents()))
                        .collect(),
                    // B2：当日分钟K桶投影（save 链——session 镜像权威源）。
                    minute_bars: self
                        .state
                        .market_minute_bars
                        .get(code)
                        .cloned()
                        .unwrap_or_default(),
                },
            );
        }
        seeds
    }

    /// runtime 日结：清会话分钟历史（D4——旧管线 day_end 清、desk
    /// roll_day 清；位置合并的跨日对齐前提）。
    pub fn clear_runtime_minute_history(&mut self) {
        self.mark_external_mutation();
        for history in self.state.market_minute_closes.values_mut() {
            history.clear();
        }
        // B2：分钟K桶同步清（desk roll_day 对齐）。
        for bars in self.state.market_minute_bars.values_mut() {
            bars.clear();
        }
    }

    /// runtime 持仓回写配套（D1）：runtime 新增持仓时补经历条目——存档
    /// 校验 fault2038 依赖"持仓必有经历"。成本基近似=数量×现价（runtime
    /// 世界不持成本账——登记差异，D3 结算域以 FillFact 精确收口）。
    pub fn runtime_hold_experience_init(
        &mut self,
        id: &crate::AccountId,
        code: &crate::StockId,
        qty: u32,
        price_cents: i64,
    ) -> Result<(), String> {
        let moment = crate::experience::ExperienceMoment {
            civil_date: self.state.civil_clock.current_date(),
            market_minute: self.current_market_minute(),
            trading_day: u64::from(self.state.day),
        };
        self.runtime_hold_experience_init_at(id, code, qty, price_cents, moment)
    }

    /// 日结首买使用收盘事实的时刻，避免安装时 day 已推进导致经历跨日。
    pub(crate) fn runtime_hold_experience_init_at(
        &mut self,
        id: &crate::AccountId,
        code: &crate::StockId,
        _qty: u32,
        price_cents: i64,
        moment: crate::experience::ExperienceMoment,
    ) -> Result<(), String> {
        // P2-3b：首写前值入 undo（改写前记录——回滚源）。
        self.runtime_record_retail_undo(id);
        let Some(ledger) = self.state.retail_experience.get_mut(id) else {
            // 无经历账的账户（玩家惯例位）无 fault2038 约束——跳过。
            return Ok(());
        };
        // 入场参考=现价（fault2049 要求 peak≥entry；runtime 不持成交价
        // 账——近似登记，D3 结算域以 FillFact 精确收口）。
        let price = crate::money::Money::from_cents(price_cents.max(1));
        ledger
            .initialize_holding_dated(code, Some(price), price, moment)
            .map_err(|e| format!("经历条目初始化失败：{e:?}"))
    }

    /// Sol 终审四审经历族（fault2051）：清仓经历的**同拍投影面**——组端
    /// apply_full_exit_experience 在最终 staging 落账（晚于本拍 Belief
    /// 上行），save 面需要本拍即见的转移（与账户面 apply_same_tick_
    /// fill_projection 同架构：组权威+同拍投影双写，下一拍组行整值
    /// 同步后内容一致）。对齐旧管线 record_fill 的 Sell 且 after=0
    /// 分支：清观察字段+设退出冷静期与 last_sell 身份。
    pub fn runtime_record_full_exit(
        &mut self,
        id: &crate::AccountId,
        code: &crate::StockId,
        price_cents: i64,
        before_qty: u32,
        cost_before: Option<crate::money::Money>,
        order_id: u64,
    ) -> Result<(), String> {
        let minute = self.current_market_minute();
        // P2-3b：首写前值入 undo（改写前记录——回滚源）。
        self.runtime_record_retail_undo(id);
        let Some(ledger) = self.state.retail_experience.get_mut(id) else {
            // 无经历账的账户（玩家惯例位）无 fault2051 约束——跳过。
            return Ok(());
        };
        ledger
            .record_fill_with_order(
                code,
                crate::orderbook::Side::Sell,
                crate::money::Money::from_cents(price_cents),
                before_qty,
                0,
                cost_before,
                minute,
                Some(order_id),
            )
            .map_err(|e| format!("清仓经历转移失败：{e:?}"))
    }

    /// runtime 驱动器翻转面（Sol 二审修复集 3）：价格历史长度只读
    /// （fail-fast 断言——step 失败后安装不得写会话状态）。
    pub fn runtime_price_history_len(&self, code: &crate::StockId) -> usize {
        self.state
            .price_history
            .get(code)
            .map(|h| h.len())
            .unwrap_or(0)
    }

    /// runtime 驱动器翻转面（翻转批）：交易日计数推进。
    pub fn advance_runtime_market_day(&mut self) {
        self.mark_external_mutation();
        self.state.day += 1;
    }

    /// runtime 驱动器翻转面（批 8h）：tick 前进（经直改标记）。
    pub fn set_runtime_tick(&mut self, tick: u64) {
        self.mark_external_mutation();
        self.state.tick = tick;
    }

    /// runtime 驱动器翻转面（批 8h）：事件权威 seq 写回。
    pub fn set_runtime_seq(&mut self, seq: u64) {
        self.mark_external_mutation();
        self.state.seq = seq;
    }

    /// 当前事件权威 seq（驱动器游标初始化）。
    pub fn current_seq(&self) -> u64 {
        self.state.seq
    }


    /// runtime 驱动器翻转面（批 8h）：披露 observer 注册透传。
    pub fn add_disclosure_observer_for_runtime(
        &mut self,
        observer: fn(crate::calendar::CivilInstant),
    ) {
        self.state.civil_clock.add_disclosure_observer(observer);
    }

    /// runtime 驱动器翻转面（批 8h）：行情直改入口（X1 标记）。
    pub fn market_mut_for_runtime(
        &mut self,
        code: &crate::StockId,
    ) -> Option<&mut crate::Market> {
        self.state.markets.get_mut(code)
    }

    /// runtime 日 K 历史长度只读（D3a-2：install_candles 位置合并——
    /// desk 含种子历史时跳过会话已有天数，防重复入档）。
    pub fn runtime_candle_history_len(&self, code: &crate::StockId) -> usize {
        self.state
            .candle_book
            .histories()
            .get(code)
            .map(|h| h.len())
            .unwrap_or(0)
    }

    /// 全股日 K 历史长度只读（Sol 终审四审 A2：驱动器构造期的归档
    /// 上限基准）。
    pub fn runtime_candle_histories(&self) -> Vec<(crate::StockId, usize)> {
        self.state
            .candle_book
            .histories()
            .iter()
            .map(|(code, h)| (*code, h.len()))
            .collect()
    }

    /// runtime 驱动器翻转面（批 8 终点）：历史 K 回写。
    pub fn push_historical_candle_for_runtime(
        &mut self,
        code: &crate::StockId,
        candle: crate::session::DailyCandle,
    ) {
        self.mark_external_mutation();
        self.state.candle_book.push_history(code, candle);
    }

    /// runtime 驱动器翻转面（批 8 终点）：活动 K 安装。
    pub fn set_active_candle_for_runtime(
        &mut self,
        code: &crate::StockId,
        candle: crate::session::DailyCandle,
    ) {
        self.mark_external_mutation();
        self.state.candle_book.install_active(code.clone(), candle);
    }

    /// runtime 驱动器翻转面（D5 日界）：活动 K 清除（world 日结已把当日
    /// K 归档进 histories——session 侧残留 active 会在 TradingDayEnd 的
    /// commit 二次归档（save fault2064 双计显红））。
    pub fn clear_active_candle_for_runtime(&mut self, code: &crate::StockId) {
        self.mark_external_mutation();
        self.state.candle_book.remove_active(code);
    }

    /// runtime 驱动器翻转面（翻转批）：价格历史整值替换。
    pub fn set_runtime_price_history(
        &mut self,
        code: &crate::StockId,
        prices: Vec<crate::money::Money>,
    ) {
        self.mark_external_mutation();
        self.state
            .price_history
            .insert(code.clone(), prices.into_iter().collect());
    }

    /// runtime 驱动器翻转面（翻转批）：市场分钟收盘整值替换。
    pub fn set_runtime_minute_closes(
        &mut self,
        code: &crate::StockId,
        closes: Vec<crate::observation::MarketMinuteClose>,
    ) {
        self.mark_external_mutation();
        self.state.market_minute_closes.insert(code.clone(), closes);
    }

    /// B2：当日分钟K桶安装（驱动器 install 段——发布版本读面投影进
    /// session 镜像；save/查询面的权威源）。
    pub fn set_runtime_minute_bars(
        &mut self,
        code: &crate::StockId,
        bars: Vec<crate::runtime::MinuteBarFact>,
    ) {
        self.mark_external_mutation();
        self.state.market_minute_bars.insert(code.clone(), bars);
    }

    /// runtime 驱动器翻转面（翻转批）：注意力候选推进。
    pub fn advance_runtime_attention_candidates(&mut self) {
        self.mark_external_mutation();
        let tick = self.state.tick;
        let ids: Vec<crate::AccountId> = self
            .state
            .npc_attention
            .iter()
            .filter(|(_, s)| s.next_attention_candidate_tick < tick)
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            if let Some(state) = self.state.npc_attention.get_mut(&id) {
                state.next_attention_candidate_tick = tick;
            }
        }
    }

    /// runtime 驱动器翻转面（翻转批）：活动 K 自然日时间戳。
    pub fn candle_date_time_for_runtime(&self) -> i64 {
        crate::session::candles::candle_date_time(self.state.civil_clock.current_date())
    }

    /// runtime 驱动器翻转面（step_frame 接轨件 3）：本股活动 K 只读
    /// （PriceTick.daily_candle 的 session 侧来源——install_candles 已装；
    /// 空拍缺席由调用方合成占位，对齐旧管线 mark_candle(None, ..)）。
    pub fn runtime_active_candle(&self, code: &crate::StockId) -> Option<crate::session::DailyCandle> {
        self.state.candle_book.active().get(code).cloned()
    }

    /// runtime 驱动器翻转面（批 8h）：失败 poison（旧管线语义）。
    pub fn poison_runtime_step(&mut self, fatal: StepFatal) -> StepFatal {
        self.poison_failed_step(fatal)
    }


    pub(super) fn poison_failed_step(&mut self, fatal: StepFatal) -> StepFatal {
        if let Some(first) = &self.poison {
            return first.clone();
        }
        self.poison = Some(fatal.clone());
        if let Some(binding) = &self.ingress { binding.source.publish_terminal(fatal.clone()); }
        fatal
    }

    /// pending 更正尚未形成可持久的完成事实；先日终提交或取消，再生成当前状态存档。
    pub fn save(&self) -> Result<SaveSlot, SessionError> {
        self.require_healthy()?;
        if self.has_pending_report_corrections() {
            return Err(SessionError::PendingReportCorrections);
        }
        let mut save = self.save_committed_projection()?;
        self.project_shared_ingress_save(&mut save)?;
        Ok(save)
    }

    pub(in crate::session) fn save_committed_projection(&self) -> Result<SaveSlot, StepFatal> {
        self.require_healthy()?;
        let runtime_state = super::persistence::capture_runtime_state(self)?;
        Ok(self.save_projection(runtime_state))
    }

    #[cfg(test)]
    pub(super) fn inject_step_failure(&mut self, fatal: StepFatal) {
        self.injected_failure = Some(fatal);
    }

    #[cfg(test)]
    pub(crate) fn inject_post_shadow_failure(&mut self, fatal: StepFatal) {
        self.post_shadow_failure = Some(fatal);
    }

    /// 测试注入：runtime 步进成功后强制安装失败（Sol 二审修复集 3——
    /// 验证安装失败撤销已推进的 tick/seq/账户基线）。
    #[cfg(test)]
    pub(crate) fn inject_runtime_install_failure(&mut self) {
        self.runtime_install_failure = true;
    }

    #[cfg(test)]
    fn run_pre_mutation_hook(&mut self) -> Result<(), StepFatal> {
        match self.injected_failure.take() {
            Some(fatal) => Err(fatal),
            None => Ok(()),
        }
    }

    #[cfg(test)]
    pub(super) fn run_post_shadow_hook(&mut self) -> Result<(), StepFatal> {
        match self.post_shadow_failure.take() {
            Some(fatal) => Err(fatal),
            None => Ok(()),
        }
    }
}

/// P2-4b：在途单事实 → 信封构造（全量重建与增量 upsert 共用——数值面
/// 同源保证两路径逐位一致：nominal 自费用表重算、charged 直取 desk
/// 累计、OrderId=serial 域、live resources 买=purse/卖=余量、
/// origin=TickStart）。owner 名册外（account_by_owner 缺席）返回
/// Ok(None)——两路径同语义跳过（账本不含名册外账户的信封）。
/// Sol 终审修复 1c：归户保留组号（此前 account_by_local 丢组号）。
/// P2-4：单条在途事实 → 信封（owner→账户由调用方解析——快照 id_map
/// 或 driver 均可作源；构造逻辑与原全量重建/增量 upsert 同一函数体）。
fn runtime_resting_envelope(
    account: crate::AccountId,
    config: &crate::config::GameConfig,
    stock_idx: usize,
    fact: &crate::runtime::RestingFact,
) -> Result<Option<crate::session::pipeline::Envelope>, String> {
    let key = crate::session::pipeline::EnvelopeKey {
        account,
        stock: crate::StockId(u32::from(stock_idx as u16)),
        order: crate::OrderId(fact.order_serial),
        side: fact.side,
    };
    let nominal = crate::runtime::fees::FeeQuote::nominal(
        config,
        fact.side,
        crate::money::Money::from_cents(fact.filled_value),
    )
    .map_err(|e| format!("名义费用重算失败：{e:?}"))?;
    let audit = crate::session::pipeline::EnvelopeAudit {
        // Sol 终审四审符号价族（session 集成 sparse_fifo/maker_split 收口）：
        /// limit=受理价真值（desk 解析后的事实）——restore 侧 desk 种子
        /// 价格面与 verified_live 的 escrow 核对都依赖真值；desk 挂簿时
        /// purse 已收敛到 buy_purse_limit(受理价, 余量, 成交额)（与旧
        /// 管线 buy_order_reservation 同式），runtime 行因此与旧管线行
        /// 同过证明，A2 时期的"nominal/limit 双零跳过通道"与证明空窗
        /// 就此退役。
        limit: crate::money::Money::from_cents(fact.price),
        remaining_qty: fact.qty,
        // Sol 五审件 1：真实成交量入审计（desk 权威计数——restore 侧
        /// desk 种子/存档链的 filled_qty 源；此前恒 0）。
        filled_qty: fact.filled_qty,
        filled_value: crate::money::Money::from_cents(fact.filled_value),
        nominal: crate::session::pipeline::FeeComponents {
            commission: nominal.commission,
            stamp_tax: nominal.stamp_tax,
            transfer_fee: nominal.transfer_fee,
        },
        charged: crate::session::pipeline::FeeComponents {
            commission: crate::money::Money::from_cents(fact.charged_commission),
            stamp_tax: crate::money::Money::from_cents(fact.charged_stamp_tax),
            transfer_fee: crate::money::Money::from_cents(fact.charged_transfer_fee),
        },
    };
    // live resources：买=purse（desk 预留）、卖=余量股数（ledger 校验
    // live 行须有资源——全零非法）。origin=TickStart（save quiet point
    // 校验的 rebased 形态）。
    let (cash, shares) = if fact.side == crate::orderbook::Side::Buy {
        (crate::money::Money::from_cents(fact.purse), 0u32)
    } else {
        (crate::money::Money::ZERO, fact.qty)
    };
    Ok(Some(
        crate::session::pipeline::Envelope::tick_start_existing(key, cash, shares, audit),
    ))
}

impl GameSession {
    /// P2-4（视图内生战役）：信封派生投影——发布版本快照 → Envelope
    /// 列表（ledger 每拍维护退役后的查询/hash 数据源；构造逻辑与原
    /// 全量重建同一函数——内容等价由同源性承载）。无快照（restore 后
    /// 未步进）=空投影（runtime 存档旧簿恒空语义）。
    pub fn runtime_derive_envelopes(
        &self,
    ) -> Result<Vec<crate::session::pipeline::Envelope>, StepFatal> {
        let Some(snapshot) = self.state.runtime_resting_snapshot.as_ref() else {
            return Ok(Vec::new());
        };
        let mut envelopes = Vec::new();
        for (stock_idx, book) in snapshot.facts.iter().enumerate() {
            for fact in book.iter() {
                let global = fact.owner.0 as usize * snapshot.accounts_per_group
                    + fact.owner.1 as usize;
                // 名册外 owner：与原全量重建同语义跳过。
                let Some(&account) = snapshot.id_map.get(global) else {
                    continue;
                };
                match runtime_resting_envelope(account, &self.state.setup.config, stock_idx, fact)
                {
                    Ok(Some(envelope)) => envelopes.push(envelope),
                    Ok(None) => {}
                    Err(description) => {
                        return Err(StepFatal::InvariantViolation {
                            location: "GameSession::runtime_derive_envelopes".to_owned(),
                            description,
                        });
                    }
                }
            }
        }
        Ok(envelopes)
    }
}
