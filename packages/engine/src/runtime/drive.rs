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
}
