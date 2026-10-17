use super::{Event, GameSession, SaveSlot};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, thiserror::Error)]
pub enum StepFatal {
    #[error("invariant violation at {location}: {description}")]
    InvariantViolation {
        description: String,
        location: String,
    },
}

/// 安装失败回滚基线（Sol 三审 (b)）：step 推进与安装面写入的全部
/// 会话域整值快照（见 `GameSession::runtime_install_baseline`）。
pub(crate) struct RuntimeInstallBaseline {
    tick: u64,
    seq: u64,
    // P2-2b：accounts 全名册快照退役——改写时 undo（drive 两写入点经
    // runtime_record_account_undo 记录首写前值，O(dirty)；回滚恢复。
    // 原 O(名册) 每 tick 快照是驱动器侧 AccountBook::get 全名册遍历+
    // positions 深拷贝的主要来源（P2-2a 后画像 19% 读写下的大项）。
    markets: std::collections::BTreeMap<crate::StockId, crate::Market>,
    price_history:
        std::collections::BTreeMap<crate::StockId, std::collections::VecDeque<crate::money::Money>>,
    minute_closes: std::collections::BTreeMap<
        crate::StockId,
        Vec<crate::observation::MarketMinuteClose>,
    >,
    candle_histories: std::collections::BTreeMap<crate::StockId, super::candles::DailyCandleHistory>,
    candle_active: std::collections::BTreeMap<crate::StockId, crate::session::DailyCandle>,
    npc_attention: super::AccountPagedMap<super::attention::NpcAttentionState>,
    // D5 #4 retail 链：个人态写入面的回滚基线。
    // P1-C 修正：heap 仍随基线快照——回滚必须逐项还原（旧管线/混合
    // 路径的基线 heap 可含陈旧项——"从权威重建"会丢陈旧项使
    // business_state_hash 回滚对比显红，实测 compute_context_shared_
    // sessions 用例）。无界增长由 set_runtime_attention 的镜像重建
    // 消除（heap 恒 ≤名册——每 tick clone 成本 O(N) 恒定，不再 O(N·T)）。
    attention_scheduler: super::attention::NpcAttentionScheduler,
    belief_participants: super::AccountPagedMap<super::decision_chain::personal_state::BeliefParticipantState>,
    retail_experience: super::AccountPagedMap<crate::RetailExperienceState>,
    history_reads: super::AccountPagedMap<crate::experience::PersonalHistoryReadLedger>,
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
        // attention（调度域 candidate 保持组权威值——P1-10 链路回写的
        /// 镜像同步；information 域经 Belief-1b 投递演进——此处整态
        /// 覆盖为组快照，两域同源）。
        self.state.npc_attention.insert(id, snapshot.attention.clone());
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
    pub fn runtime_envelope_ledger_snapshot(
        &self,
    ) -> Vec<(crate::session::pipeline::EnvelopeKey, crate::session::pipeline::Envelope)> {
        self.state
            .envelope_ledger
            .iter()
            .map(|(key, envelope)| (key.clone(), envelope.clone()))
            .collect()
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
        let facts = driver.collect_resting_facts().expect("在途簿收集");
        let idx = code.0 as usize;
        facts
            .get(idx)
            .map(|book| {
                book.iter()
                    .map(|fact| {
                        let account = driver
                            .account_by_local(u16::try_from(fact.owner.1).expect("local 下标"))
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
        mutate: Box<dyn FnOnce(&mut crate::plans::PlanBook) + Send>,
    ) {
        let mut driver = self
            .runtime_driver
            .as_mut()
            .and_then(|cell| cell.lock().ok())
            .expect("runtime 驱动器在位（调用方已判）");
        driver.mutate_plan_book_for_test(mutate);
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

    /// runtime 驱动翻转入口（翻转批）：环境变量 ENGINE_STEP_RUNTIME=1
    /// 时 `step()` 走 RuntimeDriver（常驻 World+安装面），否则旧管线
    /// （缺省 0=旧管线，见 `runtime::step_runtime_enabled`）。
    /// `step_frame_and_commit`（candidate 投影契约/Escrow 验证）恒走
    /// 旧管线为 fallback——翻转范围=生产便捷入口。
    /// Sol 二审修复集 3：前置校验**真正共用** `check_step_start`
    /// （健康→注入失败→缺策略，与旧管线同判同序——此前 runtime 路径
    /// 自带一份缺策略检查且先于注入失败执行，两路径不同判）。
    pub fn step_via_runtime(&mut self) -> Result<Vec<Event>, StepFatal> {
        // 翻转形态（正确性优先）：每次构造驱动器（World 线程 spawn）。
        // 每步 World 重建是已知开销——性能敏感路径由调用方直接持有
        // RuntimeDriver（step_frame_equivalent 常驻形态）承载；
        /// 本入口服务"opt-in 翻转"的正确性验证。
        // P1-8（Sol）：(a) 每步重建 World 会把市场拉回 tick 1——非首
        // tick 会话上运行=状态不一致 ⇒ 显式拒绝（不静默重放）；常驻
        // 形态由调用方直接持有 RuntimeDriver。
        self.check_step_start()
            .map_err(|fatal| self.poison_runtime_step(fatal))?;
        // D0 常驻 driver：首次调用惰性构造（initial_committed_tick 对齐
        // 当前 session.tick——全新会话即 0；此前每步重建 World 丢失
        // 簿/冻结/行情态，非首 tick 门禁即由此而来，已移除）。
        if self.runtime_driver.is_none() {
            // Sol 三审修复 2：首步前 queue 的 staged handover 消费（恰一次，
            // cfg(test)——staged 槽位仅测试适配器存在；消费器内含消费拍抑制
            /// ——重排落在消费 tick 的账户顶到 tick+2，对齐旧消费 seam 的
            /// "消费 tick 不再决策"）。attention 已由 produce 写回镜像，
            /// 构造器恢复即组 next_due（不重复推进）。
            #[cfg(test)]
            let queued_intents = crate::session::pipeline::take_staged_intents_for_runtime(self);
            let mut driver =
                crate::runtime::drive::RuntimeDriver::from_session_grouped(
                    self,
                    self.runtime_group_count,
                )
                .map_err(|e| StepFatal::InvariantViolation {
                    location: "GameSession::step_via_runtime".to_owned(),
                    description: e,
                })?;
            driver.adopt_session_seq(self);
            #[cfg(test)]
            if let Some(intents) = queued_intents {
                if !intents.is_empty() {
                    let upper: Vec<i64> = driver
                        .world_public_views()
                        .iter()
                        .map(|v| v.daily_upper_limit)
                        .collect();
                    driver
                        .inject_player_intents(&intents, &upper)
                        .map_err(|e| StepFatal::InvariantViolation {
                            location: "GameSession::step_via_runtime::queue 消费".to_owned(),
                            description: e,
                        })?;
                }
            }
            self.runtime_driver = Some(std::sync::Mutex::new(driver));
        }
        // 取出锁单元（避免 &mut self 与 guard 交叉借用）；失败/成功都
        // 放回——Abort 语义下 worker 存活，driver 可重跑。
        let cell = self
            .runtime_driver
            .take()
            .expect("runtime_driver 刚构造或在位");
        let outcome = {
            let guard = cell.lock();
            let poisoned = guard.is_err();
            let mut driver = if poisoned {
                // 锁被 poison=前次持有期 panic——显式失败，不静默复用
                // （不 into_inner：poison 期状态未知）。
                drop(guard);
                self.runtime_driver = Some(cell);
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
                drop(driver);
                self.runtime_driver = Some(cell);
                return Err(self.poison_runtime_step(StepFatal::InvariantViolation {
                    location: "GameSession::step_via_runtime".to_owned(),
                    description: format!(
                        "锁步失配：world 已转正 tick={world_tick} 而 session.tick={}（两权威漂移——不得静默重放）",
                        self.tick()
                    ),
                }));
            }
            // D4（t3 对等）：ingress 截取时刻——宿主经 shared ingress
            // 绑定入队的玩家意图搬运进 pending_player（旧管线 cutoff
            // 同口径；无绑定为 no-op）。
            if let Err(fatal) = self.freeze_shared_ingress() {
                drop(driver);
                self.runtime_driver = Some(cell);
                return Err(self.poison_runtime_step(fatal));
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
                    drop(driver);
                    self.restore_runtime_player_intents(intents);
                    self.runtime_driver = Some(cell);
                    return Err(self.poison_runtime_step(StepFatal::InvariantViolation {
                        location: "GameSession::step_via_runtime".to_owned(),
                        description: route_err,
                    }));
                }
            }
            let stepped = self.runtime_step_with_install(&mut driver);
            if stepped.is_err() {
                self.restore_runtime_player_intents(intents);
            }
            stepped
        };
        self.runtime_driver = Some(cell);
        outcome
    }

    /// runtime 步进+安装本体（Sol 二审修复集 3）：独立成方法供测试注入
    /// 驱动器（`step_via_runtime` 每次构造驱动器，测试无法在其内部注入
    /// tick 级失败）。三段语义：
    /// - **step 失败：立即停止安装**（不把已回滚世界的状态安装进会话）；
    /// - **安装失败：撤销已推进的 tick/seq/账户+安装面写入的行情/历史/
    ///   注意力基线**（Sol 三审 (b)：market_views 成功、candles 失败时
    ///   前半已写入——只回滚 tick/seq/账户会残留半安装会话）；
    /// - 全部成功：返回事件。
    /// 任一失败都 poison（旧管线 `poison_failed_step` 语义）。
    pub(in crate::session) fn runtime_step_with_install(
        &mut self,
        driver: &mut crate::runtime::drive::RuntimeDriver,
    ) -> Result<Vec<Event>, StepFatal> {
        // 生产探针对齐（t3 契约：每 tick 恰一次"生产调用"）。
        #[cfg(test)]
        self.compute_context()
            .t1_probes
            .queue_attempt_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let baseline = self.runtime_install_baseline();
        // step 失败即停（此前先存 Result 再无条件安装——已 Abort 的
        // 世界状态会被写进会话）。
        let mut events = driver.step_frame_equivalent(self)?;
        // 晚失败注入（D0：旧管线 post_shadow 语义等价点——生产工作完成、
        // 安装之前；晚失败不得泄漏任何已计算状态。两路径同判：此前
        // runtime 路径不消费该注入，注入后期望 Err 的对照测试被
        // 首 tick 门禁掩盖）。
        #[cfg(test)]
        if let Some(fatal) = self.post_shadow_failure.take() {
            self.rollback_runtime_step(baseline);
            return Err(self.poison_runtime_step(fatal));
        }
        // 安装失败注入（测试面——与真实安装失败走同一回滚分支）。
        // Sol 三审 (b)：注入点在 install_market_views **之后**——复现
        // 真实缺口路径"前半安装已写入、后半失败"。
        #[cfg(test)]
        let install_injected = self.runtime_install_failure;
        #[cfg(not(test))]
        let install_injected = false;
        // 安装错误传播（不吞）——失败即撤销已推进状态并 poison，
        // 不返回部分成功。
        let install_outcome = if install_injected {
            match driver.install_market_views(self) {
                Ok(()) => Err(
                    "安装失败注入（Sol 三审 (b) 测试面——行情已写入后失败）".to_owned(),
                ),
                // market_views 自身失败同样走注入描述（注入场景聚焦后半）。
                Err(e) => Err(format!("{e}（注入点：market_views）")),
            }
        } else {
            driver
                .install_market_views(self)
                .and_then(|()| driver.install_candles(self))
        };
        if let Err(install_err) = install_outcome {
            self.rollback_runtime_step(baseline);
            return Err(self.poison_runtime_step(StepFatal::InvariantViolation {
                location: "GameSession::step_via_runtime::install".to_owned(),
                description: install_err,
            }));
        }
        // D5 诊断面派生：retail 拒收诊断（旧管线 continuous lifecycle
        /// projection 的 Rejected 面——路由层组端校验拒收/交易部回执的
        /// runtime 等价；受理/成交/撤销诊断面随事件域迁移逐步补，此处
        /// 仅派生已迁移的 Rejected）。
        self.state.last_retail_order_events.clear();
        for account in self.state.retail_experience.keys().copied().collect::<Vec<_>>() {
            self.record_retail_intent_rejections(account, &events);
        }
        // D5 #4 retail 链：决策 trace 投影（组内生产——本拍清+装，旧管线
        /// 「本 tick 最新」瞬态语义；收集失败按段面显式失败不静默）。
        {
            let traces = match driver.collect_retail_traces() {
                Ok(collected) => collected.into_iter().flatten().collect::<Vec<_>>(),
                Err(e) => {
                    self.rollback_runtime_step(baseline);
                    return Err(self.poison_runtime_step(StepFatal::InvariantViolation {
                        location: "GameSession::step_via_runtime::retail_traces".to_owned(),
                        description: format!("零售决策 trace 收集失败：{e:?}"),
                    }));
                }
            };
            self.state.last_retail_decisions = traces;
        }
        // D5 日界·world 段（ends_day 拍在 step 内编排——desk roll_day/
        /// 组日翻，**先于**信封重建：日终清簿后的 desk 在途=空集，ledger
        /// 重建与旧管线"日终无在途"校验面同源；drained 撤单事件并入本拍
        /// 事件流。此前该编排面缺失（world_day_end_for_test 未接线）——
        /// 多日 step 的 ledger 残留使 session 日终校验显红）。
        let ends_day = {
            let tpd = u64::from(self.state.setup.ticks_per_day);
            tpd > 0 && self.tick() % tpd == 0
        };
        // D5 envelope 投影：desk 在途单重建 session 信封账本（审计面——
        /// nominal 自费用表重算、charged 直取 desk 累计；OrderId=serial 域
        ///（与事件 id 一致）。下拍重建覆盖（权威在 desk）。
        let resting_by_stock = match driver.collect_resting_facts() {
            Ok(facts) => facts,
            Err(e) => {
                self.rollback_runtime_step(baseline);
                return Err(self.poison_runtime_step(StepFatal::InvariantViolation {
                    location: "GameSession::step_via_runtime::envelope".to_owned(),
                    description: e,
                }));
            }
        };
        let mut envelopes = Vec::new();
        for (stock_idx, facts) in resting_by_stock.iter().enumerate() {
            for fact in facts {
                let Some(account) = driver.account_by_local(u16::try_from(fact.owner.1).expect("local 下标在 u16")) else {
                    continue;
                };
                let code = crate::StockId(u32::from(stock_idx as u16));
                let key = crate::session::pipeline::EnvelopeKey {
                    account,
                    stock: code,
                    order: crate::OrderId(fact.order_serial),
                    side: fact.side,
                };
                let nominal = match crate::runtime::fees::FeeQuote::nominal(
                    &self.state.setup.config,
                    fact.side,
                    crate::money::Money::from_cents(fact.filled_value),
                ) {
                    Ok(nominal) => nominal,
                    Err(e) => {
                        self.rollback_runtime_step(baseline);
                        return Err(self.poison_runtime_step(StepFatal::InvariantViolation {
                            location: "GameSession::step_via_runtime::envelope".to_owned(),
                            description: format!("名义费用重算失败：{e:?}"),
                        }));
                    }
                };
                let audit = crate::session::pipeline::EnvelopeAudit {
                    limit: crate::money::Money::from_cents(fact.price),
                    remaining_qty: fact.qty,
                    filled_qty: 0,
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
                // live resources：买=purse（desk 预留）、卖=余量股数
                //（ledger 校验 live 行须有资源——全零非法）。
                let (cash, shares) = if fact.side == crate::orderbook::Side::Buy {
                    (crate::money::Money::from_cents(fact.purse), 0u32)
                } else {
                    (crate::money::Money::ZERO, fact.qty)
                };
                // origin=TickStart（save quiet point 校验的 rebased 形态）。
                envelopes.push(
                    crate::session::pipeline::Envelope::tick_start_existing(
                        key, cash, shares, audit,
                    ),
                );
            }
        }
        self.mark_external_mutation();
        self.state.envelope_ledger = match crate::session::pipeline::EnvelopeLedger::new(
            self.state.next_receipt_base,
            envelopes,
        ) {
            Ok(ledger) => ledger,
            Err(fatal) => {
                let fatal = StepFatal::InvariantViolation {
                    location: "GameSession::step_via_runtime::envelope".to_owned(),
                    description: format!("信封账本重建失败：{fatal:?}"),
                };
                self.rollback_runtime_step(baseline);
                return Err(self.poison_runtime_step(fatal));
            }
        };
        // D5 attention 公开可见性门禁：desk 簿镜像刷新（瞬态读面源——
        /// snapshot 公开投影/玩家在途/预留；desk 权威事实+公开截点）。
        {
            let views = driver.world_public_views();
            let mut mirror = std::collections::BTreeMap::new();
            for (stock_idx, book) in resting_by_stock.iter().enumerate() {
                let Some(view) = views.get(stock_idx) else {
                    continue;
                };
                let mut orders = Vec::with_capacity(book.len());
                for fact in book {
                    let Some(owner) = driver.account_by_local(
                        u16::try_from(fact.owner.1).expect("local 下标在 u16"),
                    ) else {
                        continue;
                    };
                    orders.push((
                        owner,
                        fact.side,
                        crate::Money::from_cents(fact.price),
                        fact.qty,
                        fact.order_serial,
                        fact.filled_value,
                    ));
                }
                mirror.insert(
                    crate::StockId(u32::try_from(stock_idx).expect("股下标在 u32")),
                    super::DeskBookMirror {
                        orders,
                        last_price: crate::Money::from_cents(view.last_price),
                        public_cutoff_serial: view.public_cutoff_serial,
                    },
                );
            }
            self.state.desk_book_mirror = mirror;
        }
        // D5 lifecycle 投影：runtime 下 NPC 报价生命周期权威=desk TTL
        ///（save 的 2170 校验绑定旧簿在场委托——runtime 旧簿恒空，投影
        /// 空集通过；TTL 到期撤销由 desk begin_tick 承载）。
        self.mark_external_mutation();
        self.state.npc_order_lifecycles = Vec::new();
        // D5 日界安装（ends_day 拍）：session 侧交易日终迁移（day 推进/
        /// candle 提交/T+1 解锁/DayBoundary——与旧管线 continuous_tick_
        /// finalizer 同一迁移；在行情安装**之后**（本拍 active candle 先
        /// 写入、日终提交清空——save 的 day_tick=0 期望空 active 对齐；
        /// 价格历史长度由 install 侧按 tick 推导，与本序无耦合）。
        /// world 侧日结编排不在 step 内（生产由 driver.day_end 承载），
        /// 登记差异。
        if ends_day {
            let facts = crate::session::pipeline::install_runtime_day_boundary(self)?;
            for fact in facts {
                self.set_runtime_seq(self.current_seq().checked_add(1).expect("事件 seq 溢出"));
                let mut event = fact.event;
                if let Event::DayBoundary { seq, .. } = &mut event {
                    *seq = self.current_seq();
                }
                events.push(event);
            }
            // D5 日界·world 段在 session 日终迁移**之后**（组内/计划事件
            /// 日历以 session day 为准——world 日结先跑会使组内事件落在
            /// 新日而 session 迁移还携旧日，plan 事件序 precede 显红）。
            /// desk 清簿后 envelope 重建为空集（下节），drained 撤单
            /// 事件并入本拍事件流。
            if let Err(e) = driver.world_day_end_for_step() {
                self.rollback_runtime_step(baseline);
                return Err(self.poison_runtime_step(StepFatal::InvariantViolation {
                    location: "GameSession::step_via_runtime::day_end_world".to_owned(),
                    description: e,
                }));
            }
            let drained = driver.take_drained_events();
            events.extend(drained);
            // D5 调序收尾：desk 清簿后重刷 mirror（此前刷新在 world 段前
            /// ——旧在途残留使公开面/玩家在途读面跨日泄漏）。
            {
                let views: Vec<_> = driver
                    .world_public_views()
                    .iter()
                    .map(|v| (v.last_price, v.public_cutoff_serial))
                    .collect();
                let mut mirror = std::collections::BTreeMap::new();
                let facts = driver.collect_resting_facts().map_err(|e| {
                    StepFatal::InvariantViolation {
                        location: "GameSession::step_via_runtime::day_end_mirror".to_owned(),
                        description: e,
                    }
                })?;
                for (stock_idx, book) in facts.iter().enumerate() {
                    let Some((last_price, cutoff)) = views.get(stock_idx).copied() else {
                        continue;
                    };
                    let orders = book
                        .iter()
                        .filter_map(|fact| {
                            Some((
                                driver.account_by_owner(fact.owner)?,
                                fact.side,
                                crate::Money::from_cents(fact.price),
                                fact.qty,
                                fact.order_serial,
                                fact.filled_value,
                            ))
                        })
                        .collect::<Vec<_>>();
                    mirror.insert(
                        crate::StockId(u32::try_from(stock_idx).expect("股下标在 u32")),
                        super::DeskBookMirror {
                            orders,
                            last_price: crate::Money::from_cents(last_price),
                            public_cutoff_serial: cutoff,
                        },
                    );
                }
                self.state.desk_book_mirror = mirror;
            }
        }
        // P2-2b：成功 step 尾清账户写时 undo（失败路径已由 rollback 消费）。
        self.runtime_clear_account_undo();
        Ok(events)
    }

    /// 安装失败回滚基线（Sol 三审 (b)）：step 推进+安装面可能写入的
    /// 全部会话域快照。candle 域当前为防御性覆盖（install_candles 的
    /// 写入循环不产生中途失败——收集失败时不写任何条目；若后续写入
    /// 面变为可失败，此快照即回滚依据）。
    fn runtime_install_baseline(&self) -> RuntimeInstallBaseline {
        RuntimeInstallBaseline {
            tick: self.tick(),
            seq: self.current_seq(),
            markets: self.state.markets.clone(),
            price_history: self.state.price_history.clone(),
            minute_closes: self.state.market_minute_closes.clone(),
            candle_histories: self.state.candle_book.histories().clone(),
            candle_active: self.state.candle_book.active().clone(),
            npc_attention: self.state.npc_attention.clone(),
            // P1-C 修正：heap 随基线快照（回滚逐项还原——含旧管线陈旧项；
            // 见结构体注释）。
            attention_scheduler: self.state.attention_scheduler.clone(),
            // D5 #4 retail 链：个人态写入面（step_frame 内回写——失败回滚
            // 基线必须覆盖，否则失败拍泄漏个人态演进）。
            belief_participants: self.state.belief_participants.clone(),
            retail_experience: self.state.retail_experience.clone(),
            history_reads: self.state.history_reads.clone(),
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
        Ok(())
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
            if let Some(state) = self.state.npc_attention.get_mut(id) {
                let due = (*next_due).max(tick);
                state.next_attention_candidate_tick = due;
                // D5 #2：注意力共享流续传写回（组内 evaluate 消费后的
                // 权威——restore 双轨逐位等价依赖该面）。
                state.rng_state = *rng_state;
            }
        }
        // 镜像重建：heap=权威候选的精确快照（含未被本批 entries 触及的
        // 在册 NPC——与 restore 的 from_iter 重建同式）。
        self.state.attention_scheduler = self
            .state
            .npc_attention
            .iter()
            .map(|(id, state)| (state.next_attention_candidate_tick, *id))
            .collect();
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
    pub fn runtime_resting_orders_snapshot(
        &self,
    ) -> Vec<(crate::StockId, Vec<crate::orderbook::Order>)> {
        self.state
            .markets
            .iter()
            .map(|(code, market)| (code.clone(), market.resting_orders()))
            .collect()
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
    pub fn runtime_clear_account_undo(&mut self) {
        self.state.runtime_account_undo.clear();
        self.state.runtime_account_undo_ids.clear();
    }

    pub fn runtime_accounts_snapshot(
        &self,
    ) -> Vec<(
        crate::AccountId,
        i64,
        std::collections::BTreeMap<crate::StockId, crate::account::Position>,
    )> {
        (0..self.account_count() as u64)
            .filter_map(|i| {
                let id = crate::AccountId(i);
                self.account(id)
                    .map(|a| (id, a.cash().cents(), a.positions().clone()))
            })
            .collect()
    }

    /// runtime 驱动器翻转面（Sol 二审修复集 3 + 三审 (b)）：安装失败
    /// 撤销已推进的 tick/seq/账户基线**及安装面已写入的行情/历史/
    /// 注意力域**（经直改标记——回滚也是外部直改；不残留半安装会话）。
    pub fn rollback_runtime_step(&mut self, baseline: RuntimeInstallBaseline) {
        let RuntimeInstallBaseline {
            tick,
            seq,
            markets,
            price_history,
            minute_closes,
            candle_histories,
            candle_active,
            npc_attention,
            attention_scheduler,
            belief_participants,
            retail_experience,
            history_reads,
        } = baseline;
        self.mark_external_mutation();
        self.state.tick = tick;
        self.state.seq = seq;
        // P2-2b：账户恢复改走写时 undo（首写前值——step 内全部账户写入
        /// 的回滚等价；无记录=step 内未写=无需恢复）。
        for (id, cash, positions) in std::mem::take(&mut self.state.runtime_account_undo) {
            if let Some(acct) = self.fixture_account_mut(&id) {
                acct.fixture_set_cash(crate::money::Money::from_cents(cash));
                acct.fixture_set_positions(positions);
            }
        }
        self.state.runtime_account_undo_ids.clear();
        self.state.markets = markets;
        self.state.price_history = price_history;
        self.state.market_minute_closes = minute_closes;
        self.state.candle_book.replace_histories(candle_histories);
        self.state.candle_book.replace_active(candle_active);
        self.state.npc_attention = npc_attention;
        // P1-C 修正：heap 快照逐项还原（不重建——基线可含旧管线陈旧项，
        // 重建会丢项使回滚 hash 对比显红；见结构体注释）。
        self.state.attention_scheduler = attention_scheduler;
        self.state.belief_participants = belief_participants;
        self.state.retail_experience = retail_experience;
        self.state.history_reads = history_reads;
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

    /// 股→发行人映射只读（投递载荷组装）。
    pub fn runtime_issuer_map(
        &self,
    ) -> std::collections::BTreeMap<crate::StockId, crate::company::CompanyId> {
        self.state
            .markets
            .keys()
            .filter_map(|code| Some((*code, self.state.issuer_of_stock(*code)?)))
            .collect()
    }

    /// 发行人 spec 快照（投递载荷组装——(kind, issued_shares)）。
    pub fn runtime_company_specs(
        &self,
    ) -> std::collections::BTreeMap<crate::company::CompanyId, (crate::company::CompanyKind, u64)> {
        let mut out: std::collections::BTreeMap<
            crate::company::CompanyId,
            (crate::company::CompanyKind, u64),
        > = Default::default();
        for company in self.runtime_issuer_map().values().copied() {
            if let Some(op) = self.state.operations.company(&company) {
                out.insert(company, (op.spec().kind, op.spec().issued_shares));
            }
        }
        out
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
        self.mark_external_mutation();
        for (id, updated) in updates {
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
        self.state.plans = plans;
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

    /// 每股发行总股本投影（D3a-2c 件4：CandidateTargetProposal 上限输入
    /// ——session setup 权威，u32 收敛口径同 lifecycle）。
    pub fn runtime_total_shares(&self) -> std::collections::BTreeMap<crate::StockId, u64> {
        self.state
            .setup
            .stocks
            .iter()
            .map(|stock| {
                (
                    self.state.stocks.id(stock.code.0.as_str()).expect("setup 股在目录"),
                    stock.total_shares.min(u64::from(u32::MAX)),
                )
            })
            .collect()
    }

    /// 公司域只读句柄（D3a-2c 件2：RootReadContext 组装的构造期 Arc 投影源
    /// ——catalog 克隆、registry/operations Arc 共享，与 capture 同一对象面）。
    pub fn runtime_company_handles(
        &self,
    ) -> (
        crate::stock::StockCatalog,
        std::sync::Arc<crate::company::CompanyRegistry>,
        std::sync::Arc<crate::company::operations::CompanyOperations>,
    ) {
        (
            self.state.stocks.clone(),
            std::sync::Arc::clone(&self.state.company_registry),
            std::sync::Arc::clone(&self.state.operations),
        )
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

    /// D5 restore 链最后一层：日内行情聚合种子读面（每股——save 点的
    /// desk 行情权威投影源）。活动 K 为空（日界空窗/竞价窗）→ None
    /// （desk 从构造零点=昨收锚定起，恰为新日起点语义）。昨收=日 K
    /// 历史末条 close，首日无历史时退初始价。
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

    /// runtime 驱动器翻转面（批 8h）：失败 poison（旧管线语义）。
    pub fn poison_runtime_step(&mut self, fatal: StepFatal) -> StepFatal {
        self.poison_failed_step(fatal)
    }


    pub(super) fn poison_failed_step(&mut self, fatal: StepFatal) -> StepFatal {
        self.poison = Some(fatal.clone());
        fatal
    }

    pub fn save(&self) -> Result<SaveSlot, StepFatal> {
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
