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
    accounts: Vec<(
        crate::AccountId,
        i64,
        std::collections::BTreeMap<crate::StockId, crate::account::Position>,
    )>,
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
    attention_scheduler: super::attention::NpcAttentionScheduler,
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
            let mut driver = crate::runtime::drive::RuntimeDriver::from_session(self)
                .map_err(|e| StepFatal::InvariantViolation {
                    location: "GameSession::step_via_runtime".to_owned(),
                    description: e,
                })?;
            driver.adopt_session_seq(self);
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
        let events = driver.step_frame_equivalent(self)?;
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
            accounts: self.runtime_accounts_snapshot(),
            markets: self.state.markets.clone(),
            price_history: self.state.price_history.clone(),
            minute_closes: self.state.market_minute_closes.clone(),
            candle_histories: self.state.candle_book.histories().clone(),
            candle_active: self.state.candle_book.active().clone(),
            npc_attention: self.state.npc_attention.clone(),
            attention_scheduler: self.state.attention_scheduler.clone(),
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
    pub fn set_runtime_attention(
        &mut self,
        entries: &[(crate::AccountId, u64)],
    ) {
        self.mark_external_mutation();
        let tick = self.state.tick;
        for (id, next_due) in entries {
            if let Some(state) = self.state.npc_attention.get_mut(id) {
                let due = (*next_due).max(tick);
                state.next_attention_candidate_tick = due;
                self.state.attention_scheduler.enqueue(due, *id);
            }
        }
    }

    /// runtime 驱动器翻转面（Sol 二审修复集 5）：attention 状态只读投影
    /// `(AccountId, 下次候选 tick, 注意力 RNG 状态)`——驱动器构造器
    /// 恢复会话真实调度态用（不合成固定 seed/初始 due）。
    pub fn runtime_attention_state(&self) -> Vec<(crate::AccountId, u64, u64)> {
        self.state
            .npc_attention
            .iter()
            .map(|(id, s)| (*id, s.next_attention_candidate_tick, s.rng_state))
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
            accounts,
            markets,
            price_history,
            minute_closes,
            candle_histories,
            candle_active,
            npc_attention,
            attention_scheduler,
        } = baseline;
        self.mark_external_mutation();
        self.state.tick = tick;
        self.state.seq = seq;
        for (id, cash, positions) in accounts {
            if let Some(acct) = self.fixture_account_mut(&id) {
                acct.fixture_set_cash(crate::money::Money::from_cents(cash));
                acct.fixture_set_positions(positions);
            }
        }
        self.state.markets = markets;
        self.state.price_history = price_history;
        self.state.market_minute_closes = minute_closes;
        self.state.candle_book.replace_histories(candle_histories);
        self.state.candle_book.replace_active(candle_active);
        self.state.npc_attention = npc_attention;
        self.state.attention_scheduler = attention_scheduler;
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
