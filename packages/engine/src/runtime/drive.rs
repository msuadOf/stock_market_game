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

/// Q2 批 3：账户分片覆盖校验——本拍 desk 事实（成交双腿/生命周期/回执）
/// 的每个 owner 必须在对应组分片中（组端 undo 集漏标即红——不静默装
/// 陈旧镜像；原 validate_sparse_echo_reply"缺项=RuntimeError"语义的
/// 等价物，请求集改述为本拍事实集）。独立自由函数供负控单测直击。
pub(crate) fn ensure_fact_owners_covered(
    fact_rec: Option<&super::world::TickRecord>,
    snapshots: &[Vec<(super::LocalIdx, super::group::GroupAccount)>],
    accounts_per_group: usize,
) -> Result<(), String> {
    let Some(rec) = fact_rec else {
        return Ok(());
    };
    let mut covered: Vec<Vec<bool>> = snapshots
        .iter()
        .map(|group| {
            let mut flags = vec![false; accounts_per_group];
            for (local, _) in group {
                if let Some(flag) = flags.get_mut(*local as usize) {
                    *flag = true;
                }
            }
            flags
        })
        .collect();
    let mut check = |owner: crate::runtime::Owner, what: &str| -> Result<(), String> {
        let (g, local) = (owner.0 as usize, owner.1 as usize);
        let ok = covered
            .get(g)
            .and_then(|flags| flags.get(local))
            .copied()
            .unwrap_or(false);
        if !ok {
            return Err(format!(
                "账户分片缺项（组端 undo 集漏标？）：{what} owner=(g={g}, local={local}) 不在分片"
            ));
        }
        Ok(())
    };
    for fact in &rec.desk_trades {
        check(fact.maker, "成交 maker")?;
        check(fact.taker, "成交 taker")?;
    }
    for fact in &rec.desk_lifecycle {
        check(fact.owner, "生命周期")?;
    }
    for fact in &rec.desk_receipts {
        check(fact.owner, "回执")?;
    }
    Ok(())
}

/// 常驻 runtime 驱动器。
pub struct RuntimeDriver {
    world: World,
    /// Owner(组内下标) → 引擎 AccountId（玩家=0 惯例位）。
    pub id_map: Vec<AccountId>,
    /// P2-4：id_map 的 Arc 共享副本（构造一次——session 信封派生快照用，
    /// 免每拍 clone 全名册映射）。
    id_map_shared: std::sync::Arc<Vec<AccountId>>,
    /// 每股 desk 序号游标缓存（最近在途快照收集时的权威值——Sol 四轮
    /// 修复：save 投影/DeskBookMirror 的游标源）。
    desk_next_serials: Vec<u64>,
    /// 事件 seq（协调者全局单调）。
    next_seq: u64,
    /// P2-1a 红证计数（cfg(test)）：set_runtime_personal_state 实际安装
    /// 次数——守卫测试断言"安装数==dirty 位计数"（全量重装突变即红）。
    #[cfg(test)]
    personal_installs: u64,
    /// P2-1b：日结（Drain/T+1/AdminBatch 改账户但无逐账户事实事件）后
    /// 置 true——下一 step 全量回写一次（低频路径，语义安全优先）。
    /// P2 复核阻断 2 后改 pub(crate)：生产日结路径（session/failure.rs
    /// 的 ends_day 段）需直接置位（此前仅手动 day_end 设=接线缺口）。
    pub(crate) force_full_accounts: bool,
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
    /// P2-4b：上一拍在途单快照基线（信封账本增量维护的差分源——
    /// None=基线缺失，首拍全量重建后建立）。
    resting_echo_prev: Option<Vec<Vec<super::protocol::RestingFact>>>,
    /// Sol 终审四审 A2：每股构造期日 K 历史长度（install_candles 的
    /// 归档上限基准——desk 侧 roll_day 按绝对 tick 界自动归档，超前
    /// 于 session 日终（session.day 未推进时档校验 fault2064 期望少
    /// 一条）；push 上限=initial+session.day（归档不超前日历）。
    initial_candle_history_len: std::collections::BTreeMap<crate::StockId, usize>,
    /// P2-4a→4b 红证旋钮（cfg(test)）：强制每拍全量重建信封账本
    ///（增量路径 A/B 等价守卫的对照臂）。
    #[cfg(test)]
    force_full_envelope_rebuild: bool,
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
        let mut setup: SessionSetup = session.export_setup_for_runtime();
        // StockId 按代码升序分配；交易部、账户矩阵与报价数组必须共用该顺序。
        setup.stocks.sort_unstable_by(|a, b| a.code.0.cmp(&b.code.0));
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
        // Sol 四轮修复：恢复链序号游标贯通——session 权威（save 的
        // book_next_sequences 经 restore 载入旧簿游标；运行中镜像
        // next_serial）投影为新 Desk 的起始 next_serial（空簿也保留
        // 已耗序号——R26）。
        spec.initial_desk_next_serial = session.runtime_book_next_sequences();
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
        // D5 restore 链最后一层（Sol 终审四审预留族重述）：desk 种子的
        // 组端资源预留——**restore 账本与 adopt 旧簿两形态统一**。基线
        /// 投影（accounts_projection）读的是 session 公开面：现金=组现金
        /// +在途买占用（escrow 加回）、持仓=组 shares+在途卖余量（qty
        /// 含在途口径）。组端严格口径须在基线之上反向扣除：
        /// · 买种子 purse → 组现金扣减 + buy_purse_held 登记（公开现金
        ///   =组现金+占用重建，face 不变；adopt 形态此前不扣——费用
        ///   边际滞留组内可再消费）；
        /// · 卖种子余量 → 组 shares 扣减 + pending_sell_qty 登记（face
        ///   不变；restore 形态此前在 face 之上**加**余量=双计使可卖
        ///   翻倍——resting_sell_orders_reserve 显红；adopt 形态不扣
        ///   则成交后组侧幻影股，日结全量回写复活已卖持仓）。
        let desk_order_seeds = resting_order_seeds_from_session(session, &ids, per_group)?;
        {
            let mut purse_by_group = vec![vec![0i64; per_group]; n_groups];
            let mut pending_sell_by_group = vec![vec![vec![0u32; n_stocks]; per_group]; n_groups];
            for (stock, seeds) in desk_order_seeds.iter().enumerate() {
                if stock >= n_stocks {
                    return Err(format!(
                        "desk 种子股索引越界（无法投影）：stock={stock}（股数 {n_stocks}）"
                    ));
                }
                for seed in seeds {
                    let (g, l) = (seed.owner.0 as usize, seed.owner.1 as usize);
                    if g >= n_groups || l >= per_group {
                        return Err(format!(
                            "desk 种子组位越界（无法投影）：owner={:?}（组数 {n_groups}，每组 {per_group}）",
                            seed.owner
                        ));
                    }
                    match seed.side {
                        crate::orderbook::Side::Buy => {
                            let slot = &mut cash[g][l];
                            *slot = (*slot).checked_sub(seed.purse).ok_or_else(|| {
                                format!(
                                    "desk 买种子 purse 超过组现金（档损坏）：owner={:?}, stock={stock}, purse={}",
                                    seed.owner, seed.purse
                                )
                            })?;
                            purse_by_group[g][l] += seed.purse;
                        }
                        crate::orderbook::Side::Sell => {
                            let slot = &mut shares[g][l][stock];
                            *slot = (*slot).checked_sub(seed.qty).ok_or_else(|| {
                                format!(
                                    "desk 卖种子余量超过组持仓（档损坏）：owner={:?}, stock={stock}, qty={}",
                                    seed.owner, seed.qty
                                )
                            })?;
                            pending_sell_by_group[g][l][stock] += seed.qty;
                        }
                    }
                }
            }
            spec.initial_purse_held = purse_by_group;
            spec.initial_pending_sell = pending_sell_by_group;
        }
        // D0：世界从会话当前 tick 起步（锁步不变量的构造期对齐——
        // 全新会话即 0；恢复/接管场景相位按绝对 tick 推导）。
        spec.initial_committed_tick = session.tick();
        // Sol 终审四审 A2（restore 重复日结）：恢复形态的上一拍若为
        /// 日结拍（tick%tpd==0——session 日终迁移已完成）即视为该交易
        /// 日已日结，防首拍 run_ticks 自动日结重复归档日 K。
        let restore_tpd = u64::from(session.export_setup_for_runtime().ticks_per_day).max(1);
        spec.initial_last_day_end_tick =
            if session.tick() > 0 && session.tick() % restore_tpd == 0 {
                Some(session.tick())
            } else {
                None
            };
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
        // （种子已在预留段先行计算——组端现金/持仓扣减与之同源。）
        spec.initial_desk_orders = desk_order_seeds;
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
            id_map_shared: std::sync::Arc::new(ids.clone()),
            id_map: ids.clone(),
            desk_next_serials: Vec::new(),
            next_seq: 0,
            #[cfg(test)]
            personal_installs: 0,
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
            resting_echo_prev: None,
            // Sol 终审四审 A2 修正：initial 记**净预置史**（构造时长度−
            /// 构造时 session.day）——restore 场景构造期长度已含已归档日
            /// （361=360 预置+1 已归档），直接当 initial 会使归档上限
            /// 双计（limit=361+day → push 超前一条，档校验 fault2064）。
            initial_candle_history_len: session
                .runtime_candle_histories()
                .into_iter()
                .map(|(code, len)| {
                    let net = len.saturating_sub(session.day() as usize);
                    (code, net)
                })
                .collect(),
            #[cfg(test)]
            force_full_envelope_rebuild: false,
        };
        Ok(driver)
    }

    /// Sol 二审修复集 5：会话旧簿在途挂单迁入世界（ExternalPlan 路由——
    /// 与玩家意图注入同一通道）。仅构造期调用：runtime 步进开始后 session
    /// 旧簿不再新增挂单（写面在组/desk），无重复迁移。owner 不在名册=
    /// 投影缺口，显式失败（不静默丢单）。
    /// Sol 终审修复 1c 同族：owner→(组, 组内 local) 换算+按组定向发送
    ///（此前全局下标当 local 广播给组 0——多组下错归）。
    fn adopt_session_resting_orders(&mut self, session: &GameSession) -> Result<(), String> {
        let n_groups = self.world.n_groups();
        let mut ops: Vec<Vec<super::decision::PendingOp>> = vec![Vec::new(); n_groups];
        for (code, orders) in session.runtime_resting_orders_snapshot() {
            for order in orders {
                let (g, local) = self.owner_slot(order.owner).map_err(|e| {
                    format!("旧簿挂单 {e}（无法迁移）：{:?}", code)
                })?;
                ops[g as usize].push(super::decision::PendingOp {
                    local,
                    stock: code.0 as super::StockIdx,
                    side: order.side,
                    routed: super::protocol::RoutedPrice::Fixed(order.price.cents()),
                    purse_reference: order.price.cents(),
                    qty: order.qty,
                });
            }
        }
        self.world.send_external_plan_grouped(ops, vec![Vec::new(); n_groups]);
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
        // P0 画像：本函数各段用块作用域 + guard 计时（未启用=零成本）。
        use super::step_profile::{self, StepSegment};
        // D4 相位：密封分配快照段（tick 起点，组/部广播前）。
        crate::verification_evidence::enter_phase(
            crate::session::pipeline::TickPhase::SealAllocationSnapshot,
        );
        // 【P0 画像】world_run 段：裸引擎本体（对照基线——D 系列之外）。
        let last_prices: Vec<i64> = {
            let _seg = step_profile::scope(StepSegment::WorldRun);
            // D1：新增持仓补经历条目（fault2038 存档校验依赖）——在快照
            // 回写前取现价。
            let prices: Vec<i64> = self
                .world_public_views()
                .iter()
                .map(|v| v.last_price.max(1))
                .collect();
            self.world
                .run_ticks(1)
                .map_err(|e| format!("runtime tick 失败：{e:?}"))?;
            prices
        };
        // D3 台账收口（入场参考价近似→闭合）：新持仓经历入场参考用本
        // tick 买入腿成交 VWAP（旧管线以真实成交成本为基准；此前近似
        // 现价）。后续加仓的 experience 迁移仍近似（登记——D5 经历域）。
        // 【P0 画像】vwap_calc 段。
        let buy_vwap: std::collections::BTreeMap<
            (super::GroupIdx, super::LocalIdx, super::StockIdx),
            i64,
        > = {
            let _seg = step_profile::scope(StepSegment::VwapCalc);
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
        // Q2 批 3（视图内生战役）：账户稀疏分片随 SealAck 上行（Seal
        /// 落账后 undo 集口径——决策扣除/全部轮成交/退款终结全含；
        /// acct 竞态根因由 Seal 的 S4 不变量消除）。驱动器读发布版本，
        /// EchoAccountsSparse 独立往返退役；覆盖校验承接原"缺项=
        /// RuntimeError"语义（请求集改述为本拍事实集）。全量读面
        ///（force_full/日结/调试）仍走 EchoAccounts。
        let fact_rec = self.world.report_log.last().cloned();
        let _ = self.id_map.len();
        // 【P0 画像】acct_echo_collect 段（残余=发布版本分片重组；
        /// fact_rec 的 clone 归 remainder——事件面 R4 必留）。
        let snapshots = {
            let _seg = step_profile::scope(StepSegment::AcctEchoCollect);
            let skip_all = !self.force_full_accounts;
            if skip_all {
                let published = self.world.publication().load();
                if published.tick != self.world.global.committed_tick {
                    return Err(format!(
                        "发布版本滞后：published={} 而 committed={}（单点发布不变量破坏）",
                        published.tick, self.world.global.committed_tick
                    ));
                }
                let n_groups = self.world.n_groups();
                let mut pairs: Vec<Vec<(super::LocalIdx, super::group::GroupAccount)>> =
                    vec![Vec::new(); n_groups];
                for shard in &published.groups {
                    let Some(slot) = pairs.get_mut(shard.group as usize) else {
                        return Err(format!(
                            "账户分片组号越界：g={}（组数 {n_groups}）",
                            shard.group
                        ));
                    };
                    *slot = shard.accounts.iter().cloned().collect();
                }
                ensure_fact_owners_covered(fact_rec.as_ref(), &pairs, self.accounts_per_group)?;
                pairs
            } else {
                // 日结/首拍全量（EchoAccounts 全量读面不变）——转 (组,
                // 下标) 对形态与稀疏路径共用回写循环。
                let dense = self
                    .world
                    .snapshot_accounts()
                    .map_err(|e| format!("账户快照失败：{e:?}"))?;
                let mut pairs = vec![Vec::new(); self.world.n_groups()];
                for (g, accounts) in dense {
                    let slot = pairs
                        .get_mut(g as usize)
                        .ok_or_else(|| format!("全量账户快照组号越界：g={g}"))?;
                    *slot = accounts
                        .into_iter()
                        .enumerate()
                        .map(|(local, acct)| (local as super::LocalIdx, acct))
                        .collect();
                }
                pairs
            }
        };
        // 【P0 画像】acct_writeback 段（session 镜像回写循环）。
        {
            let _seg = step_profile::scope(StepSegment::AcctWriteback);
            for (g, accounts) in snapshots.into_iter().enumerate() {
            for (local, acct) in accounts {
                let global = g * self.accounts_per_group + local as usize;
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
                        // Sol 终审修复 1c：VWAP 查找用真实组号（此前写死
                        /// group=0——多组下组 0 的 VWAP 误配给其他组账户）。
                        let entry = buy_vwap
                                .get(&(
                                    g as super::GroupIdx,
                                    local as super::LocalIdx,
                                    idx as super::StockIdx,
                                ))
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
                // Sol 终审修复 1b：本账户的首买经历 init 收集并入成员槽
                ///（此前局部 Vec 随循环体丢弃——Continue 早轮成交已被排除
                /// 出 unsettled_trades，组回写段是其唯一 init 机会，丢弃
                /// 形成"有持仓、无经历"，破坏 fault2038 保存校验）。
                self.pending_hold_inits.extend(pending_hold_inits);
            }
            }
            // P2-1b：本 tick 事实集传下一拍（并集自身不传递——否则一旦
            // dirty 永远 dirty）；日结强制标记在此消费（本次已全量）。
            self.force_full_accounts = false;
        }
        // 事件：Trade（双腿）+ 生命周期（受理/撤单/到期）+ 回执拒收。
        let rec = fact_rec;
        // Q2 批 4（D3 退役）：同拍成交公开投影删除——Seal 落账（批 2）
        // 使组账户在发布点已含本拍全部成交，驱动器回写（发布分片）
        // 即期末权威值，无"组 staging 未落账尾巴"可叠加。原投影的
        // 双写补偿语义（Sol 五审修复 1）由 Seal 承接；Continue 早轮
        // 的恰一次落账不变（prereq1 合同承载）。
        // 【P0 画像】event_assembly 段（事件投影——R4 必留面的成本基线）。
        let (trades, events) = {
            let _seg = step_profile::scope(StepSegment::EventAssembly);
            let trades = rec
                .as_ref()
                .map(|r| r.desk_trades.clone())
                .unwrap_or_default();
            let lifecycle = rec
                .as_ref()
                .map(|r| r.desk_lifecycle.clone())
                .unwrap_or_default();
            let mut receipts = rec
                .as_ref()
                .map(|r| r.desk_receipts.clone())
                .unwrap_or_default();
            // Sol 复审修复 2：组端业务拒绝（资金/可卖/零股/竞价×市价）与
            /// desk 回执同入事件流（IntentRejected 映射）。
            if let Some(r) = rec.as_ref() {
                receipts.extend(r.group_rejections.iter().copied());
            }
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
            // step_frame 接轨件 2：先加后取（对齐旧管线 collect 语义）。
            for (code, owner) in std::mem::take(&mut self.pending_rejections) {
                self.next_seq = self
                    .next_seq
                    .checked_add(1)
                    .expect("事件 seq 溢出");
                events.push(Event::IntentRejected {
                    seq: self.next_seq,
                    account: owner,
                    code,
                    reason: crate::RejectionReason::InvalidQuantity,
                });
            }
            (trades, events)
        };
        let outcome = Ok(events);
        // D4 相位：派生审计段（回写后对账面——本批为标记位，D3 候选
        // 校验接入后承载实质校验计时）。
        crate::verification_evidence::enter_phase(
            crate::session::pipeline::TickPhase::DerivationAudit,
        );
        // Sol 整改个人态交付投影：组权威个人态（observe 演进）上行→
        // session 镜像回写（reference/peak equity 等读面；Belief-1b 的
        // information 域投递回写保持——整态覆盖与其同源）。
        // 【P0 画像】belief_echo 段（D1 EchoBelief 残余=session 整态安装；
        /// P2-2：独立 EchoBelief 往返退役——稀疏分片已随 GroupReport 上行
        /// 进发布版本，此处从发布版本读（读面归 owner 内生维护）。
        {
            let _seg = step_profile::scope(StepSegment::BeliefEcho);
            let belief_states = {
                let published = self.world.publication().load();
                if published.tick != self.world.global.committed_tick {
                    return Err(format!(
                        "发布版本滞后：published={} 而 committed={}（单点发布不变量破坏）",
                        published.tick, self.world.global.committed_tick
                    ));
                }
                published
                    .groups
                    .iter()
                    .map(|shard| {
                        (
                            shard.group,
                            shard.belief.iter().cloned().collect::<Vec<_>>(),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            {
                for (group, states) in belief_states {
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
        }
        // D5 #4：首买经历 init（回写/投影段收集——个人态整态回写之后
        /// 执行，组权威经验行不冲掉首买条目）。
        // 【P0 画像】hold_inits 段。
        {
            let _seg = step_profile::scope(StepSegment::HoldInits);
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
        // 【P0 画像】plans_echo 段（D1 EchoPlans + D3 同拍执行腿补写）。
        {
            let _seg = step_profile::scope(StepSegment::PlansEcho);
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
                    // 计划记录（含 terminal）即安装。
                    // Q2 批 4（D3 退役）：本拍成交执行腿补写删除——
                    // Seal 落账使组簿 collect 已含全部轮成交推进
                    //（apply_staging 的 apply_fill_plan_progress）。
                    session.set_runtime_plans(plans);
                }
            }
            Err(projection_error) => return Err(projection_error),
            }
        }
        // D3b 件 1：成交触达账户的个人记忆裁剪（同拍——组内 prune 在下一
        // Start 落账；个人态同步在上方用组镜像整态覆盖，裁剪必须在其后；
        // 下一拍组镜像已裁剪同值，双写不漂移）。
        // 【P0 画像】memory_prune 段（D3）。
        {
            let _seg = step_profile::scope(StepSegment::MemoryPrune);
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
        }
        outcome
    }

    /// 事件 seq 游标（诊断）。
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// 分配一条事件 seq（先加后取——与事件流同一游标；调用方负责
    /// 随后 `set_runtime_seq(driver.next_seq())` 同步会话权威）。
    /// step_frame 接轨件 4：DayBoundary 等安装段事件此前直改
    /// `state.seq` 而不推进驱动器游标——下一拍驱动器重复分配同一
    /// seq（tick_frame_commit 的日结后首拍 coverage 显红）。
    pub fn take_event_seq(&mut self) -> u64 {
        self.next_seq = self
            .next_seq
            .checked_add(1)
            .expect("事件 seq 溢出（u64 单调计数）");
        self.next_seq
    }

    /// step_frame 接轨件 3：连续相位行情事件生成（旧管线
    /// finish_market_data 的 runtime 等价）——每连续拍每股一条
    /// `Event::PriceTick`（客户端 timeseries 依赖；竞价相位由
    /// desk_auction facts 承载 AuctionTick/AuctionCompleted，PreOpen/
    /// 日结拍不产——与旧管线同判）。seq 经驱动器游标分配（先加后取
    /// ——与事件流同一空间），随后同步会话权威。
    pub fn emit_price_tick_events(
        &mut self,
        session: &mut GameSession,
        events: &mut Vec<Event>,
    ) -> Result<(), String> {
        // 相位基准=本拍**开始时**相位（tick−1 的 day_tick——与旧管线
        /// finish_market_data 的 boundary 政策同拍口径；用步后 tick 判
        /// 会使转连续过渡拍早一拍产 PriceTick，parity 对照显红）。
        let setup = session.export_setup_for_runtime();
        let tpd = setup.ticks_per_day.max(1);
        let day_tick_before = session.tick().saturating_sub(1) % tpd;
        let entry = setup.auction_ticks - setup.auction_ticks / 3;
        let closing_start = tpd.saturating_sub(setup.closing_auction_ticks);
        let phase_before = if day_tick_before < entry {
            crate::TradingPhase::CallAuction
        } else if day_tick_before < setup.auction_ticks {
            crate::TradingPhase::PreOpen
        } else if day_tick_before >= closing_start && setup.closing_auction_ticks > 0 {
            crate::TradingPhase::ClosingAuction
        } else {
            crate::TradingPhase::Continuous
        };
        if phase_before != crate::TradingPhase::Continuous {
            return Ok(());
        }
        let tick_after = session.tick();
        let candle_time = session.candle_date_time_for_runtime();
        // 逐字段借用（不 clone 整个 StockPublic——recent/minute 等大面无关）。
        let views: Vec<(i64, Vec<(i64, u64)>, Vec<(i64, u64)>)> = self
            .world_public_views()
            .iter()
            .map(|v| (v.last_price, v.depth_bids.clone(), v.depth_asks.clone()))
            .collect();
        for (idx, (last_price, bids, asks)) in views.into_iter().enumerate() {
            let code = crate::StockId(u32::try_from(idx).map_err(|_| {
                format!("PriceTick 生成：股下标 {idx} 超 u32（世界规模越界）")
            })?);
            let last = crate::money::Money::from_cents(last_price.max(1));
            // 活动 K：session 侧（install_candles 已装，含 stats）优先；
            // 空拍缺席合成零成交占位（o=h=l=c=last——对齐旧管线
            // checked_mark_candle(None, time, last, 0) 的占位语义）。
            let daily_candle = session.runtime_active_candle(&code).unwrap_or(
                crate::session::DailyCandle {
                    time: candle_time,
                    open: last,
                    high: last,
                    low: last,
                    close: last,
                    volume: 0,
                    trade_stats: Some(Default::default()),
                },
            );
            let seq = self.take_event_seq();
            events.push(Event::PriceTick {
                seq,
                tick: tick_after,
                code,
                last_price: last,
                daily_candle,
                bids: bids
                    .into_iter()
                    .map(|(p, q)| (crate::money::Money::from_cents(p), q))
                    .collect(),
                asks: asks
                    .into_iter()
                    .map(|(p, q)| (crate::money::Money::from_cents(p), q))
                    .collect(),
            });
        }
        // 会话权威与驱动器游标保持合一（PriceTick 分配推进了游标）。
        session.set_runtime_seq(self.next_seq);
        Ok(())
    }

    /// runtime 全局日历读取（诊断/对账面）。
    pub fn world_global_day(&self) -> u32 {
        self.world.global_day()
    }

    /// world 已转正 tick（D0 锁步不变量的驱动器侧读数）。
    pub fn world_committed_tick(&self) -> u64 {
        self.world.global.committed_tick
    }

    /// Q1 发布槽句柄（驱动器侧转发——session/server/测试读者经此取
    /// 一致读点；install_tick 单点发布，load 一次 clone）。
    pub fn publication(
        &self,
    ) -> std::sync::Arc<super::publication::ViewPublication> {
        self.world.publication()
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
    /// 附带缓存每股 desk 真实 next_serial（Sol 四轮修复：save 投影/
    /// DeskBookMirror 游标源——空簿也保留已耗序号）。
    /// P2-3（视图内生战役）：生产每拍读面改从**发布版本**取（在途单观察
    /// 面已随 DeskReport 上行进 PublishedTick.desks——EchoResting 独立
    /// 往返退役）。游标缓存同步自发布版本（save 投影权威源）。
    /// 发布滞后=单点发布不变量破坏，显式失败（不静默装旧版）。
    pub fn collect_resting_facts(
        &mut self,
    ) -> Result<Vec<Vec<super::protocol::RestingFact>>, String> {
        let published = self.world.publication().load();
        if published.tick != self.world.global.committed_tick {
            return Err(format!(
                "发布版本滞后：published={} 而 committed={}（单点发布不变量破坏）",
                published.tick, self.world.global.committed_tick
            ));
        }
        let mut facts = vec![Vec::new(); published.desks.len()];
        let mut cursors = vec![0u64; published.desks.len()];
        for shard in &published.desks {
            let idx = shard.stock as usize;
            if let (Some(slot), Some(cursor)) = (facts.get_mut(idx), cursors.get_mut(idx)) {
                *slot = shard.resting.as_ref().clone();
                *cursor = shard.next_serial;
            }
        }
        self.desk_next_serials = cursors;
        Ok(facts)
    }

    /// P2-3：live 收集通道（日结重刷等**发布版本外**的静点读面——日结
    /// 清簿后发布版本仍是清簿前 tick 的，须向 desk 现拉）。
    pub fn collect_resting_facts_live(
        &mut self,
    ) -> Result<Vec<Vec<super::protocol::RestingFact>>, String> {
        let (facts, cursors) = self
            .world
            .collect_desk_resting()
            .map_err(|e| format!("在途单快照 live 收集失败：{e:?}"))?;
        self.desk_next_serials = cursors;
        Ok(facts)
    }

    /// 每股 desk 序号游标（最近一次在途快照收集时的权威值）。
    pub fn desk_next_serials(&self) -> &[u64] {
        &self.desk_next_serials
    }

    /// P2-4（视图内生战役）：信封派生快照——发布版本在途单分片句柄
    ///（Arc 共享零拷贝）+id_map 共享副本。session 侧派生投影的数据源
    ///（每拍 step 尾存入 session，替代 ledger 每拍增量维护）。游标缓存
    /// 同步自发布版本（mirror/save 投影源）。
    pub fn resting_snapshot_for_session(
        &mut self,
    ) -> Result<
        (
            Vec<std::sync::Arc<Vec<super::protocol::RestingFact>>>,
            std::sync::Arc<Vec<AccountId>>,
        ),
        String,
    > {
        let published = self.world.publication().load();
        if published.tick != self.world.global.committed_tick {
            return Err(format!(
                "发布版本滞后：published={} 而 committed={}（单点发布不变量破坏）",
                published.tick, self.world.global.committed_tick
            ));
        }
        let n = published.desks.len();
        let mut facts = vec![std::sync::Arc::new(Vec::new()); n];
        let mut cursors = vec![0u64; n];
        for shard in &published.desks {
            let idx = shard.stock as usize;
            if let (Some(slot), Some(cursor)) = (facts.get_mut(idx), cursors.get_mut(idx)) {
                *slot = std::sync::Arc::clone(&shard.resting);
                *cursor = shard.next_serial;
            }
        }
        self.desk_next_serials = cursors;
        Ok((facts, std::sync::Arc::clone(&self.id_map_shared)))
    }

    /// 每组账户数（Owner→全局下标换算基——快照派生用）。
    pub fn accounts_per_group(&self) -> usize {
        self.accounts_per_group
    }

    /// P2-4b：对相邻两拍在途单快照差分（信封账本增量维护输入）。
    /// 键=(股下标, Owner, side, order_serial)；值变化（部分成交）按
    /// Changed 整值替换。基线缺失（构造后首拍/红证强制）返回
    /// full_rebuild 并以本拍建立基线。**等价合同**：给定基线=上一拍
    /// 快照，本差分应用后的账本内容 == 以本拍快照全量重建的内容
    ///（守卫：p2_4b_envelope_incremental_matches_full_rebuild）。
    pub fn resting_delta_since_last_echo(
        &mut self,
        current: &[Vec<super::protocol::RestingFact>],
    ) -> super::protocol::RestingDelta {
        #[cfg(test)]
        if self.force_full_envelope_rebuild {
            self.resting_echo_prev = Some(current.to_vec());
            return super::protocol::RestingDelta {
                full_rebuild: true,
                entries: Vec::new(),
            };
        }
        let Some(prev) = self.resting_echo_prev.take() else {
            self.resting_echo_prev = Some(current.to_vec());
            return super::protocol::RestingDelta {
                full_rebuild: true,
                entries: Vec::new(),
            };
        };
        let mut delta = super::protocol::RestingDelta {
            full_rebuild: false,
            entries: Vec::new(),
        };
        // 股数不一致（世界重建/构造形态变化）：快照面错配，退全量
        ///（不静默截断差分）。前置判——zip 截断不产生半截差分。
        if prev.len() != current.len() {
            self.resting_echo_prev = Some(current.to_vec());
            return super::protocol::RestingDelta {
                full_rebuild: true,
                entries: Vec::new(),
            };
        }
        for (stock, (cur_book, prev_book)) in current.iter().zip(prev.iter()).enumerate() {
            // 键控差分（O(n)——首版逐项线性扫 O(n²) 在 resting ~4700 时
            /// 每股每拍 2200 万次比较，1000 tick 长跑 271→135 显著回退，
            /// 实测打回）。
            let mut prev_index: std::collections::HashMap<
                (super::Owner, crate::orderbook::Side, u64),
                &super::protocol::RestingFact,
            > = prev_book
                .iter()
                .map(|f| ((f.owner, f.side, f.order_serial), f))
                .collect();
            for fact in cur_book {
                match prev_index.remove(&(fact.owner, fact.side, fact.order_serial)) {
                    None => delta.entries.push(super::protocol::RestingDeltaEntry::Added {
                        stock: stock as super::StockIdx,
                        fact: fact.clone(),
                    }),
                    Some(prev_fact) if prev_fact != fact => {
                        delta.entries.push(super::protocol::RestingDeltaEntry::Changed {
                            stock: stock as super::StockIdx,
                            fact: fact.clone(),
                        })
                    }
                    Some(_) => {}
                }
            }
            for (_, fact) in prev_index {
                delta
                    .entries
                    .push(super::protocol::RestingDeltaEntry::Removed {
                        stock: stock as super::StockIdx,
                        owner: fact.owner,
                        side: fact.side,
                        order_serial: fact.order_serial,
                    });
            }
        }
        self.resting_echo_prev = Some(current.to_vec());
        #[cfg(test)]
        if let Some(counts) = crate::runtime::drive::RESTING_DELTA_FACE_COUNTS.with(|c| c.borrow().clone()) {
            let mut counts = counts.lock().expect("面计数锁");
            for e in &delta.entries {
                match e {
                    super::protocol::RestingDeltaEntry::Added { .. } => counts.0 += 1,
                    super::protocol::RestingDeltaEntry::Removed { .. } => counts.1 += 1,
                    super::protocol::RestingDeltaEntry::Changed { .. } => counts.2 += 1,
                }
            }
        }
        delta
    }

    /// P2-4b 红证旋钮（cfg(test)）：强制每拍全量重建信封账本（A/B
    /// 等价守卫对照臂）。
    #[cfg(test)]
    pub fn set_force_full_envelope_rebuild_for_test(&mut self, on: bool) {
        self.force_full_envelope_rebuild = on;
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
        // Sol 收官再复核阻断 3（最终形态）：分段编号分段取——竞价事件
        ///（DayBoundary 之前输出）先入独立缓冲；撤单事件编号**延迟**到
        /// DayBoundary 序号分配之后（collect_drained_lifecycle_after_
        /// boundary）。此前一次性编号使 DayBoundary(24) 晚于撤单(23)
        /// 而输出在其前——seq 非单调。
        self.collect_drained_auction_events();
    }

    /// 日结竞价事件段（DayBoundary 之前输出）——立即编号。
    fn collect_drained_auction_events(&mut self) {
        let drained_auction = std::mem::take(&mut self.world.drained_auction);
        if !drained_auction.is_empty() {
            let mut auction_events = project_auction_events(&drained_auction, &mut self.next_seq);
            if !auction_events.is_empty() {
                self.drained_events.append(&mut auction_events);
            }
        }
    }

    /// 日结撤单事件段（DayBoundary 之后输出）——在 DayBoundary 序号
    /// 分配完成后由 failure.rs 段调用编号。
    pub fn collect_drained_lifecycle_after_boundary(&mut self) {
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

    /// Sol 六审段 1.2（事件序）：分离取走日结 drained 事件中的**竞价**
    /// 事件（AuctionTick/AuctionCompleted——需先于 DayBoundary；余下
    /// 撤单事件仍由 take_drained_events 按原序取走）。
    pub fn take_drained_auction_events(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        self.drained_events.retain(|event| {
            let is_auction = matches!(
                event,
                Event::AuctionTick { .. } | Event::AuctionCompleted { .. }
            );
            if is_auction {
                out.push(event.clone());
            }
            !is_auction
        });
        out
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
        // driver 直调路径无 DayBoundary 事件——撤单段紧随竞价段编号
        ///（编号序=输出序：auction → lifecycle）。
        self.collect_drained_lifecycle_after_boundary();
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
        // Sol 六审段 1.2 取证修复：日结 drained 事件（含收盘竞价清算
        /// AuctionTick/AuctionCompleted）经 next_seq 分配后**回写会话
        /// seq 权威**——否则下一拍 step_frame_equivalent 的
        /// adopt_session_seq 从会话旧值重取游标，日结事件已耗的
        /// seq 被复用（batch4_seq_continuous 15<=15 双用实证）。
        session.set_runtime_seq(self.next_seq);
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
            Vec<Vec<super::decision::PendingOp>>,
            Vec<Vec<(super::LocalIdx, super::StockIdx, u64)>>,
        ),
        String,
    > {
        // Sol 终审修复 1c 同族：多组名册的 owner→(组, 组内 local) 换算
        ///（此前全局下标直接当 local——双组下组 1 账户的单被发给组 0
        /// 的越界 local，tick 失败）。产物按组号分桶，定向发送。
        let n_groups = self.world.n_groups();
        let mut ops: Vec<Vec<super::decision::PendingOp>> = vec![Vec::new(); n_groups];
        let mut cancels: Vec<Vec<(super::LocalIdx, super::StockIdx, u64)>> =
            vec![Vec::new(); n_groups];
        for r in intents {
            // D5 终程：Cancel 意图路由（OrderId 低 48 位=desk serial——
            /// 组端直转 DeskIn::Cancel；撤单窗/存在性由 desk 权威判定）。
            if let crate::Intent::Cancel { code, id } = &r.intent {
                let (g, local) = self.owner_slot(r.owner)?;
                cancels[g as usize].push((
                    local,
                    code.0 as super::StockIdx,
                    super::group::desk_order_serial(id.0),
                ));
                continue;
            }
            let crate::Intent::PlaceLimit { code, side, price, qty } = &r.intent else {
                // PlaceMarket（sol-103 上报项 2 修复）：此前此处静默
                // continue——默认翻转后玩家市价单经 step() 被静默丢弃
                // （铁律二违规）。desk 已完整支持 RoutedPrice::Market
                // （涨跌停保护、逐档即成、余量不进簿）；竞价相位由
                // desk 显式拒收（AuctionLimitOrderRequired 回执）。
                let crate::Intent::PlaceMarket { code, side, qty } = &r.intent else {
                    // 词表只剩以上三种（scope_boundary 编译期锚）——
                    // 不可达；防御式显式失败不静默。
                    return Err(format!(
                        "外部意图词表外变体（无法路由）：{:?}",
                        r.intent
                    ));
                };
                let (g, local) = self.owner_slot(r.owner)?;
                let upper = upper_limits
                    .get(code.0 as usize)
                    .copied()
                    .unwrap_or(0);
                // D5 组端校验：qty=0 意图路由层拒收（同限价臂）。
                if *qty == 0 {
                    self.pending_rejections.push((*code, r.owner));
                    continue;
                }
                // purse 最坏情况=日涨停（与 decision.rs intent_to_pending
                /// 的 NPC 市价路径同口径，F §1.5）。
                ops[g as usize].push(super::decision::PendingOp {
                    local,
                    stock: code.0 as super::StockIdx,
                    side: *side,
                    routed: super::protocol::RoutedPrice::Market,
                    purse_reference: upper,
                    qty: *qty,
                });
                continue;
            };
            let (g, local) = self.owner_slot(r.owner)?;
            let upper = upper_limits
                .get(code.0 as usize)
                .copied()
                .unwrap_or(0);
            let (routed, purse_reference) = match price {
                crate::strategy::LimitPrice::Fixed(p) => {
                    (super::protocol::RoutedPrice::Fixed(p.cents()), p.cents())
                }
                // 符号价（sol-103 上报项 2 附带修复）：按变体正向路由——
                // 此前 `_ =>` 把 Lowest 也折叠成 Highest（玩家 Lowest 卖
                /// 会解析到涨停）。purse 最坏情况（Sol 复审修复 4，
                /// ADR-0022 裁定）：Highest=日涨停；**Lowest=日跌停**
                /// （Lowest 买等效跌停限价单，永远不可能高于跌停成交，
                /// 预留跌停即足够——F §1.5"统一涨停"属过度预留，已按
                /// 裁定对齐并同步 F 文档注记）。
                crate::strategy::LimitPrice::Highest => {
                    (super::protocol::RoutedPrice::Highest, upper)
                }
                crate::strategy::LimitPrice::Lowest => {
                    let lower = self
                        .world_public_views()
                        .get(code.0 as usize)
                        .map(|v| v.daily_lower_limit)
                        .unwrap_or(upper);
                    (super::protocol::RoutedPrice::Lowest, lower)
                }
            };
            // D5 组端校验：qty=0 意图路由层拒收（desk 侧为防御性 panic
            /// ——组端先验，拒收事件入本拍事件流——旧管线 InvalidQuantity
            /// 回执的 runtime 等价）。
            if *qty == 0 {
                self.pending_rejections.push((*code, r.owner));
                continue;
            }
            ops[g as usize].push(super::decision::PendingOp {
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

    /// owner → (组号, 组内 local)（多组名册换算——单组退化为全局
    /// 下标；owner 不在名册=显式失败不静默）。
    fn owner_slot(
        &self,
        owner: crate::AccountId,
    ) -> Result<(super::GroupIdx, super::LocalIdx), String> {
        let global = self
            .id_map
            .iter()
            .position(|id| *id == owner)
            .ok_or_else(|| format!("外部意图 owner 不在世界名册（无法路由）：{owner:?}"))?;
        let per = self.accounts_per_group.max(1);
        Ok((
            (global / per) as super::GroupIdx,
            (global % per) as super::LocalIdx,
        ))
    }

    /// 外部意图注入世界（D2/D1）：经 ExternalPlan 通道**按组定向**发送
    ///（Sol 终审修复 1c 同族——此前广播给组 0 且全局下标当 local，
    /// 多组下组 1 账户的单路由错组；单组语义不变）。owner 不在名册=
    /// 显式失败（不静默丢单）。
    pub fn inject_player_intents(
        &mut self,
        intents: &[crate::session::ReceiptBearingIntent],
        upper_limits: &[i64],
    ) -> Result<(), String> {
        let (ops, cancels) = self.route_external_intents(intents, upper_limits)?;
        if ops.iter().all(Vec::is_empty) && cancels.iter().all(Vec::is_empty) {
            return Ok(());
        }
        self.world.send_external_plan_grouped(ops, cancels);
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

    /// step_frame 接轨前置约束① fixture 通道：强制下一次轮上报
    /// continuation=true（cfg(test)——Continue 轮的确定性触发）。
    #[cfg(test)]
    pub fn force_continuation_for_test(&mut self) {
        self.world.force_continuation_for_test();
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

    /// P2-4a 红证注入通道（cfg(test)）：组 0 稀疏账户回声回执丢最后
    /// 一项——收集端精确集校验的显式失败判别用。
    #[cfg(test)]
    pub fn corrupt_sparse_echo_omit_last_for_test(&mut self) {
        self.world.corrupt_sparse_echo_omit_last_for_test();
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

    /// P2-3d 守卫读面（cfg(test)）：组端 EchoBelief 真读面（稀疏——本 tick
    /// 演进位）。collect_belief 是驱动器缓存、不反映组槽——Abort 恢复
    /// 完整性守卫须读组端权威槽。
    #[cfg(test)]
    pub fn collect_group_belief_for_test(
        &mut self,
    ) -> Result<
        Vec<(
            super::GroupIdx,
            Vec<(super::LocalIdx, super::world::BeliefParticipantSnapshot)>,
        )>,
        String,
    > {
        self.world
            .collect_group_belief()
            .map_err(|e| format!("belief 组读面失败：{e:?}"))
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
        // step_frame 接轨件 4：seq 权威=会话，每拍从权威重取游标——
        /// civil 面（end_civil_day_update 走旧管线 candidate 的 session
        /// 侧 seq 分配）推进 state.seq 后驱动器游标即过期，本拍重取
        /// 保证分配续接（tick 的锁步校验同式：会话是唯一权威）。
        self.adopt_session_seq(session);
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
    /// step_frame 接轨件 2：游标语义=**最后已用 seq**（与旧管线
    /// `collect_events(facts, state.seq)` 的输入/返回游标同式——分配
    /// 器先加后取，第一条取得 state.seq+1，TickFrame `(seq_from,
    /// seq_to]` 契约；空拍游标不动=权威 seq 不变）。
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
        // 权威（替换"推到 tick"的假迁移）。P2-3e：生产安装段改稀疏上行
        ///（只回本 tick 演进位；非演进位 session 镜像==组权威——不变式
        /// 同 belief P2-2a。B fixture due≈20-40/tick vs 名册 20000）。
        // 【P0 画像】attention_heap 段（D6：稀疏收集 + heap 全名册镜像
        /// 重建——set_runtime_attention 内 O(名册) collect）。
        {
            let _seg = super::step_profile::scope(super::step_profile::StepSegment::AttentionHeap);
            let collected = self
                .world
                .collect_group_attention_sparse()
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
        }
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
        self.install_candles_with_mode(session, false)
    }

    /// Sol 终审四审 A2：post_day_end=world 日结**之后**的补装（ends_day
    /// 段尾）——desk roll 后新日 active 是起始占位，档校验（fault2076）
    /// 期望日界静点 active 为空 ⇒ 强制清除；常规安装（false）不变。
    pub fn install_candles_with_mode(
        &mut self,
        session: &mut GameSession,
        post_day_end: bool,
    ) -> Result<(), String> {
        let finals = self
            .world_desk_finals_for_candles()
            .map_err(|e| format!("K 投影收集失败：{e:?}"))?;
        let tpd_now = u64::from(session.export_setup_for_runtime().ticks_per_day).max(1);
        // 日结拍判定（C2/C3 相位基准）：本拍步后 tick 为 tpd 倍数 ⇒
        /// ends_day 段的日终迁移（commit_active）即将执行。
        let ends_day = session.tick() % tpd_now == 0;
        for (stock_idx, _desk_hist, active) in finals {
            let code = crate::StockId(u32::from(stock_idx));
            // 日 K 相位状态机（Sol 终审四审 public_step 方案 B，协调者
            /// 批复）：归档权收归 session 日终迁移（commit_active）唯一
            /// 承载——desk roll 归档的 hist 条与 session active 是同一份
            /// 数据的两面，运行期不再自 desk hist push（push 与 commit
            /// 并存即双重归档，A2PUSH/A2COMMIT 打点实证；撤 push 后
            /// desk hist 仅余 restore 种子装载与测试面对账两类读者，
            /// 全仓 rg 已核）。本函数只承载 C1（active 装载/占位）与
            /// C3（清空）。多日一次归档守卫随 push 退役（迁移每拍至多
            /// 提交一条 active；一次 step 至多跨一日）。
            // 活跃 K 窗口对齐（存档校验 fault2076）：竞价申报窗
            // （day_tick < auction_entry=10）活跃 K 须为空。
            let s = session.export_setup_for_runtime();
            // C3（方案 B）：active 清空仅两源——迁移后补装
            /// （post_day_end）与**迁移已执行后的**新日申报窗。日结拍
            /// 的常规 install（先于 ends_day 迁移执行）不得清当日
            /// active：session active 仍持当日终态（前拍装载），是日终
            /// 迁移 commit_active 的唯一归档输入；此处清空会使日界
            /// 静点丢当日 K（fault2064 少条——tick8 [0,360,361] 形态）。
            /// 相位基准=迁移执行标志（ends_day/post_day_end），不再以
            /// day_tick 几何单独推断（几何推断正是 premature clear 的
            /// 来源——此前 tick−1 拍前口径尝试亦因与档校验错位交替
            /// 显红，两案均在状态机统一后退役）。
            let day_tick = session.tick() % s.ticks_per_day.max(1);
            let auction_entry = s.auction_ticks - s.auction_ticks / 3;
            // 活动 K 空窗与存档校验 fault2076 同式：竞价申报窗——
            // Sol 终审四审 A2：去掉原 day_tick==0（日界拍）分支——常规
            /// install 在 world 日结**之前**执行（当日 active 必须装载，
            /// session 日终迁移才能提交归档；原分支使 tpd=1 场景的当日
            /// K 永不归档，档少一条 fault2064 反截）。日结后的第二次
            /// install 时 desk roll 已清 active（None=清除，无残留）。
            let active_must_be_empty = post_day_end || (day_tick < auction_entry && !ends_day);
            let candle_time = session.candle_date_time_for_runtime();
            if active_must_be_empty {
                session.clear_active_candle_for_runtime(&code);
            }
            // Sol 终审四审 A2 补齐：无成交日的**占位 K**（旧管线每
            /// PriceTick 拍 mark_candle(None, time, last, 0) 的占位语义
            /// ——日终 commit 归档占位 K，档校验期望=预置+day 不因无
            /// 交易日缺条 fault2064）。desk active 仅在成交时建立——
            /// 连续段缺席时以收束点 last_price 合成。
            if active.is_none() && !active_must_be_empty && !post_day_end {
                let last = self
                    .world_public_views()
                    .get(stock_idx as usize)
                    .map(|v| v.last_price.max(1))
                    .unwrap_or(1);
                let placeholder = crate::money::Money::from_cents(last);
                session.set_active_candle_for_runtime(
                    &code,
                    crate::session::DailyCandle {
                        time: candle_time,
                        open: placeholder,
                        high: placeholder,
                        low: placeholder,
                        close: placeholder,
                        volume: 0,
                        trade_stats: Some(Default::default()),
                    },
                );
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
    accounts_per_group: usize,
) -> Result<Vec<Vec<super::desk::DeskRestingSeed>>, String> {
    // D5 restore 等价：runtime 形态（旧簿空）下单权威=restore 重建的信封
    // 账本（save 的 live_envelopes）——desk 种子从账本键恢复（purse 与
    // 价格面自 audit 真值直取——runtime 信封已收口真 limit/精确 escrow
    /// purse，A2 时期的零标记/1 分占位形态退役）。
    let snapshot = session.runtime_resting_orders_snapshot();
    let from_ledger = snapshot.iter().all(|(_, orders)| orders.is_empty());
    let mut seeds: Vec<Vec<super::desk::DeskRestingSeed>> = Vec::new();
    if from_ledger {
        for (key, envelope) in session.runtime_envelope_ledger_snapshot() {
            // Sol 终审修复 1c 同族：owner 用真实组号（此前写死组 0
            /// ——多组 restore 的挂单被错归组 0 的同 local 账户）。
            let global = id_map
                .iter()
                .position(|id| *id == key.account)
                .ok_or_else(|| {
                    format!("账本挂单 owner 不在世界名册（无法迁移）：{:?}", key.account)
                })?;
            let (g, local) = (
                (global / accounts_per_group.max(1)) as super::GroupIdx,
                (global % accounts_per_group.max(1)) as super::LocalIdx,
            );
            let stock = key.stock.0 as usize;
            if seeds.len() <= stock {
                seeds.resize(stock + 1, Vec::new());
            }
            // D5 restore 链最后一层：audit/ purse 数值面自 save 契约直取
            /// （desk 权威——此前 remaining 退 1 股占位使续拍种子量错）。
            // Sol 终审四审符号价族：limit=受理价真值（runtime 信封已收口
            /// 真价面）——1 分占位标记退役；非正=档损坏，显式失败（此前
            /// 占位使 restore 后公开簿/笼子参考价错乱——bids=[(1,100)]
            /// 形态、新单被 cage 误拒）。
            let price = envelope.audit().limit.cents();
            if price <= 0 {
                return Err(format!(
                    "账本挂单 limit 非正（档损坏或旧标记形态）：account={:?}, stock={:?}, order={}",
                    key.account, key.stock, key.order.0
                ));
            }
            let qty = envelope.audit().remaining_qty;
            let purse = if key.side == crate::orderbook::Side::Buy {
                envelope.live().cash.cents()
            } else {
                0
            };
            // Sol 五审件 1：累计事实（成交额/数量/费用分项）自 audit
            /// 直取——desk 种子归零会使续填重算最低佣金（purse 不足
            /// 显式失败）与费用口径漂移。
            let audit = envelope.audit();
            seeds[stock].push(super::desk::DeskRestingSeed {
                owner: (g, local),
                side: key.side,
                price,
                qty,
                purse,
                filled_value: audit.filled_value.cents(),
                filled_qty: audit.filled_qty,
                charged_commission: audit.charged.commission.cents(),
                charged_stamp_tax: audit.charged.stamp_tax.cents(),
                charged_transfer_fee: audit.charged.transfer_fee.cents(),
                // restore 形态：受理事件已随此前世界交付（save 前事件
                // 历史），直入不重放 Accepted。
                accepted_delivered: true,
                // D5 复核修复（Major 1，复审修正）：OrderId 编码低 48 位
                // =desk 本地序号（高位为 stock 域——group.rs order_id_of
                // 的编码式；直取整值会使 stock≥1 时 serial 跳天、截点
                // 过滤反向失真）。
                serial: super::group::desk_order_serial(key.order.0),
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
    // Sol 五审件 1：adopt 单的累计事实自信封账本 join（旧簿 Order 不
    /// 携费用分项；restore 侧 build_ledger 对每张在场委托都建有信封
    /// ——fault2218 缺失即显式拒绝，join 恒命中）。
    // 键 Side 无 Ord——方向入键用秩（Buy=0/Sell=1，与会话侧 side_rank 同式）。
    let ledger: std::collections::BTreeMap<
        (crate::AccountId, crate::StockId, crate::OrderId, u8),
        crate::session::pipeline::Envelope,
    > = session
        .runtime_envelope_ledger_snapshot()
        .into_iter()
        .map(|(key, envelope)| {
            let side_rank = u8::from(matches!(key.side, crate::orderbook::Side::Sell));
            ((key.account, key.stock, key.order, side_rank), envelope)
        })
        .collect();
    for (code, orders) in snapshot {
        for order in orders {
            // Sol 终审修复 1c 同族：owner 用真实组号（同上）。
            let global = id_map
                .iter()
                .position(|id| *id == order.owner)
                .ok_or_else(|| {
                    format!(
                        "旧簿挂单 owner 不在世界名册（无法迁移）：{:?} {:?}",
                        order.owner, code
                    )
                })?;
            let (g, local) = (
                (global / accounts_per_group.max(1)) as super::GroupIdx,
                (global % accounts_per_group.max(1)) as super::LocalIdx,
            );
            // Sol 五审复核阻断 1 修正：adopt 买腿 purse 自信封 live 直取
            /// （=旧管线 buy_order_reservation(限价, 余量, **真实累计
            /// 成交额**) 的精确 escrow——含按真实累计的费用边际）。此前
            /// 按零成交额式 buy_purse_limit 计算：部分成交单的种子
            /// purse 偏大（如 100@990 已成交、余 100@1000：种子 100_501
            /// vs 精确 100_001），空跑存档的 verified_live 期望失配 →
            /// fault2220。
            let envelope = ledger
                .get(&(
                    order.owner,
                    code.clone(),
                    order.id,
                    u8::from(matches!(order.side, crate::orderbook::Side::Sell)),
                ))
                .ok_or_else(|| {
                    format!(
                        "adopt 旧簿挂单缺信封账本条目（累计费用/资源无法迁移）：owner={:?}, code={code:?}, order={:?}",
                        order.owner, order.id
                    )
                })?;
            let audit = envelope.audit();
            let purse = if order.side == crate::orderbook::Side::Buy {
                envelope.live().cash.cents()
            } else {
                0
            };
            seeds[code.0 as usize].push(super::desk::DeskRestingSeed {
                owner: (g, local),
                side: order.side,
                price: order.price.cents(),
                qty: order.qty,
                purse,
                filled_value: audit.filled_value.cents(),
                filled_qty: audit.filled_qty,
                charged_commission: audit.charged.commission.cents(),
                charged_stamp_tax: audit.charged.stamp_tax.cents(),
                charged_transfer_fee: audit.charged.transfer_fee.cents(),
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

/// P2-4b 判别面前置（cfg(test)）：差分三面计数（Added/Removed/Changed）
/// ——等价守卫的 fixture 前置断言数据源。
#[cfg(test)]
thread_local! {
    pub static RESTING_DELTA_FACE_COUNTS: std::cell::RefCell<
        Option<std::sync::Arc<std::sync::Mutex<(usize, usize, usize)>>>
    > = const { std::cell::RefCell::new(None) };
}
