//! runtime 驱动器纵切（批 8d）：`step_frame` 切换本体的可运行预演。
//!
//! 形态：`RuntimeDriver` 持常驻 World（组/交易部线程跨 tick 存活，账户在
//! 组线程内演进——不经每步重建）；`step` = 协调者一个 tick（广播 Start→
//! 收束→安装）+ 账户/行情回写 `GameSession` + 事件/TickFrame 组装返回。
//!
//! **已接面**：账户现金/持仓回写（`fixture_account_mut` 直改入口，X1 标记）、
//! tick 前进、Trade 事件（双腿事实→`Event::Trade`+`EventStableKey::for_event`
//! 身份域）、公共态安装。**登记批 8e**：撤单/到期/竞价事件、AuctionPoint、
//! envelope/receipt/公司域/civil 面（协调者全局域全量迁移）、默认翻转与
//! 旧管线退役、集成测试批量迁移。切换前 step_frame 主路径保持旧管线。

use super::bridge::{accounts_projection_from_session, world_spec_from_setup};
use super::decision::{DecisionSetup, NpcProfile};
use super::event_bridge::{
    collect_lifecycle, project_auction_events, project_lifecycle_events, project_trade_events,
};
use super::world::World;
use crate::money::Money;
use crate::strategy::StrategyData;
use crate::{AccountId, Event, GameSession, SessionSetup};

/// 常驻 runtime 驱动器。
pub struct RuntimeDriver {
    world: World,
    /// Owner(组内下标) → 引擎 AccountId（玩家=0 惯例位）。
    pub id_map: Vec<AccountId>,
    /// 事件 seq（协调者全局单调）。
    next_seq: u64,
    /// 日终清簿事件缓冲（take_drained_events 取走）。
    drained_events: Vec<Event>,
}

impl RuntimeDriver {
    /// 从会话构造驱动器（配置面+账户面投影；全 retail 决策缺省装配）。
    pub fn from_session(session: &GameSession) -> Result<Self, String> {
        let setup: SessionSetup = session.export_setup_for_runtime();
        let n_stocks = setup.stocks.len();
        let (cash, shares, ids) = accounts_projection_from_session(session, n_stocks);
        let npcs = ids.len();
        let profiles: Vec<NpcProfile> = (0..npcs.max(1))
            .map(|i| NpcProfile {
                strategy: StrategyData::retail(0.30, 200, 0.35),
                seed: 0x5EED_0000_0000_0001u64.wrapping_add(i as u64),
                next_due: (i % 8) as u64,
            })
            .collect();
        let decision = DecisionSetup {
            profiles,
            attention_gap_ticks: 4,
        };
        let mut spec = world_spec_from_setup(
            &setup,
            Some(decision),
            Box::new(|_g| Box::new(|_t, _r| super::group::TickPlan::default())),
        )?;
        spec.n_groups = 1;
        spec.accounts_per_group = npcs.max(1);
        spec.initial_cash = 0;
        spec.initial_cash_override = Some(vec![cash]);
        spec.initial_shares = vec![shares];
        let world = World::spawn(spec).map_err(|e| format!("驱动器世界构造失败：{e:?}"))?;
        Ok(Self {
            world,
            id_map: ids,
            next_seq: 0,
            drained_events: Vec::new(),
        })
    }

    /// 驱动一个 tick：runtime 收束 → 账户/行情回写会话 → 事件产出。
    /// 会话 tick 由调用方（或批 8e 的 step_frame 本体）推进。
    pub fn step(&mut self, session: &mut GameSession) -> Result<Vec<Event>, String> {
        self.world
            .run_ticks(1)
            .map_err(|e| format!("runtime tick 失败：{e:?}"))?;
        // 账户回写（直改入口——X1 标记外部直改）。
        let snapshots = self
            .world
            .snapshot_accounts()
            .map_err(|e| format!("账户快照失败：{e:?}"))?;
        for (_g, accounts) in snapshots {
            for (local, acct) in accounts.iter().enumerate() {
                let Some(id) = self.id_map.get(local).copied() else {
                    continue;
                };
                if let Some(engine_acct) = session.fixture_account_mut(&id) {
                    engine_acct.fixture_set_cash(Money::from_cents(acct.cash));
                }
            }
        }
        // 事件：Trade（双腿）+ 生命周期（受理/撤单/到期）+ 回执拒收。
        let rec = self.world.report_log.last().cloned();
        let trades = rec
            .as_ref()
            .map(|r| r.desk_trades.clone())
            .unwrap_or_default();
        let lifecycle = rec
            .as_ref()
            .map(|r| r.desk_lifecycle.clone())
            .unwrap_or_default();
        let receipts = rec
            .as_ref()
            .map(|r| r.desk_receipts.clone())
            .unwrap_or_default();
        let auction = rec
            .as_ref()
            .map(|r| r.desk_auction.clone())
            .unwrap_or_default();
        let mut events = project_trade_events(&trades, &mut self.next_seq);
        let input = collect_lifecycle(&lifecycle, &receipts);
        events.extend(project_lifecycle_events(&input, &mut self.next_seq));
        events.extend(project_auction_events(&auction, &mut self.next_seq));
        Ok(events)
    }

    /// 事件 seq 游标（诊断）。
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// runtime 全局日历读取（诊断/对账面）。
    pub fn world_global_day(&self) -> u32 {
        self.world.global_day()
    }
}

impl RuntimeDriver {
    /// 日结+自然日推进（批 8g：公司域/civil 全局域等价路径）。
    ///
    /// 语义对齐（业务照搬 `end_civil_day` 形状，执行形态改 runtime）：
    /// - runtime DayEnd（Drain 清簿→组日翻转/权益→候选校验）对应市场侧
    ///   日终点；`next_seq` 语义由会话权威保留（事件 seq 仍全局单调）；
    /// - `session.end_civil_day()` 保持为**唯一权威调用**（公司调度/
    ///   日历/披露的 candidate-安装语义不变）——本方法把两者编排为
    ///   一个原子序列：runtime 日结成功 ⇒ 会话自然日推进；任一失败
    ///   ⇒ 两侧各自回滚（runtime Abort 可逆日志冲回、会话 candidate
    ///   丢弃），日历不推进。
    pub fn day_end(&mut self, session: &mut GameSession) -> Result<(), String> {
        // runtime 侧日结：Drain→组日翻转→候选校验（失败=内部已 Abort 冲回）。
        self.world
            .run_day_end_public()
            .map_err(|e| format!("runtime 日结失败（内部已 Abort 冲回）：{e:?}"))?;
        // 清簿撤单事实转事件（seq 分配），并入待取缓冲。
        let drained = std::mem::take(&mut self.world.drained_lifecycle);
        if !drained.is_empty() {
            let input = collect_lifecycle(&drained, &[]);
            self.drained_events
                .extend(project_lifecycle_events(&input, &mut self.next_seq));
        }
        // 会话自然日推进（公司调度/关账/披露/civil clock——旧引擎权威）。
        session.end_civil_day().map_err(|e| {
            format!("自然日推进失败（runtime 已日结、会话 candidate 已丢弃）：{e:?}")
        })?;
        Ok(())
    }

    /// 取日终清簿事件（清空式——调用方拼进 tick 帧或日结帧）。
    pub fn take_drained_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.drained_events)
    }

    /// 日结编排（runtime 市场侧 day 同步版）：runtime DayEnd → 会话
    /// 交易日计数推进（直改标记——旧管线 commit_from 的 day 域等价）
    /// → 自然日推进（end_civil_day 时钟校验通过）→ 清簿事件。
    pub fn day_end_with_runtime_market_day(
        &mut self,
        session: &mut GameSession,
    ) -> Result<(), String> {
        self.world
            .run_day_end_public()
            .map_err(|e| format!("runtime 日结失败：{e:?}"))?;
        let drained = std::mem::take(&mut self.world.drained_lifecycle);
        if !drained.is_empty() {
            let input = collect_lifecycle(&drained, &[]);
            self.drained_events
                .extend(project_lifecycle_events(&input, &mut self.next_seq));
        }
        // P1-7（Sol）：日结失败原子性——day/seq 基线回滚。
        let day_before = session.day();
        let seq_before = session.current_seq();
        session.advance_runtime_market_day();
        if let Err(e) = session.end_civil_day() {
            session.rollback_runtime_day_end(day_before, seq_before);
            return Err(format!("自然日推进失败（day/seq 已回滚）：{e:?}"));
        }
        Ok(())
    }
}

impl RuntimeDriver {
    /// 组装 TickFrame 形状的事件帧（批 8h：TickFrame 组装串联）。
    ///
    /// 把已映射的引擎事件（Trade/受理/撤单/拒收/竞价）按 seq 排序输出，
    /// 附带时序快照（MarketSnap/ContinuousPoint/DailyCandle）——这是
    /// `step_frame_and_commit` 管线调用替换件的产物面。稳定键
    /// （EventStableKey 的 local_event_index 语义）尚未复刻（登记），
    /// 因此返回 `Event` 列表而非 `EventFact`——翻转的最后一关。
    pub fn step_with_frame(
        &mut self,
        session: &mut GameSession,
    ) -> Result<super::event_bridge::FrameProjection, String> {
        let events = self.step(session)?;
        let views: Vec<super::protocol::StockPublic> = self.world_public_views().to_vec();
        Ok(super::event_bridge::FrameProjection { events, views })
    }

    /// 测试面：切断协调者通道（构造失败路径——poison 等价验证）。
    pub fn sever_world_for_test(&mut self) {
        self.world.sever();
    }

    /// 公共视图只读（组装面）。
    pub fn world_public_views(&self) -> &[super::protocol::StockPublic] {
        self.world.public_views()
    }
}

impl RuntimeDriver {
    /// 翻转缺口②（批 8h·部分3）：tick 计数与 poison 安装面。
    ///
    /// `step_frame_equivalent`：runtime 驱动 + 会话市场侧安装（tick 前进/
    /// poison 语义）——对照旧管线 `step_frame_and_commit` 的安装段逐段
    /// 迁移的**第一段**（tick 计数 + 失败 poison）：
    /// - 成功：`session.state.tick` 前进（经直改标记入口）、事件返回；
    /// - runtime 失败：映射为 `StepFatal` 并 poison 会话（旧管线
    ///   `poison_failed_step` 语义：失败后 `require_healthy` 恒拒）。
    /// 未迁移段（登记）：candidate 投影/observer 回调/事件 seq 写入
    /// 会话权威（`state.seq`）/日界安装——这些与 envelope/公司域联动，
    /// 属翻转最后一步的整合面。
    pub fn step_frame_equivalent(
        &mut self,
        session: &mut GameSession,
    ) -> Result<Vec<Event>, crate::session::StepFatal> {
        session.require_healthy()?;
        let tick_after = session.tick() + 1;
        match self.step(session) {
            Ok(events) => {
                session.set_runtime_tick(tick_after);
                self.sync_seq_into_session(session);
                Ok(events)
            }
            Err(runtime_err) => {
                let fatal = crate::session::StepFatal::InvariantViolation {
                    location: "RuntimeDriver::step_frame_equivalent".to_owned(),
                    description: runtime_err,
                };
                Err(session.poison_runtime_step(fatal))
            }
        }
    }
}

impl RuntimeDriver {
    /// 翻转缺口②余段（批 8h·部分4）：事件 seq 写入会话权威。
    ///
    /// 旧管线语义（`continuous_tick_finalizer.rs:244-246`）：事件经
    /// `collect_events(facts, session.state.seq)` 从当前权威 seq 起单调
    /// 分配，安装后 `state.seq = collected.next_seq`。runtime 等价：
    /// driver 的全局 seq 游标在**构造时从 `session.state.seq` 初始化**、
    /// 每步安装后写回——两端 seq 空间合一（单调续接，无重号无跳号）。
    pub fn sync_seq_into_session(&mut self, session: &mut GameSession) {
        session.set_runtime_seq(self.next_seq);
    }

    /// 从会话权威 seq 初始化驱动器游标（构造后、首步前调用一次）。
    pub fn adopt_session_seq(&mut self, session: &GameSession) {
        self.next_seq = session.current_seq();
    }
}

impl RuntimeDriver {
    /// 翻转缺口②第三段（批 8h·部分5）：行情安装（candidate 投影等价）。
    ///
    /// 旧管线 `commit_from` 以整 candidate 交换安装全部权威域；runtime
    /// 等价的第一块是**行情域**：`restore_prices`（旧管线 `Market` 的
    /// 恢复事实写入口，校验后安装 last/last_close）——driver 每步把
    /// 收束点公共态写入会话 `markets`。逐步补齐其余域（登记）。
    pub fn install_market_views(&mut self, session: &mut GameSession) -> Result<(), String> {
        let views: Vec<super::protocol::StockPublic> = self.world_public_views().to_vec();
        // P1-5（Sol）：配置从会话真实 setup 读——不硬编码。
        let setup = session.export_setup_for_runtime();
        let tpd = setup.ticks_per_day.max(1);
        let auction = setup.auction_ticks;
        let closing = setup.closing_auction_ticks;
        let history_len = setup.history_len;
        let tick = session.tick();
        let day = session.day() as u64;
        let continuous_per_day = tpd.saturating_sub(auction).saturating_sub(closing);
        let completed_cont =
            day * continuous_per_day + (tick % tpd).saturating_sub(auction).min(continuous_per_day);
        let expected_len = (completed_cont as usize).min(history_len);
        for (idx, view) in views.iter().enumerate() {
            let code = crate::StockId(idx as u32);
            if let Some(market) = session.market_mut_for_runtime(&code) {
                market.restore_prices(
                    Money::from_cents(view.last_price),
                    Money::from_cents(view.last_price.max(1)),
                );
            } else {
                return Err(format!("行情安装：股 {idx} 不在会话目录"));
            }
            // 价格历史（尾部 expected_len 条，正价）。
            let hist: Vec<Money> = view
                .recent_prices
                .iter()
                .rev()
                .take(expected_len)
                .rev()
                .map(|&c| Money::from_cents(c.max(1)))
                .collect();
            session.set_runtime_price_history(&code, hist);
            // 分钟收盘（绝对分钟→Money；覆盖全部已完成分钟口径）。
            // 分钟键：desk 已带绝对偏移（P1-6 修复后）——直接透传。
            let minutes: Vec<crate::observation::MarketMinuteClose> = view
                .minute_closes
                .iter()
                .map(|&(minute, c)| crate::observation::MarketMinuteClose {
                    absolute_trading_minute: minute,
                    close: Money::from_cents(c.max(1)),
                })
                .collect();
            session.set_runtime_minute_closes(&code, minutes);
        }
        // P1-10（Sol）：注意力候选真实写回——组线程 next_due 是调度
        // 权威（替换"推到 tick"的假迁移）。
        let collected = self
            .world
            .collect_group_attention()
            .map_err(|e| format!("注意力收集失败：{e:?}"))?;
        let mut entries = Vec::new();
        for (_g, attention) in collected {
            for (local, next_due) in attention {
                if let Some(id) = self.id_map.get(local as usize).copied() {
                    entries.push((id, next_due));
                }
            }
        }
        session.set_runtime_attention(&entries);
        Ok(())
    }

    /// 翻转缺口②第三段：日界安装（runtime DayEnd → 会话日界）。
    /// `day_end_with_market` = `day_end`（既有编排）+ 行情安装收尾——
    /// 旧管线日界的行情面（昨收翻转）由 `roll_day` 已在 runtime 侧完成、
    /// 会话侧经 `end_civil_day` 权威推进，此处补行情同步。
    pub fn day_end_with_market(&mut self, session: &mut GameSession) -> Result<(), String> {
        self.day_end(session)?;
        self.install_market_views(session)
    }
}

impl RuntimeDriver {
    /// 翻转附带域补建（批 8 终点批）：candle/K 历史回写（history_reads
    /// 的读侧数据源）。
    ///
    /// 旧管线：`candle_book`（会话权威）由 finalizer 每 tick 安装活动 K
    /// 与历史；history_reads 在查询时记录（beliefs 经 BeliefBook 参与者
    /// 集合由策略侧维护）。runtime 等价：desk 端 `market.candles`（日界
    /// 入档）与活动 K 由收束点公共态承载——本方法把它们回写会话
    /// `candle_book`（X1 标记），读侧查询/记录路径不变（旧管线单一代码
    /// 路径 ⇒ 等价传递）。belief_participants：策略数据化（StrategyData）
    /// 已承载参与者身份，BeliefBook 的会话侧镜像随账户面批（登记）。
    pub fn install_candles(&mut self, session: &mut GameSession) -> Result<(), String> {
        let finals = self.world_desk_finals_for_candles();
        for (stock_idx, candles, active) in finals {
            let code = crate::StockId(u32::from(stock_idx));
            for candle in candles {
                session.push_historical_candle_for_runtime(
                    &code,
                    crate::session::DailyCandle {
                        time: candle.0,
                        open: Money::from_cents(candle.1),
                        high: Money::from_cents(candle.2),
                        low: Money::from_cents(candle.3),
                        close: Money::from_cents(candle.4),
                        volume: candle.5,
                        trade_stats: None,
                    },
                );
            }
            let _ = ();
            // 活跃 K 窗口对齐（存档校验 fault2076）：竞价申报窗
            // （day_tick < auction_entry=10）活跃 K 须为空。
            let s = session.export_setup_for_runtime();
            let day_tick = session.tick() % s.ticks_per_day.max(1);
            let auction_entry = s.auction_ticks - s.auction_ticks / 3;
            let in_auction_entry = day_tick < auction_entry;
            let candle_time = session.candle_date_time_for_runtime();
            if let Some(a) = active.filter(|_| !in_auction_entry) {
                session.set_active_candle_for_runtime(
                    &code,
                    crate::session::DailyCandle {
                        time: candle_time,
                        open: Money::from_cents(a.1),
                        high: Money::from_cents(a.2),
                        low: Money::from_cents(a.3),
                        close: Money::from_cents(a.4),
                        volume: a.5,
                        trade_stats: Some(crate::session::DailyTradeStats {
                            turnover_cents: a.6.max(0) as u64,
                            trade_count: a.7,
                        }),
                    },
                );
            }
        }
        Ok(())
    }

    /// desk K 数据抽取（测试面通道；生产每步安装走 install_candles）。
    fn world_desk_finals_for_candles(
        &mut self,
    ) -> Vec<(
        super::StockIdx,
        Vec<(i64, i64, i64, i64, i64, u64, i64, u64)>,
        Option<(i64, i64, i64, i64, i64, u64, i64, u64)>,
    )> {
        self.world.take_candle_projection()
    }
}
