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
    /// 已投递的库版本（Belief-1b·下：稀疏判据）。
    last_library_version: Option<u32>,
}

impl RuntimeDriver {
    /// 个体策略投影（D3 台账收口）：账户已采样策略 → 三族 StrategyData
    /// 全参数（含风格修饰——与旧管线决策输入逐字段同源）。
    pub fn individual_strategy_data(
        strategy: &crate::account::StoredStrategy,
    ) -> Result<StrategyData, String> {
        let state = strategy
            .production_state()
            .map_err(|e| format!("策略状态提取失败：{e:?}"))?;
        Ok(match state {
            crate::strategy::StrategyState::ZiNoise(s) => s.strategy_data(),
            crate::strategy::StrategyState::Momentum(s) => s.strategy_data(),
            crate::strategy::StrategyState::BeliefInstitution(s) => s.strategy_data(),
        })
    }

    /// 基线回退（无策略账户/玩家）：族基线参数（setup 配置面）。
    fn baseline_strategy_data(setup: &SessionSetup, _i: usize) -> StrategyData {
        let params = &setup.strategy_params;
        StrategyData::retail(
            params.retail.arrival_rate,
            params.retail.order_size_mean,
            params.retail.chase_prob,
        )
    }

    /// 从会话构造驱动器（配置面+账户面投影；全 retail 决策缺省装配）。
    /// Sol 二审修复集 5：决策档案恢复会话 attention 真实状态——
    /// `next_due` ← 会话下次候选 tick、`seed` ← 注意力 RNG 状态
    /// （此前合成固定 seed/`i%8` 初始 due，恢复前后行为不同）。
    /// 无 attention 状态的账户（玩家惯例位）保留合成缺省。
    pub fn from_session(session: &GameSession) -> Result<Self, String> {
        let setup: SessionSetup = session.export_setup_for_runtime();
        let n_stocks = setup.stocks.len();
        let (cash, shares, invested, recovered, ever_held, ids) =
            accounts_projection_from_session(session, n_stocks);
        let npcs = ids.len();
        let attention: std::collections::BTreeMap<crate::AccountId, (u64, u64)> = session
            .runtime_attention_state()
            .into_iter()
            .map(|(id, due, rng)| (id, (due, rng)))
            .collect();
        // D3 台账收口（D1 登记差异→闭合）：策略面直接投影账户的**已采样
        // 个体策略**（StrategyState::from_strategy → 三族 strategy_data()
        // ——含风格修饰后的到达率/阈值/步幅/观察概率），族基线参数仅作
        // 无策略账户的回退。此前用 setup 基线参数，个体风格 RNG 采样
        // 差异已登记（.tmp/multi-tick-design.md D1 节），本批收口。
        let profiles: Vec<NpcProfile> = (0..npcs.max(1))
            .map(|i| {
                let id = ids.get(i).copied();
                let attention_entry = id.and_then(|id| attention.get(&id)).copied();
                // 玩家不经组线程自主决策（意图只走 ExternalPlan——D2 路由）；
                // next_due=u64::MAX 显式永不到期。
                let strategy = match id.map(|id| session.account(id).map(|a| a.strategy())) {
                    Some(Some(Some(strategy))) => {
                        Self::individual_strategy_data(strategy).unwrap_or_else(|e| {
                            // 个体策略不可投影=数据面缺失，显式失败（不静默
                            // 回退基线——两路径决策分歧不可解释）。
                            panic!("个体策略投影失败：{e}")
                        })
                    }
                    _ => Self::baseline_strategy_data(&setup, i),
                };
                let fallback_due = if matches!(
                    id.map(|id| session.account(id).map(|a| a.kind())),
                    Some(Some(crate::account::AccountKind::Player)) | None
                ) && id.map(|id| session.account(id).map(|a| a.kind())) != Some(Some(crate::account::AccountKind::Retail))
                    && id.map(|id| session.account(id).map(|a| a.kind())) != Some(Some(crate::account::AccountKind::Inst))
                    && id.map(|id| session.account(id).map(|a| a.kind())) != Some(Some(crate::account::AccountKind::Hot))
                {
                    u64::MAX
                } else {
                    (i % 8) as u64
                };
                match attention_entry {
                    Some((due, rng)) => NpcProfile {
                        strategy,
                        seed: rng,
                        next_due: due,
                    },
                    None => NpcProfile {
                        strategy,
                        seed: 0x5EED_0000_0000_0001u64.wrapping_add(i as u64),
                        next_due: fallback_due,
                    },
                }
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
        // D3a-2b Belief-1 + D3a-2c 件2：完整个人态构造期投影（id_map 序
        // ——None=非机构位；observe 直调的组内权威承载：六件全投影）。
        {
            let personals = session.runtime_personal_states();
            spec.belief_seeds = (0..npcs.max(1))
                .map(|i| {
                    let id = ids.get(i).copied()?;
                    personals.get(&id).map(|p| super::world::BeliefParticipantSnapshot {
                        watchlist: p.watchlist.clone(),
                        price_memory: p.price_memory.clone(),
                        information: p.information.clone(),
                        belief: p.belief.clone(),
                        attention: p.attention.clone(),
                        history_reads: p.history_reads.clone(),
                    })
                })
                .collect();
        }
        // Belief-1b·下 补投影：公开库初始 Arc（组内 observe 的
        // discovery_candidates 输入——此前构造期空库，红证
        // d3a2c_group_open_derivation 显红：组内 belief 条目不创建）。
        spec.library = Some(session.runtime_library_handle().1);
        // D3a-2c 件2：决策链根组装投影（catalog 克隆、registry/operations
        // Arc 共享、名册 AccountId、相位参数——observe 直调的组内输入面）。
        {
            let (catalog, registry, operations) = session.runtime_company_handles();
            let setup_civil = session.export_setup_for_runtime();
            let chain_params_by_id = session.runtime_chain_params();
            let total_shares = session.runtime_total_shares();
            spec.root_projection = Some(std::sync::Arc::new(super::world::RootProjection {
                catalog,
                company_registry: registry,
                operations,
                account_ids: ids.clone(),
                auction_ticks: setup_civil.auction_ticks,
                closing_auction_ticks: setup_civil.closing_auction_ticks,
                chain_params: ids
                    .iter()
                    .map(|id| chain_params_by_id.get(id).copied())
                    .collect(),
                urgency_policy: session.runtime_urgency_policy(),
                total_shares,
                max_order_qty_limit: session.runtime_max_order_qty_limit(),
                tick_size_cents: session.runtime_tick_size_cents(),
                stock_codes: session.runtime_stock_codes(),
            }));
        }
        // D3a-2：会话预置日 K 历史投影进 desk（链观察的日 K 权威——
        // 此前 desk 从零起，观察面与 session 读面天然分歧）。
        {
            let histories = session.runtime_daily_histories();
            for (idx, stock) in spec.stocks.iter_mut().enumerate() {
                if let Some((bars, _traded)) = histories.get(&crate::StockId(idx as u32)) {
                    // 全量种子投影：traded 由种子条自计（initial=0——
                    // 传入 session.traded_count 会双计）。
                    stock.initial_daily_bars = bars.clone();
                    stock.initial_traded_days = 0;
                }
            }
        }
        // D0：世界从会话当前 tick 起步（锁步不变量的构造期对齐——
        // 全新会话即 0；恢复/接管场景相位按绝对 tick 推导）。
        spec.initial_committed_tick = session.tick();
        spec.accounts_per_group = npcs.max(1);
        spec.initial_cash = 0;
        spec.initial_cash_override = Some(vec![cash]);
        spec.initial_shares = vec![shares];
        // D3a-2c 件1：成本基构造投影（开仓派生 cost_price 消费——组侧
        // 权威自此起步，D1 登记"成本基保留会话侧"差异就此闭合）。
        spec.initial_invested = vec![invested];
        spec.initial_recovered = vec![recovered];
        // D3a-3：曾持有位投影（session positions 零量条目面——清仓后
        // 观察面 held 集成员）。
        spec.initial_ever_held = vec![ever_held];
        let world = World::spawn(spec).map_err(|e| format!("驱动器世界构造失败：{e:?}"))?;
        let mut driver = Self {
            world,
            id_map: ids,
            next_seq: 0,
            drained_events: Vec::new(),
            last_library_version: None,
        };
        // Sol 二审修复集 5：旧簿在途挂单迁入世界（构造期一次——组 0 在
        // 下一 tick round 0 经 ExternalPlan 挂出）。此前首构造不迁入：
        // 首步前 seed 的挂单只在 session 旧簿，runtime 世界不可见。
        driver.adopt_session_resting_orders(session)?;
        Ok(driver)
    }

    /// Sol 二审修复集 5：会话旧簿在途挂单迁入世界（ExternalPlan 路由——
    /// 与玩家意图注入同一通道）。仅构造期调用：runtime 步进开始后 session
    /// 旧簿不再新增挂单（写面在组/desk），无重复迁移。owner 不在名册=
    /// 投影缺口，显式失败（不静默丢单）。
    fn adopt_session_resting_orders(&mut self, session: &GameSession) -> Result<(), String> {
        let mut ops = Vec::new();
        for (code, orders) in session.runtime_resting_orders_snapshot() {
            for order in orders {
                let local = self
                    .id_map
                    .iter()
                    .position(|id| *id == order.owner)
                    .ok_or_else(|| {
                        format!(
                            "旧簿挂单 owner 不在世界名册（无法迁移）：{:?} {:?}",
                            order.owner, code
                        )
                    })? as super::LocalIdx;
                ops.push(super::decision::PendingOp {
                    local,
                    stock: code.0 as super::StockIdx,
                    side: order.side,
                    routed: super::protocol::RoutedPrice::Fixed(order.price.cents()),
                    purse_reference: order.price.cents(),
                    qty: order.qty,
                });
            }
        }
        if ops.is_empty() {
            return Ok(());
        }
        self.world.broadcast_external_plan(ops);
        Ok(())
    }

    /// 驱动一个 tick：runtime 收束 → 账户/行情回写会话 → 事件产出。
    /// 会话 tick 由调用方（或批 8e 的 step_frame 本体）推进。
    pub fn step(&mut self, session: &mut GameSession) -> Result<Vec<Event>, String> {
        // D3a-2c 件2：自然日同步（会话 civil clock 权威——快照 civil_date
        // 的唯一来源；休市日跳变经此传导）。
        self.world.set_civil_date(session.civil_date());
        // D4 相位计时：挂当前捕获绑定（组/部 worker 读取安装）；世界运行
        // 结束必摘（槽内克隆持 Arc，不清则捕获收尾 try_unwrap 失败）。
        self.world.set_tick_capture(
            crate::verification_evidence::current_phase_timing_binding(),
        );
        let outcome = self.step_with_capture(session);
        self.world.clear_tick_capture();
        outcome
    }

    /// step 本体（D4：捕获绑定已挂）——驱动器侧相位对齐旧管线 10 相。
    fn step_with_capture(&mut self, session: &mut GameSession) -> Result<Vec<Event>, String> {
        // D4 相位：密封分配快照段（tick 起点，组/部广播前）。
        crate::verification_evidence::enter_phase(
            crate::session::pipeline::TickPhase::SealAllocationSnapshot,
        );
        // D1：新增持仓补经历条目（fault2038 存档校验依赖）——在快照
        // 回写前取现价。
        let last_prices: Vec<i64> = self
            .world_public_views()
            .iter()
            .map(|v| v.last_price.max(1))
            .collect();
        self.world
            .run_ticks(1)
            .map_err(|e| format!("runtime tick 失败：{e:?}"))?;
        // D3 台账收口（入场参考价近似→闭合）：新持仓经历入场参考用本
        // tick 买入腿成交 VWAP（旧管线以真实成交成本为基准；此前近似
        // 现价）。后续加仓的 experience 迁移仍近似（登记——D5 经历域）。
        let buy_vwap: std::collections::BTreeMap<
            (super::GroupIdx, super::LocalIdx, super::StockIdx),
            i64,
        > = {
            let trades: Vec<_> = self
                .world
                .report_log
                .last()
                .map(|r| r.desk_trades.clone())
                .unwrap_or_default();
            let mut sums: std::collections::BTreeMap<_, (i128, u64)> = Default::default();
            for fact in &trades {
                let legs = [
                    (fact.maker, fact.taker_side == crate::orderbook::Side::Sell),
                    (fact.taker, fact.taker_side == crate::orderbook::Side::Buy),
                ];
                for ((g, local), is_buy) in legs {
                    if is_buy {
                        let slot = sums.entry((g, local, fact.stock)).or_default();
                        slot.0 += i128::from(fact.price) * i128::from(fact.qty);
                        slot.1 += u64::from(fact.qty);
                    }
                }
            }
            sums.into_iter()
                .filter_map(|(k, (value, qty))| {
                    (qty > 0).then(|| {
                        (k, i64::try_from(value / i128::from(qty)).unwrap_or(1).max(1))
                    })
                })
                .collect()
        };
        // D4 相位：结算影子段（账户/持仓回写）。
        crate::verification_evidence::enter_phase(
            crate::session::pipeline::TickPhase::SettlementShadow,
        );
        // 账户回写（直改入口——X1 标记外部直改）。组快照不含在途票——
        // 在途持仓口径（旧管线=在手+在途卖余量）由组侧读面承担
        //（RootReadContext 投影含在途；回写 qty=组 shares——qty 口径差
        // 登记项的回写侧：session 域 qty>0 假设依赖非零面，在途卖单
        // 期间 qty=0 条目由 ever_held 保留观察面，权益/目标量读面已含
        // 在途（Sol 整改 3 组侧收口——回写侧不再加回，避免 session 旧
        // 管线代码读到双计）。
        let snapshots = self
            .world
            .snapshot_accounts()
            .map_err(|e| format!("账户快照失败：{e:?}"))?;
        for (_g, accounts) in snapshots {
            for (local, acct) in accounts.iter().enumerate() {
                let Some(id) = self.id_map.get(local).copied() else {
                    continue;
                };
                // D3a-2c 件1：持仓回写全面以组权威同步——数量、T+1 冻结、
                // 成本基（invested/recovered）均取组侧值（组侧由 FillFact
                // 落账维护；清仓清零对齐 session 清仓删除条目语义）。此前
                // "成本基保留会话侧"=runtime 模式下恒 0——D1 登记差异就此
                // 闭合。先只读快照+经历补账，再单次可变写回（借用分离）。
                let mut merged = session
                    .account(id)
                    .map(|a| a.positions().clone())
                    .unwrap_or_default();
                for (idx, &qty_raw) in acct.shares.iter().enumerate() {
                    // Sol 整改 3：qty 含在途卖余量（旧管线口径=在手+在途
                    /// ——qty 口径差收口；session 域 cost_price/权益观察
                    /// 因此与组读面同源）。
                    let qty = qty_raw
                        .saturating_add(
                            acct.pending_sell_qty.get(idx).copied().unwrap_or(0),
                        );
                    let code = crate::StockId(idx as u32);
                    // 清仓条目删除（Sol 四审修复 4，单列的清仓持仓回写差）：
                    // qty 已含在途卖余量（pending_sell_qty 镜像），0=真清仓
                    // 且无在途——对齐旧管线 apply_sell 清仓删条目语义。
                    // 此前"清零保留"（qty 口径差时代在途期间条目清零）——
                    // 余量入 qty 后保留语义只剩清仓残留条目（旧管线会删除，
                    // completed_sell 场景断言红）。组侧"曾持有"观察面由
                    // ever_held 位承载（held 集等价不依赖 session 条目）。
                    if qty == 0 {
                        merged.remove(&code);
                        continue;
                    }
                    let is_new_hold = qty > 0 && !merged.contains_key(&code);
                    if is_new_hold {
                        let entry = buy_vwap
                                .get(&(0, local as super::LocalIdx, idx as super::StockIdx))
                                .copied()
                                .or_else(|| last_prices.get(idx).copied())
                                .unwrap_or(1);
                        session
                            .runtime_hold_experience_init(&id, &code, qty, entry)
                            .map_err(|e| format!("持仓回写配套失败：{e}"))?;
                    }
                    merged.insert(
                        code,
                        crate::account::Position::from_restored_parts(
                            qty,
                            acct.frozen_today.get(idx).copied().unwrap_or(0),
                            acct.invested_cents.get(idx).copied().unwrap_or(0),
                            acct.recovered_cents.get(idx).copied().unwrap_or(0),
                        ),
                    );
                }
                if let Some(engine_acct) = session.fixture_account_mut(&id) {
                    engine_acct.fixture_set_cash(Money::from_cents(acct.cash));
                    engine_acct.fixture_set_positions(merged);
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
        // D4 相位：回执聚合段（事件/回执投影）。
        crate::verification_evidence::enter_phase(
            crate::session::pipeline::TickPhase::ReceiptAggregation,
        );
        let mut events = project_trade_events(&trades, &mut self.next_seq);
        let input = collect_lifecycle(&lifecycle, &receipts);
        events.extend(project_lifecycle_events(&input, &mut self.next_seq));
        events.extend(project_auction_events(&auction, &mut self.next_seq));
        let outcome = Ok(events);
        // D4 相位：派生审计段（回写后对账面——本批为标记位，D3 候选
        // 校验接入后承载实质校验计时）。
        crate::verification_evidence::enter_phase(
            crate::session::pipeline::TickPhase::DerivationAudit,
        );
        // Sol 整改个人态交付投影：组权威个人态（observe 演进）上行→
        // session 镜像回写（reference/peak equity 等读面；Belief-1b 的
        // information 域投递回写保持——整态覆盖与其同源）。
        let belief_states = self
            .world
            .collect_group_belief()
            .map_err(|e| format!("个人态交付投影失败：{e:?}"));
        match belief_states {
            Ok(collected) => {
                for (_group, states) in collected {
                    for (local, snapshot) in states.iter().enumerate() {
                        let Some(&id) = self.id_map.get(local) else {
                            continue;
                        };
                        if let Some(snapshot) = snapshot {
                            session.set_runtime_personal_state(id, snapshot);
                        }
                    }
                }
            }
            Err(projection_error) => return Err(projection_error),
        }
        // D3a-2c 件5：组内计划簿交付点投影（plans_debug 等读面源——组
        // 线程权威，X1 直改）。**空簿不覆盖**：迁移期 session 侧仍承载
        // 测试注入/旧管线场景的计划（组内尚未接管注入路由——D3a-3），
        // 空簿覆盖会清空它们（env=1 plan_owner 回归显红后收口）。
        // 收集失败显式上抛（铁律二：不静默丢——plans_debug 静默过期
        // 不可接受；复核发现 1 收口）。
        let plans_projection = self
            .world
            .collect_plans()
            .map_err(|e| format!("计划簿交付投影失败：{e:?}"));
        match plans_projection {
            Ok(collected) => {
                if let Some((_, plans)) = collected.into_iter().next() {
                    // Sol 整改 4：权威状态控制的完整投影——组簿有任何
                    // 计划记录（含 terminal：最后计划结束的事实必须覆盖
                    /// session 旧镜像）即安装；空簿不覆盖（迁移期注入面
                    /// 保护——组从未开仓时保留 session 侧测试注入）。
                    if plans.plan_ids().next().is_some() {
                        session.set_runtime_plans(plans);
                    }
                }
            }
            Err(projection_error) => return Err(projection_error),
        }
        outcome
    }

    /// 事件 seq 游标（诊断）。
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// runtime 全局日历读取（诊断/对账面）。
    pub fn world_global_day(&self) -> u32 {
        self.world.global_day()
    }

    /// world 已转正 tick（D0 锁步不变量的驱动器侧读数）。
    pub fn world_committed_tick(&self) -> u64 {
        self.world.global.committed_tick
    }

    /// world tick 记录只读（D0 常驻断言：单一世界连续累积）。
    pub fn world_report_log(&self) -> &[super::world::TickRecord] {
        &self.world.report_log
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
        // 基线（Sol 二审修复集 2）：civil 失败时全量回滚——World 日结
        // （Abort 冲回）、driver seq/事件缓冲、会话 day/seq。
        let day_before = session.day();
        let seq_before = session.current_seq();
        let next_seq_before = self.next_seq;
        let drained_len_before = self.drained_events.len();
        // runtime 侧日结：Drain→组日翻转→候选校验（失败=内部已 Abort 冲回）。
        self.world
            .run_day_end_public()
            .map_err(|e| format!("runtime 日结失败（内部已 Abort 冲回）：{e:?}"))?;
        self.collect_drained_events();
        // 会话自然日推进（公司调度/关账/披露/civil clock——旧引擎权威）。
        if let Err(e) = session.end_civil_day() {
            self.rollback_day_end(
                session,
                day_before,
                seq_before,
                next_seq_before,
                drained_len_before,
            );
            return Err(format!("自然日推进失败（日结已全量回滚）：{e:?}"));
        }
        // D3a-2c 件2：日结后同步自然日（快照 civil_date 跟随 civil clock）。
        self.world.set_civil_date(session.civil_date());
        // D4：日结清会话分钟历史（位置合并的跨日对齐——旧管线同口径）。
        session.clear_runtime_minute_history();
        Ok(())
    }

    /// 清簿撤单事实转事件（seq 分配），并入待取缓冲。
    fn collect_drained_events(&mut self) {
        let drained = std::mem::take(&mut self.world.drained_lifecycle);
        if !drained.is_empty() {
            let input = collect_lifecycle(&drained, &[]);
            self.drained_events
                .extend(project_lifecycle_events(&input, &mut self.next_seq));
        }
    }

    /// 日结失败全量回滚（Sol 二审修复集 2）：撤销 World 日结（Abort——
    /// 组/部按可逆日志冲回上一交易日收敛态）、恢复 driver seq 与事件
    /// 缓冲、恢复会话 day/seq 基线。调用后可原样重跑日结（恰一次语义
    /// 由缓冲截断保证——失败尝试不残留事件、不消耗 seq）。
    fn rollback_day_end(
        &mut self,
        session: &mut GameSession,
        day_before: u32,
        seq_before: u64,
        next_seq_before: u64,
        drained_len_before: usize,
    ) {
        session.rollback_runtime_day_end(day_before, seq_before);
        self.world.abort_day_end();
        self.next_seq = next_seq_before;
        self.drained_events.truncate(drained_len_before);
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
        // 基线（Sol 二审修复集 2，扩展 P1-7）：civil 失败时除会话
        // day/seq 外，同步撤销 World 日结与 driver seq/事件缓冲。
        let day_before = session.day();
        let seq_before = session.current_seq();
        let next_seq_before = self.next_seq;
        let drained_len_before = self.drained_events.len();
        self.world
            .run_day_end_public()
            .map_err(|e| format!("runtime 日结失败：{e:?}"))?;
        self.collect_drained_events();
        session.advance_runtime_market_day();
        if let Err(e) = session.end_civil_day() {
            self.rollback_day_end(
                session,
                day_before,
                seq_before,
                next_seq_before,
                drained_len_before,
            );
            return Err(format!("自然日推进失败（日结已全量回滚）：{e:?}"));
        }
        // D4：日结清会话分钟历史（位置合并的跨日对齐——旧管线同口径）。
        session.clear_runtime_minute_history();
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

    /// 外部意图路由（D1 修正）：Intent → PendingOp。local 按**意图
    /// owner 在 id_map 的下标**路由（此前硬编码 local:0——经
    /// pending_player 暂存的 NPC 意图会被误当玩家单，同 owner 撞
    /// 自成交零成交）。PlaceMarket/Cancel 需 OrderRef 跟踪——登记后续。
    fn route_external_intents(
        &self,
        intents: &[crate::session::ReceiptBearingIntent],
        upper_limits: &[i64],
    ) -> Result<Vec<super::decision::PendingOp>, String> {
        let mut ops = Vec::new();
        for r in intents {
            let crate::Intent::PlaceLimit { code, side, price, qty } = &r.intent else {
                // PlaceMarket/Cancel：登记（首版限价面——其余变体补）。
                continue;
            };
            let local = self
                .id_map
                .iter()
                .position(|id| *id == r.owner)
                .ok_or_else(|| {
                    format!(
                        "外部意图 owner 不在世界名册（无法路由到组内账户）：{:?}",
                        r.owner
                    )
                })? as super::LocalIdx;
            let upper = upper_limits
                .get(code.0 as usize)
                .copied()
                .unwrap_or(0);
            let (routed, purse_reference) = match price {
                crate::strategy::LimitPrice::Fixed(p) => {
                    (super::protocol::RoutedPrice::Fixed(p.cents()), p.cents())
                }
                _ => (super::protocol::RoutedPrice::Highest, upper),
            };
            ops.push(super::decision::PendingOp {
                local,
                stock: code.0 as super::StockIdx,
                side: *side,
                routed,
                purse_reference,
                qty: *qty,
            });
        }
        Ok(ops)
    }

    /// 外部意图注入世界（D2/D1）：经 ExternalPlan 通道广播全部组（组 0
    /// 消费）。owner 不在名册=显式失败（不静默丢单）。
    pub fn inject_player_intents(
        &mut self,
        intents: &[crate::session::ReceiptBearingIntent],
        upper_limits: &[i64],
    ) -> Result<(), String> {
        let ops = self.route_external_intents(intents, upper_limits)?;
        if ops.is_empty() {
            return Ok(());
        }
        self.world.broadcast_external_plan(ops);
        Ok(())
    }

    /// 测试面：切断协调者通道（构造失败路径——poison 等价验证）。
    pub fn sever_world_for_test(&mut self) {
        self.world.sever();
    }

    /// 测试控制面（Sol 整改注入通道）：按 AccountId 强制到期（显式
    /// 目标 tick——指令表达账户+目标，组验证执行；Abort 冲回后指令
    /// 失效需重发）。
    #[cfg(test)]
    pub fn force_due_for_test_at(&mut self, ids: &[crate::AccountId], tick: u64) {
        let locals: Vec<super::LocalIdx> = self
            .id_map
            .iter()
            .enumerate()
            .filter_map(|(local, id)| ids.contains(id).then_some(local as super::LocalIdx))
            .collect();
        // 目标不早于世界下一 tick（游标领先时对齐——组验证拒绝过期）。
        let target = tick.max(self.world.global.committed_tick + 1);
        self.world.force_due_for_test(locals, target);
    }

    /// 组注意力调度收集（Sol 二审修复集 5：构造器恢复断言/对账面——
    /// 组线程 next_due 是 runtime 调度权威的真实状态源）。
    pub fn collect_attention(
        &mut self,
    ) -> Result<Vec<(super::GroupIdx, Vec<(super::LocalIdx, u64)>)>, String> {
        self.world
            .collect_group_attention()
            .map_err(|e| format!("注意力收集失败：{e:?}"))
    }

    /// 信息投递（Belief-1b·下：稀疏——披露版本变化才下行；组内纯函数
    /// 推进；回写**仅 information 域**——权威调和见设计注记）。
    /// 返回回写条数。
    pub fn deliver_information(&mut self, session: &mut GameSession) -> Result<usize, String> {
        let (version, library) = session.runtime_library_handle();
        if Some(version) == self.last_library_version {
            return Ok(0);
        }
        let updates = self.collect_delivery(session, library)?;
        self.last_library_version = Some(version);
        session.set_runtime_information_checks(&updates);
        Ok(updates.len())
    }

    /// 测试面：跳过版本判据强制投递（机制验证用）。
    pub fn deliver_information_for_test(
        &mut self,
        session: &mut GameSession,
    ) -> Result<usize, String> {
        let (version, library) = session.runtime_library_handle();
        let updates = self.collect_delivery(session, library)?;
        self.last_library_version = Some(version);
        session.set_runtime_information_checks(&updates);
        Ok(updates.len())
    }

    /// 不可变会话相：组装载荷→组内推进→收更新（借用隔离——回写在
    /// 调用方可变相执行）。
    fn collect_delivery(
        &mut self,
        session: &GameSession,
        library: std::sync::Arc<crate::information::PublicLibrary>,
    ) -> Result<Vec<(crate::AccountId, crate::session::NpcAttentionState)>, String> {
        let now = session.runtime_observation_instant();
        let day = u64::from(session.day());
        let attentions_full = session.runtime_attention_full_states();
        let by_id: std::collections::BTreeMap<crate::AccountId, _> =
            attentions_full.into_iter().collect();
        let mut attentions = Vec::new();
        for (local, id) in self.id_map.iter().enumerate() {
            if let Some(state) = by_id.get(id) {
                attentions.push((local as super::LocalIdx, *id, state.clone()));
            }
        }
        let payload = super::world::GroupInformationPayload {
            now,
            day,
            library,
            attentions,
            issuer_map: session.runtime_issuer_map(),
            company_specs: session.runtime_company_specs(),
        };
        let collected = self
            .world
            .deliver_information(payload)
            .map_err(|e| format!("信息投递编排失败：{e:?}"))?;
        let mut updates = Vec::new();
        for (_g, group_updates) in collected {
            for (local, state) in group_updates {
                if let Some(id) = self.id_map.get(local as usize).copied() {
                    updates.push((id, state));
                }
            }
        }
        Ok(updates)
    }

    /// 信念个人态收集（D3a-2b：同源校验/后续驱动读面）。
    pub fn collect_belief(
        &mut self,
    ) -> Result<Vec<(super::GroupIdx, Vec<Option<super::world::BeliefParticipantSnapshot>>)>, String>
    {
        self.world
            .collect_group_belief()
            .map_err(|e| format!("信念态收集失败：{e:?}"))
    }

    /// 计划簿收集（D3a-2c 件5：组内 PlanBook 上行——测试/红证面；生产
    /// 交付点投影在 step 内）。
    pub fn collect_plans(
        &mut self,
    ) -> Result<Vec<(super::GroupIdx, crate::plans::PlanBook)>, String> {
        self.world.collect_plans().map_err(|e| format!("计划簿收集失败：{e:?}"))
    }

    /// 开仓派生读面收集（D3a-2c：组线程 observe 直调的派生——红证锚）。
    pub fn collect_open_derivations(
        &mut self,
    ) -> Result<
        Vec<(
            super::GroupIdx,
            crate::strategy::MarketView,
            crate::calendar::CivilInstant,
            Vec<super::world::OpenDerivationRecord>,
        )>,
        String,
    > {
        self.world
            .collect_open_derivations()
            .map_err(|e| format!("开仓派生收集失败：{e:?}"))
    }

    /// 观察基收集（D3a-1：组线程 price_paths 上行——计划链端口径/
    /// 测试面）。
    pub fn collect_observations(
        &mut self,
    ) -> Result<
        Vec<(
            super::GroupIdx,
            Vec<(
                crate::observation::PricePathObservation,
                crate::observation::TechnicalObservation,
            )>,
        )>,
        String,
    > {
        self.world
            .collect_group_observations()
            .map_err(|e| format!("观察基收集失败：{e:?}"))
    }

    /// 组账户快照（Sol 二审修复集 4：T+1 冻结面断言/对账——
    /// frozen_today 的权威在组线程，会话侧无镜像）。
    pub fn world_accounts_snapshot(
        &mut self,
    ) -> Result<Vec<(super::GroupIdx, Vec<super::group::GroupAccount>)>, String> {
        self.world
            .snapshot_accounts()
            .map_err(|e| format!("账户快照失败：{e:?}"))
    }

    /// 测试面：注入 tick 级校验器（构造 step 失败路径——Sol 二审
    /// 修复集 3 的 fail-fast 断言用；step_via_runtime 每次构造驱动器，
    /// 测试只能经独立驱动器注入）。
    pub fn set_world_validator_for_test(&mut self, v: super::coordinator::Validator) {
        self.world.set_validator(v);
    }

    /// 测试控制面（Sol 二审修复集 4）：注入日结候选校验器（构造日结
    /// 失败路径——组日结日志 Abort 冲回的等价断言用）。
    #[cfg(test)]
    pub fn set_day_end_validator_for_test(&mut self, v: super::world::DayEndValidator) {
        self.world.set_day_end_validator(v);
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
        // D4 相位计时：tick 界登记（context_workers 采样真实池容量）。
        crate::verification_evidence::begin_authoritative_tick(
            session.tick(),
            session.compute_context(),
        )?;
        match self.step(session) {
            Ok(events) => {
                // D4 相位：提交前校验段（相位完整性——缺相=显式失败）。
                crate::verification_evidence::enter_phase(
                    crate::session::pipeline::TickPhase::PreCommitValidation,
                );
                crate::verification_evidence::validate_precommit()?;
                session.set_runtime_tick(tick_after);
                // D4 相位：提交段（权威 tick/seq 写回）。
                crate::verification_evidence::enter_phase(
                    crate::session::pipeline::TickPhase::CommitTick,
                );
                self.sync_seq_into_session(session);
                crate::verification_evidence::mark_committed(tick_after);
                // D4（t3 读面对等）：root 完成交接见证——runtime 的等价
                // 交付点=本 tick 事件即将返回（收束完成→交付）。计数域/
                // 序数/反捕获栈逻辑与旧写入者（ready_ingress）逐字对齐：
                // 完成序号恒奇数位（fetch+1 先于交付 fetch），同 tick 重
                // 捕获首例抓栈。roots 非空标记=本 tick 事件非空（生产
                // 产物面——映射登记）。
                #[cfg(test)]
                {
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
                    let attempt =
                        probes.queue_attempt_seq.load(std::sync::atomic::Ordering::SeqCst);
                    let roots = usize::from(!events.is_empty());
                    let mut trace = probes
                        .root_handover_trace
                        .lock()
                        .expect("root 交接见证锁被 poison");
                    let capture_ordinal =
                        trace.iter().filter(|r| r.tick == tick_after).count() as u32 + 1;
                    let backtrace = if capture_ordinal >= 2
                        && trace.iter().all(|r| r.backtrace.is_none())
                    {
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
            // 分钟收盘（D4·t4 对等）：**位置合并**保留会话先验前缀、
            // 追加 desk 本日新增段（旧管线=owner 补丁"先验逐字节保留+
            // 追加段=本 tick 收盘价"；此前整值替换丢先验）。跨日由
            // 日结清会话侧历史对齐（desk roll_day 清、键带当日偏移）。
            let mut merged: Vec<crate::observation::MarketMinuteClose> = session
                .runtime_minute_history(&code)
                .to_vec();
            let skip = merged.len();
            for &(minute, c) in view.minute_closes.iter().skip(skip) {
                merged.push(crate::observation::MarketMinuteClose {
                    absolute_trading_minute: minute,
                    close: Money::from_cents(c.max(1)),
                });
            }
            session.set_runtime_minute_closes(&code, merged);
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
        let finals = self
            .world_desk_finals_for_candles()
            .map_err(|e| format!("K 投影收集失败：{e:?}"))?;
        for (stock_idx, candles, active) in finals {
            let code = crate::StockId(u32::from(stock_idx));
            // D3a-2：位置合并——desk 含种子历史（会话预置已入档），仅
            // 追加会话长度之后的新天（防种子重复入档）。
            let skip = session.runtime_candle_history_len(&code);
            for (day, candle) in candles.into_iter().enumerate() {
                if day < skip {
                    continue;
                }
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
    /// Sol 二审修复集 3：收集错误全传播（不吞发送失败/异常消息）。
    fn world_desk_finals_for_candles(
        &mut self,
    ) -> Result<
        Vec<(
            super::StockIdx,
            Vec<(i64, i64, i64, i64, i64, u64, i64, u64)>,
            Option<(i64, i64, i64, i64, i64, u64, i64, u64)>,
        )>,
        super::protocol::RuntimeError,
    > {
        self.world.take_candle_projection()
    }
}
