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
    /// P2-1a 红证计数（cfg(test)）：set_runtime_personal_state 实际安装
    /// 次数——守卫测试断言"安装数==dirty 位计数"（全量重装突变即红）。
    #[cfg(test)]
    personal_installs: u64,
    /// P2-1b 账户回写 dirty 状态：上一 tick 事实 owner 集（组端 staging
    /// 落账滞后一拍——fill owner 的整值同步发生在下一拍，两拍并集覆盖）。
    account_dirty_prev: Vec<bool>,
    /// P2-1b：日结（Drain/T+1/AdminBatch 改账户但无逐账户事实事件）后
    /// 置 true——下一 step 全量回写一次（低频路径，语义安全优先）。
    force_full_accounts: bool,
    /// P2-1b 红证计数（cfg(test)）：账户回写实际写入次数。
    #[cfg(test)]
    account_writes: u64,
    /// P2-2a：信念全量缓存（全局名册序——稀疏上行增量合并；构造期
    /// =belief_seeds 同源投影，collect_belief 读面由它承载全量语义）。
    belief_cache: Vec<Option<super::world::BeliefParticipantSnapshot>>,
    /// 日终清簿事件缓冲（take_drained_events 取走）。
    drained_events: Vec<Event>,
    /// 路由层拒收待投影（D5 组端校验：qty=0 等——(code, owner)，本拍
    /// 事件组装时分配 seq 投影为 IntentRejected）。
    pending_rejections: Vec<(crate::StockId, crate::AccountId)>,
    /// 每组账户数（多组形态的 Owner(g,l)→全局名册映射基；单组=名册长，
    /// owner.0 恒 0 时 g*k+l==l 兼容）。翻转批组参数化（探针负载用）。
    accounts_per_group: usize,
    /// D5 #4：首买经历 init 待办（回写/投影段收集、个人态回写后执行）。
    pending_hold_inits: Vec<(crate::AccountId, crate::StockId, u32, i64)>,
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
    pub fn from_session(session: &mut GameSession) -> Result<Self, String> {
        Self::from_session_grouped(session, 1)
    }

    /// 多组构造（翻转批探针负载：组数参数化——账户按 id_map 序均分，
    /// 尾组余位空槽；生产 from_session 保持单组）。
    pub fn from_session_grouped(
        session: &mut GameSession,
        n_groups: usize,
    ) -> Result<Self, String> {
        let setup: SessionSetup = session.export_setup_for_runtime();
        let n_stocks = setup.stocks.len();
        let (cash_raw, shares_raw, invested_raw, recovered_raw, frozen_raw, ever_held_raw, ids) =
            accounts_projection_from_session(session, n_stocks);
        let npcs = ids.len();
        // 翻转批组参数化：账户矩阵按组切分（[group][local]——id_map 序
        /// 均分、尾组余位零填充；单组 n_groups=1 时与原全量单行等价）。
        let n_groups = n_groups.max(1);
        let per_group = npcs.div_ceil(n_groups);
        fn split_rows<T: Clone>(
            mut row: Vec<T>,
            n_groups: usize,
            per_group: usize,
            pad: T,
        ) -> Vec<Vec<T>> {
            let mut out = Vec::with_capacity(n_groups);
            for _ in 0..n_groups {
                let take = row.drain(..per_group.min(row.len())).collect::<Vec<T>>();
                let mut padded = take;
                padded.resize(per_group, pad.clone());
                out.push(padded);
            }
            out
        }
        // 尾组零位：Vec 行按股数定长（world 的 copy_from_slice 要求）。
        let mut cash = split_rows(cash_raw, n_groups, per_group, 0i64);
        let mut shares = split_rows(
            shares_raw,
            n_groups,
            per_group,
            vec![0u32; n_stocks],
        );
        let invested = split_rows(
            invested_raw,
            n_groups,
            per_group,
            vec![0i64; n_stocks],
        );
        let recovered = split_rows(
            recovered_raw,
            n_groups,
            per_group,
            vec![0i64; n_stocks],
        );
        let frozen = split_rows(
            frozen_raw,
            n_groups,
            per_group,
            vec![0u32; n_stocks],
        );
        let ever_held = split_rows(
            ever_held_raw,
            n_groups,
            per_group,
            vec![false; n_stocks],
        );
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
                // D5 #2：个体基础观察概率投影（npc_attention 的个体采样
                /// 值；无 attention 态的位（玩家惯例位）不消费 evaluate。
                let attention_base = id
                    .map(|id| session.runtime_attention_base_for(&id))
                    .unwrap_or(None)
                    // 玩家惯例位无 attention 态：base 不被 evaluate 消费
                    //（next_due=MAX 恒不 due）——占位 1.0 非静默兜底。
                    .unwrap_or(1.0);
                // D5 #4 retail 链：零售风格投影（retail 位决策内核输入）。
                let retail_style = id.and_then(|id| session.account(id)).and_then(|a| {
                    a.strategy().and_then(|s| s.retail_style())
                });
                match attention_entry {
                    Some((due, rng)) => NpcProfile {
                        strategy,
                        seed: rng,
                        next_due: due,
                        attention_base,
                        retail_style,
                    },
                    None => NpcProfile {
                        strategy,
                        seed: 0x5EED_0000_0000_0001u64.wrapping_add(i as u64),
                        next_due: fallback_due,
                        attention_base,
                        retail_style,
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
        spec.n_groups = n_groups;
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
                        // D5 #4：零售经验行（retail 位 Some）。
                        retail_experience: session
                            .runtime_retail_experience_for(&id),
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
                // D5 #4 retail 链：决策流种子基（npc_seed_base 同源）。
                session_seed: session.runtime_session_seed(),
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
        // D5 restore 链最后一层：restore 形态（旧簿空——desk 权威在
        // ledger）下，desk 种子单的在途面补进组初始账户，与 live 轨
        /// （构造期旧簿种子）演化同构：
        /// · 卖种子余量 → initial_shares（「在手+在途卖」口径——live 轨
        ///   的组 shares 演进天然含在途，restore 投影 positions 只含
        ///   在手，不补则成交回写双计下穿）；
        /// · 买种子 purse → 组现金扣减 + buy_purse_held 登记（公开现金
        ///   =组现金+占用口径；构造期旧簿种子形态不补——grant 投影已
        ///   含在途面，补则双计）。
        {
            let restore_shape = session
                .runtime_resting_orders_snapshot()
                .iter()
                .all(|(_, orders)| orders.is_empty());
            if restore_shape {
                // 组参数化：全局名册 local → (组, 组内 l) 寻址。
                let mut purse_by_group = vec![vec![0i64; per_group]; n_groups];
                for (key, envelope) in session.runtime_envelope_ledger_snapshot() {
                    let local = ids
                        .iter()
                        .position(|id| *id == key.account)
                        .ok_or_else(|| {
                            format!(
                                "账本挂单 owner 不在世界名册（无法投影）：{:?}",
                                key.account
                            )
                        })?;
                    let (g, l) = (local / per_group, local % per_group);
                    let stock = key.stock.0 as usize;
                    if stock >= n_stocks {
                        return Err(format!(
                            "账本挂单股索引越界（无法投影）：{:?}",
                            key.stock
                        ));
                    }
                    let audit = envelope.audit();
                    match key.side {
                        crate::orderbook::Side::Sell => {
                            shares[g][l][stock] =
                                shares[g][l][stock].saturating_add(audit.remaining_qty);
                        }
                        crate::orderbook::Side::Buy => {
                            let purse = envelope.live().cash.cents();
                            purse_by_group[g][l] += purse;
                            cash[g][l] -= purse;
                        }
                    }
                }
                spec.initial_purse_held = purse_by_group;
            }
        }
        // D0：世界从会话当前 tick 起步（锁步不变量的构造期对齐——
        // 全新会话即 0；恢复/接管场景相位按绝对 tick 推导）。
        spec.initial_committed_tick = session.tick();
        spec.accounts_per_group = per_group;
        spec.initial_cash = 0;
        spec.initial_cash_override = Some(cash);
        spec.initial_shares = shares;
        // D3a-2c 件1：成本基构造投影（开仓派生 cost_price 消费——组侧
        // 权威自此起步，D1 登记"成本基保留会话侧"差异就此闭合）。
        spec.initial_invested = invested;
        spec.initial_recovered = recovered;
        // D3a-3：曾持有位投影（session positions 零量条目面——清仓后
        // 观察面 held 集成员）。
        spec.initial_ever_held = ever_held;
        // D5 restore 链最后一层：T+1 当日冻结投影。
        spec.initial_frozen_today = frozen;
        // D5 adopt 直入（Sol 二审修复集 5 的到簿序收口）：旧簿在途单
        // 构造期直入 desk 初始簿——先于本 tick 新单（此前经 ExternalPlan
        // 消息链晚于外部新单到达，maker/taker 身分颠倒——live_plan_
        // partial_fill 红证）。owner 不在名册=投影缺口，显式失败。
        spec.initial_desk_orders = resting_order_seeds_from_session(session, &ids)?;
        // D5 restore 等价：tick 价格历史投影（save fault2010 长度面）+
        /// 日内行情聚合种子（最后一层——活动 K/日内高低调/分钟收盘/昨收，
        /// 续拍在 save 聚合之上演进）。
        {
            let market_seeds = session.runtime_market_seeds();
            for (idx, (stock_spec, history)) in spec
                .stocks
                .iter_mut()
                .zip(session.runtime_price_histories())
                .enumerate()
            {
                stock_spec.initial_recent_prices = history;
                stock_spec.initial_market = market_seeds.get(&crate::StockId(idx as u32)).cloned();
                // D5 attention：收盘竞价公开截点投影（restore 链——market
                // 域承载；desk 权威自此起步）。
                stock_spec.initial_public_cutoff = session
                    .runtime_public_cutoff_for(&crate::StockId(idx as u32));
            }
        }
        // D5 adopt 收口：迁入后清旧簿（双域一致——save fault2226）。
        session.runtime_clear_resting_for_adoption();
        // P2-2a：稀疏上行读面的全量缓存基线（构造期投影==组端初值）。
        let belief_cache = spec.belief_seeds.clone();
        let world = World::spawn(spec).map_err(|e| format!("驱动器世界构造失败：{e:?}"))?;
        let driver = Self {
            world,
            id_map: ids.clone(),
            next_seq: 0,
            #[cfg(test)]
            personal_installs: 0,
            account_dirty_prev: Vec::new(),
            // 首拍全量（初始镜像同步——构造后首 step 建立 session 侧
            /// 组权威镜像基线；此后按事实 dirty 增量）。
            force_full_accounts: true,
            #[cfg(test)]
            account_writes: 0,
            belief_cache,
            drained_events: Vec::new(),
            pending_rejections: Vec::new(),
            accounts_per_group: per_group.max(1),
            pending_hold_inits: Vec::new(),
            last_library_version: None,
        };
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
        self.world.broadcast_external_plan(ops, Vec::new());
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
                // 归一化身份：maker≡卖方、taker≡买方（Sol 七审修复 2：
                // 此前按原始 taker_side 判买腿——主动卖时把卖方当买方，
                /// 真买方缺 VWAP 退用上一价，经历入场价错）。
                let legs = [(fact.taker, true), (fact.maker, false)];
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
        // P2-1b：账户回写 dirty 过滤——只重写"本 tick 事实 owner ∪ 上一
        /// tick 事实 owner"（组端 staging 落账滞后一拍，两拍并集覆盖；
        /// 多装等值无害，漏装才会镜像陈旧）∪ 日结强制全量（Drain/T+1/
        /// AdminBatch 无逐账户事实事件）。等价不变式：非 dirty ⇒ 组端
        /// 账户==session 镜像（首拍全量建立基线后，两侧只经事实通道
        /// 同步）。省每 tick 全名册 positions 深拷贝回写。
        let fact_rec = self.world.report_log.last().cloned();
        let roster = self.id_map.len();
        let mut dirty_now: Vec<bool> = vec![false; roster];
        let mark = |dirty: &mut Vec<bool>, owner: crate::runtime::Owner| {
            let idx = owner.0 as usize * self.accounts_per_group + owner.1 as usize;
            if let Some(flag) = dirty.get_mut(idx) {
                *flag = true;
            }
        };
        if let Some(rec) = &fact_rec {
            for fact in &rec.desk_trades {
                mark(&mut dirty_now, fact.maker);
                mark(&mut dirty_now, fact.taker);
            }
            for fact in &rec.desk_lifecycle {
                mark(&mut dirty_now, fact.owner);
            }
            for fact in &rec.desk_receipts {
                mark(&mut dirty_now, fact.owner);
            }
        }
        let skip_all = !self.force_full_accounts;
        let dirty_union: Vec<bool> = if skip_all {
            dirty_now
                .iter()
                .zip(self.account_dirty_prev.iter().chain(std::iter::repeat(&false)))
                .map(|(a, b)| *a || *b)
                .collect()
        } else {
            vec![true; roster]
        };
        for (g, accounts) in snapshots {
            for (local, acct) in accounts.iter().enumerate() {
                let global = g as usize * self.accounts_per_group + local;
                if skip_all && !dirty_union.get(global).copied().unwrap_or(false) {
                    continue;
                }
                let Some(id) = self.id_map.get(global).copied() else {
                    continue;
                };
                // P2-2b：首写前值入 undo（安装失败回滚源——替代 baseline
                /// 全名册账户快照）。
                session.runtime_record_account_undo(id);
                #[cfg(test)]
                {
                    self.account_writes += 1;
                }
                // D3a-2c 件1：持仓回写全面以组权威同步——数量、T+1 冻结、
                // 成本基（invested/recovered）均取组侧值（组侧由 FillFact
                // 落账维护；清仓清零对齐 session 清仓删除条目语义）。此前
                // "成本基保留会话侧"=runtime 模式下恒 0——D1 登记差异就此
                // 闭合。先只读快照+经历补账，再单次可变写回（借用分离）。
                let mut merged = session
                    .account(id)
                    .map(|a| a.positions().clone())
                    .unwrap_or_default();
                let mut pending_hold_inits: Vec<(crate::AccountId, crate::StockId, u32, i64)> =
                    Vec::new();
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
                        // D5 #4：首买经历 init 延后到个人态回写之后（组权威
                        /// 经验行整态覆盖会冲掉先行写入——同拍投影段同式）。
                        pending_hold_inits.push((id, code, qty, entry));
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
                    // Sol 五审：公开现金=组现金+在途买占用（挂出不减、成交
                    /// 实付减、终结不变——旧管线 reservation 观同口径；组
                    /// 内部 purse 所有权口径不因此改变）。
                    engine_acct.fixture_set_cash(Money::from_cents(
                        acct.cash + acct.buy_purse_held_cents,
                    ));
                    engine_acct.fixture_set_positions(merged);
                }
            }
        }
        // P2-1b：本 tick 事实集传下一拍（并集自身不传递——否则一旦
        // dirty 永远 dirty）；日结强制标记在此消费（本次已全量）。
        self.account_dirty_prev = dirty_now;
        self.force_full_accounts = false;
        // 事件：Trade（双腿）+ 生命周期（受理/撤单/到期）+ 回执拒收。
        let rec = fact_rec;
        // Sol 五审修复 1：**本拍公开期末投影**——step() 返回前把本 tick
        // 已成交事实合入公开 session 账户（ADR-0017 完整结算同拍交付；
        /// 组内 staging 下一 Start 合并机制保留=双写：投影即时、内部
        /// 权威随后对齐——下一拍回写为整值同步，天然不重复入账）。
        if let Some(rec) = &rec {
            self.apply_same_tick_fill_projection(
                session,
                &rec.desk_trades,
                &buy_vwap,
                &last_prices,
            )?;
        }
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
        // D5 组端校验拒收（路由层 qty=0 等）——本拍事件流投影。
        for (code, owner) in std::mem::take(&mut self.pending_rejections) {
            events.push(Event::IntentRejected {
                seq: {
                    let seq = self.next_seq;
                    self.next_seq = seq
                        .checked_add(1)
                        .expect("事件 seq 溢出");
                    seq
                },
                account: owner,
                code,
                reason: crate::RejectionReason::InvalidQuantity,
            });
        }
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
                for (group, states) in collected {
                    // P2-2a：稀疏条目=本 tick 生产演进位（组端已滤非
                    /// dirty——上行即 dirty）。逐条重装 session 镜像并
                    /// 增量维护全量缓存（读面语义）。等价不变式：
                    /// 缓存/镜像与组端只在 dirty 通道同步、构造期同源。
                    for (local, snapshot) in states {
                        let global =
                            group as usize * self.accounts_per_group + local as usize;
                        let Some(&id) = self.id_map.get(global) else {
                            continue;
                        };
                        #[cfg(test)]
                        {
                            self.personal_installs += 1;
                        }
                        if let Some(slot) = self.belief_cache.get_mut(global) {
                            *slot = Some(snapshot.clone());
                        }
                        session.set_runtime_personal_state(id, &snapshot);
                    }
                }
            }
            Err(projection_error) => return Err(projection_error),
        }
        // D5 #4：首买经历 init（回写/投影段收集——个人态整态回写之后
        /// 执行，组权威经验行不冲掉首买条目）。
        {
            let mut inits = std::mem::take(&mut self.pending_hold_inits);
            for (id, code, qty, entry) in inits.drain(..) {
                session
                    .runtime_hold_experience_init(&id, &code, qty, entry)
                    .map_err(|e| format!("首买经历初始化失败：{e}"))?;
            }
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
                let group_has_plans = collected
                    .iter()
                    .any(|(_, plans)| plans.plan_ids().next().is_some());
                if group_has_plans {
                    let (_, mut plans) = collected
                        .into_iter()
                        .find(|(_, plans)| plans.plan_ids().next().is_some())
                        .expect("上方已判非空");
                    // Sol 整改 4：权威状态控制的完整投影——组簿有任何
                    // 计划记录（含 terminal：最后计划结束的事实必须覆盖
                    /// session 旧镜像）即安装。
                    // Sol 五审修复 1：本拍成交事实合入执行腿（组内
                    /// staging 下一 Start 合并——公开计划读面同拍；
                    /// 下一拍被组权威整值覆盖，不双计）。
                    if !trades.is_empty() {
                        let day = u64::from(session.day());
                        self.apply_same_tick_plan_fills(&mut plans, &trades, day)?;
                    }
                    session.set_runtime_plans(plans);
                } else if !trades.is_empty() {
                    // D5 cross_stock 收口：组簿无计划（迁移期注入面——
                    /// session 侧测试注入的计划）时，本拍成交事实对
                    /// session plans 同拍合入（与组簿非空路径同式；
                    /// 下一拍组权威接管后此分支自然失效）。
                    let day = u64::from(session.day());
                    self.apply_same_tick_plan_fills_for_session(session, &trades, day)?;
                }
            }
            Err(projection_error) => return Err(projection_error),
        }
        // D3b 件 1：成交触达账户的个人记忆裁剪（同拍——组内 prune 在下一
        // Start 落账；个人态同步在上方用组镜像整态覆盖，裁剪必须在其后；
        // 下一拍组镜像已裁剪同值，双写不漂移）。
        if !trades.is_empty() {
            let mut touched: Vec<crate::AccountId> = trades
                .iter()
                .flat_map(|tr| [tr.maker, tr.taker])
                .filter_map(|owner| self.id_map.get(self.global_index(owner)).copied())
                .collect();
            touched.sort();
            touched.dedup();
            for account in touched {
                session.runtime_prune_personal_memory(account);
            }
        }
        outcome
    }

    /// Sol 五审修复 1：本拍成交的执行腿同拍投影——按 (账户,股,方向)
    /// 匹配活跃计划的活动子单推进成交（record_child_fill 语义：余量/
    /// 全成收口）。组内权威下一 Start 落账同值，投影不构成双计；无
    /// 匹配子单的事实（外部单/非计划腿）自然跳过。
    /// D5 cross_stock：session 侧计划簿的同拍成交合入（组簿空、迁移期
    /// 注入面保护的计划——与组簿路径同式）。
    fn apply_same_tick_plan_fills_for_session(
        &mut self,
        session: &mut GameSession,
        trades: &[super::protocol::DeskTradeFact],
        day: u64,
    ) -> Result<(), String> {
        let mut plans = session.take_runtime_plans();
        // 身份核用低 48 位（迁移期注入面的计划子单持旧管线 OrderId 域
        /// ——无 stock 高位编码；组内链子单为全等核，见下方 fn）。
        let result = self.apply_same_tick_plan_fills_inner(&mut plans, trades, day, false);
        session.set_runtime_plans(plans);
        result
    }

    fn apply_same_tick_plan_fills(
        &self,
        plans: &mut crate::plans::PlanBook,
        trades: &[super::protocol::DeskTradeFact],
        day: u64,
    ) -> Result<(), String> {
        self.apply_same_tick_plan_fills_inner(plans, trades, day, true)
    }

    /// `strict_identity`：true=OrderId 全等核（组内链——stock 高位编码
    /// 域）；false=低 48 位核（迁移期 session 注入面的旧管线 OrderId 域）。
    fn apply_same_tick_plan_fills_inner(
        &self,
        plans: &mut crate::plans::PlanBook,
        trades: &[super::protocol::DeskTradeFact],
        day: u64,
        strict_identity: bool,
    ) -> Result<(), String> {
        for tr in trades {
            // maker≡卖方/taker≡买方（归一化身份——费用与订单序列同序）。
            for (owner, is_buyer, order_serial) in [
                (tr.maker, false, tr.maker_order_serial),
                (tr.taker, true, tr.taker_order_serial),
            ] {
                let Some(&id) = self.id_map.get(self.global_index(owner)) else {
                    continue;
                };
                let code = crate::StockId(tr.stock as u32);
                let side = if is_buyer {
                    crate::orderbook::Side::Buy
                } else {
                    crate::orderbook::Side::Sell
                };
                // Sol 六审修复 2：按**真实订单身份**推进——成交的 desk
                // 序号必须等于计划活动子单身份（合成同式：OrderRef=
                //（股, 序号）→OrderId 高位编码股）。外部/retail 单的同键
                // 成交身份不匹配即跳过（此前按账户+股+方向套挂，把组内
                // 已修好的身份关联漏洞带回投影层）。
                let child_order = plans
                    .active_plan(id, &code)
                    .filter(|p| p.direction() == side)
                    .and_then(|p| p.active_child_order_id());
                let Some(child_order) = child_order else {
                    continue;
                };
                let identity_ok = if strict_identity {
                    child_order.0 == ((u64::from(tr.stock) << 48) | (order_serial & 0x0000_FFFF_FFFF_FFFF))
                } else {
                    child_order.0 & 0x0000_FFFF_FFFF_FFFF == order_serial & 0x0000_FFFF_FFFF_FFFF
                };
                if !identity_ok {
                    continue;
                }
                // 身份已核——record_child_fill 的 Ok(false)/Err 均为投影
                // 数据不一致（身份匹配后计划不应拒收），显式失败。
                let pushed = plans
                    .record_child_fill(id, &code, side, child_order, tr.qty, day)
                    .map_err(|e| format!("同拍执行腿投影推进失败 {id:?} {code:?}: {e:?}"))?;
                if !pushed {
                    return Err(format!(
                        "同拍执行腿投影未被计划消费（身份已核）：{id:?} {code:?} order={child_order:?}"
                    ));
                }
            }
        }
        Ok(())
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
        // P2-1b：日结改账户（Drain 回流/T+1 解冻/AdminBatch 权益）但无
        // 逐账户事实事件——置强制全量回写标记（下一 step 全名册同步
        // 一次；低频路径语义安全优先）。
        self.force_full_accounts = true;
        Ok(())
    }

    /// D5 envelope 投影：名册只读（组内下标→AccountId——envelope 键组装）。
    pub fn account_by_local(&self, local: u16) -> Option<crate::AccountId> {
        self.id_map.get(local as usize).copied()
    }

    /// Owner(g,l) → AccountId（多组映射；上行消费统一入口）。
    pub fn account_by_owner(&self, owner: super::Owner) -> Option<crate::AccountId> {
        self.id_map.get(self.global_index(owner)).copied()
    }

    /// D5 envelope 投影：desk 在途单收集（session 信封账本重建输入）。
    pub fn collect_resting_facts(
        &mut self,
    ) -> Result<Vec<Vec<super::protocol::RestingFact>>, String> {
        self.world
            .collect_desk_resting()
            .map_err(|e| format!("在途单快照收集失败：{e:?}"))
    }

    /// D5 日界·world 段（ends_day 拍在 step 内编排——desk roll_day/组
    /// 日翻；civil 侧不在内，生产显式 day_end 编排的 world 部分等价）。
    /// 清簿撤单事实收集进 drained 缓冲（调用方 take_drained_events 取走）。
    pub fn world_day_end_for_step(&mut self) -> Result<(), String> {
        self.world
            .run_day_end_public()
            .map_err(|e| format!("runtime 世界日结失败（内部已 Abort 冲回）：{e:?}"))?;
        self.collect_drained_events();
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
        &mut self,
        intents: &[crate::session::ReceiptBearingIntent],
        upper_limits: &[i64],
    ) -> Result<
        (
            Vec<super::decision::PendingOp>,
            Vec<(super::LocalIdx, super::StockIdx, u64)>,
        ),
        String,
    > {
        let mut ops = Vec::new();
        let mut cancels = Vec::new();
        for r in intents {
            // D5 终程：Cancel 意图路由（OrderId 低 48 位=desk serial——
            /// 组端直转 DeskIn::Cancel；撤单窗/存在性由 desk 权威判定）。
            if let crate::Intent::Cancel { code, id } = &r.intent {
                let local = self
                    .id_map
                    .iter()
                    .position(|owner| *owner == r.owner)
                    .ok_or_else(|| {
                        format!(
                            "外部撤单 owner 不在世界名册（无法路由）：{:?}",
                            r.owner
                        )
                    })? as super::LocalIdx;
                cancels.push((local, code.0 as super::StockIdx, id.0 & 0x0000_FFFF_FFFF_FFFF));
                continue;
            }
            let crate::Intent::PlaceLimit { code, side, price, qty } = &r.intent else {
                // PlaceMarket：登记（限价面先行——市价变体后续补）。
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
            // D5 组端校验：qty=0 意图路由层拒收（desk 侧为防御性 panic
            /// ——组端先验，拒收事件入本拍事件流——旧管线 InvalidQuantity
            /// 回执的 runtime 等价）。
            if *qty == 0 {
                self.pending_rejections.push((*code, r.owner));
                continue;
            }
            ops.push(super::decision::PendingOp {
                local,
                stock: code.0 as super::StockIdx,
                side: *side,
                routed,
                purse_reference,
                qty: *qty,
            });
        }
        Ok((ops, cancels))
    }

    /// 外部意图注入世界（D2/D1）：经 ExternalPlan 通道广播全部组（组 0
    /// 消费）。owner 不在名册=显式失败（不静默丢单）。
    pub fn inject_player_intents(
        &mut self,
        intents: &[crate::session::ReceiptBearingIntent],
        upper_limits: &[i64],
    ) -> Result<(), String> {
        let (ops, cancels) = self.route_external_intents(intents, upper_limits)?;
        if ops.is_empty() && cancels.is_empty() {
            return Ok(());
        }
        self.world.broadcast_external_plan(ops, cancels);
        Ok(())
    }

    /// Sol 五审修复 1：本拍成交事实的公开期末投影。数据源=本 tick
    /// desk_trades（双腿 owner/价/量/实收费用分项）；写入面=session 账户
    /// （在组回写之后执行——回写值为本拍落账前权威，此处的本拍增量叠
    /// 加后即期末公开态）。下一拍组回写为整值同步（已含落账），投影
    /// 不构成重复入账。
    fn apply_same_tick_fill_projection(
        &mut self,
        session: &mut GameSession,
        trades: &[super::protocol::DeskTradeFact],
        buy_vwap: &std::collections::BTreeMap<(super::GroupIdx, super::LocalIdx, super::StockIdx), i64>,
        last_prices: &[i64],
    ) -> Result<(), String> {
        if trades.is_empty() {
            return Ok(());
        }
        // (账户, 股) → 本拍增量：qty（买卖净额）、frozen（买入）、
        // invested/recovered（成交额）、cash（买负实付/卖正净得）。
        use std::collections::BTreeMap;
        #[derive(Default)]
        struct Delta {
            qty: i64,
            frozen: i64,
            invested: i64,
            recovered: i64,
            cash: i64,
        }
        let mut by_account: BTreeMap<crate::AccountId, BTreeMap<crate::StockId, Delta>> =
            BTreeMap::new();
        for tr in trades {
            // maker≡卖方/taker≡买方（desk 产出侧两方向统一——台账备忘）。
            for (owner, is_buyer, fees) in [
                (tr.maker, false, tr.maker_fees_cents),
                (tr.taker, true, tr.taker_fees_cents),
            ] {
                let Some(&id) = self.id_map.get(self.global_index(owner)) else {
                    continue;
                };
                let code = crate::StockId(tr.stock as u32);
                let gross = i64::from(tr.price) * i64::from(tr.qty);
                let d = by_account
                    .entry(id)
                    .or_default()
                    .entry(code)
                    .or_default();
                if is_buyer {
                    d.qty += i64::from(tr.qty);
                    d.frozen += i64::from(tr.qty);
                    d.invested += gross;
                    // 买腿现金=实付（gross+费用）：公开现金基数=组现金+
                    // 在途占用（挂出全额加回——基数不含 purse），本拍实付
                    // 经此投影即时扣；下一拍占用/退款对齐后回写同值
                    //（整值同步，不双计）。
                    d.cash -= gross + fees;
                } else {
                    d.qty -= i64::from(tr.qty);
                    d.recovered += gross;
                    // 卖腿净得（组现金下一 Start 才落账——同拍补齐）。
                    d.cash += gross - fees;
                }
            }
        }
        for (id, by_code) in by_account {
            // P2-2b：首写前值入 undo（同上）。
            session.runtime_record_account_undo(id);
            let Some(engine_acct) = session.fixture_account_mut(&id) else {
                continue;
            };
            let local = self
                .id_map
                .iter()
                .position(|map_id| *map_id == id)
                .expect("投影账户必在名册") as super::LocalIdx;
            let mut positions = engine_acct.positions().clone();
            // Sol 六审修复 3：首买经历初始化（与回写路径同款——此前
            /// 直接建条目，下一拍条目已在使 is_new_hold 永假，retail
            /// 首次持有永久缺经历条目→fault2038 保存校验）。收集后在
            /// 账户借用外执行。
            let mut new_hold_inits: Vec<(crate::StockId, i64, u32)> = Vec::new();
            for (code, d) in &by_code {
                let is_new_hold = d.qty > 0 && !positions.contains_key(code);
                if is_new_hold {
                    let entry = buy_vwap
                        .get(&(0, local, code.0 as super::StockIdx))
                        .copied()
                        .or_else(|| last_prices.get(code.0 as usize).copied())
                        .or_else(|| trades.iter().map(|t| t.price).min())
                        .unwrap_or(1);
                    new_hold_inits.push((code.clone(), entry, d.qty as u32));
                }
                let entry = positions
                    .entry(code.clone())
                    .or_insert_with(|| crate::account::Position::from_restored_parts(0, 0, 0, 0));
                let new_qty = i64::from(entry.qty()) + d.qty;
                let new_frozen = i64::from(entry.t1_locked()) + d.frozen;
                let new_invested = entry.invested_cents() + d.invested;
                let new_recovered = entry.recovered_cents() + d.recovered;
                if new_qty <= 0 && new_frozen <= 0 {
                    // 清仓（同拍）：删除条目——旧管线 apply_sell 语义
                    ///（qty 已含在途卖余量口径的回写面；本拍卖增量后归零
                    /// 即真清仓）。
                    if new_qty < 0 || new_frozen < 0 {
                        return Err(format!(
                            "同拍投影持仓下穿：{id:?} {code:?} qty={new_qty} frozen={new_frozen}"
                        ));
                    }
                    positions.remove(code);
                    continue;
                }
                *entry = crate::account::Position::from_restored_parts(
                    new_qty as u32,
                    new_frozen as u32,
                    new_invested,
                    new_recovered,
                );
            }
            engine_acct.fixture_set_positions(positions);
            let cash_delta: i64 = by_code.values().map(|d| d.cash).sum();
            let cash = engine_acct.cash().cents() + cash_delta;
            engine_acct.fixture_set_cash(crate::Money::from_cents(cash));
            drop(engine_acct);
            for (code, entry, qty) in new_hold_inits {
                self.pending_hold_inits.push((id, code, qty, entry));
            }
        }
        Ok(())
    }

    /// 测试面：切断协调者通道（构造失败路径——poison 等价验证）。
    pub fn sever_world_for_test(&mut self) {
        self.world.sever();
    }

    /// D5 #4 fixture 通道：推远名册账户 next_due（cfg(test)——组权威
    /// 调度面的"无 due tick"形态构造）。
    #[cfg(test)]
    pub fn defer_due_for_test_at(&mut self, ids: &[crate::AccountId], tick: u64) {
        let locals: Vec<super::LocalIdx> = self
            .id_map
            .iter()
            .enumerate()
            .filter_map(|(local, id)| ids.contains(id).then_some(local as super::LocalIdx))
            .collect();
        self.world.defer_due_for_test(locals, tick);
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
    /// D5 #4 retail 链：本拍零售决策 trace 收集（world 通道面包装）。
    pub fn collect_retail_traces(
        &mut self,
    ) -> Result<Vec<Vec<crate::session::RetailDecisionTrace>>, super::protocol::RuntimeError> {
        self.world.collect_retail_traces().map(|collected| {
            collected.into_iter().map(|(_, traces)| traces).collect()
        })
    }

    /// Owner(g, l) → 全局名册下标（多组映射基；单组 g=0 时退化为 l）。
    fn global_index(&self, owner: super::Owner) -> usize {
        owner.0 as usize * self.accounts_per_group + owner.1 as usize
    }

    pub fn collect_attention(
        &mut self,
    ) -> Result<Vec<(super::GroupIdx, Vec<(super::LocalIdx, u64, u64)>)>, String> {
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
        for (g, group_updates) in collected {
            for (local, state) in group_updates {
                if let Some(id) = self
                    .id_map
                    .get(g as usize * self.accounts_per_group + local as usize)
                    .copied()
                {
                    updates.push((id, state));
                }
            }
        }
        Ok(updates)
    }

    /// P2-1a 红证读面（cfg(test)）：个人态实际安装计数。
    #[cfg(test)]
    pub fn personal_installs_for_test(&self) -> u64 {
        self.personal_installs
    }

    /// P2-1b 红证读面（cfg(test)）：账户回写实际写入计数。
    #[cfg(test)]
    pub fn account_writes_for_test(&self) -> u64 {
        self.account_writes
    }

    /// 信念个人态收集（D3a-2b：同源校验/后续驱动读面——**全量语义**
    /// 读面；生产安装链的 dirty 过滤在 step 内部承载，不经此面。
    /// P2-1a：随附 dirty 掩码（读面一般忽略；同源校验可用）。
    pub fn collect_belief(
        &self,
    ) -> Result<Vec<(super::GroupIdx, Vec<Option<super::world::BeliefParticipantSnapshot>>)>, String>
    {
        // P2-2a：读面不经组线程（上行已稀疏化）——由增量缓存承载
        //（构造期投影基线+每 tick dirty 增量合并，与组端权威同源）。
        let per = self.accounts_per_group.max(1);
        Ok((0..self.id_map.len().div_ceil(per))
            .map(|g| {
                (
                    g as super::GroupIdx,
                    self.belief_cache[g * per..].iter().take(per).cloned().collect(),
                )
            })
            .collect())
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

    /// D3b 件 5：个人态注入（cfg(test)）——restructure fixture 的组内
    /// 等价（session 版 personal.install 的下行面）。
    #[cfg(test)]
    pub fn install_personal_for_test(
        &mut self,
        local: super::LocalIdx,
        snapshot: super::world::BeliefParticipantSnapshot,
    ) {
        self.world.install_personal_for_test(local, snapshot);
    }

    /// D3b 复审注入通道（cfg(test)）：计划事件下行组 0（restructure
    /// fixture 的组内等价——session 版 commit_injected_plan_roots 的
    /// 计划事件面）。
    #[cfg(test)]
    pub fn apply_plan_events_for_test(
        &mut self,
        events: Vec<(crate::plans::PlanId, crate::plans::PlanEvent)>,
    ) {
        self.world.apply_plan_events_for_test(events);
    }

    /// D3b 件 4 fixture 通道（cfg(test)）：复审候选注入下行组 0（组内
    /// 复审块+开仓循环用注入值替代 observe 产物——一次消费即清）。
    #[cfg(test)]
    pub fn inject_review_assessments_for_test(
        &mut self,
        account: crate::AccountId,
        assessments: std::collections::BTreeMap<crate::StockId, crate::plans::CandidateAssessment>,
    ) {
        self.world.inject_review_assessments_for_test(account, assessments);
    }

    /// D3b 件 4 fixture 通道（cfg(test)）：组簿测试变换下行组 0（手工
    /// 簿推进的组内等价——闭包直施组权威簿，undo 可逆）。
    #[cfg(test)]
    pub fn mutate_plan_book_for_test(
        &mut self,
        mutate: Box<dyn FnOnce(&mut crate::plans::PlanBook) + Send>,
    ) {
        self.world.mutate_plan_book_for_test(mutate);
    }

    /// D3b 件 4 fixture 通道（cfg(test)）：观察基分钟序列注入下行组 0
    ///（set_urgency_minutes 的组内等价）。
    #[cfg(test)]
    pub fn inject_observation_minutes_for_test(
        &mut self,
        stock: u16,
        minutes: Vec<(u64, i64)>,
    ) {
        self.world.inject_observation_minutes_for_test(stock, minutes);
    }

    /// D3b 件 4 fixture 通道（cfg(test)）：belief 簿测试变换下行组 0
    ///（observe_equity 等手工 belief 面直改的组内等价）。
    #[cfg(test)]
    pub fn mutate_belief_for_test(
        &mut self,
        account: crate::AccountId,
        mutate: Box<dyn FnOnce(&mut crate::strategy::BeliefBook) + Send>,
    ) {
        self.world.mutate_belief_for_test(account, mutate);
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
        // D5 存档对齐：完成连续 tick 数按 tick 推导（tick/tpd 替代
        /// state.day——日界安装序在 install 之后时 day 尚未推进，而 save
        /// 校验口径（persistence.rs）以含日界的绝对 tick 计——两序下
        /// tick 推导恒一致）。
        let day = tick / tpd.max(1);
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
        for (g, attention) in collected {
            for (local, next_due, rng_state) in attention {
                if let Some(id) = self
                    .id_map
                    .get(g as usize * self.accounts_per_group + local as usize)
                    .copied()
                {
                    entries.push((id, next_due, rng_state));
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
            // D5 日界归档：desk 日 K 的 time 为占位 0（desk 域无 civil
            /// 日历）——追加段用 session 日历的当日 time 重写（save 校验
            /// fault2065 非严格递增显红）。多日一次归档未支持（显式失败
            /// ——生产一次 step 至多跨一日）。
            let new_days = candles.len().saturating_sub(skip);
            if new_days > 1 {
                return Err(format!(
                    "日界归档一次 {new_days} 日未支持（日历重写面待补）"
                ));
            }
            let archive_time = if new_days == 1 {
                Some(session.candle_date_time_for_runtime())
            } else {
                None
            };
            for (day, candle) in candles.into_iter().enumerate() {
                if day < skip {
                    continue;
                }
                session.push_historical_candle_for_runtime(
                    &code,
                    crate::session::DailyCandle {
                        time: archive_time.unwrap_or(candle.0),
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
            // 活动 K 空窗与存档校验 fault2076 同式：day_tick==0（日界拍）
            // 或竞价申报窗——world 日结已把当日归档 histories，残留
            // active 会二次归档（fault2064）。
            let active_must_be_empty = day_tick == 0 || day_tick < auction_entry;
            let candle_time = session.candle_date_time_for_runtime();
            if active_must_be_empty {
                session.clear_active_candle_for_runtime(&code);
            }
            if let Some(a) = active.filter(|_| !active_must_be_empty) {
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

/// D5 adopt 收集：会话旧簿在途单 → desk 初始簿种子（按股 idx——owner
/// 名册映射组 0 位；直入形态不受理回执/不 purse——组内成交落账走外来
/// 单路径，Sol 六/七审的外来面承载）。
fn resting_order_seeds_from_session(
    session: &GameSession,
    id_map: &[crate::AccountId],
) -> Result<Vec<Vec<super::desk::DeskRestingSeed>>, String> {
    // D5 restore 等价：runtime 形态（旧簿空）下单权威=restore 重建的信封
    // 账本（save 的 live_envelopes）——desk 种子从账本键恢复（purse 由
    // buy_order_reservation 与 resource 占位同式；价格面以 charged 下界
    // 无法精确——用 audit.limit==ZERO 的重建标记形态时退 1 分占位，
    // 真实价格面由卖/买腿的 desk 撮合域重建（后续 step 覆盖））。
    let snapshot = session.runtime_resting_orders_snapshot();
    let from_ledger = snapshot.iter().all(|(_, orders)| orders.is_empty());
    let mut seeds: Vec<Vec<super::desk::DeskRestingSeed>> = Vec::new();
    if from_ledger {
        for (key, envelope) in session.runtime_envelope_ledger_snapshot() {
            let local = id_map
                .iter()
                .position(|id| *id == key.account)
                .ok_or_else(|| {
                    format!("账本挂单 owner 不在世界名册（无法迁移）：{:?}", key.account)
                })? as super::LocalIdx;
            let stock = key.stock.0 as usize;
            if seeds.len() <= stock {
                seeds.resize(stock + 1, Vec::new());
            }
            // D5 restore 链最后一层：audit/ purse 数值面自 save 契约直取
            /// （desk 权威——此前 remaining 退 1 股占位使续拍种子量错）。
            let price = if envelope.audit().limit.cents() > 0 {
                envelope.audit().limit.cents()
            } else {
                1
            };
            let qty = envelope.audit().remaining_qty;
            let purse = if key.side == crate::orderbook::Side::Buy {
                envelope.live().cash.cents()
            } else {
                0
            };
            seeds[stock].push(super::desk::DeskRestingSeed {
                owner: (0, local),
                side: key.side,
                price,
                qty,
                purse,
                // restore 形态：受理事件已随此前世界交付（save 前事件
                // 历史），直入不重放 Accepted。
                accepted_delivered: true,
                // D5 复核修复（Major 1，复审修正）：OrderId 编码低 48 位
                // =desk 本地序号（高位为 stock 域——group.rs order_id_of
                // 的编码式；直取整值会使 stock≥1 时 serial 跳天、截点
                // 过滤反向失真）。
                serial: key.order.0 & 0x0000_FFFF_FFFF_FFFF,
            });
        }
        return Ok(seeds);
    }
    let n_stocks = snapshot
        .iter()
        .map(|(code, _)| code.0 as usize + 1)
        .max()
        .unwrap_or(0);
    seeds = vec![Vec::new(); n_stocks];
    // D5 复核修复（Major 1，复审修正）：adopt 形态的 desk 序号=构造期
    // 递增自然序（与 desk next_serial 起始衔接；0 字面量会使多单共享
    // (stock, 0) 身份——组端 Cancel/释放按 order_serial 关联错乱）。
    let mut adopt_serial: u64 = 1;
    for (code, orders) in snapshot {
        for order in orders {
            let local = id_map
                .iter()
                .position(|id| *id == order.owner)
                .ok_or_else(|| {
                    format!(
                        "旧簿挂单 owner 不在世界名册（无法迁移）：{:?} {:?}",
                        order.owner, code
                    )
                })? as super::LocalIdx;
            // 买腿 purse=全额预留（挂单价×量——desk 侧扣减权威；
            /// envelope live 资源面）。
            let purse = if order.side == crate::orderbook::Side::Buy {
                order.price.cents() * i64::from(order.qty)
            } else {
                0
            };
            seeds[code.0 as usize].push(super::desk::DeskRestingSeed {
                owner: (0, local),
                side: order.side,
                price: order.price.cents(),
                qty: order.qty,
                purse,
                // live 构造期 adopt：受理事实照记（对齐旧管线 enqueue 后
                // 首拍交付面）。
                accepted_delivered: false,
                // adopt 形态：构造期自然序（desk 域从此起步）。
                serial: {
                    let serial = adopt_serial;
                    adopt_serial += 1;
                    serial
                },
            });
        }
    }
    Ok(seeds)
}
